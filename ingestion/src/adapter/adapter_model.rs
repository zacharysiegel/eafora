//! Cross-adapter types: knobs every adapter accepts, the canonical
//! normalized-row form they emit, the per-row outcome `normalize` produces,
//! and the warnings adapters attach to rows they couldn't normalize.
//! Per-source intermediate types (response shapes, parser outputs) live in
//! `<source>_model.rs`. Aggregate ingest-layer types (IngestReport,
//! UpsertOutcome) live in `ingest::ingest_model`.

use chrono::NaiveDate;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, Copy)]
pub struct AdapterOptions {
    pub force_full_refetch: bool,
}

/// Half-open `[start, end)` interval matching the canonical store's
/// `statistic_value.period_start` / `period_end` columns. Always paired —
/// having a struct here prevents the two-NaiveDate-arg-inversion class of
/// bugs and gives us one place to hang constructors like `from_year`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl Period {
    /// Builds the calendar-year period `[YYYY-01-01, YYYY+1-01-01)` for an
    /// integer year. Used by adapters whose sources publish annual values.
    pub fn from_year(year: i32) -> Result<Period, AppError> {
        let start: NaiveDate = NaiveDate::from_ymd_opt(year, 1, 1)
            .ok_or_else(|| AppError::from(format!("Period::from_year: invalid year {}", year)))?;
        let end: NaiveDate = NaiveDate::from_ymd_opt(year + 1, 1, 1).ok_or_else(|| {
            AppError::from(format!("Period::from_year: invalid year+1 from {}", year))
        })?;
        Ok(Period { start, end })
    }
}

#[derive(Debug)]
pub struct NormalizedRow {
    pub region_id: Uuid,
    pub statistic_id: Uuid,
    pub period: Period,
    pub value: f64,
    pub data_status: String,
}

/// Per-row result of an adapter's normalize step. Every adapter accumulates
/// these into `(Vec<NormalizedRow>, Vec<IngestWarning>)` for the ingest
/// layer to consume.
#[derive(Debug)]
pub enum NormalizeOutcome {
    Normalized(NormalizedRow),
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
    NaValue,
    UnparsableRow,
}
