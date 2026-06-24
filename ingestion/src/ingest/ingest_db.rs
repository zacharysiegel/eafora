use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::adapter::NormalizedStatisticValue;
use shared::canonical::canonical_model::SourceRevision;

use crate::canonical::canonical_entity::{StatisticValue, StatisticValueEntity};
use crate::error::AppError;

/// Latest publication = most recently *published* one. `published` is the
/// source's own publication timestamp; sources without a derivable one
/// store null and rank last. `fetched` is only the tiebreaker (or the sole
/// criterion when every row is null-published), never an override of a
/// successfully-parsed `published`.
pub async fn read_latest_publication<'e>(
    executor: impl PgExecutor<'e>,
    data_source_id: Uuid,
) -> Result<Option<SourceRevision>, AppError> {
    struct LatestPublicationProjection {
        revision_label: String,
        published: Option<DateTime<Utc>>,
        fetched: DateTime<Utc>,
    }

    let projection: Option<LatestPublicationProjection> = sqlx::query_as!(
        LatestPublicationProjection,
        r#"
        select revision_label as "revision_label!", published, fetched as "fetched!"
        from data_source_publication
        where data_source_id = $1
        order by published desc nulls last, fetched desc
        limit 1
        "#,
        data_source_id,
    )
    .fetch_optional(executor)
    .await?;

    Ok(projection.map(|projection| SourceRevision {
        revision: projection.revision_label,
        published: projection.published,
        fetched: projection.fetched,
    }))
}

pub async fn insert_publication_or_match<'e>(
    executor: impl PgExecutor<'e>,
    data_source_id: Uuid,
    revision_label: &str,
    published: Option<DateTime<Utc>>,
    fetched: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    let publication_id: Uuid = sqlx::query_scalar!(
        r#"
        insert into data_source_publication (data_source_id, revision_label, published, fetched)
        values ($1, $2, $3, $4)
        on conflict (data_source_id, revision_label) do update
            set revision_label = excluded.revision_label
        returning id
        "#,
        data_source_id,
        revision_label,
        published,
        fetched,
    )
    .fetch_one(executor)
    .await?;
    Ok(publication_id)
}

pub async fn find_current_value<'e>(
    executor: impl PgExecutor<'e>,
    normalized_statistic_value: &NormalizedStatisticValue,
    data_source_id: Uuid,
) -> Result<Option<StatisticValue>, AppError> {
    let record: Option<StatisticValueEntity> = sqlx::query_as!(
        StatisticValueEntity,
        r#"
        select id, region_id, statistic_id, period_start, period_end, value, data_source_id, data_source_publication_id, data_status, superseded, created, modified
        from statistic_value
        where region_id = $1
          and statistic_id = $2
          and period_start = $3
          and period_end = $4
          and data_source_id = $5
          and superseded is null
        "#,
        normalized_statistic_value.region_id,
        normalized_statistic_value.statistic_id,
        normalized_statistic_value.period.start,
        normalized_statistic_value.period.end,
        data_source_id,
    )
    .fetch_optional(executor)
    .await?;

    record.map(StatisticValue::try_from).transpose()
}

pub async fn insert_statistic_value<'e>(
    executor: impl PgExecutor<'e>,
    normalized_statistic_value: &NormalizedStatisticValue,
    data_source_id: Uuid,
    data_source_publication_id: Uuid,
) -> Result<Uuid, AppError> {
    let inserted_id: Uuid = sqlx::query_scalar!(
        r#"
        insert into statistic_value (region_id, statistic_id, period_start, period_end, value, data_source_id, data_source_publication_id, data_status)
        values ($1, $2, $3, $4, $5, $6, $7, $8)
        returning id
        "#,
        normalized_statistic_value.region_id,
        normalized_statistic_value.statistic_id,
        normalized_statistic_value.period.start,
        normalized_statistic_value.period.end,
        normalized_statistic_value.value,
        data_source_id,
        data_source_publication_id,
        normalized_statistic_value.data_status.as_str(),
    )
    .fetch_one(executor)
    .await?;
    Ok(inserted_id)
}

pub async fn set_superseded<'e>(
    executor: impl PgExecutor<'e>,
    statistic_value_id: Uuid,
    superseded_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query!(
        r#"
        update statistic_value
        set superseded = $2,
            modified = now()
        where id = $1
        "#,
        statistic_value_id,
        superseded_at,
    )
    .execute(executor)
    .await?;
    Ok(())
}
