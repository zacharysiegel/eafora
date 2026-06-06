//! Serialization order is deterministic (BTreeMap on every map) so two
//! identical inputs produce byte-identical manifest.json files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::artifact::artifact_model::{FileReference, StatisticShard};
use crate::artifact::content_hashing::Hashed;
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

pub fn write_manifest(
    shards: &[StatisticShard],
    geometry: &Hashed<FileReference>,
    version_label: &str,
    data_source_revisions: &BTreeMap<DataSourceKind, SourceRevision>,
    output_dir: &Path,
) -> Result<Hashed<FileReference>, AppError> {
    let artifact_created: DateTime<Utc> = Utc::now();
    let json: String = build_manifest_json(shards, geometry, version_label, &artifact_created, data_source_revisions)?;

    let path: PathBuf = output_dir.join(MANIFEST_FILENAME);
    fs::write(&path, &json)?;

    let byte_count: u64 = json.as_bytes().len() as u64;

    Ok(Hashed::new(FileReference { path, byte_count }, json.as_bytes()))
}

fn build_manifest_json(
    shards: &[StatisticShard],
    geometry: &Hashed<FileReference>,
    version_label: &str,
    artifact_created: &DateTime<Utc>,
    data_source_revisions: &BTreeMap<DataSourceKind, SourceRevision>,
) -> Result<String, AppError> {
    let geometry_entry: ManifestEntry<'_> = ManifestEntry {
        url: relative_url(geometry, "geometry")?,
        size_bytes: geometry.byte_count,
        sha256: geometry.sha256_hex(),
    };

    let mut statistics: BTreeMap<&str, BTreeMap<&str, ManifestEntry<'_>>> = BTreeMap::new();
    for statistic_shard in shards {
        let entry: ManifestEntry<'_> = ManifestEntry {
            url: relative_url(&statistic_shard.hashed_file, "data")?,
            size_bytes: statistic_shard.hashed_file.byte_count,
            sha256: statistic_shard.hashed_file.sha256_hex(),
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
        geometry: geometry_entry,
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
        .ok_or_else(|| AppError::from(format!("bad path {:?}", hashed_file.path)))?;
    Ok(format!("{}/{}", subdir, filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::artifact::artifact_model::StatisticShard;
    use crate::artifact::content_hashing;
    use crate::canonical::canonical_model::{LicenseShardClass, StatisticKind};

    fn make_pre_manifest_artifacts() -> (Vec<StatisticShard>, Hashed<FileReference>) {
        let shards: Vec<StatisticShard> = vec![
            StatisticShard {
                statistic_kind: StatisticKind::Tfr,
                license_shard_class: LicenseShardClass::Base,
                hashed_file: Hashed::new_with_sha(
                    FileReference {
                        path: PathBuf::from("/tmp/eafora/data/tfr-base-ef561234.sqlite"),
                        byte_count: 89000,
                    },
                    "ef561234".repeat(8),
                ),
            },
            StatisticShard {
                statistic_kind: StatisticKind::Tfr,
                license_shard_class: LicenseShardClass::NonCommercial,
                hashed_file: Hashed::new_with_sha(
                    FileReference {
                        path: PathBuf::from("/tmp/eafora/data/tfr-noncommercial-78ab9012.sqlite"),
                        byte_count: 4200,
                    },
                    "78ab9012".repeat(8),
                ),
            },
            StatisticShard {
                statistic_kind: StatisticKind::TestAlpha,
                license_shard_class: LicenseShardClass::Base,
                hashed_file: Hashed::new_with_sha(
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
    fn build_manifest_json_sorts_statistics_alphabetically() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::from([
            (DataSourceKind::WorldBankWDI, SourceRevision { revision: "2024-Q4".to_string(), fetched: "2024-12-31T00:00:00Z".parse().unwrap() }),
        ]);
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        let test_alpha_position: usize = json.find("\"_test_alpha\"").expect("_test_alpha present");
        let tfr_position: usize = json.find("\"tfr\"").expect("tfr present");
        assert!(test_alpha_position < tfr_position);
    }

    #[test]
    fn build_manifest_json_sorts_license_classes_alphabetically_within_statistic() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        let base_position: usize = json.find("\"base\"").expect("base present");
        let noncommercial_position: usize = json.find("\"noncommercial\"").expect("noncommercial present");
        assert!(base_position < noncommercial_position);
    }

    #[test]
    fn build_manifest_json_emits_relative_urls_under_geometry_and_data() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", &artifact_created, &data_source_revisions).unwrap();

        assert!(json.contains("\"url\": \"geometry/world-50m-ab12cd34.fgb\""));
        assert!(json.contains("\"url\": \"data/tfr-base-ef561234.sqlite\""));
        assert!(json.contains("\"url\": \"data/_test_alpha-base-cccc1111.sqlite\""));
    }

    #[test]
    fn build_manifest_json_is_deterministic_byte_for_byte() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::from([
            (DataSourceKind::WorldBankWDI, SourceRevision { revision: "2024-Q4".to_string(), fetched: "2024-12-31T00:00:00Z".parse().unwrap() }),
            (DataSourceKind::WorldBankWDI, SourceRevision { revision: "2026-w20".to_string(), fetched: "2026-05-15T00:00:00Z".parse().unwrap() }),
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
        let computed: String = content_hashing::sha256_hex(&bytes_on_disk);
        assert_eq!(computed, manifest.sha256_hex());
    }
}
