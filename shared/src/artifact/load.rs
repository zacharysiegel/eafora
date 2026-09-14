use crate::artifact::{self, fetch, manifest, version_rank, ArtifactCache, AuthoritativeBase, Bundle,
    CachedVersionRank, DiscoveryDocument, Manifest, ManifestEntry};
use crate::error::{AppError, AppErrorStatic};
use crate::filesystem;
use crate::http::HttpFetch;
use crate::license::DistributionContext;
use futures_util::StreamExt;

const LIVE_FETCH_CONCURRENCY: usize = 6;
const VERSIONS_KEPT: usize = 2;

/// The repository base the live bundle is fetched from. Its `latest/manifest.json` bytes travel with it
/// because resolution already fetched them; fetching them again would cost a redundant round trip.
struct ResolvedRepository {
    base_url: String,
    manifest_bytes: Vec<u8>,
}

pub async fn load_embedded_bundle(
    cache: &impl ArtifactCache,
    http_fetch: &impl HttpFetch,
    embedded_base_url: &str,
    distribution_context: DistributionContext,
) -> Result<Bundle, AppErrorStatic> {
    let manifest_bytes: Vec<u8> = fetch::fetch_embedded_manifest(http_fetch, embedded_base_url).await?;
    let manifest: Manifest = manifest::parse_manifest(&manifest_bytes)?;

    cache.put(&manifest.version, manifest::MANIFEST_FILENAME, &manifest_bytes).await?;

    for entry in manifest.file_entries() {
        let file_bytes: Vec<u8> =
            fetch::fetch_embedded_file(http_fetch, embedded_base_url, &entry.relative_path).await?;

        filesystem::verify_sha256(&file_bytes, &entry.sha256)?;
        cache.put(&manifest.version, &entry.relative_path, &file_bytes).await?;
    }

    Ok(Bundle::open(cache, &manifest.version, distribution_context).await?)
}

pub async fn open_newest_cached_bundle(
    cache: &impl ArtifactCache,
    distribution_context: DistributionContext,
) -> Result<Option<Bundle>, AppErrorStatic> {
    for version_label in version_labels_newest_first(cache).await? {
        let opened: Result<Bundle, AppErrorStatic> =
            Bundle::open(cache, &version_label, distribution_context).await;

        let open_failure: String = match opened {
            Ok(bundle) => return Ok(Some(bundle)),
            Err(error) => error.to_string(),
        };

        /* A cached version this build cannot read is useless and re-fetchable, and keeping it would fail
           the same way on the next start while holding one of the retained slots. */
        log::warn!(
            "discarding a cached bundle this build cannot open; [version_label={version_label} error={open_failure}]"
        );
        cache.delete_version(&version_label).await?;
    }

    Ok(None)
}

/// Deletes every cached version past the `VERSIONS_KEPT` newest.
pub async fn evict_stale_versions(cache: &impl ArtifactCache) -> Result<(), AppErrorStatic> {
    let kept_version_labels: Vec<String> =
        version_labels_newest_first(cache).await?.into_iter().take(VERSIONS_KEPT).collect();

    for version_label in cache.list_versions().await? {
        if kept_version_labels.contains(&version_label) {
            continue;
        }

        cache.delete_version(&version_label).await?;
    }

    Ok(())
}

/// Cached version labels, best first. The rank comes from each version's manifest rather than from
/// comparing labels: `YYYY-MM-DD+<surname>` orders chronologically only across differing dates, and two
/// builds sharing a date fall back to comparing arbitrary surnames. A version whose manifest cannot be read
/// ranks last, so it is opened last and evicted first.
pub async fn version_labels_newest_first(cache: &impl ArtifactCache) -> Result<Vec<String>, AppErrorStatic> {
    let version_labels: Vec<String> = cache.list_versions().await?;
    let mut ranked_labels: Vec<(CachedVersionRank, String)> = Vec::with_capacity(version_labels.len());

    for version_label in version_labels {
        let manifest: Option<Manifest> = read_cached_manifest(cache, &version_label).await;
        let rank: CachedVersionRank = version_rank::rank_cached_version(manifest.as_ref());

        ranked_labels.push((rank, version_label));
    }

    ranked_labels.sort_by(|(left_rank, _), (right_rank, _)| right_rank.cmp(left_rank));

    Ok(ranked_labels.into_iter().map(|(_rank, version_label)| version_label).collect())
}

/// `None` when the version's manifest is absent or unparseable. A damaged version is unrankable rather
/// than fatal, so it cannot stop the others from being opened or evicted.
async fn read_cached_manifest(cache: &impl ArtifactCache, version_label: &str) -> Option<Manifest> {
    let manifest_bytes: Option<Vec<u8>> = cache
        .get(version_label, manifest::MANIFEST_FILENAME)
        .await
        .map_err(|error| {
            log::warn!("reading a cached manifest failed; [version_label={version_label} error={error}]")
        })
        .ok()?;

    manifest::parse_manifest(&manifest_bytes?)
        .map_err(|error| {
            log::warn!("parsing a cached manifest failed; [version_label={version_label} error={error}]")
        })
        .ok()
}

pub async fn load_live_bundle(
    cache: &impl ArtifactCache,
    http_fetch: &impl HttpFetch,
    discovery_url: &str,
    static_base: &str,
    distribution_context: DistributionContext,
) -> Result<Bundle, AppErrorStatic> {
    let resolved_repository: ResolvedRepository =
        resolve_repository(http_fetch, discovery_url, static_base).await?;
    let manifest_bytes: Vec<u8> = readable_manifest_bytes(http_fetch, &resolved_repository).await;

    open_fetched_live_bundle(
        cache,
        http_fetch,
        &resolved_repository.base_url,
        &manifest_bytes,
        distribution_context,
    )
    .await
}

/// The repository's manifest when this build can read it, and otherwise the newest manifest published at the
/// schema version it does read. A pointer that cannot be fetched leaves the resolved bytes in place, so the
/// version mismatch is what the caller reports.
async fn readable_manifest_bytes(
    http_fetch: &impl HttpFetch,
    resolved_repository: &ResolvedRepository,
) -> Vec<u8> {
    let fallback_key: Option<String> = manifest::schema_fallback_key(&resolved_repository.manifest_bytes);

    let Some(fallback_key) = fallback_key
    else {
        return resolved_repository.manifest_bytes.clone();
    };

    let fetched: Result<Vec<u8>, AppErrorStatic> =
        fetch::fetch_manifest_at_key(http_fetch, &resolved_repository.base_url, &fallback_key).await;

    match fetched {
        Ok(fallback_bytes) => {
            log::info!("the repository is at a newer schema version; [pointer={fallback_key}]");

            fallback_bytes
        }
        Err(error) => {
            log::warn!("fetching the schema pointer failed; [pointer={fallback_key} error={error}]");

            resolved_repository.manifest_bytes.clone()
        }
    }
}

/// Reconciles the discovery document against the static base. The static base's manifest is requested
/// concurrently with discovery, so the common case (discovery agrees with the static base, or discovery is
/// unavailable) resolves in one round trip; a discovery document naming a different base discards that
/// response and pays a second.
async fn resolve_repository(
    http_fetch: &impl HttpFetch,
    discovery_url: &str,
    static_base: &str,
) -> Result<ResolvedRepository, AppErrorStatic> {
    let (discovery_bytes_result, speculative_manifest_result): (
        Result<Vec<u8>, AppErrorStatic>,
        Result<Vec<u8>, AppErrorStatic>,
    ) = futures_util::future::join(
        fetch::fetch_discovery(http_fetch, discovery_url),
        fetch::fetch_manifest(http_fetch, static_base),
    )
    .await;

    let parsed_discovery: Result<DiscoveryDocument, AppError> = discovery_bytes_result
        .map_err(AppError::from)
        .and_then(|discovery_bytes| artifact::parse_discovery_document(&discovery_bytes))
        .inspect_err(|error| {
            log::warn!("discovery unavailable, falling back to the static repository base; [error={error}]")
        });
    let authoritative_base: AuthoritativeBase =
        artifact::authoritative_repository_base(static_base, parsed_discovery);

    match authoritative_base {
        AuthoritativeBase::Static => {
            let manifest_bytes: Vec<u8> = speculative_manifest_result?;

            Ok(ResolvedRepository {
                base_url: static_base.to_string(),
                manifest_bytes,
            })
        }
        AuthoritativeBase::Discovered(discovered_base) => {
            let manifest_bytes: Vec<u8> = fetch::fetch_manifest(http_fetch, &discovered_base).await?;

            Ok(ResolvedRepository {
                base_url: discovered_base,
                manifest_bytes,
            })
        }
    }
}

async fn open_fetched_live_bundle(
    cache: &impl ArtifactCache,
    http_fetch: &impl HttpFetch,
    repository_base_url: &str,
    manifest_bytes: &[u8],
    distribution_context: DistributionContext,
) -> Result<Bundle, AppErrorStatic> {
    let manifest: Manifest = manifest::parse_manifest(manifest_bytes)?;

    put_live_files(cache, http_fetch, repository_base_url, &manifest).await?;

    cache.put(&manifest.version, manifest::MANIFEST_FILENAME, manifest_bytes).await?;

    Ok(Bundle::open(cache, &manifest.version, distribution_context).await?)
}

/// At most `LIVE_FETCH_CONCURRENCY` files are in flight, all within this task: the work is I/O-bound, and
/// spawning would demand a `Send` bound `ArtifactCache` does not carry.
///
/// The entries are cloned up front so each fetch owns its own. A closure whose argument is a reference and
/// whose return value borrows it cannot be proven general over lifetimes, and the iOS FFI needs this future
/// to be `Send`.
async fn put_live_files(
    cache: &impl ArtifactCache,
    http_fetch: &impl HttpFetch,
    repository_base_url: &str,
    manifest: &Manifest,
) -> Result<(), AppErrorStatic> {
    let file_entries: Vec<ManifestEntry> = manifest.file_entries().cloned().collect();

    let pending_fetches = file_entries.into_iter().map(|entry| async move {
        fetch_and_cache_artifact_file(
            cache,
            http_fetch,
            repository_base_url,
            &manifest.version,
            &entry.relative_path,
            &entry.sha256,
        )
        .await
    });

    let mut fetch_results = futures_util::stream::iter(pending_fetches).buffer_unordered(LIVE_FETCH_CONCURRENCY);

    while let Some(fetch_result) = fetch_results.next().await {
        fetch_result?;
    }

    Ok(())
}

async fn fetch_and_cache_artifact_file(
    cache: &impl ArtifactCache,
    http_fetch: &impl HttpFetch,
    repository_base_url: &str,
    version_label: &str,
    relative_path: &str,
    sha256: &str,
) -> Result<(), AppErrorStatic> {
    if is_already_cached(cache, version_label, relative_path, sha256).await {
        return Ok(());
    }

    let file_bytes: Vec<u8> =
        fetch::fetch_artifact_file(http_fetch, repository_base_url, version_label, relative_path).await?;

    filesystem::verify_sha256(&file_bytes, sha256)?;
    cache.put(version_label, relative_path, &file_bytes).await?;

    Ok(())
}

/// Whether the cache already holds this file with the hash the manifest declares. A read failure or a hash
/// mismatch answers false, which re-fetches and so repairs a truncated or corrupted entry.
async fn is_already_cached(
    cache: &impl ArtifactCache,
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

    let Some(cached_bytes) = cached_bytes
    else {
        return false;
    };

    filesystem::verify_sha256(&cached_bytes, sha256).is_ok()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{DateTime, Utc};

    use crate::artifact::cache::tests::MockArtifactCache;
    use crate::artifact::{BundleVariant, ManifestEntry};
    use crate::canonical::{LicenseShardClass, StatisticKind};
    use crate::http::http_model::tests::MockHttpFetch;

    use super::*;

    const REPOSITORY_BASE_URL: &str = "https://repository.example";
    const VERSION_LABEL: &str = "2026-08-14+macdiarmid";

    fn artifact_url(relative_path: &str) -> String {
        format!("{REPOSITORY_BASE_URL}/{VERSION_LABEL}/{relative_path}")
    }

    /// A manifest naming one geometry file and one statistics shard, so a test covers both arms of
    /// `file_entries`.
    fn two_file_manifest(geometry_bytes: &[u8], shard_bytes: &[u8]) -> Manifest {
        let mut statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>> = BTreeMap::new();
        let mut shards_by_license_class: BTreeMap<LicenseShardClass, ManifestEntry> = BTreeMap::new();

        shards_by_license_class.insert(LicenseShardClass::Base, ManifestEntry {
            relative_path: "statistics/tfr.base.sqlite".to_string(),
            size_bytes: shard_bytes.len() as u64,
            sha256: filesystem::sha256_hex(shard_bytes),
        });
        statistics.insert(StatisticKind::Tfr, shards_by_license_class);

        Manifest {
            manifest_schema_version: manifest::MANIFEST_SCHEMA_VERSION,
            version: VERSION_LABEL.to_string(),
            variant: BundleVariant::Complete,
            artifact_created: "2026-08-14T02:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            geometry: ManifestEntry {
                relative_path: "geometry/world.fgb".to_string(),
                size_bytes: geometry_bytes.len() as u64,
                sha256: filesystem::sha256_hex(geometry_bytes),
            },
            statistics,
            source_revisions: BTreeMap::new(),
            source_attributions: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn put_live_files_caches_every_manifest_entry() {
        let geometry_bytes: &[u8] = b"geometry";
        let shard_bytes: &[u8] = b"shard";
        let manifest: Manifest = two_file_manifest(geometry_bytes, shard_bytes);
        let cache: MockArtifactCache = MockArtifactCache::new();
        let http_fetch: MockHttpFetch = MockHttpFetch::new(BTreeMap::from([
            (artifact_url("geometry/world.fgb"), geometry_bytes.to_vec()),
            (artifact_url("statistics/tfr.base.sqlite"), shard_bytes.to_vec()),
        ]));

        put_live_files(&cache, &http_fetch, REPOSITORY_BASE_URL, &manifest).await.unwrap();

        assert_eq!(
            cache.get(VERSION_LABEL, "geometry/world.fgb").await.unwrap().as_deref(),
            Some(geometry_bytes),
        );
        assert_eq!(
            cache.get(VERSION_LABEL, "statistics/tfr.base.sqlite").await.unwrap().as_deref(),
            Some(shard_bytes),
        );
    }

    #[tokio::test]
    async fn put_live_files_does_not_refetch_a_file_already_cached_at_the_manifest_hash() {
        let geometry_bytes: &[u8] = b"geometry";
        let shard_bytes: &[u8] = b"shard";
        let manifest: Manifest = two_file_manifest(geometry_bytes, shard_bytes);
        let cache: MockArtifactCache = MockArtifactCache::new();
        cache.insert(VERSION_LABEL, "geometry/world.fgb", geometry_bytes.to_vec()).await;
        let http_fetch: MockHttpFetch = MockHttpFetch::new(BTreeMap::from([(
            artifact_url("statistics/tfr.base.sqlite"),
            shard_bytes.to_vec(),
        )]));

        put_live_files(&cache, &http_fetch, REPOSITORY_BASE_URL, &manifest).await.unwrap();

        assert_eq!(
            http_fetch.requested_urls().await,
            vec![artifact_url("statistics/tfr.base.sqlite")],
        );
    }

    /// A cached copy that no longer matches the manifest is refetched, which repairs a truncated entry.
    #[tokio::test]
    async fn put_live_files_refetches_a_cached_file_whose_hash_no_longer_matches() {
        let geometry_bytes: &[u8] = b"geometry";
        let shard_bytes: &[u8] = b"shard";
        let manifest: Manifest = two_file_manifest(geometry_bytes, shard_bytes);
        let cache: MockArtifactCache = MockArtifactCache::new();
        cache.insert(VERSION_LABEL, "geometry/world.fgb", b"truncated".to_vec()).await;
        let http_fetch: MockHttpFetch = MockHttpFetch::new(BTreeMap::from([
            (artifact_url("geometry/world.fgb"), geometry_bytes.to_vec()),
            (artifact_url("statistics/tfr.base.sqlite"), shard_bytes.to_vec()),
        ]));

        put_live_files(&cache, &http_fetch, REPOSITORY_BASE_URL, &manifest).await.unwrap();

        assert_eq!(
            cache.get(VERSION_LABEL, "geometry/world.fgb").await.unwrap().as_deref(),
            Some(geometry_bytes),
        );
    }

    #[tokio::test]
    async fn put_live_files_rejects_a_file_whose_bytes_do_not_match_the_manifest_hash() {
        let geometry_bytes: &[u8] = b"geometry";
        let shard_bytes: &[u8] = b"shard";
        let manifest: Manifest = two_file_manifest(geometry_bytes, shard_bytes);
        let cache: MockArtifactCache = MockArtifactCache::new();
        let http_fetch: MockHttpFetch = MockHttpFetch::new(BTreeMap::from([
            (artifact_url("geometry/world.fgb"), b"tampered".to_vec()),
            (artifact_url("statistics/tfr.base.sqlite"), shard_bytes.to_vec()),
        ]));

        let error: AppErrorStatic = put_live_files(&cache, &http_fetch, REPOSITORY_BASE_URL, &manifest)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("sha256"));
        assert_eq!(cache.get(VERSION_LABEL, "geometry/world.fgb").await.unwrap(), None);
    }

    #[tokio::test]
    async fn resolve_repository_uses_the_speculative_manifest_when_discovery_agrees() {
        let static_base: &str = "/repository";
        let discovery_url: &str = "/discovery";
        let discovery_json: String = format!(
            r#"{{"schema_version": {}, "repository_base_url": "{static_base}", "minimum_client_version": "0.1.0", "sunset": null}}"#,
            artifact::DISCOVERY_SCHEMA_VERSION,
        );
        let http_fetch: MockHttpFetch = MockHttpFetch::new(BTreeMap::from([
            (discovery_url.to_string(), discovery_json.into_bytes()),
            (format!("{static_base}/{}", manifest::MANIFEST_LATEST_KEY), b"static manifest".to_vec()),
        ]));

        let resolved: ResolvedRepository = resolve_repository(&http_fetch, discovery_url, static_base)
            .await
            .unwrap();

        assert_eq!(resolved.base_url, static_base);
        assert_eq!(resolved.manifest_bytes, b"static manifest");
    }

    #[tokio::test]
    async fn resolve_repository_refetches_from_the_base_discovery_names() {
        let static_base: &str = "/repository";
        let discovery_url: &str = "/discovery";
        let discovery_json: String = format!(
            r#"{{"schema_version": {}, "repository_base_url": "{REPOSITORY_BASE_URL}", "minimum_client_version": "0.1.0", "sunset": null}}"#,
            artifact::DISCOVERY_SCHEMA_VERSION,
        );
        let http_fetch: MockHttpFetch = MockHttpFetch::new(BTreeMap::from([
            (discovery_url.to_string(), discovery_json.into_bytes()),
            (format!("{static_base}/{}", manifest::MANIFEST_LATEST_KEY), b"static manifest".to_vec()),
            (
                format!("{REPOSITORY_BASE_URL}/{}", manifest::MANIFEST_LATEST_KEY),
                b"discovered manifest".to_vec(),
            ),
        ]));

        let resolved: ResolvedRepository = resolve_repository(&http_fetch, discovery_url, static_base)
            .await
            .unwrap();

        assert_eq!(resolved.base_url, REPOSITORY_BASE_URL);
        assert_eq!(resolved.manifest_bytes, b"discovered manifest");
    }

    /// Discovery is advisory: an unreachable document leaves the static base in force rather than failing
    /// the load.
    #[tokio::test]
    async fn resolve_repository_falls_back_to_the_static_base_when_discovery_is_unreachable() {
        let static_base: &str = "/repository";
        let http_fetch: MockHttpFetch = MockHttpFetch::new(BTreeMap::from([(
            format!("{static_base}/{}", manifest::MANIFEST_LATEST_KEY),
            b"static manifest".to_vec(),
        )]));

        let resolved: ResolvedRepository = resolve_repository(&http_fetch, "/discovery", static_base)
            .await
            .unwrap();

        assert_eq!(resolved.base_url, static_base);
    }

    #[tokio::test]
    async fn version_labels_newest_first_ranks_an_unreadable_manifest_last() {
        let cache: MockArtifactCache = MockArtifactCache::new();
        let readable_label: &str = "2026-01-01+alpha";
        let damaged_label: &str = "2026-12-31+bravo";
        let manifest: Manifest = two_file_manifest(b"geometry", b"shard");
        cache
            .insert(readable_label, manifest::MANIFEST_FILENAME, serde_json::to_vec(&manifest).unwrap())
            .await;
        cache.insert(damaged_label, manifest::MANIFEST_FILENAME, b"not json".to_vec()).await;

        let ordered_version_labels: Vec<String> = version_labels_newest_first(&cache).await.unwrap();

        assert_eq!(ordered_version_labels, vec![readable_label.to_string(), damaged_label.to_string()]);
    }

    #[tokio::test]
    async fn evict_stale_versions_keeps_the_newest_by_artifact_created() {
        let cache: MockArtifactCache = MockArtifactCache::new();
        let version_labels_oldest_first: [&str; 3] = ["2026-07-01+zulu", "2026-07-02+alpha", "2026-07-03+bravo"];

        for (index, version_label) in version_labels_oldest_first.iter().enumerate() {
            let mut manifest: Manifest = two_file_manifest(b"geometry", b"shard");
            manifest.version = version_label.to_string();
            manifest.artifact_created = format!("2026-07-0{}T00:00:00Z", index + 1)
                .parse::<DateTime<Utc>>()
                .unwrap();

            cache
                .insert(version_label, manifest::MANIFEST_FILENAME, serde_json::to_vec(&manifest).unwrap())
                .await;
        }

        evict_stale_versions(&cache).await.unwrap();

        assert_eq!(cache.get("2026-07-01+zulu", manifest::MANIFEST_FILENAME).await.unwrap(), None);
        assert!(cache.get("2026-07-02+alpha", manifest::MANIFEST_FILENAME).await.unwrap().is_some());
        assert!(cache.get("2026-07-03+bravo", manifest::MANIFEST_FILENAME).await.unwrap().is_some());
    }
}

/* The iOS FFI exports the live load as an async function and UniFFI requires a Send future, so this
   pins the bound against the concrete implementations a non-browser client uses. */
#[cfg(all(test, not(target_arch = "wasm32")))]
mod send_bound {
    use super::*;
    use crate::artifact::FilesystemArtifactCache;
    use crate::http::ReqwestHttpFetch;

    fn assert_send<T: Send>(_value: T) {}

    #[test]
    fn the_live_load_future_is_send_for_the_native_instantiation() {
        let cache: FilesystemArtifactCache = FilesystemArtifactCache::create(std::path::PathBuf::from("/tmp/x"));
        let http_fetch: ReqwestHttpFetch = ReqwestHttpFetch::create().unwrap();

        assert_send(load_live_bundle(&cache, &http_fetch, "/discovery", "/repository", DistributionContext::FirstParty));
    }
}
