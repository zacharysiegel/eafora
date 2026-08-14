use std::sync::Arc;

use shared::artifact::{self, manifest, ArtifactCache, Bundle, DiscoveryDocument, Manifest, ManifestEntry};
use shared::filesystem;
use shared::http::{HttpCacheMode, HttpMethod, HttpRequest};
use shared::license::DistributionContext;
use shared::AppError;
use tokio::sync::oneshot;
use tokio::sync::oneshot::error::RecvError;
use tokio::sync::oneshot::{Receiver, Sender};
use tokio::sync::{AcquireError, Semaphore, SemaphorePermit};

use crate::client::cache::OpfsArtifactCache;
use crate::client::fetch;
use crate::live_resolve::{self, AuthoritativeBase};

const EMBEDDED_BASE_URL: &str = "/embedded_artifacts";
const LIVE_FETCH_PARALLELISM: usize = 6;

pub async fn load_embedded_bundle(cache: &OpfsArtifactCache) -> Result<Bundle, AppError> {
    let manifest_url: String = format!("{EMBEDDED_BASE_URL}/{}", manifest::MANIFEST_FILENAME);

    let manifest_bytes: Vec<u8> = fetch::fetch_bytes(&HttpRequest {
        method: HttpMethod::Get,
        url: manifest_url,
        cache: HttpCacheMode::Reload,
    })
    .await?;

    let manifest: Manifest = manifest::parse_manifest(&manifest_bytes)?;

    cache.put(&manifest.version, manifest::MANIFEST_FILENAME, &manifest_bytes).await?;

    for entry in manifest.file_entries() {
        let file_url: String = format!("{EMBEDDED_BASE_URL}/{}", entry.relative_path);

        let file_bytes: Vec<u8> = fetch::fetch_bytes(&HttpRequest {
            method: HttpMethod::Get,
            url: file_url,
            cache: HttpCacheMode::Default,
        })
        .await?;

        filesystem::verify_sha256(&file_bytes, &entry.sha256)?;
        cache.put(&manifest.version, &entry.relative_path, &file_bytes).await?;
    }

    Bundle::open(cache, &manifest.version, DistributionContext::Embedded).await
}

pub async fn open_newest_cached_bundle(cache: &OpfsArtifactCache) -> Result<Option<Bundle>, AppError> {
    let mut version_labels: Vec<String> = cache.list_versions().await?;
    version_labels.sort();

    for version_label in version_labels.into_iter().rev() {
        match Bundle::open(cache, &version_label, DistributionContext::FirstParty).await {
            Ok(bundle) => return Ok(Some(bundle)),
            Err(error) => {
                log::warn!(
                    "opening a cached bundle failed, trying an older version; [version_label={version_label} error={error}]"
                );
            }
        }
    }

    Ok(None)
}

pub async fn load_live_bundle(cache: &OpfsArtifactCache, repository_base_url: &str) -> Result<Bundle, AppError> {
    let manifest_bytes: Vec<u8> = fetch::fetch_manifest(repository_base_url).await?;

    open_fetched_live_bundle(cache, repository_base_url, &manifest_bytes).await
}

pub async fn load_live_after_discovery(cache: &OpfsArtifactCache, baked_base: &str) -> Result<Bundle, AppError> {
    let discovery_bytes_result: Result<Vec<u8>, AppError> = fetch::fetch_discovery(live_resolve::DISCOVERY_PATH).await;
    let speculative_manifest_result: Result<Vec<u8>, AppError> = fetch::fetch_manifest(baked_base).await;

    let parsed_discovery: Result<DiscoveryDocument, AppError> = match discovery_bytes_result {
        Ok(discovery_bytes) => artifact::parse_discovery_document(&discovery_bytes),
        Err(error) => Err(error),
    };

    let authoritative_base: AuthoritativeBase =
        live_resolve::authoritative_repository_base(baked_base, parsed_discovery);

    let (repository_base_url, manifest_bytes): (String, Vec<u8>) = match authoritative_base {
        AuthoritativeBase::Baked => {
            let manifest_bytes: Vec<u8> = speculative_manifest_result?;

            (baked_base.to_string(), manifest_bytes)
        }
        AuthoritativeBase::Discovered(other) => {
            let _: Result<Vec<u8>, AppError> = speculative_manifest_result;
            let manifest_bytes: Vec<u8> = fetch::fetch_manifest(&other).await?;

            (other, manifest_bytes)
        }
    };

    open_fetched_live_bundle(cache, &repository_base_url, &manifest_bytes).await
}

async fn open_fetched_live_bundle(
    cache: &OpfsArtifactCache,
    repository_base_url: &str,
    manifest_bytes: &[u8],
) -> Result<Bundle, AppError> {
    let manifest: Manifest = manifest::parse_manifest(manifest_bytes)?;

    put_live_files(repository_base_url, &manifest).await?;

    cache.put(&manifest.version, manifest::MANIFEST_FILENAME, manifest_bytes).await?;

    Bundle::open(cache, &manifest.version, DistributionContext::FirstParty).await
}

async fn put_live_files(repository_base_url: &str, manifest: &Manifest) -> Result<(), AppError> {
    let semaphore: Arc<Semaphore> = Arc::new(Semaphore::new(LIVE_FETCH_PARALLELISM));
    let file_entries: Vec<&ManifestEntry> = manifest.file_entries().collect();
    let mut result_receivers: Vec<Receiver<Result<(), AppError>>> = Vec::new();

    for entry in file_entries {
        let (result_sender, result_receiver): (Sender<Result<(), AppError>>, Receiver<Result<(), AppError>>) =
            oneshot::channel();
        result_receivers.push(result_receiver);

        let semaphore: Arc<Semaphore> = Arc::clone(&semaphore);
        let repository_base_url: String = repository_base_url.to_string();
        let version_label: String = manifest.version.clone();
        let relative_path: String = entry.relative_path.clone();
        let sha256: String = entry.sha256.clone();

        wasm_bindgen_futures::spawn_local(async move {
            let cache: OpfsArtifactCache = OpfsArtifactCache;
            let result: Result<(), AppError> = fetch_and_cache_artifact_file(
                &cache,
                &semaphore,
                &repository_base_url,
                &version_label,
                &relative_path,
                &sha256,
            )
            .await;

            let _: Result<(), Result<(), AppError>> = result_sender.send(result);
        });
    }

    for result_receiver in result_receivers {
        let result: Result<(), AppError> = result_receiver
            .await
            .map_err(|error: RecvError| AppError::from(format!("live file fetch dropped; [error={error}]")))?;

        result?;
    }

    Ok(())
}

async fn fetch_and_cache_artifact_file(
    cache: &OpfsArtifactCache,
    semaphore: &Semaphore,
    repository_base_url: &str,
    version_label: &str,
    relative_path: &str,
    sha256: &str,
) -> Result<(), AppError> {
    let _permit: SemaphorePermit<'_> = semaphore
        .acquire()
        .await
        .map_err(|error: AcquireError| AppError::from(format!("live fetch semaphore closed; [error={error}]")))?;

    let file_bytes: Vec<u8> = fetch::fetch_artifact_file(repository_base_url, version_label, relative_path).await?;

    filesystem::verify_sha256(&file_bytes, sha256)?;
    cache.put(version_label, relative_path, &file_bytes).await?;

    Ok(())
}
