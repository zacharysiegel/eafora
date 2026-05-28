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
    pub code: String,
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
    pub data_status: String,
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
}

/// Enumerates the `statistic_value.data_status` values per the schema's
/// `comment on column`: final | provisional | preliminary | projection |
/// imputed | interpolated.
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

/// Enumerates the `data_source.license_class` values per the schema's
/// `comment on column`: public_domain | attribution | attribution_sa |
/// noncommercial. Parsed from the column on read; written via `as_str`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LicenseClass {
    PublicDomain,
    Attribution,
    AttributionSa,
    NonCommercial,
}

impl LicenseClass {
    pub fn as_str(self) -> &'static str {
        match self {
            LicenseClass::PublicDomain => "public_domain",
            LicenseClass::Attribution => "attribution",
            LicenseClass::AttributionSa => "attribution_sa",
            LicenseClass::NonCommercial => "noncommercial",
        }
    }

    pub fn parse_str(value: &str) -> Result<LicenseClass, AppError> {
        match value {
            "public_domain" => Ok(LicenseClass::PublicDomain),
            "attribution" => Ok(LicenseClass::Attribution),
            "attribution_sa" => Ok(LicenseClass::AttributionSa),
            "noncommercial" => Ok(LicenseClass::NonCommercial),
            other => Err(AppError::from(format!("LicenseClass::parse_str: unknown value {:?}", other))),
        }
    }
}
