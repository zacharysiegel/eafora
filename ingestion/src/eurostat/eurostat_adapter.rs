use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::Utc;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use shared::canonical::canonical_model::{
    Country, DataSource, DataSourceKind, DataStatus, NaiveDatePeriod, SourceRevision, Statistic, StatisticKind,
    Subdivision,
};

use crate::adapter::{self, AdapterOptions, IngestWarning, IngestWarningKind, NormalizedStatisticValue};
use crate::canonical::canonical_db;
use crate::error::AppError;
use crate::eurostat::eurostat_client::{self, EurostatExtraction, EurostatGeoLevel};
use crate::eurostat::eurostat_model::{ParsedEurostatObservation, ParsedEurostatResponse};
use crate::ingest;
use crate::ingest::IngestReport;

/// Eurostat publishes Greece and the United Kingdom under codes that are not their ISO 3166-1 alpha-2.
const ISO2_BY_EUROSTAT_GEO_CODE: [GeoCodeAlias; 2] = [
    GeoCodeAlias { geo_code: "EL", iso2: "GR" },
    GeoCodeAlias { geo_code: "UK", iso2: "GB" },
];

/// Metropolitan France is a subset of France, which the same response also carries.
const EXCLUDED_GEO_CODES: [&str; 1] = ["FX"];

/// The observation-status characters that carry a `DataStatus`, in the order they win when a flag names
/// several. Eurostat's others qualify comparability rather than the value's standing and have no canonical
/// status: `b` break in series, `d` definition differs, `n` not significant, `u` low reliability, `m` missing
/// and cannot exist.
const STATUS_BY_FLAG_CHARACTER: [FlagStatus; 4] = [
    FlagStatus { character: 'f', status: DataStatus::Projection },
    FlagStatus { character: 'i', status: DataStatus::Imputed },
    FlagStatus { character: 'e', status: DataStatus::Estimated },
    FlagStatus { character: 'p', status: DataStatus::Provisional },
];

/// Which statistic each ingested indicator becomes.
const INGESTED_INDICATORS: [(&str, StatisticKind); 3] = [
    (eurostat_client::INDICATOR_TOTAL_FERTILITY_RATE, StatisticKind::Tfr),
    (eurostat_client::INDICATOR_MEAN_AGE_AT_CHILDBIRTH, StatisticKind::MeanAgeAtChildbirth),
    (eurostat_client::INDICATOR_MEAN_AGE_AT_FIRST_BIRTH, StatisticKind::MeanAgeAtFirstBirth),
];

struct GeoCodeAlias {
    geo_code: &'static str,
    iso2: &'static str,
}

struct FlagStatus {
    character: char,
    status: DataStatus,
}

enum RegionOutcome {
    Resolved(Uuid),
    Warned(IngestWarning),
    Excluded,
}

/// One transaction over the whole run, so a mid-run failure leaves the canonical store untouched.
pub async fn fetch_and_store(pool: &PgPool, options: AdapterOptions) -> Result<IngestReport, AppError> {
    let mut transaction: Transaction<'_, Postgres> = pool.begin().await?;

    let data_source: DataSource =
        canonical_db::find_data_source_by_kind(&mut *transaction, DataSourceKind::Eurostat)
            .await?
            .ok_or_else(|| {
                AppError::from(format!(
                    "data_source {:?} missing from canonical store",
                    DataSourceKind::Eurostat,
                ))
            })?;
    let last_seen: Option<SourceRevision> =
        ingest::ingest_db::read_latest_publication(&mut *transaction, data_source.id).await?;

    let mut responses: Vec<(&EurostatExtraction, ParsedEurostatResponse)> = Vec::new();
    for extraction in &eurostat_client::EXTRACTIONS {
        let body: String = eurostat_client::fetch_upstream(extraction).await?;
        let response: ParsedEurostatResponse = eurostat_client::parse_response(extraction, &body)?;

        responses.push((extraction, response));
    }

    let revision_label: String = revision_label_of(&responses);

    if adapter::should_skip_run(&last_seen, &revision_label, options) {
        log::info!("eurostat is unchanged since the last run; [revision_label={revision_label}]");

        return Ok(IngestReport::default());
    }

    let seeded_revision_by_geo_code: BTreeMap<String, i32> =
        canonical_db::read_nuts_revision_by_code(&mut *transaction).await?;

    let mut normalized_statistic_values: Vec<NormalizedStatisticValue> = Vec::new();
    let mut warnings: Vec<IngestWarning> = Vec::new();

    for (extraction, response) in &responses {
        warnings.extend(get_recut_region_warning(extraction, response, &seeded_revision_by_geo_code));

        for (indicator_code, statistic_kind) in INGESTED_INDICATORS {
            if !extraction.indicator_codes.contains(&indicator_code) {
                continue;
            }

            let for_indicator: Vec<ParsedEurostatObservation> = response.observations
                .iter()
                .filter(|observation| observation.indicator_code == indicator_code)
                .filter(|observation| {
                    is_from_seeded_revision(
                        response.revision_by_geo_code.get(&observation.geo_code).copied(),
                        seeded_revision_by_geo_code.get(&observation.geo_code).copied(),
                    )
                })
                .cloned()
                .collect();

            let (values, indicator_warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
                normalize(&mut *transaction, for_indicator, statistic_kind, extraction.geo_level).await?;

            normalized_statistic_values.extend(values);
            warnings.extend(indicator_warnings);
        }
    }

    let mut report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source.id,
        &revision_label,
        None,
        Utc::now(),
        normalized_statistic_values,
    )
    .await?;
    report.warnings = warnings;

    transaction.commit().await?;

    Ok(report)
}

/// One run reads several datasets, each carrying its own `updated` timestamp, so the revision is all of them
/// together and any one of them moving defeats the skip.
fn revision_label_of(responses: &[(&EurostatExtraction, ParsedEurostatResponse)]) -> String {
    let updated_by_dataset: BTreeMap<&str, &str> = responses
        .iter()
        .map(|(extraction, response)| {
            (extraction.dataset, response.publication.revision_label.as_str())
        })
        .collect();

    updated_by_dataset
        .into_iter()
        .map(|(dataset, updated)| format!("{dataset}={updated}"))
        .collect::<Vec<String>>()
        .join(",")
}

/// Eurostat marks a label with a revision only once that revision has retired the code, so a marked code the
/// store does not hold is one the classification has recut and the seed deliberately leaves out: its ground is
/// covered by the live code that took its name. An unmarked code the store does not hold is a gap, and reaches
/// the region lookup to be reported as one.
fn is_from_seeded_revision(published_revision: Option<i32>, seeded_revision: Option<i32>) -> bool {
    match (published_revision, seeded_revision) {
        (Some(published), Some(seeded)) => published == seeded,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// A code the store holds under one revision and the source publishes under another names different ground on
/// the two sides, so its values are skipped. This is the signal to re-seed.
fn get_recut_region_warning(
    extraction: &EurostatExtraction,
    response: &ParsedEurostatResponse,
    seeded_revision_by_geo_code: &BTreeMap<String, i32>,
) -> Option<IngestWarning> {
    let recut_geo_codes: BTreeSet<&str> = response.revision_by_geo_code
        .iter()
        .filter_map(|(geo_code, published)| {
            let seeded: &i32 = seeded_revision_by_geo_code.get(geo_code)?;

            (seeded != published).then_some(geo_code.as_str())
        })
        .collect();

    if recut_geo_codes.is_empty() {
        return None;
    }

    Some(IngestWarning {
        kind: IngestWarningKind::MismatchedRegionRevision,
        message: format!(
            "regions the source publishes under a different NUTS revision than the seed holds are skipped; \
             [dataset={} geo_level={} regions={recut_geo_codes:?}]",
            extraction.dataset,
            extraction.geo_level.code(),
        ),
    })
}

pub async fn normalize(
    connection: &mut PgConnection,
    observations: Vec<ParsedEurostatObservation>,
    statistic_kind: StatisticKind,
    geo_level: EurostatGeoLevel,
) -> Result<(Vec<NormalizedStatisticValue>, Vec<IngestWarning>), AppError> {
    let statistic: Statistic =
        canonical_db::find_statistic_by_code(&mut *connection, statistic_kind.code())
            .await?
            .ok_or_else(|| {
                AppError::from(format!(
                    "statistic {:?} missing from canonical store (run dbmate up)",
                    statistic_kind.code(),
                ))
            })?;

    let mut statistic_values: Vec<NormalizedStatisticValue> = Vec::with_capacity(observations.len());
    let mut warnings: Vec<IngestWarning> = Vec::new();
    let mut region_by_geo_code: HashMap<String, RegionOutcome> = HashMap::new();

    for observation in observations {
        if !region_by_geo_code.contains_key(&observation.geo_code) {
            let outcome: RegionOutcome =
                resolve_region(&mut *connection, &observation.geo_code, geo_level).await?;

            if let RegionOutcome::Warned(warning) = &outcome {
                warnings.push(warning.clone());
            }

            region_by_geo_code.insert(observation.geo_code.clone(), outcome);
        }

        let RegionOutcome::Resolved(region_id) = region_by_geo_code[&observation.geo_code]
        else {
            continue;
        };

        statistic_values.push(normalize_row(&observation, region_id, statistic.id)?);
    }

    Ok((statistic_values, warnings))
}

async fn resolve_region(
    connection: &mut PgConnection,
    geo_code: &str,
    geo_level: EurostatGeoLevel,
) -> Result<RegionOutcome, AppError> {
    if EXCLUDED_GEO_CODES.contains(&geo_code) {
        return Ok(RegionOutcome::Excluded);
    }

    match geo_level {
        EurostatGeoLevel::Country => resolve_country_region(&mut *connection, geo_code).await,
        EurostatGeoLevel::Nuts1 | EurostatGeoLevel::Nuts2 | EurostatGeoLevel::Nuts3 => {
            resolve_subdivision_region(&mut *connection, geo_code).await
        },
    }
}

async fn resolve_country_region(
    connection: &mut PgConnection,
    geo_code: &str,
) -> Result<RegionOutcome, AppError> {
    let iso2: &str = get_iso2_for_geo_code(geo_code);
    let country: Option<Country> = canonical_db::find_country_by_iso2(&mut *connection, iso2).await?;

    match country {
        Some(country) => Ok(RegionOutcome::Resolved(country.region_id)),
        None => Ok(RegionOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::UnrecognizedRegionCode,
            message: format!("code {geo_code} matches no canonical region"),
        })),
    }
}

/// The alpha-2 a Eurostat geo code stands for, which is the code itself except where Eurostat departs from
/// ISO 3166-1. Public because the seed generator resolves a NUTS region's country the same way, and a
/// disagreement would parent those regions under the wrong country.
pub fn get_iso2_for_geo_code(geo_code: &str) -> &str {
    ISO2_BY_EUROSTAT_GEO_CODE
        .iter()
        .find(|alias| alias.geo_code == geo_code)
        .map_or(geo_code, |alias| alias.iso2)
}

async fn resolve_subdivision_region(
    connection: &mut PgConnection,
    nuts_code: &str,
) -> Result<RegionOutcome, AppError> {
    let subdivision: Option<Subdivision> =
        canonical_db::find_subdivision_by_nuts_code(&mut *connection, nuts_code).await?;

    match subdivision {
        Some(subdivision) => Ok(RegionOutcome::Resolved(subdivision.region_id)),
        None => Ok(RegionOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::UnrecognizedRegionCode,
            message: format!("NUTS code {nuts_code} matches no canonical region"),
        })),
    }
}

fn normalize_row(
    observation: &ParsedEurostatObservation,
    region_id: Uuid,
    statistic_id: Uuid,
) -> Result<NormalizedStatisticValue, AppError> {
    Ok(NormalizedStatisticValue {
        region_id,
        statistic_id,
        period: NaiveDatePeriod::from_year(observation.period_year)?,
        value: observation.value,
        data_status: status_for_flag(observation.flag.as_deref()),
    })
}

/// Every code is one character and every combination an ordered run of them, so membership decides the
/// status and no tokenizer is needed.
pub fn status_for_flag(flag: Option<&str>) -> DataStatus {
    let Some(flag) = flag
    else {
        return DataStatus::Final;
    };

    STATUS_BY_FLAG_CHARACTER
        .iter()
        .find(|candidate| flag.contains(candidate.character))
        .map_or(DataStatus::Final, |candidate| candidate.status)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::eurostat::eurostat_model::ParsedEurostatPublication;

    fn nuts_2_extraction() -> &'static EurostatExtraction {
        eurostat_client::EXTRACTIONS
            .iter()
            .find(|extraction| extraction.geo_level == EurostatGeoLevel::Nuts2)
            .expect("an extraction for the level")
    }

    fn response_revised(revision_by_geo_code: BTreeMap<String, i32>) -> ParsedEurostatResponse {
        ParsedEurostatResponse {
            publication: ParsedEurostatPublication {
                revision_label: "2026-01-01T00:00:00+0100".to_string(),
            },
            revision_by_geo_code,
            observations: Vec::new(),
        }
    }

    #[test]
    fn is_from_seeded_revision_keeps_a_code_no_revision_has_recut() {
        assert!(is_from_seeded_revision(None, Some(2021)));
    }

    #[test]
    fn is_from_seeded_revision_matches_only_the_seeded_revision() {
        assert!(is_from_seeded_revision(Some(2021), Some(2021)));
        assert!(!is_from_seeded_revision(Some(2016), Some(2021)));
        assert!(!is_from_seeded_revision(Some(2024), Some(2021)));
    }

    #[test]
    fn is_from_seeded_revision_skips_a_retired_code_the_store_does_not_hold() {
        assert!(!is_from_seeded_revision(Some(2016), None));
    }

    #[test]
    fn is_from_seeded_revision_keeps_an_unmarked_code_the_store_does_not_hold() {
        assert!(is_from_seeded_revision(None, None));
    }

    #[test]
    fn get_recut_region_warning_stays_silent_when_the_two_revisions_agree() {
        let response: ParsedEurostatResponse =
            response_revised(BTreeMap::from([("HR04".to_string(), 2016)]));
        let seeded: BTreeMap<String, i32> = BTreeMap::from([("HR04".to_string(), 2016)]);

        assert!(get_recut_region_warning(nuts_2_extraction(), &response, &seeded).is_none());
    }

    #[test]
    fn get_recut_region_warning_stays_silent_for_a_code_the_store_does_not_hold() {
        let response: ParsedEurostatResponse =
            response_revised(BTreeMap::from([("HR04".to_string(), 2016)]));

        assert!(get_recut_region_warning(nuts_2_extraction(), &response, &BTreeMap::new()).is_none());
    }

    #[test]
    fn get_recut_region_warning_names_a_code_the_source_has_recut() {
        let response: ParsedEurostatResponse =
            response_revised(BTreeMap::from([("HR04".to_string(), 2024)]));
        let seeded: BTreeMap<String, i32> = BTreeMap::from([("HR04".to_string(), 2021)]);

        let warning: IngestWarning = get_recut_region_warning(nuts_2_extraction(), &response, &seeded)
            .expect("a warning");

        assert!(matches!(warning.kind, IngestWarningKind::MismatchedRegionRevision));
        assert!(warning.message.contains("HR04"), "{}", warning.message);
    }

    #[test]
    fn status_for_flag_maps_each_modelled_character() {
        assert_eq!(status_for_flag(Some("f")), DataStatus::Projection);
        assert_eq!(status_for_flag(Some("i")), DataStatus::Imputed);
        assert_eq!(status_for_flag(Some("e")), DataStatus::Estimated);
        assert_eq!(status_for_flag(Some("p")), DataStatus::Provisional);
    }

    #[test]
    fn status_for_flag_treats_an_unmodelled_qualifier_as_final() {
        for qualifier in ["b", "d", "n", "u", "m", "bd"] {
            assert_eq!(status_for_flag(Some(qualifier)), DataStatus::Final, "{qualifier}");
        }
    }

    #[test]
    fn status_for_flag_is_final_when_absent() {
        assert_eq!(status_for_flag(None), DataStatus::Final);
    }

    /// The runs the country-level extraction carries, plus two longer ones.
    #[test]
    fn status_for_flag_takes_the_highest_precedence_character_in_a_run() {
        assert_eq!(status_for_flag(Some("ep")), DataStatus::Estimated);
        assert_eq!(status_for_flag(Some("be")), DataStatus::Estimated);
        assert_eq!(status_for_flag(Some("bp")), DataStatus::Provisional);
        assert_eq!(status_for_flag(Some("bdep")), DataStatus::Estimated);
        assert_eq!(status_for_flag(Some("bdip")), DataStatus::Imputed);
    }
}
