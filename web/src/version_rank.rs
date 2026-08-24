use chrono::{DateTime, Utc};

use shared::artifact::manifest::BundleVariant;
use shared::artifact::Manifest;

/// Sort key for a cached version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CachedVersionRank {
    is_complete: bool,
    artifact_created: Option<DateTime<Utc>>,
}

/// `None` for a version whose manifest could not be read, which ranks below every readable one so it is
/// opened last and evicted first.
pub fn rank_cached_version(manifest: Option<&Manifest>) -> CachedVersionRank {
    CachedVersionRank {
        is_complete: manifest.is_some_and(|manifest| manifest.variant == BundleVariant::Complete),
        artifact_created: manifest.map(|manifest| manifest.artifact_created),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use shared::artifact::manifest::{self, ManifestEntry};

    use super::*;

    fn manifest_of(variant: BundleVariant, artifact_created: &str) -> Manifest {
        Manifest {
            manifest_schema_version: manifest::MANIFEST_SCHEMA_VERSION,
            version: "2026-08-16+test".to_string(),
            variant,
            artifact_created: artifact_created.parse().unwrap(),
            geometry: ManifestEntry {
                relative_path: "geometry/world.fgb".to_string(),
                size_bytes: 1,
                sha256: "ab".repeat(32),
            },
            statistics: BTreeMap::new(),
            source_revisions: BTreeMap::new(),
            source_attribution: BTreeMap::new(),
            statistic_definitions: BTreeMap::new(),
        }
    }

    /// The case this ranking exists for: the onboard bundle is built after the last publish, so it is the
    /// newer artifact, and must still lose to the complete one.
    #[test]
    fn a_complete_bundle_outranks_a_newer_downsampled_one() {
        let complete: Manifest = manifest_of(BundleVariant::Complete, "2026-08-14T20:31:54Z");
        let newer_downsampled: Manifest = manifest_of(BundleVariant::Downsampled, "2026-08-16T05:16:20Z");

        assert!(rank_cached_version(Some(&complete)) > rank_cached_version(Some(&newer_downsampled)));
    }

    #[test]
    fn the_newer_artifact_wins_within_one_variant() {
        let older: Manifest = manifest_of(BundleVariant::Complete, "2026-08-14T20:31:54Z");
        let newer: Manifest = manifest_of(BundleVariant::Complete, "2026-08-16T05:16:20Z");

        assert!(rank_cached_version(Some(&newer)) > rank_cached_version(Some(&older)));
    }

    #[test]
    fn an_unreadable_manifest_ranks_below_every_readable_one() {
        let downsampled: Manifest = manifest_of(BundleVariant::Downsampled, "2020-01-01T00:00:00Z");

        assert!(rank_cached_version(Some(&downsampled)) > rank_cached_version(None));
    }

    /// A bundle published before the manifest carried a variant reads as complete, so an existing cached
    /// complete bundle keeps outranking the onboard one rather than being demoted by the upgrade.
    #[test]
    fn a_manifest_without_a_variant_ranks_as_complete() {
        let json_without_variant: String = format!(
            r#"{{
                "manifest_schema_version": 1,
                "version": "2026-08-14+macdiarmid",
                "artifact_created": "2026-08-14T20:31:54Z",
                "geometry": {{ "relative_path": "geometry/world.fgb", "size_bytes": 1, "sha256": "{}" }},
                "statistics": {{}},
                "source_revisions": {{}}
            }}"#,
            "ab".repeat(32),
        );

        let without_variant: Manifest = manifest::parse_manifest(json_without_variant.as_bytes()).unwrap();

        assert_eq!(without_variant.variant, BundleVariant::Complete);

        let newer_downsampled: Manifest = manifest_of(BundleVariant::Downsampled, "2026-08-16T05:16:20Z");
        assert!(rank_cached_version(Some(&without_variant)) > rank_cached_version(Some(&newer_downsampled)));
    }
}
