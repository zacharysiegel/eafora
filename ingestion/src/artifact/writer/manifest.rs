//! Serialization order is deterministic (BTreeMap on every map) so two
//! identical inputs produce byte-identical manifest.json files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use shared::artifact::manifest::{self, BundleVariant, Manifest, ManifestEntry};
use shared::canonical::canonical_model::{LicenseShardClass, StatisticKind};
use shared::filesystem::{FileReference, Hashed};

use crate::artifact::artifact_model::{BundleProvenance, StatisticShard};
use crate::error::AppError;

pub fn write_manifest(
    shards: &[StatisticShard<Hashed<FileReference>>],
    geometry: &Hashed<FileReference>,
    version_label: &str,
    variant: BundleVariant,
    provenance: &BundleProvenance,
    artifact_dir: &Path,
) -> Result<Hashed<FileReference>, AppError> {
    let artifact_created: DateTime<Utc> = Utc::now();
    let json: String = build_manifest_json(shards, geometry, version_label, variant, &artifact_created, provenance)?;

    let path: PathBuf = artifact_dir.join(manifest::MANIFEST_FILENAME);
    fs::write(&path, &json)?;

    let byte_count: u64 = json.len() as u64;

    Ok(Hashed::new(FileReference { path, byte_count }, json.as_bytes()))
}

fn build_manifest_json(
    shards: &[StatisticShard<Hashed<FileReference>>],
    geometry: &Hashed<FileReference>,
    version_label: &str,
    variant: BundleVariant,
    artifact_created: &DateTime<Utc>,
    provenance: &BundleProvenance,
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
        variant,
        artifact_created: *artifact_created,
        geometry: geometry_entry,
        statistics,
        source_revisions: provenance.source_revisions.clone(),
        source_attribution: provenance.source_attribution.clone(),
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
    use shared::canonical::canonical_model::{DataSourceKind, SourceAttribution, SourceRevision};

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
                    statistic_kind: StatisticKind::Ccf,
                    license_shard_class: LicenseShardClass::Base,
                },
                file: Hashed::new_with_sha(
                    FileReference {
                        path: PathBuf::from("/tmp/eafora/data/ccf-base-cccc1111.sqlite"),
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

    fn empty_provenance() -> BundleProvenance {
        BundleProvenance {
            source_revisions: BTreeMap::new(),
            source_attribution: BTreeMap::new(),
        }
    }

    fn populated_provenance() -> BundleProvenance {
        BundleProvenance {
            source_revisions: BTreeMap::from([(
                DataSourceKind::WorldBankWDI,
                SourceRevision {
                    revision: "2026-05-15".to_string(),
                    published: Some("2026-05-15T00:00:00Z".parse().unwrap()),
                    fetched: "2026-05-15T00:00:00Z".parse().unwrap(),
                },
            )]),
            source_attribution: BTreeMap::from([(
                DataSourceKind::WorldBankWDI,
                SourceAttribution {
                    attribution_text: "World Bank, World Development Indicators (CC BY 4.0)".to_string(),
                    license_name: "CC BY 4.0".to_string(),
                    license_url: "https://creativecommons.org/licenses/by/4.0/".to_string(),
                    homepage_url: "https://databank.worldbank.org/source/world-development-indicators".to_string(),
                },
            )]),
        }
    }

    /// The consumer ranks a cached bundle by this field, so a variant that never reaches the JSON would
    /// silently let a downsampled bundle be treated as complete.
    #[test]
    fn build_manifest_json_records_the_bundle_variant() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let provenance: BundleProvenance = empty_provenance();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let complete: String = build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &artifact_created, &provenance).unwrap();
        let downsampled: String = build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Downsampled, &artifact_created, &provenance).unwrap();

        assert!(complete.contains("\"variant\": \"complete\""));
        assert!(downsampled.contains("\"variant\": \"downsampled\""));
    }

    #[test]
    fn build_manifest_json_includes_schema_version() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let provenance: BundleProvenance = empty_provenance();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &artifact_created, &provenance).unwrap();

        assert!(json.contains(&format!("\"manifest_schema_version\": {}", manifest::MANIFEST_SCHEMA_VERSION)));
    }

    #[test]
    fn build_manifest_json_orders_statistics_by_statistic_kind() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let provenance: BundleProvenance = empty_provenance();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &artifact_created, &provenance).unwrap();

        let tfr_position: usize = json.find("\"tfr\"").expect("tfr present");
        let ccf_position: usize = json.find("\"ccf\"").expect("ccf present");
        assert!(tfr_position < ccf_position);
    }

    /// A source's attribution is a licence obligation, so it has to survive the write rather than be dropped
    /// as a field the writer forgot.
    #[test]
    fn build_manifest_json_writes_the_attribution_and_the_definitions() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();
        let provenance: BundleProvenance = populated_provenance();

        let json: String =
            build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &artifact_created, &provenance)
                .unwrap();

        let manifest: Manifest = manifest::parse_manifest(json.as_bytes()).unwrap();

        assert_eq!(manifest.source_attribution[&DataSourceKind::WorldBankWDI].license_name, "CC BY 4.0");
    }

    #[test]
    fn build_manifest_json_orders_license_classes_by_shard_class() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let provenance: BundleProvenance = empty_provenance();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &artifact_created, &provenance).unwrap();

        let base_position: usize = json.find("\"base\"").expect("base present");
        let noncommercial_position: usize = json.find("\"noncommercial\"").expect("noncommercial present");
        assert!(base_position < noncommercial_position);
    }

    #[test]
    fn build_manifest_json_emits_relative_paths_under_geometry_and_data() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let provenance: BundleProvenance = empty_provenance();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json: String = build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &artifact_created, &provenance).unwrap();

        assert!(json.contains(&format!("\"relative_path\": \"{}/world-50m-ab12cd34.fgb\"", manifest::SUBDIR_GEOMETRY)));
        assert!(json.contains(&format!("\"relative_path\": \"{}/tfr-base-ef561234.sqlite\"", manifest::SUBDIR_DATA)));
        assert!(json.contains(&format!("\"relative_path\": \"{}/ccf-base-cccc1111.sqlite\"", manifest::SUBDIR_DATA)));
    }

    #[test]
    fn build_manifest_json_is_deterministic_byte_for_byte() {
        let (shards, geometry) = make_pre_manifest_artifacts();
        let provenance: BundleProvenance = populated_provenance();
        let artifact_created: DateTime<Utc> = "2026-05-18T03:00:00Z".parse().unwrap();

        let json_one: String = build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &artifact_created, &provenance).unwrap();
        let json_two: String = build_manifest_json(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &artifact_created, &provenance).unwrap();

        assert_eq!(json_one, json_two);
    }

    #[test]
    fn write_manifest_writes_file_and_returns_consistent_sha256() {
        let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
        let (shards, geometry) = make_pre_manifest_artifacts();
        let provenance: BundleProvenance = empty_provenance();

        let manifest: Hashed<FileReference> =
            write_manifest(&shards, &geometry, "2026-05-18", BundleVariant::Complete, &provenance, temp_dir.path()).unwrap();

        assert!(manifest.path.exists());
        let bytes_on_disk: Vec<u8> = fs::read(&manifest.path).unwrap();
        let computed: String = shared::filesystem::sha256_hex(&bytes_on_disk);
        assert_eq!(computed, manifest.sha256_hex());
    }
}
