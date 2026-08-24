use leptos::prelude::*;

use crate::i18n::*;
use crate::map::canvas::{BundleProseView, GlobalView, LegendView, MapCanvas, SelectionView, ViewControls};
use crate::map::controls::Controls;
use crate::map::detail_panel::{DetailSurface, RegionDetailPanel};
use crate::map::escape::{self, DismissableSurfaces};
use crate::map::legend::Legend;
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

    let live_load_failed: RwSignal<bool> = RwSignal::new(false);
    provide_context(live_load_failed);

    let detail_surface: RwSignal<DetailSurface> = RwSignal::new(DetailSurface::Summary);
    provide_context(detail_surface);

    let bundle_prose: RwSignal<Option<BundleProseView>> = RwSignal::new(None);
    provide_context(bundle_prose);

    let settings_surface: RwSignal<SettingsSurface> = RwSignal::new(SettingsSurface::Closed);
    provide_context(settings_surface);

    escape::dismiss_on_escape(DismissableSurfaces {
        settings: settings_surface,
        detail: detail_surface,
    });

    let i18n = use_i18n();

    view! {
        <main id="map-view">
            <MapCanvas />
            <RegionDetailPanel />
            <Controls />
            <Legend />
            <SettingsModal />
            {move || if live_load_failed.get() {
                view! {
                    <div class="map-live-banner panel" role="status">{t!(i18n, live.load_failed)}</div>
                }
                .into_any()
            } else {
                ().into_any()
            }}
        </main>
    }
}
