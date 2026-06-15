use crate::error::AppError;

pub fn read_var(name: &str) -> Result<String, AppError> {
    dotenvy::var(name).map_err(|err| AppError::from(format!("env var {}: {}", name, err)))
}
