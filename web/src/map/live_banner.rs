use std::time::Duration;

use leptos::prelude::*;

use crate::i18n::*;

const VISIBLE_DURATION: Duration = Duration::from_secs(4);

/// The notice shown when the live bundle could not be loaded. Nothing clears the failure itself, so the notice
/// retires on a timer and can be dismissed; the map it sits over stays usable from the cached or embedded
/// bundle either way.
#[component]
pub fn LiveBanner() -> impl IntoView {
    let i18n = use_i18n();
    let live_load_notice_shown: RwSignal<bool> = expect_context();

    // Effects do not run during server rendering, so the timer is only ever scheduled in the browser.
    Effect::new(move |_| {
        if !live_load_notice_shown.get() {
            return;
        }

        set_timeout(move || live_load_notice_shown.set(false), VISIBLE_DURATION);
    });

    view! {
        <Show when=move || live_load_notice_shown.get()>
            <div class="map-live-banner panel" role="status">
                <span>{t!(i18n, live.load_failed)}</span>
                <button
                    class="map-live-banner-close"
                    aria-label=move || t_string!(i18n, live.dismiss)
                    on:click=move |_| live_load_notice_shown.set(false)
                >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">
                        <path d="M5 5l14 14M19 5L5 19"></path>
                    </svg>
                </button>
            </div>
        </Show>
    }
}
