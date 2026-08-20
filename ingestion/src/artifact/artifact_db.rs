use std::collections::{BTreeMap, BTreeSet};

use sqlx::{PgConnection, PgExecutor};

use crate::artifact::artifact_model::{
    ArtifactVersion, ArtifactVersionEntity, CandidateValue, CandidateValueProjection, CountryMetadataProjection,
};
use crate::canonical::canonical_db;
use shared::canonical::canonical_model::{DataSource, DataSourceKind, SourceRevision, StatisticKind};
use crate::error::AppError;
use crate::ingest::ingest_db;

pub async fn read_candidate_values_for_statistic<'e>(
    executor: impl PgExecutor<'e>,
    statistic_kind: StatisticKind,
) -> Result<Vec<CandidateValue>, AppError> {
    let projections: Vec<CandidateValueProjection> = sqlx::query_as!(
        CandidateValueProjection,
        r#"
        select
            statistic_value.region_id              as "region_id!",
            region.code                            as "region_code!",
            statistic.code                         as "statistic_code!",
            statistic_value.period_start           as "period_start!",
            statistic_value.period_end             as "period_end!",
            statistic_value.value                  as "value!",
            statistic_value.data_status            as "data_status!",
            data_source.code                       as "data_source_code!",
            data_source_publication.revision_label as "data_source_revision!",
            data_source.license_class              as "license_class!"
        from statistic_value
        join region on region.id = statistic_value.region_id
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

    projections.into_iter()
        .map(CandidateValue::try_from)
        .collect()
}

pub async fn read_country_iso3_to_metadata<'e>(
    executor: impl PgExecutor<'e>,
) -> Result<BTreeMap<String, CountryMetadataProjection>, AppError> {
    let projections: Vec<CountryMetadataProjection> = sqlx::query_as!(
        CountryMetadataProjection,
        r#"
        select country.iso3, region.name_en, region.code as region_code
        from country
        join region on region.id = country.region_id
        "#,
    )
    .fetch_all(executor)
    .await?;

    let map: BTreeMap<String, CountryMetadataProjection> = projections.into_iter()
        .map(|projection| (projection.iso3.clone(), projection))
        .collect();
    Ok(map)
}

/// An unreleased statistic is ingested but not yet offered to clients, so it is skipped here rather than
/// failing the build for having no [`StatisticKind`] variant. A released one with no variant is a
/// misconfiguration and stays a hard error.
pub async fn read_all_statistic_kinds<'e>(
    executor: impl PgExecutor<'e>,
) -> Result<BTreeSet<StatisticKind>, AppError> {
    let codes: Vec<String> = sqlx::query_scalar!("select code from statistic where released is not null")
        .fetch_all(executor)
        .await?;

    codes
        .iter()
        .map(|code| StatisticKind::try_from(code.as_str()).map_err(AppError::from))
        .collect()
}

pub async fn read_latest_revisions(
    connection: &mut PgConnection,
    data_source_kinds: &BTreeSet<DataSourceKind>,
) -> Result<BTreeMap<DataSourceKind, SourceRevision>, AppError> {
    let mut revisions: BTreeMap<DataSourceKind, SourceRevision> = BTreeMap::new();
    for kind in data_source_kinds {
        let data_source: DataSource = canonical_db::find_data_source_by_kind(&mut *connection, *kind)
            .await?
            .ok_or_else(|| AppError::from(format!("data_source {:?} missing from canonical store", kind)))?;
        let revision: SourceRevision = ingest_db::read_latest_publication(&mut *connection, data_source.id)
            .await?
            .ok_or_else(|| AppError::from(format!("no publication recorded for {:?}", kind)))?;
        revisions.insert(*kind, revision);
    }

    Ok(revisions)
}

pub async fn read_artifact_version_exists<'e>(
    executor: impl PgExecutor<'e>,
    version_label: &str,
) -> Result<bool, AppError> {
    let exists: bool = sqlx::query_scalar!(
        r#"select exists(select 1 from artifact_version where version_label = $1) as "exists!""#,
        version_label,
    )
    .fetch_one(executor)
    .await?;
    Ok(exists)
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
