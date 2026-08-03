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
use shared::canonical::{DataSourceKind, StatisticKind};
use shared::map::{FrameState, GeoPoint, ProjectedPoint, RegionCode, RegionHit, Renderer, RendererBackend, SurfacePoint, SurfaceDimensions, Viewport};
use shared::map::hit_test;
use shared::map::projection;
use shared::sqlite::shard_db;

use crate::client::cache::OpfsArtifactCache;
use crate::client::load;

use super::{RenderStatus, LegendView, SelectionView, ViewControls};

thread_local! {
    static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
}

/// Washington DC. The home view is centered horizontally on its longitude; vertically it is centered on
/// the home-view latitude framing's midpoint (see `HOME_VIEW_MIN_LAT` / `home_viewport`), so DC drives
/// the horizontal center only.
const HOME_CENTER: GeoPoint = GeoPoint {
    lat: 38.9072,
    lon: -77.0369,
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

/// Caps a single wheel event's `delta_y` magnitude before the zoom factor is computed, so one line- or
/// page-mode notch (whose delta is far larger than a pixel-mode notch) cannot zoom absurdly far. The
/// deltaMode varies by browser and OS; this bounds the raw value rather than interpreting it.
const MAX_WHEEL_DELTA: f64 = 240.0;

/// Pointer travel in device pixels, between press and release, beyond which a single-pointer gesture is a
/// pan rather than a tap, so it does not select. A few-pixel deadzone keeps a click that jitters slightly
/// from being swallowed.
const DRAG_SELECT_SUPPRESS_PX: f64 = 7.0;

/// A pointer in contact (a held mouse button or a touching finger), tracked by `pointerId` so pan and
/// pinch can follow the right one across moves.
#[derive(Debug, Clone, Copy)]
struct PointerState {
    pointer_id: i32,
    position: SurfacePoint,
}

/// The in-progress pointer gesture. Each variant owns exactly the state its stage needs, so combinations
/// like a press origin with no pointer down cannot arise. Only `Tap` selects on release; `Pan` and
/// `Pinch` do not.
#[derive(Debug, Clone, Copy)]
enum Gesture {
    Idle,
    /// One pointer down, not yet past the tap threshold. `origin` is where it pressed, for that test.
    Tap { pointer: PointerState, origin: SurfacePoint },
    /// One pointer dragged past the threshold: panning.
    Pan { pointer: PointerState },
    /// Two pointers down: pinch-zoom. A third finger is ignored, not tracked.
    Pinch { first: PointerState, second: PointerState },
}

/// Whether releasing a pointer completed a tap (so the caller selects) or ended a pan/pinch (so it does
/// not).
enum PointerRelease {
    Tap,
    NoSelect,
}

impl Gesture {
    fn is_active(&self) -> bool {
        !matches!(self, Gesture::Idle)
    }

    /// Transitions on a newly-pressed pointer: the first press is a tap candidate; a second turns the
    /// gesture into a pinch; a third while pinching is ignored.
    fn begin(&mut self, pointer_id: i32, position: SurfacePoint) {
        let pointer: PointerState = PointerState { pointer_id, position };

        *self = match *self {
            Gesture::Idle => Gesture::Tap { pointer, origin: position },
            Gesture::Tap { pointer: existing, .. } | Gesture::Pan { pointer: existing } => {
                Gesture::Pinch { first: existing, second: pointer }
            },
            Gesture::Pinch { first, second } => Gesture::Pinch { first, second },
        };
    }

    /// Transitions on a released or canceled pointer: a single-pointer gesture ends, a pinch collapses to
    /// a pan of the remaining pointer (no jump, since that pointer keeps its position). Reports whether a
    /// tap completed so the caller can decide to select; a pan or pinch reports `NoSelect`.
    fn release(&mut self, pointer_id: i32) -> PointerRelease {
        match *self {
            Gesture::Tap { pointer, .. } if pointer.pointer_id == pointer_id => {
                *self = Gesture::Idle;
                PointerRelease::Tap
            },
            Gesture::Pan { pointer } if pointer.pointer_id == pointer_id => {
                *self = Gesture::Idle;
                PointerRelease::NoSelect
            },
            Gesture::Pinch { first, second } if first.pointer_id == pointer_id || second.pointer_id == pointer_id => {
                let remaining: PointerState = if first.pointer_id == pointer_id { second } else { first };
                *self = Gesture::Pan { pointer: remaining };
                PointerRelease::NoSelect
            },
            _ => PointerRelease::NoSelect,
        }
    }
}

/// The result of hit-testing a pointer against the regions, compared to the previously known region.
enum RegionChange {
    /// The pointer is over the same region as before, or still over none; nothing to update.
    Unchanged,
    /// The pointer moved to a different region, or off all regions (`None`).
    Changed(Option<RegionHit>),
}

/// What a statistic or period change republishes: fresh controls and legend extent, plus the
/// re-resolved selection when a region is selected.
struct RepublishedViews {
    view_controls: ViewControls,
    legend: LegendView,
    selection: Option<SelectionView>,
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
    view_controls: WriteSignal<Option<ViewControls>>,
    legend: WriteSignal<Option<LegendView>>,
    selection: Option<SelectionView>,
    redraw_pending: bool,
    gesture: Gesture,
    redraw_callback: Option<Closure<dyn FnMut()>>,
    resize_callback: Option<Closure<dyn FnMut()>>,
    pointer_down_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    pointer_move_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    pointer_up_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    pointer_cancel_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
    pointer_leave_callback: Option<Closure<dyn FnMut(PointerEvent)>>,
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
        self.surface_dimensions = SurfaceDimensions { width, height };

        // Preserve the current pan/zoom across a resize or device-pixel-ratio change, re-fitting only the
        // aspect; do not reset to the home view.
        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        let ceiling: f64 = zoom_out_ceiling_half_height(self.surface_dimensions);
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

    fn region_at(&self, surface_point: SurfacePoint) -> Option<RegionHit> {
        let bundle: Arc<Bundle> = self.bundle_sender.borrow().clone();

        hit_test::region_at_point(&bundle.geometry, self.viewport, self.surface_dimensions, surface_point)
    }

    /// Reads the shard the map colors from (the active statistic's first authorized license class),
    /// logging and dropping a read failure so the caller degrades to "no data" rather than propagating.
    fn read_active_shard(&self) -> Option<shard_db::ShardValues> {
        let bundle: Arc<Bundle> = self.bundle_sender.borrow().clone();
        let shard_bytes: &Vec<u8> = bundle.shard_for(self.frame_state.active_statistic)?;

        shard_db::read_shard(shard_bytes)
            .map_err(|error| log::error!("reading the shard for the active statistic failed [statistic={:?} error={error}]", self.frame_state.active_statistic))
            .ok()
    }

    fn resolve_selection_view(&self, iso3: &str, name_en: &str) -> SelectionView {
        let cell: Option<shard_db::CellValue> = self
            .read_active_shard()
            .and_then(|shard_values| shard_values.cell(iso3, self.frame_state.active_period_start).cloned());

        let value: Option<f64> = cell.as_ref().map(|cell| cell.value);
        let source: Option<DataSourceKind> = cell.as_ref().and_then(|cell| {
            DataSourceKind::try_from(cell.source_code.as_str())
                .map_err(|error| log::warn!("shard cell has an unrecognized data source [code={} error={error}]", cell.source_code))
                .ok()
        });

        SelectionView {
            iso3: iso3.to_string(),
            name_en: name_en.to_string(),
            statistic: self.frame_state.active_statistic,
            period_start: self.frame_state.active_period_start,
            value,
            source,
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

    /// Updates the selected region from `surface_point`. Returns the `SelectionView` to publish, or
    /// `None` when the selection is unchanged so no redundant publish happens.
    fn select_region_at(&mut self, surface_point: SurfacePoint) -> Option<Option<SelectionView>> {
        let RegionChange::Changed(region_hit) = self.region_change(surface_point, &self.frame_state.selected_region)
        else {
            return None;
        };

        self.frame_state.selected_region = region_hit.as_ref().map(|region_hit| region_hit.region_code.clone());
        self.request_redraw();

        let selection_view: Option<SelectionView> =
            region_hit.map(|region_hit| self.resolve_selection_view(&region_hit.iso3, &region_hit.name_en));
        self.selection = selection_view.clone();

        match &selection_view {
            Some(view) => log::info!("region selected [name={} iso3={} value={:?}]", view.name_en, view.iso3, view.value),
            None => log::info!("region deselected"),
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
    fn drive_pointer_move(&mut self, pointer_id: i32, surface_point: SurfacePoint) {
        match self.gesture {
            Gesture::Tap { pointer, origin } if pointer.pointer_id == pointer_id => {
                self.pan_view(pointer.position, surface_point);

                if hit_test::surface_distance(origin, surface_point) > DRAG_SELECT_SUPPRESS_PX {
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
        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        self.viewport = hit_test::pan(self.viewport, self.surface_dimensions, from, to, home_min_y, home_max_y);

        self.request_redraw();
    }

    fn pinch_view(&mut self, previous_a: SurfacePoint, previous_b: SurfacePoint, current_a: SurfacePoint, current_b: SurfacePoint) {
        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        let ceiling: f64 = zoom_out_ceiling_half_height(self.surface_dimensions);
        self.viewport = hit_test::pinch(
            self.viewport, self.surface_dimensions, previous_a, previous_b, current_a, current_b,
            ceiling, home_min_y, home_max_y,
        );

        self.request_redraw();
    }

    /// Ends a released pointer. A tap (a single pointer that never dragged) selects at the release point;
    /// a pan or pinch does not. Returns the selection to publish, or `None` when nothing selects.
    fn end_pointer(&mut self, pointer_id: i32, surface_point: SurfacePoint) -> Option<Option<SelectionView>> {
        match self.gesture.release(pointer_id) {
            PointerRelease::Tap => self.select_region_at(surface_point),
            PointerRelease::NoSelect => None,
        }
    }

    /// Ends a canceled pointer (the browser aborted the gesture). Never selects.
    fn cancel_pointer(&mut self, pointer_id: i32) {
        self.gesture.release(pointer_id);
    }

    /// Wheel-zooms toward the cursor: maps the wheel delta to a multiplicative factor and zooms about the
    /// projected point under the cursor, clamped to the zoom-out ceiling and the home latitude range.
    fn zoom_at(&mut self, surface_point: SurfacePoint, delta_y: f64) {
        let clamped_delta: f64 = delta_y.clamp(-MAX_WHEEL_DELTA, MAX_WHEEL_DELTA);
        let factor: f64 = (-clamped_delta * WHEEL_ZOOM_SENSITIVITY).exp();

        let (home_min_y, home_max_y): (f64, f64) = home_range_projected_y_bounds();
        let ceiling: f64 = zoom_out_ceiling_half_height(self.surface_dimensions);
        self.viewport = hit_test::zoom_at_surface_point(self.viewport, self.surface_dimensions, surface_point, factor, ceiling, home_min_y, home_max_y);

        self.request_redraw();
    }

    fn view_controls(&self) -> ViewControls {
        let bundle: Arc<Bundle> = self.bundle_sender.borrow().clone();
        let available_statistics: Vec<StatisticKind> = bundle.manifest.statistics.keys().copied().collect();

        let period_range: Option<(NaiveDate, NaiveDate)> =
            self.read_active_shard().and_then(|shard_values| shard_values.period_range());

        ViewControls {
            active_statistic: self.frame_state.active_statistic,
            available_statistics,
            active_period_start: self.frame_state.active_period_start,
            period_range,
        }
    }

    fn legend_view(&self) -> LegendView {
        let value_range: Option<(f64, f64)> = self.read_active_shard()
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
        self.request_redraw();

        Some(self.republish())
    }

    fn scrub_to_period(&mut self, period_start: NaiveDate) -> Option<RepublishedViews> {
        if period_start == self.frame_state.active_period_start {
            return None;
        }

        self.frame_state.active_period_start = period_start;
        self.request_redraw();

        Some(self.republish())
    }

    /// Re-resolves the retained selection against the current frame state and bundles it with fresh
    /// controls, so a statistic or period change refreshes both the detail panel and the controls.
    fn republish(&mut self) -> RepublishedViews {
        let identity: Option<(String, String)> = self
            .selection
            .as_ref()
            .map(|selection| (selection.iso3.clone(), selection.name_en.clone()));
        self.selection = identity.map(|(iso3, name_en)| self.resolve_selection_view(&iso3, &name_en));

        RepublishedViews {
            view_controls: self.view_controls(),
            legend: self.legend_view(),
            selection: self.selection.clone(),
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
    pub view_controls: WriteSignal<Option<ViewControls>>,
    pub legend: WriteSignal<Option<LegendView>>,
}

pub fn start(canvas: HtmlCanvasElement, signals: DriverSignals) {
    let DriverSignals { render_status, selection_view, view_controls, legend } = signals;

    leptos::task::spawn_local(async move {
        let status: RenderStatus = match set_up_driver(canvas, selection_view, view_controls, legend).await {
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

async fn set_up_driver(canvas: HtmlCanvasElement, selection_view: WriteSignal<Option<SelectionView>>, view_controls: WriteSignal<Option<ViewControls>>, legend: WriteSignal<Option<LegendView>>) -> Result<(), StartupError> {
    let cache: OpfsArtifactCache = OpfsArtifactCache::create()
        .await
        .map_err(StartupError::BrowserUnsupported)?;
    let bundle: Bundle = load::load_embedded_bundle(&cache)
        .await
        .map_err(StartupError::DataUnavailable)?;

    if let Err(error) = cache.evict_old_versions().await {
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
        view_controls,
        legend,
        selection: None,
        redraw_pending: false,
        gesture: Gesture::Idle,
        redraw_callback: None,
        resize_callback: Some(install_resize_listener(&canvas)),
        pointer_down_callback: Some(install_pointer_down_listener(&canvas)),
        pointer_move_callback: Some(install_pointer_move_listener(&canvas)),
        pointer_up_callback: Some(install_pointer_up_listener(&canvas)),
        pointer_cancel_callback: Some(install_pointer_cancel_listener(&canvas)),
        pointer_leave_callback: Some(install_pointer_leave_listener(&canvas)),
        wheel_callback: Some(install_wheel_listener(&canvas)),
    };

    let initial_controls: ViewControls = driver.view_controls();
    let initial_legend: LegendView = driver.legend_view();

    DRIVER.with_borrow_mut(|driver_slot| {
        driver_slot.insert(driver).request_redraw();
    });

    view_controls.set(Some(initial_controls));
    legend.set(Some(initial_legend));

    Ok(())
}

/// Anchors the initial period to the reference year the embedded bundle ships (its single period).
/// Falls back to the Unix epoch when the default statistic's shard is missing, so the map still paints
/// geometry with every region reading "no data".
fn initial_frame_state(bundle: &Bundle) -> FrameState {
    let active_statistic: StatisticKind = StatisticKind::Tfr;
    let active_period_start: NaiveDate = latest_period_start(bundle, active_statistic)
        .unwrap_or_else(|| NaiveDate::from_epoch_days(0).expect("day 0 is the Unix epoch"));

    FrameState {
        active_statistic,
        active_period_start,
        selected_region: None,
        hovered_region: None,
    }
}

/// The latest `period_start` in the shard the renderer would color from: the first authorized license
/// class that ships a shard for the statistic, matching `select_shard`'s policy so the seeded period and
/// the colored shard never disagree.
fn latest_period_start(bundle: &Bundle, statistic: StatisticKind) -> Option<NaiveDate> {
    let shard_bytes: &Vec<u8> = bundle.shard_for(statistic)?;
    let shard_values: shard_db::ShardValues = shard_db::read_shard(shard_bytes)
        .map_err(|error| log::error!("reading the shard to seed the initial period failed [statistic={statistic:?} error={error}]"))
        .ok()?;

    shard_values.period_range().map(|(_earliest, latest)| latest)
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
/// vertically, centered horizontally on Washington DC's longitude. Longitude runs at the same isotropic
/// scale, so the surface width shows as much as fits and the rest is reached by panning; the wraparound
/// places DC at the middle with the world continuing across the seam. Framing a fixed content band
/// rather than the ±85° world keeps the empty ocean below the southernmost land off-screen.
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

/// The largest half-height (furthest zoom-out): the home range, capped so the aspect-locked width never
/// exceeds one world turn. On a surface wider than the range allows within one turn the cap wins, and the
/// furthest zoom-out shows the full world width with a vertical slice of the range rather than the whole
/// of it.
fn zoom_out_ceiling_half_height(surface: SurfaceDimensions) -> f64 {
    let (min_y, max_y): (f64, f64) = home_range_projected_y_bounds();
    let home_half_height: f64 = (max_y - min_y) / 2.0;
    let width_cap_half_height: f64 = std::f64::consts::PI * (surface.height as f64 / surface.width as f64);

    home_half_height.min(width_cap_half_height)
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

        DRIVER.with_borrow_mut(|driver_slot| {
            if let Some(driver) = driver_slot {
                driver.resize(width, height);
            }
        });
    });

    if let Some(window) = web_sys::window() {
        let _ = window.add_event_listener_with_callback("resize", resize_callback.as_ref().unchecked_ref());
    }

    resize_callback
}

fn draw_pending_frame() {
    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.draw();
        }
    });
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
    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);
    let pointer_id: i32 = event.pointer_id();

    capture_pointer(event);

    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.gesture.begin(pointer_id, surface_point);
        }
    });
}

fn handle_pointer_move(event: &PointerEvent) {
    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);
    let pointer_id: i32 = event.pointer_id();
    let is_mouse: bool = event.pointer_type() == "mouse";

    DRIVER.with_borrow_mut(|driver_slot| {
        let Some(driver) = driver_slot else {
            return;
        };

        if driver.gesture.is_active() {
            driver.drive_pointer_move(pointer_id, surface_point);
        } else if is_mouse {
            // No gesture in progress: a mouse move is a hover; touch and pen produce no hover.
            driver.hover_region_at(surface_point);
        }
    });
}

fn handle_pointer_up(event: &PointerEvent) {
    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);
    let pointer_id: i32 = event.pointer_id();

    let published: Option<(WriteSignal<Option<SelectionView>>, Option<SelectionView>)> =
        DRIVER.with_borrow_mut(|driver_slot| {
            let driver: &mut Driver = driver_slot.as_mut()?;
            let new_selection: Option<SelectionView> = driver.end_pointer(pointer_id, surface_point)?;

            Some((driver.selection_view, new_selection))
        });

    if let Some((selection_view, new_selection)) = published {
        selection_view.set(new_selection);
    }
}

fn handle_pointer_cancel(event: &PointerEvent) {
    let pointer_id: i32 = event.pointer_id();

    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.cancel_pointer(pointer_id);
        }
    });
}

fn handle_pointer_leave() {
    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.clear_hover();
        }
    });
}

fn handle_wheel(event: &WheelEvent) {
    // Suppress the page scroll so the wheel zooms the map. The canvas is not a passive-by-default wheel
    // target (only the window, document, and body are), so preventDefault applies here.
    event.prevent_default();

    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);
    let delta_y: f64 = event.delta_y();

    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.zoom_at(surface_point, delta_y);
        }
    });
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
        legend_signal: WriteSignal<Option<LegendView>>,
        views: RepublishedViews,
    }

    let pending: Option<PendingPublish> = DRIVER.with_borrow_mut(|driver_slot| {
        let driver: &mut Driver = driver_slot.as_mut()?;
        let views: RepublishedViews = mutate(driver)?;

        Some(PendingPublish {
            controls_signal: driver.view_controls,
            selection_signal: driver.selection_view,
            legend_signal: driver.legend,
            views,
        })
    });

    if let Some(pending) = pending {
        pending.controls_signal.set(Some(pending.views.view_controls));
        pending.selection_signal.set(pending.views.selection);
        pending.legend_signal.set(Some(pending.views.legend));
    }
}

pub fn apply_statistic(statistic: StatisticKind) {
    publish_mutation(|driver| driver.set_active_statistic(statistic));
}

pub fn apply_period(period_start: NaiveDate) {
    publish_mutation(|driver| driver.scrub_to_period(period_start));
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

fn install_wheel_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(WheelEvent)> {
    let wheel_callback: Closure<dyn FnMut(WheelEvent)> = Closure::new(move |event: WheelEvent| {
        handle_wheel(&event);
    });

    let _ = canvas.add_event_listener_with_callback("wheel", wheel_callback.as_ref().unchecked_ref());

    wheel_callback
}
