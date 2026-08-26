# Implementation Plan: manifest schema backtracking

**Branch**: `manifest-schema-backtracking` | **Date**: 2026-08-26 | **Spec**: [spec.md](spec.md)

## Summary

Every publish writes the just-built manifest to one extra key, `latest/manifest.schema-<N>.json`, where N is the producer's own compile-time `MANIFEST_SCHEMA_VERSION`. Nothing detects a bump: the producer refreshes only its own version's pointer, so when the constant moves to N+1 the pointer for N stops being refreshed and is left holding the last manifest published at schema N. A client that finds `latest/manifest.json` reporting a schema version above its own builds its own version's key directly, fetches that one object, and hands the bytes to the existing load path unchanged.

The whole producer half is one `put_file` call reusing a source path the manifest is already uploaded from twice. The whole client half is one pure decision function in `shared`, one generalized fetch signature, and a three-line branch at one call site. There is no new trait surface, no new dependency, no schema column, and no interface change.

Three PRs on a linear stack, plus one prerequisite off `master`.

- **Prerequisite.** Repair ingestion/tests/publish_integration.rs, which does not compile on `master`. Own PR off `master`, because the breakage predates this feature and the only test target that can prove Phase A is dead until it lands.
- **A, the producer writes the pointer.** The key builder in `shared`, the added upload in `publish_artifacts`, and the two publish integration assertions.
- **B, the client reads it.** The version read extracted, the fallback decision, the generalized manifest fetch, and the branch in `load_live_bundle`.

Affected repositories: this monorepo only (`/Users/singularity/eafora`).

## The prerequisite is not optional

`cargo check -p ingestion --tests` fails on a clean `master`:

```text
error[E0308]: mismatched types
   --> ingestion/tests/publish_integration.rs:237:9
    |
237 |         &data_source_revisions,
    |         ^^^^^^^^^^^^^^^^^^^^^^ expected `&BundleProvenance`, found `&BTreeMap<DataSourceKind, SourceRevision>`
```

`write_manifest` takes a `&BundleProvenance` (ingestion/src/artifact/writer/manifest.rs:17) and the call site at publish_integration.rs:232-240 still passes the revision map. The whole `publish_integration` target therefore does not build, which means the five publish behaviours it asserts are unguarded on `master` and Phase A cannot demonstrate a passing test. Repair it first, in its own PR, so the fix is reviewable as the pre-existing-breakage fix it is rather than as noise inside a feature diff.

Note that specs/007-hfd-ingestion/tasks.md records this same file being repaired once before, for a parameter added by `c1f85b8` (eafora). It has broken again for the same reason. The target needs a live Postgres at `TEST_DATABASE_URL` and cannot run under `SQLX_OFFLINE`, because the test's own `delete from artifact_version` query is absent from the committed `.sqlx` cache; that is why nothing catches the regression.

## Decisions

### The mechanism is write-current-per-publish, not copy-aside-on-bump

The alternative considered was for the producer to read `latest/manifest.json` back off the destination immediately before overwriting it, classify the schema version it finds, and copy those bytes aside under the outgoing version's key whenever the version differs. It was the owner's implicit shape in the backlog, and it is rejected.

The client halves of the two designs are identical: the same pure decision function in `shared`, the same branch point, the same one extra round trip on the same condition, the same inherited parse-before-cache-write ordering. So the choice turns entirely on the producer half, and there the gap is not close. Copy-aside needs `read_bytes` and `put_bytes` on `ArtifactRepository`: a trait declaration, three implementations, and a hand-written enum dispatcher, which is roughly ten new items across a file that is 37 lines long today and has no read path anywhere in it. That capability is used by nothing else in the repository, and it is the wrong shape for the future uses that would justify it, since resumable publish and integrity sweeps want a HEAD and an ETag while a republish wants a stream rather than a `Vec<u8>`. It also widens the R2 publish credential from write-only to read-and-write, which is a real privilege expansion for a token that currently cannot read the bucket at all.

Copy-aside also puts a never-exercised `GetObject` on the critical path after the `artifact_version` row commits, and its own analysis correctly insists a read error must abort rather than fall through, since falling through would destroy the only copy of the outgoing manifest. Write-current-per-publish adds a step there too, but it is the same operation, with the same credential, on the same source path that has already succeeded twice earlier in the same function.

Copy-aside has two genuine advantages, and neither survives being priced.

- Its key name is honest, because the object appears only once the version has been superseded. That is a naming problem, and renaming the key costs nothing, so the honesty is fully obtainable inside the cheaper mechanism. The spec's naming section is that rename.
- It covers producer rollback: a producer rolled back from schema 3 to 2 writes the pointer for 3, which a schema-3 client can still use, and a design keyed on its own constant structurally cannot do that. This coverage is declined. A schema-3 client in that state would be pinned to frozen data while fresh data flows at a version it cannot read, which is not obviously better than the notice it gets today; the backlog frames the problem in one direction only; and it is a symmetric one-line change to the decision function later if it ever bites.

Two of copy-aside's claims are weaker than they read. Its "the key set is the audit" argument does not hold, because under the chosen design the current version's pointer is identifiable as the one byte-identical to `latest/manifest.json` and the deployed producer's constant answers the question directly. And its `--dry` behaviour is worse: a dry repository has no state, so its read must answer "nothing there" and the branch becomes permanently invisible to the cheapest pre-flight the pipeline has, whereas an extra upload shows up in dry output for free through the existing log at dry_artifact_repository.rs:15-20.

### The two points that could have sunk the chosen mechanism, checked

The pointer key sits inside `latest/`, and `retain_newest_versions` enumerates only immediate subdirectories and skips the one named `LATEST_POINTER` (local_artifact_repository.rs:61-88 and :50-54), so a file inside `latest/` is never a pruning candidate under any number of publishes. And the local `put_file` creates the parent directory before copying (local_artifact_repository.rs:110-118), so the key needs no directory setup on any implementation.

### The added upload goes before the latest-pointer upload

Place it immediately before ingestion/src/artifact/publish.rs:69, not after, so `latest/manifest.json` remains the last object a publish writes. A failure on the added upload then aborts with the destination still coherent at the previous version, and in the success case the two pointers can disagree for at most one request.

### `require_schema_version` does not gain a typed outcome

`describe_mismatch` (shared/src/artifact/schema_version.rs:31-39) already decides older-versus-newer and then formats it into prose. The temptation is to return that decision typed. Declining: making `require_schema_version` return a typed mismatch widens `parse_manifest`'s error type and ripples to the discovery caller, three sites in load.rs, publish.rs:86, and local_artifact_repository.rs:100, for a direction needed at exactly one call site. The decision function recomputes it from the bytes instead, which costs one `serde_json::Value` parse of a few kilobytes off the first-paint path.

### The key is a function, not a `formatcp!` constant

Both production call sites pass the compile-time constant, so a constant would work. A function is chosen so a test can assert `schema_pointer_key(2)` against the literal `"latest/manifest.schema-2.json"` and keep pinning the wire contract after the constant bumps. A constant could only be compared against itself. The parameter is `u32`, matching `MANIFEST_SCHEMA_VERSION` at shared/src/artifact/manifest.rs:13; choosing this mechanism makes the u32-versus-u64 question moot, because the key is only ever built from the caller's own constant and never from a version read out of a foreign document.

## Module layout

```text
shared/src/artifact/
├── manifest.rs         # + schema_pointer_key, + schema_fallback_key, + the field-name const
└── schema_version.rs   # + read_schema_version, extracted out of require_schema_version

ingestion/src/artifact/
└── publish.rs          # + one put_file before the latest-pointer put_file

web/src/client/
├── fetch.rs            # fetch_manifest generalized to take a key
└── load.rs             # + a three-line branch in load_live_bundle
```

Nothing is created and nothing moves. `shared/src/artifact/mod.rs` re-exports each submodule with a wildcard, so the two new functions are reachable as `shared::artifact::*` with no list to update.

## Phasing and the loader's move into `shared`

specs/004-ios-client/plan.md §Phasing for PRs, phase 0.2, moves web/src/client/load.rs, web/src/live_resolve.rs, and web/src/version_rank.rs into `shared/src/artifact/`, parameterized over `ArtifactCache` and a new `HttpFetch` trait, with live_resolve.rs renamed to discovery_resolve.rs. Both features touch load.rs, so the order matters.

This feature first is cheaper, and by a wide margin. Its decision function already lives in `shared`, which 0.2 does not move, so what 0.2 has to carry across is a three-line branch inside a function it is rewriting anyway plus one generalized signature in fetch.rs, which 0.2 is rewriting as an `HttpFetch` implementation regardless. The reverse order costs more: 0.2 is a large refactor blocked on nothing here, this feature is in progress now per docs/task-order.md, and holding it behind 0.2 buys nothing except that the branch is written against the final signatures. The one honest cost of going first is that the branch is written to move unchanged and no test verifies that claim; the mitigation is to keep the branch to a `match` over the decision function's `Option` with no local state, which is what makes it mechanical to re-parameterize.

Do not quote 0.2's line counts. plan.md:67-69 and :124 and docs/task-order.md say 374 / 94 / 94 for a total of 562; the actual counts are 380 / 94 / 71 for a total of 545. Fixing those numbers is not this feature's job.

## Failure modes

| Situation                                              | Outcome                                                |
| ------------------------------------------------------ | ------------------------------------------------------ |
| `latest/manifest.json` is newer than the client        | The client's own pointer is fetched, its bundle loads  |
| The client is newer than every pointer ever written    | 404 on the pointer, then the version-mismatch error    |
| No pointer exists at all (the bump predates this)      | 404 on the pointer, then the version-mismatch error    |
| The pointer fetch fails on the network                 | The version-mismatch error, not the transport one      |
| `latest/manifest.json` predates the client             | No pointer attempted, unchanged behaviour              |
| Same version, a field-level deserialization failure    | No pointer attempted, the field error surfaces         |
| The body is not JSON (CDN error page, captive portal)  | No pointer attempted, unchanged behaviour              |
| The pointer's version directory was pruned locally     | The manifest parses, a shard 404s, the notice shows    |
| The added upload fails mid-publish                     | Publish aborts before `latest/manifest.json` is put    |
| A client reads between the two pointer uploads         | Harmless in either ordering, both name a real version  |

The first four rows are the ones worth stating explicitly, because they are the four a reader will ask about and three of them look alike from the outside. What separates them is where the error comes from: only the first is a success, and the other three all end at the version-mismatch error that `latest/manifest.json` already produced, because a failed pointer fetch falls through to the original bytes rather than reporting itself.

## Risks

- **Coverage is silently conditional on a coincidence.** A client generation is protected only if at least one publish happened while its `MANIFEST_SCHEMA_VERSION` was current. A double bump inside one deploy window, or a client built from a tree that bumped ahead of the producer, yields a 404, a warning, and the status quo plus one wasted request. No test can catch this, because the producer and the client read the same constant.
- **It converts a visible failure into permanent silence.** A stuck client now succeeds, so the notice never fires, and a population pinned to a frozen version is indistinguishable from a healthy one. The only in-scope mitigation is an `info` log at the fallback decision, which a release build keeps per commit 442b02d (eafora). The real fix is `minimum_client_version`, deliberately out of scope.
- **It buys "keeps working", not "keeps updating".** The pointer stops being refreshed at the bump, so a stuck client is pinned to the last data published at its schema version forever, with no signal on either side.
- **It fixes manifest-document skew only.** This must not be described as making schema bumps deploy-order-insensitive in general. A shard SQLite schema change, a geometry format change, or a new `StatisticKind` or `LicenseShardClass` code still breaks a client, and those fail after the manifest parses, inside `Bundle::open`, where no fallback exists and the fallback manifest has already been written into the cache.
- **Steady state for a stuck client is wasteful on every start.** The fallback fetches the manifest it already has cached, every shard short-circuits as already cached, and `apply_live_bundle` (web/src/map/canvas/driver.rs:876-880) republishes an identical bundle after a full re-open of every shard out of the cache. This is a pre-existing defect on the healthy path too, and this feature makes it permanent for the clients it serves.
- **Two byte-identical objects invite a cleanup.** While a schema version is current, `latest/` holds the pointer and `latest/manifest.json` with identical bytes, and they diverge only after a bump. Nothing prevents a future reader from diagnosing that as redundancy and deleting the pointer, breaking exactly the clients it exists for. The `schema-<N>` name reduces that risk relative to `deprecated`; it does not eliminate it.
- **No rollback coverage.** A producer rolled back to an older schema version leaves newer-schema clients with no fallback, because the client only ever constructs the key for its own version and the decision function correctly declines to walk backward when the repository is behind the reader.
- **The central end-to-end claim is provable by no automated test here.** See the test plan's last item; it is a manual check and is stated as one.
- **The added upload lands after the `artifact_version` insert commits.** A failure there aborts a publish whose row already exists, and the duplicate-label guard at ingestion/src/artifact/publish.rs:33 refuses the retry until that row is deleted by hand. The risk is small, being the same operation and credential and source path that already succeeded twice in the same function, but it is not zero.
- **Two functions are added to a file that phase 0.2 is about to rewrite the callers of.** The branch is written to move unchanged, and no test verifies that claim.

## Test plan

Host unit tests in `shared`, written before the implementation per Constitution Principle VII, run with `cargo test -p shared`:

- `read_schema_version` returns the found value for a document whose version does not match, and errors for a missing field and for a field that is not an integer.
- The five existing `require_schema_version` and manifest version tests pass verbatim, which is the proof the extraction changed no message. shared/src/artifact/schema_version.rs:53-59 asserts a full message string and shared/src/artifact/manifest.rs:174-184 asserts the newer-build wording; neither may be edited.
- `schema_pointer_key(2)` equals `"latest/manifest.schema-2.json"`, and `schema_pointer_key(11)` ends in `schema-11.json`. Literals rather than the constant, so the assertion keeps pinning the wire contract after a bump instead of decaying into a tautology.
- `schema_fallback_key` as a truth table over `valid_manifest_json()` (shared/src/artifact/manifest.rs:125-139) mutated by `replace`: a newer version yields the reader's own pointer key, and equal, older, a missing field, and a body that is not JSON all yield nothing. Write the `replace` needle by interpolating `MANIFEST_SCHEMA_VERSION` the way manifest.rs:177 already does, not by hardcoding `2`.
- Repair `parse_manifest_ignores_unknown_fields` (shared/src/artifact/manifest.rs:208-217) in the same commit. Its needle is `"manifest_schema_version": 1,` while the fixture emits `2`, so no substitution happens, no unknown field is ever inserted, and the test re-parses the untouched valid fixture and passes without exercising anything. The correct pattern is the sibling at shared/src/artifact/discovery.rs:73-85, which declares a separate literal carrying the extra field.

Integration tests in `ingestion`, extending the existing publish target rather than adding a third publish, run with `cargo test -p ingestion --test publish_integration`. Requires a live Postgres at `TEST_DATABASE_URL`; the target cannot run under `SQLX_OFFLINE`. Uuid-suffixed version labels and an explicit `delete_artifact_version` teardown per test, per that file's own conventions (publish_integration.rs:1-6, :189-191, :254-259), because `artifact_version` inserts commit through the pool and rollback isolation is unavailable.

- Extend `publish_artifacts_uploads_every_file_to_local_repository_and_inserts_artifact_version` (publish_integration.rs:33) to assert the pointer exists at `destination_dir.path().join(manifest::schema_pointer_key(manifest::MANIFEST_SCHEMA_VERSION))` and is byte-equal to both the versioned manifest and `latest/manifest.json`. The key is built from the same function the producer used, never from a literal, matching the existing idiom at publish_integration.rs:61-63.
- Extend `publish_local_keeps_only_the_two_newest_version_directories` (publish_integration.rs:96) to assert the pointer still exists after retention, pinning the property that a key inside `latest/` is exempt from pruning.
- Both assertions must be written and observed failing before the `publish.rs` change, then passing after.

No `#[wasm_bindgen_test]` anywhere in this feature. Nothing on the fallback path diverges between wasm32 and the host, and the web crate's browser cases are collected by no runner today: scripts/test/test-wasm.sh:22-24 changes into `shared/` before invoking `wasm-pack`. The client half is covered by the host tests over the decision function plus `cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown`.

The end-to-end claim is not covered by any automated test, and that is stated rather than implied. A client actually loading a bundle from a pointer cannot be proved here: the web crate has no runnable browser harness, its two tests at web/src/client/load.rs:325 and :350 carry no host test attribute and are additionally stale (their fixture declares schema 1 and omits the now-required `variant` and `source_attribution`, so `parse_manifest` rejects it and every version ranks identically, which is the ordering those tests claim to disprove); the ingestion tests have no HTTP client; and the local tree prunes to two versions, so a frozen pointer's shards disappear after two further publishes. The manual check is:

1. Publish once into the local static tree with `scripts/build/publish-web-local.sh`.
2. Edit the tree's `latest/manifest.json` to carry a `manifest_schema_version` one above the constant, leaving the pointer alone.
3. Load the app under `cargo leptos watch` and confirm from the console that the pointer key is requested and that the bundle loads with no notice.

Keep the publish count within the two versions local retention keeps, or the shards the pointer names will be gone. Whether to repair or delete the two stale load.rs tests, and whether to extend the wasm runner to the web crate, are separate questions and not in this feature.

## PR description, the prerequisite

Repairs the `publish_integration` test target, which has not compiled since `write_manifest` took a `BundleProvenance` in place of the revision map.

The target asserts five publish behaviours, including the byte-equality of `latest/manifest.json` with the versioned manifest, and none of them has been guarded while it was broken. The target needs a live Postgres and cannot run under `SQLX_OFFLINE`, which is why nothing caught the regression.

## PR description, Phase A

Publishes a stable manifest pointer per manifest schema version, so a client too old to read `latest/manifest.json` has an address it can read.

Every publish now uploads the just-built manifest to `latest/manifest.schema-<N>.json` for the producer's own schema version, immediately before `latest/manifest.json`. When the constant bumps, the producer starts refreshing the new version's pointer and leaves the old one holding the last manifest published at that version, so no code has to detect the bump. The key is derived in `shared` beside `latest/manifest.json`'s, both sides import it from there, and the repository trait is unchanged: this is one more upload of a file the manifest is already uploaded from twice.

## PR description, Phase B

Lets a client that cannot read the published manifest load the newest bundle it can read, instead of never updating again.

When `latest/manifest.json` reports a manifest schema version above the reader's own, the client fetches the pointer for its own version and loads that bundle through the unchanged load path, at a cost of one extra request and only on that condition. A repository at the reader's version, a repository behind the reader, a document that fails on a field, and a body that is not JSON are all left alone, so a producer bug still surfaces rather than being masked by older data. Reading the schema version out of a document is now one function shared by the gate and the decision.

## Deviations

To be recorded in [tasks.md](tasks.md) §Deviations as the phases land.
