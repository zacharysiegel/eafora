---
description: "Task list for feature 005-core-data implementation"
---

# Tasks: core/ crate — data layer

**Feature**: 005-core-data

**Spec**: `specs/005-core-data/spec.md`

**Plan**: `specs/005-core-data/plan.md`

**Inputs**: `data-model.md`, `contracts/core-public-api.md`, `quickstart.md`

Task ordering reflects the single-PR Phasing in plan.md §Phasing for PRs. Per Constitution VII, tests for the FR-024 / FR-025 surfaces (`parse_manifest`, `parse_discovery_document`, `verify_sha256`, `DistributionContext::authorized_classes`, `Bundle::open`) land BEFORE their implementations within each story phase.

User-story mapping (matches spec.md's P1-P6 priority sections):
- **US1**: Workspace member with extracted producer types (spec P1).
- **US2**: Manifest parsing — consumer side (spec P2).
- **US3**: `ArtifactCache` trait + cross-platform cache contract (spec P3).
- **US4**: Discovery document type + parse (spec P4).
- **US5**: `Bundle` loader + hot-swap channel (spec P5).
- **US6**: License authorization (spec P6).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks).
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3).
- File paths are absolute or repo-rooted.

## Path conventions

Single workspace, Rust monorepo. New crate at `core/`; modifications under `ingestion/`. No `tests/` directory at repo root; all tests live in `#[cfg(test)] mod tests` blocks adjacent to their module OR in `<crate>/tests/<integration_name>.rs` for cross-module integration tests.

---

## Phase 1: Setup (shared infrastructure)

**Purpose**: Workspace + new-crate scaffolding. No user-story work yet.

- [x] T001 Add `"core"` to the workspace `members` array in `/Users/singularity/eafora/Cargo.toml`.
- [x] T002 Create `/Users/singularity/eafora/core/Cargo.toml` with `[package] name = "core", version = "0.0.0", edition.workspace = true, publish = false`; declare `[lib]` (no `[[bin]]`). Dependencies split by target per plan §Topic 2: in `[dependencies]`, the cross-target deps (`serde`, `serde_json`, `chrono`, `uuid`, `sha2`, `flatgeobuf`, `geozero`, `geo-types`, `minimer`, `log`, `bytes`, `tokio = { workspace = true, default-features = false, features = ["sync"] }`). In `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`: `rusqlite = { workspace = true }` (rusqlite's bundled feature does not cross-compile to wasm32 — see plan §Topic 2). In `[target.'cfg(target_arch = "wasm32")'.dependencies]`: `sqlite-wasm-rs = { workspace = true }`, `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys` (resolve `sqlite-wasm-rs` version against latest stable at implementation time and add to root `[workspace.dependencies]`). Add `[dev-dependencies]` with `tempfile` and `tokio = { workspace = true, default-features = false, features = ["sync", "macros", "rt"] }`. Add `[target.'cfg(target_arch = "wasm32")'.dev-dependencies]` with `wasm-bindgen-test`.
- [x] T002a Create `/Users/singularity/eafora/core/build.rs` per spec FR-020k + data-model.md §Crate root. The script runs `git rev-parse HEAD` via `std::process::Command`, emits `cargo:rustc-env=EAFORA_REVISION={revision}`, registers `cargo:rerun-if-changed=.git/HEAD` + `cargo:rerun-if-changed=.git/refs` so revision changes trigger a rebuild. Falls back to emitting `EAFORA_REVISION=unknown` if `git rev-parse` fails or its output is non-UTF-8 (shallow checkout / archive extraction case).
- [x] T003 [P] Add the wasm-bindgen-family pins to `[workspace.dependencies]` in `/Users/singularity/eafora/Cargo.toml`: `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys`, `wasm-bindgen-test` (resolve versions against the latest stable wasm-bindgen and its required wasm-bindgen-test version at implementation time).
- [x] T004 [P] Create `/Users/singularity/eafora/rust-toolchain.toml` pinning `channel = "1.83"` (or the most recent stable at implementation time; floor is 1.75 for stable AFIT per plan.md §Topic 1).
- [x] T005 Create `/Users/singularity/eafora/core/src/lib.rs` containing the `pub mod` declarations + the wildcard re-exports per data-model.md §Public-API surface summary + `pub const REVISION: &str = env!("EAFORA_REVISION");` per FR-020k (the build.rs from T002a emits the env var; the const surfaces it to consumers). Initially the modules point at empty submodules; they get bodies in subsequent phases. Run `cargo check --workspace` to confirm the empty-shell compiles and the `REVISION` env var resolves.

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: AppError + filesystem helpers MUST land before any user-story phase because every other module imports them.

**⚠️ CRITICAL**: No user story work can begin until Phase 2 is complete.

- [x] T006 [Foundational] Create `/Users/singularity/eafora/core/src/error.rs` with `minimer::define_app_error!(pub AppError);` plus `impl_from_error!` for: `serde_json::Error`, `rusqlite::Error` (gated `#[cfg(not(target_arch = "wasm32"))]` since rusqlite is native-only), `flatgeobuf::Error`, `geozero::error::GeozeroError`, `log::SetLoggerError`. Move `render_error_chain(error: &dyn Error) -> String` verbatim from `/Users/singularity/eafora/ingestion/src/error.rs`. Per plan §Outstanding decision #1, `core::AppError` is core's OWN newtype (not shared with ingestion) — Rust's orphan rule blocks ingestion from adding `From<sqlx::Error>` to a core-defined type, so each crate defines its own newtype.
- [x] T007 [Foundational] Rewrite `/Users/singularity/eafora/ingestion/src/error.rs` to keep ingestion's OWN `AppError` newtype: `minimer::define_app_error!(pub AppError);` followed by the `impl_from_error!` calls for the ingestion-only error families: `sqlx::Error`, `reqwest::Error`, `zip::result::ZipError`, `shapefile::Error`, `shapefile::dbase::Error`, `secr::error::Error`, `dotenvy::Error`, `base64::DecodeError`. Add ONE cross-conversion impl: `impl From<core::AppError> for AppError { fn from(err: core::AppError) -> Self { Self(err.0) } }` (orphan-rule-OK because the target type `ingestion::AppError` is local to ingestion; both newtypes wrap the same `minimer::AppError` so the inner `.0` move is correct). Lets ingestion code `?`-propagate from core functions. Move `render_error_chain` to `core::error` (so both crates reach for the same impl); ingestion `pub use core::error::render_error_chain;` if it wants the shorter import.
- [x] T008 [Foundational] Add `core = { workspace = true }` to `/Users/singularity/eafora/ingestion/Cargo.toml`'s `[dependencies]` block.
- [x] T009 [Foundational] Add `core = { path = "core" }` (or equivalent) to `/Users/singularity/eafora/Cargo.toml`'s `[workspace.dependencies]` block so other crates can reach `core` via `{ workspace = true }`.
- [x] T010 [Foundational] Create `/Users/singularity/eafora/core/src/filesystem.rs` by moving the entire body of `/Users/singularity/eafora/ingestion/src/filesystem.rs`. Add `#[cfg(not(target_arch = "wasm32"))]` gates to `sha256_hex_of_file`, `filename_of`, `read_bytes`, `load_hashed_file`. Cross-target items (`FileReference`, `Hashed<T>`, `sha256_hex`) work on both. Add NEW function `verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), AppError>` per spec FR-009: on mismatch, returns `AppError` whose message contains both `expected_hex` (first 8 hex chars) and the actual hash (first 8 hex chars).
- [x] T011 [Foundational] Rewrite `/Users/singularity/eafora/ingestion/src/filesystem.rs` to a single-line `pub use core::filesystem::*;` re-export. (Alternative per plan §Outstanding decision #2: delete the file entirely and add `pub use core::filesystem;` to `/Users/singularity/eafora/ingestion/src/lib.rs` — implementation-time choice. Either path keeps existing `crate::filesystem::*` imports valid throughout ingestion.)
- [x] T012 [Foundational] Run `cargo check --workspace` to confirm ingestion still compiles against the moved error + filesystem types via the re-exports. Address any import-path drift that surfaces (none expected if re-exports are wildcards; per-symbol drift would mean a `crate::filesystem::sha256_hex` call site now resolves to `core::filesystem::sha256_hex` via the re-export, which is correct).
- [x] T013 [Foundational] Run `cargo test -p ingestion` and confirm every pre-existing ingestion test passes against the moved types. Per spec SC-003 + P1 acceptance #3, this is the regression net for the type extraction.

**Checkpoint**: Foundation ready — user story work can begin in parallel.

---

## Phase 3: US1 — Workspace member with extracted producer types (Priority: P1) 🎯 MVP

**Goal**: The new `core/` crate compiles for host + wasm32; canonical-store enums move from ingestion; ingestion `pub use`s them; the type-extraction round-trip is verified.

**Independent test criteria**: `cargo build -p core` succeeds on host; `cargo build -p core --target wasm32-unknown-unknown` succeeds; `cargo test -p ingestion` continues to pass; greps for `pub use core::canonical::canonical_model::*` in `ingestion/src/canonical/canonical_model.rs` find the re-export.

- [ ] T014 [P] [US1] Create `/Users/singularity/eafora/core/src/canonical/mod.rs` with `pub mod canonical_model; pub use canonical_model::*;`.
- [ ] T015 [US1] Create `/Users/singularity/eafora/core/src/canonical/canonical_model.rs` by moving from `/Users/singularity/eafora/ingestion/src/canonical/canonical_model.rs`:
  - The 6 enums + 1 struct: `StatisticKind`, `DataSourceKind`, `DataStatus`, `LicenseClass`, `LicenseShardClass`, `SourceRevision`. Add `Serialize` + `Deserialize` derives to the enums using `#[serde(try_from = "&str", into = "&str")]` (or the appropriate serde attribute that delegates to the existing `TryFrom<&str>` + `code()` / `as_str()` impls).
  - The 4 consumer-facing **Models** (Model half of the Model + Entity pair): `Region`, `Country`, `Statistic`, `DataSource`. Move the struct definitions only — NOT the matching `*Entity` types and NOT the `(Try)From<Entity> for Model` impls (those stay in ingestion with the Entity per `docs/conventions/types.md` §Core dichotomy).
  - Also lift `NaiveDatePeriod` (struct + `from_year` + `to_year`) from `/Users/singularity/eafora/ingestion/src/adapter/adapter_model.rs` per spec FR-005a. Drop the `#[allow(dead_code)]` on `to_year` — it becomes consumer-side live code.
  - The `StatisticValue` + `StatisticValueEntity` + `SourceChoice` + `SourceChoiceEntity` types STAY in ingestion entirely (consumer-side `statistic_value` reads happen against SQLite shards with a different column set; `SourceChoice` is producer-only merge config). The `ArtifactVersion` + `ArtifactVersionEntity` types also stay (producer-only publish bookkeeping).
- [ ] T015a [P] [US1] In `/Users/singularity/eafora/ingestion/src/canonical/canonical_model.rs`, keep the `*Entity` wire-shape types (`RegionEntity`, `CountryEntity`, `StatisticEntity`, `DataSourceEntity`, `StatisticValueEntity`, `SourceChoiceEntity`) and their `(Try)From<Entity> for Model` impls in place. The `From<RegionEntity> for Region` impl now constructs a foreign-from-core type (Region moved); the orphan rule allows this because ingestion owns the Entity. Update the imports at the top of the file to reach `Region`, `Country`, `Statistic`, `DataSource` through the `pub use core::canonical::canonical_model::*;` re-export added in T016. Run `cargo check -p ingestion` to confirm the impls still compile.
- [ ] T015b [P] [US1] In `/Users/singularity/eafora/ingestion/src/adapter/adapter_model.rs`, remove the local `NaiveDatePeriod` struct + impl block; add `pub use core::canonical::canonical_model::NaiveDatePeriod;` at the top of the file so existing `crate::adapter::NaiveDatePeriod` import sites continue to resolve. The other `adapter_model.rs` types (`AdapterOptions`, `NormalizedStatisticValue`, `NormalizeOutcome`, `IngestWarning`, `IngestWarningKind`) stay verbatim — they're producer-only normalization-pipeline types.
- [ ] T016 [US1] Rewrite `/Users/singularity/eafora/ingestion/src/canonical/canonical_model.rs` to `pub use core::canonical::canonical_model::*;` at the top, followed by the `*Entity` definitions (`RegionEntity`, `CountryEntity`, `StatisticEntity`, `DataSourceEntity`, `StatisticValueEntity`, `SourceChoiceEntity`) and their `(Try)From<Entity> for Model` impls (kept per T015a). Also keep the producer-only types: `StatisticValue` Model + `SourceChoice` Model (consumer never sees them per spec FR-005). Per `feedback_wildcard_re_exports`: the wildcard re-export covers the moved Models + enums + `NaiveDatePeriod`; existing `crate::canonical::canonical_model::StatisticKind` / `crate::canonical::canonical_model::Region` call sites resolve through it.
- [ ] T017 [US1] Move `StatisticShardKey` from `/Users/singularity/eafora/ingestion/src/artifact/artifact_model.rs` to `/Users/singularity/eafora/core/src/artifact/bundle.rs` (the file will be created in T031; this task adds the type at the head of that file with `#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]`). Update `ingestion/src/artifact/artifact_model.rs` to `pub use core::artifact::bundle::StatisticShardKey;`. Required for FR-018 + the bundle's `shard_bytes: BTreeMap<StatisticShardKey, Vec<u8>>` field.
- [ ] T018 [US1] Run `cargo build --workspace` and `cargo build -p core --target wasm32-unknown-unknown` to confirm both targets compile cleanly per spec SC-001 + SC-002. If wasm32 fails because of an unexpected sqlx or reqwest dep being pulled transitively, audit `core/Cargo.toml`'s per-target dependency tables and remove offending defaults.
- [ ] T019 [US1] [P] Run `cargo test -p ingestion` to confirm the type extraction didn't regress any existing tests per spec SC-003.

**Checkpoint**: US1 done. Workspace is set up; types are moved; both targets build.

---

## Phase 4: US2 — Manifest parsing (consumer side) (Priority: P2)

**Goal**: `core::artifact::manifest::Manifest` is the canonical owned consumer-side type; `parse_manifest` validates `manifest_schema_version == 1`; the producer-side `write_manifest` is rewritten to use it; the byte-equal round-trip property holds post-move.

**Independent test criteria**: `parse_manifest(write_manifest_output_bytes)` returns a `Manifest` whose fields match the input; `parse_manifest(json_with_schema_version_2)` returns `AppError` containing `"unknown manifest_schema_version 2"`; the producer-side determinism test continues to pass against the new shape.

- [ ] T020 [P] [US2] Create `/Users/singularity/eafora/core/src/artifact/mod.rs` with `pub mod manifest; pub mod bundle; pub mod bundle_watch; pub mod cache; pub mod discovery; pub mod geometry; pub use {manifest,bundle,bundle_watch,cache,discovery,geometry}::*;`. The submodules are empty until later tasks; the module compiles as a forward declaration.
- [ ] T021 [Tests-first] [US2] Write unit tests in `/Users/singularity/eafora/core/src/artifact/manifest.rs::#[cfg(test)] mod tests` per contracts/core-public-api.md §Tests as executable contracts: `parse_manifest_round_trips_fixture_set` (bytes parse + re-serialize byte-equal), `parse_manifest_rejects_unknown_schema_version`, `parse_manifest_rejects_unknown_statistic_code`, `parse_manifest_rejects_malformed_sha256`, `parse_manifest_rejects_path_traversal_relative_path`. Sample manifest bytes inline via `const FIXTURE: &str = r#"{ ... }"#;` since the producer-side `ingestion::artifact::writer::manifest::write_manifest` output shape is documented in data-model.md §Module: `core::artifact::manifest`.
- [ ] T022 [US2] Implement `/Users/singularity/eafora/core/src/artifact/manifest.rs` per data-model.md §Module: `core::artifact::manifest`. Define constants (`MANIFEST_FILENAME`, `MANIFEST_SCHEMA_VERSION = 1`, `SUBDIR_GEOMETRY`, `SUBDIR_DATA`, `MANIFEST_LATEST_KEY` per FR-020h, `CONTENT_TYPE_MANIFEST`/`CONTENT_TYPE_FLATGEOBUF`/`CONTENT_TYPE_SQLITE` per FR-020g, `CACHE_CONTROL_MANIFEST = "public, max-age=300"` and `CACHE_CONTROL_SHARD = "public, max-age=31536000, immutable"` per FR-020j); structs (`Manifest`, `ManifestEntry`) with `#[derive(Debug, Clone, Serialize, Deserialize)]`. `Manifest`'s field declaration order matches the serialization order; `manifest_schema_version` is the FIRST field. Implement `parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError>` per spec FR-012 + §Clarifications Q3: serde-deserialize, validate `manifest_schema_version == MANIFEST_SCHEMA_VERSION` (reject others with the documented `AppError` shape), validate each entry's `sha256` is 64 hex chars, validate each entry's `relative_path` does not contain `..` and does not start with `/`. Make T021 pass.
- [ ] T023 [US2] Rewrite `/Users/singularity/eafora/ingestion/src/artifact/writer/manifest.rs::write_manifest` to construct a `core::artifact::manifest::Manifest` with `manifest_schema_version: 1`, then serialize via `serde_json::to_string_pretty` against that struct. Delete the private `ManifestSerializer<'a>` + `ManifestEntry<'a>` structs. The `relative_path` helper function stays in `ingestion/` (it's producer-only — constructs paths from `Hashed<FileReference>` values that consumers don't see).
- [ ] T024 [Tests-first] [US2] Update the existing producer-side tests in `/Users/singularity/eafora/ingestion/src/artifact/writer/manifest.rs::#[cfg(test)] mod tests`: every assertion that checked the serialized JSON contents now expects `manifest_schema_version: 1` to appear in the output. The `build_manifest_json_is_deterministic_byte_for_byte` test continues to pass against the new shape; the `build_manifest_json_sorts_statistics_alphabetically` and `build_manifest_json_sorts_license_classes_alphabetically_within_statistic` tests continue to pass. Run `cargo test -p ingestion` to confirm.
- [ ] T025 [US2] Run `cargo test -p core` to confirm the T021 tests pass. Run `cargo test -p ingestion` to confirm the producer-side tests pass. Verify spec SC-005's round-trip property: bytes written by ingestion's `write_manifest` parse via `core::artifact::manifest::parse_manifest` and re-serialize byte-equal to the original.
- [ ] T025a [US2] Rewrite `/Users/singularity/eafora/ingestion/src/artifact/publish.rs::load_build_report_from_disk` per spec FR-020e to use `core::artifact::manifest::parse_manifest` instead of its private `ManifestOnDisk` / `ManifestEntryOnDisk` deserializer. Delete `ManifestOnDisk` and `ManifestEntryOnDisk` struct definitions. The function now: reads `manifest.json` bytes via `fs::read`; parses via `core::artifact::manifest::parse_manifest`; iterates `manifest.geometry` + `manifest.statistics` (now using the typed `StatisticKind` / `LicenseShardClass` keys instead of `String` keys + `TryFrom` conversion); calls the existing local `load_hashed_file` helper for each referenced file (compares the file's recomputed SHA-256 to `manifest.geometry.sha256` / `entry.sha256`). The local `load_hashed_file` is a private helper that stays; only the parallel manifest deserializer goes away. ALSO per FR-020g: delete the local `CONTENT_TYPE_FLATGEOBUF` / `CONTENT_TYPE_MANIFEST` / `CONTENT_TYPE_SQLITE` consts at the top of the file; the `repository.put_file(..., CONTENT_TYPE_*)` call sites reach via `core::artifact::manifest::CONTENT_TYPE_*` instead. Run `cargo test -p ingestion` to confirm existing publish-side tests pass against the rewritten function.

**Checkpoint**: US2 done. Manifest schema is owned by core; producer and consumer agree on the shape.

---

## Phase 5: US3 — `ArtifactCache` trait + cross-platform cache contract (Priority: P3)

**Goal**: The `ArtifactCache` async trait + an in-crate `MockArtifactCache` (`#[cfg(test)]`-only) for `core/`'s own tests of `Bundle::open`.

**Independent test criteria**: The trait compiles with stable AFIT (no `async-trait` crate). The `MockArtifactCache` round-trips inserts via the trait surface; tests in later phases consume it.

- [ ] T026 [US3] Create `/Users/singularity/eafora/core/src/artifact/cache.rs` per data-model.md §Module: `core::artifact::cache`. Define the trait `pub trait ArtifactCache { async fn put(...); async fn get(...); async fn list_versions(...); async fn delete_version(...); }` with the exact signatures from data-model.md. No `Send + Sync` bounds.
- [ ] T027 [Tests-first] [US3] In the same file, behind `#[cfg(test)]`, write tests for `MockArtifactCache` (defined in T028): `mock_cache_put_get_round_trip_returns_byte_equal`, `mock_cache_get_missing_returns_none`, `mock_cache_list_versions_returns_inserted_keys`, `mock_cache_delete_version_removes_only_that_version`. The tests don't need spec-level FR coverage; they're confirming the mock works correctly so US5's `Bundle::open` tests can rely on it.
- [ ] T028 [US3] Implement `MockArtifactCache` behind `#[cfg(test)]` per data-model.md (post-edit) — `BTreeMap<(String, String), Vec<u8>>` wrapped in `tokio::sync::Mutex`; constructor `MockArtifactCache::new()` returns an empty mock; convenience `insert(version_label, file_relative_path, bytes)` helper for test-side seeding. Implement `ArtifactCache for MockArtifactCache` with the four async methods. Make T027 pass.
- [ ] T029 [US3] Run `cargo test -p core` to confirm the mock tests pass on the host target. The trait + mock are both `#[cfg(test)]`-relevant; the trait itself is public-API.

**Checkpoint**: US3 done. The cache contract exists; US5 can build on it.

---

## Phase 6: US4 — Discovery document type + parse (Priority: P4)

**Goal**: `DiscoveryDocument` + `parse_discovery_document` with `schema_version == 1` validation. Independent of US5; can land before or after.

**Independent test criteria**: `parse_discovery_document(json_with_v1_shape)` returns the populated struct; `parse_discovery_document(json_with_schema_version_2)` returns `AppError` containing `"unknown schema_version 2"`; absent `sunset` field deserializes to `None`.

- [ ] T030 [Tests-first] [US4] Write unit tests in `/Users/singularity/eafora/core/src/artifact/discovery.rs::#[cfg(test)] mod tests` per contracts/core-public-api.md §Tests: `parse_discovery_document_round_trips_fixture`, `parse_discovery_document_rejects_unknown_schema_version`, `parse_discovery_document_handles_missing_sunset_field`. Sample bytes inline matching `docs/architecture/client.md` §Discovery document shape.
- [ ] T031 [US4] Implement `/Users/singularity/eafora/core/src/artifact/discovery.rs` per data-model.md §Module: `core::artifact::discovery`. Define `DISCOVERY_SCHEMA_VERSION = 1`, `DISCOVERY_URL = "https://eafora.org/discovery"` per FR-020i, `DiscoveryDocument` struct with `#[derive(Debug, Clone, Serialize, Deserialize)]`, fields `schema_version: u32`, `repository_base_url: String`, `minimum_client_version: String`, `sunset: Option<String>`. Implement `parse_discovery_document(bytes: &[u8]) -> Result<DiscoveryDocument, AppError>` per spec FR-015: serde-deserialize, validate `schema_version == DISCOVERY_SCHEMA_VERSION` (reject others with the documented `AppError` shape). Make T030 pass.

**Checkpoint**: US4 done. Discovery flow's consumer-side parse exists.

---

## Phase 7: US6 — License authorization (Priority: P6)

**Goal**: `DistributionContext` enum + `authorized_classes()` returning the static slice per `client.md` §Attaching license shards. Independent of US5 but US5 consumes it.

**Independent test criteria**: `DistributionContext::FirstParty.authorized_classes() == &[Base, NonCommercial, ShareAlike]`; `DistributionContext::Embedded.authorized_classes() == &[Base]`; adding a hypothetical new `LicenseShardClass` variant breaks compilation in `authorized_classes()` (per spec FR-022; verifiable by attempting the addition during implementation as a sanity check, not by a runtime test).

- [ ] T032 [P] [US6] Create `/Users/singularity/eafora/core/src/license/mod.rs` with `pub mod license; pub use license::*;`.
- [ ] T033 [Tests-first] [US6] Write unit tests in `/Users/singularity/eafora/core/src/license/license.rs::#[cfg(test)] mod tests`: `distribution_context_first_party_authorizes_all_classes`, `distribution_context_embedded_authorizes_base_only`. (No runtime test for the compile-error-on-new-variant property; that's a compile-time guarantee enforced by the `match` having no wildcard arm.)
- [ ] T034 [US6] Implement `/Users/singularity/eafora/core/src/license/license.rs` per data-model.md §Module: `core::license::license`. Define `DistributionContext` enum with `FirstParty` + `Embedded` variants (derive `Debug, Clone, Copy, PartialEq, Eq, Hash`). Implement `authorized_classes(self) -> &'static [LicenseShardClass]` as a `match self { ... }` with NO wildcard arm — both arms list the slices explicitly per `docs/architecture/client.md` §Attaching license shards. Make T033 pass.

**Checkpoint**: US6 done. US5's `Bundle::open` can now filter shards by authorized class.

---

## Phase 8: US5 — `Bundle` loader + hot-swap channel (Priority: P5)

**Goal**: `Bundle::open(version_label, &cache, ctx)` reads through the cache, verifies SHA-256s, parses geometry, populates `shard_bytes` filtered by `DistributionContext::authorized_classes()`, returns a `Send + Sync` `Bundle`. The SQLite WASM VFS module + geometry-reader module land here too; both are dependencies of `Bundle::open`. The `bundle_watch` re-export of `tokio::sync::watch` rounds out the hot-swap surface.

**Independent test criteria**: `Bundle::open` against a populated mock cache returns a bundle whose `shard_bytes` matches the manifest entries; a SHA-256 mismatch produces the documented error; an unauthorized shard isn't in `shard_bytes`; `Arc<Bundle>: Send + Sync` (compile-time assertion).

This phase has the most internal sequencing because `Bundle::open` depends on the geometry reader (T036), the SQLite VFS (T037), the manifest parser (US2), the cache trait (US3), and the license matrix (US6) — all of which must be complete before T040.

### SQLite WASM VFS (FR-020)

- [ ] T035 [Tests-first] [US5] Write unit tests in `/Users/singularity/eafora/core/src/sqlite/vfs.rs::#[cfg(test)] mod tests` (host + wasm32 via `wasm-bindgen-test`): `open_connection_from_bytes_can_run_select_against_known_bytes` — construct a small SQLite DB via `rusqlite::Connection::open_in_memory()`, serialize it to bytes via `Connection::serialize`, then pass those bytes back to `open_connection_from_bytes("test", bytes)` and run a `SELECT` to confirm the data is accessible. Same test runs on both targets; the public API surface is identical.
- [ ] T036 [US5] Create `/Users/singularity/eafora/core/src/sqlite/mod.rs` with `pub mod vfs; pub mod schema; pub use {vfs,schema}::*;`.
- [ ] T037 [US5] Implement `/Users/singularity/eafora/core/src/sqlite/vfs.rs` per data-model.md §Module: `core::sqlite::vfs`. Public surface: cfg-gated `Connection` typedef (`pub type Connection = rusqlite::Connection;` on native, `pub type Connection = sqlite_wasm_rs::Connection;` on wasm32 — verify the exact crate path against the pinned `sqlite-wasm-rs` version), and `open_connection_from_bytes(name: &str, bytes: Vec<u8>) -> Result<Connection, AppError>` with the same signature on both targets. Native impl uses `rusqlite::Connection::deserialize`. wasm32 impl uses `sqlite-wasm-rs`'s equivalent (resolve at implementation time — likely `Connection::open_in_memory` followed by a `restore` call against the bytes; verify against the pinned version's API). Where rusqlite and sqlite-wasm-rs API surfaces diverge for any subsequent operation the renderer needs, add a thin facade function with one signature in `core::sqlite::vfs` rather than cfg-branching the renderer code. Make T035 pass on both targets.

### Geometry reader (FR-020a)

- [ ] T038 [Tests-first] [US5] Write unit tests in `/Users/singularity/eafora/core/src/artifact/geometry.rs::#[cfg(test)] mod tests`: `open_flatgeobuf_reader_parses_known_fixture` — embed a small fixture FlatGeobuf via `include_bytes!` (one feature, e.g. a 2-point square polygon; can be generated once via `flatgeobuf`'s writer and committed); assert `open_flatgeobuf_reader(bytes.to_vec())` returns a reader whose `iter_features()` yields exactly one Feature with the expected `iso3` + `name_en` + bbox. (The fixture file lives at `core/tests/samples/tiny.fgb`; commit it as a generated artifact since reproducing it requires the upstream `flatgeobuf` writer which we don't run from `core/`.)
- [ ] T039 [US5] Implement `/Users/singularity/eafora/core/src/artifact/geometry.rs` per data-model.md §Module: `core::artifact::geometry`. Define the constants per FR-020f (`GEOMETRY_LAYER_NAME`, `GEOMETRY_FILENAME_STEM`, `FEATURE_COLUMN_ISO3`, `FEATURE_COLUMN_NAME_EN`, `SHARD_FILENAME_EXTENSION`, `GEOMETRY_FILENAME_EXTENSION`); define `Feature`, `Polygon`, `BoundingBox` structs per the data-model spec. Implement `FlatGeobufReader` wrapping `flatgeobuf::FgbReader<std::io::Cursor<Vec<u8>>>`; expose `iter_features() -> impl Iterator<Item = Result<Feature, AppError>>` (uses `FEATURE_COLUMN_ISO3` + `FEATURE_COLUMN_NAME_EN` to read property values from each feature) and `features_in_bbox(bbox: BoundingBox) -> impl Iterator<Item = Result<Feature, AppError>>` (the bbox-query exercises the R-tree spatial index that 006's hit-test path consumes). Implement `open_flatgeobuf_reader(bytes: Vec<u8>) -> Result<FlatGeobufReader, AppError>` — parse the bytes into the upstream reader eagerly; any parse error returns `AppError`. Make T038 pass.
- [ ] T039d [US5] Rewrite `/Users/singularity/eafora/ingestion/src/artifact/writer/flatgeobuf.rs` per spec FR-020f to use the moved constants from `core::artifact::geometry`: `GEOMETRY_LAYER_NAME`, `GEOMETRY_FILENAME_STEM`, `FEATURE_COLUMN_ISO3` (replacing the local `COLUMN_ISO3.name`), `FEATURE_COLUMN_NAME_EN` (replacing the local `COLUMN_NAME_EN.name`), `GEOMETRY_FILENAME_EXTENSION` (used in the `format!("{}.tmp-{}.fgb", ...)` filename construction; replace with `format!("{}.tmp-{}.{}", GEOMETRY_FILENAME_STEM, tmp_uuid, GEOMETRY_FILENAME_EXTENSION)`). Delete the local `pub const GEOMETRY_LAYER_NAME` / `pub const GEOMETRY_FILENAME_STEM` consts; keep the local `Column` struct + `COLUMN_ISO3` / `COLUMN_NAME_EN` const struct values (they bind the producer-side index `0` / `1` to the moved name constants — `COLUMN_ISO3 = Column { index: 0, name: FEATURE_COLUMN_ISO3 }`). Existing producer-side tests continue to pass.

### SQLite schema contract (FR-020b through FR-020e)

- [ ] T039a [Tests-first] [US5] Write unit tests in `/Users/singularity/eafora/core/src/sqlite/schema.rs::#[cfg(test)] mod tests` per contracts/core-public-api.md §Tests: `shard_schema_ddl_creates_expected_tables_and_index` (execute DDL against in-memory rusqlite Connection; assert tables + columns + index via `sqlite_master` queries that reference the column-name constants); `validate_shard_header_accepts_correctly_initialized_connection` (open in-memory Connection; set `application_id` + `user_version` PRAGMAs to `APPLICATION_ID` + `SCHEMA_VERSION`; assert `Ok(())`); `validate_shard_header_rejects_wrong_application_id` (set to `0xDEADBEEF`; assert `AppError` whose message starts with `"sqlite shard: application_id mismatch"`); `validate_shard_header_rejects_unknown_schema_version` (set `user_version` to `99`; assert `AppError` whose message starts with `"sqlite shard: unknown schema_version"`).
- [ ] T039b [US5] Implement `/Users/singularity/eafora/core/src/sqlite/schema.rs` per data-model.md §Module: `core::sqlite::schema`. Define every constant (`APPLICATION_ID`, `SCHEMA_VERSION`, `TABLE_*`, `INDEX_*`, every `COL_*`, `PERIOD_DATE_FORMAT`). Implement `shard_schema_ddl() -> &'static str` — either via `const_format::concatcp!` composing the DDL string from the column-name constants at compile time (preferred; `const_format` is already in workspace deps; the DDL is then a true compile-time constant), or via a runtime-built `String` joined from the constants returned through a helper that the caller doesn't see (acceptable fallback if `concatcp!` proves awkward for this size of string). Implement `validate_shard_header(connection: &rusqlite::Connection) -> Result<(), AppError>` per spec FR-020c: read `application_id` PRAGMA via `connection.pragma_query_value(None, "application_id", |row| row.get::<_, i32>(0))?` and compare; same for `user_version`; documented error message prefixes on mismatch. Make T039a pass.
- [ ] T039c [US5] Rewrite `/Users/singularity/eafora/ingestion/src/artifact/writer/sqlite.rs` per spec FR-020d to use `core::sqlite::schema`'s constants + `shard_schema_ddl()` instead of its private `SQLITE_APPLICATION_ID` / `SQLITE_USER_VERSION` / `create_schema` / hand-written SQL. Delete the local `SQLITE_APPLICATION_ID`, `SQLITE_USER_VERSION`, `create_schema` items. Replace the `create_schema` call with `connection.execute_batch(core::sqlite::schema::shard_schema_ddl())`. The `insert_shard_key` and `insert_rows` SQL strings reference the column-name constants from `core::sqlite::schema` via `const_format::formatcp!` (e.g. `formatcp!("insert into {} ({}, {}) values (?1, ?2)", TABLE_SHARD_KEY, COL_STATISTIC_KIND, COL_LICENSE_SHARD_CLASS)`). The PRAGMA writes (`application_id`, `user_version`) use the `APPLICATION_ID` and `SCHEMA_VERSION` constants. The existing producer-side tests in `writer/sqlite.rs::#[cfg(test)] mod tests` continue to pass against the post-rewrite shape; only the source of the magic values changes, not their values.

### Bundle + hot-swap (FR-018, FR-019, FR-023)

- [ ] T040 [P] [US5] Create `/Users/singularity/eafora/core/src/artifact/bundle_watch.rs` with `pub use tokio::sync::watch::{Sender, Receiver, channel};` per data-model.md §Module: `core::artifact::bundle_watch`.
- [ ] T041 [Tests-first] [US5] Write unit tests in `/Users/singularity/eafora/core/src/artifact/bundle.rs::#[cfg(test)] mod tests` per contracts/core-public-api.md §Tests: `bundle_open_round_trip_against_mock_cache`, `bundle_open_rejects_missing_manifest`, `bundle_open_rejects_sha256_mismatch`, `bundle_open_skips_unauthorized_shards`, `bundle_open_eagerly_parses_geometry`, `bundle_is_send_sync` (compile-time via `fn assert_send_sync<T: Send + Sync>() {}` then `assert_send_sync::<Arc<Bundle>>();`). Tests build fixture manifests + shard bytes via the `MockArtifactCache` (US3) + a tiny geometry FlatGeobuf (reuse the fixture from T038's `core/tests/samples/tiny.fgb` or generate a 1-shard variant).
- [ ] T042 [US5] Implement `/Users/singularity/eafora/core/src/artifact/bundle.rs` per data-model.md §Module: `core::artifact::bundle`. Define `Bundle` struct with fields `manifest: Manifest`, `geometry_reader: FlatGeobufReader`, `shard_bytes: BTreeMap<StatisticShardKey, Vec<u8>>`, `distribution_context: DistributionContext`. Verify `Bundle: Send + Sync` compiles. Implement `Bundle::open(version_label: &str, cache: &dyn ArtifactCache, distribution_context: DistributionContext) -> Result<Bundle, AppError>` per spec FR-019 + the algorithm in spec.md §Bundle loader paragraph: (1) `cache.get(version_label, MANIFEST_FILENAME)` returning Err if Ok(None); (2) `parse_manifest`; (3) `cache.get(version_label, &manifest.geometry.relative_path)` + `verify_sha256` + `open_flatgeobuf_reader`; (4) compute `distribution_context.authorized_classes()`; (5) iterate `manifest.statistics`, skipping unauthorized license_shard_class values, calling `cache.get` + `verify_sha256` for each authorized shard, inserting into `shard_bytes` keyed by `StatisticShardKey`; (6) return the constructed Bundle. Make T041 pass on the host target.
- [ ] T043 [US5] Run `cargo test -p core` to confirm all the Bundle::open tests pass on the host target.

**Checkpoint**: US5 done. The full bundle-loading + hot-swap shape exists.

---

## Phase 9: WASM-target test coverage (FR-025)

**Purpose**: Run a subset of the core tests on `wasm32-unknown-unknown` via `wasm-bindgen-test --headless --chrome` to confirm the cfg-gating actually works.

- [ ] T044 Add `wasm-bindgen-test` configuration to `/Users/singularity/eafora/core/src/lib.rs` (or to a dedicated test entrypoint file under `core/tests/`): `#![cfg(all(test, target_arch = "wasm32"))] use wasm_bindgen_test::*; wasm_bindgen_test_configure!(run_in_browser);` per the `wasm-bindgen-test` documentation. Verify the harness invocation (`cargo test -p core --target wasm32-unknown-unknown` or `wasm-pack test --headless --chrome --package core`) at implementation time; the exact invocation depends on the pinned wasm-bindgen-test version's CLI shape.
- [ ] T045 [P] Duplicate the relevant tests with `#[cfg(target_arch = "wasm32")] #[wasm_bindgen_test]` annotations: `parse_manifest_round_trips_fixture_set`, `parse_discovery_document_round_trips_fixture`, `verify_sha256_accepts_matching_hash`, `bundle_open_round_trip_against_mock_cache`, `open_connection_from_bytes_can_run_select_against_known_bytes`. Same test bodies; different attribute. Per `feedback_reuse_constants_no_magic_restating`, the fixture bytes / SHA-256 hashes are shared constants between host + wasm32 tests.
- [ ] T046 Run the wasm32 tests via the wasm-bindgen-test harness and confirm they pass. Per spec SC-002 + SC-004's wasm32 coverage.

---

## Phase 10: Polish + cross-cutting

- [ ] T047 [P] Run `cargo clippy --workspace --all-targets -- -D warnings` and address any new lints introduced by the move.
- [ ] T048 [P] Run `cargo fmt --all` and confirm no diffs (workspace `rustfmt.toml` already covers `core/`).
- [ ] T049 [P] Run `cargo build --workspace` + `cargo build -p core --target wasm32-unknown-unknown` + `cargo test --workspace` + the wasm32 test harness from T046 one final time as a pre-PR sanity check. Verify spec SC-001 + SC-002 + SC-003 + SC-004 all pass.
- [ ] T050 [P] Run `cargo llvm-cov -p core` and verify line coverage on `parse_manifest`, `parse_discovery_document`, `verify_sha256`, `DistributionContext::authorized_classes`, `Bundle::open` is ≥90% per spec SC-004 (informational; gate is the pass count, not the coverage number).
- [ ] T051 [P] Audit touched files against the project's conventions per `feedback_audit_conformance_before_handoff`: no em dashes in code comments; log messages use `<message>; [key=value]` format per `docs/conventions/logging.md` (no log calls in `core/` should need brackets — `Bundle::open` failure paths log via the `AppError` itself; consumer-side surfacing happens in 003 / 004); explicit type annotations on `let` bindings per `~/.claude/CLAUDE.md`'s "Always specify explicit types" rule; no `..Default::default()` struct update syntax; `mod.rs` files contain only `pub mod` + `pub use` per `feedback_mod_rs_holds_only_declarations`; wildcard re-exports per `feedback_wildcard_re_exports`; static variables (the const `MANIFEST_SCHEMA_VERSION` / `DISCOVERY_SCHEMA_VERSION`) at top of file after imports.
- [ ] T052 [P] Manual verification of plan.md §Brief PR description coverage: confirm every claimed deliverable lands (workspace member exists; types extracted with `pub use` re-export from ingestion; `manifest_schema_version` is the first field of the manifest; `ArtifactCache` trait uses stable AFIT; `Bundle::open` takes `version_label` (not a path); `Bundle: Send + Sync`; SQLite VFS module exists for wasm32; `bundle_watch` re-exports tokio::sync::watch; architecture docs already updated). Update plan.md with a "Status note (post-implementation review)" section documenting any deviations per the convention in `feedback_pr_description_style` + the `002-artifact-builder/plan.md` reference.
- [ ] T053 Open a draft PR via `gh pr create` against `master` per `feedback_branch_per_body_of_work` and `feedback_no_new_pr_for_review_feedback`. PR description follows `feedback_pr_description_style`: opens with a verb, tight summary prose, no chat narration / no file enumeration / no "Next phase" pointers. Assign `zacharysiegel` immediately after creation per the user-memory rule. Per `feedback_spec_and_plan_same_pr`: the PR includes spec + plan + tasks + research (embedded in plan) + data-model + contracts + quickstart + the implementation + tests.

---

## Dependencies & execution order

```text
Phase 1 (T001-T005, setup) → ready
Phase 2 (T006-T013, foundational) → ready (AppError + filesystem move; regression check)
  ↓
  Phase 3 (US1, T014-T019)   ← ready after Phase 2
       ↓
       Phase 4 (US2, T020-T025)   ← needs Phase 3 (the moved types)
            ↓                                ↘
            Phase 5 (US3, T026-T029)         Phase 6 (US4, T030-T031)   ← parallel after Phase 4
            Phase 7 (US6, T032-T034)         ↙
                  ↓                         ↙
                  Phase 8 (US5, T035-T043)  ← needs US2 + US3 + US6 (and the geometry / VFS / SQLite-schema internal sub-phases)
                        ↓
                        Phase 9 (T044-T046, wasm32 tests)
                              ↓
                              Phase 10 (T047-T053, polish + PR)
```

## Parallel execution within each phase

- **Phase 1**: T003 (workspace deps) and T004 (rust-toolchain.toml) touch different files — drafted in parallel.
- **Phase 2**: T006 (core::error) and T010 (core::filesystem) touch different files — drafted in parallel; T007 (ingestion::error rewrite) depends on T006; T011 (ingestion::filesystem rewrite) depends on T010.
- **Phase 3 (US1)**: T014 (mod.rs) and T015 (canonical_model.rs) touch different files — drafted in parallel; T015a (Entity-impl review in ingestion's canonical_model.rs) depends on T015 + T016 sequence; T015b (adapter_model.rs NaiveDatePeriod re-export) is parallel to T015a (different file); T016 (ingestion canonical_model rewrite) depends on T015; T017 (StatisticShardKey move) is independent of the canonical-model move.
- **Phase 5 (US3)**: T026 (trait) and T028 (mock) touch the same file; sequential within the file.
- **Phase 7 (US6)**: T032 (mod.rs) and T034 (license.rs) touch different files — drafted in parallel; T033 (tests) lives in the same file as T034.
- **Phase 8 (US5)**: T035-T037 (SQLite VFS), T038-T039 (geometry reader), and T039a-T039c (SQLite schema contract) touch different files — three fully parallel sub-phases. All three must complete before T040-T043 (bundle). T039c (producer-side `writer/sqlite.rs` rewrite) depends on T039b (the `core::sqlite::schema` constants + DDL function); apart from that, the schema sub-phase is independent of the VFS sub-phase.
- **Phase 10**: T047, T048, T049, T050, T051, T052 are all parallelizable (different commands, mostly read-only).

## Implementation strategy

Single PR per plan.md §Phasing. Total task count: 61. Estimated effort: ~2.5 days of focused work (the type extraction is mechanical; the WASM VFS + geometry-reader fixtures + the SQLite-schema producer-rewrite are the time sinks). The MVP-shaped subset is US1 + US2 (Phases 1-4) — that's enough to verify the workspace member compiles + types extracted cleanly + manifest parses end-to-end + the producer-side publish flow no longer carries a parallel manifest deserializer. US3 + US4 + US6 are small (~3 tasks each); US5 (Bundle::open + SQLite VFS + geometry reader + SQLite schema contract + producer-side flatgeobuf rewrite) is the largest phase because it depends on every prior story AND introduces the cross-cutting producer-consumer SQLite contract.
