use std::cell::RefCell;
use std::sync::Arc;

use chrono::NaiveDate;
use tokio::sync::watch;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen::closure::Closure;
use web_sys::{HtmlCanvasElement, MouseEvent};

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

struct WorldBounds {
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
}

/// The whole-world extent in degrees. Latitude is clamped to ±85° to keep the poles out of the Miller
/// projection's asymptote; longitude spans the full ±180°.
const WORLD_BOUNDS: WorldBounds = WorldBounds {
    min_lat: -85.0,
    max_lat: 85.0,
    min_lon: -180.0,
    max_lon: 180.0,
};

/// Washington DC. The home view is centered horizontally on its longitude; the vertical center stays
/// on the equator so the full latitude range shows.
const HOME_CENTER: GeoPoint = GeoPoint {
    lat: 38.9072,
    lon: -77.0369,
};

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
    redraw_callback: Option<Closure<dyn FnMut()>>,
    resize_callback: Option<Closure<dyn FnMut()>>,
    click_callback: Option<Closure<dyn FnMut(MouseEvent)>>,
    mousemove_callback: Option<Closure<dyn FnMut(MouseEvent)>>,
    mouseleave_callback: Option<Closure<dyn FnMut(MouseEvent)>>,
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
        self.viewport = home_viewport(self.surface_dimensions);

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
    }

    fn clear_hover(&mut self) {
        self.frame_state.hovered_region = None;
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
        redraw_callback: None,
        resize_callback: Some(install_resize_listener(&canvas)),
        click_callback: Some(install_click_listener(&canvas)),
        mousemove_callback: Some(install_mousemove_listener(&canvas)),
        mouseleave_callback: Some(install_mouseleave_listener(&canvas)),
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

/// The home view: the whole world fit to the surface aspect, centered horizontally on Washington DC's
/// longitude. The full latitude range shows (vertical center on the equator); the horizontal wraparound
/// places DC at the middle with the world continuing across the seam.
fn home_viewport(surface: SurfaceDimensions) -> Viewport {
    let world_min: ProjectedPoint = projection::project(WORLD_BOUNDS.min_lat, WORLD_BOUNDS.min_lon);
    let world_max: ProjectedPoint = projection::project(WORLD_BOUNDS.max_lat, WORLD_BOUNDS.max_lon);

    let center: ProjectedPoint = ProjectedPoint {
        x: projection::project(HOME_CENTER.lat, HOME_CENTER.lon).x,
        y: (world_min.y + world_max.y) / 2.0,
    };
    let half_width: f64 = (world_max.x - world_min.x) / 2.0;
    let half_height: f64 = (world_max.y - world_min.y) / 2.0;

    Viewport::fit(center, half_width, half_height, surface)
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

fn handle_click(event: &MouseEvent) {
    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);

    let published: Option<(WriteSignal<Option<SelectionView>>, Option<SelectionView>)> =
        DRIVER.with_borrow_mut(|driver_slot| {
            let driver: &mut Driver = driver_slot.as_mut()?;
            let new_selection: Option<SelectionView> = driver.select_region_at(surface_point)?;

            Some((driver.selection_view, new_selection))
        });

    if let Some((selection_view, new_selection)) = published {
        selection_view.set(new_selection);
    }
}

fn handle_mousemove(event: &MouseEvent) {
    let surface_point: SurfacePoint = surface_point_from_mouse_event(event);

    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.hover_region_at(surface_point);
        }
    });
}

fn handle_mouseleave() {
    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.clear_hover();
        }
    });
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

fn install_click_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(MouseEvent)> {
    let click_callback: Closure<dyn FnMut(MouseEvent)> = Closure::new(move |event: MouseEvent| {
        handle_click(&event);
    });

    let _ = canvas.add_event_listener_with_callback("click", click_callback.as_ref().unchecked_ref());

    click_callback
}

fn install_mousemove_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(MouseEvent)> {
    let mousemove_callback: Closure<dyn FnMut(MouseEvent)> = Closure::new(move |event: MouseEvent| {
        handle_mousemove(&event);
    });

    let _ = canvas.add_event_listener_with_callback("mousemove", mousemove_callback.as_ref().unchecked_ref());

    mousemove_callback
}

fn install_mouseleave_listener(canvas: &HtmlCanvasElement) -> Closure<dyn FnMut(MouseEvent)> {
    let mouseleave_callback: Closure<dyn FnMut(MouseEvent)> = Closure::new(move |_event: MouseEvent| {
        handle_mouseleave();
    });

    let _ = canvas.add_event_listener_with_callback("mouseleave", mouseleave_callback.as_ref().unchecked_ref());

    mouseleave_callback
}
