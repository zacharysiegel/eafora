use std::fmt::{Display, Formatter, Result as FormatResult};

use shared::AppError;

/// What Swift catches. UniFFI maps an exported error enum to a `throws`, and requires an enum, which the
/// crate-wide `AppError` is not; callers that must distinguish failures match on the message.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_app_error_carries_the_message() {
        let error: AppError = AppError::from("the manifest names a newer schema version".to_string());

        let ffi_error: FfiError = FfiError::from(error);

        match ffi_error {
            FfiError::Failed { message } => {
                assert!(message.contains("the manifest names a newer schema version"));
            }
        }
    }

    /// Swift shows the message, so `Display` must be the message alone and not wrap it in the variant name.
    #[test]
    fn display_renders_the_message_alone() {
        let ffi_error: FfiError = FfiError::Failed {
            message: "opfs unsupported".to_string(),
        };

        assert_eq!(ffi_error.to_string(), "opfs unsupported");
    }
}
