//! Ingest layer: takes `NormalizedStatisticValue` batches from any source adapter and
//! persists them to the canonical store with append-with-supersede
//! semantics. Source-agnostic — every adapter calls `record_statistic_values` with
//! the same signature.

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::adapter::NormalizedStatisticValue;
use crate::canonical::canonical_model::StatisticValue;
use crate::error::AppError;
use crate::ingest::ingest_db;
use crate::ingest::{IngestReport, RecordOutcome};

/// Persists a batch of normalized rows under a single publication. The
/// publication is INSERTed (or matched against an existing row with the same
/// `(data_source_id, revision_label)`) before any value writes; every
/// inserted statistic_value row points at the resulting publication id.
///
/// For each normalized row we look up the current `superseded is null` row
/// for `(region_id, statistic_id, period_start, period_end, data_source_id)`:
/// - no current row: INSERT the new row, count `values_added`
/// - current row matches new value + status: skip, count `values_skipped`
/// - current row differs: set the old row's `superseded = now()`, INSERT a
///   new row pointing at the new publication, count `values_revised`
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
    if let Some(current_row) = current {
        if current_row.value == normalized_statistic_value.value
            && current_row.data_status == normalized_statistic_value.data_status
        {
            return Ok(RecordOutcome::Skipped);
        }
        ingest_db::set_superseded(&mut *connection, current_row.id, Utc::now()).await?;
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
