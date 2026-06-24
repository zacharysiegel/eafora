use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

/// Generate `Serialize` / `Deserialize` for a string-coded enum by delegating to
/// its existing `$accessor()` (the canonical-store string code) and `TryFrom<&str>`.
/// Keeps the code strings defined once, on the enum's own impls.
macro_rules! impl_code_serde {
    ($kind:ty, $accessor:ident) => {
        impl Serialize for $kind {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.$accessor())
            }
        }

        impl<'de> Deserialize<'de> for $kind {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<$kind, D::Error> {
                let code: String = String::deserialize(deserializer)?;
                <$kind>::try_from(code.as_str()).map_err(serde::de::Error::custom)
            }
        }
    };
}

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

#[derive(Debug, Clone)]
pub struct DataSource {
    pub id: Uuid,
    pub kind: DataSourceKind,
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

/// Enumerates the `statistic.code` values seeded in the canonical store.
/// New statistics get a variant here AND a seed migration row; the two
/// stay in sync by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatisticKind {
    Tfr,
    #[cfg(any(test, feature = "testing"))]
    TestAlpha,
}

impl StatisticKind {
    pub fn code(self) -> &'static str {
        match self {
            StatisticKind::Tfr => "tfr",
            #[cfg(any(test, feature = "testing"))]
            StatisticKind::TestAlpha => "_test_alpha",
        }
    }
}

impl TryFrom<&str> for StatisticKind {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "tfr" => Ok(StatisticKind::Tfr),
            #[cfg(any(test, feature = "testing"))]
            "_test_alpha" => Ok(StatisticKind::TestAlpha),
            other => Err(AppError::from(format!("unknown value {:?}", other))),
        }
    }
}

impl_code_serde!(StatisticKind, code);

/// Enumerates the `data_source.code` values seeded in the canonical store.
/// New sources get a variant here AND a seed migration row; the two stay
/// in sync by convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataSourceKind {
    WorldBankWDI,
    #[cfg(any(test, feature = "testing"))]
    TestAlpha,
    #[cfg(any(test, feature = "testing"))]
    TestBeta,
}

impl DataSourceKind {
    pub fn code(self) -> &'static str {
        match self {
            DataSourceKind::WorldBankWDI => "wb_wdi",
            #[cfg(any(test, feature = "testing"))]
            DataSourceKind::TestAlpha => "_test_alpha",
            #[cfg(any(test, feature = "testing"))]
            DataSourceKind::TestBeta => "_test_beta",
        }
    }
}

impl TryFrom<&str> for DataSourceKind {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "wb_wdi" => Ok(DataSourceKind::WorldBankWDI),
            #[cfg(any(test, feature = "testing"))]
            "_test_alpha" => Ok(DataSourceKind::TestAlpha),
            #[cfg(any(test, feature = "testing"))]
            "_test_beta" => Ok(DataSourceKind::TestBeta),
            other => Err(AppError::from(format!("unknown value {:?}", other))),
        }
    }
}

impl_code_serde!(DataSourceKind, code);

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
            other => Err(AppError::from(format!("unknown value {:?}", other))),
        }
    }
}

impl_code_serde!(DataStatus, as_str);

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
            other => Err(AppError::from(format!("unknown value {:?}", other))),
        }
    }
}

impl_code_serde!(LicenseClass, as_str);

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
            other => Err(AppError::from(format!("unknown value {:?}", other))),
        }
    }
}

impl_code_serde!(LicenseShardClass, as_str);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRevision {
    pub revision: String,
    pub published: Option<DateTime<Utc>>,
    pub fetched: DateTime<Utc>,
}

/// Half-open `[start, end)` interval matching the canonical store's
/// `period_start` / `period_end` columns. Always paired so the two
/// `NaiveDate` arguments can't get inverted at construction sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NaiveDatePeriod {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl NaiveDatePeriod {
    pub fn from_year(year: i32) -> Result<NaiveDatePeriod, AppError> {
        let start: NaiveDate = NaiveDate::from_ymd_opt(year, 1, 1)
            .ok_or_else(|| AppError::from(format!("invalid year {}", year)))?;
        let end: NaiveDate = NaiveDate::from_ymd_opt(year + 1, 1, 1).ok_or_else(|| {
            AppError::from(format!("invalid year+1 from {}", year))
        })?;

        Ok(NaiveDatePeriod { start, end })
    }

    pub fn to_year(&self) -> Option<i32> {
        if self.start.month() != 1 || self.start.day() != 1 {
            return None;
        }

        let expected_end: NaiveDate = NaiveDate::from_ymd_opt(self.start.year() + 1, 1, 1)?;
        if self.end != expected_end {
            return None;
        }

        Some(self.start.year())
    }
}
