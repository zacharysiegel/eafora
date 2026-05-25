//! Project-level error type — re-exports `minimer::AppError`. Construct via
//! `AppError::new(&format!("module: ..."))` for plain messages, or
//! `AppError::from_error("module: ...", Box::new(err))` to wrap a source error.
//! Convert ecosystem error types via explicit `.map_err(|err| AppError::new(...))`
//! at the call site.

pub use minimer::AppError;
