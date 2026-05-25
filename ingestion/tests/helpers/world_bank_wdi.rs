//! WB-WDI-specific test helpers (sample loading; assertion utilities can be
//! added here as Phase 3 lands). Loads checked-in sample responses from
//! `ingestion/samples/wb_wdi/` without live HTTP.

// `load_sample` is consumed by the sample-replay integration tests (T032/T033)
// that haven't landed yet; the helper module is shared across test binaries
// which prevents adding the attribute per-function, so it stays here at the
// module level until the first caller wires in.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use ingestion::world_bank_wdi::world_bank_wdi_model::WdiResponse;

/// Reads `ingestion/samples/wb_wdi/<name>.json` and deserializes it into
/// the same `WdiResponse` type the live adapter receives from the WB API.
/// Panics with a path-aware message on any I/O or parse error — these are
/// test-fixture failures, not user-facing code paths.
pub fn load_sample(name: &str) -> WdiResponse {
    let sample_path: PathBuf = sample_path(name);
    let file_text: String = read_or_panic(&sample_path);
    parse_or_panic(&sample_path, &file_text)
}

fn sample_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("samples/wb_wdi")
        .join(format!("{}.json", name))
}

fn read_or_panic(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read sample {}: {}", path.display(), err))
}

fn parse_or_panic(path: &PathBuf, file_text: &str) -> WdiResponse {
    serde_json::from_str(file_text)
        .unwrap_or_else(|err| panic!("parse sample {}: {}", path.display(), err))
}
