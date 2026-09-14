use js_sys::{ArrayBuffer, Promise, Uint8Array};
use web_sys::{RequestCache, RequestInit, Window};

use shared::http::{HttpCacheMode, HttpFetch, HttpRequest, Response};
use shared::error::AppErrorStatic;

use crate::client::js;

pub struct BrowserFetch;

impl HttpFetch for BrowserFetch {
    /// Errors only on a transport or JS failure; a non-2xx status is returned for the caller to judge.
    async fn fetch(&self, request: &HttpRequest) -> Result<Response, AppErrorStatic> {
        let window: Window = js::get_window()?;

        let init: RequestInit = RequestInit::new();
        init.set_method(request.method.as_str());

        match request.cache_mode {
            HttpCacheMode::Reload => init.set_cache(RequestCache::Reload),
            HttpCacheMode::Default => {}
        }

        let fetch_promise: Promise = window.fetch_with_str_and_init(&request.url, &init);
        let response: web_sys::Response = js::await_and_cast(fetch_promise).await?;

        let status: u16 = response.status();
        let buffer_promise: Promise = response.array_buffer().map_err(js::error)?;
        let array_buffer: ArrayBuffer = js::await_and_cast(buffer_promise).await?;
        let bytes: Vec<u8> = Uint8Array::new(&array_buffer).to_vec();

        Ok(Response { status, bytes })
    }
}
