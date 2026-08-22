//! Selection rules:
//!
//! 1. Group candidate values by `(region, statistic, license_shard_class, period)` cell.
//! 2. Emit the highest-priority source's value for that cell. `data_source.preference_rank` orders the
//!    sources; lower wins, ties broken by source so a rebuild of unchanged data produces the same shard.
//! 3. A period only a lower-priority source covers still emits, so a preferred source's narrower coverage
//!    does not truncate a region's series.

use std::collections::BTreeMap;

use uuid::Uuid;

use shared::canonical::canonical_model::{LicenseShardClass, NaiveDatePeriod, StatisticKind};

use crate::artifact::artifact_model::{CandidateValue, ResolvedValue};
use crate::error::AppError;

/// One published cell: a statistic's value for one region over one period, within one license-shard
/// partition. Exactly one source supplies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CellKey {
    pub region_id: Uuid,
    pub statistic_kind: StatisticKind,
    pub license_shard_class: LicenseShardClass,
    pub period: NaiveDatePeriod,
}

impl CellKey {
    pub fn from_candidate(candidate: &CandidateValue) -> Self {
        CellKey {
            region_id: candidate.region_id,
            statistic_kind: candidate.statistic_kind,
            license_shard_class: LicenseShardClass::from_license_class(candidate.license_class),
            period: candidate.period,
        }
    }
}

/// Reduce candidate values to one per cell, keeping the highest-priority source that covers it.
pub fn resolve_candidates(candidates: Vec<CandidateValue>) -> Result<Vec<ResolvedValue>, AppError> {
    let cells: BTreeMap<CellKey, Vec<CandidateValue>> = group_candidates(candidates);
    let mut resolved_values: Vec<ResolvedValue> = Vec::with_capacity(cells.len());

    for (cell_key, cell_candidates) in cells {
        let preferred: &CandidateValue = highest_priority(&cell_candidates)
            .ok_or_else(|| AppError::from(format!("a grouped cell holds no candidate; [cell={cell_key:?}]")))?;

        resolved_values.push(ResolvedValue::from_candidate(preferred, cell_key.license_shard_class));
    }

    Ok(resolved_values)
}

/// `data_source_kind` breaks a tie so two sources sharing a rank resolve the same way on every rebuild.
fn highest_priority(candidates: &[CandidateValue]) -> Option<&CandidateValue> {
    candidates
        .iter()
        .min_by_key(|candidate| (candidate.data_source_preference_rank, candidate.data_source_kind))
}

fn group_candidates(candidates: Vec<CandidateValue>) -> BTreeMap<CellKey, Vec<CandidateValue>> {
    let mut cells: BTreeMap<CellKey, Vec<CandidateValue>> = BTreeMap::new();

    for candidate in candidates {
        cells
            .entry(CellKey::from_candidate(&candidate))
            .or_default()
            .push(candidate);
    }

    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    use shared::canonical::canonical_model::{DataSourceKind, DataStatus, LicenseClass};

    const REGION_USA: u128 = 1;

    const WORLD_BANK_RANK: i32 = 100;
    const HFD_RANK: i32 = 50;

    fn make_candidate(
        data_source_kind: DataSourceKind,
        data_source_preference_rank: i32,
        year: i32,
        value: f64,
    ) -> CandidateValue {
        CandidateValue {
            region_id: Uuid::from_u128(REGION_USA),
            region_code: "usa".to_string(),
            statistic_kind: StatisticKind::Tfr,
            period: NaiveDatePeriod::from_year(year).unwrap(),
            value,
            data_status: DataStatus::Final,
            data_source_kind,
            data_source_preference_rank,
            data_source_revision: "rev1".to_string(),
            license_class: LicenseClass::Attribution,
        }
    }

    fn world_bank(year: i32, value: f64) -> CandidateValue {
        make_candidate(DataSourceKind::WorldBankWDI, WORLD_BANK_RANK, year, value)
    }

    fn human_fertility_database(year: i32, value: f64) -> CandidateValue {
        make_candidate(DataSourceKind::HumanFertilityDatabase, HFD_RANK, year, value)
    }

    fn value_for(resolved_values: &[ResolvedValue], year: i32) -> Option<(f64, DataSourceKind)> {
        resolved_values
            .iter()
            .find(|resolved| resolved.period.start.format("%Y").to_string() == year.to_string())
            .map(|resolved| (resolved.value, resolved.data_source_kind))
    }

    #[test]
    fn resolve_candidates_keeps_a_single_source_for_every_period() {
        let candidates: Vec<CandidateValue> = vec![world_bank(2022, 1.66), world_bank(2023, 1.62)];

        let resolved_values: Vec<ResolvedValue> = resolve_candidates(candidates).unwrap();

        assert_eq!(resolved_values.len(), 2);
    }

    #[test]
    fn resolve_candidates_prefers_the_higher_priority_source_for_a_contested_cell() {
        let candidates: Vec<CandidateValue> = vec![world_bank(2022, 1.66), human_fertility_database(2022, 1.70)];

        let resolved_values: Vec<ResolvedValue> = resolve_candidates(candidates).unwrap();

        assert_eq!(resolved_values.len(), 1);
        assert_eq!(
            value_for(&resolved_values, 2022),
            Some((1.70, DataSourceKind::HumanFertilityDatabase)),
        );
    }

    /// The behaviour the replaced rule forbade: a period the preferred source does not reach still emits.
    #[test]
    fn resolve_candidates_fills_a_period_the_preferred_source_does_not_cover() {
        let candidates: Vec<CandidateValue> = vec![
            human_fertility_database(2022, 1.70),
            world_bank(2022, 1.66),
            world_bank(2023, 1.62),
        ];

        let resolved_values: Vec<ResolvedValue> = resolve_candidates(candidates).unwrap();

        assert_eq!(resolved_values.len(), 2);
        assert_eq!(
            value_for(&resolved_values, 2022),
            Some((1.70, DataSourceKind::HumanFertilityDatabase)),
        );
        assert_eq!(value_for(&resolved_values, 2023), Some((1.62, DataSourceKind::WorldBankWDI)));
    }

    #[test]
    fn resolve_candidates_resolves_an_equal_rank_the_same_way_whichever_order_it_reads() {
        let ascending: Vec<CandidateValue> = vec![
            make_candidate(DataSourceKind::WorldBankWDI, HFD_RANK, 2022, 1.66),
            make_candidate(DataSourceKind::HumanFertilityDatabase, HFD_RANK, 2022, 1.70),
        ];
        let descending: Vec<CandidateValue> = vec![
            make_candidate(DataSourceKind::HumanFertilityDatabase, HFD_RANK, 2022, 1.70),
            make_candidate(DataSourceKind::WorldBankWDI, HFD_RANK, 2022, 1.66),
        ];

        let from_ascending: Vec<ResolvedValue> = resolve_candidates(ascending).unwrap();
        let from_descending: Vec<ResolvedValue> = resolve_candidates(descending).unwrap();

        assert_eq!(
            value_for(&from_ascending, 2022),
            value_for(&from_descending, 2022),
        );
    }

    #[test]
    fn resolve_candidates_emits_nothing_for_no_candidates() {
        let resolved_values: Vec<ResolvedValue> = resolve_candidates(Vec::new()).unwrap();

        assert!(resolved_values.is_empty());
    }
}
