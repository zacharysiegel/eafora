use chrono::Utc;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use shared::canonical::canonical_model::{
    Country, DataSource, DataSourceKind, DataStatus, NaiveDatePeriod, SourceRevision, Statistic, StatisticKind,
};

use crate::adapter::{self, AdapterOptions, IngestWarning, IngestWarningKind, NormalizedStatisticValue};
use crate::canonical::canonical_db;
use crate::error::AppError;
use crate::eurostat::eurostat_client;
use crate::eurostat::eurostat_model::{ParsedEurostatObservation, ParsedEurostatPublication};
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

/// Which statistic each ingested indicator becomes. One response carries all three.
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

    let body: String = eurostat_client::fetch_upstream().await?;
    let (publication, observations): (ParsedEurostatPublication, Vec<ParsedEurostatObservation>) =
        eurostat_client::parse_response(&body)?;

    if adapter::should_skip_run(&last_seen, &publication.revision_label, options) {
        log::info!(
            "eurostat is unchanged since the last run; [revision_label={}]",
            publication.revision_label,
        );

        return Ok(IngestReport::default());
    }

    let mut normalized_statistic_values: Vec<NormalizedStatisticValue> = Vec::new();
    let mut warnings: Vec<IngestWarning> = Vec::new();

    for (indicator_code, statistic_kind) in INGESTED_INDICATORS {
        let for_indicator: Vec<ParsedEurostatObservation> = observations
            .iter()
            .filter(|observation| observation.indicator_code == indicator_code)
            .cloned()
            .collect();

        let (values, indicator_warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
            normalize(&mut *transaction, for_indicator, statistic_kind).await?;

        normalized_statistic_values.extend(values);
        warnings.extend(indicator_warnings);
    }

    let mut report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source.id,
        &publication.revision_label,
        None,
        Utc::now(),
        normalized_statistic_values,
    )
    .await?;
    report.warnings = warnings;

    transaction.commit().await?;

    Ok(report)
}

pub async fn normalize(
    connection: &mut PgConnection,
    observations: Vec<ParsedEurostatObservation>,
    statistic_kind: StatisticKind,
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
    let mut region_by_geo_code: std::collections::HashMap<String, RegionOutcome> = std::collections::HashMap::new();

    for observation in observations {
        if !region_by_geo_code.contains_key(&observation.geo_code) {
            let outcome: RegionOutcome = resolve_region(&mut *connection, &observation.geo_code).await?;

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

async fn resolve_region(connection: &mut PgConnection, geo_code: &str) -> Result<RegionOutcome, AppError> {
    if EXCLUDED_GEO_CODES.contains(&geo_code) {
        return Ok(RegionOutcome::Excluded);
    }

    let alias: Option<&GeoCodeAlias> = ISO2_BY_EUROSTAT_GEO_CODE
        .iter()
        .find(|alias| alias.geo_code == geo_code);
    let iso2: &str = alias.map_or(geo_code, |alias| alias.iso2);

    let country: Option<Country> = canonical_db::find_country_by_iso2(&mut *connection, iso2).await?;

    match country {
        Some(country) => Ok(RegionOutcome::Resolved(country.region_id)),
        None => Ok(RegionOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::UnrecognizedRegionCode,
            message: format!("code {geo_code} matches no canonical region"),
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
