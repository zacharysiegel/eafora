use leptos::html::Canvas;
use leptos::prelude::*;

use crate::i18n::*;

/// First-paint lifecycle of the map canvas. Server-side rendering leaves it at `Loading`, since the
/// renderer only runs client-side.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))] // the ssr build never runs the renderer, so it constructs only Loading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStatus {
    /// Shown until the embedded bundle is parsed and the surface is attached.
    Loading,
    Ready,
    /// The browser lacks a hard capability: no OPFS or no usable wgpu backend.
    Unsupported,
    /// The bundle could not be fetched or opened.
    DataUnavailable,
}

/// A resolved, display-ready view of the selected region; the driver publishes it so a consumer needs
/// no bundle access to render the selection.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))] // the ssr build never runs the driver, so nothing publishes a SelectionView
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionView {
    pub iso3: String,
    pub name_en: String,
    pub value: Option<f64>,
}

#[component]
pub fn MapCanvas() -> impl IntoView {
    let canvas_ref: NodeRef<Canvas> = NodeRef::new();
    let render_status: RwSignal<RenderStatus> = RwSignal::new(RenderStatus::Loading);

    #[cfg(feature = "hydrate")]
    let selection: RwSignal<Option<SelectionView>> = RwSignal::new(None);
    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if let Some(canvas) = canvas_ref.get() {
            super::driver::start(canvas, render_status, selection.write_only());
        }
    });

    let i18n = use_i18n();

    view! {
        <canvas node_ref=canvas_ref id="map-canvas"></canvas>
        {move || match render_status.get() {
            RenderStatus::Ready => ().into_any(),
            RenderStatus::Loading => view! {
                <div class="map-overlay panel"><p>{t!(i18n, controls.loading)}</p></div>
            }
            .into_any(),
            RenderStatus::Unsupported => view! {
                <div class="map-overlay panel"><p>{t!(i18n, map.unsupported)}</p></div>
            }
            .into_any(),
            RenderStatus::DataUnavailable => view! {
                <div class="map-overlay panel"><p>{t!(i18n, map.data_unavailable)}</p></div>
            }
            .into_any(),
        }}
    }
}
