# Web client architecture

> **Status: draft, 2026-06-17.** This document is the per-platform deep-dive companion to `docs/architecture/client.md` (cross-cutting client architecture) and `docs/architecture/overview.md` (system overview). It covers everything specific to the **web** client surface: the Cargo workspace member, the build toolchain, the Leptos shell, the OPFS-backed cache, the wgpu/canvas bridge, the static-asset embedded bundle handling, the deploy target, and the testing strategy. Per-platform deltas for iOS and Android live in `client-ios.md` and `client-android.md` (sibling branches; iOS is developed in parallel with web).

## Scope of this document

This document covers everything between **the consumer-side contract `client.md` defines** and **a wasm + HTML + CSS bundle deployed to Cloudflare Workers Assets**:

- The `web/` Cargo workspace member: its layout, its dependencies, its relationship to `core/`.
- The build toolchain: `cargo-leptos`, `wasm-bindgen`, `wasm-opt`, the static-asset embedded bundle copy step, and the perf-budget warning that backstops `client.md`'s 2 MB / 3 MB targets.
- The Leptos shell: routing (client-side map view + SSG region-detail pages via Leptos's `SsrMode::Static`), the component layout, the wasm-bindgen surface boundaries, the CSS architecture (Sass partials, hand-written to match `docs/design/README.md`).
- The browser-platform glue: the `fetch()`-backed artifact loader, the OPFS-backed cache adapter that satisfies `core::artifact`'s cache contract, the canvas-to-wgpu-surface bridge, the WebGPU/WebGL2 fallback policy.
- The deploy target: Cloudflare Workers Assets, headers, and the manifest-vs-immutable cache disposition that mirrors the producer-side R2 settings.
- Testing strategy for the web-only TDD-required surfaces.

Cross-cutting client behavior (artifact-consumption contract, fetch / cache / load pipeline shape, SQLite-in-the-client, FlatGeobuf reading, license-shard composition, embedded bundle semantics, hot-swap protocol) is in `client.md` and is **not relitigated here**. The visual identity (sharp white-and-red, square corners, 1px borders, no shadows, only the zoom-to-country animation in v1) is in `docs/design/README.md` and is also not relitigated.

## Locked decisions referenced (not relitigated)

From the constitution, `docs/architecture/overview.md`, `docs/architecture/client.md`, and `docs/design/README.md`:

- Framework: Leptos, built with `cargo-leptos`. (Overview §Web client; Constitution III)
- Primary target is desktop browsers; mobile-browser is a fallback path, not a design target. (Overview §Web client)
- Rendering: wgpu via WebGPU primarily, with WebGL2 fallback through wgpu's downlevel backend. (Overview §Web client; Constitution VI)
- Threading: single-threaded WASM; **no** `SharedArrayBuffer`; no Cross-Origin-Opener-Policy / Cross-Origin-Embedder-Policy headers; per-WASM state lives in `thread_local!`. (Overview §Web client)
- Cache: OPFS for the live artifact cache. (Overview §Web client; Client §Fetch / cache / load pipeline)
- Embedded bundle on web: shipped as a static asset alongside the wasm on Cloudflare Workers Assets, fetched on first visit, HTTP-cached for return visits. (Client §Embedded downsampled artifact)
- Perf budget: 2 MB total compressed at first paint (wasm + static-asset embedded bundle + page shell), 3 MB total compressed at second paint (after the live CDN bundle resolves). A target, not a contract; exceeding it produces a CI warning, not a build failure. (Client §Web first-paint perf budget)
- CSS: hand-written CSS organized as Sass partials (Sass used only for splitting/bundling, not scripting); **Tailwind and utility-class libraries are explicitly ruled out**. (Project memory; design `README.md`)
- Visual identity: sharp, white-paper-with-red-ink, square corners (≤1px radius), 1px borders, no shadows, no gradients, only the zoom-to-country animation through v1. (`docs/design/README.md`)
- Hot-swap protocol: renderer subscribes to a `tokio::sync::watch::Receiver<Arc<Bundle>>` published by the loader. (Client §Bundle hot-swap)
- No live API through v2: every datum the user sees came from a versioned CDN artifact. (Constitution VI)
- CDN: Cloudflare R2 for artifacts, Cloudflare Workers Assets for the web app's static files. (Overview §Artifact distribution)

## Workspace placement

The web client is a single Cargo workspace member at `web/`. It depends on `core/` (which holds every consumer surface — manifest parsing, SQLite, FlatGeobuf, projection, hit-testing, wgpu pipelines, license matrix). The web crate itself owns only browser-platform glue and the Leptos UI tree.

```
eafora/
├── core/                       # the shared Rust core (consumer surface)
├── web/                        # this document's subject
│   ├── Cargo.toml              # depends on core, leptos, wasm-bindgen, web-sys, js-sys, gloo, ...; cargo-leptos config under [package.metadata.leptos]
│   ├── style/                  # Sass partials (organized per §CSS architecture)
│   ├── static/                 # static assets served verbatim
│   │   ├── embedded_artifacts/ # downsampled bundle copied here by the build script
│   │   │   ├── manifest.json
│   │   │   ├── geometry/
│   │   │   └── (statistic shards under whatever subdirectory the manifest names)
│   │   ├── favicon.ico
│   │   └── robots.txt
│   └── src/
│       ├── lib.rs              # crate root; declares `app` + the cfg-gated `client` module; both SSR and client-side builds compile this
│       ├── main.rs             # SSR-build entrypoint: calls static_routes.generate() and exits
│       ├── error.rs            # `cache: ...` and `fetch: ...` AppError prefixes documented here
│       ├── app.rs              # root App component + <Routes> tree; both the SSR build and the client-side build compile this
│       ├── about.rs            # SSG'd About page (per design.md §Naming and the About page)
│       ├── client/             # browser-only runtime glue; compiled only under the `hydrate` feature (gated once, in lib.rs)
│       │   ├── hydrate.rs      # exported `hydrate` fn (the wasm entry point) that mounts the App on `<div id="leptos">`
│       │   ├── cache.rs        # OpfsArtifactCache; implements core's cache contract
│       │   ├── fetch.rs        # browser fetch() adapter; fetch_manifest, fetch_artifact_file
│       │   └── canvas_surface.rs # <canvas> → wgpu::Surface bridge
│       ├── map/                # the primary surface (client-side map view)
│       │   ├── map.rs          # MapView component
│       │   ├── canvas.rs       # <canvas>-bearing component; bridges to canvas_surface
│       │   ├── legend.rs       # choropleth legend
│       │   └── controls.rs     # statistic picker, year scrubber, source panel
│       └── region/             # SSG'd region detail page (region = any level of the region hierarchy: country, subregion, supranational, etc.)
│           ├── region.rs       # RegionDetail component; rendered for /region/<region.code>
│           └── history.rs      # history chart (per design.md mobile frame 03)
```

Per-feature module layout: directory only when a feature has 2+ files (`map/`, `region/`); single-file feature modules sit flat under `src/`. `client/` is the exception to the by-feature rule: it groups the browser-only runtime modules (OPFS cache, fetch, the wgpu-canvas bridge, the wasm entry point) so their `#[cfg(feature = "hydrate")]` gate lives once on the parent (`mod client;` in `lib.rs`) instead of being repeated on every module. Each module-root file (`client.rs`, and any `map/`/`region/` root) holds only `pub mod ...; pub use ...::*;` declarations; primary content lives in named files.

The crate has no `<feature>_db.rs` (no Postgres in the client) and no `<feature>_api.rs` (the web client doesn't host HTTP routes; the `<feature>_client.rs` slot is reserved for v3+ if a live correction-submission API lands).

The web crate's relationship to the browser is direct: each feature module calls `web-sys` / `gloo` APIs from the place that needs them. `cache.rs` calls `web_sys::FileSystemDirectoryHandle` (no gloo wrapper for OPFS yet); `fetch.rs` uses `gloo_net::http::Request` (a thin ergonomic wrapper over `web-sys`'s raw fetch surface); `canvas_surface.rs` works with the `web_sys::HtmlCanvasElement` type. The only `wasm_bindgen` annotation in the whole crate is on the exported `hydrate` function in `client/hydrate.rs`, which is how the browser knows where to begin executing the WASM module (mount Leptos on `<div id="leptos">`). The web crate's relationship to `core/` is also direct: a normal Cargo dependency, called as Rust functions, no FFI involved (both compile into the same WASM module).

One crate, two compile modes via cargo-leptos's `bin-features = ["ssr"]` and `lib-features = ["hydrate"]` (see §Build toolchain). The SSR build is invoked once at build time to write static HTML; the client-side build produces the browser-side WASM that takes over on `/`.

## Build toolchain

### `cargo-leptos`

`cargo-leptos` is the workspace driver for the web crate. It composes:

- `cargo build --target wasm32-unknown-unknown --features hydrate` for the browser-side WASM bundle (Leptos's `hydrate` feature flag names the mode where the client-side code attaches to existing server-rendered DOM).
- `cargo build --features ssr` for the static-HTML generator binary (built at build time, runs once in CI, then exits — see §Routing and SSG).
- `wasm-bindgen` to generate the JS shim that loads the WASM.
- CSS bundling from `web/style/`.
- Watching, hot reload, and dev-server orchestration during local development.

Reference shape of `[package.metadata.leptos]` in `web/Cargo.toml`:

```toml
[package.metadata.leptos]
output-name      = "eafora"
site-root        = "target/site"
site-pkg-dir     = "pkg"
style-file       = "style/main.scss"
assets-dir       = "static"
site-addr        = "127.0.0.1:3000"
reload-port      = 3001
browserquery     = "defaults"
bin-features     = ["ssr"]
lib-features     = ["hydrate"]
bin-default-features = false
lib-default-features = false
lib-profile-release  = "wasm-release"
```

The `style-file` entry points at one entrypoint; that file `@use`s the partials under `style/` (see §CSS architecture). The `assets-dir` is verbatim-copied to `target/site/`, which is what gets uploaded to Cloudflare Workers Assets.

The SSR build is what writes the static HTML for `/region/<region.code>` and `/about` (see §Routing and SSG). The client-side build is the browser-side WASM that takes over on `/` (the map view) and on any region page if v2+ ever adds client-side interactivity to those pages.

### `wasm-opt`

Release builds run `wasm-opt -O4` on the produced `.wasm` to strip dead code and inline aggressively. cargo-leptos invokes `wasm-opt` as part of its release pipeline; size-specific profile knobs live in `[profile.wasm-release]`:

```toml
[profile.wasm-release]
inherits  = "release"
opt-level = "z"
lto       = true
codegen-units = 1
strip     = true
```

The `wasm-opt -O4` flag itself is passed through cargo-leptos's `wasm-opt-args` setting (resolve the exact key against the cargo-leptos version pinned in the workspace). Expected post-`-O4` size: approx. 600 KB brotli (per overview §Web client), against the perf-budget breakdown in §Perf-budget enforcement below.

### Compression

The build runs `brotli -q 11` over every compressible asset under `target/site/` and uploads both the original and the `.br` sibling. Workers Assets serves the `.br` file when the client's `Accept-Encoding` includes `br` and the original otherwise — same `.br`-sibling pattern as nginx's `brotli_static`. Quality 11 is the maximum; the runtime can't compress at that quality on the fly (too slow), so precompressing ourselves is the only way to ship q11 to clients.

A `scripts/build/precompress-site.sh` helper iterates `target/site/`, runs `brotli -q 11 --keep` on each eligible file (`.wasm`, `.js`, `.css`, `.html`, `.json`, `.fgb`, `.sqlite`), and exits non-zero on failure. Filtering is by extension; already-compressed types (`.png`, `.jpg`, `.woff2`) are skipped. Invoked as the final step of `cargo leptos build --release`, before the Workers Assets upload.

The compression gap between Workers Assets's automatic q4–q5 and our shipped q11 is real for the WASM bundle and the embedded artifacts (both compress well; both are perf-budget-relevant). Worth the extra build step.

### Perf-budget warning

Per `client.md` §Web first-paint perf budget, the targets are 2 MB total compressed at first paint and 3 MB at second paint. These are **soft targets**, not enforced caps — a CI step measures the deployed asset set and logs a warning when the target is exceeded, but never blocks the build. The cap is a target, not a contract; if a future change pushes the bundle past 2 MB intentionally, we want the warning to surface the regression for human review, not for CI to fail.

A `scripts/build/measure-site-budget.sh` helper:

1. Runs `cargo leptos build --release`.
2. For first paint, sums the brotli-compressed sizes (estimated by running `brotli -q 11` locally for the measurement step only — the deployed assets are compressed by Cloudflare's edge, not by us) of the WASM bundle, the JS shim, the bundled CSS, the page-shell HTML, and every file under `target/site/embedded_artifacts/` that the embedded-bundle loader fetches during initial render.
3. For second paint, additionally counts every artifact file the live-bundle fetch will need on first online connection (geometry shard + every base statistic shard).
4. Prints a breakdown in labeled, aligned key:value lines (narrow-terminal-safe; no markdown tables) and always exits zero — the warning is in the text output, not the exit code.

Example output:

```
First paint:  1.42 MB / 2.00 MB  (71%)
  wasm                  612 KB
  js shim                38 KB
  css                    12 KB
  html shell              8 KB
  embedded artifacts    752 KB

Second paint: 2.81 MB / 3.00 MB  (94%)  near cap
  + geometry            1.20 MB
  + statistic shards    190 KB
```

The script is invoked at the end of every `cargo leptos build --release` and as a CI step that posts the output as a PR comment for visibility.

### Build dependency direction

The static-asset embedded bundle is copied into `web/static/embedded_artifacts/` at the start of the build, **pulled** from the producer's downsampled output. The dependency is:

1. The producer (running on the Mac mini through v1; see overview §Ingestion) periodically runs `ingestion build` (no flag; a follow-up PR on the producer side, see `client.md` §Decisions still open), which emits the `downsampled/` subtree alongside `complete/` under `$EAFORA_ARTIFACTS_DIR/<version-label>/` and updates the `$EAFORA_ARTIFACTS_DIR/latest` pointer.
2. The web build's first step runs `scripts/build/sync-embedded-bundle.sh ./web/static/embedded_artifacts/`. The script copies `$EAFORA_ARTIFACTS_DIR/latest/downsampled/` into the destination with `cp -R` (invoking `ingestion build` first if no build exists). The producer's output location is configured via `EAFORA_ARTIFACTS_DIR`.
3. `cargo leptos build` then proceeds normally; `web/static/` is verbatim-copied into `target/site/`.

Plain copy (not symlink, not hard link, not `rsync`) on both web and iOS. The bundle is a few MB through v2; the duplication is irrelevant; the simplicity of "one shell-command shape, both platforms, no edge cases around symlink resolution or filesystem-boundary fallback" wins.

The script never modifies the producer's tree; it only reads. The result is that a fresh-clone build produces a fully-populated `web/static/embedded_artifacts/` without the developer needing to remember the regen step.

`web/static/embedded_artifacts/` itself is gitignored. The bundle is rebuilt on every CI build and on every local dev launch; staleness is not a correctness concern because the live CDN fetch upgrades it on first online interaction (per `client.md`).

## Routing and SSG

Three route categories:

- `/`
  - Mode: client-side only.
  - The map view is wgpu-on-canvas. There is nothing useful to render server-side; the canvas is empty until WebGPU initializes. SEO concerns are moot for a map.

- `/region/<region.code>`
  - Mode: SSG.
  - Region detail pages are content-shaped (region name, primary statistic, history chart, sources list). They benefit from search-engine indexing.
  - `region.code` is the existing slug from the `region` table (`usa`, `south_america`, etc.).

- `/about`
  - Mode: SSG.
  - Editorial content (per `docs/design/README.md` §Naming and the About page); no interactivity.

Note that the URL uses `region`, not `country` — the data model defines `region` as the unified hierarchy (supranational → region → subregion → intermediate_region → country → future subnational levels); the detail page handles any level. The country / subnational / aggregate distinction is a property of the underlying `region` row (its `level` column), not a routing distinction.

### Client-side map view

The map route mounts the Leptos `App` onto a dedicated `<div id="leptos">` container inside `<body>` (not onto `<body>` itself, which is owned by the browser and may carry extension-injected nodes or other ambient state we don't want to entangle with Leptos's reactive tree). The `App` renders the `MapView` component. `MapView` owns:

- A `<canvas>` element (`map::canvas::MapCanvas` component).
- The choropleth legend overlay (`map::legend::Legend`).
- The control surfaces (statistic picker, year scrubber, source panel — `map::controls::Controls`).

The canvas component bridges into `canvas_surface`, which constructs a `wgpu::Surface` from the canvas DOM node and hands it to `core::map::map_renderer`. The renderer holds the surface and a `tokio::sync::watch::Receiver<Arc<core::artifact::Bundle>>`.

Rendering is **event-driven**, not loop-driven. There is no `requestAnimationFrame` running at the display's refresh rate by default. A redraw is scheduled only in response to something that needs one:

- User input that changes the scene (`pointerdown` selecting a region, `pointermove` panning, `wheel` zooming, statistic-picker selection, year-scrubber drag).
- A new bundle landing on the watch channel (a background task awaits `Receiver::changed()` and posts a redraw on each wake).
- An in-progress animation, which is a self-perpetuating chain of `requestAnimationFrame(redraw)` callbacks that stops scheduling itself when the animation completes. Through v1 the one animation is the zoom-to-country camera move (per `docs/design/README.md` §Animation): re-selecting the already-selected country starts the loop, each tick samples the `ViewportTransition` for a fresh interpolated viewport and redraws, and the loop stops when the transition lands. Any manual gesture, press, or resize cancels it.

The scheduling primitive is a dirty flag plus `requestAnimationFrame`: input handlers set the flag and call `request_redraw()`, which calls `requestAnimationFrame(draw)` if a frame isn't already pending. Multiple `request_redraw()` calls between vsyncs coalesce into one draw at the next vsync, so smooth pan-drag still renders every frame the display can show, but stationary idle states cost zero GPU work. The matching iOS pattern is `MTKView.isPaused = true` + `setNeedsDisplay()` (see `client-ios.md` §Rendering); same shape, platform-specific mechanism.

When `draw` runs, it reads the current `Arc<Bundle>` from the watch receiver (`Receiver::borrow()`, a synchronous atomic load) and issues wgpu draw calls against that bundle. The bundle is initially the static-asset embedded bundle (loaded at startup before the first paint); the live CDN fetch replaces it via the watch channel when complete, and the next scheduled redraw picks it up.

Per `docs/design/README.md`, the only v1 animation is the zoom-to-country camera move. Every other state change is instant: the map re-renders the new selection on the next frame; controls update synchronously. A first selection does not move the map; re-selecting the already-selected country (a second tap on it, or a double-click) eases the viewport to frame it (a short cubic-eased camera move). The hover effect on regions is a discrete state change to a 1px red outline — see overview §Hover scaling for the visual-vs-hit-test transform separation already locked at the core layer.

### SSG for region detail and About pages

Leptos's `static_routes` API (stable as of Leptos 0.8 / cargo-leptos 0.3) drives the static-HTML generation natively. Each SSG route declares `SsrMode::Static(StaticRoute::new()...)` in the routes tree; the SSR-build binary's `main` calls `static_routes.generate(&leptos_options).await` once at build time, which writes the rendered HTML into `target/site/region/<region.code>/index.html` and `target/site/about/index.html`. The directory is then uploaded to Cloudflare Workers Assets.

Sketch in `web/src/app.rs`:

```rust
<Routes fallback=|| view! { "Not found." }>
    <Route path=path!("/") view=MapView />
    <Route
        path=path!("/region/:code")
        view=RegionDetail
        ssr=SsrMode::Static(
            StaticRoute::new()
                .prerender_params(|| async {
                    core::canonical::all_region_codes()
                        .into_iter()
                        .map(|code| ("code".into(), code.into()))
                        .collect()
                })
        )
    />
    <Route
        path=path!("/about")
        view=AboutView
        ssr=SsrMode::Static(StaticRoute::new())
    />
</Routes>
```

The `prerender_params` closure returns a `StaticParamsMap` of every `(code → region.code)` pair to render. The list comes from `core::canonical::all_region_codes()`, the same authoritative source the iOS deep-link router and the producer's seed migrations use. Adding a new region is one row in the seed migration; the SSG build picks it up automatically on the next CI run.

`/about` has no params, so the empty `StaticRoute::new()` renders a single HTML file.

Mechanically, CI runs:

```sh
cargo leptos build --release       # builds both the SSR binary and the client-side WASM
./target/server/release/web        # invokes static_routes.generate(); writes to target/site/
# then upload target/site/ to Cloudflare Workers Assets via wrangler deploy
```

The SSR binary's `main` only calls `static_routes.generate()` and exits; we intentionally skip the `axum::serve(...)` step from Leptos's standard SSR pattern (we have no live origin to serve; the deploy target is purely static). This is a slightly unusual usage of `static_routes` — Leptos's documented deploy story assumes the SSR binary keeps running for regeneration and dynamic routes — but the API supports it cleanly. If a future Leptos release deprecates this pattern, the fallback is the original custom-prerender approach (a small Rust binary that calls the view functions directly); low-cost migration.

## Rendering: wgpu surface acquisition

The browser-platform glue for wgpu lives in `web/src/canvas_surface.rs`. The function signature:

```rust
pub fn create_surface_from_canvas(
    canvas: web_sys::HtmlCanvasElement,
) -> Result<core::map::WgpuSurface, AppError> {
    // ...
}
```

Internally:

1. Construct a `wgpu::Instance` with backends `BROWSER_WEBGPU | GL` (the GL backend is the WebGL2 fallback path provided by wgpu's downlevel infrastructure).
2. Construct the surface via `wgpu::Instance::create_surface_unsafe` with `SurfaceTargetUnsafe::Canvas(canvas)` (resolve the exact wgpu API against the version pinned in `core/`'s Cargo.toml; the canvas-from-DOM-element shape has been stable in wgpu but the function name has shifted across releases).
3. Request an adapter; if `wgpu::Instance::request_adapter` fails on WebGPU, the GL backend takes over automatically.
4. Wrap the result in a platform-agnostic `core::map::WgpuSurface` and return.

The surface is then handed to `core::map::map_renderer::Renderer::new(surface, ...)` and held for the session.

### WebGPU vs WebGL2 fallback policy

The wgpu API exposes the adapter's capabilities through `wgpu::Adapter::features()` and `wgpu::Adapter::limits()`. Eafora's renderer (`core::map::map_renderer`) is designed to work under the WebGL2-equivalent feature set: simple vertex + fragment shaders, no compute, no indirect draws, no storage textures. This is by construction — the per-frame work is small (200 country polygons through v1) and doesn't need WebGPU-only features.

**Therefore there is no UI fallback to manage**: the same renderer code path runs on both backends with the same visual output. The user sees no banner, no quality reduction, no "your browser is unsupported" message. wgpu's downlevel backend handles the translation transparently.

If `wgpu::Instance::request_adapter` returns `None` (no WebGPU and no WebGL2 — practically never on shipping browsers in 2026, but theoretically possible on some embedded contexts), the web client surfaces the failure two ways:

- A console error log via `log::error!` carrying the failure detail (which backends were attempted, what the adapter request returned). For developers and devtools-aware users.
- A visible in-page message replacing the map area: short prose stating that the browser doesn't support the rendering backends Eafora requires, with a link to the About page. Plain HTML rendered by Leptos, styled per `docs/design/README.md`'s panel pattern. No fallback, no degraded mode, no retry button — the condition is permanent for this browser.

Both surfaces fire on the same condition; neither replaces the other. The console error is the diagnostic record; the visible message is what the user actually sees.

The WebGPU-vs-WebGL2 selection is invisible to the user. **For developers**, a query-string flag `?renderer=webgl2` forces the GL backend for testing parity (consumed by `web/src/canvas_surface.rs` before the instance is constructed). Not a user-facing toggle.

## Threading model

WASM is single-threaded. Per overview §Web client, `SharedArrayBuffer` is intentionally avoided so Eafora can be embedded by third parties without cross-origin isolation. There is no main-thread/Worker split through v1 — the entire WASM module runs on the main thread.

State is held in `thread_local!` cells (per overview §Async model). The specific shape per piece of state:

- The `core::map::map_renderer::Renderer` instance. Wrapped in `RefCell` — the renderer has `&mut self` methods that update frame state (current viewport, hover target, selected statistic, dirty flag).
- The `tokio::sync::watch::Receiver<Arc<Bundle>>`. Wrapped in `RefCell` — a small background task awaits `Receiver::changed().await` to wake on each new bundle and post a redraw request, and `changed()` takes `&mut self`. The synchronous-read path (`Receiver::borrow()` during a draw) takes `&self` and would not by itself need a cell, but the cell is already there for the awaiting task and the two share one Receiver.
- The `tokio::sync::watch::Sender<Arc<Bundle>>`. **Not** wrapped in `RefCell` — `Sender::send` takes `&self` (the Sender's internal state is itself interior-mutable via atomics). Stored as a bare `Sender<...>` in the thread-local.

The `OpfsArtifactCache` deliberately does **not** appear in this list. It is a zero-sized type holding no state: each `get` / `put` / `list_versions` / `delete_version` call re-resolves `navigator.storage.getDirectory()` and walks to `artifacts/` on its own. The saved work from caching the root `FileSystemDirectoryHandle` (two async microtask hops per operation) is negligible against the actual file I/O, and statelessness gives us automatic recovery if the browser evicts site data mid-session — a cached handle would go stale; a fresh resolve simply returns the new directory. The cache is constructed once at app start and passed to the loader as a trait object; no `thread_local!`, no `RefCell`.

Three reasons `thread_local!` rather than `static LazyLock<RwLock<_>>`, `OnceLock<_>`, or another globals shape:

- Wasm-bindgen and web-sys types are `!Send` and `!Sync` (`JsValue` and everything that holds one — DOM-element handles, the wgpu surface bound to a canvas, fetch responses). The Rust type system rejects them in any `static` that demands `Sync`. `thread_local!` has no `Send`/`Sync` bound; it stores per-thread data and trusts the runtime to keep it on its own thread. In single-threaded WASM, "the current thread" is "the only thread," and the constraint is satisfied trivially.
- WASM is single-threaded; there is no contention to mediate. A `Mutex` or `RwLock` would acquire and release on every access for no reason, and worse, holding the lock across an `.await` (the cache and fetch adapters are async) risks deadlocking the runtime since there's no other task to release it. `thread_local!` with a `RefCell` provides interior mutability without synchronization primitives — the right cost model when concurrency does not exist.
- `OnceLock` is for write-once read-many state (configuration loaded at startup). Our state is mutable over the page's lifetime (the bundle gets hot-swapped, the cache mutates as artifacts persist). A `OnceLock<RefCell<_>>` would still need `Sync` on the inner cell; combined with the `!Send` types above, it doesn't compile.

The conventional Rust idiom for "single-threaded WASM globals" is `thread_local! { static FOO: RefCell<...> = ... ; }`. We follow it.

There is no Rust async runtime in the WASM bundle through v1. Async functions exposed by `core/` (e.g. `Bundle::open`) compile fine under WASM and are driven by the browser's `Promise` event loop via wasm-bindgen's `JsFuture` glue. `tokio::sync::watch` works in single-threaded WASM because its synchronization primitives don't require a runtime.

### Worker migration path (deferred)

Per `client.md` §SQLite in the client, the migration trigger is "any per-statistic shard grows past approx. 30 MB" — at which point the SQLite engine moves to a dedicated Worker with the database file backed by an OPFS `FileSystemSyncAccessHandle`. The main thread sends query requests via `postMessage` and receives results the same way. The Worker is a separate WASM bundle containing only the SQLite engine and a thin RPC layer; the main thread keeps Leptos, wgpu, and everything else.

Through v1 this path is not built. The single-threaded shape is intentional. Tracked in `docs/backlog.md` §Client as "Move web SQLite engine into a dedicated Worker, backed by OPFS `FileSystemSyncAccessHandle`."

## OPFS cache adapter

Per `client.md` §Cache eviction, the cross-platform contract is:

```rust
trait ArtifactCache {
    async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppError>;
    async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError>;
    async fn list_versions(&self) -> Result<Vec<String>, AppError>;
    async fn delete_version(&self, version_label: &str) -> Result<(), AppError>;
}
```

(The trait shape is illustrative; `core::artifact` defines the canonical version.)

`OpfsArtifactCache` in `web/src/cache.rs` implements this trait against the browser's Origin Private File System API.

### Directory layout in OPFS

```
<opfs-root>/
└── artifacts/
    ├── <version_label>/
    │   ├── manifest.json
    │   ├── geometry/
    │   │   └── world-50m-<sha256>.fgb
    │   └── (statistic shard subdirectory per the manifest's relative_path entries)
    │       └── ...
    └── <other_version_label>/
        └── ...
```

`<version_label>` is the producer's `YYYY-MM-DD+<surname>` slug. The directory shape mirrors the CDN's per-version layout one-to-one: the cache stores files at exactly the `relative_path` the manifest carries, so `cache.get(version, "<relative_path>")` resolves to `<opfs-root>/artifacts/<version>/<relative_path>` regardless of how the producer organizes subdirectories. Storing both the latest and the most-recent prior version (per `client.md` §Cache eviction) means the cache holds at most two version subtrees at any time.

The `artifacts/` directory exists so the same OPFS root can carry other Eafora data later (user preferences, draft contributions in v3+, transient scratch state) without colliding with the artifact cache. No `eafora/` parent because OPFS is already origin-scoped — only `eafora.org`'s code can see this root, so namespacing under our own name would be redundant.

### API usage

`navigator.storage.getDirectory()` returns the OPFS root. The cache constructor walks down to `artifacts/`, creating the directory on first launch. Each `put` resolves to a `FileSystemFileHandle.createWritable()` on the main thread — async writes are supported there in every browser shipping OPFS. Each `get` resolves to a `FileSystemFileHandle.getFile()` followed by `Blob.arrayBuffer()`.

The `FileSystemSyncAccessHandle` API is **not** used by the main-thread cache adapter — it is Worker-only on every shipping browser today. It is reserved for the deferred Worker-based SQLite engine (§Threading model) where SQLite needs synchronous page reads against an OPFS-backed file.

### Quota and persistence

Per the saved memory `reference_browser_storage_quotas`, the web client interacts with browser quota policies as follows (mechanical implementation of the strategy that memory describes):

1. **On first launch**, the cache constructor calls `navigator.storage.persist()` to request persistent storage. Some browsers prompt the user; others auto-grant based on heuristics. The result (`true` = persistent; `false` = best-effort) is logged but doesn't block the cache from initializing.
2. **Before each `put`**, the cache calls `navigator.storage.estimate()` and verifies that `quota - usage >= bytes.len() + safety_margin` (safety margin: 4 MB, covering typical FS overhead). On failure, the `put` returns an `AppError` whose message starts with the literal `opfs: quota exceeded` so the caller can match on the prefix and degrade gracefully.
3. **On `QuotaExceededError` from `createWritable()` / `write()`**, the cache returns the same `opfs: quota exceeded` -prefixed `AppError`. The caller (the loader in `core::artifact`) catches it and falls back to in-memory-only mode for the session — the live bundle is held in `Arc<Bundle>` but not persisted; the cache stays at whatever state it was in. A subtle UI hint surfaces in the page shell ("Cached data unavailable; using in-memory fallback").
4. **On startup, if the cached artifact is missing** (eviction happened, or the user cleared site data), the cache `get` returns `None`, the loader treats this as a cache miss, and the fetch path runs as if first launch.
5. **On older browsers without OPFS support** (notably iOS 16 and earlier; Safari shipped OPFS in 16.4, with `createWritable` arriving later still; verify against current `caniuse.com` data before relying on the cutoff), the cache constructor returns an `AppError` whose message starts with `cache: opfs unsupported`. The loader treats this as a **hard failure**: it renders the plain-HTML unsupported panel (the same panel pattern as the no-adapter renderer case; see §WebGPU vs WebGL2 fallback policy), stating that the browser lacks the storage support Eafora requires, with no fallback, no retry, and no degraded mode. There is deliberately no in-memory fallback: web has no on-device data (the embedded bundle is itself an HTTP fetch), so running without OPFS would mean re-fetching every artifact on every visit and holding it in memory for the session only. That path is permanently untested (headless Chrome always exposes OPFS) and serves a browser population that is essentially empty by 2026, so the clean line is to require OPFS. Reload after upgrading the browser restores the working path.

The eviction policy specified by `client.md` §Cache eviction (keep current version + most recent prior version) is implemented in `OpfsArtifactCache::evict_old_versions`, called at startup after the cache opens. It enumerates the version subtrees, picks the two most recent by `<version_label>` lexicographic order (which sorts correctly because of the `YYYY-MM-DD+<surname>` shape), and deletes the rest.

## CSS architecture

Hand-written CSS organized as Sass partials (no utility-class framework; Sass is used only to split and bundle the files, not for its scripting features). The file layout under `web/style/`:

```
web/style/
├── main.scss                   # entrypoint; @use's the partials in dependency order
├── _tokens.scss                # design tokens as CSS custom properties (colors, spacing, type scale)
├── _reset.scss                 # minimal CSS reset (box-sizing, margin/padding zero, etc.)
├── _typography.scss            # font-family, font-variant-numeric (tabular-nums), line-heights
├── _layout.scss                # global layout primitives (page shell, panel container)
├── _map.scss                   # MapView + canvas + legend + controls
├── _region.scss                # RegionDetail + history chart
├── _about.scss                 # About page
└── components/
    ├── _panel.scss             # the canonical "sheet" panel pattern (per design.md)
    ├── _button.scss            # minimal text-and-border button styles
    └── ...
```

The `main.scss` entrypoint `@use`s every partial in dependency order (tokens → reset → typography → layout → page-specific styles); each partial is otherwise plain CSS. cargo-leptos compiles `main.scss` with its built-in Sass step (dart-sass, auto-downloaded the same way it fetches wasm-bindgen) into a single bundled, minified `target/site/pkg/eafora.css`.

Sass rather than plain CSS with `@import`, because cargo-leptos does NOT bundle plain-CSS `@import`: its CSS step only parses and minifies the single `style-file` via Lightning CSS (no bundler), so plain `@import`s survive into the output and the browser then tries to fetch each imported file at runtime (they 404). Its Sass step, by contrast, resolves `@use`/`@import` into one stylesheet before minifying. Do not switch back to plain `.css` partials with `@import` expecting them to bundle.

### Design tokens

`web/style/_tokens.scss` is the single source of truth for color, spacing, type scale, and border weight. Every other partial references the tokens via `var(--token-name)`; raw color values (`#ff0000`, etc.) appear nowhere else. The tokens themselves derive directly from `docs/design/README.md`:

```scss
:root {
    --color-paper:           #ffffff;
    --color-ink:             #000000;
    --color-accent-active:   #d50000;  /* red */
    --color-accent-link:     #0050ff;  /* blue */
    --color-rule:            #d4d4d4;  /* 1px-rule grey */

    --space-xs:              0.25rem;  /* approx. 4px at default root font size */
    --space-sm:              0.5rem;   /* approx. 8px */
    --space-md:              1rem;     /* approx. 16px, the default body unit */
    --space-lg:              2rem;     /* approx. 32px */

    --font-sans:             "Inter", system-ui, -apple-system, sans-serif;
    --font-mono:             "IBM Plex Mono", ui-monospace, monospace;

    --type-size-body:        0.875rem; /* approx. 14px */
    --type-size-data:        0.8125rem;/* approx. 13px */
    --type-line-height-body: 1.5;      /* unitless: multiplier of inheritor's own font-size */
    --type-line-height-data: 1.15;     /* same */

    --border-ink:            1px solid var(--color-ink);
    --border-active:         1px solid var(--color-accent-active);
    --border-rule:           1px solid var(--color-rule);

    --radius-default:        0;
    --radius-input:          1px;
}
```

Token names match the design doc's vocabulary (`paper`, `ink`, `rule`, `sheet`); they are never aliased to consumer-app vocabulary (`background`, `primary`, `text-default`). Spacing uses a four-step T-shirt scale (`xs`, `sm`, `md`, `lg`); the visual identity doesn't have enough density layers to earn a fifth.

Units split deliberately between `rem` and `px`:

- Spacing and font sizes are `rem`. They scale with the user's browser-font-size preference (an accessibility surface for users who set a non-default root size). `rem` is a multiple of that root size; `px` ignores it. Browser zoom (`Cmd-+`) scales both proportionally, so zoom isn't the differentiator — the user-font-size preference is.
- Borders and radii are `px`. These are device-pixel-precision visual constants from `docs/design/README.md` ("1px borders, black or near-black"; "0.5px via `transform: scale` on retina"). A 1px line that grows with the user's font size stops reading as a thin rule; the px-anchored value is what the design wants.

The reference HTML stubs at `docs/design/stub-desktop.html` and `docs/design/stub-mobile.html` are useful reference for developers, but they should be considered archived and not used to inspire actual programming.

### Tabular figures

Every numeric display in the UI sets `font-variant-numeric: tabular-nums` so columns align (per design.md §Typography). This is enforced by a CSS class `.numeric` applied to every element rendering a number (`<span class="numeric">{value}</span>`); the class is defined in `web/style/_typography.scss`.

### Responsive design

Per overview §Web client, mobile-browser is a fallback path, not a design target. The web client targets desktop browsers primarily. CSS media queries provide a functional mobile layout (single-column, full-width map, bottom-sheet for region detail per `docs/design/stub-mobile.html`) but bespoke mobile UX work is out of scope for v1. Mobile-specific gestures, touch-tuned targets, and mobile-perf optimization are deferred — that effort goes to the native iOS and Android shells.

The break point is `max-width: 768px`. Below that, the layout switches to the single-column mobile pattern. Above, the desktop pattern applies.

## Localization scaffolding

v1 ships English-only, but the localization machinery is in place from day one so future locales are mechanical (add a translation file) instead of a refactor (find every bare string literal). Same discipline as the iOS doc's §Localization scaffolding; same overview §FFI split:

- UI-chrome strings (controls, errors, About-page prose, accessibility labels) live in the web app's translation files.
- Domain-content strings (region names, statistic names, source attributions) live in the SQLite shard built by ingestion. Out of scope for the web shell; the client reads them via `core::*` queries.

For Leptos, **`leptos_i18n`** (verified at v0.6.2 in April 2026, compatible with Leptos 0.8.x; [crate](https://crates.io/crates/leptos_i18n), [book](https://baptistemontan.github.io/leptos_i18n)) is the established choice. It loads translation files at compile time, generates typed accessor macros per key, and reactively re-renders subscribed views when the active locale changes. JSON is the default format (JSON5, YAML, TOML also supported); we use JSON. Translations live as JSON files in `web/locales/<lang>.json`; an `<I18nContextProvider>` wraps the app root; components access translations via `use_i18n()` and the `t!()` macro. JSON files support arbitrary nesting; the macro accesses nested keys with dot notation:

```rust
// web/locales/en.json
{
    "about": {
        "title": "About Eafora",
        "etymology": "Old English, masc.: son, descendant, heir."
    },
    "controls": {
        "loading": "Loading data..."
    }
}

// in a view
let i18n = use_i18n();
view! {
    <h1>{t!(i18n, about.title)}</h1>
    <p>{t!(i18n, about.etymology)}</p>
    <p>{t!(i18n, controls.loading)}</p>
}
```

Adding a second locale is a new `web/locales/<lang>.json` file with the same key structure and translated values. Compile-time checks verify every key exists across every locale; a missing translation is a build error, not a runtime miss.

Interpolation uses named arguments. JSON placeholders are `{{ name }}`; the macro takes matching named arguments at the call site:

```json
{ "click_count": "You clicked {{ count }} times" }
```

```rust
t!(i18n, click_count, count = move || counter.get())
```

The crate also bundles ICU-backed locale-aware formatting for numbers, dates, and plurals via `t_format!` / `t_plural!` macros (gated behind optional features `icu_decimal`, `icu_datetime`, `icu_plurals`). One stack for translations + locale-aware formatting; no separate `Intl.*` calls needed.

#### Integration

The crate uses a build-script-driven setup, not a `[package.metadata.*]` block in `Cargo.toml`. Three pieces:

`web/Cargo.toml` lists `leptos_i18n` as a runtime dependency and `leptos_i18n_build` as a build dependency.

`web/build.rs` runs the codegen against the configured locale list and writes the generated module under `OUT_DIR`:

```rust
// web/build.rs
use leptos_i18n_build::{Config, TranslationsInfos};
use std::path::PathBuf;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=Cargo.toml");

    let i18n_mod_directory = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("i18n");

    let config = Config::new("en")?;
    // .add_locale("fr")? when v2 adds locales

    let translations_infos = TranslationsInfos::parse(config)?;
    translations_infos.emit_diagnostics();
    translations_infos.rerun_if_locales_changed();
    translations_infos.generate_i18n_module(i18n_mod_directory)?;

    Ok(())
}
```

`web/src/lib.rs` brings the generated module into scope at the crate root:

```rust
include!(concat!(env!("OUT_DIR"), "/i18n/mod.rs"));
use i18n::*;
```

`web/src/app.rs` wraps the app root in `<I18nContextProvider>`. The component is auto-generated in the `i18n` module from the locale config:

```rust
use crate::i18n::*;
use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <I18nContextProvider>
            // routes + map view + region/about pages
        </I18nContextProvider>
    }
}
```

Default locales path is `./locales`; we put the files at `web/locales/`. To override, pass `.locales_path("...")` on the `Config` builder.

The discipline is "no bare string literal in a user-facing position." `view! { <p>{t!(i18n, key)}</p> }` everywhere; never `view! { <p>"raw text"</p> }` for user-visible content. Bare strings stay fine for log messages, debug output, internal identifiers, anything not user-visible.

Domain-content i18n is deferred per overview §FFI. The producer side adds translation columns to the seed-data tables when a second locale becomes a real deliverable; the web client reads them via `core::canonical::region_name(code, locale)`-shaped queries — same mechanism, different layer of the stack.

## Browser fetch adapter

The `web/src/fetch.rs` module owns the browser-platform side of the load pipeline. Its public surface mirrors the platform-agnostic loader contract from `core::artifact`:

```rust
pub async fn fetch_manifest(repository_base_url: &str) -> Result<Vec<u8>, AppError>;
pub async fn fetch_artifact_file(repository_base_url: &str, version_label: &str, relative_path: &str) -> Result<Vec<u8>, AppError>;
```

Both functions delegate to the browser's `fetch()` API via `web_sys::window().unwrap().fetch_with_str(url)` and then await the response body via `js_sys::Promise` → `wasm_bindgen_futures::JsFuture`.

Concurrency cap: the loader in `core::artifact` holds a `tokio::sync::Semaphore` whose size is passed in by the platform. Web passes 6, matching `client.md` §Stage 3's per-platform cap. Each `fetch_artifact_file` call acquires a permit before issuing the request and releases on completion. We do not rely on the browser's own per-origin cap (which was 6 in HTTP/1.1 days but is effectively unbounded over HTTP/2 multiplexing, which Cloudflare's CDN uses); imposing our own limit keeps the load pattern under our control, the progress accounting stable, and the behavior testable.

Retry: per `client.md` §Stage 3, on any HTTP error or hash mismatch, retry once after approx. 100 ms, doubling to approx. 400 ms on a second attempt. Implemented in the loader (in `core::artifact`); the fetch adapter just propagates errors.

The `repository_base_url` is **not** a hand-typed constant. It's resolved at runtime via the discovery URL flow defined in `client.md` §Discovery and live bundle resolution: the client fetches same-origin `/discovery`, reads `repository_base_url` from the response, and uses that for every shard fetch. A static fallback (the committed `web/static/discovery` file, included at compile time) handles the case where discovery itself fails. Web doesn't strictly need the runtime indirection — every commit redeploys — but the contract is uniform across platforms; iOS and Android need it (binaries live on devices for months), and there's no cost to web following the same shape.

## Deploy target: Cloudflare Workers Assets

The deploy target is Cloudflare Workers Assets, served from `eafora.org` (the apex domain). The build output (`target/site/`) is uploaded as a static-asset bundle attached to a Worker; the Worker is a single pass-through that serves whatever asset matches the incoming request.

### Why Workers Assets instead of Cloudflare Pages

Workers Assets is Cloudflare's successor to Pages for static-site deploys. The deciding feature for us is **user-uploaded precompressed-asset support** (added late 2025; see [workers-sdk #11089](https://github.com/cloudflare/workers-sdk/issues/11089)): we ship `.wasm.br`, `.js.br`, etc. alongside the originals, and the runtime serves the `.br` to clients whose `Accept-Encoding` includes `br`. Pages doesn't honor user-uploaded `.br` files; it does its own on-the-fly compression at lower quality, which we can't override. Quality 11 vs. quality 4–5 is a meaningful saving on a WASM bundle.

Other differences are minor: same edge network, same TLS, same domain wiring, same custom-domain support, same configuration shape for cache headers (via `_headers`). For our use case the migration is mechanical.

### `wrangler.toml`

Configures the deploy. Reference shape:

```toml
name = "eafora-web"
compatibility_date = "2026-06-01"

[assets]
directory = "../target/site"
```

That's the whole config. **No `main` field, no Worker script.** Cloudflare's standard mode for "ship a directory of static assets and let the edge serve them" is to declare only the `[assets]` block; the edge handles asset routing directly without invoking any Worker code. We don't need a passthrough handler (`export default { fetch: (req, env) => env.ASSETS.fetch(req) }`) — that pattern exists for the case where you have request-time logic to combine with assets. We don't.

If we ever need request-time logic (response modification, A/B routing, edge-rendered pages), we add a `main = "src/index.ts"` file then. Through v1+ the static-only shape is correct.

`wrangler deploy` uploads `target/site/` (the cargo-leptos output) as the asset set. Custom-domain routing to `eafora.org` is configured in the Cloudflare dashboard or via a `routes` block in `wrangler.toml` (verify the exact syntax against current wrangler docs when we deploy).

### Headers

`web/static/_headers` is a Cloudflare-Workers-Assets header configuration file specifying response headers per path pattern. Workers Assets reads it from the deployed asset set at request time. cargo-leptos's verbatim asset copy lands it at `target/site/_headers` during the build.

The rules and their rationale live in a comment block at the top of `web/static/_headers` itself, not in this doc — easier to keep in sync when the headers change, and the comments are visible to anyone reading the file.

### Domain and DNS

Per overview §Domain and email and project memory `eafora_domain`, the production domain is `eafora.org`, registered through Cloudflare. The web app serves from the apex; the artifact CDN lives at `repository.eafora.org` (separate Cloudflare Worker + R2 bucket, owned by the ingestion side).

### Discovery endpoint

The discovery document defined in `client.md` §Discovery and live bundle resolution is served from the same Workers Assets deploy described above: `web/static/discovery` is a plain JSON file (with `Content-Type: application/json` set via `_headers`) and ends up at `https://eafora.org/discovery`. The endpoint is **not** a web-client feature — every platform (web, iOS, Android) fetches it at startup — but the asset is physically hosted alongside the web app because `eafora.org` is the obvious place for it and we already have a Workers Assets deploy serving that origin. Updating the discovery document (e.g. to point at a new repository base URL after an R2 re-platform) is a normal commit to `web/static/discovery` and a redeploy. See `client.md` §Discovery and live bundle resolution for the document schema and the client-side fetch flow.

## Testing strategy

Per Constitution Principle VII, the web-only TDD-required surfaces are:

- OPFS cache adapter contract: a `cache.put(...)` → `cache.get(...)` round-trip; assert byte-equal returns; assert a missing key returns `None`; assert eviction removes the right versions; assert quota-exceeded surfaces as an `AppError` whose message starts with `opfs: quota exceeded`. Runs against a real OPFS in headless Chrome via `wasm-bindgen-test` configured for browser execution.
- Browser fetch adapter error mapping: simulated 4xx / 5xx responses (via a mock server or `web_sys` interception layer; verify the most ergonomic option in headless Chrome) map to `AppError`s carrying the source URL and HTTP status in the message body.
- Canvas-to-wgpu-surface bridge: assert the surface's reported size matches the canvas's `clientWidth`/`clientHeight`; assert resize events propagate. Headless Chrome.
- WebGPU vs WebGL2 backend selection: the `?renderer=webgl2` query-string flag forces the GL backend; assert the resulting `wgpu::Adapter::backend()` is `Backend::Gl`; the unflagged path picks `Backend::WebGpu` on browsers that support it.
- Perf-budget reporting: `scripts/build/measure-site-budget.sh` is the test surface; CI invokes it on every PR and posts the output as a comment. The script does not fail the build; the warning is in the text output for human review.

Cross-platform surfaces (manifest parsing, SHA-256 verification, license-class authorization, FlatGeobuf hit testing) are tested in `core/` once and not re-tested per platform. See `client.md` §Testing strategy.

End-to-end browser tests are **not** in scope for the foreseeable future (through v3+). The visual ground truth lives in `docs/design/stub-desktop.html` and `docs/design/stub-mobile.html`; parity is checked manually against the stubs before every deploy. Playwright (or equivalent) would be overkill for Eafora's surface area for a long time — the UI is one map plus a small number of static-content pages, the interaction surface is narrow, and the cost of a full browser-automation suite (CI machinery, flake budget, maintenance) exceeds the value of automating tests that a manual check against the design stubs already catches.

## Decisions still open

- Page shell HTML structure. The `index.html` template that cargo-leptos uses as the SSR shell is checked in; its exact `<head>` contents (font preloads, viewport meta, OG tags for social sharing, `<link rel="canonical">`) need a small pass before the first deploy. Trigger: the first real deployment to the production domain.

## Things to verify

1. **Pinned Leptos version's exact `[package.metadata.leptos]` keys**: `wasm-opt-args`, `bin-profile-release`, `lib-profile-release`. Confirm key spellings against the version pinned in `web/Cargo.toml` before relying on the snippet in §`cargo-leptos`.
2. **wgpu surface-from-canvas function name**: `Instance::create_surface_unsafe` with `SurfaceTargetUnsafe::Canvas` is the current shape; verify against the version pinned in `core/`.
3. **Does `cargo leptos build --release` invoke the SSR binary's `main`?** The official `examples/static_routing/` documents `cargo leptos watch` only. If `--release` builds the binary but does not run it, CI needs an explicit `./target/server/release/web` step after the build to trigger `static_routes.generate()`. Confirm by running once and inspecting `target/site/region/`.
4. **Safari OPFS support cutoff**: Safari shipped OPFS in 16.4 but `createWritable` arrived later still. Verify the exact iOS/macOS Safari version cutoff against `caniuse.com` before relying on the §Quota and persistence step 5 hard-fail path.
5. **`core::canonical::all_region_codes()` shape**: the function the `prerender_params` closure calls. Confirm the exact signature when `core/` lands; the SSG step depends on this list being authoritative and complete.

## Follow-up work

- First `/speckit.specify` feature spec for the web client: see `docs/task-order.md` §Sequence step 3. The implementation feature lands the WASM bundle, the page shell, the client-side map view rendering against the static-asset embedded bundle, and the OPFS cache adapter — enough to surface the perf-budget report and render the static stub-equivalent of `docs/design/stub-desktop.html` against real data.
- The first deployment to `eafora.org` is gated on the producer side standing up `repository.eafora.org` (the artifact CDN endpoint) and on the producer-side `latest/manifest.json` upload step (a `client.md` follow-up); both can land in any order before the first web deploy.

Deferred-but-not-blocking web work lives in `docs/backlog.md` §Client (currently empty) once items earn deferral as concrete work.
