//! Manifest is uploaded last so a client that discovers a new manifest URL
//! is guaranteed to find its referenced shards already present at the same
//! repository.

use std::fs;
use std::path::{Path, PathBuf};

use sqlx::PgPool;

use shared::artifact::bundle::{self, StatisticShardKey};
use shared::artifact::manifest::{self, Manifest};
use shared::filesystem::{self, FileReference, Hashed};

use crate::artifact::artifact_db;
use crate::artifact::artifact_model::{Artifacts, ArtifactVersion, BuildReport, StatisticShard};
use crate::artifact::repository::{ArtifactRepositoryKind, LocalArtifactRepository};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct PublishReport {
    pub artifact_version: ArtifactVersion,
    pub shards_published: usize,
}

pub async fn publish_artifacts(
    pool: &PgPool,
    build_report: &BuildReport,
    repository: &ArtifactRepositoryKind,
) -> Result<PublishReport, AppError> {
    let version_label: &str = &build_report.version_label;

    let already_exists: bool = artifact_db::read_artifact_version_exists(pool, version_label).await?;
    if already_exists {
        return Err(AppError::from(format!(
            "artifact_version with version_label {:?} already exists; aborting publish",
            version_label,
        )));
    }

    for shard in &build_report.artifacts.shards {
        let filename: &str = filesystem::filename_of(&shard.file.path)?;
        let key: String = format!("{}/{}/{}", version_label, manifest::SUBDIR_DATA, filename);
        repository.put_file(&key, &shard.file.path, bundle::CONTENT_TYPE_SQLITE).await?;
        log::debug!("uploaded shard; [key={}]", key);
    }
    let shards_published: usize = build_report.artifacts.shards.len();

    let geometry_filename: &str = filesystem::filename_of(&build_report.artifacts.geometry.path)?;
    let geometry_key: String = format!("{}/{}/{}", version_label, manifest::SUBDIR_GEOMETRY, geometry_filename);
    repository.put_file(&geometry_key, &build_report.artifacts.geometry.path, bundle::CONTENT_TYPE_FLATGEOBUF).await?;
    log::debug!("uploaded geometry; [key={}]", geometry_key);

    let manifest_key: String = format!("{}/{}", version_label, manifest::MANIFEST_FILENAME);
    repository.put_file(&manifest_key, &build_report.artifacts.manifest.path, bundle::CONTENT_TYPE_MANIFEST).await?;
    let manifest_url: String = repository.url(&manifest_key);
    log::debug!("uploaded manifest; [key={} url={}]", manifest_key, manifest_url);

    let artifact_version: ArtifactVersion = artifact_db::insert_artifact_version(
        pool,
        version_label,
        build_report.artifacts.manifest.sha256_hex(),
        &manifest_url,
        &build_report.data_source_revisions,
    )
    .await?;
    log::info!("inserted artifact_version; [id={} version_label={}]", artifact_version.id, artifact_version.version_label);

    repository.put_file(manifest::MANIFEST_LATEST_KEY, &build_report.artifacts.manifest.path, bundle::CONTENT_TYPE_MANIFEST).await?;
    log::debug!("uploaded latest manifest; [key={}]", manifest::MANIFEST_LATEST_KEY);

    if let ArtifactRepositoryKind::Local(local_repository) = repository {
        let local_repository: &LocalArtifactRepository = local_repository;
        local_repository.retain_newest_versions().await?;
    }

    Ok(PublishReport {
        artifact_version,
        shards_published,
    })
}

pub fn load_build_report_from_disk(artifact_dir: &Path) -> Result<BuildReport, AppError> {
    let manifest_path: PathBuf = artifact_dir.join(manifest::MANIFEST_FILENAME);
    let manifest_bytes: Vec<u8> = fs::read(&manifest_path)
        .map_err(|err| AppError::from(format!("read {:?}: {}", manifest_path, err)))?;
    let parsed_manifest: Manifest = manifest::parse_manifest(&manifest_bytes)?;

    let geometry: Hashed<FileReference> = filesystem::load_hashed_file(
        artifact_dir,
        &parsed_manifest.geometry.relative_path,
        &parsed_manifest.geometry.sha256,
    )?;

    let mut shards: Vec<StatisticShard<Hashed<FileReference>>> = Vec::new();
    for (statistic_kind, license_shard_classes) in &parsed_manifest.statistics {
        for (license_shard_class, entry) in license_shard_classes {
            let hashed_file: Hashed<FileReference> =
                filesystem::load_hashed_file(artifact_dir, &entry.relative_path, &entry.sha256)?;

            shards.push(StatisticShard {
                key: StatisticShardKey {
                    statistic_kind: *statistic_kind,
                    license_shard_class: *license_shard_class,
                },
                file: hashed_file,
            });
        }
    }

    let manifest: Hashed<FileReference> = Hashed::new(
        FileReference { path: manifest_path, byte_count: manifest_bytes.len() as u64 },
        &manifest_bytes,
    );

    Ok(BuildReport {
        artifact_dir: artifact_dir.to_path_buf(),
        version_label: parsed_manifest.version,
        artifacts: Artifacts {
            shards,
            geometry,
            manifest,
        },
        data_source_revisions: parsed_manifest.source_revisions,
    })
}
