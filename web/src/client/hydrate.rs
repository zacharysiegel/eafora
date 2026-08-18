use crate::app::App;

// The wasm entry point cargo-leptos' generated JS calls to start the client-side app: it attaches
// to the server-rendered DOM and takes over. Exported as `hydrate` (the name cargo-leptos expects).
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();

    /* A release client reports only what a visitor's console should carry: something went wrong. The
       info and debug call sites are compiled out of that build besides, so this only decides what a
       development build shows. */
    let level: log::Level = if cfg!(debug_assertions) {
        log::Level::Debug
    } else {
        log::Level::Warn
    };
    _ = console_log::init_with_level(level);

    log::debug!("the client is attaching to the served document");

    leptos::mount::hydrate_body(App);
}
