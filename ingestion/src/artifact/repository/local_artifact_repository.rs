use std::ffi::OsString;
use std::fs::FileType;
use std::path::{Path, PathBuf};

use tokio::fs;

use chrono::{DateTime, Utc};
use shared::artifact::{manifest, Manifest};

use crate::artifact;
use crate::artifact::repository::artifact_repository::ArtifactRepository;
use crate::error::AppError;

pub const ENV_LOCAL_REPOSITORY_ROOT: &str = "EAFORA_LOCAL_REPOSITORY_ROOT";

const LOCAL_VERSIONS_KEPT: usize = 2;

pub struct LocalArtifactRepository {
    root: PathBuf,
    public_base_url: String,
}

impl LocalArtifactRepository {
    pub fn new(root: PathBuf, public_base_url: String) -> Self {
        LocalArtifactRepository { root, public_base_url }
    }

    /// Deletes version directories until `LOCAL_VERSIONS_KEPT` remain, oldest first. The order comes from
    /// each version's `artifact_created`, not from the directory name: `YYYY-MM-DD+<surname>` orders
    /// chronologically only across differing dates, and two builds sharing a date fall back to comparing
    /// arbitrary surnames. A version whose manifest cannot be read is unorderable and so pruned first.
    pub async fn retain_newest_versions(&self) -> Result<(), AppError> {
        let version_directory_names: Vec<String> = self.read_version_directory_names().await?;

        if version_directory_names.len() <= LOCAL_VERSIONS_KEPT {
            return Ok(());
        }

        let mut names_by_creation: Vec<(Option<DateTime<Utc>>, String)> =
            Vec::with_capacity(version_directory_names.len());

        for version_directory_name in version_directory_names {
            let artifact_created: Option<DateTime<Utc>> = self.read_artifact_created(&version_directory_name).await;

            names_by_creation.push((artifact_created, version_directory_name));
        }

        names_by_creation.sort_by(|(left_created, _), (right_created, _)| right_created.cmp(left_created));

        for (_, version_directory_name) in names_by_creation.into_iter().skip(LOCAL_VERSIONS_KEPT) {
            let version_directory: PathBuf = self.root.join(&version_directory_name);

            fs::remove_dir_all(&version_directory).await?;
        }

        Ok(())
    }

    /// Every immediate subdirectory of `root` naming a published version. The `latest/` pointer is not a
    /// version, and a name that is not valid Unicode cannot be a version label.
    async fn read_version_directory_names(&self) -> Result<Vec<String>, AppError> {
        let mut version_directory_names: Vec<String> = Vec::new();
        let mut read_dir: fs::ReadDir = fs::read_dir(&self.root).await?;

        while let Some(directory_entry) = read_dir.next_entry().await? {
            let file_type: FileType = directory_entry.file_type().await?;

            if !file_type.is_dir() {
                continue;
            }

            let file_name: OsString = directory_entry.file_name();
            let directory_name: String = match file_name.into_string() {
                Ok(directory_name) => directory_name,
                Err(_) => {
                    continue;
                }
            };

            if directory_name == artifact::LATEST_POINTER {
                continue;
            }

            version_directory_names.push(directory_name);
        }

        Ok(version_directory_names)
    }

    /// `None` when the version's manifest is missing or unparseable, so one damaged version cannot stop the
    /// others from being pruned.
    async fn read_artifact_created(&self, version_directory_name: &str) -> Option<DateTime<Utc>> {
        let manifest_path: PathBuf = self.root.join(version_directory_name).join(manifest::MANIFEST_FILENAME);
        let manifest_bytes: Vec<u8> = fs::read(&manifest_path)
            .await
            .map_err(|error| {
                log::warn!("reading a published manifest failed; [path={} error={error}]", manifest_path.display())
            })
            .ok()?;
        let manifest: Manifest = manifest::parse_manifest(&manifest_bytes)
            .map_err(|error| {
                log::warn!("parsing a published manifest failed; [path={} error={error}]", manifest_path.display())
            })
            .ok()?;

        Some(manifest.artifact_created)
    }
}

impl ArtifactRepository for LocalArtifactRepository {
    async fn put_file(&self, key: &str, source_path: &Path, _content_type: &str) -> Result<(), AppError> {
        let destination: PathBuf = self.root.join(key);

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(source_path, &destination).await?;

        Ok(())
    }

    fn url(&self, key: &str) -> String {
        format!("{}/{}", self.public_base_url.trim_end_matches('/'), key)
    }
}
