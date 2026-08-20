use chrono::{NaiveTime, Utc};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use shared::canonical::canonical_model::{
    Country, DataSource, DataSourceKind, DataStatus, NaiveDatePeriod, SourceRevision, Statistic,
};

use crate::adapter::{
    AdapterOptions, IngestWarning, IngestWarningKind, NormalizeOutcome, NormalizedStatisticValue,
};
use crate::canonical::canonical_db;
use crate::error::AppError;
use crate::hfd::hfd_client;
use crate::hfd::hfd_client::CohortFertilityFile;
use crate::hfd::hfd_model::{ParsedHfdPublication, ParsedHfdStatisticValue};
use crate::ingest;
use crate::ingest::IngestReport;

/// The statistic HFD's cohort file supplies. Resolved by code rather than through `StatisticKind`, which
/// has no variant for it until a client can render a cohort.
pub const COMPLETED_COHORT_FERTILITY_CODE: &str = "ccf";

/// HFD's national totals, which carry a suffix rather than a bare ISO 3166-1 alpha-3 code. `NP` is HFD's
/// marker for the whole national population, as against a territory or a civilian-only series.
const NATIONAL_TOTAL_CODES: [NationalTotalCode; 3] = [
    NationalTotalCode { hfd_code: "DEUTNP", iso3: "DEU" },
    NationalTotalCode { hfd_code: "FRATNP", iso3: "FRA" },
    NationalTotalCode { hfd_code: "GBR_NP", iso3: "GBR" },
];

/// Territories and constituent countries HFD publishes alongside the national totals. Listed so their
/// warning says what they are, rather than reporting them as countries missing from the canonical seed.
const SUBPOPULATION_CODES: [&str; 5] = ["DEUTE", "DEUTW", "GBRTENW", "GBR_NIR", "GBR_SCO"];

struct NationalTotalCode {
    hfd_code: &'static str,
    iso3: &'static str,
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

    let cohort_files: Vec<CohortFertilityFile> = hfd_client::fetch_upstream().await?;
    let cohort_file: &CohortFertilityFile = cohort_files
        .first()
        .ok_or_else(|| AppError::from("the hfd archive carries no cohort fertility file"))?;

    let (publication, parsed_hfd_statistic_values): (ParsedHfdPublication, Vec<ParsedHfdStatisticValue>) =
        hfd_client::parse_cohort_file(&cohort_file.contents)?;

    /* HFD prints its last-modification date inside the file, so an unchanged upstream is only knowable
       after the download; the request is unavoidable, the write is not. */
    if should_skip_run(&last_seen, &publication.revision_label, options) {
        log::info!(
            "hfd is unchanged since the last run; [revision_label={}]",
            publication.revision_label,
        );

        return Ok(IngestReport::default());
    }

    let normalized: NormalizedCohortValues =
        normalize(&mut *transaction, parsed_hfd_statistic_values).await?;

    let mut report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source.id,
        &publication.revision_label,
        Some(publication.last_modified.and_time(NaiveTime::MIN).and_utc()),
        Utc::now(),
        normalized.statistic_values,
    )
    .await?;
    report.warnings = normalized.warnings;
    report.values_absent_upstream = normalized.absent_upstream_count;

    transaction.commit().await?;

    Ok(report)
}

pub fn should_skip_run(
    last_seen: &Option<SourceRevision>,
    revision_label: &str,
    options: AdapterOptions,
) -> bool {
    if options.force_full_refetch {
        return false;
    }

    match last_seen {
        Some(last_seen) => last_seen.revision == revision_label,
        None => false,
    }
}

pub struct NormalizedCohortValues {
    pub statistic_values: Vec<NormalizedStatisticValue>,
    pub warnings: Vec<IngestWarning>,
    pub absent_upstream_count: u64,
}

/// An absent value is counted rather than warned: it means the cohort has not finished childbearing, which
/// is the normal state of the newest cohorts of every country. A code that resolves to a region but yields
/// nothing at all does warn, once.
pub async fn normalize(
    connection: &mut PgConnection,
    parsed_hfd_statistic_values: Vec<ParsedHfdStatisticValue>,
) -> Result<NormalizedCohortValues, AppError> {
    let statistic: Statistic =
        canonical_db::find_statistic_by_code(&mut *connection, COMPLETED_COHORT_FERTILITY_CODE)
            .await?
            .ok_or_else(|| {
                AppError::from(format!(
                    "statistic {:?} missing from canonical store (run dbmate up)",
                    COMPLETED_COHORT_FERTILITY_CODE,
                ))
            })?;

    let mut statistic_values: Vec<NormalizedStatisticValue> =
        Vec::with_capacity(parsed_hfd_statistic_values.len());
    let mut warnings: Vec<IngestWarning> = Vec::new();
    let mut absent_upstream_count: u64 = 0;
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
            match normalize_row(&parsed_value, region_id, statistic.id)? {
                NormalizeOutcome::Normalized(statistic_value) => {
                    statistic_values.push(statistic_value);
                    values_for_code += 1;
                }
                NormalizeOutcome::Warned(_) => absent_upstream_count += 1,
            }
        }

        if values_for_code == 0 {
            codes_yielding_nothing.push(hfd_code);
        }
    }

    for hfd_code in codes_yielding_nothing {
        warnings.push(IngestWarning {
            kind: IngestWarningKind::NoValuesForRegion,
            message: format!("hfd published no completed cohort for {hfd_code}"),
        });
    }

    Ok(NormalizedCohortValues { statistic_values, warnings, absent_upstream_count })
}

/// Grouped so a code resolves to its region once rather than once per cohort, and so a code that yields
/// nothing is recognisable as such. Insertion order is preserved, keeping warnings in upstream order.
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

enum RegionOutcome {
    Resolved(Uuid),
    Warned(IngestWarning),
}

async fn resolve_region(
    connection: &mut PgConnection,
    hfd_code: &str,
) -> Result<RegionOutcome, AppError> {
    if SUBPOPULATION_CODES.contains(&hfd_code) {
        return Ok(RegionOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::SubpopulationCode,
            message: format!("hfd code {hfd_code} names a subpopulation, which has no canonical region"),
        }));
    }

    let national_total: Option<&NationalTotalCode> = NATIONAL_TOTAL_CODES
        .iter()
        .find(|national_total| national_total.hfd_code == hfd_code);
    let iso3: &str = match national_total {
        Some(national_total) => national_total.iso3,
        None => hfd_code,
    };

    let country: Option<Country> = canonical_db::find_country_by_iso3(&mut *connection, iso3).await?;

    match country {
        Some(country) => Ok(RegionOutcome::Resolved(country.region_id)),
        None => Ok(RegionOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::UnknownCountry,
            message: format!("hfd code {hfd_code} matches no canonical country; [iso3={iso3}]"),
        })),
    }
}

fn normalize_row(
    parsed_hfd_statistic_value: &ParsedHfdStatisticValue,
    region_id: Uuid,
    statistic_id: Uuid,
) -> Result<NormalizeOutcome, AppError> {
    let Some(value) = parsed_hfd_statistic_value.value
    else {
        return Ok(NormalizeOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::NotApplicableValue,
            message: format!(
                "hfd has no completed cohort for {} {}",
                parsed_hfd_statistic_value.hfd_code, parsed_hfd_statistic_value.cohort_year,
            ),
        }));
    };

    Ok(NormalizeOutcome::Normalized(NormalizedStatisticValue {
        region_id,
        statistic_id,
        period: NaiveDatePeriod::from_year(parsed_hfd_statistic_value.cohort_year)?,
        value,
        data_status: DataStatus::Final,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_revision(revision: &str) -> SourceRevision {
        SourceRevision {
            revision: revision.to_string(),
            published: None,
            fetched: "2026-07-02T00:00:00Z".parse().unwrap(),
        }
    }

    fn options(force_full_refetch: bool) -> AdapterOptions {
        AdapterOptions { force_full_refetch }
    }

    #[test]
    fn should_skip_run_runs_on_a_first_run() {
        assert!(!should_skip_run(&None, "2026-07-02", options(false)));
    }

    #[test]
    fn should_skip_run_skips_an_unchanged_revision() {
        let last_seen: Option<SourceRevision> = Some(source_revision("2026-07-02"));

        assert!(should_skip_run(&last_seen, "2026-07-02", options(false)));
    }

    #[test]
    fn should_skip_run_runs_a_changed_revision() {
        let last_seen: Option<SourceRevision> = Some(source_revision("2026-07-02"));

        assert!(!should_skip_run(&last_seen, "2026-12-01", options(false)));
    }

    #[test]
    fn should_skip_run_honours_the_force_override_for_an_unchanged_revision() {
        let last_seen: Option<SourceRevision> = Some(source_revision("2026-07-02"));

        assert!(!should_skip_run(&last_seen, "2026-07-02", options(true)));
    }
}
