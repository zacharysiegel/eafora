//! Integration tests for the artifact builder pipeline against `eafora_test`.
//! Each test opens its own transaction, exercises `build_artifacts` (or its
//! components) through it, and rolls back at teardown — Postgres MVCC keeps
//! parallel runs isolated.

mod helpers;

use std::fs::{self, File};
use std::io::BufReader;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use flatgeobuf::{FgbReader, GeometryType};
use rusqlite::Connection;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use ingestion::artifact::{self, BuildOptions, BuildReport};
use shared::canonical::canonical_model::DataSourceKind;
use ingestion::artifact::writer::flatgeobuf::{write_flatgeobuf_from_shapefile, write_geometry, PLACEHOLDER_GEOMETRY_BYTES};
use shared::artifact::geometry;
use shared::artifact::manifest;
use shared::filesystem::FileReference;
use ingestion::geometry::natural_earth::{self, ShapefileBytes};

use helpers::canonical::{get_country_region_id, get_data_source_id, get_statistic_id};
use helpers::test_db::test_pool;

const BUNDLED_NATURAL_EARTH_ZIP: &str = "samples/natural_earth/ne_50m_admin_0_countries.zip";

#[tokio::test]
async fn build_artifacts_emits_sqlite_shard_with_inserted_rows_and_well_formed_manifest() {
    let pool: PgPool = test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let data_source_id: Uuid = get_data_source_id(&mut transaction, DataSourceKind::WorldBankWDI).await;
    let statistic_id: Uuid = get_statistic_id(&mut transaction, "tfr").await;
    let region_id: Uuid = get_country_region_id(&mut transaction, "USA").await;
    let wb_published: DateTime<Utc> = "2024-12-31T00:00:00Z".parse().unwrap();
    let publication_id: Uuid = insert_data_source_publication(&mut transaction, data_source_id, "2024-12-12", wb_published).await;
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
    let build: BuildReport =
        artifact::build_artifacts(&mut *transaction, temp_dir.path(), "2026-05-26-test", options)
            .await
            .expect("build_artifacts succeeds");

    assert_eq!(build.version_label, "2026-05-26-test");

    assert_eq!(build.artifacts.shards.len(), 1);
    let tfr_base_shard = &build.artifacts.shards[0];
    assert_eq!(tfr_base_shard.key.statistic_kind.code(), "tfr");
    assert_eq!(tfr_base_shard.key.license_shard_class.as_str(), "base");
    let tfr_base_filename: &str = tfr_base_shard.file.path.file_name().unwrap().to_str().unwrap();
    assert!(tfr_base_filename.starts_with("tfr-base-"));
    assert!(tfr_base_filename.ends_with(".sqlite"));
    assert_eq!(tfr_base_shard.file.sha256_hex().len(), 64);
    assert!(tfr_base_filename.contains(tfr_base_shard.file.sha256_hex()));
    assert!(tfr_base_shard.file.byte_count > 0);

    assert!(build.artifacts.manifest.path.exists());
    assert!(build.artifacts.manifest.path.ends_with(manifest::MANIFEST_FILENAME));
    assert_eq!(build.artifacts.manifest.sha256_hex().len(), 64);
    assert!(build.artifacts.manifest.byte_count > 0);

    assert!(build.artifacts.geometry.path.exists());
    assert_eq!(build.artifacts.geometry.byte_count, PLACEHOLDER_GEOMETRY_BYTES.len() as u64);
    let geometry_filename: &str = build.artifacts.geometry.path.file_name().unwrap().to_str().unwrap();
    assert!(geometry_filename.starts_with(&format!("{}-", geometry::GEOMETRY_FILENAME_STEM)));
    assert!(geometry_filename.ends_with(".fgb"));
    assert_eq!(build.artifacts.geometry.sha256_hex().len(), 64);
    assert!(geometry_filename.contains(build.artifacts.geometry.sha256_hex()));

    let tfr_shard_path: PathBuf = tfr_base_shard.file.path.clone();
    let connection: Connection = Connection::open(&tfr_shard_path).unwrap();

    let (shard_kind, shard_class): (String, String) = connection
        .query_row(
            "select statistic_kind, license_shard_class from shard_key",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(shard_kind, "tfr");
    assert_eq!(shard_class, "base");

    let row_count: i64 = connection
        .query_row("select count(*) from statistic_value", [], |row| row.get(0))
        .unwrap();
    assert_eq!(row_count, 1);

    let (value, period_start, period_end, data_status, data_source_code, data_source_revision): (
        f64,
        String,
        String,
        String,
        String,
        String,
    ) = connection
        .query_row(
            "select value, period_start, period_end, data_status, data_source_code, data_source_revision \
             from statistic_value where region_iso3 = 'USA'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .unwrap();
    assert!((value - 1.66).abs() < f64::EPSILON);
    assert_eq!(period_start, "2022-01-01");
    assert_eq!(period_end, "2023-01-01");
    assert_eq!(data_status, "final");
    assert_eq!(data_source_code, "wb_wdi");
    assert_eq!(data_source_revision, "2024-12-12");

    let manifest_text: String = fs::read_to_string(&build.artifacts.manifest.path).unwrap();
    let manifest_value: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest_value["version"], "2026-05-26-test");

    let artifact_created: &str = manifest_value["artifact_created"].as_str().expect("artifact_created");
    DateTime::parse_from_rfc3339(artifact_created).expect("artifact_created RFC3339");

    let manifest_tfr_base = &manifest_value["statistics"]["tfr"]["base"];
    assert_eq!(
        manifest_tfr_base["relative_path"].as_str().unwrap(),
        format!("{}/{}", manifest::SUBDIR_DATA, tfr_base_filename),
    );
    assert_eq!(manifest_tfr_base["size_bytes"].as_u64().unwrap(), tfr_base_shard.file.byte_count);
    assert_eq!(manifest_tfr_base["sha256"].as_str().unwrap(), tfr_base_shard.file.sha256_hex());

    let manifest_geometry = &manifest_value["geometry"];
    assert_eq!(
        manifest_geometry["relative_path"].as_str().unwrap(),
        format!("{}/{}", manifest::SUBDIR_GEOMETRY, geometry_filename),
    );
    assert_eq!(manifest_geometry["size_bytes"].as_u64().unwrap(), build.artifacts.geometry.byte_count);
    assert_eq!(manifest_geometry["sha256"].as_str().unwrap(), build.artifacts.geometry.sha256_hex());

    let wb_revision = &manifest_value["source_revisions"]["wb_wdi"];
    assert_eq!(wb_revision["revision"].as_str().unwrap(), "2024-12-12");
    let wb_published_in_manifest: DateTime<Utc> = DateTime::parse_from_rfc3339(
        wb_revision["published"].as_str().expect("published"),
    ).expect("published RFC3339").with_timezone(&Utc);
    assert_eq!(wb_published_in_manifest, wb_published);
    let wb_fetched: &str = wb_revision["fetched"].as_str().expect("fetched");
    DateTime::parse_from_rfc3339(wb_fetched).expect("fetched RFC3339");

    transaction.rollback().await.unwrap();
}

/// Live HTTP test: downloads the pinned Natural Earth release and confirms
/// the FGB has the expected layer + features. Gated behind `#[ignore]` so
/// CI doesn't depend on naciscdn.org availability; run via
/// `cargo test -p ingestion --test artifact_integration -- --ignored`.
#[tokio::test]
#[ignore]
async fn write_geometry_flatgeobuf_against_live_natural_earth_release() {
    let pool: PgPool = test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let geometry: FileReference =
        write_geometry(&mut *transaction, temp_dir.path())
            .await
            .expect("geometry shard emitted");

    assert_geometry_fgb_well_formed(&geometry.path);

    transaction.rollback().await.unwrap();
}

/// Offline counterpart to the live test: uses the bundled Natural Earth zip
/// in `samples/natural_earth/` so CI exercises the FGB pipeline without
/// hitting the network. The pinned shapefile is byte-stable, so the same
/// feature count and layer assertions apply.
#[tokio::test]
async fn write_flatgeobuf_from_bundled_natural_earth_sample() {
    let pool: PgPool = test_pool().await;
    let mut transaction: Transaction<'static, Postgres> = pool.begin().await.unwrap();

    let zip_bytes: Vec<u8> = fs::read(BUNDLED_NATURAL_EARTH_ZIP).unwrap();
    let shapefile_bytes: ShapefileBytes = natural_earth::extract_shapefile_from_zip(&zip_bytes).unwrap();

    let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let geometry: FileReference =
        write_flatgeobuf_from_shapefile(&mut *transaction, &shapefile_bytes, temp_dir.path())
            .await
            .expect("offline geometry shard emitted");

    assert_geometry_fgb_well_formed(&geometry.path);

    transaction.rollback().await.unwrap();
}

fn assert_geometry_fgb_well_formed(path: &std::path::Path) {
    assert!(path.exists());
    let bytes_on_disk: Vec<u8> = fs::read(path).unwrap();
    assert!(bytes_on_disk.starts_with(b"fgb\x03fgb\x00"));

    let mut reader: BufReader<File> = BufReader::new(File::open(path).unwrap());
    let header_reader = FgbReader::open(&mut reader).expect("FGB header");
    let header = header_reader.header();
    assert_eq!(header.name(), Some(geometry::GEOMETRY_LAYER_NAME));
    assert_eq!(header.geometry_type(), GeometryType::MultiPolygon);
    let features_count: u64 = header.features_count();
    assert!(
        (200..=300).contains(&features_count),
        "feature count {} outside expected admin-0 range [200, 300]",
        features_count,
    );
}

async fn insert_data_source_publication(
    transaction: &mut Transaction<'static, Postgres>,
    data_source_id: Uuid,
    revision_label: &str,
    published: DateTime<Utc>,
) -> Uuid {
    let publication_id: Uuid = Uuid::now_v7();
    sqlx::query!(
        r#"
        insert into data_source_publication
            (id, data_source_id, revision_label, published, fetched)
        values ($1, $2, $3, $4, $5)
        "#,
        publication_id,
        data_source_id,
        revision_label,
        published,
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
