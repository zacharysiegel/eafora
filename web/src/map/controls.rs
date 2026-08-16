use chrono::{Datelike, NaiveDate};
use leptos::prelude::*;

use shared::canonical::StatisticKind;

use crate::i18n::*;
use crate::map::canvas::ViewControls;
use crate::map::labels;

/// Ties the `YEAR` label to the editable year, which the range input cannot also claim; the range carries
/// its own `aria-label` instead. One controls panel exists, so a fixed id is unambiguous.
const YEAR_INPUT_ID: &str = "controls-year-input";

#[component]
pub fn Controls() -> impl IntoView {
    let view_controls: RwSignal<Option<ViewControls>> = expect_context();
    let i18n = use_i18n();
    let grabbing: RwSignal<bool> = RwSignal::new(false);

    move || {
        view_controls.get().map(|controls| {
            let ViewControls { active_statistic, available_statistics, active_period_start, period_range } = controls;

            view! {
                <aside class="panel controls">
                    <label class="controls-field">
                        <span class="controls-label">{t!(i18n, statistic.picker_label)}</span>
                        <select
                            class="controls-picker"
                            on:change=move |event| {
                                if let Ok(statistic) = StatisticKind::try_from(event_target_value(&event).as_str()) {
                                    dispatch_statistic(statistic);
                                }
                            }
                        >
                            {available_statistics
                                .into_iter()
                                .map(|statistic| {
                                    let is_active: bool = statistic == active_statistic;
                                    view! {
                                        <option value=statistic.code() selected=is_active>
                                            {labels::statistic_label(i18n, statistic)}
                                        </option>
                                    }
                                })
                                .collect_view()}
                        </select>
                    </label>
                    {period_range.map(|(earliest, latest)| {
                        let earliest_year: i32 = earliest.year();
                        let latest_year: i32 = latest.year();
                        let active_year: i32 = active_period_start.year();
                        let thumb_proportion: f64 = thumb_proportion(active_year, earliest_year, latest_year);

                        view! {
                            <div class="controls-field">
                                <label class="controls-label" for=YEAR_INPUT_ID>{t!(i18n, scrubber.label)}</label>
                                <div class="controls-scrubber-row">
                                    {bound_label(earliest_year, active_year)}
                                    <div class="controls-scrubber-track">
                                        <input
                                            class="controls-scrubber"
                                            class:grabbing=move || grabbing.get()
                                            type="range"
                                            aria-label=move || t_string!(i18n, scrubber.label)
                                            min=earliest_year
                                            max=latest_year
                                            value=active_year
                                            // Patching min / max re-runs the browser's value sanitization, which
                                            // clamps a dirty value against the momentarily-default bounds; the
                                            // property has to be re-asserted or the thumb snaps to the minimum.
                                            prop:value=active_year
                                            on:input=move |event| apply_year(&event_target_value(&event))
                                            on:pointerdown=move |_| grabbing.set(true)
                                            on:pointerup=move |_| grabbing.set(false)
                                            on:pointercancel=move |_| grabbing.set(false)
                                        />
                                        <input
                                            class="controls-year numeric"
                                            id=YEAR_INPUT_ID
                                            style=format!("--thumb-proportion: {thumb_proportion}")
                                            type="number"
                                            min=earliest_year
                                            max=latest_year
                                            value=active_year
                                            prop:value=active_year
                                            on:change=move |event| apply_year(&event_target_value(&event))
                                        />
                                    </div>
                                    {bound_label(latest_year, active_year)}
                                </div>
                            </div>
                        }
                    })}
                </aside>
            }
        })
    }
}

/// One endpoint of the scrubber's range. Rendered greyed when the active year already reads it, so the same
/// number is not stated twice. Hidden from assistive technology, which already gets the range from the
/// slider's own minimum and maximum.
fn bound_label(bound_year: i32, active_year: i32) -> impl IntoView {
    view! {
        <span
            class="controls-bound numeric"
            class:equals-active=move || bound_year == active_year
            aria-hidden="true"
        >
            {bound_year.to_string()}
        </span>
    }
}

/// Where `active_year` sits within the range, as a proportion for the scrubber's thumb offset. A range
/// covering a single year has no interior to place anything in, so it pins to the start.
fn thumb_proportion(active_year: i32, earliest_year: i32, latest_year: i32) -> f64 {
    let span: i32 = latest_year - earliest_year;
    if span <= 0 {
        return 0.0;
    }

    let offset: f64 = (active_year.clamp(earliest_year, latest_year) - earliest_year) as f64;

    offset / span as f64
}

fn apply_year(year_text: &str) {
    let Ok(year) = year_text.parse::<i32>() else {
        return;
    };
    if let Some(period_start) = NaiveDate::from_ymd_opt(year, 1, 1) {
        dispatch_period(period_start);
    }
}

#[cfg(feature = "hydrate")]
fn dispatch_statistic(statistic: StatisticKind) {
    crate::map::canvas::driver::apply_statistic(statistic);
}

#[cfg(not(feature = "hydrate"))] // the ssr build has no driver to dispatch to
fn dispatch_statistic(_statistic: StatisticKind) {}

#[cfg(feature = "hydrate")]
fn dispatch_period(period_start: NaiveDate) {
    crate::map::canvas::driver::apply_period(period_start);
}

#[cfg(not(feature = "hydrate"))] // the ssr build has no driver to dispatch to
fn dispatch_period(_period_start: NaiveDate) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_proportion_pins_a_single_year_range_to_the_start() {
        assert_eq!(thumb_proportion(2024, 2024, 2024), 0.0);
    }

    #[test]
    fn thumb_proportion_spans_the_endpoints() {
        assert_eq!(thumb_proportion(1960, 1960, 2024), 0.0);
        assert_eq!(thumb_proportion(2024, 1960, 2024), 1.0);
    }

    #[test]
    fn thumb_proportion_places_the_midpoint_halfway() {
        assert_eq!(thumb_proportion(1990, 1980, 2000), 0.5);
    }

    #[test]
    fn thumb_proportion_clamps_a_year_outside_the_range() {
        assert_eq!(thumb_proportion(1950, 1960, 2024), 0.0);
        assert_eq!(thumb_proportion(2030, 1960, 2024), 1.0);
    }
}
