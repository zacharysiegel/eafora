use uuid::Uuid;

use shared::canonical::canonical_model::{DataStatus, NaiveDatePeriod};

#[derive(Debug, Clone, Copy)]
pub struct AdapterOptions {
    pub force_full_refetch: bool,
}

#[derive(Debug)]
pub struct NormalizedStatisticValue {
    pub region_id: Uuid,
    pub statistic_id: Uuid,
    pub period: NaiveDatePeriod,
    pub value: f64,
    pub data_status: DataStatus,
}

#[derive(Debug)]
pub enum NormalizeOutcome {
    Normalized(NormalizedStatisticValue),
    Warned(IngestWarning),
}

#[derive(Debug, Clone)]
pub struct IngestWarning {
    pub kind: IngestWarningKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestWarningKind {
    UnrecognizedRegionCode,
    NotApplicableValue,
    UnparsableRow,
    NoValuesForRegion,
}
