//! Types for the WB WDI HTTP response shape and the per-pipeline-stage
//! intermediate representations. The HTTP response is a heterogeneous JSON
//! array `[paging_metadata_object, [row_objects...]]` — modeled here as a
//! tuple via serde so we can deserialize without a custom visitor.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct WdiResponse(pub WdiPagingMetadata, pub Vec<WdiRow>);

#[derive(Debug, Deserialize)]
pub struct WdiPagingMetadata {
    pub page: u32,
    pub pages: u32,
    pub per_page: u32,
    pub total: u32,
    pub sourceid: String,
    pub lastupdated: String,
}

#[derive(Debug, Deserialize)]
pub struct WdiRow {
    pub indicator: WdiIndicator,
    pub country: WdiCountry,
    pub countryiso3code: String,
    pub date: String,
    pub value: Option<f64>,
    pub unit: String,
    pub obs_status: String,
    pub decimal: i32,
}

#[derive(Debug, Deserialize)]
pub struct WdiIndicator {
    pub id: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct WdiCountry {
    pub id: String,
    pub value: String,
}

#[derive(Debug)]
pub struct ParsedRow {
    pub iso3: String,
    pub year: i32,
    pub value: Option<f64>,
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
