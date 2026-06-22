# Feature Specification: core/ crate — data layer (manifest, cache trait, license, Bundle, discovery)

**Feature Branch**: `005-core-data`

**Created**: 2026-06-22

**Status**: Draft

**Input**: User description: "Split into `005-core-data` (types + cache trait + license + parsers) and `006-core-renderer` (wgpu pipelines + draw_frame)." This is the first half: the data-and-types layer of the new `core/` workspace member, sized to unblock 003-web-client (FR-017 through FR-029 + the §Assumptions reference to `core::artifact`) and 004-ios-client (FR-008 through FR-014 + §Assumptions). The renderer + projection + hit_test surface lands in 006-core-renderer, stacked on this branch.

## Scenarios & Testing *(mandatory)*

### Workspace member with extracted producer types (P1)

A new `core/` Cargo workspace member exists at the workspace root. It compiles cleanly for the host target (`cargo build -p core`) and for `wasm32-unknown-unknown` (`cargo build -p core --target wasm32-unknown-unknown`). The types currently defined in `ingestion/src/canonical/canonical_model.rs` and `ingestion/src/artifact/writer/manifest.rs` (the `Manifest*` serializer + `LicenseShardClass` + `StatisticKind` + `DataSourceKind` + `SourceRevision` + `LicenseClass` + `DataStatus`) move into `core/src/`; the `ingestion/` crate `pub use`s them from `core::*` so existing producer call sites stay green. The producer side continues to build and pass tests against the post-move shape; the canonical authority for these types shifts from `ingestion/` to `core/` per `docs/architecture/client.md` §Manifest schema (consumer view) ("the manifest type lives once in `core/src/artifact/manifest.rs`...the producer and every client use it directly").

**Acceptance Scenarios**:

1. **Given** a fresh checkout post-merge, **When** the developer runs `cargo build --workspace`, **Then** every workspace member (including the new `core/` and the existing `ingestion/`, `tools/seed_generator/`) compiles successfully against the moved type definitions.
2. **Given** the same checkout, **When** the developer runs `cargo build -p core --target wasm32-unknown-unknown`, **Then** `core/` compiles for the WASM target (the web client's compile target per `docs/architecture/client-web.md` §`cargo-leptos`). Any feature gates needed to exclude host-only code (filesystem, ingestion-side glue) are in place; cargo emits no warnings about unused dependencies in the wasm32 build.
3. **Given** the same checkout, **When** the developer runs `cargo test -p ingestion`, **Then** every pre-existing ingestion test (including the manifest-determinism, license-class, statistic-kind tests) passes against the moved types reached via `core::*`.
4. **Given** the post-move state, **When** any developer greps for `pub use crate::canonical::canonical_model::*` or `pub use crate::artifact::writer::manifest::*` in `ingestion/`, **Then** the re-exports are in place at the documented locations so existing `crate::canonical::*` / `crate::artifact::*` consumer paths inside ingestion continue to resolve (no churn on every consumer per `feedback_wildcard_re_exports`).

---

### Manifest parsing (consumer side) (P2)

`core::artifact::manifest::parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError>` parses a byte slice produced by `ingestion::artifact::writer::manifest::write_manifest` into a strongly-typed consumer-side `Manifest` value. Round-trip property: bytes produced by the producer side parse into a `Manifest`; that `Manifest`'s `relative_path` / `size_bytes` / `sha256` fields exactly match what the producer wrote. Malformed input (missing field, wrong type, unknown statistic_code) returns a typed `AppError` whose message identifies what failed to parse; the function never panics on adversarial input. This is the contract `client.md` §Manifest schema (consumer view) locks in.

**Acceptance Scenarios**:

1. **Given** a manifest written by `ingestion::artifact::writer::manifest::write_manifest` against a fixture set (one geometry shard + two statistic shards across two license classes), **When** `parse_manifest` parses the bytes back, **Then** the resulting `Manifest` exposes `version`, `artifact_created` (parsed as `chrono::DateTime<chrono::Utc>`), `geometry: ManifestEntry`, `statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>>`, `source_revisions: BTreeMap<DataSourceKind, SourceRevision>`; every field matches the producer-side input.
2. **Given** a manifest with an unknown statistic code (`"unknown_stat"` instead of `"tfr"`), **When** `parse_manifest` runs, **Then** the result is `Err(AppError)` whose message contains the literal `"unknown_stat"` (so the operator can identify the offending key); the function does NOT panic.
3. **Given** a manifest with a malformed `sha256` field (length != 64 hex chars), **When** `parse_manifest` runs, **Then** the result is `Err(AppError)` whose message names the offending entry's relative_path.
4. **Given** any well-formed manifest, **When** `parse_manifest(bytes)` runs and then the result is re-serialized via the same `ManifestSerializer` shape, **Then** the re-serialized bytes are byte-equal to the input (the consumer-side parse round-trips through the producer-side serializer; this verifies the type-shape match across the producer / consumer halves).

---

### `ArtifactCache` trait + cross-platform cache contract (P3)

`core::artifact::cache::ArtifactCache` is the cross-platform cache trait that both `web/src/cache.rs::OpfsArtifactCache` (consumed by 003-web-client) and `ios/EaforaApp/FileSystemArtifactCache.swift` (consumed by 004-ios-client) implement. Surface per `client.md` §Fetch / cache / load pipeline + the trait signatures referenced in `client-web.md` §OPFS cache adapter and `client-ios.md` §Implementation: FileSystemArtifactCache.swift. The trait is defined once in `core/`; per-platform implementations satisfy it; the `core::artifact::Bundle` loader consumes the trait by reference (no platform-specific code inside `core::artifact::Bundle`). The trait is `async` (each function returns a `Future`); `Send` / `Sync` bounds are NOT required (single-threaded WASM uses it without locks).

**Acceptance Scenarios**:

1. **Given** `core/src/artifact/cache.rs` defines `pub trait ArtifactCache` with `async fn put(...) -> Result<(), AppError>`, `async fn get(...) -> Result<Option<Vec<u8>>, AppError>`, `async fn list_versions(...) -> Result<Vec<String>, AppError>`, `async fn delete_version(...) -> Result<(), AppError>`, **When** a test crate implements the trait against an in-memory `HashMap`-backed mock, **Then** `Bundle::open` (FR-019) consumes the mock as `&dyn ArtifactCache` and the test exercises a complete round-trip without invoking any per-platform code.
2. **Given** the trait definition, **When** any consumer attempts to use it from a `Send + Sync` context, **Then** the compiler does NOT enforce `Send + Sync` (web's `OpfsArtifactCache` holds `!Send` `JsValue` handles indirectly; the trait must be usable in single-threaded WASM without the bound).
3. **Given** the trait's function signatures, **When** the iOS and web spec's per-platform adapter implements the trait, **Then** the implementation requires no async-trait crate (Rust 1.75+ stable AFIT — async fn in trait — is available; verify against the workspace's Rust edition / rustc per the §Assumptions).

---

### Discovery document type + parse (P4)

`core::artifact::discovery::DiscoveryDocument` is the deserialized form of the JSON at `https://eafora.org/discovery` defined in `client.md` §Discovery document shape. The struct has fields `schema_version: u32`, `repository_base_url: String`, `minimum_client_version: String`, `sunset: Option<String>` and implements `serde::Deserialize`. A `parse_discovery_document(bytes: &[u8]) -> Result<DiscoveryDocument, AppError>` helper validates `schema_version == 1` and returns the parsed document; unknown `schema_version` returns a typed `AppError` (per `client.md` step "Clients reject documents whose `schema_version` they don't recognize, falling back to their baked-in defaults"). This struct is what both 003-web-client (FR-027) and 004-ios-client (FR-026) consume.

**Acceptance Scenarios**:

1. **Given** a discovery JSON document matching the schema in `client.md` §Discovery document shape, **When** `parse_discovery_document` parses the bytes, **Then** the resulting struct's fields match the JSON exactly.
2. **Given** a discovery JSON with `schema_version: 2`, **When** `parse_discovery_document` parses the bytes, **Then** the result is `Err(AppError)` whose message contains the literal `"unknown schema_version 2"` so the caller can fall back to its baked-in defaults per the documented protocol.
3. **Given** a discovery JSON with `sunset: "2027-01-01T00:00:00Z"`, **When** `parse_discovery_document` parses the bytes, **Then** the result's `sunset` is `Some("2027-01-01T00:00:00Z".to_string())`; the type does NOT parse this as `chrono::DateTime` at this layer — the caller decides how to compare against `now()` per the consumer-controlled fallback logic.
4. **Given** a discovery JSON missing the `sunset` field entirely, **When** `parse_discovery_document` parses the bytes, **Then** `sunset` is `None` (the field is `Option<String>` per the schema's `null in steady state` rule).

---

### `Bundle` loader + hot-swap channel (P5)

`core::artifact::Bundle::open(manifest_path: &Path, cache: &dyn ArtifactCache, distribution_context: DistributionContext) -> Result<Bundle, AppError>` opens a manifest from disk (or from cache; the caller's call-site decides), validates every referenced shard's SHA-256 against `core::hashing::verify_sha256`, attaches the license shards that this distribution context is authorized to access per `core::license::DistributionContext::authorized_classes` (FR-021), and returns a fully-constructed `Bundle` value. The `Bundle` owns the parsed manifest, the open SQLite connection (with each authorized license shard attached as a SQLite database), and the FlatGeobuf reader holding the geometry-shard bytes. The renderer (in 006-core-renderer) holds an `Arc<Bundle>` via a `tokio::sync::watch::Receiver<Arc<Bundle>>`; bundle hot-swap happens through the matching `Sender::send(new_arc)` per `client.md` §Bundle hot-swap. The watch-channel types are exported from `core` (FR-023) so both clients can use the cross-platform shape.

**Acceptance Scenarios**:

1. **Given** a manifest + matching shard files on disk produced by `ingestion::artifact::writer::manifest::write_manifest`, **When** `Bundle::open(manifest_path, &cache, DistributionContext::FirstParty)` runs, **Then** the result is `Ok(Bundle)`; the bundle's SQLite connection has every base + non-commercial + share-alike shard attached (whatever the manifest contained); every shard's bytes were SHA-256-verified during open.
2. **Given** a manifest references a shard whose actual bytes don't match the recorded SHA-256, **When** `Bundle::open` runs, **Then** the result is `Err(AppError)` whose message identifies the mismatched `relative_path` and both the expected + actual SHA-256 prefixes (first 8 hex chars each); the partially-opened SQLite connection is dropped cleanly.
3. **Given** a `DistributionContext::Embedded` context, **When** `Bundle::open` runs against the same manifest as scenario 1, **Then** only the `Base` license shards are attached; the non-commercial and share-alike shards are not opened (saves I/O for the embedded context where they wouldn't render anyway).
4. **Given** an `Arc<Bundle>` published via `tokio::sync::watch::Sender::send(new_arc)`, **When** the matching `Receiver` calls `Receiver::borrow_and_update()`, **Then** the returned guard exposes the new bundle; the previous `Arc` is dropped when its last in-flight query completes (per `client.md` §Bundle hot-swap: "in-flight queries holding an old `Arc` finish against the old bundle, and the old bundle's memory frees when the last reference drops").
5. **Given** a `core::hashing::sha256_hex(bytes: &[u8]) -> String` function (re-exported from the existing `ingestion::filesystem::sha256_hex` per the move), **When** any consumer calls it on a known input, **Then** the returned hex matches the same 64-char lowercase hex the producer side already emits (verified against the existing producer-side tests).

---

### License authorization (P6)

`core::license::DistributionContext` enum + `authorized_classes(self) -> &'static [LicenseShardClass]` function per the exact sketch in `client.md` §Attaching license shards. v1 ships two distribution-context variants: `FirstParty` (authorized for `[Base, NonCommercial, ShareAlike]`) and `Embedded` (authorized for `[Base]` only). The function returns a `&'static` slice; adding a new `LicenseShardClass` variant elsewhere in `core/` produces a compile error in every `DistributionContext` arm until each arm is updated explicitly (Rust's non-exhaustive-match enforcement); the spec's "neither addition is silent" property per `client.md` is structurally satisfied.

**Acceptance Scenarios**:

1. **Given** `DistributionContext::FirstParty`, **When** `authorized_classes()` is called, **Then** the returned slice is `&[LicenseShardClass::Base, LicenseShardClass::NonCommercial, LicenseShardClass::ShareAlike]`.
2. **Given** `DistributionContext::Embedded`, **When** `authorized_classes()` is called, **Then** the returned slice is `&[LicenseShardClass::Base]` only.
3. **Given** a new `LicenseShardClass` variant added to the enum, **When** a developer attempts to compile without updating every `DistributionContext` arm in `authorized_classes()`, **Then** `cargo check -p core` fails with a non-exhaustive match error pointing at the function; the compiler is the failure mode that catches the missing arm.

---

### Edge Cases

- **Manifest references a shard the cache doesn't have** — `Bundle::open` calls `cache.get(version_label, relative_path)` and receives `Ok(None)`. The function returns `Err(AppError)` whose message names the missing relative_path; the caller is expected to fetch the shard via the platform's fetch adapter (003 FR-025 / 004 FR-025) and call `Bundle::open` again. This spec does NOT include the fetch-then-retry orchestration — that lives in the platform shells.
- **Manifest's `artifact_created` is in the future** — parsed successfully (it's just a timestamp); no validation. The producer is the source of truth; if it produced a future timestamp, the consumer trusts it.
- **A manifest entry's `relative_path` contains `..` or absolute path** — `Bundle::open` MUST reject the manifest with an `AppError` whose message names the offending entry. Path traversal across cache subdirectories is a security concern; rejecting at parse time is cheap.
- **Concurrent `Bundle::open` calls against the same cache** — the cache trait's `get` returns owned bytes per call; two simultaneous opens against the same cache are safe at this layer. Each `Bundle::open` produces an independent `Bundle` value; the renderer's `watch::Sender` decides which becomes current.
- **WASM target requires SQLite via `Vec<u8>`-backed VFS** — per `client.md` §SQLite in the client, the web client downloads the whole shard as a `Vec<u8>` and queries in memory via a custom VFS. The VFS implementation lives in `core/` per `client.md` §Module layout (`core::sqlite::vfs`). This spec includes the VFS as part of the `Bundle::open` consumer path: when the target is `wasm32`, the VFS reads from a `Vec<u8>` held in SQLite's open connection's user data; on native targets, SQLite reads from the file path normally. This is a `cfg`-gated implementation detail of `Bundle::open`'s SQLite-opening helper.
- **FlatGeobuf bytes are partially downloaded** (consumer scenario) — the SHA-256 verification step fails because the byte count doesn't match the expected `size_bytes`; `Bundle::open` returns an error. The cache layer is responsible for not handing partial bytes to the bundle loader; this is checked at the SHA-256 layer regardless.

## Requirements *(mandatory)*

### Functional Requirements

#### Workspace + crate structure

- **FR-001**: System MUST add a `core/` Cargo workspace member at the workspace root. `core/Cargo.toml` declares `[package]` with `name = "core"`, `edition = "2024"`, `version = "0.0.0"` (matches the placeholder versioning convention from `.specify/memory/constitution.md` §Versioning); the workspace root `Cargo.toml`'s `members` array gains `"core"`. The crate is a library crate (`[lib]`); no `[[bin]]` targets.
- **FR-002**: System MUST organize `core/src/` per `docs/architecture/client.md` §Module layout: `lib.rs` (declares submodules + re-exports); per-feature module directories (`artifact/`, `hashing/`, `sqlite/`, `statistic/`, `license/`); `mod.rs` files in each subdirectory hold ONLY `pub mod ...; pub use ...::*;` per the `mod.rs holds only declarations` rule; primary module content lives in named files inside each directory.
- **FR-003**: System MUST compile cleanly for `cargo build -p core` against the host (Mac mini M1, Apple Silicon) and for `cargo build -p core --target wasm32-unknown-unknown` against the web target. Host-only code (filesystem operations that don't exist in WASM; tokio runtime features not available single-threaded) MUST be `#[cfg(not(target_arch = "wasm32"))]`-gated. The `core/Cargo.toml` MUST declare `[target.'cfg(target_arch = "wasm32")'.dependencies]` and `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` blocks to isolate target-specific dependencies (no `js-sys` / `web-sys` outside wasm32; no tokio `rt-multi-thread` inside wasm32).
- **FR-004**: System MUST register dependencies via the workspace's `[workspace.dependencies]` table where the dep is shared with ingestion (`serde`, `serde_json`, `chrono`, `uuid`, `sha2`, `rusqlite`, `flatgeobuf`, `geozero`, `geo-types`, `minimer`, `log`, `tokio` with reduced features for cross-platform use). Per-target deps (WASM-only: `wasm-bindgen`, `wasm-bindgen-futures`, `js-sys`, `web-sys`; host-only: nothing new for this feature) go in the per-target dependency tables.

#### Type extraction from `ingestion/`

- **FR-005**: System MUST move the following type definitions from `ingestion/src/canonical/canonical_model.rs` to `core/src/canonical/canonical_model.rs`: `StatisticKind` (enum + `code()` + `TryFrom<&str>`); `DataSourceKind` (enum + `code()` + `TryFrom<&str>`); `DataStatus` (enum + `as_str()` + `TryFrom<&str>`); `LicenseClass` (enum + `as_str()` + `TryFrom<&str>`); `LicenseShardClass` (enum + `from_license_class()` + `as_str()` + `TryFrom<&str>`); `SourceRevision` (struct, derives `Serialize` + `Deserialize` + `Clone`). The `Region` / `RegionEntity` / `Country` / `CountryEntity` / `Statistic` / `StatisticEntity` / `DataSource` / `DataSourceEntity` / `StatisticValue` / `StatisticValueEntity` types MAY stay in `ingestion/` if they are not consumed by the client (TBD at implementation time; this spec does NOT require moving them, since the client reads via SQLite shards not direct Postgres types).
- **FR-006**: System MUST update `ingestion/src/canonical/canonical_model.rs` to `pub use core::canonical::canonical_model::*` (or equivalent per-symbol re-exports) for the moved types so existing ingestion-side import paths (`crate::canonical::canonical_model::StatisticKind`, etc.) continue to resolve.
- **FR-007**: System MUST move `ingestion/src/artifact/writer/manifest.rs`'s manifest schema (the `ManifestSerializer<'a>` + `ManifestEntry<'a>` structs and the constants `MANIFEST_FILENAME`, `SUBDIR_GEOMETRY`, `SUBDIR_DATA`) to `core/src/artifact/manifest.rs` as the canonical owned form (with `'static` strings instead of `&'a str`; ownership moves to the consumer side per `client.md` §Manifest schema). The producer-side `write_manifest` function in `ingestion/` stays in `ingestion/`; it now consumes the `core::artifact::manifest::Manifest` shape rather than its private serializer struct. The serialize-side determinism property (BTreeMaps for stable key order; `serde_json::to_string_pretty`; byte-equal output on identical inputs) is preserved.
- **FR-008**: System MUST move `ingestion/src/artifact/hashing.rs` to `core/src/hashing/hashing.rs`. The `hash_sqlite_shards` and `hash_geometry` functions (producer-side) stay in `ingestion/`; only the cross-cutting helpers (`sha256_hex(bytes)`, `sha256_hex_of_file(path)`, `verify_sha256(bytes, expected_hex)`) move. The producer-side caller updates its imports to `core::hashing::sha256_hex` etc.
- **FR-009**: System MUST add `verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), AppError>` to `core::hashing`. Returns `Ok(())` on match; `Err(AppError)` on mismatch whose message contains both `expected_hex` (first 8 hex chars) and the actual hash (first 8 hex chars), per the §Edge Cases for `Bundle::open`.

#### Manifest consumer types

- **FR-010**: System MUST define `core::artifact::manifest::Manifest` as the owned consumer-side type with these fields (matching the on-the-wire shape in `client.md` §Manifest schema): `version: String`, `artifact_created: chrono::DateTime<chrono::Utc>`, `geometry: ManifestEntry`, `statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>>`, `source_revisions: BTreeMap<DataSourceKind, SourceRevision>`. The struct derives `Debug`, `Clone`, `Serialize`, `Deserialize`.
- **FR-011**: System MUST define `core::artifact::manifest::ManifestEntry` with fields `relative_path: String`, `size_bytes: u64`, `sha256: String`. Derives `Debug`, `Clone`, `Serialize`, `Deserialize`.
- **FR-012**: System MUST implement `core::artifact::manifest::parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError>`. Calls `serde_json::from_slice::<Manifest>(bytes)`; on `serde_json::Error`, wraps with an `AppError` whose message contains the deserialization error's display. Validates each entry's `sha256` is 64 hex chars; rejects path-traversal `relative_path` values (contains `..` or starts with `/`).
- **FR-013**: System MUST update `ingestion/src/artifact/writer/manifest.rs`'s `write_manifest` to: (a) construct a `core::artifact::manifest::Manifest`; (b) serialize via `serde_json::to_string_pretty` with the consumer-side `Manifest` `Serialize` impl producing byte-for-byte identical output to the pre-move `ManifestSerializer` impl (verified by the existing `build_manifest_json_is_deterministic_byte_for_byte` test continuing to pass post-move). Every existing manifest test in `ingestion/src/artifact/writer/manifest.rs` MUST continue to pass.

#### Discovery document

- **FR-014**: System MUST define `core::artifact::discovery::DiscoveryDocument` with fields `schema_version: u32`, `repository_base_url: String`, `minimum_client_version: String`, `sunset: Option<String>` per `client.md` §Discovery document shape. Derives `Debug`, `Clone`, `Deserialize`, `Serialize`.
- **FR-015**: System MUST implement `core::artifact::discovery::parse_discovery_document(bytes: &[u8]) -> Result<DiscoveryDocument, AppError>`. Parses via `serde_json::from_slice`; rejects `schema_version != 1` with an `AppError` whose message contains the literal `"unknown schema_version {N}"` where `{N}` is the unknown version; on serde failure wraps with a descriptive `AppError`.

#### `ArtifactCache` trait

- **FR-016**: System MUST define `core::artifact::cache::ArtifactCache` as a public async trait with the signature:
  ```rust
  pub trait ArtifactCache {
      async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppError>;
      async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError>;
      async fn list_versions(&self) -> Result<Vec<String>, AppError>;
      async fn delete_version(&self, version_label: &str) -> Result<(), AppError>;
  }
  ```
  The trait MUST NOT require `Send + Sync` bounds (web's `OpfsArtifactCache` per `client-web.md` §Threading model wraps `JsValue` types that are `!Send`). The trait uses Rust 1.75+ stable AFIT (`async fn` in trait); no `async-trait` crate dep.
- **FR-017**: System MUST provide a `core::artifact::cache::MockArtifactCache` (gated behind `#[cfg(test)]` or behind a `mock` feature, plan-time decision) that implements the trait against an in-memory `BTreeMap<(String, String), Vec<u8>>`. Used by `core/`'s own unit tests for `Bundle::open` and by either client's tests if they need it.

#### `Bundle` loader

- **FR-018**: System MUST define `core::artifact::bundle::Bundle` as an owned consumer-side value with these fields: `manifest: Manifest`, `connection: rusqlite::Connection` (open in-memory database with authorized license shards `ATTACH DATABASE`'d), `geometry_reader: core::geometry::FlatGeobufReader` (the type from 006-core-renderer; this spec defines a trait stub or returns the raw geometry bytes that 006 wraps), `distribution_context: DistributionContext`. The `Bundle` is `!Send` deliberately (the rusqlite Connection is `!Send`; we use it on the same task per `client-web.md` §Threading model). The `Arc<Bundle>` machinery for the renderer's hot-swap (FR-023) wraps it; the unsafe-impl-Send approach is rejected.
- **FR-019**: System MUST implement `Bundle::open(manifest_path: &Path, cache: &dyn ArtifactCache, distribution_context: DistributionContext) -> Result<Bundle, AppError>` per the §Acceptance Scenarios for P5. Algorithm: (1) read manifest bytes (from path on native; from cache or main-memory on WASM — the path argument is reinterpreted per target via a cfg-gated helper); (2) parse_manifest → Manifest; (3) read each shard's bytes via `cache.get(...)`; (4) verify_sha256 against each entry; (5) attach authorized license shards into an in-memory rusqlite Connection; (6) read geometry bytes (returned as part of the Bundle for 006-core-renderer's FlatGeobuf reader to wrap).
- **FR-020**: System MUST register the SQLite WASM-friendly `Vec<u8>`-backed VFS in `core/src/sqlite/vfs.rs` per `client.md` §Module layout. Cfg-gated to `target_arch = "wasm32"`. The VFS exposes a custom SQLite VFS implementation that reads pages from a `Vec<u8>` held in user-data; on native, the path is `cfg(not(...))`'d out and the normal SQLite file-path open is used. The `Bundle::open` helper picks the VFS based on target.

#### License authorization

- **FR-021**: System MUST define `core::license::DistributionContext` as a public enum per `client.md` §Attaching license shards with at least two variants for v1: `FirstParty`, `Embedded`. Derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`. Public.
- **FR-022**: System MUST implement `DistributionContext::authorized_classes(self) -> &'static [LicenseShardClass]` returning the exact slices in `client.md` §Attaching license shards: `FirstParty -> &[Base, NonCommercial, ShareAlike]`, `Embedded -> &[Base]`. The function body is a `match self` with no wildcard arm; adding a new `LicenseShardClass` requires a compile-error-driven update of every arm per `client.md`'s "neither addition is silent" property.

#### Hot-swap watch channel

- **FR-023**: System MUST re-export `tokio::sync::watch::{Sender, Receiver}` from `core::artifact::cache::bundle_watch` (or document the consumer-side import path) so clients use the canonical types. The `core/Cargo.toml` depends on `tokio` with the `sync` feature only (single-threaded WASM cannot use `rt-multi-thread`; `sync` is enough for the watch primitive). The consumer side per `client.md` §Bundle hot-swap creates a `watch::channel::<Arc<Bundle>>(initial_bundle)` at startup; the renderer holds the `Receiver`; the loader holds the `Sender`.

#### Test coverage

- **FR-024**: System MUST cover the following surfaces with TDD per Constitution VII (these are "core logic"): `parse_manifest` (round-trip + malformed-input rejection); `parse_discovery_document` (schema_version validation + sunset Option); `verify_sha256` (match + mismatch + boundary cases); `DistributionContext::authorized_classes` (every variant returns the documented slice); `Bundle::open` (full round-trip against a `MockArtifactCache`; SHA-256 mismatch surfaces the documented error; path-traversal in `relative_path` rejected; partial authorization scenarios per FR-022). Tests run against the host target via `cargo test -p core`.
- **FR-025**: System MUST add WASM-target test coverage for `parse_manifest`, `parse_discovery_document`, and `verify_sha256` via `wasm-bindgen-test` (the same harness 003 web-client uses for OPFS tests). The `Bundle::open` test against the `MockArtifactCache` should also pass on WASM (the VFS code path runs there); this verifies the cfg-gating actually works.

### Key Entities

- **`core/` workspace member**: a new library crate at the workspace root that compiles for both the host (Apple Silicon Mac mini) and `wasm32-unknown-unknown` (web client). Owns the consumer-side types + parsers + loader for artifact bundles; the renderer + projection + hit-testing live in 006-core-renderer.
- **`core::artifact::manifest::Manifest`**: the owned consumer-side parsed form of the on-the-wire manifest JSON. Replaces the producer's private `ManifestSerializer<'a>` struct as the canonical type; both ingestion (write side) and every client (read side) reach for it.
- **`core::artifact::cache::ArtifactCache`**: the cross-platform async trait both `OpfsArtifactCache` (web) and `FileSystemArtifactCache.swift`-backed UniFFI wrapper (iOS) implement. The contract `Bundle::open` consumes by reference.
- **`core::artifact::bundle::Bundle`**: the parsed manifest + open SQLite connection (with authorized license shards attached) + geometry bytes (for 006-renderer to wrap). The value the renderer holds via `Arc<Bundle>` and hot-swaps via `tokio::sync::watch`.
- **`core::artifact::discovery::DiscoveryDocument`**: the deserialized form of `https://eafora.org/discovery`. Validated to `schema_version == 1` at parse time.
- **`core::license::DistributionContext`**: the enum that decides which license shards a particular client attaches at bundle-open time. Two variants in v1 (`FirstParty`, `Embedded`).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `cargo build --workspace` (against the post-move state) succeeds with zero compilation errors and zero new warnings.
- **SC-002**: `cargo build -p core --target wasm32-unknown-unknown` succeeds with zero compilation errors.
- **SC-003**: `cargo test -p ingestion` runs every pre-existing ingestion test against the moved types and reports the same pass/fail counts as the pre-move run (no test regressions caused by the extraction).
- **SC-004**: `cargo test -p core` covers FR-024's listed surfaces and passes 100%. The line coverage on `parse_manifest` + `parse_discovery_document` + `verify_sha256` + `Bundle::open` + `DistributionContext::authorized_classes` is at least 90% measured via `cargo llvm-cov` or equivalent (informational; gate is the pass count, not the coverage number).
- **SC-005**: A round-trip integration: bytes written by `ingestion::artifact::writer::manifest::write_manifest` parse via `core::artifact::manifest::parse_manifest` into a `Manifest` whose re-serialization (via the same `serde_json::to_string_pretty` path) is byte-equal to the original. This verifies the producer-and-consumer halves agree on the type's shape.
- **SC-006**: After this PR merges, the 003-web-client and 004-ios-client implementation PRs (when they begin) reach the `core::artifact::Bundle::open`, `core::artifact::cache::ArtifactCache`, `core::license::DistributionContext`, and `core::artifact::discovery::DiscoveryDocument` symbols via the documented import paths without ambiguity. (Verifiable when those PRs go up; not gated on this PR's CI.)

## Assumptions

- The current `ingestion/` crate continues to be the source of truth for everything that doesn't move: the canonical Postgres schema + entity types, the per-source adapters, the artifact writer's `hash_sqlite_shards` / `hash_geometry` / `build_artifacts` / `publish_artifacts` orchestrators, the source-priority merge. Only types and helpers that the client side needs to consume cross the boundary.
- The workspace's Rust edition (`2024` per the workspace root `Cargo.toml`) is on a `rustc` that supports stable AFIT (async fn in trait without `async-trait`). Stable as of `rustc 1.75` (released 2023-12); the workspace's `rust-toolchain.toml` (if present) or the developer's installed `rustup` channel needs to be at or above this version. If the workspace pins below, the `async-trait` crate dependency lands at plan time as the fallback.
- The producer-side `write_manifest` and its existing tests are the regression net for the manifest extraction. The fact that they currently pass against the private serializer struct + that they pass post-move against the owned `Manifest` is the integration check that the type shapes agree.
- `core::artifact::manifest::Manifest`'s `Serialize` impl produces byte-equal output to the existing `ManifestSerializer<'a>`'s output for the same inputs. The producer's deterministic-output property (BTreeMap ordering, pretty-print indentation) is preserved through the change. Implementation-time verification: the `build_manifest_json_is_deterministic_byte_for_byte` test in `ingestion/src/artifact/writer/manifest.rs` continues to pass.
- The web client's WASM-target SQLite VFS (FR-020) is built against `rusqlite` with the `bundled` feature so SQLite ships as part of the WASM bundle (no system SQLite). The exact WASM-target compatibility of `rusqlite`'s `bundled` SQLite is verified at plan time; if the bundled C build doesn't cross-compile cleanly, the fallback is sqlite-wasm-rs or equivalent — plan-level decision.
- The `Region` / `Country` / `Statistic` / etc. entity types that currently live in `ingestion/src/canonical/canonical_model.rs` are NOT moved by this feature. They MAY move in a follow-up if a client genuinely needs them (currently neither client does; the client reads via SQLite shards, not direct Postgres types). This keeps the extraction surface tight.
- The 006-core-renderer feature (next spec, stacked on this branch) extends `core/` with the wgpu pipelines, projection math, hit-testing, and the `core::map::map_renderer::Renderer` type. The `Bundle` defined here (FR-018) intentionally returns the raw geometry bytes; 006 wraps them in the FlatGeobuf reader.

## Scope cutoff

This feature lands the data layer of `core/`: types, trait, parsers, loader, license matrix. Adjacent surfaces that ARE in the architecture but ARE NOT in this feature:

- **wgpu rendering pipelines + WGSL shaders.** Per `docs/architecture/overview.md` §wgpu rendering pipeline. Lands in 006-core-renderer.
- **`core::geometry` module — projection, polygon model, hit_test.** Per `docs/architecture/overview.md` §Geometry, projection, hit-testing. Lands in 006-core-renderer.
- **`core::map::map_renderer::Renderer` type.** Owns the wgpu surface + the bundle watch::Receiver + the dirty flag. Lands in 006-core-renderer.
- **`core::ffi::wasm` (wasm-bindgen surface) and `core::ffi::uniffi` (UniFFI surface).** Per `docs/architecture/overview.md` §Per-binding adapters. The web shell pulls `core::*` directly as a Cargo dep without an FFI module (per `client-web.md` §Workspace placement); the iOS shell needs the UniFFI module per `client-ios.md` §UniFFI: proc-macro form. Both FFI modules land inside their respective per-platform features (003-web-client / 004-ios-client) since the consuming code lives there.
- **The `Region` / `Country` / `Statistic` / etc. entity-projection types from ingestion's canonical_model.rs.** Stay in ingestion unless / until a client needs them.

## Constitution Check

Per Constitution §Compliance review, this spec honors the binding principles as follows:

- **Principle I (Educational neutrality)**: not directly applicable — `core/` ships types + parsers + loader, no UI text or editorial copy.
- **Principle II (Source provenance — NON-NEGOTIABLE)**: directly served. The `Manifest`'s `source_revisions` field carries every source contributing to the build with `revision` + `published` + `fetched` timestamps per `client.md` §Manifest schema; the `Bundle::open` loader makes this provenance reachable to every client query. The `SourceRevision` struct's existing producer-side definition is the move target; nothing changes about the provenance semantics.
- **Principle III (Rust core, native UI shells)**: directly served — this feature IS the "Rust core" extraction. The `core/` crate is the shared substrate every client (Leptos web, SwiftUI iOS, Compose Android) builds against. The native UI shells are out-of-scope by design; they're the consumers.
- **Principle IV (Singularity convention parity)**: applies. No new third-party Rust crates are introduced beyond what's already in the workspace `[workspace.dependencies]` (rusqlite, flatgeobuf, sha2, etc.); `tokio` adds the `sync` feature flag to support the watch channel in WASM (single-threaded; no rt-multi-thread on wasm32 per FR-003). Wildcard re-exports per `feedback_wildcard_re_exports`. `mod.rs` files hold only declarations per `feedback_mod_rs_holds_only_declarations`. The `core/` crate's `0.0.0` placeholder versioning matches `.specify/memory/constitution.md` §Versioning until a real publish happens.
- **Principle V (Explicit over implicit)**: applies. SQL is hand-written via `rusqlite::Connection::prepare` for any `Bundle::open` shard-attach SQL (no ORM). No RPC framework, no codegen, no route macros. The `ArtifactCache` trait is a plain async trait; no async-trait crate dep per FR-016.
- **Principle VI (CDN-delivered data, no live API through v2)**: directly served. `core/`'s entire shape assumes data arrives as CDN-hosted artifact bundles; the `Bundle::open` API takes a manifest-path-and-cache pair, not a live HTTP client. No live API surface in `core/`.
- **Principle VII (Test-first for core logic)**: directly served. FR-024 names the TDD-required surfaces (`parse_manifest`, `parse_discovery_document`, `verify_sha256`, `DistributionContext::authorized_classes`, `Bundle::open`); all are core logic that must follow Red-Green-Refactor. FR-025 extends coverage to WASM via `wasm-bindgen-test`.
- **Principle VIII (Workflow discipline)**: this is the fifth `/speckit-specify` feature; spec / plan / tasks land in the same PR per `feedback_spec_and_plan_same_pr.md`. Branch `005-core-data` follows the per-body-of-work + `>>> branch:` marker convention; the next spec (006-core-renderer) stacks on this branch since the renderer depends on `Bundle` + `Viewport` + `FrameState` defined here.

No principle violations identified; no constitution amendments proposed.
