use chrono::{DateTime, Utc};
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

pub struct Country {
    pub region_id: Uuid,
    pub iso3: String,
    pub iso2: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub deleted: Option<DateTime<Utc>>,
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

pub struct DataSource {
    pub id: Uuid,
    pub code: DataSourceCode,
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

pub struct StatisticValue {
    pub id: Uuid,
    pub region_id: Uuid,
    pub statistic_id: Uuid,
    pub period_start: chrono::NaiveDate,
    pub period_end: chrono::NaiveDate,
    pub value: f64,
    pub data_source_id: Uuid,
    pub data_source_publication_id: Uuid,
    pub data_status: DataStatus,
    pub superseded: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}

/// Enumerates the `statistic.code` values seeded in the canonical store.
/// New statistics get a variant here AND a seed migration row; the two
/// stay in sync by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatisticCode {
    Tfr,
}

impl StatisticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            StatisticCode::Tfr => "tfr",
        }
    }

    pub fn parse_str(value: &str) -> Result<StatisticCode, AppError> {
        match value {
            "tfr" => Ok(StatisticCode::Tfr),
            other => Err(AppError::from(format!("StatisticCode::parse_str: unknown value {:?}", other))),
        }
    }
}

/// Enumerates the `data_source.code` values seeded in the canonical store.
/// New sources get a variant here AND a seed migration row; the two stay
/// in sync by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataSourceCode {
    WorldBankWDI,
}

impl DataSourceCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DataSourceCode::WorldBankWDI => "wb_wdi",
        }
    }

    pub fn parse_str(value: &str) -> Result<DataSourceCode, AppError> {
        match value {
            "wb_wdi" => Ok(DataSourceCode::WorldBankWDI),
            other => Err(AppError::from(format!("DataSourceCode::parse_str: unknown value {:?}", other))),
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

    pub fn parse_str(value: &str) -> Result<DataStatus, AppError> {
        match value {
            "final" => Ok(DataStatus::Final),
            "provisional" => Ok(DataStatus::Provisional),
            "preliminary" => Ok(DataStatus::Preliminary),
            "projection" => Ok(DataStatus::Projection),
            "imputed" => Ok(DataStatus::Imputed),
            "interpolated" => Ok(DataStatus::Interpolated),
            other => Err(AppError::from(format!("DataStatus::parse_str: unknown value {:?}", other))),
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

    pub fn parse_str(value: &str) -> Result<LicenseClass, AppError> {
        match value {
            "public_domain" => Ok(LicenseClass::PublicDomain),
            "attribution" => Ok(LicenseClass::Attribution),
            "attribution_share_alike" => Ok(LicenseClass::AttributionShareAlike),
            "noncommercial" => Ok(LicenseClass::NonCommercial),
            other => Err(AppError::from(format!("LicenseClass::parse_str: unknown value {:?}", other))),
        }
    }
}
