include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));

pub mod app;

#[cfg(feature = "hydrate")]
mod hydrate;
