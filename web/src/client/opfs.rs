use js_sys::{ArrayBuffer, AsyncIterator, IteratorNext, Promise, Uint8Array};
use wasm_bindgen::JsValue;
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
    match JsFuture::from(parent.get_directory_handle(name)).await {
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
    match JsFuture::from(parent.get_file_handle(name)).await {
        Ok(value) => Ok(Some(js::dyn_into::<FileSystemFileHandle>(value)?)),
        Err(error) if js::is_dom_exception_named(&error, "NotFoundError") => Ok(None),
        Err(error) => Err(js::error(error)),
    }
}

pub async fn read_file_bytes(file: &File) -> Result<Vec<u8>, AppError> {
    let buffer_value: JsValue = JsFuture::from(file.array_buffer()).await.map_err(js::error)?;
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
        let iterator_next: IteratorNext = js::dyn_into(next_value)?;

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

pub async fn estimate() -> Result<StorageEstimate, AppError> {
    let window: web_sys::Window = js::get_window()?;

    let estimate_promise: Promise = window.navigator().storage().estimate().map_err(js::error)?;
    let estimate_value: JsValue = JsFuture::from(estimate_promise).await.map_err(js::error)?;

    js::dyn_into::<StorageEstimate>(estimate_value)
}
