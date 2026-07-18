use leptos::prelude::*;

use crate::map::canvas::MapCanvas;

/// The map surface: a full-viewport canvas. The legend and controls chrome arrive in a later phase.
#[component]
pub fn MapView() -> impl IntoView {
    view! {
        <main id="map-view">
            <MapCanvas />
        </main>
    }
}
