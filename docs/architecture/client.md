# Client architecture

> **Status: draft, 2026-06-14.** This document is the cross-cutting consumer-side companion to `docs/architecture/ingestion.md`. Ingestion ends with a content-addressed bundle on Cloudflare R2 (`manifest.json` + per-statistic SQLite shards + a FlatGeobuf geometry file); this document defines what every client (web, iOS, Android) does with that bundle to render a fertility-data atlas. Per-platform deltas (build system, hot reload, threading model, FFI surface) live in `client-web.md`, `client-ios.md`, `client-android.md`, which are subsequent branches.

## Scope of this document

This document covers everything between **a published artifact bundle on the CDN** and **a rendered map with fertility data overlaid**:

- The artifact-consumption contract: how clients discover and validate a manifest, and how the embedded vs. live bundle relationship works.
- The fetch / cache / load pipeline: HTTP → on-device persistent cache → in-memory data structures.
- SQLite-in-the-client: which engine, how the database is opened, how queries run.
- FlatGeobuf reading: which reader, how features feed the renderer.
- License-shard composition: how the client picks which shards to attach for its distribution context.
- Embedded downsampled artifact: the "good enough for first paint and offline use" bundle baked into native client binaries (no equivalent on web).
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

- `version` is the human-readable, monotonically-disambiguated label (`YYYY-MM-DD+<surname>`). It is the cache key and the URL segment.
- `relative_path` is rooted at the version directory; the absolute URL is `<repository_base_url>/<version_label>/<relative_path>`. Clients must not assume any host or scheme — they read the base URL from configuration.
- `sha256` is the SHA-256 of the file's bytes, hex-encoded. Clients verify after download and reject the bundle if any hash mismatches.
- `statistics` is keyed first by statistic code, then by license shard class; values are exactly the entries the client may attach. (`base` is the only class in v1.)
- `source_revisions` is informational — surfaced in the UI's "data sources" panel; not load-bearing for any rendering decision.

The manifest type lives once in `core/src/artifact/manifest.rs` with both `Serialize` and `Deserialize` derived; the producer and every client use it directly. The Rust type is canonical; this document describes shape and intent but defers to the code on every disagreement.

> **Producer follow-ups (small PRs):**
> - Stand up the `core/` crate (workspace member) and move the manifest type into `core::artifact::manifest`, with `ingestion::artifact::writer::manifest` importing it. Currently the producer-side struct is local (`ingestion/src/artifact/writer/manifest.rs::ManifestSerializer`) and there is no `core/`. Sequenced before the first client implementation, since the client depends on `core/` existing.
> - Rename the `data/` subdirectory to `statistics/` for symmetry with `geometry/` and to remove the ambiguity of "data" as a shard subtype name. Touches the `SUBDIR_DATA` constant and its references. Pre-dates the first client implementation, so no migration concern.
> - Add `ingestion build --downsampled <output-dir>` for generating the native-client embedded bundle directly from the canonical store. Sequenced when native-client work begins.
> - On every successful `ingestion publish`, copy the just-published manifest to the stable key `latest/manifest.json` on the destination so clients have a fixed discovery URL (see §Version pinning).

### Version pinning and discovery

A client holds (up to) two artifact bundles at any moment: an **embedded** one (native clients only — bytes baked into the app binary at build time) and a **live** one (the latest CDN-published version; resolved at runtime). On every platform, the persistent on-device cache (IndexedDB on web; file system on iOS/Android) holds the most recently fetched live bundle, so returning users get instant first-paint regardless of platform. The native embedded bundle is the additional baseline for first-ever-launch / cache-cleared / fresh-install scenarios on native; web has no such baseline.

The embedded bundle on native serves two purposes: it is the first-paint accelerant for first-ever-launch on the device, and it is the **offline-capable baseline** — a user who launches the app without connectivity and without a populated cache still sees a usable, if slightly stale, atlas. (Returning native users with a populated cache don't need the embedded bundle for first paint, but it's still there as the floor.) The live bundle is the one the user is meant to see when online.

#### Embedded bundle (native clients)

Pinned at native-client build time. The native client's build script invokes (or fetches the most recent output of) `ingestion build --downsampled` (see §Embedded downsampled artifact) and copies the result into its own asset directory. The client loads it synchronously at startup so the map renders before any network activity, and the same bytes are the offline baseline if the network is unavailable.

#### Live bundle: stable pointer at `latest/manifest.json`

The producer maintains a stable URL — `https://repository.eafora.org/latest/manifest.json` — that always points at the most recently published version. Clients fetch this URL on launch (and periodically thereafter, see below), parse out `version_label`, and load the bundle from `<repository_base_url>/<version_label>/`.

The "latest" determination is **server-side, sourced from the `artifact_version` table**:

1. `ingestion publish` finishes — inserts a row in `artifact_version` (this already happens; see `ingestion/src/artifact/artifact_db.rs`).
2. As a final publish step, the producer reads the latest row (`select * from artifact_version order by created desc limit 1`) and uploads a byte-for-byte copy of that version's `manifest.json` to the stable key `latest/manifest.json` on R2. (To be implemented as a small follow-up PR on the producer side, separate from the client-implementation work that consumes this pointer.)
3. The stable manifest is short-cached at the CDN (`max-age=300`, matching the per-version manifest's cache policy); the per-version content-addressed shards it references are immutable and cache for a year.

The DB is the source of truth for "latest"; R2 just hosts the resulting pointer. Clients never query Postgres (constitution: clients never call origin through v2). R2 listing is not used (no public listing on R2 anyway, and listing-as-discovery is fragile).

Concurrent-publish safety relies on the publish flow's manifest-last upload order (see `ingestion/src/artifact/publish.rs`): every shard a manifest references is already on R2 before the manifest goes up, so a client that fetches `latest/manifest.json` always sees a fully-published bundle.

#### Bundle hot-swap

When the live bundle finishes loading, it replaces the embedded one in-place — the renderer's `Arc<Bundle>` is swapped (see §Decisions still open for the swap-vs-frame-boundary choice). On subsequent launches the live bundle is read from cache; the client refetches `latest/manifest.json` on launch and on a long-interval periodic timer (TBD; likely once per active session, plus on focus / visibility-change for web). If the resolved `version_label` differs from the cached one, the client fetches the new bundle and hot-swaps again.

#### Future: opt-in version pin

For QA / staged-rollout use cases, a client build can override the discovery URL with a fixed `version_label`. Out of scope for v1–v2; the mechanism is just "configure `repository_base_url` to point at `<base>/<version_label>` instead of `<base>/latest`."

#### v2+: live server architecture supersedes the static pointer

The `latest/manifest.json` flow above is a v1 design. v2's live server architecture replaces it: the client resolves the current version against a live origin (under Cloudflare Tunnel from the Mac mini, dormant through v1) instead of a static R2 object. The static-pointer approach in v1 is intentionally minimal so the v2 transition is additive — clients gain a new discovery endpoint, the producer drops the `latest/manifest.json` upload step, and the per-version bundles on R2 are unchanged.

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
                       |                          |   web    -> IndexedDB    |
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

The client asks the cache for the pinned `version_label`. Three outcomes:

- **Full cache hit.** All referenced files (manifest + geometry + every shard) are present and pass SHA-256 verification. Replace the current bundle (embedded on native; loading state on web first-visit) with the cached bundle in-place. Done.
- **Partial cache hit.** Manifest is present but one or more referenced files are missing or hash-mismatched. Fall through to stage 3 for only the missing files.
- **Cache miss.** Nothing for this version. Fall through to stage 3 for everything.

The cache key is `version_label`, not the manifest URL. A new version installed by a client redeploy doesn't share keys with the previous one — old versions sit in the cache until the eviction policy reaps them.

### Stage 3: fetch

Fetch missing files via plain HTTP GET. Files are content-addressed and CDN-cached aggressively (`max-age=31536000, immutable`); the manifest is short-cached. The fetcher is platform-specific (`fetch()` in JS-land; `URLSession` on iOS; `OkHttp` on Android per the constitution); the bytes are then handed to the Rust core uniformly as `&[u8]`.

Fetches are issued concurrently up to a per-platform parallelism cap (browser typically 6 per origin; native clients 4). The client renders progress as bytes-received over expected-total (sum of `size_bytes` from the manifest). On any HTTP error or hash mismatch, retry once with exponential backoff (250ms / 1s); persistent failure leaves the embedded bundle (native) or loading state (web) in place and surfaces a UI-level banner.

### Stage 4: persist + attach

Verified bytes go into the persistent cache and into a fresh `core::artifact::Bundle`. The bundle replaces whatever the renderer was previously holding (embedded on native; loading state on web first-visit; cached prior version on any returning client). The renderer is notified via a one-shot signal; it discards its current draw state and re-issues from the new bundle's geometry on the next frame.

### Cache eviction

The cache holds one or more complete artifact versions. The default policy is **keep the current pinned version + the most recent prior version**; older versions are deleted on launch. The prior-version retention exists so a client downgrade (rare; happens if a deploy is rolled back) doesn't force a full re-fetch.

Per-platform policy differs in failure modes — see `client-web.md` for IndexedDB quota / `navigator.storage.persist()` / `estimate()` handling per the saved memory `reference_browser_storage_quotas`; see `client-ios.md` and `client-android.md` for iOS document-directory and Android internal-storage equivalents. The cross-platform contract is just: a `cache.put(version_label, file_relative_path, bytes)` / `cache.get(version_label, file_relative_path) -> Option<Bytes>` interface, implemented per platform and consumed by the same Rust core.

## SQLite in the client

The chosen approach is **download-the-whole-shard-and-query-in-memory**, on every platform. SQLite's file format is identical across platforms; the only thing that changes per-platform is which library opens the file.

| Platform | SQLite library                  | File access                                                  |
| -------- | ------------------------------- | ------------------------------------------------------------ |
| Web      | `rusqlite` compiled to WASM, with the database backed by the SQLite VFS reading from a `Vec<u8>` held in WASM linear memory. | The downloaded `.sqlite` bytes are passed to a custom VFS layer; no OPFS / no file handle. |
| iOS      | `rusqlite` (statically linked).  | Open the cached file path via `Connection::open(path)`. |
| Android  | `rusqlite` (statically linked).  | Open the cached file path via `Connection::open(path)`. |

The web platform does **not** use OPFS, sql.js, or wa-sqlite. Reasoning:

- `rusqlite`-on-WASM means the same `core::*` query code runs everywhere, byte-for-byte. No JS/TS query layer; no parallel implementation to keep in sync; no FFI marshaling for query results. This is a direct application of Constitution Principle V (explicit over implicit) and the architectural premise that Rust core is the single source of truth.
- Per-statistic shards are tens of KB to a few MB through v2 (memory: `feedback_consolidated_migrations` is unrelated, but the producer-side per-shard size estimates in `docs/architecture/ingestion.md` apply). Holding a 5 MB `Vec<u8>` in WASM linear memory is trivially cheap compared to the ~3 MB raw WASM bundle itself; there's no pressure to stream.
- OPFS is a 2024+ API with uneven Safari support (file handle support landed in iOS 17 but the synchronous handle API needed for SQLite has historically required Worker context — and we deliberately don't use Workers, see Web client overview). Until OPFS is uniformly available *and* the data sizes warrant the streaming model, in-memory wins on simplicity.
- sql.js is JavaScript-implemented SQLite. Using it means parsing query results in JS and marshaling across the wasm-bindgen boundary on every read. wa-sqlite is the modern equivalent. Both have the same problem: the query layer lives outside Rust.

**Migration trigger:** if any per-statistic shard grows past ~30 MB (to be reviewed when v2's license shards land or when subnational statistics arrive), revisit by switching the WASM client's SQLite to read via HTTP range requests through a custom `Connection` impl. This is mechanical work behind the same `core::statistic` API.

### Attaching license shards

The bundle's `statistics` map is keyed by statistic-code → license-shard-class. The client identifies its **distribution context** at startup (eafora.org-first-party, embedded-third-party-widget, etc.) and the runtime computes the *authorized class set* — the subset of license classes its context is permitted to access. For v1 the only class is `base` and every context authorizes it; the mechanism is exercised but trivially.

Per statistic, the client opens an in-memory SQLite database and `ATTACH DATABASE` of every authorized shard for that statistic. Queries union across attached databases as a SQLite-native operation. The attach order is the alphabetical order of license-class names (matches the manifest's serialization order), which means the resulting `sqlite_master` is deterministic — useful for debugging.

Authorized-class evaluation is a function in `core::license` whose input is a context enum (`DistributionContext::FirstParty | EmbeddedWidget | ...`) and whose output is a `BTreeSet<LicenseShardClass>`. This is the only place the per-context license matrix lives; both the client and any future server-side filter (v3+) call it.

## FlatGeobuf in the client

The chosen reader is **the upstream `flatgeobuf` Rust crate**, compiled into the same Rust core that runs everywhere. The crate provides streaming feature reads and an embedded R-tree spatial index for hit testing. Per the overview's §Polygon representation, the spatial index in the FlatGeobuf file is the hit-test index for the renderer — no separate index build step.

Why not the JavaScript `flatgeobuf` package: same reason as not sql.js. Keeping the parser in Rust avoids a duplicated implementation on each platform and a per-feature marshaling cost across wasm-bindgen / UniFFI.

The reader is initialized once per bundle load. Country features parse first, in the foreground; if subnational features are present (v2+) they parse in the background after the initial render is up, scheduled by the platform shell. The progress signal goes through the same renderer-notification channel as cache replacement.

## Embedded downsampled artifact (native only)

Native clients (iOS, Android) embed a small artifact bundle directly in the app binary so the first frame renders before any network or filesystem activity. The web client has no equivalent — there is no shipped binary for web; visitors download wasm + JS + static assets fresh each visit (modulo browser HTTP caching) and the first-paint accelerant on web is the previous session's IndexedDB cache, not an embedded bundle.

The downsampled bundle is generated by `ingestion build --downsampled`, which reads the canonical store directly (no CDN round trip) and writes a reduced artifact set to a single output directory (alongside the regular `ingestion build` output). It does not touch any per-platform asset directory.

Downsampling rules applied during shard emission:

- Drop sub-national geometry (v2+); keep country polygons.
- Reduce country geometry resolution to ~1:110m equivalent (Natural Earth's coarsest released set), enough for instant first paint without visible artifacts at low zoom.
- For each statistic, keep only the most recent year of values per country.

Each native client's build pipeline is responsible for fetching (or regenerating + fetching) the latest downsampled output and loading it into its own asset directory:

- iOS: bundle build script reads from the downsampled output directory and copies into `ios/EaforaApp/Resources/embedded/` as part of the Xcode build.
- Android: Gradle task reads from the downsampled output directory and copies into `android/app/src/main/assets/embedded/` as part of the Android build.

The dependency direction is **client build pulls from the producer's output**, never **producer pushes into client trees**. This keeps `ingestion` agnostic to per-platform layout and lets each client decide when (and whether) to refresh its embedded bundle.

The embedded bundle is read into memory at app startup, parsed by the same `core::artifact` code path that handles CDN bundles, and replaced in-place when the CDN fetch completes. From the renderer's point of view there is exactly one source of bundles; the embedded one is just the one without an HTTP round trip.

The embedded bundle is regenerated and re-bundled into native-client builds **on every native-client redeploy**. Stale embedded bundles are not a correctness issue — the CDN fetch upgrades them — but a fresh first-paint experience is a UX win, and the redeploy hook is the natural moment to refresh.

### Web first-paint without an embedded bundle

The previous-visit cache (IndexedDB) gives the web client the same returning-user UX as native: a populated cache renders the previous bundle before any network activity. The difference is only in the cache-empty case:

- **Returning visitor (IndexedDB populated).** Same as native — render the cached bundle, then upgrade in the background.
- **First-ever visitor (cache empty).** No bundle is available before the CDN fetch returns. The client renders a loading state (skeleton map / progress indicator) until the first manifest + geometry land. Whether to also ship a downsampled bundle as a static asset alongside the wasm in `web/static/` — which would give first-ever visitors an instant render at the cost of a larger initial download — is open (see §Decisions still open).

## Cross-platform consistency

The Rust core enforces consistency on the things that should be consistent. Per-platform code owns the things that genuinely differ.

| Concern                                            | Owner       | Rationale |
| -------------------------------------------------- | ----------- | --------- |
| Manifest parsing                                   | Rust core   | Bytes are bytes; no platform reason to diverge. |
| SQLite query strings (statistic-by-region-by-year) | Rust core   | One source of truth; tested once. |
| FlatGeobuf parsing                                 | Rust core   | Same as SQLite. |
| Hit testing                                        | Rust core   | Spatial-index reads from the FlatGeobuf are framework-agnostic. |
| Projection (Miller cylindrical)                    | Rust core   | Closed-form math; lives in `core::projection`. |
| HTTP fetch                                         | Per-platform | Native APIs are the right tool: `fetch()` (web), `URLSession` (iOS), `OkHttp` (Android). |
| Cache persistence                                  | Per-platform | IndexedDB / file-system contracts differ enough that a Rust abstraction would be a leaky shim. |
| Render loop                                        | Per-platform | wgpu surface acquisition is platform-specific; the draw calls themselves are shared. |
| UI chrome (legend, statistic picker, source panel) | Per-platform | Leptos / SwiftUI / Compose own their idiomatic UI; the data shown is identical because it's read from the same `core` queries. |

The per-platform shell is intentionally thin. A typical client per-platform layer is on the order of 1–2k LOC: a fetcher, a cache adapter, a render-surface bridge, and the UI tree. Anything beyond that should be re-evaluated as a candidate for promotion into `core`.

## Module layout (`core/` consumer surface)

The producer (`ingestion/`) writes manifests; the consumer (every client via `core/`) reads them. The consumer types live alongside the geometry / statistic types they wrap.

```
core/
├── src/
│   ├── lib.rs
│   ├── artifact/
│   │   ├── artifact.rs            # Bundle: open(manifest_bytes, cache_reader) -> Bundle
│   │   ├── artifact_model.rs      # Manifest, ManifestEntry, StatisticEntry, etc.
│   │   ├── manifest.rs            # parse_manifest(bytes) -> Manifest
│   │   └── verifier.rs            # verify_sha256(bytes, expected_hex)
│   ├── statistic/
│   │   ├── statistic.rs           # query the in-memory SQLite database
│   │   ├── statistic_model.rs     # StatisticValue, Series, etc. (shared with ingestion via core)
│   │   └── attach.rs              # ATTACH-DATABASE composition across license shards
│   ├── geometry/
│   │   ├── geometry.rs            # FlatGeobuf reader wiring; feature iteration
│   │   └── geometry_model.rs      # Feature, Polygon, BoundingBox
│   ├── license/
│   │   └── license.rs             # DistributionContext -> authorized BTreeSet<LicenseShardClass>
│   ├── projection.rs              # Miller cylindrical
│   ├── hit_test.rs                # spatial-index lookup
│   ├── render/                    # wgpu pipeline (shared across platforms)
│   └── ffi/
│       ├── wasm.rs                # wasm-bindgen surface (web)
│       └── uniffi.rs              # UniFFI surface (iOS, Android)
```

Per-feature module layout follows the Singularity `lobby/` triplet pattern; the consumer side has no `<feature>_db.rs` because there is no Postgres in the client, and no `<feature>_api.rs` because the client doesn't host HTTP routes. Where a feature needs an external-call abstraction in the future (a v3+ live correction-submission API), the `<feature>_client.rs` slot is reserved.

## Testing strategy

Per Constitution Principle VII, each TDD-required surface gets unit tests written before implementation. The client side has the following such surfaces:

- Manifest parsing: round-trip a known wire-format manifest through `parse_manifest` and assert every field. Reject malformed input with a typed `ManifestError`.
- SHA-256 verification: known input bytes → known hex digest; mismatch fails fast.
- License-class authorization: every `DistributionContext` variant returns the documented `BTreeSet<LicenseShardClass>`.
- Cache adapter contract: a per-platform integration test that does `cache.put(...) -> cache.get(...)` round-trips and asserts a missing key returns `None`. Web's version runs against IndexedDB in headless Chrome; iOS / Android run against the real device file system in their native test runner.
- SQLite ATTACH composition: build two trivial shards on the fly, attach both, assert a `select` unions correctly.
- FlatGeobuf hit testing: a feature collection with two known polygons; clicks at known points return the expected feature ids.

Live HTTP against the CDN is **not** part of automated tests; it's a manual smoke step run after each client deploy. The producer side already covers "the CDN serves what we think it serves" through `ingestion publish cloudflare-r2` + a curl check; duplicating that on the client side would add wall-clock time without catching a class of bug the producer side doesn't.

## Decisions still open

- **wgpu / WebGPU fallback policy.** WebGPU is stable in Chromium and Safari 18.4+; Firefox is on WebGL2 via the wgpu downlevel backend. The capability detection happens inside `wgpu::Instance::request_adapter`, so the client doesn't need its own logic — but the *UI fallback* (do we render a coarser version under WebGL2, or do we render the same version with a perf-warning banner?) is per-platform UX work. Defer to `client-web.md`.
- **Embedded-bundle build automation (native).** `ingestion build --downsampled` does not exist yet; today's `ingestion build` produces only the full artifact. To be added as a separate small PR on the producer side when native-client work begins. Each native build script then needs to invoke (or fetch the latest output of) the downsampled command and copy the result into its own asset directory.
- **Whether to ship a static-asset downsampled bundle on web.** Bundling a downsampled artifact alongside the wasm in `web/static/` would give first-ever visitors an instant render, at the cost of a larger initial download. Defer to the first web-client spec branch.
- **Translation table location.** Per overview §FFI, country / statistic / source-attribution display names are baked into the SQLite at build time, sourced from ISO 3166 + per-language overrides. v1 is English-only; the hooks need to exist for v2+. Whether the translation table is a separate SQLite shard or rolled into each statistic shard is open. Defer to the artifact-builder spec when i18n lands.
- **Embedded-third-party-widget distribution context.** The license matrix has a `EmbeddedWidget` slot but v1 has only one shard (`base`) which every context authorizes. The first source with stricter-than-WB license terms forces a real decision about which classes the widget context authorizes. Defer until that source lands in the canonical store.
- **Bundle hot-swap semantics for in-flight queries.** A user can issue a hover query at the exact moment the CDN fetch completes and the bundle is being replaced. Two safe strategies: (a) the renderer holds an `Arc<Bundle>` and the swap is an `Arc::store`, so an in-flight query reads the old bundle to completion; (b) the swap is done at a frame boundary, so no in-flight query exists at the moment of swap. (a) composes better with future async query work. Defer to implementation.
