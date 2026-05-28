use crate::adapter::AdapterOptions;
use crate::error::AppError;
use crate::world_bank_wdi::world_bank_wdi_model::{ParsedWdiStatisticValue, WdiResponse, WdiStatisticValue};

const WB_WDI_API_URL: &str =
    "https://api.worldbank.org/v2/country/all/indicator/SP.DYN.TFRT.IN?format=json&per_page=20000";

/// WB has no native incremental query for TFR, so we always pull the full
/// set. The per-row supersede logic in `ingest::record_statistic_values`
/// keeps writes proportional to actual changes.
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

pub fn parse_response(raw: WdiResponse) -> Result<Vec<ParsedWdiStatisticValue>, AppError> {
    let WdiResponse(_metadata, raw_wdi_statistic_values) = raw;
    let mut parsed_wdi_statistic_values: Vec<ParsedWdiStatisticValue> = Vec::with_capacity(raw_wdi_statistic_values.len());

    for raw_wdi_statistic_value in raw_wdi_statistic_values {
        // WB intermixes regional aggregates (e.g. country.id="XD"
        // "Late-demographic dividend") with country rows. Aggregates have
        // empty `countryiso3code` and aren't country data, so drop them
        // before normalize sees them.
        if raw_wdi_statistic_value.countryiso3code.is_empty() {
            continue;
        }
        let parsed_wdi_statistic_value: ParsedWdiStatisticValue = parse_row(&raw_wdi_statistic_value)?;
        parsed_wdi_statistic_values.push(parsed_wdi_statistic_value);
    }

    Ok(parsed_wdi_statistic_values)
}

fn parse_row(raw_wdi_statistic_value: &WdiStatisticValue) -> Result<ParsedWdiStatisticValue, AppError> {
    let year: i32 = raw_wdi_statistic_value.date.parse::<i32>().map_err(|err| {
        AppError::from(format!(
            "wb_wdi: parse_row: non-numeric date {:?} for {}: {}",
            raw_wdi_statistic_value.date, raw_wdi_statistic_value.countryiso3code, err,
        ))
    })?;
    Ok(ParsedWdiStatisticValue {
        iso3: raw_wdi_statistic_value.countryiso3code.clone(),
        year,
        value: raw_wdi_statistic_value.value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_bank_wdi::world_bank_wdi_model::{WdiCountry, WdiIndicator, WdiPagingMetadata, WdiStatisticValue};

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

    fn row(iso3: &str, date: &str, value: Option<f64>) -> WdiStatisticValue {
        let country_id: String = iso3.chars().take(2).collect();
        WdiStatisticValue {
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
            vec![row("USA", "2024", Some(1.66)), row("DEU", "2023", Some(1.36))],
        );
        let parsed_wdi_statistic_values: Vec<ParsedWdiStatisticValue> = parse_response(raw).expect("parse_response succeeds");
        assert_eq!(parsed_wdi_statistic_values.len(), 2);
        assert_eq!(parsed_wdi_statistic_values[0].iso3, "USA");
        assert_eq!(parsed_wdi_statistic_values[0].year, 2024);
        assert_eq!(parsed_wdi_statistic_values[0].value, Some(1.66));
        assert_eq!(parsed_wdi_statistic_values[1].iso3, "DEU");
        assert_eq!(parsed_wdi_statistic_values[1].year, 2023);
    }

    #[test]
    fn parse_response_preserves_null_value() {
        let raw: WdiResponse = WdiResponse(metadata(), vec![row("USA", "2025", None)]);
        let parsed_wdi_statistic_values: Vec<ParsedWdiStatisticValue> = parse_response(raw).expect("parse_response succeeds");
        assert_eq!(parsed_wdi_statistic_values.len(), 1);
        assert_eq!(parsed_wdi_statistic_values[0].value, None);
    }

    #[test]
    fn parse_response_rejects_non_numeric_date() {
        let raw: WdiResponse = WdiResponse(metadata(), vec![row("USA", "twenty-twenty-four", Some(1.66))]);
        let result: Result<Vec<ParsedWdiStatisticValue>, AppError> = parse_response(raw);
        assert!(result.is_err());
    }

    #[test]
    fn parse_response_drops_empty_iso3_aggregates() {
        let raw: WdiResponse = WdiResponse(
            metadata(),
            vec![
                row("USA", "2024", Some(1.66)),
                row("", "2024", Some(2.10)),
                row("DEU", "2023", Some(1.36)),
            ],
        );
        let parsed_wdi_statistic_values: Vec<ParsedWdiStatisticValue> = parse_response(raw).expect("parse_response succeeds");
        assert_eq!(parsed_wdi_statistic_values.len(), 2);
        assert_eq!(parsed_wdi_statistic_values[0].iso3, "USA");
        assert_eq!(parsed_wdi_statistic_values[1].iso3, "DEU");
    }
}
