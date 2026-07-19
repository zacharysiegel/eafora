use js_sys::{Promise, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::DomException;

use shared::AppError;

pub fn get_window() -> Result<web_sys::Window, AppError> {
    web_sys::window().ok_or_else(|| AppError::from("no window".to_string()))
}

/// A JS-heap-owned `Uint8Array` copy of `bytes`. Async JS APIs (an OPFS writable, a `fetch` body) may
/// read their argument after the call returns; a `Uint8Array` view over wasm linear memory can detach
/// if wasm memory grows in the meantime, so bytes handed to such an API must be owned, not viewed.
pub fn owned_uint8_array(bytes: &[u8]) -> Uint8Array {
    let array: Uint8Array = Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    array
}

pub async fn await_and_cast<T: JsCast>(promise: Promise) -> Result<T, AppError> {
    let value: JsValue = JsFuture::from(promise).await.map_err(error)?;

    dyn_into(value)
}

pub fn dyn_into<T: JsCast>(value: JsValue) -> Result<T, AppError> {
    value.dyn_into::<T>().map_err(type_error)
}

// The by-reference sibling of `dyn_into`, kept for API parity; no call site needs it yet.
#[allow(dead_code)]
pub fn dyn_ref<T: JsCast>(value: &JsValue) -> Result<&T, AppError> {
    value.dyn_ref::<T>().ok_or_else(|| type_error(value.clone()))
}

pub fn error_message(error: &JsValue) -> String {
    if let Some(dom_exception) = error.dyn_ref::<DomException>() {
        return format!("{}: {}", dom_exception.name(), dom_exception.message());
    }

    error.as_string().unwrap_or_else(|| format!("{error:?}"))
}

pub fn error(error: JsValue) -> AppError {
    AppError::from(error_message(&error))
}

pub fn type_error(value: JsValue) -> AppError {
    AppError::from(format!("unexpected JS value type: {value:?}"))
}

pub fn is_dom_exception_named(error: &JsValue, name: &str) -> bool {
    error
        .dyn_ref::<DomException>()
        .is_some_and(|dom_exception| dom_exception.name() == name)
}
