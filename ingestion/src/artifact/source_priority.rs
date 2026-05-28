//! Source-priority merge: collapses many candidate values per cell down to
//! one winner using the data_source preference ordering.
//!
//! Grouping key: `(region_id, statistic_id, period.start, period.end,
//! license_shard_class)`. Within a group:
//!
//! 1. `final` data_status outranks any non-final status; within the same
//!    status tier, lower `preference_rank` wins.
//! 2. Equal preference_rank ties break by `data_source_id` (ascending) so
//!    builds are deterministic.
//!
//! Pure function. No I/O. The grouping key intentionally includes
//! `license_shard_class` so a Base-licensed source never displaces a
//! NonCommercial-licensed source in the Base shard, and vice versa — every
//! shard tells a self-consistent story.

use std::collections::BTreeMap;

use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;
use crate::artifact::artifact_model::{CandidateValue, LicenseShardClass, MergedValue};

pub fn apply_source_priority(candidates: Vec<CandidateValue>) -> Vec<MergedValue> {
    let mut groups: BTreeMap<MergeKey, CandidateValue> = BTreeMap::new();

    for candidate in candidates {
        let license_shard_class: LicenseShardClass =
            LicenseShardClass::from_license_class(candidate.license_class);
        let key: MergeKey = MergeKey {
            region_id: candidate.region_id,
            statistic_id: candidate.statistic_id,
            period: candidate.period,
            license_shard_class,
        };

        match groups.get(&key) {
            Some(incumbent) if !candidate_outranks(&candidate, incumbent) => {}
            _ => {
                groups.insert(key, candidate);
            }
        }
    }

    groups
        .into_iter()
        .map(|(key, winner)| MergedValue {
            region_id: winner.region_id,
            region_iso3: winner.region_iso3,
            statistic_id: winner.statistic_id,
            statistic_code: winner.statistic_code,
            period: winner.period,
            value: winner.value,
            data_status: winner.data_status,
            data_source_code: winner.data_source_code,
            data_source_revision: winner.data_source_revision,
            license_shard_class: key.license_shard_class,
        })
        .collect()
}

pub fn collect_data_source_versions(candidates: &[CandidateValue]) -> BTreeMap<String, String> {
    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    for candidate in candidates {
        let entry: &mut String = versions
            .entry(candidate.data_source_code.clone())
            .or_default();
        if candidate.data_source_revision.as_str() > entry.as_str() {
            *entry = candidate.data_source_revision.clone();
        }
    }
    versions
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MergeKey {
    region_id: Uuid,
    statistic_id: Uuid,
    period: NaiveDatePeriod,
    license_shard_class: LicenseShardClass,
}

fn candidate_outranks(challenger: &CandidateValue, incumbent: &CandidateValue) -> bool {
    let challenger_key: (u8, i32, Uuid) = ranking_key(challenger);
    let incumbent_key: (u8, i32, Uuid) = ranking_key(incumbent);
    challenger_key < incumbent_key
}

fn ranking_key(candidate: &CandidateValue) -> (u8, i32, Uuid) {
    let status_tier: u8 = if candidate.data_status == "final" { 0 } else { 1 };
    (
        status_tier,
        candidate.data_source_preference_rank,
        candidate.data_source_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::canonical::canonical_model::LicenseClass;

    fn period_2024() -> NaiveDatePeriod {
        NaiveDatePeriod::from_year(2024).expect("valid year")
    }

    fn make_candidate(
        data_source_id_low_byte: u128,
        preference_rank: i32,
        data_status: &str,
        license_class: LicenseClass,
        value: f64,
    ) -> CandidateValue {
        CandidateValue {
            region_id: Uuid::from_u128(1),
            region_iso3: "USA".to_string(),
            statistic_id: Uuid::from_u128(100),
            statistic_code: "tfr".to_string(),
            period: period_2024(),
            value,
            data_status: data_status.to_string(),
            data_source_id: Uuid::from_u128(data_source_id_low_byte),
            data_source_code: format!("source_{}", data_source_id_low_byte),
            data_source_revision: "rev1".to_string(),
            data_source_preference_rank: preference_rank,
            license_class,
        }
    }

    #[test]
    fn apply_source_priority_single_candidate_returns_unchanged_value() {
        let candidates: Vec<CandidateValue> = vec![make_candidate(10, 5, "final", LicenseClass::Attribution, 1.5)];

        let merged: Vec<MergedValue> = apply_source_priority(candidates);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].value, 1.5);
        assert_eq!(merged[0].data_source_code, "source_10");
        assert_eq!(merged[0].license_shard_class, LicenseShardClass::Base);
    }

    #[test]
    fn apply_source_priority_lower_preference_rank_wins() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(20, 9, "final", LicenseClass::Attribution, 1.0),
            make_candidate(10, 1, "final", LicenseClass::Attribution, 2.0),
        ];

        let merged: Vec<MergedValue> = apply_source_priority(candidates);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].value, 2.0);
        assert_eq!(merged[0].data_source_code, "source_10");
    }

    #[test]
    fn apply_source_priority_equal_preference_rank_breaks_tie_by_data_source_id() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(99, 5, "final", LicenseClass::Attribution, 1.0),
            make_candidate(7, 5, "final", LicenseClass::Attribution, 2.0),
        ];

        let merged: Vec<MergedValue> = apply_source_priority(candidates);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].value, 2.0);
        assert_eq!(merged[0].data_source_code, "source_7");
    }

    #[test]
    fn apply_source_priority_final_status_overrides_lower_preference_rank() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(10, 1, "provisional", LicenseClass::Attribution, 1.0),
            make_candidate(20, 9, "final", LicenseClass::Attribution, 2.0),
        ];

        let merged: Vec<MergedValue> = apply_source_priority(candidates);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].value, 2.0);
        assert_eq!(merged[0].data_status, "final");
    }

    #[test]
    fn apply_source_priority_different_license_classes_emit_separate_outputs() {
        let candidates: Vec<CandidateValue> = vec![
            make_candidate(10, 1, "final", LicenseClass::Attribution, 1.0),
            make_candidate(20, 1, "final", LicenseClass::NonCommercial, 2.0),
        ];

        let merged: Vec<MergedValue> = apply_source_priority(candidates);

        assert_eq!(merged.len(), 2);
        let base: &MergedValue = merged
            .iter()
            .find(|merged_value| merged_value.license_shard_class == LicenseShardClass::Base)
            .expect("base shard present");
        let non_commercial: &MergedValue = merged
            .iter()
            .find(|merged_value| merged_value.license_shard_class == LicenseShardClass::NonCommercial)
            .expect("non-commercial shard present");
        assert_eq!(base.value, 1.0);
        assert_eq!(non_commercial.value, 2.0);
    }

    #[test]
    fn collect_data_source_versions_picks_highest_revision_per_source() {
        let candidates: Vec<CandidateValue> = vec![
            CandidateValue {
                data_source_revision: "2024-Q3".to_string(),
                ..make_candidate(10, 1, "final", LicenseClass::Attribution, 1.0)
            },
            CandidateValue {
                data_source_revision: "2024-Q4".to_string(),
                ..make_candidate(10, 1, "final", LicenseClass::Attribution, 2.0)
            },
        ];

        let versions: BTreeMap<String, String> = collect_data_source_versions(&candidates);

        assert_eq!(versions.get("source_10").map(String::as_str), Some("2024-Q4"));
    }
}
