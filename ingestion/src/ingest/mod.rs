//! Ingest layer: takes `NormalizedRow` batches from any source adapter and
//! persists them to the canonical store with append-with-supersede
//! semantics. Source-agnostic — every adapter calls `upsert_rows` with
//! the same signature.

pub mod ingest_db;
pub mod ingest_model;

pub use ingest_model::{IngestReport, UpsertOutcome};

use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::adapter::NormalizedRow;
use crate::error::AppError;

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
pub async fn upsert_rows(
    connection: &mut PgConnection,
    data_source_id: Uuid,
    publication_revision_label: &str,
    publication_fetched: DateTime<Utc>,
    normalized_rows: Vec<NormalizedRow>,
) -> Result<IngestReport, AppError> {
    let publication_id: Uuid = ingest_db::insert_publication_or_match(
        &mut *connection,
        data_source_id,
        publication_revision_label,
        publication_fetched,
    )
    .await?;
    let mut report: IngestReport = IngestReport::default();
    for normalized_row in normalized_rows {
        let outcome: UpsertOutcome =
            upsert_row(&mut *connection, data_source_id, publication_id, &normalized_row).await?;
        match outcome {
            UpsertOutcome::Added => report.values_added += 1,
            UpsertOutcome::Revised => report.values_revised += 1,
            UpsertOutcome::Skipped => report.values_skipped += 1,
        }
    }
    Ok(report)
}

pub async fn upsert_row(
    connection: &mut PgConnection,
    data_source_id: Uuid,
    publication_id: Uuid,
    normalized_row: &NormalizedRow,
) -> Result<UpsertOutcome, AppError> {
    let current = ingest_db::find_current_value(
        &mut *connection,
        normalized_row.region_id,
        normalized_row.statistic_id,
        normalized_row.period_start,
        normalized_row.period_end,
        data_source_id,
    )
    .await?;
    if let Some(current_row) = current {
        if current_row.value == normalized_row.value
            && current_row.data_status == normalized_row.data_status
        {
            return Ok(UpsertOutcome::Skipped);
        }
        ingest_db::set_superseded(&mut *connection, current_row.id, Utc::now()).await?;
        ingest_db::insert_statistic_value(
            &mut *connection,
            normalized_row.region_id,
            normalized_row.statistic_id,
            normalized_row.period_start,
            normalized_row.period_end,
            normalized_row.value,
            data_source_id,
            publication_id,
            &normalized_row.data_status,
        )
        .await?;
        return Ok(UpsertOutcome::Revised);
    }
    ingest_db::insert_statistic_value(
        &mut *connection,
        normalized_row.region_id,
        normalized_row.statistic_id,
        normalized_row.period_start,
        normalized_row.period_end,
        normalized_row.value,
        data_source_id,
        publication_id,
        &normalized_row.data_status,
    )
    .await?;
    Ok(UpsertOutcome::Added)
}
