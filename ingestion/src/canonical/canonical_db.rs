use sqlx::PgExecutor;

use shared::canonical::canonical_model::{Country, DataSource, DataSourceKind, Region, Statistic};

use crate::canonical::canonical_entity::{
    CountryEntity, DataSourceEntity, RegionEntity, StatisticEntity,
};
use crate::error::AppError;

pub async fn find_country_by_iso3<'e>(
    executor: impl PgExecutor<'e>,
    iso3: &str,
) -> Result<Option<Country>, AppError> {
    let country_entity: Option<CountryEntity> = sqlx::query_as!(
        CountryEntity,
        r#"
        select region_id, iso3, iso2, created, modified
        from country
        where iso3 = $1
        "#,
        iso3,
    )
    .fetch_optional(executor)
    .await?;
    Ok(country_entity.map(Country::from))
}

pub async fn find_country_by_iso2<'e>(
    executor: impl PgExecutor<'e>,
    iso2: &str,
) -> Result<Option<Country>, AppError> {
    let country_entity: Option<CountryEntity> = sqlx::query_as!(
        CountryEntity,
        r#"
        select region_id, iso3, iso2, created, modified
        from country
        where iso2 = $1
        "#,
        iso2,
    )
    .fetch_optional(executor)
    .await?;
    Ok(country_entity.map(Country::from))
}

pub async fn find_region_by_code<'e>(
    executor: impl PgExecutor<'e>,
    code: &str,
) -> Result<Option<Region>, AppError> {
    let region_entity: Option<RegionEntity> = sqlx::query_as!(
        RegionEntity,
        r#"
        select id, code, name_en, level, parent_region_id, m49_code, nuts_code, iso_3166_2, created, modified
        from region
        where code = $1
        "#,
        code,
    )
    .fetch_optional(executor)
    .await?;

    Ok(region_entity.map(Region::from))
}

pub async fn find_region_by_nuts_code<'e>(
    executor: impl PgExecutor<'e>,
    nuts_code: &str,
) -> Result<Option<Region>, AppError> {
    let region_entity: Option<RegionEntity> = sqlx::query_as!(
        RegionEntity,
        r#"
        select id, code, name_en, level, parent_region_id, m49_code, nuts_code, iso_3166_2, created, modified
        from region
        where nuts_code = $1
        "#,
        nuts_code,
    )
    .fetch_optional(executor)
    .await?;

    Ok(region_entity.map(Region::from))
}

pub async fn find_region_by_iso_3166_2<'e>(
    executor: impl PgExecutor<'e>,
    iso_3166_2: &str,
) -> Result<Option<Region>, AppError> {
    let region_entity: Option<RegionEntity> = sqlx::query_as!(
        RegionEntity,
        r#"
        select id, code, name_en, level, parent_region_id, m49_code, nuts_code, iso_3166_2, created, modified
        from region
        where iso_3166_2 = $1
        "#,
        iso_3166_2,
    )
    .fetch_optional(executor)
    .await?;

    Ok(region_entity.map(Region::from))
}

pub async fn find_statistic_by_code<'e>(
    executor: impl PgExecutor<'e>,
    code: &str,
) -> Result<Option<Statistic>, AppError> {
    let statistic_entity: Option<StatisticEntity> = sqlx::query_as!(
        StatisticEntity,
        r#"
        select id, code, name_en, units, created, modified
        from statistic
        where code = $1
        "#,
        code,
    )
    .fetch_optional(executor)
    .await?;
    Ok(statistic_entity.map(Statistic::from))
}

pub async fn find_data_source_by_kind<'e>(
    executor: impl PgExecutor<'e>,
    kind: DataSourceKind,
) -> Result<Option<DataSource>, AppError> {
    let data_source_entity: Option<DataSourceEntity> = sqlx::query_as!(
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

    data_source_entity.map(DataSource::try_from).transpose()
}
