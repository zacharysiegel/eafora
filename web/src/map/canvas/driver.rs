use std::cell::RefCell;
use std::sync::Arc;

use chrono::NaiveDate;
use tokio::sync::watch;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen::closure::Closure;
use web_sys::HtmlCanvasElement;

use leptos::prelude::*;

use shared::AppError;
use shared::artifact::{Bundle, StatisticShardKey};
use shared::canonical::StatisticKind;
use shared::map::{FrameState, Renderer, RendererBackend, Viewport};
use shared::map::projection;
use shared::sqlite::shard_db;

use crate::client::cache::OpfsArtifactCache;
use crate::client::load;

use super::RenderStatus;

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

/// The render state the browser callbacks reach through the `DRIVER` thread-local, kept outside the
/// reactive graph because `Renderer` owns single-thread-bound, `!Send` wgpu resources. Each JS callback
/// borrows `DRIVER` once and drives it through `&mut self`, so no method re-borrows the thread-local.
struct Driver {
    renderer: Renderer,
    #[allow(dead_code)] // the send half of the bundle channel, held so the channel stays open for later swaps
    bundle_sender: watch::Sender<Arc<Bundle>>,
    viewport: Viewport,
    frame_state: FrameState,
    redraw_pending: bool,
    redraw_callback: Option<Closure<dyn FnMut()>>,
    #[allow(dead_code)] // held to keep the window resize listener's closure alive for the page lifetime
    resize_callback: Option<Closure<dyn FnMut()>>,
}

impl Driver {
    fn draw(&mut self) {
        self.redraw_pending = false;

        if let Err(error) = self.renderer.draw_frame(self.viewport, &self.frame_state) {
            log::error!("drawing a frame failed [error={error}]");
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if let Err(error) = self.renderer.resize_surface(width, height) {
            log::error!("resizing the render surface failed [error={error}]");
        }

        self.request_redraw();
    }

    /// Coalesce redraw requests into one `requestAnimationFrame`; there is no idle refresh
    /// loop. The pending flag is set only once a frame is actually scheduled, so a failed schedule stays
    /// retryable; otherwise the flag would latch and every later redraw would short-circuit on it.
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
            self.redraw_pending = true;
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

pub fn start(canvas: HtmlCanvasElement, render_status: RwSignal<RenderStatus>) {
    leptos::task::spawn_local(async move {
        let status: RenderStatus = match set_up_renderer(canvas).await {
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

async fn set_up_renderer(canvas: HtmlCanvasElement) -> Result<(), StartupError> {
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
        viewport: world_viewport(),
        frame_state,
        redraw_pending: false,
        redraw_callback: None,
        resize_callback: Some(install_resize_listener(canvas)),
    };
    DRIVER.with_borrow_mut(|driver_slot| *driver_slot = Some(driver));

    DRIVER.with_borrow_mut(|driver_slot| {
        if let Some(driver) = driver_slot {
            driver.request_redraw();
        }
    });

    Ok(())
}

/// Anchors the initial period to the reference year the embedded bundle ships (its single period).
/// Falls back to the Unix epoch when the default statistic's shard is missing, so the map still paints
/// geometry with every region reading "no data".
fn initial_frame_state(bundle: &Bundle) -> FrameState {
    let active_statistic: StatisticKind = default_statistic(bundle);
    let active_period_start: NaiveDate = latest_period_start(bundle, active_statistic)
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).expect("the Unix epoch is a valid date"));

    FrameState {
        active_statistic,
        active_period_start,
        selected_region: None,
        hovered_region: None,
    }
}

/// The statistic to show at first paint: the first one the bundle ships an authorized shard for, so the
/// client tracks whatever the downsampled bundle contains. Falls back to `Tfr` only for a bundle with no
/// authorized shards, where nothing colors regardless.
fn default_statistic(bundle: &Bundle) -> StatisticKind {
    bundle
        .shard_bytes
        .keys()
        .next()
        .map(|shard_key| shard_key.statistic_kind)
        .unwrap_or(StatisticKind::Tfr)
}

/// The latest `period_start` in the shard the renderer would color from: the first authorized license
/// class that ships a shard for the statistic, matching `select_shard`'s policy so the seeded period and
/// the colored shard never disagree.
fn latest_period_start(bundle: &Bundle, statistic: StatisticKind) -> Option<NaiveDate> {
    let shard_bytes: &Vec<u8> = bundle
        .distribution_context
        .authorized_classes()
        .iter()
        .find_map(|license_shard_class| {
            bundle.shard_bytes.get(&StatisticShardKey {
                statistic_kind: statistic,
                license_shard_class: *license_shard_class,
            })
        })?;
    let shard_values: shard_db::ShardValues = shard_db::read_shard(shard_bytes).ok()?;

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

fn world_viewport() -> Viewport {
    Viewport {
        min: projection::project(WORLD_BOUNDS.min_lat, WORLD_BOUNDS.min_lon),
        max: projection::project(WORLD_BOUNDS.max_lat, WORLD_BOUNDS.max_lon),
    }
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

fn install_resize_listener(canvas: HtmlCanvasElement) -> Closure<dyn FnMut()> {
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
