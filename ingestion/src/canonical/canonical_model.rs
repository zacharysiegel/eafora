use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use crate::error::AppError;

pub struct Region {
    pub id: Uuid,
    pub code: String,
    pub name_en: String,
    pub level: String,
    pub parent_region_id: Option<Uuid>,
    pub m49_code: Option<String>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

pub struct RegionEntity {
    pub id: Uuid,
    pub code: String,
    pub name_en: String,
    pub level: String,
    pub parent_region_id: Option<Uuid>,
    pub m49_code: Option<String>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

impl From<RegionEntity> for Region {
    fn from(entity: RegionEntity) -> Self {
        Region {
            id: entity.id,
            code: entity.code,
            name_en: entity.name_en,
            level: entity.level,
            parent_region_id: entity.parent_region_id,
            m49_code: entity.m49_code,
            created: entity.created,
            modified: entity.modified,
        }
    }
}

pub struct Country {
    pub region_id: Uuid,
    pub iso3: String,
    pub iso2: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub deleted: Option<DateTime<Utc>>,
}

pub struct CountryEntity {
    pub region_id: Uuid,
    pub iso3: String,
    pub iso2: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub deleted: Option<DateTime<Utc>>,
}

impl From<CountryEntity> for Country {
    fn from(entity: CountryEntity) -> Self {
        Country {
            region_id: entity.region_id,
            iso3: entity.iso3,
            iso2: entity.iso2,
            created: entity.created,
            modified: entity.modified,
            deleted: entity.deleted,
        }
    }
}

pub struct Statistic {
    pub id: Uuid,
    pub code: String,
    pub name_en: String,
    pub description: String,
    pub units: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

pub struct StatisticEntity {
    pub id: Uuid,
    pub code: String,
    pub name_en: String,
    pub description: String,
    pub units: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

impl From<StatisticEntity> for Statistic {
    fn from(entity: StatisticEntity) -> Self {
        Statistic {
            id: entity.id,
            code: entity.code,
            name_en: entity.name_en,
            description: entity.description,
            units: entity.units,
            created: entity.created,
            modified: entity.modified,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DataSource {
    pub id: Uuid,
    pub code: DataSourceKind,
    pub name_en: String,
    pub homepage_url: String,
    pub license_class: LicenseClass,
    pub license_name: String,
    pub license_url: String,
    pub attribution_text: String,
    pub preference_rank: i32,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DataSourceEntity {
    pub id: Uuid,
    pub code: String,
    pub name_en: String,
    pub homepage_url: String,
    pub license_class: String,
    pub license_name: String,
    pub license_url: String,
    pub attribution_text: String,
    pub preference_rank: i32,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

impl TryFrom<DataSourceEntity> for DataSource {
    type Error = AppError;

    fn try_from(entity: DataSourceEntity) -> Result<Self, Self::Error> {
        Ok(DataSource {
            id: entity.id,
            code: DataSourceKind::try_from(entity.code.as_str())?,
            name_en: entity.name_en,
            homepage_url: entity.homepage_url,
            license_class: LicenseClass::try_from(entity.license_class.as_str())?,
            license_name: entity.license_name,
            license_url: entity.license_url,
            attribution_text: entity.attribution_text,
            preference_rank: entity.preference_rank,
            created: entity.created,
            modified: entity.modified,
        })
    }
}

#[derive(Debug, Clone)]
pub struct StatisticValue {
    pub id: Uuid,
    pub region_id: Uuid,
    pub statistic_id: Uuid,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub value: f64,
    pub data_source_id: Uuid,
    pub data_source_publication_id: Uuid,
    pub data_status: DataStatus,
    pub superseded: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StatisticValueEntity {
    pub id: Uuid,
    pub region_id: Uuid,
    pub statistic_id: Uuid,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub value: f64,
    pub data_source_id: Uuid,
    pub data_source_publication_id: Uuid,
    pub data_status: String,
    pub superseded: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

impl TryFrom<StatisticValueEntity> for StatisticValue {
    type Error = AppError;

    fn try_from(entity: StatisticValueEntity) -> Result<Self, Self::Error> {
        Ok(StatisticValue {
            id: entity.id,
            region_id: entity.region_id,
            statistic_id: entity.statistic_id,
            period_start: entity.period_start,
            period_end: entity.period_end,
            value: entity.value,
            data_source_id: entity.data_source_id,
            data_source_publication_id: entity.data_source_publication_id,
            data_status: DataStatus::try_from(entity.data_status.as_str())?,
            superseded: entity.superseded,
            created: entity.created,
            modified: entity.modified,
        })
    }
}

/// Enumerates the `statistic.code` values seeded in the canonical store.
/// New statistics get a variant here AND a seed migration row; the two
/// stay in sync by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatisticKind {
    Tfr,
}

impl StatisticKind {
    pub fn code(self) -> &'static str {
        match self {
            StatisticKind::Tfr => "tfr",
        }
    }
}

impl TryFrom<&str> for StatisticKind {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "tfr" => Ok(StatisticKind::Tfr),
            other => Err(AppError::from(format!("StatisticKind::try_from: unknown value {:?}", other))),
        }
    }
}

/// Enumerates the `data_source.code` values seeded in the canonical store.
/// New sources get a variant here AND a seed migration row; the two stay
/// in sync by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataSourceKind {
    WorldBankWDI,
}

impl DataSourceKind {
    pub fn code(self) -> &'static str {
        match self {
            DataSourceKind::WorldBankWDI => "wb_wdi",
        }
    }
}

impl TryFrom<&str> for DataSourceKind {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "wb_wdi" => Ok(DataSourceKind::WorldBankWDI),
            other => Err(AppError::from(format!("DataSourceKind::try_from: unknown value {:?}", other))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataStatus {
    Final,
    Provisional,
    Preliminary,
    Projection,
    Imputed,
    Interpolated,
}

impl DataStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DataStatus::Final => "final",
            DataStatus::Provisional => "provisional",
            DataStatus::Preliminary => "preliminary",
            DataStatus::Projection => "projection",
            DataStatus::Imputed => "imputed",
            DataStatus::Interpolated => "interpolated",
        }
    }
}

impl TryFrom<&str> for DataStatus {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "final" => Ok(DataStatus::Final),
            "provisional" => Ok(DataStatus::Provisional),
            "preliminary" => Ok(DataStatus::Preliminary),
            "projection" => Ok(DataStatus::Projection),
            "imputed" => Ok(DataStatus::Imputed),
            "interpolated" => Ok(DataStatus::Interpolated),
            other => Err(AppError::from(format!("DataStatus::try_from: unknown value {:?}", other))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LicenseClass {
    PublicDomain,
    Attribution,
    AttributionShareAlike,
    NonCommercial,
}

impl LicenseClass {
    pub fn as_str(self) -> &'static str {
        match self {
            LicenseClass::PublicDomain => "public_domain",
            LicenseClass::Attribution => "attribution",
            LicenseClass::AttributionShareAlike => "attribution_share_alike",
            LicenseClass::NonCommercial => "noncommercial",
        }
    }
}

impl TryFrom<&str> for LicenseClass {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "public_domain" => Ok(LicenseClass::PublicDomain),
            "attribution" => Ok(LicenseClass::Attribution),
            "attribution_share_alike" => Ok(LicenseClass::AttributionShareAlike),
            "noncommercial" => Ok(LicenseClass::NonCommercial),
            other => Err(AppError::from(format!("LicenseClass::try_from: unknown value {:?}", other))),
        }
    }
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

impl TryFrom<&str> for LicenseShardClass {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "base" => Ok(LicenseShardClass::Base),
            "share_alike" => Ok(LicenseShardClass::ShareAlike),
            "noncommercial" => Ok(LicenseShardClass::NonCommercial),
            other => Err(AppError::from(format!("LicenseShardClass::try_from: unknown value {:?}", other))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceChoice {
    pub id: Uuid,
    pub region_id: Option<Uuid>,
    pub statistic_id: Uuid,
    pub license_shard_class: LicenseShardClass,
    pub data_source_id: Uuid,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SourceChoiceEntity {
    pub id: Uuid,
    pub region_id: Option<Uuid>,
    pub statistic_id: Uuid,
    pub license_shard_class: String,
    pub data_source_id: Uuid,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

impl TryFrom<SourceChoiceEntity> for SourceChoice {
    type Error = AppError;

    fn try_from(entity: SourceChoiceEntity) -> Result<Self, Self::Error> {
        Ok(SourceChoice {
            id: entity.id,
            region_id: entity.region_id,
            statistic_id: entity.statistic_id,
            license_shard_class: LicenseShardClass::try_from(entity.license_shard_class.as_str())?,
            data_source_id: entity.data_source_id,
            created: entity.created,
            modified: entity.modified,
        })
    }
}
