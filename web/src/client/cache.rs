use js_sys::{ArrayBuffer, AsyncIterator, IteratorNext, Promise, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DomException, File, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemGetFileOptions, FileSystemRemoveOptions, FileSystemWritableFileStream, StorageEstimate,
};

use shared::AppError;
use shared::artifact::ArtifactCache;

const ARTIFACTS_DIRECTORY: &str = "artifacts";
const VERSIONS_KEPT: usize = 2;
const QUOTA_SAFETY_MARGIN_BYTES: f64 = 1_048_576.0; // 1 MB headroom left free on every write
const ERROR_PREFIX_OPFS_UNSUPPORTED: &str = "cache: opfs unsupported";
const ERROR_PREFIX_QUOTA_EXCEEDED: &str = "cache: quota exceeded";

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
        let root: FileSystemDirectoryHandle = opfs_root().await?;
        get_or_create_directory(&root, ARTIFACTS_DIRECTORY).await?;

        request_persistence().await;

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
        let root: FileSystemDirectoryHandle = opfs_root().await?;

        check_quota(bytes.len()).await?;

        let (parent, file_name): (FileSystemDirectoryHandle, &str) =
            walk_to_file_parent(&root, version_label, file_relative_path, WalkMode::Create)
                .await?
                .expect("WalkMode::Create always resolves a parent");

        let file_options: FileSystemGetFileOptions = FileSystemGetFileOptions::new();
        file_options.set_create(true);
        let file_handle: FileSystemFileHandle =
            await_and_cast(parent.get_file_handle_with_options(file_name, &file_options)).await?;

        let writable_value: JsValue = JsFuture::from(file_handle.create_writable())
            .await
            .map_err(map_quota_or_generic)?;
        let writable: FileSystemWritableFileStream = writable_value.dyn_into().map_err(dyn_into_error)?;

        let write_promise: Promise = writable.write_with_u8_array(bytes).map_err(map_quota_or_generic)?;
        JsFuture::from(write_promise)
            .await
            .map_err(map_quota_or_generic)?;

        JsFuture::from(writable.close())
            .await
            .map_err(map_quota_or_generic)?;

        Ok(())
    }

    async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError> {
        let root: FileSystemDirectoryHandle = opfs_root().await?;

        let Some((parent, file_name)) =
            walk_to_file_parent(&root, version_label, file_relative_path, WalkMode::ReadOnly).await?
        else {
            return Ok(None);
        };

        let Some(file_handle) = get_file_handle_if_present(&parent, file_name).await? else {
            return Ok(None);
        };

        let file: File = await_and_cast(file_handle.get_file()).await?;
        let bytes: Vec<u8> = read_file_bytes(&file).await?;

        Ok(Some(bytes))
    }

    async fn list_versions(&self) -> Result<Vec<String>, AppError> {
        let root: FileSystemDirectoryHandle = opfs_root().await?;

        let Some(artifacts) = get_directory_if_present(&root, ARTIFACTS_DIRECTORY).await? else {
            return Ok(Vec::new());
        };

        list_directory_keys(&artifacts).await
    }

    async fn delete_version(&self, version_label: &str) -> Result<(), AppError> {
        let root: FileSystemDirectoryHandle = opfs_root().await?;

        let Some(artifacts) = get_directory_if_present(&root, ARTIFACTS_DIRECTORY).await? else {
            return Ok(());
        };

        let remove_options: FileSystemRemoveOptions = FileSystemRemoveOptions::new();
        remove_options.set_recursive(true);

        let remove_result: Result<JsValue, JsValue> =
            JsFuture::from(artifacts.remove_entry_with_options(version_label, &remove_options)).await;
        match remove_result {
            Ok(_) => Ok(()),
            Err(error) if is_dom_exception_not_found(&error) => Ok(()),
            Err(error) => Err(map_generic(error)),
        }
    }
}

/// The OPFS root handle, or a `cache: opfs unsupported`-prefixed error when the browser lacks OPFS.
async fn opfs_root() -> Result<FileSystemDirectoryHandle, AppError> {
    let window: web_sys::Window = web_sys::window()
        .ok_or_else(|| AppError::from(format!("{ERROR_PREFIX_OPFS_UNSUPPORTED}: no window")))?;

    let root_value: JsValue = JsFuture::from(window.navigator().storage().get_directory())
        .await
        .map_err(|error| AppError::from(format!("{ERROR_PREFIX_OPFS_UNSUPPORTED}: {}", describe_js_error(&error))))?;

    root_value
        .dyn_into::<FileSystemDirectoryHandle>()
        .map_err(|_| AppError::from(format!("{ERROR_PREFIX_OPFS_UNSUPPORTED}: getDirectory returned a non-handle")))
}

/// Requests persistent (non-evictable) storage. The result is advisory, so this only logs it and never
/// blocks cache construction.
async fn request_persistence() {
    let Some(window) = web_sys::window() else {
        return;
    };

    match window.navigator().storage().persist() {
        Ok(promise) => match JsFuture::from(promise).await {
            Ok(granted) => log::info!("requested persistent storage [granted={:?}]", granted.as_bool()),
            Err(error) => log::info!("persistent-storage request rejected [error={}]", describe_js_error(&error)),
        },
        Err(error) => log::info!("persistent-storage request unavailable [error={}]", describe_js_error(&error)),
    }
}

async fn await_and_cast<T: JsCast>(promise: Promise) -> Result<T, AppError> {
    let value: JsValue = JsFuture::from(promise).await.map_err(map_generic)?;

    value.dyn_into::<T>().map_err(dyn_into_error)
}

async fn get_or_create_directory(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<FileSystemDirectoryHandle, AppError> {
    let options: FileSystemGetDirectoryOptions = FileSystemGetDirectoryOptions::new();
    options.set_create(true);

    await_and_cast(parent.get_directory_handle_with_options(name, &options)).await
}

/// The child directory, or `None` when it is absent (a `NotFoundError` rejection = cache miss). Any
/// other rejection is a real failure and propagates.
async fn get_directory_if_present(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<Option<FileSystemDirectoryHandle>, AppError> {
    match JsFuture::from(parent.get_directory_handle(name)).await {
        Ok(value) => Ok(Some(value.dyn_into::<FileSystemDirectoryHandle>().map_err(dyn_into_error)?)),
        Err(error) if is_dom_exception_not_found(&error) => Ok(None),
        Err(error) => Err(map_generic(error)),
    }
}

/// The file handle, or `None` on a `NotFoundError` (cache miss); other rejections propagate.
async fn get_file_handle_if_present(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<Option<FileSystemFileHandle>, AppError> {
    match JsFuture::from(parent.get_file_handle(name)).await {
        Ok(value) => Ok(Some(value.dyn_into::<FileSystemFileHandle>().map_err(dyn_into_error)?)),
        Err(error) if is_dom_exception_not_found(&error) => Ok(None),
        Err(error) => Err(map_generic(error)),
    }
}

enum WalkMode {
    Create,
    ReadOnly,
}

/// Resolves the directory holding the target file, splitting `file_relative_path` on `/` so nested
/// paths (`data/tfr-base.sqlite`) map to nested OPFS directories under `artifacts/<version_label>/`.
/// In `ReadOnly` mode a missing directory short-circuits to `Ok(None)` (cache miss); in `Create` mode
/// every directory is created. Returns the parent handle and the final path component (the file name).
async fn walk_to_file_parent<'path>(
    root: &FileSystemDirectoryHandle,
    version_label: &str,
    file_relative_path: &'path str,
    mode: WalkMode,
) -> Result<Option<(FileSystemDirectoryHandle, &'path str)>, AppError> {
    let mut components = file_relative_path.split('/');
    let file_name: &str = components
        .next_back()
        .ok_or_else(|| AppError::from("cache: empty file_relative_path".to_string()))?;

    let mut current: FileSystemDirectoryHandle = match mode {
        WalkMode::Create => {
            let artifacts: FileSystemDirectoryHandle = get_or_create_directory(root, ARTIFACTS_DIRECTORY).await?;
            get_or_create_directory(&artifacts, version_label).await?
        }
        WalkMode::ReadOnly => {
            let Some(artifacts) = get_directory_if_present(root, ARTIFACTS_DIRECTORY).await? else {
                return Ok(None);
            };
            let Some(version) = get_directory_if_present(&artifacts, version_label).await? else {
                return Ok(None);
            };
            version
        }
    };

    for component in components {
        match mode {
            WalkMode::Create => current = get_or_create_directory(&current, component).await?,
            WalkMode::ReadOnly => {
                let Some(next) = get_directory_if_present(&current, component).await? else {
                    return Ok(None);
                };
                current = next;
            }
        }
    }

    Ok(Some((current, file_name)))
}

/// Fails with a `cache: quota exceeded`-prefixed error when writing `incoming_len` bytes would leave
/// less than the safety margin free, so a partial write never lands.
async fn check_quota(incoming_len: usize) -> Result<(), AppError> {
    let Some(window) = web_sys::window() else {
        return Ok(());
    };

    let estimate_promise: Promise = window
        .navigator()
        .storage()
        .estimate()
        .map_err(|error| AppError::from(format!("cache: storage estimate failed: {}", describe_js_error(&error))))?;
    let estimate_value: JsValue = JsFuture::from(estimate_promise)
        .await
        .map_err(|error| AppError::from(format!("cache: storage estimate rejected: {}", describe_js_error(&error))))?;
    let estimate: StorageEstimate = estimate_value.dyn_into().map_err(dyn_into_error)?;

    let usage: f64 = estimate.get_usage().unwrap_or(0.0);
    let quota: f64 = estimate.get_quota().unwrap_or(f64::INFINITY);

    if !quota_allows(usage, quota, incoming_len) {
        return Err(AppError::from(format!(
            "{ERROR_PREFIX_QUOTA_EXCEEDED}: writing {incoming_len} bytes leaves under the {QUOTA_SAFETY_MARGIN_BYTES} byte margin (usage {usage}, quota {quota})"
        )));
    }

    Ok(())
}

fn quota_allows(usage: f64, quota: f64, incoming_len: usize) -> bool {
    quota - usage >= incoming_len as f64 + QUOTA_SAFETY_MARGIN_BYTES
}

async fn read_file_bytes(file: &File) -> Result<Vec<u8>, AppError> {
    let buffer_value: JsValue = JsFuture::from(file.array_buffer()).await.map_err(map_generic)?;
    let array_buffer: ArrayBuffer = buffer_value.dyn_into().map_err(dyn_into_error)?;

    Ok(Uint8Array::new(&array_buffer).to_vec())
}

/// The immediate entry names of a directory. `FileSystemDirectoryHandle::keys()` returns a JS async
/// iterator; each `next()` yields a promise resolving to an `{ value, done }` object, so the drive loop
/// awaits every step.
async fn list_directory_keys(handle: &FileSystemDirectoryHandle) -> Result<Vec<String>, AppError> {
    let iterator: AsyncIterator = handle.keys();

    let mut key_strings: Vec<String> = Vec::new();
    loop {
        let next_promise: Promise = iterator.next().map_err(map_generic)?;
        let next_value: JsValue = JsFuture::from(next_promise).await.map_err(map_generic)?;
        let iterator_next: IteratorNext = next_value.dyn_into().map_err(dyn_into_error)?;

        if iterator_next.done() {
            break;
        }

        let key_string: String = iterator_next
            .value()
            .as_string()
            .ok_or_else(|| AppError::from("cache: directory key is not a string".to_string()))?;
        key_strings.push(key_string);
    }

    Ok(key_strings)
}

fn describe_js_error(error: &JsValue) -> String {
    if let Some(dom_exception) = error.dyn_ref::<DomException>() {
        return format!("{}: {}", dom_exception.name(), dom_exception.message());
    }

    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

fn map_generic(error: JsValue) -> AppError {
    AppError::from(format!("cache: {}", describe_js_error(&error)))
}

fn dyn_into_error<T>(_original: T) -> AppError {
    AppError::from("cache: unexpected JS value type".to_string())
}

fn is_dom_exception_named(error: &JsValue, name: &str) -> bool {
    error
        .dyn_ref::<DomException>()
        .is_some_and(|dom_exception| dom_exception.name() == name)
}

fn is_dom_exception_not_found(error: &JsValue) -> bool {
    is_dom_exception_named(error, "NotFoundError")
}

fn is_dom_exception_quota_exceeded(error: &JsValue) -> bool {
    is_dom_exception_named(error, "QuotaExceededError")
}

fn map_quota_or_generic(error: JsValue) -> AppError {
    if is_dom_exception_quota_exceeded(&error) {
        return AppError::from(format!("{ERROR_PREFIX_QUOTA_EXCEEDED}: {}", describe_js_error(&error)));
    }

    map_generic(error)
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

    // The quota-exceeded and opfs-unsupported runtime branches can't be triggered in headless Chrome
    // (quota can't be exhausted deterministically; OPFS is always present), so their FR-040 coverage is
    // the exact error-prefix literals the loader matches on, pinned here, plus the quota arithmetic.

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn error_prefixes_are_the_loader_contract() {
        assert_eq!(ERROR_PREFIX_OPFS_UNSUPPORTED, "cache: opfs unsupported");
        assert_eq!(ERROR_PREFIX_QUOTA_EXCEEDED, "cache: quota exceeded");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn quota_allows_leaves_the_safety_margin_free() {
        // Exactly margin + payload free -> allowed; one byte less -> refused.
        assert!(quota_allows(0.0, QUOTA_SAFETY_MARGIN_BYTES + 100.0, 100));
        assert!(!quota_allows(0.0, QUOTA_SAFETY_MARGIN_BYTES + 100.0, 101));
        assert!(!quota_allows(QUOTA_SAFETY_MARGIN_BYTES, QUOTA_SAFETY_MARGIN_BYTES + 50.0, 100));
    }
}
