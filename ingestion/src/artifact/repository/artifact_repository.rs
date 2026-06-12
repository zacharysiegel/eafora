use std::path::Path;

use crate::artifact::repository::cloudflare_r2_artifact_repository::CloudflareR2ArtifactRepository;
use crate::artifact::repository::dryrun_artifact_repository::DryrunArtifactRepository;
use crate::artifact::repository::local_artifact_repository::LocalArtifactRepository;
use crate::error::AppError;

pub enum ArtifactRepository {
    Local(LocalArtifactRepository),
    CloudflareR2(CloudflareR2ArtifactRepository),
    Dryrun(DryrunArtifactRepository),
}

impl ArtifactRepository {
    pub async fn put_file(&self, key: &str, source_path: &Path, content_type: &str) -> Result<(), AppError> {
        match self {
            ArtifactRepository::Local(repository) => repository.put_file(key, source_path, content_type).await,
            ArtifactRepository::CloudflareR2(repository) => repository.put_file(key, source_path, content_type).await,
            ArtifactRepository::Dryrun(repository) => repository.put_file(key, source_path, content_type).await,
        }
    }

    pub fn url_for(&self, key: &str) -> String {
        match self {
            ArtifactRepository::Local(repository) => repository.url_for(key),
            ArtifactRepository::CloudflareR2(repository) => repository.url_for(key),
            ArtifactRepository::Dryrun(repository) => repository.url_for(key),
        }
    }
}

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
            ArtifactRepositoryKind::CloudflareR2 => "cloudflare-r2",
            ArtifactRepositoryKind::Dryrun => "dryrun",
        }
    }
}

impl TryFrom<&str> for ArtifactRepositoryKind {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "local" => Ok(ArtifactRepositoryKind::Local),
            "cloudflare-r2" => Ok(ArtifactRepositoryKind::CloudflareR2),
            "dryrun" => Ok(ArtifactRepositoryKind::Dryrun),
            other => Err(AppError::from(format!("unknown repository kind {:?}", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_accepts_each_known_code() {
        assert_eq!(ArtifactRepositoryKind::try_from("local").unwrap(), ArtifactRepositoryKind::Local);
        assert_eq!(ArtifactRepositoryKind::try_from("cloudflare-r2").unwrap(), ArtifactRepositoryKind::CloudflareR2);
        assert_eq!(ArtifactRepositoryKind::try_from("dryrun").unwrap(), ArtifactRepositoryKind::Dryrun);
    }

    #[test]
    fn try_from_round_trips_each_variants_code() {
        for kind in [ArtifactRepositoryKind::Local, ArtifactRepositoryKind::CloudflareR2, ArtifactRepositoryKind::Dryrun] {
            assert_eq!(ArtifactRepositoryKind::try_from(kind.code()).unwrap(), kind);
        }
    }

    #[test]
    fn try_from_errors_on_unknown_code() {
        let error: AppError = ArtifactRepositoryKind::try_from("s3").expect_err("unknown code errors");
        assert!(error.to_string().contains("unknown repository kind"));
    }
}
