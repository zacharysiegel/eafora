use leptos::prelude::*;

use shared::map::color::{self, Rgba};

use crate::i18n::*;
use crate::map::canvas::LegendView;

const LEGEND_GRADIENT_STOPS: usize = 16;

#[component]
pub fn Legend() -> impl IntoView {
    let legend: RwSignal<Option<LegendView>> = expect_context();
    let i18n = use_i18n();

    move || {
        legend.get().map(|legend_view| {
            let LegendView { value_range } = legend_view;

            view! {
                <aside class="legend">
                    <span class="legend-title">{t!(i18n, legend.title)}</span>
                    {value_range.map(|(minimum, maximum)| view! {
                        <div class="legend-scale">
                            <div class="legend-gradient" style=gradient_css()></div>
                            <div class="legend-ticks">
                                <span class="legend-tick">
                                    <span class="legend-tick-caption">{t!(i18n, legend.low)}</span>
                                    <span class="legend-tick-value numeric">{format!("{minimum:.2}")}</span>
                                </span>
                                <span class="legend-tick legend-tick-end">
                                    <span class="legend-tick-caption">{t!(i18n, legend.high)}</span>
                                    <span class="legend-tick-value numeric">{format!("{maximum:.2}")}</span>
                                </span>
                            </div>
                        </div>
                    })}
                    <div class="legend-no-data">
                        <span class="legend-swatch" style=no_data_swatch_css()></span>
                        <span>{t!(i18n, legend.no_data)}</span>
                    </div>
                </aside>
            }
        })
    }
}

/// The choropleth scale sampled into a horizontal CSS gradient. Sampled at intervals rather than emitted
/// as a two-stop gradient so a nonlinear `CHOROPLETH_SCALE.interpolator` still renders faithfully.
fn gradient_css() -> String {
    let stops: String = (0..LEGEND_GRADIENT_STOPS)
        .map(|index| {
            let position: f32 = index as f32 / (LEGEND_GRADIENT_STOPS - 1) as f32;
            let color: Rgba = color::CHOROPLETH_SCALE.sample(position);

            format!("{} {:.2}%", css_rgb(color), position * 100.0)
        })
        .collect::<Vec<String>>()
        .join(", ");

    format!("background: linear-gradient(90deg, {stops});")
}

fn no_data_swatch_css() -> String {
    format!("background: {};", css_rgb(color::CHOROPLETH_SCALE.no_data()))
}

fn css_rgb(color: Rgba) -> String {
    let channel = |value: f32| (value * 255.0).round() as u8;

    format!("rgb({}, {}, {})", channel(color.r), channel(color.g), channel(color.b))
}
