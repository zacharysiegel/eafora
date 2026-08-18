/* The bin lays out the future returned by the lib's render path, which monomorphizes the whole view tree
   and overflows rustc's default query depth. Only a release build shows it: `cargo check` skips codegen,
   and cargo-leptos type-erases dev builds. */
#![recursion_limit = "512"]

/* With no argument this runs the dev server (`cargo leptos watch`); with `prerender` it renders the map
   route once and writes the document the deploy serves. Production serves static files and runs no server. */
#[cfg(feature = "ssr")]
const PRERENDER_ARGUMENT: &str = "prerender";

/* 64 is the conventional status for a misuse of a command, and the scripts in this repository exit 64 for
   the same reason, so a wrapper can tell a mistyped command from work that was attempted and failed. */
#[cfg(feature = "ssr")]
enum ExitStatus {
    Failed = 1,
    Usage = 64,
}

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use std::env;
    use std::process;

    use shared::AppError;

    let first_argument: Option<String> = env::args().nth(1);

    let result: Result<(), AppError> = match first_argument.as_deref() {
        None => web::server::serve().await,
        Some(PRERENDER_ARGUMENT) => web::server::write_prerendered_document().await,
        Some(unrecognized) => {
            eprintln!("unrecognized argument; [argument={unrecognized} expected={PRERENDER_ARGUMENT}]");
            process::exit(ExitStatus::Usage as i32);
        },
    };

    if let Err(error) = result {
        eprintln!("{error}");
        process::exit(ExitStatus::Failed as i32);
    }
}

#[cfg(not(feature = "ssr"))]
fn main() {}
