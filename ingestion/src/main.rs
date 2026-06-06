use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command};
use sqlx::{PgPool, Postgres, Transaction};

use ingestion::adapter::AdapterOptions;
use ingestion::artifact::{self, BuildOptions, ArtifactBuildReport};
use ingestion::canonical::canonical_model::DataSourceKind;
use ingestion::db;
use ingestion::error::AppError;
use ingestion::ingest::IngestReport;
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
                .about("Upload a previously-built artifact set to R2")
                .arg(Arg::new("version-label").required(true)),
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
        "build complete: version_label={} output_dir={:?} statistic_shards={} geometry={:?} manifest={:?}",
        build.version_label,
        build.output_dir,
        build.artifacts.statistic_shards.len(),
        build.artifacts.geometry.path,
        build.artifacts.manifest.path,
    );
    Ok(())
}

async fn dispatch_seed() -> Result<(), AppError> {
    Err(AppError::new("seed: not yet implemented"))
}

async fn dispatch_publish(_matches: &ArgMatches) -> Result<(), AppError> {
    Err(AppError::new("publish: not yet implemented"))
}
