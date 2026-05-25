//! Project-level error type. `AppError` is a newtype wrapping `minimer::AppError`
//! that lets us register `From` conversions for foreign error types via
//! `minimer::impl_from_error!` (the orphan rule prevents us from adding
//! conversions onto `minimer::AppError` directly).
//!
//! Construct via `AppError::new(&format!("module: ..."))` for plain messages,
//! `AppError::from_error("module: ...", Box::new(err))` to wrap a source error,
//! `AppError::from(format!("..."))` via the `From<String>` impl, or
//! `?` for any error type registered with `impl_from_error!` below.

minimer::define_app_error!(pub AppError);

minimer::impl_from_error!(AppError, std::env::VarError);
minimer::impl_from_error!(AppError, sqlx::Error);
minimer::impl_from_error!(AppError, reqwest::Error);
minimer::impl_from_error!(AppError, serde_json::Error);
