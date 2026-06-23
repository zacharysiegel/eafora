# Implementation Plan: core/ crate — data layer

**Branch**: `005-core-data` | **Date**: 2026-06-22 | **Spec**: `specs/005-core-data/spec.md`

**Input**: Feature specification from `/specs/005-core-data/spec.md`

## Summary

Stand up a new `core/` Cargo workspace member that compiles for both the host (Apple Silicon Mac mini) and `wasm32-unknown-unknown`. Move the existing canonical-store enums (`StatisticKind`, `DataSourceKind`, `LicenseClass`, `LicenseShardClass`, `DataStatus`, `SourceRevision`) and the SHA-256 helpers (`sha256_hex`, `sha256_hex_of_file`) from `ingestion/` into `core/`; ingestion `pub use`s them so its existing import paths stay valid. Extract the producer's private `ManifestSerializer<'a>` into an owned consumer-side `Manifest` type with a new leading `manifest_schema_version: u32` field. Define the cross-platform `ArtifactCache` async trait (stable AFIT — no `async-trait` crate). Define `DiscoveryDocument` + `parse_discovery_document`. Eagerly-parse FlatGeobuf geometry into a `FlatGeobufReader` type owned by `core::artifact::geometry`. Implement `Bundle::open(version_label, &cache, ctx)` reading every file through the cache trait (no filesystem path argument); produce a `Send + Sync` `Bundle` of pure parsed data — manifest + reader + `BTreeMap<StatisticShardKey, Vec<u8>>` shard bytes (filtered by `DistributionContext::authorized_classes`). Ship the SQLite `Vec<u8>`-backed VFS as a `core::sqlite::vfs` module (the renderer in 006 consumes it). Re-export `tokio::sync::watch::{Sender, Receiver}` from `core::artifact::bundle_watch` for the cross-platform hot-swap channel.

The producer side (`ingestion/`) continues to be the canonical author of manifests + shards; this feature decouples the consumer-side TYPES from the producer side. Per the §Clarifications session in spec.md, `Bundle` is pure data (no `Connection` inside) so the hot-swap channel works cleanly on both single-threaded WASM and multi-threaded iOS. The `manifest_schema_version` gate makes future shape changes visible to older binaries in the field rather than silently misinterpreted.

## Technical Context

**Language/Version**: Rust 2024 edition (workspace pin in `Cargo.toml`). Stable AFIT (async fn in trait without `async-trait` crate) requires `rustc 1.75+`; no `rust-toolchain.toml` pin currently — developers use their installed `rustup default`. Plan-time decision: add a `rust-toolchain.toml` pinning `1.83` or later (covers stable AFIT + cargo edition 2024 stability) — see §Outstanding plan decisions.

**Primary Dependencies (new for this feature, added to `[workspace.dependencies]` if not present)**:

| Crate          | Purpose                                                                 | Wildcard pin | Notes |
|----------------|-------------------------------------------------------------------------|--------------|-------|
| (existing)     | `serde`, `serde_json`, `chrono`, `uuid`, `sha2`, `rusqlite`, `flatgeobuf`, `geozero`, `geo-types`, `minimer`, `log`, `tokio`, `bytes` | (already pinned) | All consumed by `core/` via `{ workspace = true }`; no new versions. |
| (existing tokio reduced features) | `tokio` workspace pin already declares `features = ["full"]`. `core/` consumes `tokio` with `features = ["sync"]` only — single-threaded WASM cannot use `rt-multi-thread`; `sync` is enough for `watch`. | (same pin) | Use per-crate `default-features = false, features = ["sync"]` in `core/Cargo.toml`. |
| `wasm-bindgen` | wasm32-only target dependency; needed by `core::sqlite::vfs` for any JS-bridge calls and for `wasm-bindgen-test` in test mode. | resolve from `wasm-bindgen-test`'s pin (verify against the version `wasm-bindgen-test` requires) | New addition to `[workspace.dependencies]`. |
| `wasm-bindgen-futures` | wasm32-only; converts JS promises to Rust futures for any async file-reading inside the VFS path. | matches `wasm-bindgen` | New addition. |
| `js-sys`       | wasm32-only; for `BigInt64Array` and similar typed-array conversions the VFS may need. | matches `wasm-bindgen` | New addition. |
| `wasm-bindgen-test` | dev-dependency (wasm32-only); the test harness for FR-025. | matches `wasm-bindgen` | New addition. |

No `async-trait` (stable AFIT covers `ArtifactCache`). No new third-party deps beyond the wasm-bindgen family. Per `feedback_eafora_library_conventions`, the wasm-bindgen family is the established WASM-target idiom; doesn't trigger the "ask before adding" rule since the architecture doc (`docs/architecture/client-web.md` §Workspace placement) already names them as the web target's required toolchain.

**Storage**: N/A — `core/` is consumed by both producer (`ingestion/`) and clients (003 / 004); the clients are the storage owners (OPFS for web; `Library/Caches/` for iOS). `Bundle::open` consumes the `ArtifactCache` trait, never touches filesystems directly.

**Testing**:
- Host: `cargo test -p core` (FR-024 surfaces). Standard `#[cfg(test)] mod tests` blocks adjacent to each module.
- wasm32: `cargo test -p core --target wasm32-unknown-unknown` via `wasm-bindgen-test --headless --chrome` for the wasm-target-specific paths (FR-025: `parse_manifest`, `parse_discovery_document`, `verify_sha256`, `Bundle::open` against the `MockArtifactCache`).
- Regression net: `cargo test -p ingestion` must continue to pass post-move (P1 acceptance #3) — the existing producer-side determinism / sort-order / round-trip tests are what catch any regression from extracting the types.

**Target Platform**:
- Host targets: `aarch64-apple-darwin` (the Mac mini M1), `x86_64-unknown-linux-gnu` (for any future Linux CI).
- WASM target: `wasm32-unknown-unknown` (the web client's compile target per `docs/architecture/client-web.md` §`cargo-leptos`).
- iOS / Android targets are not needed for THIS feature (`core/` builds host-only here; 004-ios-client adds `aarch64-apple-ios` + `aarch64-apple-ios-sim` builds against `core/` when 004 begins implementation).

**Project Type**: library crate (`[lib]` only; no `[[bin]]`).

**Performance Goals**:
- `parse_manifest` on a realistic v1 manifest (one geometry shard + a handful of statistic shards): under 1ms on the Mac mini M1 (consumer-side, called once per bundle open).
- `Bundle::open` on a realistic v1 bundle (~5MB geometry + ~100KB total shards): under 100ms on the Mac mini M1, dominated by SHA-256 verification of the geometry. The host-target `sha2` crate uses platform SIMD; expect 2-3GB/s throughput, so 5MB hashes in roughly 2ms; remainder is FlatGeobuf parsing (the upstream crate's R-tree index build).
- `Bundle::open` on wasm32: under 250ms for the same bundle (slower SHA-256 in WASM; FlatGeobuf parse comparable).

**Constraints**:
- Single-threaded WASM (per `docs/architecture/overview.md` §Web client + §Async model): `tokio` features restricted to `sync` only; no spawning; the `watch` channel works without a runtime.
- No `SharedArrayBuffer` on web (per overview §Web client): SQLite-via-`Vec<u8>`-VFS pattern, not OPFS file handles.
- `Bundle: Send + Sync` (per spec.md §Clarifications Q2): `Arc<Bundle>` crosses thread boundaries on iOS where the live-fetch task runs on a tokio worker thread.
- No new third-party deps beyond the wasm-bindgen family (per Constitution Principle IV + `feedback_eafora_library_conventions`).

**Scale/Scope**: ~43 FRs across 8 modules (`core/src/{lib,error,filesystem,canonical/canonical_model,artifact/{manifest,bundle,bundle_watch,cache,discovery,geometry},license/license,sqlite/{vfs,schema}}.rs`) plus `core/build.rs` for the revision capture. Estimated ~2000 LOC for the implementation (the Model + Entity split adds ~100 LOC over the original estimate; the SQLite-schema contract module adds ~150 LOC; the geometry + manifest constant additions add ~30 LOC; the new constants from FR-020i / j + build.rs add ~30 LOC), ~900 LOC for tests. The biggest single chunks are `Bundle::open` (~120 LOC), the WASM VFS (~200 LOC), and the SQLite-schema contract (~150 LOC).

## Constitution Check

**Gate**: must pass before Phase 0 research; re-checked after Phase 1 design.

| Principle | Status | Justification |
|-----------|--------|---------------|
| I. Educational neutrality (NON-NEGOTIABLE) | N/A | `core/` ships types + parsers + loader; no UI text or editorial copy. |
| II. Source provenance (NON-NEGOTIABLE) | Pass | `Manifest.source_revisions` carries every contributing source with `revision` + `published` + `fetched` timestamps; `Bundle::open` makes provenance reachable to every client query. The producer-side `SourceRevision` struct is moved verbatim (no semantic change). |
| III. Rust core, native UI shells | Pass | This feature IS the "Rust core" extraction. No UI code; consumed by both the producer (ingestion) and every native UI shell (web / iOS / Android via 003 / 004 / future). |
| IV. Singularity convention parity | Pass | No new third-party crates beyond the wasm-bindgen family (which is the established WASM-target convention; matches `client-web.md` §Workspace placement). Wildcard re-exports per `feedback_wildcard_re_exports`. `mod.rs` files hold only declarations + re-exports per `feedback_mod_rs_holds_only_declarations`. `core/` versioned at `0.0.0` per the constitution's placeholder versioning rule. Per `docs/conventions/types.md`: the moved types (Model + Entity / Projection / SerialIn / SerialOut pairs, `Kind` enum suffix, `TryFrom<&str>` parsing) preserve their existing convention compliance from `ingestion/`. Per `docs/conventions/logging.md`: any log calls in `Bundle::open` or `parse_manifest` use the `<message>; [key=value]` format. |
| V. Explicit over implicit | Pass | `Bundle::open` does NOT touch SQLite (per FR-019); the consuming renderer in 006 opens its own `rusqlite::Connection`. `parse_manifest` is a plain `serde_json::from_slice` call with explicit validation; no derive-macro magic beyond `Serialize` / `Deserialize`. The `ArtifactCache` trait is a stable AFIT async trait — no `async-trait` macro indirection. No RPC framework, no codegen, no route attribute macros. |
| VI. CDN-delivered data, no live API through v2 | Pass | `core/`'s entire shape assumes data arrives as CDN-hosted artifact bundles; `Bundle::open(version_label, &cache, ...)` has no HTTP client; the cache trait is the only I/O surface. No live API surface in `core/`. |
| VII. Test-first for core logic | Pass | FR-024 explicitly names the TDD-required surfaces (`parse_manifest`, `parse_discovery_document`, `verify_sha256`, `DistributionContext::authorized_classes`, `Bundle::open`). FR-025 extends to wasm32 coverage. Implementation tasks (see `tasks.md` when generated) follow Red-Green-Refactor. The wasm-bindgen-family tests cover the WASM-target VFS path. |
| VIII. Workflow discipline | Pass | Branch `005-core-data` follows the per-body-of-work + `>>> branch:` marker convention. Spec + clarifications + this plan + the upcoming tasks all land on the same PR per `feedback_spec_and_plan_same_pr`. 006-core-renderer stacks on this branch (already done; see `git log 006-core-renderer`). |

No violations. No Complexity Tracking entries needed.

## Project Structure

### Documentation (this feature)

```text
specs/005-core-data/
├── plan.md              # This file
├── spec.md              # Feature spec (with §Clarifications session 2026-06-22)
├── checklists/
│   └── requirements.md  # Specification Quality Checklist (created during /speckit-specify)
├── research.md          # Phase 0 output (this command) — covers AFIT support + WASM VFS choice + tokio feature trimming
├── data-model.md        # Phase 1 output (this command) — `Manifest`, `Bundle`, `DiscoveryDocument`, `ArtifactCache` trait shape
├── quickstart.md        # Phase 1 output (this command) — onboarding for a future contributor implementing 003 / 004 / 006 against `core/`
├── contracts/
│   └── core-public-api.md  # Phase 1 output — the public symbols `core/` exposes, grouped by submodule
└── tasks.md             # Phase 2 output (/speckit-tasks command — NOT created here)
```

### Source Code (repository root)

This feature adds the `core/` workspace member and modifies `ingestion/` to re-export the moved types. The other top-level directories (`web/`, `ios/`, `android/`) don't exist yet (003 / 004 add them); `tools/` is unchanged.

```text
core/                                       # NEW workspace member
├── Cargo.toml                              # name="core", version="0.0.0", edition.workspace=true
├── build.rs                                # Captures source revision via `git rev-parse HEAD`; emits `cargo:rustc-env=EAFORA_REVISION=...`; falls back to "unknown" on shallow checkout (per FR-020k)
├── src/
│   ├── lib.rs                              # `pub mod` + `pub use` declarations + `pub const REVISION: &str = env!("EAFORA_REVISION");` (per FR-020k)
│   ├── error.rs                            # CANONICAL HOME for `AppError` (moved from ingestion). minimer::define_app_error! + From impls for serde_json, rusqlite, flatgeobuf, geozero, log::SetLoggerError (the parser-surface set core touches); ingestion adds the rest in its own error.rs via `pub use core::error::AppError;` + ingestion-side From impls (orphan rule allows: type from core, impl from ingestion for ingestion-side deps).
│   ├── filesystem.rs                       # MOVED wholesale from ingestion/src/filesystem.rs. Cross-target: FileReference, Hashed<T>, sha256_hex, verify_sha256 (new). Host-only (cfg(not(target_arch = "wasm32"))): sha256_hex_of_file, filename_of, read_bytes, load_hashed_file.
│   ├── canonical/
│   │   ├── mod.rs                          # pub mod canonical_model; pub use canonical_model::*;
│   │   └── canonical_model.rs              # StatisticKind, DataSourceKind, DataStatus, LicenseClass, LicenseShardClass, SourceRevision (moved from ingestion)
│   ├── artifact/
│   │   ├── mod.rs                          # pub mod manifest; pub mod bundle; pub mod bundle_watch; pub mod cache; pub mod discovery; pub mod geometry; pub use {manifest,bundle,bundle_watch,cache,discovery,geometry}::*;
│   │   ├── manifest.rs                     # Manifest, ManifestEntry, parse_manifest, MANIFEST_FILENAME, MANIFEST_SCHEMA_VERSION, MANIFEST_LATEST_KEY, CONTENT_TYPE_MANIFEST, CONTENT_TYPE_FLATGEOBUF, CONTENT_TYPE_SQLITE, CACHE_CONTROL_MANIFEST, CACHE_CONTROL_SHARD constants (per FR-020j)
│   │   ├── bundle.rs                       # Bundle struct (pure data), Bundle::open(version_label, &cache, ctx)
│   │   ├── bundle_watch.rs                 # pub use tokio::sync::watch::{Sender, Receiver, channel};
│   │   ├── cache.rs                        # ArtifactCache async trait + MockArtifactCache (#[cfg(test)]-only)
│   │   ├── discovery.rs                    # DiscoveryDocument, parse_discovery_document, DISCOVERY_SCHEMA_VERSION, DISCOVERY_URL constant (per FR-020i)
│   │   └── geometry.rs                     # FlatGeobufReader, open_flatgeobuf_reader, Feature, Polygon, BoundingBox, plus producer/consumer-shared constants: GEOMETRY_LAYER_NAME, GEOMETRY_FILENAME_STEM, FEATURE_COLUMN_ISO3, FEATURE_COLUMN_NAME_EN, SHARD_FILENAME_EXTENSION, GEOMETRY_FILENAME_EXTENSION (per FR-020f)
│   ├── license/
│   │   ├── mod.rs                          # pub mod license; pub use license::*;
│   │   └── license.rs                      # DistributionContext enum + authorized_classes()
│   └── sqlite/
│       ├── mod.rs                          # pub mod vfs; pub mod schema; pub use {vfs,schema}::*;
│       ├── vfs.rs                          # SQLite Vec<u8>-backed custom VFS (cfg-gated to wasm32; native target is empty cfg-out)
│       └── schema.rs                       # Shared producer/consumer contract: APPLICATION_ID, SCHEMA_VERSION, table + column + index name constants, PERIOD_DATE_FORMAT, shard_schema_ddl(), validate_shard_header()

# MODIFIED:
ingestion/
├── Cargo.toml                              # add `core = { workspace = true }` to dependencies
└── src/
    ├── error.rs                            # `pub use core::error::AppError;` + the additional `From` impls for ingestion-only error families (sqlx, reqwest, zip, shapefile, shapefile::dbase, secr, dotenvy, base64)
    ├── canonical/
    │   └── canonical_model.rs              # The `*Entity` wire-shape types (`RegionEntity`, `CountryEntity`, `StatisticEntity`, `DataSourceEntity`, `StatisticValueEntity`, `SourceChoiceEntity`) STAY here — they're producer-only Postgres wire shapes. Their `From<Entity> for Model` / `TryFrom<Entity> for Model` impls also stay here (orphan rule allows: ingestion owns the Entity even though Model is foreign from core). `StatisticValue` + `SourceChoice` Models stay too (consumers read `statistic_value` from SQLite shards directly, not via the Postgres Model). At the top of the file: `pub use core::canonical::canonical_model::*;` so existing `crate::canonical::canonical_model::Region` import sites resolve via the moved Model.
    ├── adapter/
    │   └── adapter_model.rs                # `NaiveDatePeriod` moves to core; the rest (`AdapterOptions`, `NormalizedStatisticValue`, `NormalizeOutcome`, `IngestWarning`, `IngestWarningKind`) stays. At the top: `pub use core::canonical::canonical_model::NaiveDatePeriod;` so existing `crate::adapter::NaiveDatePeriod` imports resolve.
    ├── filesystem.rs                       # `pub use core::filesystem::*;` (single-line re-export; the rest moved to core/src/filesystem.rs). Alternatively delete the file entirely and add `pub use core::filesystem;` to lib.rs — implementation-time choice.
    ├── artifact/
    │   ├── hashing.rs                      # ingestion-side producer orchestrators only (hash_sqlite_shards, hash_geometry). The sha256_hex / sha256_hex_of_file helpers now reach via core::filesystem::*; this file stays for the rename-dance logic that's producer-specific.
    │   ├── publish.rs                      # `load_build_report_from_disk` rewritten to use `core::artifact::manifest::parse_manifest` instead of its private `ManifestOnDisk` / `ManifestEntryOnDisk` structs (deleted). The `CONTENT_TYPE_*` consts also delete (moved to `core::artifact::manifest` per FR-020g); call sites reach via the moved constants. Eliminates the parallel manifest-deserializer drift risk per FR-020e.
    │   ├── writer/
    │   │   ├── manifest.rs                 # rewritten: use core::artifact::manifest::Manifest; ingestion's write_manifest constructs a Manifest with manifest_schema_version: 1 and serializes via the consumer-side Manifest's Serialize impl. The private ManifestSerializer struct goes away.
    │   │   ├── sqlite.rs                   # rewritten to use core::sqlite::schema constants + shard_schema_ddl() per FR-020d. Private SQLITE_APPLICATION_ID / SQLITE_USER_VERSION / create_schema function removed; insert_shard_key + insert_rows SQL strings reference core::sqlite::schema column-name constants via const_format::formatcp!. Existing producer-side tests continue to pass.
    │   │   └── flatgeobuf.rs               # rewritten per FR-020f to use the moved constants from core::artifact::geometry: GEOMETRY_LAYER_NAME, GEOMETRY_FILENAME_STEM, FEATURE_COLUMN_ISO3, FEATURE_COLUMN_NAME_EN. Private copies of these constants delete; call sites reach via the moved constants. Existing producer-side tests continue to pass.
    │   └── artifact_model.rs               # `pub use core::artifact::manifest::*;` for any types that moved + ingestion-only types stay (BuildReport, ArtifactVersion, etc.)

# UNCHANGED:
Cargo.toml                                  # workspace root: add "core" to members array; add wasm-bindgen-family deps to [workspace.dependencies]
.specify/                                   # spec-kit machinery (this file lives under .specify/templates/plan-template.md)
docs/                                       # architecture docs (already updated to include manifest_schema_version per the §Clarifications session)
```

**Structure Decision**: Single new workspace member (`core/`) added to the existing workspace alongside `ingestion/` and `tools/seed_generator/`. `core/` is a pure library crate consumed by both `ingestion/` (the producer) and — once 003 / 004 land — `web/` (Leptos) and `ios/` (via UniFFI in the FFI layer that lives inside 003 / 004, not in `core/`). The per-feature module layout inside `core/src/` mirrors `ingestion/src/`'s convention: each concern is a directory under `src/` with a `mod.rs` (declarations + re-exports only per `feedback_mod_rs_holds_only_declarations`) and a single primary file by the same name.

## Phase 0: Outline & Research

Three unknowns surfaced in Technical Context required research before the type extraction can begin. The §Outstanding plan decisions section below captures the remaining 3 decisions that need owner input at plan-review time.

The research output lands at `specs/005-core-data/research.md` (created as Phase 0 of this command).

### Topic 1: Stable AFIT (async fn in trait) support across the workspace toolchain

**Decision**: Adopt `rust-toolchain.toml` pinning Rust 1.83 (or the most recent stable as of 2026-06; verify at plan-review time). Use stable AFIT for `ArtifactCache`; do NOT introduce the `async-trait` crate.

**Rationale**: Stable AFIT lands in Rust 1.75 (2023-12). Eafora's workspace already uses edition 2024 (released with Rust 1.85 in 2025-02), so the toolchain is already comfortably past 1.75. The owner's installed `rustup default` resolves to a recent stable; no developer is on a pre-1.75 toolchain. Pinning via `rust-toolchain.toml` makes the floor explicit and ensures CI reproducibility. The `async-trait` crate adds a heap allocation per call (boxes the returned future) and is unnecessary at this version floor.

**Alternatives considered**:
- **`async-trait` crate**: rejected — adds a dependency for no benefit; the boxing cost is real for hot paths even if small at v1 scale.
- **No `rust-toolchain.toml`**: rejected — leaves the floor implicit; CI machines and new contributors could drift.
- **`#[trait_variant::make(Send)]`** for `Send`-bound async traits: rejected — `ArtifactCache` deliberately does NOT require `Send` (web's `OpfsArtifactCache` holds `!Send` `JsValue` indirectly); the wrapper macro doesn't help.

### Topic 2: SQLite VFS strategy for wasm32 — `rusqlite` bundled vs. `sqlite-wasm-rs` vs. JS-side `sql.js`

**Decision**: `rusqlite` with the `bundled` feature, compiled for `wasm32-unknown-unknown` via a custom VFS that reads pages from a `Vec<u8>` held in the connection's open-state. Same crate as native targets; one source of truth for query code.

**Rationale**: Per `docs/architecture/client.md` §SQLite in the client, the decision is "rusqlite compiled to WASM, with the database backed by the SQLite VFS reading from a `Vec<u8>` held in WASM linear memory" — that's already a locked architecture decision; this plan just operationalizes it. The `rusqlite = { version = "0.32.*", features = ["bundled"] }` workspace pin already includes the bundled SQLite C source, which cross-compiles cleanly to `wasm32-unknown-unknown` via Emscripten or via the `cc` crate's wasm32 support (verify at implementation time which path `rusqlite`'s bundled feature uses on wasm32 — likely the latter). The custom VFS is ~200 LOC and lives in `core/src/sqlite/vfs.rs` cfg-gated to wasm32.

**Alternatives considered**:
- **`sqlite-wasm-rs`**: rejected — separate crate, different query API; would split the codebase's SQLite idiom between native (`rusqlite`) and web. The cost of unified `rusqlite::Connection` is worth the VFS implementation effort.
- **JS-side `sql.js` (or `wa-sqlite`)**: rejected per `client.md` §SQLite in the client — the query layer would live in JavaScript, marshaling results across wasm-bindgen on every read.
- **OPFS `FileSystemSyncAccessHandle` directly**: rejected for v1 per `client.md` §SQLite in the client — Worker-only on every shipping browser today; would require hosting SQLite in a Worker with `postMessage` query API. Tracked as backlog item "Move web SQLite engine into a dedicated Worker" with the 30MB-shard trigger.

**Fallback**: if `rusqlite` bundled SQLite does NOT cross-compile cleanly to `wasm32-unknown-unknown` at implementation time (verification step in the first task), switch to `sqlite-wasm-rs` — at the cost of duplicating query code between targets. The fallback would be a real divergence from `client.md`'s locked decision and would require an architecture-doc amendment, but is the recovery path if the bundled cross-compilation fails.

### Topic 3: `tokio` feature flag scope for wasm32

**Decision**: `core/Cargo.toml` declares `tokio = { workspace = true, default-features = false, features = ["sync"] }`. The workspace pin's `features = ["full"]` is for `ingestion/`; `core/` opts out of the runtime / I/O / signal features and pulls only the `sync` module (for `watch`).

**Rationale**: The `tokio::sync::watch` primitive doesn't need a runtime; it works in single-threaded WASM without any tokio runtime spun up. Pulling `features = ["full"]` into `core/` would either (a) fail to compile on wasm32 (mio doesn't compile there) or (b) inflate the WASM bundle with rt-multi-thread machinery the web client never uses. The per-crate feature-trimming pattern is standard for shared crates that consume a single primitive from a larger toolkit.

**Alternatives considered**:
- **Default features**: rejected — same wasm32 compile / bundle-size problems.
- **`tokio = "1.52.*"` repinned per-crate without workspace**: rejected — fragments the pin; ingestion and core would drift over time.
- **Replace `tokio::sync::watch` with a hand-rolled `Arc<RwLock<Arc<Bundle>>>` + manual change-notification**: rejected — the `watch` channel's wait-free reader / writer semantics are exactly what `client.md` §Bundle hot-swap describes; reinventing them is not worth the dependency saving.

### Outstanding plan decisions — RESOLVED 2026-06-22

All three items resolved per owner feedback.

1. **`AppError` ownership: transfers from `ingestion` to `core/`. Ingestion imports.**
   - `core/src/error.rs` defines `AppError` via `minimer::define_app_error!(pub AppError)` and registers the parser-surface `From` impls (serde_json, rusqlite, flatgeobuf, geozero, log::SetLoggerError).
   - `ingestion/src/error.rs` becomes `pub use core::error::AppError;` plus the additional `From` impls for ingestion-only error families (sqlx, reqwest, zip, shapefile, shapefile::dbase, secr, dotenvy, base64). The orphan rule allows ingestion to add `From` impls for its own deps even when the type is defined in `core/` — that's a standard cross-crate error-conversion pattern.
   - `render_error_chain` moves to `core::error` alongside `AppError`.

2. **Move `ingestion/src/filesystem.rs` wholesale to `core/src/filesystem.rs`.**
   - The previous plan split the file (7 items split 4/3 between `core/` and `ingestion/`). The split was arbitrary; there's no semantic reason to leave host-only helpers in ingestion when the rest of the module is moving.
   - Whole-file move: `FileReference`, `Hashed<T>`, `sha256_hex`, `sha256_hex_of_file`, `verify_sha256` (new), `filename_of`, `read_bytes`, `load_hashed_file` ALL land in `core/src/filesystem.rs`.
   - Host-only functions (`sha256_hex_of_file`, `filename_of`, `read_bytes`, `load_hashed_file`) are gated `#[cfg(not(target_arch = "wasm32"))]`.
   - Cross-target functions (`sha256_hex`, `verify_sha256`, `FileReference`, `Hashed<T>`) work on both targets.
   - `ingestion/src/filesystem.rs` becomes `pub use core::filesystem::*;` (or the file is deleted entirely and `ingestion/src/lib.rs` does `pub use core::filesystem;` — implementation-time choice).
   - The originally-planned `core::hashing` module name is dropped; `core::filesystem` is the correct module name (matches the source file).

3. **`MockArtifactCache` gating: `#[cfg(test)]`-only.**
   - The mock exists for `core/`'s own tests of `Bundle::open`. The web and iOS clients will build their own platform-specific mocks (web's against OPFS in headless Chrome; iOS's against `Library/Caches/` in XCTest); neither needs to import `core/`'s mock.
   - Promoting `#[cfg(test)]` to `#[cfg(any(test, feature = "mock"))]` later is a one-character change if a shared-mock need ever surfaces.

## Phase 1: Design & Contracts

**Prerequisites**: `research.md` complete (Phase 0 above).

Phase 1 outputs land at:

- `specs/005-core-data/data-model.md` — full struct definitions, field types, the `ArtifactCache` trait signature, the `DistributionContext::authorized_classes` lookup table, `Manifest`'s `Serialize` field-ordering rule (`manifest_schema_version` first).
- `specs/005-core-data/contracts/core-public-api.md` — the public symbols `core/` exposes to its consumers, grouped by submodule. Generated from the FR list in spec.md.
- `specs/005-core-data/quickstart.md` — for a contributor implementing 003 / 004 / 006 against `core/`: how to `use core::artifact::{Bundle, Manifest, ManifestEntry, ArtifactCache}`, how to construct a `MockArtifactCache` for tests, how to spin up a `tokio::sync::watch::channel::<Arc<Bundle>>` and wire the renderer's `Receiver`.

Phase 1 also updates the agent context file (`CLAUDE.md`) to point at this plan between the `<!-- SPECKIT START -->` and `<!-- SPECKIT END -->` markers.

## Phasing for PRs

This feature is small enough to land as ONE PR (no internal stack). Per `feedback_spec_and_plan_same_pr`, the PR includes spec.md + plan.md + tasks.md + research.md + data-model.md + contracts/ + quickstart.md + the implementation + tests.

Rough implementation order inside the single PR (tasks.md will codify this):

1. Workspace setup (`core/Cargo.toml`, root `Cargo.toml` members + wasm-bindgen-family deps, `rust-toolchain.toml` pin, `core/build.rs` for revision capture per FR-020k).
2. `core::error::AppError` (canonical home, moves from ingestion) + `core::filesystem` (moves wholesale from `ingestion/src/filesystem.rs`).
3. `core::canonical::canonical_model` (move from `ingestion/src/canonical/canonical_model.rs`): the 6 enums + `SourceRevision` + `NaiveDatePeriod` (lifted from `ingestion/src/adapter/adapter_model.rs`) + the 4 consumer-facing Models (`Region`, `Country`, `Statistic`, `DataSource`). The matching `*Entity` types + their `(Try)From<Entity> for Model` impls stay in ingestion. Ingestion re-exports the moved Models via `pub use core::canonical::canonical_model::*;`.
4. `core::artifact::manifest` (Manifest + ManifestEntry + parse_manifest + MANIFEST_SCHEMA_VERSION = 1); ingestion's `write_manifest` rewritten to use it.
5. `core::artifact::discovery` (DiscoveryDocument + parse_discovery_document + DISCOVERY_SCHEMA_VERSION = 1).
6. `core::artifact::cache` (ArtifactCache trait + MockArtifactCache).
7. `core::license::license` (DistributionContext + authorized_classes).
8. `core::artifact::geometry` (FlatGeobufReader wrapping the upstream `flatgeobuf` crate's reader + open_flatgeobuf_reader; cfg-gating for any host-vs-wasm differences in the upstream crate's API).
9. `core::sqlite::vfs` (Vec<u8>-backed VFS for wasm32; native target is `cfg(not(target_arch = "wasm32"))` no-op).
10. `core::sqlite::schema` (FR-020b through FR-020e: constants + `shard_schema_ddl()` + `validate_shard_header()`). Producer-side `ingestion/src/artifact/writer/sqlite.rs` updated to use the constants + DDL function; private `SQLITE_APPLICATION_ID` / `SQLITE_USER_VERSION` / `create_schema` removed. Producer-side `ingestion/src/artifact/publish.rs::load_build_report_from_disk` updated to use `core::artifact::manifest::parse_manifest`; private `ManifestOnDisk` / `ManifestEntryOnDisk` deleted.
11. `core::artifact::bundle::Bundle` + `Bundle::open(version_label, &cache, ctx)`.
12. `core::artifact::bundle_watch` (re-export `tokio::sync::watch::{Sender, Receiver}`).
13. Test suites (host: `cargo test -p core`; wasm32: `wasm-bindgen-test --headless --chrome`).
14. Regression check: `cargo test -p ingestion` passes; `cargo build --workspace` succeeds; `cargo build -p core --target wasm32-unknown-unknown` succeeds.
15. Polish: agent-context update (CLAUDE.md), final clippy sweep, doc-comment review per `feedback_no_process_narration_in_doc_comments`.

## Brief PR description (per `feedback_pr_description_style`)

> Adds a new `core/` Cargo workspace member that compiles for both host (Apple Silicon) and `wasm32-unknown-unknown`. Extracts the canonical-store enums + manifest schema + SHA-256 helpers from `ingestion/`; ingestion re-exports them. Introduces a new `manifest_schema_version: u32` field on the manifest (first key; v1 = 1) as a forward-compat gate for v2+ shape changes. Defines the cross-platform `ArtifactCache` async trait (stable AFIT, no `async-trait` crate). Defines `DiscoveryDocument` + `parse_discovery_document`. Implements `Bundle::open(version_label, &cache, distribution_context)` returning a `Send + Sync` `Bundle` of pure parsed data (manifest + eagerly-parsed FlatGeobufReader + license-filtered shard bytes; no SQLite Connection — the consuming renderer in 006 opens its own thread-local). Adds `core::sqlite::schema` as the shared producer / consumer SQLite-shard contract (constants for the `application_id` / `user_version` magic numbers + every table / column / index name + the `PERIOD_DATE_FORMAT`; `shard_schema_ddl()` builds the schema DDL from those constants; `validate_shard_header()` is the consumer-side "is this an Eafora shard with a version I understand?" gate). Producer-side `ingestion/src/artifact/writer/sqlite.rs` now uses those constants instead of its own copies; `ingestion/src/artifact/publish.rs::load_build_report_from_disk` now uses `core::artifact::manifest::parse_manifest` instead of its private deserializer. Re-exports `tokio::sync::watch` for the bundle hot-swap channel. Ships the SQLite `Vec<u8>`-backed VFS for the wasm32 target. Architecture docs (`client.md` §Manifest schema, `ingestion.md` §Manifest format, `overview.md` §Artifact format) updated to include `manifest_schema_version`. Producer-side tests continue to pass; new test suite covers `parse_manifest`, `parse_discovery_document`, `verify_sha256`, `DistributionContext::authorized_classes`, `Bundle::open`, `validate_shard_header`, and `shard_schema_ddl` on both targets.
