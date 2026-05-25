//! Cross-adapter types: knobs every adapter accepts, the per-run report
//! shape they return, the canonical normalized-row form they emit before
//! the persistence layer takes over. Per-source intermediate types
//! (response shapes, parser outputs) live in `<source>_model.rs`.

#[derive(Debug, Clone, Copy)]
pub struct AdapterOptions {
    pub force_full_refetch: bool,
}

#[derive(Debug, Default)]
pub struct IngestReport {
    pub values_added: u64,
    pub values_revised: u64,
    pub values_skipped: u64,
    pub warnings: Vec<IngestWarning>,
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

#[derive(Debug)]
pub struct NormalizedRow {
    pub region_id: uuid::Uuid,
    pub statistic_id: uuid::Uuid,
    pub period_start: chrono::NaiveDate,
    pub period_end: chrono::NaiveDate,
    pub value: f64,
    pub data_status: String,
}
