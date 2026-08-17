---
description: "Task list for feature 005-core-data implementation"
---

# Tasks: shared/ crate — data layer

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

Single workspace, Rust monorepo. New crate at `shared/`; modifications under `ingestion/`. No `tests/` directory at repo root; all tests live in `#[cfg(test)] mod tests` blocks adjacent to their module OR in `<crate>/tests/<integration_name>.rs` for cross-module integration tests.

---

## Phase 1: Setup (shared infrastructure)

**Purpose**: Workspace + new-crate scaffolding. No user-story work yet.

- [x] T001 Add `"shared"` to the workspace `members` array in `/Users/singularity/eafora/Cargo.toml`.
- [x] T002 Create `/Users/singularity/eafora/shared/Cargo.toml` with `[package] name = "shared", version = "0.0.0", edition.workspace = true, publish = false`; declare `[lib]` (no `[[bin]]`). Dependencies split by target per plan §Topic 2: in `[dependencies]`, the cross-target deps (`serde`, `serde_json`, `chrono`, `uuid`, `sha2`, `flatgeobuf`, `geozero`, `geo-types`, `minimer`, `log`, `bytes`, `tokio = { workspace = true, default-features = false, features = ["sync"] }`). In `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`: `rusqlite = { workspace = true }` (rusqlite's bundled feature does not cross-compile to wasm32 — see plan §Topic 2). In `[target.'cfg(target_arch = "wasm32")'.dependencies]`: `sqlite-wasm-rs = { workspace = true }`, `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys` (resolve `sqlite-wasm-rs` version against latest stable at implementation time and add to root `[workspace.dependencies]`). Add `[dev-dependencies]` with `tempfile` and `tokio = { workspace = true, default-features = false, features = ["sync", "macros", "rt"] }`. Add `[target.'cfg(target_arch = "wasm32")'.dev-dependencies]` with `wasm-bindgen-test`.
- [x] T002a Create `/Users/singularity/eafora/shared/build.rs` per spec FR-020k + data-model.md §Crate root. The script runs `git rev-parse HEAD` via `std::process::Command`, emits `cargo:rustc-env=EAFORA_REVISION={revision}`, and emits NO `cargo:rerun-if-changed` directives (the `.git/HEAD` path-watching is brittle; release builds are clean builds that re-run the script regardless). Git-unavailable handling is profile-conditional (gated on `PROFILE != "debug"`): debug builds emit a `cargo:warning` and fall back to `EAFORA_REVISION=unknown`; release builds `panic!` (abort) so a shipped binary never carries `unknown`.
- [x] T003 [P] Add the wasm-bindgen-family pins to `[workspace.dependencies]` in `/Users/singularity/eafora/Cargo.toml`: `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys`, `wasm-bindgen-test` (resolve versions against the latest stable wasm-bindgen and its required wasm-bindgen-test version at implementation time).
- [x] T004 [P] Create `/Users/singularity/eafora/rust-toolchain.toml` pinning `channel = "1.83"` (or the most recent stable at implementation time; floor is 1.75 for stable AFIT per plan.md §Topic 1).
- [x] T005 Create `/Users/singularity/eafora/shared/src/lib.rs` containing ONLY the `pub mod` declarations + the wildcard re-exports per data-model.md §Public-API surface summary (per `feedback_mod_rs_holds_only_declarations`, no definitions in `lib.rs`). Create `/Users/singularity/eafora/shared/src/revision.rs` holding `pub const REVISION: &str = env!("EAFORA_REVISION");` per FR-020k (the build.rs from T002a emits the env var; the const surfaces it to consumers); `lib.rs` re-exports it via `pub use revision::*;`. Initially the modules point at empty submodules; they get bodies in subsequent phases. Run `cargo check --workspace` to confirm the empty-shell compiles and the `REVISION` env var resolves.

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: AppError + filesystem helpers MUST land before any user-story phase because every other module imports them.

**⚠️ CRITICAL**: No user story work can begin until Phase 2 is complete.

- [x] T006 [Foundational] Create `/Users/singularity/eafora/shared/src/error.rs` with `minimer::define_app_error!(pub AppError);` plus `impl_from_error!` for: `serde_json::Error`, `rusqlite::Error` (gated `#[cfg(not(target_arch = "wasm32"))]` since rusqlite doesn't compile to wasm32), `flatgeobuf::Error`, `geozero::error::GeozeroError`, `log::SetLoggerError`. Move `render_error_chain(error: &dyn Error) -> String` verbatim from `/Users/singularity/eafora/ingestion/src/error.rs`. Per plan §Outstanding decision #1, `shared::AppError` is shared's OWN newtype (not shared with ingestion) — Rust's orphan rule blocks ingestion from adding `From<sqlx::Error>` to a shared-defined type, so each crate defines its own newtype.
- [x] T007 [Foundational] Rewrite `/Users/singularity/eafora/ingestion/src/error.rs` to keep ingestion's OWN `AppError` newtype: `minimer::define_app_error!(pub AppError);` followed by the `impl_from_error!` calls for the ingestion-only error families: `sqlx::Error`, `reqwest::Error`, `zip::result::ZipError`, `shapefile::Error`, `shapefile::dbase::Error`, `secr::error::Error`, `dotenvy::Error`, `base64::DecodeError`. Add ONE cross-conversion impl: `impl From<shared::AppError> for AppError { fn from(err: shared::AppError) -> Self { Self(err.0) } }` (orphan-rule-OK because the target type `ingestion::AppError` is local to ingestion; both newtypes wrap the same `minimer::AppError` so the inner `.0` move is correct). Lets ingestion code `?`-propagate from shared functions. `render_error_chain` moves to `shared::error` (so both crates reach for the same impl); ingestion call sites reference `shared::error::render_error_chain` directly (no re-export per feedback on forwarding declarations).
- [x] T008 [Foundational] Add `shared = { workspace = true }` to `/Users/singularity/eafora/ingestion/Cargo.toml`'s `[dependencies]` block.
- [x] T009 [Foundational] Add `shared = { path = "shared" }` (or equivalent) to `/Users/singularity/eafora/Cargo.toml`'s `[workspace.dependencies]` block so other crates can reach `shared` via `{ workspace = true }`.
- [x] T010 [Foundational] Create `/Users/singularity/eafora/shared/src/filesystem.rs` by moving the entire body of `/Users/singularity/eafora/ingestion/src/filesystem.rs`. Add `#[cfg(not(target_arch = "wasm32"))]` gates to `FileReference`, `sha256_hex_of_file`, `filename_of`, `read_bytes`, `load_hashed_file` (each touches `std::path` / `std::fs`; gated off wasm32). Cross-target items (`Hashed<T>`, `sha256_hex`) work on both. Add NEW function `verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), AppError>` per spec FR-009: on mismatch, returns `AppError` whose message contains both `expected_hex` (first 8 hex chars) and the actual hash (first 8 hex chars).
- [x] T011 [Foundational] Delete `/Users/singularity/eafora/ingestion/src/filesystem.rs` and remove `pub mod filesystem;` from `/Users/singularity/eafora/ingestion/src/lib.rs`. Migrate the call sites (`artifact/artifact_model.rs`, `artifact/hashing.rs`, `artifact/artifact.rs`, `artifact/writer/sqlite.rs`, `artifact/publish.rs`, `artifact/writer/flatgeobuf.rs`, `artifact/writer/manifest.rs`) from `crate::filesystem::*` to `shared::filesystem::*` directly. No re-export (per feedback on forwarding declarations).
- [x] T012 [Foundational] Run `cargo check --workspace` to confirm ingestion still compiles against the moved error + filesystem types via direct `shared::` imports. Address any import-path drift that surfaces (a former `crate::filesystem::sha256_hex` call site now reads `shared::filesystem::sha256_hex`, which is correct).
- [x] T013 [Foundational] Run `cargo test -p ingestion` and confirm every pre-existing ingestion test passes against the moved types. Per spec SC-003 + P1 acceptance #3, this is the regression net for the type extraction.

**Checkpoint**: Foundation ready — user story work can begin in parallel.

---

## Phase 3: US1 — Workspace member with extracted producer types (Priority: P1) 🎯 MVP

**Goal**: The new `shared/` crate compiles for host + wasm32; canonical-store enums move from ingestion; ingestion `pub use`s them; the type-extraction round-trip is verified.

**Independent test criteria**: `cargo build -p shared` succeeds on host; `cargo build -p shared --target wasm32-unknown-unknown` succeeds; `cargo test -p ingestion` continues to pass; greps for `pub use shared::canonical` / `pub use shared::artifact::bundle::StatisticShardKey` in `ingestion/` find NONE (no forwarding re-exports — call sites reference `shared::` paths directly).

- [x] T014 [P] [US1] Create `/Users/singularity/eafora/shared/src/canonical/mod.rs` with `pub mod canonical_model; pub use canonical_model::*;`.
- [x] T015 [US1] Create `/Users/singularity/eafora/shared/src/canonical/canonical_model.rs` by moving from `/Users/singularity/eafora/ingestion/src/canonical/canonical_model.rs`:
  - The 5 enums + 1 struct: `StatisticKind`, `DataSourceKind`, `DataStatus`, `LicenseClass`, `LicenseShardClass`, `SourceRevision`. The enums get `Serialize` / `Deserialize` via a local `impl_code_serde!` macro that delegates to each enum's existing `code()` / `as_str()` + `TryFrom<&str>` (serialize as the string code; deserialize an owned `String`, then `TryFrom`). **Deviation**: this replaces the originally-prescribed `#[serde(try_from = "&str", into = "&str")]` — `&str` deserialization fails on owned / escaped / map-key input, and `into = "&str"` needs an `Into<&str>` impl the spec never supplied. The macro keeps the code strings defined once, on each enum's own impls.
  - The 4 consumer-facing **Models** (Model half of the Model + Entity pair): `Region`, `Country`, `Statistic`, `DataSource`. Move the struct definitions only — NOT the matching `*Entity` types and NOT the `(Try)From<Entity> for Model` impls (those stay in ingestion with the Entity per `docs/conventions/types.md` §Core dichotomy).
  - Also lift `NaiveDatePeriod` (struct + `from_year` + `to_year`) from `/Users/singularity/eafora/ingestion/src/adapter/adapter_model.rs` per spec FR-005a. Drop the `#[allow(dead_code)]` on `to_year` — it becomes consumer-side live code.
  - The `StatisticValue` + `StatisticValueEntity` + `SourceChoice` + `SourceChoiceEntity` types STAY in ingestion entirely (consumer-side `statistic_value` reads happen against SQLite shards with a different column set; `SourceChoice` is producer-only merge config). The `ArtifactVersion` + `ArtifactVersionEntity` types also stay (producer-only publish bookkeeping).
  - **Deviation**: the test-only enum variants (`StatisticKind::TestAlpha`, `DataSourceKind::TestAlpha` / `TestBeta`) are unconditional (not `#[cfg(test)]`-gated) so dependent crates can construct them in tests with no cross-crate feature plumbing. Only the inbound parse arms (the `TryFrom<&str>` arms, which serde `Deserialize` routes through) are gated `#[cfg(test)]`, so a production build still rejects `"_test_alpha"` / `"_test_beta"` as unknown codes. (Earlier iterations gated the whole variants behind a `testing` Cargo feature; that was dropped — nothing parses the test codes cross-crate, so the variants can be plain construction-only values and the feature is unnecessary.)
- [x] T015a [P] [US1] In `/Users/singularity/eafora/ingestion/src/canonical/canonical_model.rs`, keep the `*Entity` wire-shape types (`RegionEntity`, `CountryEntity`, `StatisticEntity`, `DataSourceEntity`, `StatisticValueEntity`, `SourceChoiceEntity`) and their `(Try)From<Entity> for Model` impls in place. The `From<RegionEntity> for Region` impl now constructs a foreign-from-shared type (Region moved); the orphan rule allows this because ingestion owns the Entity. Add `use shared::canonical::canonical_model::{Region, Country, Statistic, DataSource};` at the top of the file (direct import, no re-export). Run `cargo check -p ingestion` to confirm the impls still compile.
- [x] T015b [P] [US1] In `/Users/singularity/eafora/ingestion/src/adapter/adapter_model.rs`, remove the local `NaiveDatePeriod` struct + impl block. Migrate every `crate::adapter::NaiveDatePeriod` / `crate::adapter::adapter_model::NaiveDatePeriod` call site across ingestion to `shared::canonical::canonical_model::NaiveDatePeriod` directly (no re-export). The other `adapter_model.rs` types (`AdapterOptions`, `NormalizedStatisticValue`, `NormalizeOutcome`, `IngestWarning`, `IngestWarningKind`) stay verbatim — they're producer-only normalization-pipeline types.
- [x] T016 [US1] Rewrite `/Users/singularity/eafora/ingestion/src/canonical/canonical_model.rs` to hold ONLY the producer-side types: the `*Entity` definitions (`RegionEntity`, `CountryEntity`, `StatisticEntity`, `DataSourceEntity`, `StatisticValueEntity`, `SourceChoiceEntity`) and their `(Try)From<Entity> for Model` impls (kept per T015a), plus the producer-only Models `StatisticValue` + `SourceChoice` (consumer never sees them per spec FR-005). NO `pub use shared::...` re-export. Migrate every ingestion call site that referenced a moved type (`crate::canonical::canonical_model::StatisticKind`, `::Region`, `::Country`, `::Statistic`, `::DataSource`, the moved enums, `NaiveDatePeriod`) to the corresponding `shared::canonical::canonical_model::*` path directly. **Rename** (done): the file/module landed as `canonical_entity.rs` (the canonical Models left, so `canonical_model` was a misnomer); `ingestion/src/canonical/mod.rs` and the migrated call-site paths use the new name.
- [x] T017 [US1] Move `StatisticShardKey` from `/Users/singularity/eafora/ingestion/src/artifact/artifact_model.rs` to `/Users/singularity/eafora/shared/src/artifact/bundle.rs` (PR B creates `bundle.rs` holding only this type; the `Bundle` struct + `open` arrive in PR D / T031-T042) with `#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]`. Migrate ingestion's `StatisticShardKey` call sites to `shared::artifact::bundle::StatisticShardKey` directly (no re-export). **Deviation**: the `StatisticShardKey::from_value(&ResolvedValue)` constructor moved to a `ResolvedValue::shard_key(&self) -> StatisticShardKey` method — `ResolvedValue` is ingestion-only and ingestion cannot add an inherent impl to the now-foreign `StatisticShardKey`. Required for FR-018 + the bundle's `shard_bytes: BTreeMap<StatisticShardKey, Vec<u8>>` field.
- [x] T018 [US1] Run `cargo build --workspace` and `cargo build -p shared --target wasm32-unknown-unknown` to confirm both targets compile cleanly per spec SC-001 + SC-002. If wasm32 fails because of an unexpected sqlx or reqwest dep being pulled transitively, audit `shared/Cargo.toml`'s per-target dependency tables and remove offending defaults.
- [x] T019 [US1] [P] Run `cargo test --workspace` to confirm the type extraction didn't regress any existing tests per spec SC-003. **No feature flag is needed**: the test-only enum variants are unconditional (construction-only; their `TryFrom` parse arms are `#[cfg(test)]`-gated), and `main.rs`'s source-kind match handles `TestAlpha | TestBeta` with an explicit "no adapter for test-only source" arm (listed explicitly, not a catch-all, so a future real source still breaks the match). Also fixed `ingestion/tests/` integration tests that still referenced the PR-A-deleted `ingestion::filesystem` (PR A only ran `--lib`, so the `tests/` targets never recompiled). Result: 43 tests green (29 lib + 14 integration, 1 ignored).

**Checkpoint**: US1 done. Workspace is set up; types are moved; both targets build.

---

## Phase 4: US2 — Manifest parsing (consumer side) (Priority: P2)

**Goal**: `shared::artifact::manifest::Manifest` is the canonical owned consumer-side type; `parse_manifest` validates `manifest_schema_version == 1`; the producer-side `write_manifest` is rewritten to use it; the byte-equal round-trip property holds post-move.

**Independent test criteria**: `parse_manifest(write_manifest_output_bytes)` returns a `Manifest` whose fields match the input; `parse_manifest(json_with_schema_version_2)` returns `AppError` containing `"unknown manifest_schema_version 2"`; the producer-side determinism test continues to pass against the new shape.

- [x] T020 [P] [US2] Update `/Users/singularity/eafora/shared/src/artifact/mod.rs` to declare the submodules this feature implements: `pub mod bundle; pub mod cache; pub mod discovery; pub mod manifest;` + wildcard re-exports. **Deviation**: `geometry` is NOT declared here — it lands in PR D (US5) per the no-empty-stub preference; `bundle` already exists from PR B; `bundle_watch` was ultimately dropped entirely (the thin `tokio::sync::watch` re-export removed — consumers use tokio directly). The originally-listed all-six forward-declaration would have required empty stub files.
- [x] T021 [Tests-first] [US2] Write unit tests in `/Users/singularity/eafora/shared/src/artifact/manifest.rs::#[cfg(test)] mod tests` per contracts/core-public-api.md §Tests as executable contracts: `parse_manifest_round_trips_fixture_set` (bytes parse + re-serialize byte-equal), `parse_manifest_rejects_unknown_schema_version`, `parse_manifest_rejects_unknown_statistic_code`, `parse_manifest_rejects_malformed_sha256`, `parse_manifest_rejects_path_traversal_relative_path`. Sample manifest bytes inline via `const FIXTURE: &str = r#"{ ... }"#;` since the producer-side `ingestion::artifact::writer::manifest::write_manifest` output shape is documented in data-model.md §Module: `shared::artifact::manifest`.
- [x] T022 [US2] Implement `/Users/singularity/eafora/shared/src/artifact/manifest.rs` per data-model.md §Module: `shared::artifact::manifest`. Define constants (`MANIFEST_FILENAME`, `MANIFEST_SCHEMA_VERSION = 1`, `SUBDIR_GEOMETRY`, `SUBDIR_DATA`, `MANIFEST_LATEST_KEY` per FR-020h). **Deviation**: the `CONTENT_TYPE_*` (FR-020g) and `CACHE_CONTROL_*` (FR-020j) constants live in `shared/src/artifact/bundle.rs`, not here — they describe the artifact bundle's file kinds as HTTP objects, not the manifest schema (`ManifestEntry` carries no content type). Define structs (`Manifest`, `ManifestEntry`) with `#[derive(Debug, Clone, Serialize, Deserialize)]`. `Manifest`'s field declaration order matches the serialization order; `manifest_schema_version` is the FIRST field. Implement `parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError>` per spec FR-012 + §Clarifications Q3: serde-deserialize, validate `manifest_schema_version == MANIFEST_SCHEMA_VERSION` (reject others with the documented `AppError` shape), validate each entry's `sha256` is 64 hex chars, validate each entry's `relative_path` does not contain `..` and does not start with `/`. Make T021 pass.
- [x] T023 [US2] Rewrite `/Users/singularity/eafora/ingestion/src/artifact/writer/manifest.rs::write_manifest` to construct a `shared::artifact::manifest::Manifest` with `manifest_schema_version: 1`, then serialize via `serde_json::to_string_pretty` against that struct. Delete the private `ManifestSerializer<'a>` + `ManifestEntry<'a>` structs. The `relative_path` helper function stays in `ingestion/` (it's producer-only — constructs paths from `Hashed<FileReference>` values that consumers don't see).
- [x] T024 [Tests-first] [US2] Update the existing producer-side tests in `/Users/singularity/eafora/ingestion/src/artifact/writer/manifest.rs::#[cfg(test)] mod tests`: every assertion that checked the serialized JSON contents now expects `manifest_schema_version: 1` to appear in the output. **Deviation**: the `statistics` / `source_revisions` maps are now `BTreeMap` keyed by the typed `StatisticKind` / `LicenseShardClass` / `DataSourceKind` (not `String`), so JSON key order is by enum-discriminant declaration order, not by code-string alphabetical order. The two former "sorts alphabetically" assertions were renamed to `orders_statistics_by_statistic_kind` / `orders_license_classes_by_shard_class` and assert the discriminant order (`tfr` before `_test_alpha`; `base` before `noncommercial`). Determinism (SC-005) is unchanged — a `BTreeMap` over a `Copy + Ord` enum is still byte-stable. Run `cargo test --workspace` to confirm.
- [x] T025 [US2] Run `cargo test -p shared` to confirm the T021 tests pass. Run `cargo test -p ingestion` to confirm the producer-side tests pass. Verify spec SC-005's round-trip property: bytes written by ingestion's `write_manifest` parse via `shared::artifact::manifest::parse_manifest` and re-serialize byte-equal to the original.
- [x] T025a [US2] Rewrite `/Users/singularity/eafora/ingestion/src/artifact/publish.rs::load_build_report_from_disk` per spec FR-020e to use `shared::artifact::manifest::parse_manifest` instead of its private `ManifestOnDisk` / `ManifestEntryOnDisk` deserializer. Delete `ManifestOnDisk` and `ManifestEntryOnDisk` struct definitions. The function now: reads `manifest.json` bytes via `fs::read`; parses via `shared::artifact::manifest::parse_manifest`; iterates `manifest.geometry` + `manifest.statistics` (now using the typed `StatisticKind` / `LicenseShardClass` keys instead of `String` keys + `TryFrom` conversion); calls the existing local `load_hashed_file` helper for each referenced file (compares the file's recomputed SHA-256 to `manifest.geometry.sha256` / `entry.sha256`). The local `load_hashed_file` is a private helper that stays; only the parallel manifest deserializer goes away. ALSO per FR-020g: delete the local `CONTENT_TYPE_FLATGEOBUF` / `CONTENT_TYPE_MANIFEST` / `CONTENT_TYPE_SQLITE` consts at the top of the file; the `repository.put_file(..., CONTENT_TYPE_*)` call sites reach via `shared::artifact::bundle::CONTENT_TYPE_*` instead. Run `cargo test -p ingestion` to confirm existing publish-side tests pass against the rewritten function.

**Checkpoint**: US2 done. Manifest schema is owned by shared; producer and consumer agree on the shape.

---

## Phase 5: US3 — `ArtifactCache` trait + cross-platform cache contract (Priority: P3)

**Goal**: The `ArtifactCache` async trait + an in-crate `MockArtifactCache` (`#[cfg(test)]`-only) for `shared/`'s own tests of `Bundle::open`.

**Independent test criteria**: The trait compiles with stable AFIT (no `async-trait` crate). The `MockArtifactCache` round-trips inserts via the trait surface; tests in later phases consume it.

- [x] T026 [US3] Create `/Users/singularity/eafora/shared/src/artifact/cache.rs` per data-model.md §Module: `shared::artifact::cache`. Define the trait `pub trait ArtifactCache { async fn put(...); async fn get(...); async fn list_versions(...); async fn delete_version(...); }` with the exact signatures from data-model.md. No `Send + Sync` bounds.
- [x] T027 [Tests-first] [US3] In the same file, behind `#[cfg(test)]`, write tests for `MockArtifactCache` (defined in T028): `mock_cache_put_get_round_trip_returns_byte_equal`, `mock_cache_get_missing_returns_none`, `mock_cache_list_versions_returns_inserted_keys`, `mock_cache_delete_version_removes_only_that_version`. The tests don't need spec-level FR coverage; they're confirming the mock works correctly so US5's `Bundle::open` tests can rely on it.
- [x] T028 [US3] Implement `MockArtifactCache` per data-model.md (post-edit) — `BTreeMap<(String, String), Vec<u8>>` wrapped in `tokio::sync::Mutex`; constructor `MockArtifactCache::new()` returns an empty mock; convenience `insert(version_label, file_relative_path, bytes)` helper for test-side seeding. Implement `ArtifactCache for MockArtifactCache` with the four async methods. **Placement**: the mock + its tests live inside a `#[cfg(test)] pub(crate) mod tests` in `cache.rs`, with `pub(crate)` items, so PR D's `bundle.rs` tests import `crate::artifact::cache::tests::MockArtifactCache` (`cfg(test)` is crate-wide under `cargo test`, so no cross-crate boundary). Make T027 pass.
- [x] T029 [US3] Run `cargo test -p shared` to confirm the mock tests pass on the host target. The trait + mock are both `#[cfg(test)]`-relevant; the trait itself is public-API. The trait carries `#[allow(async_fn_in_trait)]` with a rationale comment — the `Send` bound is deliberately omitted because the web cache impl (`OpfsArtifactCache`) holds `!Send` `JsValue`, NOT because consumers are single-threaded (native is multi-threaded; only the resulting `Arc<Bundle>` crosses threads, via the watch channel).

**Checkpoint**: US3 done. The cache contract exists; US5 can build on it.

---

## Phase 6: US4 — Discovery document type + parse (Priority: P4)

**Goal**: `DiscoveryDocument` + `parse_discovery_document` with `schema_version == 1` validation. Independent of US5; can land before or after.

**Independent test criteria**: `parse_discovery_document(json_with_v1_shape)` returns the populated struct; `parse_discovery_document(json_with_schema_version_2)` returns `AppError` containing `"unknown schema_version 2"`; absent `sunset` field deserializes to `None`.

- [x] T030 [Tests-first] [US4] Write unit tests in `/Users/singularity/eafora/shared/src/artifact/discovery.rs::#[cfg(test)] mod tests` per contracts/core-public-api.md §Tests: `parse_discovery_document_round_trips_fixture`, `parse_discovery_document_rejects_unknown_schema_version`, `parse_discovery_document_handles_missing_sunset_field`. Sample bytes inline matching `docs/architecture/client.md` §Discovery document shape.
- [x] T031 [US4] Implement `/Users/singularity/eafora/shared/src/artifact/discovery.rs` per data-model.md §Module: `shared::artifact::discovery`. Define `DISCOVERY_SCHEMA_VERSION = 1`, `DISCOVERY_URL = "https://eafora.org/discovery"` per FR-020i, `DiscoveryDocument` struct with `#[derive(Debug, Clone, Serialize, Deserialize)]`, fields `schema_version: u32`, `repository_base_url: String`, `minimum_client_version: String`, `sunset: Option<String>`. Implement `parse_discovery_document(bytes: &[u8]) -> Result<DiscoveryDocument, AppError>` per spec FR-015: serde-deserialize, validate `schema_version == DISCOVERY_SCHEMA_VERSION` (reject others with the documented `AppError` shape). Make T030 pass.

**Checkpoint**: US4 done. Discovery flow's consumer-side parse exists.

---

## Phase 7: US6 — License authorization (Priority: P6)

**Goal**: `DistributionContext` enum + `authorized_classes()` returning the static slice per `client.md` §Attaching license shards. Independent of US5 but US5 consumes it.

**Independent test criteria**: `DistributionContext::FirstParty.authorized_classes() == &[Base, NonCommercial, ShareAlike]`; `DistributionContext::ThirdParty.authorized_classes() == &[Base]`; adding a hypothetical new `LicenseShardClass` variant breaks compilation in `authorized_classes()` (per spec FR-022; verifiable by attempting the addition during implementation as a sanity check, not by a runtime test).

- [x] T032 [P] [US6] Create `/Users/singularity/eafora/shared/src/license/mod.rs` with `pub mod license; pub use license::*;`.
- [x] T033 [Tests-first] [US6] Write unit tests in `/Users/singularity/eafora/shared/src/license/license.rs::#[cfg(test)] mod tests`: `distribution_context_first_party_authorizes_all_classes`, `distribution_context_embedded_authorizes_base_only`. (No runtime test for the compile-error-on-new-variant property; that's a compile-time guarantee enforced by the `match` having no wildcard arm.)
- [x] T034 [US6] Implement `/Users/singularity/eafora/shared/src/license/license.rs` per data-model.md §Module: `shared::license::license`. Define `DistributionContext` enum with `FirstParty` + `Embedded` variants (derive `Debug, Clone, Copy, PartialEq, Eq, Hash`). Implement `authorized_classes(self) -> &'static [LicenseShardClass]` as a `match self { ... }` with NO wildcard arm — both arms list the slices explicitly per `docs/architecture/client.md` §Attaching license shards. Make T033 pass.

**Checkpoint**: US6 done. US5's `Bundle::open` can now filter shards by authorized class.

---

## Phase 8: US5 — `Bundle` loader + hot-swap channel (Priority: P5)

**Goal**: `Bundle::open(version_label, &cache, ctx)` reads through the cache, verifies SHA-256s, parses geometry, populates `shard_bytes` filtered by `DistributionContext::authorized_classes()`, returns a `Send + Sync` `Bundle`. The geometry-reader module lands here too. The bundle hot-swap channel is `tokio::sync::watch`, used by consumers directly (no `shared` re-export). **The SQLite VFS / Connection bridge is NOT here** — see the deferral note under "SQLite WASM VFS" below.

**Independent test criteria**: `Bundle::open` against a populated mock cache returns a bundle whose `shard_bytes` matches the manifest entries; a SHA-256 mismatch produces the documented error; an unauthorized shard isn't in `shard_bytes`; `Arc<Bundle>: Send + Sync` (compile-time assertion).

`Bundle::open` depends on the geometry reader (T039), the manifest parser (US2), the cache trait (US3), and the license matrix (US6) — NOT on the SQLite VFS: `Bundle` carries shard BYTES and opens no connection.

### SQLite WASM VFS (FR-020) — DEFERRED TO 006

- [~] T035 / T036 (vfs only) / T037 **DEFERRED to 006-core-renderer.** `sqlite-wasm-rs` 0.4.x is raw libsqlite3 C bindings with NO `Connection` type, so the cross-target `Connection` typedef cannot exist; the bridge must be a real wrapper (native `rusqlite::Connection::deserialize` + the `serialize` feature; wasm32 raw `sqlite3_*` FFI) behind one facade, and its method surface is defined by the renderer's queries (006). `Bundle` doesn't use it (carries bytes; opens no connection), so it's descoped from 005. The deferral is recorded in `docs/task-order.md`, spec FR-020 / FR-020c, and `specs/006-core-renderer/spec.md` §Assumptions. `shared::sqlite::schema` (the stable contract, T039a-c) stays in 005; `shared/src/sqlite/mod.rs` declares only `pub mod schema;`.

### Geometry reader (FR-020a)

- [x] T038 [Tests-first] [US5] Unit tests in `shared/src/artifact/geometry.rs::#[cfg(test)] mod tests`. **Deviation**: instead of committing an opaque `shared/tests/samples/tiny.fgb`, the test builds a one-feature FlatGeobuf in memory via the upstream `FgbWriter` (`one_feature_fgb_bytes`, `pub(crate)` so `bundle.rs` tests reuse it) — fully reproducible, no checked-in binary. Tests: `parse_geometry_layer_parses_known_fixture`, `features_intersecting_bbox_returns_matching_feature`, `parse_geometry_layer_rejects_garbage_bytes`.
- [x] T039 [US5] Implement `shared/src/artifact/geometry.rs` per data-model.md §Module: `shared::artifact::geometry`. Constants per FR-020f; `CountryFeature`/`Polygon`/`BoundingBox`. **Deviations**: (1) `GeometryLayer` holds the owned `bytes: Vec<u8>` and opens a fresh `FgbReader` per query, because `FgbReader::select_all`/`select_bbox` consume the reader (one pass each) — it cannot hold a live `FgbReader<Cursor<Vec<u8>>>` and query repeatedly. (2) `iter_features`/`features_intersecting_bbox` are `(&self) -> Result<Vec<CountryFeature>, AppError>` (eager collect), not `(&mut self) -> impl Iterator<Item = Result<CountryFeature>>` — the underlying `FallibleStreamingIterator` borrows the consumed reader, so returning a borrowing iterator is impractical; eager collect over a few hundred countries is fine. Property reads use geozero `FeatureProperties::property::<String>`; geometry via geozero `ToGeo` (`with-geo`). (3) The parsed struct is `CountryFeature` (renamed from `Feature`: the layer is admin-0, so every feature is a country, and the generic name oversold its generality); upstream→domain conversions are expressed as `From<&geo_types::Polygon<f64>> for Polygon` and `TryFrom<&FgbFeature> for CountryFeature`, with `BoundingBox::from_polygons` as the extent constructor.
- [x] T039d [US5] Rewrite `ingestion/src/artifact/writer/flatgeobuf.rs` to use the moved constants from `shared::artifact::geometry` (`GEOMETRY_LAYER_NAME`, `GEOMETRY_FILENAME_STEM`, `FEATURE_COLUMN_ISO3`/`_NAME_EN` bound onto the local `Column` struct, `GEOMETRY_FILENAME_EXTENSION` in the tmp filename); deleted the producer's local `GEOMETRY_LAYER_NAME`/`GEOMETRY_FILENAME_STEM`; migrated `artifact_integration.rs` to import them from `shared::artifact::geometry` (module-qualified). Producer tests pass.

### SQLite schema contract (FR-020b through FR-020e)

- [x] T039a [Tests-first] [US5] Unit tests in `shared/src/sqlite/schema.rs` (native-gated, since they use `rusqlite`): `shard_schema_ddl_creates_expected_tables_and_index`, `validate_shard_header_accepts_correctly_initialized_connection`, `validate_shard_header_rejects_wrong_application_id`, `validate_shard_header_rejects_unknown_schema_version`. Assertions use `.contains(...)` (not `.starts_with(...)`) because `minimer::AppError`'s `Display` wraps the message as `AppError [<message>]`.
- [x] T039b [US5] Implement `shared/src/sqlite/schema.rs`. All constants defined; `shard_schema_ddl() -> &'static str` built via `const_format::formatcp!` with inline const references (confirmed supported by const_format 0.2.36). `validate_shard_header(connection: &rusqlite::Connection)` is `#[cfg(not(target_arch = "wasm32"))]`-gated (rusqlite is native-only; the wasm32 renderer validates through its own connection in 006).
- [x] T039c [US5] Rewrite `ingestion/src/artifact/writer/sqlite.rs` to use `shared::sqlite::schema` (deleted local `SQLITE_APPLICATION_ID`/`SQLITE_USER_VERSION`/`create_schema`; `execute_batch(schema::shard_schema_ddl())`; insert SQL via `formatcp!` over the column constants; PRAGMA writes use `schema::APPLICATION_ID`/`SCHEMA_VERSION`; period formatting uses `schema::PERIOD_DATE_FORMAT`). Producer tests pass.

### Bundle + hot-swap (FR-018, FR-019, FR-023)

- [x] T040 [P] [US5] ~~Create `shared/src/artifact/bundle_watch.rs`~~ **Dropped**: the thin `tokio::sync::watch` re-export module was removed (pure forwarding of a third-party type, against the no-forwarding-declarations preference). Consumers (003/004/006) create the hot-swap channel via `tokio::sync::watch` directly; `shared`'s tokio dependency moved to dev-only (the test mock's `tokio::sync::Mutex`).
- [x] T041 [Tests-first] [US5] Unit tests in `bundle.rs::#[cfg(test)] mod tests`: `bundle_open_round_trip_against_mock_cache`, `bundle_open_eagerly_parses_geometry`, `bundle_open_skips_unauthorized_shards`, `bundle_open_rejects_missing_manifest`, `bundle_open_rejects_sha256_mismatch`, `bundle_is_send_sync`. Fixtures: a `Manifest` built in-test + serialized via `serde_json::to_vec`, the geometry fgb from `geometry::tests::one_feature_fgb_bytes`, arbitrary shard bytes (Bundle stores shard bytes without parsing), seeded into `cache::tests::MockArtifactCache`.
- [x] T042 [US5] Implement `shared/src/artifact/bundle.rs`: `Bundle` struct (`manifest`, `geometry`, `shard_bytes`, `distribution_context`); `Bundle::open` per the spec algorithm. **Deviation**: `Bundle::open` is generic — `open<C: ArtifactCache>(version_label, cache: &C, ctx)` — NOT `cache: &dyn ArtifactCache`. Stable async-fn-in-trait makes `ArtifactCache` not dyn-compatible (opaque future return type), so `&dyn` won't compile; static dispatch is the idiomatic fix and the loader always has a concrete cache type. `Bundle: Send + Sync` verified by the compile-time assertion test.
- [x] T043 [US5] Ran `cargo test -p shared` — all Bundle::open tests pass on the host target (33 shared tests total).

**Checkpoint**: US5 done. The full bundle-loading + hot-swap shape exists.

---

## Phase 9: WASM-target test coverage (FR-025)

**Purpose**: Run a subset of the shared crate tests on `wasm32-unknown-unknown` via `wasm-bindgen-test --headless --chrome` to confirm the cfg-gating actually works.

- [x] T044 Added `wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);` under `#[cfg(all(test, target_arch = "wasm32"))]` in `shared/src/lib.rs`. Harness invocation is `wasm-pack test --headless --chrome` run from `shared/` (not the workspace root — `wasm-pack` wants a package manifest); `cargo test --target wasm32-unknown-unknown` alone fails because it tries to execute the `.wasm` directly without the `wasm-bindgen-test-runner`.
- [x] T045 [P] **Deviation**: tests are NOT duplicated, and only ONE test is wasm-annotated. Per the significant-divergence bar (use `#[wasm_bindgen_test]` only where wasm and non-wasm behavior genuinely diverge), only `parse_geometry_layer_parses_known_fixture` carries `#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]` — flatgeobuf/geozero is the one dependency with a wasm-hostile runtime path (its `FgbWriter` traps on `env::temp_dir`), so the reader's wasm-safety is a real runtime claim that `cargo check --target wasm32` cannot establish. The other surfaces (`verify_sha256`, `parse_manifest`, `parse_discovery_document`, `Bundle::open`) are pure computation / serde parsing with no target branching, so they stay host-only `#[test]` / `#[tokio::test]`; `cargo check --target wasm32` covers their wasm compile. `open_connection_from_bytes` is EXCLUDED (SQLite VFS deferred to 006). The geometry fixture `one_feature_fgb_bytes()` is a single `include_bytes!` of the committed `shared/tests/samples/one-feature.fgb` (496 bytes) on both targets — the `FgbWriter` host-generation and a `dump_sample_fgb` regen test were dropped, since the writer can't run on wasm and the sample is committed anyway.
- [x] T046 Ran `./scripts/test/test-wasm.sh` (`wasm-pack test --headless --chrome` from `shared/`) against locally-installed Chrome 149 + matching chromedriver 149. The one wasm test (the geometry reader) passes headless. Per spec SC-002 + SC-004.

---

## Phase 10: Polish + cross-cutting

- [x] T047 [P] `cargo clippy --workspace --all-targets -- -D warnings` is clean. Fixed mechanical lints (needless_borrow, needless_as_bytes, new_without_default, manual_range_contains, `&Vec` → `&[SourceChoice]`, a `ShapefileReader` type alias for the complex shapefile reader return type). Two lints are allowed workspace-wide via `[workspace.lints.clippy]` because they fight deliberate conventions: `module_inception` (the `mod.rs`-holds-only-declarations layout) and `explicit_auto_deref` (the `&mut *tx` transaction-threading idiom — clippy would rewrite it to `&mut tx`). Each member opts in with `lints.workspace = true`.
- [x] T048 [P] **Deviation — not run.** The repo is hand-formatted and has never been through `cargo fmt`; rustfmt would reformat ~148 files (collapsing the deliberate blank-line paragraphs, one-item-per-line clauses, and split `log::` calls). Per the owner's decision, formatting stays manual; `cargo fmt --all` is intentionally NOT applied. (The `rustfmt.toml` present is aspirational, not enforced.)
- [x] T049 [P] Pre-PR sanity: `cargo build --workspace`, `cargo check -p shared --target wasm32-unknown-unknown`, `cargo test --workspace` (no `--features` flag needed — the test-only enum variants are unconditional now), and `wasm-pack test --headless --chrome` from `shared/` all pass. SC-001 + SC-002 + SC-003 + SC-004 hold.
- [x] T050 [P] **Deviation — skipped.** `cargo llvm-cov` is not installed and coverage is explicitly informational (spec SC-004's gate is the pass count, not a coverage number). The target surfaces all have direct host + wasm32 tests; coverage instrumentation adds no gate.
- [x] T051 [P] Self-audit clean: no em dashes in added code/comments; no new `log::` calls in `shared/`; all added `let` bindings carry explicit types; no `..Default::default()`; `mod.rs` files hold only declarations; constants at top of file. Confirmed by grep over `git diff master`.
- [x] T052 [P] Status note added to plan.md §Status note (post-implementation review). Every §Brief-PR-description deliverable verified present.
- [ ] T053 Open a draft PR via `gh pr create` against `master` per `feedback_branch_per_body_of_work`. PR description follows `feedback_pr_description_style`. Assign `zacharysiegel` immediately after creation.

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
- **Phase 2**: T006 (shared::error) and T010 (shared::filesystem) touch different files — drafted in parallel; T007 (ingestion::error rewrite) depends on T006; T011 (ingestion::filesystem rewrite) depends on T010.
- **Phase 3 (US1)**: T014 (mod.rs) and T015 (canonical_model.rs) touch different files — drafted in parallel; T015a (Entity-impl review in ingestion's canonical_model.rs) depends on T015 + T016 sequence; T015b (adapter_model.rs NaiveDatePeriod migration) is parallel to T015a (different file); T016 (ingestion canonical_model rewrite + rename) depends on T015; T017 (StatisticShardKey move) is independent of the canonical-model move.
- **Phase 5 (US3)**: T026 (trait) and T028 (mock) touch the same file; sequential within the file.
- **Phase 7 (US6)**: T032 (mod.rs) and T034 (license.rs) touch different files — drafted in parallel; T033 (tests) lives in the same file as T034.
- **Phase 8 (US5)**: T035-T037 (SQLite VFS), T038-T039 (geometry reader), and T039a-T039c (SQLite schema contract) touch different files — three fully parallel sub-phases. All three must complete before T040-T043 (bundle). T039c (producer-side `writer/sqlite.rs` rewrite) depends on T039b (the `shared::sqlite::schema` constants + DDL function); apart from that, the schema sub-phase is independent of the VFS sub-phase.
- **Phase 10**: T047, T048, T049, T050, T051, T052 are all parallelizable (different commands, mostly read-only).

## Implementation strategy

Single PR per plan.md §Phasing. Total task count: 61. Estimated effort: ~2.5 days of focused work (the type extraction is mechanical; the WASM VFS + geometry-reader fixtures + the SQLite-schema producer-rewrite are the time sinks). The MVP-shaped subset is US1 + US2 (Phases 1-4) — that's enough to verify the workspace member compiles + types extracted cleanly + manifest parses end-to-end + the producer-side publish flow no longer carries a parallel manifest deserializer. US3 + US4 + US6 are small (~3 tasks each); US5 (Bundle::open + SQLite VFS + geometry reader + SQLite schema contract + producer-side flatgeobuf rewrite) is the largest phase because it depends on every prior story AND introduces the cross-cutting producer-consumer SQLite contract.
