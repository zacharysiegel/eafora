use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::adapter::NormalizedStatisticValue;
use crate::canonical::canonical_model::StatisticValue;
use crate::error::AppError;
use crate::ingest::ingest_db;
use crate::ingest::{IngestReport, RecordOutcome};

/// For each `(region, statistic, period, data_source)` cell, the existing
/// `superseded is null` row is compared against the new value:
///
/// - no current row: INSERT the new row, count `values_added`.
/// - current row matches new value + status: skip, count `values_skipped`.
/// - current row differs: stamp the old row's `superseded`, INSERT a new
///   row pointing at the new publication, count `values_revised`.
pub async fn record_statistic_values(
    connection: &mut PgConnection,
    data_source_id: Uuid,
    publication_revision_label: &str,
    publication_fetched: DateTime<Utc>,
    normalized_statistic_values: Vec<NormalizedStatisticValue>,
) -> Result<IngestReport, AppError> {
    let publication_id: Uuid = ingest_db::insert_publication_or_match(
        &mut *connection,
        data_source_id,
        publication_revision_label,
        publication_fetched,
    )
    .await?;
    let mut report: IngestReport = IngestReport::default();

    for normalized_statistic_value in normalized_statistic_values {
        let outcome: RecordOutcome =
            record_statistic_value(&mut *connection, data_source_id, publication_id, &normalized_statistic_value).await?;
        match outcome {
            RecordOutcome::Added => report.values_added += 1,
            RecordOutcome::Revised => report.values_revised += 1,
            RecordOutcome::Skipped => report.values_skipped += 1,
        }
    }

    Ok(report)
}

pub async fn record_statistic_value(
    connection: &mut PgConnection,
    data_source_id: Uuid,
    publication_id: Uuid,
    normalized_statistic_value: &NormalizedStatisticValue,
) -> Result<RecordOutcome, AppError> {
    let current: Option<StatisticValue> =
        ingest_db::find_current_value(&mut *connection, normalized_statistic_value, data_source_id).await?;

    if let Some(current_record) = current {
        if current_record.value == normalized_statistic_value.value
            && current_record.data_status == normalized_statistic_value.data_status.as_str()
        {
            return Ok(RecordOutcome::Skipped);
        }
        ingest_db::set_superseded(&mut *connection, current_record.id, Utc::now()).await?;
        ingest_db::insert_statistic_value(
            &mut *connection,
            normalized_statistic_value,
            data_source_id,
            publication_id,
        )
        .await?;
        return Ok(RecordOutcome::Revised);
    }

    ingest_db::insert_statistic_value(
        &mut *connection,
        normalized_statistic_value,
        data_source_id,
        publication_id,
    )
    .await?;
    Ok(RecordOutcome::Added)
}
