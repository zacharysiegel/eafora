use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::artifact::artifact_model::{
    Artifacts, BuildReport, CandidateValue, ResolvedValue,
};
use crate::artifact::writer::{flatgeobuf, manifest as manifest_writer, sqlite};
use crate::artifact::{artifact_db, hashing, source_choice, StatisticShard};
use crate::canonical::canonical_db;
use shared::canonical::canonical_model::{DataSourceKind, SourceRevision, StatisticKind};
use crate::canonical::canonical_entity::SourceChoice;
use crate::error::AppError;
use shared::artifact::manifest;
use shared::filesystem::{FileReference, Hashed};

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

        for candidate in &candidates {
            data_sources.insert(candidate.data_source_kind);
        }

        let resolved: Vec<ResolvedValue> = source_choice::resolve_candidates(candidates, source_choices)?;
        let resolved: Vec<ResolvedValue> = if options.downsampled {
            keep_latest_period_per_region(resolved)
        } else {
            resolved
        };

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

/// Reduces a statistic's resolved values to each region's most-recent one, for the embedded bundle's
/// single-time-slice shape. Run after `resolve_candidates` so the source is already chosen (preserving
/// one-source-per-series); the `BTreeMap` keeps the output order deterministic.
fn keep_latest_period_per_region(resolved: Vec<ResolvedValue>) -> Vec<ResolvedValue> {
    let mut latest_value_by_region: BTreeMap<Uuid, ResolvedValue> = BTreeMap::new();
    for value in resolved {
        let is_more_recent: bool = match latest_value_by_region.get(&value.region_id) {
            Some(existing) => value.period.end > existing.period.end,
            None => true,
        };
        if is_more_recent {
            latest_value_by_region.insert(value.region_id, value);
        }
    }

    latest_value_by_region.into_values().collect()
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
    use chrono::NaiveDate;
    use shared::canonical::canonical_model::{DataStatus, LicenseShardClass, NaiveDatePeriod};

    fn resolved_value(region_id: Uuid, year: i32, value: f64) -> ResolvedValue {
        ResolvedValue {
            region_id,
            region_iso3: "AAA".to_string(),
            statistic_kind: StatisticKind::try_from("tfr").unwrap(),
            period: NaiveDatePeriod {
                start: NaiveDate::from_ymd_opt(year, 1, 1).unwrap(),
                end: NaiveDate::from_ymd_opt(year, 12, 31).unwrap(),
            },
            value,
            data_status: DataStatus::try_from("final").unwrap(),
            data_source_kind: DataSourceKind::WorldBankWDI,
            data_source_revision: "rev".to_string(),
            license_shard_class: LicenseShardClass::try_from("base").unwrap(),
        }
    }

    #[test]
    fn keep_latest_period_per_region_keeps_the_newest_value_per_region() {
        let region_a: Uuid = Uuid::now_v7();
        let region_b: Uuid = Uuid::now_v7();
        let resolved: Vec<ResolvedValue> = vec![
            resolved_value(region_a, 2020, 1.0),
            resolved_value(region_a, 2022, 2.0),
            resolved_value(region_b, 2021, 3.0),
        ];

        let kept: Vec<ResolvedValue> = keep_latest_period_per_region(resolved);

        assert_eq!(kept.len(), 2);
        let region_a_kept: &ResolvedValue = kept.iter().find(|value| value.region_id == region_a).unwrap();
        assert_eq!(region_a_kept.period.start, NaiveDate::from_ymd_opt(2022, 1, 1).unwrap());
        assert!((region_a_kept.value - 2.0).abs() < f64::EPSILON);
        let region_b_kept: &ResolvedValue = kept.iter().find(|value| value.region_id == region_b).unwrap();
        assert_eq!(region_b_kept.period.start, NaiveDate::from_ymd_opt(2021, 1, 1).unwrap());
    }
}
