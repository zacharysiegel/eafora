use crate::app::App;

// The wasm entry point cargo-leptos' generated JS calls to start the client-side app: it attaches
// to the server-rendered DOM and takes over. Exported as `hydrate` (the name cargo-leptos expects).
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();

    /* One level serves both builds: a release client has no debug call sites left to filter, since
       Cargo.toml caps this target at info, and it keeps its info output because a static deploy has no
       server to log to. */
    _ = console_log::init_with_level(log::Level::Debug);

    log::debug!("the client is attaching to the served document");

    leptos::mount::hydrate_body(App);
}
