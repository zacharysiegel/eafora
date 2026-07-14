use leptos::hydration::{AutoReload, HydrationScripts};
use leptos::prelude::*;
use leptos_i18n::context::UseLocalesOptions;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::StaticSegment;

use crate::i18n::*;

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <link rel="stylesheet" id="leptos" href="/pkg/eafora.css" />
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
    // English-only v1: no request-based locale detection. A header getter that returns None keeps the
    // SSR locale at the default (`en`) and avoids leptos_i18n's Accept-Language warning, without the
    // axum/actix detection path (whose async locale resolution panics under SSR in leptos_i18n 0.6.2).
    let ssr_lang_header_getter: UseLocalesOptions =
        UseLocalesOptions::default().ssr_lang_header_getter(|| None::<String>);

    view! {
        <I18nContextProvider ssr_lang_header_getter=ssr_lang_header_getter>
            <Router>
                <Routes fallback=|| "Page not found.".into_view()>
                    <Route path=StaticSegment("") view=MapView />
                </Routes>
            </Router>
        </I18nContextProvider>
    }
}

#[component]
fn MapView() -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <main id="map-view">
            <p>{t!(i18n, controls.loading)}</p>
        </main>
    }
}
