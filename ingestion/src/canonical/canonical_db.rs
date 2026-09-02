use sqlx::PgExecutor;

use shared::canonical::canonical_model::{Country, DataSource, DataSourceKind, Region, Statistic, Subdivision};

use crate::canonical::canonical_entity::{
    CountryEntity, DataSourceEntity, RegionEntity, StatisticEntity, SubdivisionEntity,
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
        select id, code, name_en, level, parent_region_id, m49_code, created, modified
        from region
        where code = $1
        "#,
        code,
    )
    .fetch_optional(executor)
    .await?;

    region_entity.map(Region::try_from).transpose()
}

pub async fn find_subdivision_by_nuts_code<'e>(
    executor: impl PgExecutor<'e>,
    nuts_code: &str,
) -> Result<Option<Subdivision>, AppError> {
    let subdivision_entity: Option<SubdivisionEntity> = sqlx::query_as!(
        SubdivisionEntity,
        r#"
        select region_id, nuts_code, nuts_revision, iso_3166_2, created, modified
        from subdivision
        where nuts_code = $1
        "#,
        nuts_code,
    )
    .fetch_optional(executor)
    .await?;

    Ok(subdivision_entity.map(Subdivision::from))
}

pub async fn find_subdivision_by_iso_3166_2<'e>(
    executor: impl PgExecutor<'e>,
    iso_3166_2: &str,
) -> Result<Option<Subdivision>, AppError> {
    let subdivision_entity: Option<SubdivisionEntity> = sqlx::query_as!(
        SubdivisionEntity,
        r#"
        select region_id, nuts_code, nuts_revision, iso_3166_2, created, modified
        from subdivision
        where iso_3166_2 = $1
        "#,
        iso_3166_2,
    )
    .fetch_optional(executor)
    .await?;

    Ok(subdivision_entity.map(Subdivision::from))
}

/// The NUTS revision every seeded code belongs to. More than one would mean the store models two namings of
/// the same territory at once, which no map layer can draw and no lookup by code alone can disambiguate.
pub async fn read_nuts_revision<'e>(executor: impl PgExecutor<'e>) -> Result<Option<i32>, AppError> {
    let nuts_revisions: Vec<i32> = sqlx::query_scalar!(
        r#"
        select distinct nuts_revision as "nuts_revision!"
        from subdivision
        where nuts_revision is not null
        "#,
    )
    .fetch_all(executor)
    .await?;

    if nuts_revisions.len() > 1 {
        return Err(AppError::from(format!(
            "subdivision holds more than one NUTS revision; [revisions={nuts_revisions:?}]",
        )));
    }

    Ok(nuts_revisions.into_iter().next())
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
