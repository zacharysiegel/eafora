use std::cell::RefCell;
use std::sync::Arc;

use chrono::NaiveDate;
use tokio::sync::watch;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen::closure::Closure;
use web_sys::{HtmlCanvasElement, MouseEvent, PointerEvent, WheelEvent};

use leptos::prelude::*;

use shared::AppError;
use shared::artifact::Bundle;
use shared::canonical::{DataSourceKind, DataStatus, StatisticKind};
use shared::license::DistributionContext;
use shared::map::{ViewportTransition, CountryFraming, FrameState, GeoPoint, ProjectedPoint, AnimationProgress, RegionCode, RegionHit, Renderer, RendererBackend, SurfacePoint, SurfaceDimensions, Viewport};
use shared::map::hit_test;
use shared::map::projection;
use shared::sqlite::shard_db::{CellValue, ShardValues};

use crate::client::cache::OpfsArtifactCache;
use crate::client::load;
use crate::distribution;
use crate::live_resolve;

use super::gesture::{Gesture, PointerRelease, PointerState, is_map_gesture_button};
use super::{CellView, RenderStatus, GlobalView, LegendView, SelectionView, ViewControls};

thread_local! {
    static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
}

/// Greenwich, on the prime meridian. The home view is centered horizontally on its longitude (0°);
/// vertically it is centered on the home-view latitude framing's midpoint (see `HOME_VIEW_MIN_LAT` /
/// `home_viewport`), so only the longitude is used.
const HOME_CENTER: GeoPoint = GeoPoint {
    lat: 51.4779,
    lon: 0.0,
};

/// The home view's latitude framing, in degrees: chosen to enclose the drawn continents (Tierra del
/// Fuego to northern Greenland) with no empty polar ocean. Deliberate design values, not derived from
/// the geometry, so added polar data reframes the home view only when these are changed on purpose.
/// Revisit if Antarctica or sub-Antarctic islands are added to the layer.
const HOME_VIEW_MIN_LAT: f64 = -56.0;
const HOME_VIEW_MAX_LAT: f64 = 84.0;

/// Wheel-zoom feel: the per-event zoom factor is `exp(-delta_y * WHEEL_ZOOM_SENSITIVITY)`, so scrolling
/// is multiplicative and symmetric (opposite scrolls of equal magnitude compose to identity). A tuning
/// constant with no correctness role.
const WHEEL_ZOOM_SENSITIVITY: f64 = 0.0015;

/// Trackpad (or browser) pinch-zoom feel: the browser reports a trackpad pinch as a wheel event with
/// `ctrlKey` set, at a smaller per-event delta than a scroll notch, so it gets a higher sensitivity than a
/// scroll wheel. This is not the touchscreen two-finger pinch, which is a real multi-pointer gesture
/// applied in `hit_test::pinch` by the finger-distance ratio (no sensitivity constant). Tuning constant
/// with no correctness role.
const TRACKPAD_PINCH_ZOOM_SENSITIVITY: f64 = 0.006;

/// Distinguishes a trackpad pinch from a real Ctrl+mouse-wheel, which both set `ctrlKey`: a pinch sends a
/// small, pixel-mode delta, while a wheel notch is larger (or reported in line/page mode). A ctrlKey wheel
/// event with a pixel delta below this is a pinch; at or above it (or in a non-pixel mode) it is treated
/// as an ordinary wheel zoom.
const TRACKPAD_PINCH_MAX_DELTA: f64 = 50.0;

/// Caps a single wheel event's `delta_y` magnitude before the zoom factor is computed, so one line- or
/// page-mode notch (whose delta is far larger than a pixel-mode notch) cannot zoom absurdly far. The
/// deltaMode varies by browser and OS; this bounds the raw value rather than interpreting it.
const MAX_WHEEL_DELTA: f64 = 240.0;

/// Pointer travel in device pixels, between press and release, beyond which a single-pointer gesture is a
/// pan rather than a tap, so it does not select. A few-pixel deadzone keeps a click that jitters slightly
/// from being swallowed.
const DRAG_SELECT_SUPPRESS_PX: f64 = 7.0;

const ZOOM_TO_COUNTRY_ANIMATION_DURATION_MS: f64 = 600.0;

/// Padding around the framed country, as a proportion of its projected extent.
const ZOOM_TO_COUNTRY_MARGIN_PROPORTION: f64 = 1.5;

/// The zoom-to-country zoom-in floor, as the half-height in degrees of an equatorial latitude band.
/// Deliberately looser than the manual zoom-in floor (`MIN_ZOOM_IN_HEIGHT`) so a small country frames its
/// surrounding region, not just itself.
const ZOOM_TO_COUNTRY_MIN_BAND_HALF_LAT: f64 = 8.0;

/// The floor, in projected units, on the margin opposite the clipped (pole) side. When a country sits
/// hard against the pole-side clip edge, `Viewport::clamp_vertical_balanced` shrinks the opposite margin
/// by the clipped amount, which would otherwise reach zero; this is the minimum it may shrink to.
const ZOOM_TO_COUNTRY_MIN_EDGE_MARGIN: f64 = 0.1;

/// Minimum coverage for the default period, as a proportion of the best-covered period's.
const MINIMUM_DEFAULT_COVERAGE_PROPORTION: f64 = 0.8;

/// Canonical `region.code` of the World aggregate. World has no geometry, so it is never a hit-test result; the driver looks it up as the empty-state figure.
const WORLD_REGION_CODE: &str = "world";

/// The result of hit-testing a pointer against the regions, compared to the previously known region.
enum RegionChange {
    /// The pointer is over the same region as before, or still over none; nothing to update.
    Unchanged,
    /// The pointer moved to a different region, or off all regions (`None`).
    Changed(Option<RegionHit>),
}

/// What a statistic or period change republishes: fresh controls and legend extent, the
/// re-resolved selection when a region is selected, and the World figure for the empty state.
struct RepublishedViews {
    view_controls: ViewControls,
    legend: LegendView,
    selection: Option<SelectionView>,
    global: GlobalView,
}

/// The render state the browser callbacks reach through the `DRIVER` thread-local, kept outside the
/// reactive graph because `Renderer` owns single-thread-bound, `!Send` wgpu resources. Each JS callback
/// borrows `DRIVER` once and drives it through `&mut self`, so no method re-borrows the thread-local.
// several fields are held only for lifetime side effects (open channel, live closures), never read
#[allow(dead_code)]
struct Driver {
    renderer: Renderer,
    bundle_sender: watch::Sender<Arc<Bundle>>,
    viewport: Viewport,
    surface_dimensions: SurfaceDimensions,
    frame_state: FrameState,
    selection_view: WriteSignal<Option<SelectionView>>,
    global_view: WriteSignal<Option<GlobalView>>,
    view_controls: WriteSignal<Option<ViewControls>>,
    legend: WriteSignal<Option<LegendView>>,
    selection: Option<SelectionView>,
    redraw_pending: bool,
    gesture: Gesture,
    transition: Option<ViewportTransition>,
    animation_frame_pending: bool,
    redraw_callback: Option<Closure<dyn FnMut()>>,
    animation_callback: Option<Closure<dyn FnMut(f64)>>,
    resize_callback: Option<Closure<dyn FnMut()>>,
    pointer_down_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    pointer_move_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    pointer_up_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    pointer_cancel_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    pointer_leave_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    context_menu_callback: Option<Closure<dyn FnMut(web_sys::Event)>>,
    lost_pointer_capture_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    wheel_callback: Option<Closure<dyn FnMut(WheelEvent)>>,
}

impl Driver {
    fn draw(&mut self) {
        self.redraw_pending = false;

        if let Err(error) = self.renderer.draw_frame(self.viewport, &self.frame_state) {
            log::error!("drawing a frame failed [error={error}]");
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.transition = None;
        self.surface_dimensions = SurfaceDimensions { width, height };

        // Preserve the current pan/zoom across a resize or device-pixel-ratio change, re-fitting only the
        // aspect; do not reset to the home view.
        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        let ceiling: f64 = zoom_out_ceiling_height(self.surface_dimensions);
        self.viewport = self.viewport.refit_to_surface(self.surface_dimensions, ceiling, home_min_y, home_max_y);

        if let Err(error) = self.renderer.resize_surface(width, height) {
            log::error!("resizing the render surface failed [error={error}]");
        }

        self.request_redraw();
    }

    /// Coalesce redraw requests into one `requestAnimationFrame`; there is no idle refresh
    /// loop.
    fn request_redraw(&mut self) {
        if self.redraw_pending {
            return;
        }

        let callback: &Closure<dyn FnMut()> =
            self.redraw_callback.get_or_insert_with(|| Closure::new(draw_pending_frame));

        let Some(window) = web_sys::window() else {
            log::error!("no window available; cannot schedule a redraw");
            return;
        };

        let schedule_result: Result<i32, JsValue> = window.request_animation_frame(callback.as_ref().unchecked_ref());
        if schedule_result.is_ok() {
            // The pending flag is set only once a frame is actually scheduled, so a failed schedule stays
            // retryable; otherwise the flag would latch and every later redraw would short-circuit on it.
            self.redraw_pending = true;
        }
    }

    /// Schedules one animation frame if none is already queued. Mirrors `request_redraw`'s failed-schedule
    /// discipline; the `animation_frame_pending` gate (not `transition.is_some()`) is what guarantees exactly
    /// one loop survives a cancel-then-restart interleaving, since a stale queued frame can outlive the
    /// transition it was scheduled for.
    fn schedule_animation_frame(&mut self) {
        if self.animation_frame_pending {
            return;
        }

        let callback: &Closure<dyn FnMut(f64)> =
            self.animation_callback.get_or_insert_with(|| Closure::new(advance_pending_animation));

        let Some(window) = web_sys::window() else {
            log::error!("no window available; cannot schedule an animation frame");
            return;
        };

        let schedule_result: Result<i32, JsValue> = window.request_animation_frame(callback.as_ref().unchecked_ref());
        if schedule_result.is_ok() {
            self.animation_frame_pending = true;
        }
    }

    /// Advances the zoom-to-country animation by one frame. Draws directly rather than through the
    /// coalesced `request_redraw`, which would fight this loop's own scheduling.
    fn advance_animation(&mut self, now_ms: f64) {
        self.animation_frame_pending = false;

        let Some(transition) = self.transition
        else {
            return;
        };

        let (viewport, progress): (Viewport, AnimationProgress) = transition.sample(now_ms, self.surface_dimensions);
        self.viewport = viewport;
        self.draw();

        match progress {
            AnimationProgress::Animating => self.schedule_animation_frame(),
            AnimationProgress::Finished => self.transition = None,
        }
    }

    /// Starts (or redirects) the zoom-to-country animation toward the viewport framing `framing`, from the
    /// current viewport. A no-op when the current view already matches that target within a tolerance, so a
    /// tap on an already-framed country does not jitter.
    fn start_zoom_to_country(&mut self, framing: CountryFraming) {
        let target: Viewport = self.zoom_target(framing);
        if viewports_match(self.viewport, target) {
            return;
        }

        self.transition = Some(ViewportTransition::new(self.viewport, target, now_ms(), ZOOM_TO_COUNTRY_ANIMATION_DURATION_MS));
        self.schedule_animation_frame();
    }

    /// The viewport that frames a country's projected bounds and centroid with a margin and a
    /// generous-context minimum zoom-out, balanced against the home latitude range (so a pole-adjacent
    /// country zooms in and stays centered rather than sliding off the far edge) and re-normalized across
    /// the seam.
    fn zoom_target(&self, framing: CountryFraming) -> Viewport {
        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        let ceiling: f64 = zoom_out_ceiling_height(self.surface_dimensions);
        let min_height: f64 = zoom_to_country_min_height();

        Viewport::fit_bounds(
            framing.min.x, framing.max.x, framing.min.y, framing.max.y,
            framing.centroid, ZOOM_TO_COUNTRY_MARGIN_PROPORTION, min_height, ceiling, self.surface_dimensions,
        )
        .clamp_vertical_balanced(
            framing.min, framing.max, home_min_y, home_max_y,
            ZOOM_TO_COUNTRY_MIN_EDGE_MARGIN, self.surface_dimensions,
        )
        .normalize_longitude_turns()
    }

    fn region_at(&self, surface_point: SurfacePoint) -> Option<RegionHit> {
        let bundle: Arc<Bundle> = self.bundle_sender.borrow().clone();

        hit_test::region_at_point(&bundle.geometry, self.viewport, self.surface_dimensions, surface_point)
    }

    /// The published bundle. Every handler takes it once and threads it down, so two reads within one
    /// event cannot straddle a hot-swap and mix values from two bundles.
    fn current_bundle(&self) -> Arc<Bundle> {
        self.bundle_sender.borrow().clone()
    }

    /// The values the map colors from: the active statistic's first authorized license class. `None`
    /// when the bundle ships no shard for that statistic, so the caller degrades to "no data".
    fn active_shard_values<'bundle>(&self, bundle: &'bundle Bundle) -> Option<&'bundle ShardValues> {
        bundle.shard_values_for(self.frame_state.active_statistic)
    }

    fn decode_cell(cell: Option<&CellValue>) -> CellView {
        let value: Option<f64> = cell.map(|cell| cell.value);
        let source: Option<DataSourceKind> = cell.and_then(|cell| {
            DataSourceKind::try_from(cell.source_code.as_str())
                .map_err(|error| log::warn!("shard cell has an unrecognized data source; [code={} error={error}]", cell.source_code))
                .ok()
        });
        let data_status: Option<DataStatus> = cell.map(|cell| cell.data_status);

        CellView { value, source, data_status }
    }

    fn resolve_selection_view(&self, bundle: &Bundle, region_code: &str, name_en: &str) -> SelectionView {
        let cell: Option<&CellValue> = self
            .active_shard_values(bundle)
            .and_then(|shard_values| shard_values.cell(region_code, self.frame_state.active_period_start));

        SelectionView {
            region_code: region_code.to_string(),
            name_en: name_en.to_string(),
            statistic: self.frame_state.active_statistic,
            period_start: self.frame_state.active_period_start,
            cell: Self::decode_cell(cell),
        }
    }

    fn resolve_global_view(&self, bundle: &Bundle) -> GlobalView {
        let cell: Option<&CellValue> = self
            .active_shard_values(bundle)
            .and_then(|shard_values| shard_values.cell(WORLD_REGION_CODE, self.frame_state.active_period_start));

        GlobalView {
            statistic: self.frame_state.active_statistic,
            period_start: self.frame_state.active_period_start,
            cell: Self::decode_cell(cell),
        }
    }

    /// Hit-tests `surface_point` against the regions, reporting a change only when the region under it
    /// differs from `previous`, so callers skip work on repeat hits over the same region.
    fn region_change(&self, surface_point: SurfacePoint, previous: &Option<RegionCode>) -> RegionChange {
        let region_hit: Option<RegionHit> = self.region_at(surface_point);
        let region_code: Option<RegionCode> =
            region_hit.as_ref().map(|region_hit| region_hit.region_code.clone());

        if region_code == *previous {
            return RegionChange::Unchanged;
        }

        RegionChange::Changed(region_hit)
    }

    /// Updates the selected region from an already-resolved hit (`None` for empty space). Returns the
    /// `SelectionView` to publish, or `None` when the selection is unchanged so no redundant publish happens.
    fn select_region(&mut self, region_hit: Option<RegionHit>) -> Option<Option<SelectionView>> {
        let region_code: Option<RegionCode> =
            region_hit.as_ref().map(|region_hit| region_hit.region_code.clone());

        if region_code == self.frame_state.selected_region {
            return None;
        }

        self.frame_state.selected_region = region_code;
        self.request_redraw();

        let bundle: Arc<Bundle> = self.current_bundle();
        let selection_view: Option<SelectionView> =
            region_hit.map(|region_hit| self.resolve_selection_view(&bundle, &region_hit.region_code.0, &region_hit.name_en));
        self.selection = selection_view.clone();

        match &selection_view {
            Some(view) => log::debug!(
                "region selected; [name={} region_code={} value={:?}]",
                view.name_en,
                view.region_code,
                view.cell.value,
            ),
            None => log::debug!("region deselected"),
        }

        Some(selection_view)
    }

    fn hover_region_at(&mut self, surface_point: SurfacePoint) {
        let RegionChange::Changed(region_hit) = self.region_change(surface_point, &self.frame_state.hovered_region)
        else {
            return;
        };

        self.frame_state.hovered_region = region_hit.map(|region_hit| region_hit.region_code);
        self.request_redraw();
    }

    fn clear_hover(&mut self) {
        if self.frame_state.hovered_region.is_none() {
            return;
        }

        self.frame_state.hovered_region = None;
        self.request_redraw();
    }

    /// Advances the active gesture as a tracked pointer moves: a single pointer pans (and a `Tap` becomes
    /// a `Pan` once it crosses the threshold), two pointers pinch.
    fn apply_pointer_move(&mut self, pointer_id: i32, surface_point: SurfacePoint) {
        match self.gesture {
            Gesture::Tap { pointer, origin } if pointer.pointer_id == pointer_id => {
                self.pan_view(pointer.position, surface_point);

                if origin.euclidean_distance(surface_point) > DRAG_SELECT_SUPPRESS_PX {
                    // The press has become a pan; suppress the pre-press hover highlight so it does not
                    // stay pinned to its region while the map moves.
                    self.gesture = Gesture::Pan { pointer: PointerState { pointer_id, position: surface_point } };
                    self.clear_hover();
                } else {
                    self.gesture = Gesture::Tap { pointer: PointerState { pointer_id, position: surface_point }, origin };
                }
            },
            Gesture::Pan { pointer } if pointer.pointer_id == pointer_id => {
                self.pan_view(pointer.position, surface_point);
                self.gesture = Gesture::Pan { pointer: PointerState { pointer_id, position: surface_point } };
            },
            Gesture::Pinch { first, second } => {
                let (new_first, new_second): (PointerState, PointerState) = if first.pointer_id == pointer_id {
                    (PointerState { pointer_id, position: surface_point }, second)
                } else if second.pointer_id == pointer_id {
                    (first, PointerState { pointer_id, position: surface_point })
                } else {
                    // A third finger moved; it does not drive the pinch.
                    return;
                };

                self.pinch_view(first.position, second.position, new_first.position, new_second.position);
                self.gesture = Gesture::Pinch { first: new_first, second: new_second };
            },
            _ => {},
        }
    }

    fn pan_view(&mut self, from: SurfacePoint, to: SurfacePoint) {
        self.transition = None;
        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        self.viewport = hit_test::pan(self.viewport, self.surface_dimensions, from, to, home_min_y, home_max_y);

        self.request_redraw();
    }

    fn pinch_view(&mut self, previous_a: SurfacePoint, previous_b: SurfacePoint, current_a: SurfacePoint, current_b: SurfacePoint) {
        self.transition = None;
        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        let ceiling: f64 = zoom_out_ceiling_height(self.surface_dimensions);
        self.viewport = hit_test::pinch(
            self.viewport, self.surface_dimensions, previous_a, previous_b, current_a, current_b,
            ceiling, home_min_y, home_max_y,
        );

        self.request_redraw();
    }

    /// Ends a released pointer: a tap (a single pointer that never dragged) selects, and may zoom, via
    /// `select_from_tap`; a pan or pinch does neither. Returns the selection to publish, or `None` when
    /// nothing selects.
    fn end_pointer(&mut self, pointer_id: i32, surface_point: SurfacePoint) -> Option<Option<SelectionView>> {
        match self.gesture.release(pointer_id) {
            PointerRelease::Tap => self.select_from_tap(surface_point),
            PointerRelease::NoSelect => None,
        }
    }

    /// Completes a tap at `surface_point`: selects the region there, and zooms to frame it when the tap
    /// re-selects the already-selected region (so a first tap selects, and a second tap or a double-click
    /// zooms). Returns the selection to publish.
    fn select_from_tap(&mut self, surface_point: SurfacePoint) -> Option<Option<SelectionView>> {
        let region_hit: Option<RegionHit> = self.region_at(surface_point);

        if let Some(hit) = &region_hit {
            let re_taps_the_selection: bool =
                self.frame_state.selected_region.as_ref() == Some(&hit.region_code);

            if re_taps_the_selection {
                self.start_zoom_to_country(hit.framing);
            }
        }

        self.select_region(region_hit)
    }

    /// Ends a canceled pointer (the browser aborted the gesture). Never selects.
    fn cancel_pointer(&mut self, pointer_id: i32) {
        self.gesture.release(pointer_id);
    }

    /// Wheel-zooms toward the cursor: maps the wheel delta to a multiplicative factor (scaled by
    /// `sensitivity`, which differs for a scroll wheel versus a pinch) and zooms about the projected point
    /// under the cursor, clamped to the zoom-out ceiling and the home latitude range.
    fn zoom_at(&mut self, surface_point: SurfacePoint, delta_y: f64, sensitivity: f64) {
        self.transition = None;
        let clamped_delta: f64 = delta_y.clamp(-MAX_WHEEL_DELTA, MAX_WHEEL_DELTA);
        let factor: f64 = (-clamped_delta * sensitivity).exp();

        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        let ceiling: f64 = zoom_out_ceiling_height(self.surface_dimensions);
        self.viewport = hit_test::zoom_at_surface_point(self.viewport, self.surface_dimensions, surface_point, factor, ceiling, home_min_y, home_max_y);

        self.request_redraw();
    }

    fn view_controls(&self, bundle: &Bundle) -> ViewControls {
        let available_statistics: Vec<StatisticKind> = bundle.manifest.statistics.keys().copied().collect();

        let period_range: Option<(NaiveDate, NaiveDate)> =
            self.active_shard_values(bundle).and_then(|shard_values| shard_values.period_range());
        let active_period_end: Option<NaiveDate> = self
            .active_shard_values(bundle)
            .and_then(|shard_values| shard_values.period_end(self.frame_state.active_period_start));

        ViewControls {
            active_statistic: self.frame_state.active_statistic,
            available_statistics,
            active_period_start: self.frame_state.active_period_start,
            active_period_end,
            period_range,
        }
    }

    fn legend_view(&self, bundle: &Bundle) -> LegendView {
        let value_range: Option<(f64, f64)> = self.active_shard_values(bundle)
            .and_then(|shard_values| shard_values.value_range());

        LegendView {
            statistic: self.frame_state.active_statistic,
            value_range,
        }
    }

    fn set_active_statistic(&mut self, statistic: StatisticKind) -> Option<RepublishedViews> {
        if statistic == self.frame_state.active_statistic {
            return None;
        }

        self.frame_state.active_statistic = statistic;

        let bundle: Arc<Bundle> = self.current_bundle();

        /* Two statistics need not cover the same periods, and a cohort measure ends decades before a period
           one. Holding the old period would leave the map blank on a period the new statistic never
           covers. */
        clamp_active_period(self, &bundle);

        self.request_redraw();

        Some(self.republish(&bundle))
    }

    fn scrub_to_period(&mut self, period_start: NaiveDate) -> Option<RepublishedViews> {
        if period_start == self.frame_state.active_period_start {
            return None;
        }

        self.frame_state.active_period_start = period_start;
        self.request_redraw();

        let bundle: Arc<Bundle> = self.current_bundle();

        Some(self.republish(&bundle))
    }

    /// Toggles the hovered-region lift (the "regions expand on hover" setting). Redraws only on a change;
    /// affects rendering alone, so it does not republish the panel or controls.
    fn set_hover_lift_enabled(&mut self, enabled: bool) {
        if self.frame_state.hover_lift_enabled == enabled {
            return;
        }

        self.frame_state.hover_lift_enabled = enabled;
        self.request_redraw();
    }

    /// Re-resolves the retained selection against the current frame state and bundles it with fresh
    /// controls, so a statistic or period change refreshes both the detail panel and the controls.
    fn republish(&mut self, bundle: &Bundle) -> RepublishedViews {
        let identity: Option<(String, String)> = self
            .selection
            .as_ref()
            .map(|selection| (selection.region_code.clone(), selection.name_en.clone()));
        self.selection = identity.map(|(region_code, name_en)| self.resolve_selection_view(bundle, &region_code, &name_en));

        RepublishedViews {
            view_controls: self.view_controls(bundle),
            legend: self.legend_view(bundle),
            selection: self.selection.clone(),
            global: self.resolve_global_view(bundle),
        }
    }
}

/// The two first-paint failure modes `start` distinguishes to choose the panel; each carries the
/// originating error for logging.
enum StartupError {
    /// A transient or data-integrity failure fetching or opening the bundle.
    DataUnavailable(AppError),
    /// A missing hard capability: no Origin Private File System or no usable wgpu backend, both of
    /// which show the unsupported panel.
    BrowserUnsupported(AppError),
}

/// The reactive signals wiring the map component to the driver.
pub struct DriverSignals {
    pub render_status: RwSignal<RenderStatus>,
    pub selection_view: WriteSignal<Option<SelectionView>>,
    pub global_view: WriteSignal<Option<GlobalView>>,
    pub view_controls: WriteSignal<Option<ViewControls>>,
    pub legend: WriteSignal<Option<LegendView>>,
    pub live_load_failed: WriteSignal<bool>,
}

pub fn start(canvas: HtmlCanvasElement, signals: DriverSignals) {
    let DriverSignals { render_status, selection_view, global_view, view_controls, legend, live_load_failed } = signals;

    leptos::task::spawn_local(async move {
        let status: RenderStatus = match set_up_driver(
            canvas,
            selection_view,
            global_view,
            view_controls,
            legend,
            live_load_failed,
        ).await {
            Ok(()) => RenderStatus::Ready,
            Err(StartupError::DataUnavailable(error)) => {
                log::error!("map data could not be loaded [error={error}]");
                RenderStatus::DataUnavailable
            }
            Err(StartupError::BrowserUnsupported(error)) => {
                log::error!("browser is missing a required capability, showing the unsupported panel [error={error}]");
                RenderStatus::Unsupported
            }
        };

        render_status.set(status);
    });
}

async fn set_up_driver(
    canvas: HtmlCanvasElement,
    selection_view: WriteSignal<Option<SelectionView>>,
    global_view: WriteSignal<Option<GlobalView>>,
    view_controls: WriteSignal<Option<ViewControls>>,
    legend: WriteSignal<Option<LegendView>>,
    live_load_failed: WriteSignal<bool>,
) -> Result<(), StartupError> {
    let cache: OpfsArtifactCache = OpfsArtifactCache::create()
        .await
        .map_err(StartupError::BrowserUnsupported)?;

    let distribution_context: DistributionContext = distribution::resolve_context();

    let bundle: Bundle = match load::open_newest_cached_bundle(&cache, distribution_context).await {
        Ok(Some(cached)) => cached,
        Ok(None) => load::load_embedded_bundle(&cache, distribution_context)
            .await
            .map_err(StartupError::DataUnavailable)?,
        Err(error) => {
            log::warn!("opening a cached bundle failed, falling back to embedded; [error={error}]");
            load::load_embedded_bundle(&cache, distribution_context)
                .await
                .map_err(StartupError::DataUnavailable)?
        }
    };

    log::info!(
        "first paint bundle opened; [version_label={} distribution_context={:?} periods={:?}]",
        bundle.manifest.version,
        bundle.distribution_context,
        bundle
            .shard_values_for(StatisticKind::Tfr)
            .and_then(|shard_values| shard_values.period_range()),
    );

    if let Err(error) = load::evict_stale_versions(&cache).await {
        log::warn!("evicting old cached bundle versions failed [error={error}]");
    }

    let frame_state: FrameState = initial_frame_state(&bundle);
    let (bundle_sender, bundle_receiver): (watch::Sender<Arc<Bundle>>, watch::Receiver<Arc<Bundle>>) =
        watch::channel(Arc::new(bundle));

    let backend: RendererBackend = backend_from_query();
    let mut renderer: Renderer = Renderer::new(bundle_receiver, backend)
        .await
        .map_err(StartupError::BrowserUnsupported)?;

    let (width, height): (u32, u32) = configure_canvas_backing_store(&canvas);
    renderer
        .attach_surface_from_canvas(canvas.clone(), width, height)
        .await
        .map_err(StartupError::BrowserUnsupported)?;

    let driver: Driver = Driver {
        renderer,
        bundle_sender,
        viewport: home_viewport(SurfaceDimensions { width, height }),
        surface_dimensions: SurfaceDimensions { width, height },
        frame_state,
        selection_view,
        global_view,
        view_controls,
        legend,
        selection: None,
        redraw_pending: false,
        gesture: Gesture::Idle,
        transition: None,
        animation_frame_pending: false,
        redraw_callback: None,
        animation_callback: None,
        resize_callback: Some(install_resize_listener(&canvas)),
        pointer_down_callback: Some(install_pointer_down_listener(&canvas)),
        pointer_move_callback: Some(install_pointer_move_listener(&canvas)),
        pointer_up_callback: Some(install_pointer_up_listener(&canvas)),
        pointer_cancel_callback: Some(install_pointer_cancel_listener(&canvas)),
        pointer_leave_callback: Some(install_pointer_leave_listener(&canvas)),
        context_menu_callback: Some(install_context_menu_listener(&canvas)),
        lost_pointer_capture_callback: Some(install_lost_pointer_capture_listener(&canvas)),
        wheel_callback: Some(install_wheel_listener(&canvas)),
    };

    let published_bundle: Arc<Bundle> = driver.current_bundle();
    let initial_controls: ViewControls = driver.view_controls(&published_bundle);
    let initial_legend: LegendView = driver.legend_view(&published_bundle);
    let initial_global: GlobalView = driver.resolve_global_view(&published_bundle);
    log::info!(
        "initial global figure resolved; [period_start={} value={:?} source={:?} data_status={:?}]",
        initial_global.period_start,
        initial_global.cell.value,
        initial_global.cell.source,
        initial_global.cell.data_status,
    );
    let live_bundle_sender: watch::Sender<Arc<Bundle>> = driver.bundle_sender.clone();

    DRIVER.with_borrow_mut(|driver_slot| {
        driver_slot.insert(driver).request_redraw();
    });

    view_controls.set(Some(initial_controls));
    legend.set(Some(initial_legend));
    global_view.set(Some(initial_global));

    leptos::task::spawn_local(async move {
        upgrade_to_live_bundle(
            cache,
            distribution_context,
            live_bundle_sender,
            view_controls,
            legend,
            selection_view,
            global_view,
            live_load_failed,
        )
        .await;
    });

    Ok(())
}

async fn upgrade_to_live_bundle(
    cache: OpfsArtifactCache,
    distribution_context: DistributionContext,
    live_bundle_sender: watch::Sender<Arc<Bundle>>,
    view_controls: WriteSignal<Option<ViewControls>>,
    legend: WriteSignal<Option<LegendView>>,
    selection_view: WriteSignal<Option<SelectionView>>,
    global_view: WriteSignal<Option<GlobalView>>,
    live_load_failed: WriteSignal<bool>,
) {
    let static_base: String = match live_resolve::static_repository_base_url() {
        Ok(static_base) => static_base,
        Err(error) => {
            log::warn!("reading the static repository base failed; [error={error}]");
            live_load_failed.set(true);
            return;
        }
    };

    match load::load_live_bundle(&cache, &static_base, distribution_context).await {
        Ok(bundle) => apply_live_bundle(
            live_bundle_sender,
            bundle,
            view_controls,
            legend,
            selection_view,
            global_view,
        ),
        Err(error) => {
            log::warn!("loading the live bundle failed; [error={error}]");
            live_load_failed.set(true);
        }
    }
}

fn apply_live_bundle(
    live_bundle_sender: watch::Sender<Arc<Bundle>>,
    bundle: Bundle,
    view_controls: WriteSignal<Option<ViewControls>>,
    legend: WriteSignal<Option<LegendView>>,
    selection_view: WriteSignal<Option<SelectionView>>,
    global_view: WriteSignal<Option<GlobalView>>,
) {
    if let Err(error) = live_bundle_sender.send(Arc::new(bundle)) {
        log::warn!("publishing the live bundle failed; [error={error}]");
        return;
    }

    let published: Option<RepublishedViews> = DRIVER.with_borrow_mut(|driver_slot| {
        let driver: &mut Driver = driver_slot.as_mut()?;

        let bundle: Arc<Bundle> = driver.current_bundle();
        clamp_active_period(driver, &bundle);

        let views: RepublishedViews = driver.republish(&bundle);
        driver.request_redraw();

        Some(views)
    });

    if let Some(views) = published {
        view_controls.set(Some(views.view_controls));
        legend.set(Some(views.legend));
        selection_view.set(views.selection);
        global_view.set(Some(views.global));
    }
}

/// A period outside what the statistic covers lands on its default, not on the nearest bound.
fn clamp_active_period(driver: &mut Driver, bundle: &Bundle) {
    let Some((earliest, latest)) = driver
        .active_shard_values(bundle)
        .and_then(|shard_values| shard_values.period_range())
    else {
        return;
    };

    let covers_active_period: bool = driver.frame_state.active_period_start >= earliest
        && driver.frame_state.active_period_start <= latest;
    if covers_active_period {
        return;
    }

    if let Some(period_start) = default_period_start(bundle, driver.frame_state.active_statistic) {
        driver.frame_state.active_period_start = period_start;
    }
}

/// Falls back to the Unix epoch when the default statistic's shard is missing, so the map still paints
/// geometry with every region reading "no data".
fn initial_frame_state(bundle: &Bundle) -> FrameState {
    let active_statistic: StatisticKind = StatisticKind::Tfr;
    let active_period_start: NaiveDate = default_period_start(bundle, active_statistic)
        .unwrap_or_else(|| NaiveDate::from_epoch_days(0).expect("day 0 is the Unix epoch"));

    FrameState {
        active_statistic,
        active_period_start,
        selected_region: None,
        hovered_region: None,
        hover_lift_enabled: crate::map::settings::regions_expand_on_hover(),
    }
}

/// Read through `Bundle::shard_values_for` so the seeded period and the coloured shard never disagree about
/// which license class won.
fn default_period_start(bundle: &Bundle, statistic: StatisticKind) -> Option<NaiveDate> {
    let shard_values: &ShardValues = bundle.shard_values_for(statistic)?;

    shard_values.newest_well_covered_period_start(MINIMUM_DEFAULT_COVERAGE_PROPORTION)
}

/// `?renderer=webgl2` forces the WebGL2 backend for developer parity testing. Not a user-facing
/// toggle.
fn backend_from_query() -> RendererBackend {
    let query: String = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();

    let forces_webgl2: bool = query
        .trim_start_matches('?')
        .split('&')
        .any(|parameter| parameter == "renderer=webgl2");

    if forces_webgl2 {
        RendererBackend::ForceGl
    } else {
        RendererBackend::Default
    }
}

/// The home view: the `HOME_VIEW_MIN_LAT`..`HOME_VIEW_MAX_LAT` latitude band fills the surface
/// vertically, centered horizontally on the prime meridian (Greenwich, longitude 0°). Longitude runs at
/// the same isotropic scale, so the surface width shows as much as fits and the rest is reached by
/// panning; the wraparound places the prime meridian at the middle with the world continuing across the
/// seam. Framing a fixed content band rather than the ±85° world keeps the empty ocean below the
/// southernmost land off-screen.
fn home_viewport(surface: SurfaceDimensions) -> Viewport {
    let center_x: f64 = projection::project(HOME_VIEW_MIN_LAT, HOME_CENTER.lon).x;
    let (min_y, max_y): (f64, f64) = home_range_projected_y_bounds();

    Viewport::fill_height(center_x, min_y, max_y, surface)
}

/// The home latitude range's lower and upper bounds in projected space, the vertical limits pan and
/// zoom-out clamp against.
fn home_range_projected_y_bounds() -> (f64, f64) {
    let southern_edge: ProjectedPoint = projection::project(HOME_VIEW_MIN_LAT, HOME_CENTER.lon);
    let northern_edge: ProjectedPoint = projection::project(HOME_VIEW_MAX_LAT, HOME_CENTER.lon);

    (southern_edge.y, northern_edge.y)
}

/// The projected height of the `ZOOM_TO_COUNTRY_MIN_BAND_HALF_LAT` band, the zoom-to-country height floor.
fn zoom_to_country_min_height() -> f64 {
    let northern_edge: ProjectedPoint = projection::project(ZOOM_TO_COUNTRY_MIN_BAND_HALF_LAT, HOME_CENTER.lon);
    let southern_edge: ProjectedPoint = projection::project(-ZOOM_TO_COUNTRY_MIN_BAND_HALF_LAT, HOME_CENTER.lon);

    northern_edge.y - southern_edge.y
}

/// The largest height (furthest zoom-out): the home range, capped so the aspect-locked width never
/// exceeds one world turn. On a surface wider than the range allows within one turn the cap wins, and the
/// furthest zoom-out shows the full world width with a vertical slice of the range rather than the whole
/// of it.
fn zoom_out_ceiling_height(surface: SurfaceDimensions) -> f64 {
    let (min_y, max_y): (f64, f64) = home_range_projected_y_bounds();
    let home_height: f64 = max_y - min_y;
    let width_cap_height: f64 = std::f64::consts::TAU * (surface.height as f64 / surface.width as f64);

    home_height.min(width_cap_height)
}

/// Sizes the canvas's drawing buffer to its displayed size in device pixels so the map renders crisply
/// on high-DPI displays, and returns that size for the surface configuration.
fn configure_canvas_backing_store(canvas: &HtmlCanvasElement) -> (u32, u32) {
    let device_pixel_ratio: f64 = web_sys::window()
        .map(|window| window.device_pixel_ratio())
        .unwrap_or(1.0);

    let width: u32 = scale_to_backing_pixels(canvas.client_width(), device_pixel_ratio);
    let height: u32 = scale_to_backing_pixels(canvas.client_height(), device_pixel_ratio);

    canvas.set_width(width);
    canvas.set_height(height);

    (width, height)
}

fn scale_to_backing_pixels(css_pixels: i32, device_pixel_ratio: f64) -> u32 {
    let scaled: f64 = (css_pixels.max(0) as f64) * device_pixel_ratio;

    (scaled.round() as u32).max(1)
}

fn install_resize_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut()> {
    let canvas: HtmlCanvasElement = canvas.clone();
    let resize_callback: Closure<dyn FnMut()> = Closure::new(move || {
        let (width, height): (u32, u32) = configure_canvas_backing_store(&canvas);

        with_driver(|driver| driver.resize(width, height));
    });

    if let Some(window) = web_sys::window() {
        let _ = window.add_event_listener_with_callback("resize", resize_callback.as_ref().unchecked_ref());
    }

    resize_callback
}

/// Runs `action` against the live driver, or does nothing if the driver is absent or its `RefCell` is
/// already borrowed. DOM event handlers funnel through here rather than `with_borrow_mut` so a synchronous
/// re-entrant dispatch (a pointer event delivered by the browser while an animation frame still holds the
/// borrow across its draw) is dropped instead of panicking the abort-on-panic `RefCell`; the transient
/// hover/gesture state the dropped event would have set is re-established by the next event or frame.
fn with_driver<R>(action: impl FnOnce(&mut Driver) -> R) -> Option<R> {
    DRIVER.with(|cell| {
        let mut slot: std::cell::RefMut<'_, Option<Driver>> = cell.try_borrow_mut().ok()?;
        let driver: &mut Driver = slot.as_mut()?;

        Some(action(driver))
    })
}

fn draw_pending_frame() {
    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.draw();
        }
    });
}

/// The `requestAnimationFrame` callback for the zoom-to-country loop; the browser passes the frame's
/// `performance.now()` timestamp, which the transition samples against.
fn advance_pending_animation(now_ms: f64) {
    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.advance_animation(now_ms);
        }
    });
}

/// The current high-resolution monotonic timestamp (`performance.now()`), the same clock
/// `requestAnimationFrame` stamps its callbacks with. Falls back to `0.0` if `performance` is
/// unavailable, which collapses a started animation to its final frame rather than crashing.
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or(0.0)
}

/// Whether two viewports are equal within a projected-space tolerance, used to skip a zoom-to-country
/// animation whose target is already on screen. The tolerance absorbs the sub-pixel drift of re-deriving
/// a viewport through the fit-and-clamp path.
fn viewports_match(a: Viewport, b: Viewport) -> bool {
    const EPSILON: f64 = 1e-9;

    (a.min.x - b.min.x).abs() < EPSILON
        && (a.min.y - b.min.y).abs() < EPSILON
        && (a.max.x - b.max.x).abs() < EPSILON
        && (a.max.y - b.max.y).abs() < EPSILON
}

fn surface_point_from_mouse_event(event: &MouseEvent) -> SurfacePoint {
    let device_pixel_ratio: f64 = web_sys::window()
        .map(|window| window.device_pixel_ratio())
        .unwrap_or(1.0);

    SurfacePoint {
        x: event.offset_x() as f64 * device_pixel_ratio,
        y: event.offset_y() as f64 * device_pixel_ratio,
    }
}

fn handle_pointer_down(event: &PointerEvent) {
    if !is_map_gesture_button(event.button()) {
        return;
    }

    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);
    let pointer_id: i32 = event.pointer_id();

    capture_pointer(event);

    with_driver(|driver| {
        driver.transition = None;
        driver.gesture.begin(pointer_id, surface_point);
    });
}

fn handle_pointer_move(event: &PointerEvent) {
    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);
    let pointer_id: i32 = event.pointer_id();
    let is_mouse: bool = event.pointer_type() == "mouse";
    let holds_no_button: bool = event.buttons() == 0;

    with_driver(|driver| {
        if is_mouse && holds_no_button {
            // A mouse with nothing held cannot be mid-gesture, so a release that never arrived (delivered
            // to another element, or swallowed) cannot leave a gesture latched and hover switched off.
            driver.gesture.clear();
        }

        if driver.gesture.is_active() {
            driver.apply_pointer_move(pointer_id, surface_point);
        } else if is_mouse {
            // No gesture in progress: a mouse move is a hover; touch and pen produce no hover.
            driver.hover_region_at(surface_point);
        }
    });
}

fn handle_pointer_up(event: &PointerEvent) {
    if !is_map_gesture_button(event.button()) {
        return;
    }

    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);
    let pointer_id: i32 = event.pointer_id();

    let published: Option<(WriteSignal<Option<SelectionView>>, Option<SelectionView>)> =
        with_driver(|driver| {
            let new_selection: Option<SelectionView> = driver.end_pointer(pointer_id, surface_point)?;

            Some((driver.selection_view, new_selection))
        })
        .flatten();

    if let Some((selection_view, new_selection)) = published {
        selection_view.set(new_selection);
    }
}

fn handle_pointer_cancel(event: &PointerEvent) {
    let pointer_id: i32 = event.pointer_id();

    with_driver(|driver| driver.cancel_pointer(pointer_id));
}

fn handle_pointer_leave() {
    with_driver(|driver| driver.clear_hover());
}

fn handle_context_menu(event: &web_sys::Event) {
    event.prevent_default();

    with_driver(|driver| driver.gesture.clear());
}

fn handle_lost_pointer_capture(event: &PointerEvent) {
    let pointer_id: i32 = event.pointer_id();

    with_driver(|driver| driver.cancel_pointer(pointer_id));
}

fn handle_wheel(event: &WheelEvent) {
    // Suppress the page scroll so the wheel zooms the map. The canvas is not a passive-by-default wheel
    // target (only the window, document, and body are), so preventDefault applies here.
    event.prevent_default();

    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);
    let delta_y: f64 = event.delta_y();
    let sensitivity: f64 = if is_trackpad_pinch(event) { TRACKPAD_PINCH_ZOOM_SENSITIVITY } else { WHEEL_ZOOM_SENSITIVITY };

    with_driver(|driver| driver.zoom_at(surface_point, delta_y, sensitivity));
}

/// Whether a wheel event is a trackpad or browser pinch rather than a mouse wheel. Both set ctrlKey, so
/// they are told apart by the delta: a pinch is small and in pixel mode, while a mouse wheel notch is
/// larger or reported in line/page mode.
fn is_trackpad_pinch(event: &WheelEvent) -> bool {
    event.ctrl_key()
        && event.delta_mode() == web_sys::WheelEvent::DOM_DELTA_PIXEL
        && event.delta_y().abs() < TRACKPAD_PINCH_MAX_DELTA
}

/// Captures the pointer to the canvas so a drag keeps delivering move and up events after the pointer
/// leaves the canvas; the browser releases the capture on pointerup/pointercancel.
fn capture_pointer(event: &PointerEvent) {
    let Some(target) = event.target() else {
        return;
    };
    let Ok(element) = target.dyn_into::<web_sys::Element>() else {
        return;
    };

    let _ = element.set_pointer_capture(event.pointer_id());
}

fn publish_mutation(mutate: impl FnOnce(&mut Driver) -> Option<RepublishedViews>) {
    struct PendingPublish {
        controls_signal: WriteSignal<Option<ViewControls>>,
        selection_signal: WriteSignal<Option<SelectionView>>,
        global_signal: WriteSignal<Option<GlobalView>>,
        legend_signal: WriteSignal<Option<LegendView>>,
        views: RepublishedViews,
    }

    let pending: Option<PendingPublish> = with_driver(|driver| {
        let views: RepublishedViews = mutate(driver)?;

        Some(PendingPublish {
            controls_signal: driver.view_controls,
            selection_signal: driver.selection_view,
            global_signal: driver.global_view,
            legend_signal: driver.legend,
            views,
        })
    })
    .flatten();

    if let Some(pending) = pending {
        pending.controls_signal.set(Some(pending.views.view_controls));
        pending.selection_signal.set(pending.views.selection);
        pending.global_signal.set(Some(pending.views.global));
        pending.legend_signal.set(Some(pending.views.legend));
    }
}

pub fn apply_statistic(statistic: StatisticKind) {
    publish_mutation(|driver| driver.set_active_statistic(statistic));
}

pub fn apply_period(period_start: NaiveDate) {
    publish_mutation(|driver| driver.scrub_to_period(period_start));
}

pub fn apply_regions_expand_on_hover(enabled: bool) {
    with_driver(|driver| driver.set_hover_lift_enabled(enabled));
}

fn install_pointer_down_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(PointerEvent)> {
    let pointer_down_callback: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |event: PointerEvent| {
        handle_pointer_down(&event);
    });

    let _ = canvas.add_event_listener_with_callback("pointerdown", pointer_down_callback.as_ref().unchecked_ref());

    pointer_down_callback
}

fn install_pointer_move_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(PointerEvent)> {
    let pointer_move_callback: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |event: PointerEvent| {
        handle_pointer_move(&event);
    });

    let _ = canvas.add_event_listener_with_callback("pointermove", pointer_move_callback.as_ref().unchecked_ref());

    pointer_move_callback
}

fn install_pointer_up_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(PointerEvent)> {
    let pointer_up_callback: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |event: PointerEvent| {
        handle_pointer_up(&event);
    });

    let _ = canvas.add_event_listener_with_callback("pointerup", pointer_up_callback.as_ref().unchecked_ref());

    pointer_up_callback
}

fn install_pointer_cancel_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(PointerEvent)> {
    let pointer_cancel_callback: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |event: PointerEvent| {
        handle_pointer_cancel(&event);
    });

    let _ = canvas.add_event_listener_with_callback("pointercancel", pointer_cancel_callback.as_ref().unchecked_ref());

    pointer_cancel_callback
}

fn install_pointer_leave_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(PointerEvent)> {
    let pointer_leave_callback: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |_event: PointerEvent| {
        handle_pointer_leave();
    });

    let _ = canvas.add_event_listener_with_callback("pointerleave", pointer_leave_callback.as_ref().unchecked_ref());

    pointer_leave_callback
}

fn install_context_menu_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(web_sys::Event)> {
    let context_menu_callback: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |event: web_sys::Event| {
        handle_context_menu(&event);
    });

    let _ = canvas.add_event_listener_with_callback("contextmenu", context_menu_callback.as_ref().unchecked_ref());

    context_menu_callback
}

fn install_lost_pointer_capture_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(PointerEvent)> {
    let lost_pointer_capture_callback: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |event: PointerEvent| {
        handle_lost_pointer_capture(&event);
    });

    let _ = canvas.add_event_listener_with_callback(
        "lostpointercapture",
        lost_pointer_capture_callback.as_ref().unchecked_ref(),
    );

    lost_pointer_capture_callback
}

fn install_wheel_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(WheelEvent)> {
    let wheel_callback: Closure<dyn FnMut(WheelEvent)> = Closure::new(move |event: WheelEvent| {
        handle_wheel(&event);
    });

    let _ = canvas.add_event_listener_with_callback("wheel", wheel_callback.as_ref().unchecked_ref());

    wheel_callback
}
