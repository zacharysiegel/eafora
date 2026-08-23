use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::NaiveDate;
use sqlx::PgConnection;

use crate::artifact::artifact_model::{
    Artifacts, BuildReport, CandidateValue, CoupledBuildReport, PartitionedValue,
};
use crate::artifact::writer::{flatgeobuf, manifest as manifest_writer, sqlite};
use crate::artifact::{artifact_db, hashing, StatisticShard};
use shared::canonical::canonical_model::{DataSourceKind, LicenseShardClass, SourceRevision, StatisticKind};
use crate::error::AppError;
use shared::artifact::manifest::{self, BundleVariant};
use shared::filesystem::{self, FileReference, Hashed};

const UNITED_STATES_REGION_CODE: &str = "usa";

/// The subdirectories of a version directory holding the two bundle variants every build emits.
/// `complete` carries all periods and sources and publishes to the CDN; `downsampled` is World Bank
/// WDI at the United States reference year and is embedded into clients.
pub const SUBDIR_COMPLETE: &str = "complete";
pub const SUBDIR_DOWNSAMPLED: &str = "downsampled";
/// The symlink under `EAFORA_ARTIFACTS_DIR` that points at the newest build's version directory.
pub const LATEST_POINTER: &str = "latest";

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    pub test_offline: bool,
}

pub async fn build_artifacts(
    connection: &mut PgConnection,
    version_dir: &Path,
    version_label: &str,
    options: BuildOptions,
) -> Result<CoupledBuildReport, AppError> {
    let started: Instant = Instant::now();
    log::info!("starting; [version_label={} version_dir={:?}]", version_label, version_dir);

    let statistic_kinds: BTreeSet<StatisticKind> = artifact_db::read_all_statistic_kinds(&mut *connection).await?;

    let complete: BuildReport = build_bundle_variant(
        connection,
        &version_dir.join(SUBDIR_COMPLETE),
        BundleVariant::Complete,
        statistic_kinds.clone(),
        version_label,
        options,
        None,
    ).await?;

    let downsampled: BuildReport = build_bundle_variant(
        connection,
        &version_dir.join(SUBDIR_DOWNSAMPLED),
        BundleVariant::Downsampled,
        statistic_kinds,
        version_label,
        options,
        Some(&complete.artifacts.geometry),
    ).await?;

    log::info!(
        "complete in {:?}; [complete_manifest_sha256={} downsampled_manifest_sha256={}]",
        started.elapsed(),
        complete.artifacts.manifest.sha256_hex(),
        downsampled.artifacts.manifest.sha256_hex(),
    );

    Ok(CoupledBuildReport { complete, downsampled })
}

/// Builds one bundle variant into `variant_dir` as a self-contained tree (its own manifest, geometry,
/// and shards). `shared_geometry` lets the second variant reuse the first's already-built 1:50m
/// FlatGeobuf by copying it in, so the pinned Natural Earth source is fetched once per build.
async fn build_bundle_variant(
    connection: &mut PgConnection,
    variant_dir: &Path,
    variant: BundleVariant,
    statistic_kinds: BTreeSet<StatisticKind>,
    version_label: &str,
    options: BuildOptions,
    shared_geometry: Option<&Hashed<FileReference>>,
) -> Result<BuildReport, AppError> {
    fs::create_dir_all(variant_dir)?;

    let (shards, data_sources): (Vec<StatisticShard<Hashed<FileReference>>>, BTreeSet<DataSourceKind>) =
        create_statistic_shards(connection, variant_dir, statistic_kinds, variant).await?;

    let geometry: Hashed<FileReference> = match shared_geometry {
        Some(existing) => copy_geometry_into(existing, variant_dir)?,
        None => create_geometry(connection, variant_dir, options).await?,
    };

    let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> =
        artifact_db::read_latest_revisions(&mut *connection, &data_sources).await?;
    let manifest: Hashed<FileReference> =
        manifest_writer::write_manifest(&shards, &geometry, version_label, variant, &data_source_revisions, variant_dir)?;

    Ok(BuildReport {
        artifact_dir: variant_dir.to_path_buf(),
        version_label: version_label.to_string(),
        artifacts: Artifacts {
            shards,
            geometry,
            manifest,
        },
        data_source_revisions,
    })
}

async fn create_statistic_shards(
    connection: &mut PgConnection,
    variant_dir: &Path,
    statistic_kinds: BTreeSet<StatisticKind>,
    variant: BundleVariant,
) -> Result<(Vec<StatisticShard<Hashed<FileReference>>>, BTreeSet<DataSourceKind>), AppError> {
    let mut shards: Vec<StatisticShard<Hashed<FileReference>>> = Vec::new();
    let mut data_sources: BTreeSet<DataSourceKind> = BTreeSet::new();

    for kind in statistic_kinds {
        let candidates: Vec<CandidateValue> =
            artifact_db::read_candidate_values_for_statistic(&mut *connection, kind).await?;
        if candidates.is_empty() {
            log::warn!(
                "statistic {:?} has no candidate values; shard will be missing from this build",
                kind
            );
            continue;
        }

        /* The complete bundle keeps every source's value so a consumer can present an alternative to the one
           preference picks. The downsampled bundle exists to be small for first paint, so it keeps one. */
        let partitioned_values: Vec<PartitionedValue> = match variant {
            BundleVariant::Complete => partition_by_license(candidates),
            BundleVariant::Downsampled => downsample_to_reference_year(candidates, kind),
        };
        if partitioned_values.is_empty() {
            continue;
        }

        for value in &partitioned_values {
            data_sources.insert(value.data_source_kind);
        }

        let tmp_shards: Vec<StatisticShard<FileReference>> = sqlite::write_sqlite_shards(&partitioned_values, &variant_dir.join(manifest::SUBDIR_DATA))?;
        let hashed_shards: Vec<StatisticShard<Hashed<FileReference>>> = hashing::hash_sqlite_shards(tmp_shards)?;
        log::info!(
            "statistic {:?}: {} values across {} shards",
            kind,
            partitioned_values.len(),
            hashed_shards.len()
        );
        shards.extend(hashed_shards);
    }

    Ok((shards, data_sources))
}

fn partition_by_license(candidates: Vec<CandidateValue>) -> Vec<PartitionedValue> {
    candidates
        .iter()
        .map(|candidate| {
            let license_shard_class: LicenseShardClass =
                LicenseShardClass::from_license_class(candidate.license_class);

            PartitionedValue::from_candidate(candidate, license_shard_class)
        })
        .collect()
}

/// Reduces a statistic to its World Bank WDI values at one reference year (the most-recent period
/// the United States reports) for the embedded bundle's single time slice. One shared year is
/// required because the renderer resolves each region's value by exact period; a per-region-latest
/// slice would leave every region whose latest year differs from the active period with nothing to
/// draw. Yields nothing when the United States has no World Bank WDI value to anchor the year.
fn downsample_to_reference_year(
    candidates: Vec<CandidateValue>,
    statistic_kind: StatisticKind,
) -> Vec<PartitionedValue> {
    let world_bank_wdi_candidates: Vec<CandidateValue> = candidates
        .into_iter()
        .filter(|candidate| candidate.data_source_kind == DataSourceKind::WorldBankWDI)
        .collect();

    let reference_period_start: Option<NaiveDate> = world_bank_wdi_candidates
        .iter()
        .filter(|candidate| candidate.region_code == UNITED_STATES_REGION_CODE)
        .map(|candidate| candidate.period.start)
        .max();
    let Some(reference_period_start) = reference_period_start else {
        log::warn!(
            "downsampled build omits statistic {:?}: no World Bank WDI United States value to anchor the reference year",
            statistic_kind,
        );
        return Vec::new();
    };

    world_bank_wdi_candidates
        .into_iter()
        .filter(|candidate| candidate.period.start == reference_period_start)
        .map(|candidate| {
            let license_shard_class: LicenseShardClass = LicenseShardClass::from_license_class(candidate.license_class);
            PartitionedValue::from_candidate(&candidate, license_shard_class)
        })
        .collect()
}

async fn create_geometry(
    connection: &mut PgConnection,
    variant_dir: &Path,
    options: BuildOptions,
) -> Result<Hashed<FileReference>, AppError> {
    let geometry: FileReference = if options.test_offline {
        flatgeobuf::write_placeholder_geometry(variant_dir)?
    } else {
        flatgeobuf::write_geometry(&mut *connection, variant_dir).await?
    };
    log::info!("wrote geometry {:?}", geometry.path);
    let geometry: Hashed<FileReference> = hashing::hash_geometry(geometry)?;
    Ok(geometry)
}

/// Copies an already-built, content-addressed geometry file into `variant_dir`'s geometry subdir and
/// returns a handle to the copy. The bytes are identical, so the sha and byte count carry over without
/// re-hashing.
fn copy_geometry_into(geometry: &Hashed<FileReference>, variant_dir: &Path) -> Result<Hashed<FileReference>, AppError> {
    let geometry_dir: PathBuf = variant_dir.join(manifest::SUBDIR_GEOMETRY);
    fs::create_dir_all(&geometry_dir)?;

    let filename: &str = filesystem::filename_of(&geometry.path)?;
    let destination: PathBuf = geometry_dir.join(filename);
    fs::copy(&geometry.path, &destination)
        .map_err(|err| AppError::from(format!("copying geometry into {:?} failed: {}", variant_dir, err)))?;

    Ok(Hashed::new_with_sha(
        FileReference { path: destination, byte_count: geometry.byte_count },
        geometry.sha256_hex().to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use uuid::Uuid;
    use shared::canonical::canonical_model::{DataStatus, LicenseClass, NaiveDatePeriod};

    fn candidate_value(region_code: &str, data_source_kind: DataSourceKind, year: i32, value: f64) -> CandidateValue {
        CandidateValue {
            region_id: Uuid::now_v7(),
            region_code: region_code.to_string(),
            statistic_kind: StatisticKind::try_from("tfr").unwrap(),
            period: NaiveDatePeriod {
                start: NaiveDate::from_ymd_opt(year, 1, 1).unwrap(),
                end: NaiveDate::from_ymd_opt(year, 12, 31).unwrap(),
            },
            value,
            data_status: DataStatus::try_from("final").unwrap(),
            data_source_kind,
            data_source_preference_rank: 100,
            data_source_revision: "rev".to_string(),
            license_class: LicenseClass::Attribution,
        }
    }

    #[test]
    fn downsample_to_reference_year_keeps_every_region_at_the_united_states_latest_period() {
        let candidates: Vec<CandidateValue> = vec![
            candidate_value("usa", DataSourceKind::WorldBankWDI, 2021, 1.66),
            candidate_value("usa", DataSourceKind::WorldBankWDI, 2023, 1.62),
            candidate_value("deu", DataSourceKind::WorldBankWDI, 2021, 1.58),
            candidate_value("deu", DataSourceKind::WorldBankWDI, 2023, 1.46),
            candidate_value("fra", DataSourceKind::WorldBankWDI, 2023, 1.79),
            candidate_value("bra", DataSourceKind::WorldBankWDI, 2021, 1.64),
        ];

        let kept: Vec<PartitionedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

        let reference_period_start: NaiveDate = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        assert!(kept.iter().all(|value| value.period.start == reference_period_start));
        assert!(kept.iter().any(|value| value.region_code == "usa"));
        assert!(kept.iter().any(|value| value.region_code == "deu"));
        assert!(kept.iter().any(|value| value.region_code == "fra"));
        assert!(!kept.iter().any(|value| value.region_code == "bra"));
        assert_eq!(kept.len(), 3);
    }

    #[test]
    fn downsample_to_reference_year_excludes_sources_other_than_world_bank_wdi() {
        let candidates: Vec<CandidateValue> = vec![
            candidate_value("usa", DataSourceKind::WorldBankWDI, 2023, 1.62),
            candidate_value("usa", DataSourceKind::HumanFertilityDatabase, 2025, 1.50),
            candidate_value("deu", DataSourceKind::HumanFertilityDatabase, 2023, 1.46),
        ];

        let kept: Vec<PartitionedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].region_code, "usa");
        assert_eq!(kept[0].data_source_kind, DataSourceKind::WorldBankWDI);
        assert_eq!(kept[0].period.start, NaiveDate::from_ymd_opt(2023, 1, 1).unwrap());
    }

    #[test]
    fn downsample_to_reference_year_yields_nothing_without_united_states_data() {
        let candidates: Vec<CandidateValue> = vec![
            candidate_value("deu", DataSourceKind::WorldBankWDI, 2023, 1.46),
        ];

        let kept: Vec<PartitionedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

        assert!(kept.is_empty());
    }
}
