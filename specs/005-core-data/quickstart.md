# Quickstart: consuming `shared/` from a downstream crate

> Phase 1 output of `/speckit-plan` for 005-core-data. For a future contributor implementing 003 (web client), 004 (iOS client), 006 (renderer), or any other crate that depends on `shared/`. Read after `data-model.md` and `contracts/core-public-api.md`.

## Add `shared` as a dependency

```toml
# In your crate's Cargo.toml:
[dependencies]
shared = { workspace = true }
```

The root workspace `Cargo.toml` registers `shared = { path = "shared" }` under `[workspace.dependencies]`.

(The in-crate `MockArtifactCache` is `#[cfg(test)]`-only — accessible only inside `shared/`'s own tests. If your crate needs a mock cache for its tests, build a small platform-appropriate mock locally; promoting `shared`'s mock to a `mock` cargo feature is a one-character change if a real shared-mock need ever surfaces.)

## Build for both targets

```sh
cargo build -p <your-crate>                                       # host (default)
cargo build -p <your-crate> --target wasm32-unknown-unknown       # web
```

If your crate doesn't compile for wasm32 (e.g. it pulls `sqlx` or `reqwest::blocking`), `shared/` still works fine independently — only the consuming crate fails the wasm32 build, not `shared/`.

## Construct an `ArtifactCache` for production

Production cache adapters live in the platform crates, not in `shared/`:

- **Web (003-web-client)**: `web::cache::OpfsArtifactCache` against `<opfs-root>/artifacts/...` (per `client-web.md` §OPFS cache adapter).
- **iOS (004-ios-client)**: a Swift `FileSystemArtifactCache.swift` against `<app-sandbox>/Library/Caches/artifacts/...`, wrapped via UniFFI as a Rust trait impl (per `client-ios.md` §Cache: Library/Caches/).

For tests, define a small `MockArtifactCache` inside your own crate's test module. The trait surface is small (4 async methods) so a `BTreeMap`-backed mock is ~30 lines:

```rust
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use shared::artifact::cache::ArtifactCache;
use shared::artifact::{Bundle, MANIFEST_FILENAME};
use shared::license::DistributionContext;
use shared::AppError;

#[cfg(test)]
struct MockArtifactCache {
    entries: Mutex<BTreeMap<(String, String), Vec<u8>>>,
}

#[cfg(test)]
impl MockArtifactCache {
    fn new() -> Self {
        MockArtifactCache { entries: Mutex::new(BTreeMap::new()) }
    }
    async fn insert(&self, version_label: &str, file_relative_path: &str, bytes: Vec<u8>) {
        self.entries.lock().await.insert(
            (version_label.to_string(), file_relative_path.to_string()),
            bytes,
        );
    }
}

#[cfg(test)]
impl ArtifactCache for MockArtifactCache {
    async fn put(&self, version_label: &str, file_relative_path: &str, bytes: &[u8]) -> Result<(), AppError> {
        self.entries.lock().await.insert(
            (version_label.to_string(), file_relative_path.to_string()),
            bytes.to_vec(),
        );
        Ok(())
    }
    async fn get(&self, version_label: &str, file_relative_path: &str) -> Result<Option<Vec<u8>>, AppError> {
        Ok(self.entries.lock().await.get(&(version_label.to_string(), file_relative_path.to_string())).cloned())
    }
    async fn list_versions(&self) -> Result<Vec<String>, AppError> {
        let entries = self.entries.lock().await;
        let mut versions: Vec<String> = entries.keys().map(|(v, _)| v.clone()).collect();
        versions.sort();
        versions.dedup();
        Ok(versions)
    }
    async fn delete_version(&self, version_label: &str) -> Result<(), AppError> {
        self.entries.lock().await.retain(|(v, _), _| v != version_label);
        Ok(())
    }
}

#[tokio::test]
async fn my_consumer_test() {
    let cache: MockArtifactCache = MockArtifactCache::new();

    // Populate the cache with a manifest + shard bytes for the version under test.
    let manifest_bytes: Vec<u8> = include_bytes!("../samples/2026-06-22+ada/manifest.json").to_vec();
    cache.insert("2026-06-22+ada", MANIFEST_FILENAME, manifest_bytes).await;

    let geometry_bytes: Vec<u8> = include_bytes!("../samples/2026-06-22+ada/geometry/world-50m-<sha>.fgb").to_vec();
    cache.insert("2026-06-22+ada", "geometry/world-50m-<sha>.fgb", geometry_bytes).await;

    let tfr_base_bytes: Vec<u8> = include_bytes!("../samples/2026-06-22+ada/data/tfr-base-<sha>.sqlite").to_vec();
    cache.insert("2026-06-22+ada", "data/tfr-base-<sha>.sqlite", tfr_base_bytes).await;

    let bundle: Bundle = Bundle::open(
        "2026-06-22+ada",
        &cache,
        DistributionContext::FirstParty,
    ).await.expect("bundle opens against the mock");

    // Assert against the parsed bundle:
    assert_eq!(bundle.manifest.manifest_schema_version, 1);
    assert_eq!(bundle.manifest.version, "2026-06-22+ada");
    assert!(!bundle.shard_bytes.is_empty());
}
```

If multiple test modules in your crate need the mock, hoist it to `tests/helpers/mock_cache.rs` per the project's `feedback_no_per_source_test_helper_modules` rule — but in practice one place suffices.

## Wire the bundle hot-swap channel

The loader publishes; the renderer subscribes. Per spec FR-023 + `client.md` §Bundle hot-swap:

```rust
use std::sync::Arc;
use shared::artifact::Bundle;
use tokio::sync::watch;

// Loader-side: construct the channel with the initial (embedded) bundle.
let initial_bundle: Arc<Bundle> = Arc::new(/* Bundle::open(embedded_version, ...) */);
let (sender, receiver) = watch::channel::<Arc<Bundle>>(initial_bundle);

// Hand the receiver to the renderer (006-core-renderer's Renderer::new takes it).
// Hand the sender to the loader task.

// Renderer-side (in 006's draw loop):
let current: Arc<Bundle> = receiver.borrow_and_update().clone();
// ... draw against current ...

// Loader-side (when the live bundle finishes loading):
let new_bundle: Arc<Bundle> = Arc::new(Bundle::open(new_version_label, &cache, ctx).await?);
sender.send(new_bundle)?;
// The renderer's next borrow_and_update picks up the new bundle; in-flight
// queries holding the old Arc complete against the old bundle; old bundle's
// memory frees when last reference drops.
```

The hot-swap channel is `tokio::sync::watch`; the loader holds the `Sender`, the renderer the `Receiver`. 005 does not re-export these — consumers use `tokio::sync::watch` directly (it's a direct dependency of every client).

## Parse a discovery document

Per spec FR-014 + the iOS / web startup flow in `client.md` §Speculative parallel fetch at startup:

```rust
use shared::artifact::discovery::{DiscoveryDocument, parse_discovery_document, DISCOVERY_SCHEMA_VERSION};

// Bytes from `fetch("https://eafora.org/discovery")` (web) or `URLSession.shared.data(from: discovery_url)` (iOS).
let bytes: Vec<u8> = /* ... */;
let discovery: DiscoveryDocument = parse_discovery_document(&bytes)
    .expect("discovery document parses");

// Use the resolved repository base URL for shard fetches:
let repository_base_url: &str = &discovery.repository_base_url;

// Check sunset (optional; v1 doesn't act on it).
if let Some(sunset_timestamp) = &discovery.sunset {
    log::warn!("contract sunset announced; [date={}]", sunset_timestamp);
}
```

If `schema_version != 1`, `parse_discovery_document` returns an `AppError` and the caller falls back to the static `repository_base_url` constant.

## License-shard authorization

When you need to know which license shards apply to your distribution context:

```rust
use shared::license::DistributionContext;
use shared::canonical::canonical_model::LicenseShardClass;

let context: DistributionContext = DistributionContext::FirstParty;
let authorized: &'static [LicenseShardClass] = context.authorized_classes();
// authorized == &[LicenseShardClass::Base, LicenseShardClass::NonCommercial, LicenseShardClass::ShareAlike]

// Or for an embedded third-party widget:
let embedded_context: DistributionContext = DistributionContext::Embedded;
let embedded_authorized: &'static [LicenseShardClass] = embedded_context.authorized_classes();
// embedded_authorized == &[LicenseShardClass::Base]
```

`Bundle::open` consults this when filtering which license shards to load into memory; you typically don't need to call `authorized_classes` directly unless you're inspecting authorization outside the bundle-open flow (e.g. for UI that says "this widget is restricted to base-license data").

## Verify SHA-256

When you've fetched bytes (from anywhere) and want to verify them against a recorded hash:

```rust
use shared::filesystem::verify_sha256;

let bytes: Vec<u8> = /* ... */;
let expected_hex: &str = "ddd660b71c1a36c881f8504889efe39845e04fb2b20ca10340a48c9c7dace87f";

verify_sha256(&bytes, expected_hex).expect("hash matches");
```

On mismatch, the `AppError` message contains both `expected_hex` (first 8 hex chars) and the actual hash (first 8 hex chars) so the error log identifies what failed.

## Open a SQLite connection against bundle shard bytes (from 006-core-renderer)

When 006's renderer needs to query a statistic shard:

```rust
use shared::canonical::canonical_model::{StatisticKind, LicenseShardClass};
use shared::artifact::bundle::{Bundle, StatisticShardKey};
use shared::sqlite::vfs::open_connection_from_bytes;

let bundle: &Bundle = /* from the watch channel */;
let key: StatisticShardKey = StatisticShardKey {
    statistic_kind: StatisticKind::Tfr,
    license_shard_class: LicenseShardClass::Base,
};
let shard_bytes: Vec<u8> = bundle.shard_bytes.get(&key)
    .expect("shard authorized by distribution context")
    .clone();   // clone because the connection takes ownership

let connection: rusqlite::Connection = open_connection_from_bytes("tfr_base", shard_bytes)
    .expect("connection opens against the bytes");

// Query the shard:
let mut stmt: rusqlite::Statement = connection.prepare(
    "SELECT value FROM statistic_value WHERE region_iso3 = ?1 AND period_start = ?2"
).expect("prepare succeeds");
// ... etc.
```

The `open_connection_from_bytes` function has the same signature on both targets; the wasm32 implementation goes through `shared::sqlite::vfs::register_vec_u8_vfs` machinery internally, while the non-wasm32 implementation goes through rusqlite's `Connection::deserialize` API. Consumers don't see the difference.

## Common pitfalls

1. **Don't pass `shared::artifact::cache::ArtifactCache` by trait object across an FFI boundary.** UniFFI doesn't support trait objects. iOS's `FileSystemArtifactCache.swift` wraps the trait at the Swift layer; the Rust UniFFI surface takes a concrete type.
2. **Don't construct `Bundle` directly — always go through `Bundle::open`.** The fields are public for read access only (the watch channel exposes `Receiver::borrow` which gives `&Bundle`); construction requires the validation steps `Bundle::open` performs (SHA-256, license filtering, geometry parsing).
3. **Don't expect `Bundle::open` to fetch.** It only reads through the cache. The platform shell is responsible for ensuring the cache has the bytes the requested `version_label` references (via the speculative parallel fetch at startup in `client.md` §Speculative parallel fetch).
4. **Don't downstream-define your own `AppError` type if you can reach for `shared::AppError`.** The `From` impls in `shared::error` cover most parser failure modes; only add new conversions in your crate's own error layer.
5. **Don't store `&Bundle` across an `.await`.** The `watch::Receiver::borrow_and_update` guard holds a read lock; await-while-borrowed risks the reader-writer-deadlock path (in single-threaded WASM it just hangs; in multi-threaded tokio it deadlocks). Pattern: clone the `Arc<Bundle>` out of the borrow guard, drop the guard, then await against the clone.
