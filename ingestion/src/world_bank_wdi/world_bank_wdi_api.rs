//! WB WDI adapter: orchestrator (`fetch_and_store`) + the five named helpers
//! per `docs/architecture/ingestion.md` §Adapter contract.
//!
//! Pure-logic helpers (`parse_response`) are unit-tested in this file's
//! `mod tests` block. DB-touching helpers (`normalize`, `upsert_rows`) are
//! exercised through integration tests in `tests/world_bank_wdi_integration.rs`.

use crate::error::AppError;
use crate::world_bank_wdi::world_bank_wdi_model::{ParsedRow, WdiResponse};

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
