use js_sys::{ArrayBuffer, Promise, Uint8Array};
use web_sys::Response;

use shared::AppError;

use crate::client::js;

/// GETs `url` and returns the response body. A non-2xx status maps to an `AppError` carrying the URL
/// and status so the loader can surface which fetch failed (FR-041).
pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, AppError> {
    let window: web_sys::Window = js::get_window()?;

    let fetch_promise: Promise = window.fetch_with_str(url);
    let response: Response = js::await_and_cast(fetch_promise).await?;

    if !response.ok() {
        return Err(AppError::from(format!("fetch: {url} returned HTTP {}", response.status())));
    }

    let buffer_promise: Promise = response.array_buffer().map_err(js::error)?;
    let array_buffer: ArrayBuffer = js::await_and_cast(buffer_promise).await?;

    Ok(Uint8Array::new(&array_buffer).to_vec())
}
