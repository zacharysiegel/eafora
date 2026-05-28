//! Lookups against the canonical-store reference tables. Every adapter's
//! normalize step calls these to resolve foreign-key IDs from human-readable
//! codes.
//!
//! All functions take `impl PgExecutor<'_>` so callers can pass either a
//! `&PgPool` (production code, each call acquires its own connection) or a
//! `&mut *tx` re-borrow (tests, all calls run inside one transaction that
//! gets rolled back at teardown).

use sqlx::PgExecutor;

use crate::canonical::canonical_model::{Country, DataSource, LicenseClass, Statistic};
use crate::error::AppError;

pub async fn find_country_by_iso3<'e>(
    executor: impl PgExecutor<'e>,
    iso3: &str,
) -> Result<Option<Country>, AppError> {
    let country_row: Option<Country> = sqlx::query_as!(
        Country,
        r#"
        select region_id, iso3, iso2, created, modified, deleted
        from country
        where iso3 = $1
        "#,
        iso3,
    )
    .fetch_optional(executor)
    .await?;
    Ok(country_row)
}

pub async fn find_statistic_by_code<'e>(
    executor: impl PgExecutor<'e>,
    code: &str,
) -> Result<Option<Statistic>, AppError> {
    let statistic_row: Option<Statistic> = sqlx::query_as!(
        Statistic,
        r#"
        select id, code, name_en, description, units, created, modified
        from statistic
        where code = $1
        "#,
        code,
    )
    .fetch_optional(executor)
    .await?;
    Ok(statistic_row)
}

pub async fn find_data_source_by_code<'e>(
    executor: impl PgExecutor<'e>,
    code: &str,
) -> Result<Option<DataSource>, AppError> {
    struct DataSourceRow {
        id: uuid::Uuid,
        code: String,
        name_en: String,
        homepage_url: String,
        license_class: String,
        license_name: String,
        license_url: String,
        attribution_text: String,
        preference_rank: i32,
        created: chrono::DateTime<chrono::Utc>,
        modified: chrono::DateTime<chrono::Utc>,
    }

    let row: Option<DataSourceRow> = sqlx::query_as!(
        DataSourceRow,
        r#"
        select id, code, name_en, homepage_url, license_class, license_name, license_url, attribution_text, preference_rank, created, modified
        from data_source
        where code = $1
        "#,
        code,
    )
    .fetch_optional(executor)
    .await?;

    row.map(|data_source_row| {
        Ok(DataSource {
            id: data_source_row.id,
            code: data_source_row.code,
            name_en: data_source_row.name_en,
            homepage_url: data_source_row.homepage_url,
            license_class: LicenseClass::parse_str(&data_source_row.license_class)?,
            license_name: data_source_row.license_name,
            license_url: data_source_row.license_url,
            attribution_text: data_source_row.attribution_text,
            preference_rank: data_source_row.preference_rank,
            created: data_source_row.created,
            modified: data_source_row.modified,
        })
    })
    .transpose()
}
