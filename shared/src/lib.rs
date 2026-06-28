pub mod artifact;
pub mod canonical;
pub mod error;
pub mod filesystem;
pub mod license;
pub mod revision;
pub mod sqlite;

pub use artifact::*;
pub use canonical::*;
pub use filesystem::*;
pub use license::*;
pub use revision::*;
pub use sqlite::*;

pub use error::AppError;

// Routes the target-agnostic #[wasm_bindgen_test] cases (gated via cfg_attr in each module's
// test block) through a headless browser, so the same parse/verify/bundle logic is exercised on
// wasm32, not just the host. Run with `wasm-pack test --headless --chrome --package shared`.
#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
