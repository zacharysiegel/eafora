//! Integration tests for the WB WDI adapter pipeline against `eafora_test`.
//! Each test opens its own transaction, runs the adapter code through it,
//! and rolls back at teardown — Postgres MVCC provides full isolation so
//! tests run in parallel without table contention.

mod helpers;

use chrono::{NaiveDate, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use ingestion::adapter::*;
use ingestion::canonical::canonical_db;
use ingestion::ingest;
use ingestion::ingest::*;
use ingestion::world_bank_wdi::world_bank_wdi_adapter;
use ingestion::world_bank_wdi::world_bank_wdi_model::ParsedRow;

async fn get_wb_wdi_data_source_id(transaction: &mut Transaction<'static, Postgres>) -> Uuid {
    canonical_db::find_data_source_by_code(&mut **transaction, "wb_wdi")
        .await
        .expect("find wb_wdi")
        .expect("wb_wdi exists")
        .id
}

async fn get_usa_region_id(transaction: &mut Transaction<'static, Postgres>) -> Uuid {
    canonical_db::find_country_by_iso3(&mut **transaction, "USA")
        .await
        .expect("find USA")
        .expect("USA exists")
        .region_id
}

async fn get_tfr_statistic_id(transaction: &mut Transaction<'static, Postgres>) -> Uuid {
    canonical_db::find_statistic_by_code(&mut **transaction, "tfr")
        .await
        .expect("find tfr")
        .expect("tfr exists")
        .id
}

fn usa_2024(region_id: Uuid, statistic_id: Uuid, value: f64) -> NormalizedRow {
    NormalizedRow {
        region_id,
        statistic_id,
        period: Period::from_year(2024).unwrap(),
        value,
        data_status: "final".to_string(),
    }
}

#[tokio::test]
async fn normalize_known_country_resolves_region_id() {
    let pool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();
    let parsed_rows: Vec<ParsedRow> = vec![ParsedRow {
        iso3: "USA".to_string(),
        year: 2024,
        value: Some(1.66),
    }];
    let (normalized_rows, warnings) =
        world_bank_wdi_adapter::normalize(&mut *transaction, parsed_rows)
            .await
            .expect("normalize succeeds");
    assert_eq!(normalized_rows.len(), 1);
    assert!(warnings.is_empty());
    let row: &NormalizedRow = &normalized_rows[0];
    assert_eq!(row.period.start, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
    assert_eq!(row.period.end, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
    assert_eq!(row.value, 1.66);
    assert_eq!(row.data_status, "final");
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_unknown_country_warns_and_skips() {
    let pool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();
    let parsed_rows: Vec<ParsedRow> = vec![ParsedRow {
        iso3: "XKX".to_string(),
        year: 2024,
        value: Some(1.5),
    }];
    let (normalized_rows, warnings) =
        world_bank_wdi_adapter::normalize(&mut *transaction, parsed_rows)
            .await
            .expect("normalize succeeds");
    assert!(normalized_rows.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, IngestWarningKind::UnknownCountry);
    assert!(warnings[0].message.contains("XKX"));
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_null_value_warns_and_skips() {
    let pool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();
    let parsed_rows: Vec<ParsedRow> = vec![ParsedRow {
        iso3: "USA".to_string(),
        year: 2025,
        value: None,
    }];
    let (normalized_rows, warnings) =
        world_bank_wdi_adapter::normalize(&mut *transaction, parsed_rows)
            .await
            .expect("normalize succeeds");
    assert!(normalized_rows.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, IngestWarningKind::NaValue);
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn upsert_inserts_new_publication_and_value() {
    let pool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();
    let data_source_id: Uuid = get_wb_wdi_data_source_id(&mut transaction).await;
    let statistic_id: Uuid = get_tfr_statistic_id(&mut transaction).await;
    let region_id: Uuid = get_usa_region_id(&mut transaction).await;
    let report: IngestReport = ingest::upsert_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-upsert_inserts_new",
        Utc::now(),
        vec![usa_2024(region_id, statistic_id, 1.66)],
    )
    .await
    .expect("upsert_statistic_values succeeds");
    assert_eq!(report.values_added, 1);
    assert_eq!(report.values_revised, 0);
    assert_eq!(report.values_skipped, 0);
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn upsert_re_fetch_same_revision_matches_publication_and_skips() {
    let pool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();
    let data_source_id: Uuid = get_wb_wdi_data_source_id(&mut transaction).await;
    let statistic_id: Uuid = get_tfr_statistic_id(&mut transaction).await;
    let region_id: Uuid = get_usa_region_id(&mut transaction).await;
    let row = || usa_2024(region_id, statistic_id, 1.66);
    ingest::upsert_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-upsert_refetch",
        Utc::now(),
        vec![row()],
    )
    .await
    .expect("first upsert");
    let report: IngestReport = ingest::upsert_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-upsert_refetch",
        Utc::now(),
        vec![row()],
    )
    .await
    .expect("second upsert");
    assert_eq!(report.values_added, 0);
    assert_eq!(report.values_skipped, 1);
    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn upsert_revised_value_supersedes_old_and_inserts_new() {
    let pool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();
    let data_source_id: Uuid = get_wb_wdi_data_source_id(&mut transaction).await;
    let statistic_id: Uuid = get_tfr_statistic_id(&mut transaction).await;
    let region_id: Uuid = get_usa_region_id(&mut transaction).await;
    ingest::upsert_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-upsert_revised-rev1",
        Utc::now(),
        vec![usa_2024(region_id, statistic_id, 1.66)],
    )
    .await
    .expect("first upsert");
    let report: IngestReport = ingest::upsert_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-upsert_revised-rev2",
        Utc::now(),
        vec![usa_2024(region_id, statistic_id, 1.62)],
    )
    .await
    .expect("revised upsert");
    assert_eq!(report.values_revised, 1);
    assert_eq!(report.values_added, 0);
    assert_eq!(report.values_skipped, 0);
    let current = ingest::ingest_db::find_current_value(
        &mut *transaction,
        &usa_2024(region_id, statistic_id, 0.0),
        data_source_id,
    )
    .await
    .expect("find current succeeds");
    let current_row = current.expect("current row exists");
    assert_eq!(current_row.value, 1.62);
    let scoped_rows: i64 = sqlx::query_scalar!(
        "select count(*) as \"count!\" from statistic_value where region_id = $1 and period_start = $2",
        region_id,
        NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
    )
    .fetch_one(&mut *transaction)
    .await
    .expect("count");
    assert_eq!(scoped_rows, 2);
    transaction.rollback().await.unwrap();
}
