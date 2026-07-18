use js_sys::{ArrayBuffer, AsyncIterator, IteratorNext, Promise, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    File, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetDirectoryOptions, StorageEstimate,
};

use shared::AppError;

use crate::client::js;

pub async fn root() -> Result<FileSystemDirectoryHandle, AppError> {
    let window: web_sys::Window = js::get_window()?;

    let root_value: JsValue = JsFuture::from(window.navigator().storage().get_directory())
        .await
        .map_err(js::error)?;

    root_value.dyn_into::<FileSystemDirectoryHandle>().map_err(js::type_error)
}

/// Requests persistent (non-evictable) storage; `Ok(true)` if granted, `Ok(false)` if denied.
pub async fn request_persistence() -> Result<bool, AppError> {
    let window: web_sys::Window = js::get_window()?;

    let persist_promise: Promise = window.navigator().storage().persist().map_err(js::error)?;
    let granted: JsValue = JsFuture::from(persist_promise).await.map_err(js::error)?;
    let granted: bool = granted.as_bool().unwrap_or(false);
    Ok(granted)
}

pub async fn await_and_cast<T: JsCast>(promise: Promise) -> Result<T, AppError> {
    let value: JsValue = JsFuture::from(promise).await.map_err(js::error)?;

    value.dyn_into::<T>().map_err(js::type_error)
}

pub async fn get_or_create_directory(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<FileSystemDirectoryHandle, AppError> {
    let options: FileSystemGetDirectoryOptions = FileSystemGetDirectoryOptions::new();
    options.set_create(true);

    await_and_cast(parent.get_directory_handle_with_options(name, &options)).await
}

/// The child directory, or `None` when it is absent (a `NotFoundError`). Other rejections propagate.
pub async fn get_directory_if_present(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<Option<FileSystemDirectoryHandle>, AppError> {
    match JsFuture::from(parent.get_directory_handle(name)).await {
        Ok(value) => Ok(Some(value.dyn_into::<FileSystemDirectoryHandle>().map_err(js::type_error)?)),
        Err(error) if js::is_dom_exception_named(&error, "NotFoundError") => Ok(None),
        Err(error) => Err(js::error(error)),
    }
}

/// The file handle, or `None` when it is absent (a `NotFoundError`). Other rejections propagate.
pub async fn get_file_handle_if_present(
    parent: &FileSystemDirectoryHandle,
    name: &str,
) -> Result<Option<FileSystemFileHandle>, AppError> {
    match JsFuture::from(parent.get_file_handle(name)).await {
        Ok(value) => Ok(Some(value.dyn_into::<FileSystemFileHandle>().map_err(js::type_error)?)),
        Err(error) if js::is_dom_exception_named(&error, "NotFoundError") => Ok(None),
        Err(error) => Err(js::error(error)),
    }
}

pub async fn read_file_bytes(file: &File) -> Result<Vec<u8>, AppError> {
    let buffer_value: JsValue = JsFuture::from(file.array_buffer()).await.map_err(js::error)?;
    let array_buffer: ArrayBuffer = buffer_value.dyn_into().map_err(js::type_error)?;

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
        let iterator_next: IteratorNext = next_value.dyn_into().map_err(js::type_error)?;

        if iterator_next.done() {
            break;
        }

        let key_string: String = iterator_next
            .value()
            .as_string()
            .ok_or_else(|| AppError::from("directory key is not a string".to_string()))?;
        key_strings.push(key_string);
    }

    Ok(key_strings)
}

/// The storage estimate, or `None` when there is no window to query.
pub async fn estimate() -> Result<Option<StorageEstimate>, AppError> {
    let Ok(window) = js::get_window() else {
        return Ok(None);
    };

    let estimate_promise: Promise = window.navigator().storage().estimate().map_err(js::error)?;
    let estimate_value: JsValue = JsFuture::from(estimate_promise).await.map_err(js::error)?;

    Ok(Some(estimate_value.dyn_into::<StorageEstimate>().map_err(js::type_error)?))
}
