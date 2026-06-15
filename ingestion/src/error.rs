//! Newtype around `minimer::AppError`. The orphan rule blocks adding
//! `From` impls for foreign errors directly to the upstream type.

use std::error::Error;

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

/// Walk an error's `source()` chain and concatenate each level's Display
/// joined by ` -> `. Library errors (especially the AWS SDK family) often
/// summarize at the top level (e.g. "service error", "dispatch failure")
/// and carry the actual connector / HTTP / TLS detail in the source chain.
pub fn render_error_chain(error: &dyn Error) -> String {
    let mut rendered: String = error.to_string();
    let mut next: Option<&dyn Error> = error.source();

    while let Some(source) = next {
        rendered.push_str(" -> ");
        rendered.push_str(&source.to_string());
        next = source.source();
    }

    rendered
}
