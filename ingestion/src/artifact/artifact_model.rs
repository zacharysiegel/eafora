use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;
use crate::artifact::SeriesKey;
use crate::canonical::canonical_model::{DataSourceKind, DataStatus, LicenseClass, LicenseShardClass, StatisticKind};
use crate::error::AppError;

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

#[derive(Debug, Clone)]
pub struct ShardOutput {
    pub path: PathBuf,
    pub byte_count: u64,
}

#[derive(Debug, Clone)]
pub struct HashedOutputs {
    pub statistic_shards: Vec<HashedStatisticShard>,
    pub geometry_shard: HashedShard,
}

#[derive(Debug, Clone)]
pub struct HashedStatisticShard {
    pub statistic_code: String,
    pub license_shard_class: LicenseShardClass,
    pub shard: HashedShard,
}

#[derive(Debug, Clone)]
pub struct HashedShard {
    pub path: PathBuf,
    pub byte_count: u64,
    pub sha256_hex: String,
}

#[derive(Debug, Clone)]
pub struct LocalArtifactBuild {
    pub output_dir: PathBuf,
    pub version_label: String,
    pub hashed: HashedOutputs,
    pub manifest: HashedShard,
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
