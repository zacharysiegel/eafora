use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;
use crate::artifact::content_hashing::Hashed;
use crate::canonical::canonical_model::{DataSourceKind, DataStatus, LicenseClass, LicenseShardClass, StatisticKind};
use crate::error::AppError;

/// One value as it sits in the canonical store: a single source's
/// reading for a `(region, statistic, period)` cell. Multiple candidates
/// can exist for the same cell, one per data source that publishes it.
/// Carries the source's `license_class`; the shard bin isn't decided yet.
#[derive(Debug, Clone)]
pub struct CandidateValue {
    pub region_id: Uuid,
    pub region_iso3: String,
    pub statistic_kind: StatisticKind,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: DataStatus,
    pub data_source_kind: DataSourceKind,
    pub data_source_revision: String,
    pub license_class: LicenseClass,
}

#[derive(Debug, Clone)]
pub struct CandidateValueProjection {
    pub region_id: Uuid,
    pub region_iso3: String,
    pub statistic_code: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub value: f64,
    pub data_status: String,
    pub data_source_code: String,
    pub data_source_revision: String,
    pub license_class: String,
}

impl TryFrom<CandidateValueProjection> for CandidateValue {
    type Error = AppError;

    fn try_from(projection: CandidateValueProjection) -> Result<Self, Self::Error> {
        Ok(CandidateValue {
            region_id: projection.region_id,
            region_iso3: projection.region_iso3,
            statistic_kind: StatisticKind::try_from(projection.statistic_code.as_str())?,
            period: NaiveDatePeriod {
                start: projection.period_start,
                end: projection.period_end,
            },
            value: projection.value,
            data_status: DataStatus::try_from(projection.data_status.as_str())?,
            data_source_kind: DataSourceKind::try_from(projection.data_source_code.as_str())?,
            data_source_revision: projection.data_source_revision,
            license_class: LicenseClass::try_from(projection.license_class.as_str())?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CountryNameProjection {
    pub iso3: String,
    pub name_en: String,
}

/// A "resolved" `CandidateValue` (after data source selection). Exactly one
/// `ResolvedValue` per `(region, statistic, period)` cell, drawn from the source
/// chosen for that series.
#[derive(Debug, Clone)]
pub struct ResolvedValue {
    pub region_id: Uuid,
    pub region_iso3: String,
    pub statistic_kind: StatisticKind,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: DataStatus,
    pub data_source_kind: DataSourceKind,
    pub data_source_revision: String,
    pub license_shard_class: LicenseShardClass,
}

impl ResolvedValue {
    pub fn from_candidate(candidate: &CandidateValue, license_shard_class: LicenseShardClass) -> Self {
        ResolvedValue {
            region_id: candidate.region_id,
            region_iso3: candidate.region_iso3.clone(),
            statistic_kind: candidate.statistic_kind,
            period: candidate.period,
            value: candidate.value,
            data_status: candidate.data_status,
            data_source_kind: candidate.data_source_kind,
            data_source_revision: candidate.data_source_revision.clone(),
            license_shard_class,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileReference {
    pub path: PathBuf,
    pub byte_count: u64,
}

#[derive(Debug, Clone)]
pub struct StatisticShard<F> {
    pub statistic_kind: StatisticKind,
    pub license_shard_class: LicenseShardClass,
    pub file: F,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StatisticShardKey {
    pub statistic_kind: StatisticKind,
    pub license_shard_class: LicenseShardClass,
}

impl StatisticShardKey {
    pub fn from_resolved(resolved: &ResolvedValue) -> Self {
        StatisticShardKey {
            statistic_kind: resolved.statistic_kind,
            license_shard_class: resolved.license_shard_class,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Artifacts {
    pub shards: Vec<StatisticShard<Hashed<FileReference>>>,
    pub geometry: Hashed<FileReference>,
    pub manifest: Hashed<FileReference>,
}

#[derive(Debug, Clone)]
pub struct ArtifactBuildReport {
    pub output_dir: PathBuf,
    pub version_label: String,
    pub artifacts: Artifacts,
}

#[derive(Debug, Clone)]
pub struct ArtifactVersion {
    pub id: Uuid,
    pub version_label: String,
    pub artifact_created: DateTime<Utc>,
    pub manifest_sha256: String,
    pub manifest_url: String,
    pub data_source_revisions_jsonb: serde_json::Value,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtifactVersionEntity {
    pub id: Uuid,
    pub version_label: String,
    pub artifact_created: DateTime<Utc>,
    pub manifest_sha256: String,
    pub manifest_url: String,
    pub data_source_revisions_jsonb: serde_json::Value,
    pub notes: Option<String>,
}

impl From<ArtifactVersionEntity> for ArtifactVersion {
    fn from(entity: ArtifactVersionEntity) -> Self {
        ArtifactVersion {
            id: entity.id,
            version_label: entity.version_label,
            artifact_created: entity.artifact_created,
            manifest_sha256: entity.manifest_sha256,
            manifest_url: entity.manifest_url,
            data_source_revisions_jsonb: entity.data_source_revisions_jsonb,
            notes: entity.notes,
        }
    }
}
