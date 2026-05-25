//! Cross-adapter types: knobs every adapter accepts, the canonical
//! normalized-row form they emit, the per-row outcome `normalize` produces,
//! and the warnings adapters attach to rows they couldn't normalize.
//! Per-source intermediate types (response shapes, parser outputs) live in
//! `<source>_model.rs`. Aggregate ingest-layer types (IngestReport,
//! UpsertOutcome) live in `ingest::ingest_model`.

use uuid::Uuid;
use chrono::NaiveDate;

#[derive(Debug, Clone, Copy)]
pub struct AdapterOptions {
    pub force_full_refetch: bool,
}

#[derive(Debug)]
pub struct NormalizedRow {
    pub region_id: Uuid,
    pub statistic_id: Uuid,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub value: f64,
    pub data_status: String,
}

/// Per-row result of an adapter's normalize step. Every adapter accumulates
/// these into `(Vec<NormalizedRow>, Vec<IngestWarning>)` for the ingest
/// layer to consume.
#[derive(Debug)]
pub enum NormalizeOutcome {
    Normalized(NormalizedRow),
    Warned(IngestWarning),
}

#[derive(Debug, Clone)]
pub struct IngestWarning {
    pub kind: IngestWarningKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestWarningKind {
    UnknownCountry,
    NaValue,
    UnparsableRow,
}
