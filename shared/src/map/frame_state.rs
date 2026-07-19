use chrono::NaiveDate;

use crate::canonical::StatisticKind;

/// A region's `code` slug (e.g. `"usa"`, `"germany"`), wrapping the canonical `region.code`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionCode(pub String);

/// The per-frame inputs the renderer needs beyond the `Viewport`.
#[derive(Debug, Clone)]
pub struct FrameState {
    pub active_statistic: StatisticKind,
    /// The year scrubber's current position; a single date, not a range, hence `_start` (a period
    /// is a [start, end] pair).
    pub active_period_start: NaiveDate,
    pub selected_region: Option<RegionCode>,
    pub hovered_region: Option<RegionCode>,
}
