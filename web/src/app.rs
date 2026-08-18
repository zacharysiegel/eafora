use leptos::prelude::*;
use leptos_i18n::context::{CookieOptions, UseLocalesOptions};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::StaticSegment;

use crate::i18n::*;
use crate::map::MapView;

/* Only the ssr build renders a document, and these three exist solely to build it: on wasm they would
   compile HashedStylesheet's filesystem lookup and AutoReload's embedded reload script for a function
   nothing calls. */
#[cfg(feature = "ssr")]
use leptos::hydration::{AutoReload, HydrationScripts};
#[cfg(feature = "ssr")]
use leptos_meta::HashedStylesheet;

#[cfg(feature = "ssr")]
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <link rel="icon" type="image/svg+xml" href="/favicon.svg" />
                <HashedStylesheet options=options.clone() id="leptos" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    // This site is statically generated: the server render runs at build time, so there is no
    // per-request Accept-Language header or Cookie to read or set. Both SSR getters are therefore
    // no-ops and the server renders the default locale; the locale cookie is enabled for client-side
    // persistence, which begins to matter once a second locale and a switcher exist.
    let cookie_options: CookieOptions<Locale> = CookieOptions::default()
        .ssr_cookies_header_getter(|| None::<String>)
        .ssr_set_cookie(|_| {});
    let ssr_lang_header_getter: UseLocalesOptions =
        UseLocalesOptions::default().ssr_lang_header_getter(|| None::<String>);

    view! {
        <I18nContextProvider
            cookie_options=cookie_options
            ssr_lang_header_getter=ssr_lang_header_getter
        >
            <Router>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path=StaticSegment("") view=MapView />
                </Routes>
            </Router>
        </I18nContextProvider>
    }
}
