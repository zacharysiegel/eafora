use crate::adapter::IngestWarning;

#[derive(Debug, Default)]
pub struct IngestReport {
    pub values_added: u64,
    pub values_revised: u64,
    pub values_skipped: u64,
    pub warnings: Vec<IngestWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    Added,
    Revised,
    Skipped,
}
