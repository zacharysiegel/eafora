# Implementation Plan: iOS client (UniFFI boundary + SwiftUI shell + Metal surface)

**Branch**: `004-ios-client` | **Date**: 2026-08-20 | **Spec**: [`spec.md`](./spec.md)

**Input**: Feature specification from `specs/004-ios-client/spec.md`

## Summary

Deliver the iOS surface of Eafora: a SwiftUI app that renders the world choropleth through the same `shared` renderer the web client uses, painting from an in-bundle embedded artifact set at first launch and swapping to the live CDN bundle in the background.

The spec was drafted before the web client existed and describes a thick Swift layer: a Swift cache, a Swift fetcher, and a Swift reimplementation of the load orchestration. Reading the shipped web client changes that shape. The loader is already platform-agnostic Rust that happens to live under `web/`, the surface-attach path for UIKit already exists in `shared`, and `reqwest` is already a workspace dependency. So this plan moves logic into `shared` rather than rewriting it in Swift, and the Swift layer shrinks to what only Swift can do: the SwiftUI tree, the `MTKView` bridge, and gesture recognition.

That inversion is the plan's main content. It replaces the shape of FR-019 through FR-030 and it is what keeps the per-platform layer inside the "1 to 2k LOC of glue" target in `client.md` §Cross-platform consistency.

## Technical Context

**Language/Version**: Rust (edition 2024) for everything below the FFI; Swift 5 with SwiftUI for the shell. Xcode 26.5 (build 17F42) is installed, past the iOS 18 SDK the architecture doc assumed.

**Primary Dependencies**: `shared` with the `render` feature, whose native `wgpu` already enables the `metal` backend (`shared/Cargo.toml:35`). `reqwest 0.12.*` with `rustls-tls` is already a workspace dependency (`Cargo.toml:12`), used by ingestion, so the iOS fetch path needs no new third-party crate. One genuinely new dependency is required and needs owner approval: `uniffi` for the FFI boundary, with a matching `uniffi-bindgen-swift` binary.

**Platform-provided**: `MetalKit`, `Metal`, `UIKit`, `SwiftUI`, `Foundation`. New build tooling: `xcodegen` (absent) and `yq` (present, v4.52.5).

**Target platform**: iOS 18+, iPhone first. The simulator is the development target through Phase B; a device needs the enrollment that Phase D waits on.

**Toolchain gaps to close in `setup.sh`**: no iOS simulator runtime is installed (`xcrun simctl list runtimes` is empty), the `aarch64-apple-ios` and `aarch64-apple-ios-sim` Rust targets are absent, and `xcodegen` is not installed. There are zero code-signing identities, confirming no Developer Program enrollment.

**Testing**: Rust unit tests for everything below the FFI, which is where nearly all logic now sits. XCTest for the Swift surfaces that remain: the `MTKView` bridge, the embedded-bundle locator, and Universal Link routing. No simulator is needed to run the Rust tests, which is why Phase 0 is reviewable today.

## Constitution Check

The spec's own §Constitution Check holds, with one principle served more strongly than it planned:

- **Principle III (Rust core, native UI shells)**: better served than the spec describes. Moving the loader into `shared` rather than reimplementing it in Swift means the Swift layer holds no data logic at all.
- **Principle IV (Singularity convention parity)**: `uniffi` is the one new Rust dependency and needs explicit approval before Phase 0.1 begins. `reqwest` is already approved and in the workspace.
- **Principle V (Explicit over implicit)**: UniFFI is code generation, which the architecture doc already argues for as the only viable FFI path, with `ios/ffi/` as the single reviewable surface. The decision to fetch in Rust rather than Swift keeps the wire visible in one place instead of two.
- **Principle VII (Test-first for core logic)**: the logic being tested is Rust, so this applies with full force rather than being softened for UI code.

No violations identified; no amendments proposed.

## Project Structure

### Documentation (this feature)

```
specs/004-ios-client/
├── spec.md                     # corrected against the tree, 2026-08-20
├── plan.md                     # this file
├── tasks.md                    # phases 0 through B only
└── checklists/requirements.md
```

### Source code (repository root)

```
# NEW — the FFI boundary, its own crate rather than part of shared (Phase 0.1, see Topic 6)
ios/ffi/
├── Cargo.toml                  # crate-type = ["staticlib"]; depends on shared with the render feature
└── src/
    ├── lib.rs                  # the UniFFI surface: pub use of the items below
    ├── client.rs               # EaforaClient, the opaque handle Swift holds
    └── handle.rs               # WindowHandle marshaling, u64 pointers rebuilt into the shared enum

# NEW — a bindgen binary (Phase 0.1)
tools/uniffi_bindgen_swift/     # [[bin]] wrapper calling uniffi's Swift bindgen

# MOVED into shared, from web (Phase 0.2)
shared/src/artifact/load.rs     # was web/src/client/load.rs, 374 lines, parameterized
shared/src/artifact/discovery_resolve.rs  # was web/src/live_resolve.rs, 94 lines
shared/src/artifact/version_rank.rs       # was web/src/version_rank.rs, 94 lines

# NEW — platform implementations of the two traits the loader needs (Phase 0.2)
shared/src/artifact/filesystem_cache.rs   # std::fs ArtifactCache, used by iOS
shared/src/http/reqwest_fetch.rs          # reqwest HttpFetch, used by iOS

# NEW — the app (Phase A onward)
ios/
├── project.yml                 # XcodeGen config; committed
├── Eafora.xcodeproj/           # gitignored; regenerated
├── setup.sh                    # per FR-046
├── ExportOptions.plist         # Phase D
├── README.md
├── EaforaApp/
│   ├── EaforaApp.swift         # @main, sheet bindings, Universal Link routing
│   ├── EmbeddedBundle.swift    # locates Resources/embedded_artifacts
│   ├── DesignTokens.swift      # hand-maintained beside web/style/_tokens.scss
│   ├── Localizable.xcstrings
│   ├── Map/
│   │   ├── MapView.swift       # SwiftUI container
│   │   ├── MapMTKView.swift    # UIViewRepresentable over MTKView
│   │   └── MapCoordinator.swift # draw callback, gestures, redraw scheduling
│   ├── Region/
│   │   └── RegionDetailView.swift
│   └── Resources/embedded_artifacts/   # gitignored; synced by script
├── EaforaAppTests/
└── (EaforaAppUITests/ deferred)

# NEW — build scripts (Phase 0.1, A)
scripts/build/build-ios-xcframework.sh
scripts/build/inject-git-revision.sh
scripts/build/deploy-aasa.sh    # Phase D

# MODIFIED
shared/Cargo.toml               # crate-type, uniffi dependency, ios target table
setup.sh                        # simulator runtime, rust targets, xcodegen
docs/task-order.md              # phase status
```

## Phase 0: outline & research

Everything below was verified against `shared` and `web` on `master` at `e8a0867`, not against the architecture doc. Five parallel research agents were dispatched for this and all five died on an API budget limit, so these findings are hand-verified; the items in Topic 8 are the ones that consequently remain unchecked.

### Topic 1: the UIKit surface path already exists

The spec treats the Metal surface as new work. It is not. `WindowHandle::UiKit { layer_ptr: u64, view_ptr: u64 }` is defined at `shared/src/render/window_handle.rs:4`, its doc comment already anticipating a native shell. `WgpuSurface::from_window_handle` handles that variant at `shared/src/render/surface.rs:96`, converting it to a `RawWindowHandle::UiKit`, and `Renderer::attach_surface_from_window_handle(window_handle, width, height)` sits at `shared/src/map/renderer.rs:188`.

So the iOS attach path is a call, not an implementation. What the FFI adds is marshaling: Swift passes two `u64` pointers, Rust rebuilds the enum. FR-009's "define `WindowHandle` as a UniFFI enum" becomes "expose the existing enum across the FFI".

### Topic 2: Metal needs no backend work

`shared/Cargo.toml:35` already enables `wgpu`'s `metal` feature for non-wasm targets, and `create_instance(RendererBackend::Default)` returns `Instance::default()` (`shared/src/map/renderer.rs:468`), which includes Metal on Apple platforms. `RendererBackend` needs no iOS variant; `ForceGl` exists only for the web's WebGL2 parity path.

### Topic 3: 562 lines of loader logic are platform-agnostic but live under `web/`

This is the finding that reshapes the feature. `web/src/client/load.rs` is 374 lines of load orchestration — embedded load, cached-version selection, discovery reconciliation, the speculative parallel fetch, hash verification, eviction, hot-swap publication — and it imports nothing browser-specific. No `web_sys`, no `js_sys`, no `wasm_bindgen`. Its only crate-local dependencies are the concrete `OpfsArtifactCache` type and the `fetch` free functions.

`web/src/live_resolve.rs` (94 lines, discovery reconciliation and the static fallback) and `web/src/version_rank.rs` (94 lines, the cached-version sort key) are likewise pure logic with unit tests already attached.

Against that, the genuinely browser-specific code is `cache.rs` (205), `opfs.rs` (170), `fetch.rs` (79), and `js.rs` (57).

If iOS follows the spec and reimplements the loader in Swift, that is 562 lines of behavior maintained twice, in two languages, with the version-ranking and discovery-reconciliation rules — the subtlest logic in the client, and the source of a bug already fixed once on the web side — duplicated. The correction is to move those three files into `shared`, parameterized over the `ArtifactCache` trait that already exists and over a new `HttpFetch` trait, and to have the web client consume them. The web client's existing tests are what prove the move was faithful.

### Topic 4: the fetch side is not abstracted, and Rust should own it on iOS

`shared/src/http.rs` defines `HttpRequest`, `HttpMethod`, `HttpCacheMode`, and `Response`, but no trait: `web/src/client/fetch.rs` provides free functions over `web_sys`. Moving the loader therefore requires a seam, and the shape of it decides how much Swift gets written.

The spec assumes a Swift `URLSessionFetcher` reaching back across the FFI (FR-025). That is possible — UniFFI supports foreign trait implementations — but it means an async callback across the boundary for every artifact fetch, and it puts retry and error-mapping rules in Swift where the web's equivalent is in Rust.

The alternative is a `reqwest` implementation of the same trait, compiled into the iOS binary. `reqwest 0.12.*` with `rustls-tls` is already a workspace dependency, so this adds nothing new to approve, and `URLSession` is not required for correctness: the client fetches immutable public artifacts over HTTPS with no cookies, no auth, and no system-integration requirements.

**Decision: implement `HttpFetch` in Rust for non-wasm targets with `reqwest`.** FR-025 changes from "implement `URLSessionFetcher.swift`" to "implement `reqwest_fetch.rs` in `shared`". The cost is that iOS networking no longer honours system URL-loading behaviours a Swift app would inherit, which for immutable static assets is not a loss worth 562 lines of duplication. If ATS or proxy behaviour later demands `URLSession`, the trait is the seam to swap it at, and the loader above it does not change.

### Topic 5: the cache should be Rust too, for the same reason

`ArtifactCache` (`shared/src/artifact/cache.rs:9`) is four async functions: `put`, `get`, `list_versions`, `delete_version`. The web implements it over OPFS because a browser has no filesystem. iOS does have one, so a `std::fs` implementation in `shared` satisfies the trait with no FFI callback at all, and Swift's only involvement is passing the cache directory path in at construction.

This replaces FR-019 through FR-023. Two spec requirements survive as Swift-side concerns because they are platform policy rather than logic: the directory choice (FR-020, `Library/Caches/artifacts/`, chosen so iOS may evict it under pressure) and excluding it from backup (FR-021, `NSURLIsExcludedFromBackupKey`). Both are set by Swift on the directory before handing the path to Rust.

The P2 scenario — iOS purging the cache mid-session — becomes a Rust test rather than an XCTest, because a `std::fs` cache can have its directory removed underneath it in a unit test far more easily than a simulator can be made to evict one.

### Topic 6: `shared` needs a static library, and that has consequences

`shared/Cargo.toml` declares `[lib]` with no `crate-type`, so it builds as an rlib only. An xcframework needs a static library for each iOS slice.

Adding `crate-type = ["staticlib", "rlib"]` to `shared` would make every host build also produce a static library, slowing the ingestion and web builds for no benefit. The alternative is a thin `ios/ffi` crate that depends on `shared` and carries the `staticlib` type plus the UniFFI scaffolding, leaving `shared` untouched.

**Decision: a separate crate.** It keeps the `staticlib` cost on the one target that wants it, gives the UniFFI attributes a home that is not the middle of the domain code, and means `shared` stays a library that the ingestion producer links without dragging FFI scaffolding along. This supersedes FR-008, which places the surface at `shared/src/ffi/uniffi.rs`: the module moves to the new crate and `shared` gains nothing.

### Topic 7: the revision surface already exists

FR-011 asks for `eafora.revision()`. `shared/src/revision.rs` already exposes `REVISION`, populated by `shared/build.rs`, which reads `git rev-parse HEAD` and panics on a shipping build if it cannot. Only the FFI accessor and the Swift-side `Info.plist` injection (FR-042) remain.

### Topic 8: what remains unverified

Each of these is a hazard the plan cannot close from this machine, listed with what would close it:

- **The UniFFI version and its Swift bindgen invocation.** `uniffi` is not in the local registry cache, so neither the current version nor the `uniffi-bindgen-swift` argument shape could be checked. The spec already flags the per-artifact invocation pattern as possibly shifted. Closing it: add the dependency once approved, then read the installed crate's own documentation rather than trusting the architecture doc.
- **Whether UniFFI's async support covers the loader's shape.** The loader is `async` and holds a `tokio` `Semaphore` across awaits. Either the FFI exposes blocking calls over an owned `tokio` runtime, or it uses UniFFI's async support. This is the largest open design question in Phase 0.1 and should be settled by reading the installed crate before writing the surface.
- **The XcodeGen schema.** `xcodegen` is not installed, so `project.yml` cannot be validated. Closing it: install it in Phase A and run `xcodegen generate`.
- **`MTKView.isPaused` plus `setNeedsDisplay` semantics.** No simulator runtime is installed, so the event-driven loop is unverified. Closing it: install a runtime in Phase A.
- **Everything in Phase D**, which needs an enrollment that does not exist.

## Phase 1: design & contracts

The FFI surface is one opaque handle and a small set of calls on it. Swift holds the handle; Rust holds all state, including the renderer, the bundle watch channel, the cache, and the runtime.

```
EaforaClient
    new(cache_directory: String, distribution: DistributionContext) -> EaforaClient
    attach_surface(handle: WindowHandle, width: u32, height: u32)
    resize_surface(width: u32, height: u32)
    detach_surface()
    draw_frame()
    load_embedded_bundle(root_directory: String)
    start_live_load(discovery_url: String)
    region_at_point(x: f64, y: f64) -> Option<RegionHit>
    set_period(period_start: NaiveDate-as-String)
    set_statistic(statistic: StatisticKind)
    pan(dx: f64, dy: f64) / zoom(factor: f64, at_x: f64, at_y: f64)
    revision() -> String
```

Three properties of this shape matter. The renderer never crosses the boundary, so no wgpu type needs a UniFFI representation. `draw_frame` takes no arguments because the viewport and frame state live in Rust, which is also what lets a gesture be a single call rather than a state exchange. And every fallible call returns `Result<_, AppError>`, which UniFFI maps to a Swift `throws` per the project's recorded preference for UniFFI's default error mapping.

The Swift side then holds: `EaforaApp.swift` (lifecycle, sheets, link routing), `MapMTKView.swift` (the `UIViewRepresentable`), `MapCoordinator.swift` (the `draw(in:)` callback, gesture recognizers, and the `setNeedsDisplay` scheduling that mirrors the web driver's dirty-flag-plus-rAF pattern), `EmbeddedBundle.swift` (locating the bundled artifact root), and the two view files. Nothing else.

## Phasing for PRs

Phase 0.1 and 0.2 are independent of each other and both are off `master`; the rest is a linear stack. Only 0.1 needs a dependency approval, and only 0.1 through B are planned in detail.

- **Phase 0.1 — the FFI boundary** (own PR, off `master`). The `ios/ffi` crate, the `uniffi-bindgen-swift` binary, `scripts/build/build-ios-xcframework.sh`, and the `setup.sh` additions for the iOS Rust targets. FR-003, 004, 005, 006, 007, 008, 009, 010, 011. Pure Rust and shell: it builds and reviews with no Xcode project and no simulator. **Blocked on approving the `uniffi` dependency.**
- **Phase 0.2 — move the loader into `shared`** (own PR, off `master`). `load.rs`, `live_resolve.rs`, and `version_rank.rs` move into `shared/src/artifact/`, parameterized over `ArtifactCache` and a new `HttpFetch` trait; `shared` gains the `std::fs` cache and the `reqwest` fetch for non-wasm; `web/` is refactored to consume the moved code, with its existing tests as the proof the move was faithful. No FR of its own: it is the prerequisite that stops FR-019 through FR-030 being written twice.
- **Phase A — the app renders** (stacks on 0.1). `ios/` scaffolding, `project.yml`, the app skeleton, `EmbeddedBundle.swift`, the `MTKView` bridge, and first paint on the simulator. FR-001, 002, 012, 013, 014, 015, 016, 017, 035, 036, 037, 038, 039, 040, 041, 042, 043, 046, 056, 057. Closes P1. Needs a simulator runtime installed first.
- **Phase B — data over time** (stacks on A and 0.2). Wiring the moved loader to the app: cache directory choice and backup exclusion in Swift, discovery, the speculative fetch, and hot-swap. FR-019, 020, 021, 022, 023, 025, 026, 027, 028, 029, 030, 054, 055. Closes P2 and P3, with the cache-purge scenario as a Rust test per Topic 5.
- **Phase C — the rest of the surface** (stacks on B). Region detail sheet, settings, About, `DesignTokens.swift` refinement, gesture parity with the web. FR-018, 031, 032, 033. **Sketch only: no task breakdown exists, and writing one is the first step of picking it up.** It depends on nothing outside the repository.
- **Phase D — distribution** (stacks on C). Universal Links, the AASA worker, code signing, and the TestFlight pipeline. FR-044, 045, 047, 048, 049, 050, 051, 053, 058. **Sketch only, and unplannable in detail until the Developer Program enrollment exists**, because the signing identity, the AASA `appID`, and the App Store Connect key are inputs to those steps rather than outputs of them.

Four requirements carry no phase because they are prohibitions rather than deliverables: FR-024 (no quota machinery, since iOS exposes no per-app quota), FR-034 (no splash screen), FR-052 (no `.xcarchive` retention in CI), and FR-059 (no XCUITest). Each constrains the phase its subject belongs to — B, A, D, and the testing approach throughout — and is satisfied by not writing something.

A phase marked sketch-only is an unplanned phase, not a lighter one.

## Brief PR description

**eafora**: Corrects the iOS spec against the tree it will be built in and plans the feature through Phase B.

The spec predated the web client and assumed a `core/` crate, flat script paths, and an FFI boundary that would already exist. It now names `shared/`, the real script locations, and the toolchain actually installed: Xcode 26.5 but no simulator runtime, no iOS Rust targets, no `xcodegen`, and no Developer Program enrollment.

The plan's substantive change is to invert where the work happens. The web client's load orchestration, discovery reconciliation, and version ranking are 562 lines of platform-agnostic Rust that merely live under `web/`; the UIKit surface path and the Metal backend already exist in `shared`. So the iOS client moves that logic into `shared` and implements the cache and fetch traits in Rust rather than reimplementing them in Swift, which reshapes FR-019 through FR-030 and leaves the Swift layer holding only the SwiftUI tree, the `MTKView` bridge, and gestures.

## Post-implementation notes

To be appended per phase, recording deviations from this plan.
