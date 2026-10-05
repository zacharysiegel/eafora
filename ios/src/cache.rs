use std::path::PathBuf;
use std::sync::OnceLock;

use shared::artifact::FilesystemArtifactCache;
use shared::error::AppError;

use crate::error::FfiError;

static CACHE_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

#[uniffi::export]
pub fn set_cache_directory(cache_directory: String) -> Result<(), FfiError> {
    let set_result: Result<(), PathBuf> = CACHE_DIRECTORY.set(PathBuf::from(cache_directory));

    match set_result {
        Ok(()) => Ok(()),
        Err(_rejected_cache_directory) => Err(FfiError::Failed {
            message: "the cache directory is already set".to_string(),
        }),
    }
}

pub fn create_cache() -> Result<FilesystemArtifactCache, AppError> {
    let cache_directory: Option<&PathBuf> = CACHE_DIRECTORY.get();

    let Some(cache_directory) = cache_directory
    else {
        return Err(AppError::from("the cache directory has not been set".to_string()));
    };

    Ok(FilesystemArtifactCache::create(cache_directory.clone()))
}
