use std::fmt::{Display, Formatter, Result as FormatResult};

use shared::error::{AppError, AppErrorStatic};

/// What Swift catches, returned by exported functions only. `#[derive(uniffi::Error)]` requires an enum; a
/// caller that must distinguish failures matches on the message.
#[derive(Debug, uniffi::Error)]
pub enum FfiError {
    Failed { message: String },
}

impl Display for FfiError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FormatResult {
        match self {
            FfiError::Failed { message } => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for FfiError {}

impl From<AppError> for FfiError {
    fn from(error: AppError) -> FfiError {
        FfiError::Failed {
            message: error.to_string(),
        }
    }
}

impl From<AppErrorStatic> for FfiError {
    fn from(error: AppErrorStatic) -> FfiError {
        FfiError::Failed {
            message: error.to_string(),
        }
    }
}
