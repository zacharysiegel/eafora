//! Source choice resolution.
//!
//! Per (region, statistic, license_shard_class), pick one data source for the
//! cell's full time series, based on the `source_choice` table:
//!
//! 1. If a per-region override row exists, use that data source.
//! 2. Else use the global default (region_id NULL) for the (statistic, license).
//! 3. If neither exists, error: the editorial config is incomplete.
//!
//! Then per period: emit the chosen source's value. If the chosen source has
//! no value for a period AND the chosen source differs from the global default
//! AND the global default has a value, fall back to the default's value and
//! log a warning at build time. Else emit nothing.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;
use crate::artifact::artifact_model::{CandidateValue, MergedValue};
use crate::canonical::canonical_model::{LicenseShardClass, SourceChoice};
use crate::error::AppError;

pub fn apply_source_choice(
    candidates: Vec<CandidateValue>,
    source_choices: &[SourceChoice],
) -> Result<Vec<MergedValue>, AppError> {
    let resolver: SourceChoiceResolver = SourceChoiceResolver::build(source_choices);

    let mut groups: BTreeMap<CellKey, Vec<CandidateValue>> = BTreeMap::new();
    for candidate in candidates {
        let license_shard_class: LicenseShardClass = LicenseShardClass::from_license_class(candidate.license_class);
        let key: CellKey = CellKey {
            region_id: candidate.region_id,
            statistic_id: candidate.statistic_id,
            license_shard_class,
        };
        groups.entry(key).or_default().push(candidate);
    }

    let mut merged_values: Vec<MergedValue> = Vec::new();
    for (cell_key, cell_candidates) in groups {
        let chosen_data_source_id: Uuid = resolver
            .resolve_chosen(cell_key.region_id, cell_key.statistic_id, cell_key.license_shard_class)
            .ok_or_else(|| {
                AppError::from(format!(
                    "apply_source_choice: no source_choice configured for region={} statistic={} shard={:?}",
                    cell_key.region_id, cell_key.statistic_id, cell_key.license_shard_class,
                ))
            })?;
        let default_data_source_id: Option<Uuid> =
            resolver.resolve_global_default(cell_key.statistic_id, cell_key.license_shard_class);

        merged_values.extend(
            select_per_period(&cell_candidates, chosen_data_source_id, default_data_source_id, cell_key.license_shard_class)
        );
    }

    Ok(merged_values)
}

fn select_per_period(
    candidates: &[CandidateValue],
    chosen_data_source_id: Uuid,
    default_data_source_id: Option<Uuid>,
    license_shard_class: LicenseShardClass,
) -> Vec<MergedValue> {
    let mut by_period: BTreeMap<NaiveDatePeriod, Vec<&CandidateValue>> = BTreeMap::new();
    for candidate in candidates {
        by_period.entry(candidate.period).or_default().push(candidate);
    }

    let mut emitted: Vec<MergedValue> = Vec::with_capacity(by_period.len());
    for (period, period_candidates) in by_period {
        let chosen: Option<&CandidateValue> = period_candidates
            .iter()
            .copied()
            .find(|candidate| candidate.data_source_id == chosen_data_source_id);

        if let Some(candidate) = chosen {
            emitted.push(merged_value_from(candidate, license_shard_class));
            continue;
        }

        let Some(default_data_source_id) = default_data_source_id else {
            continue;
        };
        if default_data_source_id == chosen_data_source_id {
            continue;
        }
        let Some(candidate) = period_candidates
            .iter()
            .copied()
            .find(|candidate| candidate.data_source_id == default_data_source_id)
        else {
            continue;
        };

        log::warn!(
            "apply_source_choice: region={} statistic={} period=[{},{}) fell back from chosen source {} to global default {}",
            candidate.region_iso3, candidate.statistic_code, period.start, period.end,
            chosen_data_source_id, default_data_source_id,
        );
        emitted.push(merged_value_from(candidate, license_shard_class));
    }

    emitted
}

fn merged_value_from(candidate: &CandidateValue, license_shard_class: LicenseShardClass) -> MergedValue {
    MergedValue {
        region_id: candidate.region_id,
        region_iso3: candidate.region_iso3.clone(),
        statistic_id: candidate.statistic_id,
        statistic_code: candidate.statistic_code.clone(),
        period: candidate.period,
        value: candidate.value,
        data_status: candidate.data_status,
        data_source_kind: candidate.data_source_kind,
        data_source_revision: candidate.data_source_revision.clone(),
        license_shard_class,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CellKey {
    region_id: Uuid,
    statistic_id: Uuid,
    license_shard_class: LicenseShardClass,
}

struct SourceChoiceResolver {
    overrides: BTreeMap<(Uuid, Uuid, LicenseShardClass), Uuid>,
    globals: BTreeMap<(Uuid, LicenseShardClass), Uuid>,
}

impl SourceChoiceResolver {
    fn build(source_choices: &[SourceChoice]) -> Self {
        let mut overrides: BTreeMap<(Uuid, Uuid, LicenseShardClass), Uuid> = BTreeMap::new();
        let mut globals: BTreeMap<(Uuid, LicenseShardClass), Uuid> = BTreeMap::new();
        for choice in source_choices {
            match choice.region_id {
                Some(region_id) => {
                    overrides.insert((region_id, choice.statistic_id, choice.license_shard_class), choice.data_source_id);
                }
                None => {
                    globals.insert((choice.statistic_id, choice.license_shard_class), choice.data_source_id);
                }
            }
        }
        SourceChoiceResolver { overrides, globals }
    }

    fn resolve_chosen(&self, region_id: Uuid, statistic_id: Uuid, license: LicenseShardClass) -> Option<Uuid> {
        self.overrides
            .get(&(region_id, statistic_id, license))
            .copied()
            .or_else(|| self.globals.get(&(statistic_id, license)).copied())
    }

    fn resolve_global_default(&self, statistic_id: Uuid, license: LicenseShardClass) -> Option<Uuid> {
        self.globals.get(&(statistic_id, license)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::{DateTime, Utc};

    use crate::canonical::canonical_model::{DataSourceKind, DataStatus, LicenseClass};

    const STATISTIC_TFR: u128 = 100;
    const REGION_USA: u128 = 1;
    const SOURCE_WB: u128 = 10;
    const SOURCE_CENSUS: u128 = 20;

    fn now() -> DateTime<Utc> { "2026-05-30T00:00:00Z".parse().unwrap() }

    fn period_year(year: i32) -> NaiveDatePeriod { NaiveDatePeriod::from_year(year).unwrap() }

    fn make_candidate(data_source_id: u128, year: i32, value: f64) -> CandidateValue {
        CandidateValue {
            region_id: Uuid::from_u128(REGION_USA),
            region_iso3: "USA".to_string(),
            statistic_id: Uuid::from_u128(STATISTIC_TFR),
            statistic_code: "tfr".to_string(),
            period: period_year(year),
            value,
            data_status: DataStatus::Final,
            data_source_id: Uuid::from_u128(data_source_id),
            data_source_kind: DataSourceKind::WorldBankWDI,
            data_source_revision: "rev1".to_string(),
            license_class: LicenseClass::Attribution,
        }
    }

    fn global_choice(data_source_id: u128) -> SourceChoice {
        SourceChoice {
            id: Uuid::now_v7(),
            region_id: None,
            statistic_id: Uuid::from_u128(STATISTIC_TFR),
            license_shard_class: LicenseShardClass::Base,
            data_source_id: Uuid::from_u128(data_source_id),
            created: now(),
            modified: now(),
        }
    }

    fn override_choice(region: u128, data_source_id: u128) -> SourceChoice {
        SourceChoice {
            id: Uuid::now_v7(),
            region_id: Some(Uuid::from_u128(region)),
            statistic_id: Uuid::from_u128(STATISTIC_TFR),
            license_shard_class: LicenseShardClass::Base,
            data_source_id: Uuid::from_u128(data_source_id),
            created: now(),
            modified: now(),
        }
    }

    #[test]
    fn global_default_with_no_override_uses_default_for_every_period() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(SOURCE_WB, 2022, 1.66),
            make_candidate(SOURCE_WB, 2023, 1.62),
        ];
        let merged: Vec<MergedValue> = apply_source_choice(candidates, &[global_choice(SOURCE_WB)]).unwrap();
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn override_wins_over_global_default_when_override_has_value() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(SOURCE_WB, 2022, 1.66),
            make_candidate(SOURCE_CENSUS, 2022, 1.70),
        ];
        let choices: Vec<SourceChoice> = vec![global_choice(SOURCE_WB), override_choice(REGION_USA, SOURCE_CENSUS)];

        let merged: Vec<MergedValue> = apply_source_choice(candidates, &choices).unwrap();

        assert_eq!(merged.len(), 1);
        assert!((merged[0].value - 1.70).abs() < f64::EPSILON);
    }

    #[test]
    fn override_falls_back_to_default_when_override_has_no_value_for_period() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(SOURCE_WB, 2021, 1.71),     // WB has 2021; Census does not
            make_candidate(SOURCE_CENSUS, 2022, 1.70),
        ];
        let choices: Vec<SourceChoice> = vec![global_choice(SOURCE_WB), override_choice(REGION_USA, SOURCE_CENSUS)];

        let merged: Vec<MergedValue> = apply_source_choice(candidates, &choices).unwrap();

        assert_eq!(merged.len(), 2);
        let value_2021 = merged.iter().find(|m| m.period.start.format("%Y").to_string() == "2021").unwrap().value;
        let value_2022 = merged.iter().find(|m| m.period.start.format("%Y").to_string() == "2022").unwrap().value;
        assert!((value_2021 - 1.71).abs() < f64::EPSILON);
        assert!((value_2022 - 1.70).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_when_neither_chosen_nor_default_has_a_value() {
        let candidates: Vec<CandidateValue> = vec![make_candidate(99, 2022, 1.99)]; // some third source
        let choices: Vec<SourceChoice> = vec![global_choice(SOURCE_WB), override_choice(REGION_USA, SOURCE_CENSUS)];

        let merged: Vec<MergedValue> = apply_source_choice(candidates, &choices).unwrap();

        assert!(merged.is_empty());
    }

    #[test]
    fn errors_when_no_choice_configured_for_cell() {
        let candidates: Vec<CandidateValue> = vec![make_candidate(SOURCE_WB, 2022, 1.66)];

        let result: Result<Vec<MergedValue>, AppError> = apply_source_choice(candidates, &[]);

        assert!(result.is_err());
    }
}
