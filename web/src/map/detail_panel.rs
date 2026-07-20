use chrono::Datelike;
use leptos::prelude::*;

use shared::canonical::StatisticKind;

use crate::i18n::*;
use crate::map::canvas::SelectionView;

/// The top-left panel showing the selected region's name, the active statistic and period, and the
/// region's value at that cell. Renders nothing until a region is selected.
#[component]
pub fn RegionDetailPanel() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = expect_context();
    let i18n = use_i18n();

    move || {
        selection.get().map(|selection_view| {
            let SelectionView { iso3: _, name_en, statistic, period_start, value } = selection_view;

            view! {
                <aside class="panel detail-panel">
                    <h2 class="detail-panel-heading">{t!(i18n, detail.heading)}</h2>
                    <p class="detail-panel-region">{name_en}</p>
                    <p class="detail-panel-statistic">
                        {match statistic {
                            StatisticKind::Tfr => t!(i18n, statistic.tfr).into_any(),
                            // test-only variant; never active in production, so this arm only satisfies match exhaustiveness
                            StatisticKind::TestAlpha => statistic.code().into_any(),
                        }}
                        " · "
                        {period_start.year().to_string()}
                    </p>
                    {match value {
                        Some(value) => view! {
                            <p class="detail-panel-value numeric">{format!("{value:.2}")}</p>
                            <p class="detail-panel-unit">
                                {match statistic {
                                    StatisticKind::Tfr => t!(i18n, statistic.tfr_unit).into_any(),
                                    StatisticKind::TestAlpha => ().into_any(),
                                }}
                            </p>
                        }
                        .into_any(),
                        None => view! {
                            <p class="detail-panel-no-data">{t!(i18n, detail.no_data)}</p>
                        }
                        .into_any(),
                    }}
                </aside>
            }
        })
    }
}
