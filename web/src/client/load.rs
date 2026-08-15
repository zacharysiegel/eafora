use std::sync::Arc;

use chrono::{DateTime, Utc};
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
const VERSIONS_KEPT: usize = 2;

/// The repository base the live bundle is fetched from. Its `latest/manifest.json` bytes travel with it
/// because resolution already fetched them; fetching them again would cost a redundant round trip.
struct ResolvedRepository {
    base_url: String,
    manifest_bytes: Vec<u8>,
}

pub async fn load_embedded_bundle(
    cache: &OpfsArtifactCache,
    distribution_context: DistributionContext,
) -> Result<Bundle, AppError> {
    let manifest_url: String = format!("{EMBEDDED_BASE_URL}/{}", manifest::MANIFEST_FILENAME);

    let manifest_bytes: Vec<u8> = fetch::fetch_bytes(&HttpRequest {
        method: HttpMethod::Get,
        url: manifest_url,
        cache_mode: HttpCacheMode::Reload,
    })
    .await?;

    let manifest: Manifest = manifest::parse_manifest(&manifest_bytes)?;

    cache.put(&manifest.version, manifest::MANIFEST_FILENAME, &manifest_bytes).await?;

    for entry in manifest.file_entries() {
        let file_url: String = format!("{EMBEDDED_BASE_URL}/{}", entry.relative_path);

        let file_bytes: Vec<u8> = fetch::fetch_bytes(&HttpRequest {
            method: HttpMethod::Get,
            url: file_url,
            cache_mode: HttpCacheMode::Default,
        })
        .await?;

        filesystem::verify_sha256(&file_bytes, &entry.sha256)?;
        cache.put(&manifest.version, &entry.relative_path, &file_bytes).await?;
    }

    Bundle::open(cache, &manifest.version, distribution_context).await
}

pub async fn open_newest_cached_bundle(
    cache: &OpfsArtifactCache,
    distribution_context: DistributionContext,
) -> Result<Option<Bundle>, AppError> {
    for version_label in version_labels_newest_first(cache).await? {
        match Bundle::open(cache, &version_label, distribution_context).await {
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

/// Deletes every cached version past the `VERSIONS_KEPT` newest.
pub async fn evict_stale_versions(cache: &OpfsArtifactCache) -> Result<(), AppError> {
    let kept_version_labels: Vec<String> =
        version_labels_newest_first(cache).await?.into_iter().take(VERSIONS_KEPT).collect();

    cache.evict_all_except(&kept_version_labels).await
}

/// Cached version labels, newest artifact first. The order comes from each version's `artifact_created`
/// rather than from comparing labels: `YYYY-MM-DD+<surname>` orders chronologically only across differing
/// dates, and two builds sharing a date fall back to comparing arbitrary surnames. A version whose
/// manifest cannot be read sorts last, so it is opened last and evicted first.
async fn version_labels_newest_first(cache: &OpfsArtifactCache) -> Result<Vec<String>, AppError> {
    let version_labels: Vec<String> = cache.list_versions().await?;
    let mut labels_by_creation: Vec<(Option<DateTime<Utc>>, String)> = Vec::with_capacity(version_labels.len());

    for version_label in version_labels {
        let artifact_created: Option<DateTime<Utc>> = read_cached_artifact_created(cache, &version_label).await;

        labels_by_creation.push((artifact_created, version_label));
    }

    labels_by_creation.sort_by(|(left_created, _), (right_created, _)| right_created.cmp(left_created));

    Ok(labels_by_creation.into_iter().map(|(_, version_label)| version_label).collect())
}

/// `None` when the version's manifest is absent or unparseable. A damaged version is unorderable rather
/// than fatal, so it cannot stop the others from being opened or evicted.
async fn read_cached_artifact_created(cache: &OpfsArtifactCache, version_label: &str) -> Option<DateTime<Utc>> {
    let manifest_bytes: Option<Vec<u8>> = cache
        .get(version_label, manifest::MANIFEST_FILENAME)
        .await
        .map_err(|error| {
            log::warn!("reading a cached manifest failed; [version_label={version_label} error={error}]")
        })
        .ok()?;
    let manifest: Manifest = manifest::parse_manifest(&manifest_bytes?)
        .map_err(|error| {
            log::warn!("parsing a cached manifest failed; [version_label={version_label} error={error}]")
        })
        .ok()?;

    Some(manifest.artifact_created)
}

pub async fn load_live_bundle(
    cache: &OpfsArtifactCache,
    static_base: &str,
    distribution_context: DistributionContext,
) -> Result<Bundle, AppError> {
    let resolved_repository: ResolvedRepository = resolve_repository(static_base).await?;

    open_fetched_live_bundle(
        cache,
        &resolved_repository.base_url,
        &resolved_repository.manifest_bytes,
        distribution_context,
    )
    .await
}

/// Reconciles the discovery document against the static base. The static base's manifest is requested
/// concurrently with discovery, so the common case (discovery agrees with the static base, or discovery is
/// unavailable) resolves in one round trip; a discovery document naming a different base discards that
/// response and pays a second.
async fn resolve_repository(static_base: &str) -> Result<ResolvedRepository, AppError> {
    let (discovery_bytes_result, speculative_manifest_result): (Result<Vec<u8>, AppError>, Result<Vec<u8>, AppError>) =
        tokio::join!(
            fetch::fetch_discovery(live_resolve::DISCOVERY_PATH),
            fetch::fetch_manifest(static_base),
        );

    let parsed_discovery: Result<DiscoveryDocument, AppError> = discovery_bytes_result
        .and_then(|discovery_bytes| artifact::parse_discovery_document(&discovery_bytes))
        .inspect_err(|error| {
            log::warn!("discovery unavailable, falling back to the static repository base; [error={error}]")
        });
    let authoritative_base: AuthoritativeBase =
        live_resolve::authoritative_repository_base(static_base, parsed_discovery);

    match authoritative_base {
        AuthoritativeBase::Static => {
            let manifest_bytes: Vec<u8> = speculative_manifest_result?;

            Ok(ResolvedRepository {
                base_url: static_base.to_string(),
                manifest_bytes,
            })
        }
        AuthoritativeBase::Discovered(discovered_base) => {
            let manifest_bytes: Vec<u8> = fetch::fetch_manifest(&discovered_base).await?;

            Ok(ResolvedRepository {
                base_url: discovered_base,
                manifest_bytes,
            })
        }
    }
}

async fn open_fetched_live_bundle(
    cache: &OpfsArtifactCache,
    repository_base_url: &str,
    manifest_bytes: &[u8],
    distribution_context: DistributionContext,
) -> Result<Bundle, AppError> {
    let manifest: Manifest = manifest::parse_manifest(manifest_bytes)?;

    put_live_files(repository_base_url, &manifest).await?;

    cache.put(&manifest.version, manifest::MANIFEST_FILENAME, manifest_bytes).await?;

    Bundle::open(cache, &manifest.version, distribution_context).await
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
    if is_already_cached(cache, version_label, relative_path, sha256).await {
        return Ok(());
    }

    let _permit: SemaphorePermit<'_> = semaphore
        .acquire()
        .await
        .map_err(|error: AcquireError| AppError::from(format!("live fetch semaphore closed; [error={error}]")))?;

    let file_bytes: Vec<u8> = fetch::fetch_artifact_file(repository_base_url, version_label, relative_path).await?;

    filesystem::verify_sha256(&file_bytes, sha256)?;
    cache.put(version_label, relative_path, &file_bytes).await?;

    Ok(())
}

/// Whether the cache already holds this file with the hash the manifest declares. Checked before the
/// semaphore, since a local read should not queue behind in-flight downloads. A read failure or a hash
/// mismatch answers false, which re-fetches and so repairs a truncated or corrupted entry.
async fn is_already_cached(
    cache: &OpfsArtifactCache,
    version_label: &str,
    relative_path: &str,
    sha256: &str,
) -> bool {
    let cached_bytes: Option<Vec<u8>> = cache
        .get(version_label, relative_path)
        .await
        .map_err(|error| {
            log::warn!("reading a cached artifact file failed; [relative_path={relative_path} error={error}]")
        })
        .ok()
        .flatten();

    let Some(cached_bytes) = cached_bytes else {
        return false;
    };

    filesystem::verify_sha256(&cached_bytes, sha256).is_ok()
}

/// Exercised in the browser: the ordering these cover is a property of real OPFS reads, since the
/// version list and each manifest come back through `FileSystemDirectoryHandle`.
#[cfg(test)]
mod tests {
    use super::*;

    /// A parseable manifest carrying no files, so a test can seed a version whose only meaningful
    /// content is its `artifact_created`.
    fn manifest_json(version_label: &str, artifact_created: &str) -> Vec<u8> {
        format!(
            r#"{{
                "manifest_schema_version": 1,
                "version": "{version_label}",
                "artifact_created": "{artifact_created}",
                "geometry": {{
                    "relative_path": "geometry/world.fgb",
                    "size_bytes": 1,
                    "sha256": "{}"
                }},
                "statistics": {{}},
                "source_revisions": {{}}
            }}"#,
            "ab".repeat(32),
        )
        .into_bytes()
    }

    /// The labels here are the shape that broke the previous lexicographic ordering: same date, and the
    /// surname of the older artifact sorts above the newer one.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    async fn version_labels_newest_first_orders_same_date_labels_by_artifact_created() {
        let cache: OpfsArtifactCache = OpfsArtifactCache::create().await.unwrap();
        let newer_label: &str = "2026-08-14+macdiarmid";
        let older_label: &str = "2026-08-14+yeats";

        cache
            .put(older_label, manifest::MANIFEST_FILENAME, &manifest_json(older_label, "2026-08-14T01:00:00Z"))
            .await
            .unwrap();
        cache
            .put(newer_label, manifest::MANIFEST_FILENAME, &manifest_json(newer_label, "2026-08-14T02:00:00Z"))
            .await
            .unwrap();

        let ordered_version_labels: Vec<String> = version_labels_newest_first(&cache).await.unwrap();

        let newer_position: usize = ordered_version_labels.iter().position(|label| label == newer_label).unwrap();
        let older_position: usize = ordered_version_labels.iter().position(|label| label == older_label).unwrap();
        assert!(newer_position < older_position);

        cache.delete_version(newer_label).await.unwrap();
        cache.delete_version(older_label).await.unwrap();
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    async fn evict_stale_versions_keeps_the_newest_by_artifact_created() {
        let cache: OpfsArtifactCache = OpfsArtifactCache::create().await.unwrap();
        let evicted_label: &str = "2026-07-01+zulu";
        let kept_labels: [&str; 2] = ["2026-07-02+alpha", "2026-07-03+bravo"];

        cache
            .put(evicted_label, manifest::MANIFEST_FILENAME, &manifest_json(evicted_label, "2026-07-01T00:00:00Z"))
            .await
            .unwrap();
        for (index, kept_label) in kept_labels.iter().enumerate() {
            let artifact_created: String = format!("2026-07-0{}T00:00:00Z", index + 2);

            cache
                .put(kept_label, manifest::MANIFEST_FILENAME, &manifest_json(kept_label, &artifact_created))
                .await
                .unwrap();
        }

        evict_stale_versions(&cache).await.unwrap();

        assert_eq!(cache.get(evicted_label, manifest::MANIFEST_FILENAME).await.unwrap(), None);
        for kept_label in kept_labels {
            assert!(cache.get(kept_label, manifest::MANIFEST_FILENAME).await.unwrap().is_some());
        }

        for kept_label in kept_labels {
            cache.delete_version(kept_label).await.unwrap();
        }
    }
}
