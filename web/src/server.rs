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

const HASH_FILES_VARIABLE: &str = "LEPTOS_HASH_FILES";
const PRERENDERED_ROUTE_PATH: &str = "/";
const PRERENDERED_DOCUMENT_NAME: &str = "index.html";
const WORKSPACE_MANIFEST_MARKER: &str = "[workspace";

const CRATE_DIRECTORY: &str = env!("CARGO_MANIFEST_DIR");
const MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");

pub async fn serve() -> Result<(), AppError> {
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

/* Production serves static files and runs no server, so the one document a visitor loads is rendered here
   at build time and written into the site tree. */
pub async fn write_prerendered_document() -> Result<(), AppError> {
    let leptos_options: LeptosOptions = read_leptos_manifest_options()?;
    let site_root: PathBuf = resolve_site_root(&leptos_options)?;
    let document_bytes: Bytes = prerender_document(leptos_options).await?;

    let document_path: PathBuf = site_root.join(PRERENDERED_DOCUMENT_NAME);
    fs::write(&document_path, &document_bytes)
        .map_err(|error| AppError::from(format!(
            "could not write the prerendered document; [path={} error={error}]",
            document_path.display(),
        )))?;

    log!(
        "wrote the prerendered document; [path={} bytes={}]",
        document_path.display(),
        document_bytes.len(),
    );

    Ok(())
}

/* Renders with the async mode rather than either streaming mode so the result is one complete document;
   a streamed render emits out-of-order chunks that only resolve once the client runs their patch
   scripts. */
pub async fn prerender_document(leptos_options: LeptosOptions) -> Result<Bytes, AppError> {
    let declared_paths: Vec<String> = declared_route_paths();

    if declared_paths != [PRERENDERED_ROUTE_PATH] {
        return Err(AppError::from(format!(
            "the app declares routes this build does not prerender; [declared={} prerendered={PRERENDERED_ROUTE_PATH}]",
            declared_paths.join(","),
        )));
    }

    let render_document = leptos_axum::render_app_async(move || shell(leptos_options.clone()));
    let request: Request<Body> = Request::builder()
        .uri(PRERENDERED_ROUTE_PATH)
        .body(Body::empty())
        .map_err(|error| AppError::from(format!("could not build the render request; [error={error}]")))?;
    let response: axum::response::Response = render_document(request)
        .await;
    let status: StatusCode = response.status();

    /* The router answers an unmatched path with its fallback view and a non-success status, which would
       otherwise be written out as the deployed document. */
    if !status.is_success() {
        return Err(AppError::from(format!(
            "rendering did not succeed; [status={status} path={PRERENDERED_ROUTE_PATH}]",
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
   environment, and a build-time render runs on its own, so the settings come from the manifest instead. */
pub fn read_leptos_manifest_options() -> Result<LeptosOptions, AppError> {
    let configuration: ConfFile = get_configuration(Some(MANIFEST_PATH))
        .map_err(|error| AppError::from(format!(
            "could not read the leptos settings; [path={MANIFEST_PATH} error={error}]",
        )))?;

    apply_hash_files_override(configuration.leptos_options)
}

/* Hashed filenames are a deploy-time choice rather than a manifest setting, because cargo-leptos only
   re-hashes on a full build: under `watch`, an incremental rebuild writes the unhashed names and leaves
   the hash file naming the previous build, so the page would load stale assets. The variable is
   cargo-leptos's own, and it layers the same variable over the manifest, so both processes agree on the
   filenames. */
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
    let workspace_root: PathBuf = find_workspace_root()?;
    let declared_site_root: PathBuf = workspace_root.join(leptos_options.site_root.as_ref());

    fs::canonicalize(&declared_site_root)
        .map_err(|error| AppError::from(format!(
            "the site root does not exist, so no build has run; [path={} error={error}]",
            declared_site_root.display(),
        )))
}

/* The manifest's site-root is relative to the workspace root, so it resolves against the directory holding
   the workspace manifest rather than this crate's own. Searching upward keeps working if this crate moves
   deeper in the tree, which counting parent directories would not. */
fn find_workspace_root() -> Result<PathBuf, AppError> {
    let mut directory: Option<&Path> = Some(Path::new(CRATE_DIRECTORY));

    while let Some(candidate) = directory {
        if declares_workspace(&candidate.join("Cargo.toml")) {
            return Ok(candidate.to_path_buf());
        }

        directory = candidate.parent();
    }

    Err(AppError::from(format!(
        "no Cargo.toml declaring a workspace at or above this crate; [crate={CRATE_DIRECTORY}]",
    )))
}

fn declares_workspace(manifest_path: &Path) -> bool {
    let manifest_text: Result<String, std::io::Error> = fs::read_to_string(manifest_path);

    match manifest_text {
        Ok(manifest_text) => manifest_text
            .lines()
            .any(|line| line.trim_start().starts_with(WORKSPACE_MANIFEST_MARKER)),
        Err(_) => false,
    }
}
