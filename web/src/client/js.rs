use js_sys::Promise;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::DomException;

use shared::AppError;

pub fn get_window() -> Result<web_sys::Window, AppError> {
    web_sys::window().ok_or_else(|| AppError::from("no window".to_string()))
}

pub async fn await_and_cast<T: JsCast>(promise: Promise) -> Result<T, AppError> {
    let value: JsValue = JsFuture::from(promise).await.map_err(error)?;

    value.dyn_into::<T>().map_err(type_error)
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
