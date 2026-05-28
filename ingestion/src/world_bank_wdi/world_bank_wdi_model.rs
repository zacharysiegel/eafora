use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct WdiResponse(pub WdiPagingMetadata, pub Vec<WdiStatisticValue>);

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
pub struct WdiStatisticValue {
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
pub struct ParsedWdiStatisticValue {
    pub iso3: String,
    pub year: i32,
    pub value: Option<f64>,
}
