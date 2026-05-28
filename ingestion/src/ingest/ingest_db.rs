use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::adapter::NormalizedStatisticValue;
use crate::canonical::canonical_model::StatisticValue;
use crate::error::AppError;

pub async fn read_latest_publication_revision<'e>(
    executor: impl PgExecutor<'e>,
    data_source_id: Uuid,
) -> Result<Option<String>, AppError> {
    let revision_label: Option<String> = sqlx::query_scalar!(
        r#"
        select revision_label
        from data_source_publication
        where data_source_id = $1
        order by fetched desc
        limit 1
        "#,
        data_source_id,
    )
    .fetch_optional(executor)
    .await?;
    Ok(revision_label)
}

pub async fn insert_publication_or_match<'e>(
    executor: impl PgExecutor<'e>,
    data_source_id: Uuid,
    revision_label: &str,
    fetched: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    let publication_id: Uuid = sqlx::query_scalar!(
        r#"
        insert into data_source_publication (data_source_id, revision_label, fetched)
        values ($1, $2, $3)
        on conflict (data_source_id, revision_label) do update
            set revision_label = excluded.revision_label
        returning id
        "#,
        data_source_id,
        revision_label,
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
    let current: Option<StatisticValue> = sqlx::query_as!(
        StatisticValue,
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
    Ok(current)
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
