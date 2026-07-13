# Contract: web module surface + cross-crate prerequisites

The public surface of each `web/src` module and the exact `shared` / `ingestion` additions the
prerequisite PRs introduce. Signatures are the contract; bodies are implementation detail. Types are
imported directly; free functions and constants are called through their parent module per the
project convention.

## Prerequisite additions

### Phase 0a — `shared` (own PR, off `master`)

`shared/src/render/surface.rs` — new `#[cfg(target_arch = "wasm32")] mod wasm` (mirrors the existing
`mod native`, scoping its own `use`), adding one inherent function to `WgpuSurface`:

```rust
// #[cfg(target_arch = "wasm32")] mod wasm
impl WgpuSurface {
    pub fn from_canvas(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<WgpuSurface, AppError>;
}
```

It builds the surface with the safe API — `instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas))`
(no `unsafe`) — then derives the same `SurfaceConfiguration` shape as `from_window_handle` and
`configure`s it. The shared `WgpuSurface` type + its format/resize/reconfigure functions are unchanged.

`shared/src/map/renderer.rs` — two changes:

```rust
// backend preference for FR-015 (?renderer=webgl2). Native callers pass Default.
pub enum RendererBackends {
    Default,   // BROWSER_WEBGPU | GL
    ForceGl,   // WebGL2 only
}

impl Renderer {
    // signature change: new() takes the backend preference (was: new(bundle_receiver) only)
    pub async fn new(
        bundle_receiver: watch::Receiver<Arc<Bundle>>,
        backends: RendererBackends,
    ) -> Result<Renderer, AppError>;

    #[cfg(target_arch = "wasm32")] // not for non-wasm32: attaches from an HtmlCanvasElement, not a window handle
    pub async fn attach_surface_from_canvas(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<(), AppError>;
}
```

`new` builds the `Instance` from `backends` instead of `Instance::default()`. `attach_surface_from_canvas`
calls `WgpuSurface::from_canvas(&self.instance, &self.adapter, &self.device, canvas, width, height)?`
then the existing private `self.attach(surface).await`. Both use `self.instance` / `self.adapter` on
wasm32, clearing the deferred dead-code warnings. `shared/Cargo.toml` gains `web-sys` (with the
`HtmlCanvasElement` feature) under `[target.'cfg(target_arch = "wasm32")'.dependencies]`, gated behind
the `render` feature.

The native `attach_surface`, `detach_surface`, `resize_surface`, and `draw_frame` signatures are
unchanged; native callers update the one `Renderer::new` call site to pass `RendererBackends::Default`.

### Phase 0b — `ingestion` (own PR, off `master`)

`ingestion/src/main.rs` — the existing `build` subcommand gains a `--downsampled <output-dir>` flag
(clap builder API). Contract: emits a bundle whose geometry is the full 1:50m FlatGeobuf (unchanged)
and whose statistic shards keep only the most-recent-year value per country per statistic, into
`<output-dir>/latest/`. The manifest schema is identical to the live bundle's; only the shard row
counts differ. Reuses the existing `build_artifacts` path with a downsampling filter at shard emission.
Independent of Phase 0a and of the web stack.

## `web/src` module surface

### `lib.rs` / `main.rs`

```rust
// lib.rs — the wasm32 (hydrate) entrypoint
include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));
use i18n::*;

#[wasm_bindgen(start)]
pub fn hydrate();   // installs console_error_panic_hook + console_log, then mounts App

// main.rs — the ssr entrypoint (no-op pass-through this feature)
// resolves LeptosOptions, calls static_routes.generate(&options).await, exits.
```

### `app.rs`

```rust
#[component]
pub fn App() -> impl IntoView;   // <I18nContextProvider> wrapping a <Routes> tree with only "/" -> MapView
```

### `cache.rs`

```rust
pub struct OpfsArtifactCache;   // zero-sized (FR-018)

impl OpfsArtifactCache {
    pub async fn create() -> Result<OpfsArtifactCache, AppError>;   // "cache: opfs unsupported" on missing OPFS; requests persist()
    pub async fn evict_old_versions(&self) -> Result<(), AppError>; // keep 2 most recent (FR-024)
}

impl shared::artifact::ArtifactCache for OpfsArtifactCache { /* put/get/list_versions/delete_version */ }
```

### `fetch.rs`

```rust
pub async fn fetch_manifest(repository_base_url: &str) -> Result<Vec<u8>, AppError>;
pub async fn fetch_artifact_file(repository_base_url: &str, version_label: &str, relative_path: &str) -> Result<Vec<u8>, AppError>;
pub async fn fetch_discovery(discovery_url: &str) -> Result<shared::artifact::DiscoveryDocument, AppError>;
```

All three call `web_sys` `window().fetch_with_str(...)` through `wasm_bindgen_futures::JsFuture` (the
wire is visible; no abstraction). Non-2xx maps to an `AppError` carrying the URL + HTTP status (FR-041).

### `loader.rs`

```rust
// Orchestrates first paint + background upgrade; publishes on BUNDLE_TX. Cap concurrent shard fetches at 6.
pub async fn run_startup_load(cache: &OpfsArtifactCache) -> Result<(), AppError>;
```

### `map/` components

```rust
#[component] pub fn MapView() -> impl IntoView;    // canvas + Legend + Controls, absolute-positioned per the stub
#[component] pub fn MapCanvas() -> impl IntoView;   // owns <canvas>; RENDERER thread_local; request_redraw() + rAF; input handlers
#[component] pub fn Legend() -> impl IntoView;      // choropleth intensity legend (bottom-left)
#[component] pub fn Controls() -> impl IntoView;    // statistic picker + year scrubber + source panel + detail panel
```

`MapCanvas` bridges the DOM canvas to the renderer via `RENDERER.with_borrow_mut(|r| r.attach_surface_from_canvas(canvas, w, h))`
(after `Renderer::new(rx, backends).await`), forces `RendererBackends::ForceGl` when the URL carries
`?renderer=webgl2` (FR-015), and on adapter-request failure renders the plain-HTML unsupported panel
(FR-016) instead of the canvas.

## Static file contracts

- `web/static/discovery` — the `DiscoveryDocument` JSON (FR-034); served with `Content-Type: application/json`, `Cache-Control: public, max-age=3600` per `_headers`.
- `web/static/_headers` — per-path response headers (FR-033); content-hashed assets get `public, max-age=31536000, immutable`; rationale in a comment block at the file head.
- `web/wrangler.toml` — `name = "eafora-web"`, a `compatibility_date`, `[assets] directory = "../target/site"`; no `main`, no Worker script (FR-032). Deploy: `wrangler deploy`.
