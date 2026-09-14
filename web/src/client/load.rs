use shared::artifact::{load, Bundle};
use shared::license::DistributionContext;
use shared::AppError;

use crate::client::cache::OpfsArtifactCache;
use crate::client::fetch::BrowserFetch;
use crate::live_resolve;

pub use shared::artifact::load::{evict_stale_versions, open_newest_cached_bundle};

const EMBEDDED_BASE_URL: &str = "/embedded_artifacts";

pub async fn load_embedded_bundle(
    cache: &OpfsArtifactCache,
    distribution_context: DistributionContext,
) -> Result<Bundle, AppError> {
    load::load_embedded_bundle(cache, &BrowserFetch, EMBEDDED_BASE_URL, distribution_context)
        .await
        .map_err(AppError::from)
}

pub async fn load_live_bundle(
    cache: &OpfsArtifactCache,
    static_base: &str,
    distribution_context: DistributionContext,
) -> Result<Bundle, AppError> {
    load::load_live_bundle(
        cache,
        &BrowserFetch,
        live_resolve::DISCOVERY_PATH,
        static_base,
        distribution_context,
    )
    .await
    .map_err(AppError::from)
}

/// Exercised in the browser: the ordering these cover is a property of real OPFS reads, since the
/// version list and each manifest come back through `FileSystemDirectoryHandle`.
#[cfg(test)]
mod tests {
    use shared::artifact::{manifest, ArtifactCache};

    use super::*;

    /// A parseable manifest carrying no files, so a test can seed a version whose only meaningful
    /// content is its `artifact_created`.
    fn manifest_json(version_label: &str, artifact_created: &str) -> Vec<u8> {
        format!(
            r#"{{
                "manifest_schema_version": {schema_version},
                "version": "{version_label}",
                "artifact_created": "{artifact_created}",
                "geometry": {{
                    "relative_path": "geometry/world.fgb",
                    "size_bytes": 1,
                    "sha256": "{sha256}"
                }},
                "statistics": {{}},
                "source_revisions": {{}}
            }}"#,
            schema_version = manifest::MANIFEST_SCHEMA_VERSION,
            sha256 = "ab".repeat(32),
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

        let ordered_version_labels: Vec<String> = load::version_labels_newest_first(&cache).await.unwrap();

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
