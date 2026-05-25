//! WB WDI adapter: orchestrator (`fetch_and_store`) + the five named helpers
//! per `docs/architecture/ingestion.md` §Adapter contract.
//!
//! Pure-logic helpers (`parse_response`) are unit-tested in this file's
//! `mod tests` block. DB-touching helpers (`normalize`, `upsert_rows`) are
//! exercised through integration tests in `tests/world_bank_wdi_integration.rs`.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::canonical::canonical_db;
use crate::error::AppError;
use crate::world_bank_wdi::world_bank_wdi_db;
use crate::world_bank_wdi::world_bank_wdi_model::{
    AdapterOptions, IngestReport, IngestWarning, IngestWarningKind, NormalizedRow, ParsedRow,
    WdiResponse,
};

const WB_WDI_DATA_SOURCE_CODE: &str = "wb_wdi";
const WB_WDI_STATISTIC_CODE: &str = "tfr";
const WB_WDI_DATA_STATUS_FINAL: &str = "final";
const WB_WDI_API_URL: &str =
    "https://api.worldbank.org/v2/country/all/indicator/SP.DYN.TFRT.IN?format=json&per_page=20000";

/// Converts a deserialized WB WDI response into the parser's intermediate
/// shape. Pure function — no I/O. Per-row parse failures (non-numeric
/// `date`, missing iso3) become `AppError`s rather than silent skips so
/// the caller can decide how to handle them; today the pipeline aborts on
/// any parse error, but a future relaxation could treat them as warnings.
pub fn parse_response(raw: WdiResponse) -> Result<Vec<ParsedRow>, AppError> {
    let WdiResponse(_metadata, raw_rows) = raw;
    let mut parsed_rows: Vec<ParsedRow> = Vec::with_capacity(raw_rows.len());
    for raw_row in raw_rows {
        let parsed_row: ParsedRow = parse_row(&raw_row)?;
        parsed_rows.push(parsed_row);
    }
    Ok(parsed_rows)
}

fn parse_row(raw_row: &crate::world_bank_wdi::world_bank_wdi_model::WdiRow) -> Result<ParsedRow, AppError> {
    if raw_row.countryiso3code.is_empty() {
        return Err(AppError::from(format!(
            "wb_wdi: parse_row: empty countryiso3code (country.id={}, date={})",
            raw_row.country.id, raw_row.date,
        )));
    }
    let year: i32 = raw_row.date.parse::<i32>().map_err(|err| {
        AppError::from(format!(
            "wb_wdi: parse_row: non-numeric date {:?} for {}: {}",
            raw_row.date, raw_row.countryiso3code, err,
        ))
    })?;
    Ok(ParsedRow {
        iso3: raw_row.countryiso3code.clone(),
        year,
        value: raw_row.value,
    })
}

/// Joins parsed rows to canonical-store IDs and computes period bounds.
/// Rows whose country isn't in our seed produce an `UnknownCountry` warning
/// and are dropped from the normalized output. Rows with `value: None`
/// produce an `NaValue` warning and are dropped (we only persist published
/// values; `None` means the source has no figure to publish for that cell).
///
/// Takes `&mut PgConnection` so it can issue many lookups (one statistic
/// + N country) over the same connection without acquiring per-call; callers
/// pass `&mut *tx` (tests) or `&mut *pool.acquire().await?` (production).
pub async fn normalize(
    connection: &mut PgConnection,
    parsed_rows: Vec<ParsedRow>,
) -> Result<(Vec<NormalizedRow>, Vec<IngestWarning>), AppError> {
    let statistic_id: uuid::Uuid = resolve_statistic_id(&mut *connection).await?;
    let mut normalized_rows: Vec<NormalizedRow> = Vec::with_capacity(parsed_rows.len());
    let mut warnings: Vec<IngestWarning> = Vec::new();
    for parsed_row in parsed_rows {
        match normalize_row(&mut *connection, &parsed_row, statistic_id).await? {
            NormalizeOutcome::Normalized(row) => normalized_rows.push(row),
            NormalizeOutcome::Warned(warning) => warnings.push(warning),
        }
    }
    Ok((normalized_rows, warnings))
}

enum NormalizeOutcome {
    Normalized(NormalizedRow),
    Warned(IngestWarning),
}

async fn normalize_row(
    connection: &mut PgConnection,
    parsed_row: &ParsedRow,
    statistic_id: uuid::Uuid,
) -> Result<NormalizeOutcome, AppError> {
    let Some(value) = parsed_row.value else {
        return Ok(NormalizeOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::NaValue,
            message: format!("wb_wdi: NA value for {} {}", parsed_row.iso3, parsed_row.year),
        }));
    };
    let Some(country) = canonical_db::find_country_by_iso3(&mut *connection, &parsed_row.iso3).await?
    else {
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

async fn resolve_statistic_id(connection: &mut PgConnection) -> Result<uuid::Uuid, AppError> {
    let statistic = canonical_db::find_statistic_by_code(&mut *connection, WB_WDI_STATISTIC_CODE)
        .await?
        .ok_or_else(|| {
            AppError::from(format!(
                "wb_wdi: statistic {:?} missing from canonical store (run dbmate up)",
                WB_WDI_STATISTIC_CODE,
            ))
        })?;
    Ok(statistic.id)
}

fn year_to_period(year: i32) -> Result<(chrono::NaiveDate, chrono::NaiveDate), AppError> {
    let period_start: chrono::NaiveDate = chrono::NaiveDate::from_ymd_opt(year, 1, 1)
        .ok_or_else(|| AppError::from(format!("wb_wdi: invalid year {}", year)))?;
    let period_end: chrono::NaiveDate = chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
        .ok_or_else(|| AppError::from(format!("wb_wdi: invalid year+1 from {}", year)))?;
    Ok((period_start, period_end))
}

/// Persists a batch of normalized rows under a single publication. The
/// publication is INSERTed (or matched against an existing row with the same
/// `(data_source_id, revision_label)`) before any value writes; every
/// inserted statistic_value row points at the resulting publication id.
///
/// For each normalized row we look up the current `superseded is null` row
/// for `(region_id, statistic_id, period_start, period_end, data_source_id)`:
/// - no current row: INSERT the new row, count `values_added`
/// - current row matches new value + status: skip, count `values_skipped`
/// - current row differs: set the old row's `superseded = now()`, INSERT a
///   new row pointing at the new publication, count `values_revised`
pub async fn upsert_rows(
    connection: &mut PgConnection,
    data_source_id: Uuid,
    publication_revision_label: &str,
    publication_fetched: DateTime<Utc>,
    normalized_rows: Vec<NormalizedRow>,
) -> Result<IngestReport, AppError> {
    let publication_id: Uuid = world_bank_wdi_db::insert_publication_or_match(
        &mut *connection,
        data_source_id,
        publication_revision_label,
        publication_fetched,
    )
    .await?;
    let mut report: IngestReport = IngestReport::default();
    for normalized_row in normalized_rows {
        let outcome: UpsertOutcome =
            upsert_row(&mut *connection, data_source_id, publication_id, &normalized_row).await?;
        match outcome {
            UpsertOutcome::Added => report.values_added += 1,
            UpsertOutcome::Revised => report.values_revised += 1,
            UpsertOutcome::Skipped => report.values_skipped += 1,
        }
    }
    Ok(report)
}

enum UpsertOutcome {
    Added,
    Revised,
    Skipped,
}

async fn upsert_row(
    connection: &mut PgConnection,
    data_source_id: Uuid,
    publication_id: Uuid,
    normalized_row: &NormalizedRow,
) -> Result<UpsertOutcome, AppError> {
    let current: Option<crate::canonical::canonical_model::StatisticValue> =
        world_bank_wdi_db::find_current_value(
            &mut *connection,
            normalized_row.region_id,
            normalized_row.statistic_id,
            normalized_row.period_start,
            normalized_row.period_end,
            data_source_id,
        )
        .await?;
    if let Some(current_row) = current {
        if current_row.value == normalized_row.value
            && current_row.data_status == normalized_row.data_status
        {
            return Ok(UpsertOutcome::Skipped);
        }
        world_bank_wdi_db::set_superseded(&mut *connection, current_row.id, Utc::now()).await?;
        world_bank_wdi_db::insert_statistic_value(
            &mut *connection,
            normalized_row.region_id,
            normalized_row.statistic_id,
            normalized_row.period_start,
            normalized_row.period_end,
            normalized_row.value,
            data_source_id,
            publication_id,
            &normalized_row.data_status,
        )
        .await?;
        return Ok(UpsertOutcome::Revised);
    }
    world_bank_wdi_db::insert_statistic_value(
        &mut *connection,
        normalized_row.region_id,
        normalized_row.statistic_id,
        normalized_row.period_start,
        normalized_row.period_end,
        normalized_row.value,
        data_source_id,
        publication_id,
        &normalized_row.data_status,
    )
    .await?;
    Ok(UpsertOutcome::Added)
}

/// Calls the WB WDI HTTP API for the TFR indicator across every country and
/// every available year. WB has no native incremental query for this
/// indicator, so we always pull the full set; the per-row supersede logic in
/// `upsert_rows` keeps writes proportional to actual changes.
pub async fn fetch_upstream(_options: AdapterOptions) -> Result<WdiResponse, AppError> {
    let response: reqwest::Response = reqwest::get(WB_WDI_API_URL).await?;
    if !response.status().is_success() {
        return Err(AppError::from(format!(
            "wb_wdi: fetch_upstream: status {} from {}",
            response.status(),
            WB_WDI_API_URL,
        )));
    }
    let parsed: WdiResponse = response.json().await?;
    Ok(parsed)
}

/// Adapter orchestrator. Opens a single transaction, then chains the five
/// named helpers — read latest publication revision (informational only;
/// WB has no native incremental query) → fetch upstream → parse response →
/// normalize → upsert rows — inside that transaction. The whole batch
/// commits atomically or rolls back together, so a mid-run failure can't
/// leave the canonical store with partial publication state.
pub async fn fetch_and_store(
    pool: &PgPool,
    options: AdapterOptions,
) -> Result<IngestReport, AppError> {
    let mut transaction: sqlx::Transaction<'_, sqlx::Postgres> = pool.begin().await?;
    let data_source =
        canonical_db::find_data_source_by_code(&mut *transaction, WB_WDI_DATA_SOURCE_CODE)
            .await?
            .ok_or_else(|| {
                AppError::from(format!(
                    "wb_wdi: data_source {:?} missing from canonical store",
                    WB_WDI_DATA_SOURCE_CODE,
                ))
            })?;
    let _last_seen: Option<String> =
        world_bank_wdi_db::read_latest_publication_revision(&mut *transaction, data_source.id)
            .await?;
    let raw: WdiResponse = fetch_upstream(options).await?;
    let revision_label: String = raw.0.lastupdated.clone();
    let parsed_rows: Vec<ParsedRow> = parse_response(raw)?;
    let (normalized_rows, warnings): (Vec<NormalizedRow>, Vec<IngestWarning>) =
        normalize(&mut *transaction, parsed_rows).await?;
    let mut report: IngestReport = upsert_rows(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_bank_wdi::world_bank_wdi_model::{
        WdiCountry, WdiIndicator, WdiPagingMetadata, WdiRow,
    };

    fn metadata() -> WdiPagingMetadata {
        WdiPagingMetadata {
            page: 1,
            pages: 1,
            per_page: 100,
            total: 0,
            sourceid: "2".to_string(),
            lastupdated: "2026-04-08".to_string(),
        }
    }

    fn row(iso3: &str, date: &str, value: Option<f64>) -> WdiRow {
        let country_id: String = iso3.chars().take(2).collect();
        WdiRow {
            indicator: WdiIndicator {
                id: "SP.DYN.TFRT.IN".to_string(),
                value: "Fertility rate, total (births per woman)".to_string(),
            },
            country: WdiCountry {
                id: country_id,
                value: "test".to_string(),
            },
            countryiso3code: iso3.to_string(),
            date: date.to_string(),
            value,
            unit: String::new(),
            obs_status: String::new(),
            decimal: 1,
        }
    }

    #[test]
    fn parse_response_happy_path() {
        let raw: WdiResponse = WdiResponse(
            metadata(),
            vec![
                row("USA", "2024", Some(1.66)),
                row("DEU", "2023", Some(1.36)),
            ],
        );
        let parsed_rows: Vec<ParsedRow> = parse_response(raw).expect("parse_response succeeds");
        assert_eq!(parsed_rows.len(), 2);
        assert_eq!(parsed_rows[0].iso3, "USA");
        assert_eq!(parsed_rows[0].year, 2024);
        assert_eq!(parsed_rows[0].value, Some(1.66));
        assert_eq!(parsed_rows[1].iso3, "DEU");
        assert_eq!(parsed_rows[1].year, 2023);
    }

    #[test]
    fn parse_response_preserves_null_value() {
        let raw: WdiResponse = WdiResponse(
            metadata(),
            vec![row("USA", "2025", None)],
        );
        let parsed_rows: Vec<ParsedRow> = parse_response(raw).expect("parse_response succeeds");
        assert_eq!(parsed_rows.len(), 1);
        assert_eq!(parsed_rows[0].value, None);
    }

    #[test]
    fn parse_response_rejects_non_numeric_date() {
        let raw: WdiResponse = WdiResponse(
            metadata(),
            vec![row("USA", "twenty-twenty-four", Some(1.66))],
        );
        let result: Result<Vec<ParsedRow>, AppError> = parse_response(raw);
        assert!(result.is_err());
    }

    #[test]
    fn parse_response_rejects_empty_iso3() {
        let raw: WdiResponse = WdiResponse(
            metadata(),
            vec![row("", "2024", Some(1.66))],
        );
        let result: Result<Vec<ParsedRow>, AppError> = parse_response(raw);
        assert!(result.is_err());
    }
}
