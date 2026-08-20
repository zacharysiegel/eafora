use crate::adapter::IngestWarning;

#[derive(Debug, Default)]
pub struct IngestReport {
    pub values_added: u64,
    pub values_revised: u64,
    pub values_skipped: u64,
    /// Cells the source publishes with no value. Counted rather than warned: for some sources this is the
    /// normal state of a large minority of rows, and one warning each would bury the rest.
    pub values_absent_upstream: u64,
    pub warnings: Vec<IngestWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    Added,
    Revised,
    Skipped,
}
