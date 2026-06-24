# Implementation Plan: shared/ crate — data layer

**Branch**: `005-core-data` | **Date**: 2026-06-22 | **Spec**: `specs/005-core-data/spec.md`

**Input**: Feature specification from `/specs/005-core-data/spec.md`

## Summary

Stand up a new `shared/` Cargo workspace member that compiles for both the host (Apple Silicon Mac mini) and `wasm32-unknown-unknown`. Move the existing canonical-store enums (`StatisticKind`, `DataSourceKind`, `LicenseClass`, `LicenseShardClass`, `DataStatus`, `SourceRevision`) and the SHA-256 helpers (`sha256_hex`, `sha256_hex_of_file`) from `ingestion/` into `shared/`; ingestion `pub use`s them so its existing import paths stay valid. Extract the producer's private `ManifestSerializer<'a>` into an owned consumer-side `Manifest` type with a new leading `manifest_schema_version: u32` field. Define the cross-platform `ArtifactCache` async trait (stable AFIT — no `async-trait` crate). Define `DiscoveryDocument` + `parse_discovery_document`. Eagerly-parse FlatGeobuf geometry into a `FlatGeobufReader` type owned by `shared::artifact::geometry`. Implement `Bundle::open(version_label, &cache, ctx)` reading every file through the cache trait (no filesystem path argument); produce a `Send + Sync` `Bundle` of pure parsed data — manifest + reader + `BTreeMap<StatisticShardKey, Vec<u8>>` shard bytes (filtered by `DistributionContext::authorized_classes`). Ship the SQLite `Vec<u8>`-backed VFS as a `shared::sqlite::vfs` module (the renderer in 006 consumes it). Re-export `tokio::sync::watch::{Sender, Receiver}` from `shared::artifact::bundle_watch` for the cross-platform hot-swap channel.

The producer side (`ingestion/`) continues to be the canonical author of manifests + shards; this feature decouples the consumer-side TYPES from the producer side. Per the §Clarifications session in spec.md, `Bundle` is pure data (no `Connection` inside) so the hot-swap channel works cleanly on both single-threaded WASM and multi-threaded iOS. The `manifest_schema_version` gate makes future shape changes visible to older binaries in the field rather than silently misinterpreted.

## Technical Context

**Language/Version**: Rust 2024 edition (workspace pin in `Cargo.toml`). Stable AFIT (async fn in trait without `async-trait` crate) requires `rustc 1.75+`; no `rust-toolchain.toml` pin currently — developers use their installed `rustup default`. Plan-time decision: add a `rust-toolchain.toml` pinning `1.83` or later (covers stable AFIT + cargo edition 2024 stability) — see §Outstanding plan decisions.

**Primary Dependencies (new for this feature, added to `[workspace.dependencies]` if not present)**:

| Crate          | Purpose                                                                 | Wildcard pin | Notes |
|----------------|-------------------------------------------------------------------------|--------------|-------|
| (existing)     | `serde`, `serde_json`, `chrono`, `uuid`, `sha2`, `rusqlite`, `flatgeobuf`, `geozero`, `geo-types`, `minimer`, `log`, `tokio`, `bytes` | (already pinned) | Consumed by `shared/` via `{ workspace = true }`. Note: `rusqlite` is gated to `cfg(not(target_arch = "wasm32"))` in `shared/Cargo.toml`. |
| `sqlite-wasm-rs` | wasm32-only SQLite library (replaces `rusqlite` on the wasm32 target; `rusqlite`'s `bundled` feature does not cross-compile to `wasm32-unknown-unknown` per Topic 2 below). Pinned in the per-target dependency table. | `0.4.*` (verify against latest stable at implementation time) | New addition to `[workspace.dependencies]`. |
| (existing tokio reduced features) | `tokio` workspace pin already declares `features = ["full"]`. `shared/` consumes `tokio` with `features = ["sync"]` only — single-threaded WASM cannot use `rt-multi-thread`; `sync` is enough for `watch`. | (same pin) | Use per-crate `default-features = false, features = ["sync"]` in `shared/Cargo.toml`. |
| `wasm-bindgen` | wasm32-only target dependency; needed by `shared::sqlite::vfs` for any JS-bridge calls and for `wasm-bindgen-test` in test mode. | resolve from `wasm-bindgen-test`'s pin (verify against the version `wasm-bindgen-test` requires) | New addition to `[workspace.dependencies]`. |
| `wasm-bindgen-futures` | wasm32-only; converts JS promises to Rust futures for any async file-reading inside the VFS path. | matches `wasm-bindgen` | New addition. |
| `js-sys`       | wasm32-only; for `BigInt64Array` and similar typed-array conversions the VFS may need. | matches `wasm-bindgen` | New addition. |
| `wasm-bindgen-test` | dev-dependency (wasm32-only); the test harness for FR-025. | matches `wasm-bindgen` | New addition. |

No `async-trait` (stable AFIT covers `ArtifactCache`). No new third-party deps beyond the wasm-bindgen family. Per `feedback_eafora_library_conventions`, the wasm-bindgen family is the established WASM-target idiom; doesn't trigger the "ask before adding" rule since the architecture doc (`docs/architecture/client-web.md` §Workspace placement) already names them as the web target's required toolchain.

**Storage**: N/A — `shared/` is consumed by both producer (`ingestion/`) and clients (003 / 004); the clients are the storage owners (OPFS for web; `Library/Caches/` for iOS). `Bundle::open` consumes the `ArtifactCache` trait, never touches filesystems directly.

**Testing**:
- Host: `cargo test -p shared` (FR-024 surfaces). Standard `#[cfg(test)] mod tests` blocks adjacent to each module.
- wasm32: `cargo test -p shared --target wasm32-unknown-unknown` via `wasm-bindgen-test --headless --chrome` for the wasm-target-specific paths (FR-025: `parse_manifest`, `parse_discovery_document`, `verify_sha256`, `Bundle::open` against the `MockArtifactCache`).
- Regression net: `cargo test -p ingestion` must continue to pass post-move (P1 acceptance #3) — the existing producer-side determinism / sort-order / round-trip tests are what catch any regression from extracting the types.

**Target Platform**:
- Host targets: `aarch64-apple-darwin` (the Mac mini M1), `x86_64-unknown-linux-gnu` (for any future Linux CI).
- WASM target: `wasm32-unknown-unknown` (the web client's compile target per `docs/architecture/client-web.md` §`cargo-leptos`).
- iOS / Android targets are not needed for THIS feature (`shared/` builds host-only here; 004-ios-client adds `aarch64-apple-ios` + `aarch64-apple-ios-sim` builds against `shared/` when 004 begins implementation).

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

**Scale/Scope**: ~43 FRs across 8 modules (`shared/src/{lib,error,filesystem,canonical/canonical_model,artifact/{manifest,bundle,bundle_watch,cache,discovery,geometry},license/license,sqlite/{vfs,schema}}.rs`) plus `shared/build.rs` for the revision capture. Estimated ~2000 LOC for the implementation (the Model + Entity split adds ~100 LOC over the original estimate; the SQLite-schema contract module adds ~150 LOC; the geometry + manifest constant additions add ~30 LOC; the new constants from FR-020i / j + build.rs add ~30 LOC), ~900 LOC for tests. The biggest single chunks are `Bundle::open` (~120 LOC), the WASM VFS (~200 LOC), and the SQLite-schema contract (~150 LOC).

## Constitution Check

**Gate**: must pass before Phase 0 research; re-checked after Phase 1 design.

| Principle | Status | Justification |
|-----------|--------|---------------|
| I. Educational neutrality (NON-NEGOTIABLE) | N/A | `shared/` ships types + parsers + loader; no UI text or editorial copy. |
| II. Source provenance (NON-NEGOTIABLE) | Pass | `Manifest.source_revisions` carries every contributing source with `revision` + `published` + `fetched` timestamps; `Bundle::open` makes provenance reachable to every client query. The producer-side `SourceRevision` struct is moved verbatim (no semantic change). |
| III. Rust core, native UI shells | Pass | This feature IS the "Rust core" extraction. No UI code; consumed by both the producer (ingestion) and every native UI shell (web / iOS / Android via 003 / 004 / future). |
| IV. Singularity convention parity | Pass | No new third-party crates beyond the wasm-bindgen family (which is the established WASM-target convention; matches `client-web.md` §Workspace placement). Wildcard re-exports per `feedback_wildcard_re_exports`. `mod.rs` files hold only declarations + re-exports per `feedback_mod_rs_holds_only_declarations`. `shared/` versioned at `0.0.0` per the constitution's placeholder versioning rule. Per `docs/conventions/types.md`: the moved types (Model + Entity / Projection / SerialIn / SerialOut pairs, `Kind` enum suffix, `TryFrom<&str>` parsing) preserve their existing convention compliance from `ingestion/`. Per `docs/conventions/logging.md`: any log calls in `Bundle::open` or `parse_manifest` use the `<message>; [key=value]` format. |
| V. Explicit over implicit | Pass | `Bundle::open` does NOT touch SQLite (per FR-019); the consuming renderer in 006 opens its own `rusqlite::Connection`. `parse_manifest` is a plain `serde_json::from_slice` call with explicit validation; no derive-macro magic beyond `Serialize` / `Deserialize`. The `ArtifactCache` trait is a stable AFIT async trait — no `async-trait` macro indirection. No RPC framework, no codegen, no route attribute macros. |
| VI. CDN-delivered data, no live API through v2 | Pass | `shared/`'s entire shape assumes data arrives as CDN-hosted artifact bundles; `Bundle::open(version_label, &cache, ...)` has no HTTP client; the cache trait is the only I/O surface. No live API surface in `shared/`. |
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
├── quickstart.md        # Phase 1 output (this command) — onboarding for a future contributor implementing 003 / 004 / 006 against `shared/`
├── contracts/
│   └── core-public-api.md  # Phase 1 output — the public symbols `shared/` exposes, grouped by submodule
└── tasks.md             # Phase 2 output (/speckit-tasks command — NOT created here)
```

### Source Code (repository root)

This feature adds the `shared/` workspace member and modifies `ingestion/` to import the moved types directly from `shared::` (no forwarding re-exports). The other top-level directories (`web/`, `ios/`, `android/`) don't exist yet (003 / 004 add them); `tools/` is unchanged.

```text
shared/                                       # NEW workspace member
├── Cargo.toml                              # name="shared", version="0.0.0", edition.workspace=true
├── build.rs                                # Captures source revision via `git rev-parse HEAD`; emits `cargo:rustc-env=EAFORA_REVISION=...`; falls back to "unknown" on shallow checkout (per FR-020k)
├── src/
│   ├── lib.rs                              # `pub mod` + `pub use` declarations ONLY (per feedback_mod_rs_holds_only_declarations); re-exports `REVISION` via `pub use revision::*;`
│   ├── revision.rs                         # `pub const REVISION: &str = env!("EAFORA_REVISION");` (per FR-020k; kept out of lib.rs so lib.rs stays pure redirection)
│   ├── error.rs                            # `shared::AppError` newtype generated via `minimer::define_app_error!(pub AppError);` + parser-surface `From` impls (serde_json, rusqlite, flatgeobuf, geozero, log::SetLoggerError) via `minimer::impl_from_error!`. `render_error_chain` lives here. Ingestion has its OWN `AppError` newtype (orphan rule prevents ingestion from adding `From<sqlx::Error>` to a shared-defined type); ingestion's `From<shared::AppError> for ingestion::AppError` is the cross-conversion bridge.
│   ├── filesystem.rs                       # MOVED wholesale from ingestion/src/filesystem.rs. Cross-target: Hashed<T>, sha256_hex, verify_sha256 (new). Not for wasm32 (cfg(not(target_arch = "wasm32"))): FileReference, sha256_hex_of_file, filename_of, read_bytes, load_hashed_file.
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
│       ├── vfs.rs                          # SQLite Vec<u8>-backed custom VFS (cfg-gated to wasm32; the non-wasm32 target is an empty cfg-out)
│       └── schema.rs                       # Shared producer/consumer contract: APPLICATION_ID, SCHEMA_VERSION, table + column + index name constants, PERIOD_DATE_FORMAT, shard_schema_ddl(), validate_shard_header()

# MODIFIED:
ingestion/
├── Cargo.toml                              # add `shared = { workspace = true }` to dependencies
└── src/
    ├── error.rs                            # ingestion's OWN `AppError` newtype (separate from shared's; required by Rust's orphan rule). `minimer::define_app_error!(pub AppError);` + ingestion-only `From` impls (sqlx::Error, reqwest::Error, zip::result::ZipError, shapefile::Error, shapefile::dbase::Error, secr::error::Error, dotenvy::Error, base64::DecodeError) via `impl_from_error!`. Adds one cross-conversion: `impl From<shared::AppError> for ingestion::AppError { fn from(err: shared::AppError) -> Self { Self(err.0) } }` — lets ingestion `?`-propagate from shared functions.
    ├── canonical/
    │   └── canonical_entity.rs             # RENAMED from canonical_model.rs (the canonical Models left, so the old name is a misnomer). Holds the `*Entity` wire-shape types (`RegionEntity`, `CountryEntity`, `StatisticEntity`, `DataSourceEntity`, `StatisticValueEntity`, `SourceChoiceEntity`) — producer-only Postgres wire shapes. Their `From<Entity> for Model` / `TryFrom<Entity> for Model` impls also stay here (orphan rule allows: ingestion owns the Entity even though Model is foreign from shared). `StatisticValue` + `SourceChoice` Models stay too (consumers read `statistic_value` from SQLite shards directly, not via the Postgres Model). At the top of the file: `use shared::canonical::canonical_model::{Region, Country, Statistic, DataSource};` (direct import, no re-export); ingestion's `crate::canonical::canonical_model::*` call sites are migrated to `shared::canonical::canonical_model::*` directly.
    ├── adapter/
    │   └── adapter_model.rs                # `NaiveDatePeriod` moves to shared; the rest (`AdapterOptions`, `NormalizedStatisticValue`, `NormalizeOutcome`, `IngestWarning`, `IngestWarningKind`) stays. The `crate::adapter::NaiveDatePeriod` call sites are migrated to `shared::canonical::canonical_model::NaiveDatePeriod` directly (no re-export).
    ├── filesystem.rs                       # DELETED. The whole file moved to shared/src/filesystem.rs; ingestion call sites import `shared::filesystem` directly (no re-export per feedback on forwarding declarations). The `pub mod filesystem;` line is removed from ingestion/src/lib.rs.
    ├── artifact/
    │   ├── hashing.rs                      # ingestion-side producer orchestrators only (hash_sqlite_shards, hash_geometry). The sha256_hex / sha256_hex_of_file helpers now reach via shared::filesystem::*; this file stays for the rename-dance logic that's producer-specific.
    │   ├── publish.rs                      # `load_build_report_from_disk` rewritten to use `shared::artifact::manifest::parse_manifest` instead of its private `ManifestOnDisk` / `ManifestEntryOnDisk` structs (deleted). The `CONTENT_TYPE_*` consts also delete (moved to `shared::artifact::manifest` per FR-020g); call sites reach via the moved constants. Eliminates the parallel manifest-deserializer drift risk per FR-020e.
    │   ├── writer/
    │   │   ├── manifest.rs                 # rewritten: use shared::artifact::manifest::Manifest; ingestion's write_manifest constructs a Manifest with manifest_schema_version: 1 and serializes via the consumer-side Manifest's Serialize impl. The private ManifestSerializer struct goes away.
    │   │   ├── sqlite.rs                   # rewritten to use shared::sqlite::schema constants + shard_schema_ddl() per FR-020d. Private SQLITE_APPLICATION_ID / SQLITE_USER_VERSION / create_schema function removed; insert_shard_key + insert_rows SQL strings reference shared::sqlite::schema column-name constants via const_format::formatcp!. Existing producer-side tests continue to pass.
    │   │   └── flatgeobuf.rs               # rewritten per FR-020f to use the moved constants from shared::artifact::geometry: GEOMETRY_LAYER_NAME, GEOMETRY_FILENAME_STEM, FEATURE_COLUMN_ISO3, FEATURE_COLUMN_NAME_EN. Private copies of these constants delete; call sites reach via the moved constants. Existing producer-side tests continue to pass.
    │   └── artifact_model.rs               # `StatisticShardKey` moves to shared::artifact::bundle (call sites migrated to `shared::artifact::bundle::StatisticShardKey` directly, no re-export); ingestion-only types stay (BuildReport, ArtifactVersion, etc.)

# UNCHANGED:
Cargo.toml                                  # workspace root: add "shared" to members array; add wasm-bindgen-family deps to [workspace.dependencies]
.specify/                                   # spec-kit machinery (this file lives under .specify/templates/plan-template.md)
docs/                                       # architecture docs (already updated to include manifest_schema_version per the §Clarifications session)
```

**Structure Decision**: Single new workspace member (`shared/`) added to the existing workspace alongside `ingestion/` and `tools/seed_generator/`. `shared/` is a pure library crate consumed by both `ingestion/` (the producer) and — once 003 / 004 land — `web/` (Leptos) and `ios/` (via UniFFI in the FFI layer that lives inside 003 / 004, not in `shared/`). The per-feature module layout inside `shared/src/` mirrors `ingestion/src/`'s convention: each concern is a directory under `src/` with a `mod.rs` (declarations + re-exports only per `feedback_mod_rs_holds_only_declarations`) and a single primary file by the same name.

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

### Topic 2: SQLite library for wasm32 — `rusqlite` bundled rejected; `sqlite-wasm-rs` adopted

**Decision (revised 2026-06-22 after empirical test)**: non-wasm32 targets use `rusqlite` with `features = ["bundled"]`; wasm32 uses `sqlite-wasm-rs`. The two libraries are exposed through a thin cfg-gated typedef in `shared::sqlite` so the renderer's query code is target-agnostic at the surface.

**Rationale**: An empirical test confirmed `rusqlite` with `features = ["bundled"]` does NOT cross-compile to `wasm32-unknown-unknown`. The `bundled` feature compiles SQLite's C source via `cc-rs`; that pipeline ultimately invokes clang against `wasm32-unknown-unknown`, which fails because `wasm32-unknown-unknown` has no libc (clang reports `'stdio.h' file not found`). WASI provides a libc via `wasm32-wasip1`, but the web client's compile target is `wasm32-unknown-unknown` per `client-web.md` §`cargo-leptos`; switching the web target to WASI cascades through cargo-leptos, wasm-bindgen, and the wasm bundle shape. The `sqlite-wasm-rs` crate ships a pre-built SQLite WASM blob designed for `wasm32-unknown-unknown` consumers and exposes a near-rusqlite API surface (`Connection`, `prepare`, `query_row`, etc.). This is the pragmatic path. `client.md` §SQLite in the client has been amended to reflect this two-library decision.

**Implementation shape**: `shared::sqlite` exposes a target-agnostic `Connection` typedef (`pub type Connection = rusqlite::Connection;` on non-wasm32 targets, `pub type Connection = sqlite_wasm_rs::Connection;` on wasm32 — verify the exact crate path against the pinned `sqlite-wasm-rs` version at implementation time). Renderer code in 006 uses `shared::sqlite::Connection`. Where the API surfaces diverge (any method name or signature differing between rusqlite and sqlite-wasm-rs), `shared::sqlite` exposes a thin facade function with a single signature backed by per-target implementation. Both libraries are designed to be near-drop-in replacements for the read-only SELECT queries the renderer issues; the divergence-handling layer should be small.

**Alternatives considered and rejected**:
- **`rusqlite` bundled** — empirically fails to compile (this section's whole subject); originally the decision; superseded.
- **JS-side `sql.js` / `wa-sqlite`** — rejected per `client.md` §SQLite in the client: query layer would live in JavaScript, marshaling results across wasm-bindgen on every read.
- **OPFS `FileSystemSyncAccessHandle` directly** — rejected for v1 per `client.md`: Worker-only on every shipping browser today; would require hosting SQLite in a Worker with `postMessage` query API. Tracked as backlog item with the 30MB-shard trigger.
- **Switch web target to `wasm32-wasip1`** — rejected: cascades through cargo-leptos / wasm-bindgen / bundle shape; far heavier than swapping one crate.

The original "fallback" note from this Topic 2 (a paragraph proposing exactly this swap) was the recovery path that the implementation-time verification triggered.

### Topic 3: `tokio` feature flag scope for wasm32

**Decision**: `shared/Cargo.toml` declares `tokio = { workspace = true, default-features = false, features = ["sync"] }`. The workspace pin's `features = ["full"]` is for `ingestion/`; `shared/` opts out of the runtime / I/O / signal features and pulls only the `sync` module (for `watch`).

**Rationale**: The `tokio::sync::watch` primitive doesn't need a runtime; it works in single-threaded WASM without any tokio runtime spun up. Pulling `features = ["full"]` into `shared/` would either (a) fail to compile on wasm32 (mio doesn't compile there) or (b) inflate the WASM bundle with rt-multi-thread machinery the web client never uses. The per-crate feature-trimming pattern is standard for shared crates that consume a single primitive from a larger toolkit.

**Alternatives considered**:
- **Default features**: rejected — same wasm32 compile / bundle-size problems.
- **`tokio = "1.52.*"` repinned per-crate without workspace**: rejected — fragments the pin; ingestion and shared would drift over time.
- **Replace `tokio::sync::watch` with a hand-rolled `Arc<RwLock<Arc<Bundle>>>` + manual change-notification**: rejected — the `watch` channel's wait-free reader / writer semantics are exactly what `client.md` §Bundle hot-swap describes; reinventing them is not worth the dependency saving.

### Outstanding plan decisions — RESOLVED 2026-06-22

All three items resolved per owner feedback.

1. **`AppError` ownership: each crate has its own newtype via `define_app_error!`. Producer-surface From impls live in ingestion; consumer-surface From impls live in shared.** (Earlier framing of this — "ownership transfers from ingestion to shared; ingestion imports it from shared" — turned out to be incompatible with Rust's orphan rule. Verified 2026-06-22 by reading `minimer-2.1.0/src/error.rs:32-39`, whose macro doc explicitly notes: "Rust's orphan rule prevents downstream crates from implementing From<X> for $crate::AppError directly, so the downstream defines its own newtype.")
   - `shared/src/error.rs` calls `minimer::define_app_error!(pub AppError);` to generate `shared::AppError` (a newtype wrapping `minimer::AppError`). Adds the parser-surface `From` impls via `minimer::impl_from_error!(AppError, serde_json::Error)` etc. — works because `shared::AppError` is local to shared.
   - `ingestion/src/error.rs` ALSO calls `minimer::define_app_error!(pub AppError);` to generate `ingestion::AppError` (a separate newtype, also wrapping `minimer::AppError`). Adds the producer-surface `From` impls (sqlx::Error, reqwest::Error, zip::result::ZipError, shapefile::Error, shapefile::dbase::Error, secr::error::Error, dotenvy::Error, base64::DecodeError) via `impl_from_error!` — works because `ingestion::AppError` is local to ingestion.
   - Cross-conversion: `ingestion/src/error.rs` adds `impl From<shared::AppError> for ingestion::AppError { fn from(err: shared::AppError) -> Self { Self(err.0) } }` (one line; orphan-rule-OK since the target type is local to ingestion). Lets ingestion code `?`-propagate from a shared function: `let manifest = shared::artifact::manifest::parse_manifest(&bytes)?;` works because shared's error converts into ingestion's error at the `?` boundary.
   - Both newtypes wrap the same `minimer::AppError`, so the underlying error storage is uniform. The two newtypes are conceptually one error type with two namespaces.

2. **Move `ingestion/src/filesystem.rs` wholesale to `shared/src/filesystem.rs`.** (Unchanged from earlier resolution.)

3. **`MockArtifactCache` gating: `#[cfg(test)]`-only.** (Unchanged.)

## Phase 1: Design & Contracts

**Prerequisites**: `research.md` complete (Phase 0 above).

Phase 1 outputs land at:

- `specs/005-core-data/data-model.md` — full struct definitions, field types, the `ArtifactCache` trait signature, the `DistributionContext::authorized_classes` lookup table, `Manifest`'s `Serialize` field-ordering rule (`manifest_schema_version` first).
- `specs/005-core-data/contracts/core-public-api.md` — the public symbols `shared/` exposes to its consumers, grouped by submodule. Generated from the FR list in spec.md.
- `specs/005-core-data/quickstart.md` — for a contributor implementing 003 / 004 / 006 against `shared/`: how to `use shared::artifact::{Bundle, Manifest, ManifestEntry, ArtifactCache}`, how to construct a `MockArtifactCache` for tests, how to spin up a `tokio::sync::watch::channel::<Arc<Bundle>>` and wire the renderer's `Receiver`.

Phase 1 also updates the agent context file (`CLAUDE.md`) to point at this plan between the `<!-- SPECKIT START -->` and `<!-- SPECKIT END -->` markers.

## Phasing for PRs

This feature breaks naturally into **5 serial PRs**, stacked linearly per the constitution's §Branch per body of work rule. Each PR ships one logical slice with its own PR description and review boundary; per `feedback_branch_per_body_of_work` and `feedback_pr_description_style`, every PR is `gh pr create`'d and assigned to `zacharysiegel`.

The **spec-and-design artifacts** (this `plan.md`, `data-model.md`, `contracts/core-public-api.md`, `quickstart.md`, `tasks.md`, plus the architecture-doc amendments) are already on `005-core-data` per `feedback_spec_and_plan_same_pr`; the implementation PRs stack on that branch.

| PR | Branch                                  | Phases (per tasks.md)                    | Off-branch              | Scope summary |
|----|-----------------------------------------|------------------------------------------|-------------------------|---------------|
| A  | `impl-005-foundational`                 | 1, 2 (Setup + Foundational)              | `005-core-data`         | Workspace member + `shared/Cargo.toml` + `shared/build.rs` + `shared::revision::REVISION` + `shared::AppError` (newtype) + ingestion's own `AppError` newtype + `shared::filesystem` whole-file move; ingestion's `filesystem.rs` deleted and its call sites migrated to `shared::filesystem` directly (no re-export); `render_error_chain` called via `shared::error::` directly. No user-visible behavior change; `cargo test -p ingestion` passes against the moved files. ~150 LOC. |
| B  | `impl-005-canonical-types`              | 3 (US1)                                  | `impl-005-foundational` | Canonical Models extracted (`Region`, `Country`, `Statistic`, `DataSource`) + enums (`StatisticKind`, `DataSourceKind`, `LicenseClass`, `LicenseShardClass`, `DataStatus`) + `SourceRevision` + `NaiveDatePeriod` + `StatisticShardKey`. Ingestion call sites migrated to `shared::` paths directly (no re-exports); `From<Entity>` impls stay in ingestion; ingestion's `canonical_model.rs` renamed to `canonical_entity.rs`. `cargo build -p shared --target wasm32-unknown-unknown` succeeds for the first time at the end of this PR. ~250 LOC. |
| C  | `impl-005-manifest-discovery-cache`     | 4, 5, 6 (US2 + US3 + US4)                | `impl-005-canonical-types` | `shared::artifact::manifest::Manifest` + `parse_manifest` + manifest constants (FR-020g, FR-020h, FR-020j) + `manifest_schema_version: 1`. `shared::artifact::discovery::DiscoveryDocument` + `parse_discovery_document` + `DISCOVERY_URL`. `shared::artifact::cache::ArtifactCache` trait + `MockArtifactCache`. Producer-side `ingestion/src/artifact/writer/manifest.rs::write_manifest` rewritten to use `shared::Manifest`; `ingestion/src/artifact/publish.rs::load_build_report_from_disk` rewritten to use `shared::parse_manifest`; producer-side `CONTENT_TYPE_*` consts deleted (moved to shared). The MVP-shaped slice. ~400 LOC + tests. |
| D  | `impl-005-license-bundle-vfs`           | 7, 8 (US6 + US5)                         | `impl-005-manifest-discovery-cache` | `shared::license::DistributionContext` + `authorized_classes`. `shared::artifact::geometry::FlatGeobufReader` + geometry constants (FR-020f) + producer-side `ingestion/src/artifact/writer/flatgeobuf.rs` rewrite. `shared::sqlite::vfs::open_connection_from_bytes` (cfg-gated: rusqlite for non-wasm32, sqlite-wasm-rs for wasm32) + `shared::sqlite::schema` (FR-020b through FR-020e: constants + DDL + header-validate) + producer-side `ingestion/src/artifact/writer/sqlite.rs` rewrite. `shared::artifact::bundle::Bundle` + `Bundle::open(version_label, &cache, ctx)` + `shared::artifact::bundle_watch` re-export. The largest PR by LOC; the renderer (006) is fully unblocked at the end of this PR. ~700 LOC + tests. |
| E  | `impl-005-wasm-tests-polish`            | 9, 10 (wasm32 tests + Polish)            | `impl-005-license-bundle-vfs` | `wasm-bindgen-test` configuration + duplicate test attributes for `parse_manifest`, `parse_discovery_document`, `verify_sha256`, `Bundle::open`, `open_connection_from_bytes`. Final clippy / fmt / coverage / convention audit. CLAUDE.md update. ~150 LOC. |

Each branch starts with the `>>> branch: <name>` marker commit per the constitution + `feedback_branch_marker_commits` (use `./scripts/branch-init.sh <name>`). Each PR's body matches `feedback_pr_description_style`: opens with a verb describing the change; tight summary prose; no chat narration; no file enumeration; no "Next phase" pointers.

Per `feedback_squash_merge_and_rebase_onto`: PRs integrate via `scripts/pr-integrate.sh`; for any stacked child whose parent was squash-merged, use `git rebase --onto master <former-parent>` to rebase the child onto master.

## Brief PR description (per `feedback_pr_description_style`)

> Adds a new `shared/` Cargo workspace member that compiles for both host (Apple Silicon) and `wasm32-unknown-unknown`. Extracts the canonical-store enums + manifest schema + SHA-256 helpers from `ingestion/`; ingestion imports them from `shared::` directly (no forwarding re-exports), and ingestion's `canonical_model.rs` is renamed to `canonical_entity.rs` once the canonical Models leave. Introduces a new `manifest_schema_version: u32` field on the manifest (first key; v1 = 1) as a forward-compat gate for v2+ shape changes. Defines the cross-platform `ArtifactCache` async trait (stable AFIT, no `async-trait` crate). Defines `DiscoveryDocument` + `parse_discovery_document`. Implements `Bundle::open(version_label, &cache, distribution_context)` returning a `Send + Sync` `Bundle` of pure parsed data (manifest + eagerly-parsed FlatGeobufReader + license-filtered shard bytes; no SQLite Connection — the consuming renderer in 006 opens its own thread-local). Adds `shared::sqlite::schema` as the shared producer / consumer SQLite-shard contract (constants for the `application_id` / `user_version` magic numbers + every table / column / index name + the `PERIOD_DATE_FORMAT`; `shard_schema_ddl()` builds the schema DDL from those constants; `validate_shard_header()` is the consumer-side "is this an Eafora shard with a version I understand?" gate). Producer-side `ingestion/src/artifact/writer/sqlite.rs` now uses those constants instead of its own copies; `ingestion/src/artifact/publish.rs::load_build_report_from_disk` now uses `shared::artifact::manifest::parse_manifest` instead of its private deserializer. Re-exports `tokio::sync::watch` for the bundle hot-swap channel. Ships the SQLite `Vec<u8>`-backed VFS for the wasm32 target. Architecture docs (`client.md` §Manifest schema, `ingestion.md` §Manifest format, `overview.md` §Artifact format) updated to include `manifest_schema_version`. Producer-side tests continue to pass; new test suite covers `parse_manifest`, `parse_discovery_document`, `verify_sha256`, `DistributionContext::authorized_classes`, `Bundle::open`, `validate_shard_header`, and `shard_schema_ddl` on both targets.
