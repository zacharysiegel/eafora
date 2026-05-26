//! sqlx queries that bridge the canonical store and the artifact pipeline.
//! `read_candidate_values` drives the build; `insert_artifact_version` lands
//! after a successful publish. Both take `impl PgExecutor<'_>` so callers
//! pass either `&PgPool` or `&mut *tx`.

use std::collections::BTreeMap;

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;
use crate::artifact::artifact_model::CandidateValue;
use crate::error::AppError;

struct CandidateRow {
    region_id: Uuid,
    region_iso3: String,
    statistic_id: Uuid,
    statistic_code: String,
    period_start: chrono::NaiveDate,
    period_end: chrono::NaiveDate,
    value: f64,
    data_status: String,
    data_source_id: Uuid,
    data_source_code: String,
    data_source_revision: String,
    data_source_preference_rank: i32,
    license_class: String,
}

pub async fn read_candidate_values<'e>(
    executor: impl PgExecutor<'e>,
) -> Result<Vec<CandidateValue>, AppError> {
    let rows: Vec<CandidateRow> = sqlx::query_as!(
        CandidateRow,
        r#"
        select
            statistic_value.region_id            as "region_id!",
            country.iso3                         as "region_iso3!",
            statistic_value.statistic_id         as "statistic_id!",
            statistic.code                       as "statistic_code!",
            statistic_value.period_start         as "period_start!",
            statistic_value.period_end           as "period_end!",
            statistic_value.value                as "value!",
            statistic_value.data_status          as "data_status!",
            statistic_value.data_source_id       as "data_source_id!",
            data_source.code                     as "data_source_code!",
            data_source_publication.revision_label as "data_source_revision!",
            data_source.preference_rank          as "data_source_preference_rank!",
            data_source.license_class            as "license_class!"
        from statistic_value
        join country on country.region_id = statistic_value.region_id
        join statistic on statistic.id = statistic_value.statistic_id
        join data_source on data_source.id = statistic_value.data_source_id
        join data_source_publication on data_source_publication.id = statistic_value.data_source_publication_id
        where statistic_value.superseded is null
        "#,
    )
    .fetch_all(executor)
    .await?;

    let candidate_values: Vec<CandidateValue> = rows
        .into_iter()
        .map(|row| CandidateValue {
            region_id: row.region_id,
            region_iso3: row.region_iso3,
            statistic_id: row.statistic_id,
            statistic_code: row.statistic_code,
            period: NaiveDatePeriod {
                start: row.period_start,
                end: row.period_end,
            },
            value: row.value,
            data_status: row.data_status,
            data_source_id: row.data_source_id,
            data_source_code: row.data_source_code,
            data_source_revision: row.data_source_revision,
            data_source_preference_rank: row.data_source_preference_rank,
            license_class: row.license_class,
        })
        .collect();

    Ok(candidate_values)
}

pub async fn read_country_iso3_to_name_en<'e>(
    executor: impl PgExecutor<'e>,
) -> Result<BTreeMap<String, String>, AppError> {
    struct CountryNameRow {
        iso3: String,
        name_en: String,
    }

    let rows: Vec<CountryNameRow> = sqlx::query_as!(
        CountryNameRow,
        r#"
        select country.iso3 as "iso3!", region.name_en as "name_en!"
        from country
        join region on region.id = country.region_id
        where country.deleted is null
        "#,
    )
    .fetch_all(executor)
    .await?;

    let iso3_to_name_en: BTreeMap<String, String> = rows
        .into_iter()
        .map(|country_name_row| (country_name_row.iso3, country_name_row.name_en))
        .collect();

    Ok(iso3_to_name_en)
}
