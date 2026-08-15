# Data model: shared/ crate — data layer (005-core-data)

> Phase 1 output of `/speckit-plan` for 005-core-data. Strict definitions of every public type `shared/` exposes, plus the trait signatures and constants. Sourced directly from spec.md's FR list + §Clarifications session 2026-06-22.

## Conventions

- Per `docs/conventions/types.md` §Core dichotomy: every DB-touched type has a Model (typed enums; bare-named) + Wire (`String` for text columns; suffixed `Entity` / `Projection`). The types here are CONSUMER-side; they don't touch the database. Most don't need an Entity / Projection pair; the few that do (`Manifest` exists in both Rust-owned consumer form AND wire JSON form) reuse a single struct with serde derives because serde IS the wire layer for these types.
- Per `docs/conventions/types.md` §Enums: bare descriptive name when the type reads as a classification (`LicenseClass`, `LicenseShardClass`, `DataStatus`, `DistributionContext`); `Kind` suffix when the bare name would shadow a struct (`StatisticKind`, `DataSourceKind`). Both flavors implement `TryFrom<&str>` for the wire-string direction.
- Per `docs/conventions/types.md` §Variable naming: collection variables prefixed with the type they contain (`shard_bytes` not `bytes`).
- Per `feedback_inline_sql_constraints` / `feedback_no_dollar_quoted_sql` / etc.: no SQL in this layer (consumer side).

## Module: `shared::error`

`shared::AppError` is shared's own newtype, generated via `minimer::define_app_error!`. Ingestion has its own separate `AppError` newtype; the two interconvert at the boundary. (Earlier framing of "ingestion imports it from shared" is incompatible with Rust's orphan rule — minimer's macro doc explicitly notes this and says downstream crates should define their own newtype. See plan.md §Outstanding decision #1 for the reasoning.)

### `AppError`

Per `minimer::define_app_error!(pub AppError)`. The `From` impls registered in `shared/src/error.rs`:

```rust
use std::error::Error;

minimer::define_app_error!(pub AppError);

minimer::impl_from_error!(AppError, serde_json::Error);
minimer::impl_from_error!(AppError, rusqlite::Error);
minimer::impl_from_error!(AppError, flatgeobuf::Error);
minimer::impl_from_error!(AppError, geozero::error::GeozeroError);
minimer::impl_from_error!(AppError, log::SetLoggerError);
```

Parser-surface set; covers what `shared/` itself touches.

`render_error_chain(error: &dyn Error) -> String` — moved verbatim from `ingestion/src/error.rs` (walks the error's `source()` chain, joining each level with ` -> `).

### Ingestion's separate newtype + cross-conversion

`ingestion/src/error.rs` keeps its own `AppError` newtype:

```rust
minimer::define_app_error!(pub AppError);

// Ingestion-only From impls for ingestion-only error families:
minimer::impl_from_error!(AppError, sqlx::Error);
minimer::impl_from_error!(AppError, reqwest::Error);
minimer::impl_from_error!(AppError, zip::result::ZipError);
minimer::impl_from_error!(AppError, shapefile::Error);
minimer::impl_from_error!(AppError, shapefile::dbase::Error);
minimer::impl_from_error!(AppError, secr::error::Error);
minimer::impl_from_error!(AppError, dotenvy::Error);
minimer::impl_from_error!(AppError, base64::DecodeError);

// One-line cross-conversion bridge — orphan-rule-OK because the target
// (ingestion::AppError) is local to ingestion. Lets ingestion `?`-propagate
// from shared functions like `shared::artifact::manifest::parse_manifest`.
impl From<shared::AppError> for AppError {
    fn from(err: shared::AppError) -> Self {
        Self(err.0)
    }
}
```

Both newtypes wrap the same `minimer::AppError`, so the underlying error storage is uniform across the two crates. The two newtypes are conceptually one error type with two namespaces; the orphan rule forces the dual-newtype shape rather than a single shared type.

## Crate root: `shared::REVISION` (and `shared/build.rs`)

Per spec FR-020k. The source revision the binary was built from, captured at compile time and exposed as a `&'static str` constant at the crate root.

### `shared/build.rs`

```rust
use std::process::Command;

fn main() {
    let revision: Option<String> = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|out| if out.status.success() { Some(out.stdout) } else { None })
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string());

    let is_shipping_build: bool = std::env::var("PROFILE").unwrap_or_default() != "debug";

    let resolved_revision: String = match revision {
        Some(revision) => revision,
        None if is_shipping_build => panic!(
            "git rev-parse HEAD failed: a release build MUST embed a real source revision \
             (EAFORA_REVISION) for crash symbolication. Build from a full git clone with `git` on PATH.",
        ),
        None => {
            println!("cargo:warning=git rev-parse HEAD failed; EAFORA_REVISION=unknown (debug build)");
            "unknown".to_string()
        }
    };

    println!("cargo:rustc-env=EAFORA_REVISION={}", resolved_revision);
}
```

Git-unavailable handling is profile-conditional. On a **debug build** the script emits a `cargo:warning` and falls back to `EAFORA_REVISION=unknown`, so the crate builds anywhere (shallow / archive checkout, no `git` on PATH). On any **release build** (`PROFILE != "debug"`) the script `panic!`s and aborts — a shipped binary must never carry `unknown`, since the revision's only load-bearing use is crash symbolication of release builds. The gate is `!= "debug"` (fail-closed) so every non-debug profile is covered.

### `shared/src/revision.rs`

```rust
pub const REVISION: &str = env!("EAFORA_REVISION");
```

`REVISION` lives in its own module (not `lib.rs`) so `lib.rs` stays pure module redirection (`pub mod` + `pub use` only) per `feedback_mod_rs_holds_only_declarations`. `lib.rs` re-exports it via `pub use revision::*;`. The matching iOS-side `eafora.revision()` UniFFI export lives in 004-ios-client's scope.

No `cargo:rerun-if-changed` directives are emitted (the `.git/HEAD` path-watching is brittle and the value is dubious). With no directives, Cargo re-runs the script when any file under `shared/` changes; `REVISION` can therefore lag HEAD during dev iteration when a commit doesn't touch `shared/`, but shipped release builds are clean builds that always capture the correct revision, which is the only context where `REVISION` is load-bearing (crash symbolication).

## Module: `shared::filesystem`

Moved wholesale from `ingestion/src/filesystem.rs`. Cross-target items compile for every target; items that can't compile to wasm32 are `#[cfg(not(target_arch = "wasm32"))]`-gated. `ingestion/src/filesystem.rs` is deleted; ingestion call sites import `shared::filesystem` directly (no re-export).

### `FileReference`

```rust
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct FileReference {
    pub path: PathBuf,
    pub byte_count: u64,
}
```

(Producer-side use case; gated off wasm32 because it holds a `PathBuf`. The consumer-side `Bundle::open` doesn't construct these — it goes through the cache, not the filesystem.)

### `Hashed<T>`

```rust
#[derive(Debug, Clone)]
pub struct Hashed<T> {
    inner: T,
    sha256_hex: String,
}

impl<T> Hashed<T> {
    pub fn new(inner: T, bytes: impl AsRef<[u8]>) -> Self;
    pub fn new_with_sha(inner: T, sha256_hex: String) -> Self;
    pub fn sha256_hex(&self) -> &str;
}

impl<T> Deref for Hashed<T> { /* delegates to inner */ }
```

### Functions

```rust
// Cross-target:
pub fn sha256_hex(bytes: &[u8]) -> String;
pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), AppError>;

// Not for wasm32:
#[cfg(not(target_arch = "wasm32"))]
pub fn sha256_hex_of_file(path: &Path) -> Result<String, AppError>;

#[cfg(not(target_arch = "wasm32"))]
pub fn filename_of(path: &Path) -> Result<&str, AppError>;

#[cfg(not(target_arch = "wasm32"))]
pub fn read_bytes(path: &Path) -> Result<Vec<u8>, AppError>;

#[cfg(not(target_arch = "wasm32"))]
pub fn load_hashed_file(
    base_dir: &Path,
    relative_path: &str,
    expected_sha256_hex: &str,
) -> Result<Hashed<FileReference>, AppError>;
```

`verify_sha256` per spec FR-009: on mismatch, returns `AppError` whose message contains both `expected_hex` (first 8 hex chars) and the actual hash (first 8 hex chars). On match, returns `Ok(())`. Cross-target so wasm32 consumers (the web client's loader, when verifying fetched bytes against manifest entries) can use it.

`sha256_hex_of_file`, `filename_of`, `read_bytes`, `load_hashed_file`: gated off wasm32 (cfg-gated). `Bundle::open` does NOT call any of these — it goes through the cache trait. Producer-side code (ingestion's publish flow) uses them via `shared::filesystem::` paths.

## Module: `shared::canonical::canonical_model`

Moved from `ingestion/src/canonical/canonical_model.rs` per plan.md §Phasing step 3, with `NaiveDatePeriod` lifted from `ingestion/src/adapter/adapter_model.rs` per spec FR-005a.

Per `docs/conventions/types.md` §Core dichotomy: every DB-touched type has a Model (consumer-facing, typed) + an Entity / Projection (Postgres wire shape). This module holds the **Models**; the matching **Entities stay in `ingestion/src/canonical/canonical_model.rs`** since only the producer's sqlx queries construct them. The `From<Entity> for Model` / `TryFrom<Entity> for Model` impls live next to the Entity (per the same convention) — the orphan rule permits ingestion adding the impl because ingestion owns the Entity even though Model is now foreign (from shared).

### `StatisticKind`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "&str", into = "&str")]
pub enum StatisticKind {
    Tfr,
    #[cfg(test)]
    TestAlpha,
}

impl StatisticKind {
    pub fn code(self) -> &'static str;
}

impl TryFrom<&str> for StatisticKind { type Error = AppError; /* ... */ }
```

Adds `Serialize` / `Deserialize` so the consumer-side `Manifest.statistics: BTreeMap<StatisticKind, _>` round-trips through JSON. **As implemented** (code is canonical over this doc): the five string-coded enums get their impls from a local `impl_code_serde!` macro that serializes each value as its `code()` / `as_str()` string and deserializes an owned `String` via `TryFrom<&str>`, so the code strings stay defined once on each enum's own impls. This supersedes the `#[serde(try_from = "&str", into = "&str")]` attribute shown in the blocks above — `&str` deserialization fails on owned / escaped / map-key input, and `into = "&str"` needs an `Into<&str>` impl that was never supplied. The same macro covers `DataSourceKind`, `DataStatus`, `LicenseClass`, and `LicenseShardClass`. The test-only variants (`StatisticKind::TestAlpha`, `DataSourceKind::TestAlpha` / `TestBeta`) are unconditional, so dependent crates can construct them in tests without any cross-crate feature plumbing; only the inbound parse arms (the `TryFrom<&str>` match arms, which serde `Deserialize` routes through) are gated `#[cfg(test)]`, so a production build rejects `"_test_alpha"` / `"_test_beta"` as unknown codes and no synthetic identity can enter from untrusted strings.

### `DataSourceKind`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "&str", into = "&str")]
pub enum DataSourceKind {
    WorldBankWDI,
    #[cfg(test)] TestAlpha,
    #[cfg(test)] TestBeta,
}

impl DataSourceKind {
    pub fn code(self) -> &'static str;
}

impl TryFrom<&str> for DataSourceKind { type Error = AppError; /* ... */ }
```

Same Serialize / Deserialize addition rationale as `StatisticKind`.

### `DataStatus`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "&str", into = "&str")]
pub enum DataStatus {
    Final,
    Provisional,
    Preliminary,
    Projection,
    Imputed,
    Interpolated,
}

impl DataStatus {
    pub fn as_str(self) -> &'static str;
}

impl TryFrom<&str> for DataStatus { type Error = AppError; /* ... */ }
```

### `LicenseClass`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "&str", into = "&str")]
pub enum LicenseClass {
    PublicDomain,
    Attribution,
    AttributionShareAlike,
    NonCommercial,
}

impl LicenseClass {
    pub fn as_str(self) -> &'static str;
}

impl TryFrom<&str> for LicenseClass { type Error = AppError; /* ... */ }
```

### `LicenseShardClass`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "&str", into = "&str")]
pub enum LicenseShardClass {
    Base,
    ShareAlike,
    NonCommercial,
}

impl LicenseShardClass {
    pub fn from_license_class(license_class: LicenseClass) -> LicenseShardClass;
    pub fn as_str(self) -> &'static str;
}

impl TryFrom<&str> for LicenseShardClass { type Error = AppError; /* ... */ }
```

### `SourceRevision`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRevision {
    pub revision: String,
    pub published: Option<DateTime<Utc>>,
    pub fetched: DateTime<Utc>,
}
```

Already has Serialize / Deserialize in the current producer-side definition; moves verbatim.

### `NaiveDatePeriod`

Pure value type lifted from `ingestion/src/adapter/adapter_model.rs` per spec FR-005a. Half-open `[start, end)` interval matching the canonical store's `period_start` / `period_end` columns. Consumers use it for period-keyed SQLite shard queries (year scrubber).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NaiveDatePeriod {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl NaiveDatePeriod {
    pub fn from_year(year: i32) -> Result<NaiveDatePeriod, AppError>;
    pub fn to_year(&self) -> Option<i32>;
}
```

`to_year` returns `Some(year)` iff the period is exactly a calendar year (`YYYY-01-01` to `YYYY+1-01-01`). The `#[allow(dead_code)]` annotation on the producer-side definition drops on move — `to_year` becomes consumer-side live code (the map view's year-scrubber label reads it).

### `Region` (Model)

The consumer-side Model paired with the producer-side `RegionEntity` (stays in ingestion). Consumers use this for region-detail UI (`name_en`, `level`, `parent_region_id` parent-chain rendering) and Universal Link routing (`code` slug).

```rust
pub struct Region {
    pub id: Uuid,
    pub code: String,                       // URL-safe slug, e.g. "usa", "south_america"
    pub name_en: String,
    pub level: String,                      // e.g. "country", "subregion", "supranational"
    pub parent_region_id: Option<Uuid>,
    pub m49_code: Option<String>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}
```

`RegionEntity` + `impl From<RegionEntity> for Region` stay in `ingestion/src/canonical/canonical_model.rs` (Entity is producer-only Postgres wire shape).

### `Country` (Model)

Paired with `CountryEntity` (stays in ingestion). Consumers join FlatGeobuf features (which carry `iso3`) to `Country` to reach the parent `Region`.

```rust
pub struct Country {
    pub region_id: Uuid,
    pub iso3: String,
    pub iso2: String,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}
```

### `Statistic` (Model)

Paired with `StatisticEntity` (stays in ingestion). Consumers display `name_en` + `units` + `description` in the statistic-picker chrome + the detail panel.

```rust
pub struct Statistic {
    pub id: Uuid,
    pub code: String,                       // matches `StatisticKind::code()` for the typed variant
    pub name_en: String,
    pub description: String,
    pub units: String,                      // e.g. "births per woman" for TFR
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}
```

### `DataSource` (Model)

Paired with `DataSourceEntity` (stays in ingestion). Consumers display attribution in the source-panel chrome per `docs/design/stub-desktop.html` (top-right citation, etc.).

```rust
pub struct DataSource {
    pub id: Uuid,
    pub kind: DataSourceKind,               // typed; matches the moved enum
    pub name_en: String,                    // e.g. "World Bank: World Development Indicators"
    pub homepage_url: String,
    pub license_class: LicenseClass,        // typed; matches the moved enum
    pub license_name: String,               // e.g. "CC BY 4.0"
    pub license_url: String,
    pub attribution_text: String,           // publisher's literal attribution string
    pub preference_rank: i32,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
}
```

`DataSourceEntity` (with `code: String` instead of `kind: DataSourceKind` + `license_class: String` instead of `license_class: LicenseClass`) + `impl TryFrom<DataSourceEntity> for DataSource` stay in `ingestion/src/canonical/canonical_model.rs`.

### Types that DO NOT move

These stay in ingestion entirely (not just Entity-stays-Model-moves; nothing about them crosses into `shared/`):

- **`StatisticValue` + `StatisticValueEntity`**: consumer-side `statistic_value` reads happen against SQLite shards with a different column set (`region_iso3 text` not `region_id uuid`; no `superseded`; no FK references). The Postgres-shaped `StatisticValue` Model isn't the right consumer type; consumers query the shard directly.
- **`SourceChoice` + `SourceChoiceEntity`**: producer-only merge configuration. Written into shards at build time; consumers see only the resulting `statistic_value` rows.
- **`ArtifactVersion` + `ArtifactVersionEntity`**: producer-only publish bookkeeping. The `latest/manifest.json` discovery flow exposes `version_label` + `manifest_url` indirectly (via the manifest's URL), but consumers don't read the Postgres row directly.

## Module: `shared::artifact::manifest`

### Constants

```rust
pub const MANIFEST_FILENAME: &str = "manifest.json";
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const SUBDIR_GEOMETRY: &str = "geometry";
pub const SUBDIR_DATA: &str = "data";

/// The stable-pointer key on the destination per `client.md` §Live bundle.
/// Producer (when the future `latest/manifest.json` upload step lands) uploads
/// a byte-for-byte copy of the just-published manifest under this key.
/// Consumer fetches `<repository_base_url>/<MANIFEST_LATEST_KEY>` at startup.
pub const MANIFEST_LATEST_KEY: &str = "latest/manifest.json";
```

### `Manifest`

Per spec FR-010 + §Clarifications Q3:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Always 1 for v1. First field in serialization order (so a parser can
    /// fail fast on shape changes before parsing the rest).
    pub manifest_schema_version: u32,

    /// `YYYY-MM-DD+<surname>` from the Nobel-laureate generator (producer side).
    pub version: String,

    pub artifact_created: DateTime<Utc>,

    pub geometry: ManifestEntry,

    /// Keyed first by statistic code, then by license shard class.
    /// BTreeMap (not HashMap) for deterministic serialization order.
    pub statistics: BTreeMap<StatisticKind, BTreeMap<LicenseShardClass, ManifestEntry>>,

    /// Per-source revision metadata; one entry per data source contributing to the build.
    pub source_revisions: BTreeMap<DataSourceKind, SourceRevision>,
}
```

Field order in the source matches field order in serialized JSON. `serde_json::to_string_pretty` respects the source order for structs.

### `ManifestEntry`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Rooted at the version directory. Validated to NOT contain `..` and NOT
    /// start with `/` (per FR-012 + §Edge Cases).
    pub relative_path: String,
    pub size_bytes: u64,
    /// Full SHA-256 hex; 64 lowercase hex chars.
    pub sha256: String,
}
```

### Functions

```rust
/// Parse manifest bytes into the owned consumer-side `Manifest`. Validates
/// `manifest_schema_version == MANIFEST_SCHEMA_VERSION`; rejects unknown
/// versions with an `AppError` whose message contains `"unknown manifest_schema_version {N}"`.
/// Validates each entry's `sha256` is 64 hex chars; rejects path-traversal
/// `relative_path` (`..` or absolute paths).
pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, AppError>;
```

The `manifest_schema_version` gate (and the discovery `schema_version` gate) is implemented via the shared `shared::artifact::schema_version::require_schema_version(bytes, field_name, expected)` helper, which reads only the version field through `serde_json::Value` — so a future version that changes the document shape is reported as `unknown {field_name} {found}` rather than a field-level parse error.

## Module: `shared::artifact::discovery`

### Constants

```rust
pub const DISCOVERY_SCHEMA_VERSION: u32 = 1;

/// The single forever-URL of the Eafora system per `client.md` §Discovery URL.
/// Consumers commit to exactly one immutable URL; everything else (including
/// `repository_base_url`) is server-supplied at runtime by the discovery doc
/// fetched from this URL. Both web (`fetch.rs`) and iOS (`URLSessionFetcher`)
/// reach for this constant rather than hand-coding the literal independently.
pub const DISCOVERY_URL: &str = "https://eafora.org/discovery";
```

### `DiscoveryDocument`

Per spec FR-014 + `docs/architecture/client.md` §Discovery document shape:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryDocument {
    pub schema_version: u32,

    /// e.g. "https://repository.eafora.org"
    pub repository_base_url: String,

    /// Lowest client version this contract still supports.
    pub minimum_client_version: String,

    /// `None` in steady state. RFC 3339 timestamp string when the contract is being retired.
    /// Type is `Option<String>` (not `Option<DateTime>`); the caller decides how to compare against `now()`.
    pub sunset: Option<String>,
}
```

### Functions

```rust
/// Parse a discovery document JSON. Rejects `schema_version` other than
/// `DISCOVERY_SCHEMA_VERSION` with an `AppError` whose message contains the
/// literal `"unknown schema_version {N}"`.
pub fn parse_discovery_document(bytes: &[u8]) -> Result<DiscoveryDocument, AppError>;
```

## Module: `shared::artifact::cache`

### `ArtifactCache` (trait)

Per spec FR-016 + the §Clarifications Q5 decision (cache trait is the cross-platform "where do bytes live" abstraction):

```rust
pub trait ArtifactCache {
    async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppError>;
    async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError>;
    async fn list_versions(&self) -> Result<Vec<String>, AppError>;
    async fn delete_version(&self, version_label: &str) -> Result<(), AppError>;
}
```

No `Send + Sync` bounds (web's `OpfsArtifactCache` holds `!Send` `JsValue` indirectly). Stable AFIT — no `async-trait` crate per research Topic 1.

### `MockArtifactCache`

Per spec FR-017 + plan §Outstanding decision #3 (test-only). Lives inside `cache.rs`'s `#[cfg(test)] pub(crate) mod tests`, so PR D's `bundle.rs` tests reach it as `crate::artifact::cache::tests::MockArtifactCache` (a `pub(crate)` item inside a `pub(crate)` test module; `cfg(test)` is crate-wide under `cargo test`, so it doesn't need to cross a crate boundary):

```rust
#[cfg(test)]
pub(crate) mod tests {
    pub(crate) struct MockArtifactCache {
        /// Keyed by (version_label, file_relative_path) — bytes are owned.
        entries: tokio::sync::Mutex<BTreeMap<(String, String), Vec<u8>>>,
    }

    impl MockArtifactCache {
        pub(crate) fn new() -> Self;
        pub(crate) async fn insert(&self, version_label: &str, file_relative_path: &str, bytes: Vec<u8>);
    }

    impl ArtifactCache for MockArtifactCache {
        /* trait impl bodies */
    }

    /* the mock's own #[tokio::test] functions */
}
```

Note: `tokio::sync::Mutex` (not `std::sync::Mutex`) because the trait is async; the mutex is held across awaits in tests. If a downstream CRATE later needs the mock (today's 003 / 004 build their own platform-specific mocks; no shared-mock need), promote `#[cfg(test)]` to `#[cfg(any(test, feature = "mock"))]` and lift the type out of `mod tests` — a `cfg(test)` gate does not cross crate boundaries.

## Module: `shared::artifact::geometry`

### Constants

Per spec FR-020f. The producer / consumer shared FlatGeobuf naming + structure contract:

```rust
/// FlatGeobuf layer name written into the file's header by the producer's
/// writer; consumers may read for diagnostics. The "world_50m" indicates the
/// Natural Earth 1:50m source.
pub const GEOMETRY_LAYER_NAME: &str = "world_50m_admin_0";

/// Filename stem the producer uses; final filename is `{stem}-{sha8}.fgb`.
/// Consumers don't construct geometry filenames (they read
/// `manifest.geometry.relative_path`); this constant is for diagnostic logs
/// that want to identify "is this file the geometry shard?"
pub const GEOMETRY_FILENAME_STEM: &str = "world-50m";

/// FlatGeobuf feature column carrying the country's ISO 3166 alpha-3 code.
/// Producer's writer adds the column with this exact name; consumer's
/// `GeometryLayer::iter_features` reads features by this column to populate
/// `Feature.iso3`.
pub const FEATURE_COLUMN_ISO3: &str = "iso3";

/// FlatGeobuf feature column carrying the country's English name. Same
/// producer/consumer contract pattern as `FEATURE_COLUMN_ISO3`.
pub const FEATURE_COLUMN_NAME_EN: &str = "name_en";

/// File extensions in content-hashed filenames. Producer constructs filenames
/// with these; consumers may use them to type-dispatch on `relative_path`.
pub const SHARD_FILENAME_EXTENSION: &str = "sqlite";
pub const GEOMETRY_FILENAME_EXTENSION: &str = "fgb";
```

### `GeometryLayer`

Per spec FR-020a. Owns the geometry file bytes and opens a transient `flatgeobuf::FgbReader` per query — `FgbReader::select_all`/`select_bbox` take `self` by value and move the inner reader into the returned iterator, so a single `FgbReader` can't serve repeated queries. Re-opening only re-reads the small header; bbox queries still use the file's R-tree index.

```rust
pub struct GeometryLayer {
    bytes: Vec<u8>,
}

impl GeometryLayer {
    /// All features in the file, collected eagerly.
    pub fn iter_features(&self) -> Result<Vec<CountryFeature>, AppError>;

    /// Features whose bounding box intersects `bbox`, via the file's R-tree spatial index.
    pub fn features_intersecting_bbox(&self, bbox: BoundingBox) -> Result<Vec<CountryFeature>, AppError>;
}
```

Both query functions return `Result<Vec<CountryFeature>, AppError>` (eager collect), not `impl Iterator` — the upstream `FallibleStreamingIterator` borrows the consumed reader, making a borrowing-iterator return impractical, and eager collection over a few hundred countries is negligible. Properties via geozero `FeatureProperties::property::<String>`; geometry via geozero `ToGeo`.

### Functions

```rust
/// Parse the geometry bytes eagerly into a `GeometryLayer`. Failures wrap
/// with a descriptive `AppError`.
pub fn parse_geometry_layer(bytes: Vec<u8>) -> Result<GeometryLayer, AppError>;
```

Note: takes `bytes: Vec<u8>` (owned), not `&[u8]`, because `GeometryLayer` holds the bytes for its lifetime. `Bundle::open` reads the geometry bytes from the cache then passes them in by value. `parse_geometry_layer` opens a throwaway `FgbReader` once to validate the header eagerly (a corrupt file fails here, not on first query), then keeps the bytes.

### `CountryFeature`, `Polygon`, `BoundingBox`

```rust
#[derive(Debug, Clone)]
pub struct CountryFeature {
    pub iso3: String,           // ISO 3166 alpha-3 — matches `region.code` for country-level features
    pub name_en: String,        // The country's English name (joined from the canonical store by iso3 at write time)
    pub polygons: Vec<Polygon>, // A country may be multi-polygon (USA includes Alaska; Russia spans the antimeridian; etc.)
    pub bbox: BoundingBox,      // Pre-computed bounding box; what `features_intersecting_bbox` indexes on
}

#[derive(Debug, Clone)]
pub struct Polygon {
    /// Exterior ring.
    pub exterior: Vec<(f64, f64)>,
    /// Interior rings (holes); zero or more.
    pub interiors: Vec<Vec<(f64, f64)>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}
```

Conversions use the standard traits: `impl From<&geo_types::Polygon<f64>> for Polygon` and `impl TryFrom<&FgbFeature> for CountryFeature` (`Error = AppError`); `BoundingBox::from_polygons(&[Polygon]) -> Option<Self>` derives the extent from the already-converted polygons. A private `polygons_from_geometry` covers the `geo_types::Geometry → Vec<Polygon>` step, which can't be a trait impl (a `Vec` target trips the orphan rule).

## Module: `shared::artifact::bundle`

### Constants

The HTTP-serving metadata for the bundle's three file kinds (manifest JSON, geometry FlatGeobuf, SQLite shards). Producer sets them as upload metadata; the CDN edge / browser HTTP cache honor them. They live here (not in `manifest`) because they describe the artifact bundle's files as HTTP objects, not the manifest schema — `ManifestEntry` carries no content type.

```rust
pub const CONTENT_TYPE_MANIFEST: &str = "application/json";
pub const CONTENT_TYPE_FLATGEOBUF: &str = "application/octet-stream";
pub const CONTENT_TYPE_SQLITE: &str = "application/vnd.sqlite3";

/// Short-cached so re-platforms propagate within minutes.
pub const CACHE_CONTROL_MANIFEST: &str = "public, max-age=300";
/// Immutable: shard filenames are content-addressed, so a shard's bytes never change.
pub const CACHE_CONTROL_SHARD: &str = "public, max-age=31536000, immutable";
```

### `Bundle`

Per spec FR-018 + §Clarifications Q1+Q2 (no SQLite Connection; `Send + Sync`; eagerly-parsed GeometryLayer):

```rust
pub struct Bundle {
    pub manifest: Manifest,
    pub geometry: GeometryLayer,
    /// Authorized license shards' raw bytes; the renderer opens its own
    /// `rusqlite::Connection` against these on construction + hot-swap.
    /// Keyed by `(statistic_kind, license_shard_class)`.
    /// BTreeMap (not HashMap) for deterministic iteration order in tests.
    pub shard_bytes: BTreeMap<StatisticShardKey, Vec<u8>>,
    pub distribution_context: DistributionContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StatisticShardKey {
    pub statistic_kind: StatisticKind,
    pub license_shard_class: LicenseShardClass,
}
```

`Bundle: Send + Sync`. Every field is `Send + Sync`:
- `Manifest` derives serde; field types are all `Send + Sync`.
- `GeometryLayer` owns a `Vec<u8>` (the geometry file bytes); its query functions open transient readers and return owned data, so the value itself is plain `Send + Sync` data.
- `BTreeMap<K, Vec<u8>>` is `Send + Sync` trivially.
- `DistributionContext` is `Copy`.

`StatisticShardKey` moves from `ingestion/src/artifact/artifact_model.rs` to `shared/`; ingestion migrates its call sites to `shared::artifact::bundle::StatisticShardKey` directly (no re-export). The producer side already uses it in the manifest serializer.

### Functions

```rust
/// Open a complete artifact bundle for `version_label` by reading every file
/// through the supplied cache. Validates every shard's SHA-256, eagerly parses
/// the geometry into a GeometryLayer, loads byte buffers for every license
/// shard this distribution context is authorized to access (per
/// DistributionContext::authorized_classes); unauthorized shards are NOT loaded
/// into memory.
///
/// Returns `Err(AppError)` if:
/// - The manifest is missing from the cache (`cache.get(version_label, "manifest.json")` returns `Ok(None)`).
/// - The manifest fails `parse_manifest` (unknown schema_version, malformed sha256, path traversal, etc.).
/// - Any referenced shard is missing from the cache.
/// - Any referenced shard's SHA-256 doesn't match the manifest's recorded value.
/// - The geometry fails `parse_geometry_layer`.
impl Bundle {
    pub async fn open<C: ArtifactCache>(
        version_label: &str,
        cache: &C,
        distribution_context: DistributionContext,
    ) -> Result<Bundle, AppError>;
}
```

**As implemented**: `open` is generic over `C: ArtifactCache`, not `cache: &dyn ArtifactCache` as sketched above — stable async-fn-in-trait makes `ArtifactCache` not dyn-compatible (async methods return opaque futures), so `&dyn` won't compile. Static dispatch is the idiomatic fix and the loader always holds a concrete cache type.

## Hot-swap channel (no `bundle_watch` module)

The bundle hot-swap channel is `tokio::sync::watch::channel::<Arc<Bundle>>(...)`, created and used by consumers via `tokio::sync::watch` directly (the loader holds the `Sender`, the renderer the `Receiver`). 005 does NOT re-export the watch types — a thin third-party re-export module (`shared::artifact::bundle_watch`) was dropped; tokio is a direct dependency of every consumer (003 / 004 / 006). `shared` itself depends on tokio only as a dev-dependency (the test mock's `tokio::sync::Mutex`); its async `ArtifactCache` trait needs no runtime.

## Module: `shared::license::license`

### `DistributionContext`

Per spec FR-021 + the exact sketch in `docs/architecture/client.md` §Attaching license shards:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistributionContext {
    FirstParty,
    Embedded,
}

impl DistributionContext {
    pub fn authorized_classes(self) -> &'static [LicenseShardClass] {
        match self {
            DistributionContext::FirstParty => &[
                LicenseShardClass::Base,
                LicenseShardClass::NonCommercial,
                LicenseShardClass::ShareAlike,
            ],
            DistributionContext::ThirdParty => &[
                LicenseShardClass::Base,
            ],
        }
    }
}
```

Per FR-022: no wildcard arm in the `match`; adding a new `LicenseShardClass` variant requires a compile-error-driven update of every `DistributionContext` arm. Adding a new `DistributionContext` variant requires explicit slice authorship.

## Module: `shared::sqlite::vfs`

**DEFERRED to 006-core-renderer** (not implemented in 005). `sqlite-wasm-rs` 0.4.x is raw libsqlite3 C bindings with no `Connection` type, so the cross-target `Connection` typedef below cannot exist — the bridge must be a real wrapper (native `rusqlite::Connection::deserialize` behind the `serialize` feature; wasm32 raw `sqlite3_*` FFI) behind one facade, whose method surface is defined by the renderer's queries (006). `Bundle` opens no connection (carries bytes), so 005 doesn't need it; `shared/src/sqlite/mod.rs` declares only `pub mod schema;`. The sketch below is retained as 006's starting point.

Per spec FR-020 + plan.md §Topic 2 (revised 2026-06-22 after empirical wasm32 build failure). Wraps two underlying SQLite libraries — `rusqlite` on non-wasm32 targets, `sqlite-wasm-rs` on wasm32 — behind a unified `Connection` typedef and a single `open_connection_from_bytes` entry point.

### Target-agnostic `Connection` typedef

```rust
#[cfg(not(target_arch = "wasm32"))]
pub type Connection = rusqlite::Connection;

#[cfg(target_arch = "wasm32")]
pub type Connection = sqlite_wasm_rs::Connection;  // verify exact path against pinned sqlite-wasm-rs version
```

Renderer code in 006 imports `shared::sqlite::Connection` and writes queries that work on both libraries (read-only `SELECT` queries with positional `?1` / `?2` parameters; the renderer's surface is small enough that the rusqlite + sqlite-wasm-rs API divergence doesn't bite). Where any single method's name or signature does diverge, `shared::sqlite` exposes a thin facade function with one signature that the renderer calls.

### Functions

```rust
/// Open an in-memory SQLite connection seeded with the given bytes. Same
/// signature on both targets so consumers don't cfg-branch their open code.
///
/// Non-wasm32: uses rusqlite::Connection::deserialize.
/// wasm32: uses sqlite-wasm-rs's equivalent; the `name` parameter is the
///         logical database name used in subsequent ATTACH DATABASE calls.
pub fn open_connection_from_bytes(name: &str, bytes: Vec<u8>) -> Result<Connection, AppError>;
```

`name` is the SQLite logical database name (e.g. `"tfr-base"`); used in `ATTACH DATABASE` calls by the renderer.

`name` is the SQLite logical database name (e.g. `"tfr-base"`); used in `ATTACH DATABASE` calls by the renderer.

## Module: `shared::sqlite::schema`

Per spec FR-020b through FR-020e. The shared producer / consumer contract for the SQLite shard shape: every magic-number, every table / column / index name, the period date format, the DDL builder, and the consumer-side header-validator all live here. Producer (`ingestion/src/artifact/writer/sqlite.rs`) and consumer (006-core-renderer's renderer when it opens connections) both reach for these constants; the parallel-magic-value drift risk is eliminated.

### Constants

```rust
/// ASCII "EAFO"; written to SQLite's `application_id` PRAGMA (offset 60).
/// Lets `file(1)` and hex viewers identify Eafora shards by magic number
/// alone, independent of filename or context.
pub const APPLICATION_ID: i32 = 0x4541464F;

/// Schema version written to SQLite's `user_version` PRAGMA (offset 68).
/// Bump when the shard schema changes in a way consumers need to detect.
/// Same forward-compat motivation as `MANIFEST_SCHEMA_VERSION` for the manifest JSON.
pub const SCHEMA_VERSION: i32 = 1;

// Table names:
pub const TABLE_STATISTIC_VALUE: &str = "statistic_value";
pub const TABLE_SHARD_KEY: &str = "shard_key";

// Index names:
pub const INDEX_STATISTIC_VALUE_BY_REGION: &str = "statistic_value_by_region";

// statistic_value columns:
pub const COL_REGION_ISO3: &str = "region_iso3";
pub const COL_REGION_ID: &str = "region_id";
pub const COL_PERIOD_START: &str = "period_start";
pub const COL_PERIOD_END: &str = "period_end";
pub const COL_VALUE: &str = "value";
pub const COL_DATA_STATUS: &str = "data_status";
pub const COL_DATA_SOURCE_CODE: &str = "data_source_code";
pub const COL_DATA_SOURCE_REVISION: &str = "data_source_revision";

// shard_key columns:
pub const COL_STATISTIC_KIND: &str = "statistic_kind";
pub const COL_LICENSE_SHARD_CLASS: &str = "license_shard_class";

/// ISO 8601 date format used by `period_start` / `period_end` columns. Producer
/// formats periods with it (via `chrono::NaiveDate::format`); consumer's SQLite
/// queries assume it for string-comparison-friendly periods without date-function support.
pub const PERIOD_DATE_FORMAT: &str = "%Y-%m-%d";
```

### Functions

```rust
/// The full schema DDL composed from the constants above. Producer calls this
/// to create the schema; consumer never calls it directly but the column-name
/// constants it composes from are what consumer queries reference.
///
/// Implementation: either built via `const_format::concatcp!` at compile time
/// (returns `&'static str`) or returned as a runtime-built `String` joined from
/// the constants. Implementation-time choice; functionally identical.
pub fn shard_schema_ddl() -> &'static str;

/// Validate a connection's SQLite header. Returns `Ok(())` if the connection's
/// `application_id` and `user_version` PRAGMAs match `APPLICATION_ID` and
/// `SCHEMA_VERSION`. On `application_id` mismatch: `AppError` whose message
/// starts with `"sqlite shard: application_id mismatch"`. On `user_version`
/// mismatch: `AppError` whose message starts with `"sqlite shard: unknown schema_version"`.
///
/// 006-core-renderer's renderer calls this on every connection opened via
/// `shared::sqlite::vfs::open_connection_from_bytes` before issuing any query.
pub fn validate_shard_header(connection: &rusqlite::Connection) -> Result<(), AppError>;
```

## Public-API surface summary (re-exports from `shared::lib`)

```rust
// shared/src/lib.rs
pub mod error;
pub mod filesystem;
pub mod revision;
pub mod canonical;
pub mod artifact;
pub mod license;
pub mod sqlite;

pub use error::AppError;
pub use filesystem::*;
pub use revision::*;
pub use canonical::canonical_model::*;
pub use artifact::{manifest::*, bundle::*, cache::*, discovery::*, geometry::*};
pub use license::license::*;
pub use sqlite::{vfs::*, schema::*};
```

Wildcard re-exports per `feedback_wildcard_re_exports`. Consumers can `use shared::*` for the broadest reach or `use shared::artifact::{Bundle, Manifest}` for the specific reach.

## Validation rules from the spec

| Type / function           | Validation                                                                                | Source FR  |
|---------------------------|-------------------------------------------------------------------------------------------|------------|
| `parse_manifest`          | `manifest_schema_version == 1`; SHA-256 64 hex chars; no path-traversal `relative_path`. | FR-012     |
| `parse_discovery_document`| `schema_version == 1`.                                                                    | FR-015     |
| `verify_sha256`           | Computed hex matches expected hex (case-insensitive).                                    | FR-009     |
| `Bundle::open`            | Manifest present in cache; every shard present; every SHA-256 matches; geometry parses. | FR-019, §Edge Cases |
| `DistributionContext::authorized_classes` | Returns `&'static [LicenseShardClass]`; compile-error on new variant.       | FR-022     |
| `validate_shard_header`   | Connection's `application_id` == `APPLICATION_ID`; connection's `user_version` == `SCHEMA_VERSION`. Mismatch returns `AppError` with documented prefix. | FR-020c |
| `shard_schema_ddl`        | DDL composed from constants; executing it creates `statistic_value` + `shard_key` tables and `statistic_value_by_region` index with the names from the column-constants. | FR-020b |
| `MANIFEST_SCHEMA_VERSION` | Compile-time constant; `1`.                                                              | FR-010     |
| `DISCOVERY_SCHEMA_VERSION`| Compile-time constant; `1`.                                                              | FR-015     |
| `APPLICATION_ID`          | Compile-time constant; `0x4541464F` ("EAFO").                                            | FR-020b    |
| `SCHEMA_VERSION` (sqlite) | Compile-time constant; `1`.                                                              | FR-020b    |

## State transitions

`shared/`'s types are mostly value-types with no state transitions. The two stateful surfaces:

1. **`Bundle` hot-swap** (state lives outside `shared/` in the consuming renderer's `tokio::sync::watch::Sender<Arc<Bundle>>`):
   - Initial state: renderer holds `Arc<Bundle>` from the embedded bundle.
   - Transition: loader publishes `Sender::send(Arc<new_bundle>)`.
   - Reader sees: next `Receiver::borrow_and_update()` call returns the new bundle; in-flight queries holding the old `Arc` complete against the old bundle; old bundle's memory frees when last reference drops.

2. **`MockArtifactCache`** (test helper):
   - `new()` → empty cache.
   - `insert(version, path, bytes)` → adds an entry.
   - `get(version, path)` → returns `Some(bytes)` if inserted, `None` otherwise.
   - No eviction logic in the mock (production-grade adapters in 003 / 004 handle their own eviction).
