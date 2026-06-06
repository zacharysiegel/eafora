//! Selection rules:
//!
//! 1. For each `(region, statistic, license_shard_class)` series, resolve
//!    the chosen source: per-region override if present, else global default.
//! 2. Emit the chosen source's value for every period it has. Periods the
//!    chosen source doesn't cover emit nothing. Never mix sources within a
//!    series.
//! 3. If neither override nor global default exists for a series, error:
//!    the editorial config is incomplete.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::artifact::artifact_model::{CandidateValue, ResolvedValue, StatisticShardKey};
use crate::canonical::canonical_model::{DataSourceKind, LicenseShardClass, SourceChoice, StatisticKind};
use crate::error::AppError;

/// A series is the time series of one statistic for one region within
/// one license-shard partition. We pick one source per series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SeriesKey {
    pub region_id: Uuid,
    pub statistic_kind: StatisticKind,
    pub license_shard_class: LicenseShardClass,
}

impl SeriesKey {
    pub fn from_candidate(candidate: &CandidateValue) -> Self {
        SeriesKey {
            region_id: candidate.region_id,
            statistic_kind: candidate.statistic_kind,
            license_shard_class: LicenseShardClass::from_license_class(candidate.license_class),
        }
    }
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

    fn choose_default(&self, shard_key: StatisticShardKey) -> Option<DataSourceKind> {
        self.globals.get(&shard_key).copied()
    }
}

/// Resolve data source selection rules to reduce candidate values to only one per
/// (region, statistic, period) cell
pub fn resolve_candidates(
    candidates: Vec<CandidateValue>,
    source_choices: &[SourceChoice],
) -> Result<Vec<ResolvedValue>, AppError> {
    let source_resolver: SourceChoiceResolver = SourceChoiceResolver::from_slice(source_choices);
    let groups: BTreeMap<SeriesKey, Vec<CandidateValue>> = group_candidates(candidates);

    let mut resolved_values: Vec<ResolvedValue> = Vec::new();
    for (series_key, series_candidates) in groups {
        let chosen_data_source_kind: DataSourceKind = source_resolver
            .choose(series_key)
            .ok_or_else(|| {
                AppError::from(format!("no source_choice configured for series key [{series_key:?}]"))
            })?;

        resolved_values.extend(
            series_candidates
                .iter()
                .filter(|candidate| candidate.data_source_kind == chosen_data_source_kind)
                .map(|candidate| ResolvedValue::from_candidate(candidate, series_key.license_shard_class))
        );
    }

    Ok(resolved_values)
}

fn group_candidates(candidates: Vec<CandidateValue>) -> BTreeMap<SeriesKey, Vec<CandidateValue>> {
    let mut groups: BTreeMap<SeriesKey, Vec<CandidateValue>> = BTreeMap::new();
    for candidate in candidates {
        groups
            .entry(SeriesKey::from_candidate(&candidate))
            .or_default()
            .push(candidate);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::{DateTime, Utc};

    use crate::adapter::adapter_model::NaiveDatePeriod;
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
    fn override_emits_only_periods_the_override_covers_no_default_mixing() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(DataSourceKind::TestAlpha, 2021, 1.71),
            make_candidate(DataSourceKind::TestBeta, 2022, 1.70),
        ];
        let choices: Vec<SourceChoice> = vec![
            global_choice(DataSourceKind::TestAlpha),
            override_choice(REGION_USA, DataSourceKind::TestBeta),
        ];

        let merged: Vec<ResolvedValue> = resolve_candidates(candidates, &choices).unwrap();

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].data_source_kind, DataSourceKind::TestBeta);
        assert!((merged[0].value - 1.70).abs() < f64::EPSILON);
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
