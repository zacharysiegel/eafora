use chrono::Datelike;
use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::{DataSourceKind, DataStatus, StatisticKind};

use crate::i18n::*;
use crate::map::canvas::{CellView, GlobalView, SelectionView};
use crate::map::labels;

#[component]
pub fn RegionDetailPanel() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = expect_context();
    let global: RwSignal<Option<GlobalView>> = expect_context();
    let i18n = use_i18n();

    move || match selection.get() {
        Some(selection_view) => {
            let SelectionView { region_code: _, name_en, statistic, period_start, cell } = selection_view;

            Some(detail_panel(i18n, name_en.into_any(), statistic, period_start.year(), cell))
        },
        None => global.get().map(|global_view| {
            let GlobalView { statistic, period_start, cell } = global_view;

            detail_panel(i18n, t!(i18n, detail.world).into_any(), statistic, period_start.year(), cell)
        }),
    }
}

fn detail_panel(
    i18n: I18nContext<Locale>,
    region_label: AnyView,
    statistic: StatisticKind,
    year: i32,
    cell: CellView,
) -> impl IntoView {
    let CellView { value, source, data_status } = cell;
    let status: Option<AnyView> = data_status.and_then(|data_status| status_label(i18n, data_status));

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
                    {status.map(|status| view! {
                        <p class="detail-panel-status">{status}</p>
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

/// `None` for a confirmed figure, which qualifies nothing about the value above it.
fn status_label(i18n: I18nContext<Locale>, data_status: DataStatus) -> Option<AnyView> {
    match data_status {
        DataStatus::Final => None,
        DataStatus::Provisional => Some(t!(i18n, detail.status.provisional).into_any()),
        DataStatus::Preliminary => Some(t!(i18n, detail.status.preliminary).into_any()),
        DataStatus::Projection => Some(t!(i18n, detail.status.projection).into_any()),
        DataStatus::Imputed => Some(t!(i18n, detail.status.imputed).into_any()),
        DataStatus::Interpolated => Some(t!(i18n, detail.status.interpolated).into_any()),
    }
}

fn source_label(i18n: I18nContext<Locale>, source: DataSourceKind) -> AnyView {
    match source {
        DataSourceKind::WorldBankWDI => t!(i18n, source.wb_wdi).into_any(),
        DataSourceKind::HumanFertilityDatabase => t!(i18n, source.hfd).into_any(),
    }
}
