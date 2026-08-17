/* Rendering the component tree monomorphizes deeply nested view types, which overflows the default
   limit while computing their layout. The lib target raises it for the same reason. */
#![recursion_limit = "512"]

/* The binary cargo-leptos compiles with the `ssr` feature. With no argument it runs the dev server
   (`cargo leptos watch`); with `export-shell` it renders `/` once and writes the document the
   production deploy serves. Production serves static assets and runs no server. */
#[cfg(feature = "ssr")]
mod server {
    use std::env;
    use std::fs;
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};
    use std::process;

    use axum::body::{Body, Bytes};
    use axum::http::Request;
    use axum::Router;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::LeptosRoutes;

    use web::app::{shell, App};

    const EXPORT_SHELL_ARGUMENT: &str = "export-shell";
    const HASH_FILES_VARIABLE: &str = "LEPTOS_HASH_FILES";
    const SHELL_ROUTE_PATH: &str = "/";
    const SHELL_DOCUMENT_NAME: &str = "index.html";
    const UNRECOGNIZED_ARGUMENT_EXIT_CODE: i32 = 2;
    const EXPORT_FAILED_EXIT_CODE: i32 = 1;

    const MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    /* The manifest's site-root is relative to the workspace root, which is this crate's parent. */
    const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

    pub async fn run() {
        let first_argument: Option<String> = env::args().nth(1);

        match first_argument.as_deref() {
            None => serve().await,
            Some(EXPORT_SHELL_ARGUMENT) => export_shell().await,
            Some(unrecognized) => {
                eprintln!("unrecognized argument; [argument={unrecognized} expected={EXPORT_SHELL_ARGUMENT}]");
                process::exit(UNRECOGNIZED_ARGUMENT_EXIT_CODE);
            },
        }
    }

    async fn serve() {
        let configuration = get_configuration(None)
            .unwrap();
        let leptos_options: LeptosOptions = configuration.leptos_options;
        let address: SocketAddr = leptos_options.site_addr;
        let route_listings = leptos_axum::generate_route_list(App);

        let router: Router = Router::new()
            .leptos_routes(&leptos_options, route_listings, {
                let leptos_options: LeptosOptions = leptos_options.clone();
                move || shell(leptos_options.clone())
            })
            .fallback(leptos_axum::file_and_error_handler(shell))
            .with_state(leptos_options);

        let listener: tokio::net::TcpListener = tokio::net::TcpListener::bind(&address)
            .await
            .unwrap();
        log!("listening on http://{}", &address);

        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    }

    /* Renders with the async mode rather than either streaming mode so the result is one complete
       document; a streamed render emits out-of-order chunks that only resolve once the client runs
       their patch scripts. */
    async fn export_shell() {
        let leptos_options: LeptosOptions = read_manifest_options();
        let site_root: PathBuf = resolve_site_root(&leptos_options);

        assert_shell_route_is_declared();

        let render_document = leptos_axum::render_app_async(move || shell(leptos_options.clone()));
        let request: Request<Body> = Request::builder()
            .uri(SHELL_ROUTE_PATH)
            .body(Body::empty())
            .expect("a GET request for the shell route is well-formed");
        let response = render_document(request)
            .await;

        let document_bytes: Bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("an async render buffers the whole document before responding");

        let document_path: PathBuf = site_root.join(SHELL_DOCUMENT_NAME);
        let write_result: Result<(), std::io::Error> = fs::write(&document_path, &document_bytes);

        if let Err(error) = write_result {
            exit_with_message(&format!(
                "could not write the shell document; [path={} error={error}]",
                document_path.display(),
            ));
        }

        log!(
            "wrote the shell document; [path={} bytes={}]",
            document_path.display(),
            document_bytes.len(),
        );
    }

    /* Also initializes the global executor that rendering spawns onto, which the renderer panics
       without and which nothing else on this path sets up. Reporting the app's declared routes makes
       a route that this export does not write fail here rather than reach a deploy undeployed. */
    fn assert_shell_route_is_declared() {
        let route_listings = leptos_axum::generate_route_list(App);
        let declared_paths: Vec<&str> = route_listings
            .iter()
            .map(|route_listing| route_listing.path())
            .collect();

        if declared_paths != [SHELL_ROUTE_PATH] {
            exit_with_message(&format!(
                "the app declares routes this export does not write; [declared={} written={SHELL_ROUTE_PATH}]",
                declared_paths.join(","),
            ));
        }
    }

    /* cargo-leptos is what sets the LEPTOS_* environment variables get_configuration reads from the
       environment, and the export runs on its own, so the settings come from the manifest instead. */
    fn read_manifest_options() -> LeptosOptions {
        let configuration_result = get_configuration(Some(MANIFEST_PATH));

        match configuration_result {
            Ok(configuration) => apply_hash_files_override(configuration.leptos_options),
            Err(error) => exit_with_message(&format!(
                "could not read the leptos settings; [path={MANIFEST_PATH} error={error}]",
            )),
        }
    }

    /* Hashed filenames are a deploy-time choice rather than a manifest setting, because cargo-leptos
       only re-hashes on a full build: under `watch`, an incremental rebuild writes the unhashed names
       and leaves the hash file naming the previous build, so the page would load stale assets. The
       build that hashes sets this variable, and cargo-leptos itself layers the same variable over the
       manifest, so both processes agree on the filenames. */
    fn apply_hash_files_override(mut leptos_options: LeptosOptions) -> LeptosOptions {
        let declared_hash_files: Result<String, env::VarError> = env::var(HASH_FILES_VARIABLE);

        if let Ok(declared_hash_files) = declared_hash_files {
            match declared_hash_files.parse::<bool>() {
                Ok(hash_files) => leptos_options.hash_files = hash_files,
                Err(error) => exit_with_message(&format!(
                    "could not read the hashed-filenames setting; [{HASH_FILES_VARIABLE}={declared_hash_files} error={error}]",
                )),
            }
        }

        leptos_options
    }

    fn resolve_site_root(leptos_options: &LeptosOptions) -> PathBuf {
        let declared_site_root: PathBuf =
            Path::new(WORKSPACE_ROOT).join(leptos_options.site_root.as_ref());
        let canonical_result: Result<PathBuf, std::io::Error> = fs::canonicalize(&declared_site_root);

        match canonical_result {
            Ok(site_root) => site_root,
            Err(error) => exit_with_message(&format!(
                "the site root does not exist, so no build has run; [path={} error={error}]",
                declared_site_root.display(),
            )),
        }
    }

    fn exit_with_message(message: &str) -> ! {
        eprintln!("{message}");
        process::exit(EXPORT_FAILED_EXIT_CODE);
    }
}

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    server::run()
        .await;
}

#[cfg(not(feature = "ssr"))]
fn main() {}
