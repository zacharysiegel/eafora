use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command};
use sqlx::{PgPool, Postgres, Transaction};

use ingestion::adapter::AdapterOptions;
use ingestion::artifact::{self, ArtifactBuildReport, BuildOptions, PublishReport};
use ingestion::artifact::repository::{
    ArtifactRepository, ArtifactRepositoryKind, CloudflareR2ArtifactRepository, CloudflareR2Config,
    DryrunArtifactRepository, LocalArtifactRepository,
};
use ingestion::canonical::canonical_model::DataSourceKind;
use ingestion::db;
use ingestion::error::AppError;
use ingestion::ingest::IngestReport;
use ingestion::secrets;
use ingestion::world_bank_wdi::world_bank_wdi_adapter;

/// Registered source adapters. Adding a new source = one entry here plus
/// the source's per-feature module + a `data_source` seed row.
const REGISTERED_SOURCES: &[DataSourceKind] = &[DataSourceKind::WorldBankWDI];

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
        Some(("source", sub_matches)) => dispatch_source(sub_matches).await,
        Some(("all", _)) => dispatch_all().await,
        Some(("build", sub_matches)) => dispatch_build(sub_matches).await,
        Some(("seed", _)) => dispatch_seed().await,
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
            Command::new("source")
                .about("Run a single source adapter")
                .arg(Arg::new("source").required(true).help("source code (e.g. wb_wdi)"))
                .arg(Arg::new("force-full-refetch").long("force-full-refetch").action(ArgAction::SetTrue)),
        )
        .subcommand(Command::new("all").about("Run every registered source adapter"))
        .subcommand(
            Command::new("build")
                .about("Build CDN artifacts from the current canonical store")
                .arg(Arg::new("output-dir").required(true))
                .arg(Arg::new("version-label").required(true)),
        )
        .subcommand(Command::new("seed").about("Load checked-in sample responses"))
        .subcommand(
            Command::new("publish")
                .about("Upload a previously-built artifact set to a repository")
                .arg(Arg::new("output-dir").required(true).help("directory containing manifest.json and referenced files"))
                .arg(Arg::new("repository").long("repository").required(true).help("local | cloudflare-r2 | dryrun"))
                .arg(Arg::new("local-root").long("local-root").help("destination root directory (required when --repository=local)"))
                .arg(Arg::new("local-public-url-base").long("local-public-url-base").help("public URL prefix for local repository (required when --repository=local)"))
                .arg(Arg::new("build").long("build").action(ArgAction::SetTrue).help("build the artifact set first, then publish; requires --version-label"))
                .arg(Arg::new("version-label").long("version-label").help("version label for --build")),
        )
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

async fn dispatch_all() -> Result<(), AppError> {
    let pool: PgPool = db::create_pool().await?;
    let options: AdapterOptions = AdapterOptions {
        force_full_refetch: false,
    };

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
    }
}

fn log_report(source_kind: DataSourceKind, report: &IngestReport) {
    log::info!(
        "source {} complete: added={} revised={} skipped={} warnings={}",
        source_kind.code(),
        report.values_added,
        report.values_revised,
        report.values_skipped,
        report.warnings.len(),
    );
    for warning in &report.warnings {
        log::warn!("source {} {:?}: {}", source_kind.code(), warning.kind, warning.message);
    }
}

async fn dispatch_build(matches: &ArgMatches) -> Result<(), AppError> {
    let output_dir: PathBuf =
        PathBuf::from(matches.get_one::<String>("output-dir").expect("output-dir is required via clap"));
    let version_label: &String =
        matches.get_one::<String>("version-label").expect("version-label is required via clap");

    let pool: PgPool = db::create_pool().await?;
    let mut transaction: Transaction<'_, Postgres> = pool.begin().await?;
    let options: BuildOptions = BuildOptions::default();
    let build: ArtifactBuildReport =
        artifact::build_artifacts(&mut *transaction, &output_dir, version_label, options).await?;
    transaction.commit().await?;

    log::info!(
        "build complete: version_label={} output_dir={:?} shards={} geometry={:?} manifest={:?}",
        build.version_label,
        build.output_dir,
        build.artifacts.shards.len(),
        build.artifacts.geometry.path,
        build.artifacts.manifest.path,
    );
    Ok(())
}

async fn dispatch_seed() -> Result<(), AppError> {
    Err(AppError::new("seed: not yet implemented"))
}

async fn dispatch_publish(matches: &ArgMatches) -> Result<(), AppError> {
    let output_dir: PathBuf =
        PathBuf::from(matches.get_one::<String>("output-dir").expect("output-dir is required via clap"));
    let repository_str: &String =
        matches.get_one::<String>("repository").expect("repository is required via clap");
    let repository_kind: ArtifactRepositoryKind = ArtifactRepositoryKind::try_from(repository_str.as_str())?;
    let build_first: bool = matches.get_flag("build");

    let pool: PgPool = db::create_pool().await?;

    let build_report: ArtifactBuildReport = if build_first {
        let version_label: &String = matches
            .get_one::<String>("version-label")
            .ok_or_else(|| AppError::new("--build requires --version-label"))?;

        let mut transaction: Transaction<'_, Postgres> = pool.begin().await?;
        let report: ArtifactBuildReport =
            artifact::build_artifacts(&mut *transaction, &output_dir, version_label, BuildOptions::default()).await?;
        transaction.commit().await?;
        report
    } else {
        artifact::load_build_report_from_disk(&output_dir)?
    };

    let repository: Box<dyn ArtifactRepository> = create_repository(repository_kind, matches).await?;
    let publish_report: PublishReport = artifact::publish_artifacts(&pool, &build_report, repository.as_ref()).await?;

    log::info!(
        "publish complete: version_label={} manifest_url={} shards={} geometry={} manifest={}",
        publish_report.version_label,
        publish_report.manifest_url,
        publish_report.shards_uploaded,
        publish_report.geometry_uploaded,
        publish_report.manifest_uploaded,
    );
    Ok(())
}

async fn create_repository(
    kind: ArtifactRepositoryKind,
    matches: &ArgMatches,
) -> Result<Box<dyn ArtifactRepository>, AppError> {
    match kind {
        ArtifactRepositoryKind::Local => {
            let root: PathBuf = matches
                .get_one::<String>("local-root")
                .map(PathBuf::from)
                .ok_or_else(|| AppError::new("--repository=local requires --local-root"))?;
            let public_url_base: String = matches
                .get_one::<String>("local-public-url-base")
                .cloned()
                .ok_or_else(|| AppError::new("--repository=local requires --local-public-url-base"))?;

            Ok(Box::new(LocalArtifactRepository::new(root, public_url_base)))
        }
        ArtifactRepositoryKind::CloudflareR2 => {
            let config: CloudflareR2Config = CloudflareR2Config {
                account_id: dotenvy::var("R2_ACCOUNT_ID")?,
                bucket: dotenvy::var("R2_ARTIFACT_BUCKET")?,
                access_key_id: secrets::master_decrypt_utf8("r2_access_key_id")?,
                secret_access_key: secrets::master_decrypt_utf8("r2_secret_access_key")?,
                public_url_base: dotenvy::var("R2_ARTIFACT_PUBLIC_URL_BASE")?,
            };
            let repository: CloudflareR2ArtifactRepository = CloudflareR2ArtifactRepository::create(config).await?;
            Ok(Box::new(repository))
        }
        ArtifactRepositoryKind::Dryrun => {
            let public_url_base: String = matches
                .get_one::<String>("local-public-url-base")
                .cloned()
                .unwrap_or_else(|| "dryrun://".to_string());
            Ok(Box::new(DryrunArtifactRepository::new(public_url_base)))
        }
    }
}
