//! Types produced by the ingest layer. `IngestReport` aggregates per-row
//! outcomes across a batch; `UpsertOutcome` is the per-row classification
//! that `upsert_statistic_value` returns and `upsert_statistic_values`
//! tallies. `IngestWarning` / `IngestWarningKind` (which
//! `IngestReport.warnings` carries) live in `adapter::adapter_model` —
//! they're produced by the adapter's normalize step and ingest just
//! transports them.

use crate::adapter::IngestWarning;

#[derive(Debug, Default)]
pub struct IngestReport {
    pub values_added: u64,
    pub values_revised: u64,
    pub values_skipped: u64,
    pub warnings: Vec<IngestWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    Added,
    Revised,
    Skipped,
}
