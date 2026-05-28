use sqlx::PgExecutor;

use crate::canonical::canonical_model::{Country, DataSource, DataSourceCode, LicenseClass, Statistic};
use crate::error::AppError;

pub async fn find_country_by_iso3<'e>(
    executor: impl PgExecutor<'e>,
    iso3: &str,
) -> Result<Option<Country>, AppError> {
    let country_record: Option<Country> = sqlx::query_as!(
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
    Ok(country_record)
}

pub async fn find_statistic_by_code<'e>(
    executor: impl PgExecutor<'e>,
    code: &str,
) -> Result<Option<Statistic>, AppError> {
    let statistic_record: Option<Statistic> = sqlx::query_as!(
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
    Ok(statistic_record)
}

pub async fn find_data_source_by_code<'e>(
    executor: impl PgExecutor<'e>,
    code: DataSourceCode,
) -> Result<Option<DataSource>, AppError> {
    struct DataSourceRecord {
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

    let data_source_record: Option<DataSourceRecord> = sqlx::query_as!(
        DataSourceRecord,
        r#"
        select id, code, name_en, homepage_url, license_class, license_name, license_url, attribution_text, preference_rank, created, modified
        from data_source
        where code = $1
        "#,
        code.as_str(),
    )
    .fetch_optional(executor)
    .await?;

    data_source_record
        .map(|data_source_record| {
            Ok(DataSource {
                id: data_source_record.id,
                code: DataSourceCode::parse_str(&data_source_record.code)?,
                name_en: data_source_record.name_en,
                homepage_url: data_source_record.homepage_url,
                license_class: LicenseClass::parse_str(&data_source_record.license_class)?,
                license_name: data_source_record.license_name,
                license_url: data_source_record.license_url,
                attribution_text: data_source_record.attribution_text,
                preference_rank: data_source_record.preference_rank,
                created: data_source_record.created,
                modified: data_source_record.modified,
            })
        })
        .transpose()
}
