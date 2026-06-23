//! Newtype around `minimer::AppError`. The orphan rule blocks adding
//! `From` impls for foreign errors directly to the upstream type, so each
//! crate that wants its own `From<X>` set defines its own newtype via
//! `minimer::define_app_error!`. `core::AppError` is the consumer-side
//! parser-surface newtype; this is the producer-side newtype with HTTP /
//! database / secrets / archive conversions on top.

minimer::define_app_error!(pub AppError);

minimer::impl_from_error!(AppError, sqlx::Error);
minimer::impl_from_error!(AppError, reqwest::Error);
minimer::impl_from_error!(AppError, serde_json::Error);
minimer::impl_from_error!(AppError, rusqlite::Error);
minimer::impl_from_error!(AppError, zip::result::ZipError);
minimer::impl_from_error!(AppError, shapefile::Error);
minimer::impl_from_error!(AppError, shapefile::dbase::Error);
minimer::impl_from_error!(AppError, flatgeobuf::Error);
minimer::impl_from_error!(AppError, geozero::error::GeozeroError);
minimer::impl_from_error!(AppError, log::SetLoggerError);
minimer::impl_from_error!(AppError, secr::error::Error);
minimer::impl_from_error!(AppError, dotenvy::Error);
minimer::impl_from_error!(AppError, base64::DecodeError);

/// Cross-conversion bridge: lets ingestion `?`-propagate from `eafora_core::*`
/// functions. Both newtypes wrap the same `minimer::AppError`, so the inner
/// `.0` move is correct. Orphan-rule-OK because the target type
/// (`ingestion::AppError`) is local to ingestion.
impl From<eafora_core::AppError> for AppError {
    fn from(err: eafora_core::AppError) -> Self {
        Self(err.0)
    }
}

pub use eafora_core::error::render_error_chain;
