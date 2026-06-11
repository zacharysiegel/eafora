//! Walk an `ArtifactBuildReport` and ship every output file through an
//! `ArtifactRepository`. Manifest is uploaded last so a client that
//! discovers a new manifest URL is guaranteed to find its referenced
//! shards already present at the same repository.

use std::collections::BTreeMap;
use std::path::Path;

use sqlx::PgPool;

use crate::artifact::artifact_db;
use crate::artifact::artifact_model::{ArtifactBuildReport, ArtifactVersion};
use crate::artifact::repository::ArtifactRepository;
use crate::artifact::writer::manifest::{MANIFEST_FILENAME, SUBDIR_DATA, SUBDIR_GEOMETRY};
use crate::canonical::canonical_model::{DataSourceKind, SourceRevision};
use crate::error::AppError;

const CONTENT_TYPE_SQLITE: &str = "application/vnd.sqlite3";
const CONTENT_TYPE_FLATGEOBUF: &str = "application/octet-stream";
const CONTENT_TYPE_MANIFEST_JSON: &str = "application/json";

#[derive(Debug, Clone)]
pub struct PublishReport {
    pub version_label: String,
    pub manifest_url: String,
    pub artifact_version: ArtifactVersion,
    pub shards_uploaded: usize,
    pub geometry_uploaded: bool,
    pub manifest_uploaded: bool,
}

pub async fn publish_artifacts(
    pool: &PgPool,
    build_report: &ArtifactBuildReport,
    repository: &dyn ArtifactRepository,
    data_source_revisions: &BTreeMap<DataSourceKind, SourceRevision>,
) -> Result<PublishReport, AppError> {
    let version_label: &str = &build_report.version_label;

    let already_exists: bool = artifact_db::read_artifact_version_exists(pool, version_label).await?;
    if already_exists {
        return Err(AppError::from(format!(
            "artifact_version with version_label {:?} already exists; publish is non-clobbering",
            version_label,
        )));
    }

    let mut shards_uploaded: usize = 0;
    for shard in &build_report.artifacts.shards {
        let filename: &str = filename_of(&shard.file.path)?;
        let key: String = format!("{}/{}/{}", version_label, SUBDIR_DATA, filename);
        repository.put_file(&key, &shard.file.path, CONTENT_TYPE_SQLITE).await?;
        log::info!("uploaded shard key={} sha256={}", key, shard.file.sha256_hex());
        shards_uploaded += 1;
    }

    let geometry_filename: &str = filename_of(&build_report.artifacts.geometry.path)?;
    let geometry_key: String = format!("{}/{}/{}", version_label, SUBDIR_GEOMETRY, geometry_filename);
    repository.put_file(&geometry_key, &build_report.artifacts.geometry.path, CONTENT_TYPE_FLATGEOBUF).await?;
    log::info!("uploaded geometry key={} sha256={}", geometry_key, build_report.artifacts.geometry.sha256_hex());

    let manifest_key: String = format!("{}/{}", version_label, MANIFEST_FILENAME);
    repository.put_file(&manifest_key, &build_report.artifacts.manifest.path, CONTENT_TYPE_MANIFEST_JSON).await?;
    let manifest_url: String = repository.url_for(&manifest_key);
    log::info!("uploaded manifest key={} url={} sha256={}", manifest_key, manifest_url, build_report.artifacts.manifest.sha256_hex());

    let artifact_version: ArtifactVersion = artifact_db::insert_artifact_version(
        pool,
        version_label,
        build_report.artifacts.manifest.sha256_hex(),
        &manifest_url,
        data_source_revisions,
    )
    .await?;
    log::info!("inserted artifact_version id={} version_label={}", artifact_version.id, artifact_version.version_label);

    Ok(PublishReport {
        version_label: version_label.to_string(),
        manifest_url,
        artifact_version,
        shards_uploaded,
        geometry_uploaded: true,
        manifest_uploaded: true,
    })
}

fn filename_of(path: &Path) -> Result<&str, AppError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::from(format!("path missing filename component: {:?}", path)))
}
