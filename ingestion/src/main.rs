//! ingestion: the Eafora canonical-store CLI binary.
//!
//! This file is the clap entrypoint and the dispatch table. Each subcommand
//! routes to a `dispatch_*` helper; in this scaffolding PR the helpers are
//! all stubs returning "not yet implemented" — subsequent PRs replace each
//! stub with the real implementation per `specs/001-wb-wdi-ingestion/tasks.md`.

use clap::{Arg, ArgAction, ArgMatches, Command};

use ingestion::error::AppError;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    env_logger::init();
    let _ = dotenvy::dotenv();

    let matches: ArgMatches = build_cli().get_matches();
    match matches.subcommand() {
        Some(("ingest-source",    sub_matches)) => dispatch_ingest_source(sub_matches).await,
        Some(("run-all",          _))           => dispatch_run_all().await,
        Some(("build-artifacts",  sub_matches)) => dispatch_build_artifacts(sub_matches).await,
        Some(("seed-samples",     _))           => dispatch_seed_samples().await,
        Some(("upload-artifacts", sub_matches)) => dispatch_upload_artifacts(sub_matches).await,
        _                                        => unreachable!("subcommand_required guarantees a match"),
    }
}

fn build_cli() -> Command {
    Command::new("ingestion")
        .about("Eafora canonical-store CLI")
        .subcommand_required(true)
        .subcommand(
            Command::new("ingest-source")
                .about("Run a single source adapter")
                .arg(Arg::new("source").required(true).help("source code (e.g. wb_wdi)"))
                .arg(Arg::new("force-full-refetch").long("force-full-refetch").action(ArgAction::SetTrue)),
        )
        .subcommand(Command::new("run-all").about("Run every registered source adapter"))
        .subcommand(
            Command::new("build-artifacts")
                .about("Build CDN artifacts from the current canonical store")
                .arg(Arg::new("output-dir").required(true))
                .arg(Arg::new("version-label").required(true)),
        )
        .subcommand(Command::new("seed-samples").about("Load checked-in sample responses"))
        .subcommand(
            Command::new("upload-artifacts")
                .about("Upload a previously-built artifact set to R2")
                .arg(Arg::new("version-label").required(true)),
        )
}

async fn dispatch_ingest_source(_matches: &ArgMatches) -> Result<(), AppError> {
    Err(AppError::new("ingest-source: not yet implemented"))
}

async fn dispatch_run_all() -> Result<(), AppError> {
    Err(AppError::new("run-all: not yet implemented"))
}

async fn dispatch_build_artifacts(_matches: &ArgMatches) -> Result<(), AppError> {
    Err(AppError::new("build-artifacts: not yet implemented"))
}

async fn dispatch_seed_samples() -> Result<(), AppError> {
    Err(AppError::new("seed-samples: not yet implemented"))
}

async fn dispatch_upload_artifacts(_matches: &ArgMatches) -> Result<(), AppError> {
    Err(AppError::new("upload-artifacts: not yet implemented"))
}
