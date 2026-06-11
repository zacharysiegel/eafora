use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;

use crate::artifact::repository::artifact_repository::ArtifactRepository;
use crate::error::AppError;

pub struct LocalArtifactRepository {
    root: PathBuf,
    public_url_base: String,
}

impl LocalArtifactRepository {
    pub fn new(root: PathBuf, public_url_base: String) -> Self {
        LocalArtifactRepository { root, public_url_base }
    }
}

#[async_trait]
impl ArtifactRepository for LocalArtifactRepository {
    async fn put_file(&self, key: &str, source_path: &Path, _content_type: &str) -> Result<(), AppError> {
        let destination: PathBuf = self.root.join(key);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(source_path, &destination).await?;
        Ok(())
    }

    fn url_for(&self, key: &str) -> String {
        format!("{}/{}", self.public_url_base.trim_end_matches('/'), key)
    }
}
