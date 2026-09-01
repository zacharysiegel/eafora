use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use leptos::html::{Aside, Button};
use leptos::prelude::*;
use leptos_i18n::I18nContext;

use shared::canonical::{DataSourceKind, DataStatus, SourceAttribution, StatisticKind};

use crate::i18n::*;
use crate::map::canvas::{
    self, CellView, GlobalView, RankView, RegionDetail, SelectionView, SeriesPointView, SourceCellView,
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
   right. Equal widths, so the plot stays centred in the panel. Both labels are anchored to the plot's edge and
   grow away from it, so the gap holds whatever the label says and a long one runs out of the chart rather than
   over the series. */
const AXIS_GUTTER_WIDTH: f64 = 27.0;
/* The unit's title is rotated, so it needs only the height of its type and can stand further off the plot than
   the value on the right, which needs the width of its digits. */
const UNIT_LABEL_GAP: f64 = 8.0;
const REFERENCE_LABEL_GAP: f64 = 6.0;
const PLOT_LEFT: f64 = AXIS_GUTTER_WIDTH;
const PLOT_RIGHT: f64 = CHART_WIDTH - AXIS_GUTTER_WIDTH;

/// Half the active period's marker height, so a marker on the first or last period sits inside the drawing
/// rather than half-clipped by its edge.
const MARKER_RADIUS: f64 = 4.0;

/// The readout's box height, which the view reserves whether or not a readout is showing.
const READOUT_HEIGHT: f64 = 15.0;

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
    let attribution: RwSignal<BTreeMap<DataSourceKind, SourceAttribution>> = expect_context();
    let i18n = use_i18n();

    let figure: Memo<Option<ActiveFigure>> = Memo::new(move |_| active_figure(i18n, selection.get(), global.get()));
    /* Whether the expand control was reached by keyboard, which decides whether the dock takes focus: moving
       focus for a mouse click puts a focus ring on a control the reader did not ask to be on. */
    let expanded_by_keyboard: RwSignal<bool> = RwSignal::new(false);

    /* Each surface is built once, when it becomes the one showing, and its values then update through the
       closures below. Reading the figure here instead would rebuild the whole panel on every republish, which
       means rebuilding the chart's elements on every scrub tick. */
    view! {
        <Show when=move || surface.get() == DetailSurface::Summary && figure.with(Option::is_some)>
            {summary_panel(i18n, figure, surface, expanded_by_keyboard)}
        </Show>
        <Show when=move || surface.get() == DetailSurface::Expanded && figure.with(Option::is_some)>
            {detail_dock(i18n, figure, surface, attribution, expanded_by_keyboard)}
        </Show>
    }
}

/// The figure both surfaces render, whichever of the two the driver published. The world is the figure
/// whenever no region is selected.
#[derive(Clone, PartialEq)]
struct ActiveFigure {
    label: String,
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
            label: name_en,
            statistic,
            period_start,
            cell,
            detail,
        });
    }

    let GlobalView { statistic, period_start, cell, detail } = global?;

    Some(ActiveFigure {
        label: t_string!(i18n, detail.world).to_string(),
        statistic,
        period_start,
        cell,
        detail,
    })
}

/// Reads one string off the active figure. Each of these subscribes only itself, so a republish rewrites the
/// text that changed and leaves the elements holding it alone.
fn figure_text(
    figure: Memo<Option<ActiveFigure>>,
    read: impl Fn(&ActiveFigure) -> String + Copy + Send + Sync + 'static,
) -> impl Fn() -> String + Copy + Send + Sync {
    move || figure.with(|figure| figure.as_ref().map(read).unwrap_or_default())
}

/// The statistic and the period, which always change together.
fn figure_heading(figure: Memo<Option<ActiveFigure>>, i18n: I18nContext<Locale>) -> impl Fn() -> String + Copy + Send + Sync {
    figure_text(figure, move |figure| {
        format!(
            "{} · {}",
            labels::statistic_label(i18n, figure.statistic),
            figure.period_start.year(),
        )
    })
}

/// The small top-left figure: the value and its source, and the control that expands to the dock.
fn summary_panel(
    i18n: I18nContext<Locale>,
    figure: Memo<Option<ActiveFigure>>,
    surface: RwSignal<DetailSurface>,
    expanded_by_keyboard: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <aside class="panel detail-panel">
            <button
                class="button button-icon detail-panel-expand"
                type="button"
                aria-label=t_string!(i18n, detail.expand)
                on:click=move |event| {
                    expanded_by_keyboard.set(activated_by_keyboard(&event));
                    surface.set(DetailSurface::Expanded);
                }
            >
                {expand_icon()}
            </button>
            <p class="detail-panel-region">{figure_text(figure, |figure| figure.label.clone())}</p>
            <p class="detail-panel-statistic">{figure_heading(figure, i18n)}</p>
            {move || figure.with(|figure| {
                let Some(figure) = figure.as_ref()
                else {
                    return ().into_any();
                };

                summary_figure(i18n, figure)
            })}
        </aside>
    }
}

/// The value block alone is rebuilt when a region loses or gains a reading, since it swaps a figure and its
/// source for a sentence. Keeping it in its own closure holds that rebuild away from the panel around it.
fn summary_figure(i18n: I18nContext<Locale>, figure: &ActiveFigure) -> AnyView {
    let CellView { value, source, data_status } = figure.cell;
    let statistic: StatisticKind = figure.statistic;
    let status: Option<AnyView> = data_status.and_then(|data_status| status_label(i18n, data_status));

    let Some(value) = value
    else {
        return view! {
            <p class="detail-panel-no-data">{t!(i18n, detail.no_data)}</p>
        }
        .into_any();
    };

    view! {
        <p class="detail-panel-value numeric">{format_value(statistic, value)}</p>
        <p class="detail-panel-unit">{labels::statistic_unit(i18n, statistic)}</p>
        {source.map(|source| view! {
            <p class="detail-panel-source">{t!(i18n, detail.source)} ": " {source_label(i18n, source)}</p>
        })}
        {status.map(|status| view! {
            <p class="detail-panel-status">{status}</p>
        })}
    }
    .into_any()
}

fn detail_dock(
    i18n: I18nContext<Locale>,
    figure: Memo<Option<ActiveFigure>>,
    surface: RwSignal<DetailSurface>,
    attribution: RwSignal<BTreeMap<DataSourceKind, SourceAttribution>>,
    expanded_by_keyboard: RwSignal<bool>,
) -> impl IntoView {
    let thumb: ScrollThumbState = scroll_thumb::create_state();
    let dock: NodeRef<Aside> = NodeRef::new();
    let collapse: NodeRef<Button> = NodeRef::new();

    Effect::new(move |_| report_covered_surface(dock));
    Effect::new(move |_| {
        if expanded_by_keyboard.get_untracked() {
            take_focus(collapse);
        }
    });
    on_cleanup(|| dispatch_left_surface_inset(0.0));

    let value_text = figure_text(figure, move |figure| match figure.cell.value {
        Some(value) => format_value(figure.statistic, value),
        None => t_string!(i18n, detail.not_applicable).to_string(),
    });

    view! {
        <aside class="panel region-dock" node_ref=dock>
        <div
            class="region-dock-scroll"
            node_ref=thumb.scroller()
            on:scroll=move |_| scroll_thumb::refresh(thumb)
            on:pointerenter=move |_| scroll_thumb::refresh(thumb)
        >
            <header class="region-dock-header">
                <h2 class="region-dock-region">{figure_text(figure, |figure| figure.label.clone())}</h2>
                <button
                    class="button button-icon region-dock-collapse"
                    node_ref=collapse
                    type="button"
                    aria-label=t_string!(i18n, detail.collapse)
                    on:click=move |_| surface.set(DetailSurface::Summary)
                >
                    {collapse_icon()}
                </button>
            </header>

            <p class="region-dock-statistic">{figure_heading(figure, i18n)}</p>

            /* The dock keeps the value's block whether or not there is a value, so scrubbing across a gap in
               coverage does not move everything below it. */
            <p class="region-dock-value numeric">{value_text}</p>
            <p class="region-dock-unit">
                {figure_text(figure, move |figure| labels::statistic_unit(i18n, figure.statistic))}
            </p>
            <p class="region-dock-status">
                {figure_text(figure, move |figure| {
                    figure.cell.data_status.and_then(|status| status_text(i18n, status)).unwrap_or_default()
                })}
            </p>

            {context_rows(i18n, figure)}
            {history_section(i18n, figure)}
            {move || figure.with(|figure| {
                let Some(figure) = figure.as_ref()
                else {
                    return ().into_any();
                };

                attribution.with(|attribution| sources_section(i18n, figure.statistic, &figure.detail.sources, attribution))
            })}
            <h3 class="region-dock-heading">{t!(i18n, detail.about)}</h3>
            <p class="region-dock-about">
                {figure_text(figure, move |figure| labels::statistic_description(i18n, figure.statistic))}
            </p>
        </div>
        {scroll_thumb::view(thumb)}
        </aside>
    }
}

/// The figures that put the primary value in proportion. A row whose comparison the shard does not cover still
/// renders, so the block keeps its shape as the reader scrubs.
fn context_rows(i18n: I18nContext<Locale>, figure: Memo<Option<ActiveFigure>>) -> impl IntoView {
    view! {
        <dl class="region-dock-context">
            {context_row(t!(i18n, detail.rank).into_any(), rank_text(i18n, figure))}
            {context_row(
                t!(i18n, detail.change_over_1_period).into_any(),
                change_text(i18n, figure, CHANGE_INTERVAL_IN_YEARS_SHORT),
            )}
            {context_row(
                t!(i18n, detail.change_over_10_periods).into_any(),
                change_text(i18n, figure, CHANGE_INTERVAL_IN_YEARS_LONG),
            )}
        </dl>
    }
}

fn context_row(label: AnyView, value: impl Fn() -> String + Send + Sync + 'static) -> impl IntoView {
    view! {
        <div class="region-dock-context-row">
            <dt>{label}</dt>
            <dd class="numeric">{value}</dd>
        </div>
    }
}

/// "Lowest of 217", "Highest of 217", or "22nd lowest of 217" between them, which states the direction rather
/// than leaving the reader to infer it from a sorting convention. The ordinal is English, so the phrase is
/// assembled here; a second locale wants the whole phrase interpolated in the locale file instead.
fn rank_text(i18n: I18nContext<Locale>, figure: Memo<Option<ActiveFigure>>) -> impl Fn() -> String + Copy + Send + Sync {
    figure_text(figure, move |figure| match figure.detail.rank {
        Some(rank) => match rank_phrase(rank) {
            RankPhrase::Lowest => format!("{} {}", t_string!(i18n, detail.rank_lowest_of), rank.of),
            RankPhrase::Highest => format!("{} {}", t_string!(i18n, detail.rank_highest_of), rank.of),
            RankPhrase::Ordinal => format!(
                "{} {} {}",
                ordinal(rank.position),
                t_string!(i18n, detail.rank_ordinal_lowest_of),
                rank.of,
            ),
        },
        None => t_string!(i18n, detail.not_applicable).to_string(),
    })
}

/// Which of the three phrasings a rank takes. An ordinal reads wrong at either end, where "1st lowest" and
/// "217th lowest of 217" say plainly what "Lowest" and "Highest" say well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RankPhrase {
    Lowest,
    Highest,
    Ordinal,
}

fn rank_phrase(rank: RankView) -> RankPhrase {
    if rank.position <= 1 {
        return RankPhrase::Lowest;
    }

    if rank.position >= rank.of {
        return RankPhrase::Highest;
    }

    RankPhrase::Ordinal
}

/// Eleven, twelve and thirteen take "th" despite their final digit, and so does any number ending in them.
fn ordinal(position: usize) -> String {
    let suffix: &str = match (position % 100, position % 10) {
        (11 | 12 | 13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        (_, _) => "th",
    };

    format!("{position}{suffix}")
}

fn change_text(
    i18n: I18nContext<Locale>,
    figure: Memo<Option<ActiveFigure>>,
    years: i32,
) -> impl Fn() -> String + Copy + Send + Sync {
    figure_text(figure, move |figure| {
        let change: Option<f64> = change_over_years(&figure.detail.series, figure.period_start, years);

        format_change_or(change, t_string!(i18n, detail.not_applicable))
    })
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

/// Everything the chart draws, as the strings its attributes take. One memo, so a republish that leaves the
/// drawing unchanged does not touch the DOM at all, and one that changes it rewrites only the values.
#[derive(Clone, PartialEq)]
struct ChartGeometry {
    is_plottable: bool,
    polyline_points: String,
    marker_x: String,
    marker_y: String,
    marker_radius: String,
    reference_y: String,
    reference_label: String,
    has_reference: bool,
    unit_label: String,
    first_year: String,
    last_year: String,
}

fn chart_geometry(i18n: I18nContext<Locale>, figure: Option<&ActiveFigure>) -> ChartGeometry {
    let Some(figure) = figure
    else {
        return ChartGeometry {
            is_plottable: false,
            polyline_points: format!("{PLOT_LEFT:.1},{PLOT_BOTTOM:.1}"),
            marker_x: chart_unit(PLOT_LEFT),
            marker_y: chart_unit(PLOT_BOTTOM),
            marker_radius: chart_unit(0.0),
            reference_y: chart_unit(PLOT_BOTTOM),
            reference_label: String::new(),
            has_reference: false,
            unit_label: String::new(),
            first_year: String::new(),
            last_year: String::new(),
        };
    };

    let series: &[SeriesPointView] = &figure.detail.series;
    let reference: Option<f64> = reference_value(figure.statistic);
    let scale: ChartScale = ChartScale::from_series(series, reference);
    let marker: ActiveMarker = active_marker(series, figure.period_start, &scale);

    ChartGeometry {
        is_plottable: scale.is_plottable(),
        polyline_points: scale.polyline_points(series),
        marker_x: chart_unit(marker.point.x),
        marker_y: chart_unit(marker.point.y),
        marker_radius: chart_unit(marker.radius),
        reference_y: chart_unit(reference.map_or(PLOT_BOTTOM, |value| scale.y(value))),
        reference_label: reference.map(|value| format!("{value:.1}")).unwrap_or_default(),
        has_reference: reference.is_some(),
        unit_label: labels::statistic_unit(i18n, figure.statistic),
        first_year: scale.first_period_start.year().to_string(),
        last_year: scale.last_period_start.year().to_string(),
    }
}

fn history_section(i18n: I18nContext<Locale>, figure: Memo<Option<ActiveFigure>>) -> impl IntoView {
    let geometry: Memo<ChartGeometry> = Memo::new(move |_| figure.with(|figure| chart_geometry(i18n, figure.as_ref())));

    /* One shape whatever the series holds: a chart with nothing to draw hides, and the line explaining why
       shows. Swapping the two would tear the chart's elements down. */
    view! {
        <h3 class="region-dock-heading">{t!(i18n, detail.history)}</h3>
        <div
            class="region-dock-chart-figure"
            class:is-empty=move || !geometry.with(|geometry| geometry.is_plottable)
        >
            {history_chart(geometry, figure)}
            <p class="region-dock-chart-bounds">
                <span class="numeric">{move || geometry.with(|geometry| geometry.first_year.clone())}</span>
                <span class="numeric">{move || geometry.with(|geometry| geometry.last_year.clone())}</span>
            </p>
        </div>
        <p
            class="region-dock-no-history"
            class:is-empty=move || geometry.with(|geometry| geometry.is_plottable)
        >
            {t!(i18n, detail.no_history)}
        </p>
    }
}

/// The value a statistic's series is read against, or `None` for one with no such threshold.
fn reference_value(statistic: StatisticKind) -> Option<f64> {
    match statistic {
        StatisticKind::Tfr => Some(REPLACEMENT_RATE),
        StatisticKind::Ccf => Some(REPLACEMENT_RATE),
        StatisticKind::MeanAgeAtChildbirth => None,
        StatisticKind::MeanAgeAtFirstBirth => None,
    }
}

/// The period a pointer is resting on in the chart, with the plot coordinates its marks are drawn at.
#[derive(Clone, PartialEq)]
struct ChartCursor {
    period_start: NaiveDate,
    x: f64,
    y: f64,
    readout: ChartReadout,
}

/// The readout's text and the box behind it, already in chart units, so the view only places what it is given.
#[derive(Clone, PartialEq)]
struct ChartReadout {
    text: String,
    text_x: String,
    text_y: String,
    box_x: String,
    box_y: String,
    box_width: String,
    anchor: &'static str,
}

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))] // only the pointer handler builds a readout
mod readout_geometry {
    use super::{chart_unit, ChartReadout, CHART_WIDTH, PLOT_BOTTOM, PLOT_TOP, READOUT_HEIGHT};

    /// How far across the plot the cursor has to be before its readout crosses to the line's other side.
    const FLIP_PROPORTION: f64 = 0.62;

    /* The box is sized here rather than by layout, because an SVG box cannot be sized by its content. The type
       size matches `--type-size-bound`, which the chart's other labels use, and a monospace glyph advances by a
       fixed proportion of it. */
    const TYPE_SIZE: f64 = 11.0;
    const MONOSPACE_ADVANCE_PROPORTION: f64 = 0.6;
    const PADDING: f64 = 4.0;
    const GAP: f64 = 5.0;

    pub fn build(text: String, x: f64, y: f64) -> ChartReadout {
        let text_width: f64 = text.chars().count() as f64 * TYPE_SIZE * MONOSPACE_ADVANCE_PROPORTION;
        let box_width: f64 = text_width + PADDING * 2.0;
        let crosses_to_the_left: bool = x > CHART_WIDTH * FLIP_PROPORTION;

        let (box_x, text_x, anchor): (f64, f64, &'static str) = match crosses_to_the_left {
            true => (x - GAP - box_width, x - GAP - PADDING, "end"),
            false => (x + GAP, x + GAP + PADDING, "start"),
        };

        let box_y: f64 = (y - READOUT_HEIGHT / 2.0).clamp(PLOT_TOP, PLOT_BOTTOM - READOUT_HEIGHT);

        ChartReadout {
            text,
            text_x: chart_unit(text_x),
            text_y: chart_unit(box_y + READOUT_HEIGHT / 2.0),
            box_x: chart_unit(box_x),
            box_y: chart_unit(box_y),
            box_width: chart_unit(box_width),
            anchor,
        }
    }
}

/// The plotted point nearest `plot_x`, so a pointer between two periods resolves to one of them rather than to
/// an interpolation the series does not contain.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))] // only the pointer handler calls it, and ssr has no pointer
fn nearest_point(series: &[SeriesPointView], scale: &ChartScale, plot_x: f64) -> Option<SeriesPointView> {
    series.iter().copied().min_by(|left, right| {
        let left_distance: f64 = (scale.x(left.period_start) - plot_x).abs();
        let right_distance: f64 = (scale.x(right.period_start) - plot_x).abs();

        left_distance.total_cmp(&right_distance)
    })
}

#[cfg(feature = "hydrate")] // reads the pointer's position against the rendered element
fn cursor_at(figure: Option<&ActiveFigure>, event: &leptos::ev::PointerEvent) -> Option<ChartCursor> {
    use wasm_bindgen::JsCast;

    let figure: &ActiveFigure = figure?;
    let scale: ChartScale = ChartScale::from_series(&figure.detail.series, reference_value(figure.statistic));
    if !scale.is_plottable() {
        return None;
    }

    let plot: web_sys::Element = event.current_target()?.dyn_into().ok()?;
    let plot_box: web_sys::DomRect = plot.get_bounding_client_rect();
    if plot_box.width() <= 0.0 {
        return None;
    }

    let plot_x: f64 = (f64::from(event.client_x()) - plot_box.left()) / plot_box.width() * CHART_WIDTH;
    let point: SeriesPointView = nearest_point(&figure.detail.series, &scale, plot_x)?;

    let x: f64 = scale.x(point.period_start);
    let y: f64 = scale.y(point.value);
    let label: String = format!("{} · {}", point.period_start.year(), format_value(figure.statistic, point.value));

    Some(ChartCursor {
        period_start: point.period_start,
        x,
        y,
        readout: readout_geometry::build(label, x, y),
    })
}

#[cfg(not(feature = "hydrate"))] // there are no pointers to read without a browser
fn cursor_at(_figure: Option<&ActiveFigure>, _event: &leptos::ev::PointerEvent) -> Option<ChartCursor> {
    None
}

#[cfg(feature = "hydrate")] // retargets the pointer's events at the plot
fn capture_pointer(event: &leptos::ev::PointerEvent) {
    use wasm_bindgen::JsCast;

    let plot: Option<web_sys::Element> =
        event.current_target().and_then(|target| target.dyn_into().ok());

    if let Some(plot) = plot {
        let _ = plot.set_pointer_capture(event.pointer_id());
    }
}

#[cfg(not(feature = "hydrate"))] // there is no pointer to capture without a browser
fn capture_pointer(_event: &leptos::ev::PointerEvent) {}

/// Built once. Every attribute below is bound to a closure, so the elements outlive every republish and only
/// the values they hold are rewritten.
fn history_chart(geometry: Memo<ChartGeometry>, figure: Memo<Option<ActiveFigure>>) -> impl IntoView {
    let unit_label_x: f64 = PLOT_LEFT - UNIT_LABEL_GAP;
    let unit_label_y: f64 = (PLOT_TOP + PLOT_BOTTOM) / 2.0;
    let cursor: RwSignal<Option<ChartCursor>> = RwSignal::new(None);
    let dragging: RwSignal<bool> = RwSignal::new(false);

    view! {
        <svg
            class="region-dock-chart"
            viewBox=format!("0 0 {CHART_WIDTH} {CHART_HEIGHT}")
            aria-hidden="true"
            on:pointerdown=move |event| {
                event.prevent_default();
                dragging.set(true);
                capture_pointer(&event);
                cursor.set(rest_and_commit(figure, &event, Commit::Yes));
            }
            on:pointermove=move |event| {
                let commit: Commit = match dragging.get_untracked() {
                    true => Commit::Yes,
                    false => Commit::No,
                };

                cursor.set(rest_and_commit(figure, &event, commit));
            }
            on:pointerup=move |_| dragging.set(false)
            on:pointercancel=move |_| dragging.set(false)
            on:lostpointercapture=move |_| dragging.set(false)
            on:pointerleave=move |_| {
                // A drag continues past the plot's edge, so only an idle pointer leaving clears the marks.
                if !dragging.get_untracked() {
                    cursor.set(None);
                }
            }
        >
            <text
                class="region-dock-chart-unit"
                x=chart_unit(unit_label_x)
                y=chart_unit(unit_label_y)
                text-anchor="middle"
                transform=format!("rotate(-90, {unit_label_x}, {unit_label_y})")
            >
                {move || geometry.with(|geometry| geometry.unit_label.clone())}
            </text>
            <line
                class=move || geometry.with(|geometry| match geometry.has_reference {
                    true => "region-dock-chart-reference",
                    false => "region-dock-chart-reference is-absent",
                })
                x1=chart_unit(PLOT_LEFT)
                x2=chart_unit(PLOT_RIGHT)
                y1=move || geometry.with(|geometry| geometry.reference_y.clone())
                y2=move || geometry.with(|geometry| geometry.reference_y.clone())
            />
            <text
                class=move || geometry.with(|geometry| match geometry.has_reference {
                    true => "region-dock-chart-reference-value numeric",
                    false => "region-dock-chart-reference-value numeric is-absent",
                })
                x=chart_unit(PLOT_RIGHT + REFERENCE_LABEL_GAP)
                y=move || geometry.with(|geometry| geometry.reference_y.clone())
                dominant-baseline="middle"
            >
                {move || geometry.with(|geometry| geometry.reference_label.clone())}
            </text>
            <line
                class="region-dock-chart-baseline"
                x1=chart_unit(PLOT_LEFT)
                x2=chart_unit(PLOT_RIGHT)
                y1=chart_unit(PLOT_BOTTOM)
                y2=chart_unit(PLOT_BOTTOM)
            />
            <line
                class="region-dock-chart-cursor"
                class:is-visible=move || cursor.with(Option::is_some)
                x1=move || cursor_unit(cursor, |cursor| cursor.x)
                x2=move || cursor_unit(cursor, |cursor| cursor.x)
                y1=chart_unit(PLOT_TOP)
                y2=chart_unit(PLOT_BOTTOM)
            />
            <polyline
                class="region-dock-chart-line"
                points=move || geometry.with(|geometry| geometry.polyline_points.clone())
            />
            <circle
                class="region-dock-chart-marker"
                cx=move || geometry.with(|geometry| geometry.marker_x.clone())
                cy=move || geometry.with(|geometry| geometry.marker_y.clone())
                r=move || geometry.with(|geometry| geometry.marker_radius.clone())
            />
            <rect
                class="region-dock-chart-readout-panel"
                class:is-visible=move || cursor.with(Option::is_some)
                x=move || readout_text(cursor, |readout| readout.box_x.clone())
                y=move || readout_text(cursor, |readout| readout.box_y.clone())
                width=move || readout_text(cursor, |readout| readout.box_width.clone())
                height=chart_unit(READOUT_HEIGHT)
            />
            <text
                class="region-dock-chart-readout numeric"
                class:is-visible=move || cursor.with(Option::is_some)
                x=move || readout_text(cursor, |readout| readout.text_x.clone())
                y=move || readout_text(cursor, |readout| readout.text_y.clone())
                text-anchor=move || readout_text(cursor, |readout| readout.anchor.to_string())
                dominant-baseline="middle"
            >
                {move || readout_text(cursor, |readout| readout.text.clone())}
            </text>
        </svg>
    }
}

/// Keeps the last value when the pointer has left, so an attribute is never written empty.
fn readout_text(cursor: RwSignal<Option<ChartCursor>>, read: impl Fn(&ChartReadout) -> String) -> String {
    cursor.with(|cursor| cursor.as_ref().map(|cursor| read(&cursor.readout)).unwrap_or_default())
}

/// Whether the period under the pointer becomes the active one, which a drag does on every step and an idle
/// pointer never does.
#[derive(Clone, Copy, PartialEq)]
enum Commit {
    Yes,
    No,
}

/// The period under the pointer, dispatched to the driver when the caller is dragging and it is not already
/// the active one, so a drag across a wide period does not republish the same year on every step.
fn rest_and_commit(
    figure: Memo<Option<ActiveFigure>>,
    event: &leptos::ev::PointerEvent,
    commit: Commit,
) -> Option<ChartCursor> {
    let (resting_on, active_period_start): (Option<ChartCursor>, Option<NaiveDate>) =
        figure.with(|figure| (cursor_at(figure.as_ref(), event), figure.as_ref().map(|figure| figure.period_start)));
    let cursor: ChartCursor = resting_on?;

    if commit == Commit::Yes && Some(cursor.period_start) != active_period_start {
        canvas::dispatch_period(cursor.period_start);
    }

    Some(cursor)
}

/// Keeps the last coordinate when the pointer has left, so an attribute is never written empty.
fn cursor_unit(cursor: RwSignal<Option<ChartCursor>>, read: impl Fn(&ChartCursor) -> f64) -> String {
    chart_unit(cursor.with(|cursor| cursor.as_ref().map_or(PLOT_LEFT, read)))
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

struct ActiveMarker {
    point: ChartPoint,
    radius: f64,
}

/* A radius of zero draws nothing, which is how the marker is hidden. Rendering the element unconditionally
   rather than only when the active period has a value keeps it mounted: Safari logs an invalid empty value for
   every attribute Leptos removes when it tears an SVG element down, and scrubbing to a period the region does
   not cover would tear this one down on each pass. */
fn active_marker(series: &[SeriesPointView], active_period_start: NaiveDate, scale: &ChartScale) -> ActiveMarker {
    let Some(value) = value_at(series, active_period_start)
    else {
        return ActiveMarker {
            point: ChartPoint { x: PLOT_LEFT, y: PLOT_BOTTOM },
            radius: 0.0,
        };
    };

    ActiveMarker {
        point: scale.point(active_period_start, value),
        radius: MARKER_RADIUS,
    }
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
    /* Never absent. A series with too few periods to plot gets a degenerate scale rather than suppressing the
       chart, because Leptos tears an element down when a view changes shape and Safari reports an invalid empty
       value for every attribute removed on the way out. `included_value` is held inside the vertical range even
       when the series never reaches it. */
    fn from_series(series: &[SeriesPointView], included_value: Option<f64>) -> ChartScale {
        let first_period_start: NaiveDate = series.first().map_or(NaiveDate::MIN, |point| point.period_start);
        let last_period_start: NaiveDate = series.last().map_or(NaiveDate::MIN, |point| point.period_start);

        let mut low: f64 = included_value.unwrap_or(f64::INFINITY);
        let mut high: f64 = included_value.unwrap_or(f64::NEG_INFINITY);

        for point in series {
            low = low.min(point.value);
            high = high.max(point.value);
        }

        if !low.is_finite() || !high.is_finite() {
            low = 0.0;
            high = 0.0;
        }

        let extent: f64 = high - low;
        let margin: f64 = if extent > 0.0 {
            extent * CHART_RANGE_MARGIN_PROPORTION
        } else {
            FLAT_SERIES_HALF_EXTENT
        };

        ChartScale {
            first_period_start,
            last_period_start,
            low: low - margin,
            high: high + margin,
        }
    }

    /// A series confined to one period, or none, has nothing to draw a line between.
    fn is_plottable(&self) -> bool {
        self.first_period_start != self.last_period_start
    }

    fn x(&self, period_start: NaiveDate) -> f64 {
        let total_days: f64 = (self.last_period_start - self.first_period_start).num_days() as f64;
        if total_days <= 0.0 {
            return PLOT_LEFT;
        }

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

    /// One coordinate for a series with nothing to plot, rather than an empty list: an empty attribute is the
    /// same defect as a removed one, and a single-point polyline draws nothing.
    fn polyline_points(&self, series: &[SeriesPointView]) -> String {
        let coordinates: Vec<String> = series
            .iter()
            .map(|series_point| self.point(series_point.period_start, series_point.value))
            .map(|chart_point| format!("{:.1},{:.1}", chart_point.x, chart_point.y))
            .collect();

        if coordinates.is_empty() {
            return format!("{PLOT_LEFT:.1},{PLOT_BOTTOM:.1}");
        }

        coordinates.join(" ")
    }
}

/// Every source covering the active period, so a reader can see that sources disagree and by how much.
fn sources_section(
    i18n: I18nContext<Locale>,
    statistic: StatisticKind,
    sources: &[SourceCellView],
    attribution: &BTreeMap<DataSourceKind, SourceAttribution>,
) -> AnyView {
    if sources.is_empty() {
        return ().into_any();
    }

    let is_contested: bool = sources.len() > 1;
    let rows: Vec<AnyView> = sources
        .iter()
        .map(|source_cell| {
            let source_attribution: Option<&SourceAttribution> = attribution.get(&source_cell.source);

            source_row(i18n, statistic, source_cell, is_contested, source_attribution)
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
    statistic: StatisticKind,
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
            <span class="region-dock-source-value numeric">{format_value(statistic, source_cell.value)}</span>
            {status.map(|status| view! {
                <span class="region-dock-source-status">{status}</span>
            })}
            {attribution.map(attribution_lines)}
        </div>
    }
    .into_any()
}

/// The citation is rendered verbatim because the source's licence asks for exactly that string.
fn attribution_lines(attribution: &SourceAttribution) -> AnyView {
    view! {
        <span class="region-dock-source-attribution">{attribution.attribution_text.clone()}</span>
        <span class="region-dock-source-links">
            <a href=attribution.license_url.clone() target="_blank" rel="noopener noreferrer">
                {attribution.license_name.clone()}
            </a>
            " · "
            <a href=attribution.homepage_url.clone() target="_blank" rel="noopener noreferrer">
                {link_host(&attribution.homepage_url)}
            </a>
        </span>
    }
    .into_any()
}

/// The host a link points at, which says where it goes without claiming it is a "home" page and without a word
/// to translate. The whole URL is the fallback, since a link is more useful mislabelled than unlabelled.
fn link_host(url: &str) -> String {
    let without_scheme: &str = url.split("://").nth(1).unwrap_or(url);
    let host: &str = without_scheme.split('/').next().unwrap_or(without_scheme);

    host.strip_prefix("www.").unwrap_or(host).to_string()
}

fn format_value(statistic: StatisticKind, value: f64) -> String {
    format!("{value:.*}", labels::statistic_decimals(statistic))
}

/// Signed, since the reader is being shown a direction rather than a magnitude.
fn format_change_or(change: Option<f64>, absent: &str) -> String {
    match change {
        Some(change) => format!("{change:+.2}"),
        None => absent.to_string(),
    }
}

/// `None` for a confirmed figure, which qualifies nothing about the value above it.
fn status_text(i18n: I18nContext<Locale>, data_status: DataStatus) -> Option<String> {
    match data_status {
        DataStatus::Final => None,
        DataStatus::Provisional => Some(t_string!(i18n, detail.status.provisional).to_string()),
        DataStatus::Preliminary => Some(t_string!(i18n, detail.status.preliminary).to_string()),
        DataStatus::Projection => Some(t_string!(i18n, detail.status.projection).to_string()),
        DataStatus::Imputed => Some(t_string!(i18n, detail.status.imputed).to_string()),
        DataStatus::Interpolated => Some(t_string!(i18n, detail.status.interpolated).to_string()),
        DataStatus::Estimated => Some(t_string!(i18n, detail.status.estimated).to_string()),
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
        DataStatus::Estimated => Some(t!(i18n, detail.status.estimated).into_any()),
    }
}

fn source_label(i18n: I18nContext<Locale>, source: DataSourceKind) -> AnyView {
    match source {
        DataSourceKind::WorldBankWDI => t!(i18n, source.wb_wdi).into_any(),
        DataSourceKind::HumanFertilityDatabase => t!(i18n, source.hfd).into_any(),
        DataSourceKind::Eurostat => t!(i18n, source.eurostat).into_any(),
    }
}

/// A control activated by keyboard has a visible focus ring; one activated by pointer does not.
#[cfg(feature = "hydrate")]
fn activated_by_keyboard(event: &leptos::ev::MouseEvent) -> bool {
    use wasm_bindgen::JsCast;

    event
        .current_target()
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        .and_then(|element| element.matches(":focus-visible").ok())
        .unwrap_or(false)
}

#[cfg(not(feature = "hydrate"))] // the ssr build has no element to interrogate
fn activated_by_keyboard(_event: &leptos::ev::MouseEvent) -> bool {
    false
}

/* Opening the dock destroys the control that opened it, so a keyboard reader's focus would fall to the document
   and the next tab would skip the dock. The collapse control takes it, and only for a keyboard activation: it
   sits inside the scrolling element so the arrow keys scroll on arrival. */
#[cfg(feature = "hydrate")]
fn take_focus(collapse: NodeRef<Button>) {
    let Some(collapse) = collapse.get()
    else {
        return;
    };

    let _ = collapse.focus();
}

#[cfg(not(feature = "hydrate"))] // the ssr build has nothing focusable
fn take_focus(_collapse: NodeRef<Button>) {}

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

    #[test]
    fn nearest_point_snaps_to_whichever_period_the_pointer_is_closer_to() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2010, 1.4), (2020, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, None);
        let midpoint: f64 = (scale.x(series[0].period_start) + scale.x(series[1].period_start)) / 2.0;

        let just_after: SeriesPointView = nearest_point(&series, &scale, midpoint + 1.0).unwrap();
        let just_before: SeriesPointView = nearest_point(&series, &scale, midpoint - 1.0).unwrap();

        assert_eq!(just_after.period_start, series[1].period_start);
        assert_eq!(just_before.period_start, series[0].period_start);
    }

    /// The pointer can sit in the axis gutters either side of the plot, which belong to no period.
    #[test]
    fn nearest_point_resolves_a_pointer_outside_the_plot_to_the_nearer_end() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2020, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, None);

        assert_eq!(nearest_point(&series, &scale, -400.0).unwrap().period_start, series[0].period_start);
        assert_eq!(nearest_point(&series, &scale, CHART_WIDTH * 4.0).unwrap().period_start, series[1].period_start);
    }

    #[test]
    fn nearest_point_reports_nothing_for_a_series_with_no_points() {
        let scale: ChartScale = ChartScale::from_series(&[], None);

        assert!(nearest_point(&[], &scale, 100.0).is_none());
    }

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
    fn link_host_keeps_only_where_the_link_goes() {
        assert_eq!(link_host("https://www.humanfertility.org/"), "humanfertility.org");
        assert_eq!(
            link_host("https://databank.worldbank.org/source/world-development-indicators"),
            "databank.worldbank.org",
        );
        assert_eq!(link_host("not a url"), "not a url");
    }

    #[test]
    fn rank_phrase_names_the_extremes_and_numbers_everything_between() {
        assert_eq!(rank_phrase(RankView { position: 1, of: 217 }), RankPhrase::Lowest);
        assert_eq!(rank_phrase(RankView { position: 217, of: 217 }), RankPhrase::Highest);
        assert_eq!(rank_phrase(RankView { position: 2, of: 217 }), RankPhrase::Ordinal);
        assert_eq!(rank_phrase(RankView { position: 216, of: 217 }), RankPhrase::Ordinal);
    }

    /// One region covered at a period is both ends at once, and reading as the lowest is the less odd of the
    /// two.
    #[test]
    fn rank_phrase_calls_a_lone_region_the_lowest() {
        assert_eq!(rank_phrase(RankView { position: 1, of: 1 }), RankPhrase::Lowest);
    }

    #[test]
    fn ordinal_takes_th_for_the_teens_whatever_their_final_digit() {
        let ordinals: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 101, 111, 112, 113, 217]
            .into_iter()
            .map(ordinal)
            .collect();

        assert_eq!(
            ordinals,
            vec![
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "23rd", "101st", "111th",
                "112th", "113th", "217th",
            ],
        );
    }

    #[test]
    fn format_change_or_signs_a_change_and_falls_back_when_absent() {
        assert_eq!(format_change_or(Some(-0.4), "N/A"), "-0.40");
        assert_eq!(format_change_or(Some(0.08), "N/A"), "+0.08");
        assert_eq!(format_change_or(None, "N/A"), "N/A");
    }

    /// A one-period series still yields a scale, because suppressing the chart would change the view's shape.
    /// It reports that it has nothing to plot instead.
    #[test]
    fn chart_scale_reports_a_series_confined_to_one_period_as_unplottable() {
        assert!(!ChartScale::from_series(&series_of(&[(2024, 1.20)]), None).is_plottable());
        assert!(!ChartScale::from_series(&[], None).is_plottable());
        assert!(ChartScale::from_series(&series_of(&[(2000, 1.5), (2024, 1.2)]), None).is_plottable());
    }

    /// An empty attribute is the same defect as a removed one, so the points list is never empty.
    #[test]
    fn polyline_points_are_never_empty() {
        let scale: ChartScale = ChartScale::from_series(&[], None);

        assert_eq!(scale.polyline_points(&[]), format!("{PLOT_LEFT:.1},{PLOT_BOTTOM:.1}"));
    }

    #[test]
    fn chart_scale_places_a_lone_period_at_the_plot_s_left() {
        let scale: ChartScale = ChartScale::from_series(&series_of(&[(2024, 1.20)]), None);

        assert_eq!(scale.x(january(2024)), PLOT_LEFT);
    }

    #[test]
    fn chart_scale_spans_the_plot_from_the_first_period_to_the_last() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2012, 1.4), (2024, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, None);

        assert_eq!(scale.x(january(2000)), PLOT_LEFT);
        assert_eq!(scale.x(january(2024)), PLOT_RIGHT);
    }

    /// A gap in coverage has to read as a gap, so an unevenly spaced period lands off the midpoint its index
    /// would have put it on.
    #[test]
    fn chart_scale_places_a_period_by_its_distance_in_time() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2018, 1.4), (2024, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, None);

        let plot_midpoint: f64 = (PLOT_LEFT + PLOT_RIGHT) / 2.0;

        assert!(scale.x(january(2018)) > plot_midpoint);
    }

    #[test]
    fn chart_scale_spans_the_plot_for_an_age_series_with_no_reference() {
        let series: Vec<SeriesPointView> = series_of(&[(1990, 24.9), (2005, 27.4), (2020, 29.7)]);

        let plot_height: f64 = PLOT_BOTTOM - PLOT_TOP;
        let drawn_extent = |scale: &ChartScale| -> f64 { scale.y(24.9) - scale.y(29.7) };

        let without_reference: ChartScale = ChartScale::from_series(&series, None);
        let with_replacement: ChartScale = ChartScale::from_series(&series, Some(REPLACEMENT_RATE));

        // Holding 2.1 inside the range of an age series squeezes the series itself into a sliver; the whole
        // point of a statistic having no reference is that its own range sets the scale.
        assert!(drawn_extent(&without_reference) > plot_height * 0.7);
        assert!(drawn_extent(&with_replacement) < plot_height * 0.2);
    }

    #[test]
    fn a_statistic_measured_in_years_reports_no_reference() {
        assert_eq!(reference_value(StatisticKind::MeanAgeAtChildbirth), None);
        assert_eq!(reference_value(StatisticKind::MeanAgeAtFirstBirth), None);
        assert_eq!(reference_value(StatisticKind::Tfr), Some(REPLACEMENT_RATE));
    }

    #[test]
    fn an_age_is_shown_to_one_decimal_and_a_rate_to_two() {
        assert_eq!(format_value(StatisticKind::MeanAgeAtFirstBirth, 29.35), "29.4");
        assert_eq!(format_value(StatisticKind::Tfr, 1.456), "1.46");
    }

    #[test]
    fn chart_scale_holds_the_reference_line_inside_the_range() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2024, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, Some(REPLACEMENT_RATE));

        let reference_y: f64 = scale.y(REPLACEMENT_RATE);

        assert!(reference_y > PLOT_TOP);
        assert!(reference_y < PLOT_BOTTOM);
    }

    #[test]
    fn chart_scale_draws_a_flat_series_inside_the_plot() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.4), (2024, 1.4)]);
        let scale: ChartScale = ChartScale::from_series(&series, None);

        let flat_y: f64 = scale.y(1.4);

        assert!(flat_y > PLOT_TOP);
        assert!(flat_y < PLOT_BOTTOM);
    }

    #[test]
    fn polyline_points_carries_one_coordinate_per_period() {
        let series: Vec<SeriesPointView> = series_of(&[(2000, 1.5), (2012, 1.4), (2024, 1.2)]);
        let scale: ChartScale = ChartScale::from_series(&series, None);

        let points: String = scale.polyline_points(&series);

        assert_eq!(points.split(' ').count(), 3);
    }
}
