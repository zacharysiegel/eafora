//! WB-WDI-specific test helpers (sample loading; assertion utilities can be
//! added here as Phase 3 lands). Loads checked-in sample responses from
//! `ingestion/samples/wb_wdi/` without live HTTP.

use std::path::PathBuf;

use ingestion::world_bank_wdi::world_bank_wdi_model::WdiResponse;

pub fn load_sample(name: &str) -> WdiResponse {
    let mut path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("samples/wb_wdi");
    path.push(format!("{}.json", name));
    let file_text: String = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read sample {}: {}", path.display(), err));
    serde_json::from_str(&file_text)
        .unwrap_or_else(|err| panic!("parse sample {}: {}", path.display(), err))
}
