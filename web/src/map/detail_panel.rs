use chrono::Datelike;
use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::{DataSourceKind, StatisticKind};

use crate::i18n::*;
use crate::map::canvas::{GlobalView, SelectionView};
use crate::map::labels;

#[component]
pub fn RegionDetailPanel() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = expect_context();
    let global: RwSignal<Option<GlobalView>> = expect_context();
    let i18n = use_i18n();

    move || match selection.get() {
        Some(selection_view) => {
            let SelectionView { region_code: _, name_en, statistic, period_start, value, source } = selection_view;

            Some(detail_panel(i18n, name_en.into_any(), statistic, period_start.year(), value, source))
        },
        None => global.get().map(|global_view| {
            let GlobalView { statistic, period_start, value, source } = global_view;

            detail_panel(i18n, t!(i18n, detail.world).into_any(), statistic, period_start.year(), value, source)
        }),
    }
}

fn detail_panel(
    i18n: I18nContext<Locale>,
    region_label: AnyView,
    statistic: StatisticKind,
    year: i32,
    value: Option<f64>,
    source: Option<DataSourceKind>,
) -> impl IntoView {
    view! {
        <aside class="panel detail-panel">
            <p class="detail-panel-region">{region_label}</p>
            <p class="detail-panel-statistic">
                {labels::statistic_label(i18n, statistic)}
                " · "
                {year.to_string()}
            </p>
            {match value {
                Some(value) => view! {
                    <p class="detail-panel-value numeric">{format!("{value:.2}")}</p>
                    <p class="detail-panel-unit">{labels::statistic_unit(i18n, statistic)}</p>
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
}

fn source_label(i18n: I18nContext<Locale>, source: DataSourceKind) -> AnyView {
    match source {
        DataSourceKind::WorldBankWDI => t!(i18n, source.wb_wdi).into_any(),
        // test-only variants; never present in production shards, so these arms only satisfy match exhaustiveness
        DataSourceKind::TestAlpha => source.code().into_any(),
        DataSourceKind::TestBeta => source.code().into_any(),
    }
}
