use sqlx::PgExecutor;

use crate::canonical::canonical_model::{Country, DataSource, DataSourceKind, DataSourceEntity, Statistic};
use crate::error::AppError;

pub async fn find_country_by_iso3<'e>(
    executor: impl PgExecutor<'e>,
    iso3: &str,
) -> Result<Option<Country>, AppError> {
    let record: Option<Country> = sqlx::query_as!(
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
    Ok(record)
}

pub async fn find_statistic_by_code<'e>(
    executor: impl PgExecutor<'e>,
    code: &str,
) -> Result<Option<Statistic>, AppError> {
    let record: Option<Statistic> = sqlx::query_as!(
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
    Ok(record)
}

pub async fn find_data_source_by_kind<'e>(
    executor: impl PgExecutor<'e>,
    kind: DataSourceKind,
) -> Result<Option<DataSource>, AppError> {
    let record: Option<DataSourceEntity> = sqlx::query_as!(
        DataSourceEntity,
        r#"
        select id, code, name_en, homepage_url, license_class, license_name, license_url, attribution_text, preference_rank, created, modified
        from data_source
        where code = $1
        "#,
        kind.code(),
    )
    .fetch_optional(executor)
    .await?;

    record.map(DataSource::try_from).transpose()
}
