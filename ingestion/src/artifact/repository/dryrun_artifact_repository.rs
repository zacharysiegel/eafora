use std::path::Path;

use async_trait::async_trait;

use crate::artifact::repository::artifact_repository::ArtifactRepository;
use crate::error::AppError;

/// Logs every PUT and returns Ok without performing any I/O. Use via
/// `--repository dryrun` to preview a publish.
pub struct DryrunArtifactRepository {
    public_base_url: String,
}

impl DryrunArtifactRepository {
    pub fn new(public_base_url: String) -> Self {
        DryrunArtifactRepository { public_base_url }
    }
}

#[async_trait]
impl ArtifactRepository for DryrunArtifactRepository {
    async fn put_file(&self, key: &str, source_path: &Path, content_type: &str) -> Result<(), AppError> {
        log::info!(
            "[dryrun] would PUT key={key} content_type={content_type} source={source_path:?}",
        );
        Ok(())
    }

    fn url_for(&self, key: &str) -> String {
        format!("{}/{}", self.public_base_url.trim_end_matches('/'), key)
    }
}
