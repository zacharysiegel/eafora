use std::collections::{BTreeMap, HashSet};

use crate::error::AppError;
use crate::eurostat::eurostat_model::{
    EurostatDimension, EurostatResponse, ParsedEurostatObservation, ParsedEurostatPublication,
    ParsedEurostatResponse,
};
use crate::http;

const API_BASE_URL: &str = "https://ec.europa.eu/eurostat/api/dissemination/statistics/1.0/data";

/// The indicators this adapter ingests, one dimension member each.
pub const INDICATOR_TOTAL_FERTILITY_RATE: &str = "TOTFERRT";
pub const INDICATOR_MEAN_AGE_AT_CHILDBIRTH: &str = "AGEMOTH";
pub const INDICATOR_MEAN_AGE_AT_FIRST_BIRTH: &str = "AGEMOTH1";

const DATASET_COUNTRY: &str = "demo_find";
const DATASET_NUTS_1_AND_2: &str = "demo_r_find2";
const DATASET_NUTS_3: &str = "demo_r_find3";

/// Eurostat splits fertility across datasets by the level of the regions it reports, so one run reads
/// several. Mean age at first birth is published at country level only.
pub const EXTRACTIONS: [EurostatExtraction; 4] = [
    EurostatExtraction {
        dataset: DATASET_COUNTRY,
        geo_level: EurostatGeoLevel::Country,
        indicator_codes: &[
            INDICATOR_TOTAL_FERTILITY_RATE,
            INDICATOR_MEAN_AGE_AT_CHILDBIRTH,
            INDICATOR_MEAN_AGE_AT_FIRST_BIRTH,
        ],
    },
    EurostatExtraction {
        dataset: DATASET_NUTS_1_AND_2,
        geo_level: EurostatGeoLevel::Nuts1,
        indicator_codes: &[INDICATOR_TOTAL_FERTILITY_RATE, INDICATOR_MEAN_AGE_AT_CHILDBIRTH],
    },
    EurostatExtraction {
        dataset: DATASET_NUTS_1_AND_2,
        geo_level: EurostatGeoLevel::Nuts2,
        indicator_codes: &[INDICATOR_TOTAL_FERTILITY_RATE, INDICATOR_MEAN_AGE_AT_CHILDBIRTH],
    },
    EurostatExtraction {
        dataset: DATASET_NUTS_3,
        geo_level: EurostatGeoLevel::Nuts3,
        indicator_codes: &[INDICATOR_TOTAL_FERTILITY_RATE, INDICATOR_MEAN_AGE_AT_CHILDBIRTH],
    },
];

const DIMENSION_INDICATOR: &str = "indic_de";
const DIMENSION_GEO: &str = "geo";
const DIMENSION_TIME: &str = "time";

/// Eurostat's own name for a revision of the classification, as it appears in a geo label.
const REVISION_MARKER_PREFIXES: [&str; 2] = ["NUTS ", "statistical region "];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EurostatGeoLevel {
    Country,
    Nuts1,
    Nuts2,
    Nuts3,
}

impl EurostatGeoLevel {
    pub const fn code(self) -> &'static str {
        match self {
            EurostatGeoLevel::Country => "country",
            EurostatGeoLevel::Nuts1 => "nuts1",
            EurostatGeoLevel::Nuts2 => "nuts2",
            EurostatGeoLevel::Nuts3 => "nuts3",
        }
    }
}

pub struct EurostatExtraction {
    pub dataset: &'static str,
    pub geo_level: EurostatGeoLevel,
    pub indicator_codes: &'static [&'static str],
}

/// Above 500,000 cells Eurostat answers asynchronously and above 5,000,000 it refuses; the largest extraction
/// here is approx. 77,520, so the synchronous path is the only one this adapter needs.
pub async fn fetch_upstream(extraction: &EurostatExtraction) -> Result<String, AppError> {
    let indicator_parameters: String = extraction.indicator_codes
        .iter()
        .map(|indicator_code| format!("&{DIMENSION_INDICATOR}={indicator_code}"))
        .collect();
    let url: String = format!(
        "{API_BASE_URL}/{}?format=JSON&lang=EN&geoLevel={}{indicator_parameters}",
        extraction.dataset,
        extraction.geo_level.code(),
    );

    let response: reqwest::Response = http::HTTP_CLIENT
        .get(&url)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(AppError::from(format!(
            "eurostat request failed; [status={} url={}]",
            response.status(),
            url,
        )));
    }

    Ok(response.text().await?)
}

/// Eurostat answers an over-large extraction with a SOAP fault under a successful status, so a body that does
/// not deserialize is reported against the cell limit rather than as a parse bug.
pub fn parse_response(
    extraction: &EurostatExtraction,
    body: &str,
) -> Result<ParsedEurostatResponse, AppError> {
    let response: EurostatResponse = serde_json::from_str(body).map_err(|error| {
        AppError::from(format!(
            "eurostat response is not a JSON-stat dataset, which is how an extraction over 5,000,000 cells is \
             refused; [dataset={} error={error}]",
            extraction.dataset,
        ))
    })?;

    let publication: ParsedEurostatPublication = ParsedEurostatPublication {
        revision_label: response.updated.clone(),
    };

    Ok(ParsedEurostatResponse {
        publication,
        revision_by_geo_code: revision_by_geo_code(&response),
        observations: parse_observations(&response)?,
    })
}

/// A response names territory under several revisions of the classification at once, so which revision a
/// code belongs to is what decides whether it means what the canonical store thinks it means.
fn revision_by_geo_code(response: &EurostatResponse) -> BTreeMap<String, i32> {
    let Some(geo) = response.dimension.get(DIMENSION_GEO)
    else {
        return BTreeMap::new();
    };

    geo.category.label
        .iter()
        .filter_map(|(code, label)| geo_revision_of(label).map(|revision| (code.clone(), revision)))
        .collect()
}

/// Eurostat suffixes a geo label with the revision that defines the region, `(NUTS 2021)` inside the
/// regulation and `(statistical region 2021)` outside it, and leaves the suffix off where no revision has
/// recut the region.
fn geo_revision_of(label: &str) -> Option<i32> {
    let opening: usize = label.rfind('(')?;
    let marker: &str = label[opening + 1..].strip_suffix(')')?;
    let year: &str = REVISION_MARKER_PREFIXES
        .iter()
        .find_map(|prefix| marker.strip_prefix(prefix))?;

    year.parse().ok()
}

/// JSON-stat addresses an observation by one flat index over the dimensions in `id` order, so a position is
/// decomposed by the strides its dimension sizes imply, right-most varying fastest.
fn parse_observations(response: &EurostatResponse) -> Result<Vec<ParsedEurostatObservation>, AppError> {
    let indicator_codes: Vec<&str> = positional_codes(response, DIMENSION_INDICATOR)?;
    let geo_codes: Vec<&str> = positional_codes(response, DIMENSION_GEO)?;
    let time_codes: Vec<&str> = positional_codes(response, DIMENSION_TIME)?;

    let indicator_axis: usize = axis_of(response, DIMENSION_INDICATOR)?;
    let geo_axis: usize = axis_of(response, DIMENSION_GEO)?;
    let time_axis: usize = axis_of(response, DIMENSION_TIME)?;
    let strides: Vec<usize> = strides_of(&response.size);

    let mut observations: Vec<ParsedEurostatObservation> = Vec::with_capacity(response.value.len());
    let mut projected_coordinates: HashSet<(usize, usize, usize)> = HashSet::with_capacity(response.value.len());
    for (&position, &value) in &response.value {
        let indicator_index: usize = (position / strides[indicator_axis]) % response.size[indicator_axis];
        let geo_index: usize = (position / strides[geo_axis]) % response.size[geo_axis];
        let time_index: usize = (position / strides[time_axis]) % response.size[time_axis];

        let is_first: bool = projected_coordinates.insert((indicator_index, geo_index, time_index));
        if !is_first {
            return Err(AppError::from(format!(
                "eurostat cells differ only in a dimension this parser drops, so one would silently replace \
                 the other; [indicator={} geo={} period={}]",
                indicator_codes[indicator_index],
                geo_codes[geo_index],
                time_codes[time_index],
            )));
        }

        let period_year: i32 = time_codes[time_index].parse().map_err(|_| {
            AppError::from(format!(
                "eurostat time category is not a year; [code={}]",
                time_codes[time_index],
            ))
        })?;

        observations.push(ParsedEurostatObservation {
            indicator_code: indicator_codes[indicator_index].to_string(),
            geo_code: geo_codes[geo_index].to_string(),
            period_year,
            value,
            flag: response.status.get(&position).cloned(),
        });
    }

    Ok(observations)
}

/// The codes of one dimension's categories, ordered by the position each occupies. The index map is
/// code-to-position, and a position missing from it would silently shift every code after it, so a gap is an
/// error rather than a hole.
fn positional_codes<'a>(response: &'a EurostatResponse, dimension_name: &str) -> Result<Vec<&'a str>, AppError> {
    let dimension: &EurostatDimension = response.dimension.get(dimension_name).ok_or_else(|| {
        AppError::from(format!("eurostat response has no such dimension; [dimension={dimension_name}]"))
    })?;

    let mut codes_by_position: BTreeMap<usize, &str> = BTreeMap::new();
    for (code, &position) in &dimension.category.index {
        codes_by_position.insert(position, code.as_str());
    }

    let is_contiguous: bool = codes_by_position.len() == dimension.category.index.len()
        && codes_by_position.keys().copied().eq(0..codes_by_position.len());
    if !is_contiguous {
        return Err(AppError::from(format!(
            "eurostat category index is not a bijection onto its positions; [dimension={dimension_name} categories={}]",
            dimension.category.index.len(),
        )));
    }

    Ok(codes_by_position.into_values().collect())
}

fn axis_of(response: &EurostatResponse, dimension_name: &str) -> Result<usize, AppError> {
    response.id.iter().position(|name| name == dimension_name).ok_or_else(|| {
        AppError::from(format!("eurostat response does not order that dimension; [dimension={dimension_name}]"))
    })
}

/// The multiplier each axis contributes to a flat index: the product of every size to its right.
fn strides_of(size: &[usize]) -> Vec<usize> {
    let mut strides: Vec<usize> = vec![1; size.len()];
    for axis in (0..size.len().saturating_sub(1)).rev() {
        strides[axis] = strides[axis + 1] * size[axis + 1];
    }

    strides
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTRY_LEVEL_EXTRACTION: &str = include_str!("../../samples/eurostat/country_level_extraction.json");
    const FLAGGED_OBSERVATIONS: &str = include_str!("../../samples/eurostat/flagged_observations.json");
    const UNFLAGGED_OBSERVATIONS: &str = include_str!("../../samples/eurostat/unflagged_observations.json");

    fn extraction_for(geo_level: EurostatGeoLevel) -> &'static EurostatExtraction {
        EXTRACTIONS
            .iter()
            .find(|extraction| extraction.geo_level == geo_level)
            .expect("an extraction for the level")
    }

    fn parse_country_level(body: &str) -> Result<ParsedEurostatResponse, AppError> {
        parse_response(extraction_for(EurostatGeoLevel::Country), body)
    }

    fn observation<'a>(
        observations: &'a [ParsedEurostatObservation],
        indicator_code: &str,
        geo_code: &str,
        period_year: i32,
    ) -> Option<&'a ParsedEurostatObservation> {
        observations.iter().find(|observation| {
            observation.indicator_code == indicator_code
                && observation.geo_code == geo_code
                && observation.period_year == period_year
        })
    }

    #[test]
    fn parse_response_reads_the_updated_timestamp_as_the_revision_label() {
        let response: ParsedEurostatResponse = parse_country_level(COUNTRY_LEVEL_EXTRACTION).unwrap();

        assert_eq!(response.publication.revision_label, "2026-08-14T23:00:00+0200");
    }

    #[test]
    fn parse_response_addresses_every_observation_by_its_flat_index() {
        let response: ParsedEurostatResponse = parse_country_level(COUNTRY_LEVEL_EXTRACTION).unwrap();

        // 5,001 of 9,555 addressable cells carry a value; the rest are simply absent from the value map.
        assert_eq!(response.observations.len(), 5001);

        let french_rate: &ParsedEurostatObservation =
            observation(&response.observations, INDICATOR_TOTAL_FERTILITY_RATE, "FR", 2023).unwrap();
        assert!((french_rate.value - 1.66).abs() < 1e-9);

        let french_first_birth: &ParsedEurostatObservation =
            observation(&response.observations, INDICATOR_MEAN_AGE_AT_FIRST_BIRTH, "FR", 2023).unwrap();
        assert!(french_first_birth.value > 28.0 && french_first_birth.value < 30.0);
    }

    #[test]
    fn parse_response_reads_all_three_indicators() {
        let response: ParsedEurostatResponse = parse_country_level(COUNTRY_LEVEL_EXTRACTION).unwrap();

        for indicator_code in [
            INDICATOR_TOTAL_FERTILITY_RATE,
            INDICATOR_MEAN_AGE_AT_CHILDBIRTH,
            INDICATOR_MEAN_AGE_AT_FIRST_BIRTH,
        ] {
            let count: usize = response.observations
                .iter()
                .filter(|observation| observation.indicator_code == indicator_code)
                .count();

            assert!(count > 1000, "{indicator_code} carried only {count} observations");
        }
    }

    #[test]
    fn parse_response_attaches_a_flag_to_the_observation_it_belongs_to() {
        let response: ParsedEurostatResponse = parse_country_level(FLAGGED_OBSERVATIONS).unwrap();

        // The sample's status map flags positions 2 and 3, but only position 3 carries a value; a flag on a
        // valueless cell has no observation to attach to and is dropped with it.
        assert_eq!(response.observations.len(), 1);
        assert_eq!(response.observations[0].period_year, 2016);
        assert_eq!(response.observations[0].flag.as_deref(), Some("p"));
    }

    #[test]
    fn parse_response_leaves_the_flag_absent_when_the_response_carries_no_status() {
        let response: ParsedEurostatResponse = parse_country_level(UNFLAGGED_OBSERVATIONS).unwrap();

        assert!(!response.observations.is_empty());
        assert!(response.observations.iter().all(|observation| observation.flag.is_none()));
    }

    #[test]
    fn parse_response_rejects_a_body_that_is_not_a_dataset() {
        let error: AppError =
            parse_country_level("<S:Fault><faultcode>413</faultcode></S:Fault>").unwrap_err();

        assert!(error.to_string().contains("5,000,000"));
    }

    #[test]
    fn parse_response_reads_each_label_revision_and_leaves_the_unmarked_absent() {
        let body: &str = r#"{
            "updated": "2026-01-01T00:00:00+0100",
            "id": ["indic_de", "geo", "time"],
            "size": [1, 3, 1],
            "dimension": {
                "indic_de": {"category": {"index": {"TOTFERRT": 0}}},
                "geo": {"category": {
                    "index": {"HR02": 0, "HR04": 1, "DE3": 2},
                    "label": {
                        "HR02": "Panonska Hrvatska (NUTS 2021)",
                        "HR04": "Kontinentalna Hrvatska (NUTS 2016)",
                        "DE3": "Berlin"
                    }
                }},
                "time": {"category": {"index": {"2020": 0}}}
            },
            "value": {"0": 1.5, "1": 1.4, "2": 1.3}
        }"#;

        let response: ParsedEurostatResponse =
            parse_response(extraction_for(EurostatGeoLevel::Nuts2), body).unwrap();

        assert_eq!(
            response.revision_by_geo_code,
            BTreeMap::from([("HR02".to_string(), 2021), ("HR04".to_string(), 2016)]),
        );
    }

    #[test]
    fn parse_response_rejects_cells_that_collide_once_a_dimension_is_dropped() {
        let body: &str = r#"{
            "updated": "2026-01-01T00:00:00+0100",
            "id": ["indic_de", "unit", "geo", "time"],
            "size": [1, 2, 1, 1],
            "dimension": {
                "indic_de": {"category": {"index": {"TOTFERRT": 0}}},
                "unit": {"category": {"index": {"NR": 0, "YR": 1}}},
                "geo": {"category": {"index": {"DE3": 0}}},
                "time": {"category": {"index": {"2020": 0}}}
            },
            "value": {"0": 1.5, "1": 30.2}
        }"#;

        let error: AppError = parse_response(extraction_for(EurostatGeoLevel::Nuts1), body).unwrap_err();

        assert!(error.to_string().contains("differ only in a dimension"));
    }

    #[test]
    fn geo_revision_of_reads_both_marker_forms() {
        assert_eq!(geo_revision_of("North East (UK) (NUTS 2021)"), Some(2021));
        assert_eq!(geo_revision_of("Kontinentalna Hrvatska (NUTS 2016)"), Some(2016));
        assert_eq!(geo_revision_of("Oslo og Akershus (statistical region 2016)"), Some(2016));
    }

    #[test]
    fn geo_revision_of_leaves_a_parenthesis_that_belongs_to_the_name() {
        assert_eq!(geo_revision_of("Berlin"), None);
        assert_eq!(geo_revision_of("Centro (ES)"), None);
        assert_eq!(geo_revision_of("Lindau (Bodensee)"), None);
        assert_eq!(geo_revision_of("Frankfurt (Oder), Kreisfreie Stadt"), None);
    }

    #[test]
    fn strides_of_multiplies_the_sizes_to_the_right() {
        assert_eq!(strides_of(&[1, 3, 49, 65]), vec![9555, 3185, 65, 1]);
    }
}
