//! Manifest is uploaded last so a client that discovers a new manifest URL
//! is guaranteed to find its referenced shards already present at the same
//! repository.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sqlx::PgPool;

use crate::artifact::artifact_db;
use crate::artifact::artifact_model::{
    Artifacts, ArtifactVersion, BuildReport, FileReference, StatisticShard, StatisticShardKey,
};
use crate::artifact::hashing::Hashed;
use crate::artifact::repository::ArtifactRepositoryKind;
use crate::artifact::writer::manifest::{MANIFEST_FILENAME, SUBDIR_DATA, SUBDIR_GEOMETRY};
use crate::canonical::canonical_model::{DataSourceKind, LicenseShardClass, SourceRevision, StatisticKind};
use crate::error::AppError;
use crate::filesystem;

const CONTENT_TYPE_FLATGEOBUF: &str = "application/octet-stream";
const CONTENT_TYPE_MANIFEST: &str = "application/json";
const CONTENT_TYPE_SQLITE: &str = "application/vnd.sqlite3";

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
        let key: String = format!("{}/{}/{}", version_label, SUBDIR_DATA, filename);
        repository.put_file(&key, &shard.file.path, CONTENT_TYPE_SQLITE).await?;
        log::debug!("uploaded shard; [key={}]", key);
    }
    let shards_published: usize = build_report.artifacts.shards.len();

    let geometry_filename: &str = filesystem::filename_of(&build_report.artifacts.geometry.path)?;
    let geometry_key: String = format!("{}/{}/{}", version_label, SUBDIR_GEOMETRY, geometry_filename);
    repository.put_file(&geometry_key, &build_report.artifacts.geometry.path, CONTENT_TYPE_FLATGEOBUF).await?;
    log::debug!("uploaded geometry; [key={}]", geometry_key);

    let manifest_key: String = format!("{}/{}", version_label, MANIFEST_FILENAME);
    repository.put_file(&manifest_key, &build_report.artifacts.manifest.path, CONTENT_TYPE_MANIFEST).await?;
    let manifest_url: String = repository.url_for(&manifest_key);
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

    Ok(PublishReport {
        artifact_version,
        shards_published,
    })
}

#[derive(Debug, Deserialize)]
struct ManifestOnDisk {
    version: String,
    geometry: ManifestEntryOnDisk,
    statistics: BTreeMap<String, BTreeMap<String, ManifestEntryOnDisk>>,
    source_revisions: BTreeMap<String, SourceRevision>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntryOnDisk {
    relative_path: String,
    sha256: String,
}

pub fn load_build_report_from_disk(artifact_dir: &Path) -> Result<BuildReport, AppError> {
    let manifest_path: PathBuf = artifact_dir.join(MANIFEST_FILENAME);
    let manifest_bytes: Vec<u8> = fs::read(&manifest_path)
        .map_err(|err| AppError::from(format!("read {:?}: {}", manifest_path, err)))?;
    let manifest_on_disk: ManifestOnDisk = serde_json::from_slice(&manifest_bytes)
        .map_err(|err| AppError::from(format!("parse {:?}: {}", manifest_path, err)))?;

    let geometry: Hashed<FileReference> = load_hashed_file(artifact_dir, &manifest_on_disk.geometry)?;

    let mut shards: Vec<StatisticShard<Hashed<FileReference>>> = Vec::new();
    for (statistic_code, license_shard_classes) in &manifest_on_disk.statistics {
        let statistic_kind: StatisticKind = StatisticKind::try_from(statistic_code.as_str())?;
        for (license_shard_code, entry) in license_shard_classes {
            let license_shard_class: LicenseShardClass = LicenseShardClass::try_from(license_shard_code.as_str())?;
            let hashed_file: Hashed<FileReference> = load_hashed_file(artifact_dir, entry)?;

            shards.push(StatisticShard {
                key: StatisticShardKey { statistic_kind, license_shard_class },
                file: hashed_file,
            });
        }
    }

    let manifest_hashed: Hashed<FileReference> = Hashed::new(
        FileReference { path: manifest_path, byte_count: manifest_bytes.len() as u64 },
        &manifest_bytes,
    );

    let mut data_source_revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
    for (data_source_code, revision) in manifest_on_disk.source_revisions {
        let kind: DataSourceKind = DataSourceKind::try_from(data_source_code.as_str())?;
        data_source_revisions.insert(kind, revision);
    }

    Ok(BuildReport {
        artifact_dir: artifact_dir.to_path_buf(),
        version_label: manifest_on_disk.version,
        artifacts: Artifacts {
            shards,
            geometry,
            manifest: manifest_hashed,
        },
        data_source_revisions,
    })
}

fn load_hashed_file(artifact_dir: &Path, entry: &ManifestEntryOnDisk) -> Result<Hashed<FileReference>, AppError> {
    let path: PathBuf = artifact_dir.join(&entry.relative_path);
    let bytes: Vec<u8> = fs::read(&path)
        .map_err(|err| AppError::from(format!("read {:?}: {}", path, err)))?;
    let hashed: Hashed<FileReference> = Hashed::new(
        FileReference { path: path.clone(), byte_count: bytes.len() as u64 },
        &bytes,
    );

    if hashed.sha256_hex() != entry.sha256 {
        return Err(AppError::from(format!(
            "sha256 mismatch for {:?}: manifest says {}, file hashes to {}",
            path, entry.sha256, hashed.sha256_hex(),
        )));
    }

    Ok(hashed)
}
