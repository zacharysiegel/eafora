use chrono::{Datelike, NaiveDate};
use leptos::html::Aside;
use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::{DataSourceKind, DataStatus, SourceAttribution, StatisticKind};

use crate::i18n::*;
use crate::map::canvas::{
    BundleProseView, CellView, GlobalView, RankView, RegionDetail, SelectionView, SeriesPointView, SourceCellView,
};
use crate::map::labels;
use crate::map::scroll_thumb::{self, ScrollThumbState};

/// Births per woman at which a generation replaces itself. Drawn as the line a fertility series is read
/// against, so it is the one value on the chart that does not come from the data.
const REPLACEMENT_RATE: f64 = 2.1;

/// The chart's coordinate space. It scales to the dock's width through the `viewBox`, so these are
/// proportions of the drawing rather than pixels on screen.
const CHART_WIDTH: f64 = 320.0;
const CHART_HEIGHT: f64 = 142.6;
const PLOT_TOP: f64 = 8.0;
const PLOT_BOTTOM: f64 = 133.4;

/* A gutter either side of the plot: the unit's rotated title on the left, the reference line's value on the
   right. Equal widths, so the plot stays centred in the panel. The right label is anchored to the chart's edge
   rather than to the plot's, so a wider value encroaches on the gutter instead of being clipped away. */
const AXIS_GUTTER_WIDTH: f64 = 25.0;
const AXIS_LABEL_GAP: f64 = 4.0;
const PLOT_LEFT: f64 = AXIS_GUTTER_WIDTH;
const PLOT_RIGHT: f64 = CHART_WIDTH - AXIS_GUTTER_WIDTH;

/// Half the active period's marker height, so a marker on the first or last period sits inside the drawing
/// rather than half-clipped by its edge.
const MARKER_RADIUS: f64 = 4.0;

/// Headroom above and below the series, as a proportion of its extent, so a peak does not touch the top of
/// the plot.
const CHART_RANGE_MARGIN_PROPORTION: f64 = 0.08;

/// Half-extent of the range given to a series whose values are all equal, which has no extent of its own to
/// take a margin from.
const FLAT_SERIES_HALF_EXTENT: f64 = 0.5;

/* The `detail.change_over_*` labels state these intervals in words, so a label and its interval move
   together. */
const CHANGE_INTERVAL_IN_YEARS_SHORT: i32 = 1;
const CHANGE_INTERVAL_IN_YEARS_LONG: i32 = 10;

/// Which detail surface is up. Independent of the selection, so collapsing leaves the region selected and
/// outlined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailSurface {
    Summary,
    Expanded,
}

#[component]
pub fn RegionDetailPanel() -> impl IntoView {
    let selection: RwSignal<Option<SelectionView>> = expect_context();
    let global: RwSignal<Option<GlobalView>> = expect_context();
    let surface: RwSignal<DetailSurface> = expect_context();
    let prose: RwSignal<Option<BundleProseView>> = expect_context();
    let i18n = use_i18n();

    move || {
        let figure: ActiveFigure = active_figure(i18n, selection.get(), global.get())?;

        let view: AnyView = match surface.get() {
            DetailSurface::Summary => summary_panel(i18n, figure, surface).into_any(),
            DetailSurface::Expanded => detail_dock(i18n, figure, surface, prose.get()).into_any(),
        };

        Some(view)
    }
}

/// The figure both surfaces render, whichever of the two the driver published. The world is the figure
/// whenever no region is selected.
struct ActiveFigure {
    label: AnyView,
    statistic: StatisticKind,
    period_start: NaiveDate,
    cell: CellView,
    detail: RegionDetail,
}

fn active_figure(
    i18n: I18nContext<Locale>,
    selection: Option<SelectionView>,
    global: Option<GlobalView>,
) -> Option<ActiveFigure> {
    if let Some(selection) = selection {
        let SelectionView { region_code: _, name_en, statistic, period_start, cell, detail } = selection;

        return Some(ActiveFigure {
            label: name_en.into_any(),
            statistic,
            period_start,
            cell,
            detail,
        });
    }

    let GlobalView { statistic, period_start, cell, detail } = global?;

    Some(ActiveFigure {
        label: t!(i18n, detail.world).into_any(),
        statistic,
        period_start,
        cell,
        detail,
    })
}

/// The small top-left figure: the value and its source, and the control that expands to the dock.
fn summary_panel(
    i18n: I18nContext<Locale>,
    figure: ActiveFigure,
    surface: RwSignal<DetailSurface>,
) -> impl IntoView {
    let ActiveFigure { label, statistic, period_start, cell, detail: _ } = figure;
    let CellView { value, source, data_status } = cell;
    let status: Option<AnyView> = data_status.and_then(|data_status| status_label(i18n, data_status));

    view! {
        <aside class="panel detail-panel">
            <button
                class="button button-icon detail-panel-expand"
                type="button"
                aria-label=t_string!(i18n, detail.expand)
                on:click=move |_| surface.set(DetailSurface::Expanded)
            >
                {expand_icon()}
            </button>
            <p class="detail-panel-region">{label}</p>
            <p class="detail-panel-statistic">
                {labels::statistic_label(i18n, statistic)}
                " · "
                {period_start.year().to_string()}
            </p>
            {match value {
                Some(value) => view! {
                    <p class="detail-panel-value numeric">{format_value(value)}</p>
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

fn detail_dock(
    i18n: I18nContext<Locale>,
    figure: ActiveFigure,
    surface: RwSignal<DetailSurface>,
    prose: Option<BundleProseView>,
) -> impl IntoView {
    let ActiveFigure { label, statistic, period_start, cell, detail } = figure;
    let RegionDetail { series, sources, rank } = detail;
    let CellView { value, source: _, data_status } = cell;
    let status: Option<AnyView> = data_status.and_then(|data_status| status_label(i18n, data_status));
    let thumb: ScrollThumbState = scroll_thumb::create_state();
    let dock: NodeRef<Aside> = NodeRef::new();

    Effect::new(move |_| report_covered_surface(dock));
    on_cleanup(|| dispatch_left_surface_inset(0.0));

    view! {
        <aside class="panel region-dock" node_ref=dock>
        <div
            class="region-dock-scroll"
            node_ref=thumb.scroller()
            on:scroll=move |_| scroll_thumb::refresh(thumb)
            on:pointerenter=move |_| scroll_thumb::refresh(thumb)
        >
            <header class="region-dock-header">
                <h2 class="region-dock-region">{label}</h2>
                <button
                    class="button button-icon region-dock-collapse"
                    type="button"
                    aria-label=t_string!(i18n, detail.collapse)
                    on:click=move |_| surface.set(DetailSurface::Summary)
                >
                    {collapse_icon()}
                </button>
            </header>

            <p class="region-dock-statistic">
                {labels::statistic_label(i18n, statistic)}
                " · "
                {period_start.year().to_string()}
            </p>

            /* The dock keeps the value's block whether or not there is a value, so scrubbing across a gap in
               coverage does not move everything below it. */
            <p class="region-dock-value numeric">
                {match value {
                    Some(value) => format_value(value),
                    None => t_string!(i18n, detail.not_applicable).to_string(),
                }}
            </p>
            <p class="region-dock-unit">{labels::statistic_unit(i18n, statistic)}</p>
            {status.map(|status| view! {
                <p class="region-dock-status">{status}</p>
            })}

            {context_rows(i18n, &series, period_start, rank)}
            {history_section(i18n, statistic, &series, period_start)}
            {sources_section(i18n, &sources, prose.as_ref())}
            {about_section(i18n, statistic, prose.as_ref())}
        </div>
        {scroll_thumb::view(thumb)}
        </aside>
    }
}

/// The figures that put the primary value in proportion. A row whose comparison the shard does not cover
/// still renders, so the block keeps its shape as the reader scrubs.
fn context_rows(
    i18n: I18nContext<Locale>,
    series: &[SeriesPointView],
    active_period_start: NaiveDate,
    rank: Option<RankView>,
) -> AnyView {
    let not_applicable: String = t_string!(i18n, detail.not_applicable).to_string();

    let rank_value: String = match rank {
        Some(rank) => format!("{} / {}", rank.position, rank.of),
        None => not_applicable.clone(),
    };
    let short_change: Option<f64> = change_over_years(series, active_period_start, CHANGE_INTERVAL_IN_YEARS_SHORT);
    let long_change: Option<f64> = change_over_years(series, active_period_start, CHANGE_INTERVAL_IN_YEARS_LONG);

    view! {
        <dl class="region-dock-context">
            {context_row(t!(i18n, detail.rank).into_any(), rank_value)}
            {context_row(
                t!(i18n, detail.change_over_1_period).into_any(),
                format_change_or(short_change, &not_applicable),
            )}
            {context_row(
                t!(i18n, detail.change_over_10_periods).into_any(),
                format_change_or(long_change, &not_applicable),
            )}
        </dl>
    }
    .into_any()
}

fn context_row(label: AnyView, value: String) -> AnyView {
    view! {
        <div class="region-dock-context-row">
            <dt>{label}</dt>
            <dd class="numeric">{value}</dd>
        </div>
    }
    .into_any()
}

/// The difference between the active period's value and the one `years` earlier. `None` unless the shard
/// covers both, so a gap yields nothing rather than a difference over the wrong interval.
fn change_over_years(series: &[SeriesPointView], active_period_start: NaiveDate, years: i32) -> Option<f64> {
    let earlier_period_start: NaiveDate = active_period_start.with_year(active_period_start.year() - years)?;

    let active_value: f64 = value_at(series, active_period_start)?;
    let earlier_value: f64 = value_at(series, earlier_period_start)?;

    Some(active_value - earlier_value)
}

fn value_at(series: &[SeriesPointView], period_start: NaiveDate) -> Option<f64> {
    series
        .iter()
        .find(|point| point.period_start == period_start)
        .map(|point| point.value)
}

fn history_section(
    i18n: I18nContext<Locale>,
    statistic: StatisticKind,
    series: &[SeriesPointView],
    active_period_start: NaiveDate,
) -> AnyView {
    let heading: AnyView = view! {
        <h3 class="region-dock-heading">{t!(i18n, detail.history)}</h3>
    }
    .into_any();

    let Some(scale) = ChartScale::from_series(series, reference_value(statistic))
    else {
        return view! {
            {heading}
            <p class="region-dock-no-history">{t!(i18n, detail.no_history)}</p>
        }
        .into_any();
    };

    view! {
        {heading}
        {history_chart(i18n, statistic, series, active_period_start, &scale)}
        <p class="region-dock-chart-bounds">
            <span class="numeric">{scale.first_period_start.year().to_string()}</span>
            {labels::reference_caption_string(i18n, statistic).map(|caption| view! {
                <span class="region-dock-chart-reference-key">{caption}</span>
            })}
            <span class="numeric">{scale.last_period_start.year().to_string()}</span>
        </p>
    }
    .into_any()
}

/// The value a statistic's series is read against. Both fertility measures are births per woman, so both are
/// read against replacement.
fn reference_value(statistic: StatisticKind) -> Option<f64> {
    match statistic {
        StatisticKind::Tfr => Some(REPLACEMENT_RATE),
        StatisticKind::Ccf => Some(REPLACEMENT_RATE),
    }
}

fn history_chart(
    i18n: I18nContext<Locale>,
    statistic: StatisticKind,
    series: &[SeriesPointView],
    active_period_start: NaiveDate,
    scale: &ChartScale,
) -> AnyView {
    let polyline_points: String = scale.polyline_points(series);
    let active_marker: Option<ChartPoint> = value_at(series, active_period_start)
        .map(|value| scale.point(active_period_start, value));
    let reference: Option<(f64, f64)> = reference_value(statistic).map(|value| (value, scale.y(value)));
    let unit_label_x: f64 = PLOT_LEFT - AXIS_LABEL_GAP;
    let unit_label_y: f64 = (PLOT_TOP + PLOT_BOTTOM) / 2.0;

    view! {
        <svg
            class="region-dock-chart"
            viewBox=format!("0 0 {CHART_WIDTH} {CHART_HEIGHT}")
            aria-hidden="true"
        >
            <text
                class="region-dock-chart-unit"
                x=chart_unit(unit_label_x)
                y=chart_unit(unit_label_y)
                text-anchor="middle"
                transform=format!("rotate(-90, {unit_label_x}, {unit_label_y})")
            >
                {labels::statistic_unit_string(i18n, statistic)}
            </text>
            {reference.map(|(value, reference_y)| view! {
                <line
                    class="region-dock-chart-reference"
                    x1=chart_unit(PLOT_LEFT)
                    x2=chart_unit(PLOT_RIGHT)
                    y1=chart_unit(reference_y)
                    y2=chart_unit(reference_y)
                />
                <text
                    class="region-dock-chart-reference-value numeric"
                    x=chart_unit(CHART_WIDTH - AXIS_LABEL_GAP)
                    y=chart_unit(reference_y)
                    text-anchor="end"
                    dominant-baseline="middle"
                >
                    {format!("{value:.1}")}
                </text>
            })}
            <line
                class="region-dock-chart-baseline"
                x1=chart_unit(PLOT_LEFT)
                x2=chart_unit(PLOT_RIGHT)
                y1=chart_unit(PLOT_BOTTOM)
                y2=chart_unit(PLOT_BOTTOM)
            />
            <polyline class="region-dock-chart-line" points=polyline_points />
            {active_marker.map(|marker| view! {
                <circle
                    class="region-dock-chart-marker"
                    cx=chart_unit(marker.x)
                    cy=chart_unit(marker.y)
                    r=chart_unit(MARKER_RADIUS)
                />
            })}
        </svg>
    }
    .into_any()
}


/// An SVG attribute takes a string, and a numeric one handed to the view macro is written through a path that
/// Safari reports an error for on every rebuild. One decimal is finer than the chart can draw.
fn chart_unit(value: f64) -> String {
    format!("{value:.1}")
}

struct ChartPoint {
    x: f64,
    y: f64,
}

/// Maps a series onto the chart's coordinate space. A period is placed by its distance in time, not by its
/// index in the series, so a gap in coverage reads as a gap.
struct ChartScale {
    first_period_start: NaiveDate,
    last_period_start: NaiveDate,
    low: f64,
    high: f64,
}

impl ChartScale {
    /// `None` for a series confined to one period, which has no extent to scale against. `included_value`
    /// is held inside the vertical range even when the series never reaches it.
    fn from_series(series: &[SeriesPointView], included_value: Option<f64>) -> Option<ChartScale> {
        let first_period_start: NaiveDate = series.first()?.period_start;
        let last_period_start: NaiveDate = series.last()?.period_start;

        if first_period_start == last_period_start {
            return None;
        }

        let mut low: f64 = included_value.unwrap_or(f64::INFINITY);
        let mut high: f64 = included_value.unwrap_or(f64::NEG_INFINITY);

        for point in series {
            low = low.min(point.value);
            high = high.max(point.value);
        }

        let extent: f64 = high - low;
        let margin: f64 = if extent > 0.0 {
            extent * CHART_RANGE_MARGIN_PROPORTION
        } else {
            FLAT_SERIES_HALF_EXTENT
        };

        Some(ChartScale {
            first_period_start,
            last_period_start,
            low: low - margin,
            high: high + margin,
        })
    }

    fn x(&self, period_start: NaiveDate) -> f64 {
        let total_days: f64 = (self.last_period_start - self.first_period_start).num_days() as f64;
        let elapsed_days: f64 = (period_start - self.first_period_start).num_days() as f64;
        let plot_width: f64 = PLOT_RIGHT - PLOT_LEFT;

        PLOT_LEFT + elapsed_days / total_days * plot_width
    }

    fn y(&self, value: f64) -> f64 {
        let plot_height: f64 = PLOT_BOTTOM - PLOT_TOP;
        let proportion: f64 = (value - self.low) / (self.high - self.low);

        PLOT_BOTTOM - proportion.clamp(0.0, 1.0) * plot_height
    }

    fn point(&self, period_start: NaiveDate, value: f64) -> ChartPoint {
        ChartPoint {
            x: self.x(period_start),
            y: self.y(value),
        }
    }

    fn polyline_points(&self, series: &[SeriesPointView]) -> String {
        let coordinates: Vec<String> = series
            .iter()
            .map(|series_point| self.point(series_point.period_start, series_point.value))
            .map(|chart_point| format!("{:.1},{:.1}", chart_point.x, chart_point.y))
            .collect();

        coordinates.join(" ")
    }
}

/// Every source covering the active period, so a reader can see that sources disagree and by how much.
fn sources_section(i18n: I18nContext<Locale>, sources: &[SourceCellView], prose: Option<&BundleProseView>) -> AnyView {
    if sources.is_empty() {
        return ().into_any();
    }

    let is_contested: bool = sources.len() > 1;
    let rows: Vec<AnyView> = sources
        .iter()
        .map(|source_cell| {
            let attribution: Option<&SourceAttribution> =
                prose.and_then(|prose| prose.source_attribution.get(&source_cell.source));

            source_row(i18n, source_cell, is_contested, attribution)
        })
        .collect();

    view! {
        <h3 class="region-dock-heading">{t!(i18n, detail.sources)}</h3>
        <div class="region-dock-sources">{rows}</div>
    }
    .into_any()
}

/// `is_contested` gates the tag: with one source there is nothing for a priority to have decided.
fn source_row(
    i18n: I18nContext<Locale>,
    source_cell: &SourceCellView,
    is_contested: bool,
    attribution: Option<&SourceAttribution>,
) -> AnyView {
    let status: Option<AnyView> = status_label(i18n, source_cell.data_status);
    let is_tagged: bool = source_cell.is_preferred && is_contested;

    view! {
        <div class="region-dock-source-row">
            <span class="region-dock-source-name">
                {source_label(i18n, source_cell.source)}
                {is_tagged.then(|| view! {
                    <span class="tag tag-ink">{t!(i18n, detail.source_preferred)}</span>
                })}
            </span>
            <span class="region-dock-source-value numeric">{format_value(source_cell.value)}</span>
            {status.map(|status| view! {
                <span class="region-dock-source-status">{status}</span>
            })}
            {attribution.map(|attribution| attribution_lines(i18n, attribution))}
        </div>
    }
    .into_any()
}

/// The citation is rendered verbatim because the source's licence asks for exactly that string.
fn attribution_lines(i18n: I18nContext<Locale>, attribution: &SourceAttribution) -> AnyView {
    view! {
        <span class="region-dock-source-attribution">{attribution.attribution_text.clone()}</span>
        <span class="region-dock-source-links">
            <a href=attribution.license_url.clone() target="_blank" rel="noopener noreferrer">
                {attribution.license_name.clone()}
            </a>
            " · "
            <a href=attribution.homepage_url.clone() target="_blank" rel="noopener noreferrer">
                {t!(i18n, detail.homepage)}
            </a>
        </span>
    }
    .into_any()
}

/// The statistic's own definition, last, for the reader who wants to know what the figure above measures.
fn about_section(i18n: I18nContext<Locale>, statistic: StatisticKind, prose: Option<&BundleProseView>) -> AnyView {
    let Some(definition) = prose.and_then(|prose| prose.statistic_definitions.get(&statistic))
    else {
        return ().into_any();
    };

    view! {
        <h3 class="region-dock-heading">{t!(i18n, detail.about)}</h3>
        <p class="region-dock-about">{definition.description.clone()}</p>
    }
    .into_any()
}

fn format_value(value: f64) -> String {
    format!("{value:.2}")
}

/// Signed, since the reader is being shown a direction rather than a magnitude.
fn format_change_or(change: Option<f64>, absent: &str) -> String {
    match change {
        Some(change) => format!("{change:+.2}"),
        None => absent.to_string(),
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

/// The dock covers the map's left edge, and how much depends on its stylesheet, so it measures itself rather
/// than the camera assuming a width.
#[cfg(feature = "hydrate")]
fn report_covered_surface(dock: NodeRef<Aside>) {
    let Some(dock) = dock.get()
    else {
        return;
    };

    let device_pixel_ratio: f64 = window().device_pixel_ratio();

    dispatch_left_surface_inset(dock.get_bounding_client_rect().right() * device_pixel_ratio);
}

#[cfg(not(feature = "hydrate"))] // the ssr build has no laid-out element to measure
fn report_covered_surface(_dock: NodeRef<Aside>) {}

#[cfg(feature = "hydrate")]
fn dispatch_left_surface_inset(inset: f64) {
    crate::map::canvas::driver::apply_left_surface_inset(inset);
}

#[cfg(not(feature = "hydrate"))] // the ssr build has no driver to dispatch to
fn dispatch_left_surface_inset(_inset: f64) {}

/// Arrows toward opposite corners, and toward each other to collapse.
fn expand_icon() -> impl IntoView {
    view! {
        <svg class="icon" viewBox="0 0 14 14" aria-hidden="true">
            <path d="M8 2 H12 V6 M12 2 L8 6 M6 12 H2 V8 M2 12 L6 8" />
        </svg>
    }
}

fn collapse_icon() -> impl IntoView {
    view! {
        <svg class="icon" viewBox="0 0 14 14" aria-hidden="true">
            <path d="M12 6 H8 V2 M8 6 L12 2 M2 8 H6 V12 M6 8 L2 12" />
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series_of(points: &[(i32, f64)]) -> Vec<SeriesPointView> {
        points
            .iter()
            .map(|(year, value)| SeriesPointView {
                period_start: NaiveDate::from_ymd_opt(*year, 1, 1).unwrap(),
                value: *value,
            })
            .collect()
    }

    fn january(year: i32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, 1, 1).unwrap()
    }

    /// The interval is passed rather than read from the constants, so retuning which intervals the dock shows
    /// does not rewrite the lookup's test.
    #[test]
    fn change_over_years_reads_the_period_that_many_years_earlier() {
        let series: Vec<SeriesPointView> = series_of(&[(2014, 1.50), (2019, 1.60), (2024, 1.20)]);

        let change: f64 = change_over_years(&series, january(2024), 5).unwrap();

        assert!((change - -0.40).abs() < 1e-9);
    }

    #[test]
    fn change_over_years_is_absent_when_the_earlier_period_is_uncovered() {
        let series: Vec<SeriesPointView> = series_of(&[(2019, 1.60), (2024, 1.20)]);

        assert_eq!(change_over_years(&series, january(2024), 10), None);
    }

    #[test]
    fn format_change_or_signs_a_change_and_falls_back_when_absent() {
        assert_eq!(format_change_or(Some(-0.4), "N/A"), "-0.40");
        assert_eq!(format_change_or(Some(0.08), "N/A"), "+0.08");
        assert_eq!(format_change_or(None, "N/A"), "N/A");
    }

    #[test]
    fn chart_scale_is_absent_for_a_series_confined_to_one_period() {
        assert!(ChartScale::from_series(&series_of(&[(2024, 1.20)]), None).is_none());
    }

    #[test]
    fn chart_scale_spans_the_plot_from_the_first_period_to_the_last() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2012, 1.4), (2024, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, None).unwrap();

        assert_eq!(scale.x(january(2000)), PLOT_LEFT);
        assert_eq!(scale.x(january(2024)), PLOT_RIGHT);
    }

    /// A gap in coverage has to read as a gap, so an unevenly spaced period lands off the midpoint its index
    /// would have put it on.
    #[test]
    fn chart_scale_places_a_period_by_its_distance_in_time() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2018, 1.4), (2024, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, None).unwrap();

        let plot_midpoint: f64 = (PLOT_LEFT + PLOT_RIGHT) / 2.0;

        assert!(scale.x(january(2018)) > plot_midpoint);
    }

    #[test]
    fn chart_scale_holds_the_reference_line_inside_the_range() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2024, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, Some(REPLACEMENT_RATE)).unwrap();

        let reference_y: f64 = scale.y(REPLACEMENT_RATE);

        assert!(reference_y > PLOT_TOP);
        assert!(reference_y < PLOT_BOTTOM);
    }

    #[test]
    fn chart_scale_draws_a_flat_series_inside_the_plot() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.4), (2024, 1.4)]);
        let scale: ChartScale = ChartScale::from_series(&series, None).unwrap();

        let flat_y: f64 = scale.y(1.4);

        assert!(flat_y > PLOT_TOP);
        assert!(flat_y < PLOT_BOTTOM);
    }

    #[test]
    fn polyline_points_carries_one_coordinate_per_period() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2012, 1.4), (2024, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, None).unwrap();

        let points: String = scale.polyline_points(&series);

        assert_eq!(points.split(' ').count(), 3);
    }
}
