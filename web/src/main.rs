/* The bin lays out the future returned by the lib's render path, which monomorphizes the whole view tree
   and overflows rustc's default query depth. Only a release build shows it: `cargo check` skips codegen,
   and cargo-leptos type-erases dev builds. */
#![recursion_limit = "512"]

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    let result: Result<(), shared::AppError> = web::server::run()
        .await;

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "ssr"))]
fn main() {}
