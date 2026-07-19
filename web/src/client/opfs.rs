use js_sys::{ArrayBuffer, AsyncIterator, IteratorNext, Promise, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    File, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions,
    FileSystemWritableFileStream, StorageEstimate,
};

use shared::AppError;

use crate::client::js;

const ERROR_PREFIX_QUOTA_EXCEEDED: &str = "opfs: quota exceeded";
const QUOTA_SAFETY_MARGIN_BYTES: f64 = 1_048_576.0; // 1 MB headroom left free on every write

pub async fn root() -> Result<FileSystemDirectoryHandle, AppError> {
    let window: web_sys::Window = js::get_window()?;

    let directory_promise: Promise = window.navigator().storage().get_directory();
    let root_value: JsValue = JsFuture::from(directory_promise).await.map_err(js::error)?;

    js::dyn_into::<FileSystemDirectoryHandle>(root_value)
}

/// Requests persistent (non-evictable) storage; `Ok(true)` if granted, `Ok(false)` if denied.
pub async fn request_persistence() -> Result<bool, AppError> {
    let window: web_sys::Window = js::get_window()?;

    let persist_promise: Promise = window.navigator().storage().persist().map_err(js::error)?;
    let granted: JsValue = JsFuture::from(persist_promise).await.map_err(js::error)?;
    let granted: bool = granted.as_bool().unwrap_or(false);
    Ok(granted)
}

pub async fn get_or_create_directory(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<FileSystemDirectoryHandle, AppError> {
    let options: FileSystemGetDirectoryOptions = FileSystemGetDirectoryOptions::new();
    options.set_create(true);

    js::await_and_cast(parent.get_directory_handle_with_options(name, &options)).await
}

/// The child directory, or `None` when it is absent (a `NotFoundError`). Other rejections propagate.
pub async fn get_directory(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<Option<FileSystemDirectoryHandle>, AppError> {
    let directory_promise: Promise = parent.get_directory_handle(name);
    match JsFuture::from(directory_promise).await {
        Ok(value) => Ok(Some(js::dyn_into::<FileSystemDirectoryHandle>(value)?)),
        Err(error) if js::is_dom_exception_named(&error, "NotFoundError") => Ok(None),
        Err(error) => Err(js::error(error)),
    }
}

/// The file handle, or `None` when it is absent (a `NotFoundError`). Other rejections propagate.
pub async fn get_file(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<Option<FileSystemFileHandle>, AppError> {
    let file_promise: Promise = parent.get_file_handle(name);
    match JsFuture::from(file_promise).await {
        Ok(value) => Ok(Some(js::dyn_into::<FileSystemFileHandle>(value)?)),
        Err(error) if js::is_dom_exception_named(&error, "NotFoundError") => Ok(None),
        Err(error) => Err(js::error(error)),
    }
}

pub async fn read_file_bytes(file: &File) -> Result<Vec<u8>, AppError> {
    let buffer_promise: Promise = file.array_buffer();
    let buffer_value: JsValue = JsFuture::from(buffer_promise).await.map_err(js::error)?;
    let array_buffer: ArrayBuffer = js::dyn_into(buffer_value)?;

    Ok(Uint8Array::new(&array_buffer).to_vec())
}

/// `FileSystemDirectoryHandle::keys()` returns a JS async iterator; each `next()` yields a promise
/// resolving to an `{ value, done }` object, so the drive loop awaits every step.
pub async fn list_directory_keys(handle: &FileSystemDirectoryHandle) -> Result<Vec<String>, AppError> {
    let iterator: AsyncIterator = handle.keys();

    let mut key_strings: Vec<String> = Vec::new();
    loop {
        let next_promise: Promise = iterator.next().map_err(js::error)?;
        let next_value: JsValue = JsFuture::from(next_promise).await.map_err(js::error)?;
        let next: IteratorNext = next_value.unchecked_into();

        if next.done() {
            break;
        }

        let key_string: String = next
            .value()
            .as_string()
            .ok_or_else(|| AppError::from("directory key is not a string".to_string()))?;
        key_strings.push(key_string);
    }

    Ok(key_strings)
}

pub async fn estimate() -> Result<StorageEstimate, AppError> {
    let window: web_sys::Window = js::get_window()?;

    let estimate_promise: Promise = window.navigator().storage().estimate().map_err(js::error)?;
    let estimate_value: JsValue = JsFuture::from(estimate_promise).await.map_err(js::error)?;

    Ok(estimate_value.unchecked_into::<StorageEstimate>())
}

pub async fn write_file(file_handle: &FileSystemFileHandle, bytes: &[u8]) -> Result<(), AppError> {
    let writable_value: JsValue = JsFuture::from(file_handle.create_writable())
        .await
        .map_err(write_error)?;
    let writable: FileSystemWritableFileStream = js::dyn_into(writable_value)?;

    let data: Uint8Array = js::owned_uint8_array(bytes);
    let write_promise: Promise = writable.write_with_js_u8_array(&data).map_err(write_error)?;
    JsFuture::from(write_promise).await.map_err(write_error)?;

    JsFuture::from(writable.close()).await.map_err(write_error)?;

    Ok(())
}

/// Best-effort pre-check that fails with the `opfs: quota exceeded` sentinel when writing `incoming_len`
/// bytes would leave less than the safety margin free. Not the sole guard: a missing estimate is treated
/// permissively, and a true overflow still surfaces as a `QuotaExceededError` from the write itself.
pub async fn check_quota(incoming_len: usize) -> Result<(), AppError> {
    let storage_estimate: StorageEstimate = estimate().await?;

    let usage: f64 = storage_estimate.get_usage().unwrap_or(0.0);
    let quota: f64 = storage_estimate.get_quota().unwrap_or(f64::INFINITY);

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

/// A `QuotaExceededError` gets a clear `opfs: quota exceeded` prefix for diagnostics; other rejections
/// pass through as their raw JS message.
fn write_error(error: JsValue) -> AppError {
    if js::is_dom_exception_named(&error, "QuotaExceededError") {
        return AppError::from(format!("{ERROR_PREFIX_QUOTA_EXCEEDED}: {}", js::error_message(&error)));
    }

    js::error(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn quota_allows_leaves_the_safety_margin_free() {
        assert!(quota_allows(0.0, QUOTA_SAFETY_MARGIN_BYTES + 100.0, 100));
        assert!(!quota_allows(0.0, QUOTA_SAFETY_MARGIN_BYTES + 100.0, 101));
        assert!(!quota_allows(QUOTA_SAFETY_MARGIN_BYTES, QUOTA_SAFETY_MARGIN_BYTES + 50.0, 100));
    }
}
