use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode};
use axum::Router;
use leptos::logging::log;
use leptos::prelude::*;
use leptos_axum::LeptosRoutes;
use shared::AppError;

use crate::app::{shell, App};

const EXPORT_SHELL_ARGUMENT: &str = "export-shell";
const HASH_FILES_VARIABLE: &str = "LEPTOS_HASH_FILES";
const SHELL_ROUTE_PATH: &str = "/";
const SHELL_DOCUMENT_NAME: &str = "index.html";

const MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
/* The manifest's site-root is relative to the workspace root, which is this crate's parent. */
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/* With no argument this runs the dev server (`cargo leptos watch`); with `export-shell` it renders `/`
   once and writes the document the production deploy serves. Production serves static assets and runs
   no server. */
pub async fn run() -> Result<(), AppError> {
    let first_argument: Option<String> = env::args().nth(1);

    match first_argument.as_deref() {
        None => serve().await,
        Some(EXPORT_SHELL_ARGUMENT) => export_shell().await,
        Some(unrecognized) => Err(AppError::from(format!(
            "unrecognized argument; [argument={unrecognized} expected={EXPORT_SHELL_ARGUMENT}]",
        ))),
    }
}

async fn serve() -> Result<(), AppError> {
    let configuration: ConfFile = get_configuration(None)
        .map_err(|error| AppError::from(format!("could not read the leptos settings; [error={error}]")))?;
    let leptos_options: LeptosOptions = configuration.leptos_options;
    let address: SocketAddr = leptos_options.site_addr;
    let route_listings: Vec<leptos_axum::AxumRouteListing> = leptos_axum::generate_route_list(App);

    let router: Router = Router::new()
        .leptos_routes(&leptos_options, route_listings, {
            let leptos_options: LeptosOptions = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    let listener: tokio::net::TcpListener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|error| AppError::from(format!("could not bind; [address={address} error={error}]")))?;
    log!("listening on http://{}", &address);

    axum::serve(listener, router.into_make_service())
        .await
        .map_err(|error| AppError::from(format!("the server stopped; [error={error}]")))
}

async fn export_shell() -> Result<(), AppError> {
    let leptos_options: LeptosOptions = read_manifest_options()?;
    let site_root: PathBuf = resolve_site_root(&leptos_options)?;
    let document_bytes: Bytes = render_shell_document(leptos_options).await?;

    let document_path: PathBuf = site_root.join(SHELL_DOCUMENT_NAME);
    fs::write(&document_path, &document_bytes)
        .map_err(|error| AppError::from(format!(
            "could not write the shell document; [path={} error={error}]",
            document_path.display(),
        )))?;

    log!(
        "wrote the shell document; [path={} bytes={}]",
        document_path.display(),
        document_bytes.len(),
    );

    Ok(())
}

/* Renders with the async mode rather than either streaming mode so the result is one complete document;
   a streamed render emits out-of-order chunks that only resolve once the client runs their patch
   scripts. */
pub async fn render_shell_document(leptos_options: LeptosOptions) -> Result<Bytes, AppError> {
    let declared_paths: Vec<String> = declared_route_paths();

    if declared_paths != [SHELL_ROUTE_PATH] {
        return Err(AppError::from(format!(
            "the app declares routes this export does not write; [declared={} written={SHELL_ROUTE_PATH}]",
            declared_paths.join(","),
        )));
    }

    let render_document = leptos_axum::render_app_async(move || shell(leptos_options.clone()));
    let request: Request<Body> = Request::builder()
        .uri(SHELL_ROUTE_PATH)
        .body(Body::empty())
        .map_err(|error| AppError::from(format!("could not build the shell request; [error={error}]")))?;
    let response: axum::response::Response = render_document(request)
        .await;
    let status: StatusCode = response.status();

    /* The router answers an unmatched path with its fallback view and a non-success status, which would
       otherwise be written out as the deployed document. */
    if !status.is_success() {
        return Err(AppError::from(format!(
            "rendering the shell route did not succeed; [status={status} path={SHELL_ROUTE_PATH}]",
        )));
    }

    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|error| AppError::from(format!("could not read the rendered document; [error={error}]")))
}

/* leptos_axum initializes the global executor that rendering spawns onto inside this call, and rendering
   panics without it, so this has to run before any render on this path. */
fn declared_route_paths() -> Vec<String> {
    leptos_axum::generate_route_list(App)
        .iter()
        .map(|route_listing| route_listing.path().to_string())
        .collect()
}

/* cargo-leptos is what sets the LEPTOS_* environment variables get_configuration reads from the
   environment, and the export runs on its own, so the settings come from the manifest instead. */
pub fn read_manifest_options() -> Result<LeptosOptions, AppError> {
    let configuration: ConfFile = get_configuration(Some(MANIFEST_PATH))
        .map_err(|error| AppError::from(format!(
            "could not read the leptos settings; [path={MANIFEST_PATH} error={error}]",
        )))?;

    apply_hash_files_override(configuration.leptos_options)
}

/* Hashed filenames are a deploy-time choice rather than a manifest setting, because cargo-leptos only
   re-hashes on a full build: under `watch`, an incremental rebuild writes the unhashed names and leaves
   the hash file naming the previous build, so the page would load stale assets. The build that hashes
   sets this variable, and cargo-leptos itself layers the same variable over the manifest, so both
   processes agree on the filenames. */
fn apply_hash_files_override(mut leptos_options: LeptosOptions) -> Result<LeptosOptions, AppError> {
    let declared_hash_files: Result<String, env::VarError> = env::var(HASH_FILES_VARIABLE);

    if let Ok(declared_hash_files) = declared_hash_files {
        leptos_options.hash_files = declared_hash_files
            .parse::<bool>()
            .map_err(|error| AppError::from(format!(
                "could not read the hashed-filenames setting; [{HASH_FILES_VARIABLE}={declared_hash_files} error={error}]",
            )))?;
    }

    Ok(leptos_options)
}

fn resolve_site_root(leptos_options: &LeptosOptions) -> Result<PathBuf, AppError> {
    let declared_site_root: PathBuf =
        Path::new(WORKSPACE_ROOT).join(leptos_options.site_root.as_ref());

    fs::canonicalize(&declared_site_root)
        .map_err(|error| AppError::from(format!(
            "the site root does not exist, so no build has run; [path={} error={error}]",
            declared_site_root.display(),
        )))
}
