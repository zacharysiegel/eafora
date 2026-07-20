use chrono::{Datelike, NaiveDate};
use leptos::prelude::*;

use shared::canonical::StatisticKind;

use crate::i18n::*;
use crate::map::canvas::ViewControls;
use crate::map::labels;

#[component]
pub fn Controls() -> impl IntoView {
    let view_controls: RwSignal<Option<ViewControls>> = expect_context();
    let i18n = use_i18n();

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
                                    forward_statistic(statistic);
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
                    {period_range.map(|(earliest, latest)| view! {
                        <label class="controls-field">
                            <span class="controls-label">{t!(i18n, scrubber.label)}</span>
                            <input
                                class="controls-scrubber"
                                type="range"
                                min=earliest.year()
                                max=latest.year()
                                value=active_period_start.year()
                                on:input=move |event| {
                                    let Ok(year) = event_target_value(&event).parse::<i32>() else {
                                        return;
                                    };
                                    if let Some(period_start) = NaiveDate::from_ymd_opt(year, 1, 1) {
                                        forward_period(period_start);
                                    }
                                }
                            />
                            <span class="controls-scrubber-year numeric">{active_period_start.year().to_string()}</span>
                        </label>
                    })}
                </aside>
            }
        })
    }
}

#[cfg(feature = "hydrate")]
fn forward_statistic(statistic: StatisticKind) {
    crate::map::canvas::apply_statistic(statistic);
}

#[cfg(not(feature = "hydrate"))] // the ssr build has no driver to forward to
fn forward_statistic(_statistic: StatisticKind) {}

#[cfg(feature = "hydrate")]
fn forward_period(period_start: NaiveDate) {
    crate::map::canvas::apply_period(period_start);
}

#[cfg(not(feature = "hydrate"))] // the ssr build has no driver to forward to
fn forward_period(_period_start: NaiveDate) {}
