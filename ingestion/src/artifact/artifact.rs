use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use chrono::NaiveDate;
use sqlx::PgConnection;

use crate::artifact::artifact_model::{
    Artifacts, BuildReport, CandidateValue, ResolvedValue,
};
use crate::artifact::writer::{flatgeobuf, manifest as manifest_writer, sqlite};
use crate::artifact::{artifact_db, hashing, source_choice, StatisticShard};
use crate::canonical::canonical_db;
use shared::canonical::canonical_model::{DataSourceKind, LicenseShardClass, SourceRevision, StatisticKind};
use crate::canonical::canonical_entity::SourceChoice;
use crate::error::AppError;
use shared::artifact::manifest;
use shared::filesystem::{FileReference, Hashed};

const UNITED_STATES_ISO3: &str = "USA";

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    pub test_offline: bool,
    pub downsampled: bool,
}

pub async fn build_artifacts(
    connection: &mut PgConnection,
    artifact_dir: &Path,
    version_label: &str,
    options: BuildOptions,
) -> Result<BuildReport, AppError> {
    let started: Instant = Instant::now();
    log::info!("starting version_label={} artifact_dir={:?}", version_label, artifact_dir,);

    fs::create_dir_all(artifact_dir)?;

    let source_choices: Vec<SourceChoice> = canonical_db::read_source_choices(&mut *connection).await?;
    let statistic_kinds: BTreeSet<StatisticKind> = artifact_db::read_all_statistic_kinds(&mut *connection).await?;

    let (shards, data_sources): (Vec<StatisticShard<Hashed<FileReference>>>, BTreeSet<DataSourceKind>) =
        create_statistic_shards(connection, artifact_dir, &source_choices, statistic_kinds, options).await?;
    let geometry: Hashed<FileReference> = create_geometry(connection, artifact_dir, options).await?;

    let data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> =
        artifact_db::read_latest_revisions(&mut *connection, &data_sources).await?;
    let manifest: Hashed<FileReference> =
        manifest_writer::write_manifest(&shards, &geometry, version_label, &data_source_revisions, artifact_dir)?;

    log::info!(
        "complete in {:?}; manifest sha256={}",
        started.elapsed(),
        manifest.sha256_hex(),
    );

    Ok(BuildReport {
        artifact_dir: artifact_dir.to_path_buf(),
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
    artifact_dir: &Path,
    source_choices: &[SourceChoice],
    statistic_kinds: BTreeSet<StatisticKind>,
    options: BuildOptions,
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

        let resolved: Vec<ResolvedValue> = if options.downsampled {
            downsample_to_reference_year(candidates, kind)
        } else {
            source_choice::resolve_candidates(candidates, source_choices)?
        };
        if resolved.is_empty() {
            continue;
        }

        for value in &resolved {
            data_sources.insert(value.data_source_kind);
        }

        let tmp_shards: Vec<StatisticShard<FileReference>> = sqlite::write_sqlite_shards(&resolved, &artifact_dir.join(manifest::SUBDIR_DATA))?;
        let hashed_shards: Vec<StatisticShard<Hashed<FileReference>>> = hashing::hash_sqlite_shards(tmp_shards)?;
        log::info!(
            "statistic {:?}: {} resolved values across {} shards",
            kind,
            resolved.len(),
            hashed_shards.len()
        );
        shards.extend(hashed_shards);
    }

    Ok((shards, data_sources))
}

/// Reduces a statistic to its World Bank WDI values at one reference year (the most-recent period
/// the United States reports) for the embedded bundle's single time slice. One shared year is
/// required because the renderer resolves each region's value by exact period; a per-region-latest
/// slice would leave every region whose latest year differs from the active period with nothing to
/// draw. Yields nothing when the United States has no World Bank WDI value to anchor the year.
fn downsample_to_reference_year(
    candidates: Vec<CandidateValue>,
    statistic_kind: StatisticKind,
) -> Vec<ResolvedValue> {
    let world_bank_wdi_candidates: Vec<CandidateValue> = candidates
        .into_iter()
        .filter(|candidate| candidate.data_source_kind == DataSourceKind::WorldBankWDI)
        .collect();

    let reference_period_start: Option<NaiveDate> = world_bank_wdi_candidates
        .iter()
        .filter(|candidate| candidate.region_iso3 == UNITED_STATES_ISO3)
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
            ResolvedValue::from_candidate(&candidate, license_shard_class)
        })
        .collect()
}

async fn create_geometry(
    connection: &mut PgConnection,
    artifact_dir: &Path,
    options: BuildOptions,
) -> Result<Hashed<FileReference>, AppError> {
    let geometry: FileReference = if options.test_offline {
        flatgeobuf::write_placeholder_geometry(artifact_dir)?
    } else {
        flatgeobuf::write_geometry(&mut *connection, artifact_dir).await?
    };
    log::info!("wrote geometry {:?}", geometry.path);
    let geometry: Hashed<FileReference> = hashing::hash_geometry(geometry)?;
    Ok(geometry)
}

#[cfg(test)]
mod tests {
    use super::*;

    use uuid::Uuid;
    use shared::canonical::canonical_model::{DataStatus, LicenseClass, NaiveDatePeriod};

    fn candidate_value(region_iso3: &str, data_source_kind: DataSourceKind, year: i32, value: f64) -> CandidateValue {
        CandidateValue {
            region_id: Uuid::now_v7(),
            region_iso3: region_iso3.to_string(),
            statistic_kind: StatisticKind::try_from("tfr").unwrap(),
            period: NaiveDatePeriod {
                start: NaiveDate::from_ymd_opt(year, 1, 1).unwrap(),
                end: NaiveDate::from_ymd_opt(year, 12, 31).unwrap(),
            },
            value,
            data_status: DataStatus::try_from("final").unwrap(),
            data_source_kind,
            data_source_revision: "rev".to_string(),
            license_class: LicenseClass::Attribution,
        }
    }

    #[test]
    fn downsample_to_reference_year_keeps_every_region_at_the_united_states_latest_period() {
        let candidates: Vec<CandidateValue> = vec![
            candidate_value("USA", DataSourceKind::WorldBankWDI, 2021, 1.66),
            candidate_value("USA", DataSourceKind::WorldBankWDI, 2023, 1.62),
            candidate_value("DEU", DataSourceKind::WorldBankWDI, 2021, 1.58),
            candidate_value("DEU", DataSourceKind::WorldBankWDI, 2023, 1.46),
            candidate_value("FRA", DataSourceKind::WorldBankWDI, 2023, 1.79),
            candidate_value("BRA", DataSourceKind::WorldBankWDI, 2021, 1.64),
        ];

        let kept: Vec<ResolvedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

        let reference_period_start: NaiveDate = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        assert!(kept.iter().all(|value| value.period.start == reference_period_start));
        assert!(kept.iter().any(|value| value.region_iso3 == "USA"));
        assert!(kept.iter().any(|value| value.region_iso3 == "DEU"));
        assert!(kept.iter().any(|value| value.region_iso3 == "FRA"));
        assert!(!kept.iter().any(|value| value.region_iso3 == "BRA"));
        assert_eq!(kept.len(), 3);
    }

    #[test]
    fn downsample_to_reference_year_excludes_sources_other_than_world_bank_wdi() {
        let candidates: Vec<CandidateValue> = vec![
            candidate_value("USA", DataSourceKind::WorldBankWDI, 2023, 1.62),
            candidate_value("USA", DataSourceKind::TestAlpha, 2025, 1.50),
            candidate_value("DEU", DataSourceKind::TestAlpha, 2023, 1.46),
        ];

        let kept: Vec<ResolvedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].region_iso3, "USA");
        assert_eq!(kept[0].data_source_kind, DataSourceKind::WorldBankWDI);
        assert_eq!(kept[0].period.start, NaiveDate::from_ymd_opt(2023, 1, 1).unwrap());
    }

    #[test]
    fn downsample_to_reference_year_yields_nothing_without_united_states_data() {
        let candidates: Vec<CandidateValue> = vec![
            candidate_value("DEU", DataSourceKind::WorldBankWDI, 2023, 1.46),
        ];

        let kept: Vec<ResolvedValue> = downsample_to_reference_year(candidates, StatisticKind::try_from("tfr").unwrap());

        assert!(kept.is_empty());
    }
}
