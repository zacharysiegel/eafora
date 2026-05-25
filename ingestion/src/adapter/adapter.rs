//! Generic helpers shared across source adapters. Functions here have no
//! per-source state — they're pure utilities that multiple adapters reach
//! for during parse/normalize. The criterion: if the function would be
//! identical in `eurostat_client.rs` and `hfd_client.rs`, it belongs here.

use chrono::NaiveDate;

use crate::error::AppError;

/// Maps an integer year to the half-open `[period_start, period_end)`
/// interval the canonical store stores in `statistic_value`. A 2024 row
/// becomes `[2024-01-01, 2025-01-01)`. Used by adapters whose sources
/// publish annual values.
pub fn year_to_period(year: i32) -> Result<(NaiveDate, NaiveDate), AppError> {
    let period_start: NaiveDate = NaiveDate::from_ymd_opt(year, 1, 1)
        .ok_or_else(|| AppError::from(format!("year_to_period: invalid year {}", year)))?;
    let period_end: NaiveDate = NaiveDate::from_ymd_opt(year + 1, 1, 1)
        .ok_or_else(|| AppError::from(format!("year_to_period: invalid year+1 from {}", year)))?;
    Ok((period_start, period_end))
}
