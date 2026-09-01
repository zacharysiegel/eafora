use std::collections::BTreeMap;

use crate::error::AppError;
use crate::eurostat::eurostat_model::{
    EurostatDimension, EurostatResponse, ParsedEurostatObservation, ParsedEurostatPublication,
};
use crate::http;

const API_BASE_URL: &str = "https://ec.europa.eu/eurostat/api/dissemination/statistics/1.0/data";
const DATASET: &str = "demo_find";

/// The indicators phase one ingests, one dimension member each.
pub const INDICATOR_TOTAL_FERTILITY_RATE: &str = "TOTFERRT";
pub const INDICATOR_MEAN_AGE_AT_CHILDBIRTH: &str = "AGEMOTH";
pub const INDICATOR_MEAN_AGE_AT_FIRST_BIRTH: &str = "AGEMOTH1";

const DIMENSION_INDICATOR: &str = "indic_de";
const DIMENSION_GEO: &str = "geo";
const DIMENSION_TIME: &str = "time";

/// Above 500,000 cells Eurostat answers asynchronously and above 5,000,000 it refuses; one indicator across
/// every country and year is approx. 9,555, so the synchronous path is the only one this adapter needs.
pub async fn fetch_upstream() -> Result<String, AppError> {
    let url: String = format!(
        "{API_BASE_URL}/{DATASET}?format=JSON&lang=EN&geoLevel=country\
         &{DIMENSION_INDICATOR}={INDICATOR_TOTAL_FERTILITY_RATE}\
         &{DIMENSION_INDICATOR}={INDICATOR_MEAN_AGE_AT_CHILDBIRTH}\
         &{DIMENSION_INDICATOR}={INDICATOR_MEAN_AGE_AT_FIRST_BIRTH}",
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
pub fn parse_response(body: &str) -> Result<(ParsedEurostatPublication, Vec<ParsedEurostatObservation>), AppError> {
    let response: EurostatResponse = serde_json::from_str(body).map_err(|error| {
        AppError::from(format!(
            "eurostat response is not a JSON-stat dataset, which is how an extraction over 5,000,000 cells is \
             refused; [dataset={DATASET} error={error}]",
        ))
    })?;

    let publication: ParsedEurostatPublication = ParsedEurostatPublication {
        revision_label: response.updated.clone(),
    };
    let observations: Vec<ParsedEurostatObservation> = parse_observations(&response)?;

    Ok((publication, observations))
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
    for (&position, &value) in &response.value {
        let indicator_index: usize = (position / strides[indicator_axis]) % response.size[indicator_axis];
        let geo_index: usize = (position / strides[geo_axis]) % response.size[geo_axis];
        let time_index: usize = (position / strides[time_axis]) % response.size[time_axis];

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
        let (publication, _) = parse_response(COUNTRY_LEVEL_EXTRACTION).unwrap();

        assert_eq!(publication.revision_label, "2026-08-14T23:00:00+0200");
    }

    #[test]
    fn parse_response_addresses_every_observation_by_its_flat_index() {
        let (_, observations) = parse_response(COUNTRY_LEVEL_EXTRACTION).unwrap();

        // 5,001 of 9,555 addressable cells carry a value; the rest are simply absent from the value map.
        assert_eq!(observations.len(), 5001);

        let french_rate: &ParsedEurostatObservation =
            observation(&observations, INDICATOR_TOTAL_FERTILITY_RATE, "FR", 2023).unwrap();
        assert!((french_rate.value - 1.66).abs() < 1e-9);

        let french_first_birth: &ParsedEurostatObservation =
            observation(&observations, INDICATOR_MEAN_AGE_AT_FIRST_BIRTH, "FR", 2023).unwrap();
        assert!(french_first_birth.value > 28.0 && french_first_birth.value < 30.0);
    }

    #[test]
    fn parse_response_reads_all_three_indicators() {
        let (_, observations) = parse_response(COUNTRY_LEVEL_EXTRACTION).unwrap();

        for indicator_code in [
            INDICATOR_TOTAL_FERTILITY_RATE,
            INDICATOR_MEAN_AGE_AT_CHILDBIRTH,
            INDICATOR_MEAN_AGE_AT_FIRST_BIRTH,
        ] {
            let count: usize = observations.iter().filter(|observation| observation.indicator_code == indicator_code).count();

            assert!(count > 1000, "{indicator_code} carried only {count} observations");
        }
    }

    #[test]
    fn parse_response_attaches_a_flag_to_the_observation_it_belongs_to() {
        let (_, observations) = parse_response(FLAGGED_OBSERVATIONS).unwrap();

        // The sample's status map flags positions 2 and 3, but only position 3 carries a value; a flag on a
        // valueless cell has no observation to attach to and is dropped with it.
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].period_year, 2016);
        assert_eq!(observations[0].flag.as_deref(), Some("p"));
    }

    #[test]
    fn parse_response_leaves_the_flag_absent_when_the_response_carries_no_status() {
        let (_, observations) = parse_response(UNFLAGGED_OBSERVATIONS).unwrap();

        assert!(!observations.is_empty());
        assert!(observations.iter().all(|observation| observation.flag.is_none()));
    }

    #[test]
    fn parse_response_rejects_a_body_that_is_not_a_dataset() {
        let error: AppError = parse_response("<S:Fault><faultcode>413</faultcode></S:Fault>").unwrap_err();

        assert!(error.to_string().contains("5,000,000"));
    }

    #[test]
    fn strides_of_multiplies_the_sizes_to_the_right() {
        assert_eq!(strides_of(&[1, 3, 49, 65]), vec![9555, 3185, 65, 1]);
    }
}
