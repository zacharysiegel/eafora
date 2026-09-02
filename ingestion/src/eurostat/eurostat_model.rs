use std::collections::BTreeMap;

use serde::Deserialize;

/// A JSON-stat 2.0 dataset. Observations are addressed by a single flat index into `value`, which the
/// dimension sizes in `size` decompose; `id` gives the dimension order those sizes are in.
#[derive(Debug, Deserialize)]
pub struct EurostatResponse {
    pub updated: String,
    pub value: BTreeMap<usize, f64>,
    #[serde(default)]
    pub status: BTreeMap<usize, String>,
    pub id: Vec<String>,
    pub size: Vec<usize>,
    pub dimension: BTreeMap<String, EurostatDimension>,
}

#[derive(Debug, Deserialize)]
pub struct EurostatDimension {
    pub category: EurostatCategory,
}

#[derive(Debug, Deserialize)]
pub struct EurostatCategory {
    pub index: BTreeMap<String, usize>,
    #[serde(default)]
    pub label: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEurostatPublication {
    pub revision_label: String,
}

#[derive(Debug)]
pub struct ParsedEurostatResponse {
    pub publication: ParsedEurostatPublication,
    /// The revision of the classification each geo label names, absent for a code no revision has recut.
    pub revision_by_geo_code: BTreeMap<String, i32>,
    pub observations: Vec<ParsedEurostatObservation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEurostatObservation {
    pub indicator_code: String,
    pub geo_code: String,
    pub period_year: i32,
    pub value: f64,
    /// Eurostat's `OBS_FLAG`, a run of single-character codes such as `ep`. Absent where the observation
    /// carries no status.
    pub flag: Option<String>,
}
