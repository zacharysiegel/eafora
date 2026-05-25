//! Project-level error type — re-exports `minimer::AppError`. Construct via
//! `AppError::new(&format!("module: ..."))` for plain messages, or
//! `AppError::from_error("module: ...", Box::new(err))` to wrap a source error.
//!
//! Note: minimer's `impl_from_error!` macro is intended for use inside
//! minimer itself (orphan-rule constraints prevent downstream crates from
//! invoking it for foreign error types). Until minimer ships a downstream-
//! friendly extension surface, call sites use explicit `.map_err(|err| ...)`
//! to convert from std and ecosystem error types into `AppError`.

pub use minimer::AppError;
