// Only the `dom` submodule reads the priority below, and the ssr build does not compile it.
#![cfg_attr(not(feature = "hydrate"), allow(dead_code))]

use leptos::prelude::*;

use crate::map::detail_panel::DetailSurface;
use crate::map::settings::SettingsSurface;

const ESCAPE_KEY: &str = "Escape";
const KEYDOWN_EVENT: &str = "keydown";

/// The surfaces Escape closes, in the order it closes them. One press closes only the topmost, so the surface
/// beneath it survives to be closed by the next.
#[derive(Clone, Copy)]
pub struct DismissableSurfaces {
    pub settings: RwSignal<SettingsSurface>,
    pub detail: RwSignal<DetailSurface>,
}

impl DismissableSurfaces {
    fn dismiss_topmost(&self) -> KeyDisposition {
        if self.settings.get_untracked() == SettingsSurface::Open {
            self.settings.set(SettingsSurface::Closed);

            return KeyDisposition::Consumed;
        }

        if self.detail.get_untracked() == DetailSurface::Expanded {
            self.detail.set(DetailSurface::Summary);

            return KeyDisposition::Consumed;
        }

        KeyDisposition::Ignored
    }
}

/// Whether the key belonged to a surface. An ignored key is left alone: with nothing open, Escape is not ours.
enum KeyDisposition {
    Consumed,
    Ignored,
}

#[cfg(feature = "hydrate")]
pub use dom::*;

#[cfg(feature = "hydrate")] // listens on the window
mod dom {
    use leptos::prelude::*;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    use super::{DismissableSurfaces, KeyDisposition, ESCAPE_KEY, KEYDOWN_EVENT};

    /// Installed once for the map view, in the capture phase, so a press that belongs to an open surface reaches
    /// nothing else. One handler rather than one per surface, because priority between them cannot be expressed
    /// by registration order.
    pub fn dismiss_on_escape(surfaces: DismissableSurfaces) {
        let handler: Closure<dyn FnMut(web_sys::KeyboardEvent)> = Closure::new(move |event: web_sys::KeyboardEvent| {
            if event.key() != ESCAPE_KEY {
                return;
            }

            let KeyDisposition::Consumed = surfaces.dismiss_topmost()
            else {
                return;
            };

            event.prevent_default();
            event.stop_propagation();
        });

        let listen_options: web_sys::AddEventListenerOptions = web_sys::AddEventListenerOptions::new();
        listen_options.set_capture(true);

        let _ = window().add_event_listener_with_callback_and_add_event_listener_options(
            KEYDOWN_EVENT,
            handler.as_ref().unchecked_ref(),
            &listen_options,
        );

        /* A JS closure is neither Send nor Sync and the cleanup must be both, so the store holds it: the handle
           is what the cleanup captures, and the closure stays alive as long as it is registered. */
        let registered: StoredValue<Closure<dyn FnMut(web_sys::KeyboardEvent)>, LocalStorage> =
            StoredValue::new_local(handler);

        on_cleanup(move || {
            let remove_options: web_sys::EventListenerOptions = web_sys::EventListenerOptions::new();
            remove_options.set_capture(true);

            registered.with_value(|handler| {
                let _ = window().remove_event_listener_with_callback_and_event_listener_options(
                    KEYDOWN_EVENT,
                    handler.as_ref().unchecked_ref(),
                    &remove_options,
                );
            });
        });
    }
}

#[cfg(not(feature = "hydrate"))]
pub use ssr::*;

#[cfg(not(feature = "hydrate"))] // the ssr build has no window to listen on
mod ssr {
    use super::DismissableSurfaces;

    pub fn dismiss_on_escape(_surfaces: DismissableSurfaces) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surfaces(settings: SettingsSurface, detail: DetailSurface) -> DismissableSurfaces {
        DismissableSurfaces {
            settings: RwSignal::new(settings),
            detail: RwSignal::new(detail),
        }
    }

    /// The whole point of closing one at a time: with both up, the first press leaves the dock for the second.
    #[test]
    fn dismiss_topmost_closes_the_settings_before_the_dock() {
        let _owner: Owner = Owner::new();
        _owner.set();
        let surfaces: DismissableSurfaces = surfaces(SettingsSurface::Open, DetailSurface::Expanded);

        assert!(matches!(surfaces.dismiss_topmost(), KeyDisposition::Consumed));
        assert_eq!(surfaces.settings.get_untracked(), SettingsSurface::Closed);
        assert_eq!(surfaces.detail.get_untracked(), DetailSurface::Expanded);

        assert!(matches!(surfaces.dismiss_topmost(), KeyDisposition::Consumed));
        assert_eq!(surfaces.detail.get_untracked(), DetailSurface::Summary);
    }

    #[test]
    fn dismiss_topmost_closes_the_dock_when_the_settings_are_shut() {
        let _owner: Owner = Owner::new();
        _owner.set();
        let surfaces: DismissableSurfaces = surfaces(SettingsSurface::Closed, DetailSurface::Expanded);

        assert!(matches!(surfaces.dismiss_topmost(), KeyDisposition::Consumed));
        assert_eq!(surfaces.detail.get_untracked(), DetailSurface::Summary);
    }

    #[test]
    fn dismiss_topmost_ignores_a_press_with_nothing_open() {
        let _owner: Owner = Owner::new();
        _owner.set();
        let surfaces: DismissableSurfaces = surfaces(SettingsSurface::Closed, DetailSurface::Summary);

        assert!(matches!(surfaces.dismiss_topmost(), KeyDisposition::Ignored));
    }
}
