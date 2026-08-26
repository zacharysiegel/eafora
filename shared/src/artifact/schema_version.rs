//! Forward-compatible schema-version gate for versioned JSON documents (manifest,
//! discovery). Reads only the version field, so a future schema version that
//! changes the document's shape is reported as a version mismatch rather than a
//! field-level parse error. The rest of the (possibly incompatible) document is
//! never deserialized here.

use crate::error::AppError;

/// Require the `field_name` integer in `bytes` to equal `expected`.
pub fn require_schema_version(bytes: &[u8], field_name: &str, expected: u32) -> Result<(), AppError> {
    let found: u64 = read_schema_version(bytes, field_name)?;

    // `as_u64` is serde_json's only unsigned accessor; compare in u64 (widening the u32 `expected`)
    // rather than narrowing `found`, so an out-of-u32-range version is rejected, not truncated into a match.
    if found != u64::from(expected) {
        return Err(AppError::from(describe_mismatch(field_name, found, u64::from(expected))));
    }

    Ok(())
}

/// The `field_name` integer alone. Parsing to `serde_json::Value` accepts any well-formed JSON regardless of
/// the document's shape.
pub fn read_schema_version(bytes: &[u8], field_name: &str) -> Result<u64, AppError> {
    let document: serde_json::Value = serde_json::from_slice(bytes)?;

    document
        .get(field_name)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| AppError::from(format!("document missing integer {} field", field_name)))
}

/// A stale document and a stale reader both produce a mismatch, and their remedies are opposite, so the message
/// names which one it found. Generic over the version's type so no caller has to convert and risk truncating.
pub fn describe_mismatch<T: std::fmt::Display + PartialOrd>(field_name: &str, found: T, expected: T) -> String {
    let diagnosis: &str = if found < expected {
        "predates this build and must be republished"
    } else {
        "comes from a newer build, so this one is out of date"
    };

    format!("{field_name} {found} {diagnosis}; [expected={expected}]")
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
    fn read_schema_version_returns_the_found_value_whatever_the_reader_expects() {
        let json: &[u8] = br#"{ "v": 7, "other": "ignored" }"#;

        assert_eq!(read_schema_version(json, "v").unwrap(), 7);
    }

    #[test]
    fn read_schema_version_rejects_a_document_without_an_integer_field() {
        assert!(read_schema_version(br#"{ "other": 1 }"#, "v").is_err());
        assert!(read_schema_version(br#"{ "v": "2" }"#, "v").is_err());
        assert!(read_schema_version(b"not json at all", "v").is_err());
    }

    #[test]
    fn require_schema_version_reports_a_document_older_than_the_reader() {
        let json: &[u8] = br#"{ "v": 1 }"#;

        let error: AppError = require_schema_version(json, "v", 2).unwrap_err();

        assert!(error.to_string().contains("v 1 predates this build and must be republished; [expected=2]"));
    }

    #[test]
    fn require_schema_version_reports_a_document_newer_than_the_reader() {
        let json: &[u8] = br#"{ "v": 2 }"#;

        let error: AppError = require_schema_version(json, "v", 1).unwrap_err();

        assert!(error.to_string().contains("v 2 comes from a newer build, so this one is out of date; [expected=1]"));
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

        assert!(error.to_string().contains("v 2 comes from a newer build"));
    }
}
