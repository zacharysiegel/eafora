include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));

pub mod app;
mod map;

// Browser-only runtime glue (OPFS cache, fetch, loader, and the wasm entry point). Gated once here;
// the ssr build compiles none of it. The map render path in `map` compiles under both targets; its
// wgpu driver is gated internally.
#[cfg(feature = "hydrate")]
mod client;

// In wasm32 test builds, run the #[wasm_bindgen_test] cases in a headless browser.
#[cfg(all(test, target_arch = "wasm32"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
