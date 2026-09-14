use crate::artifact::manifest;
use crate::error::AppErrorStatic;
use crate::http::{HttpCacheMode, HttpFetch, HttpMethod, HttpRequest, Response};

pub async fn fetch_bytes(http_fetch: &impl HttpFetch, request: &HttpRequest) -> Result<Vec<u8>, AppErrorStatic> {
    let response: Response = http_fetch.fetch(request).await?;

    if !response.is_success() {
        return Err(AppErrorStatic::from(format!("fetch: {} returned HTTP {}", request.url, response.status)));
    }

    Ok(response.bytes)
}

pub async fn fetch_discovery(http_fetch: &impl HttpFetch, discovery_url: &str) -> Result<Vec<u8>, AppErrorStatic> {
    fetch_bytes(http_fetch, &HttpRequest {
        method: HttpMethod::Get,
        url: discovery_url.to_string(),
        cache_mode: HttpCacheMode::Reload,
    })
    .await
}

pub async fn fetch_manifest(http_fetch: &impl HttpFetch, repository_base_url: &str) -> Result<Vec<u8>, AppErrorStatic> {
    fetch_manifest_at_key(http_fetch, repository_base_url, manifest::MANIFEST_LATEST_KEY).await
}

pub async fn fetch_manifest_at_key(
    http_fetch: &impl HttpFetch,
    repository_base_url: &str,
    key: &str,
) -> Result<Vec<u8>, AppErrorStatic> {
    let base: &str = repository_base_url.trim_end_matches('/');
    let url: String = format!("{base}/{key}");

    fetch_bytes(http_fetch, &HttpRequest {
        method: HttpMethod::Get,
        url,
        cache_mode: HttpCacheMode::Reload,
    })
    .await
}

pub async fn fetch_artifact_file(
    http_fetch: &impl HttpFetch,
    repository_base_url: &str,
    version_label: &str,
    relative_path: &str,
) -> Result<Vec<u8>, AppErrorStatic> {
    let base: &str = repository_base_url.trim_end_matches('/');
    let url: String = format!("{base}/{version_label}/{relative_path}");

    fetch_bytes(http_fetch, &HttpRequest {
        method: HttpMethod::Get,
        url,
        cache_mode: HttpCacheMode::Default,
    })
    .await
}

pub async fn fetch_embedded_manifest(
    http_fetch: &impl HttpFetch,
    embedded_base_url: &str,
) -> Result<Vec<u8>, AppErrorStatic> {
    let base: &str = embedded_base_url.trim_end_matches('/');
    let url: String = format!("{base}/{}", manifest::MANIFEST_FILENAME);

    fetch_bytes(http_fetch, &HttpRequest {
        method: HttpMethod::Get,
        url,
        cache_mode: HttpCacheMode::Reload,
    })
    .await
}

pub async fn fetch_embedded_file(
    http_fetch: &impl HttpFetch,
    embedded_base_url: &str,
    relative_path: &str,
) -> Result<Vec<u8>, AppErrorStatic> {
    let base: &str = embedded_base_url.trim_end_matches('/');
    let url: String = format!("{base}/{relative_path}");

    fetch_bytes(http_fetch, &HttpRequest {
        method: HttpMethod::Get,
        url,
        cache_mode: HttpCacheMode::Default,
    })
    .await
}
