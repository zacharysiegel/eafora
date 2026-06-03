use sqlx::PgExecutor;

use crate::canonical::canonical_model::{
    Country, CountryEntity, DataSource, DataSourceEntity, DataSourceKind, SourceChoice, SourceChoiceEntity,
    Statistic, StatisticEntity,
};
use crate::error::AppError;

pub async fn find_country_by_iso3<'e>(
    executor: impl PgExecutor<'e>,
    iso3: &str,
) -> Result<Option<Country>, AppError> {
    let country_entity: Option<CountryEntity> = sqlx::query_as!(
        CountryEntity,
        r#"
        select region_id, iso3, iso2, created, modified, deleted
        from country
        where iso3 = $1
        "#,
        iso3,
    )
    .fetch_optional(executor)
    .await?;
    Ok(country_entity.map(Country::from))
}

pub async fn find_statistic_by_code<'e>(
    executor: impl PgExecutor<'e>,
    code: &str,
) -> Result<Option<Statistic>, AppError> {
    let statistic_entity: Option<StatisticEntity> = sqlx::query_as!(
        StatisticEntity,
        r#"
        select id, code, name_en, description, units, created, modified
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

pub async fn read_source_choices<'e>(executor: impl PgExecutor<'e>) -> Result<Vec<SourceChoice>, AppError> {
    let source_choice_entities: Vec<SourceChoiceEntity> = sqlx::query_as!(
        SourceChoiceEntity,
        r#"
        select id, region_id, statistic_id, license_shard_class, data_source_id, created, modified
        from source_choice
        "#,
    )
    .fetch_all(executor)
    .await?;

    source_choice_entities.into_iter().map(SourceChoice::try_from).collect()
}
