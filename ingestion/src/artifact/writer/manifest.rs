//! Serialization order is deterministic (BTreeMap on every map) so two
//! identical inputs produce byte-identical manifest.json files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::artifact::artifact_model::{FileReference, Hashed, HashedArtifacts};
use crate::canonical::canonical_model::{DataSourceKind, SourceRevision};
use crate::error::AppError;

const MANIFEST_FILENAME: &str = "manifest.json";

#[derive(Debug, Serialize)]
struct ManifestSerializer<'a> {
    version: &'a str,
    artifact_created: String,
    geometry: ManifestEntry<'a>,
    statistics: BTreeMap<&'a str, BTreeMap<&'a str, ManifestEntry<'a>>>,
    source_revisions: BTreeMap<&'a str, &'a SourceRevision>,
}

#[derive(Debug, Serialize)]
struct ManifestEntry<'a> {
    url: String,
    size_bytes: u64,
    sha256: &'a str,
}

pub fn emit_manifest(
    hashed: &HashedArtifacts,
    version_label: &str,
    data_source_revisions: &BTreeMap<DataSourceKind, SourceRevision>,
    output_dir: &Path,
) -> Result<Hashed<FileReference>, AppError> {
    let artifact_created: DateTime<Utc> = Utc::now();
    let json: String = build_manifest_json(hashed, version_label, &artifact_created, data_source_revisions)?;

    let path: PathBuf = output_dir.join(MANIFEST_FILENAME);
    fs::write(&path, &json)?;

    let mut hasher: Sha256 = Sha256::new();
    hasher.update(json.as_bytes());
    let sha256_hex: String = hex_encode(&Into::<[u8; 32]>::into(hasher.finalize()));

    let byte_count: u64 = json.as_bytes().len() as u64;

    Ok(Hashed {
        inner: FileReference { path, byte_count },
        sha256_hex,
    })
}

fn build_manifest_json(
    hashed: &HashedArtifacts,
    version_label: &str,
    artifact_created: &DateTime<Utc>,
    data_source_revisions: &BTreeMap<DataSourceKind, SourceRevision>,
) -> Result<String, AppError> {
    let geometry: ManifestEntry<'_> = ManifestEntry {
        url: relative_url(&hashed.geometry, "geometry")?,
        size_bytes: hashed.geometry.byte_count,
        sha256: &hashed.geometry.sha256_hex,
    };

    let mut statistics: BTreeMap<&str, BTreeMap<&str, ManifestEntry<'_>>> = BTreeMap::new();
    for statistic_shard in &hashed.statistic_shards {
        let entry: ManifestEntry<'_> = ManifestEntry {
            url: relative_url(&statistic_shard.hashed_file, "data")?,
            size_bytes: statistic_shard.hashed_file.byte_count,
            sha256: &statistic_shard.hashed_file.sha256_hex,
        };
        statistics
            .entry(statistic_shard.statistic_kind.code())
            .or_default()
            .insert(statistic_shard.license_shard_class.as_str(), entry);
    }

    let source_revisions: BTreeMap<&str, &SourceRevision> = data_source_revisions
        .iter()
        .map(|(kind, revision)| (kind.code(), revision))
        .collect();

    let manifest: ManifestSerializer<'_> = ManifestSerializer {
        version: version_label,
        artifact_created: artifact_created.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        geometry,
        statistics,
        source_revisions,
    };

    let json: String = serde_json::to_string_pretty(&manifest)?;
    Ok(json)
}

fn relative_url(hashed_file: &Hashed<FileReference>, subdir: &str) -> Result<String, AppError> {
    let filename: &str = hashed_file
        .path
        .file_name()
        .and_then(|os| os.to_str())
        .ok_or_else(|| AppError::from(format!("emit_manifest: bad path {:?}", hashed_file.path)))?;
    Ok(format!("{}/{}", subdir, filename))
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

    use crate::artifact::artifact_model::StatisticShard;
    use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};

    fn make_hashed_artifacts() -> HashedArtifacts {
        HashedArtifacts {
            statistic_shards: vec![
                StatisticShard {
                    statistic_kind: StatisticKind::Tfr,
                    license_shard_class: LicenseShardClass::Base,
                    hashed_file: Hashed {
                        inner: FileReference {
                            path: PathBuf::from("/tmp/eafora/data/tfr-base-ef561234.sqlite"),
                            byte_count: 89000,
                        },
                        sha256_hex: "ef561234".repeat(8),
                    },
                },
                StatisticShard {
                    statistic_kind: StatisticKind::Tfr,
                    license_shard_class: LicenseShardClass::NonCommercial,
                    hashed_file: Hashed {
                        inner: FileReference {
                            path: PathBuf::from("/tmp/eafora/data/tfr-noncommercial-78ab9012.sqlite"),
                            byte_count: 4200,
                        },
                        sha256_hex: "78ab9012".repeat(8),
                    },
                },
                StatisticShard {
                    statistic_kind: StatisticKind::TestAlpha,
                    license_shard_class: LicenseShardClass::Base,
                    hashed_file: Hashed {
                        inner: FileReference {
                            path: PathBuf::from("/tmp/eafora/data/_test_alpha-base-cccc1111.sqlite"),
                            byte_count: 50000,
                        },
                        sha256_hex: "cccc1111".repeat(8),
                    },
                },
            ],
            geometry: Hashed {
                inner: FileReference {
                    path: PathBuf::from("/tmp/eafora/geometry/world-50m-ab12cd34.fgb"),
                    byte_count: 4380000,
                },
                sha256_hex: "ab12cd34".repeat(8),
            },
        }
    }

    #[test]
    fn build_manifest_json_sorts_statistics_alphabetically() {
        let hashed: HashedArtifacts = make_hashed_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::from([
            (DataSourceKind::WorldBankWDI, SourceRevision { revision: "2024-Q4".to_string(), fetched: "2024-12-31T00:00:00Z".parse().unwrap() }),
        ]);
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        let test_alpha_position: usize = json.find("\"_test_alpha\"").expect("_test_alpha present");
        let tfr_position: usize = json.find("\"tfr\"").expect("tfr present");
        assert!(test_alpha_position < tfr_position);
    }

    #[test]
    fn build_manifest_json_sorts_license_classes_alphabetically_within_statistic() {
        let hashed: HashedArtifacts = make_hashed_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        let base_position: usize = json.find("\"base\"").expect("base present");
        let noncommercial_position: usize = json.find("\"noncommercial\"").expect("noncommercial present");
        assert!(base_position < noncommercial_position);
    }

    #[test]
    fn build_manifest_json_emits_relative_urls_under_geometry_and_data() {
        let hashed: HashedArtifacts = make_hashed_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        assert!(json.contains("\"url\": \"geometry/world-50m-ab12cd34.fgb\""));
        assert!(json.contains("\"url\": \"data/tfr-base-ef561234.sqlite\""));
        assert!(json.contains("\"url\": \"data/_test_alpha-base-cccc1111.sqlite\""));
    }

    #[test]
    fn build_manifest_json_is_deterministic_byte_for_byte() {
        let hashed: HashedArtifacts = make_hashed_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::from([
            (DataSourceKind::WorldBankWDI, SourceRevision { revision: "2024-Q4".to_string(), fetched: "2024-12-31T00:00:00Z".parse().unwrap() }),
            (DataSourceKind::WorldBankWDI, SourceRevision { revision: "2026-w20".to_string(), fetched: "2026-05-15T00:00:00Z".parse().unwrap() }),
        ]);
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json_one: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();
        let json_two: String = build_manifest_json(&hashed, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        assert_eq!(json_one, json_two);
    }

    #[test]
    fn emit_manifest_writes_file_and_returns_consistent_sha256() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let hashed: HashedArtifacts = make_hashed_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();

        let manifest: Hashed<FileReference> =
            emit_manifest(&hashed, "2026-05-18", &data_source_revisions, temp_dir.path()).unwrap();

        assert!(manifest.path.exists());
        let bytes_on_disk: Vec<u8> = fs::read(&manifest.path).unwrap();
        let mut hasher: Sha256 = Sha256::new();
        hasher.update(&bytes_on_disk);
        let computed: String = hex_encode(&Into::<[u8; 32]>::into(hasher.finalize()));
        assert_eq!(computed, manifest.sha256_hex);
    }
}
