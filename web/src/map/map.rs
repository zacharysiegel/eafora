use std::collections::BTreeMap;

use leptos::prelude::*;

use shared::canonical::{DataSourceKind, SourceAttribution};

use crate::map::canvas::{GlobalView, LegendView, MapCanvas, SelectionView, ViewControls};
use crate::map::controls::Controls;
use crate::map::detail_panel::{DetailSurface, RegionDetailPanel};
use crate::map::escape::{self, DismissableSurfaces};
use crate::map::legend::Legend;
use crate::map::live_banner::LiveBanner;
use crate::map::settings::{SettingsModal, SettingsSurface};

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

    let live_load_notice_shown: RwSignal<bool> = RwSignal::new(false);
    provide_context(live_load_notice_shown);

    let detail_surface: RwSignal<DetailSurface> = RwSignal::new(DetailSurface::Summary);
    provide_context(detail_surface);

    let source_attribution: RwSignal<BTreeMap<DataSourceKind, SourceAttribution>> =
        RwSignal::new(BTreeMap::new());
    provide_context(source_attribution);

    let settings_surface: RwSignal<SettingsSurface> = RwSignal::new(SettingsSurface::Closed);
    provide_context(settings_surface);

    escape::dismiss_on_escape(DismissableSurfaces {
        settings: settings_surface,
        detail: detail_surface,
    });

    view! {
        <main id="map-view">
            <MapCanvas />
            <RegionDetailPanel />
            <Controls />
            <Legend />
            <SettingsModal />
            <LiveBanner />
        </main>
    }
}
