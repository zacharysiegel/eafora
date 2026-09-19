pub mod artifact;
pub mod canonical;
pub mod error;
pub mod filesystem;
pub mod http;
pub mod license;
pub mod map;
pub mod math;
// Feature-gated so a consumer that does not render never links wgpu.
#[cfg(feature = "render")]
pub mod render;
pub mod revision;
pub mod settings;
pub mod sqlite;

pub use error::AppError;

// wasm-bindgen-test is a wasm32-only dev-dependency.
#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
