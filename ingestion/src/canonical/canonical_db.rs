//! Lookups against the canonical-store reference tables. Every adapter's
//! normalize step calls these to resolve foreign-key IDs from human-readable
//! codes.

use sqlx::PgPool;

use crate::canonical::canonical_model::{Country, DataSource, Statistic};
use crate::error::AppError;

pub async fn find_country_by_iso3(pool: &PgPool, iso3: &str) -> Result<Option<Country>, AppError> {
    let country_row: Option<Country> = sqlx::query_as!(
        Country,
        r#"
        select region_id, iso3, iso2, created, modified, deleted
        from country
        where iso3 = $1
        "#,
        iso3,
    )
    .fetch_optional(pool)
    .await?;
    Ok(country_row)
}

pub async fn find_statistic_by_code(pool: &PgPool, code: &str) -> Result<Option<Statistic>, AppError> {
    let statistic_row: Option<Statistic> = sqlx::query_as!(
        Statistic,
        r#"
        select id, code, name_en, description, units, created, modified
        from statistic
        where code = $1
        "#,
        code,
    )
    .fetch_optional(pool)
    .await?;
    Ok(statistic_row)
}

pub async fn find_data_source_by_code(pool: &PgPool, code: &str) -> Result<Option<DataSource>, AppError> {
    let data_source_row: Option<DataSource> = sqlx::query_as!(
        DataSource,
        r#"
        select id, code, name_en, homepage_url, license_class, license_name, license_url, attribution_text, preference_rank, created, modified
        from data_source
        where code = $1
        "#,
        code,
    )
    .fetch_optional(pool)
    .await?;
    Ok(data_source_row)
}
