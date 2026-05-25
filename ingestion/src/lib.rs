//! Eafora canonical-store ingestion crate.
//!
//! Subsequent PRs add the per-source adapter modules (`world_bank_wdi/`),
//! the shared canonical-store reads (`canonical/`), and the schema migrations
//! under `db/migrations/`.

pub mod db;
pub mod error;
