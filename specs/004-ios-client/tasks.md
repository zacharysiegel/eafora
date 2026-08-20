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

Stacks on 0.1. Needs a simulator runtime installed.

1. Install a simulator runtime and confirm `xcrun simctl list runtimes` reports it. Nothing else in this phase can be verified until it does.
2. Write `ios/project.yml` for XcodeGen: the app target, the deployment target, the pre-build run-script phases in order (xcframework, embedded-bundle sync, revision injection), the xcframework link, and `Resources/embedded_artifacts` in Copy Bundle Resources. Run `xcodegen generate` and confirm the project opens.
3. Gitignore `ios/Eafora.xcodeproj/` and `ios/EaforaApp/Resources/embedded_artifacts/`.
4. Write `ios/setup.sh` per FR-046, and `ios/README.md` with the iOS quickstart.
5. Write `scripts/build/inject-git-revision.sh`, writing the revision into `Info.plist`, and surface it in the app per FR-043.
6. Confirm `scripts/build/sync-embedded-bundle.sh` works unchanged against the iOS destination, which the spec claims and which is worth verifying rather than assuming.
7. Write `EaforaApp/EmbeddedBundle.swift`: locate the bundled artifact root and hand its path to Rust. The parsing stays in Rust; Swift supplies a path.
8. Write `EaforaApp/Map/MapMTKView.swift` as a `UIViewRepresentable` over `MTKView`, and `MapCoordinator.swift` holding the `draw(in:)` callback.
9. Attach the surface exactly once, when the `CAMetalLayer` first becomes available, passing the layer and view pointers through the FFI handle. Guard against the repeated-attach path the web client also had to guard.
10. Implement the event-driven loop: `isPaused = true` plus `setNeedsDisplay()`, scheduled by the same events the web driver schedules on. Read `web/src/map/canvas/driver.rs` for the list rather than inventing one.
11. Write `EaforaApp/DesignTokens.swift` from `web/style/_tokens.scss`, and `Localizable.xcstrings` with the strings the first screen needs.
12. Write `EaforaApp/EaforaApp.swift` and `Map/MapView.swift`: launch straight into the map, no splash, per FR-034.
13. XCTest the surface bridge (reported size matches the layer's drawable size) and the embedded-bundle locator.
14. Verify first paint on the simulator against `docs/design/stub-mobile.html` frame 00, and confirm an idle app issues no GPU work.

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
