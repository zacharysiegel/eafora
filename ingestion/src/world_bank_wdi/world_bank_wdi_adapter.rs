//! WB WDI adapter: normalizes parsed WB rows into the canonical
//! `NormalizedRow` shape and orchestrates the full pipeline
//! (client → adapter → ingest) under one transaction. The fetch + parse
//! steps live in `world_bank_wdi_client`; the persistence step lives in
//! `crate::ingest`.

use chrono::Utc;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::adapter::{
    AdapterOptions, IngestWarning, IngestWarningKind, NormalizeOutcome, NormalizedRow, year_to_period,
};
use crate::canonical::canonical_db;
use crate::error::AppError;
use crate::ingest;
use crate::ingest::IngestReport;
use crate::world_bank_wdi::world_bank_wdi_client;
use crate::world_bank_wdi::world_bank_wdi_model::{ParsedRow, WdiResponse};

const WB_WDI_DATA_SOURCE_CODE: &str = "wb_wdi";
const WB_WDI_STATISTIC_CODE: &str = "tfr";
const WB_WDI_DATA_STATUS_FINAL: &str = "final";

/// Joins parsed rows to canonical-store IDs and computes period bounds.
/// Rows whose country isn't in our seed produce an `UnknownCountry` warning
/// and are dropped from the normalized output. Rows with `value: None`
/// produce an `NaValue` warning and are dropped (we only persist published
/// values; `None` means the source has no figure to publish for that cell).
pub async fn normalize(
    connection: &mut PgConnection,
    parsed_rows: Vec<ParsedRow>,
) -> Result<(Vec<NormalizedRow>, Vec<IngestWarning>), AppError> {
    let statistic =
        canonical_db::find_statistic_by_code(&mut *connection, WB_WDI_STATISTIC_CODE)
            .await?
            .ok_or_else(|| {
                AppError::from(format!(
                    "wb_wdi: statistic {WB_WDI_STATISTIC_CODE:?} missing from canonical store (run dbmate up)",
                ))
            })?;
    let mut normalized_rows: Vec<NormalizedRow> = Vec::with_capacity(parsed_rows.len());
    let mut warnings: Vec<IngestWarning> = Vec::new();
    for parsed_row in parsed_rows {
        match normalize_row(&mut *connection, &parsed_row, statistic.id).await? {
            NormalizeOutcome::Normalized(row) => normalized_rows.push(row),
            NormalizeOutcome::Warned(warning) => warnings.push(warning),
        }
    }
    Ok((normalized_rows, warnings))
}

async fn normalize_row(
    connection: &mut PgConnection,
    parsed_row: &ParsedRow,
    statistic_id: Uuid,
) -> Result<NormalizeOutcome, AppError> {
    let Some(value) = parsed_row.value else {
        return Ok(NormalizeOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::NaValue,
            message: format!("wb_wdi: NA value for {} {}", parsed_row.iso3, parsed_row.year),
        }));
    };
    let Some(country) = canonical_db::find_country_by_iso3(&mut *connection, &parsed_row.iso3).await? else {
        return Ok(NormalizeOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::UnknownCountry,
            message: format!(
                "wb_wdi: unknown countryiso3code {:?} for year {}",
                parsed_row.iso3, parsed_row.year,
            ),
        }));
    };
    let (period_start, period_end) = year_to_period(parsed_row.year)?;
    Ok(NormalizeOutcome::Normalized(NormalizedRow {
        region_id: country.region_id,
        statistic_id,
        period_start,
        period_end,
        value,
        data_status: WB_WDI_DATA_STATUS_FINAL.to_string(),
    }))
}

/// Adapter orchestrator. Opens a single transaction, then chains
/// `read_latest_publication_revision` (informational only; WB has no
/// native incremental query) → client::fetch_upstream → client::parse_response
/// → normalize → ingest::upsert_rows under that transaction. The whole
/// batch commits atomically or rolls back together, so a mid-run failure
/// can't leave the canonical store with partial publication state.
pub async fn fetch_and_store(pool: &PgPool, options: AdapterOptions) -> Result<IngestReport, AppError> {
    let mut transaction: sqlx::Transaction<'_, sqlx::Postgres> = pool.begin().await?;
    let data_source = canonical_db::find_data_source_by_code(&mut *transaction, WB_WDI_DATA_SOURCE_CODE)
        .await?
        .ok_or_else(|| {
            AppError::from(format!(
                "wb_wdi: data_source {:?} missing from canonical store",
                WB_WDI_DATA_SOURCE_CODE,
            ))
        })?;
    let _last_seen: Option<String> =
        ingest::ingest_db::read_latest_publication_revision(&mut *transaction, data_source.id).await?;
    let raw: WdiResponse = world_bank_wdi_client::fetch_upstream(options).await?;
    let revision_label: String = raw.0.lastupdated.clone();
    let parsed_rows: Vec<ParsedRow> = world_bank_wdi_client::parse_response(raw)?;
    let (normalized_rows, warnings): (Vec<NormalizedRow>, Vec<IngestWarning>) =
        normalize(&mut *transaction, parsed_rows).await?;
    let mut report: IngestReport = ingest::upsert_rows(
        &mut *transaction,
        data_source.id,
        &revision_label,
        Utc::now(),
        normalized_rows,
    )
    .await?;
    report.warnings = warnings;
    transaction.commit().await?;
    Ok(report)
}
