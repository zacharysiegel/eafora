use leptos::prelude::*;

use crate::i18n::*;

/// The bottom-right settings button, replaced in place by a settings panel while open. Closes on the ✕,
/// or a click outside the panel. The panel's toggles read and persist through the platform settings store
/// and dispatch the change to the map driver.
#[component]
pub fn SettingsModal() -> impl IntoView {
    let i18n = use_i18n();
    let open: RwSignal<bool> = RwSignal::new(false);
    // Seed the default; a first client render must match the server's, so the persisted value is loaded
    // in an effect after mount rather than read during render.
    let regions_expand: RwSignal<bool> = RwSignal::new(true);

    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        regions_expand.set(regions_expand_on_hover());
    });

    let toggle_regions_expand = move |_| {
        let next: bool = !regions_expand.get_untracked();
        regions_expand.set(next);
        set_regions_expand_on_hover(next);
    };

    view! {
        <div class="settings" class:is-open=move || open.get()>
            <div class="settings-scrim" on:click=move |_| open.set(false)></div>

            <button
                class="settings-button"
                aria-label=move || t_string!(i18n, settings.open)
                on:click=move |_| open.set(true)
            >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                    <circle cx="12" cy="12" r="3"></circle>
                    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
                </svg>
            </button>

            <section class="settings-modal panel" role="dialog" aria-modal="true">
                <header class="settings-modal-header">
                    <span class="settings-modal-title">{t!(i18n, settings.title)}</span>
                    <button
                        class="settings-close"
                        aria-label=move || t_string!(i18n, settings.close)
                        on:click=move |_| open.set(false)
                    >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">
                            <path d="M5 5l14 14M19 5L5 19"></path>
                        </svg>
                    </button>
                </header>

                <div class="settings-row">
                    <span class="settings-row-label">{t!(i18n, settings.regions_expand_on_hover)}</span>
                    <button
                        class="settings-check"
                        role="checkbox"
                        aria-checked=move || if regions_expand.get() { "true" } else { "false" }
                        aria-label=move || t_string!(i18n, settings.regions_expand_on_hover)
                        on:click=toggle_regions_expand
                    >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                            <path d="M4 12l5 5L20 6"></path>
                        </svg>
                    </button>
                </div>
            </section>
        </div>
    }
}

// The persistence layer splits by build: `hydrate` (the client) has a DOM and a real localStorage-backed
// store; `ssr` (the server) has neither, so it returns the component's defaults. Each build re-exports its
// own pair of accessors.
#[cfg(feature = "hydrate")]
pub use hydrate::regions_expand_on_hover;
#[cfg(feature = "hydrate")]
use hydrate::set_regions_expand_on_hover;
#[cfg(not(feature = "hydrate"))]
pub use ssr::regions_expand_on_hover;
#[cfg(not(feature = "hydrate"))]
use ssr::set_regions_expand_on_hover;

#[cfg(feature = "hydrate")]
mod hydrate {
    use base64::Engine;
    use shared::settings::{Setting, SettingKey, SettingValue, SettingsStore};

    /// The web client's settings keys. Platform-specific: a touch platform, which has no hover, would not
    /// define `RegionsExpandOnHover`.
    #[derive(Clone, Copy)]
    enum WebSettingKey {
        RegionsExpandOnHover,
    }

    impl SettingKey for WebSettingKey {
        fn storage_key(self) -> &'static str {
            match self {
                WebSettingKey::RegionsExpandOnHover => "regions-expand-on-hover",
            }
        }
    }

    const REGIONS_EXPAND_ON_HOVER: Setting<WebSettingKey, bool> =
        Setting::new(WebSettingKey::RegionsExpandOnHover, true);

    // localStorage holds strings, so each value's bytes are base64-encoded.
    struct LocalStorageStore;

    impl SettingsStore for LocalStorageStore {
        fn load<K: SettingKey>(&self, key: K) -> Option<SettingValue> {
            let storage: web_sys::Storage = local_storage()?;
            let encoded: String = storage.get_item(key.storage_key()).ok().flatten()?;
            let bytes: Vec<u8> = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;

            SettingValue::from_bytes(&bytes)
        }

        fn store<K: SettingKey>(&self, key: K, value: SettingValue) {
            let Some(storage) = local_storage()
            else {
                return;
            };
            let encoded: String = base64::engine::general_purpose::STANDARD.encode(value.to_bytes());

            let _ = storage.set_item(key.storage_key(), &encoded);
        }
    }

    fn local_storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }

    pub fn regions_expand_on_hover() -> bool {
        REGIONS_EXPAND_ON_HOVER.read(&LocalStorageStore)
    }

    pub fn set_regions_expand_on_hover(enabled: bool) {
        REGIONS_EXPAND_ON_HOVER.write(&LocalStorageStore, enabled);
        crate::map::canvas::driver::apply_regions_expand_on_hover(enabled);
    }
}

#[cfg(not(feature = "hydrate"))]
mod ssr {
    pub fn regions_expand_on_hover() -> bool {
        true
    }

    pub fn set_regions_expand_on_hover(_enabled: bool) {}
}
