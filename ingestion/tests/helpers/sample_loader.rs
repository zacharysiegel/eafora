//! Loads checked-in WB WDI sample responses from `ingestion/samples/wb_wdi/`
//! for integration tests that exercise the parse → normalize → upsert
//! pipeline without live HTTP. The CARGO_MANIFEST_DIR base ensures the path
//! resolves correctly regardless of the cwd cargo invokes the test from.

use std::path::PathBuf;

use ingestion::world_bank_wdi::world_bank_wdi_model::WdiResponse;

pub fn load_wb_wdi_sample(name: &str) -> WdiResponse {
    let mut path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("samples/wb_wdi");
    path.push(format!("{}.json", name));
    let file_text: String = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read sample {}: {}", path.display(), err));
    serde_json::from_str(&file_text)
        .unwrap_or_else(|err| panic!("parse sample {}: {}", path.display(), err))
}
