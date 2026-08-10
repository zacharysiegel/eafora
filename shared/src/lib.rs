pub mod artifact;
pub mod canonical;
pub mod error;
pub mod filesystem;
pub mod http;
pub mod license;
pub mod map;
pub mod math;
// render: the wgpu GPU stack. Feature-gated so the ingestion producer never links wgpu. Reached via
// `render::` rather than flattened at the crate root: it is general GPU infrastructure
#[cfg(feature = "render")]
pub mod render;
pub mod revision;
pub mod sqlite;

pub use error::AppError;

// wasm32 only: configures wasm-bindgen-test to run #[wasm_bindgen_test] cases in a headless browser.
#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
