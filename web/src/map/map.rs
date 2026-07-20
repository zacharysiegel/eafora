use leptos::prelude::*;

use crate::map::canvas::{MapCanvas, SelectionView};
use crate::map::detail_panel::RegionDetailPanel;

/// The map surface: the wgpu canvas and the region-detail panel.
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
