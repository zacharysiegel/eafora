# Contract: core/ public API

> Phase 1 output of `/speckit-plan` for 005-core-data. The complete list of public symbols `core/` exposes to its consumers (`ingestion/` as the producer; `web/`, `ios/`, `android/` via 003 / 004 / future as the consumers; `core/`'s own tests). Sourced from `data-model.md`.

## Contract type

Library crate: the contract is the public Rust API surface (types, traits, functions, constants). No HTTP routes, no CLI subcommands, no IPC schemas — `core/` is purely consumed in-process by other Rust crates.

## Public symbol inventory

| Symbol                                                            | Module                          | Kind     | Stability |
|-------------------------------------------------------------------|---------------------------------|----------|-----------|
| `AppError`                                                        | `core::error`                   | type     | locked    |
| `render_error_chain(error: &dyn Error) -> String`                | `core::error`                   | function | locked    |
| `REVISION`                                                        | `core` (crate root)             | const    | new       |
| `FileReference`                                                   | `core::filesystem`              | type     | locked    |
| `Hashed<T>`                                                       | `core::filesystem`              | type     | locked    |
| `sha256_hex(bytes: &[u8]) -> String`                              | `core::filesystem`              | function | locked    |
| `sha256_hex_of_file(path: &Path) -> Result<String, AppError>`     | `core::filesystem` (native only)| function | locked    |
| `verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), AppError>` | `core::filesystem`      | function | new       |
| `filename_of(path: &Path) -> Result<&str, AppError>`              | `core::filesystem` (native only)| function | locked    |
| `read_bytes(path: &Path) -> Result<Vec<u8>, AppError>`            | `core::filesystem` (native only)| function | locked    |
| `load_hashed_file(base_dir: &Path, relative_path: &str, expected_sha256_hex: &str) -> Result<Hashed<FileReference>, AppError>` | `core::filesystem` (native only) | function | locked |
| `StatisticKind`                                                   | `core::canonical::canonical_model` | type  | locked    |
| `DataSourceKind`                                                  | `core::canonical::canonical_model` | type  | locked    |
| `DataStatus`                                                      | `core::canonical::canonical_model` | type  | locked    |
| `LicenseClass`                                                    | `core::canonical::canonical_model` | type  | locked    |
| `LicenseShardClass`                                               | `core::canonical::canonical_model` | type  | locked    |
| `SourceRevision`                                                  | `core::canonical::canonical_model` | type  | locked    |
| `NaiveDatePeriod`                                                 | `core::canonical::canonical_model` | type  | locked    |
| `Region` (Model only; Entity stays in ingestion)                  | `core::canonical::canonical_model` | type  | locked    |
| `Country` (Model only; Entity stays in ingestion)                 | `core::canonical::canonical_model` | type  | locked    |
| `Statistic` (Model only; Entity stays in ingestion)               | `core::canonical::canonical_model` | type  | locked    |
| `DataSource` (Model only; Entity stays in ingestion)              | `core::canonical::canonical_model` | type  | locked    |
| `MANIFEST_FILENAME`                                               | `core::artifact::manifest`      | const    | locked    |
| `MANIFEST_SCHEMA_VERSION`                                         | `core::artifact::manifest`      | const    | new       |
| `MANIFEST_LATEST_KEY`                                             | `core::artifact::manifest`      | const    | new       |
| `CACHE_CONTROL_MANIFEST`, `CACHE_CONTROL_SHARD`                   | `core::artifact::manifest`      | const    | new       |
| `CONTENT_TYPE_MANIFEST`, `CONTENT_TYPE_FLATGEOBUF`, `CONTENT_TYPE_SQLITE` | `core::artifact::manifest` | const | new |
| `SUBDIR_GEOMETRY`                                                 | `core::artifact::manifest`      | const    | locked    |
| `SUBDIR_DATA`                                                     | `core::artifact::manifest`      | const    | locked    |
| `Manifest`                                                        | `core::artifact::manifest`      | type     | new       |
| `ManifestEntry`                                                   | `core::artifact::manifest`      | type     | new       |
| `parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError>`      | `core::artifact::manifest`      | function | new       |
| `DISCOVERY_SCHEMA_VERSION`                                        | `core::artifact::discovery`     | const    | new       |
| `DISCOVERY_URL`                                                   | `core::artifact::discovery`     | const    | new       |
| `DiscoveryDocument`                                               | `core::artifact::discovery`     | type     | new       |
| `parse_discovery_document(bytes: &[u8]) -> Result<DiscoveryDocument, AppError>` | `core::artifact::discovery` | function | new |
| `ArtifactCache` (trait)                                           | `core::artifact::cache`         | trait    | new       |
| `MockArtifactCache` (`#[cfg(test)]`)                              | `core::artifact::cache`         | type     | new       |
| `FlatGeobufReader`                                                | `core::artifact::geometry`      | type     | new       |
| `open_flatgeobuf_reader(bytes: Vec<u8>) -> Result<FlatGeobufReader, AppError>` | `core::artifact::geometry` | function | new |
| `GEOMETRY_LAYER_NAME`, `GEOMETRY_FILENAME_STEM`                   | `core::artifact::geometry`      | const    | new       |
| `FEATURE_COLUMN_ISO3`, `FEATURE_COLUMN_NAME_EN`                   | `core::artifact::geometry`      | const    | new       |
| `SHARD_FILENAME_EXTENSION`, `GEOMETRY_FILENAME_EXTENSION`         | `core::artifact::geometry`      | const    | new       |
| `Feature`                                                         | `core::artifact::geometry`      | type     | new       |
| `Polygon`                                                         | `core::artifact::geometry`      | type     | new       |
| `BoundingBox`                                                     | `core::artifact::geometry`      | type     | new       |
| `Bundle`                                                          | `core::artifact::bundle`        | type     | new       |
| `Bundle::open(version_label: &str, cache: &dyn ArtifactCache, distribution_context: DistributionContext) -> Result<Bundle, AppError>` (async) | `core::artifact::bundle` | function | new |
| `StatisticShardKey`                                               | `core::artifact::bundle`        | type     | locked    |
| `Sender`, `Receiver`, `channel` (re-exports of `tokio::sync::watch::*`) | `core::artifact::bundle_watch` | re-export | new |
| `DistributionContext`                                             | `core::license::license`        | type     | new       |
| `DistributionContext::authorized_classes() -> &'static [LicenseShardClass]` | `core::license::license` | function | new |
| `Connection` (typedef: `rusqlite::Connection` on native, `sqlite_wasm_rs::Connection` on wasm32) | `core::sqlite::vfs` | type | new |
| `open_connection_from_bytes(name: &str, bytes: Vec<u8>) -> Result<Connection, AppError>` | `core::sqlite::vfs` | function | new |
| `APPLICATION_ID`                                                  | `core::sqlite::schema`          | const    | new       |
| `SCHEMA_VERSION`                                                  | `core::sqlite::schema`          | const    | new       |
| `TABLE_STATISTIC_VALUE`, `TABLE_SHARD_KEY`                        | `core::sqlite::schema`          | const    | new       |
| `INDEX_STATISTIC_VALUE_BY_REGION`                                 | `core::sqlite::schema`          | const    | new       |
| `COL_REGION_ISO3`, `COL_REGION_ID`, `COL_PERIOD_START`, `COL_PERIOD_END`, `COL_VALUE`, `COL_DATA_STATUS`, `COL_DATA_SOURCE_CODE`, `COL_DATA_SOURCE_REVISION` | `core::sqlite::schema` | const | new |
| `COL_STATISTIC_KIND`, `COL_LICENSE_SHARD_CLASS`                   | `core::sqlite::schema`          | const    | new       |
| `PERIOD_DATE_FORMAT`                                              | `core::sqlite::schema`          | const    | new       |
| `shard_schema_ddl() -> &'static str`                              | `core::sqlite::schema`          | function | new       |
| `validate_shard_header(connection: &rusqlite::Connection) -> Result<(), AppError>` | `core::sqlite::schema` | function | new |

Stability key:
- **locked**: moves from `ingestion/` with the same shape; `ingestion/` keeps a `pub use` re-export so existing call sites stay valid.
- **new**: new in this feature; no prior shape to preserve.

## Compatibility contract

- **`Manifest` byte-equal-output property**: The producer's `write_manifest` (post-move) MUST emit byte-equal output for byte-equal inputs (the existing `build_manifest_json_is_deterministic_byte_for_byte` test continues to pass). It does NOT emit byte-equal output to pre-move bytes — the post-move bytes include the new `manifest_schema_version: 1` field. See spec.md FR-013 + SC-005.
- **Re-export discipline**: every `ingestion/` import path that worked before this feature continues to work after (e.g. `use crate::canonical::canonical_model::StatisticKind` still resolves; the type is now `core::canonical::canonical_model::StatisticKind` reached via a `pub use` in ingestion's `canonical_model.rs`). Per spec P1 acceptance #4.
- **`Bundle: Send + Sync`**: the type-level guarantee that `Arc<Bundle>` can cross thread boundaries. Per spec FR-018 + §Clarifications Q2. The compiler enforces this; if any future field violates the bound, compilation fails (a deliberate design check).
- **No `Send + Sync` bound on `ArtifactCache`**: the trait MUST be usable from `!Send` contexts (web's `OpfsArtifactCache` via `JsValue`). Per spec FR-016.
- **`DistributionContext::authorized_classes`** is a `&'static` slice; the caller doesn't need to allocate. Per spec FR-022.

## Backward-compatibility versioning

`core/` is at `0.0.0` per the constitution's placeholder-versioning rule (§Versioning). Until the crate is published to crates.io (no plan for that in v1–v2), version bumps are internal-only and don't follow SemVer. The `MANIFEST_SCHEMA_VERSION` constant is the contract that DOES need careful versioning: v2+ shape changes increment it; consumers that don't know about the new version reject manifests at parse time.

## Tests as executable contracts

Per spec FR-024 + FR-025, the following tests are themselves part of the public-API contract — any code change that breaks them either breaks the contract (requires owner discussion) or has a buggy test:

| Test                                                      | Asserts                                                                 | Target  |
|-----------------------------------------------------------|-------------------------------------------------------------------------|---------|
| `parse_manifest_round_trips_fixture_set`                  | Manifest bytes parse + re-serialize byte-equal.                          | host    |
| `parse_manifest_rejects_unknown_schema_version`           | `schema_version != 1` returns the documented `AppError` shape.          | host    |
| `parse_manifest_rejects_unknown_statistic_code`           | Manifest with `"unknown_stat"` returns `AppError` containing that string.| host    |
| `parse_manifest_rejects_malformed_sha256`                 | `sha256.len() != 64` returns `AppError` naming the entry's relative_path.| host    |
| `parse_manifest_rejects_path_traversal_relative_path`     | `relative_path` containing `..` or starting with `/` returns `AppError`. | host    |
| `parse_discovery_document_round_trips_fixture`            | DiscoveryDocument parse round-trip works.                                | host    |
| `parse_discovery_document_rejects_unknown_schema_version` | `schema_version != 1` returns the documented `AppError` shape.          | host    |
| `parse_discovery_document_handles_missing_sunset_field`   | Absent `sunset` → `None`.                                                | host    |
| `verify_sha256_accepts_matching_hash`                     | Matching input returns `Ok(())`.                                         | host    |
| `verify_sha256_rejects_mismatched_hash`                   | Mismatched input returns `AppError` containing both 8-hex prefixes.     | host    |
| `distribution_context_first_party_authorizes_all_classes` | `FirstParty.authorized_classes() == &[Base, NonCommercial, ShareAlike]`. | host    |
| `distribution_context_embedded_authorizes_base_only`      | `Embedded.authorized_classes() == &[Base]`.                              | host    |
| `bundle_open_round_trip_against_mock_cache`               | Populated `MockArtifactCache` → `Bundle::open` succeeds; bundle's `shard_bytes` matches the manifest entries. | host |
| `bundle_open_rejects_missing_manifest`                    | `cache.get(version, "manifest.json")` returns `Ok(None)` → `AppError`.   | host    |
| `bundle_open_rejects_sha256_mismatch`                     | Shard bytes don't match SHA-256 → `AppError` naming the mismatched entry.| host    |
| `bundle_open_skips_unauthorized_shards`                   | `Embedded` context → only `Base` shards in `bundle.shard_bytes`.        | host    |
| `bundle_open_eagerly_parses_geometry`                     | Bundle's `geometry_reader` is constructed; iteration returns features.   | host    |
| `bundle_is_send_sync`                                     | Compile-time assertion: `Arc<Bundle>: Send + Sync` (via `fn assert_send_sync<T: Send + Sync>() {}`). | host |
| `shard_schema_ddl_creates_expected_tables_and_index`      | Execute `shard_schema_ddl()` against an in-memory rusqlite Connection; assert `statistic_value`, `shard_key` tables exist with the expected columns; assert `statistic_value_by_region` index exists. | host |
| `validate_shard_header_accepts_correctly_initialized_connection` | Open in-memory Connection; set `application_id` + `user_version` PRAGMAs to the constants; `validate_shard_header(&conn)` returns `Ok(())`. | host |
| `validate_shard_header_rejects_wrong_application_id`      | Set `application_id` to `0xDEADBEEF`; `validate_shard_header` returns `AppError` whose message starts with `"sqlite shard: application_id mismatch"`. | host |
| `validate_shard_header_rejects_unknown_schema_version`    | Set `user_version` to `99`; `validate_shard_header` returns `AppError` whose message starts with `"sqlite shard: unknown schema_version"`. | host |
| `parse_manifest_round_trips_fixture_set` (wasm32 dup)     | Same as host, on wasm32-unknown-unknown.                                 | wasm32  |
| `parse_discovery_document_round_trips_fixture` (wasm32 dup)| Same as host.                                                            | wasm32  |
| `verify_sha256_accepts_matching_hash` (wasm32 dup)        | Same as host.                                                            | wasm32  |
| `bundle_open_round_trip_against_mock_cache` (wasm32 dup)  | Same as host (verifies the VFS cfg-gating works on the wasm32 target).  | wasm32  |

The exact test names will likely shift during implementation (the convention is `function_under_test_scenario` per `~/.claude/CLAUDE.md`'s test-naming preference); the column above describes the asserted property, not the literal test name.
