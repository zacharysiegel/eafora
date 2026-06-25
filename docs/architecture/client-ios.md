# iOS client architecture

> **Status: draft, 2026-06-17.** This document is the per-platform deep-dive companion to `docs/architecture/client.md` (cross-cutting client architecture) and `docs/architecture/overview.md` (system overview), and a sibling to `docs/architecture/client-web.md` (web client). It covers everything specific to the **iOS** client surface: the native Xcode project, the Rust core's xcframework binary product, the UniFFI binding generation, the SwiftUI + MTKView render bridge, the `Library/Caches`-backed artifact cache, the in-bundle embedded artifact, the App Store distribution flow, and the testing strategy. The sibling `client-android.md` lags iOS per overview §Per-platform v1 build vs iteration scope.

## Scope of this document

This document covers everything between **the consumer-side contract `client.md` defines** and **a signed `.ipa` on TestFlight or the App Store**:

- The `ios/` directory: a native Xcode project (not a Cargo crate) that consumes a Rust-produced xcframework.
- The xcframework build pipeline: `cargo build` for `aarch64-apple-ios` (device) and `aarch64-apple-ios-sim` (simulator); `xcodebuild -create-xcframework`; UniFFI-generated Swift bindings via the proc-macro form; the Run Script build phase that ties them together.
- The SwiftUI shell: navigation, the design-token mapping from `docs/design/README.md` to a Swift extension, the MTKView + `UIViewRepresentable` map surface, the bottom-sheet region detail.
- The platform glue: the `URLSession`-backed fetch adapter, the `FileManager`-backed `Library/Caches/` artifact cache, the bytes-in-`Resources/` embedded bundle.
- The UniFFI binding boundary: what crosses the FFI seam, what stays Swift-side, the async + error mapping conventions.
- App Store distribution: signing via App Store Connect API key, TestFlight, Universal Links.
- Testing strategy for the iOS-only TDD-required surfaces.

Cross-cutting client behavior (artifact-consumption contract, fetch / cache / load pipeline shape, SQLite-in-the-client, FlatGeobuf reading, license-shard composition, embedded bundle semantics, hot-swap protocol) is in `client.md` and is **not relitigated here**. The visual identity is in `docs/design/README.md` and is also not relitigated.

## Locked decisions referenced (not relitigated)

From the constitution, `docs/architecture/overview.md`, `docs/architecture/client.md`, and `docs/design/README.md`:

- UI framework: SwiftUI. (Overview §iOS client; Constitution III)
- Map surface: `MTKView` wrapped in `UIViewRepresentable`; render loop on main thread; heavy compute offloaded to background tasks before the next frame. (Overview §iOS client)
- Rust integration: `core` is built as an xcframework; UniFFI generates Swift bindings via the **proc-macro form** (`#[uniffi::export]` annotations on Rust items in a dedicated `core/src/ffi/uniffi.rs` module), not the declarative UDL form. (Overview §UDL vs proc-macro for UniFFI)
- GPU baseline: Apple A14 / A15 (iPhone 12 / 13 generation, 2020–2021) and later; iOS 18+ minimum SDK target. (Overview §iOS client)
- Async: Swift's `async`/`await` consumes UniFFI async functions; cancellation is one-way (Swift task cancellation does not propagate; Rust must self-cancel based on a polled flag if cancellation matters). (Overview §iOS client; §FFI design rules)
- HTTP: `URLSession`. (Overview §iOS client; §FFI dividing line)
- Cache: file system inside the app sandbox. (Overview §Client cache strategy)
- Embedded bundle on native: bytes baked into the app binary at build time; loads synchronously before the first frame; doubles as the offline-capable baseline. (Client §Embedded downsampled artifact)
- Web and iOS develop in parallel from v1, deliberately, to prevent the architecture from overfitting to the web platform's constraints. Android lags. The native apps double as personal-learning goals for the parallel game project; for funder pitches, only the web is the user-facing v1 deliverable. (Project memory)
- Apple Developer Program: $99/year; App Store Connect API key for CI; TestFlight for testing; ~24–48 hour App Store review. (Overview §App store distribution)
- Visual identity: sharp white-paper-with-red-ink, square corners (≤1px radius), 1px borders, no shadows, no gradients, no animations through v1. (`docs/design/README.md`)
- No live API through v2: every datum the user sees came from a versioned CDN artifact. (Constitution VI)

## Workspace placement

The iOS client is a native Xcode project at `ios/`. It is **not** a Cargo crate; it consumes a Rust-produced xcframework that bundles `core/`'s static libraries plus UniFFI-generated Swift bindings.

```
eafora/
├── core/                                     # the shared Rust core (consumer surface)
├── ios/                                      # this document's subject
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
│   │   │   ├── MapRenderer.swift             # CAMetalLayer + drawable lifecycle, calls into core
│   │   │   ├── LegendView.swift              # choropleth legend overlay
│   │   │   └── ControlsView.swift            # statistic picker, year scrubber, source panel
│   │   ├── Region/                           # region detail (a destination — region = any level of the region hierarchy: country, subregion, supranational, etc.)
│   │   │   ├── RegionDetailView.swift
│   │   │   └── HistoryChartView.swift
│   │   ├── SettingsView.swift                # bottom-sheet Settings; About inlined at top, utility rows below, build info at bottom (no separate AboutView through v1)
│   │   ├── FileSystemArtifactCache.swift     # implements the cache contract from core::artifact
│   │   ├── URLSessionFetcher.swift           # URLSession-based fetch; bridges to core::artifact loader
│   │   └── EmbeddedBundle.swift              # locates and reads the bundled embedded artifact
│   ├── EaforaAppTests/                       # XCTest unit tests
│   ├── EaforaAppUITests/                     # XCUITest UI tests (deferred per §Testing strategy)
│   └── README.md                             # quickstart for iOS development
```

The Swift code is organized by feature: directory only when a feature has 2+ files (`Map/`, `Region/`); single-file features sit flat under `EaforaApp/`. Mirrors the same convention web uses. Shared shell code (design tokens, navigation root) sits at the `EaforaApp/` top level.

Directory names use PascalCase (`Map/`, `Region/`), matching iOS convention. The web crate uses lowercase (`map/`, `region/`) because Rust's convention is lowercase modules; both follow their host language's idiom rather than enforcing project-wide uniformity.

The `core` crate is **not** referenced from the iOS project directly. Instead, the project references `target/uniffi/EaforaCore.xcframework` as a binary dependency (declared in `ios/project.yml`); the xcframework is built by the pipeline described in §Build toolchain. The xcframework lives under `target/` like every other generated artifact in the workspace; it is not committed and is rebuilt on demand.

## Build toolchain

### xcframework build pipeline

The Rust core ships as an xcframework: a single `.xcframework` bundle containing static libraries for every Apple target slice plus the matching Swift module headers. Build flow:

1. `cargo build --target aarch64-apple-ios --release` → produces `target/aarch64-apple-ios/release/libcore.a` (device slice).
2. `cargo build --target aarch64-apple-ios-sim --release` → produces `target/aarch64-apple-ios-sim/release/libcore.a` (Apple-Silicon-Mac simulator slice).
3. Generate Swift bindings from the compiled library's UniFFI metadata. Use the dedicated `uniffi-bindgen-swift` binary (separate from the generic `uniffi-bindgen`; gives finer-grained control over Swift-specific artifacts like xcframework-compatible modulemaps). Three separate invocations to produce each artifact independently:

   ```sh
   cargo run -p uniffi-bindgen-swift -- target/aarch64-apple-ios/release/libcore.a target/uniffi-swift --swift-sources
   cargo run -p uniffi-bindgen-swift -- target/aarch64-apple-ios/release/libcore.a target/uniffi-swift/Headers --headers
   cargo run -p uniffi-bindgen-swift -- target/aarch64-apple-ios/release/libcore.a target/uniffi-swift/Modules --xcframework --modulemap --modulemap-filename module.modulemap
   ```

   `uniffi-bindgen-swift` is a tiny binary we define in the workspace (a `[[bin]]` containing `fn main() { uniffi::uniffi_bindgen_swift() }`). The `--xcframework` flag is what makes the modulemap suitable for the `xcodebuild -create-xcframework` step that follows.
4. `xcodebuild -create-xcframework -library target/aarch64-apple-ios/release/libcore.a -headers target/uniffi-swift/Headers -library target/aarch64-apple-ios-sim/release/libcore.a -headers target/uniffi-swift/Headers -output target/uniffi/EaforaCore.xcframework` → produces the binary product under `target/` alongside the rest of the build outputs.
5. `target/uniffi/EaforaCore.xcframework` is referenced as a binary framework dependency in `ios/project.yml` (via the relative path `../target/uniffi/EaforaCore.xcframework`), picked up by the regenerated `Eafora.xcodeproj`.

The pipeline is encapsulated in `scripts/build-ios-xcframework.sh`, checked into the repo. Invoked:

- As an Xcode Run Script build phase before the "Compile Sources" phase, so opening the project in Xcode and building rebuilds the xcframework if the Rust source has changed.
- In CI explicitly before `xcodebuild build`.

`setup.sh` does not invoke it — `setup.sh` sets up the environment (install toolchains, run `xcodegen generate` to produce the project file, decrypt secrets), not perform builds. The first build after a fresh clone runs `build-ios-xcframework.sh` for the first time via Xcode's pre-build phase; that's where the compilation happens.

The Run Script build phase is conservative: it invokes the shell script unconditionally and lets the script's internal cache + Cargo's incremental compilation determine whether work actually happens. A no-op rebuild after the first run takes approx. 5–10 seconds — acceptable overhead for the every-build correctness guarantee.

The xcframework itself is **gitignored**. Every build produces it from source; staleness is impossible.

### UniFFI: proc-macro form, dedicated FFI module

Per overview §UDL vs proc-macro for UniFFI, Eafora uses the **proc-macro form**: `#[uniffi::export]` annotations on Rust items, no separate `.udl` file. The discipline that makes this work is a **dedicated FFI module** at `core/src/ffi/uniffi.rs` that imports types from internal modules and either re-exports them with annotations or wraps them in thin FFI-facing adapter types when the internal shape isn't the right contract for the boundary.

The module is the single reviewable surface for "what the iOS and Android apps see." A PR that touches it changes the FFI; a PR that doesn't touch it doesn't.

Sketch of `core/src/ffi/uniffi.rs`:

```rust
use crate::artifact::Bundle;
use crate::map::{FrameState, RegionCode, ScreenPoint, Viewport};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum AppError {
    #[error("{message}")]
    GenericError { message: String },
}

#[derive(uniffi::Object)]
pub struct EaforaCore {
    // internal fields not exposed across the FFI; surface lives here once attached
    inner: std::sync::Mutex<EaforaCoreInner>,
}

#[uniffi::export]
impl EaforaCore {
    #[uniffi::constructor]
    pub fn new(artifact_path: String) -> Result<std::sync::Arc<Self>, AppError> { /* ... */ }

    pub fn attach_surface(&self, handle: WindowHandle, width: u32, height: u32) -> Result<(), AppError> { /* ... */ }
    pub fn detach_surface(&self) -> Result<(), AppError> { /* ... */ }
    pub fn resize_surface(&self, width: u32, height: u32) -> Result<(), AppError> { /* ... */ }

    pub fn draw_frame(&self, viewport: Viewport, frame_state: FrameState) -> Result<(), AppError> { /* ... */ }

    pub fn region_at_point(&self, viewport: Viewport, point: ScreenPoint) -> Option<RegionCode> { /* ... */ }

    pub async fn push_bundle(&self, manifest_path: String) -> Result<(), AppError> { /* ... */ }

    pub fn revision(&self) -> String { crate::REVISION.to_string() }
}

// Platform-agnostic window-handle enum. Each platform's shell constructs the matching variant;
// Rust unwraps and converts into raw-window-handle's RawWindowHandle to build a wgpu::Surface.
// Same FFI method on every platform; only the value's variant differs.
#[derive(uniffi::Enum)]
pub enum WindowHandle {
    UiKit { layer_ptr: u64, view_ptr: u64 },          // iOS: CAMetalLayer + UIView pointers
    AndroidNdk { native_window_ptr: u64 },             // Android: ANativeWindow pointer
}

// re-export internal types that are part of the FFI surface
#[derive(uniffi::Record)]
pub struct Viewport {
    pub longitude_min: f64,
    pub longitude_max: f64,
    pub latitude_min: f64,
    pub latitude_max: f64,
}

#[derive(uniffi::Record)]
pub struct ScreenPoint {
    pub x: f64,
    pub y: f64,
}
// ...etc for FrameState, RegionCode, etc.
```

The intent: opaque `EaforaCore` handle + concrete request / response types + a single `AppError` thrown on failure. Generic types and trait objects are absent (UniFFI doesn't support them). Per the project's error-strings-over-enums preference, `AppError` carries a `message` payload rather than a per-failure-mode variant; callers that need to branch (e.g. quota-exceeded vs. opfs-unsupported on the web side) match on a documented prefix in the message body.

`uniffi-bindgen-swift` reads the metadata that the proc-macros embed in the compiled `libcore.a` and produces idiomatic Swift: an `EaforaCore` class with a constructor and methods; `throws` for fallible methods; `async` for `async fn` Rust functions; optionals (`RegionCode?`) for `T?` returns. Swift `do` / `try` / `catch` is the call-site idiom.

When a type's internal shape isn't quite right for the FFI (e.g. the internal `Bundle` carries `Arc` references that don't cross the boundary cleanly), the FFI module defines a thin adapter struct alongside the `#[uniffi::Record]` — wrapping or projecting the internal type's relevant fields. The adapter is the FFI surface; the internal type stays free to evolve.

### Build profile

The xcframework build invokes `cargo build --target aarch64-apple-ios --release` (and the simulator equivalent). The standard `[profile.release]` settings apply (including the workspace-wide `panic = "abort"` — see overview §Workspace Cargo profile); no additional iOS-specific tuning.

Binary size doesn't justify a custom profile on iOS. The whole app ships once at install time and updates infrequently; users don't watch the size on every launch the way a web client downloads its WASM. The few megabytes a size-trade like `opt-level = "z"` would save are invisible to users, while the runtime cost (less aggressive inlining, slower hot paths) is real even if small. Standard release optimization wins.

Web is the asymmetric case: every cold-cache page load downloads the WASM, so shaved KB are shaved network-transfer time on first paint — directly user-visible. Web's `[profile.wasm-release]` (see `client-web.md` §`wasm-opt`) does use `opt-level = "z"` for exactly that reason. iOS and Android don't.

Expected size of `libcore.a` post-strip: roughly 8-12 MB per slice, dominated by `wgpu` + `flatgeobuf` + `rusqlite`'s sqlite3 bytes. The xcframework is the union of slices; `xcframework` deduplicates internally to the extent possible. App Store thinning at install time delivers only the architecture the device uses.

### Xcode integration

The Xcode project file is **generated** from `ios/project.yml` via [XcodeGen](https://github.com/yonaskolb/XcodeGen), not hand-edited or committed. `xcodegen generate` produces `ios/Eafora.xcodeproj/`; the directory is gitignored. Anyone (including CI) regenerates it from the YAML on demand. Editing the project means editing `project.yml` in your text editor, then re-running `xcodegen generate`.

This avoids:

- Hand-edited XML in the project file (the format is opaque, version conflicts on it are painful, and small Xcode actions can rewrite huge swaths unpredictably).
- The need to open Xcode to add a source file or change a build setting.

`setup.sh` installs the iOS-side toolchain (`xcode-select --install`, `brew install xcodegen`, `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`) and runs `xcodegen generate` to produce the initial project file. Setup is environment + project-file-materialization only; it does not compile or build (see §xcframework build pipeline for why building happens via Xcode's Run Script phase instead).

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
      - framework: ../target/uniffi/EaforaCore.xcframework
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
      - name: Build EaforaCore xcframework
        script: ../scripts/build-ios-xcframework.sh
      - name: Sync embedded artifacts
        script: ../scripts/sync-embedded-bundle.sh ${SRCROOT}/EaforaApp/Resources/embedded_artifacts/
      - name: Inject git revision
        script: ../scripts/inject-git-revision.sh
```

Configuration the YAML expresses:

- Deployment target: iOS 18.0 (per overview §iOS client).
- Architectures: `arm64` only (Apple Silicon; armv7 is not built).
- Frameworks: `EaforaCore.xcframework` linked against; `MetalKit.framework` and `Metal.framework` linked for the MTKView path.
- Pre-build scripts: rebuild the xcframework on demand, then sync the embedded bundle into the app's `Resources/`, then inject the source revision into `Info.plist`.
- Code-signing style: automatic for Debug (Xcode picks any installed cert that matches), manual for Release (uses a named distribution cert + provisioning profile created in App Store Connect). The Debug path is what developers use day-to-day; the Release path is what CI uses for App Store / TestFlight uploads.
- `DEVELOPMENT_TEAM` is the 10-character Apple Developer Team identifier. One value, shared across every machine that builds the app (your developer Mac, the Mac mini CI). What differs per machine is which signing certificate is in the keychain — your dev machine has a Development cert; CI has a Distribution cert — and whether the App Store Connect API key for upload automation is installed (CI only). All of those live under the same team. Not a secret; the team ID appears in every provisioning profile inside the shipped `.ipa` and in the public `apple-app-site-association` file. Committed directly in `project.yml`. The actual auth lives in the certificates and the App Store Connect API key (per §Signing and CI, treated as a real secret).

Anything that genuinely needs Xcode (asset catalog edits for the app icon, one-time signing setup with the developer account, occasional debugging) still gets done in Xcode — open the generated `Eafora.xcodeproj`, do the thing, save what you can save back into source-controlled files (`Info.plist`, asset catalogs); changes to the project file itself are pointless because regeneration overwrites them.

A distinction worth naming: the project bundle (`Eafora.xcodeproj/`, a directory presented as a single file in Finder; macOS calls this a "package") is generated and gitignored. The asset *catalog* (`Assets.xcassets/`, also a package) is content — Xcode-editable JSON files under that directory — and is **committed**. Same applies to `Info.plist`. The "no hand-editing in Xcode" rule applies only to project structure (targets, build phases, build settings); the rest of what Xcode lets you edit (catalogs, plists, code) stays normal hand-or-Xcode-edit-then-commit. The `Resources/embedded_artifacts/` directory is a third category — opaque-bytes asset that lives at the bundle root, gitignored, regenerated by `scripts/sync-embedded-bundle.sh`. We use the asset catalog for app-icon-shaped content (multi-resolution variants, accent colors) and raw `Resources/` for the embedded bundle (just files; the asset catalog has no idea what a `manifest.json` or a `world-50m-*.fgb` is).

### Build dependency direction

Identical to the web side: the iOS build **pulls** the static-asset embedded bundle from the producer's downsampled output via `scripts/sync-embedded-bundle.sh`, never the producer pushes into `ios/EaforaApp/Resources/`. The script takes the destination directory as its first argument and plain-copies (`cp -R`) the contents of `$EAFORA_DOWNSAMPLED_DIR/latest/` into it. Same script as web, different argument.

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

#### Core FFI (runtime surface)

The Rust core exposes its own revision via UniFFI. Used for anything the running app needs to **act** on the version: error-message annotations, log lines, server-reported-minimum-version comparisons. `core/build.rs` captures the revision at compile time:

```rust
// core/build.rs
use std::process::Command;

fn main() {
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=EAFORA_REVISION={}", revision);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}

// core/src/lib.rs
pub const REVISION: &str = env!("EAFORA_REVISION");

// core/src/ffi/uniffi.rs
#[uniffi::export]
pub fn revision() -> String {
    crate::REVISION.to_string()
}
```

Swift sees `eafora.revision()` (top-level function in the generated bindings).

#### Why two values

These two facts can diverge. The Info.plist `EaforaRevision` is "what was the source state when this Xcode build ran"; `core::REVISION` is "what was the source state when this Rust core compiled." Almost always identical, but in development they can drift if one side is rebuilt without the other. Surfacing them as separate values means the divergence is visible when it matters; conflating them into one would hide a real signal.

The name "revision" rather than "sha" is deliberate. The value is a git SHA today, but the *concept* the app cares about is "what source state did this binary come from"; if we ever migrate version control systems (Jujutsu, Mercurial, Pijul, anything else), the script changes, the displayed value changes, the consumer-facing name doesn't.

## Rendering: MTKView + wgpu Metal

iOS rendering uses `MTKView` as the render surface and wgpu's Metal backend underneath. The bridge:

1. SwiftUI declares a `UIViewRepresentable` wrapping `MTKView`.
2. `MTKView`'s delegate (`MTKViewDelegate`) is the SwiftUI-side `MapRenderer` Swift class.
3. When the MTKView enters the window hierarchy and its `CAMetalLayer` becomes available, the `MapRenderer` calls `eaforaCore.attachSurface(handle: .uiKit(layerPtr: ..., viewPtr: ...), width: ..., height: ...)`. Rust constructs a `wgpu::Surface` against the layer and stores it on `EaforaCore` for the lifetime of the view. **One-time call, not per-frame.**
4. On `mtkView(_:drawableSizeWillChange:)`, the `MapRenderer` calls `eaforaCore.resizeSurface(width: ..., height: ...)`. Rust reconfigures the wgpu surface to the new size.
5. On `setNeedsDisplay()` triggering a `draw(in:)` callback, the `MapRenderer` calls `eaforaCore.drawFrame(viewport: ..., frameState: ...)`. Rust pulls the current frame's texture from its persistent surface (`surface.get_current_texture()`), encodes wgpu draw commands, submits to the Metal queue, presents.
6. When the MTKView leaves the hierarchy (view torn down, scene phase changes), the `MapRenderer` calls `eaforaCore.detachSurface()`. Rust drops the wgpu surface; the rest of `EaforaCore` (bundle, pipelines, device) lives on.

The Swift-to-Rust attach call hands a `WindowHandle.uiKit(layerPtr:, viewPtr:)` value — a UniFFI-marshaled enum carrying the platform-specific pointers as `u64`. Internally, Rust reconstructs `raw-window-handle`'s `RawWindowHandle::UiKit(...)` variant from the enum data and hands that to wgpu's surface constructor. Same FFI method (`attach_surface`) on every platform; the platform variation lives in the value the shell constructs (UiKit on iOS, AndroidNdk on Android), not in the FFI signature. (Verify the exact wgpu surface-creation API against the version pinned in `core/`'s Cargo.toml; the Metal-from-CAMetalLayer path is stable in wgpu but the function name has shifted across releases.)

The renderer's wgpu device, queue, and pipeline state are constructed once at `EaforaCore::new` time. The surface is attached later when the view is ready. The two-phase setup is necessary because the platform render target doesn't exist at app init; everything else does.

Per `docs/design/README.md`, **no animations through v1**. Rendering is **event-driven**, not loop-driven: `MTKView.isPaused = true` plus `setNeedsDisplay()` on every state change. State changes — selection, hover, statistic-picker, year-scrubber drag, bundle hot-swap — invalidate the view; MTKView coalesces multiple invalidations between vsyncs into one draw call at the next vsync; the GPU stays idle when nothing is happening. The same shape `client-web.md` §Client-side map view describes for web (dirty flag + `requestAnimationFrame`), expressed in iOS's native vocabulary.

When v2+ adds animations (per `docs/design/README.md` §Animation — under 150ms, linear easing, snap-don't-glide), the animation handler self-perpetuates a chain of `setNeedsDisplay()` calls until the animation ends, then stops. Continuous render rate during the animation; idle again after. Same pattern client-web.md describes for animation handling there.

### GPU baseline

Per overview §iOS client, the deployment target is iOS 18 + Apple A14 / A15 minimum. Metal feature levels at this baseline are uniformly modern: argument buffers tier 2, indirect command buffers, programmable blending, etc. wgpu's Metal backend abstracts over the version differences automatically; the renderer (in `core::map::map_renderer`) does not branch on Metal feature levels.

### Threading

Unlike the web (single-threaded WASM), iOS supports full multithreading. The Rust core's tokio runtime can use `features = ["full"]` on this target. Practical use:

- Render loop: main thread, driven by MTKView's display link.
- Live-bundle fetch: a background task spawned from the `MapRenderer` constructor, running on a tokio worker thread. The fetched bytes are handed to `core::artifact::Bundle` and published via `tokio::sync::watch::Sender<Arc<Bundle>>`.
- Subnational geometry parsing (v2+): another background task; published through the same watch channel as a partial-update.

Per `client.md` §Bundle hot-swap, in-flight queries holding an old `Arc<Bundle>` complete against the old bundle; the swap is wait-free.

The Swift side does not see tokio. It calls async UniFFI functions (`async fn` in Rust → `async` in Swift), and Swift's structured concurrency owns the Swift-side task lifecycle. Cancellation is one-way: cancelling a Swift `Task` does not cancel the Rust async future. Long-running Rust futures must self-poll a cancellation flag if the user-visible operation has a "Cancel" button (none through v1).

**Open item — awaiting `Bundle::open` on the multi-threaded runtime.** `shared::artifact::ArtifactCache` deliberately has no `Send` bound on its async functions: the web's `OpfsArtifactCache` holds `!Send` `JsValue`, and one trait serves every platform (see `shared/src/artifact/cache.rs`). So `Bundle::open(&dyn ArtifactCache)` returns a `!Send` future that cannot be `tokio::spawn`'d directly onto the full multi-threaded runtime. The live-bundle fetch task above must therefore run `Bundle::open` on a current-thread / `tokio::task::LocalSet` task (or a dedicated loader thread) and publish only the finished `Arc<Bundle>` — which IS `Send + Sync` — across the watch channel. The trait and `Bundle` shapes are fixed in 005; pick the exact spawn mechanism when 004 implementation begins.

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
- No support-version branching: every supported iOS version (iOS 18+) has the same `FileManager` API. There is no equivalent of the OPFS-unsupported fallback path.
- `NSURLIsExcludedFromBackupKey` set on the `artifacts/` directory at first creation; the cache contents are reproducible from the CDN and don't belong in iCloud / iTunes backup.

### Implementation: `FileSystemArtifactCache.swift`

The cache adapter on iOS is a Swift class implementing the same contract `core::artifact` defines. Reading and writing happens entirely Swift-side via `FileManager` + `Data(contentsOf:)` / `Data.write(to:)`; the byte buffers are then handed into Rust through UniFFI on demand. This avoids a Rust-Swift boundary cross per byte and keeps file-system access on the platform's native API.

The cache contract surface from Swift's perspective:

```swift
protocol ArtifactCache {
    func put(versionLabel: String, fileRelativePath: String, bytes: Data) async throws
    func get(versionLabel: String, fileRelativePath: String) async throws -> Data?
    func listVersions() async throws -> [String]
    func deleteVersion(versionLabel: String) async throws
}
```

`FileSystemArtifactCache` implements this against `Library/Caches/artifacts/`. Errors propagate as Swift `throws`; the caller (the UniFFI wrapper around the loader in `core::artifact`) catches and converts to the Rust `AppError` shape the loader expects.

Eviction policy from `client.md` §Cache eviction (keep current + most-recent prior) is implemented in `evictOldVersions()`, called at app launch after the cache initializes. Enumerates version subdirectories, picks the two most recent by `<version_label>` lexicographic order (correct because of the `YYYY-MM-DD+<surname>` shape), deletes the rest.

## Embedded bundle: app bundle Resources

Per `client.md` §Embedded downsampled artifact, native clients ship the embedded bundle as bytes baked into the app binary at build time. On iOS:

- The downsampled output (`manifest.json` + `geometry/` + statistic shards) is copied into `ios/EaforaApp/Resources/embedded_artifacts/` by `scripts/sync-embedded-bundle.sh` (Run Script build phase 2; see §Build toolchain).
- The "Copy Bundle Resources" build phase copies that directory into the `.app` bundle.
- At app launch, `EmbeddedBundle.swift` locates the bundle root via `Bundle.main.url(forResource: "embedded_artifacts", withExtension: nil)`, reads the manifest, and constructs a `core::artifact::Bundle` synchronously **before the first frame is drawn**.
- The renderer's `tokio::sync::watch::Sender<Arc<Bundle>>` is initialized with the embedded bundle. The map renders its first frame against the embedded data within milliseconds of process start.
- In parallel, `EaforaApp.swift` kicks off the discovery + live-fetch flow defined in `client.md` §Discovery and live bundle resolution: fire the discovery fetch (`https://eafora.org/discovery`) and a speculative manifest fetch (against the baked-in `repository_base_url` fallback) concurrently; reconcile per `client.md`; persist any verified bytes to the file-system cache; publish the resulting `Arc<Bundle>` to the renderer's watch channel. If both fetches fail, the embedded bundle remains the floor.
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
- Domain-content strings (region names, statistic names, source attributions, data-status labels) live in the SQLite shard built by ingestion, joined to upstream-source values by code. **Out of scope for this section** — the iOS app reads them via `core::*` queries; the i18n is producer-side.

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

Domain-content i18n (region names, etc.) is deferred per overview §FFI. The producer side adds translation columns to the seed-data tables when a second locale becomes a real deliverable; the iOS client reads them via `core::canonical::region_name(code, locale)`-shaped queries — same mechanism, different layer of the stack.

## URLSession fetch adapter

`URLSessionFetcher.swift` owns the platform-side fetch path. It mirrors the loader contract from `core::artifact`:

```swift
final class URLSessionFetcher {
    init(repositoryBaseURL: URL) { ... }

    func fetchManifest() async throws -> Data { ... }

    func fetchArtifactFile(versionLabel: String, relativePath: String) async throws -> Data { ... }
}
```

Concurrency cap: `URLSession.shared` defaults are reasonable for the per-platform cap (`client.md` §Stage 3 specifies 4 concurrent for native; configure `URLSessionConfiguration.httpMaximumConnectionsPerHost = 4` if the default exceeds it).

Retry: per `client.md` §Stage 3, the loader inside `core::artifact` owns the retry loop (approx. 100 ms / 400 ms backoff). The fetch adapter just propagates errors.

`repositoryBaseURL` is resolved at runtime via the discovery URL flow defined in `client.md` §Discovery and live bundle resolution: the app fetches `https://eafora.org/discovery`, reads `repository_base_url` from the response, and uses that for every shard fetch. A baked-in fallback (populated at build time by a small script that reads the current discovery doc and writes the value into a generated Swift constant) handles the case where discovery itself fails. The indirection earns its keep on iOS specifically — TestFlight and App Store installs live on devices for months or years, and an R2 re-platform without the discovery indirection would silently break every install in the field on the next launch.

For local development, override the discovery URL itself (point it at a local web server serving a development discovery document) rather than overriding `repositoryBaseURL` directly. Keeps the production code path identical to dev; one less divergence to debug.

## UniFFI surface

The UniFFI binding is the only place Swift sees Rust. The boundary is intentionally narrow:

- Lifecycle: `EaforaCore.init(artifactPath:)` constructs the core given a path to the embedded-bundle root. The core opens the manifest, parses it, opens any SQLite shards in memory, and is ready.
- Surface lifecycle: `attach_surface(handle: WindowHandle, width, height)` + `detach_surface()` + `resize_surface(width, height)`. The shell calls `attach_surface` once when the MTKView's layer becomes available, passing a `WindowHandle.uiKit` value containing the layer + view pointers; Rust constructs a `wgpu::Surface` against the layer and holds it for the view's lifetime. `resize_surface` reconfigures on size change. `detach_surface` drops the surface when the view goes away. The same `attach_surface` method serves Android: shell constructs `WindowHandle.androidNdk` instead. Single FFI signature; platform-specific data lives in the enum variant.
- Per-frame draw: `draw_frame(viewport, frame_state)` issues wgpu draw calls against the already-attached surface. Rust pulls the current texture from `wgpu::Surface::get_current_texture()`, encodes draw commands, submits, presents. No platform-specific data crosses the FFI per frame; the surface was attached once. No drawing instructions cross the FFI either — the renderer draws directly into the texture from its persistent surface.
- Hit testing: `region_at_point(viewport, point)` returns an optional `RegionCode`. Used for tap-to-select on the map.
- Live-bundle handoff: `push_bundle(manifest_path: String)` lets the Swift fetcher hand a freshly-cached live bundle to the core. Swift writes the fetched bytes to the cache directory at the manifest-declared paths first, then calls `push_bundle` with the absolute path to the manifest. Rust opens the manifest from disk, reads its `relative_path` entries, opens each shard relative to the manifest's directory, **validates SHA-256s** (this is the only verification — Swift doesn't duplicate it), constructs a `core::artifact::Bundle`, publishes it to the renderer's watch channel. Verification mismatch returns an error; Swift handles retry per the loader's retry policy. No bundle bytes cross the FFI — only a path string. (Resolve the exact UniFFI signature against `core/src/ffi/uniffi.rs` when implementing.)
- Errors: every fallible function returns `Result<T, AppError>` in Rust → throws `AppError` in Swift. Per the project's error-strings preference, `AppError` is a single-variant enum carrying a `message: String`; no per-failure typed variants.

Anything that doesn't need to cross the seam stays Swift-side: `URLSession`, `FileManager`, `MTKView`, navigation state, animation timing (when v2+ adds it), the entire SwiftUI view tree. Per overview §FFI dividing line, this is intentional and load-bearing.

Asymmetric with the web client: web has no `ffi/` directory and no `core::ffi::wasm` module (per `client-web.md` §Workspace placement), because Leptos is itself Rust and calls `core::*` directly as a normal Cargo dependency — there's no language boundary to mediate. iOS doesn't have that option: Swift can't depend on `core/` as a Cargo crate, so the only path is `core/src/ffi/uniffi.rs` exposing the Rust surface for `uniffi-bindgen-swift` to consume. The asymmetry follows from the language boundary actually existing on iOS and not existing on web; both are correct for their platform.

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

The recovery path for crash-report symbolication months after a build shipped is `git checkout <revision> && xcodebuild archive`, where `<revision>` comes from the `EaforaRevision` value baked into the binary's `Info.plist` (per §Build version provenance). The user's crash report carries the revision; we check out the matching source state; we rebuild; the rebuilt archive's `.dSYM` UUIDs match the original (assuming our build is deterministic enough — pinned Xcode + Rust toolchains, standard release profile, `panic = "abort"` workspace-wide). Symbolication proceeds normally.

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
- Mishandling of politically contested borders: addressed by Constitution VI's US-recognized-borders default plus the `core::boundary` swap design (overview §Borders) for any future market that requires alternate boundaries.

The owner submits via a personal Apple Developer Program account — the same enrollment path any individual developer uses.

### Universal Links

Universal Links let `https://eafora.org/region/<region.code>` deep-link into the iOS app when installed. Setup has three pieces.

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

The app's `EaforaApp.swift` handles incoming URLs via `.onOpenURL { ... }` and routes by setting the appropriate sheet binding: a `/region/<region.code>` URL sets `selectedRegion = .some(RegionCode("..."))` (presents the region-detail sheet); an `/about` URL sets `settingsPresented = true` (presents the Settings sheet, where About is the first section visible without scrolling). SwiftUI presents the corresponding sheet on the next render.

### Domain and email

Per overview §Domain and email, the production domain is `eafora.org`, registered through Cloudflare. Universal Links sit on the apex; the artifact CDN at `repository.eafora.org` is invisible to App Store users.

## Testing strategy

Per Constitution Principle VII, the iOS-only TDD-required surfaces are:

- `FileSystemArtifactCache` contract: a `cache.put(...)` → `cache.get(...)` round-trip; assert byte-equal returns; assert a missing key returns `nil`; assert eviction removes the right versions; assert directory creation is idempotent. Runs against the iOS simulator's `Library/Caches/` via XCTest.
- `URLSessionFetcher` error mapping: simulated 4xx / 5xx responses (via `URLProtocol` interception) map to thrown errors carrying the source URL and HTTP status in the message body.
- MTKView ↔ Rust surface bridge: assert the surface's reported size matches the MTKView's `drawableSize`; assert resize events propagate. iOS simulator.
- `EmbeddedBundle.swift` contract: assert the bundle reads from `Bundle.main`, parses the manifest, and constructs a `core::artifact::Bundle` synchronously without I/O on a background queue (the spec requires synchronous startup load).
- Universal Link routing: assert that an incoming `https://eafora.org/region/usa` URL sets `selectedRegion` to `RegionCode("usa")` and the region-detail sheet presents with that region; assert the same from a fresh launch and from a backgrounded resume. Assert that `https://eafora.org/about` sets `settingsPresented = true` and the Settings sheet presents (About is the top section, visible without scrolling).

Cross-platform surfaces (manifest parsing, SHA-256 verification, license-class authorization, FlatGeobuf hit testing) are tested in `core/` once and not re-tested per platform. See `client.md` §Testing strategy.

XCUITest (UI automation) is **not** in scope for the foreseeable future (through v3+). The visual ground truth lives in `docs/design/stub-mobile.html`; parity is checked manually against the stubs before review submission. The cost of a full UI-automation suite (test maintenance, simulator flakiness, CI time) exceeds the value of automating what a manual check already catches at Eafora's surface area.

## Things to verify

1. `uniffi-bindgen-swift` exact flag set — the per-artifact invocation pattern (separate calls for `--swift-sources`, `--headers`, `--xcframework --modulemap`) is documented as of UniFFI 0.29+; verify against the version pinned in `core/`'s Cargo.toml.
2. **wgpu `Surface::from_metal_layer` (or current equivalent)** — verify against the wgpu version pinned in `core/`. The Metal-from-CAMetalLayer path is stable in wgpu but the function name has shifted across releases.
3. **MTKView `isPaused` + `setNeedsDisplay()` semantics** — confirm against current MTKView docs that this combination drives the on-demand-only render loop without periodic GPU wakeups.
4. **`xcodebuild -create-xcframework` flag form** — the `-library` + `-headers` repetition for multiple slices has been stable since 2020 but is worth a spot-check against current Xcode docs.
5. `xcodebuild -exportArchive` direct-upload flow — verify `ExportOptions.plist`'s `destination = upload` key + the `-authenticationKey*` flag spellings against the Xcode version pinned in CI. The two-call chain (archive → exportArchive-with-upload) replaces the older three-call chain (archive → exportArchive → altool); spot-check the modern shape works end-to-end against the current Xcode before relying on it. Fallback if it doesn't: separate `xcodebuild -exportArchive` (export only) + `xcrun altool --upload-package`, even though `altool` is deprecated.
6. AASA Worker route — `tools/aasa-deploy/wrangler.toml`'s route pattern (`eafora.org/.well-known/apple-app-site-association`) needs to take precedence over the main web Worker's catch-all route. Cloudflare's route-matching rules are documented but worth a spot-check after the first deploy that hitting the URL serves the AASA Worker, not the web Worker. Also confirm the served `Content-Type` is `application/json` and that no extension appears on the path.

## Follow-up work

- First `/speckit.specify` feature spec for the iOS client: see `docs/task-order.md` §Sequence step 4. The implementation feature lands the Xcode project, the xcframework build pipeline, the SwiftUI shell with `MapView` + `RegionDetailView`, the file-system cache adapter, and the in-bundle embedded artifact loader — enough to render the static-stub-equivalent of `docs/design/stub-mobile.html` against real data on the iOS simulator.
- Initial Apple Developer Program enrollment is a prerequisite. TestFlight internal testing can begin as soon as enrollment completes; external testing follows after a beta review.

Deferred-but-not-blocking iOS work lives in `docs/backlog.md` §Client (currently empty) once items earn deferral as concrete work.
