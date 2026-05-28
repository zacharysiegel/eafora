use std::collections::BTreeMap;

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::adapter::adapter_model::NaiveDatePeriod;
use crate::artifact::artifact_model::{ArtifactVersion, CandidateValue};
use crate::canonical::canonical_model::{DataSourceCode, DataStatus, LicenseClass};
use crate::error::AppError;

struct CandidateRecord {
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
    let candidate_records: Vec<CandidateRecord> = sqlx::query_as!(
        CandidateRecord,
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

    candidate_records
        .into_iter()
        .map(|candidate_record| {
            Ok(CandidateValue {
                region_id: candidate_record.region_id,
                region_iso3: candidate_record.region_iso3,
                statistic_id: candidate_record.statistic_id,
                statistic_code: candidate_record.statistic_code,
                period: NaiveDatePeriod {
                    start: candidate_record.period_start,
                    end: candidate_record.period_end,
                },
                value: candidate_record.value,
                data_status: DataStatus::parse_str(&candidate_record.data_status)?,
                data_source_id: candidate_record.data_source_id,
                data_source_code: DataSourceCode::parse_str(&candidate_record.data_source_code)?,
                data_source_revision: candidate_record.data_source_revision,
                data_source_preference_rank: candidate_record.data_source_preference_rank,
                license_class: LicenseClass::parse_str(&candidate_record.license_class)?,
            })
        })
        .collect()
}

pub async fn read_country_iso3_to_name_en<'e>(
    executor: impl PgExecutor<'e>,
) -> Result<BTreeMap<String, String>, AppError> {
    struct CountryNameRecord {
        iso3: String,
        name_en: String,
    }

    let country_name_records: Vec<CountryNameRecord> = sqlx::query_as!(
        CountryNameRecord,
        r#"
        select country.iso3 as "iso3!", region.name_en as "name_en!"
        from country
        join region on region.id = country.region_id
        where country.deleted is null
        "#,
    )
    .fetch_all(executor)
    .await?;

    let iso3_to_name_en: BTreeMap<String, String> = country_name_records
        .into_iter()
        .map(|country_name_record| (country_name_record.iso3, country_name_record.name_en))
        .collect();

    Ok(iso3_to_name_en)
}

pub async fn read_all_statistic_codes<'e>(
    executor: impl PgExecutor<'e>,
) -> Result<Vec<String>, AppError> {
    struct StatisticCodeRecord {
        code: String,
    }

    let statistic_code_records: Vec<StatisticCodeRecord> = sqlx::query_as!(
        StatisticCodeRecord,
        r#"select code as "code!" from statistic"#,
    )
    .fetch_all(executor)
    .await?;

    Ok(statistic_code_records
        .into_iter()
        .map(|statistic_code_record| statistic_code_record.code)
        .collect())
}

pub async fn insert_artifact_version<'e>(
    executor: impl PgExecutor<'e>,
    version_label: &str,
    manifest_sha256: &str,
    manifest_url: &str,
    data_source_versions: &BTreeMap<String, String>,
) -> Result<ArtifactVersion, AppError> {
    let data_source_versions_json: serde_json::Value = serde_json::to_value(data_source_versions)?;

    let artifact_version_record: ArtifactVersion = sqlx::query_as!(
        ArtifactVersion,
        r#"
        insert into artifact_version
            (version_label, manifest_sha256, manifest_url, data_source_versions_jsonb)
        values ($1, $2, $3, $4)
        on conflict (version_label) do nothing
        returning
            id                         as "id!",
            version_label              as "version_label!",
            artifact_created           as "artifact_created!",
            manifest_sha256            as "manifest_sha256!",
            manifest_url               as "manifest_url!",
            data_source_versions_jsonb as "data_source_versions_jsonb!",
            notes
        "#,
        version_label,
        manifest_sha256,
        manifest_url,
        data_source_versions_json,
    )
    .fetch_one(executor)
    .await?;

    Ok(artifact_version_record)
}
