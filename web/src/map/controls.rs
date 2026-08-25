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

    /* Every attribute reads its own `Memo` so a publish patches only what changed. Re-rendering the
    elements instead strips and re-adds each attribute, and a range input whose `type` is removed rebuilds
    its shadow slider, destroying the pointer capture an in-progress drag depends on. `Memo` notifies only
    on inequality, so the wrapper closures below re-run when a range appears or disappears, never on a
    year change. */
    let has_controls: Memo<bool> = Memo::new(move |_| view_controls.get().is_some());
    let available_statistics: Memo<Vec<StatisticKind>> = Memo::new(move |_| {
        view_controls
            .get()
            .map(|controls| controls.available_statistics)
            .unwrap_or_default()
    });
    let active_statistic: Memo<Option<StatisticKind>> =
        Memo::new(move |_| view_controls.get().map(|controls| controls.active_statistic));
    let period_range: Memo<Option<(NaiveDate, NaiveDate)>> =
        Memo::new(move |_| view_controls.get().and_then(|controls| controls.period_range));
    let has_period_range: Memo<bool> = Memo::new(move |_| period_range.get().is_some());
    let earliest_year: Memo<Option<i32>> =
        Memo::new(move |_| period_range.get().map(|(earliest, _latest)| earliest.year()));
    let latest_year: Memo<Option<i32>> =
        Memo::new(move |_| period_range.get().map(|(_earliest, latest)| latest.year()));
    let active_year: Memo<Option<i32>> =
        Memo::new(move |_| view_controls.get().map(|controls| controls.active_period_start.year()));
    let thumb_offset: Memo<f64> = Memo::new(move |_| {
        match (active_year.get(), earliest_year.get(), latest_year.get()) {
            (Some(active), Some(earliest), Some(latest)) => thumb_proportion(active, earliest, latest),
            _ => 0.0,
        }
    });
    let active_period_end: Memo<Option<NaiveDate>> =
        Memo::new(move |_| view_controls.get().and_then(|controls| controls.active_period_end));
    let span_width: Memo<f64> = Memo::new(move |_| {
        match (active_year.get(), earliest_year.get(), latest_year.get()) {
            (Some(active), Some(earliest), Some(latest)) => {
                span_proportion(active, active_period_end.get(), earliest, latest)
            }
            _ => 0.0,
        }
    });

    view! {
        {move || has_controls.get().then(|| view! {
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
                        {move || available_statistics
                            .get()
                            .into_iter()
                            .map(|statistic| view! {
                                <option
                                    value=statistic.code()
                                    selected=move || active_statistic.get() == Some(statistic)
                                >
                                    {labels::statistic_label(i18n, statistic)}
                                </option>
                            })
                            .collect_view()}
                    </select>
                </label>
                {move || has_period_range.get().then(|| view! {
                    <div class="controls-field">
                        <label class="controls-label" for=YEAR_INPUT_ID>
                            {move || active_statistic.get().map(|statistic| labels::period_axis_label(i18n, statistic))}
                        </label>
                        <div class="controls-scrubber-row">
                            {bound_label(earliest_year, active_year)}
                            <div class="controls-scrubber-track">
                                <span
                                    class="controls-span"
                                    style=move || format!(
                                        "--thumb-proportion: {}; --span-proportion: {}",
                                        thumb_offset.get(),
                                        span_width.get(),
                                    )
                                    aria-hidden="true"
                                ></span>
                                <input
                                    class="controls-scrubber"
                                    class:grabbing=move || grabbing.get()
                                    type="range"
                                    aria-label=move || active_statistic
                                        .get()
                                        .map(|statistic| labels::period_axis_label(i18n, statistic))
                                    min=move || earliest_year.get()
                                    max=move || latest_year.get()
                                    value=move || active_year.get()
                                    prop:value=move || active_year.get()
                                    on:input=move |event| apply_year(&event, earliest_year.get(), latest_year.get())
                                    on:pointerdown=move |_| grabbing.set(true)
                                    on:pointerup=move |_| grabbing.set(false)
                                    on:pointercancel=move |_| grabbing.set(false)
                                    // A release outside the input leaves no pointerup here, so the enlarged
                                    // thumb would stay enlarged; losing capture ends the grab either way.
                                    on:lostpointercapture=move |_| grabbing.set(false)
                                />
                                <input
                                    class="controls-year numeric"
                                    id=YEAR_INPUT_ID
                                    style=move || format!("--thumb-proportion: {}", thumb_offset.get())
                                    type="number"
                                    min=move || earliest_year.get()
                                    max=move || latest_year.get()
                                    value=move || active_year.get()
                                    prop:value=move || active_year.get()
                                    on:change=move |event| apply_year(&event, earliest_year.get(), latest_year.get())
                                />
                            </div>
                            {bound_label(latest_year, active_year)}
                        </div>
                    </div>
                })}
            </aside>
        })}
    }
}

/// One endpoint of the scrubber's range. Rendered greyed when the active year already reads it, so the same
/// number is not stated twice. Hidden from assistive technology, which already gets the range from the
/// slider's own minimum and maximum.
fn bound_label(bound_year: Memo<Option<i32>>, active_year: Memo<Option<i32>>) -> impl IntoView {
    view! {
        <span
            class="controls-bound numeric"
            class:equals-active=move || bound_year.get().is_some() && bound_year.get() == active_year.get()
            aria-hidden="true"
        >
            {move || bound_year.get().map(|year| year.to_string())}
        </span>
    }
}

/// How much of the axis the active period covers, as a proportion, so the value reads as the span it
/// describes rather than as an instant. `period_end` is exclusive, so a period ending 2025-01-01 covers the
/// single year 2024 and one axis step.
fn span_proportion(
    active_year: i32,
    period_end: Option<NaiveDate>,
    earliest_year: i32,
    latest_year: i32,
) -> f64 {
    let axis_years: i32 = latest_year - earliest_year;
    if axis_years <= 0 {
        return 1.0;
    }

    let period_years: i32 = match period_end {
        Some(period_end) => (period_end.year() - active_year).max(1),
        None => 1,
    };

    period_years as f64 / axis_years as f64
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

fn apply_year(event: &leptos::ev::Event, earliest_year: Option<i32>, latest_year: Option<i32>) {
    let typed_text: String = event_target_value(event);
    let Some(year) = clamped_year(&typed_text, earliest_year, latest_year) else {
        return;
    };

    /* Clamping to the year already active leaves the driver's state unchanged, so nothing would reassert
       the field and it would keep showing the out-of-range number that was typed. */
    if year.to_string() != typed_text {
        overwrite_year_field(event, year);
    }

    if let Some(period_start) = NaiveDate::from_ymd_opt(year, 1, 1) {
        dispatch_period(period_start);
    }
}

/// The typed year held inside the range the bundle covers, or `None` when the text is not a year. The
/// input's `min` and `max` bound its steppers and its validity, neither of which stops a typed value.
fn clamped_year(year_text: &str, earliest_year: Option<i32>, latest_year: Option<i32>) -> Option<i32> {
    let year: i32 = year_text
        .trim()
        .parse::<i32>()
        .ok()?;

    match (earliest_year, latest_year) {
        (Some(earliest), Some(latest)) => Some(year.clamp(earliest, latest)),
        _ => Some(year),
    }
}

#[cfg(feature = "hydrate")] // writes to the DOM element the event came from
fn overwrite_year_field(event: &leptos::ev::Event, year: i32) {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlInputElement;

    let field: Option<HtmlInputElement> = event
        .target()
        .and_then(|target| target.dyn_into::<HtmlInputElement>().ok());

    if let Some(field) = field {
        field.set_value(&year.to_string());
    }
}

#[cfg(not(feature = "hydrate"))] // no DOM to write to
fn overwrite_year_field(_event: &leptos::ev::Event, _year: i32) {}

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
    fn clamped_year_holds_a_typed_year_inside_the_covered_range() {
        assert_eq!(clamped_year("1800", Some(1960), Some(2024)), Some(1960));
        assert_eq!(clamped_year("3000", Some(1960), Some(2024)), Some(2024));
        assert_eq!(clamped_year("1990", Some(1960), Some(2024)), Some(1990));
        assert_eq!(clamped_year("1960", Some(1960), Some(2024)), Some(1960));
        assert_eq!(clamped_year("2024", Some(1960), Some(2024)), Some(2024));
    }

    #[test]
    fn clamped_year_rejects_text_that_is_not_a_year() {
        assert_eq!(clamped_year("", Some(1960), Some(2024)), None);
        assert_eq!(clamped_year("nineteen", Some(1960), Some(2024)), None);
        assert_eq!(clamped_year("19 90", Some(1960), Some(2024)), None);
    }

    #[test]
    fn clamped_year_passes_a_year_through_when_no_range_is_known() {
        assert_eq!(clamped_year("1800", None, None), Some(1800));
        assert_eq!(clamped_year("1800", Some(1960), None), Some(1800));
    }

    fn january(year: i32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(year, 1, 1)
    }

    #[test]
    fn span_proportion_covers_one_axis_step_for_a_single_year_period() {
        assert_eq!(span_proportion(2000, january(2001), 1960, 2020), 1.0 / 60.0);
    }

    #[test]
    fn span_proportion_widens_with_a_multi_year_period() {
        assert_eq!(span_proportion(1950, january(1955), 1900, 2000), 5.0 / 100.0);
    }

    /// A period with no known ending still reads as one axis step, never as a point.
    #[test]
    fn span_proportion_covers_one_step_when_the_ending_is_unknown() {
        assert_eq!(span_proportion(2000, None, 1960, 2020), 1.0 / 60.0);
    }

    #[test]
    fn span_proportion_fills_a_single_year_axis() {
        assert_eq!(span_proportion(2024, january(2025), 2024, 2024), 1.0);
    }

    /// An ending at or before the period's own year would otherwise vanish the span.
    #[test]
    fn span_proportion_never_falls_below_one_step() {
        assert_eq!(span_proportion(2000, january(2000), 1960, 2020), 1.0 / 60.0);
        assert_eq!(span_proportion(2000, january(1990), 1960, 2020), 1.0 / 60.0);
    }

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
