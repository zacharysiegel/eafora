use std::path::PathBuf;
use std::sync::OnceLock;

use shared::artifact::FilesystemArtifactCache;

use crate::error::FfiError;

static CACHE_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

#[uniffi::export]
pub fn set_cache_directory(cache_directory: String) -> Result<(), FfiError> {
    set_once(&CACHE_DIRECTORY, PathBuf::from(cache_directory))
}

fn set_once(cell: &OnceLock<PathBuf>, cache_directory: PathBuf) -> Result<(), FfiError> {
    let set_result: Result<(), PathBuf> = cell.set(cache_directory);

    match set_result {
        Ok(()) => Ok(()),
        Err(_rejected_cache_directory) => Err(FfiError::Failed {
            message: "the cache directory is already set".to_string(),
        }),
    }
}

pub fn create_cache() -> Result<FilesystemArtifactCache, FfiError> {
    let cache_directory: Option<&PathBuf> = CACHE_DIRECTORY.get();

    let Some(cache_directory) = cache_directory
    else {
        return Err(FfiError::Failed {
            message: "the cache directory has not been set".to_string(),
        });
    };

    Ok(FilesystemArtifactCache::create(cache_directory.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_once_accepts_the_first_cache_directory_and_rejects_a_second() {
        let cell: OnceLock<PathBuf> = OnceLock::new();

        let first: Result<(), FfiError> = set_once(&cell, PathBuf::from("/caches/first"));
        let second: Result<(), FfiError> = set_once(&cell, PathBuf::from("/caches/second"));

        assert!(first.is_ok());
        assert!(second.is_err());
        assert_eq!(cell.get(), Some(&PathBuf::from("/caches/first")));
    }
}
