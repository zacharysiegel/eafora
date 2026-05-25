//! Project-level error type.
//!
//! Currently a thin string-wrapper. Will be replaced with `minimer::Error`
//! once the minimer crate is wired into `ingestion/Cargo.toml`. The public
//! surface (`AppError`, `From<String>`, `From<&str>`) is shaped to match
//! minimer so the swap is mechanical.

use std::fmt;

#[derive(Debug)]
pub struct AppError(String);

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AppError {}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for AppError {
    fn from(message: &str) -> Self {
        Self(message.to_string())
    }
}
