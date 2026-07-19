use js_sys::Promise;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    File, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetFileOptions, FileSystemRemoveOptions,
};

use shared::AppError;
use shared::artifact::ArtifactCache;

use crate::client::{js, opfs};

const ARTIFACTS_DIRECTORY: &str = "artifacts";
const VERSIONS_KEPT: usize = 2;
const ERROR_PREFIX_OPFS_UNSUPPORTED: &str = "cache: opfs unsupported";

/// The browser implementation of [`ArtifactCache`], backed by the Origin Private File System. A
/// zero-sized, stateless type: it resolves `navigator.storage.getDirectory()` on every call and caches
/// no directory handle (holding a `FileSystemDirectoryHandle` across calls is the antipattern the
/// stateless design avoids). `!Send`, like every OPFS handle it touches.
pub struct OpfsArtifactCache;

impl OpfsArtifactCache {
    /// Confirms OPFS is available (older Safari lacks it), ensures the `artifacts/` root exists, and
    /// requests persistent storage. Returns a `cache: opfs unsupported`-prefixed error when OPFS is
    /// absent.
    pub async fn create() -> Result<OpfsArtifactCache, AppError> {
        let root: FileSystemDirectoryHandle = opfs::root()
            .await
            .map_err(|error| AppError::from(format!("{ERROR_PREFIX_OPFS_UNSUPPORTED}: {error}")))?;
        opfs::get_or_create_directory(&root, ARTIFACTS_DIRECTORY).await?;

        // Persistent storage is advisory; log the outcome but never fail construction if it's denied.
        match opfs::request_persistence().await {
            Ok(granted) => log::info!("requested persistent storage [granted={granted}]"),
            Err(error) => log::info!("persistent-storage request failed [error={error}]"),
        }

        Ok(OpfsArtifactCache)
    }

    /// Deletes all but the two most recent version subtrees under `artifacts/`. `version_label` is
    /// `YYYY-MM-DD+<surname>`, which sorts chronologically under lexicographic ordering, so the two
    /// highest strings are the two newest bundles. Called once at startup.
    pub async fn evict_old_versions(&self) -> Result<(), AppError> {
        let mut version_labels: Vec<String> = self.list_versions().await?;
        version_labels.sort();

        if version_labels.len() <= VERSIONS_KEPT {
            return Ok(());
        }

        let evict_count: usize = version_labels.len() - VERSIONS_KEPT;
        for version_label in version_labels.into_iter().take(evict_count) {
            self.delete_version(&version_label).await?;
        }

        Ok(())
    }
}

impl ArtifactCache for OpfsArtifactCache {
    async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppError> {
        let root: FileSystemDirectoryHandle = opfs::root().await?;

        opfs::check_quota(bytes.len()).await?;

        let (parent, file_name): (FileSystemDirectoryHandle, &str) =
            create_artifact_directory(&root, version_label, file_relative_path).await?;

        let file_options: FileSystemGetFileOptions = FileSystemGetFileOptions::new();
        file_options.set_create(true);
        let file_handle: FileSystemFileHandle = js::await_and_cast(parent.get_file_handle_with_options(file_name, &file_options)).await?;

        opfs::write_file(&file_handle, bytes).await?;

        Ok(())
    }

    async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError> {
        let root: FileSystemDirectoryHandle = opfs::root().await?;

        let Some((parent, file_name)) =
            find_artifact_directory(&root, version_label, file_relative_path).await?
        else {
            return Ok(None);
        };

        let Some(file_handle) = opfs::get_file(&parent, file_name).await? else {
            return Ok(None);
        };

        let file: File = js::await_and_cast(file_handle.get_file()).await?;
        let bytes: Vec<u8> = opfs::read_file_bytes(&file).await?;

        Ok(Some(bytes))
    }

    async fn list_versions(&self) -> Result<Vec<String>, AppError> {
        let root: FileSystemDirectoryHandle = opfs::root().await?;

        let Some(artifacts) = opfs::get_directory(&root, ARTIFACTS_DIRECTORY).await? else {
            return Ok(Vec::new());
        };

        opfs::list_directory_keys(&artifacts).await
    }

    async fn delete_version(&self, version_label: &str) -> Result<(), AppError> {
        let root: FileSystemDirectoryHandle = opfs::root().await?;

        let Some(artifacts) = opfs::get_directory(&root, ARTIFACTS_DIRECTORY).await? else {
            return Ok(());
        };

        let remove_options: FileSystemRemoveOptions = FileSystemRemoveOptions::new();
        remove_options.set_recursive(true);

        let remove_promise: Promise = artifacts.remove_entry_with_options(version_label, &remove_options);
        let remove_result: Result<JsValue, JsValue> = JsFuture::from(remove_promise).await;
        match remove_result {
            Ok(_) => Ok(()), // removeEntry promise resolves to undefined
            Err(error) if js::is_dom_exception_named(&error, "NotFoundError") => Ok(()),
            Err(error) => Err(js::error(error)),
        }
    }
}

fn split_directory_and_file(file_relative_path: &str) -> (&str, &str) {
    file_relative_path.rsplit_once('/').unwrap_or(("", file_relative_path))
}

fn artifact_directory_segments<'a>(
    version_label: &'a str,
    directory_path: &'a str,
) -> impl Iterator<Item = &'a str> {
    [ARTIFACTS_DIRECTORY, version_label]
        .into_iter()
        .chain(directory_path.split('/').filter(|segment| !segment.is_empty()))
}

async fn create_artifact_directory<'path>(
    root: &FileSystemDirectoryHandle,
    version_label: &str,
    file_relative_path: &'path str,
) -> Result<(FileSystemDirectoryHandle, &'path str), AppError> {
    let (directory_path, file_name): (&str, &str) = split_directory_and_file(file_relative_path);

    let mut directory: FileSystemDirectoryHandle = root.clone();
    for segment in artifact_directory_segments(version_label, directory_path) {
        directory = opfs::get_or_create_directory(&directory, segment).await?;
    }

    Ok((directory, file_name))
}

async fn find_artifact_directory<'path>(
    root: &FileSystemDirectoryHandle,
    version_label: &str,
    file_relative_path: &'path str,
) -> Result<Option<(FileSystemDirectoryHandle, &'path str)>, AppError> {
    let (directory_path, file_name): (&str, &str) = split_directory_and_file(file_relative_path);

    let mut directory: FileSystemDirectoryHandle = root.clone();
    for segment in artifact_directory_segments(version_label, directory_path) {
        let Some(next) = opfs::get_directory(&directory, segment).await? else {
            return Ok(None);
        };
        directory = next;
    }

    Ok(Some((directory, file_name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // These exercise real OPFS and only run in a browser (wasm-pack test --headless --chrome). Each
    // uses a unique version label so the session-persistent OPFS state doesn't couple the tests.

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    async fn put_get_round_trip_returns_byte_equal() {
        let cache: OpfsArtifactCache = OpfsArtifactCache::create().await.unwrap();
        cache.put("2026-06-22+roundtrip", "manifest.json", b"hello").await.unwrap();

        let bytes: Option<Vec<u8>> = cache.get("2026-06-22+roundtrip", "manifest.json").await.unwrap();

        assert_eq!(bytes.as_deref(), Some(b"hello".as_slice()));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    async fn put_get_round_trip_through_nested_path() {
        let cache: OpfsArtifactCache = OpfsArtifactCache::create().await.unwrap();
        cache.put("2026-06-22+nested", "data/tfr-base.sqlite", b"shard-bytes").await.unwrap();

        let bytes: Option<Vec<u8>> = cache.get("2026-06-22+nested", "data/tfr-base.sqlite").await.unwrap();

        assert_eq!(bytes.as_deref(), Some(b"shard-bytes".as_slice()));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    async fn get_missing_returns_none() {
        let cache: OpfsArtifactCache = OpfsArtifactCache::create().await.unwrap();

        let bytes: Option<Vec<u8>> = cache.get("2026-06-22+absent", "manifest.json").await.unwrap();

        assert_eq!(bytes, None);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    async fn evict_old_versions_keeps_the_two_most_recent() {
        let cache: OpfsArtifactCache = OpfsArtifactCache::create().await.unwrap();
        for version_label in ["2026-06-01+evict-a", "2026-06-10+evict-b", "2026-06-22+evict-c"] {
            cache.put(version_label, "manifest.json", b"x").await.unwrap();
        }

        cache.evict_old_versions().await.unwrap();

        assert_eq!(cache.get("2026-06-01+evict-a", "manifest.json").await.unwrap(), None);
        assert_eq!(cache.get("2026-06-10+evict-b", "manifest.json").await.unwrap().as_deref(), Some(b"x".as_slice()));
        assert_eq!(cache.get("2026-06-22+evict-c", "manifest.json").await.unwrap().as_deref(), Some(b"x".as_slice()));
    }

    // The opfs-unsupported branch can't be triggered in headless Chrome (OPFS is always present), so
    // its FR-040 coverage is the exact error-prefix literal the load path matches on, pinned here.

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn opfs_unsupported_prefix_is_the_load_contract() {
        assert_eq!(ERROR_PREFIX_OPFS_UNSUPPORTED, "cache: opfs unsupported");
    }
}
