use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;
use crate::canonical::canonical_model::{DataSourceKind, DataStatus, LicenseClass};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct CandidateValue {
    pub region_id: Uuid,
    pub region_iso3: String,
    pub statistic_id: Uuid,
    pub statistic_code: String,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: DataStatus,
    pub data_source_id: Uuid,
    pub data_source_kind: DataSourceKind,
    pub data_source_revision: String,
    pub data_source_preference_rank: i32,
    pub license_class: LicenseClass,
}

#[derive(Debug, Clone)]
pub struct CandidateValueEntity {
    pub region_id: Uuid,
    pub region_iso3: String,
    pub statistic_id: Uuid,
    pub statistic_code: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub value: f64,
    pub data_status: String,
    pub data_source_id: Uuid,
    pub data_source_code: String,
    pub data_source_revision: String,
    pub data_source_preference_rank: i32,
    pub license_class: String,
}

impl TryFrom<CandidateValueEntity> for CandidateValue {
    type Error = AppError;

    fn try_from(entity: CandidateValueEntity) -> Result<Self, Self::Error> {
        Ok(CandidateValue {
            region_id: entity.region_id,
            region_iso3: entity.region_iso3,
            statistic_id: entity.statistic_id,
            statistic_code: entity.statistic_code,
            period: NaiveDatePeriod {
                start: entity.period_start,
                end: entity.period_end,
            },
            value: entity.value,
            data_status: DataStatus::from_str(&entity.data_status)?,
            data_source_id: entity.data_source_id,
            data_source_kind: DataSourceKind::from_str(&entity.data_source_code)?,
            data_source_revision: entity.data_source_revision,
            data_source_preference_rank: entity.data_source_preference_rank,
            license_class: LicenseClass::from_str(&entity.license_class)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CountryNameProjection {
    pub iso3: String,
    pub name_en: String,
}

#[derive(Debug, Clone)]
pub struct MergedValue {
    pub region_id: Uuid,
    pub region_iso3: String,
    pub statistic_id: Uuid,
    pub statistic_code: String,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: DataStatus,
    pub data_source_kind: DataSourceKind,
    pub data_source_revision: String,
    pub license_shard_class: LicenseShardClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LicenseShardClass {
    Base,
    ShareAlike,
    NonCommercial,
}

impl LicenseShardClass {
    pub fn from_license_class(license_class: LicenseClass) -> LicenseShardClass {
        match license_class {
            LicenseClass::PublicDomain | LicenseClass::Attribution => LicenseShardClass::Base,
            LicenseClass::AttributionShareAlike => LicenseShardClass::ShareAlike,
            LicenseClass::NonCommercial => LicenseShardClass::NonCommercial,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LicenseShardClass::Base => "base",
            LicenseShardClass::ShareAlike => "share_alike",
            LicenseShardClass::NonCommercial => "noncommercial",
        }
    }
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
    pub data_source_versions_jsonb: serde_json::Value,
    pub notes: Option<String>,
}
