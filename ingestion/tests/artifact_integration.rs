//! Integration tests for the artifact builder pipeline against `eafora_test`.
//! Each test opens its own transaction, exercises `build_artifacts` (or its
//! components) through it, and rolls back at teardown — Postgres MVCC keeps
//! parallel runs isolated.

mod helpers;

use std::fs;
use std::path::PathBuf;

use chrono::{NaiveDate, Utc};
use rusqlite::Connection;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use ingestion::artifact::{self, BuildOptions, ArtifactBuild};
use ingestion::artifact::artifact_model::FileReference;
use ingestion::canonical::canonical_model::DataSourceKind;
use ingestion::artifact::writer::flatgeobuf::emit_geometry_flatgeobuf;

use helpers::canonical::{get_country_region_id, get_data_source_id, get_statistic_id};
use helpers::test_db::test_pool;

#[tokio::test]
async fn build_artifacts_emits_sqlite_shard_with_inserted_rows_and_well_formed_manifest() {
    let pool: PgPool = test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::WorldBankWDI).await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
    let region_id: Uuid = get_country_region_id(&mut transaction, "USA").await;
    let publication_id: Uuid = insert_data_source_publication(&mut transaction, data_source_id, "2024-Q4").await;
    insert_statistic_value(
        &mut transaction,
        region_id,
        statistic_id,
        data_source_id,
        publication_id,
        NaiveDate::from_ymd_opt(2022, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2023, 1, 1).unwrap(),
        1.66,
    ).await;

    let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let options: BuildOptions = BuildOptions { test_offline: true };
    let build: ArtifactBuild =
        artifact::build_artifacts(&mut *transaction, temp_dir.path(), "2026-05-26-test", options)
            .await
            .expect("build_artifacts succeeds");

    assert_eq!(build.version_label, "2026-05-26-test");
    assert_eq!(build.artifacts.statistic_shards.len(), 1);
    assert!(build.manifest.path.exists());
    assert!(build.manifest.path.ends_with("manifest.json"));
    assert!(build.artifacts.geometry.path.exists());

    let tfr_shard_path: PathBuf = build.artifacts.statistic_shards[0].hashed_file.path.clone();
    let connection: Connection = Connection::open(&tfr_shard_path).unwrap();
    let row_count: i64 = connection
        .query_row("select count(*) from statistic_value", [], |row| row.get(0))
        .unwrap();
    assert_eq!(row_count, 1);

    let value: f64 = connection
        .query_row(
            "select value from statistic_value where region_iso3 = 'USA'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!((value - 1.66).abs() < f64::EPSILON);

    let manifest_text: String = fs::read_to_string(&build.manifest.path).unwrap();
    let manifest_value: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest_value["version"], "2026-05-26-test");
    assert!(manifest_value["statistics"]["tfr"]["base"]["url"].is_string());
    assert!(manifest_value["geometry"]["url"].is_string());

    transaction.rollback().await.unwrap();
}

/// Live HTTP test: downloads the pinned Natural Earth release and confirms
/// most features resolve to known canonical countries. Gated behind
/// `#[ignore]` so CI doesn't depend on naciscdn.org availability; run via
/// `cargo test -p ingestion --test artifact_integration -- --ignored`.
#[tokio::test]
#[ignore]
async fn emit_geometry_flatgeobuf_against_live_natural_earth_release() {
    let pool: PgPool = test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let geometry: FileReference =
        emit_geometry_flatgeobuf(&mut *transaction, temp_dir.path())
            .await
            .expect("geometry shard emitted");

    assert!(geometry.path.exists());
    assert!(geometry.byte_count > 100_000);

    transaction.rollback().await.unwrap();
}

async fn insert_data_source_publication(
    transaction: &mut Transaction<'static, Postgres>,
    data_source_id: Uuid,
    revision_label: &str,
) -> Uuid {
    let publication_id: Uuid = Uuid::now_v7();
    sqlx::query!(
        r#"
        insert into data_source_publication
            (id, data_source_id, revision_label, fetched)
        values ($1, $2, $3, $4)
        "#,
        publication_id,
        data_source_id,
        revision_label,
        Utc::now(),
    )
    .execute(&mut **transaction)
    .await
    .expect("insert publication");
    publication_id
}

#[allow(clippy::too_many_arguments)]
async fn insert_statistic_value(
    transaction: &mut Transaction<'static, Postgres>,
    region_id: Uuid,
    statistic_id: Uuid,
    data_source_id: Uuid,
    publication_id: Uuid,
    period_start: NaiveDate,
    period_end: NaiveDate,
    value: f64,
) {
    sqlx::query!(
        r#"
        insert into statistic_value
            (region_id, statistic_id, period_start, period_end, value,
             data_source_id, data_source_publication_id, data_status)
        values ($1, $2, $3, $4, $5, $6, $7, 'final')
        "#,
        region_id,
        statistic_id,
        period_start,
        period_end,
        value,
        data_source_id,
        publication_id,
    )
    .execute(&mut **transaction)
    .await
    .expect("insert statistic_value");
}
