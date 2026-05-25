//! Integration tests for the WB WDI adapter pipeline against `eafora_test`.
//! Exercises `parse_response` → `normalize` → `upsert_rows` end-to-end via
//! checked-in sample responses (no live HTTP).

mod helpers;

use chrono::NaiveDate;

use ingestion::world_bank_wdi::world_bank_wdi_api;
use ingestion::world_bank_wdi::world_bank_wdi_model::{
    IngestWarningKind, NormalizedRow, ParsedRow,
};

#[tokio::test]
async fn normalize_known_country_resolves_region_id() {
    let pool = helpers::test_db::test_pool().await;
    let parsed_rows: Vec<ParsedRow> = vec![ParsedRow {
        iso3: "USA".to_string(),
        year: 2024,
        value: Some(1.66),
    }];
    let (normalized_rows, warnings) = world_bank_wdi_api::normalize(pool, parsed_rows)
        .await
        .expect("normalize succeeds");
    assert_eq!(normalized_rows.len(), 1);
    assert!(warnings.is_empty());
    let row: &NormalizedRow = &normalized_rows[0];
    assert_eq!(row.period_start, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
    assert_eq!(row.period_end, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
    assert_eq!(row.value, 1.66);
    assert_eq!(row.data_status, "final");
}

#[tokio::test]
async fn normalize_unknown_country_warns_and_skips() {
    let pool = helpers::test_db::test_pool().await;
    let parsed_rows: Vec<ParsedRow> = vec![ParsedRow {
        iso3: "XKX".to_string(),
        year: 2024,
        value: Some(1.5),
    }];
    let (normalized_rows, warnings) = world_bank_wdi_api::normalize(pool, parsed_rows)
        .await
        .expect("normalize succeeds");
    assert!(normalized_rows.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, IngestWarningKind::UnknownCountry);
    assert!(warnings[0].message.contains("XKX"));
}

#[tokio::test]
async fn normalize_null_value_warns_and_skips() {
    let pool = helpers::test_db::test_pool().await;
    let parsed_rows: Vec<ParsedRow> = vec![ParsedRow {
        iso3: "USA".to_string(),
        year: 2025,
        value: None,
    }];
    let (normalized_rows, warnings) = world_bank_wdi_api::normalize(pool, parsed_rows)
        .await
        .expect("normalize succeeds");
    assert!(normalized_rows.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, IngestWarningKind::NaValue);
}
