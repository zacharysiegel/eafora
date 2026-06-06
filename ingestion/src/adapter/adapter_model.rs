use chrono::{Datelike, NaiveDate};
use uuid::Uuid;

use crate::canonical::canonical_model::DataStatus;
use crate::error::AppError;

#[derive(Debug, Clone, Copy)]
pub struct AdapterOptions {
    pub force_full_refetch: bool,
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

    #[allow(dead_code)]
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

#[derive(Debug)]
pub struct NormalizedStatisticValue {
    pub region_id: Uuid,
    pub statistic_id: Uuid,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: DataStatus,
}

#[derive(Debug)]
pub enum NormalizeOutcome {
    Normalized(NormalizedStatisticValue),
    Warned(IngestWarning),
}

#[derive(Debug, Clone)]
pub struct IngestWarning {
    pub kind: IngestWarningKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestWarningKind {
    UnknownCountry,
    NotApplicableValue,
    UnparsableRow,
}
