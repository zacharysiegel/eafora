use chrono::Datelike;
use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::{DataSourceKind, StatisticKind};

use crate::i18n::*;
use crate::map::canvas::SelectionView;

#[component]
pub fn RegionDetailPanel() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = expect_context();
    let i18n = use_i18n();

    move || {
        selection.get().map(|selection_view| {
            let SelectionView { iso3: _, name_en, statistic, period_start, value, source } = selection_view;

            view! {
                <aside class="panel detail-panel">
                    <h2 class="detail-panel-heading">{t!(i18n, detail.heading)}</h2>
                    <p class="detail-panel-region">{name_en}</p>
                    <p class="detail-panel-statistic">
                        {statistic_label(i18n, statistic)}
                        " · "
                        {period_start.year().to_string()}
                    </p>
                    {match value {
                        Some(value) => view! {
                            <p class="detail-panel-value numeric">{format!("{value:.2}")}</p>
                            <p class="detail-panel-unit">{statistic_unit(i18n, statistic)}</p>
                            {source.map(|source| view! {
                                <p class="detail-panel-source">{t!(i18n, detail.source)} ": " {source_label(i18n, source)}</p>
                            })}
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

fn statistic_label(i18n: I18nContext<Locale>, statistic: StatisticKind) -> AnyView {
    match statistic {
        StatisticKind::Tfr => t!(i18n, statistic.tfr).into_any(),
        // test-only variant; never active in production, so this arm only satisfies match exhaustiveness
        StatisticKind::TestAlpha => statistic.code().into_any(),
    }
}

fn statistic_unit(i18n: I18nContext<Locale>, statistic: StatisticKind) -> AnyView {
    match statistic {
        StatisticKind::Tfr => t!(i18n, statistic.tfr_unit).into_any(),
        StatisticKind::TestAlpha => ().into_any(),
    }
}

fn source_label(i18n: I18nContext<Locale>, source: DataSourceKind) -> AnyView {
    match source {
        DataSourceKind::WorldBankWDI => t!(i18n, source.wb_wdi).into_any(),
        // test-only variants; never present in production shards, so these arms only satisfy match exhaustiveness
        DataSourceKind::TestAlpha => source.code().into_any(),
        DataSourceKind::TestBeta => source.code().into_any(),
    }
}
