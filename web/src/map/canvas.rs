use leptos::html::Canvas;
use leptos::prelude::*;

use crate::i18n::*;

/// First-paint lifecycle of the map canvas. Server-side rendering leaves it at `Loading`, since the
/// renderer only runs client-side.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))] // the ssr build never runs the renderer, so it constructs only Loading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderStatus {
    /// Shown until the embedded bundle is parsed and the surface is attached.
    Loading,
    Ready,
    /// The browser lacks a hard capability: no OPFS (FR-023) or no usable wgpu backend (FR-016).
    Unsupported,
    /// The bundle could not be fetched or opened.
    DataUnavailable,
}

/// The projected-space bounds of the whole world.
struct WorldBounds {
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
}

/// `x` is longitude passed through unchanged;
/// `y` is the Miller value, so ±85° latitude keeps the poles out of the asymptote.
const WORLD_BOUNDS: WorldBounds = WorldBounds {
    min_lat: -85.0,
    max_lat: 85.0,
    min_lon: -180.0,
    max_lon: 180.0,
};

#[component]
pub fn MapCanvas() -> impl IntoView {
    let canvas_ref: NodeRef<Canvas> = NodeRef::new();
    let render_status: RwSignal<RenderStatus> = RwSignal::new(RenderStatus::Loading);

    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        if let Some(canvas) = canvas_ref.get() {
            driver::start(canvas, render_status);
        }
    });

    let i18n = use_i18n();

    view! {
        <canvas node_ref=canvas_ref id="map-canvas"></canvas>
        {move || match render_status.get() {
            RenderStatus::Ready => ().into_any(),
            RenderStatus::Loading => view! {
                <div class="map-overlay panel"><p>{t!(i18n, controls.loading)}</p></div>
            }
            .into_any(),
            RenderStatus::Unsupported => view! {
                <div class="map-overlay panel"><p>{t!(i18n, map.unsupported)}</p></div>
            }
            .into_any(),
            RenderStatus::DataUnavailable => view! {
                <div class="map-overlay panel"><p>{t!(i18n, map.data_unavailable)}</p></div>
            }
            .into_any(),
        }}
    }
}

/// The browser render driver. wgpu resources and the `Renderer` are single-thread-bound, so they live
/// in thread-locals on the one WASM thread rather than in the reactive graph; the component only holds
/// the `RenderStatus` signal. Compiled solely under `hydrate` (the server never renders wgpu).
#[cfg(feature = "hydrate")]
mod driver {
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;

    use chrono::NaiveDate;
    use tokio::sync::watch;
    use wasm_bindgen::JsCast;
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

    use super::{RenderStatus, WORLD_BOUNDS};

    thread_local! {
        static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
        static BUNDLE_TX: RefCell<Option<watch::Sender<Arc<Bundle>>>> = const { RefCell::new(None) };
        static FRAME: RefCell<Option<(Viewport, FrameState)>> = const { RefCell::new(None) };
        static REDRAW_PENDING: Cell<bool> = const { Cell::new(false) };
        static REDRAW_CALLBACK: RefCell<Option<Closure<dyn FnMut()>>> = const { RefCell::new(None) };
    }

    /// The statistic shown at first paint per FR (P1). The embedded bundle ships exactly one period,
    /// which the initial `FrameState` anchors to.
    const DEFAULT_STATISTIC: StatisticKind = StatisticKind::Tfr;

    /// The two first-paint failure modes the shell distinguishes. `DataUnavailable` is a transient or
    /// data-integrity failure fetching or opening the bundle. `BrowserUnsupported` is a missing hard
    /// capability: no Origin Private File System (FR-023) or no usable wgpu backend (FR-016), both of
    /// which share the unsupported panel. `start` matches on this to choose the panel; the variants
    /// carry the originating error for logging.
    enum StartupError {
        DataUnavailable(AppError),
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
        let cache: OpfsArtifactCache = OpfsArtifactCache::create().await.map_err(StartupError::BrowserUnsupported)?;
        let bundle: Bundle = load::load_embedded_bundle(&cache).await.map_err(StartupError::DataUnavailable)?;
        if let Err(error) = cache.evict_old_versions().await {
            log::warn!("evicting old cached bundle versions failed [error={error}]");
        }

        let frame_state: FrameState = initial_frame_state(&bundle);
        let (bundle_sender, bundle_receiver): (watch::Sender<Arc<Bundle>>, watch::Receiver<Arc<Bundle>>) =
            watch::channel(Arc::new(bundle));

        let backend: RendererBackend = backend_from_query();
        let mut renderer: Renderer =
            Renderer::new(bundle_receiver, backend).await.map_err(StartupError::BrowserUnsupported)?;

        let (width, height): (u32, u32) = configure_canvas_backing_store(&canvas);
        renderer
            .attach_surface_from_canvas(canvas.clone(), width, height)
            .await
            .map_err(StartupError::BrowserUnsupported)?;

        RENDERER.with_borrow_mut(|renderer_slot| *renderer_slot = Some(renderer));
        BUNDLE_TX.with_borrow_mut(|sender_slot| *sender_slot = Some(bundle_sender));
        FRAME.with_borrow_mut(|frame_slot| *frame_slot = Some((world_viewport(), frame_state)));

        install_resize_listener(canvas);
        request_redraw();

        Ok(())
    }

    /// Anchors the initial period to the reference year the embedded bundle ships (its single period).
    /// Falls back to the Unix epoch when the default statistic's shard is missing, so the map still
    /// paints geometry with every region reading "no data".
    fn initial_frame_state(bundle: &Bundle) -> FrameState {
        let active_period_start: NaiveDate = latest_period_start(bundle, DEFAULT_STATISTIC)
            .unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).expect("the Unix epoch is a valid date"));

        FrameState {
            active_statistic: DEFAULT_STATISTIC,
            active_period_start,
            selected_region: None,
            hovered_region: None,
        }
    }

    /// The latest `period_start` in the shard the renderer would color from: the first authorized
    /// license class that ships a shard for the statistic, matching `select_shard`'s policy so the
    /// seeded period and the colored shard never disagree.
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

    /// FR-015: `?renderer=webgl2` forces the WebGL2 backend for developer parity testing. Not a
    /// user-facing toggle.
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

    /// Sizes the canvas's drawing buffer to its displayed size in device pixels so the map renders
    /// crisply on high-DPI displays, and returns that size for the surface configuration.
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

    fn install_resize_listener(canvas: HtmlCanvasElement) {
        let resize_callback: Closure<dyn FnMut()> = Closure::new(move || {
            let (width, height): (u32, u32) = configure_canvas_backing_store(&canvas);

            RENDERER.with_borrow_mut(|renderer_slot| {
                let Some(renderer) = renderer_slot.as_mut() else {
                    return;
                };
                if let Err(error) = renderer.resize_surface(width, height) {
                    log::error!("resizing the render surface failed [error={error}]");
                }
            });

            request_redraw();
        });

        if let Some(window) = web_sys::window() {
            let _ = window.add_event_listener_with_callback("resize", resize_callback.as_ref().unchecked_ref());
        }

        resize_callback.forget();
    }

    /// FR-013: coalesce redraw requests into one `requestAnimationFrame`; there is no idle
    /// refresh-rate loop. The pending flag is committed only once a frame is actually scheduled, so a
    /// failed schedule stays retryable rather than wedging every later redraw.
    fn request_redraw() {
        let already_pending: bool = REDRAW_PENDING.get();
        if already_pending {
            return;
        }

        REDRAW_CALLBACK.with_borrow_mut(|callback_slot| {
            let callback: &Closure<dyn FnMut()> =
                callback_slot.get_or_insert_with(|| Closure::new(draw_pending_frame));

            let Some(window) = web_sys::window() else {
                return;
            };
            if window.request_animation_frame(callback.as_ref().unchecked_ref()).is_ok() {
                REDRAW_PENDING.set(true);
            }
        });
    }

    fn draw_pending_frame() {
        REDRAW_PENDING.set(false);

        let inputs: Option<(Viewport, FrameState)> = FRAME.with_borrow(|frame_slot| frame_slot.clone());
        let Some((viewport, frame_state)) = inputs else {
            return;
        };

        RENDERER.with_borrow_mut(|renderer_slot| {
            let Some(renderer) = renderer_slot.as_mut() else {
                return;
            };
            if let Err(error) = renderer.draw_frame(viewport, frame_state) {
                log::error!("drawing a frame failed [error={error}]");
            }
        });
    }
}
