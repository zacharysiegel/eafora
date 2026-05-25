//! ingestion: the Eafora canonical-store CLI binary.

use clap::{Arg, ArgAction, ArgMatches, Command};

use ingestion::error::AppError;

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
        _ => unreachable!("subcommand_required guarantees a match"),
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

async fn dispatch_source(_matches: &ArgMatches) -> Result<(), AppError> {
    Err(AppError::new("source: not yet implemented"))
}

async fn dispatch_all() -> Result<(), AppError> {
    Err(AppError::new("all: not yet implemented"))
}

async fn dispatch_build(_matches: &ArgMatches) -> Result<(), AppError> {
    Err(AppError::new("build: not yet implemented"))
}

async fn dispatch_seed() -> Result<(), AppError> {
    Err(AppError::new("seed: not yet implemented"))
}

async fn dispatch_publish(_matches: &ArgMatches) -> Result<(), AppError> {
    Err(AppError::new("publish: not yet implemented"))
}
