//! Selection rules:
//!
//! 1. For each `(region, statistic, license_shard_class)` series, resolve
//!    the chosen `data_source_id`: per-region override if present, else
//!    global default.
//! 2. For each period in the series: emit the chosen source's value. If the
//!    chosen source has no value for that period AND the chosen source
//!    differs from the global default AND the default has a value, fall
//!    back to the default's value and log a warning. Else emit nothing.
//! 3. If neither override nor global default exists for a series, error:
//!    the editorial config is incomplete.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;
use crate::artifact::artifact_model::{CandidateValue, ResolvedValue};
use crate::canonical::canonical_model::{DataSourceKind, LicenseShardClass, SourceChoice, StatisticKind};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SeriesKey {
    region_id: Uuid,
    statistic_kind: StatisticKind,
    license_shard_class: LicenseShardClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct StatisticShardKey {
    statistic_kind: StatisticKind,
    license_shard_class: LicenseShardClass,
}

struct SourceChoiceResolver {
    overrides: BTreeMap<SeriesKey, DataSourceKind>,
    globals: BTreeMap<StatisticShardKey, DataSourceKind>,
}

impl SourceChoiceResolver {
    fn from_slice(source_choices: &[SourceChoice]) -> Self {
        let mut overrides: BTreeMap<SeriesKey, DataSourceKind> = BTreeMap::new();
        let mut globals: BTreeMap<StatisticShardKey, DataSourceKind> = BTreeMap::new();
        for choice in source_choices {
            match choice.region_id {
                Some(region_id) => {
                    overrides.insert(
                        SeriesKey {
                            region_id,
                            statistic_kind: choice.statistic_kind,
                            license_shard_class: choice.license_shard_class,
                        },
                        choice.data_source_kind,
                    );
                }
                None => {
                    globals.insert(
                        StatisticShardKey {
                            statistic_kind: choice.statistic_kind,
                            license_shard_class: choice.license_shard_class,
                        },
                        choice.data_source_kind,
                    );
                }
            }
        }
        SourceChoiceResolver { overrides, globals }
    }

    fn choose(&self, series_key: SeriesKey) -> Option<DataSourceKind> {
        self.overrides
            .get(&series_key)
            .copied()
            .or_else(|| self.choose_default(StatisticShardKey {
                statistic_kind: series_key.statistic_kind,
                license_shard_class: series_key.license_shard_class,
            }))
    }

    fn choose_default(&self, statistic_shard_key: StatisticShardKey) -> Option<DataSourceKind> {
        self.globals.get(&statistic_shard_key).copied()
    }
}

pub fn resolve_candidates(
    candidates: Vec<CandidateValue>,
    source_choices: &[SourceChoice],
) -> Result<Vec<ResolvedValue>, AppError> {
    let resolver: SourceChoiceResolver = SourceChoiceResolver::from_slice(source_choices);

    let mut groups: BTreeMap<SeriesKey, Vec<CandidateValue>> = BTreeMap::new();
    for candidate in candidates {
        let license_shard_class: LicenseShardClass = LicenseShardClass::from_license_class(candidate.license_class);
        let key: SeriesKey = SeriesKey {
            region_id: candidate.region_id,
            statistic_kind: candidate.statistic_kind,
            license_shard_class,
        };
        groups.entry(key).or_default().push(candidate);
    }

    let mut resolved_values: Vec<ResolvedValue> = Vec::new();
    for (series_key, series_candidates) in groups {
        let chosen_data_source_kind: DataSourceKind = resolver
            .choose(series_key)
            .ok_or_else(|| {
                AppError::from(format!(
                    "resolve_candidates: no source_choice configured for region={} statistic={:?} shard={:?}",
                    series_key.region_id, series_key.statistic_kind, series_key.license_shard_class,
                ))
            })?;
        let default_data_source_kind: Option<DataSourceKind> =
            resolver.choose_default(StatisticShardKey {
                statistic_kind: series_key.statistic_kind,
                license_shard_class: series_key.license_shard_class,
            });

        resolved_values.extend(
            select_per_period(&series_candidates, chosen_data_source_kind, default_data_source_kind, series_key.license_shard_class)
        );
    }

    Ok(resolved_values)
}

fn select_per_period(
    candidates: &[CandidateValue],
    chosen_data_source_kind: DataSourceKind,
    default_data_source_kind: Option<DataSourceKind>,
    license_shard_class: LicenseShardClass,
) -> Vec<ResolvedValue> {
    let mut by_period: BTreeMap<NaiveDatePeriod, Vec<&CandidateValue>> = BTreeMap::new();
    for candidate in candidates {
        by_period.entry(candidate.period).or_default().push(candidate);
    }

    let mut emitted: Vec<ResolvedValue> = Vec::with_capacity(by_period.len());
    for (period, period_candidates) in by_period {
        let chosen: Option<&CandidateValue> = period_candidates
            .iter()
            .copied()
            .find(|candidate| candidate.data_source_kind == chosen_data_source_kind);

        if let Some(candidate) = chosen {
            emitted.push(resolved_value_from(candidate, license_shard_class));
            continue;
        }

        let Some(default_data_source_kind) = default_data_source_kind else {
            continue;
        };
        if default_data_source_kind == chosen_data_source_kind {
            continue;
        }
        let Some(candidate) = period_candidates
            .iter()
            .copied()
            .find(|candidate| candidate.data_source_kind == default_data_source_kind)
        else {
            continue;
        };

        log::warn!(
            "resolve_candidates: region={} statistic={:?} period=[{},{}) fell back from chosen source {:?} to global default {:?}",
            candidate.region_iso3, candidate.statistic_kind, period.start, period.end,
            chosen_data_source_kind, default_data_source_kind,
        );
        emitted.push(resolved_value_from(candidate, license_shard_class));
    }

    emitted
}

fn resolved_value_from(candidate: &CandidateValue, license_shard_class: LicenseShardClass) -> ResolvedValue {
    ResolvedValue {
        region_id: candidate.region_id,
        region_iso3: candidate.region_iso3.clone(),
        statistic_kind: candidate.statistic_kind,
        period: candidate.period,
        value: candidate.value,
        data_status: candidate.data_status,
        data_source_kind: candidate.data_source_kind,
        data_source_revision: candidate.data_source_revision.clone(),
        license_shard_class,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::{DateTime, Utc};

    use crate::canonical::canonical_model::{DataSourceKind, DataStatus, LicenseClass, StatisticKind};

    const REGION_USA: u128 = 1;

    fn now() -> DateTime<Utc> { "2026-05-30T00:00:00Z".parse().unwrap() }

    fn period_year(year: i32) -> NaiveDatePeriod { NaiveDatePeriod::from_year(year).unwrap() }

    fn make_candidate(data_source_kind: DataSourceKind, year: i32, value: f64) -> CandidateValue {
        CandidateValue {
            region_id: Uuid::from_u128(REGION_USA),
            region_iso3: "USA".to_string(),
            statistic_kind: StatisticKind::Tfr,
            period: period_year(year),
            value,
            data_status: DataStatus::Final,
            data_source_kind,
            data_source_revision: "rev1".to_string(),
            license_class: LicenseClass::Attribution,
        }
    }

    fn global_choice(data_source_kind: DataSourceKind) -> SourceChoice {
        SourceChoice {
            id: Uuid::now_v7(),
            region_id: None,
            statistic_kind: StatisticKind::Tfr,
            license_shard_class: LicenseShardClass::Base,
            data_source_kind,
            created: now(),
            modified: now(),
        }
    }

    fn override_choice(region: u128, data_source_kind: DataSourceKind) -> SourceChoice {
        SourceChoice {
            id: Uuid::now_v7(),
            region_id: Some(Uuid::from_u128(region)),
            statistic_kind: StatisticKind::Tfr,
            license_shard_class: LicenseShardClass::Base,
            data_source_kind,
            created: now(),
            modified: now(),
        }
    }

    #[test]
    fn global_default_with_no_override_uses_default_for_every_period() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(DataSourceKind::TestAlpha, 2022, 1.66),
            make_candidate(DataSourceKind::TestAlpha, 2023, 1.62),
        ];
        let merged: Vec<ResolvedValue> =
            resolve_candidates(candidates, &[global_choice(DataSourceKind::TestAlpha)]).unwrap();
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn override_wins_over_global_default_when_override_has_value() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(DataSourceKind::TestAlpha, 2022, 1.66),
            make_candidate(DataSourceKind::TestBeta, 2022, 1.70),
        ];
        let choices: Vec<SourceChoice> = vec![
            global_choice(DataSourceKind::TestAlpha),
            override_choice(REGION_USA, DataSourceKind::TestBeta),
        ];

        let merged: Vec<ResolvedValue> = resolve_candidates(candidates, &choices).unwrap();

        assert_eq!(merged.len(), 1);
        assert!((merged[0].value - 1.70).abs() < f64::EPSILON);
    }

    #[test]
    fn override_falls_back_to_default_when_override_has_no_value_for_period() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(DataSourceKind::TestAlpha, 2021, 1.71),
            make_candidate(DataSourceKind::TestBeta, 2022, 1.70),
        ];
        let choices: Vec<SourceChoice> = vec![
            global_choice(DataSourceKind::TestAlpha),
            override_choice(REGION_USA, DataSourceKind::TestBeta),
        ];

        let merged: Vec<ResolvedValue> = resolve_candidates(candidates, &choices).unwrap();

        assert_eq!(merged.len(), 2);
        let value_2021 = merged.iter().find(|m| m.period.start.format("%Y").to_string() == "2021").unwrap().value;
        let value_2022 = merged.iter().find(|m| m.period.start.format("%Y").to_string() == "2022").unwrap().value;
        assert!((value_2021 - 1.71).abs() < f64::EPSILON);
        assert!((value_2022 - 1.70).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_when_neither_chosen_nor_default_has_a_value() {
        let candidates: Vec<CandidateValue> = vec![make_candidate(DataSourceKind::WorldBankWDI, 2022, 1.99)];
        let choices: Vec<SourceChoice> = vec![
            global_choice(DataSourceKind::TestAlpha),
            override_choice(REGION_USA, DataSourceKind::TestBeta),
        ];

        let merged: Vec<ResolvedValue> = resolve_candidates(candidates, &choices).unwrap();

        assert!(merged.is_empty());
    }

    #[test]
    fn errors_when_no_choice_configured_for_series() {
        let candidates: Vec<CandidateValue> = vec![make_candidate(DataSourceKind::TestAlpha, 2022, 1.66)];

        let result: Result<Vec<ResolvedValue>, AppError> = resolve_candidates(candidates, &[]);

        assert!(result.is_err());
    }
}
