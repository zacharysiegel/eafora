use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::canonical::canonical_model::{DataSourceKind, LicenseShardClass, SourceRevision, StatisticKind};
use crate::error::AppError;

pub const MANIFEST_FILENAME: &str = "manifest.json";
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const SUBDIR_GEOMETRY: &str = "geometry";
pub const SUBDIR_DATA: &str = "data";

/// Stable-pointer key on the destination. The producer uploads a byte-for-byte
/// copy of the just-published manifest here; the consumer fetches
/// `<repository_base_url>/<MANIFEST_LATEST_KEY>` at startup.
pub const MANIFEST_LATEST_KEY: &str = "latest/manifest.json";

pub const CONTENT_TYPE_MANIFEST: &str = "application/json";
pub const CONTENT_TYPE_FLATGEOBUF: &str = "application/octet-stream";
pub const CONTENT_TYPE_SQLITE: &str = "application/vnd.sqlite3";

/// Short-cached so re-platforms propagate within minutes.
pub const CACHE_CONTROL_MANIFEST: &str = "public, max-age=300";
/// Immutable: shard filenames are content-addressed, so a shard's bytes never change.
pub const CACHE_CONTROL_SHARD: &str = "public, max-age=31536000, immutable";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_schema_version: u32,
    pub version: String,
    pub artifact_created: DateTime<Utc>,
    pub geometry: ManifestEntry,
    pub statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>>,
    pub source_revisions: BTreeMap<DataSourceKind, SourceRevision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Peeks only the version field so a shape change in a future schema version is
/// rejected with a clear message before the full (possibly incompatible) parse.
#[derive(Deserialize)]
struct ManifestSchemaProbe {
    manifest_schema_version: u32,
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError> {
    let probe: ManifestSchemaProbe = serde_json::from_slice(bytes)?;
    if probe.manifest_schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(AppError::from(format!(
            "unknown manifest_schema_version {}",
            probe.manifest_schema_version,
        )));
    }

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
  "manifest_schema_version": 1,
  "version": "2026-05-18+laureate",
  "artifact_created": "2026-05-18T03:00:00Z",
  "geometry": {{ "relative_path": "geometry/world-50m-{sha}.fgb", "size_bytes": 4380000, "sha256": "{sha}" }},
  "statistics": {{ "tfr": {{ "base": {{ "relative_path": "data/tfr-base-{sha}.sqlite", "size_bytes": 89000, "sha256": "{sha}" }} }} }},
  "source_revisions": {{ "wb_wdi": {{ "revision": "2024-12-12", "published": "2024-12-12T00:00:00Z", "fetched": "2024-12-31T00:00:00Z" }} }}
}}"#,
            sha = valid_sha256(),
        )
    }

    #[test]
    fn parse_manifest_round_trips_fixture_set() {
        let manifest: Manifest = parse_manifest(valid_manifest_json().as_bytes()).unwrap();

        assert_eq!(manifest.manifest_schema_version, 1);
        assert_eq!(manifest.version, "2026-05-18+laureate");
        assert!(manifest.statistics.contains_key(&StatisticKind::Tfr));
        assert_eq!(manifest.source_revisions[&DataSourceKind::WorldBankWDI].revision, "2024-12-12");

        let produced: String = serde_json::to_string_pretty(&manifest).unwrap();
        let reparsed: Manifest = parse_manifest(produced.as_bytes()).unwrap();
        let reproduced: String = serde_json::to_string_pretty(&reparsed).unwrap();
        assert_eq!(produced, reproduced);
    }

    #[test]
    fn parse_manifest_rejects_unknown_schema_version() {
        let json: String = valid_manifest_json().replace("\"manifest_schema_version\": 1", "\"manifest_schema_version\": 2");

        let error: AppError = parse_manifest(json.as_bytes()).unwrap_err();

        assert!(error.to_string().contains("unknown manifest_schema_version 2"));
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
}
