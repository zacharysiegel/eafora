pub enum HttpMethod {
    Get,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
        }
    }
}

pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
}

pub struct Response {
    pub status: u16,
    pub bytes: Vec<u8>,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..=299).contains(&self.status)
    }
}
