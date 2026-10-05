use std::path::PathBuf;
use std::sync::OnceLock;

use shared::artifact::FilesystemArtifactCache;
use shared::error::AppError;

use crate::error::FfiError;

static CACHE: OnceLock<FilesystemArtifactCache> = OnceLock::new();

#[uniffi::export]
pub fn set_cache_directory(cache_directory: String) -> Result<(), FfiError> {
    let cache: FilesystemArtifactCache = FilesystemArtifactCache::create(PathBuf::from(cache_directory));
    let set_result: Result<(), FilesystemArtifactCache> = CACHE.set(cache);

    match set_result {
        Ok(()) => Ok(()),
        Err(_rejected_cache) => Err(FfiError::Failed {
            message: "the cache directory is already set".to_string(),
        }),
    }
}

pub fn get_cache() -> Result<&'static FilesystemArtifactCache, AppError> {
    let cache: Option<&'static FilesystemArtifactCache> = CACHE.get();

    let Some(cache) = cache
    else {
        return Err(AppError::from("the cache directory has not been set".to_string()));
    };

    Ok(cache)
}
