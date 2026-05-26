//! Integration tests for the WB WDI adapter pipeline against `eafora_test`.
//! Each test opens its own transaction, runs the adapter code through it,
//! and rolls back at teardown — Postgres MVCC provides full isolation so
//! tests run in parallel without table contention.

mod helpers;

use chrono::{NaiveDate, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use ingestion::adapter::*;
use ingestion::canonical::canonical_model::{DataStatus, StatisticValue};
use ingestion::ingest;
use ingestion::ingest::*;
use ingestion::world_bank_wdi::world_bank_wdi_adapter;
use ingestion::world_bank_wdi::world_bank_wdi_model::ParsedWdiStatisticValue;

use helpers::canonical::{get_country_region_id, get_data_source_id, get_statistic_id};

/// Builds a `NormalizedStatisticValue` for the given country region + year
/// + value, defaulted to `DataStatus::Final`. Used by record-* tests that
/// need a known-shape input without going through `normalize`.
fn new_normalized_statistic_value(
    region_id: Uuid,
    statistic_id: Uuid,
    year: i32,
    value: f64,
) -> NormalizedStatisticValue {
    NormalizedStatisticValue {
        region_id,
        statistic_id,
        period: NaiveDatePeriod::from_year(year).expect("valid year"),
        value,
        data_status: DataStatus::Final,
    }
}

#[tokio::test]
async fn normalize_known_country_resolves_region_id() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let parsed: Vec<ParsedWdiStatisticValue> = vec![ParsedWdiStatisticValue {
        iso3: "USA".to_string(),
        year: 2024,
        value: Some(1.66),
    }];

    let (normalized, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        world_bank_wdi_adapter::normalize(&mut *transaction, parsed)
            .await
            .expect("normalize succeeds");

    assert_eq!(normalized.len(), 1);
    assert!(warnings.is_empty());

    let normalized_statistic_value: &NormalizedStatisticValue = &normalized[0];
    assert_eq!(normalized_statistic_value.period.start, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());
    assert_eq!(normalized_statistic_value.period.end, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
    assert_eq!(normalized_statistic_value.value, 1.66);

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_unknown_country_warns_and_skips() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let parsed: Vec<ParsedWdiStatisticValue> = vec![ParsedWdiStatisticValue {
        iso3: "XKX".to_string(),
        year: 2024,
        value: Some(1.5),
    }];

    let (normalized, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        world_bank_wdi_adapter::normalize(&mut *transaction, parsed)
            .await
            .expect("normalize succeeds");

    assert!(normalized.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, IngestWarningKind::UnknownCountry);
    assert!(warnings[0].message.contains("XKX"));

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_null_value_warns_and_skips() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let parsed: Vec<ParsedWdiStatisticValue> = vec![ParsedWdiStatisticValue {
        iso3: "USA".to_string(),
        year: 2025,
        value: None,
    }];

    let (normalized, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        world_bank_wdi_adapter::normalize(&mut *transaction, parsed)
            .await
            .expect("normalize succeeds");

    assert!(normalized.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, IngestWarningKind::NotApplicableValue);

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn record_inserts_new_publication_and_value() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let data_source_id: Uuid = get_data_source_id(&mut transaction, "wb_wdi").await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
    let region_id: Uuid = get_country_region_id(&mut transaction, "USA").await;

    let report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-record_inserts_new",
        Utc::now(),
        vec![new_normalized_statistic_value(region_id, statistic_id, 2024, 1.66)],
    )
    .await
    .expect("record_statistic_values succeeds");

    assert_eq!(report.values_added, 1);
    assert_eq!(report.values_revised, 0);
    assert_eq!(report.values_skipped, 0);

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn record_re_fetch_same_revision_matches_publication_and_skips() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let data_source_id: Uuid = get_data_source_id(&mut transaction, "wb_wdi").await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
    let region_id: Uuid = get_country_region_id(&mut transaction, "USA").await;

    let usa_2024_166 = || new_normalized_statistic_value(region_id, statistic_id, 2024, 1.66);

    ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-record_refetch",
        Utc::now(),
        vec![usa_2024_166()],
    )
    .await
    .expect("first record");

    let report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-record_refetch",
        Utc::now(),
        vec![usa_2024_166()],
    )
    .await
    .expect("second record");

    assert_eq!(report.values_added, 0);
    assert_eq!(report.values_skipped, 1);

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn record_revised_value_supersedes_old_and_inserts_new() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let data_source_id: Uuid = get_data_source_id(&mut transaction, "wb_wdi").await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
    let region_id: Uuid = get_country_region_id(&mut transaction, "USA").await;

    ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-record_revised-rev1",
        Utc::now(),
        vec![new_normalized_statistic_value(region_id, statistic_id, 2024, 1.66)],
    )
    .await
    .expect("first record");

    let report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "test-record_revised-rev2",
        Utc::now(),
        vec![new_normalized_statistic_value(region_id, statistic_id, 2024, 1.62)],
    )
    .await
    .expect("revised record");

    assert_eq!(report.values_revised, 1);
    assert_eq!(report.values_added, 0);
    assert_eq!(report.values_skipped, 0);

    let current: Option<StatisticValue> = ingest::ingest_db::find_current_value(
        &mut *transaction,
        &new_normalized_statistic_value(region_id, statistic_id, 2024, 0.0),
        data_source_id,
    )
    .await
    .expect("find current succeeds");
    let current_row: StatisticValue = current.expect("current row exists");
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
