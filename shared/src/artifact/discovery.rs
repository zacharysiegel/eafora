use serde::{Deserialize, Serialize};

use crate::error::AppError;

pub const DISCOVERY_SCHEMA_VERSION: u32 = 1;

/// The single forever-URL of the Eafora system. Consumers commit to exactly this
/// URL; everything else (including `repository_base_url`) is server-supplied at
/// runtime by the discovery document fetched from here.
pub const DISCOVERY_URL: &str = "https://eafora.org/discovery";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryDocument {
    pub schema_version: u32,
    pub repository_base_url: String,
    pub minimum_client_version: String,
    pub sunset: Option<String>,
}

/// Peeks only the version field so a shape change in a future schema version is
/// rejected with a clear message before the full (possibly incompatible) parse.
#[derive(Deserialize)]
struct DiscoverySchemaProbe {
    schema_version: u32,
}

pub fn parse_discovery_document(bytes: &[u8]) -> Result<DiscoveryDocument, AppError> {
    let probe: DiscoverySchemaProbe = serde_json::from_slice(bytes)?;
    if probe.schema_version != DISCOVERY_SCHEMA_VERSION {
        return Err(AppError::from(format!(
            "unknown schema_version {}",
            probe.schema_version,
        )));
    }

    let document: DiscoveryDocument = serde_json::from_slice(bytes)?;

    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_DISCOVERY_JSON: &str = r#"{
  "schema_version": 1,
  "repository_base_url": "https://repository.eafora.org",
  "minimum_client_version": "1.0.0",
  "sunset": null
}"#;

    #[test]
    fn parse_discovery_document_round_trips_fixture() {
        let document: DiscoveryDocument = parse_discovery_document(VALID_DISCOVERY_JSON.as_bytes()).unwrap();

        assert_eq!(document.schema_version, 1);
        assert_eq!(document.repository_base_url, "https://repository.eafora.org");
        assert_eq!(document.minimum_client_version, "1.0.0");
        assert_eq!(document.sunset, None);
    }

    #[test]
    fn parse_discovery_document_rejects_unknown_schema_version() {
        let json: String = VALID_DISCOVERY_JSON.replace("\"schema_version\": 1", "\"schema_version\": 2");

        let error: AppError = parse_discovery_document(json.as_bytes()).unwrap_err();

        assert!(error.to_string().contains("unknown schema_version 2"));
    }

    #[test]
    fn parse_discovery_document_handles_missing_sunset_field() {
        let json: &str = r#"{
  "schema_version": 1,
  "repository_base_url": "https://repository.eafora.org",
  "minimum_client_version": "1.0.0"
}"#;

        let document: DiscoveryDocument = parse_discovery_document(json.as_bytes()).unwrap();

        assert_eq!(document.sunset, None);
    }

    #[test]
    fn parse_discovery_document_ignores_unknown_fields() {
        let json: &str = r#"{
  "schema_version": 1,
  "repository_base_url": "https://repository.eafora.org",
  "minimum_client_version": "1.0.0",
  "sunset": null,
  "field_added_in_a_future_v1_revision": "ignored"
}"#;

        let document: DiscoveryDocument = parse_discovery_document(json.as_bytes()).unwrap();

        assert_eq!(document.repository_base_url, "https://repository.eafora.org");
    }
}
