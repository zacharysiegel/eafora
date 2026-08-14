use std::path::{Path, PathBuf};

use tokio::fs;

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

    pub async fn retain_newest_versions(&self) -> Result<(), AppError> {
        let mut version_directory_names: Vec<String> = Vec::new();
        let mut read_dir: fs::ReadDir = fs::read_dir(&self.root).await?;

        while let Some(directory_entry) = read_dir.next_entry().await? {
            let file_type: std::fs::FileType = directory_entry.file_type().await?;

            if !file_type.is_dir() {
                continue;
            }

            let file_name: std::ffi::OsString = directory_entry.file_name();
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

        version_directory_names.sort();

        if version_directory_names.len() <= LOCAL_VERSIONS_KEPT {
            return Ok(());
        }

        let prune_count: usize = version_directory_names.len() - LOCAL_VERSIONS_KEPT;

        for version_directory_name in &version_directory_names[..prune_count] {
            let version_directory: PathBuf = self.root.join(version_directory_name);
            fs::remove_dir_all(&version_directory).await?;
        }

        Ok(())
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
