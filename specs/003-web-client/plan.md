# Implementation Plan: web client (WASM + Leptos + wgpu shell + OPFS cache)

**Branch**: `003-web-client` | **Date**: 2026-07-13 | **Spec**: [`spec.md`](./spec.md)

**Input**: Feature specification from `specs/003-web-client/spec.md`

## Summary

Deliver the browser surface of Eafora: a Leptos/WASM single-page app that renders the world
choropleth map (via the merged `shared` renderer) against a static-asset embedded bundle at first
paint, then hot-swaps to the live CDN bundle in the background. The web crate is thin platform glue —
an `OpfsArtifactCache` (the browser implementation of `shared::artifact::ArtifactCache`), a browser
fetch adapter, a canvas→wgpu bridge, and the Leptos component tree matching `docs/design/stub-desktop.html` —
sitting on top of the Rust core that already handles manifest parsing, SQLite queries, geometry
reading, projection, hit-testing, and the wgpu pipeline.

The spec was drafted (2026-06-22) against a planned `core` crate; the core shipped as `shared`. This
plan authors against the real `shared::*` surface and records every correction the reality of the built
crate forces. Two of those corrections require small `shared`-side and `ingestion`-side changes that
cannot live under `web/`; they are split into independent prerequisite PRs (Phase 0a, Phase 0b).

## Technical Context

**Language/Version**: Rust (edition 2024). Two build outputs from the one `web/` crate: a
`wasm32-unknown-unknown` browser library (`lib-features = ["hydrate"]`) and a native binary
(`bin-features = ["ssr"]`) that is a no-op `static_routes.generate()` pass-through this feature.

**Primary Dependencies**: `shared = { path = "../shared", features = ["render"] }` (the `render`
feature is off by default and gates the entire wgpu stack; the web crate MUST enable it). New
third-party crates (owner-approved 2026-07-13): `leptos 0.8.*`, `leptos_i18n 0.6.*`,
`leptos_i18n_build 0.6.*` (build-dep), `gloo-net 0.5.*`, `console_log 0.2.*`,
`console_error_panic_hook 0.1.*`. Already transitively present (become direct deps): `wasm-bindgen 0.2.*`,
`wasm-bindgen-futures 0.4.*`, `web-sys 0.3.*`, `js-sys 0.3.*`, `log 0.4.*`. Build tooling
(owner-approved): `cargo-leptos 0.3.*`, `wasm-opt 4.*` (Binaryen, via cargo-leptos `wasm-opt-args`),
`brotli 5.*` (CLI, used by the perf-budget report to compute sizes; it writes no compressed file).

**Storage**: OPFS (Origin Private File System) via `OpfsArtifactCache`; the static-asset embedded
bundle under `web/static/embedded_artifacts/` (gitignored, rebuilt each CI run); the live CDN bundle
resolved at runtime. `Bundle::open` reads bytes from an `ArtifactCache` — it does not fetch — so the
loader fetches into the cache, then opens.

**Testing**: host `cargo test` covers cross-platform logic once inside `shared` (manifest parse,
SHA-256, license authorization, hit-test, projection) — not re-tested in `web`. `wasm-bindgen-test
--headless --chrome` covers the three genuinely browser-divergent surfaces: the OPFS cache contract,
the browser fetch adapter error mapping, and the canvas→wgpu bridge / backend selection. No
end-to-end browser tests; visual ground truth is `docs/design/stub-desktop.html`, checked manually
pre-deploy.

**Target Platform**: modern desktop browsers (Chromium 124+, Safari 18.4+, Firefox 130+). WebGPU
primary, WebGL2 fallback — the renderer is already built to the WebGL2 feature set
(`Limits::downlevel_webgl2_defaults()`, `Features::empty()` at `shared/src/map/renderer.rs:123`).

**Project Type**: web (Rust/WASM SPA served as static assets from Cloudflare Workers Assets, plus
static CDN-delivered data; no origin server, no live API).

**Performance Goals**: first paint within the 2 MB compressed cap (WASM approx. 600–700 KB brotli +
embedded bundle approx. 700 KB–1 MB + page shell <50 KB); second paint under 3 MB. Interactions are
instant apart from the zoom-to-country camera move (the one v1 animation). Rendering is event-driven
(dirty flag + `requestAnimationFrame`); no idle rAF loop at refresh rate.

**Constraints**: single-threaded WASM — no `SharedArrayBuffer`, so no cross-origin-isolation headers
required, so the page embeds in a third-party `<iframe>` freely. The `ArtifactCache` trait is `!Send`
by design (the web impl holds `!Send` JS handles); only `Arc<Bundle>` (Send + Sync) crosses the
hot-swap `watch` channel. Offline-capable: OPFS cache + the embedded bundle as the first-paint floor.

**Scale/Scope**: 43 functional requirements (FR-001..FR-043) across 8 areas. One map route (`/`) this
feature; SSG region/about routes are a follow-up. Target approx. 1–2k LOC of per-platform glue.

## Constitution Check

*GATE: passed. Re-checked after design.*

The spec's Constitution Check (spec.md §Constitution Check) holds unchanged under this plan:

- **I Educational neutrality**: UI text is labels, units, source attribution, statistic definitions only; no editorial copy (the About page is out of scope).
- **II Source provenance (non-negotiable)**: the source panel exposes the manifest's `source_revisions`; every datum reads through `shared::*` queries against the bundle's shards.
- **III Rust core, native UI shells**: `web/` is thin glue over `shared`; all parsing, querying, projection, hit-testing, and the wgpu pipeline stay in `shared`.
- **IV Singularity convention parity**: new deps are the pre-vetted Leptos/wasm family (owner-approved); `AppError` shaped through `minimer`-via-`shared`; wildcard re-exports at module roots; `mod.rs` holds only declarations.
- **V Explicit over implicit**: the fetch adapter calls `web_sys` `fetch` directly (the wire is visible); no `#[server]`, no RPC codegen; imperative `<Routes>` tree, no route attribute macros. `leptos_i18n` is build-time translation codegen, not a network abstraction.
- **VI CDN-delivered data, no live API**: the client consumes only the static manifest + shards + the static discovery document; the Workers Assets deploy is pure static serving (no Worker handler).
- **VII Test-first for core logic**: cross-platform logic is TDD'd in `shared`; the browser-divergent glue (cache, fetch, canvas bridge) has headless-Chrome tests; Leptos UI components are exempt (visual ground truth in the stub).
- **VIII Workflow discipline**: spec + plan + supporting artifacts land on `003-web-client`; implementation proceeds as a stacked-branch sequence (below).

No principle violations; no Complexity Tracking entries required.

## Project Structure

### Documentation (this feature)

```text
specs/003-web-client/
├── spec.md                       # Feature spec (already on branch)
├── plan.md                       # This file
├── data-model.md                 # Web-side entities + the shared corrections they consume
├── quickstart.md                 # Build / run / test / deploy the web client
├── contracts/
│   └── web-module-surface.md     # web/ module surface + the shared/ingestion prereq additions
└── checklists/
    └── requirements.md           # Spec quality checklist (already on branch)
```

### Source code (repository root)

```text
# NEW — the web crate (Phases A–E)
web/
├── Cargo.toml                    # [package.metadata.leptos]; deps incl. shared { features=["render"] }
├── build.rs                      # leptos_i18n_build codegen into OUT_DIR/i18n
├── wrangler.toml                 # Cloudflare Workers Assets (static only, no Worker)
├── index.html                    # SSR page shell; mounts App on <div id="leptos">
├── locales/
│   └── en.json                   # English locale (leptos_i18n source of truth)
├── style/
│   ├── main.css                  # @imports the rest in dependency order
│   ├── tokens.css                # design tokens (accent red #d50000, link blue #0050ff — palette A)
│   ├── reset.css, typography.css, layout.css, map.css
│   └── components/{panel,button,...}.css
├── static/
│   ├── discovery                 # JSON discovery document (schema_version 1)
│   ├── _headers                  # per-path response headers
│   └── embedded_artifacts/       # GITIGNORED; synced from the downsampled bundle each CI run
└── src/
    ├── lib.rs                    # hydrate() entrypoint; include! generated i18n; module decls
    ├── main.rs                   # ssr entrypoint (no-op static_routes.generate() pass-through)
    ├── app.rs                    # root App component; <Routes> with "/" only
    ├── cache.rs                  # OpfsArtifactCache : shared::artifact::ArtifactCache
    ├── fetch.rs                  # browser fetch adapter (fetch_manifest / fetch_artifact_file)
    ├── loader.rs                 # orchestration: embedded first-paint + speculative live fetch + hot-swap
    ├── error.rs                  # web-side AppError construction helpers (thin)
    └── map/
        ├── map.rs                # MapView component
        ├── canvas.rs             # MapCanvas: owns <canvas>, RENDERER thread_local, redraw driving
        ├── legend.rs             # Legend overlay
        └── controls.rs           # statistic picker, year scrubber, source panel

# MODIFIED — shared, Phase 0a (own PR, off master)
shared/src/render/surface.rs      # + #[cfg(wasm32)] mod wasm: WgpuSurface::from_canvas(...)
shared/src/map/renderer.rs        # + #[cfg(wasm32)] attach_surface_from_canvas(...); new() takes a backend preference
shared/Cargo.toml                 # web-sys canvas types under the wasm32 target deps

# MODIFIED — ingestion, Phase 0b (own PR, off master)
ingestion/src/main.rs             # `ingestion build` (no flag) emits both bundles per version
ingestion/src/artifact/...        # build orchestration emits complete/ and downsampled/ subtrees under <version-label>/ + a latest pointer; downsampling filter: statistics = World Bank WDI at the USA reference year; geometry unfiltered

# MODIFIED — design doc (folds into Phase A)
docs/design/stub-desktop.html     # accent colors #e60019/#0030d4 -> #d50000/#0050ff (palette A canonical)

# NEW — build/deploy scripts (Phase E, except sync which pairs with 0b)
scripts/build/sync-embedded-bundle.sh   # cp -R $EAFORA_ARTIFACTS_DIR/latest/downsampled/* into web/static/embedded_artifacts/
scripts/build/measure-site-budget.sh    # artifact-byte report vs 2 MB / 8 MB caps; always exits 0
```

**Structure decision**: single `web/` workspace member (added to the root `[workspace]` members
list), feature-organized `src/` with a `map/` feature directory, per the `code-organization` rules.
The two cross-crate prerequisites live in their home crates (`shared`, `ingestion`) as independent
PRs, not under `web/`.

## Phase 0: outline & research

All findings verified against source on `master` (feature 005/006 merged) plus the installed wgpu
30.0.0 crate. Full reconciliation detail is in [`contracts/web-module-surface.md`](./contracts/web-module-surface.md)
and [`data-model.md`](./data-model.md).

### Topic 1: crate + module drift — `core::*` is `shared::*`

The spec names `core::*` throughout; the crate shipped as `shared`. The plan substitutes `core` →
`shared` everywhere, and beyond the crate rename:

- `core::hashing::sha256_hex` / `verify_sha256` → `shared::filesystem::sha256_hex` / `verify_sha256`
  (`shared/src/filesystem.rs:46,53`). No `hashing` module exists.
- `core::map::WgpuSurface` → `shared::render::WgpuSurface` (`shared/src/render/surface.rs:6`). It holds
  only `inner: Surface<'static>` + `config: SurfaceConfiguration` — NOT the adapter/device/queue (those
  live on `Renderer`). The spec's Key-Entities description (spec.md:159) is wrong on both counts.
- `core::map::map_renderer::Renderer` → `shared::map::Renderer` (`shared/src/map/renderer.rs:33`; no
  `map_renderer` submodule — re-exported flat via `pub use renderer::*`).
- `DiscoveryDocument` already exists as `shared::artifact::DiscoveryDocument` with
  `parse_discovery_document(&[u8])` (`shared/src/artifact/discovery.rs:14,21`); the spec's
  Key-Entities hedge ("in `core::artifact` or `web/src/fetch.rs`") resolves to reuse — do not redefine.

### Topic 2: FR-012 correction — the canvas surface must be built inside `shared`

`Renderer::new(bundle_receiver)` (`renderer.rs:106`) constructs its own `Instance`, `Adapter`, and
`Device`. The native attach builds the surface from those (`attach_surface` → `WgpuSurface::from_window_handle(&self.instance, &self.adapter, &self.device, …)` → private un-gated `attach(surface)`,
`renderer.rs:156–173`). A wgpu surface must be created from the same instance the device came from, so
FR-012's model (web/ builds its own `Instance` and hands back a `WgpuSurface`) is unworkable. Correction:
the canvas→surface step lives in `shared`, mirroring the native path (Phase 0a):

- `#[cfg(target_arch = "wasm32")] mod wasm` in `surface.rs` adding `WgpuSurface::from_canvas(instance, adapter, device, canvas: web_sys::HtmlCanvasElement, width, height)`.
- `#[cfg(target_arch = "wasm32")] pub async fn attach_surface_from_canvas(&mut self, canvas, width, height)` on `Renderer`, calling `WgpuSurface::from_canvas(...)` then `self.attach(surface)`.

`web/src/map/canvas.rs` then only acquires the `HtmlCanvasElement` from the DOM and calls
`renderer.attach_surface_from_canvas(canvas, w, h)`. This also uses `instance`/`adapter` on wasm32,
clearing the deferred dead-code warnings tracked in `docs/backlog.md` §Client.

### Topic 3: verified wgpu-30 canvas-surface API

Use the safe API — no `unsafe`: `Instance::create_surface(target: impl Into<SurfaceTarget<'window>>)`
with `SurfaceTarget::Canvas(canvas: web_sys::HtmlCanvasElement)` (the `Canvas` variant is `#[cfg(web)]`).
There is no `SurfaceTargetUnsafe::Canvas` variant in wgpu 30; the spec's `create_surface_unsafe`
sketch (spec.md Assumptions) is stale. FR-012's "verify against the pinned version" is satisfied.

### Topic 4: FR-015 correction — forcing WebGL2 needs a `Renderer::new` backend parameter

`Renderer::new` hardcodes `Instance::default()` (`renderer.rs:107`). Forcing `Backend::Gl` for
`?renderer=webgl2` requires building the instance with `Backends::GL`. Browsers have no process env
for wgpu's `Backends::from_env()`, so the preference must be passed in. Phase 0a extends `new()` to
take a backend selection (a small enum: default = `BROWSER_WEBGPU | GL`, or forced `GL`). Native
callers pass the default.

### Topic 5: embedded-bundle loading is the live path, same-origin

`Bundle::open<C: ArtifactCache>(cache, version_label, distribution_context)` (`bundle.rs:44`) reads
files from the cache and does not fetch. The loader (Phase C for embedded, Phase D for live) fetches
the manifest + its referenced files, `cache.put`s them, then `Bundle::open(cache, version, distribution_context)`.
The context is resolved once per session from the deployment (`distribution::resolve_context`)
and passed to every open; it is not derived from which bundle was loaded, since the onboard and live
bundles are the same deployment. Same code path as the live bundle, pointed
at the same-origin `embedded_artifacts/` directory.

### Topic 6: downsampling is the plan — statistics only, geometry full

Confirmed across `docs/architecture/{client,overview,client-web}.md`: the embedded bundle keeps
geometry at full 1:50m resolution and downsamples statistics to a single reference year (World Bank WDI only, the United States' most-recent period)
(total approx. 1.5–1.7 MB), which is what the 2 MB first-paint cap requires. `ingestion build`
emits the downsampled variant as a `downsampled/` subtree of every build (it is implemented). Until it is available, Phase C renders against a
hand-built stub bundle under `web/static/embedded_artifacts/` (gitignored).

### Topic 7: dependency decisions — RESOLVED 2026-07-13

Owner approved all new deps and tooling (Primary Dependencies above). Palette A (`#d50000` /
`#0050ff`) is canonical for `tokens.css`; the stub's `#e60019`/`#0030d4` are corrected to match
(Phase A).

## Phase 1: design & contracts

- [`data-model.md`](./data-model.md): the web-side entities (`OpfsArtifactCache`, the `thread_local`
  set — `RENDERER`, `BUNDLE_RX`, `BUNDLE_TX` — the loader state machine), how each consumes the
  `shared` types, and the corrected construction sequence (`Renderer::new(rx)` → `attach_surface_from_canvas` → `draw_frame`).
- [`contracts/web-module-surface.md`](./contracts/web-module-surface.md): the public surface of each
  `web/src` module and the exact `shared`/`ingestion` additions the prerequisite PRs introduce, with
  signatures.
- [`quickstart.md`](./quickstart.md): `cargo leptos watch`, the embedded-bundle sync, the headless
  test invocation, and `wrangler deploy`.

Threading model (single-threaded WASM): `thread_local! { static RENDERER: RefCell<Renderer> }`,
`thread_local! { static BUNDLE_RX: RefCell<watch::Receiver<Arc<Bundle>>> }`, and
`thread_local! { static BUNDLE_TX: watch::Sender<Arc<Bundle>> }` (bare — `Sender::send` takes `&self`).
`OpfsArtifactCache` is a zero-sized stateless type, never in a `thread_local!` (FR-018).

## Phasing for PRs

Two independent prerequisite PRs off `master`, then a linear stack for the web feature. Phase 0a and
0b do not depend on each other.

- **Phase 0a — `shared`: wasm32 canvas attach** (own PR, off `master`). `WgpuSurface::from_canvas`,
  `Renderer::attach_surface_from_canvas`, `Renderer::new` backend parameter. Closes the
  `docs/backlog.md` §Client canvas-attach item and clears the wasm32 dead-code warnings. Covers the
  `shared` half of FR-012 and FR-015.
- **Phase 0b — `ingestion`: dual-bundle build orchestration** (own PR, off `master`). `ingestion build`
  emits both the complete and downsampled variants under one `<version-label>/` plus a `latest` pointer;
  unblocks FR-004 and the real `sync-embedded-bundle.sh`. Independent of
  the web stack; the interim hand-stub covers Phase C in the meantime.
- **Phase A — workspace + build toolchain + Leptos shell + CSS tokens** (stacks on `003-web-client`).
  FR-001, 002, 003, 007, 008, 009, 010, 010a, 035, 036, 037, 038, 039; plus the stub color correction.
  Deliverable: `web/` compiles under `ssr` and `hydrate`; empty `/` route renders the chrome.
- **Phase B — OPFS cache adapter** (stacks on A). FR-017..024, 040. Self-contained; headless-Chrome
  tested against a real OPFS; needs no bundle.
- **Phase C — canvas→wgpu surface + MapView first paint** (stacks on B; needs Phase 0a merged + a
  bundle). FR-011, 012, 013, 014, 015, 016, 031, 042. Closes P1. Renders the hand-stub embedded bundle.
- **Phase D — fetch + discovery + speculative fetch + hot-swap** (stacks on C). FR-025..030, 041.
  Adds `BUNDLE_TX` publishing; the renderer's `borrow_and_update` read path (`renderer.rs:188`) already
  consumes it. Closes P3.
- **Phase E — perf-budget + static shell export + deploy config** (stacks on A; `sync` needs Phase 0b). Precompression was in this phase's scope and was withdrawn; see spec FR-005.
  FR-004, 005, 006, 032, 033, 034, 043. Closes P4.

### Deferred to C2: viewport aspect ratio

Phase C1 renders the whole world stretched to fill the surface: the `Viewport` is fixed to
`world_viewport()` and mapped edge-to-edge onto the surface with no aspect correction, so a non-square
canvas distorts the map. This is deferred to C2, where the `Viewport` becomes a camera (center + zoom)
whose aspect ratio tracks the surface's. On resize, recompute the viewport extent to match the new
surface aspect (keeping center and zoom) so a wider canvas reveals more world horizontally instead of
stretching; pan translates the viewport, zoom scales it. Resizing the backing store to the canvas's
device pixels is correct and independent (render resolution, not framing) and stays as is.

Findings to carry into that work:

- **Natural Miller aspect ratio** (width:height): approx. 1.36:1 across the full ±90° latitude; approx.
  1.53:1 at the current ±85° clamp.
- **Unit reconciliation.** `projection::project` returns `x` as longitude in degrees but `y` derived
  from radians. Compute the aspect with `x` in radians (360° = 2π): width `2π`, height `2·y(clamp)`.
  Using the raw stored degrees (x-extent 360 vs. y-extent approx. 4.09) yields an approx. 88:1 nonsense
  ratio.
- **`WORLD_BOUNDS` ±85° clamp is a Web-Mercator carryover, not a Miller requirement.** Miller's `tan`
  asymptote sits at ±112.5° latitude (`π/4 + 0.4·φ = π/2`), outside the ±90° domain, so Miller maps the
  actual poles to a finite `y` (approx. 2.30 at ±90°) — its whole advantage over Mercator. The ±85°
  clamp buys nothing for asymptote-avoidance; revisit it (full ±90°, or a deliberate aesthetic crop) and
  correct the clamp's comment, which currently states the false asymptote rationale.

## Brief PR description

**Web client — plan and design artifacts (`003-web-client`).**

Plans the browser surface: a Leptos/WASM app rendering the choropleth map through the `shared`
renderer against a static embedded bundle, hot-swapping to the live CDN bundle. Records the
corrections the built `shared` crate forces on the spec (the crate is `shared` not `core`;
`WgpuSurface` lives in `shared::render` and holds only surface+config; the canvas surface must be
built inside `shared` from the renderer's own instance; forcing WebGL2 needs a `Renderer::new`
backend parameter; `Bundle::open` reads from the cache rather than fetching). Splits the two
cross-crate prerequisites — the wasm32 canvas attach in `shared` and the `ingestion build` dual-bundle orchestration —
into their own PRs, and lays out the web feature as a linear stack (workspace/shell → OPFS cache →
first paint → live fetch/hot-swap → perf-budget/deploy). Affected crates: `web` (new), `shared`,
`ingestion`; plus `docs/design/stub-desktop.html` (accent palette corrected to the canonical `#d50000`/`#0050ff`).

## Post-implementation notes — Phase A (2026-07-13)

Delivered on branch `web-scaffold` (stacked on `master`, since the `003-web-client` planning artifacts
were merged into `master` rather than kept on an open branch). Deviations from the plan/spec, all
validated against the pinned crates:

- **FR-007 (`index.html`) → a `shell()` fn.** leptos 0.8 ssr+hydrate renders the HTML shell from
  `web/src/app.rs::shell(options)` (with `HydrationScripts`), not a static `index.html`.
- **SSR binary uses `leptos_axum` as a running dev server** (`cargo leptos watch`), owner-approved
  2026-07-13, rather than the "no serve / SSG-only" sketch in `client-web.md`. Production still ships
  static assets; the static-export step is deferred to Phase E. New deps: `leptos_axum` + `axum`
  (build/dev-time only).
- **`leptos_meta` dropped** for the minimal shell; add it if `<title>`/`<meta>` management is needed.
- **`gloo-net` pins to `0.6.*`** (the resolved version), not `0.5.*`; its wiring is deferred to Phase D.
- **Deferred deps** (`web-sys`, `js-sys`, `gloo-net`, `wasm-bindgen-futures`) are added when Phases
  B/C/D consume them, rather than all up front.
- Validated: the crate compiles under both `hydrate` (wasm32) and `ssr`, and `cargo leptos build`
  produces `target/site/` (WASM + JS + bundled CSS + static assets) end to end.
