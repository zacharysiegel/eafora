use leptos::prelude::*;

use crate::map::canvas::{MapCanvas, SelectionView, ViewControls};
use crate::map::controls::Controls;
use crate::map::detail_panel::RegionDetailPanel;

#[component]
pub fn MapView() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = RwSignal::new(None);
    provide_context(selection);

    let view_controls: RwSignal<Option<ViewControls>> = RwSignal::new(None);
    provide_context(view_controls);

    view! {
        <main id="map-view">
            <MapCanvas />
            <RegionDetailPanel />
            <Controls />
        </main>
    }
}
