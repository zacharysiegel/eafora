use std::path::Path;

use crate::artifact::repository::artifact_repository::ArtifactRepository;
use crate::error::AppError;

pub struct DryArtifactRepository {}

impl DryArtifactRepository {
    pub fn new() -> Self {
        DryArtifactRepository {}
    }
}

impl ArtifactRepository for DryArtifactRepository {
    async fn put_file(&self, key: &str, source_path: &Path, content_type: &str) -> Result<(), AppError> {
        log::info!(
            "[dry] would PUT; [key={key} content_type={content_type} source={source_path:?}]",
        );
        Ok(())
    }

    fn url_for(&self, key: &str) -> String {
        format!("dry:///{}", key)
    }
}
