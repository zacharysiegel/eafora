//! Serialization order is deterministic (BTreeMap on every map) so two
//! identical inputs produce byte-identical manifest.json files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::artifact::artifact_model::{HashedOutputs, HashedShard, LicenseShardClass, ShardOutput};
use crate::canonical::canonical_model::DataSourceKind;
use crate::error::AppError;

const MANIFEST_FILENAME: &str = "manifest.json";

#[derive(Debug, Serialize)]
struct ManifestSerializer<'a> {
    version: &'a str,
    artifact_created: String,
    geometry: ManifestEntry<'a>,
    statistics: BTreeMap<String, BTreeMap<String, ManifestEntry<'a>>>,
    source_versions: BTreeMap<&'a str, &'a str>,
}

#[derive(Debug, Serialize)]
struct ManifestEntry<'a> {
    url: String,
    size_bytes: u64,
    sha256: &'a str,
}

pub struct ManifestEmission {
    pub output: ShardOutput,
    pub sha256_hex: String,
}

pub fn emit_manifest(
    hashed: &HashedOutputs,
    version_label: &str,
    data_source_versions: &BTreeMap<DataSourceKind, String>,
    output_dir: &Path,
) -> Result<ManifestEmission, AppError> {
    let artifact_created: DateTime<Utc> = Utc::now();
    let json: String = build_manifest_json(hashed, version_label, &artifact_created, data_source_versions)?;

    let path: PathBuf = output_dir.join(MANIFEST_FILENAME);
    fs::write(&path, &json)?;

    let mut hasher: Sha256 = Sha256::new();
    hasher.update(json.as_bytes());
    let sha256_hex: String = hex_encode(&Into::<[u8; 32]>::into(hasher.finalize()));

    let byte_count: u64 = json.as_bytes().len() as u64;

    Ok(ManifestEmission {
        output: ShardOutput { path, byte_count },
        sha256_hex,
    })
}

fn build_manifest_json(
    hashed: &HashedOutputs,
    version_label: &str,
    artifact_created: &DateTime<Utc>,
    data_source_versions: &BTreeMap<DataSourceKind, String>,
) -> Result<String, AppError> {
    let geometry: ManifestEntry<'_> = ManifestEntry {
        url: relative_url(&hashed.geometry_shard, "geometry")?,
        size_bytes: hashed.geometry_shard.byte_count,
        sha256: &hashed.geometry_shard.sha256_hex,
    };

    let mut statistics: BTreeMap<String, BTreeMap<String, ManifestEntry<'_>>> = BTreeMap::new();
    for statistic_shard in &hashed.statistic_shards {
        let entry: ManifestEntry<'_> = ManifestEntry {
            url: relative_url(&statistic_shard.shard, "data")?,
            size_bytes: statistic_shard.shard.byte_count,
            sha256: &statistic_shard.shard.sha256_hex,
        };
        statistics
            .entry(statistic_shard.statistic_code.clone())
            .or_default()
            .insert(license_label(statistic_shard.license_shard_class).to_string(), entry);
    }

    let source_versions: BTreeMap<&str, &str> = data_source_versions
        .iter()
        .map(|(kind, revision)| (kind.code(), revision.as_str()))
        .collect();

    let manifest: ManifestSerializer<'_> = ManifestSerializer {
        version: version_label,
        artifact_created: artifact_created.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        geometry,
        statistics,
        source_versions,
    };

    let json: String = serde_json::to_string_pretty(&manifest)?;
    Ok(json)
}

fn relative_url(shard: &HashedShard, subdir: &str) -> Result<String, AppError> {
    let filename: &str = shard
        .path
        .file_name()
        .and_then(|os| os.to_str())
        .ok_or_else(|| AppError::from(format!("emit_manifest: bad path {:?}", shard.path)))?;
    Ok(format!("{}/{}", subdir, filename))
}

fn license_label(license_shard_class: LicenseShardClass) -> &'static str {
    license_shard_class.as_str()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut hex_string: String = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex_string.push_str(&format!("{:02x}", byte));
    }
    hex_string
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::artifact::artifact_model::HashedStatisticShard;

    fn make_hashed_outputs() -> HashedOutputs {
        HashedOutputs {
            statistic_shards: vec![
                HashedStatisticShard {
                    statistic_code: "tfr".to_string(),
                    license_shard_class: LicenseShardClass::Base,
                    shard: HashedShard {
                        path: PathBuf::from("/tmp/eafora/data/tfr-base-ef561234.sqlite"),
                        byte_count: 89000,
                        sha256_hex: "ef561234".repeat(8),
                    },
                },
                HashedStatisticShard {
                    statistic_code: "tfr".to_string(),
                    license_shard_class: LicenseShardClass::NonCommercial,
                    shard: HashedShard {
                        path: PathBuf::from("/tmp/eafora/data/tfr-noncommercial-78ab9012.sqlite"),
                        byte_count: 4200,
                        sha256_hex: "78ab9012".repeat(8),
                    },
                },
                HashedStatisticShard {
                    statistic_code: "cbr".to_string(),
                    license_shard_class: LicenseShardClass::Base,
                    shard: HashedShard {
                        path: PathBuf::from("/tmp/eafora/data/cbr-base-cccc1111.sqlite"),
                        byte_count: 50000,
                        sha256_hex: "cccc1111".repeat(8),
                    },
                },
            ],
            geometry_shard: HashedShard {
                path: PathBuf::from("/tmp/eafora/geometry/world-50m-ab12cd34.fgb"),
                byte_count: 4380000,
                sha256_hex: "ab12cd34".repeat(8),
            },
        }
    }

    #[test]
    fn build_manifest_json_sorts_statistics_alphabetically() {
        let hashed: HashedOutputs = make_hashed_outputs();
        let data_source_versions: BTreeMap<DataSourceKind, String> = BTreeMap::from([
            (DataSourceKind::WorldBankWDI, "2024-Q4".to_string()),
        ]);
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_versions).unwrap();

        let cbr_position: usize = json.find("\"cbr\"").expect("cbr present");
        let tfr_position: usize = json.find("\"tfr\"").expect("tfr present");
        assert!(cbr_position < tfr_position);
    }

    #[test]
    fn build_manifest_json_sorts_license_classes_alphabetically_within_statistic() {
        let hashed: HashedOutputs = make_hashed_outputs();
        let data_source_versions: BTreeMap<DataSourceKind, String> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_versions).unwrap();

        let base_position: usize = json.find("\"base\"").expect("base present");
        let noncommercial_position: usize = json.find("\"noncommercial\"").expect("noncommercial present");
        assert!(base_position < noncommercial_position);
    }

    #[test]
    fn build_manifest_json_emits_relative_urls_under_geometry_and_data() {
        let hashed: HashedOutputs = make_hashed_outputs();
        let data_source_versions: BTreeMap<DataSourceKind, String> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_versions).unwrap();

        assert!(json.contains("\"url\": \"geometry/world-50m-ab12cd34.fgb\""));
        assert!(json.contains("\"url\": \"data/tfr-base-ef561234.sqlite\""));
        assert!(json.contains("\"url\": \"data/cbr-base-cccc1111.sqlite\""));
    }

    #[test]
    fn build_manifest_json_is_deterministic_byte_for_byte() {
        let hashed: HashedOutputs = make_hashed_outputs();
        let data_source_versions: BTreeMap<DataSourceKind, String> = BTreeMap::from([
            (DataSourceKind::WorldBankWDI, "2024-Q4".to_string()),
            (DataSourceKind::WorldBankWDI, "2026-w20".to_string()),
        ]);
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json_one: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_versions).unwrap();
        let json_two: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_versions).unwrap();

        assert_eq!(json_one, json_two);
    }

    #[test]
    fn emit_manifest_writes_file_and_returns_consistent_sha256() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let hashed: HashedOutputs = make_hashed_outputs();
        let data_source_versions: BTreeMap<DataSourceKind, String> = BTreeMap::new();

        let emission: ManifestEmission =
            emit_manifest(&hashed, "2026-05-18", &data_source_versions, temp_dir.path()).unwrap();

        assert!(emission.output.path.exists());
        let bytes_on_disk: Vec<u8> = fs::read(&emission.output.path).unwrap();
        let mut hasher: Sha256 = Sha256::new();
        hasher.update(&bytes_on_disk);
        let computed: String = hex_encode(&Into::<[u8; 32]>::into(hasher.finalize()));
        assert_eq!(computed, emission.sha256_hex);
    }
}
