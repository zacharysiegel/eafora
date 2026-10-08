use chrono::NaiveDate;

use crate::artifact::Bundle;
use crate::canonical::StatisticKind;
use crate::sqlite::shard_db::ShardValues;

/// Minimum coverage for the default period, as a proportion of the best-covered period's.
const MINIMUM_DEFAULT_COVERAGE_PROPORTION: f64 = 0.8;

/// A region's `code` slug (e.g. `"usa"`, `"germany"`), wrapping the canonical `region.code`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionCode(pub String);

/// The per-frame inputs the renderer needs beyond the `Viewport`.
#[derive(Debug, Clone)]
pub struct FrameState {
    pub active_statistic: StatisticKind,
    /// A single date, not a period; a period is a [start, end] pair.
    pub active_period_start: NaiveDate,
    pub selected_region: Option<RegionCode>,
    pub hovered_region: Option<RegionCode>,
    /// When false, a hovered region keeps its outline but does not lift.
    pub hover_lift_enabled: bool,
}

impl FrameState {
    /// Falls back to the Unix epoch when the default statistic's shard is missing.
    pub fn initial(bundle: &Bundle, hover_lift_enabled: bool) -> FrameState {
        let active_statistic: StatisticKind = StatisticKind::Tfr;
        let active_period_start: NaiveDate = default_period_start(bundle, active_statistic)
            .unwrap_or_else(|| NaiveDate::from_epoch_days(0).expect("day 0 is the Unix epoch"));

        FrameState {
            active_statistic,
            active_period_start,
            selected_region: None,
            hovered_region: None,
            hover_lift_enabled,
        }
    }

    /// Leaves the period unchanged when the statistic has no shard to take a default from.
    pub fn reset_active_period_if_uncovered(&mut self, bundle: &Bundle) {
        let Some((earliest, latest)) = bundle
            .shard_values_for(self.active_statistic)
            .and_then(|shard_values| shard_values.period_range())
        else {
            return;
        };

        let covers_active_period: bool = self.active_period_start >= earliest && self.active_period_start <= latest;
        if covers_active_period {
            return;
        }

        if let Some(period_start) = default_period_start(bundle, self.active_statistic) {
            self.active_period_start = period_start;
        }
    }
}

/// Read through `Bundle::shard_values_for` so the seeded period and the coloured shard never disagree about
/// which license class won.
fn default_period_start(bundle: &Bundle, statistic: StatisticKind) -> Option<NaiveDate> {
    let shard_values: &ShardValues = bundle.shard_values_for(statistic)?;

    shard_values.newest_well_covered_period_start(MINIMUM_DEFAULT_COVERAGE_PROPORTION)
}
