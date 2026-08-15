include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));

pub mod app;
pub mod distribution;
pub mod live_resolve;
pub mod map;

// Browser-only runtime glue; the ssr build compiles none of it.
#[cfg(feature = "hydrate")]
pub mod client;

// In wasm32 test builds, run the #[wasm_bindgen_test] cases in a headless browser.
#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
