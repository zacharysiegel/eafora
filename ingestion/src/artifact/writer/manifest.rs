//! Serialization order is deterministic (BTreeMap on every map) so two
//! identical inputs produce byte-identical manifest.json files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use shared::artifact::manifest::{self, Manifest, ManifestEntry};
use shared::canonical::canonical_model::{DataSourceKind, LicenseShardClass, SourceRevision, StatisticKind};
use shared::filesystem::{FileReference, Hashed};

use crate::artifact::artifact_model::StatisticShard;
use crate::error::AppError;

pub fn write_manifest(
    shards: &[StatisticShard<Hashed<FileReference>>],
    geometry: &Hashed<FileReference>,
    version_label: &str,
    data_source_revisions: &BTreeMap<DataSourceKind, SourceRevision>,
    artifact_dir: &Path,
) -> Result<Hashed<FileReference>, AppError> {
    let artifact_created: DateTime<Utc> = Utc::now();
    let json: String = build_manifest_json(shards, geometry, version_label, &artifact_created, data_source_revisions)?;

    let path: PathBuf = artifact_dir.join(manifest::MANIFEST_FILENAME);
    fs::write(&path, &json)?;

    let byte_count: u64 = json.as_bytes().len() as u64;

    Ok(Hashed::new(FileReference { path, byte_count }, json.as_bytes()))
}

fn build_manifest_json(
    shards: &[StatisticShard<Hashed<FileReference>>],
    geometry: &Hashed<FileReference>,
    version_label: &str,
    artifact_created: &DateTime<Utc>,
    data_source_revisions: &BTreeMap<DataSourceKind, SourceRevision>,
) -> Result<String, AppError> {
    let geometry_entry: ManifestEntry = ManifestEntry {
        relative_path: relative_path(manifest::SUBDIR_GEOMETRY, geometry)?,
        size_bytes: geometry.byte_count,
        sha256: geometry.sha256_hex().to_string(),
    };

    let mut statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>> = BTreeMap::new();
    for statistic_shard in shards {
        let entry: ManifestEntry = ManifestEntry {
            relative_path: relative_path(manifest::SUBDIR_DATA, &statistic_shard.file)?,
            size_bytes: statistic_shard.file.byte_count,
            sha256: statistic_shard.file.sha256_hex().to_string(),
        };
        statistics
            .entry(statistic_shard.key.statistic_kind)
            .or_default()
            .insert(statistic_shard.key.license_shard_class, entry);
    }

    let manifest: Manifest = Manifest {
        manifest_schema_version: manifest::MANIFEST_SCHEMA_VERSION,
        version: version_label.to_string(),
        artifact_created: *artifact_created,
        geometry: geometry_entry,
        statistics,
        source_revisions: data_source_revisions.clone(),
    };

    let json: String = serde_json::to_string_pretty(&manifest)?;

    Ok(json)
}

fn relative_path(subdir: &str, hashed_file: &Hashed<FileReference>) -> Result<String, AppError> {
    let filename: &str = hashed_file
        .path
        .file_name()
        .and_then(|os| os.to_str())
        .ok_or_else(|| AppError::new(&format!("bad path {:?}", hashed_file.path)))?;

    Ok(format!("{}/{}", subdir, filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    use shared::artifact::bundle::StatisticShardKey;

    fn make_pre_manifest_artifacts() -> (Vec<StatisticShard<Hashed<FileReference>>>, Hashed<FileReference>) {
        let shards: Vec<StatisticShard<Hashed<FileReference>>> = vec![
            StatisticShard {
                key: StatisticShardKey {
                    statistic_kind: StatisticKind::Tfr,
                    license_shard_class: LicenseShardClass::Base,
                },
                file: Hashed::new_with_sha(
                    FileReference {
                        path: PathBuf::from("/tmp/eafora/data/tfr-base-ef561234.sqlite"),
                        byte_count: 89000,
                    },
                    "ef561234".repeat(8),
                ),
            },
            StatisticShard {
                key: StatisticShardKey {
                    statistic_kind: StatisticKind::Tfr,
                    license_shard_class: LicenseShardClass::NonCommercial,
                },
                file: Hashed::new_with_sha(
                    FileReference {
                        path: PathBuf::from("/tmp/eafora/data/tfr-noncommercial-78ab9012.sqlite"),
                        byte_count: 4200,
                    },
                    "78ab9012".repeat(8),
                ),
            },
            StatisticShard {
                key: StatisticShardKey {
                    statistic_kind: StatisticKind::TestAlpha,
                    license_shard_class: LicenseShardClass::Base,
                },
                file: Hashed::new_with_sha(
                    FileReference {
                        path: PathBuf::from("/tmp/eafora/data/_test_alpha-base-cccc1111.sqlite"),
                        byte_count: 50000,
                    },
                    "cccc1111".repeat(8),
                ),
            },
        ];
        let geometry: Hashed<FileReference> = Hashed::new_with_sha(
            FileReference {
                path: PathBuf::from("/tmp/eafora/geometry/world-50m-ab12cd34.fgb"),
                byte_count: 4380000,
            },
            "ab12cd34".repeat(8),
        );
        (shards, geometry)
    }

    #[test]
    fn build_manifest_json_includes_schema_version() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        assert!(json.contains("\"manifest_schema_version\": 1"));
    }

    #[test]
    fn build_manifest_json_orders_statistics_by_statistic_kind() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::from([
            (DataSourceKind::WorldBankWDI, SourceRevision { revision: "2024-12-12".to_string(), published: Some("2024-12-12T00:00:00Z".parse().unwrap()), fetched: "2024-12-31T00:00:00Z".parse().unwrap() }),
        ]);
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        let tfr_position: usize = json.find("\"tfr\"").expect("tfr present");
        let test_alpha_position: usize = json.find("\"_test_alpha\"").expect("_test_alpha present");
        assert!(tfr_position < test_alpha_position);
    }

    #[test]
    fn build_manifest_json_orders_license_classes_by_shard_class() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        let base_position: usize = json.find("\"base\"").expect("base present");
        let noncommercial_position: usize = json.find("\"noncommercial\"").expect("noncommercial present");
        assert!(base_position < noncommercial_position);
    }

    #[test]
    fn build_manifest_json_emits_relative_paths_under_geometry_and_data() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        assert!(json.contains(&format!("\"relative_path\": \"{}/world-50m-ab12cd34.fgb\"", manifest::SUBDIR_GEOMETRY)));
        assert!(json.contains(&format!("\"relative_path\": \"{}/tfr-base-ef561234.sqlite\"", manifest::SUBDIR_DATA)));
        assert!(json.contains(&format!("\"relative_path\": \"{}/_test_alpha-base-cccc1111.sqlite\"", manifest::SUBDIR_DATA)));
    }

    #[test]
    fn build_manifest_json_is_deterministic_byte_for_byte() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::from([
            (DataSourceKind::WorldBankWDI, SourceRevision { revision: "2026-05-15".to_string(), published: Some("2026-05-15T00:00:00Z".parse().unwrap()), fetched: "2026-05-15T00:00:00Z".parse().unwrap() }),
        ]);
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json_one: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();
        let json_two: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        assert_eq!(json_one, json_two);
    }

    #[test]
    fn write_manifest_writes_file_and_returns_consistent_sha256() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();

        let manifest: Hashed<FileReference> =
            write_manifest(&shards, &geometry, "2026-05-18", &data_source_revisions, temp_dir.path()).unwrap();

        assert!(manifest.path.exists());
        let bytes_on_disk: Vec<u8> = fs::read(&manifest.path).unwrap();
        let computed: String = shared::filesystem::sha256_hex(&bytes_on_disk);
        assert_eq!(computed, manifest.sha256_hex());
    }
}
