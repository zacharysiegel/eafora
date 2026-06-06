use std::collections::BTreeMap;

use sqlx::PgExecutor;

use crate::artifact::artifact_model::{
    ArtifactVersion, ArtifactVersionEntity, CandidateValue, CandidateValueProjection, CountryNameProjection,
};
use crate::canonical::canonical_model::{DataSourceKind, SourceRevision, StatisticKind};
use crate::error::AppError;

pub async fn read_candidate_values_for_statistic<'e>(
    executor: impl PgExecutor<'e>,
    statistic_kind: StatisticKind,
) -> Result<Vec<CandidateValue>, AppError> {
    let projections: Vec<CandidateValueProjection> = sqlx::query_as!(
        CandidateValueProjection,
        r#"
        select
            statistic_value.region_id              as "region_id!",
            country.iso3                           as "region_iso3!",
            statistic.code                         as "statistic_code!",
            statistic_value.period_start           as "period_start!",
            statistic_value.period_end             as "period_end!",
            statistic_value.value                  as "value!",
            statistic_value.data_status            as "data_status!",
            data_source.code                       as "data_source_code!",
            data_source_publication.revision_label as "data_source_revision!",
            data_source.license_class              as "license_class!"
        from statistic_value
        join country on country.region_id = statistic_value.region_id
        join statistic on statistic.id = statistic_value.statistic_id
        join data_source on data_source.id = statistic_value.data_source_id
        join data_source_publication on data_source_publication.id = statistic_value.data_source_publication_id
        where statistic_value.superseded is null
          and statistic.code = $1
        "#,
        statistic_kind.code(),
    )
    .fetch_all(executor)
    .await?;

    projections.into_iter().map(CandidateValue::try_from).collect()
}

pub async fn read_country_iso3_to_name_en<'e>(
    executor: impl PgExecutor<'e>,
) -> Result<BTreeMap<String, String>, AppError> {
    let projections: Vec<CountryNameProjection> = sqlx::query_as!(
        CountryNameProjection,
        r#"
        select country.iso3, region.name_en
        from country
        join region on region.id = country.region_id
        where country.deleted is null
        "#,
    )
    .fetch_all(executor)
    .await?;

    Ok(projections.into_iter().map(|projection| (projection.iso3, projection.name_en)).collect())
}

pub async fn read_all_statistic_kinds<'e>(
    executor: impl PgExecutor<'e>,
) -> Result<Vec<StatisticKind>, AppError> {
    let codes: Vec<String> = sqlx::query_scalar!("select code from statistic")
        .fetch_all(executor)
        .await?;
    codes.iter().map(|code| StatisticKind::try_from(code.as_str())).collect()
}

pub async fn insert_artifact_version<'e>(
    executor: impl PgExecutor<'e>,
    version_label: &str,
    manifest_sha256: &str,
    manifest_url: &str,
    data_source_revisions: &BTreeMap<DataSourceKind, SourceRevision>,
) -> Result<ArtifactVersion, AppError> {
    let data_source_revisions: BTreeMap<&str, &SourceRevision> = data_source_revisions
        .iter()
        .map(|(kind, revision)| (kind.code(), revision))
        .collect();
    let data_source_revisions: serde_json::Value = serde_json::to_value(data_source_revisions)?;

    let artifact_version_entity: ArtifactVersionEntity = sqlx::query_as!(
        ArtifactVersionEntity,
        r#"
        insert into artifact_version
            (version_label, manifest_sha256, manifest_url, data_source_revisions_jsonb)
        values ($1, $2, $3, $4)
        on conflict (version_label) do nothing
        returning
            id                          as "id!",
            version_label               as "version_label!",
            artifact_created            as "artifact_created!",
            manifest_sha256             as "manifest_sha256!",
            manifest_url                as "manifest_url!",
            data_source_revisions_jsonb as "data_source_revisions_jsonb!",
            notes
        "#,
        version_label,
        manifest_sha256,
        manifest_url,
        data_source_revisions,
    )
    .fetch_one(executor)
    .await?;

    Ok(ArtifactVersion::from(artifact_version_entity))
}
