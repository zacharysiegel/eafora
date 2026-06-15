//! Integration tests for the publish pipeline. Each test writes a tiny
//! synthetic bundle into a tempdir, runs `publish_artifacts` through a
//! `LocalArtifactRepository` (or `DryArtifactRepository`), and cleans
//! up the resulting `artifact_version` record. `artifact_version` inserts
//! commit through the pool, so MVCC rollback isn't available — uuid-suffixed
//! version labels keep parallel tests from colliding.

mod helpers;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use ingestion::artifact;
use ingestion::artifact::artifact_model::{
    ArtifactBuildReport, Artifacts, FileReference, StatisticShard, StatisticShardKey,
};
use ingestion::artifact::hashing::Hashed;
use ingestion::artifact::publish::PublishReport;
use ingestion::artifact::repository::{ArtifactRepositoryKind, DryArtifactRepository, LocalArtifactRepository};
use ingestion::artifact::writer::manifest::{MANIFEST_FILENAME, SUBDIR_DATA, SUBDIR_GEOMETRY};
use ingestion::canonical::canonical_model::{
    DataSourceKind, LicenseShardClass, SourceRevision, StatisticKind,
};

use helpers::test_db::test_pool;

#[tokio::test]
async fn publish_artifacts_uploads_every_file_to_local_repository_and_inserts_artifact_version() {
    let pool: PgPool = test_pool().await;
    let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let version_label: String = unique_version_label();
    let build_report: ArtifactBuildReport = write_synthetic_bundle(temp_dir.path(), &version_label);

    let destination_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let public_base_url: String = "https://example.invalid/artifacts".to_string();
    let repository: ArtifactRepositoryKind = ArtifactRepositoryKind::Local(
        LocalArtifactRepository::new(destination_dir.path().to_path_buf(), public_base_url.clone()),
    );

    let publish_report: PublishReport = artifact::publish_artifacts(&pool, &build_report, &repository)
        .await
        .expect("publish succeeds");

    let manifest_key: String = format!("{}/{}", version_label, MANIFEST_FILENAME);
    assert_eq!(publish_report.version_label, version_label);
    assert_eq!(publish_report.manifest_url, format!("{}/{}", public_base_url, manifest_key));
    assert_eq!(publish_report.shards_published, 1);

    let shard_destination: PathBuf = destination_dir.path().join(format!("{}/{}/shard.sqlite", version_label, SUBDIR_DATA));
    let geometry_destination: PathBuf = destination_dir.path().join(format!("{}/{}/world.fgb", version_label, SUBDIR_GEOMETRY));
    let manifest_destination: PathBuf = destination_dir.path().join(&manifest_key);
    assert!(shard_destination.exists(), "shard at {:?} missing", shard_destination);
    assert!(geometry_destination.exists(), "geometry at {:?} missing", geometry_destination);
    assert!(manifest_destination.exists(), "manifest at {:?} missing", manifest_destination);

    assert_eq!(publish_report.artifact_version.version_label, version_label);

    delete_artifact_version(&pool, &version_label).await;
}

#[tokio::test]
async fn publish_artifacts_errors_when_version_label_already_published() {
    let pool: PgPool = test_pool().await;
    let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let version_label: String = unique_version_label();
    let build_report: ArtifactBuildReport = write_synthetic_bundle(temp_dir.path(), &version_label);

    let destination_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let repository: ArtifactRepositoryKind = ArtifactRepositoryKind::Local(LocalArtifactRepository::new(
        destination_dir.path().to_path_buf(),
        "https://example.invalid/artifacts".to_string(),
    ));

    artifact::publish_artifacts(&pool, &build_report, &repository)
        .await
        .expect("first publish succeeds");

    let second_attempt: Result<PublishReport, _> = artifact::publish_artifacts(&pool, &build_report, &repository).await;
    let error: ingestion::error::AppError = second_attempt.expect_err("second publish errors on duplicate label");
    assert!(error.to_string().contains("already exists"), "expected duplicate-label error, got {}", error);

    delete_artifact_version(&pool, &version_label).await;
}

#[tokio::test]
async fn publish_artifacts_against_dry_repository_does_not_write_files_but_inserts_artifact_version() {
    let pool: PgPool = test_pool().await;
    let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let version_label: String = unique_version_label();
    let build_report: ArtifactBuildReport = write_synthetic_bundle(temp_dir.path(), &version_label);

    let repository: ArtifactRepositoryKind = ArtifactRepositoryKind::Dry(
        DryArtifactRepository::new(),
    );

    let publish_report: PublishReport = artifact::publish_artifacts(&pool, &build_report, &repository)
        .await
        .expect("dry publish succeeds");

    assert_eq!(publish_report.shards_published, 1);
    assert!(publish_report.manifest_url.starts_with("dry:///"));

    delete_artifact_version(&pool, &version_label).await;
}

#[tokio::test]
async fn load_build_report_from_disk_round_trips_a_freshly_written_bundle() {
    let temp_dir: tempfile::TempDir = tempfile::tempdir().unwrap();
    let version_label: String = unique_version_label();
    let original: ArtifactBuildReport = write_synthetic_bundle(temp_dir.path(), &version_label);

    let loaded: ArtifactBuildReport = artifact::load_build_report_from_disk(temp_dir.path())
        .expect("loader succeeds on freshly-written bundle");

    assert_eq!(loaded.version_label, original.version_label);
    assert_eq!(loaded.artifacts.shards.len(), original.artifacts.shards.len());
    assert_eq!(loaded.artifacts.shards[0].file.sha256_hex(), original.artifacts.shards[0].file.sha256_hex());
    assert_eq!(loaded.artifacts.geometry.sha256_hex(), original.artifacts.geometry.sha256_hex());
    assert_eq!(loaded.artifacts.manifest.sha256_hex(), original.artifacts.manifest.sha256_hex());
    assert_eq!(loaded.data_source_revisions.len(), 1);
    assert!(loaded.data_source_revisions.contains_key(&DataSourceKind::WorldBankWDI));
}

fn unique_version_label() -> String {
    format!("test-{}", Uuid::now_v7())
}

fn write_synthetic_bundle(artifact_dir: &Path, version_label: &str) -> ArtifactBuildReport {
    let data_dir: PathBuf = artifact_dir.join(SUBDIR_DATA);
    let geometry_dir: PathBuf = artifact_dir.join(SUBDIR_GEOMETRY);
    fs::create_dir_all(&data_dir).unwrap();
    fs::create_dir_all(&geometry_dir).unwrap();

    let shard_bytes: &[u8] = b"synthetic-shard-bytes";
    let shard_path: PathBuf = data_dir.join("shard.sqlite");
    fs::write(&shard_path, shard_bytes).unwrap();

    let geometry_bytes: &[u8] = b"synthetic-geometry-bytes";
    let geometry_path: PathBuf = geometry_dir.join("world.fgb");
    fs::write(&geometry_path, geometry_bytes).unwrap();

    let shard_hashed: Hashed<FileReference> = Hashed::new(
        FileReference { path: shard_path, byte_count: shard_bytes.len() as u64 },
        shard_bytes,
    );
    let geometry_hashed: Hashed<FileReference> = Hashed::new(
        FileReference { path: geometry_path, byte_count: geometry_bytes.len() as u64 },
        geometry_bytes,
    );

    let shards: Vec<StatisticShard<Hashed<FileReference>>> = vec![StatisticShard {
        key: StatisticShardKey {
            statistic_kind: StatisticKind::Tfr,
            license_shard_class: LicenseShardClass::Base,
        },
        file: shard_hashed,
    }];

    let mut data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
    let published: DateTime<Utc> = "2024-12-12T00:00:00Z".parse().unwrap();
    let fetched: DateTime<Utc> = "2025-01-15T03:00:00Z".parse().unwrap();
    data_source_revisions.insert(
        DataSourceKind::WorldBankWDI,
        SourceRevision { revision: "2024-12-12".to_string(), published: Some(published), fetched },
    );

    let manifest_hashed: Hashed<FileReference> = artifact::writer::manifest::write_manifest(
        &shards,
        &geometry_hashed,
        version_label,
        &data_source_revisions,
        artifact_dir,
    )
    .expect("manifest writes");

    ArtifactBuildReport {
        artifact_dir: artifact_dir.to_path_buf(),
        version_label: version_label.to_string(),
        artifacts: Artifacts {
            shards,
            geometry: geometry_hashed,
            manifest: manifest_hashed,
        },
        data_source_revisions,
    }
}

async fn delete_artifact_version(pool: &PgPool, version_label: &str) {
    sqlx::query!("delete from artifact_version where version_label = $1", version_label)
        .execute(pool)
        .await
        .expect("delete artifact_version");
}
