use std::path::{Path, PathBuf};

use tokio::fs;

use crate::error::AppError;

pub struct LocalArtifactRepository {
    root: PathBuf,
    public_base_url: String,
}

impl LocalArtifactRepository {
    pub fn new(root: PathBuf, public_base_url: String) -> Self {
        LocalArtifactRepository { root, public_base_url }
    }

    pub async fn put_file(&self, key: &str, source_path: &Path, _content_type: &str) -> Result<(), AppError> {
        let destination: PathBuf = self.root.join(key);

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::copy(source_path, &destination).await?;

        Ok(())
    }

    pub fn url_for(&self, key: &str) -> String {
        format!("{}/{}", self.public_base_url.trim_end_matches('/'), key)
    }
}
