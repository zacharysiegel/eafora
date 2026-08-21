//! Integration tests for the WB WDI adapter pipeline against `eafora_test`.
//! Each test opens its own transaction, runs the adapter code through it,
//! and rolls back at teardown — Postgres MVCC provides full isolation so
//! tests run in parallel without table contention.

mod helpers;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use ingestion::adapter::*;
use shared::canonical::canonical_model::{DataSourceKind, SourceRevision};
use ingestion::canonical::canonical_entity::StatisticValue;
use ingestion::canonical::canonical_db;
use ingestion::ingest;
use ingestion::ingest::*;
use ingestion::world_bank_wdi::world_bank_wdi_adapter;
use ingestion::world_bank_wdi::world_bank_wdi_model::ParsedWdiStatisticValue;

use helpers::adapter::new_normalized_statistic_value;
use helpers::canonical::{get_country_region_id, get_data_source_id, get_statistic_id};

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
        iso3: "ZZZ".to_string(),
        year: 2024,
        value: Some(1.5),
    }];

    let (normalized, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        world_bank_wdi_adapter::normalize(&mut *transaction, parsed)
            .await
            .expect("normalize succeeds");

    assert!(normalized.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, IngestWarningKind::UnrecognizedRegionCode);
    assert!(warnings[0].message.contains("ZZZ"));

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

    let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::WorldBankWDI).await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
    let region_id: Uuid = get_country_region_id(&mut transaction, "USA").await;

    let report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "2026-04-09",
        Some(Utc::now()),
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

    let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::WorldBankWDI).await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
    let region_id: Uuid = get_country_region_id(&mut transaction, "USA").await;

    let usa_2024_166 = || new_normalized_statistic_value(region_id, statistic_id, 2024, 1.66);

    ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "2026-04-10",
        Some(Utc::now()),
        Utc::now(),
        vec![usa_2024_166()],
    )
    .await
    .expect("first record");

    let report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "2026-04-10",
        Some(Utc::now()),
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

    let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::WorldBankWDI).await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
    let region_id: Uuid = get_country_region_id(&mut transaction, "USA").await;

    ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "2026-03-15",
        Some(Utc::now()),
        Utc::now(),
        vec![new_normalized_statistic_value(region_id, statistic_id, 2024, 1.66)],
    )
    .await
    .expect("first record");

    let report: IngestReport = ingest::record_statistic_values(
        &mut *transaction,
        data_source_id,
        "2026-04-08",
        Some(Utc::now()),
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

/// `read_latest_publication` orders by `published desc nulls last, fetched desc`.
/// A null-published row must NOT win over a properly-published row even when
/// its `fetched` timestamp is newer.
#[tokio::test]
async fn read_latest_publication_orders_by_published_then_fetched() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::WorldBankWDI).await;

    let older_published: DateTime<Utc> = "2024-12-12T00:00:00Z".parse().unwrap();
    let older_fetched:   DateTime<Utc> = "2025-01-01T00:00:00Z".parse().unwrap();
    let null_published_but_newer_fetched: DateTime<Utc> = "2026-04-08T00:00:00Z".parse().unwrap();

    insert_publication(&mut transaction, data_source_id, "2024-12-12", Some(older_published), older_fetched).await;
    insert_publication(&mut transaction, data_source_id, "unparseable-revision", None, null_published_but_newer_fetched).await;

    let latest: SourceRevision = ingest::ingest_db::read_latest_publication(&mut *transaction, data_source_id)
        .await
        .expect("read_latest_publication")
        .expect("at least one row");

    assert_eq!(latest.revision, "2024-12-12");
    assert_eq!(latest.published, Some(older_published));

    let newer_published: DateTime<Utc> = "2026-04-08T00:00:00Z".parse().unwrap();
    insert_publication(&mut transaction, data_source_id, "2026-04-08", Some(newer_published), older_fetched).await;

    let latest_after: SourceRevision = ingest::ingest_db::read_latest_publication(&mut *transaction, data_source_id)
        .await
        .expect("read_latest_publication")
        .expect("at least one row");

    assert_eq!(latest_after.revision, "2026-04-08");
    assert_eq!(latest_after.published, Some(newer_published));

    transaction.rollback().await.unwrap();
}

/// When every publication for a source has `published is null`, the
/// fallback ordering by `fetched desc` picks the most recently fetched row.
#[tokio::test]
async fn read_latest_publication_falls_back_to_fetched_when_all_null() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::WorldBankWDI).await;

    let older_fetched: DateTime<Utc> = "2025-01-01T00:00:00Z".parse().unwrap();
    let newer_fetched: DateTime<Utc> = "2026-04-08T00:00:00Z".parse().unwrap();

    insert_publication(&mut transaction, data_source_id, "rev-older", None, older_fetched).await;
    insert_publication(&mut transaction, data_source_id, "rev-newer", None, newer_fetched).await;

    let latest: SourceRevision = ingest::ingest_db::read_latest_publication(&mut *transaction, data_source_id)
        .await
        .expect("read_latest_publication")
        .expect("at least one row");

    assert_eq!(latest.revision, "rev-newer");
    assert_eq!(latest.published, None);

    transaction.rollback().await.unwrap();
}

async fn insert_publication(
    transaction: &mut Transaction<'static, Postgres>,
    data_source_id: Uuid,
    revision_label: &str,
    published: Option<DateTime<Utc>>,
    fetched: DateTime<Utc>,
) {
    sqlx::query!(
        r#"
        insert into data_source_publication (data_source_id, revision_label, published, fetched)
        values ($1, $2, $3, $4)
        "#,
        data_source_id,
        revision_label,
        published,
        fetched,
    )
    .execute(&mut **transaction)
    .await
    .expect("insert publication");
}

#[tokio::test]
async fn find_region_by_code_resolves_seeded_world_region() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let region: shared::canonical::canonical_model::Region =
        canonical_db::find_region_by_code(&mut *transaction, "world")
            .await
            .expect("find_region_by_code succeeds")
            .expect("world region is seeded");

    assert_eq!(region.code, "world");
    assert_eq!(region.level, "world");
    assert_eq!(region.m49_code.as_deref(), Some("001"));

    transaction.rollback().await.unwrap();
}

#[tokio::test]
async fn normalize_maps_wld_to_world_region() {
    let pool: PgPool = helpers::test_db::test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let world_region_id: Uuid = canonical_db::find_region_by_code(&mut *transaction, "world")
        .await
        .expect("find_region_by_code succeeds")
        .expect("world region is seeded")
        .id;

    let parsed: Vec<ParsedWdiStatisticValue> = vec![ParsedWdiStatisticValue {
        iso3: "WLD".to_string(),
        year: 2024,
        value: Some(2.24),
    }];

    let (normalized, warnings): (Vec<NormalizedStatisticValue>, Vec<IngestWarning>) =
        world_bank_wdi_adapter::normalize(&mut *transaction, parsed)
            .await
            .expect("normalize succeeds");

    assert_eq!(normalized.len(), 1);
    assert!(warnings.is_empty());

    let normalized_statistic_value: &NormalizedStatisticValue = &normalized[0];
    assert_eq!(normalized_statistic_value.region_id, world_region_id);
    assert_eq!(normalized_statistic_value.value, 2.24);
    assert_eq!(normalized_statistic_value.period.start, NaiveDate::from_ymd_opt(2024, 1, 1).unwrap());

    transaction.rollback().await.unwrap();
}
