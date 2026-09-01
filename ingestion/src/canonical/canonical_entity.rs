use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use shared::canonical::canonical_model::{
    Country, DataSource, DataSourceKind, DataStatus, LicenseClass, Region, Statistic,
};

use crate::error::AppError;

pub struct RegionEntity {
    pub id: Uuid,
    pub code: String,
    pub name_en: String,
    pub level: String,
    pub parent_region_id: Option<Uuid>,
    pub m49_code: Option<String>,
    pub nuts_code: Option<String>,
    pub iso_3166_2: Option<String>,
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
            nuts_code: entity.nuts_code,
            iso_3166_2: entity.iso_3166_2,
            created: entity.created,
            modified: entity.modified,
        }
    }
}

pub struct CountryEntity {
    pub region_id: Uuid,
    pub iso3: String,
    pub iso2: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

impl From<CountryEntity> for Country {
    fn from(entity: CountryEntity) -> Self {
        Country {
            region_id: entity.region_id,
            iso3: entity.iso3,
            iso2: entity.iso2,
            created: entity.created,
            modified: entity.modified,
        }
    }
}

pub struct StatisticEntity {
    pub id: Uuid,
    pub code: String,
    pub name_en: String,
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
            units: entity.units,
            created: entity.created,
            modified: entity.modified,
        }
    }
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
            kind: DataSourceKind::try_from(entity.code.as_str())?,
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

