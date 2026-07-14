use crate::app::App;

// The wasm entry point cargo-leptos's generated JS calls to start the client-side app: it attaches
// to the server-rendered DOM and takes over. Exported as `hydrate` (the name cargo-leptos expects).
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    _ = console_log::init_with_level(log::Level::Debug);

    leptos::mount::hydrate_body(App);
}
