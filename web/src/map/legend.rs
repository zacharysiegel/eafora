use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::StatisticKind;
use shared::map::color::{self, ColorTransfer, Rgba};

use crate::i18n::*;
use crate::map::canvas::LegendView;
use crate::map::labels;

const LEGEND_GRADIENT_STOPS: usize = 24;

#[component]
pub fn Legend() -> impl IntoView {
    let legend: RwSignal<Option<LegendView>> = expect_context();
    let i18n = use_i18n();

    move || {
        legend.get().map(|legend_view| {
            let LegendView { statistic, value_range } = legend_view;

            view! {
                <aside class="legend">
                    <span class="legend-title">{labels::statistic_label(i18n, statistic)}</span>
                    {value_range.map(|(minimum, maximum)| legend_scale(i18n, statistic, minimum, maximum))}
                    <div class="legend-no-data">
                        <span class="legend-swatch" style=no_data_swatch_css()></span>
                        <span>{t!(i18n, legend.no_data)}</span>
                    </div>
                </aside>
            }
        })
    }
}

/// The gradient bar over `[minimum, maximum]`, colored through the statistic's transfer, with min /
/// inflection / max ticks. The inflection marker and its caption appear only when the transfer has an
/// inflection that falls inside the range (so `Linear` statistics get a plain min→max bar).
fn legend_scale(i18n: I18nContext<Locale>, statistic: StatisticKind, minimum: f64, maximum: f64) -> impl IntoView {
    let transfer: ColorTransfer = color::transfer_for(statistic);
    let inflection: Option<f64> = transfer
        .inflection()
        .filter(|value| (minimum..=maximum).contains(value));

    view! {
        <div class="legend-scale">
            <div class="legend-bar-wrap">
                <div class="legend-gradient" style=gradient_css(transfer, minimum, maximum)></div>
                {inflection.map(|value| view! {
                    <div class="legend-inflection" style=left_percent(value, minimum, maximum)></div>
                })}
            </div>
            <div class="legend-ticks">
                <span class="legend-tick legend-tick-start">
                    <span class="legend-tick-value numeric">{format!("{minimum:.1}")}</span>
                </span>
                {inflection.map(|value| view! {
                    <span class="legend-tick legend-tick-inflection" style=left_percent(value, minimum, maximum)>
                        <span class="legend-tick-value numeric">{format!("{value:.1}")}</span>
                        {labels::reference_caption(i18n, statistic).map(|caption| view! {
                            <span class="legend-tick-caption">{caption}</span>
                        })}
                    </span>
                })}
                <span class="legend-tick legend-tick-end">
                    <span class="legend-tick-value numeric">{format!("{maximum:.1}")}</span>
                </span>
            </div>
        </div>
    }
}

/// The choropleth scale across `[minimum, maximum]` as a horizontal CSS gradient, sampled at intervals
/// (not a two-stop gradient) so a nonlinear transfer renders faithfully. Each stop's color is the scale
/// sampled at the transfer's position for that value, so the bar matches what the map paints per value.
fn gradient_css(transfer: ColorTransfer, minimum: f64, maximum: f64) -> String {
    let stops: String = (0..LEGEND_GRADIENT_STOPS)
        .map(|index| {
            let fraction: f64 = index as f64 / (LEGEND_GRADIENT_STOPS - 1) as f64;
            let value: f64 = minimum + fraction * (maximum - minimum);
            let color: Rgba = color::CHOROPLETH_SCALE.sample(transfer.position(value, minimum, maximum));

            format!("{} {:.2}%", css_rgb(color), fraction * 100.0)
        })
        .collect::<Vec<String>>()
        .join(", ");

    format!("background: linear-gradient(90deg, {stops});")
}

/// The `left:` CSS for a value's position along a bar spanning `[minimum, maximum]` linearly in value.
fn left_percent(value: f64, minimum: f64, maximum: f64) -> String {
    let fraction: f64 = if maximum > minimum {
        (value - minimum) / (maximum - minimum)
    } else {
        0.0
    };

    format!("left: {:.2}%;", fraction * 100.0)
}

fn no_data_swatch_css() -> String {
    format!("background: {};", css_rgb(color::CHOROPLETH_SCALE.no_data()))
}

fn css_rgb(color: Rgba) -> String {
    let channel = |component: f32| (component * 255.0).round() as u8;

    format!("rgb({}, {}, {})", channel(color.r), channel(color.g), channel(color.b))
}
