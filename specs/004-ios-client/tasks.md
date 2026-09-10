# Tasks: iOS client

**Plan**: [`plan.md`](./plan.md) | **Spec**: [`spec.md`](./spec.md)

Covers phases 0.1, 0.2, A, and B. Phases C and D are sketches in the plan with no task breakdown; writing one is the first step of picking either up.

Ordering within a phase is top to bottom. Phase 0.1 and 0.2 are independent of each other; A stacks on 0.1, and B stacks on both A and 0.2.

## Phase 0.1 — the FFI boundary

Blocked until the `uniffi` dependency is approved. Everything here is Rust and shell: no Xcode project, no simulator.

1. Read the installed `uniffi` crate's own documentation and settle two questions before writing anything: the `uniffi-bindgen-swift` invocation shape, and whether the boundary exposes blocking calls over an owned `tokio` runtime or uses UniFFI's async support. Record the answer in the plan's Topic 8 as resolved. The loader holds a `Semaphore` across awaits, so this decides the shape of every call below.
2. Add `uniffi` to `[workspace.dependencies]` at the pinned minor, wildcard patch, per the version convention.
3. Create `ios/ffi/Cargo.toml`: `crate-type = ["staticlib"]`, depending on `shared` with the `render` feature. Add it to the workspace members.
4. Create `tools/uniffi_bindgen_swift/` as a `[[bin]]` whose `main` calls uniffi's Swift bindgen entry point.
5. Write `ios/ffi/src/handle.rs`: convert two `u64` pointers into the existing `shared::render::WindowHandle::UiKit`. No new enum; the FFI type is a marshaling shim over `window_handle.rs:4`.
6. Write `ios/ffi/src/client.rs` with the `EaforaClient` surface from the plan's §Phase 1. It owns the `Renderer`, the bundle `watch` channel, the cache, and the runtime. Nothing wgpu-shaped crosses the boundary.
7. Map `AppError` across as a single-variant error carrying its message, letting UniFFI's default mapping produce a Swift `throws`.
8. Expose `revision()` over the existing `shared::revision::REVISION`; the constant and its `build.rs` already exist, so this is an accessor.
9. Write `scripts/build/build-ios-xcframework.sh`: build both iOS slices, run the bindgen for Swift sources, headers, and modulemap, then combine with `xcodebuild -create-xcframework` into `target/uniffi/EaforaCore.xcframework`. Gitignore the output.
10. Extend `setup.sh` with `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`, `brew install xcodegen`, and the simulator-runtime install. Note `yq` is already present.
11. Unit-test the handle marshaling and the error mapping in Rust. The renderer calls cannot be tested without a surface, which is Phase A.
12. Verify the script produces an xcframework from a clean `target/`, and that `cargo test -p shared` and the web build both still pass, since the workspace gained a member.

## Phase 0.2 — move the loader into `shared`

Independent of 0.1. No FR of its own: it exists so Phase B does not write 562 lines of Swift that already exist as Rust.

1. Add an `HttpFetch` trait to `shared/src/http.rs` shaped around the existing `HttpRequest` and `Response` types, with one method returning `Result<Response, AppError>`. This is the seam `web` and iOS each implement.
2. Move `web/src/client/load.rs` to `shared/src/artifact/load.rs`, replacing its concrete `OpfsArtifactCache` parameter with `impl ArtifactCache` and its `crate::client::fetch` calls with `impl HttpFetch`. Prefer static dispatch per the convention.
3. Move `web/src/live_resolve.rs` to `shared/src/artifact/discovery_resolve.rs` and `web/src/version_rank.rs` to `shared/src/artifact/version_rank.rs`, carrying their existing tests with them.
4. Rewrite `web/src/client/fetch.rs` as an `HttpFetch` implementation rather than free functions, keeping its browser behaviour intact.
5. Refactor `web/` to consume the moved code. The existing web tests passing unchanged is the evidence the move was faithful; if any assertion has to change, say why in the PR.
6. Add `shared/src/artifact/filesystem_cache.rs`: an `ArtifactCache` over `std::fs`, gated to non-wasm targets, taking its root directory at construction.
7. Add `shared/src/http/reqwest_fetch.rs`: an `HttpFetch` over `reqwest`, gated to non-wasm targets. No new dependency; it is already in the workspace.
8. Test the filesystem cache against a temporary directory, including the case the plan's Topic 5 moves here from XCTest: the directory disappearing mid-session, which is what iOS eviction looks like from inside the process.
9. Test the reqwest fetch's error mapping, including a non-success status and an unreachable host.
10. Confirm the wasm build still compiles and the web client's behaviour is unchanged, then confirm `cargo test -p shared` covers the moved logic.

## Phase A — the app renders

Stacks on 0.1.

1. Write `ios/project.yml` for XcodeGen: the app target, the deployment target, the pre-build run-script phases in order (xcframework, embedded-bundle sync, revision injection), the xcframework link, and `Resources/embedded_artifacts` in Copy Bundle Resources. Run `xcodegen generate` and confirm the project opens.
2. Gitignore `ios/Eafora.xcodeproj/` and `ios/EaforaApp/Resources/embedded_artifacts/`.
3. Write `ios/setup.sh` per FR-046, and `ios/README.md` with the iOS quickstart.
4. Write `scripts/build/inject-git-revision.sh`, writing the revision into `Info.plist`, and surface it in the app per FR-043.
5. Confirm `scripts/build/sync-embedded-bundle.sh` works unchanged against the iOS destination, which the spec claims and which is worth verifying rather than assuming.
6. Write `EaforaApp/EmbeddedBundle.swift`: locate the bundled artifact root and hand its path to Rust. The parsing stays in Rust; Swift supplies a path.
7. Write `EaforaApp/Map/MapMTKView.swift` as a `UIViewRepresentable` over `MTKView`, and `MapCoordinator.swift` holding the `draw(in:)` callback.
8. Attach the surface exactly once, when the `CAMetalLayer` first becomes available, passing the layer and view pointers through the FFI handle. Guard against the repeated-attach path the web client also had to guard.
9. Implement the event-driven loop: `isPaused = true` plus `setNeedsDisplay()`, scheduled by the same events the web driver schedules on. Read `web/src/map/canvas/driver.rs` for the list rather than inventing one.
10. Write `EaforaApp/DesignTokens.swift` from `web/style/_tokens.scss`, and `Localizable.xcstrings` with the strings the first screen needs.
11. Write `EaforaApp/EaforaApp.swift` and `Map/MapView.swift`: launch straight into the map, no splash, per FR-034.
12. XCTest the surface bridge (reported size matches the layer's drawable size) and the embedded-bundle locator.
13. Verify first paint on the simulator against `docs/design/stub-mobile.html` frame 00, and confirm an idle app issues no GPU work.

## Phase B — data over time

Stacks on A and 0.2.

1. Choose and create the cache directory in Swift: `Library/Caches/artifacts/`, so iOS may evict it under pressure, and set `NSURLIsExcludedFromBackupKey` on it at first creation. Pass the path to `EaforaClient`. Both are platform policy; everything below them is Rust.
2. Wire `EaforaClient::start_live_load` to the moved loader with the filesystem cache and the reqwest fetch. Discovery, the speculative parallel fetch, version ranking, hash verification, eviction, and the hot-swap publication all come from `shared` unchanged.
3. Confirm the app paints from a cached bundle on second launch, and that the newest complete version wins, which is the ranking rule the web side already tests.
4. Handle the eviction case end to end: the OS removes the cache directory mid-session, and the client continues from what it holds rather than failing. The Rust test from 0.2 covers the logic; this step confirms the app's behaviour.
5. Confirm the live swap repaints without a relaunch, which is the `watch` channel the renderer already consumes.
6. XCTest whatever Swift remains: the directory choice, the backup attribute, and the path handoff. There should be little else.

## Out of scope here

Phase C (region detail, settings, About, gesture parity) and Phase D (Universal Links, AASA, signing, TestFlight) have no tasks. Phase D additionally cannot get them until the Developer Program enrollment exists, since the signing identity, the AASA `appID`, and the App Store Connect key are inputs to those steps.

## Deviations from the plan, Phase 0.1

- The renderer is held in a `thread_local!` in `ios/ffi/src/client.rs`, not inside `EaforaClient` as the plan's §Phase 1 described. UniFFI asserts `Send + Sync` on every exported object and hands each function `&self` through an `Arc`, while wgpu state is bound to the thread that created it. Storing it per thread satisfies both without an `unsafe impl` and without changing `shared`, and mirrors how `web/src/map/canvas/driver.rs` already holds its `Driver`. The renderer functions are therefore synchronous exports, since an `async` export may resume on a worker thread; they drive the renderer's own async setup with a current-thread runtime the client owns.
- The surface omits `draw_frame`, `region_at_point`, `set_period`, `set_statistic`, `pan`, and `zoom`. All six need the viewport and frame-state orchestration that lives in `web/src/map/canvas/driver.rs` (`home_viewport`, `initial_frame_state`, and the clamping around them) and is not in `shared`. They land in Phase A with that port, which is also the first point at which any of them can be exercised. What ships here is the boundary, the renderer lifecycle, the two load functions, and `revision()`.
- `AppError` crosses as `FfiError`, a single-variant enum in `ios/ffi`, rather than as `AppError` itself. UniFFI's `Error` derive rejects structs, and `minimer::define_app_error!` expands to a newtype struct. FR-010's intent is unchanged: one variant carrying `message: String`, which Swift catches and matches on by prefix.
- `DistributionContext` crosses as `FfiDistributionContext`, a boundary enum with a `From` conversion, rather than through `#[uniffi::remote(Enum)]`. The remote macro would attach to the `shared` type directly, but it is `#[doc(hidden)]` in uniffi 0.32.1, so its stability is unclear; an explicit conversion costs one non-exhaustive match that fails to compile if a variant is added.
- Task 3 planned `ios/ffi` as depending on `shared` with the `render` feature, which it does. Note the consequence the plan did not: feature unification means a `cargo build --workspace` now enables `render` for `shared` everywhere, so `ingestion` links wgpu in that build. Every script builds it as `cargo build -p ingestion`, which stays clean, so the intent recorded in `shared/Cargo.toml` holds for the shipped binary but not for workspace-wide dev builds.
- No `build.rs` and no `uniffi` `build` feature: `setup_scaffolding!()` in `lib.rs` is sufficient, and `crate-type = ["staticlib"]` alone works because the bindgen reads the archive directly.
- FR-006 is only partly satisfied. `setup.sh` adds the two Rust targets, installs `xcodegen`, and downloads a simulator runtime when none is present. It does not run `xcodegen generate`, which needs an `ios/project.yml` that Phase A writes, and it checks for Xcode rather than running `xcode-select --install`. `yq` was already present.
- FR-007 is deferred entirely. `ios/setup.sh` renders the AASA template by reading `TEAM_ID` and `BUNDLE_ID` from `ios/project.yml`, so it cannot exist before Phase A writes that file, and the AASA file itself is Phase D.
- `scripts/build/build-ios-xcframework.sh` generates the Swift sources from the simulator archive rather than from both. The exported surface is identical across slices, so generating twice would only overwrite.

## Deviations from the plan, Phase 0.2

- Task 3 planned to move `web/src/live_resolve.rs` whole to `shared/src/artifact/discovery_resolve.rs`. Only the reconciliation was portable, so `AuthoritativeBase` and `authoritative_repository_base` joined the existing `shared/src/artifact/discovery.rs` beside the document they reason about, and no new module was created. `web/src/live_resolve.rs` remains, holding the `include_str!` of the committed discovery document and the compile-time repository base, both of which are the web build's own.
- The live fan-out was rewritten rather than moved. It used `wasm_bindgen_futures::spawn_local` with a oneshot channel per file, neither of which exists off the browser, and `tokio` is optional in `shared` behind the `render` feature, so `Semaphore` and `join!` were unavailable too. `futures_util` supplies both replacements: `buffer_unordered` bounds the concurrency the semaphore used to, and `future::join` races discovery against the speculative manifest. A hash mismatch now cancels the files still in flight instead of letting every fetch finish first.
- Task 4 also moved the URL construction. Only `fetch()` itself was browser-specific; `fetch_bytes` and the four URL builders above it were portable, and are now `shared/src/artifact/fetch.rs` over the `HttpFetch` trait. `web/src/client/fetch.rs` holds the one browser function.
- `evict_all_except` moved off `OpfsArtifactCache` into `evict_stale_versions`, unplanned. It was written entirely against the trait, so leaving it in the web client would have had iOS reimplement it.
- Tasks 6 and 7 landed in a second PR rather than with tasks 1 through 5, which kept the move reviewable against the web client's own tests before any new implementation joined it.
- Task 9's unreachable-host case is not tested. Every address answers with a response through the HTTP proxy this repository is developed behind, so a transport failure is not reachable from a test; the arm is covered through a URL that never becomes a request.
- `shared/src/http.rs` became `shared/src/http/`, splitting the vocabulary into `http_model.rs` so `reqwest_fetch.rs` could sit beside it under a declaration-only `mod.rs`.
- The filesystem cache rejects `.` and `..` path segments, which the plan did not call for. The browser's file system forbids them structurally and a directory tree does not, so the check restores the parity the trait implies.
- The filesystem cache reads and writes through `std::fs` inside its async functions. `shared` carries no async runtime of its own, and the trait is async for the browser's sake; the loader runs off the main thread on every platform that uses this implementation.
