use std::path::Path;

use async_trait::async_trait;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactRepositoryKind {
    Local,
    CloudflareR2,
    Dryrun,
}

impl ArtifactRepositoryKind {
    pub fn code(self) -> &'static str {
        match self {
            ArtifactRepositoryKind::Local => "local",
            ArtifactRepositoryKind::CloudflareR2 => "r2",
            ArtifactRepositoryKind::Dryrun => "dryrun",
        }
    }
}

impl TryFrom<&str> for ArtifactRepositoryKind {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "local" => Ok(ArtifactRepositoryKind::Local),
            "r2" => Ok(ArtifactRepositoryKind::CloudflareR2),
            "dryrun" => Ok(ArtifactRepositoryKind::Dryrun),
            other => Err(AppError::from(format!("unknown repository kind {:?}", other))),
        }
    }
}

#[async_trait]
pub trait ArtifactRepository: Send + Sync {
    /// Stream `source_path`'s bytes to the repository under `key`. Idempotent
    /// at the implementation's discretion (R2 same-key PUT overwrites; Local
    /// overwrites; Dryrun logs and returns).
    async fn put_file(&self, key: &str, source_path: &Path, content_type: &str) -> Result<(), AppError>;

    /// Public URL where a client would fetch the object at `key`. Used by
    /// the orchestrator to write `artifact_version.manifest_url`.
    fn url_for(&self, key: &str) -> String;
}
