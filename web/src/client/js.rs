use wasm_bindgen::{JsCast, JsValue};
use web_sys::DomException;

use shared::AppError;

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
