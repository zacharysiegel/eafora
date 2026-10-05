use std::fs;
use std::io::ErrorKind;

use crate::error::AppErrorStatic;
use crate::http::{HttpFetch, HttpRequest, Response};

/// An [`HttpFetch`] which treats each request's url as a filesystem path. A missing file answers 404.
pub struct FilesystemFetch;

impl HttpFetch for FilesystemFetch {
    async fn fetch(&self, request: &HttpRequest) -> Result<Response, AppErrorStatic> {
        let read: Result<Vec<u8>, std::io::Error> = fs::read(&request.url);

        match read {
            Ok(bytes) => Ok(Response { status: 200, bytes }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Response {
                status: 404,
                bytes: Vec::new(),
            }),
            Err(error) => Err(AppErrorStatic::from(format!(
                "reading {} failed; [error={error}]",
                request.url,
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::http::{HttpCacheMode, HttpMethod};

    use super::*;

    fn create_request(url: String) -> HttpRequest {
        HttpRequest {
            method: HttpMethod::Get,
            url,
            cache_mode: HttpCacheMode::Default,
        }
    }

    #[tokio::test]
    async fn fetch_answers_404_for_a_missing_file() {
        let directory: TempDir = TempDir::new().unwrap();
        let path: String = directory.path().join("absent.json").to_string_lossy().into_owned();

        let response: Response = FilesystemFetch.fetch(&create_request(path)).await.unwrap();

        assert_eq!(response.status, 404);
        assert!(!response.is_success());
    }
}
