use chrono::NaiveDate;
use leptos::html::Canvas;
use leptos::prelude::*;

use shared::canonical::{DataSourceKind, StatisticKind};

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

/// Published by the driver so a consumer can render the selection without bundle access.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionView {
    pub region_code: String,
    pub name_en: String,
    pub statistic: StatisticKind,
    pub period_start: NaiveDate,
    pub value: Option<f64>,
    pub source: Option<DataSourceKind>,
}

/// Published by the driver so a consumer can render the empty-state world figure without bundle access.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalView {
    pub statistic: StatisticKind,
    pub period_start: NaiveDate,
    pub value: Option<f64>,
    pub source: Option<DataSourceKind>,
}

/// Published by the driver so the controls render without bundle access.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewControls {
    pub active_statistic: StatisticKind,
    pub available_statistics: Vec<StatisticKind>,
    pub active_period_start: NaiveDate,
    pub period_range: Option<(NaiveDate, NaiveDate)>,
}

/// Published by the driver so the legend renders without bundle access.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendView {
    pub statistic: StatisticKind,
    pub value_range: Option<(f64, f64)>,
}

#[component]
pub fn MapCanvas() -> impl IntoView {
    let canvas_ref: NodeRef<Canvas> = NodeRef::new();
    let render_status: RwSignal<RenderStatus> = RwSignal::new(RenderStatus::Loading);

    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if let Some(canvas) = canvas_ref.get() {
            let selection: RwSignal<Option<SelectionView>> = expect_context();
            let global: RwSignal<Option<GlobalView>> = expect_context();
            let view_controls: RwSignal<Option<ViewControls>> = expect_context();
            let legend: RwSignal<Option<LegendView>> = expect_context();
            super::driver::start(canvas, super::driver::DriverSignals {
                render_status,
                selection_view: selection.write_only(),
                global_view: global.write_only(),
                view_controls: view_controls.write_only(),
                legend: legend.write_only(),
            });
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
