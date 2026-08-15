# Client architecture

> **Status: draft, 2026-06-14.** This document is the cross-cutting consumer-side companion to `docs/architecture/ingestion.md`. Ingestion ends with a content-addressed bundle on Cloudflare R2 (`manifest.json` + per-statistic SQLite shards + a FlatGeobuf geometry file); this document defines what every client (web, iOS, Android) does with that bundle to render a fertility-data atlas. Per-platform deltas (build system, hot reload, threading model, FFI surface) live in `client-web.md`, `client-ios.md`, `client-android.md`, which are subsequent branches.

## Scope of this document

This document covers everything between **a published artifact bundle on the CDN** and **a rendered map with fertility data overlaid**:

- The artifact-consumption contract: how clients discover and validate a manifest, and how the embedded vs. live bundle relationship works.
- The fetch / cache / load pipeline: HTTP → on-device persistent cache → in-memory data structures.
- SQLite-in-the-client: which engine, how the database is opened, how queries run.
- FlatGeobuf reading: which reader, how features feed the renderer.
- License-shard composition: how the client picks which shards to attach for its distribution context.
- Embedded downsampled artifact: the "good enough for first paint and offline use" bundle. Embedded in native client binaries; shipped as a static asset alongside the wasm on web. Mechanism differs by platform; the bundle itself is the same.
- Cross-platform consistency: which decisions every client makes the same, which it doesn't.

Map rendering details (projection, hit testing, zoom-to-country) are covered in `docs/architecture/overview.md`. Per-platform UI (Leptos components, SwiftUI views, Compose composables) is covered in the per-platform docs.

## Locked decisions referenced (not relitigated)

From the constitution and `docs/architecture/overview.md`:

- v1–v2 data path is CDN-hosted versioned artifacts; clients never call origin. (Constitution VI; overview §Architecture diagram)
- Each client embeds the same Rust core via wasm-bindgen (web) or UniFFI (iOS, Android). The core owns artifact parsing, geometry, statistic queries, projection, hit testing, and camera state. (Overview §Rust core)
- Polygons are full-resolution; no LOD pyramid. ~200 country polygons (~5 MB compressed FlatGeobuf) through v1; subnational geometry joins the same FlatGeobuf as additional features in v2+. (Overview §Polygon representation)
- Web is single-threaded WASM; **no** `SharedArrayBuffer`; no cross-origin isolation headers. (Overview §Web client)
- License-segmented SQLite shards compose **additively** via `ATTACH DATABASE`. v1 ships only one license class (`base`); the mechanism is in place from day one. (Overview §License-segmented SQLite shards)
- Manifest format and shard naming convention are defined by the producer-side spec; see `docs/architecture/ingestion.md` and `ingestion/src/artifact/writer/manifest.rs` for the canonical schema.
- Plain CSS for the web client; no Tailwind or utility-class frameworks. (Project memory)
- Web and iOS are developed **in parallel** from v1, deliberately, to prevent the architecture from overfitting to the web platform's constraints. Android lags but is not foreclosed. The native apps double as personal-learning goals for the parallel game project; for funder pitches, only the web is the user-facing v1 deliverable. (Project memory)

## The artifact-consumption contract

The producer publishes one **artifact version** at a time. A version is a directory under `<repository_base_url>/<version_label>/` containing exactly one `manifest.json`, one FlatGeobuf geometry file under `geometry/`, and N SQLite shard files under `data/`. Every file other than `manifest.json` is content-addressed: its filename ends with the file's full SHA-256 in hex (e.g. `world-50m-ddd660b71c1a36c881f8504889efe39845e04fb2b20ca10340a48c9c7dace87f.fgb`). Shipped today by `ingestion publish cloudflare-r2`; live at `https://repository.eafora.org/<version_label>/`.

### Manifest schema (consumer view)

An illustrative on-the-wire shape (the Rust type, not this snippet, is the canonical definition):

```json
{
  "manifest_schema_version": 1,
  "version": "2026-06-14+laughlin",
  "artifact_created": "2026-06-14T03:00:12Z",
  "geometry": {
    "relative_path": "geometry/world-50m-ddd660b71c1a36c881f8504889efe39845e04fb2b20ca10340a48c9c7dace87f.fgb",
    "size_bytes": 4380000,
    "sha256": "ddd660b71c1a36c881f8504889efe39845e04fb2b20ca10340a48c9c7dace87f"
  },
  "statistics": {
    "tfr": {
      "base": {
        "relative_path": "data/tfr-base-2c3a91...d4e7.sqlite",
        "size_bytes": 89000,
        "sha256": "2c3a91...d4e7"
      }
    }
  },
  "source_revisions": {
    "wb_wdi": { "revision": "2024-12-12", "published": "2024-12-12T00:00:00Z", "fetched": "2024-12-31T00:00:00Z" }
  }
}
```

(Statistics-shard hashes elided to `2c3a91...d4e7` for example legibility; real entries carry the full 64-character hex.)

Properties the consumer relies on:

- `manifest_schema_version` is `1` for v1; it is the FIRST key so a parser can fail fast on shape changes without parsing the rest. Clients reject manifests whose `manifest_schema_version` they don't recognize with a typed error (same forward-compat pattern as the discovery document's `schema_version`); this is the gate that lets v2+ change the manifest shape without breaking old binaries in the field. Producers always emit it.
- `version` is the human-readable, monotonically-disambiguated label (`YYYY-MM-DD+<surname>`). It is the cache key and the URL segment.
- `relative_path` is rooted at the version directory; the absolute URL is `<repository_base_url>/<version_label>/<relative_path>`. Clients must not assume any host or scheme — they read the base URL from configuration.
- `sha256` is the SHA-256 of the file's bytes, hex-encoded. Clients verify after download and reject the bundle if any hash mismatches.
- `statistics` is keyed first by statistic code, then by license shard class; values are exactly the entries the client may attach. (`base` is the only class in v1.)
- `source_revisions` is informational — surfaced in the UI's "data sources" panel; not load-bearing for any rendering decision.

The manifest type lives once in `core/src/artifact/manifest.rs` with both `Serialize` and `Deserialize` derived; the producer and every client use it directly. The Rust type is canonical; this document describes shape and intent but defers to the code on every disagreement.

> **Producer follow-ups (small PRs):**
> - Stand up the `core/` crate (workspace member) and move the manifest type into `core::artifact::manifest`, with `ingestion::artifact::writer::manifest` importing it. Currently the producer-side struct is local (`ingestion/src/artifact/writer/manifest.rs::ManifestSerializer`) and there is no `core/`. Sequenced before the first client implementation, since the client depends on `core/` existing.
> - Rename the `data/` subdirectory to `statistics/` for symmetry with `geometry/` and to remove the ambiguity of "data" as a shard subtype name. Touches the `SUBDIR_DATA` constant and its references. Pre-dates the first client implementation, so no migration concern.
> - `ingestion build` emits a `downsampled/` subtree per build (under `$EAFORA_ARTIFACTS_DIR/<version-label>/downsampled/`) for the native-client embedded bundle, generated directly from the canonical store alongside the complete bundle.
> - Publish the discovery document at `https://eafora.org/discovery` (see §Discovery and live bundle resolution for the schema). Initially a committed static file under the web app's `static/` tree; regenerated via a small script when the contract changes.

### Discovery and live bundle resolution

A client holds (up to) two artifact bundles at any moment: an **embedded** one (the downsampled bundle embedded in the binary on native, shipped as a static asset on web) and a **live** one (the latest CDN-published version; resolved at runtime). On every platform, the persistent on-device cache (OPFS on web; file system on iOS/Android) holds the most recently fetched live bundle, so returning users get instant first-paint regardless of platform. The embedded bundle is the additional baseline for first-ever-launch / cache-cleared / fresh-install scenarios — present on every platform, so every first-time user sees the map render before any live-bundle fetch resolves.

The embedded bundle on native serves two purposes: first-paint accelerant for first-ever-launch on the device, and the **offline-capable baseline** — a user who launches the app without connectivity and without a populated cache still sees a usable, if slightly stale, atlas. (Returning native users with a populated cache don't need the embedded bundle for first paint, but it's still there as the floor.) On web, the static-asset bundle serves only as the first-paint accelerant — there is no offline use case for the web client since the wasm itself ships from the same origin and is subject to the same connectivity constraints. The live bundle is the one the user is meant to see when online.

#### Embedded bundle (native + web)

Pinned at client build time on every platform. The client's build script pulls the downsampled subtree of the latest `ingestion build` (`$EAFORA_ARTIFACTS_DIR/latest/downsampled/`) and copies the result into its own asset directory (see §Embedded downsampled artifact for the per-platform paths). On native the bundle loads synchronously at startup; on web it's fetched at static-asset speed alongside the wasm. Either way the map renders before the live CDN fetch resolves.

#### Discovery URL: the one forever-URL

Clients commit to exactly one immutable URL: `https://eafora.org/discovery`. Everything else — including the repository base URL — is server-supplied at runtime. This indirection exists almost entirely for the native clients: web rebuilds and redeploys on every commit, so a static URL would be a one-commit change to update, but iOS and Android binaries live on user devices for months or years, and a `repository.eafora.org` re-platform without runtime indirection would silently break every old install in the field. We keep the contract identical across platforms (web included) for simplicity; it costs web nothing.

The endpoint is `/discovery` with no extension. The content-type comes from the response header, not the path; an extension would prematurely couple the URL to a specific backing implementation (static file vs. Pages Function vs. someday-Worker) and we want freedom there.

##### Discovery document shape

```json
{
  "schema_version": 1,
  "repository_base_url": "https://repository.eafora.org",
  "minimum_client_version": "0.1.0",
  "sunset": null
}
```

- `schema_version` lets the document's shape evolve. Clients reject documents whose `schema_version` they don't recognize, falling back to their static defaults.
- `repository_base_url` is where every shard URL is resolved against. The client never string-formats CDN paths; it joins `repository_base_url` + the manifest's per-entry `relative_path`.
- `minimum_client_version` is the lowest client version this contract still supports. Clients older than this surface a "please update" banner. Through v1 this is informational; it becomes load-bearing when v2's live API lands and old clients genuinely lose features.
- `sunset` is `null` in steady state. When non-null (RFC 3339 timestamp), clients surface a dismissible warning banner with the date; after the date passes, clients hard-error rather than continue against an end-of-life contract. Reserved for major contract changes that can't be made backward-compatible (R2 re-platform, manifest schema bump). This field ships in v1 deliberately: it has to exist in v1's schema to ever be usable for retiring v1 clients later; adding it under `schema_version: 2` would mean v1 clients don't know to look for it.

Fields not in v1, intentionally:

- No `repository_base_url_mirrors` for failover. Cloudflare's CDN already handles regional distribution; we have no real failover use case yet. Adding the field later is a non-breaking schema bump (old clients ignore it and run single-URL as today); the absence costs us nothing.

Cache headers on the discovery doc: `Cache-Control: public, max-age=3600`. A re-platform propagates to every client within an hour. Short enough to recover from a mistake; long enough to avoid hammering the endpoint.

The document is physically hosted on the same Cloudflare Workers Assets deploy that serves the web app (`web/static/discovery`, deployed at `https://eafora.org/discovery`), because `eafora.org` is the obvious place for it and we already have a deploy serving that origin. The endpoint is platform-agnostic — every client fetches it at startup — but it ends up living in the web tree by convenience. See `client-web.md` §Deploy target for the deployment shape.

##### Static fallback

Clients also keep a static `repository_base_url` from the committed discovery document at build time (web: `include_str!` of `web/static/discovery`). This fallback is used **only** when the discovery fetch fails (offline, broken document, transient outage). It drifts from current truth over time, but it's the right behavior for "client can't reach discovery — use the last-known-good source." If both discovery and the manifest fetch under the fallback URL fail, the embedded bundle remains the floor, exactly as designed.

The fallback is the committed discovery file, not a hand-typed string and not a script that fetches `https://eafora.org/discovery`.

##### Speculative parallel fetch at startup

The expected case is "the discovery URL still points at the static repository URL." That's the steady state. To save a round trip in this expected case, the client fires the discovery fetch and the speculative manifest fetch (against the static URL) **in parallel** at startup, then reconciles:

1. Construct `Bundle` from the embedded bundle. Map renders. (No network.)
2. Fire two requests in parallel:
   - The discovery fetch to `https://eafora.org/discovery`.
   - The manifest fetch to `<static_repository_base_url>/latest/manifest.json` (speculative).
3. When discovery resolves:
   - If its `repository_base_url` matches the static URL → the speculative fetch is the one we wanted. Await it, verify, hot-swap.
   - If it differs (re-platform happened) → cancel or discard the speculative fetch, issue a new manifest fetch against the discovered URL, await, verify, hot-swap.
4. If discovery fails → use the speculative fetch's result. If it succeeded, hot-swap. If both fail, surface a UI banner; embedded bundle remains the floor.
5. If discovery succeeds but the chosen `repository_base_url` 404s → surface the failure; same fallback.

The speculative fetch's errors are silenced *only* while discovery is still in flight. Once we know which URL is authoritative, errors on that URL are real and surface normally.

One implementation note: the speculative fetch writes to the cache as soon as bytes verify against the manifest's SHA-256 entries — no waiting on discovery. The cache is keyed by `version_label`, not by URL; bytes that match a manifest's hashes are correct bytes for that version regardless of which URL served them. If discovery returns a different `repository_base_url` and Swift fetches a different version from there, that version writes under its own subtree; the cache holds both versions briefly until eviction (per §Cache eviction's "keep current + most-recent prior" policy) cleans up. There's no "stale cache from the wrong URL" failure mode because correctness is content-verified, not source-verified.

Decision-tree summary:

- Discovery says static URL is still good → 1 round trip total (the speculative manifest fetch was the right one).
- Discovery says use a new URL → 2 round trips, one wasted.
- Discovery fails → 1 round trip (the speculative one we already had).
- Both fail → embedded bundle is the floor.

#### Live bundle: stable pointer at `latest/manifest.json`

Once the client has resolved `repository_base_url`, it fetches `<repository_base_url>/latest/manifest.json`. This URL always points at the most recently published version. Clients resolve every shard's URL using the manifest's per-entry `relative_path` against `<repository_base_url>/<version>/`. Clients do not string-format shard URLs or assume the directory layout (`geometry/`, `data/`, content-hashed filename); the manifest is the only source of truth for what to fetch and where it lives. The `version` field doubles as the cache key.

The "latest" determination is **server-side, sourced from the `artifact_version` table**:

1. `ingestion publish` finishes — inserts a row in `artifact_version` (this already happens; see `ingestion/src/artifact/artifact_db.rs`).
2. After that insert, `publish` writes a byte-for-byte copy of the just-published version's `manifest.json` to the stable key `latest/manifest.json` on the same destination. This is the last object uploaded; the versioned `{version}/manifest.json` is already on the destination.
3. The stable manifest is short-cached at the CDN (`max-age=300`, matching the per-version manifest's cache policy); the per-version content-addressed shards it references are immutable and cache for a year.

The DB is the source of truth for "latest"; R2 just hosts the resulting pointer. Clients never query Postgres (constitution: clients never call origin through v2). R2 listing is not used (no public listing on R2 anyway, and listing-as-discovery is fragile).

Concurrent-publish safety relies on the publish flow's manifest-last upload order (see `ingestion/src/artifact/publish.rs`): every shard a manifest references is already on R2 before the manifest goes up, so a client that fetches `latest/manifest.json` always sees a fully-published bundle.

#### Bundle hot-swap

When the live bundle finishes loading, it replaces the embedded one in-place — the renderer's `tokio::sync::watch::Sender<Arc<Bundle>>` publishes the new `Arc<Bundle>` to all subscribed receivers. Each reader takes its own `Arc` clone via `Receiver::borrow()` (or `.borrow_and_update()`) at the start of a query and uses it to completion; in-flight queries holding an old `Arc` finish against the old bundle, and the old bundle's memory frees when the last reference drops. The swap is wait-free in both directions — no reader blocks the writer; no writer blocks readers. On subsequent launches the live bundle is read from cache; the client refetches discovery + `latest/manifest.json` on launch and on a long-interval periodic timer (TBD; likely once per active session, plus on focus / visibility-change for web). If the resolved `version_label` differs from the cached one, the client fetches the new bundle and hot-swaps again.

#### Future: opt-in version pin

For QA / staged-rollout use cases, a client build can override the discovery URL (and therefore bypass the dynamic resolution entirely) with a fixed `version_label`. Out of scope for v1–v2; the mechanism is just "configure the client to skip discovery and use `<base>/<version_label>/manifest.json` directly."

#### v2+: live server architecture supersedes the static pointer

The `latest/manifest.json` flow above is a v1 design. v2's live server architecture replaces it: the discovery document's `repository_base_url` points at a live origin (under Cloudflare Tunnel from the Mac mini, dormant through v1) instead of a static R2 object. The static-pointer approach in v1 is intentionally minimal so the v2 transition is additive — the discovery doc gets updated to point at the new origin, clients pick up the change on their next launch, the producer drops the `latest/manifest.json` upload step, and the per-version bundles on R2 are unchanged. The discovery indirection is precisely what makes this transition seamless for old native clients.

### Verifying the bundle

After downloading any file, the client recomputes its SHA-256 and compares against `manifest.json`'s entry. A mismatch is a hard error: drop the cache, log a warning, retry the fetch once, then surface a UI-level "data unavailable, please reload" if it persists. The mismatch is treated as evidence of CDN corruption or a man-in-the-middle, not as a recoverable transient.

For the manifest itself the client has nothing to compare against on first launch — it trusts TLS to the configured `repository_base_url`. On subsequent launches the manifest is short-cached (HTTP `Cache-Control: max-age=300` from Cloudflare; see `docs/architecture/ingestion.md` for the producer-side cache headers), so the verifying loop is implicitly: fetch manifest, fetch shards by content hash, verify, attach.

## Fetch / cache / load pipeline

Every client follows the same four-stage pipeline. The platform-specific layer is the cache implementation; everything else is shared Rust.

```
        [embedded bundle - native only]                       [CDN]
                       |                                          |
                       v                                          v
     +------------------------------+        +-----------------------------+
     | bytes in the app binary      |        | https://repository...       |
     | (iOS, Android only)          |        | /<version>/manifest.json    |
     +------------------------------+        | /<version>/geometry/...     |
                       |                     | /<version>/data/...         |
                       |                     +-----------------------------+
                       |                                          |
                       |                                          v
                       |                          +--------------------------+
                       |                          | persistent on-device     |
                       |                          | cache (per-platform):    |
                       |                          |   web    -> OPFS         |
                       |                          |   iOS    -> file system  |
                       |                          |   Android-> file system  |
                       |                          +--------------------------+
                       |                                          |
                       v                                          v
     +-------------------------------------------------------------------+
     | core::artifact::Bundle (parsed manifest + open SQLite + parsed   |
     | FlatGeobuf reader); held by the renderer for the session         |
     +-------------------------------------------------------------------+
```

### Stage 1: launch

On native clients, the client constructs a `core::artifact::Bundle` from the embedded downsampled bytes synchronously, before the first frame; the map renders within milliseconds of the runtime starting and remains usable offline. On web, there is no embedded bundle — the client renders a loading state and proceeds to stage 2 immediately.

### Stage 2: cache check

The client asks the cache for the `version_label` resolved from `latest/manifest.json`. Three outcomes:

- **Full cache hit.** All referenced files (manifest + geometry + every shard) are present and pass SHA-256 verification. Replace the current bundle (embedded on native; loading state on web first-visit) with the cached bundle in-place. Done.
- **Partial cache hit.** Manifest is present but one or more referenced files are missing or hash-mismatched. Fall through to stage 3 for only the missing files.
- **Cache miss.** Nothing for this version. Fall through to stage 3 for everything.

### Stage 3: fetch

Fetch missing files via plain HTTP GET. Files are content-addressed and CDN-cached aggressively (`max-age=31536000, immutable`); the manifest is short-cached. The fetcher is platform-specific (`fetch()` in JS-land; `URLSession` on iOS; `OkHttp` on Android per the constitution); the bytes are then handed to the Rust core uniformly as `&[u8]`.

Fetches are issued concurrently up to a per-platform parallelism cap (browser typically 6 per origin; native clients 4). The client renders progress as bytes-received over expected-total (sum of `size_bytes` from the manifest). On any HTTP error or hash mismatch, retry once after a short backoff (approx. 100 ms, doubling to approx. 400 ms on a second attempt); persistent failure leaves the embedded bundle (native) or loading state (web) in place and surfaces a UI-level banner.

### Stage 4: persist + attach

Verified bytes go into the persistent cache and into a fresh `core::artifact::Bundle`. The bundle replaces whatever the renderer was previously holding (embedded on native; loading state on web first-visit; cached prior version on any returning client). The renderer awaits the bundle channel via `tokio::sync::watch::Receiver::changed()` and re-reads the new `Arc<Bundle>` each time the loader publishes — the same `watch` channel that backs the hot-swap, used here and on every subsequent refetch (no separate one-shot).

### Cache eviction

The cache holds one or more complete artifact versions. The default policy is **keep the current resolved version + the most recent prior version**; older versions are deleted on launch. The prior-version retention exists so a brief publish rollback (rare) doesn't force a full re-fetch.

The embedded bundle on native is not part of the cache — it lives inside the app binary and is never evicted. It is replaced only when the user installs a new app build (whose `ingestion build` downsampled subtree captured a newer baseline). On native, the floor of available data is therefore "embedded version OR cached version, whichever is more recent"; on web, it's just "cached version, if any."

Per-platform policy differs in failure modes — see `client-web.md` for OPFS quota / `navigator.storage.persist()` / `estimate()` handling per the saved memory `reference_browser_storage_quotas`; see `client-ios.md` and `client-android.md` for iOS document-directory and Android internal-storage equivalents. The cross-platform contract is just: a `cache.put(version_label, file_relative_path, bytes)` / `cache.get(version_label, file_relative_path) -> Option<Bytes>` interface, implemented per platform and consumed by the same Rust core.

## SQLite in the client

The chosen approach is **download-the-whole-shard-and-query-in-memory**, on every platform. SQLite's file format is identical across platforms; the only thing that changes per-platform is which Rust SQLite library opens the file.

| Platform | SQLite library                                                                                                  | File access                                                  |
| -------- | --------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| Web      | `sqlite-wasm-rs` (a wasm32-targeted SQLite-in-Rust crate that ships a pre-built sqlite WASM blob with a custom VFS shim). | The downloaded `.sqlite` bytes are deserialized into the in-memory database via the crate's `Connection::deserialize`-equivalent. |
| iOS      | `rusqlite` (statically linked, `bundled` feature).                                                              | Open the cached file path via `Connection::open(path)`. |
| Android  | `rusqlite` (statically linked, `bundled` feature).                                                              | Open the cached file path via `Connection::open(path)`. |

The web platform does **not** use OPFS, sql.js, or wa-sqlite. Reasoning:

- A Rust SQLite library on WASM means the same `core::*` query code runs everywhere with at most a thin cfg-gated alias layer in `core::sqlite` (typedef `Connection = rusqlite::Connection` on non-wasm32 targets, `Connection = sqlite_wasm_rs::Connection` on wasm32; the renderer's queries are simple enough — `SELECT value FROM statistic_value WHERE region_iso3 = ?1 AND period_start = ?2` — that the API surface both libraries expose covers them. Where the surfaces diverge, `core::sqlite` exposes a thin facade.). No JS/TS query layer; no parallel implementation to keep in sync; no FFI marshaling for query results. This is a direct application of Constitution Principle V (explicit over implicit) and the architectural premise that Rust core is the single source of truth.

> **Why two libraries instead of one.** `rusqlite` with `features = ["bundled"]` does not cross-compile cleanly to `wasm32-unknown-unknown`: the bundled SQLite C source needs a libc, and `wasm32-unknown-unknown` has none. WASI provides one (`wasm32-wasip1`) but the web client's compile target is `wasm32-unknown-unknown` per `client-web.md` §`cargo-leptos`, and switching the web target to WASI would cascade through cargo-leptos, wasm-bindgen, and the wasm bundle shape. `sqlite-wasm-rs` ships a pre-built SQLite WASM blob designed for `wasm32-unknown-unknown` consumers; near-rusqlite-compatible Rust API. The two-library approach is the pragmatic answer.

- Per-statistic shards are tens of KB to a few MB through v2 (memory: `feedback_consolidated_migrations` is unrelated, but the producer-side per-shard size estimates in `docs/architecture/ingestion.md` apply). Holding a 5 MB `Vec<u8>` in WASM linear memory is trivially cheap compared to the ~3 MB raw WASM bundle itself; there's no pressure to stream.
- OPFS is a 2024+ API. The synchronous file-handle API needed for SQLite is Worker-only on every shipping browser today, so adopting OPFS would mean hosting the SQLite engine in a Worker and going through `postMessage` for every query. That is on the table for the migration path described below, but for current shard sizes the added structural complexity buys nothing — in-memory is faster and simpler.
- sql.js is JavaScript-implemented SQLite. Using it means parsing query results in JS and marshaling across the wasm-bindgen boundary on every read. wa-sqlite is the modern equivalent. Both have the same problem: the query layer lives outside Rust.

**Migration trigger:** if any per-statistic shard grows past approx. 30 MB (to be reviewed when v2's license shards land or when subnational statistics arrive), revisit by hosting the SQLite engine inside a dedicated Worker with the database file backed by an OPFS `FileSystemSyncAccessHandle`. The main thread sends query requests to the Worker via `postMessage` and receives results the same way; SQLite reads pages on demand from the OPFS file rather than from a fully-resident `Vec<u8>`. Local, persistent, no per-query network dependency, no `SharedArrayBuffer`, no COOP/COEP. The migration is structurally larger than HTTP range requests would be (Worker setup, message-passing query API, OPFS write-through on cache update) but produces materially better UX once shards stop fitting in memory.

### Attaching license shards

The bundle's `statistics` map is keyed `statistic-code → license-shard-class → shard entry` (nested, not tupled). A single statistic can expose multiple license shards in v2+; v1 ships only `base` per statistic.

The client identifies its **distribution context** at startup (serving eafora.org itself, or running inside a third party's site) and looks up the *authorized class set* — the subset of license classes its context is permitted to access. The context is a property of where the client is deployed, never of which artifact bundle it happens to have loaded. For v1 every context authorizes `base` and there is nothing else to choose between; the mechanism is exercised trivially.

Per statistic, the client opens an in-memory SQLite database and `ATTACH DATABASE` of every authorized shard for that statistic. Queries union across attached databases as a SQLite-native operation. The slice is written in the alphabetical order of license-class names (matches the manifest's serialization order), so the resulting `sqlite_master` is deterministic without a runtime sort — useful for debugging.

Authorized-class evaluation is a `match` on a context enum returning a `&'static [LicenseShardClass]`:

```rust
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

Lives in `core::license`. This is the only place the per-context license matrix lives; both the client and any future server-side filter (v3+) call it. Adding a new `LicenseShardClass` does not silently appear in any context — each `DistributionContext` arm must be updated explicitly. Adding a new `DistributionContext` requires writing the slice explicitly. Both are deliberate failure modes.

## FlatGeobuf in the client

The chosen reader is **the upstream `flatgeobuf` Rust crate**, compiled into the same Rust core that runs everywhere. The crate provides streaming feature reads and an embedded R-tree spatial index for hit testing. Per the overview's §Polygon representation, the spatial index in the FlatGeobuf file is the hit-test index for the renderer — no separate index build step.

Why not the JavaScript `flatgeobuf` package: same reason as not sql.js. Keeping the parser in Rust avoids a duplicated implementation on each platform and a per-feature marshaling cost across wasm-bindgen / UniFFI.

The reader is initialized once per bundle load. Country features parse first, in the foreground; if subnational features are present (v2+) they parse in the background after the initial render is up, scheduled by the platform shell. The progress signal goes through the same renderer-notification channel as cache replacement.

## Embedded downsampled artifact

Every client ships with the same downsampled bundle — a small subset of the live artifact that gives every first-time user (and every offline-capable device) an instant render. The bundle bytes are identical across platforms; only the **delivery mechanism** differs:

- **Native** (iOS, Android): bytes embedded in the app binary at build time. Available before any network or filesystem activity. Doubles as the offline-capable baseline when no cache and no network are present.
- **Web**: bytes shipped as a static asset alongside the wasm on Cloudflare Workers Assets. Fetched on first visit (HTTP-cached for return visits) before the live CDN bundle, so the first-ever visitor sees the map render at static-asset speed rather than waiting on a separate live-bundle fetch.

The downsampled bundle is generated by `ingestion build`, which reads the canonical store directly (no CDN round trip) and writes a reduced artifact set to the `downsampled/` subtree of the build (`$EAFORA_ARTIFACTS_DIR/<version-label>/downsampled/`, alongside the complete bundle at `.../complete/`). It does not touch any per-platform asset directory.

Downsampling rules applied during shard emission:

- **Geometry: keep country polygons at full 1:50m resolution** (the same resolution the live bundle ships); drop subnational geometry when v2+ adds it. Approx. 1.5 MB.
- **Statistics: keep only the most-recent year of values per country, for every statistic.** Historical depth is dropped; cross-statistic coverage is preserved. Approx. tens-to-low-hundreds of KB total across all statistics through v2.

Total embedded bundle: approx. 1.5–1.7 MB through v2. Matching the live bundle's geometry resolution avoids a low-res-then-high-res transition when the swap happens. Restricting statistics to the current year keeps every indicator visible offline without paying for full historical series — the live bundle fills in the time-series depth on first online connection.

Revisit when v2+ subnational geometry lands (would push the geometry portion well past the current 1.5 MB) or when current-year-across-all-statistics stops fitting comfortably in single-digit MB.

Each client's build pipeline pulls the latest downsampled bundle from `$EAFORA_ARTIFACTS_DIR/latest/downsampled/` and copies it into its own asset directory (via `scripts/sync-embedded-bundle.sh <destination-dir>`, which runs `ingestion build` first if no build exists yet):

- iOS: bundle build script reads from `$EAFORA_ARTIFACTS_DIR/latest/downsampled/` and copies into `ios/EaforaApp/Resources/embedded_artifacts/` as part of the Xcode build.
- Android: Gradle task reads from `$EAFORA_ARTIFACTS_DIR/latest/downsampled/` and copies into `android/app/src/main/assets/embedded_artifacts/` as part of the Android build.
- Web: cargo-leptos build step (or equivalent) reads from `$EAFORA_ARTIFACTS_DIR/latest/downsampled/` and copies into `web/static/embedded_artifacts/` so Cloudflare Workers Assets serves it alongside the wasm bundle.

The dependency direction is **client build pulls from the producer's output**, never **producer pushes into client trees**. This keeps `ingestion` agnostic to per-platform layout and lets each client decide when (and whether) to refresh its embedded bundle.

The embedded bundle is read into memory at app startup, parsed by the same `core::artifact` code path that handles CDN bundles, and replaced in-place when the live CDN fetch completes. From the renderer's point of view there is exactly one source of bundles; the embedded one is just the one without a live HTTP round trip.

The embedded bundle is regenerated and re-bundled into client artifacts **on every client build**. Stale embedded bundles are not a correctness issue — the CDN fetch upgrades them — but a fresh first-paint experience is a UX win, and the build step is the natural moment to refresh.

### Web first-paint perf budget

Web first paint serves wasm + the static-asset bundle together; together they're the price of "instant atlas render" on a fresh visit. Caps to enforce in the web build:

- **2 MB total compressed at first paint** — wasm bundle + static-asset embedded bundle + page shell. CI fails if the deployed total exceeds it.
- **3 MB total compressed at second paint** — once the live CDN bundle has loaded in the background.

Expected sizes against the 2 MB ceiling: wasm approx. 600 KB brotli (per overview §Web client; `wasm-opt -O4`), static-asset bundle approx. 700 KB–1 MB brotli (FlatGeobuf and SQLite both compress well), page shell <50 KB — totals approx. 1.4–1.7 MB with comfortable headroom.

### Returning-visitor flow

The previous-visit cache (OPFS on web; file system on native) holds the most recently fetched live bundle. On every platform, returning users render the cached live bundle on launch — strictly newer than the embedded bundle and so the better starting point. The embedded bundle is the floor; the cached live bundle is the next-most-recent floor when it exists; the freshly-fetched live bundle from the CDN replaces both.

## Cross-platform consistency

The Rust core enforces consistency on the things that should be consistent. Per-platform code owns the things that genuinely differ.

| Concern                                            | Owner       | Rationale |
| -------------------------------------------------- | ----------- | --------- |
| Manifest parsing                                   | Rust core   | Bytes are bytes; no platform reason to diverge. |
| SQLite query strings (statistic-by-region-by-year) | Rust core   | One source of truth; tested once. |
| FlatGeobuf parsing                                 | Rust core   | Same as SQLite. |
| Hit testing                                        | Rust core   | Spatial-index reads from the FlatGeobuf are framework-agnostic. |
| Projection (Miller cylindrical)                    | Rust core   | Closed-form math; lives in `core::map::projection`. |
| HTTP fetch                                         | Per-platform | Native APIs are the right tool: `fetch()` (web), `URLSession` (iOS), `OkHttp` (Android). |
| Cache persistence                                  | Per-platform | OPFS / file-system contracts differ enough that a Rust abstraction would be a leaky shim. |
| Render loop                                        | Per-platform | wgpu surface acquisition is platform-specific; the draw calls themselves are shared. |
| UI chrome (legend, statistic picker, source panel) | Per-platform | Leptos / SwiftUI / Compose own their idiomatic UI; the data shown is identical because it's read from the same `core` queries. |

The per-platform shell is intentionally thin. A typical client per-platform layer is on the order of 1–2k LOC: a fetcher, a cache adapter, a render-surface bridge, the UI tree, and any platform-framework integrations (notifications, sharing, deep linking, OAuth handoff, App Intents / Android Intents, accessibility services, etc.) that have no portable equivalent. Framework integrations are necessarily platform code and don't count against the "thinness" budget. Anything else beyond the categories above should be re-evaluated as a candidate for promotion into `core`.

## Module layout (`core/` consumer surface)

The producer (`ingestion/`) writes manifests; the consumer (every client via `core/`) reads them. The consumer types live alongside the geometry / statistic types they wrap.

```
core/
├── src/
│   ├── lib.rs
│   ├── artifact/
│   │   ├── artifact.rs            # Bundle: open(manifest_bytes, cache_reader) -> Bundle
│   │   ├── artifact_model.rs      # Manifest, ManifestEntry, StatisticEntry, etc.
│   │   └── manifest.rs            # parse_manifest(bytes) -> Manifest
│   ├── hashing/
│   │   └── hashing.rs             # sha256_hex(bytes), verify_sha256(bytes, expected_hex)
│   ├── sqlite/
│   │   ├── sqlite.rs              # connection wrapper around the in-memory SQLite database
│   │   ├── attach.rs              # ATTACH-DATABASE composition across license shards
│   │   └── vfs.rs                 # Vec<u8>-backed custom VFS (cfg-gated to wasm32)
│   ├── statistic/
│   │   ├── statistic.rs           # statistic-domain queries (uses crate::sqlite)
│   │   └── statistic_model.rs     # StatisticValue, Series, etc. (shared with ingestion via core)
│   ├── map/                       # interactive atlas view feature
│   │   ├── geometry/
│   │   │   ├── geometry.rs        # FlatGeobuf reader wiring; feature iteration
│   │   │   └── geometry_model.rs  # CountryFeature, Polygon, BoundingBox
│   │   ├── projection.rs          # Miller cylindrical
│   │   ├── hit_test.rs            # spatial-index lookup
│   │   └── map_renderer.rs        # wgpu pipeline (shared across platforms)
│   ├── license/
│   │   └── license.rs             # DistributionContext -> authorized &'static [LicenseShardClass]
│   └── ffi/
│       ├── wasm.rs                # wasm-bindgen surface (web)
│       └── uniffi.rs              # UniFFI surface (iOS, Android)
```

Per-feature module layout follows the Singularity `lobby/` triplet pattern; the consumer side has no `<feature>_db.rs` because there is no Postgres in the client, and no `<feature>_api.rs` because the client doesn't host HTTP routes. Where a feature needs an external-call abstraction in the future (a v3+ live correction-submission API), the `<feature>_client.rs` slot is reserved.

## Testing strategy

Per Constitution Principle VII, each TDD-required surface gets unit tests written before implementation. The client side has the following such surfaces:

- Manifest parsing: round-trip a known wire-format manifest through `parse_manifest` and assert every field. Reject malformed input with a typed `ManifestError`.
- SHA-256 verification: known input bytes → known hex digest; mismatch fails fast.
- License-class authorization: every `DistributionContext` variant returns the documented `&'static [LicenseShardClass]`.
- Cache adapter contract: a per-platform integration test that does `cache.put(...) -> cache.get(...)` round-trips and asserts a missing key returns `None`. Web's version runs against OPFS in headless Chrome; iOS / Android run against the real device file system in their native test runner.
- FlatGeobuf hit testing: a feature collection with two known polygons; clicks at known points return the expected feature ids.

Live HTTP against the CDN is **not** part of automated tests; it's a manual smoke step run after each client deploy. The producer side already covers "the CDN serves what we think it serves" through `ingestion publish cloudflare-r2` + a curl check; duplicating that on the client side would add wall-clock time without catching a class of bug the producer side doesn't.

## Decisions still open

- **wgpu / WebGPU fallback policy.** WebGPU is stable in Chromium and Safari 18.4+; Firefox is on WebGL2 via the wgpu downlevel backend. The capability detection happens inside `wgpu::Instance::request_adapter`, so the client doesn't need its own logic — but the *UI fallback* (do we render a coarser version under WebGL2, or do we render the same version with a perf-warning banner?) is per-platform UX work. Defer to `client-web.md`.
- **Embedded-bundle build automation (native).** `ingestion build` now emits the downsampled subtree (`$EAFORA_ARTIFACTS_DIR/<version-label>/downsampled/`) on every build alongside the complete bundle, and `scripts/sync-embedded-bundle.sh <destination-dir>` copies `$EAFORA_ARTIFACTS_DIR/latest/downsampled/` into a client's asset directory (running `ingestion build` first if no build exists). Each native build script needs to invoke this sync step and copy the result into its own asset directory when native-client work begins.
- **Translation table location.** Per overview §FFI, country / statistic / source-attribution display names are written into the SQLite at build time, sourced from ISO 3166 + per-language overrides. v1 is English-only; the hooks need to exist for v2+. Whether the translation table is a separate SQLite shard or rolled into each statistic shard is open. **Trigger:** i18n lands (a second locale becomes a real deliverable).

