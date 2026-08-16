use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::artifact::cache::ArtifactCache;
use crate::artifact::geometry::{self, GeometryLayer};
use crate::artifact::manifest::{self, Manifest};
use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};
use crate::error::AppError;
use crate::filesystem;
use crate::license::DistributionContext;
use crate::sqlite::shard_db::{self, ShardValues};

/// Content-Type the producer sets when uploading each artifact-bundle file kind; the CDN edge,
/// the browser HTTP cache, and Accept-header negotiation depend on them.
pub const CONTENT_TYPE_MANIFEST: &str = "application/json";
pub const CONTENT_TYPE_FLATGEOBUF: &str = "application/octet-stream";
pub const CONTENT_TYPE_SQLITE: &str = "application/vnd.sqlite3";

/// Cache-Control the producer sets per file kind. Manifest is short-cached so re-platforms
/// propagate within minutes.
pub const CACHE_CONTROL_MANIFEST: &str = "public, max-age=300";
/// Immutable: shard filenames are content-addressed, so a shard's bytes never change.
pub const CACHE_CONTROL_SHARD: &str = "public, max-age=31536000, immutable";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StatisticShardKey {
    pub statistic_kind: StatisticKind,
    pub license_shard_class: LicenseShardClass,
}

/// A fully-loaded artifact bundle: pure parsed data, `Send + Sync`, holding no SQLite connection, so
/// `Arc<Bundle>` crosses the hot-swap watch channel cleanly.
pub struct Bundle {
    pub manifest: Manifest,
    pub geometry: GeometryLayer,
    /// Every license shard this `distribution_context` is authorized to access (unauthorized shards
    /// are never read). Private so no consumer can bypass `shard_values_for`'s license precedence.
    shard_values: BTreeMap<StatisticShardKey, ShardValues>,
    pub distribution_context: DistributionContext,
}

impl Bundle {
    pub async fn open<C: ArtifactCache>(
        cache: &C,
        version_label: &str,
        distribution_context: DistributionContext,
    ) -> Result<Bundle, AppError> {
        let manifest_bytes: Vec<u8> = get_required(cache, version_label, manifest::MANIFEST_FILENAME).await?;
        let manifest: Manifest = manifest::parse_manifest(&manifest_bytes)?;

        let geometry_bytes: Vec<u8> = get_required(cache, version_label, &manifest.geometry.relative_path).await?;
        filesystem::verify_sha256(&geometry_bytes, &manifest.geometry.sha256)?;
        let geometry: GeometryLayer = geometry::parse_geometry_layer(geometry_bytes)?;

        let mut shard_values: BTreeMap<StatisticShardKey, ShardValues> = BTreeMap::new();
        let authorized_classes: &[LicenseShardClass] = distribution_context.authorized_classes();
        for (statistic_kind, license_shard_map) in &manifest.statistics {
            for (license_shard_class, entry) in license_shard_map {
                if !authorized_classes.contains(license_shard_class) {
                    continue;
                }

                let shard_bytes: Vec<u8> = get_required(cache, version_label, &entry.relative_path).await?;
                filesystem::verify_sha256(&shard_bytes, &entry.sha256)?;

                let statistic_shard_values: ShardValues = shard_db::read_shard(&shard_bytes)?;

                let key: StatisticShardKey = StatisticShardKey {
                    statistic_kind: *statistic_kind,
                    license_shard_class: *license_shard_class,
                };
                shard_values.insert(key, statistic_shard_values);
            }
        }

        Ok(Bundle {
            manifest,
            geometry,
            shard_values,
            distribution_context,
        })
    }

    /// The values that color the map for `statistic_kind`: the first authorized license class that
    /// ships a shard for it. Provisional policy shared by the renderer and the selection resolver;
    /// refining it to the source-choice rules is future work.
    pub fn shard_values_for(&self, statistic_kind: StatisticKind) -> Option<&ShardValues> {
        self.distribution_context
            .authorized_classes()
            .iter()
            .find_map(|license_shard_class| {
                self.shard_values.get(&StatisticShardKey {
                    statistic_kind,
                    license_shard_class: *license_shard_class,
                })
            })
    }
}

async fn get_required(cache: &impl ArtifactCache, version_label: &str, relative_path: &str) -> Result<Vec<u8>, AppError> {
    let bytes: Option<Vec<u8>> = cache.get(version_label, relative_path).await?;

    bytes.ok_or_else(|| {
        AppError::from(format!("bundle: {:?} missing from cache for version {:?}", relative_path, version_label))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;

    use crate::artifact::cache::tests::MockArtifactCache;
    use crate::artifact::geometry::tests::one_feature_fgb_bytes;
    use crate::artifact::manifest::ManifestEntry;

    const VERSION: &str = "2026-05-18+test";
    const GEOMETRY_PATH: &str = "geometry/world.fgb";
    const BASE_SHARD_PATH: &str = "data/tfr-base.sqlite";
    const NONCOMMERCIAL_SHARD_PATH: &str = "data/tfr-noncommercial.sqlite";

    /// The committed shard the native `shard_db::tests::dump_sample_shard` produced. Read as bytes
    /// rather than rebuilt, because building one needs rusqlite and these tests also compile for
    /// wasm32.
    fn sample_shard_bytes() -> Vec<u8> {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/tfr-sample.sqlite")).to_vec()
    }

    fn entry(relative_path: &str, bytes: &[u8]) -> ManifestEntry {
        ManifestEntry {
            relative_path: relative_path.to_string(),
            size_bytes: bytes.len() as u64,
            sha256: filesystem::sha256_hex(bytes),
        }
    }

    fn tfr_statistic(base: &ManifestEntry, noncommercial: &ManifestEntry) -> BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>> {
        let mut license_map: BTreeMap<LicenseShardClass, ManifestEntry> = BTreeMap::new();
        license_map.insert(LicenseShardClass::Base, base.clone());
        license_map.insert(LicenseShardClass::NonCommercial, noncommercial.clone());

        let mut statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>> = BTreeMap::new();
        statistics.insert(StatisticKind::Tfr, license_map);
        statistics
    }

    fn build_manifest(geometry: ManifestEntry, statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>>) -> Manifest {
        Manifest {
            manifest_schema_version: manifest::MANIFEST_SCHEMA_VERSION,
            version: VERSION.to_string(),
            artifact_created: "2026-05-18T03:00:00Z".parse().unwrap(),
            geometry,
            statistics,
            source_revisions: BTreeMap::new(),
        }
    }

    /// Seed a mock cache with a valid manifest + geometry + Base and NonCommercial Tfr shards.
    async fn seeded_mock() -> MockArtifactCache {
        let geometry_bytes: Vec<u8> = one_feature_fgb_bytes();
        let base_shard: Vec<u8> = sample_shard_bytes();
        let noncommercial_shard: Vec<u8> = sample_shard_bytes();

        let statistics = tfr_statistic(&entry(BASE_SHARD_PATH, &base_shard), &entry(NONCOMMERCIAL_SHARD_PATH, &noncommercial_shard));
        let manifest: Manifest = build_manifest(entry(GEOMETRY_PATH, &geometry_bytes), statistics);
        let manifest_bytes: Vec<u8> = serde_json::to_vec(&manifest).unwrap();

        let cache: MockArtifactCache = MockArtifactCache::new();
        cache.insert(VERSION, manifest::MANIFEST_FILENAME, manifest_bytes).await;
        cache.insert(VERSION, GEOMETRY_PATH, geometry_bytes).await;
        cache.insert(VERSION, BASE_SHARD_PATH, base_shard).await;
        cache.insert(VERSION, NONCOMMERCIAL_SHARD_PATH, noncommercial_shard).await;

        cache
    }

    #[tokio::test]
    async fn bundle_open_round_trip_against_mock_cache() {
        let cache: MockArtifactCache = seeded_mock().await;

        let bundle: Bundle = Bundle::open(&cache, VERSION, DistributionContext::FirstParty).await.unwrap();

        assert_eq!(bundle.manifest.version, VERSION);
        assert_eq!(bundle.shard_values.len(), 2);
        assert!(bundle.shard_values.contains_key(&StatisticShardKey { statistic_kind: StatisticKind::Tfr, license_shard_class: LicenseShardClass::Base }));
        assert!(bundle.shard_values.contains_key(&StatisticShardKey { statistic_kind: StatisticKind::Tfr, license_shard_class: LicenseShardClass::NonCommercial }));
    }

    #[tokio::test]
    async fn bundle_open_eagerly_parses_shards() {
        let cache: MockArtifactCache = seeded_mock().await;

        let bundle: Bundle = Bundle::open(&cache, VERSION, DistributionContext::FirstParty).await.unwrap();

        let shard_values: &ShardValues = bundle.shard_values_for(StatisticKind::Tfr).unwrap();
        assert_eq!(shard_values.value("usa", NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()), Some(1.6));
        assert_eq!(shard_values.value_range(), Some((1.5, 1.7)));
    }

    #[tokio::test]
    async fn bundle_open_rejects_an_unparseable_shard() {
        let geometry_bytes: Vec<u8> = one_feature_fgb_bytes();
        let base_shard: Vec<u8> = b"not a sqlite database".to_vec();

        let statistics = tfr_statistic(&entry(BASE_SHARD_PATH, &base_shard), &entry(NONCOMMERCIAL_SHARD_PATH, &base_shard));
        let manifest: Manifest = build_manifest(entry(GEOMETRY_PATH, &geometry_bytes), statistics);

        let cache: MockArtifactCache = MockArtifactCache::new();
        cache.insert(VERSION, manifest::MANIFEST_FILENAME, serde_json::to_vec(&manifest).unwrap()).await;
        cache.insert(VERSION, GEOMETRY_PATH, geometry_bytes).await;
        cache.insert(VERSION, BASE_SHARD_PATH, base_shard.clone()).await;
        cache.insert(VERSION, NONCOMMERCIAL_SHARD_PATH, base_shard).await;

        let result: Result<Bundle, AppError> = Bundle::open(&cache, VERSION, DistributionContext::FirstParty).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn bundle_open_eagerly_parses_geometry() {
        let cache: MockArtifactCache = seeded_mock().await;

        let bundle: Bundle = Bundle::open(&cache, VERSION, DistributionContext::FirstParty).await.unwrap();

        let features = bundle.geometry.iter_features().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].region_code, "testland");
    }

    #[tokio::test]
    async fn bundle_open_skips_unauthorized_shards() {
        let cache: MockArtifactCache = seeded_mock().await;

        let bundle: Bundle = Bundle::open(&cache, VERSION, DistributionContext::ThirdParty).await.unwrap();

        assert_eq!(bundle.shard_values.len(), 1);
        assert!(bundle.shard_values.contains_key(&StatisticShardKey { statistic_kind: StatisticKind::Tfr, license_shard_class: LicenseShardClass::Base }));
    }

    #[tokio::test]
    async fn shard_values_for_returns_the_first_authorized_shard() {
        let cache: MockArtifactCache = seeded_mock().await;

        let bundle: Bundle = Bundle::open(&cache, VERSION, DistributionContext::FirstParty).await.unwrap();

        let base_key: StatisticShardKey = StatisticShardKey {
            statistic_kind: StatisticKind::Tfr,
            license_shard_class: LicenseShardClass::Base,
        };
        let selected: &ShardValues = bundle.shard_values_for(StatisticKind::Tfr).unwrap();

        assert!(std::ptr::eq(selected, bundle.shard_values.get(&base_key).unwrap()));
    }

    #[tokio::test]
    async fn bundle_open_rejects_missing_manifest() {
        let cache: MockArtifactCache = MockArtifactCache::new();

        let result: Result<Bundle, AppError> = Bundle::open(&cache, VERSION, DistributionContext::FirstParty).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn bundle_open_rejects_sha256_mismatch() {
        let geometry_bytes: Vec<u8> = one_feature_fgb_bytes();
        let base_shard: Vec<u8> = b"real-bytes".to_vec();

        let mismatched: ManifestEntry = ManifestEntry {
            relative_path: BASE_SHARD_PATH.to_string(),
            size_bytes: base_shard.len() as u64,
            sha256: "00".repeat(32),
        };
        let mut license_map: BTreeMap<LicenseShardClass, ManifestEntry> = BTreeMap::new();
        license_map.insert(LicenseShardClass::Base, mismatched);
        let mut statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>> = BTreeMap::new();
        statistics.insert(StatisticKind::Tfr, license_map);

        let manifest: Manifest = build_manifest(entry(GEOMETRY_PATH, &geometry_bytes), statistics);
        let cache: MockArtifactCache = MockArtifactCache::new();
        cache.insert(VERSION, manifest::MANIFEST_FILENAME, serde_json::to_vec(&manifest).unwrap()).await;
        cache.insert(VERSION, GEOMETRY_PATH, geometry_bytes).await;
        cache.insert(VERSION, BASE_SHARD_PATH, base_shard).await;

        let result: Result<Bundle, AppError> = Bundle::open(&cache, VERSION, DistributionContext::FirstParty).await;

        assert!(result.is_err());
    }

    #[test]
    fn bundle_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<std::sync::Arc<Bundle>>();
    }
}
