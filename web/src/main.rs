/* The bin lays out the future returned by the lib's render path, which monomorphizes the whole view tree
   and overflows rustc's default query depth. Only a release build shows it: `cargo check` skips codegen,
   and cargo-leptos type-erases dev builds. */
#![recursion_limit = "512"]

// Production serves static files and runs no server.
#[cfg(feature = "ssr")]
const PRERENDER_ARGUMENT: &str = "prerender";

#[cfg(feature = "ssr")]
enum ExitStatus {
    Failed = 1,
    Usage = 64, // the conventional status for a misuse of a command; this repository's scripts use it too
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
