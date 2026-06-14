use std::path::Path;

use crate::artifact::repository::artifact_repository::ArtifactRepository;
use crate::error::AppError;

pub struct DryrunArtifactRepository {}

impl DryrunArtifactRepository {
    pub fn new() -> Self {
        DryrunArtifactRepository {}
    }
}

impl ArtifactRepository for DryrunArtifactRepository {
    async fn put_file(&self, key: &str, source_path: &Path, content_type: &str) -> Result<(), AppError> {
        log::info!(
            "[dryrun] would PUT key={key} content_type={content_type} source={source_path:?}",
        );
        Ok(())
    }

    fn url_for(&self, key: &str) -> String {
        format!("dryrun:///{}", key)
    }
}
