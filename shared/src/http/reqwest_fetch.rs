use bytes::Bytes;
use reqwest::header::{self, HeaderValue};
use reqwest::{Client, RequestBuilder, StatusCode};

use crate::error::AppErrorStatic;
use crate::http::{HttpCacheMode, HttpFetch, HttpMethod, HttpRequest, Response};

pub struct ReqwestHttpFetch {
    client: Client,
}

impl ReqwestHttpFetch {
    pub fn create() -> Result<ReqwestHttpFetch, AppErrorStatic> {
        let client: Client = Client::builder()
            .build()
            .map_err(|error| AppErrorStatic::from(format!("building an HTTP client failed; [error={error}]")))?;

        Ok(ReqwestHttpFetch { client })
    }
}

impl HttpFetch for ReqwestHttpFetch {
    /// Errors only on a transport failure; a non-2xx status is returned for the caller to judge.
    async fn fetch(&self, request: &HttpRequest) -> Result<Response, AppErrorStatic> {
        let builder: RequestBuilder = match request.method {
            HttpMethod::Get => self.client.get(&request.url),
        };

        let builder: RequestBuilder = match request.cache_mode {
            // reqwest holds no cache of its own; the header is addressed to intermediaries.
            HttpCacheMode::Reload => builder.header(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            HttpCacheMode::Default => builder,
        };

        let response: reqwest::Response = builder
            .send()
            .await
            .map_err(|error| AppErrorStatic::from(format!("fetching {} failed; [error={error}]", request.url)))?;

        let status: StatusCode = response.status();
        let bytes: Bytes = response
            .bytes()
            .await
            .map_err(|error| AppErrorStatic::from(format!("reading {} failed; [error={error}]", request.url)))?;

        Ok(Response {
            status: status.as_u16(),
            bytes: bytes.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::JoinHandle;

    use super::*;

    /// Serves `status_line` once on a loopback port, so a test covers the status arm without a network or
    /// an HTTP server dependency. Returns the URL to ask for.
    fn serve_one_response(status_line: &'static str, body: &'static str) -> String {
        let listener: TcpListener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url: String = format!("http://{}/manifest.json", listener.local_addr().unwrap());

        std::thread::spawn(move || {
            let (mut stream, _address) = listener.accept().unwrap();

            let mut request_bytes: [u8; 1024] = [0; 1024];
            let _read: usize = stream.read(&mut request_bytes).unwrap();

            let response: String = format!(
                "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len(),
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        url
    }

    fn get_request(url: String) -> HttpRequest {
        HttpRequest {
            method: HttpMethod::Get,
            url,
            cache_mode: HttpCacheMode::Default,
        }
    }

    #[tokio::test]
    async fn fetch_returns_the_body_of_a_successful_response() {
        let http_fetch: ReqwestHttpFetch = ReqwestHttpFetch::create().unwrap();
        let url: String = serve_one_response("200 OK", "{}");

        let response: Response = http_fetch.fetch(&get_request(url)).await.unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.bytes, b"{}");
    }

    /// A missing artifact is the caller's decision to make, so the status comes back rather than an error.
    #[tokio::test]
    async fn fetch_reports_a_non_success_status_without_erroring() {
        let http_fetch: ReqwestHttpFetch = ReqwestHttpFetch::create().unwrap();
        let url: String = serve_one_response("404 Not Found", "missing");

        let response: Response = http_fetch.fetch(&get_request(url)).await.unwrap();

        assert_eq!(response.status, 404);
        assert!(!response.is_success());
    }

    /// The transport-failure arm, reached without a network: the URL never becomes a request at all.
    #[tokio::test]
    async fn fetch_errors_when_the_url_cannot_be_parsed() {
        let http_fetch: ReqwestHttpFetch = ReqwestHttpFetch::create().unwrap();

        let error: AppErrorStatic = http_fetch
            .fetch(&get_request("manifest.json".to_string()))
            .await
            .unwrap_err();

        assert!(error.to_string().contains("fetching manifest.json failed"));
    }

    #[tokio::test]
    async fn fetch_asks_intermediaries_to_revalidate_when_the_cache_mode_is_reload() {
        let listener: TcpListener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url: String = format!("http://{}/manifest.json", listener.local_addr().unwrap());

        let request_header_thread: JoinHandle<String> = std::thread::spawn(move || {
            let (mut stream, _address): (TcpStream, _) = listener.accept().unwrap();

            let mut request_bytes: [u8; 1024] = [0; 1024];
            let read: usize = stream.read(&mut request_bytes).unwrap();
            let request_text: String = String::from_utf8_lossy(&request_bytes[..read]).into_owned();

            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .unwrap();

            request_text
        });

        let http_fetch: ReqwestHttpFetch = ReqwestHttpFetch::create().unwrap();
        http_fetch
            .fetch(&HttpRequest {
                method: HttpMethod::Get,
                url,
                cache_mode: HttpCacheMode::Reload,
            })
            .await
            .unwrap();

        let request_text: String = request_header_thread.join().unwrap();
        assert!(request_text.to_lowercase().contains("cache-control: no-cache"));
    }
}
