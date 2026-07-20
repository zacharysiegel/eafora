use leptos::prelude::*;

use crate::map::canvas::{MapCanvas, SelectionView};
use crate::map::detail_panel::RegionDetailPanel;

/// The map surface: the wgpu canvas and the region-detail panel. The legend and controls chrome
/// arrive in a later phase. The selection signal is owned here so the canvas (which writes it) and the
/// detail panel (which reads it) share one source through context.
#[component]
pub fn MapView() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = RwSignal::new(None);
    provide_context(selection);

    view! {
        <main id="map-view">
            <MapCanvas />
            <RegionDetailPanel />
        </main>
    }
}
