use chrono::{NaiveTime, Utc};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use shared::canonical::canonical_model::{
    Country, DataSource, DataSourceKind, DataStatus, NaiveDatePeriod, SourceRevision, Statistic,
    StatisticKind,
};

use crate::adapter::{self, AdapterOptions, IngestWarning, IngestWarningKind, NormalizedStatisticValue};
use crate::canonical::canonical_db;
use crate::error::AppError;
use crate::hfd::hfd_client;
use crate::hfd::hfd_model::{ParsedHfdPublication, ParsedHfdStatisticValue};
use crate::ingest;
use crate::ingest::IngestReport;

/// `NP` is HFD's suffix for a country's entire national population; it also publishes narrower series for
/// the same country, such as East and West Germany separately or France's civilians only.
const ISO3_BY_HFD_CODE: [HfdCodeAlias; 3] = [
    HfdCodeAlias { hfd_code: "DEUTNP", iso3: "DEU" },
    HfdCodeAlias { hfd_code: "FRATNP", iso3: "FRA" },
    HfdCodeAlias { hfd_code: "GBR_NP", iso3: "GBR" },
];

struct HfdCodeAlias {
    hfd_code: &'static str,
    iso3: &'static str,
}

enum RegionOutcome {
    Resolved(Uuid),
    Warned(IngestWarning),
}

/// One transaction over the whole run, so a mid-run failure leaves the canonical store untouched.
pub async fn fetch_and_store(pool: &PgPool, options: AdapterOptions) -> Result<IngestReport, AppError> {
    let mut transaction: Transaction<'_, Postgres> = pool.begin().await?;

    let data_source: DataSource =
        canonical_db::find_data_source_by_kind(&mut *transaction, DataSourceKind::HumanFertilityDatabase)
            .await?
            .ok_or_else(|| {
                AppError::from(format!(
                    "data_source {:?} missing from canonical store",
                    DataSourceKind::HumanFertilityDatabase,
                ))
            })?;
    let last_seen: Option<SourceRevision> =
        ingest::ingest_db::read_latest_publication(&mut *transaction, data_source.id).await?;

    let archive: Vec<u8> = hfd_client::fetch_upstream().await?;
    let cohort_contents: String = hfd_client::read_member(&archive, hfd_client::COHORT_MEMBER)?;
    let period_contents: String = hfd_client::read_member(&archive, hfd_client::PERIOD_MEMBER)?;

    let (publication, parsed_cohort_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
        hfd_client::parse_fertility_file(&cohort_contents, hfd_client::COHORT_FERTILITY_COLUMNS)?;
    let (_, parsed_period_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
        hfd_client::parse_fertility_file(&period_contents, hfd_client::PERIOD_FERTILITY_COLUMNS)?;

    /* HFD prints its last-modification date inside the file, so an unchanged upstream is only knowable
       after the download; the request is unavoidable, the write is not. */
    if adapter::should_skip_run(&last_seen, &publication.revision_label, options) {
        log::info!(
            "hfd is unchanged since the last run; [revision_label={}]",
            publication.revision_label,
        );

        return Ok(IngestReport::default());
    }

    let (cohort_values, cohort_warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize(&mut *transaction, parsed_cohort_values, StatisticKind::Ccf).await?;
    let (period_values, period_warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize(&mut *transaction, parsed_period_values, StatisticKind::Tfr).await?;

    let mut normalized_statistic_values: Vec<NormalizedStatisticValue> = cohort_values;
    normalized_statistic_values.extend(period_values);

    let mut report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source.id,
        &publication.revision_label,
        Some(publication.last_modified.and_time(NaiveTime::MIN).and_utc()),
        Utc::now(),
        normalized_statistic_values,
    )
    .await?;
    report.warnings = cohort_warnings;
    report.warnings.extend(period_warnings);

    transaction.commit().await?;

    Ok(report)
}

/// An absent value means the cohort has not finished childbearing, which is the normal state of the newest
/// cohorts of every country, so it is dropped without a warning.
pub async fn normalize(
    connection: &mut PgConnection,
    parsed_hfd_statistic_values: Vec<ParsedHfdStatisticValue>,
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

    let mut statistic_values: Vec<NormalizedStatisticValue> =
        Vec::with_capacity(parsed_hfd_statistic_values.len());
    let mut warnings: Vec<IngestWarning> = Vec::new();
    let mut codes_yielding_nothing: Vec<String> = Vec::new();

    for (hfd_code, parsed_values) in group_by_code(parsed_hfd_statistic_values) {
        let region_id: Uuid = match resolve_region(&mut *connection, &hfd_code).await? {
            RegionOutcome::Resolved(region_id) => region_id,
            RegionOutcome::Warned(warning) => {
                warnings.push(warning);
                continue;
            }
        };

        let mut values_for_code: u64 = 0;

        for parsed_value in parsed_values {
            let normalized: Option<NormalizedStatisticValue> =
                normalize_row(&parsed_value, region_id, statistic.id)?;

            if let Some(statistic_value) = normalized {
                statistic_values.push(statistic_value);
                values_for_code += 1;
            }
        }

        if values_for_code == 0 {
            codes_yielding_nothing.push(hfd_code);
        }
    }

    for hfd_code in codes_yielding_nothing {
        warnings.push(IngestWarning {
            kind: IngestWarningKind::NoValuesForRegion,
            message: format!("code {hfd_code} has no completed cohort in this release"),
        });
    }

    Ok((statistic_values, warnings))
}

/// Preserves upstream order, so warnings come out in the order the file lists the codes.
fn group_by_code(
    parsed_hfd_statistic_values: Vec<ParsedHfdStatisticValue>,
) -> Vec<(String, Vec<ParsedHfdStatisticValue>)> {
    let mut grouped: Vec<(String, Vec<ParsedHfdStatisticValue>)> = Vec::new();

    for parsed_value in parsed_hfd_statistic_values {
        let existing: Option<&mut (String, Vec<ParsedHfdStatisticValue>)> = grouped
            .iter_mut()
            .find(|(hfd_code, _)| *hfd_code == parsed_value.hfd_code);

        match existing {
            Some((_, values)) => values.push(parsed_value),
            None => grouped.push((parsed_value.hfd_code.clone(), vec![parsed_value])),
        }
    }

    grouped
}

async fn resolve_region(
    connection: &mut PgConnection,
    hfd_code: &str,
) -> Result<RegionOutcome, AppError> {
    let alias: Option<&HfdCodeAlias> = ISO3_BY_HFD_CODE
        .iter()
        .find(|alias| alias.hfd_code == hfd_code);
    let iso3: &str = match alias {
        Some(alias) => alias.iso3,
        None => hfd_code,
    };

    let country: Option<Country> = canonical_db::find_country_by_iso3(&mut *connection, iso3).await?;

    match country {
        Some(country) => Ok(RegionOutcome::Resolved(country.region_id)),
        None => Ok(RegionOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::UnrecognizedRegionCode,
            message: format!("code {hfd_code} matches no canonical region"),
        })),
    }
}

fn normalize_row(
    parsed_hfd_statistic_value: &ParsedHfdStatisticValue,
    region_id: Uuid,
    statistic_id: Uuid,
) -> Result<Option<NormalizedStatisticValue>, AppError> {
    let Some(value) = parsed_hfd_statistic_value.value
    else {
        return Ok(None);
    };

    Ok(Some(NormalizedStatisticValue {
        region_id,
        statistic_id,
        period: NaiveDatePeriod::from_year(parsed_hfd_statistic_value.period_year)?,
        value,
        data_status: DataStatus::Final,
    }))
}
