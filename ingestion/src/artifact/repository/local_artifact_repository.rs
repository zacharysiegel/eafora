use std::path::{Path, PathBuf};

use tokio::fs;

use crate::artifact::repository::artifact_repository::ArtifactRepository;
use crate::error::AppError;

pub const ENV_LOCAL_REPOSITORY_ROOT: &str = "EAFORA_LOCAL_REPOSITORY_ROOT";

pub struct LocalArtifactRepository {
    root: PathBuf,
    public_base_url: String,
}

impl LocalArtifactRepository {
    pub fn new(root: PathBuf, public_base_url: String) -> Self {
        LocalArtifactRepository { root, public_base_url }
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
