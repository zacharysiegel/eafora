# Feature Specification: manifest schema backtracking

**Feature Branch**: `manifest-schema-backtracking`

**Created**: 2026-08-26

**Status**: Draft

**Input**: `docs/backlog.md` §Client, "Let a client fall back to an older repository version it can read", ordered immediately after the region detail dock at the owner's direction.

## Why a client cannot fall back today

The live path fetches exactly one remote document. `resolve_repository` at web/src/client/load.rs:153 requests `latest/manifest.json` and nothing else, and `open_fetched_live_bundle` at web/src/client/load.rs:194 parses it and returns an error if the parse fails. That error reaches one arm, at web/src/map/canvas/driver.rs:869, which logs it and sets the notice signal. The notice retires on a four-second timer (web/src/map/live_banner.rs:7) and nothing retries, so a client that cannot read the published manifest keeps painting from its cached or embedded bundle and never updates again, while looking identical to a healthy client.

Because `parse_manifest` gates on `manifest_schema_version` before deserializing anything (shared/src/artifact/manifest.rs:84), the specific failure is knowable and narrow: a manifest published at a schema version above the reader's own is rejected on the version field rather than on a field the shape changed. That makes every manifest schema bump deploy-order-sensitive, since the client has to reach every visitor before the producer publishes a bundle those visitors cannot read, and there is no way to enforce or verify that ordering.

Nothing on the destination enumerates published versions. Cloudflare R2 exposes no public bucket listing, the R2 repository deletes nothing, and the discovery document carries only a base URL, a minimum client version, and a sunset date (shared/src/artifact/discovery.rs:14-19), so a client cannot learn that an older readable version exists. What it does know is its own schema version. The mechanism below turns that into an address.

## The mechanism

Every publish writes the just-built manifest to a third key alongside the two it already writes: `latest/manifest.<N>.json`, where N is the producer's own compile-time `MANIFEST_SCHEMA_VERSION`. While N is current that key and `latest/manifest.json` hold identical bytes. When the constant bumps to N+1 the producer starts refreshing the pointer for N+1 and never touches N's again, so N's pointer is left holding the last manifest published at schema N. Nothing detects the bump, nothing reads the destination, and no key is ever deleted.

A client that finds `latest/manifest.json` reporting a schema version above its own constructs its own version's pointer key directly, fetches that one key, and hands the bytes to the existing load path. A manifest locates its own files from `manifest.version` inside the document (web/src/client/load.rs:215 into web/src/client/fetch.rs:71), so a copy served from `latest/` resolves `{base}/{version}/{relative_path}` exactly as the copy at `latest/manifest.json` does.

## Scenarios

### A client older than the published manifest

A visitor's client reads manifest schema 2. The producer has since bumped to schema 3 and published. The client fetches `latest/manifest.json`, finds schema 3, fetches `latest/manifest.2.json`, and loads the bundle that pointer names: the last complete bundle published while schema 2 was current. The map gains every period that bundle carries. The visitor sees no notice, because nothing failed.

### A client and repository at the same schema version

The overwhelmingly common case. `latest/manifest.json` reports the reader's own schema version, no pointer is constructed, no second request is made, and the load proceeds as it does today. The added cost on this path is zero requests.

### A publish

The producer uploads the shards, the geometry, and the versioned manifest, inserts the `artifact_version` row, uploads the schema pointer, and uploads `latest/manifest.json` last. Ordering the pointer before `latest/manifest.json` keeps `latest/manifest.json` the final object a publish writes, so a failure on the added upload leaves the destination coherent at the previous version.

### Edge cases

- The client's own schema version was never current during any publish, because two bumps landed inside one deploy window or the client was built from a tree that bumped ahead of the producer: the pointer 404s, the client logs a warning, and the original version-mismatch error surfaces. The outcome is the status quo plus one wasted request.
- `latest/manifest.json` reports a schema version below the reader's: no pointer is attempted. The repository needs republishing, and walking backward from a stale repository would waste a request on every start.
- `latest/manifest.json` reports the reader's own schema version but fails to deserialize on a field: no pointer is attempted. That is a producer bug, and serving older data forever would mask it.
- The response body is not JSON at all, because a CDN error page or a captive portal answered: no pointer is attempted, since the same path would answer the pointer request too.
- The pointer fetch itself fails on the network: the client reports the version mismatch, which is the actionable cause, rather than a transport error against a key the operator has never heard of.
- The pointer names a version directory the local repository has since pruned, because local retention keeps two (ingestion/src/artifact/repository/local_artifact_repository.rs:16): the manifest parses, a shard fetch 404s, and the existing notice shows. This is the dangling local pointer the owner has accepted; it needs no handling.
- The added upload fails mid-publish: the publish aborts before `latest/manifest.json` is overwritten, and the `artifact_version` row inserted earlier in the same function makes the duplicate-label guard at ingestion/src/artifact/publish.rs:33 refuse a retry until that row is deleted.
- Local retention runs after the added upload: the pointer lives inside `latest/`, which `read_version_directory_names` skips by name (local_artifact_repository.rs:80) and which is not a directory candidate in any case, so no number of publishes can prune it.
- Two publishes overlap: the later writer wins on both pointers. This race already exists on `latest/manifest.json`; the feature adds one more key to it.

## Requirements

### Phase A, the producer writes the pointer

- **FR-001**: The repository key for a manifest schema version MUST be derived in one place that both the producer and the consumer import, beside `MANIFEST_LATEST_KEY` at shared/src/artifact/manifest.rs:20, so the two sides cannot disagree on the string.
- **FR-002**: The key MUST be built by a function taking the schema version, not by a compile-time constant, so a test can assert the rendered key against a literal and keep pinning the wire contract after the constant bumps.
- **FR-003**: The key MUST sit under the existing `latest/` prefix. A new top-level directory would be enumerated as a version directory by local retention, would yield no creation timestamp, and would be deleted before any real version.
- **FR-004**: Every publish MUST upload the just-built manifest to the pointer key for the producer's own `MANIFEST_SCHEMA_VERSION`, as a byte-for-byte copy of that publish's manifest.
- **FR-005**: The pointer upload MUST reuse the local source path the two existing manifest uploads already use (ingestion/src/artifact/publish.rs:55 and :69). The producer holds a path and a digest rather than the bytes (shared/src/filesystem.rs:9-12 and :79-82), and no design here needs more than that.
- **FR-006**: The pointer upload MUST be placed immediately before the `latest/manifest.json` upload, so `latest/manifest.json` stays the last object a publish writes and the two pointers can disagree for at most one request.
- **FR-007**: The producer MUST NOT read, list, or conditionally write anything on the destination. `ArtifactRepository` stays `put_file` plus `url` (ingestion/src/artifact/repository/artifact_repository.rs:9-13).
- **FR-008**: A pointer MUST be written only for the producer's own current schema version, never for another version, and MUST never be rewritten after the constant bumps. Freezing at the bump is what leaves the pointer holding the last manifest published at that version.
- **FR-009**: Pointers MUST accumulate without bound: one key per schema bump, none expired, none pruned, no object-lifecycle rule. A bounded window would require enumerating or reading the destination, which FR-007 forbids.
- **FR-010**: A dry publish MUST log the added upload as it logs every other one (ingestion/src/artifact/repository/dry_artifact_repository.rs:15-20), so the step is visible in the cheapest pre-flight the pipeline has.
- **FR-011**: The pointer's presence and its byte-equality with the versioned manifest MUST be asserted in the publish integration test, with the key built from the same function the producer uses rather than from a literal.
- **FR-012**: The pointer's survival across local retention MUST be asserted in the retention test, pinning the property FR-003 relies on.

### Phase B, the client reads it

- **FR-013**: Reading the schema version out of a document MUST be a single function, extracted from `require_schema_version` at shared/src/artifact/schema_version.rs:12-27, so the gate and the fallback decision read the one field the format guarantees through the same code.
- **FR-014**: The extraction MUST NOT change any existing error message. The existing tests passing verbatim is the proof.
- **FR-015**: The field name `manifest_schema_version` MUST be a constant rather than a literal repeated at shared/src/artifact/manifest.rs:84 and at the new decision function, which have to agree on it.
- **FR-016**: The fallback decision MUST be a pure function over the fetched bytes, returning the reader's own pointer key when the found version is strictly greater than the reader's own and nothing otherwise. Equal, older, a missing field, and a body that is not JSON all decline.
- **FR-017**: The decision function MUST live in `shared` and MUST be covered by host tests as a truth table. Nothing on the async fetch path is host-testable today, so a decision left inside it is untested by construction.
- **FR-018**: The client MUST attempt exactly one pointer, at its own schema version, constructed directly. No walk backward and no enumeration, because R2 exposes no public bucket listing.
- **FR-019**: The fallback MUST branch inside `load_live_bundle` at web/src/client/load.rs:133-147, between `resolve_repository` and `open_fetched_live_bundle`. That is the only point where the resolved base URL and the un-parsed bytes are both in scope with no cache write yet performed.
- **FR-020**: The fallback MUST be strictly sequential, after the authoritative base is known. It MUST NOT join the concurrent fetch at web/src/client/load.rs:154-158, whose speculative arm is discarded whenever discovery names a different base.
- **FR-021**: `open_fetched_live_bundle` MUST be called unchanged with whichever bytes won, so the fallback inherits the version gate, the per-file digest verification, and the ordering that keeps a rejected manifest out of the cache.
- **FR-022**: A failed pointer fetch MUST fall through to the original bytes, so the error the operator reads is the version mismatch rather than a 404 on a key they have never heard of.
- **FR-023**: The pointer MUST be fetched with the cache mode the primary manifest fetch uses (web/src/client/fetch.rs:60). It is a pointer read, not an artifact body.
- **FR-024**: The manifest fetch MUST be generalized to take a key, with the existing `fetch_manifest` delegating to it, so the base-URL trimming and the cache-mode decision stay in one place.
- **FR-025**: The common path MUST stay at the single round trip `resolve_repository`'s doc comment commits to (web/src/client/load.rs:149-152). The extra request happens only when `latest/manifest.json` parses as too new.
- **FR-026**: The fallback decision MUST be logged at a level a release build keeps, so a client running on a frozen version leaves a record in the only place a static deploy has one.
- **FR-027**: The client half MUST be written so the loader's move into `shared` can carry it unchanged: the decision function already lives in `shared`, and what moves is a three-line branch plus one fetch signature.

### Key entities

- **Schema pointer**: one repository object at `latest/manifest.<N>.json`, holding a byte-for-byte copy of a published manifest. Refreshed by every publish while N is the producer's current schema version, frozen thereafter, never deleted.
- **Fallback decision**: the reader's answer to whether the document it fetched is too new for it, and if so which key to try. A pure function over the fetched bytes and the reader's own constant.

## Naming the key

The backlog records the owner's proposed shape as `latest/manifest.deprecated.<schema_version>.json`. This feature uses `latest/manifest.<N>.json` instead, and the reason is that the mechanism and the name have to be decided together rather than separately.

Under this mechanism the producer writes the key from its own compile-time constant, so `manifest.deprecated.2.json` is created on the first publish at schema 2, while 2 is current and while the object is byte-identical to `latest/manifest.json`. The name would be false for the entire supported life of the schema version, and an operator listing a bucket that deletes nothing would read "deprecated" as permission to remove the one object older clients depend on. The owner's spelling is honest under the copy-aside mechanism, which writes the key only once the version has actually been superseded; that mechanism is rejected in the plan for unrelated reasons, and renaming the key is the whole of what recovering its honesty costs.

Spellings considered and rejected:

- `manifest.v2.json`: `v` and `version` already mean the artifact version label throughout this codebase (`Manifest.version`, the version directories, `version_label`), so `v2` invites reading "artifact version 2".
- `latest/schema-2/manifest.json`: works mechanically, since local `put_file` creates parent directories and retention never enumerates inside `latest/`, but it puts the qualifier before the noun against the most-significant-noun-first rule and hides the pointers from a flat listing of `latest/`.
- `manifest-schema-2.json`: breaks the `manifest.<qualifier>.json` dotted-suffix shape, so it reads as an unrelated filename rather than as a variant of `manifest.json`.
- `manifest.schema-2.json` and `manifest.schema-version-2.json`: both name the field the number comes from, which the surrounding key does not need, since `latest/` holds nothing else a bare integer could be counting.
- `manifest.deprecated.<N>.json`, the shape first proposed: false while N is the schema version being published, which under this mechanism is the whole of the pointer's active life.

The chosen spelling states a fact about the bytes that is true the instant they are written and never becomes false, sorts adjacent to `manifest.json` in any listing, and keeps `.json` last.

## Success criteria

- **SC-001**: A publish into an empty destination leaves the schema pointer present and byte-identical to both the versioned manifest and `latest/manifest.json`.
- **SC-002**: The schema pointer is still present after enough publishes to prune every version directory but the two newest.
- **SC-003**: A dry publish writes no files and still reports the pointer upload in its output.
- **SC-004**: The pointer key rendered for schema 2 is `latest/manifest.2.json`, asserted against that literal.
- **SC-005**: The fallback decision returns the reader's own pointer key for a document one schema version above the reader, and returns nothing for a document at the reader's version, a document below it, a document missing the field, and a body that is not JSON.
- **SC-006**: Extracting the version read changes no error message: the existing schema-version and manifest tests pass unmodified.
- **SC-007**: A client whose `latest/manifest.json` is one schema version too new loads the bundle its own pointer names, and shows no notice.
- **SC-008**: A client whose pointer does not exist reports the version mismatch, not a 404, and its exit behaviour is what it is today.
- **SC-009**: A client at the repository's schema version issues no second manifest request.

## Assumptions

- The manifest schema version is a single compile-time constant shared by the producer and every client (shared/src/artifact/manifest.rs:13), and `artifact_version` records no schema version, so a producer can only ever write a pointer for its own current version. The frozen-at-last-publish property follows from the constant changing rather than from any code that notices it changed.
- Only the complete variant is published to a repository; the downsampled tree is the embedded first-paint bundle. A pointer therefore concerns the complete manifest.
- Two byte-identical objects under `latest/` while a schema version is current is acceptable. A manifest is a few kilobytes, and R2's free tier is far from binding.
- A dangling pointer in the local repository is accepted and needs no handling, per the owner's decision recorded in the backlog. In practice it means local development cannot demonstrate a successful fallback unless fewer than two publishes have followed the frozen one.
- Pre-launch, there is no compatibility burden. Cache invalidation and schema bumps are not arguments against this design or against changing it later.

## Scope cutoff

Out of scope, each for a stated reason:

- **`minimum_client_version` gating.** The field stays parsed and unread at shared/src/artifact/discovery.rs:17. This feature is the read half, where an old client pulls what it can read; pushing a client to upgrade is the other half of the same problem and is its own feature.
- **Any repository read capability.** `ArtifactRepository` keeps `put_file` and `url`. A read would cost a trait function across a trait declaration, three implementations, and a hand-written enum dispatcher, and the dry repository has nothing it could return.
- **Pointer expiry or a bounded retention window.** Settled in favour of unbounded retention, because bounding it would require enumerating or reading the destination.
- **A `manifest_schema_version` column on `artifact_version`.** Nothing here needs it, so the database still cannot answer which schema version a published version carries.
- **Client pinning and any persisted client state.** A client that landed on a pointer retries `latest/manifest.json` first on every start, which costs one request and recovers automatically once the client is upgraded. This settles the backlog's other deferred sub-decision.
- **Walking backward past one pointer.** Exactly one key is attempted, at the reader's own version. Enumeration is unavailable and a loop would need a version list this design refuses to obtain.
- **A fallback when the repository is behind the client.** The remedy is to republish. Walking backward there would waste a request on every genuinely stale repository. It is a symmetric one-line change to the decision function if it ever matters.
- **A fallback on a same-version manifest that fails on a field, or on a non-JSON body.** Both are producer or transport bugs rather than schema skew, and must surface loudly rather than be masked by serving older data forever.
- **Any interface change.** No new notice string and no distinct message for a successful fallback. The existing four-second banner is untouched, and it would state something untrue about a fallback that succeeded.
- **A rank guard on the bundle hot swap** at web/src/map/canvas/driver.rs:876-880. It is not needed for correctness: the pointer holds the newest version ever published at the reader's schema version, and anything the reader has cached and can read was published at that schema version and so no later. The wasteful identical-version re-open this leaves in place is a pre-existing defect and belongs to its own change.
- **`version_rank.rs` and `evict_stale_versions`.** They rank cached versions and play no part in remote manifest selection. A fallback manifest caches under the version it names and is indistinguishable from having been fetched while that version was current.
- **The concurrent fetch in `resolve_repository` and the discovery reconciliation in `live_resolve.rs`.** The fallback consumes the base they produce.
- **Schema skew outside the manifest document.** A shard SQLite schema change, a geometry format change, and an unknown statistic or licence-class code all still break a client, and they fail after the manifest parses, where no fallback exists.
- **The concurrent-publish race.** Two overlapping publishes already race on `latest/manifest.json`. This adds one more key to that race rather than introducing or resolving it.

## Constitution check

- **Principle V, explicit over implicit**: directly served. The pointer key is a literal string built by one function that both sides import, the fallback is one visible branch at one call site, and there is no interception, retry policy, or middleware anywhere in it.
- **Principle VI, CDN-delivered data**: unchanged in shape. One more immutable object per schema bump on the same destination, fetched by the same client over the same HTTP path.
- **Principle VII, test-first**: the key derivation, the version read, and the fallback decision are error mapping and artifact selection, which the principle names. All three are pure functions with host tests written before the implementation. The three-line async branch and the added upload are covered by the publish integration test and by a manual browser check the plan states plainly.
- **Principle III, Rust core**: the decision lives in `shared`, so the iOS client inherits it when the loader moves rather than reimplementing it.
- **Principle II, source provenance**: unaffected. A fallback bundle carries the same provenance the bundle carried when it was current, since it is the same bytes.
- **Principle IV, Singularity convention parity**: no new dependency. The added function is `format!` and the added upload reuses the existing trait.
