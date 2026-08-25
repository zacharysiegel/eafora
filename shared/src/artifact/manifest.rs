use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::artifact::schema_version;
use crate::canonical::canonical_model::{
    impl_code_serde, DataSourceKind, LicenseShardClass, SourceAttribution, SourceRevision, StatisticDefinition,
    StatisticKind,
};
use crate::error::AppError;

pub const MANIFEST_FILENAME: &str = "manifest.json";
pub const MANIFEST_SCHEMA_VERSION: u32 = 2;
pub const SUBDIR_GEOMETRY: &str = "geometry";
pub const SUBDIR_DATA: &str = "data";

/// Stable-pointer key on the destination. The producer uploads a byte-for-byte
/// copy of the just-published manifest here; the consumer fetches
/// `<repository_base_url>/<MANIFEST_LATEST_KEY>` at startup.
pub const MANIFEST_LATEST_KEY: &str = "latest/manifest.json";

/// Which resolution a bundle carries. `Complete` has every period and every authorized source and is what
/// the CDN serves; `Downsampled` collapses to the reference year and is the onboard bundle clients embed
/// for first paint. A consumer holding both must never prefer the downsampled one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleVariant {
    Complete,
    Downsampled,
}

impl BundleVariant {
    pub fn code(self) -> &'static str {
        match self {
            BundleVariant::Complete => "complete",
            BundleVariant::Downsampled => "downsampled",
        }
    }
}

impl TryFrom<&str> for BundleVariant {
    type Error = AppError;

    fn try_from(code: &str) -> Result<BundleVariant, AppError> {
        match code {
            "complete" => Ok(BundleVariant::Complete),
            "downsampled" => Ok(BundleVariant::Downsampled),
            other => Err(AppError::from(format!("unknown bundle variant {other:?}"))),
        }
    }
}

impl_code_serde!(BundleVariant, code);


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_schema_version: u32,
    pub version: String,
    pub variant: BundleVariant,
    pub artifact_created: DateTime<Utc>,
    pub geometry: ManifestEntry,
    pub statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>>,
    pub source_revisions: BTreeMap<DataSourceKind, SourceRevision>,
    pub source_attribution: BTreeMap<DataSourceKind, SourceAttribution>,
    pub statistic_definitions: BTreeMap<StatisticKind, StatisticDefinition>,
}

impl Manifest {
    pub fn file_entries(&self) -> impl Iterator<Item = &ManifestEntry> {
        std::iter::once(&self.geometry)
            .chain(self.statistics.values().flat_map(|by_license_class| by_license_class.values()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Peeks only the version field so a shape change in a future schema version is
/// rejected with a clear message before the full (possibly incompatible) parse.
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError> {
    schema_version::require_schema_version(bytes, "manifest_schema_version", MANIFEST_SCHEMA_VERSION)?;

    let manifest: Manifest = serde_json::from_slice(bytes)?;

    validate_entry(&manifest.geometry)?;
    for license_shard_map in manifest.statistics.values() {
        for entry in license_shard_map.values() {
            validate_entry(entry)?;
        }
    }

    Ok(manifest)
}

fn validate_entry(entry: &ManifestEntry) -> Result<(), AppError> {
    let is_hex_64: bool = entry.sha256.len() == 64 && entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !is_hex_64 {
        return Err(AppError::from(format!(
            "manifest entry {:?} has malformed sha256 {:?}",
            entry.relative_path, entry.sha256,
        )));
    }

    if entry.relative_path.contains("..") || entry.relative_path.starts_with('/') {
        return Err(AppError::from(format!(
            "manifest entry has unsafe relative_path {:?}",
            entry.relative_path,
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_sha256() -> String {
        "ab12cd34".repeat(8)
    }

    fn valid_manifest_json() -> String {
        format!(
            r#"{{
  "manifest_schema_version": 2,
  "version": "2026-05-18+laureate",
  "variant": "complete",
  "artifact_created": "2026-05-18T03:00:00Z",
  "geometry": {{ "relative_path": "geometry/world-50m-{sha}.fgb", "size_bytes": 4380000, "sha256": "{sha}" }},
  "statistics": {{ "tfr": {{ "base": {{ "relative_path": "data/tfr-base-{sha}.sqlite", "size_bytes": 89000, "sha256": "{sha}" }} }} }},
  "source_revisions": {{ "wb_wdi": {{ "revision": "2024-12-12", "published": "2024-12-12T00:00:00Z", "fetched": "2024-12-31T00:00:00Z" }} }},
  "source_attribution": {{}},
  "statistic_definitions": {{}}
}}"#,
            sha = valid_sha256(),
        )
    }

    #[test]
    fn parse_manifest_round_trips_fixture_set() {
        let manifest: Manifest = parse_manifest(valid_manifest_json().as_bytes()).unwrap();

        assert_eq!(manifest.manifest_schema_version, 2);
        assert_eq!(manifest.version, "2026-05-18+laureate");
        assert!(manifest.statistics.contains_key(&StatisticKind::Tfr));
        assert_eq!(manifest.source_revisions[&DataSourceKind::WorldBankWDI].revision, "2024-12-12");

        let produced: String = serde_json::to_string_pretty(&manifest).unwrap();
        let reparsed: Manifest = parse_manifest(produced.as_bytes()).unwrap();
        let reproduced: String = serde_json::to_string_pretty(&reparsed).unwrap();
        assert_eq!(produced, reproduced);
    }

    #[test]
    fn parse_manifest_reads_the_prose_fields_when_present() {
        let json: String = valid_manifest_json()
            .replace(
                r#""source_attribution": {}"#,
                r#""source_attribution": { "wb_wdi": {
                    "attribution_text": "World Bank, World Development Indicators (CC BY 4.0)",
                    "license_name": "CC BY 4.0",
                    "license_url": "https://creativecommons.org/licenses/by/4.0/",
                    "homepage_url": "https://databank.worldbank.org/source/world-development-indicators"
                } }"#,
            )
            .replace(
                r#""statistic_definitions": {}"#,
                r#""statistic_definitions": { "tfr": { "description": "Average number of children." } }"#,
            );

        let manifest: Manifest = parse_manifest(json.as_bytes()).unwrap();

        assert_eq!(manifest.source_attribution[&DataSourceKind::WorldBankWDI].license_name, "CC BY 4.0");
        assert_eq!(
            manifest.statistic_definitions[&StatisticKind::Tfr].description,
            "Average number of children.",
        );
    }

    #[test]
    fn parse_manifest_rejects_unknown_schema_version() {
        let unknown_version: u32 = MANIFEST_SCHEMA_VERSION + 1;
        let json: String = valid_manifest_json().replace(
            &format!("\"manifest_schema_version\": {MANIFEST_SCHEMA_VERSION}"),
            &format!("\"manifest_schema_version\": {unknown_version}"),
        );

        let error: AppError = parse_manifest(json.as_bytes()).unwrap_err();

        assert!(error.to_string().contains(&format!("unknown manifest_schema_version {unknown_version}")));
    }

    #[test]
    fn parse_manifest_rejects_unknown_statistic_code() {
        let json: String = valid_manifest_json().replace("\"tfr\"", "\"bogus_statistic\"");

        assert!(parse_manifest(json.as_bytes()).is_err());
    }

    #[test]
    fn parse_manifest_rejects_malformed_sha256() {
        let json: String = valid_manifest_json().replace(&format!("\"sha256\": \"{}\"", valid_sha256()), "\"sha256\": \"not-hex\"");

        assert!(parse_manifest(json.as_bytes()).is_err());
    }

    #[test]
    fn parse_manifest_rejects_path_traversal_relative_path() {
        let json: String = valid_manifest_json().replace("geometry/world-50m", "../world-50m");

        assert!(parse_manifest(json.as_bytes()).is_err());
    }

    #[test]
    fn parse_manifest_ignores_unknown_fields() {
        let json: String = valid_manifest_json().replace(
            "\"manifest_schema_version\": 1,",
            "\"manifest_schema_version\": 1,\n  \"field_added_in_a_future_v1_revision\": \"ignored\",",
        );

        let manifest: Manifest = parse_manifest(json.as_bytes()).unwrap();

        assert_eq!(manifest.version, "2026-05-18+laureate");
    }
}
