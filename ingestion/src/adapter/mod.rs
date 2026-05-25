pub mod adapter;
pub mod adapter_model;

pub use adapter::year_to_period;
pub use adapter_model::{
    AdapterOptions,
    IngestWarning,
    IngestWarningKind,
    NormalizeOutcome,
    NormalizedRow,
};
