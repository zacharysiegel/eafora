//! Forward-compatible schema-version gate for versioned JSON documents (manifest,
//! discovery). Reads only the version field, so a future schema version that
//! changes the document's shape is reported as a version mismatch rather than a
//! field-level parse error — the rest of the (possibly incompatible) document is
//! never deserialized here.

use crate::error::AppError;

/// Require the `field_name` integer in `bytes` to equal `expected`. On mismatch,
/// the error message is `unknown {field_name} {found}`; a missing or non-integer
/// field is reported separately. Parsing to `serde_json::Value` accepts any
/// well-formed JSON regardless of the document's shape.
pub fn require_schema_version(bytes: &[u8], field_name: &str, expected: u32) -> Result<(), AppError> {
    let document: serde_json::Value = serde_json::from_slice(bytes)?;

    let found: u64 = document
        .get(field_name)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| AppError::from(format!("document missing integer {} field", field_name)))?;

    if found != expected as u64 {
        return Err(AppError::from(format!("unknown {} {}", field_name, found)));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_schema_version_accepts_matching_version() {
        let json: &[u8] = br#"{ "v": 1, "other": "ignored" }"#;

        require_schema_version(json, "v", 1).unwrap();
    }

    #[test]
    fn require_schema_version_rejects_mismatch_naming_the_field() {
        let json: &[u8] = br#"{ "v": 2 }"#;

        let error: AppError = require_schema_version(json, "v", 1).unwrap_err();

        assert!(error.to_string().contains("unknown v 2"));
    }

    #[test]
    fn require_schema_version_rejects_missing_field() {
        let json: &[u8] = br#"{ "other": 1 }"#;

        assert!(require_schema_version(json, "v", 1).is_err());
    }

    #[test]
    fn require_schema_version_reads_version_from_a_changed_document_shape() {
        let json: &[u8] = br#"{ "v": 2, "totally": { "different": ["shape"] } }"#;

        let error: AppError = require_schema_version(json, "v", 1).unwrap_err();

        assert!(error.to_string().contains("unknown v 2"));
    }
}
