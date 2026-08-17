// Release builds monomorphize the whole view tree into one deeply nested concrete type, whose layout
// computation exceeds rustc's default query depth. Dev builds do not hit it: cargo-leptos type-erases them.
#![recursion_limit = "512"]

include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));

pub mod app;
pub mod distribution;
pub mod live_resolve;
pub mod map;
pub mod version_rank;

// Browser-only runtime glue; the ssr build compiles none of it.
#[cfg(feature = "hydrate")]
pub mod client;

// The dev server and the static shell export; neither exists in a browser.
#[cfg(feature = "ssr")]
pub mod server;

// In wasm32 test builds, run the #[wasm_bindgen_test] cases in a headless browser.
#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
