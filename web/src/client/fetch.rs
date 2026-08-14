use js_sys::{ArrayBuffer, Promise, Uint8Array};
use web_sys::{RequestCache, RequestInit, Window};

use shared::artifact::manifest;
use shared::http::{HttpCacheMode, HttpMethod, HttpRequest, Response};
use shared::AppError;

use crate::client::js;

/// Issues `request` and returns the status and body without judging the status; the caller decides what
/// a non-2xx means. Errors only on a transport/JS failure.
pub async fn fetch(request: &HttpRequest) -> Result<Response, AppError> {
    let window: Window = js::get_window()?;

    let init: RequestInit = RequestInit::new();
    init.set_method(request.method.as_str());

    match request.cache_mode {
        HttpCacheMode::Reload => init.set_cache(RequestCache::Reload),
        HttpCacheMode::Default => {}
    }

    let fetch_promise: Promise = window.fetch_with_str_and_init(&request.url, &init);
    let response: web_sys::Response = js::await_and_cast(fetch_promise).await?;

    let status: u16 = response.status();
    let buffer_promise: Promise = response.array_buffer().map_err(js::error)?;
    let array_buffer: ArrayBuffer = js::await_and_cast(buffer_promise).await?;
    let bytes: Vec<u8> = Uint8Array::new(&array_buffer).to_vec();

    Ok(Response { status, bytes })
}

pub async fn fetch_bytes(request: &HttpRequest) -> Result<Vec<u8>, AppError> {
    let response: Response = fetch(request).await?;

    if !response.is_success() {
        return Err(AppError::from(format!("fetch: {} returned HTTP {}", request.url, response.status)));
    }

    Ok(response.bytes)
}

pub async fn fetch_discovery(discovery_url: &str) -> Result<Vec<u8>, AppError> {
    fetch_bytes(&HttpRequest {
        method: HttpMethod::Get,
        url: discovery_url.to_string(),
        cache_mode: HttpCacheMode::Reload,
    })
    .await
}

pub async fn fetch_manifest(repository_base_url: &str) -> Result<Vec<u8>, AppError> {
    let base: &str = repository_base_url.trim_end_matches('/');
    let url: String = format!("{base}/{}", manifest::MANIFEST_LATEST_KEY);

    fetch_bytes(&HttpRequest {
        method: HttpMethod::Get,
        url,
        cache_mode: HttpCacheMode::Reload,
    })
    .await
}

pub async fn fetch_artifact_file(
    repository_base_url: &str,
    version_label: &str,
    relative_path: &str,
) -> Result<Vec<u8>, AppError> {
    let base: &str = repository_base_url.trim_end_matches('/');
    let url: String = format!("{base}/{version_label}/{relative_path}");

    fetch_bytes(&HttpRequest {
        method: HttpMethod::Get,
        url,
        cache_mode: HttpCacheMode::Default,
    })
    .await
}
