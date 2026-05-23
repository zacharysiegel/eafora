# Architecture overview

<!--
Status: draft, 2026-05-21. This document is the cross-cutting architecture for Eafora — the contracts, the workspace shape, the data flow, the per-platform integration patterns, and the cost model. Per-platform implementation plans (web, iOS, Android, ingestion) get their own documents in follow-up branches and reference this overview for shared decisions.

Several specifics — exact pricing, the chosen CI service's free-tier limits, Apple ETP applicability — are approximate and flagged in §Things to verify near the end.
-->

## Scope of this document

This is the architecture **overview**: the cross-cutting decisions that bind the four segments of Eafora (Rust core, ingestion + canonical store, web client, native mobile clients) into one coherent system. It does not double as a per-segment implementation plan. Each of the segments below gets its own implementation document in a subsequent branch:

- `docs/architecture/client-web.md`
- `docs/architecture/client-ios.md`
- `docs/architecture/client-android.md`
- `docs/architecture/ingestion.md`

Those follow-up documents inherit the contracts established here. Conflicts between this document and a segment plan are resolved by amending this document first.

## Locked decisions referenced (not relitigated)

The constitution at `.specify/memory/constitution.md` is the source of truth. The following decisions are already binding and are referenced — not re-argued — in this overview:

- Pure data viz; no editorial copy. (I)
- Per-cell source provenance with retrieval timestamp and license. (II)
- Maximal Rust core; native UI shells (Leptos+WASM, SwiftUI, Jetpack Compose). (III)
- Convention parity with `/Users/singularity/singularity` for backend stack. (IV)
- Imperative actix-web routing; hand-written `sqlx::query_as!`; no RPC frameworks; HTTP+JSON over reqwest. (V)
- v1–v2 client data path is CDN-hosted versioned artifacts (FlatGeobuf for geometry, SQLite for indicators); no live data API through v2. (VI)
- TDD for the Rust core's logic surfaces; UI shell code exempt. (VII)
- Spec-Kit per-feature flow; `docs/` for cross-cutting work. (VIII)

The data sources doc at `docs/data/sources-survey.md` and licensing research at `docs/research/data-source-licensing.md` are the source of truth for what gets ingested and under what license. This overview only summarizes them where the architecture has to honor a constraint.

## System architecture

```
                        ┌─────────────────────────────────────────────────────┐
                        │                   Public data sources                │
                        │  World Bank WDI, Eurostat, HFD, OWID, WPP (TBD), ...  │
                        └─────────────────────────────┬───────────────────────┘
                                                      │ HTTP (reqwest)
                                                      ▼
            ┌──────────────────────────────────────────────────────────────────┐
            │                   Eafora ingestion service (Rust)                │
            │       per-source adapters → normalize → write to canonical       │
            │       PostgreSQL → emit versioned artifacts to CDN                │
            └────────────┬───────────────────────────────────────┬─────────────┘
                         │                                       │
                ┌────────▼────────┐                       ┌──────▼─────────┐
                │ canonical store │                       │ artifact store │
                │   PostgreSQL    │                       │   S3 / R2 +    │
                │  (provenance,   │                       │   CloudFront   │
                │   versioning)   │                       │  (FlatGeobuf +    │
                └─────────────────┘                       │   SQLite, +    │
                                                          │   manifest.json)│
                                                          └──────┬─────────┘
                                                                 │ HTTP (range requests; cache; brotli)
                                                                 │
                ┌────────────────────────────────────────────────┼────────────────────────────────────────┐
                │                                                │                                        │
                ▼                                                ▼                                        ▼
   ┌───────────────────────────┐                   ┌───────────────────────────┐         ┌───────────────────────────┐
   │   Web client              │                   │    iOS client             │         │   Android client          │
   │   Leptos + WASM           │                   │    SwiftUI + MTKView      │         │   Compose + SurfaceView   │
   │   wgpu (WebGPU/WebGL)     │                   │    Rust core via UniFFI   │         │   Rust core via UniFFI    │
   │   Rust core via wasm-     │                   │    xcframework, wgpu via  │         │   AAR + cargo-ndk, wgpu   │
   │   bindgen, IndexedDB      │                   │    Metal                  │         │   via Vulkan              │
   │   cache                   │                   │                           │         │                           │
   └───────────────────────────┘                   └───────────────────────────┘         └───────────────────────────┘
```

Key invariants captured by the diagram:

- **Clients never call Eafora's origin server through v2.** Every datum the user sees came from a versioned CDN artifact.
- **There is exactly one canonical store** (PostgreSQL). All other data representations downstream are derived artifacts with reproducible builds.
- **The same Rust core runs in all three clients.** Web consumes via wasm-bindgen; iOS and Android via UniFFI.
- **Per-source adapters are isolated.** Each data source is one Rust module; no source's quirks leak into the canonical store schema.

## Workspace and crate layout

Eafora is a single Cargo workspace (matching Singularity's monorepo pattern). Proposed top-level structure:

```
eafora/
├── Cargo.toml              # workspace root
├── rustfmt.toml            # max_width=120, chain_width=100, edition 2024
├── compose.template.yaml   # Podman compose template (Postgres, ingestion runtime — Singularity-style)
├── compose.yaml            # generated from template by setup.sh, gitignored
├── secrets.yaml            # secr-encrypted secrets
├── setup.sh                # first-time setup: prerequisites, secrets decrypt, sqlx prepare
├── dbmate.sh               # dbmate wrapper, also runs cargo sqlx prepare --workspace
├── scripts/                # tooling scripts (branch-init.sh, cleanup-merged.sh)
├── docs/                   # cross-cutting research and architecture
├── specs/                  # per-feature spec-kit artifacts (NNN-slug)
├── .specify/               # spec-kit machinery
├── core/                   # Rust core: data models, math, projection, wgpu, ingestion logic
│   ├── Cargo.toml
│   └── src/                # feature-organized modules; mod.rs only declares + re-exports
├── ingestion/              # actix-web binary (Singularity's lobby analog)
│   ├── Cargo.toml
│   ├── db/migrations/      # dbmate migrations
│   └── src/                # per-source adapters + artifact builder + scheduled runner
├── web/                    # Leptos web shell (cargo-leptos workspace member)
│   ├── Cargo.toml
│   └── src/
├── ios/                    # SwiftUI shell + UniFFI consumer; Xcode project + glue
│   ├── Eafora.xcodeproj
│   └── EaforaApp/
├── android/                # Compose shell + UniFFI consumer
│   ├── build.gradle.kts
│   └── app/
└── data/                   # gitignored. Bundled-fallback artifacts generated at build time
                            # by scripts/build-fallback.sh (downsamples the latest CDN
                            # artifact and copies into platform resources). Not committed —
                            # the CDN's content-hashed object store is the source of truth
                            # for any historical bundled-fallback shape.
```

Notes on this shape:

- `core/` is the most-shared crate. It exposes both wasm-bindgen and UniFFI surfaces via thin adapter modules (see §FFI boundaries) and stays free of platform-specific code.
- `ingestion/` is a separate crate that depends on `core` and adds the actix-web router, sqlx queries, source adapters, and artifact builders. Splitting it from `core` means clients don't pull in actix-web or sqlx into their WASM/UniFFI builds.
- `ios/` and `android/` are not Cargo crates — they're native projects that consume artifacts produced by `core`. The `core` crate's UniFFI build emits an `xcframework` and an AAR that these projects link against.
- `web/` is a Cargo workspace member because cargo-leptos drives it. It depends on `core` directly and adds Leptos components, routing, and the wasm-bindgen adapter.
- `data/` is gitignored. The bundled-fallback artifacts (small downsampled FlatGeobuf + SQLite shipped inside each app build for instant first-launch UX) are generated at build time by `scripts/build-fallback.sh`, which fetches the latest manifest from the CDN, downloads the latest full artifact, downsamples it (drop sub-national geometry, keep only the most recent year of statistic values), and stages the results into `data/` plus the per-platform resource directories (`ios/EaforaApp/Resources/`, `android/app/src/main/assets/`, `web/static/`). Reproducibility for any given commit comes from the CDN's content-addressed object store, not from `git`.
- Per the constitution's Singularity convention parity, `compose.yaml` / `dbmate.sh` / `setup.sh` / `secrets.yaml` mirror Singularity's setup verbatim.

## Rust core

### Module organization

Per the user's organize-by-feature preference and Singularity's pattern, `core/src/` is laid out by feature, not by layer. Sketch:

```
core/src/
├── lib.rs                  # workspace entrypoint; re-exports public API
├── error.rs                # uses minimer; per-feature error variants live in their feature module
├── geometry/               # vector polygon model, projection math, hit-testing
│   ├── mod.rs              # `mod ...; pub use ...;` only
│   ├── projection.rs       # Miller cylindrical projection + horizontal wraparound; rectangular world fills the viewport corners
│   ├── polygon.rs          # boundary representation, simplification
│   └── hit_test.rs         # smoothed-scale-without-hitbox-growth math
├── statistic/              # statistic types (TFR, CBR, CDR, etc.), units, time series, parsing
│   ├── mod.rs
│   ├── tfr.rs
│   ├── time_series.rs
│   └── status.rs           # data_status enum (final / provisional / projection / imputed / interpolated)
├── render/                 # wgpu pipelines, shader bindings, surface management
│   ├── mod.rs
│   ├── surface.rs          # platform-agnostic surface init; thin platform adapters in callers
│   ├── pipeline.rs         # vertex + fragment WGSL pipelines for borders, fills, hover ring
│   └── animation.rs        # zoom-to-country curves; smoothed hover scale
├── ingest/                 # ingestion-side feature modules (only built into `ingestion/` binary)
│   ├── mod.rs
│   ├── world_bank/         # one source = one feature module = api.rs + parser.rs + tests
│   ├── eurostat/
│   └── ...
├── artifact/               # FlatGeobuf writer, SQLite writer, manifest writer
│   ├── mod.rs
│   ├── flatgeobuf.rs
│   ├── sqlite.rs
│   └── manifest.rs
├── ffi/                    # FFI adapters; one submodule per binding tool
│   ├── mod.rs
│   ├── wasm.rs             # wasm-bindgen surface
│   └── uniffi.rs           # UniFFI surface
└── boundary/               # contested-borders abstraction (per-locale swap design)
    └── mod.rs
```

Notes:

- `mod.rs` files only declare submodules and re-export. Logic lives in named files. Pair `mod X;` with `pub use X::*;` on the next line per the Singularity convention.
- File ordering inside each `.rs`: `use`s, statics, types (with their `impl` blocks paired), then functions.
- Types are paired with their `impl` blocks immediately below them (Singularity convention). Don't group all type declarations at top.

### Public API design

The Rust core's public API has three consumers (wasm-bindgen, UniFFI on iOS, UniFFI on Android) with different constraints. The constitution's Principle V says we don't reach for code-generation magic that hides the wire — but for the FFI specifically, codegen is the only viable path. The discipline is to keep the *generated* binding layer thin and isolated, and to design the underlying API so that both binding tools can express it without contortions.

Practical rules:

1. **Use concrete types, never generics across the FFI boundary.** UniFFI does not support generics across FFI; wasm-bindgen does, but writing the API to UniFFI's constraints lets one definition serve both.
2. **No trait objects or `dyn Trait` exposed.** Same reason.
3. **All errors are concrete enums implementing minimer's traits**, with payload data limited to strings and primitives. Both UniFFI and wasm-bindgen marshal these cleanly.
4. **Async is supported but cancellation is not.** UniFFI added async support around 2023 but cancellation tokens are still missing as of early 2026. The core's async functions must self-cancel based on a polled flag if cancellation matters.
5. **Vectors and maps cross the boundary, but expensively.** Returning a `Vec<CountryStatistic>` of 200 entries is fine; doing it 200 times per frame is not. Design the API for batch calls (one call returning all statistics for a viewport, not 200 calls returning one each). FFI overhead is roughly 1–10 µs per call before payload marshaling.

The `ffi::wasm` and `ffi::uniffi` submodules contain the *only* code that knows about a binding tool. They wrap concrete `core::*` types in binding-specific facade types where the binding tool requires it (e.g. an `StatisticSetWeb` wrapping an `StatisticSet`). This isolation means the core itself stays binding-agnostic and independently testable.

### Error handling

Per the constitution: **minimer**. The core uses `minimer::Error` (or whatever the published name turns out to be — the crate is the user's published generalization of Singularity's in-house `AppError`). Per-feature modules define their own concrete error variants where useful for matching:

```rust
// core/src/statistic/error.rs (sketch)
#[derive(Debug)]
pub enum StatisticError {
    InvalidIso(String),
    UnknownStatistic(String),
    MissingYear { country: String, year: u32 },
}

// boundary code converts to minimer::Error / AppError at the public API surface
```

The FFI layer maps these to the binding-specific representation: Swift `throws`, Kotlin exceptions, JS exceptions or Result-as-tagged-union.

### Async model

Two regimes in the same crate:

- **Native (iOS, Android, ingestion binary)**: full tokio multithreading. The `ingestion/` binary uses `tokio::main` with `features = ["full"]`. iOS and Android use UniFFI's async support for fetch/parse calls.
- **WASM**: single-threaded (`SharedArrayBuffer` is intentionally avoided — see §Web client below). The core's WASM-facing surface is built without `Send + Sync` requirements; per-WASM state lives in `thread_local!`.

To keep the core code agnostic, the `core` crate is `#[cfg]`-aware where it must be:

- Functions that genuinely need threading are gated behind `#[cfg(not(target_arch = "wasm32"))]`.
- The default API works in both regimes by sticking to immediate values and `async fn` (which compiles fine in both).

### Geometry, projection, hit-testing

The `core::geometry` module is the math heart of the renderer. Concretely:

- **Projection**: **Miller cylindrical**, the only projection. Rectangular (~5:3 aspect ratio) so it fills the viewport corners with no UI dead zones; closed-form `(lon, lat) → (x, y)` in ~5 lines. Less polar inflation than Mercator while staying conformal at low latitudes. No alternate projections, no toggle, no v2+ plans for additional projections — this decision is closed. Implementation is a pure function in `core::geometry::projection`; no GIS library required.
- **Polygon representation**: full polygons everywhere; no LOD, no tile pyramid, no per-frame simplification. v1 ships ~200 country polygons (~5 MB compressed in FlatGeobuf, with the format's built-in R-tree spatial index used directly for hit-testing). When sub-national geometry lands in v2+, it joins the same FlatGeobuf as additional features and loads in the background after country features render. The wgpu pipeline rasterizes the same vertex data at every zoom level — small countries at low zoom contribute fewer pixels, which is the right behavior for free, with no LOD-boundary popping. Continuous zoom drops out automatically.
- **Hit-testing**: A spatial index (R-tree or interval-tree) over the country polygons, queried at viewport-space resolution. **Critical UX rule**: the hit-test geometry uses the *unscaled* country polygon. The hover-scale effect only changes the rendering transform, never the hit-test — this is the user-stated requirement that off-the-shelf map SDKs typically violate.
- **Animation**: Zoom-to-country uses a cubic-easing time curve; the camera target is the country's polygon centroid; the camera scale is computed from the polygon's bounding box plus a margin. Implemented as a `core::geometry::animation::Camera` state machine the renderer polls each frame.

### wgpu rendering pipeline

The core owns a small set of wgpu render pipelines, all written in WGSL:

| Pipeline | Purpose |
|---|---|
| `borders` | Country boundary lines, anti-aliased, single-pixel thin |
| `fills` | Solid country fills, color computed per-country from the statistic value (red→blue gradient for TFR) |
| `hover_scale` | Per-country scale transform applied at draw time; outputs to an offscreen buffer composited over the main render with source-over alpha |
| (no `country_label` GPU pipeline) | Country labels render as **native text overlays** on top of the wgpu canvas — positioned via Rust-computed screen coordinates of country centroids, drawn by HTML/SwiftUI/Compose text widgets. Defers font shaping, accessibility, complex-script handling, and zoom-stable typography to the platform. A future GPU-based label pipeline (single-channel SDF or MSDF) is possible if we ever want labels rendered inside the wgpu pass; not needed for v1's modest label density. |

Shaders are simple — the data is small (200 countries, simplified polygons), so we don't need indirect draws or compute. The core exposes a single `render(viewport, statistic_state) -> CommandBuffer` function the platform shells call from their render loop.

The `core::render::surface` adapter receives a platform-agnostic surface handle (a `*mut c_void` plus dimensions) and creates a `wgpu::Surface` from it. Each platform shell does the small bit of glue to provide that handle:

- **Web**: `Instance::create_surface_from_canvas(&canvas)` (wasm-bindgen).
- **iOS**: passes `MTKView`'s drawable layer pointer through UniFFI; Rust uses `raw-window-handle` 0.6's iOS variant.
- **Android**: Kotlin passes the `Surface` jobject through JNI; Rust calls `ANativeWindow_fromSurface` and constructs the wgpu surface from the Vulkan handle (no GLES path at the API-31 baseline).

## FFI boundaries

### The dividing line: what crosses, what stays native

Not everything goes through the Rust core. The cost of FFI calls plus the limitations of UniFFI's type expressiveness mean some things are better left to the platform. The recommended dividing line:

| Concern | Where it lives | Why |
|---|---|---|
| Geometry, projection, hit-testing | Rust core | Hot, math-heavy, identical across platforms |
| Statistic math (color mapping, time-series interpolation, derivation) | Rust core | Same |
| wgpu rendering pipeline | Rust core | Whole point of the architecture |
| Artifact parsing (FlatGeobuf, SQLite reads) | Rust core | One source of truth for the data format |
| HTTP fetches | Each platform's native HTTP stack | Battle-tested; integrates with platform caching, proxies, certs; async ergonomics are better; FFI overhead is dominated by the I/O time anyway (a few microseconds vs hundreds of milliseconds) |
| UI chrome (header, panels, controls, navigation) | Each platform's native UI framework | The whole point of "native UI shells" |
| Animations of UI chrome | Each platform's UI framework | Map animations are wgpu; UI animations are SwiftUI / Compose / CSS |
| Domain-content i18n (country names, statistic names, units, source attribution, data-status labels) | Rust core | Hand-curated translation table baked into the SQLite artifact at build time; sourced from ISO 3166 (countries) plus per-language overrides where the canonical name is unidiomatic. Joined to upstream-source values by ISO 3166 alpha-3. **v1 ships English only.** Single source of truth across all three clients; renaming happens in one place. FFI exposes `country_name(iso, locale)` / `statistic_definition(id, locale)` lookups |
| UI-chrome i18n (controls, errors, navigation, accessibility labels) | Each platform's native i18n | Platform APIs handle pluralization, RTL, locale-aware number/date formatting, accessibility, and translator-tooling integration (`.strings` / `strings.xml` / web i18n) for free |
| Push notifications, deep links | Each platform | Platform-specific by nature |

### Per-binding adapters

The `core::ffi::wasm` and `core::ffi::uniffi` modules are the *only* places that depend on a binding tool. Each defines a thin facade. Sketch for UniFFI:

```rust
// core/src/ffi/uniffi.rs
#[uniffi::export]
pub struct EaforaCore { /* opaque */ }

#[uniffi::export]
impl EaforaCore {
    #[uniffi::constructor]
    pub fn new(artifact_path: String) -> Result<Self, EaforaError> { /* ... */ }

    pub fn render_frame(&self, viewport: Viewport) -> Result<RenderCommands, EaforaError> { /* ... */ }

    pub fn country_at_point(&self, viewport: Viewport, point: ScreenPoint)
        -> Option<CountryId> { /* ... */ }

    pub async fn parse_country_payload(&self, json: String)
        -> Result<CountryDetail, EaforaError> { /* ... */ }
}
```

For wasm-bindgen the facade is similar but uses `#[wasm_bindgen]` and JS-friendly types (`Vec<u8>` instead of `String` for binary payloads, `JsValue` for fallible returns). The two facades wrap the same internal types from `core::statistic`, `core::geometry`, etc. — the duplication is in the *binding plumbing*, not the *logic*.

### UDL vs proc-macro for UniFFI

Eafora uses **UDL** (the declarative `.udl` file form). Pros: separation between Rust impl and FFI contract; easier IDE navigation; better error messages on schema violations; community norm (1Password, Mozilla AppServices). Proc-macros are slightly more flexible for generics (which we don't use) and inline-with-code. UDL is the boring-correct choice.

## Per-platform v1 build vs iteration scope

All three platform shells (web, iOS, Android) are **scaffolded with their full build toolchain from day one** — workspace member, FFI binding consumption, signing config, CI workflow, "hello world" surface that calls into the Rust core and renders something. The reasons are the same as for iOS earlier in this document: forcing the Rust core's API to be honest with three consumers from the start, validating the build pipeline before it's gnarly to fix, and avoiding cross-platform surprises late.

**v1 iteration scope is narrower:**

| Platform | Build toolchain | Iteration in v1 |
|---|---|---|
| Web | Live, end-to-end (cargo-leptos + wasm-bindgen + Cloudflare Pages deploy) | Yes — primary surface |
| iOS | Live, end-to-end (xcframework + Xcode + TestFlight) | Yes — second surface, lags web |
| Android | Live, end-to-end (cargo-ndk → AAR + Gradle + Play Console internal track) | Minimal — keep the hello-world building, do not actively iterate features until web + iOS stabilize |

In other words: Android's full toolchain is on the critical path for v1's "complete and working" definition; Android's *user-facing features* are not. This avoids the trap of writing a lot of Android-touching architecture that only gets validated in v2.

## Web client (overview)

Detailed plan: `docs/architecture/client-web.md` (follow-up branch). Key contracts established here:

- **Framework**: Leptos, built with `cargo-leptos`. The `web/` workspace member contains the Leptos app.
- **Primary target is desktop browsers.** Because Eafora ships native iOS and Android apps, the mobile-browser experience is an explicit fallback path — for users who haven't installed the native app yet, are on platforms we don't have native apps for, or hit Eafora once via a shared link. Mobile-browser builds need to be functional and not visibly broken, but bespoke mobile UX work (touch-tuned gestures, mobile-only layouts, mobile-perf optimization) is **not** in v1's scope; that effort goes to the native shells. Responsive layout is best-effort.
- **Rendering modes**: hybrid. Country detail pages (e.g., `/country/jp`) are SSG'd at build time so search engines see real content. The map view is CSR — the wgpu canvas can't be SSR'd, and we don't try.
- **Map rendering**: wgpu via WebGPU primarily, with WebGL2 fallback through wgpu's downlevel backend. Browser support in mid-2026: Chromium stable; Safari 18.4+ stable; Firefox WebGPU not yet shipped, falls back to WebGL2. Cargo: `wgpu = { version = "...", features = ["webgpu", "webgl"] }`.
- **Threading**: single-threaded WASM. We **do not** use `SharedArrayBuffer` and therefore do not require `Cross-Origin-Opener-Policy: same-origin` + `Cross-Origin-Embedder-Policy: require-corp` headers. This keeps the door open for future third-party embedding (UN portals, journalism sites) without wrestling with cross-origin isolation. Per-WASM state lives in `thread_local!`.
- **Bundle size**: target ~500–700 KB brotli-compressed (~2–3 MB raw WASM), of which Leptos is ~400 KB and wgpu+Naga is ~600 KB. `wasm-opt -O4` in release builds.
- **Data loading**: JS-side `fetch()` reads the artifact (manifest → SQLite + FlatGeobuf bytes) and stores in IndexedDB. Rust receives `&[u8]` and constructs the in-memory data structures. FlatGeobuf has a built-in R-tree spatial index that we use directly for hit-testing — no separate index build step. Country features parse first; subnational features parse in the background after the initial render. **For SQLite: download once, cache in IndexedDB, query in-memory via a WASM-built `rusqlite` or `sqlx`.** This works because per-statistic SQLite files are small (~tens of KB to a few MB through v2). Migration trigger: if any per-statistic file grows past tens of MB, switch to HTTP-range-request reads via a Rust-side custom `Connection` impl. Not anticipated through v2.
- **IndexedDB quota and eviction**: the cache layer must call `navigator.storage.persist()` on first launch to opt into persistent storage (avoids LRU eviction under disk pressure), call `navigator.storage.estimate()` before each artifact write to fail fast on quota-exceeded, and recover gracefully when the cache is missing on launch (re-fetch as if first run). Per-browser policies vary; the deep details belong in `docs/architecture/client-web.md` and reference [MDN: Storage quotas and eviction criteria](https://developer.mozilla.org/en-US/docs/Web/API/Storage_API/Storage_quotas_and_eviction_criteria).
- **Hot reload**: `cargo leptos watch` for development; full WASM rebuild is ~5–15 s. Splitting `core` into a library crate and `web` into a thin entry crate minimizes recompile scope.

## iOS client (overview)

Detailed plan: `docs/architecture/client-ios.md` (follow-up branch). Key contracts:

- **UI**: SwiftUI.
- **Map surface**: `MTKView` wrapped in `UIViewRepresentable`. Delegate methods (`drawableSizeWillChange`, `draw(in:)`) drive the render loop on the main thread; heavy compute is offloaded to background tasks before the next frame.
- **Rust integration**: `core` is built as an xcframework via `cargo build --target aarch64-apple-ios --release` + `cargo build --target aarch64-apple-ios-sim --release` + `xcodebuild -create-xcframework`. UniFFI generates Swift bindings into the xcframework.
- **GPU baseline**: Apple A14 (iPhone 12, late 2020) / A15 (iPhone 13, September 2021) and later. iOS 18+ minimum SDK target. Drops every device older than the 2021 generation per the user's direction; cuts long-tail support burden, modern Metal feature levels are uniformly available across all supported devices, and active iPhone-user share in the anglosphere/EU on pre-2021 hardware is small enough to ignore for v1.
- **Async**: Swift's `async`/`await` consumes UniFFI async functions naturally; cancellation is one-way (Swift task cancellation does not propagate; Rust must self-cancel).
- **HTTP**: Swift's `URLSession`. Fetched JSON payloads are passed to the Rust core for parsing.

## Android client (overview)

Detailed plan: `docs/architecture/client-android.md` (follow-up branch). Key contracts:

- **UI**: Jetpack Compose.
- **Map surface**: `SurfaceView` wrapped in `AndroidView`. The render loop runs on a dedicated thread (not Choreographer-on-main) to avoid main-thread jank from GPU command encoding. Surface lifecycle (rotation, pause/resume) is explicitly handled via `SurfaceHolder.Callback`.
- **Rust integration**: `core` is built as an AAR via `cargo-ndk -t aarch64-linux-android -t x86_64-linux-android build --release`; the resulting `.so` files go into `jniLibs/{arm64-v8a,x86_64}/`. UniFFI generates Kotlin bindings. (32-bit ARM is not built — see GPU baseline below for the API-31 cutoff rationale.)
- **GPU baseline**: minSdk = **API 31** (Android 12, October 2021), per user direction symmetric with the iOS 2021-generation baseline. Vulkan 1.0 is universally available at this level; no OpenGL ES 3.0 fallback path is needed. The 32-bit `armv7-linux-androideabi` Cargo target can also be dropped from the cargo-ndk build (no API-31+ devices ship 32-bit ARM); only `aarch64-linux-android` and `x86_64-linux-android` (emulator) are required.
- **HTTP**: **OkHttp**, called directly. Not Retrofit — Retrofit is an annotation-driven codegen layer over OkHttp that hides the HTTP shape behind interfaces, which conflicts with Constitution Principle V (explicit over implicit). OkHttp is the explicit choice, matches Singularity's pattern, and is the standard ergonomic Android HTTP client on its own.

## Ingestion + canonical store (overview)

Detailed plan: `docs/architecture/ingestion.md` (follow-up branch). Key contracts:

- **Stack**: actix-web binary, tokio runtime (`features = ["full"]`), sqlx with `query_as!` and offline cache, dbmate migrations, reqwest for outbound HTTP. Per Singularity's `lobby/` pattern: each feature module is `<feature>_api.rs`, `<feature>_db.rs`, `<feature>_model.rs` triplet inside a feature directory. Through v2 the binary doesn't actually serve HTTP requests — there's no live API. The actix-web dependency is forward-looking: if v3+ introduces a live API for user-submitted corrections (or any other online-only feature that emerges later), the same binary picks it up via a new `configurer` module.
- **Per-source adapters**: one Rust module per source. Each adapter exposes a `pub async fn fetch_and_normalize(pool: &PgPool) -> Result<IngestReport, AppError>` that reqwest-fetches, parses, and writes to the canonical store. Sources are independent — adding a new one is one new module, not a refactor.
- **Canonical store**: PostgreSQL. Schema sketch:
  - `country (iso3, name, region, ...)`
  - `statistic (id, name, units, definition, ...)` — TFR, CBR, CDR, etc.
  - `source (id, name, url, license, license_url, ...)` — every row in the licensing matrix from `docs/research/data-source-licensing.md` is one record here
  - `statistic_value (country_iso3, statistic_id, year, value, source_id, retrieved_at, data_status)` — the fact table; one row per (country, statistic, year, source). When multiple sources publish the same datum, all rows are kept; the merge is done at artifact-build time per a documented preference order.
  - `data_status` is an enum: `final`, `provisional`, `preliminary`, `flash_estimate`, `projection`, `imputed`, `interpolated` (matches the `docs/data/sources-survey.md` Preliminary section).
  - `artifact_version (id, manifest_url, built_at, source_versions_jsonb)` for reproducibility.
- **Artifact builders**: a `pub async fn build_artifacts(pool: &PgPool, output: &Path)` function reads the canonical store, applies the source-preference merge rules, and emits a `flatgeobuf` file (geometry) + a `sqlite` file (statistic data) + a `manifest.json`. The output is content-hashed and uploaded to the CDN.
- **Schedule**: **v1 runs on the owner's Mac mini M1 server** (already owned, runs continuously). The ingestion binary is triggered via `launchd` (macOS's cron equivalent) on a schedule — likely weekly Mondays for v1, more often as the source-update cadence grows. The same machine hosts the canonical Postgres. Through v1, manual invocation is also fine. **Cloudflare Tunnel** is already provisioned on the Mac mini, so when v3+ adds a live API the inbound path is in place; the Tunnel is dormant through v1–v2 because clients never call origin. Migration to managed compute (Fly.io / Render / Lambda / a VPS) is a v2+ concern, deferred until traffic or reliability needs force it.

## Artifact distribution

### Storage and CDN

Comparison (concrete numbers approximate; see §Things to verify):

| Provider | Storage cost | Egress cost | Notes |
|---|---|---|---|
| AWS S3 + CloudFront | $0.023/GB/mo | $0.085/GB (first 10TB) | Conventional; expensive at scale |
| Cloudflare R2 + free CDN | ~$0.015/GB/mo | **$0** to end-users | Best long-term economics; zero egress is the killer feature |
| Backblaze B2 + Cloudflare | ~$0.006/GB/mo | $0.01/GB (B2) but $0 to Cloudflare edge | Cheap; setup is a bit fiddlier |
| GitHub Releases | $0 | no edge caching, rate limits | Acceptable for v1 only |
| Netlify / Vercel | $0 free tier; ~$11/mo Pro | bundled in plans | Nice for the web app static files; not the right shape for binary artifacts |

**Recommendation**: **Cloudflare R2** for the artifacts. The zero-egress model is decisive; the operational story is simple (S3-compatible API, public buckets); it scales from v1 to v2 to v3 without re-platforming. The web app's static files (Leptos build output) can ride on Cloudflare Pages from the same account.

### Artifact format

```
manifest.json:
{
  "version": "2026-w21",
  "built_at": "2026-05-21T14:00:00Z",
  "geometry": {
    "url": "/geometry/world-1.50m-ab12cd34.fgb",
    "size_bytes": 4380000,
    "sha256": "..."
  },
  "statistics": {
    "tfr": { "url": "/data/tfr-ab12cd34.sqlite", "size_bytes": 89000, "sha256": "..." }
  },
  "source_versions": {
    "world_bank_wdi": "2024-q4",
    "eurostat_demo_fer": "2026-w20",
    "hfd": "2025-12"
  }
}
```

Properties:

- Filenames are content-hashed → `Cache-Control: public, max-age=31536000, immutable`. Repeat fetches are free.
- `manifest.json` itself is short-cached (e.g., `max-age=300`). Clients fetch the manifest on launch, compare versions against their local cache, fetch only what changed.
- Brotli compression at the CDN; SQLite typically compresses 70%+; FlatGeobuf compresses similarly under brotli (typed binary with run-length-friendly attribute encoding).
- Per-statistic SQLite files mean adding statistics in v2 doesn't bloat v1's payload.

### License-segmented SQLite shards (v2+)

Different upstream sources publish under different licenses. v1 ships only World Bank WDI (uniformly permissive), so v1 produces one SQLite per statistic. From v2 onward, sources with stricter terms (share-alike, no-commercial-redistribution, attribution-only) will land in the canonical store, and a single SQLite is no longer enough: an embedded third-party consumer (e.g. a NYT data dashboard) needs a redistributable subset, while a direct user on eafora.org sees the full license-aggregated set.

The chosen shape is **additive shards via SQLite `ATTACH DATABASE`**, not mutually exclusive variants:

- A base shard contains every datum whose source license permits the most-restrictive distribution context (commercial embedding by third parties).
- One or more extension shards contain the additional data permitted in successively more-permissive contexts (e.g. non-commercial-only, first-party-Eafora-display-only).
- The manifest declares which shards exist and the license tier each represents.
- Clients identify their distribution context (eafora.org loads all tiers it's authorized for; an embedded third-party widget loads only the base) and `ATTACH` each authorized shard. Queries union across attached databases as a SQLite primitive.

Why additive over mutually exclusive: (a) the licensing matrix at `docs/research/data-source-licensing.md` already has more than two distinct restriction shapes, so mutually-exclusive variants would combinatorialize; (b) the build pipeline naturally emits all shards from a single ingestion pass, keyed on per-row license metadata already present in the `source` table; the permissive subset is a free by-product of producing the full set, not a separate build; (c) the most-restrictive base is cached once on the CDN and benefits every consumer regardless of tier.

This is a working direction for v2+, not a v1 commitment. The first source with stricter-than-WB terms landing in the canonical store is what triggers shard production; until then the architecture supports it without depending on it.

### Client cache strategy

- **Web**: IndexedDB. Mobile browsers allow 50+ MB per origin without prompts in 2026. First-launch download → IndexedDB → in-memory (Rust-side). Subsequent launches read IndexedDB without network unless `manifest.json` says a newer version exists.
- **iOS / Android**: file-system cache in app sandbox. Same logic; `URLSession`/`OkHttp` already handle the HTTP cache headers.
- **Embedded fallback**: a small "good enough for first paint" SQLite + smaller FlatGeobuf is bundled in each build. App opens instantly with stale-but-real data while the latest is fetched in the background.

## Map rendering details

### Projection

**Miller cylindrical**, the sole projection. Closed-form, ~5 lines:

```rust
fn miller(longitude: f64, latitude: f64) -> (f64, f64) {
    let x: f64 = longitude;
    let y: f64 = 1.25 * ((std::f64::consts::FRAC_PI_4 + 0.4 * latitude).tan()).ln();
    (x, y)
}
```

Aspect ratio ~5:3. Polar inflation is moderate (much less than Mercator); at low latitudes Miller is conformal-like.

**Horizontal wraparound** is part of the renderer, not a separate feature: when the viewport's longitude range crosses the ±180° antimeridian, polygons that span the seam are rendered twice — once at their natural longitude and once shifted by ±360° — so a user panning east past Japan continues seamlessly into the western hemisphere. No vertical wraparound; the poles are real and stop the pan.

This decision is closed. No alternate projections in v2+, no globe-view toggle, no Equirectangular/Mercator/Robinson/Laskowski/Winkel-Tripel options.

### Hover scaling

The user-stated requirement is: the visual scale of a country grows on hover, but the **hit-test region must not grow** — this is the anti-pattern to avoid (it impedes the cursor's ability to hit a neighbor while the hovered country is enlarged).

Implementation: the renderer maintains two transforms per country — a `scale_visual` (drives drawing) and a `scale_hit` (always 1.0, drives hit-testing). The hover effect animates `scale_visual` with an easing curve; `scale_hit` is read by the hit-test path and never touched. The two paths share a single source-of-truth polygon; only the transform differs.

### Zoom-to-country

When the user clicks a country, the camera animates from current viewport to a viewport framing the country's bounding box (with margin). The Rust core exposes a `Camera` state machine; the platform shell polls it once per frame and asks for a fresh render.

### Borders

`core::boundary` abstracts the boundary data set behind a trait. v1 ships a single set (Natural Earth 1:50m, US-recognized lines per the constitution). The data layer is set up so an alternate boundary set (e.g., one that matches India's recognized lines) can be swapped in by changing one config value, without changes to the rendering code. We do not actually ship multiple boundary sets in v1 — the flexibility is for a future v3+ if a real distribution case requires it.

## Local development

The local-dev story mirrors Singularity:

- `setup.sh` checks prerequisites (cargo, podman, dbmate, secr, websocat where applicable), decrypts secrets, generates `.env` and `compose.yaml` from templates, runs `cargo sqlx prepare --workspace`.
- `compose.yaml` brings up Postgres in Podman.
- `dbmate.sh` wraps dbmate and re-runs `cargo sqlx prepare --workspace`.
- `cargo leptos watch` runs the web app on localhost.
- iOS dev: open `ios/Eafora.xcodeproj` in Xcode; the xcframework is rebuilt on the host by a Run Script build phase that invokes `cargo build` and `xcodebuild -create-xcframework`. iterations are slower than web (Xcode build + run on simulator is ~30–90 s after first build).
- Android dev: open `android/` in Android Studio; the AAR is rebuilt by a Gradle task that wraps `cargo-ndk`. Iterations are similar to iOS.

## CI/CD

**v1 builds run on the owner's Mac mini M1**, the same machine that hosts the ingestion service and Postgres. This pairs naturally: Apple Silicon natively builds iOS xcframeworks (no hosted macOS runner needed — the most expensive part of cloud CI is gone), and macOS also runs Cargo, cargo-leptos, and cargo-ndk for the Linux/Android targets without trouble.

The orchestration tool layered on top is provider-agnostic — a self-hosted GitHub Actions runner pointed at the Mac mini, a Buildkite agent, a Jenkins job, or just shell scripts on a launchd timer all work equivalently. The build *machine* is locked to the Mac mini through v1; the workflow tool is a follow-up choice that doesn't change the architecture.

A typical layout is a single workflow file with conditional jobs based on changed paths:

| Job | Runner | Triggers on | Caching |
|---|---|---|---|
| `core` (build + test) | `ubuntu-latest` | any push | `Swatinem/rust-cache` for `~/.cargo/registry`, `target/` |
| `web` (cargo-leptos build) | `ubuntu-latest` | changes in `core/`, `web/` | rust-cache + `wasm-pack` install cached |
| `ingestion` (cargo build + test) | `ubuntu-latest` | changes in `core/`, `ingestion/` | rust-cache |
| `ios` (xcframework + Xcode build) | `macos-latest` (Apple Silicon runner) | changes in `core/`, `ios/`; manual dispatch for TestFlight upload | rust-cache + DerivedData cache |
| `android` (cargo-ndk + Gradle assemble) | `ubuntu-latest` | changes in `core/`, `android/` | rust-cache + Gradle cache + Android NDK cache |

Realistic build times on GitHub-hosted runners (approximate; verify against actual project): Rust workspace clean ~15–25 min, with cache ~3–5 min; cargo-leptos ~10–15 min; iOS xcframework ~25–40 min clean / ~10–15 min cached; Android ~20–30 min clean / ~5–10 min cached. **Total clean CI time across all jobs is ~60–90 min**, which fits comfortably in the GitHub Actions free-tier monthly minutes for a private repo at our build cadence.

iOS signing: App Store Connect API key (.p8) stored in repo secrets, decoded in the workflow, passed to `xcodebuild -allowProvisioningUpdates`. Android signing: keystore base64-encoded in repo secrets, decoded in the workflow, passed to `gradlew assembleRelease`.

## App store distribution

### Apple Developer Program

- **Cost**: $99/year (verified standard pricing).
- **Enrollment**: individual or organization; identity verification typically 1–7 days; first submission viable ~2–5 business days after approval.
- **App Store Connect API key** for CI: generated under Users and Access → Keys, downloaded once (cannot be re-downloaded), stored in the chosen CI service's secret store (e.g. GitHub Actions secrets) as `APPSTORE_CONNECT_API_KEY_CONTENT`, `_KEY_ID`, `_ISSUER_ID`.
- **TestFlight**: internal testing (up to 100 testers, no review); external testing requires beta review (~24–48 hours).
- **App Store review**: ~24–48 hours typical in 2026 for compliant apps. Common rejection reasons for a map / data viz app: misleading data, claims of endorsement without evidence, mishandling of politically contested borders. Eafora's neutrality principle (no editorial copy) and US-recognized-borders default reduce both risks; the contested-borders abstraction in `core::boundary` lets us swap if a market demands it.
- **Apple-employee considerations**: per publicly documented policy, Apple employees publish apps via personal Developer accounts; Apple's External Technology Participation policy requires disclosure when an external project competes with Apple products, uses confidential information, or markets Apple trademarks. Eafora plausibly does none of these, but the owner **must verify with Apple internal policy before submitting** (we don't speculate beyond public documentation).

### Google Play

- **Cost**: $25 USD one-time registration fee.
- **Enrollment**: individual or organization; identity verification typically 1–7 days.
- **Google Play App Signing** (recommended over self-managed): Google holds the signing key; if local signing material is lost, Google can re-sign — this matters for a solo developer with no formal key-management process.
- **Internal testing track**: no review, instant updates to listed testers.
- **Play Store review**: typically 24–48 hours; less restrictive than App Store on contested-content concerns.

### Domain and email

- **Domain**: `eafora.org`, registered through **Cloudflare** (pairs naturally with the Cloudflare R2 + Pages choice in §Artifact distribution; one vendor relationship for DNS, registrar, CDN, and static hosting). `.org` is traditionally for nonprofit / educational / research-shaped projects, which fits the stated mission. Pricing ~$8–10/year through Cloudflare's at-cost registrar pricing.
- **Email**: registrar-provided forwarding (`hello@eafora.org` → personal inbox), free; outbound via Sendgrid/Postmark free tier when app-to-user emails are needed (probably not in v1–v2).

## Cost estimate

Concrete numbers carry the same approximation caveats as the source agent research; the orders of magnitude are reliable, the digits are not.

| Category | v1 (alpha, <100 users) | v1.5 (~1k DAU) | v2 (~10k DAU) |
|---|---|---|---|
| CDN (Cloudflare R2 + Pages) | ~$0–2/mo | ~$1–5/mo | ~$5–20/mo |
| Postgres (on the Mac mini through v1; managed-cloud Postgres post-migration) | **$0** | $0–15/mo | $15–50/mo |
| Ingestion compute (Mac mini through v1; managed runner post-migration) | **$0** | $0 | $0–50/mo |
| CI/CD (Mac mini through v1; managed runner post-migration) | **$0** | $0–20/mo | $20–100/mo |
| Domain (`eafora.org`, amortized) | ~$1/mo | ~$1/mo | ~$1/mo |
| Email (registrar forwarding + free SMTP) | $0 | $0–5/mo | $0–10/mo |
| Monitoring / analytics (Plausible, UptimeRobot) | $0 | $0–10/mo | $10–50/mo |
| **Recurring monthly total** | **~$2/mo** | **~$5–25/mo** | **~$40–250/mo** |
| Apple Developer Program | $99/year | $99/year | $99/year |
| Google Play registration | $25 one-time | already paid | already paid |

Headline: **v1 lives within $50/year of recurring infra cost** plus the one-time $25 Google fee and the $99/year Apple fee. v2 worst-case is a few hundred dollars per month, well under the user-stated tolerance. None of this requires a funding event.

## Open questions

1. **Postgres deployment beyond v1.** v1 runs on the Mac mini. When the Mac mini stops being enough (HA needs, geographic distribution, or a v3+ live API), the migration target is a serious managed-cloud Postgres — AWS RDS / Aurora is the canonical option; a Cloudflare-fronted Postgres (e.g. via Hyperdrive in front of a managed instance) is the alternative if and when the rest of the stack is fully on Cloudflare. **Neon is ruled out per owner direction.** Deferred until traffic, reliability needs, or a v3+ API actually forces it.
2. **Live-API readiness for v3.** The `ingestion/` actix-web binary is provisioned for a future live API but not wired. The most likely v3 use case is user-submitted corrections (Wikipedia-style) — auth, moderation, schemas, and rate-limiting all need their own spec when the time comes. **Semantic / NL-Q&A-style features are explicitly not on the roadmap.**

## Things to verify

These are claims in this document where I'm working from research-agent output without live confirmation. The user should verify any technical claim before relying on it for implementation:

1. **Apple ETP applicability for an Apple employee shipping Eafora** — public policy is summarized; the owner must verify with internal Apple policy before submission.
2. **wgpu Metal feature target** — confirm against the current wgpu repo if targeting devices below Apple A10.
3. **WebGPU readiness in Safari and Firefox in mid-2026** — Chromium is stable, Safari 18.4+ is stable as of May 2026, Firefox in progress per agent research; concrete-verify via real browser tests before relying on WebGPU as default.
4. **UniFFI 0.27+ async cancellation status** — agent research said "no cancellation tokens as of early 2026"; reverify before designing on the assumption.
5. **MTKView delegate threading guarantee** — long-standing but worth a spot-check against current iOS SDK docs.
6. **GitHub repo settings (verified 2026-05-23)** — "Automatically delete head branches" is **enabled**. "Rebase and merge" via the GitHub UI does **not** preserve empty commits, so the `>>> branch: <name>` markers do **not** land in master through the standard merge button. **Resolution**: PRs MUST be merged manually via local `git rebase` + `git push origin master` rather than via the GitHub UI button, so the marker survives into master. The constitution's Governance §Git workflow §Merge strategy clause will be amended in a follow-up branch to codify this and a `scripts/pr-merge.sh` helper will be added.

## Follow-up work

Subsequent branches that depend on this overview:

- `docs-architecture-client-web` — full Leptos + WASM + cargo-leptos plan, CSS approach, component layout, build optimizations, IndexedDB cache lifecycle.
- `docs-architecture-client-ios` — full SwiftUI + UniFFI + xcframework plan, MTKView details, App Store submission walkthrough.
- `docs-architecture-client-android` — full Compose + UniFFI + cargo-ndk plan, SurfaceView lifecycle, Play Store submission walkthrough.
- `docs-architecture-ingestion` — full Postgres schema, per-source adapters, artifact builder, scheduling, license tracking.
- `docs-product-plan` — vision, audience, monetization options, scope phases.
- `docs-claude-md-rewrite` — fold the locked decisions into `CLAUDE.md` and remove the stale next-steps section.
- **Constitution amendment (v1.3.0): swap `PMTiles` for `FlatGeobuf` in Principle VI.** The architecture overview body now uses FlatGeobuf for the geometry artifact (driven by the no-LOD / full-polygon decision); the constitution still names PMTiles, so line 29 of this doc faithfully quotes the stale wording. Small standalone amendment branch closes the gap.

The first feature spec via `/speckit-specify` should be the **World Bank WDI ingestion CLI** (per the constitution's licensing-resolved status of WB WDI as v1's primary global source). Smallest viable end-to-end exercise of the canonical store + artifact builder + manifest pipeline.
