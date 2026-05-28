use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command};
use sqlx::{PgPool, Postgres, Transaction};

use ingestion::adapter::AdapterOptions;
use ingestion::artifact::{self, BuildOptions, LocalArtifactBuild};
use ingestion::db;
use ingestion::error::AppError;
use ingestion::ingest::IngestReport;
use ingestion::world_bank_wdi::world_bank_wdi_adapter;

/// Registered source adapters. Adding a new source = one entry here plus
/// the source's per-feature module + a `data_source` seed row.
const REGISTERED_SOURCES: &[&str] = &["wb_wdi"];

#[tokio::main]
async fn main() -> Result<(), AppError> {
    env_logger::init();
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

fn require_arg<'a>(matches: &'a ArgMatches, name: &str) -> Result<&'a String, AppError> {
    matches
        .get_one::<String>(name)
        .ok_or_else(|| AppError::from(format!("missing required argument: {name}")))
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
    let source_code: &String = require_arg(matches, "source")?;
    let force_full_refetch: bool = matches.get_flag("force-full-refetch");
    let options: AdapterOptions = AdapterOptions { force_full_refetch };

    let pool: PgPool = db::create_pool().await?;
    let report: IngestReport = run_source(&pool, source_code, options).await?;

    log_report(source_code, &report);
    Ok(())
}

async fn dispatch_all() -> Result<(), AppError> {
    let pool: PgPool = db::create_pool().await?;
    let options: AdapterOptions = AdapterOptions { force_full_refetch: false };

    let mut failure_count: usize = 0;

    for source_code in REGISTERED_SOURCES {
        log::info!("source {} starting", source_code);
        match run_source(&pool, source_code, options).await {
            Ok(report) => log_report(source_code, &report),
            Err(error) => {
                log::error!("source {} failed: {}", source_code, error);
                failure_count += 1;
            }
        }
    }

    if failure_count > 0 {
        return Err(AppError::from(format!(
            "all: {failure_count} of {} adapters failed",
            REGISTERED_SOURCES.len(),
        )));
    }
    Ok(())
}

async fn run_source(
    pool: &PgPool,
    source_code: &str,
    options: AdapterOptions,
) -> Result<IngestReport, AppError> {
    match source_code {
        "wb_wdi" => world_bank_wdi_adapter::fetch_and_store(pool, options).await,
        other => Err(AppError::from(format!("unknown source code: {other:?}"))),
    }
}

fn log_report(source_code: &str, report: &IngestReport) {
    log::info!(
        "source {} complete: added={} revised={} skipped={} warnings={}",
        source_code,
        report.values_added,
        report.values_revised,
        report.values_skipped,
        report.warnings.len(),
    );
    for warning in &report.warnings {
        log::warn!("source {} {:?}: {}", source_code, warning.kind, warning.message);
    }
}

async fn dispatch_build(matches: &ArgMatches) -> Result<(), AppError> {
    let output_dir: PathBuf = PathBuf::from(require_arg(matches, "output-dir")?);
    let version_label: &String = require_arg(matches, "version-label")?;

    let pool: PgPool = db::create_pool().await?;
    let mut transaction: Transaction<'_, Postgres> = pool.begin().await?;
    let options: BuildOptions = BuildOptions::default();
    let build: LocalArtifactBuild = artifact::build_artifacts(&mut *transaction, &output_dir, version_label, options).await?;
    transaction.commit().await?;

    log::info!(
        "build complete: version_label={} output_dir={:?} statistic_shards={} geometry_shard={:?} manifest={:?}",
        build.version_label,
        build.output_dir,
        build.hashed.statistic_shards.len(),
        build.hashed.geometry_shard.path,
        build.manifest.path,
    );
    Ok(())
}

async fn dispatch_seed() -> Result<(), AppError> {
    Err(AppError::new("seed: not yet implemented"))
}

async fn dispatch_publish(_matches: &ArgMatches) -> Result<(), AppError> {
    Err(AppError::new("publish: not yet implemented"))
}
