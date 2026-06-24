//! Generic fixture builders for adapter-output types. Tests that need a
//! known-shape `NormalizedStatisticValue` without going through a real
//! adapter's `normalize` step build one with `new_normalized_statistic_value`.
#![allow(dead_code)]

use uuid::Uuid;

use ingestion::adapter::NormalizedStatisticValue;
use shared::canonical::canonical_model::{DataStatus, NaiveDatePeriod};

/// Builds a `NormalizedStatisticValue` for the given region + statistic +
/// year + value, defaulted to `DataStatus::Final`. Used by record-* tests.
pub fn new_normalized_statistic_value(
    region_id: Uuid,
    statistic_id: Uuid,
    year: i32,
    value: f64,
) -> NormalizedStatisticValue {
    NormalizedStatisticValue {
        region_id,
        statistic_id,
        period: NaiveDatePeriod::from_year(year).expect("valid year"),
        value,
        data_status: DataStatus::Final,
    }
}
