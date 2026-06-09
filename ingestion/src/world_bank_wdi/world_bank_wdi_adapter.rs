use chrono::Utc;
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::adapter::{
    AdapterOptions, IngestWarning, IngestWarningKind, NormalizeOutcome, NormalizedStatisticValue, NaiveDatePeriod,
};
use crate::canonical::canonical_db;
use crate::canonical::canonical_model::{Country, DataSource, DataSourceKind, DataStatus, SourceRevision, Statistic, StatisticKind};
use crate::error::AppError;
use crate::ingest;
use crate::ingest::IngestReport;
use crate::world_bank_wdi::world_bank_wdi_client;
use crate::world_bank_wdi::world_bank_wdi_model::{ParsedWdiStatisticValue, WdiResponse};

/// Rows whose country isn't in the canonical seed produce an
/// `UnknownCountry` warning and are dropped. Rows with `value: None`
/// produce a `NotApplicableValue` warning and are dropped: we only persist
/// published values, and `None` means the source had no figure for that cell.
pub async fn normalize(
    connection: &mut PgConnection,
    parsed_wdi_statistic_values: Vec<ParsedWdiStatisticValue>,
) -> Result<(Vec<NormalizedStatisticValue>, Vec<IngestWarning>), AppError> {
    let statistic: Statistic =
        canonical_db::find_statistic_by_code(&mut *connection, StatisticKind::Tfr.code())
            .await?
            .ok_or_else(|| {
                AppError::from(format!(
                    "statistic {:?} missing from canonical store (run dbmate up)",
                    StatisticKind::Tfr.code(),
                ))
            })?;
    let mut normalized_statistic_values: Vec<NormalizedStatisticValue> = Vec::with_capacity(parsed_wdi_statistic_values.len());
    let mut warnings: Vec<IngestWarning> = Vec::new();

    for parsed_wdi_statistic_value in parsed_wdi_statistic_values {
        match normalize_row(&mut *connection, &parsed_wdi_statistic_value, statistic.id).await? {
            NormalizeOutcome::Normalized(row) => normalized_statistic_values.push(row),
            NormalizeOutcome::Warned(warning) => warnings.push(warning),
        }
    }

    Ok((normalized_statistic_values, warnings))
}

async fn normalize_row(
    connection: &mut PgConnection,
    parsed_wdi_statistic_value: &ParsedWdiStatisticValue,
    statistic_id: Uuid,
) -> Result<NormalizeOutcome, AppError> {
    let Some(value) = parsed_wdi_statistic_value.value else {
        return Ok(NormalizeOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::NotApplicableValue,
            message: format!("NA value for {} {}", parsed_wdi_statistic_value.iso3, parsed_wdi_statistic_value.year),
        }));
    };
    let Some(country): Option<Country> = canonical_db::find_country_by_iso3(&mut *connection, &parsed_wdi_statistic_value.iso3).await? else {
        return Ok(NormalizeOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::UnknownCountry,
            message: format!(
                "wb_wdi: unknown countryiso3code {:?} for year {}",
                parsed_wdi_statistic_value.iso3, parsed_wdi_statistic_value.year,
            ),
        }));
    };
    Ok(NormalizeOutcome::Normalized(NormalizedStatisticValue {
        region_id: country.region_id,
        statistic_id,
        period: NaiveDatePeriod::from_year(parsed_wdi_statistic_value.year)?,
        value,
        data_status: DataStatus::Final,
    }))
}

/// Runs the full client + normalize + ingest pipeline under one transaction
/// so a mid-run failure leaves the canonical store untouched.
pub async fn fetch_and_store(pool: &PgPool, options: AdapterOptions) -> Result<IngestReport, AppError> {
    let mut transaction: Transaction<'_, Postgres> = pool.begin().await?;

    let data_source: DataSource = canonical_db::find_data_source_by_kind(&mut *transaction, DataSourceKind::WorldBankWDI)
        .await?
        .ok_or_else(|| {
            AppError::from(format!(
                "data_source {:?} missing from canonical store",
                DataSourceKind::WorldBankWDI,
            ))
        })?;
    let _last_seen: Option<SourceRevision> =
        ingest::ingest_db::read_latest_publication(&mut *transaction, data_source.id).await?;

    let raw: WdiResponse = world_bank_wdi_client::fetch_upstream(options).await?;
    let revision_label: String = raw.0.lastupdated.clone();
    let published: chrono::DateTime<Utc> = world_bank_wdi_client::parse_published(&raw.0.lastupdated)?;
    let parsed_wdi_statistic_values: Vec<ParsedWdiStatisticValue> = world_bank_wdi_client::parse_response(raw)?;

    let (normalized_statistic_values, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        normalize(&mut *transaction, parsed_wdi_statistic_values).await?;

    let mut report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source.id,
        &revision_label,
        Some(published),
        Utc::now(),
        normalized_statistic_values,
    )
    .await?;
    report.warnings = warnings;

    transaction.commit().await?;
    Ok(report)
}
