# Data model: web client

Web-side entities introduced by this feature, the `shared` types they consume, and the construction
sequence the built `shared` crate actually requires (which corrects the spec's Key-Entities section).
No database entities — the web crate persists only opaque artifact bytes to OPFS and holds parsed
`shared` types in memory.

## Reused `shared` types (not redefined in `web/`)

All confirmed against source on `master`.

- `shared::artifact::ArtifactCache` (`artifact/cache.rs:9`) — the trait `OpfsArtifactCache` implements.
  Four async functions, `!Send` futures (the web impl holds `!Send` JS handles):
  - `async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppError>`
  - `async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError>`
  - `async fn list_versions(&self) -> Result<Vec<String>, AppError>`
  - `async fn delete_version(&self, version_label: &str) -> Result<(), AppError>`
- `shared::artifact::Bundle` (`artifact/bundle.rs:34`) — `{ manifest, geometry: GeometryLayer, shard_bytes: BTreeMap<StatisticShardKey, Vec<u8>>, distribution_context }`. `Send + Sync`; holds no SQLite connection.
  - `async fn open<C: ArtifactCache>(cache: &C, version_label: &str, distribution_context: DistributionContext) -> Result<Bundle, AppError>` — reads files from the cache (does NOT fetch), verifies each against the manifest's SHA-256, loads only authorized shard classes.
- `shared::artifact::Manifest` + `manifest::parse_manifest(&[u8]) -> Result<Manifest, AppError>` (`manifest.rs:39`, synchronous) + `manifest::MANIFEST_FILENAME`.
- `shared::artifact::DiscoveryDocument` + `parse_discovery_document(&[u8]) -> Result<DiscoveryDocument, AppError>` (`artifact/discovery.rs:14,21`) — `{ schema_version: u32, repository_base_url: String, minimum_client_version: String, sunset: Option<String> }`. Reuse; do not redefine in `web/`.
- `shared::license::DistributionContext` (`license/license.rs`) — the web embedded + live bundles load under `Embedded` (Base shard class only) unless/until first-party contexts apply.
- `shared::filesystem::{sha256_hex, verify_sha256}` — the loader verifies fetched bytes here before `cache.put` (Topic 1 of the plan: no `core::hashing`).
- `shared::render::WgpuSurface` (`render/surface.rs:6`) — holds only `inner: Surface<'static>` + `config: SurfaceConfiguration`. Constructed inside `shared` (see prerequisites), never in `web/`.
- `shared::map::Renderer` (`map/renderer.rs:33`) — the `!Send` wgpu state machine.
- `shared::map::{Viewport, FrameState, RegionCode, ScreenPoint, SurfaceDimensions}` (`map/value_types.rs`) — the per-frame inputs the web shell owns and passes to `draw_frame`. `FrameState { active_statistic: StatisticKind, active_period_start: NaiveDate, selected_region: Option<RegionCode>, hovered_region: Option<RegionCode> }`.
- `shared::canonical::StatisticKind` — the active statistic the picker sets.

## Corrected construction sequence

The spec (FR-012, Key-Entities lines 159–160) says the surface is passed to `Renderer::new` and the
renderer "owns the `WgpuSurface`." Reality: `new` takes only the bundle receiver; the surface is
attached separately. The web sequence:

1. Build the hot-swap channel: `let (tx, rx) = watch::channel(Arc::new(embedded_bundle));`
2. `let renderer = Renderer::new(rx).await?;` — constructs the wgpu instance/adapter/device and uploads geometry from the initial bundle. (Backend preference param added in Phase 0a for FR-015.)
3. `renderer.attach_surface_from_canvas(canvas, width, height).await?;` — Phase 0a addition; builds the surface from the renderer's own instance/adapter/device and its format-specialized pipelines.
4. Per redraw: `renderer.draw_frame(viewport, frame_state)?;` — synchronous. Internally reads the latest bundle via `self.bundle_receiver.borrow_and_update()`, so a hot-swap is picked up on the next frame with no web-side plumbing beyond publishing on `tx`.
5. On canvas resize: `renderer.resize_surface(width, height)?;`

## Web-side entities

### `OpfsArtifactCache` (`web/src/cache.rs`)

Zero-sized, stateless (FR-018): holds no `FileSystemDirectoryHandle`, no `thread_local!`, no
`RefCell`. Resolves `navigator.storage.getDirectory()` on every call.

- Implements the four `ArtifactCache` trait functions against OPFS at `<opfs-root>/artifacts/<version_label>/<file_relative_path>` (FR-019).
- `evict_old_versions(&self) -> Result<(), AppError>` — web-only (NOT a trait function; built on `list_versions` + `delete_version`). Keeps the two most recent version subtrees by `YYYY-MM-DD+<surname>` lexicographic order, deletes the rest (FR-024). Called once at startup.
- Constructor detects OPFS absence and returns an `AppError` whose message starts with `cache: opfs unsupported` (FR-023).
- On construction: `navigator.storage.persist()` (logged, non-blocking, FR-020).
- `put` consults `navigator.storage.estimate()` and fails with `cache: quota exceeded` when `quota - usage < bytes.len() + 4_194_304` (FR-021); any `QuotaExceededError` from `createWritable()`/`write()` maps to the same prefix (FR-022).

### `thread_local` state (`web/src/map/canvas.rs`)

Single-threaded WASM; wgpu resources are thread-bound; `Renderer` has `&mut self` methods.

- `thread_local! { static RENDERER: RefCell<Renderer> }` (FR-031).
- `thread_local! { static BUNDLE_RX: RefCell<watch::Receiver<Arc<Bundle>>> }` — the loader awaits `changed()` on a clone and schedules a redraw; `draw_frame` reads the value itself.
- `thread_local! { static BUNDLE_TX: watch::Sender<Arc<Bundle>> }` — bare, no `RefCell` (`Sender::send` takes `&self`) (FR-031).
- A dirty flag + a "frame pending" flag guarding `requestAnimationFrame` (FR-013): input handlers and `changed()` wakes set dirty and call `request_redraw()`, which schedules one rAF if none is pending; the callback clears the flags and calls `draw_frame`. No idle rAF loop.

### Loader state machine (`web/src/loader.rs`)

Orchestrates first paint and the background upgrade. Not a persisted entity — a startup async flow.

1. **Embedded first paint**: fetch the embedded manifest from `static/embedded_artifacts/`, `cache.put` its files, `Bundle::open(cache, embedded_version, DistributionContext::Embedded)`, publish on `BUNDLE_TX`. This is the same fetch→cache→open path as the live bundle, pointed at the same origin.
2. **Speculative parallel fetch** (P3): the discovery fetch (`/discovery`) and a speculative manifest fetch against the baked-in `repository_base_url` fire concurrently; reconcile per `client.md` §Speculative parallel fetch (use speculative result when discovery fails or agrees; discard + refetch when discovery reports a different base URL).
3. **Verify + cache + hot-swap**: each fetched file is `verify_sha256`'d against the manifest entry, `cache.put`, and once complete `Bundle::open` produces the live `Arc<Bundle>`, published on `BUNDLE_TX`. Concurrency cap 6 (FR-026).
4. **Returning visit**: if OPFS already holds a current live bundle, open it first as the floor (skip embedded), then run the latest-revision check in the background.

## Static data shapes (verbatim-served)

- `web/static/discovery` — `{ "schema_version": 1, "repository_base_url": "https://repository.eafora.org", "minimum_client_version": "0.1.0", "sunset": null }` (FR-034), parsed by `parse_discovery_document`.
- `web/static/embedded_artifacts/` (gitignored) — a downsampled bundle: `manifest.json` (schema v1) + `geometry/world-50m-<sha8>.fgb` (full 1:50m) + `data/<statistic>-base-<sha8>.sqlite` (World Bank WDI at the United States' most-recent reference year). Produced by `ingestion build` (the downsampled/ subtree of each build) or, interim, hand-built.
