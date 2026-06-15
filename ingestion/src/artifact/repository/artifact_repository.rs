use std::future::Future;
use std::path::Path;

use crate::artifact::repository::cloudflare_r2_artifact_repository::CloudflareR2ArtifactRepository;
use crate::artifact::repository::dry_artifact_repository::DryArtifactRepository;
use crate::artifact::repository::local_artifact_repository::LocalArtifactRepository;
use crate::error::AppError;

pub trait ArtifactRepository {
    fn put_file(&self, key: &str, source_path: &Path, content_type: &str) -> impl Future<Output = Result<(), AppError>> + Send;

    fn url_for(&self, key: &str) -> String;
}

pub enum ArtifactRepositoryKind {
    Local(LocalArtifactRepository),
    CloudflareR2(CloudflareR2ArtifactRepository),
    Dry(DryArtifactRepository),
}

impl ArtifactRepositoryKind {
    pub async fn put_file(&self, key: &str, source_path: &Path, content_type: &str) -> Result<(), AppError> {
        match self {
            ArtifactRepositoryKind::Local(repository) => repository.put_file(key, source_path, content_type).await,
            ArtifactRepositoryKind::CloudflareR2(repository) => repository.put_file(key, source_path, content_type).await,
            ArtifactRepositoryKind::Dry(repository) => repository.put_file(key, source_path, content_type).await,
        }
    }

    pub fn url_for(&self, key: &str) -> String {
        match self {
            ArtifactRepositoryKind::Local(repository) => repository.url_for(key),
            ArtifactRepositoryKind::CloudflareR2(repository) => repository.url_for(key),
            ArtifactRepositoryKind::Dry(repository) => repository.url_for(key),
        }
    }
}
