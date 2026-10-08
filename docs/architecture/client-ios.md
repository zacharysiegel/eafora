# iOS client architecture

> **Status: draft, 2026-06-17.** This document is the per-platform deep-dive companion to `docs/architecture/client.md` (cross-cutting client architecture) and `docs/architecture/overview.md` (system overview), and a sibling to `docs/architecture/client-web.md` (web client). It covers everything specific to the **iOS** client surface: the native Xcode project, the `ios` crate's xcframework binary product, the UniFFI binding generation, the SwiftUI + MTKView render bridge, the `Library/Caches`-backed artifact cache, the in-bundle embedded artifact, the App Store distribution flow, and the testing strategy. The sibling `client-android.md` lags iOS per overview §Per-platform v1 build vs iteration scope.

## Scope of this document

This document covers everything between **the consumer-side contract `client.md` defines** and **a signed `.ipa` on TestFlight or the App Store**:

- The `ios/` directory: the `ios` Cargo crate that defines the UniFFI surface and builds into a static library, plus the native Xcode project beside `ios/src/` (added in Phase A) that consumes the xcframework built from it.
- The xcframework build pipeline: `cargo build` for `aarch64-apple-ios` (device) and `aarch64-apple-ios-sim` (simulator); `xcodebuild -create-xcframework`; UniFFI-generated Swift bindings via the proc-macro form; the Run Script build phase that ties them together.
- The SwiftUI shell: navigation, the design-token mapping from `docs/design/README.md` to a Swift extension, the MTKView + `UIViewRepresentable` map surface, the bottom-sheet region detail.
- The platform glue: choosing the `Library/Caches/` directory the Rust artifact cache writes to, and locating the embedded bundle in the app bundle's `Resources/`.
- The UniFFI binding boundary: what crosses the FFI seam, what stays Swift-side, the async + error mapping conventions.
- App Store distribution: signing via App Store Connect API key, TestFlight, Universal Links.
- Testing strategy for the iOS-only TDD-required surfaces.

Cross-cutting client behavior (artifact-consumption contract, fetch / cache / load pipeline shape, SQLite-in-the-client, FlatGeobuf reading, license-shard composition, embedded bundle semantics, hot-swap protocol) is in `client.md` and is **not relitigated here**. The visual identity is in `docs/design/README.md` and is also not relitigated.

## Locked decisions referenced (not relitigated)

From the constitution, `docs/architecture/overview.md`, `docs/architecture/client.md`, and `docs/design/README.md`:

- UI framework: SwiftUI. (Overview §iOS client; Constitution III)
- Map surface: `MTKView` wrapped in `UIViewRepresentable`; render loop on main thread; heavy compute offloaded to background tasks before the next frame. (Overview §iOS client)
- Rust integration: the `ios` crate is built as an xcframework; UniFFI generates Swift bindings via the **proc-macro form** (`#[uniffi::export]` annotations on free functions in the `ios` crate), not the declarative UDL form. (Overview §UDL vs proc-macro for UniFFI)
- GPU baseline: Apple A14 / A15 (iPhone 12 / 13 generation, 2020–2021) and later; iOS 18+ minimum SDK target. (Overview §iOS client)
- Async: Swift's `async`/`await` consumes UniFFI async functions; cancellation is one-way (Swift task cancellation does not propagate; Rust must self-cancel based on a polled flag if cancellation matters). (Overview §iOS client; §FFI design rules)
- HTTP: `shared::http::ReqwestHttpFetch`, inside Rust; Swift makes no artifact requests.
- Cache: file system inside the app sandbox, written by Rust's `shared::artifact::FilesystemArtifactCache`. (Overview §Client cache strategy)
- Embedded bundle on native: files copied into the app bundle at build time; opened before the renderer is created, so the first frame draws against it; doubles as the offline-capable baseline. (Client §Embedded downsampled artifact)
- Web and iOS develop in parallel from v1, deliberately, to prevent the architecture from overfitting to the web platform's constraints. Android lags. The native apps double as personal-learning goals for the parallel game project; for funder pitches, only the web is the user-facing v1 deliverable. (Project memory)
- Apple Developer Program: $99/year; App Store Connect API key for CI; TestFlight for testing; ~24–48 hour App Store review. (Overview §App store distribution)
- Visual identity: sharp white-paper-with-red-ink, square corners (≤1px radius), 1px borders, no shadows, no gradients, only the zoom-to-country animation through v1. (`docs/design/README.md`)
- No live API through v2: every datum the user sees came from a versioned CDN artifact. (Constitution VI)

## Workspace placement

`ios/` is the root of the `ios` Cargo crate, symmetric with `web/`: package `ios`, `[lib] name = "eafora_ios"`, `crate-type = ["staticlib"]`, depending on `shared` with the `render` feature and on `uniffi` with the `tokio` feature. `shared` and `web` have no UniFFI dependency, so the web build never compiles UniFFI. The native Xcode project, added in Phase A, lives beside `ios/src/` inside `ios/` and consumes a Rust-produced xcframework that bundles the crate's static libraries with the UniFFI-generated C headers and modulemap; the generated Swift bindings are emitted separately to `target/uniffi/swift/eafora_ios.swift`.

```
eafora/
├── shared/                                   # the shared Rust library (consumer surface)
├── ios/                                      # this document's subject
│   ├── Cargo.toml                            # the `ios` crate (lib name eafora_ios, staticlib)
│   ├── src/                                  # the UniFFI surface: cache, bundle, renderer, handle, error, revision
│   ├── project.yml                           # XcodeGen config; project file is generated from this, not committed
│   ├── Eafora.xcodeproj/                     # GITIGNORED; regenerated by `xcodegen generate`
│   ├── EaforaApp/                            # Swift sources for the app target
│   │   ├── EaforaApp.swift                   # @main entrypoint; root App struct
│   │   ├── ContentView.swift                 # root view; hosts MapView + sheet bindings (region detail, Settings)
│   │   ├── DesignTokens.swift                # Color / Font / spacing extensions per docs/design/
│   │   ├── Assets.xcassets/                  # asset catalog (app icon, launch screen)
│   │   ├── Info.plist                        # required app metadata; URL types for Universal Links
│   │   ├── Resources/
│   │   │   └── embedded_artifacts/           # downsampled bundle copied here by the build script
│   │   │       ├── manifest.json
│   │   │       ├── geometry/
│   │   │       └── (statistic shards under whatever subdirectory the manifest names)
│   │   ├── Map/                              # the primary surface (client-side map view)
│   │   │   ├── MapView.swift                 # SwiftUI container
│   │   │   ├── MapMTKView.swift              # MTKView wrapped in UIViewRepresentable
│   │   │   ├── MapRenderer.swift             # CAMetalLayer + drawable lifecycle, calls the renderer exports
│   │   │   ├── LegendView.swift              # choropleth legend overlay
│   │   │   └── ControlsView.swift            # statistic picker, year scrubber, source panel
│   │   ├── Region/                           # region detail (a destination — region = any level of the region hierarchy: country, subregion, supranational, etc.)
│   │   │   ├── RegionDetailView.swift
│   │   │   └── HistoryChartView.swift
│   │   ├── SettingsView.swift                # bottom-sheet Settings; About inlined at top, utility rows below, build info at bottom (no separate AboutView through v1)
│   │   └── EmbeddedBundle.swift              # locates the embedded bundle directory and passes it to open_first_paint_bundle
│   ├── EaforaAppTests/                       # XCTest unit tests
│   ├── EaforaAppUITests/                     # XCUITest UI tests (deferred per §Testing strategy)
│   └── README.md                             # quickstart for iOS development
```

The Swift code is organized by feature: directory only when a feature has 2+ files (`Map/`, `Region/`); single-file features sit flat under `EaforaApp/`. Mirrors the same convention web uses. Shared shell code (design tokens, navigation root) sits at the `EaforaApp/` top level.

Directory names use PascalCase (`Map/`, `Region/`), matching iOS convention. The web crate uses lowercase (`map/`, `region/`) because Rust's convention is lowercase modules; both follow their host language's idiom rather than enforcing project-wide uniformity.

The Xcode project does not reference the `ios` crate's sources directly. Instead, it references `target/uniffi/EaforaIOS.xcframework` as a binary dependency (declared in `ios/project.yml`); the xcframework is built by the pipeline described in §Build toolchain. The xcframework lives under `target/` like every other generated artifact in the workspace; it is not committed and is rebuilt on demand.

## Build toolchain

### xcframework build pipeline

The `ios` crate ships as an xcframework: a single `.xcframework` bundle containing static libraries for every Apple target slice plus the matching C headers and modulemap. Build flow (debug profile by default; `--release` on the script selects the release profile and the `release/` directories):

1. `cargo build -p ios --target aarch64-apple-ios` → produces `target/aarch64-apple-ios/debug/libeafora_ios.a` (device slice).
2. `cargo build -p ios --target aarch64-apple-ios-sim` → produces `target/aarch64-apple-ios-sim/debug/libeafora_ios.a` (Apple-Silicon-Mac simulator slice).
3. Generate Swift bindings from the simulator archive's UniFFI metadata. Use the dedicated `uniffi-bindgen-swift` binary (separate from the generic `uniffi-bindgen`; gives finer-grained control over Swift-specific artifacts). Three separate invocations to produce each artifact independently:

   ```sh
   cargo run -p uniffi_bindgen_swift -- target/aarch64-apple-ios-sim/debug/libeafora_ios.a target/uniffi/swift --swift-sources
   cargo run -p uniffi_bindgen_swift -- target/aarch64-apple-ios-sim/debug/libeafora_ios.a target/uniffi/headers --headers
   cargo run -p uniffi_bindgen_swift -- target/aarch64-apple-ios-sim/debug/libeafora_ios.a target/uniffi/headers --modulemap --module-name eafora_iosFFI --modulemap-filename module.modulemap
   ```

   `uniffi-bindgen-swift` is the binary of the workspace package `tools/uniffi_bindgen_swift`, whose `main` calls `uniffi::uniffi_bindgen_swift()`. The Swift bindings land in `target/uniffi/swift/eafora_ios.swift` and import the C module `eafora_iosFFI`. The modulemap is generated without `--xcframework`, because that flag emits a `framework module`, which a consuming Swift package cannot import.
4. `xcodebuild -create-xcframework -library target/aarch64-apple-ios/debug/libeafora_ios.a -headers target/uniffi/headers -library target/aarch64-apple-ios-sim/debug/libeafora_ios.a -headers target/uniffi/headers -output target/uniffi/EaforaIOS.xcframework` → produces the binary product under `target/` alongside the rest of the build outputs.
5. `target/uniffi/EaforaIOS.xcframework` is referenced as a binary framework dependency in `ios/project.yml` (via the relative path `../target/uniffi/EaforaIOS.xcframework`), picked up by the regenerated `Eafora.xcodeproj`.

The pipeline is encapsulated in `scripts/build/build-ios-xcframework.sh`, checked into the repo. Invoked:

- As an Xcode Run Script build phase before the "Compile Sources" phase, so opening the project in Xcode and building rebuilds the xcframework if the Rust source has changed.
- In CI explicitly before `xcodebuild build`.

`setup.sh` does not invoke it. `setup.sh` sets up the environment (installs toolchains, including the iOS Rust targets, xcodegen, and a simulator runtime when Xcode is present; decrypts secrets) and performs no builds; running `xcodegen generate` is deferred to Phase A, which adds `project.yml`. The first build after a fresh clone runs `build-ios-xcframework.sh` for the first time via Xcode's pre-build phase; that's where the compilation happens.

The Run Script build phase is conservative: it invokes the shell script unconditionally and lets Cargo's incremental compilation determine whether the slices are recompiled; the script regenerates the bindings and recreates the xcframework on every run. A no-op rebuild after the first run takes approx. 5–10 seconds — acceptable overhead for the every-build correctness guarantee.

The xcframework itself is **gitignored**. Every build produces it from source; staleness is impossible.

### UniFFI: proc-macro form, dedicated FFI crate

Per overview §UDL vs proc-macro for UniFFI, Eafora uses the **proc-macro form**: `#[uniffi::export]` annotations on Rust items, no separate `.udl` file. The discipline that makes this work is a **dedicated FFI crate**, `ios`, whose `src/` holds every export: free functions over statics, with no exported object. The crate calls into `shared` and defines thin FFI-facing types where the internal shape isn't the right contract for the boundary.

The crate is the single reviewable surface for "what the iOS app sees." A PR that touches `ios/src/` changes the FFI; a PR that doesn't touch it doesn't.

Sketch of the exports, by file:

```rust
// ios/src/lib.rs
uniffi::setup_scaffolding!();

// ios/src/error.rs
#[derive(Debug, uniffi::Error)]
pub enum FfiError {
    Failed { message: String },
}

// ios/src/cache.rs
static CACHE: OnceLock<FilesystemArtifactCache> = OnceLock::new();

#[uniffi::export]
pub fn set_cache_directory(cache_directory: String) -> Result<(), FfiError> { /* ... */ }

// ios/src/bundle.rs
static PUBLICATION: OnceLock<watch::Sender<Arc<Bundle>>> = OnceLock::new();

#[uniffi::export(async_runtime = "tokio")]
pub async fn open_first_paint_bundle(embedded_directory: String) -> Result<String, FfiError> { /* ... */ }

#[uniffi::export(async_runtime = "tokio")]
pub async fn load_live_bundle(discovery_url: String, static_repository_base_url: String) -> Result<String, FfiError> { /* ... */ }

// ios/src/handle.rs
#[derive(uniffi::Record)]
pub struct UiKitSurfaceHandle {
    pub layer_ptr: u64,
    pub view_ptr: u64,
}

// ios/src/renderer.rs
thread_local! {
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

#[uniffi::export]
pub fn create_renderer() -> Result<(), FfiError> { /* ... */ }

#[uniffi::export]
pub fn attach_surface(handle: UiKitSurfaceHandle, width: u32, height: u32) -> Result<(), FfiError> { /* ... */ }

#[uniffi::export]
pub fn resize_surface(width: u32, height: u32) -> Result<(), FfiError> { /* ... */ }

#[uniffi::export]
pub fn detach_surface() -> Result<(), FfiError> { /* ... */ }

#[uniffi::export]
pub fn destroy_renderer() -> Result<(), FfiError> { /* ... */ }

// ios/src/revision.rs
#[uniffi::export]
pub fn revision() -> String { /* ... */ }
```

The intent: free functions over set-once statics + concrete request / response types + a single `FfiError` thrown on failure. Generic types and trait objects are absent (UniFFI doesn't support them). Per the project's error-strings-over-enums preference, `FfiError` carries a `message` payload rather than a per-failure-mode variant; a Swift caller that must distinguish failures matches on the message. `FfiError` is returned only by exported functions; the code behind them returns `shared::error::AppError`, or `AppErrorStatic` on the async load path (§Threading).

`uniffi-bindgen-swift` reads the metadata that the proc-macros embed in the compiled `libeafora_ios.a` and produces idiomatic Swift in `eafora_ios.swift`: a top-level function per export; `throws` for fallible functions; `async throws` for the two loads; a `UiKitSurfaceHandle` struct for the record. Swift `do` / `try` / `catch` is the call-site idiom.

When a type's internal shape isn't quite right for the FFI, the `ios` crate defines a thin FFI-facing type that converts into the internal one: `UiKitSurfaceHandle` converts into `shared::render::WindowHandle::UiKit`. The FFI-facing type is the FFI surface; the internal type stays free to evolve. The bundle itself never crosses: the loads publish it inside Rust and return its version label.

The current exports are provisional; Phase A's Swift call sites decide each export's final shape, and nothing is exported speculatively. `draw_frame`, `region_at_point`, `set_period`, `set_statistic`, `pan`, and `zoom` are not exported yet: they need the viewport, frame-state, and gesture orchestration in `web/src/map/canvas/driver.rs` to move into `shared` first, and they land in Phase A.

### Build profile

The xcframework build invokes `cargo build -p ios --target aarch64-apple-ios` (and the simulator equivalent) in the debug profile by default; `build-ios-xcframework.sh --release` builds the release profile. The standard `[profile.release]` settings apply (Cargo defaults; the workspace sets no `panic` override, which overview §Workspace Cargo profile records as a pending decision); no additional iOS-specific tuning.

Binary size doesn't justify a custom profile on iOS. The whole app ships once at install time and updates infrequently; users don't watch the size on every launch the way a web client downloads its WASM. The few megabytes a size-trade like `opt-level = "z"` would save are invisible to users, while the runtime cost (less aggressive inlining, slower hot paths) is real even if small. Standard release optimization wins.

Web is the asymmetric case: every cold-cache page load downloads the WASM, so shaved KB are shaved network-transfer time on first paint — directly user-visible. Web's `[profile.wasm-release]` (see `client-web.md` §`wasm-opt`) does use `opt-level = "z"` for exactly that reason. iOS and Android don't.

Expected size of `libeafora_ios.a` post-strip: roughly 8-12 MB per slice, dominated by `wgpu` + `flatgeobuf` + `rusqlite`'s sqlite3 bytes. The xcframework is the union of slices; `xcframework` deduplicates internally to the extent possible. App Store thinning at install time delivers only the architecture the device uses.

### Xcode integration

The Xcode project file is **generated** from `ios/project.yml` via [XcodeGen](https://github.com/yonaskolb/XcodeGen), not hand-edited or committed. `xcodegen generate` produces `ios/Eafora.xcodeproj/`; the directory is gitignored. Anyone (including CI) regenerates it from the YAML on demand. Editing the project means editing `project.yml` in your text editor, then re-running `xcodegen generate`.

This avoids:

- Hand-edited XML in the project file (the format is opaque, version conflicts on it are painful, and small Xcode actions can rewrite huge swaths unpredictably).
- The need to open Xcode to add a source file or change a build setting.

`setup.sh` installs the iOS-side toolchain when Xcode is present (`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`, `brew install xcodegen`, and an iOS simulator runtime via `xcodebuild -downloadPlatform iOS` when none is installed). Running `xcodegen generate` to produce the initial project file is deferred to Phase A, which adds `project.yml`. Setup prepares the environment only; it does not generate the project file (deferred to Phase A) and does not compile or build (see §xcframework build pipeline for why building happens via Xcode's Run Script phase instead).

Reference shape of `ios/project.yml`:

```yaml
name: Eafora
options:
  bundleIdPrefix: org.eafora
  deploymentTarget:
    iOS: "18.0"
targets:
  Eafora:
    type: application
    platform: iOS
    sources:
      - path: EaforaApp
    dependencies:
      - framework: ../target/uniffi/EaforaIOS.xcframework
      - sdk: MetalKit.framework
      - sdk: Metal.framework
    info:
      path: EaforaApp/Info.plist
      properties:
        UIApplicationSceneManifest: { UIApplicationSupportsMultipleScenes: false }
    settings:
      base:
        TARGETED_DEVICE_FAMILY: "1"           # iPhone only
        SWIFT_VERSION: "6.0"
        ARCHS: arm64
        DEVELOPMENT_TEAM: A1B2C3D4E5          # Apple-assigned team ID; not a secret; replace with the real value
      configs:
        Debug:
          CODE_SIGN_STYLE: Automatic
        Release:
          CODE_SIGN_STYLE: Manual
          CODE_SIGN_IDENTITY: "Apple Distribution"
          PROVISIONING_PROFILE_SPECIFIER: "Eafora App Store"
    preBuildScripts:
      - name: Build EaforaIOS xcframework
        script: ../scripts/build/build-ios-xcframework.sh
      - name: Sync embedded artifacts
        script: ../scripts/build/sync-embedded-bundle.sh ${SRCROOT}/EaforaApp/Resources/embedded_artifacts/
      - name: Inject git revision
        script: ../scripts/inject-git-revision.sh
```

Configuration the YAML expresses:

- Deployment target: iOS 18.0 (per overview §iOS client).
- Architectures: `arm64` only (Apple Silicon; armv7 is not built).
- Frameworks: `EaforaIOS.xcframework` linked against; `MetalKit.framework` and `Metal.framework` linked for the MTKView path.
- Pre-build scripts: rebuild the xcframework on demand, then sync the embedded bundle into the app's `Resources/`, then inject the source revision into `Info.plist`.
- Code-signing style: automatic for Debug (Xcode picks any installed cert that matches), manual for Release (uses a named distribution cert + provisioning profile created in App Store Connect). The Debug path is what developers use day-to-day; the Release path is what CI uses for App Store / TestFlight uploads.
- `DEVELOPMENT_TEAM` is the 10-character Apple Developer Team identifier. One value, shared across every machine that builds the app (your developer Mac, the Mac mini CI). What differs per machine is which signing certificate is in the keychain — your dev machine has a Development cert; CI has a Distribution cert — and whether the App Store Connect API key for upload automation is installed (CI only). All of those live under the same team. Not a secret; the team ID appears in every provisioning profile inside the shipped `.ipa` and in the public `apple-app-site-association` file. Committed directly in `project.yml`. The actual auth lives in the certificates and the App Store Connect API key (per §Signing and CI, treated as a real secret).

Anything that genuinely needs Xcode (asset catalog edits for the app icon, one-time signing setup with the developer account, occasional debugging) still gets done in Xcode — open the generated `Eafora.xcodeproj`, do the thing, save what you can save back into source-controlled files (`Info.plist`, asset catalogs); changes to the project file itself are pointless because regeneration overwrites them.

A distinction worth naming: the project bundle (`Eafora.xcodeproj/`, a directory presented as a single file in Finder; macOS calls this a "package") is generated and gitignored. The asset *catalog* (`Assets.xcassets/`, also a package) is content — Xcode-editable JSON files under that directory — and is **committed**. Same applies to `Info.plist`. The "no hand-editing in Xcode" rule applies only to project structure (targets, build phases, build settings); the rest of what Xcode lets you edit (catalogs, plists, code) stays normal hand-or-Xcode-edit-then-commit. The `Resources/embedded_artifacts/` directory is a third category — opaque-bytes asset that lives at the bundle root, gitignored, regenerated by `scripts/build/sync-embedded-bundle.sh`. We use the asset catalog for app-icon-shaped content (multi-resolution variants, accent colors) and raw `Resources/` for the embedded bundle (just files; the asset catalog has no idea what a `manifest.json` or a `world-50m-*.fgb` is).

### Build dependency direction

Identical to the web side: the iOS build **pulls** the static-asset embedded bundle from the producer's downsampled output via `scripts/build/sync-embedded-bundle.sh`, never the producer pushes into `ios/EaforaApp/Resources/`. The script takes the destination directory as its first argument and plain-copies (`cp -R`) the contents of `$EAFORA_DOWNSAMPLED_DIR/latest/` into it. Same script as web, different argument.

Plain copy on both platforms — see `client-web.md` §Build dependency direction for the rationale (symlinks and hard links add complexity for a few-MB duplication that doesn't matter at v1's scale; Xcode's Copy Bundle Resources phase wants real files anyway).

`ios/EaforaApp/Resources/embedded_artifacts/` is gitignored. The bundle is rebuilt on every CI build and on every local Xcode build; staleness is not a correctness concern because the live CDN fetch upgrades it on first online interaction (per `client.md`).

### Build version provenance

Every shipped binary carries the source revision (today, the git SHA) it was built from. Two separate surfaces because two consumers want it for two different reasons:

#### Info.plist injection (debugging surface)

A pre-build script writes the current revision identifier (today, the git SHA) into the app's `Info.plist` at a custom key (`EaforaRevision`). Used for crash-report symbolication, support diagnostics, and "what version was the user on when they hit this bug." Read at App launch into the About-page footer (or attached as a tag to crash-reporter events in v2+).

`scripts/inject-git-revision.sh`:

```sh
#!/usr/bin/env sh
set -euo pipefail

REVISION=$(git rev-parse HEAD)
DIRTY=$(git diff --quiet HEAD -- || echo "-dirty")
BRANCH=$(git rev-parse --abbrev-ref HEAD)

INFO_PLIST="$BUILT_PRODUCTS_DIR/$INFOPLIST_PATH"

/usr/libexec/PlistBuddy -c "Set :EaforaRevision ${REVISION}${DIRTY}" "$INFO_PLIST" 2>/dev/null \
    || /usr/libexec/PlistBuddy -c "Add :EaforaRevision string ${REVISION}${DIRTY}" "$INFO_PLIST"
/usr/libexec/PlistBuddy -c "Set :EaforaBranch $BRANCH" "$INFO_PLIST" 2>/dev/null \
    || /usr/libexec/PlistBuddy -c "Add :EaforaBranch string $BRANCH" "$INFO_PLIST"
```

Wired into `ios/project.yml`'s `preBuildScripts` (see §Xcode integration for the full block) as a third entry alongside the xcframework + embedded-bundle scripts.

Swift reads at runtime:

```swift
let revision = Bundle.main.infoDictionary?["EaforaRevision"] as? String ?? "unknown"
let revisionShort = String(revision.prefix(12))   // truncate at display time
```

Full SHA is stored; truncation happens at display time. The `-dirty` suffix tags the SHA when the working tree has uncommitted changes — debug builds during dev iteration will routinely show as dirty, which is the correct signal.

#### Rust FFI (runtime surface)

The Rust side exposes its own revision via UniFFI. Used for anything the running app needs to **act** on the version: error-message annotations, log lines, server-reported-minimum-version comparisons. `shared/build.rs` captures the revision at compile time:

```rust
// shared/build.rs (abridged)
fn main() {
    // `git rev-parse HEAD`; a debug build falls back to "unknown", a release build fails without a revision.
    let resolved_revision: String = /* ... */;

    println!("cargo:rustc-env=EAFORA_REVISION={}", resolved_revision);
}

// shared/src/revision.rs
pub const REVISION: &str = env!("EAFORA_REVISION");

// ios/src/revision.rs
#[uniffi::export]
pub fn revision() -> String {
    shared::revision::REVISION.to_string()
}
```

Swift sees `revision()` (top-level function in the generated bindings).

#### Why two values

These two facts can diverge. The Info.plist `EaforaRevision` is "what was the source state when this Xcode build ran"; `shared::revision::REVISION` is "what was the source state when the Rust library compiled." Almost always identical, but in development they can drift if one side is rebuilt without the other. Surfacing them as separate values means the divergence is visible when it matters; conflating them into one would hide a real signal.

The name "revision" rather than "sha" is deliberate. The value is a git SHA today, but the *concept* the app cares about is "what source state did this binary come from"; if we ever migrate version control systems (Jujutsu, Mercurial, Pijul, anything else), the script changes, the displayed value changes, the consumer-facing name doesn't.

## Rendering: MTKView + wgpu Metal

iOS rendering uses `MTKView` as the render surface and wgpu's Metal backend underneath. The bridge:

1. SwiftUI declares a `UIViewRepresentable` wrapping `MTKView`.
2. `MTKView`'s delegate (`MTKViewDelegate`) is the SwiftUI-side `MapRenderer` Swift class.
3. When the MTKView enters the window hierarchy and its `CAMetalLayer` becomes available, the `MapRenderer` calls `attachSurface(handle: UiKitSurfaceHandle(layerPtr: ..., viewPtr: ...), width: ..., height: ...)`. Rust converts the handle to `WindowHandle::UiKit`, builds a wgpu surface from the view pointer through `Instance::create_surface_unsafe`, and stores it on the renderer for the lifetime of the view. **One-time call, not per-frame.**
4. On `mtkView(_:drawableSizeWillChange:)`, the `MapRenderer` calls `resizeSurface(width: ..., height: ...)`. Rust reconfigures the wgpu surface to the new size.
5. On `setNeedsDisplay()` triggering a `draw(in:)` callback, the `MapRenderer` asks Rust to draw a frame: Rust pulls the current frame's texture from its persistent surface (`surface.get_current_texture()`), encodes wgpu draw commands, submits to the Metal queue, presents. The draw export is deferred to Phase A (§UniFFI: proc-macro form, dedicated FFI crate).
6. When the MTKView leaves the hierarchy (view torn down, scene phase changes), the `MapRenderer` calls `detachSurface()`. Rust drops the wgpu surface; the rest of the renderer (bundle receiver, pipelines, device) lives on. `destroyRenderer()` drops the renderer itself.

All renderer functions (`createRenderer`, `attachSurface`, `resizeSurface`, `detachSurface`, `destroyRenderer`) are synchronous and must be called from the same thread, Swift's main thread: the renderer lives in a `thread_local!` because wgpu state is bound to its creating thread, and an async export may resume on another thread.

The Swift-to-Rust attach call hands a `UiKitSurfaceHandle(layerPtr:, viewPtr:)` value, a UniFFI record carrying the layer and view pointers as `u64`. Rust converts it to `shared::render::WindowHandle::UiKit`, from which `shared/src/render/surface.rs` builds the surface target and hands it to wgpu's surface constructor.

The renderer's wgpu device, queue, and pipeline state are constructed once by `createRenderer()`, which requires a published bundle, so Swift calls it after `openFirstPaintBundle`. The surface is attached later when the view is ready. The two-phase setup is necessary because the platform render target doesn't exist at app init; everything else does.

Per `docs/design/README.md`, the only v1 animation is the zoom-to-country camera move; every other state change is instant. Rendering is **event-driven**, not loop-driven: `MTKView.isPaused = true` plus `setNeedsDisplay()` on every state change. State changes — selection, hover, statistic-picker, year-scrubber drag, bundle hot-swap — invalidate the view; MTKView coalesces multiple invalidations between vsyncs into one draw call at the next vsync; the GPU stays idle when nothing is happening. The zoom-to-country move is the one case that runs a temporary per-frame loop: on re-selecting the already-selected country the shell polls the `shared::map::ViewportTransition` each vsync (via `setNeedsDisplay()` scheduled per frame) for a fresh interpolated viewport, then stops when the transition lands. The same shape `client-web.md` §Client-side map view describes for web (dirty flag + `requestAnimationFrame`), expressed in iOS's native vocabulary.

When v2+ adds further animations (per `docs/design/README.md` §Animation — under 150ms, linear easing, snap-don't-glide), the same self-perpetuating chain of `setNeedsDisplay()` calls carries them: continuous render rate during the animation; idle again after. Same pattern client-web.md describes for animation handling there.

### GPU baseline

Per overview §iOS client, the deployment target is iOS 18 + Apple A14 / A15 minimum. Metal feature levels at this baseline are uniformly modern: argument buffers tier 2, indirect command buffers, programmable blending, etc. wgpu's Metal backend abstracts over the version differences automatically; the renderer (`shared::map::Renderer`) does not branch on Metal feature levels.

### Threading

Unlike the web (single-threaded WASM), iOS supports full multithreading. The `ios` crate enables tokio's `rt-multi-thread`, and its two async exports run on UniFFI's tokio runtime (`#[uniffi::export(async_runtime = "tokio")]`). Practical use:

- Render loop: main thread, driven by MTKView's display link; the renderer exports are synchronous and bound to that thread (§Rendering: MTKView + wgpu Metal).
- Live-bundle fetch: the `load_live_bundle` export, running on the tokio runtime. The fetched files are opened as a `shared::artifact::Bundle` and published via the `tokio::sync::watch::Sender<Arc<Bundle>>` the renderer subscribes to, so a live load repaints with no relaunch.
- Subnational geometry parsing (v2+): another background task; published through the same watch channel as a partial-update.

Per `client.md` §Bundle hot-swap, in-flight queries holding an old `Arc<Bundle>` complete against the old bundle; the swap is wait-free.

The Swift side does not see tokio. It calls async UniFFI functions (`async fn` in Rust → `async throws` in Swift), and Swift's structured concurrency owns the Swift-side task lifecycle. Cancellation is one-way: cancelling a Swift `Task` does not cancel the Rust async future. Long-running Rust futures must self-poll a cancellation flag if the user-visible operation has a "Cancel" button (none through v1).

UniFFI requires an async export's future to be `Send`. `shared::artifact::ArtifactCache` deliberately has no `Send` bound on its async functions: the web's `OpfsArtifactCache` holds `!Send` `JsValue`, and one trait serves every platform (see `shared/src/artifact/cache.rs`). The iOS loads call the loader in `shared/src/artifact/load.rs` with the concrete `FilesystemArtifactCache`, `ReqwestHttpFetch`, and `FilesystemFetch`, whose futures are `Send`, and every function on their await path returns `shared::error::AppErrorStatic` (minimer's static error, which holds no boxed source error). `AppError` stays everywhere else.

## Cache: `Library/Caches/`

Per `client.md` §Cache eviction, the cross-platform cache contract is the same across web and native; the platform-specific layer is the implementation. iOS uses the app sandbox's `Library/Caches/` directory.

### Directory layout

```
<app-sandbox>/Library/Caches/artifacts/
├── <version_label>/
│   ├── manifest.json
│   ├── geometry/
│   │   └── world-50m-<sha256>.fgb
│   └── (statistic shard subdirectory per the manifest's relative_path entries)
│       └── ...
└── <other_version_label>/
    └── ...
```

Identical shape to the OPFS layout in the web client; only the root path differs. The cache stores files at exactly the `relative_path` the manifest carries; storing both the latest and the most-recent prior version means at most two version subtrees at any time. No `eafora/` parent namespace — the entire `Library/Caches/` directory is already inside Eafora's app sandbox, so namespacing under our own name would be redundant.

### iOS-specific behavior

- `Library/Caches/` is the right directory because iOS may purge it under storage pressure; the embedded bundle is the floor when that happens, and the live fetch path runs as if first launch on the next online start. Purge events are silently recovered. (Compare `Library/Application Support/`, which iOS does not reclaim — wrong shape for cached-from-network data.)
- No quota or `persist()` machinery: iOS does not expose a per-app quota the way browsers do; the cache writes until disk pressure forces a purge, at which point the loader runs the fetch path again. There is no `navigator.storage.persist()` equivalent.
- No support-version branching: the cache uses `std::fs`, which behaves the same on every supported iOS version (iOS 18+). There is no equivalent of the OPFS-unsupported fallback path.
- `NSURLIsExcludedFromBackupKey` is set by Swift on the `artifacts/` directory before it passes the path to `set_cache_directory`; the cache contents are reproducible from the CDN and don't belong in iCloud / iTunes backup.

### Implementation: `FilesystemArtifactCache`

The cache on iOS is `shared::artifact::FilesystemArtifactCache` (`shared/src/artifact/filesystem_cache.rs`), the `ArtifactCache` implementation over `std::fs`, in Rust. Reading and writing happen entirely in Rust; no artifact bytes cross the FFI.

Swift chooses the directory (`Library/Caches/artifacts/`) and calls `setCacheDirectory(cacheDirectory:)` once, before any load. The `ios` crate holds the cache in `static CACHE: OnceLock<FilesystemArtifactCache>`; a second call throws.

Eviction policy from `client.md` §Cache eviction (keep current + most-recent prior) is `shared::artifact::load::evict_stale_versions`, which `load_live_bundle` runs after publishing a live bundle. It keeps the two newest versions, ranked by each version's manifest, and deletes the rest. Opening the first-paint bundle also discards any cached version this build cannot open.

## Embedded bundle: app bundle Resources

Per `client.md` §Embedded downsampled artifact, native clients ship the embedded bundle inside the app at build time. On iOS:

- The downsampled output (`manifest.json` + `geometry/` + statistic shards) is copied into `ios/EaforaApp/Resources/embedded_artifacts/` by `scripts/build/sync-embedded-bundle.sh` (Run Script build phase 2; see §Build toolchain).
- The "Copy Bundle Resources" build phase copies that directory into the `.app` bundle.
- At app launch, `EmbeddedBundle.swift` locates the bundle root via `Bundle.main.url(forResource: "embedded_artifacts", withExtension: nil)` and passes its path to `openFirstPaintBundle(embeddedDirectory:)`. In Rust, that export opens the newest readable cached bundle, or else reads the embedded bundle from that directory through `shared::http::FilesystemFetch`, verifies each file's SHA-256, writes it into the cache, and opens it. Swift awaits it, then calls `createRenderer()`, **before the first frame is drawn**.
- The first published bundle creates the `tokio::sync::watch::Sender<Arc<Bundle>>` (`static PUBLICATION` in `ios/src/bundle.rs`) that the renderer subscribes to. The map renders its first frame against that bundle.
- Swift then calls `loadLiveBundle(discoveryUrl:staticRepositoryBaseUrl:)`, which runs the discovery + live-fetch flow defined in `client.md` §Discovery and live bundle resolution in Rust: fire the discovery fetch (`https://app.eafora.org/discovery`) and a speculative manifest fetch (against the static `repository_base_url` fallback) concurrently; reconcile per `client.md`; persist verified bytes to the file-system cache; publish the resulting `Arc<Bundle>` to the renderer's watch channel. If both fetches fail, the first-paint bundle remains the floor. Both loads return the published version label.
- On hot-swap, the live-fetch task replaces the published bundle (per `client.md` §Bundle hot-swap); the next `setNeedsDisplay()` redraws the map against the new data.

The embedded bundle is **also the offline-capable baseline**. A user opening Eafora without connectivity and without a populated cache still sees a usable, if slightly stale, atlas. Updates to the embedded bundle ride app updates: the user installs a new app build (whose `ingestion build --downsampled` output captured a newer baseline), and the floor advances.

## SwiftUI shell

### Navigation structure

Region detail and Settings are both **bottom sheets**, not stack pushes. The map view stays visible behind the sheet, preserving spatial context. Matches `docs/design/stub-mobile.html` frame 01 and the iOS-native pattern for "inspect this thing without leaving where you are."

About is **inlined** at the top of Settings, not a separate destination. The About content is one screen at most (wordmark + Bosworth-Toller subtitle + etymology link + a short paragraph on framing, per `docs/design/README.md` §Naming and the About page); spending a row + a push transition on it would be ceremony for content the user can read in five seconds. Settings becomes one flat screen: About at the top as a header section, utility rows below, build version at the bottom. No inner `NavigationStack`.

```swift
@main
struct EaforaApp: App {
    @State private var selectedRegion: RegionCode? = nil
    @State private var settingsPresented = false

    var body: some Scene {
        WindowGroup {
            MapView(selectedRegion: $selectedRegion)
                .sheet(item: $selectedRegion) { regionCode in
                    RegionDetailView(regionCode: regionCode)
                        .presentationDetents([.medium, .large])
                        .presentationDragIndicator(.visible)
                }
                .sheet(isPresented: $settingsPresented) {
                    SettingsView()
                }
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button {
                            settingsPresented = true
                        } label: {
                            Image(systemName: "gear")
                        }
                    }
                }
                .onOpenURL { url in
                    handleUniversalLink(url,
                        selectedRegion: $selectedRegion,
                        settingsPresented: $settingsPresented)
                }
        }
    }
}
```

`SettingsView` is a `List` with About inlined at the top:

```swift
struct SettingsView: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        List {
            Section {
                AboutContent()
            }

            // future utility rows:
            //   Section { NavigationLink("Data sources", destination: DataSourcesView()) }
            //   Section { Button("Clear cache", action: clearCache) }

            Section("Build") {
                LabeledContent("Version", value: appVersion())
                LabeledContent("Revision", value: revisionShort())
            }
        }
        .navigationTitle("Settings")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Done") { dismiss() }
            }
        }
    }
}

struct AboutContent: View {
    var body: some View {
        VStack(alignment: .leading, spacing: .spaceMd) {
            Text("ēafora")
                .font(.title)
            Link("Old English, masc. · son, descendant, heir.",
                 destination: URL(string: "https://bosworthtoller.com/008338")!)
                .font(.bodyEafora)
                .foregroundColor(.accentLink)
            Text("etymology and framing copy here, per docs/design/README.md")
                .font(.bodyEafora)
        }
        .padding(.vertical, .spaceSm)
    }
}
```

How navigation works in practice:

- Region detail: tap a region on the map → `selectedRegion` is set → SwiftUI presents the region-detail sheet (`.presentationDetents([.medium, .large])` for half-screen with drag-up-to-expand). Dismissing clears `selectedRegion`.
- Settings: tap the gear in the toolbar → `settingsPresented = true` → SwiftUI presents the Settings sheet, which shows About at the top, future utility rows in the middle, and build info at the bottom. Done button dismisses.
- About via deep link: an incoming `https://eafora.org/about` Universal Link sets `settingsPresented = true`. The user sees About at the top of the Settings sheet (it's the first thing visible without scrolling). (See §Universal Links.)

When v2+ adds enough utility surfaces to make Settings crowded (a separate Data sources screen, Language preferences, a Clear-cache flow with confirmation, etc.), promote About into its own `NavigationLink` row at that point. Through v1, inlined is right.

`RegionCode` is a small Swift struct wrapping the `region.code` string (the existing slug from the `region` table: `usa`, `south_america`, `germany`, etc.). It conforms to `Identifiable` (required by `.sheet(item:)`, which needs to distinguish "show me sheet for region A" from "show me sheet for region B" via the item's `id`) and `Hashable` (cheap; useful for `@State`, collection membership, and any future `NavigationStack` path binding). UniFFI-generated types may not provide these conformances automatically; the Swift-side wrap keeps the iOS code idiomatic without making the FFI surface aware of Swift's protocol requirements. Lightweight wrap; no behavior of its own.

Per `docs/design/README.md` §Naming and the About page, the app launches into the map. There is no splash screen and no gating chrome on first paint.

### Design tokens

The visual identity from `docs/design/README.md` lands in Swift as a single `DesignTokens.swift` extension file:

```swift
extension Color {
    static let paper          = Color(white: 1.0)
    static let ink            = Color(white: 0.0)
    static let accentActive   = Color(red: 0.835, green: 0.0,   blue: 0.0)   // #d50000
    static let accentLink     = Color(red: 0.0,   green: 0.314, blue: 1.0)   // #0050ff
    static let rule           = Color(white: 0.831)                            // #d4d4d4
}

extension CGFloat {
    static let spaceXs: CGFloat = 4
    static let spaceSm: CGFloat = 8
    static let spaceMd: CGFloat = 16
    static let spaceLg: CGFloat = 32
}

extension Font {
    static let bodyEafora     = Font.custom("Inter",          size: 14)
    static let dataEafora     = Font.custom("IBMPlexMono",    size: 13).monospacedDigit()
    // ... etc.
}
```

Token names match the design doc's vocabulary (`paper`, `ink`, `rule`, `sheet`); they are never aliased to consumer-app vocabulary. The reference HTML stubs at `docs/design/stub-desktop.html` and `docs/design/stub-mobile.html` are the visual ground truth; check the rendered iOS view against the mobile stub before claiming a design landed.

### Tabular figures

Every numeric display uses `.monospacedDigit()` on its `Font` (see `dataEafora` above). Table-row layouts use a `monospacedDigit` SwiftUI text style or a fixed-width `Text` modifier so columns align. This is the SwiftUI equivalent of the web's `font-variant-numeric: tabular-nums`.

### No animations through v1

SwiftUI's default transitions (`.animation()`, `withAnimation { ... }`) are **not** used in v1. State changes are explicit and instant: the new view renders without a transition. This is consistent with the paper-and-ink metaphor (per `docs/design/README.md` §Animation). v2+ may introduce a small set of step-transition-shaped animations under 150ms with linear easing; defer until then.

### Localization scaffolding

v1 ships English-only, but the localization machinery is in place from day one so future locales are mechanical (add a translation column) instead of a refactor (find every bare string literal). Per overview §FFI dividing line, the split is:

- UI-chrome strings (controls, errors, About-page prose, accessibility labels, navigation titles) live in the iOS app and use Apple's localization machinery.
- Domain-content strings (region names, statistic names, source attributions, data-status labels) live in the SQLite shard built by ingestion, joined to upstream-source values by code. **Out of scope for this section** — the iOS app reads them through the Rust exports; the i18n is producer-side.

#### String Catalog

iOS uses a **String Catalog** (`Localizable.xcstrings`, introduced in Xcode 15 / 2023) — replaces the older `.strings`/`.stringsdict` files with one JSON-on-disk catalog. The file lives at `ios/EaforaApp/Localizable.xcstrings`. Xcode auto-extracts string keys from your code on every build and adds new entries to the catalog; you fill in translations per locale in the catalog editor.

`ios/project.yml` declares the base development region and supported locales:

```yaml
options:
  developmentLanguage: en
targets:
  Eafora:
    info:
      properties:
        CFBundleLocalizations: ["en"]
```

When a second locale lands, add it to `CFBundleLocalizations` and fill in the translations in `Localizable.xcstrings`. No code changes needed.

#### Conventions in code

SwiftUI's `Text(_:)` initializer takes a `LocalizedStringKey`, which means a bare string literal in `Text("...")` is **automatically** treated as a localization key. The default usage is already correct:

```swift
Text("About")                       // automatically localizable; key = "About"
Text("Loading data...")             // same
```

For strings outside SwiftUI views — alert titles, error messages, accessibility labels constructed dynamically, anything taking a plain `String` — wrap explicitly:

```swift
let message = String(localized: "Cached data unavailable; using in-memory fallback")
throw EaforaError.invalidConfig(message: String(localized: "Repository URL malformed"))
```

`String(localized:)` produces a localized `String` from the catalog at runtime. Same key-extraction-at-build-time machinery as `Text`.

Strings with substitutions use SwiftUI's interpolation syntax, which the catalog handles natively:

```swift
Text("Population: \(populationCount, format: .number)")
String(localized: "\(regionName) has the highest TFR in \(year, format: .number)")
```

The catalog stores the format string (`"Population: %lld"`-shape internally) with placeholders identified by position; translators reorder placeholders for languages where word order differs.

#### Discipline

The discipline is "no bare string literal in a user-facing position." `Text("...")` and `String(localized: "...")` everywhere; never `Text(rawString)` where `rawString` is a String built without going through the catalog (the call still works at runtime but bypasses translation). For bare `String` constructions that genuinely shouldn't be localized — log messages, debug output, internal identifiers — that's fine; only user-visible strings need wrapping.

A SwiftLint custom rule catches obvious violations (`Text\("[^"]+"\)` is fine; `Text\(\w+\)` warrants review). Manual review for the rest. Nothing fully automated, but the discipline is small enough at v1's surface area to be tractable.

#### Numeric and date formatting

Even with one locale, use `Locale.current`-aware formatters from day one. SwiftUI provides `.formatted()` on numbers, dates, etc. that picks the user's locale automatically:

```swift
Text("\(tfrValue.formatted(.number.precision(.fractionLength(2))))")
Text("\(publicationDate.formatted(date: .abbreviated, time: .omitted))")
```

Locale-aware formatting handles thousand separators, decimal points, date order — all of which differ by locale even when the user's UI is English (an en-DE user sees German-style number formatting, en-US user sees American-style). Free correctness; no reason to skip.

Domain-content i18n (region names, etc.) is deferred per overview §FFI. The producer side adds translation columns to the seed-data tables when a second locale becomes a real deliverable; the iOS client reads them through the same shard queries the web client uses.

## HTTP fetch: `ReqwestHttpFetch`

Artifact fetching runs in Rust, in `shared`, shared with the web client. `shared::http::ReqwestHttpFetch` (`shared/src/http/reqwest_fetch.rs`) is the `HttpFetch` implementation `load_live_bundle` uses; `shared::http::FilesystemFetch` (`shared/src/http/filesystem_fetch.rs`) reads the embedded bundle from disk through the same interface. Swift makes no artifact requests.

The loader in `shared/src/artifact/load.rs` owns discovery reconciliation, version ranking, SHA-256 verification, the bound on concurrent file fetches, and eviction. The fetch implementations just propagate errors.

Swift passes the discovery URL and the static repository base URL to `loadLiveBundle(discoveryUrl:staticRepositoryBaseUrl:)`. The loader resolves `repository_base_url` at runtime via the discovery URL flow defined in `client.md` §Discovery and live bundle resolution and uses it for every shard fetch, falling back to the static base when discovery itself fails. Where Swift reads the static base from is deferred to Phase A. The indirection earns its keep on iOS specifically — TestFlight and App Store installs live on devices for months or years, and an R2 re-platform without the discovery indirection would silently break every install in the field on the next launch.

For local development, override the discovery URL itself (point it at a local web server serving a development discovery document) rather than overriding the static repository base URL directly. Keeps the production code path identical to dev; one less divergence to debug.

## UniFFI surface

The UniFFI binding is the only place Swift sees Rust. The boundary is intentionally narrow, and the exports are free functions over statics, with no exported object:

- Cache: `set_cache_directory(cache_directory)` sets the cache root once, before any load (§Implementation: `FilesystemArtifactCache`).
- Bundle loads: `open_first_paint_bundle(embedded_directory)` opens the newest readable cached bundle, else the embedded bundle read from the app-bundle directory; `load_live_bundle(discovery_url, static_repository_base_url)` fetches, verifies, and caches the live bundle. Both are `async` (Swift sees `async throws`), both publish into `static PUBLICATION: OnceLock<watch::Sender<Arc<Bundle>>>`, and both return the published version label. The distribution context is the constant `DistributionContext::FirstParty`. No bundle bytes cross the FFI; only path and URL strings and the version label.
- Renderer lifecycle: `create_renderer()`, `attach_surface(handle: UiKitSurfaceHandle, width, height)`, `resize_surface(width, height)`, `detach_surface()`, `destroy_renderer()`. Synchronous, and called from Swift's main thread (§Rendering: MTKView + wgpu Metal). `UiKitSurfaceHandle { layer_ptr, view_ptr }` is a record converted to `shared::render::WindowHandle::UiKit`.
- Revision: `revision()` (§Build version provenance).
- Deferred to Phase A: per-frame draw, hit testing (`region_at_point`), and the period, statistic, pan, and zoom controls (§UniFFI: proc-macro form, dedicated FFI crate).
- Errors: every fallible export returns `Result<T, FfiError>` in Rust → throws `FfiError` in Swift. Per the project's error-strings preference, `FfiError` is a single-variant enum (`Failed { message: String }`); no per-failure typed variants.

Swift-side: choosing the cache directory and excluding it from backup, locating the embedded bundle directory in the app bundle, `MTKView` and its `UIViewRepresentable` bridge, gestures, redraw scheduling, navigation state, animation timing (when v2+ adds it), the entire SwiftUI view tree. Per overview §FFI dividing line, this is intentional and load-bearing.

`web/` and `ios/` are sibling crates over `shared`. The web crate has no FFI layer (per `client-web.md` §Workspace placement), because Leptos is itself Rust and calls `shared::*` directly as a normal Cargo dependency — there's no language boundary to mediate. Swift can't depend on `shared` as a Cargo crate, so the `ios` crate exposes a UniFFI surface over `shared` for `uniffi-bindgen-swift` to consume. `shared` and `web` carry no UniFFI dependency, so the web build never compiles UniFFI. The asymmetry follows from the language boundary actually existing on iOS and not existing on web; both are correct for their platform.

## App Store distribution

### Signing and CI

Per overview §Apple Developer Program, signing uses an **App Store Connect API key**. The key is generated under Users and Access → Keys in App Store Connect, downloaded once (it cannot be re-downloaded), and stored in the chosen CI service's secret store as three values:

- `APPSTORE_CONNECT_API_KEY_CONTENT` — the `.p8` private key contents, base64-encoded.
- `APPSTORE_CONNECT_API_KEY_ID` — the 10-character key identifier.
- `APPSTORE_CONNECT_API_KEY_ISSUER_ID` — the issuer UUID for the App Store Connect account.

The CI build runs `xcodegen generate` first (to materialize the project file), decodes the App Store Connect API key into a temporary file, exports `APPSTORE_CONNECT_API_KEY_PATH` for `xcodebuild -allowProvisioningUpdates` to consume, and invokes the build + export + upload chain. Modern Xcode (15+) lets `xcodebuild -exportArchive` upload directly via the App Store Connect API key, no `altool` step:

```sh
pushd ios > /dev/null
xcodegen generate
popd > /dev/null
xcodebuild -project ios/Eafora.xcodeproj \
           -scheme Eafora \
           -archivePath build/Eafora.xcarchive \
           -allowProvisioningUpdates \
           archive
xcodebuild -exportArchive \
           -archivePath build/Eafora.xcarchive \
           -exportOptionsPlist ios/ExportOptions.plist \
           -exportPath build/ \
           -allowProvisioningUpdates \
           -authenticationKeyPath "$APPSTORE_CONNECT_API_KEY_PATH" \
           -authenticationKeyID "$APPSTORE_CONNECT_API_KEY_ID" \
           -authenticationKeyIssuerID "$APPSTORE_CONNECT_API_KEY_ISSUER_ID"
```

`ios/ExportOptions.plist` declares `method = app-store-connect` and `destination = upload`; the second `xcodebuild` invocation both exports the `.ipa` and uploads it to App Store Connect in one step. No separate `altool` call needed.

#### Archive retention

CI does not retain `.xcarchive`s. They're produced as a byproduct of `xcodebuild archive`, immediately consumed by the upload step, and discarded.

The recovery path for crash-report symbolication months after a build shipped is `git checkout <revision> && xcodebuild archive`, where `<revision>` comes from the `EaforaRevision` value recorded in the binary's `Info.plist` (per §Build version provenance). The user's crash report carries the revision; we check out the matching source state; we rebuild; the rebuilt archive's `.dSYM` UUIDs match the original (assuming our build is deterministic enough — pinned Xcode + Rust toolchains, standard release profile). Symbolication proceeds normally.

The git-revision-in-binary plumbing is what makes archive retention unnecessary. Without it, we'd have to retain archives because there'd be no way to know which source state to check out for a given user-reported crash. With it, the archive becomes recoverable from source, so storing it is redundant.

This relies on build determinism that probably won't hold long-term — Xcode/Rust toolchain updates, dependency bumps, and Apple Silicon codegen non-determinism can all break it. When it breaks, the policy flips to "retain `.xcarchive`s for shipped builds." Cost: approx. 100 MB per archive, 1-2 archives per month at our cadence, low-single-digit GB/year — captured by the Mac mini's regular backup. Tracked in `docs/backlog.md` §Infrastructure / ops as "Retain `.xcarchive` files for shipped iOS builds when rebuild-from-source determinism breaks." Not paid for now.

Per the build-machine decision in overview §CI/CD, **CI runs on the owner's Mac mini M1 through v1**. The Mac mini natively builds iOS (no hosted macOS runner needed); the workflow tool (self-hosted GitHub Actions runner, Buildkite, or shell scripts on a launchd timer) is interchangeable.

### TestFlight

- Internal testing: up to 100 testers; no review; instant builds. The owner is the primary internal tester through v1.
- External testing: requires a brief beta review (~24–48 hours) before each new build is distributed to external testers. Used for invited feedback rounds before the public App Store launch.
- Build numbering: every CI-uploaded build increments the build number monotonically. The build number is computed from the Git commit count on the `master` branch (`git rev-list --count master`), so it auto-advances per merge.

### App Store review

Review takes approx. 24–48 hours for compliant apps. Common rejection causes for a map / data viz app:

- Misleading data: Eafora's per-cell provenance with retrieval timestamp + license (Constitution II) addresses this directly. Every datum is attributable.
- Claims of endorsement without evidence: addressed by Constitution I (no editorial copy) and by sticking to source-attributed data.
- Mishandling of politically contested borders: addressed by Constitution VI's US-recognized-borders default plus the boundary swap design (overview §Borders) for any future market that requires alternate boundaries.

The owner submits via a personal Apple Developer Program account — the same enrollment path any individual developer uses.

### Universal Links

Universal Links let `https://eafora.org/region/<region.code>[/<statistic.code>[/<year>]]` deep-link into the iOS app when installed. Setup has three pieces.

#### 1. Xcode capability

Add an Associated Domains capability to the Xcode project: `applinks:eafora.org`. Configured in `ios/project.yml`'s `entitlements` block; XcodeGen writes it to the generated `.entitlements` file at codegen time.

#### 2. AASA file deployed by the iOS pipeline

The Universal Links machinery requires the file `apple-app-site-association` to be served from `https://eafora.org/.well-known/apple-app-site-association` (no extension; `Content-Type: application/json`). The path is hardcoded by iOS; the host has to be `eafora.org` because that's the domain we claim.

The deploy mechanism: a tiny Workers Assets deploy that handles only this one path. It lives in `tools/aasa-deploy/` (not in the web tree; not in `ios/`; in the cross-cutting `tools/` directory). The deploy is **assets-only** — no Worker script, no fetch handler — because Cloudflare's edge serves static-asset requests directly when `wrangler.toml` declares `[assets]` without a `main` field. Cloudflare's route matching sends `eafora.org/.well-known/apple-app-site-association` to this deploy; everything else on `eafora.org` continues to the main web deploy. The web tree doesn't see this deploy; the iOS pipeline owns it.

`tools/aasa-deploy/`:

```
tools/aasa-deploy/
├── wrangler.toml                                 # name, route, asset directory
├── apple-app-site-association.template.json      # template with placeholders
└── README.md
```

`wrangler.toml`:

```toml
name = "eafora-aasa"
compatibility_date = "2026-06-01"

[[routes]]
pattern = "eafora.org/.well-known/apple-app-site-association"
custom_domain = false

[assets]
directory = "./build"
```

No `main`, no Worker script, no JS runs per request. The asset bundle is one file (the rendered AASA); the edge serves it directly.

`apple-app-site-association.template.json`:

```json
{
  "applinks": {
    "apps": [],
    "details": [
      {
        "appID": "{{ TEAM_ID }}.{{ BUNDLE_ID }}",
        "paths": ["/region/*", "/about"]
      }
    ]
  }
}
```

#### 3. Rendering the AASA file (iOS-scoped)

`ios/setup.sh` reads the canonical `TEAM_ID` and `BUNDLE_ID` from `ios/project.yml` via `yq` and renders the template into `tools/aasa-deploy/build/apple-app-site-association`:

```sh
#!/usr/bin/env sh
# ios/setup.sh — render iOS-derived files that other parts of the build need
set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
TEAM_ID=$(yq -r '.targets.Eafora.settings.base.DEVELOPMENT_TEAM' "$REPO_ROOT/ios/project.yml")
BUNDLE_ID="$(yq -r '.options.bundleIdPrefix' "$REPO_ROOT/ios/project.yml").$(yq -r '.targets.Eafora.name' "$REPO_ROOT/ios/project.yml")"

rm -rf "$REPO_ROOT/tools/aasa-deploy/build"
mkdir -p "$REPO_ROOT/tools/aasa-deploy/build"
sed "s/{{ TEAM_ID }}/$TEAM_ID/g; s/{{ BUNDLE_ID }}/$BUNDLE_ID/g" \
    "$REPO_ROOT/tools/aasa-deploy/apple-app-site-association.template.json" \
    > "$REPO_ROOT/tools/aasa-deploy/build/apple-app-site-association"
```

This script lives in `ios/` because reading from `ios/project.yml` is iOS-scoped knowledge. Run by the top-level `setup.sh` as part of first-time setup; rerun manually if `project.yml` changes.

#### 4. Deploying the AASA file (cross-cutting infra)

`scripts/deploy-aasa.sh` deploys whatever's in the build output:

```sh
#!/usr/bin/env sh
# scripts/deploy-aasa.sh — deploy the AASA Worker; expects `ios/setup.sh` to have rendered the file
set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)

if [ ! -f "$REPO_ROOT/tools/aasa-deploy/build/apple-app-site-association" ]; then
    echo "error: AASA file not rendered. Run ios/setup.sh first." >&2
    exit 1
fi

pushd "$REPO_ROOT/tools/aasa-deploy" > /dev/null
wrangler deploy
popd > /dev/null
```

CI runs `ios/setup.sh && scripts/deploy-aasa.sh` once on TestFlight and App Store builds (the AASA contents are functionally static post-enrollment; redeploying on every iOS build is wasted work). Local dev: render once after the developer account is enrolled and `DEVELOPMENT_TEAM` is set in `project.yml`; deploy when the values change.

The split: rendering is iOS-scoped (consumes iOS source of truth), deploying is infrastructure (Cloudflare wrangler invocation, same shape as any other Worker deploy). Each script does one thing.

`ios/project.yml` is the canonical source of truth for both Xcode (uses the values directly) and the AASA file (renders them via `yq`). The duplication that would otherwise exist between `project.yml` and a hand-edited AASA file is gone; the rendered file is gitignored, regenerated reproducibly. If we ever change bundle ID or team ID, one edit in `project.yml` propagates to both halves.

#### 5. Routing in the app

The app's `EaforaApp.swift` handles incoming URLs via `.onOpenURL { ... }` and routes by setting the appropriate sheet binding: a `/region/...` URL sets `selectedRegion = .some(RegionCode("..."))` (presents the region-detail sheet); an `/about` URL sets `settingsPresented = true` (presents the Settings sheet, where About is the first section visible without scrolling). SwiftUI presents the corresponding sheet on the next render.

A region URL carries up to two more segments, the statistic code and the year, per `client-web.md` §Routing and SSG. Both are optional and both are view state rather than identity, so an absent, unrecognized, or uncovered value resolves to the default the map opens on instead of failing; only the region code can fail the link. The AASA `paths` entry stays `/region/*`, whose `*` is a substring wildcard and so already spans the longer forms.

### Domain and email

Per overview §Domain and email, the production domain is `eafora.org`, registered through Cloudflare. Universal Links sit on the apex; the artifact CDN at `repository.eafora.org` is invisible to App Store users.

## Testing strategy

Per Constitution Principle VII, the iOS-only TDD-required surfaces are:

- MTKView ↔ Rust surface bridge: assert the surface's reported size matches the MTKView's `drawableSize`; assert resize events propagate. iOS simulator.
- `EmbeddedBundle.swift` contract: assert it locates the `embedded_artifacts` directory in `Bundle.main` and that `openFirstPaintBundle` opens a bundle from the path it passes.
- Universal Link routing: assert that an incoming `https://eafora.org/region/usa` URL sets `selectedRegion` to `RegionCode("usa")` and the region-detail sheet presents with that region; assert the same from a fresh launch and from a backgrounded resume. Assert that `https://eafora.org/about` sets `settingsPresented = true` and the Settings sheet presents (About is the top section, visible without scrolling).

Cross-platform surfaces (manifest parsing, SHA-256 verification, license-class authorization, FlatGeobuf hit testing, the `FilesystemArtifactCache` contract, and the loader's fetch and eviction behavior) are tested in `shared/` once and not re-tested per platform. See `client.md` §Testing strategy.

XCUITest (UI automation) is **not** in scope for the foreseeable future (through v3+). The visual ground truth lives in `docs/design/stub-mobile.html`; parity is checked manually against the stubs before review submission. The cost of a full UI-automation suite (test maintenance, simulator flakiness, CI time) exceeds the value of automating what a manual check already catches at Eafora's surface area.

## Things to verify

1. `uniffi-bindgen-swift` exact flag set: settled in `scripts/build/build-ios-xcframework.sh` against the workspace's pinned `uniffi`: separate calls for `--swift-sources`, `--headers`, and `--modulemap --module-name eafora_iosFFI --modulemap-filename module.modulemap`, without `--xcframework`.
2. **wgpu surface creation from a `CAMetalLayer`**: settled in `shared/src/render/surface.rs`, which builds the surface target from `WindowHandle` and calls `Instance::create_surface_unsafe` against the wgpu version pinned in the workspace `Cargo.toml`.
3. **MTKView `isPaused` + `setNeedsDisplay()` semantics** — confirm against current MTKView docs that this combination drives the on-demand-only render loop without periodic GPU wakeups.
4. **`xcodebuild -create-xcframework` flag form** — the `-library` + `-headers` repetition for multiple slices has been stable since 2020 but is worth a spot-check against current Xcode docs.
5. `xcodebuild -exportArchive` direct-upload flow — verify `ExportOptions.plist`'s `destination = upload` key + the `-authenticationKey*` flag spellings against the Xcode version pinned in CI. The two-call chain (archive → exportArchive-with-upload) replaces the older three-call chain (archive → exportArchive → altool); spot-check the modern shape works end-to-end against the current Xcode before relying on it. Fallback if it doesn't: separate `xcodebuild -exportArchive` (export only) + `xcrun altool --upload-package`, even though `altool` is deprecated.
6. AASA Worker route — `tools/aasa-deploy/wrangler.toml`'s route pattern (`eafora.org/.well-known/apple-app-site-association`) needs to take precedence over the main web Worker's catch-all route. Cloudflare's route-matching rules are documented but worth a spot-check after the first deploy that hitting the URL serves the AASA Worker, not the web Worker. Also confirm the served `Content-Type` is `application/json` and that no extension appears on the path.

## Follow-up work

- The iOS client feature (`specs/004-ios-client/`) has landed the `ios` crate and the xcframework build pipeline; Phase A lands the Xcode project, the SwiftUI shell with `MapView` + `RegionDetailView`, the cache-directory setup, and the embedded artifact location, enough to render the static-stub-equivalent of `docs/design/stub-mobile.html` against real data on the iOS simulator.
- Initial Apple Developer Program enrollment is a prerequisite. TestFlight internal testing can begin as soon as enrollment completes; external testing follows after a beta review.

Deferred-but-not-blocking iOS work lives in `docs/backlog.md` §Client (currently empty) once items earn deferral as concrete work.
