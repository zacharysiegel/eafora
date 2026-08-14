use leptos::prelude::*;

use crate::map::canvas::{GlobalView, LegendView, MapCanvas, SelectionView, ViewControls};
use crate::map::controls::Controls;
use crate::map::detail_panel::RegionDetailPanel;
use crate::map::legend::Legend;
use crate::map::settings::SettingsModal;

#[component]
pub fn MapView() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = RwSignal::new(None);
    provide_context(selection);

    let view_controls: RwSignal<Option<ViewControls>> = RwSignal::new(None);
    provide_context(view_controls);

    let legend: RwSignal<Option<LegendView>> = RwSignal::new(None);
    provide_context(legend);

    let global: RwSignal<Option<GlobalView>> = RwSignal::new(None);
    provide_context(global);

    view! {
        <main id="map-view">
            <MapCanvas />
            <RegionDetailPanel />
            <Controls />
            <Legend />
            <SettingsModal />
        </main>
    }
}
