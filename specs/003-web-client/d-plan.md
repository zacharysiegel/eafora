# Phase D — live fetch, discovery, and bundle hot-swap

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After first paint from the downsampled embedded bundle, the web client discovers a live complete bundle, fetches and verifies it, writes it to OPFS, and hot-swaps the map onto that `Arc<Bundle>` so the year scrubber has every period.

**Architecture:** `ingestion publish` copies the just-published complete `manifest.json` to `latest/manifest.json` on the same destination. Local watch is a static site: `web/static/discovery` plus `ingestion publish local` into gitignored `web/static/repository/` (CDN key layout, at most two version directories). The wasm always fetches same-origin `/discovery`; it never requires `eafora.org` or `repository.eafora.org`. The driver paints from embedded (or a cached OPFS version), then a background task reconciles discovery with a speculative `latest/manifest.json` fetch and publishes the live bundle on the existing `watch` channel.

**Tech Stack:** Rust (`ingestion`, `shared`, `web`), Leptos / WASM, OPFS, `web_sys` `fetch`, `tokio::sync::{watch, Semaphore}`.

**Branch topology:** this plan lives on `d-plan`. Implementation is three stacked PRs: `publish-latest-pointer` off `d-plan`, `web-discovery-fetch` off that, `web-live-loader` off that. Rebase `--onto master` as each parent squash-merges.

Affected repositories: this monorepo only (`/Users/singularity/eafora`).

---

## Decisions locked

- First paint stays the downsampled tree at `/embedded_artifacts` (`DistributionContext::Embedded`). Live is the complete bundle (every year).
- The application must run locally with only this machine's data. No production, staging, or shared test server on the happy path.
- The page always fetches same-origin `/discovery`. On `eafora.org` that is the production discovery URL; on `cargo leptos watch` it is `web/static/discovery`. The wasm does not fetch `https://eafora.org/discovery`.
- Committed discovery has `repository_base_url: "/repository"`. Flipping that field to `https://repository.eafora.org` is a later deploy-time edit, not a prerequisite for D.
- Baked fallback is `include_str!` of the committed discovery file at compile time, not a script that hits the apex.
- Local live tree is more static files, not an object-storage emulator. `LocalArtifactRepository::put_file` is `create_dir_all` plus `fs::copy`.
- `ingestion publish` (local and `cloudflare-r2`) writes `latest/manifest.json` after the versioned manifest. That step was specified and never implemented.
- `publish local` keeps the two newest version directories under `--root` plus `latest/manifest.json`. R2 is not pruned.
- Complete and downsampled share a `version_label`. Live `cache.put` overwrites that version's `manifest.json` in OPFS. Leftover downsampled shard files sit until the version is evicted. No second cache namespace.
- Returning visit: if OPFS already has a version, first paint opens the newest one (`DistributionContext::FirstParty`) and skips the embedded HTTP fetch. Discovery and `latest` still run in the background.
- Discovery and `latest/manifest.json` fetch with `cache: "reload"`. Content-hashed shard and geometry URLs keep the default HTTP cache.
- Concurrent live file fetches are capped at 6 in the web loader. `shared` has no loader semaphore.
- Periodic refetch on focus / visibility is still unspecified in `docs/architecture/client.md`. Out of D.
- Wrangler, production `_headers`, and R2 CORS are Phase E (or the deploy that flips `repository_base_url`). Not D.
- Do not add `gloo-net`. Keep the existing `web_sys` fetch.

---

## File map

- Create: `web/static/discovery`
- Create: `web/src/live_resolve.rs` (always compiled; host-testable reconcile + baked document)
- Modify: `ingestion/src/artifact/publish.rs` (latest pointer; dispatch local retain)
- Modify: `ingestion/src/artifact/repository/local_artifact_repository.rs` (retain two newest version directories)
- Modify: `ingestion/src/artifact/repository/artifact_repository.rs` (forward retain if the trait needs it; prefer an inherent function on `LocalArtifactRepository` called from `publish_artifacts` when the destination is `Local`)
- Modify: `ingestion/tests/publish_integration.rs`
- Modify: `docs/architecture/client.md` (remove the "to be implemented" hedge on the latest pointer)
- Modify: `shared/src/http.rs` (`HttpCacheMode` on `HttpRequest`)
- Modify: `web/src/client/fetch.rs` (apply cache mode; `fetch_discovery` / `fetch_manifest` / `fetch_artifact_file`)
- Modify: `web/src/client/load.rs` (open cached / embedded / live)
- Modify: `web/src/client/mod.rs`
- Modify: `web/src/lib.rs` (`pub mod live_resolve`)
- Modify: `web/src/map/canvas/driver.rs` (first-paint choice; spawn live upgrade; republish chrome on swap)
- Modify: `web/Cargo.toml` (`RequestCache` feature)
- Modify: `web/.gitignore` (`/static/repository/`)
- Modify: `web/locales/en.json` (live-load failure banner)
- Modify: `specs/003-web-client/quickstart.md` (publish local into `web/static/repository/`)
- Modify: `docs/task-order.md` (Phase D in progress, then landed on the last PR)

Do not create empty `_api.rs` / `_client.rs` stubs. Do not add a `web/src/loader.rs`; the loader already lives at `web/src/client/load.rs`.

---

## D1 — publish `latest/manifest.json` and local keep-two

Branch: `publish-latest-pointer` off `d-plan`.

After a successful versioned upload (shards, geometry, `{version}/manifest.json`) and the `artifact_version` insert, copy that same manifest file to `shared::artifact::manifest::MANIFEST_LATEST_KEY` (`latest/manifest.json`) on the same destination. Then, only for `ArtifactRepositoryKind::Local`, delete version directories under `--root` until two remain.

`latest/` is not a version directory. Skip the directory named `artifact::LATEST_POINTER` when listing. Sort the remaining directory names lexicographically (`YYYY-MM-DD+<surname>` sorts correctly; tests may use dated labels). Keep the two highest. `remove_dir_all` the rest.

R2 and dry destinations do not prune. Dry still receives a `put_file` for the latest key (no-op write).

### Task 1: create the branch

- [ ] **Step 1:** From a clean `d-plan` head: `./scripts/branch-init.sh publish-latest-pointer`

### Task 2: failing tests for the latest pointer and local retain

**Files:**
- Modify: `ingestion/tests/publish_integration.rs`

- [ ] **Step 1:** In `publish_artifacts_uploads_every_file_to_local_repository_and_inserts_artifact_version`, after the versioned-file asserts, also assert:

```rust
let latest_destination: PathBuf = destination_dir.path().join(manifest::MANIFEST_LATEST_KEY);
assert!(latest_destination.exists());
assert_eq!(fs::read(&latest_destination).unwrap(), fs::read(&manifest_destination).unwrap());
```

- [ ] **Step 2:** Add `publish_local_keeps_only_the_two_newest_version_directories`. Publish three synthetic bundles with labels `2026-06-01+keep`, `2026-06-10+keep`, `2026-06-22+keep` into one destination (three `BuildReport`s, three `artifact_version` rows). After the third publish:

  - `latest/manifest.json` byte-equals the third version's manifest
  - `2026-06-01+keep/` is gone
  - `2026-06-10+keep/` and `2026-06-22+keep/` remain
  - delete all three `artifact_version` rows in teardown

- [ ] **Step 3:** Run `cargo test -p ingestion --test publish_integration`. Expected: the new asserts fail because `latest/manifest.json` is not written.

### Task 3: implement latest put + local retain

**Files:**
- Modify: `ingestion/src/artifact/publish.rs`
- Modify: `ingestion/src/artifact/repository/local_artifact_repository.rs`

- [ ] **Step 1:** After the versioned manifest `put_file` and before or after `insert_artifact_version` (after the versioned manifest is on the destination; either side of the insert is fine as long as a failed insert does not leave a latest pointer at a version that has no row — prefer insert first, then latest put, then retain). Call:

```rust
repository.put_file(manifest::MANIFEST_LATEST_KEY, &build_report.artifacts.manifest.path, bundle::CONTENT_TYPE_MANIFEST).await?;
```

- [ ] **Step 2:** Add on `LocalArtifactRepository`:

```rust
const LOCAL_VERSIONS_KEPT: usize = 2;

impl LocalArtifactRepository {
    pub async fn retain_newest_versions(&self) -> Result<(), AppError> {
        // read_dir(self.root); skip non-dirs and the dir named artifact::LATEST_POINTER;
        // sort names; if len > LOCAL_VERSIONS_KEPT, remove_dir_all the oldest prefix.
    }
}
```

Call it from `publish_artifacts` only when the destination is `ArtifactRepositoryKind::Local`.

- [ ] **Step 3:** Re-run `cargo test -p ingestion --test publish_integration`. Expected: PASS.

- [ ] **Step 4:** Update `docs/architecture/client.md` §Live bundle: delete the sentence that says the latest-pointer upload is a future follow-up. State that `publish` writes `latest/manifest.json` as the last object (after the versioned manifest).

- [ ] **Step 5:** Commit explicit paths. Subject: `publish the complete manifest to latest/manifest.json`.

### PR description (D1)

**ingestion** — After each successful publish, copy the versioned complete `manifest.json` to `latest/manifest.json` on the same destination. `publish local` also drops version directories older than the two newest so a static local repository does not grow without bound.

---

## D2 — same-origin discovery and fetch cache

Branch: `web-discovery-fetch` off `publish-latest-pointer`.

No live loader yet. This slice makes the documents and the fetch surface exist and be testable.

### Task 4: create the branch

- [ ] **Step 1:** `./scripts/branch-init.sh web-discovery-fetch`

### Task 5: committed discovery + gitignore

**Files:**
- Create: `web/static/discovery`
- Modify: `web/.gitignore`

- [ ] **Step 1:** Write `web/static/discovery` (no `.json` suffix):

```json
{
  "schema_version": 1,
  "repository_base_url": "/repository",
  "minimum_client_version": "0.1.0",
  "sunset": null
}
```

- [ ] **Step 2:** Append to `web/.gitignore`:

```
/static/repository/
```

- [ ] **Step 3:** Commit. Subject: `commit same-origin discovery pointing at /repository`.

### Task 6: `HttpCacheMode` and fetch helpers

**Files:**
- Modify: `shared/src/http.rs`
- Modify: `web/src/client/fetch.rs`
- Modify: `web/src/client/load.rs` (update `HttpRequest { ... }` literals; write every field)
- Modify: `web/Cargo.toml`
- Create: `web/src/live_resolve.rs`
- Modify: `web/src/lib.rs`

- [ ] **Step 1:** Extend `HttpRequest` (no struct-update syntax at call sites):

```rust
pub enum HttpCacheMode {
    Default,
    Reload,
}

pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub cache: HttpCacheMode,
}
```

- [ ] **Step 2:** In `web/src/client/fetch.rs`, when `cache` is `Reload`, `init.set_cache(web_sys::RequestCache::Reload)`. Add the `RequestCache` feature on `web-sys`.

- [ ] **Step 3:** Add:

```rust
pub async fn fetch_discovery(discovery_url: &str) -> Result<Vec<u8>, AppError> {
    fetch_bytes(&HttpRequest {
        method: HttpMethod::Get,
        url: discovery_url.to_string(),
        cache: HttpCacheMode::Reload,
    }).await
}

pub async fn fetch_manifest(repository_base_url: &str) -> Result<Vec<u8>, AppError> {
    let base: &str = repository_base_url.trim_end_matches('/');
    let url: String = format!("{base}/{}", manifest::MANIFEST_LATEST_KEY);
    fetch_bytes(&HttpRequest {
        method: HttpMethod::Get,
        url,
        cache: HttpCacheMode::Reload,
    }).await
}

pub async fn fetch_artifact_file(repository_base_url: &str, version_label: &str, relative_path: &str) -> Result<Vec<u8>, AppError> {
    let base: &str = repository_base_url.trim_end_matches('/');
    let url: String = format!("{base}/{version_label}/{relative_path}");
    fetch_bytes(&HttpRequest {
        method: HttpMethod::Get,
        url,
        cache: HttpCacheMode::Default,
    }).await
}
```

Non-2xx stays `fetch: {url} returned HTTP {status}`.

- [ ] **Step 4:** Add always-compiled `web/src/live_resolve.rs`:

```rust
use shared::artifact::{DiscoveryDocument, parse_discovery_document};
use shared::AppError;

pub const DISCOVERY_PATH: &str = "/discovery";
pub const BAKED_DISCOVERY_JSON: &str = include_str!("../static/discovery");

pub fn baked_discovery_document() -> Result<DiscoveryDocument, AppError> {
    parse_discovery_document(BAKED_DISCOVERY_JSON.as_bytes())
}

pub fn baked_repository_base_url() -> Result<String, AppError> {
    Ok(baked_discovery_document()?.repository_base_url)
}

/// Picks the repository base after the parallel discovery fetch.
/// Speculative errors stay silent until this returns.
pub enum AuthoritativeBase {
    Baked,
    Discovered(String),
}

pub fn authoritative_repository_base(
    baked_base: &str,
    discovery: Result<DiscoveryDocument, AppError>,
) -> AuthoritativeBase {
    match discovery {
        Err(_) => AuthoritativeBase::Baked,
        Ok(document) if document.repository_base_url == baked_base => AuthoritativeBase::Baked,
        Ok(document) => AuthoritativeBase::Discovered(document.repository_base_url),
    }
}
```

Host tests in the same file (`#[cfg(test)]`):

- `baked_discovery_document_parses_committed_file`
- `authoritative_repository_base_uses_baked_when_discovery_fails`
- `authoritative_repository_base_uses_baked_when_discovery_matches`
- `authoritative_repository_base_uses_discovered_when_base_differs`

- [ ] **Step 5:** `cargo test -p web --features ssr live_resolve`. Expected: PASS. `cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown`. Expected: PASS.

- [ ] **Step 6:** Commit. Subject: `fetch discovery and latest/manifest.json with cache reload`.

### PR description (D2)

**web** — Commit a same-origin `/discovery` document whose `repository_base_url` is `/repository`, and teach the browser fetch adapter to bypass the HTTP cache on that document and on `latest/manifest.json`.

---

## D3 — loader, hot-swap, returning visit

Branch: `web-live-loader` off `web-discovery-fetch`.

### Task 7: create the branch

- [ ] **Step 1:** `./scripts/branch-init.sh web-live-loader`

### Task 8: load a bundle from cache or from a repository base

**Files:**
- Modify: `web/src/client/load.rs`

- [ ] **Step 1:** Keep `load_embedded_bundle` for first-visit HTTP of `/embedded_artifacts` (add `HttpCacheMode::Default` on those requests; the embedded tree is content-hashed except `manifest.json` — use `Reload` on the embedded manifest too so a resynced downsampled tree is not stuck behind a cached `manifest.json`).

- [ ] **Step 2:** Add:

```rust
const LIVE_FETCH_PARALLELISM: usize = 6;

pub async fn open_newest_cached_bundle(cache: &OpfsArtifactCache) -> Result<Option<Bundle>, AppError> {
    let mut version_labels: Vec<String> = cache.list_versions().await?;
    version_labels.sort();
    let Some(version_label) = version_labels.last() else {
        return Ok(None);
    };
    let bundle: Bundle = Bundle::open(cache, version_label, DistributionContext::FirstParty).await?;
    Ok(Some(bundle))
}

pub async fn load_live_bundle(
    cache: &OpfsArtifactCache,
    repository_base_url: &str,
) -> Result<Bundle, AppError> {
    // fetch_manifest
    // parse_manifest
    // for each file_entries(), fetch_artifact_file with a Semaphore(LIVE_FETCH_PARALLELISM)
    // verify_sha256, cache.put
    // cache.put the manifest
    // Bundle::open(..., DistributionContext::FirstParty)
}
```

Extract the per-file fetch+verify+put so the semaphore wraps one file, not the whole bundle.

- [ ] **Step 3:** Add `load_live_after_discovery(cache, baked_base) -> Result<Bundle, AppError>` that:

  1. Starts `fetch_discovery(DISCOVERY_PATH)` and `fetch_manifest(baked_base)` together (`futures` join, or spawn two tasks and join). The web crate already has tokio sync; use `futures_util::future::join` if that crate is already in the graph, otherwise two sequential awaits only if join is not already a dependency. Prefer join. Check `Cargo.lock` before adding a crate; `wasm_bindgen_futures` plus a local `join` helper is enough if no join crate is present:

```rust
async fn join2<A, B>(left: A, right: B) -> (A::Output, B::Output)
where
    A: Future,
    B: Future,
{
    tokio::join!(left, right)
}
```

  `tokio::join!` needs the `macros` feature. Do not add it only for this. Write a two-future poll helper in `live_resolve.rs` or await discovery first then manifest if join is heavy. Preferred: `wasm_bindgen_futures::JsFuture` is already sequential-friendly; fire both by starting both Promises in `fetch` (they start on call) and awaiting both results. Call both `fetch_*` functions so their `window.fetch` runs before either body is awaited: start discovery, start speculative, then await both.

  2. `authoritative_repository_base(baked_base, parsed_discovery)`.
  3. If `Baked`, parse the speculative manifest bytes (surface that error now). If speculative failed, return that error.
  4. If `Discovered(other)`, ignore speculative errors, `fetch_manifest(&other)` and use that base for files.
  5. Fetch remaining files against the chosen base, verify, put, open.

- [ ] **Step 4:** `cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown`.

### Task 9: driver first paint, spawn live upgrade, republish chrome

**Files:**
- Modify: `web/src/map/canvas/driver.rs`
- Modify: `web/locales/en.json`
- Modify: map chrome that can show a non-fatal banner (prefer a small existing panel region or a new signal on `MapView`; do not invent a second source-of-truth)

- [ ] **Step 1:** Replace the unconditional `load_embedded_bundle` with:

```rust
let bundle: Bundle = match load::open_newest_cached_bundle(&cache).await {
    Ok(Some(cached)) => cached,
    Ok(None) => load::load_embedded_bundle(&cache).await.map_err(StartupError::DataUnavailable)?,
    Err(error) => {
        log::warn!("opening a cached bundle failed, falling back to embedded [error={error}]");
        load::load_embedded_bundle(&cache).await.map_err(StartupError::DataUnavailable)?
    }
};
```

Then `evict_old_versions` as today.

- [ ] **Step 2:** After the driver is in the `DRIVER` slot and the initial chrome signals are set, spawn a local task that:

  1. Reads `baked_repository_base_url()`.
  2. Calls `load_live_after_discovery`.
  3. On success: `bundle_sender.send(Arc::new(bundle))` (the sender is cloneable; clone it into the task before moving the driver). Then `DRIVER.with_borrow_mut` to clamp `active_period_start` to the new shard's range if the current year is absent, republish `ViewControls` / `LegendView` / `GlobalView` / selection view, `request_redraw`.
  4. On failure: log, set a `live_load_failed` signal so the shell shows the banner. The painted bundle stays.

- [ ] **Step 3:** Period clamp: if `read_active_shard().and_then(|shard| shard.period_range())` is `Some((earliest, latest))` and `active_period_start` is outside that inclusive range, set it to `latest`.

- [ ] **Step 4:** Banner copy via i18n, e.g. key `live.load_failed` — short, no advocacy. The map stays up.

- [ ] **Step 5:** `cargo check` hydrate wasm32 and ssr.

### Task 10: docs

**Files:**
- Modify: `specs/003-web-client/quickstart.md`
- Modify: `docs/task-order.md`

- [ ] **Step 1:** Quickstart: after the embedded sync, document:

```sh
cargo run -p ingestion -- publish local --build --root ./web/static/repository --public-base-url /repository
```

(`--build` / `--root` / `--public-base-url` must match the real clap flags in `ingestion/src/main.rs`; look them up, do not guess.) Gitignored destination. Restart or refresh `cargo leptos watch` so `/repository/latest/manifest.json` exists. First paint is still embedded (or OPFS); the scrubber gains years after the live swap.

- [ ] **Step 2:** Mark Phase D landed on this last PR (delete the pending line's "pending", or fold it as landed like C3).

- [ ] **Step 3:** Commit. Subject: `hot-swap the map to the complete live bundle`.

### PR description (D3)

**web** — After first paint, fetch same-origin discovery and `latest/manifest.json`, verify the complete bundle into OPFS, and publish it on the renderer's watch channel. A returning visit whose OPFS already has a version paints from that cache first.

---

## Local verification (not CI)

After D3, on this machine:

1. `./scripts/sync-embedded-bundle.sh ./web/static/embedded_artifacts/`
2. `cargo run -p ingestion -- publish local --build --root ./web/static/repository --public-base-url /repository` (confirm flags in source). A second publish of the same `version_label` is rejected by the existing `artifact_version` uniqueness check; rebuild with a new label, or point `--root` at a fresh tree after deleting the `artifact_version` row, if you need to overwrite the local static files.
3. `cd web && cargo leptos watch`
4. Open the printed URL. Confirm first paint. Confirm the year scrubber expands after the live load. Confirm a reload with OPFS populated does not wait on `/embedded_artifacts` for first paint (Network panel).
5. Confirm right-click / chrome inset / Global empty state still behave as on master.

Browser verification is required before calling D3 done.

---

## Out of scope

- Phase E: wrangler, production `_headers`, perf-budget, precompress.
- Changing committed `repository_base_url` to `https://repository.eafora.org`.
- R2 CORS and a real `publish cloudflare-r2` from this machine (D1 makes that publish write `latest/manifest.json` when someone runs it).
- Periodic live refetch on focus / visibility.
- Inertial pan, deep links, share images, high-value choropleth color.

---

## Brief PR descriptions (stack)

**d-plan** — Plan Phase D: live complete-bundle fetch, same-origin discovery, `latest/manifest.json` publish, and hot-swap.

D1 / D2 / D3 descriptions are under each phase above.
