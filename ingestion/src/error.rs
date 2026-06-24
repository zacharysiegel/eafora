//! Newtype around `minimer::AppError`. The orphan rule blocks adding
//! `From` impls for foreign errors directly to the upstream type.

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

impl From<shared::AppError> for AppError {
    fn from(err: shared::AppError) -> Self {
        Self(err.0)
    }
}
