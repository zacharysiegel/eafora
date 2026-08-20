use std::path::PathBuf;
#[cfg(unix)] // Path is used only by the Unix-gated latest-pointer helper
use std::path::Path;

use clap::{Arg, ArgAction, ArgMatches, Command};
use sqlx::{PgPool, Postgres, Transaction};

use ingestion::adapter::AdapterOptions;
use ingestion::artifact::{self, BuildReport, BuildOptions, CoupledBuildReport, PublishReport};
use ingestion::artifact::repository::{
    ArtifactRepositoryKind, CloudflareR2ArtifactRepository, CloudflareR2Config,
    DryArtifactRepository, LocalArtifactRepository,
    ENV_LOCAL_REPOSITORY_ROOT,
    ENV_R2_ACCOUNT_ID, ENV_R2_ARTIFACT_BUCKET, ENV_R2_ARTIFACT_PUBLIC_BASE_URL, ENV_R2_PUBLISH_ACCESS_KEY_ID,
    SECRET_R2_PUBLISH_SECRET_ACCESS_KEY,
};
use shared::canonical::canonical_model::DataSourceKind;
use ingestion::db;
use ingestion::error::AppError;
use ingestion::hfd::hfd_adapter;
use ingestion::ingest::IngestReport;
use ingestion::secrets;
use ingestion::version_label;
use ingestion::world_bank_wdi::world_bank_wdi_adapter;

/// Registered source adapters. Adding a new source = one entry here plus
/// the source's per-feature module + a `data_source` seed row.
const REGISTERED_SOURCES: &[DataSourceKind] = &[DataSourceKind::WorldBankWDI, DataSourceKind::HumanFertilityDatabase];

#[tokio::main]
async fn main() -> Result<(), AppError> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .format_source_path(true)
        .format_timestamp_millis()
        .try_init()?;

    let _ = dotenvy::dotenv();

    let matches: ArgMatches = build_cli().get_matches();

    match matches.subcommand() {
        Some(("ingest", sub_matches)) => dispatch_ingest(sub_matches).await,
        Some(("build", sub_matches)) => dispatch_build(sub_matches).await,
        Some(("publish", sub_matches)) => dispatch_publish(sub_matches).await,
        Some((other, _)) => Err(AppError::from(format!("unknown subcommand: {other}"))),
        None => Err(AppError::new("missing subcommand")),
    }
}

fn build_cli() -> Command {
    Command::new("ingestion")
        .about("Eafora canonical-store CLI")
        .subcommand_required(true)
        .subcommand(
            Command::new("ingest")
                .about("Fetch upstream source data and write it to the canonical store")
                .subcommand_required(true)
                .subcommand(
                    Command::new("source")
                        .about("Run a single source adapter")
                        .arg(Arg::new("source").required(true).help("source code (e.g. wb_wdi, hfd)"))
                        .arg(Arg::new("force-full-refetch").long("force-full-refetch").action(ArgAction::SetTrue)),
                )
                .subcommand(
                    Command::new("all")
                        .about("Run every registered source adapter")
                        .arg(Arg::new("force-full-refetch").long("force-full-refetch").action(ArgAction::SetTrue)),
                ),
        )
        .subcommand(
            Command::new("build")
                .about("Build the complete (CDN) and downsampled (embedded) bundles from the current canonical store; writes both to $EAFORA_ARTIFACTS_DIR/<version-label>/{complete,downsampled}/ and updates the latest pointer"),
        )
        .subcommand(
            Command::new("publish")
                .about("Upload a previously-built artifact set to a repository")
                .subcommand_required(true)
                .arg(Arg::new("build").long("build").action(ArgAction::SetTrue).global(true).help("build the artifact set first, then publish"))
                .subcommand(add_publish_common_args(Command::new("local"))
                    .about("Publish to a local filesystem destination served by an external HTTP server")
                    .arg(Arg::new("root").long("root").help(format!("destination root directory the publisher writes object keys under (defaults to ${})", ENV_LOCAL_REPOSITORY_ROOT)))
                    .arg(Arg::new("public-base-url").long("public-base-url").required(true).help("public URL prefix served from the destination")))
                .subcommand(add_publish_common_args(Command::new("cloudflare-r2"))
                    .about(format!(
                        "Publish to Cloudflare R2; reads {}/{}/{} from .env and the access key + secret access key from secrets.yaml",
                        ENV_R2_ACCOUNT_ID, ENV_R2_ARTIFACT_BUCKET, ENV_R2_ARTIFACT_PUBLIC_BASE_URL,
                    )))
                .subcommand(add_publish_common_args(Command::new("dry"))
                    .about("Publish nowhere; logs every PUT and inserts an artifact_version row referencing a placeholder dry:/// URL")),
        )
}

fn add_publish_common_args(command: Command) -> Command {
    command
        .arg(Arg::new("artifact-dir")
            .required_unless_present("build")
            .conflicts_with("build")
            .help("directory containing manifest.json and referenced files (omit when --build is set)"))
}

async fn dispatch_ingest(matches: &ArgMatches) -> Result<(), AppError> {
    match matches.subcommand() {
        Some(("source", sub_matches)) => dispatch_source(sub_matches).await,
        Some(("all", sub_matches)) => dispatch_all(sub_matches).await,
        Some((other, _)) => Err(AppError::from(format!("unknown ingest subcommand: {other}"))),
        None => Err(AppError::new("missing ingest subcommand")),
    }
}

async fn dispatch_source(matches: &ArgMatches) -> Result<(), AppError> {
    let source_kind_str: &String = matches.get_one::<String>("source").expect("source is required via clap");
    let source_kind: DataSourceKind = DataSourceKind::try_from(source_kind_str.as_str())?;
    let force_full_refetch: bool = matches.get_flag("force-full-refetch");
    let options: AdapterOptions = AdapterOptions { force_full_refetch };

    let pool: PgPool = db::create_pool().await?;
    let report: IngestReport = run_source(&pool, source_kind, options).await?;

    log_report(source_kind, &report);
    Ok(())
}

async fn dispatch_all(matches: &ArgMatches) -> Result<(), AppError> {
    let pool: PgPool = db::create_pool().await?;
    let force_full_refetch: bool = matches.get_flag("force-full-refetch");
    let options: AdapterOptions = AdapterOptions { force_full_refetch };

    let mut failure_count: usize = 0;

    for source_kind in REGISTERED_SOURCES {
        log::info!("source {} starting", source_kind.code());
        match run_source(&pool, *source_kind, options).await {
            Ok(report) => log_report(*source_kind, &report),
            Err(error) => {
                log::error!("source {} failed: {}", source_kind.code(), error);
                failure_count += 1;
            }
        }
    }

    if failure_count > 0 {
        return Err(AppError::from(format!(
            "{failure_count} of {} adapters failed",
            REGISTERED_SOURCES.len(),
        )));
    }
    Ok(())
}

async fn run_source(
    pool: &PgPool,
    source_kind: DataSourceKind,
    options: AdapterOptions,
) -> Result<IngestReport, AppError> {
    match source_kind {
        DataSourceKind::WorldBankWDI => world_bank_wdi_adapter::fetch_and_store(pool, options).await,
        DataSourceKind::HumanFertilityDatabase => hfd_adapter::fetch_and_store(pool, options).await,
        // The test-only source identities have no real adapter; they never appear in
        // REGISTERED_SOURCES, so this is unreachable at runtime. Listed explicitly (not a
        // catch-all) so adding a real source still breaks this match until it's handled.
        DataSourceKind::TestAlpha | DataSourceKind::TestBeta => {
            Err(AppError::from(format!("no adapter for test-only source {:?}", source_kind)))
        }
    }
}

fn log_report(source_kind: DataSourceKind, report: &IngestReport) {
    log::info!(
        "source complete; [source={} added={} revised={} skipped={} absent_upstream={} warnings={}]",
        source_kind.code(),
        report.values_added,
        report.values_revised,
        report.values_skipped,
        report.values_absent_upstream,
        report.warnings.len(),
    );
    for warning in &report.warnings {
        log::warn!(
            "{}; [source={} kind={:?}]",
            warning.message,
            source_kind.code(),
            warning.kind,
        );
    }
}

async fn dispatch_build(_matches: &ArgMatches) -> Result<(), AppError> {
    let options: BuildOptions = BuildOptions { test_offline: false };

    let pool: PgPool = db::create_pool().await?;
    let build: CoupledBuildReport = run_build(&pool, options).await?;

    log::info!(
        "build complete; [version_label={} complete_dir={:?} complete_shards={} downsampled_dir={:?} downsampled_shards={}]",
        build.complete.version_label,
        build.complete.artifact_dir,
        build.complete.artifacts.shards.len(),
        build.downsampled.artifact_dir,
        build.downsampled.artifacts.shards.len(),
    );
    Ok(())
}

async fn run_build(pool: &PgPool, options: BuildOptions) -> Result<CoupledBuildReport, AppError> {
    let parent: PathBuf = PathBuf::from(dotenvy::var("EAFORA_ARTIFACTS_DIR")?);
    let version_label: String = version_label::generate(pool).await?;
    let version_dir: PathBuf = parent.join(&version_label);

    let mut transaction: Transaction<'_, Postgres> = pool.begin().await?;
    let report: CoupledBuildReport =
        artifact::build_artifacts(&mut *transaction, &version_dir, &version_label, options).await?;
    transaction.commit().await?;

    #[cfg(unix)] // the latest pointer is a Unix symlink
    update_latest_pointer(&parent, &version_label)?;

    Ok(report)
}

/// Repoints `<EAFORA_ARTIFACTS_DIR>/latest` at the version directory just built, so consumers (the
/// embedded-bundle sync script, manual publishes) always resolve the newest build. The target is
/// relative so the pointer survives relocating the artifacts directory. Called only after the
/// transaction commits, so `latest` never names a version whose rows failed to persist.
#[cfg(unix)] // uses the Unix-only std::os::unix::fs::symlink
fn update_latest_pointer(parent: &Path, version_label: &str) -> Result<(), AppError> {
    let pointer: PathBuf = parent.join(artifact::LATEST_POINTER);

    if pointer.symlink_metadata().is_ok() {
        std::fs::remove_file(&pointer)
            .map_err(|err| AppError::from(format!("removing the existing latest pointer {:?} failed: {}", pointer, err)))?;
    }

    std::os::unix::fs::symlink(version_label, &pointer)
        .map_err(|err| AppError::from(format!("creating the latest pointer {:?} failed: {}", pointer, err)))?;

    Ok(())
}

async fn dispatch_publish(matches: &ArgMatches) -> Result<(), AppError> {
    let (kind, sub_matches): (&str, &ArgMatches) = matches
        .subcommand()
        .expect("publish subcommand is required via clap");
    let build_first: bool = sub_matches.get_flag("build");

    let pool: PgPool = db::create_pool().await?;

    let build_report: BuildReport = if build_first {
        run_build(&pool, BuildOptions::default()).await?.complete
    } else {
        let artifact_dir: PathBuf = PathBuf::from(
            sub_matches.get_one::<String>("artifact-dir").expect("artifact-dir is required when --build is absent"),
        );
        artifact::load_build_report_from_disk(&artifact_dir)?
    };

    let repository: ArtifactRepositoryKind = create_repository(kind, sub_matches).await?;
    let publish_report: PublishReport = artifact::publish_artifacts(&pool, &build_report, &repository).await?;

    log::info!(
        "publish complete; [version_label={} manifest_url={} shards={}]",
        publish_report.artifact_version.version_label,
        publish_report.artifact_version.manifest_url,
        publish_report.shards_published,
    );
    Ok(())
}

async fn create_repository(
    kind: &str,
    sub_matches: &ArgMatches,
) -> Result<ArtifactRepositoryKind, AppError> {
    match kind {
        "local" => {
            let root: PathBuf = match sub_matches.get_one::<String>("root") {
                Some(value) => PathBuf::from(value),
                None => PathBuf::from(dotenvy::var(ENV_LOCAL_REPOSITORY_ROOT)?),
            };
            let public_base_url: String = sub_matches
                .get_one::<String>("public-base-url")
                .expect("--public-base-url is required via clap")
                .clone();

            Ok(ArtifactRepositoryKind::Local(LocalArtifactRepository::new(root, public_base_url)))
        }
        "cloudflare-r2" => {
            let config: CloudflareR2Config = CloudflareR2Config {
                account_id: dotenvy::var(ENV_R2_ACCOUNT_ID)?,
                bucket: dotenvy::var(ENV_R2_ARTIFACT_BUCKET)?,
                access_key_id: dotenvy::var(ENV_R2_PUBLISH_ACCESS_KEY_ID)?,
                secret_access_key: secrets::master_decrypt_utf8(SECRET_R2_PUBLISH_SECRET_ACCESS_KEY)?,
                public_base_url: dotenvy::var(ENV_R2_ARTIFACT_PUBLIC_BASE_URL)?,
            };
            let repository: CloudflareR2ArtifactRepository = CloudflareR2ArtifactRepository::create(config).await?;
            Ok(ArtifactRepositoryKind::CloudflareR2(repository))
        }
        "dry" => Ok(ArtifactRepositoryKind::Dry(DryArtifactRepository::new())),
        other => Err(AppError::from(format!("unknown publish repository: {:?}", other))),
    }
}
