//! WB WDI adapter: orchestrator (`fetch_and_store`) + the five named helpers
//! per `docs/architecture/ingestion.md` §Adapter contract.
//!
//! Pure-logic helpers (`parse_response`) are unit-tested in this file's
//! `mod tests` block. DB-touching helpers (`normalize`, `upsert_rows`) are
//! exercised through integration tests in `tests/world_bank_wdi_integration.rs`.

use sqlx::PgPool;

use crate::canonical::canonical_db;
use crate::error::AppError;
use crate::world_bank_wdi::world_bank_wdi_model::{
    IngestWarning, IngestWarningKind, NormalizedRow, ParsedRow, WdiResponse,
};

const WB_WDI_STATISTIC_CODE: &str = "tfr";
const WB_WDI_DATA_STATUS_FINAL: &str = "final";

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
pub async fn normalize(
    pool: &PgPool,
    parsed_rows: Vec<ParsedRow>,
) -> Result<(Vec<NormalizedRow>, Vec<IngestWarning>), AppError> {
    let statistic_id: uuid::Uuid = resolve_statistic_id(pool).await?;
    let mut normalized_rows: Vec<NormalizedRow> = Vec::with_capacity(parsed_rows.len());
    let mut warnings: Vec<IngestWarning> = Vec::new();
    for parsed_row in parsed_rows {
        match normalize_row(pool, &parsed_row, statistic_id).await? {
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
    pool: &PgPool,
    parsed_row: &ParsedRow,
    statistic_id: uuid::Uuid,
) -> Result<NormalizeOutcome, AppError> {
    let Some(value) = parsed_row.value else {
        return Ok(NormalizeOutcome::Warned(IngestWarning {
            kind: IngestWarningKind::NaValue,
            message: format!("wb_wdi: NA value for {} {}", parsed_row.iso3, parsed_row.year),
        }));
    };
    let Some(country) = canonical_db::find_country_by_iso3(pool, &parsed_row.iso3).await?
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

async fn resolve_statistic_id(pool: &PgPool) -> Result<uuid::Uuid, AppError> {
    let statistic = canonical_db::find_statistic_by_code(pool, WB_WDI_STATISTIC_CODE)
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
