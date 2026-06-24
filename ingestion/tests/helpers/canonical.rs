//! Canonical-store lookup helpers for integration tests. Wraps the
//! `canonical_db::find_*` Option-returning helpers with panic-on-missing
//! semantics so test bodies stay flat.
#![allow(dead_code)]

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use ingestion::canonical::canonical_db;
use shared::canonical::canonical_model::DataSourceKind;

pub async fn get_data_source_id(transaction: &mut Transaction<'static, Postgres>, kind: DataSourceKind) -> Uuid {
    canonical_db::find_data_source_by_kind(&mut **transaction, kind)
        .await
        .unwrap_or_else(|err| panic!("find data_source {:?}: {err}", kind))
        .unwrap_or_else(|| panic!("data_source {:?} not seeded", kind))
        .id
}

pub async fn get_country_region_id(transaction: &mut Transaction<'static, Postgres>, iso3: &str) -> Uuid {
    canonical_db::find_country_by_iso3(&mut **transaction, iso3)
        .await
        .unwrap_or_else(|err| panic!("find country {iso3}: {err}"))
        .unwrap_or_else(|| panic!("country {iso3} not seeded"))
        .region_id
}

pub async fn get_statistic_id(transaction: &mut Transaction<'static, Postgres>, code: &str) -> Uuid {
    canonical_db::find_statistic_by_code(&mut **transaction, code)
        .await
        .unwrap_or_else(|err| panic!("find statistic {code}: {err}"))
        .unwrap_or_else(|| panic!("statistic {code} not seeded"))
        .id
}
