use js_sys::{ArrayBuffer, Promise, Uint8Array};
use web_sys::{RequestInit, Window};

use shared::AppError;
use shared::http::{HttpRequest, Response};

use crate::client::js;

/// Issues `request` and returns the status and body without judging the status; the caller decides what
/// a non-2xx means. Errors only on a transport/JS failure.
pub async fn fetch(request: &HttpRequest) -> Result<Response, AppError> {
    let window: Window = js::get_window()?;

    let init: RequestInit = RequestInit::new();
    init.set_method(request.method.as_str());

    let fetch_promise: Promise = window.fetch_with_str_and_init(&request.url, &init);
    let response: web_sys::Response = js::await_and_cast(fetch_promise).await?;

    let status: u16 = response.status();
    let buffer_promise: Promise = response.array_buffer().map_err(js::error)?;
    let array_buffer: ArrayBuffer = js::await_and_cast(buffer_promise).await?;
    let bytes: Vec<u8> = Uint8Array::new(&array_buffer).to_vec();

    Ok(Response { status, bytes })
}

pub async fn fetch_bytes(request: &HttpRequest) -> Result<Vec<u8>, AppError> {
    let response: Response = fetch(request).await?;

    if !response.is_success() {
        return Err(AppError::from(format!("fetch: {} returned HTTP {}", request.url, response.status)));
    }

    Ok(response.bytes)
}
