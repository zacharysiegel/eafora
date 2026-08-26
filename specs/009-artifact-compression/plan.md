# Implementation Plan: artifact compression

**Branch**: `artifact-compression` | **Date**: 2026-08-26 | **Spec**: [spec.md](spec.md)

## Summary

The producer compresses each shard and the geometry once at brotli quality 11, before the content-addressing digest is taken, and writes the compressed file to disk under a `.br`-suffixed name. The manifest's `sha256` and `size_bytes` therefore keep describing the file as served, so no manifest field changes meaning and none is added. The reader decodes inside `Bundle::open`, at the two lines where a verified buffer becomes a parsed object.

The consumer half is two decode calls in `shared`. Nothing under `web/src` changes at all: `ManifestEntry` stays at three fields, `validate_entry` stays as written, `MANIFEST_SCHEMA_VERSION` stays 2, `filesystem::verify_sha256` and all four of its call sites keep hashing exactly the bytes they hash today, and `is_already_cached`'s per-file corruption repair keeps its current meaning. One digest suffices because brotli decoding is deterministic, so a bit-exact compressed stream implies bit-exact decoded bytes.

The producer half is larger than the consumer half: a compress-and-hash step fused so both manifest numbers come from one pass, a rewrite of `build_hashed_path` to survive a double extension, an explicit flush in the geometry writer, and two collapsed content-type constants. Two build scripts change because a compressed embedded shard breaks `verify-site-tree.sh` and inverts `measure-site-budget.sh`'s premise under any naming choice.

One PR. The producer half alone changes what is published in a form no client can read, and the consumer half alone cannot read what is published, so they must ship together; a split diff would leave the trunk in a state where the site build fails and the client cannot open a bundle. The whole change is approximately 200 lines of production code plus two shell scripts.

`brotli` is one new third-party dependency. The repository's rule is to ask before adopting one, and the owner approved this one explicitly.

Affected repositories: this monorepo only (`/Users/singularity/eafora`).

## Decisions

### The decode goes inside `Bundle::open`, not at cache-write time

The alternative considered was to decode on the way into the OPFS cache: fetch the compressed artifact, verify the transferred bytes against the manifest's `sha256`, decode, verify the decoded buffer against a second manifest digest, and store the decoded form. The cache would then hold plain `.fgb` and SQLite images and everything downstream of the cache would be untouched. It is rejected.

The readers that have to change decide it. Decode-on-open changes two lines, both in `shared`. Decode-on-cache-write needs `size_bytes_decompressed` and `sha256_decompressed` on `ManifestEntry`, a wider `validate_entry`, a manifest schema bump to 3, both `verify_sha256` calls in `Bundle::open` re-pointed at the new digest, `is_already_cached` re-pointed, and a decode call inserted at two web load sites. That is a cost in the one document every platform parses, paid to avoid a cost in CPU.

And the CPU cost it avoids does not exist. This is the criterion that naively favours cache-write and it does not survive the arithmetic. Every artifact is hashed three times per warm session on the code as it stands: `Bundle::open` on the cached version (web/src/map/canvas/driver.rs:754), `is_already_cached` on the live path (web/src/client/load.rs:309), and `Bundle::open` on the live version (web/src/client/load.rs:229). At the surveyed rates, hashing 712 KB three times instead of 4.37 MB saves 18.35 ms while two decodes cost 15.68 ms, so a warm session goes 21.91 ms today to 19.25 ms with a compressed cache. Decode-on-cache-write leaves all three hash passes over 4.37 MB, so it stays at 21.91 ms and additionally hashes the decompressed bytes on a cold visit. On this codebase the compressed cache is the cheaper start, not the more expensive one.

Cache-write would only win if `Bundle::open` stopped verifying cached bytes, which is a defensible change on its own terms, since the fetch path already verified them. It is not on the table here, because it would give up the truncation repair that web/src/client/load.rs:300-302 and :71-81 are built around. If someone makes that change later, the arithmetic above inverts and the placement should be re-examined rather than defended.

At first paint the two are equal on a cold visit: both decode the embedded geometry before `Renderer::new` (web/src/map/canvas/driver.rs:785) can be constructed, and cache-write is marginally worse there because it hashes the decompressed bytes as well.

Surviving the move into `shared` for the iOS client is the deciding consideration. Decode-on-open is already in `shared`, inside the function every platform calls, so iOS and Android get it on day one and no platform loader can forget it. Cache-write puts the rule in the platform load layer with nothing in the type system enforcing it, and edits web/src/client/load.rs at exactly the lines `004-ios-client` phase 0.2 is moving into `shared` (docs/task-order.md item 1, in progress).

Cache-write's remaining cost is storage: 8,749,996 bytes against 1,425,486 for identical data across the two retained versions, approx. 6.1x. That is small against real browser quotas and is not why it is rejected.

A hybrid was not considered seriously. A compressed cache with a decompressed-bytes digest recorded for audit, or per-entry codec selection, takes the manifest cost of one design and the recurring decode of the other.

### Compressing the embedded artifacts is not carved out

The embedded geometry is 1,576,240 of the 1,646,356-byte first paint, 96% of it, so an uncompressed-embedded exception would leave first paint at 82% of cap while the client still shipped the brotli decoder for the live path. That is the worst of both. The exception would also cost the producer either a second encode or a decompress-before-copy step, because the embedded bundle is written by copying the complete variant's geometry byte-for-byte (`copy_geometry_into`, ingestion/src/artifact/artifact.rs:234-247).

The price is a brotli decode of 469,994 to 1,576,240 bytes sitting between the user and the first frame, unconditionally, on every load. That is 4.94 ms on the host and an estimated 10 to 15 ms in wasm, partly offset by 1.95 ms of hashing no longer done over the larger buffer. Against 1.17 MB less to transfer, which is approx. 940 ms at 10 Mbps, the decode is dominated on any real connection and is roughly a wash only on a local-loopback fetch, on a startup path already dominated by feature decode, earcut triangulation, and a full SQLite scan.

The decode is safe on this path for a specific reason: it runs on a digest-verified, fully materialized buffer and errors before anything is cached or drawn, so a bad embedded artifact degrades to `StartupError::DataUnavailable` rather than a half-drawn map.

### The name gains `.br` and the inner extension stays

Settled in spec.md §File naming. The one thing the plan adds is that the producer fix this forces is wanted anyway: `build_hashed_path` (ingestion/src/artifact/hashing.rs:52-70) splits the extension with `rsplit_once('.')` at :56-58 and rejoins one extension at :69, so it would silently drop `.fgb` from `.fgb.br` rather than failing. A silent name corruption in the content-addressing step is worth a rewrite and a test on its own merits, independently of what this feature needs from it.

### The content-type constants collapse rather than widen

`CONTENT_TYPE_FLATGEOBUF` is already `application/octet-stream` and `CONTENT_TYPE_SQLITE` is `application/vnd.sqlite3` (shared/src/artifact/bundle.rs:17-18), which a brotli-wrapped SQLite file is not. Collapsing both into one `application/octet-stream` constant is chosen over adding a header parameter, because `ArtifactRepository::put_file` (ingestion/src/artifact/repository/artifact_repository.rs:10) is the only upload interface and `ArtifactRepositoryKind` fans it out to three implementations plus the trait. Any per-object header beyond Content-Type would change the signature and all three implementations, and the header that would be honest here is exactly the one FR-016 forbids.

### Both manifest numbers come from one pass

Today `size_bytes` is a `fs::metadata` stat taken by the writer (ingestion/src/artifact/writer/flatgeobuf.rs:134, writer/sqlite.rs:80) and `sha256` is a later re-read from disk (`hashing.rs:20` and :31). Two independent observations of one file, and for the geometry they disagree: the shipped manifest declares 1,573,368 bytes against 1,576,240 on disk, short by 2,872, because the stat is taken while a `BufWriter` still holds the tail.

That is inert today, because nothing reads `size_bytes`. Decision 2 makes it the transferred size, so it becomes load-bearing and the fix belongs in this feature rather than in a precursor commit. The compress step is where both numbers should be derived: read the plain temporary file once, stream it through the encoder while teeing the output into a `Sha256` and a byte counter, and return a `Hashed<FileReference>` whose byte count and digest both describe the bytes just written. `filesystem::load_hashed_file` (shared/src/filesystem.rs:139) already derives both from one read and is the precedent.

The flush at the geometry writer is not cosmetic once this lands. Today an unflushed tail is a wrong number over an intact file. Once nothing stats the file and the compressor reads it, the same omission yields a truncated `.fgb` that compresses, hashes, publishes, and verifies self-consistently, which is strictly worse. Hence FR-019 plus a test that re-parses the published geometry and counts features.

### The build gate decodes with the `brotli` CLI rather than gaining a Rust subcommand

`scripts/build/verify-site-tree.sh:82` shells out to `sqlite3` for `pragma user_version`, compared against `SCHEMA_VERSION` grepped out of shared/src/sqlite/schema.rs at :73. The alternative was an `ingestion verify-shard` subcommand running `validate_shard_header` on decoded bytes, which would give the check one implementation instead of two.

Declined, because it couples the deploy gate to a built Rust binary. Decoding each match with `brotli -d` into a `mktemp` and keeping the existing pragma check is smaller, and `brotli` is already a documented system dependency (docs/system-dependencies.md:87-88). What changes is that entry's severity: it currently says the perf-budget report needs it, and now the deploy gate does.

Keeping `.sqlite` is not viable either way. `sqlite3` exits 26 on a brotli stream, and under the script's `set -euo pipefail` the command substitution propagates that status, so the build dies on a raw driver error before reaching the `fail` at :84 that names `sync-embedded-bundle.sh`.

### `measure-site-budget.sh` keeps its logic and loses its premise

`is_compressed_in_transit` (scripts/build/measure-site-budget.sh:142) already returns false for the artifact extensions, so `transfer_size_of` falls to `wc -c` at :161, which becomes the correct answer once the file on disk is the file as served. The code barely moves. What must move is the `! -name '*.br'` filter at :214, which exists to avoid double-counting a precompressed sibling and would now exclude the artifacts themselves and report first paint as 484 bytes; and the stated premise at :10-14, :138-141, and :528-530, all of which assert that the geometry and the statistic shards transfer whole.

Worth stating in the rewrite: the artifact figures stop being an estimate. They become the producer's own encoder output rather than a C-CLI approximation of it, and the two never agreed, the crate's quality-11 output being 0.35% larger for the geometry and 1.81% larger for a shard.

## Module layout

```text
shared/src/artifact/
├── compression.rs      # new: compress, decompress, the .br extension constant
├── mod.rs              # + pub mod compression; + pub use compression::*
└── bundle.rs           # + two decode calls; two content-type constants collapsed into one

ingestion/src/artifact/
├── compression.rs      # new: compress-and-hash, one pass for both manifest numbers
├── artifact.rs         # compress before the geometry copy, so one encode per build
├── hashing.rs          # build_hashed_path rewritten; trim_tmp_uuid_segment deleted
├── publish.rs          # one content-type constant for shards and geometry
└── writer/
    ├── flatgeobuf.rs   # + explicit flush; the stat dropped; the module doc rewritten
    └── sqlite.rs       # the stat dropped

scripts/build/
├── verify-site-tree.sh      # decode each embedded shard; refuse a plain *.sqlite
├── measure-site-budget.sh   # drop the .br exclusion; rewrite the premise
└── sync-embedded-bundle.sh  # cargo run gains --release

web/static/_headers         # the data rule's Content-Type; a note on the prohibition
```

Nothing under `web/src` appears in that tree, and that is the point of the placement. `shared/src/artifact/mod.rs` re-exports each submodule with a wildcard, so the new module is reachable as `shared::artifact::*` with no list to update.

## Phasing

One PR, and the judgment is not close. The producer half alone publishes artifacts in a form no deployed client can read, and it breaks `verify-site-tree.sh`, so a trunk carrying only that half cannot build a site. The consumer half alone decodes unconditionally at two sites and so cannot read anything the producer has published, which fails every `Bundle::open` including the first-paint one. Neither half is independently mergeable in any state that runs, so splitting them would mean deliberately parking the trunk in a broken state between two PRs.

The scripts are part of the same PR rather than a follow-up, for the same reason: `verify-site-tree.sh` fails as written the moment an embedded shard is compressed, so the build gate and the compression have to land together or the gate stops gating.

Ordering against `004-ios-client` phase 0.2 does not matter. This feature touches none of the three files phase 0.2 moves, so the two can land in either order with no conflict.

## Failure modes

| Situation                                                    | Outcome                                                     |
| ------------------------------------------------------------ | ----------------------------------------------------------- |
| A version published before this feature is fetched           | Digest passes, decode reports a wrong-form artifact          |
| That version is then re-fetched on the next visit            | The same failure, until the producer republishes             |
| A cached entry is truncated or partially evicted             | Digest answers false, that one file is re-fetched            |
| A decoder or encoder version mismatch                        | Decode fails at open, the version is deleted and re-fetched  |
| A digest mismatch on a correctly compressed artifact         | Reported before the decoder runs, nothing is cached          |
| A decode error on an embedded artifact, cold visit           | `StartupError::DataUnavailable`, nothing cached or drawn     |
| A decode error on a live artifact                            | A warning and the live notice; first paint stays on screen   |
| A decoded stream exceeding the declared ceiling              | The bounded read errors instead of allocating unboundedly    |
| A plain `*.sqlite` in the embedded data directory            | The build fails naming `sync-embedded-bundle.sh`             |
| An unflushed geometry writer                                 | A truncated `.fgb` that verifies self-consistently           |
| `Content-Encoding: br` added at any layer                    | Every digest check fails, no bundle ever opens               |
| `sqlite3` or QGIS opened on a local artifact                 | Fails; the `.br` suffix says why, `brotli -d` fixes it       |
| `sync-embedded-bundle.sh` run without `--release`            | The encode is approx. 13.6x slower, approx. 45 s per refresh |

The first two rows are one situation stated twice on purpose. A pre-feature version's digest describes plain bytes, so `verify_sha256` passes and the decode then fails, the version is deleted, and the next visit re-fetches it into the same failure. Pre-launch that is accepted, which is why the decode error has to name the wrong-form cause rather than surfacing as an opaque decoder fault, and why the embedded side needs the build gate that refuses a plain `*.sqlite`.

## Risks

- **The wasm decode latency before the first frame is unmeasured.** Only host figures exist, 4.94 ms for the geometry, and the 10 to 15 ms wasm estimate is a 2-3x multiple applied by hand. Measure `Bundle::open` in the browser warm and cold, before and after, as part of the work. If the warm start gets slower in a real browser rather than faster, the placement decision should be revisited rather than defended.
- **The OPFS read-volume argument is unmeasured.** The claim that a warm session reads 2.1 MB where it used to read 13.1 MB has a certain sign and an uncertain magnitude, because OPFS read throughput and the JS-to-wasm copy cost were not measured in a browser. It carries no weight in the spec and should carry none in review.
- **`build_hashed_path` fails silently rather than loudly.** `rsplit_once('.')` at ingestion/src/artifact/hashing.rs:56-58 drops `.fgb` from `.fgb.br` and produces a plausible name, so the rewrite needs a unit test that round-trips both a single and a double extension. Without one this is a corruption in the step that decides what the manifest points at.
- **The unflushed `BufWriter` turns from a wrong-size bug into a wrong-bytes bug.** Today the manifest's geometry `size_bytes` is short by 2,872 while the file is intact. Once the compressor reads that temporary file and nothing stats it, a missing flush yields a truncated `.fgb` that compresses, hashes, publishes, and verifies self-consistently. Needs the explicit flush plus a test that re-parses the published geometry and counts features.
- **`Content-Encoding: br` is one edit away from breaking everything.** Anyone adding it to web/static/_headers or to the R2 `put_object` call makes the browser decode transparently before web/src/client/fetch.rs:29 reads the body, so every digest fails and no bundle opens. Stated as a prohibition in the spec and as a comment in `_headers`, because an absence would not survive a reader who notices the bytes are brotli.
- **Every already-published version becomes permanently unopenable.** Its digest describes plain bytes, so the digest passes and the decode fails, the version is deleted, and it is re-fetched into the same failure. Pre-launch that is acceptable; what makes it tolerable in review is that the error names the cause.
- **The two web fetch-and-cache call sites have no automated coverage.** `scripts/test/test-wasm.sh:22-24` changes into `shared/` before invoking `wasm-pack`, so web-crate browser cases are collected by no runner. This placement is largely immune, changing nothing under `web/src`, but the end-to-end path remains manual-browser-only.
- **The wasm growth was checked with a probe, not the real build.** A cdylib built byte-identical with and without an unexported encode, so link-time optimization drops the encoder. Measure the real release wasm before and after and report the decoder's contribution, which the probe put at approx. 212 KB raw and approx. 61 KB over the wire, most of it brotli's static dictionary. Because `measure-site-budget.sh` reports client code separately and uncapped, its printed first-paint improvement will overstate the net for a genuine first-time visitor, approx. 1.11 MB rather than approx. 1.17 MB.
- **The decoder has no output ceiling by default.** The digest is verified first, so the stream is authenticated against a manifest from our own origin and the exposure is small, but a cheap cap on decoded size is worth having rather than trusting the stream's expansion ratio.
- **Local artifacts stop opening in `sqlite3`, `fgb info`, and QGIS.** This overrides a documented decision at ingestion/src/artifact/writer/flatgeobuf.rs:6-17, which argued on real ergonomic grounds. That module doc has to be rewritten rather than trimmed, and the workflow becomes one `brotli -d` away.
- **`verify-site-tree.sh` gains a hard dependency on the `brotli` CLI.** docs/system-dependencies.md:87-88 lists it as needed only by the perf-budget report; the deploy gate now fails without it, which changes that entry's severity.
- **Both budget caps stop discriminating**, at 24% and 15% of target, so the budget script's warning value decays until artifacts grow several-fold. Re-setting them is a product judgment this feature surfaces and deliberately does not absorb.

## Test plan

Host unit tests in `shared`, written before the implementation per Constitution Principle VII, run with `cargo test -p shared`:

- The codec round-trips the committed geometry sample (shared/tests/samples/one-feature.fgb, reached through `geometry::tests::one_feature_fgb_bytes`) and the committed shard sample, returning the input bytes exactly.
- Decoding plain bytes reports a wrong-form artifact naming the relative path, rather than returning garbage.
- A truncated brotli stream errors.
- A stream whose decoded size exceeds the ceiling errors at the bound instead of allocating.
- The digest-before-decode ordering: a bundle whose cached bytes are correctly compressed but whose manifest digest is wrong fails with a digest mismatch, never with a decoder error, which is the proof the decoder never runs on unverified bytes.
- The seven existing `Bundle::open` cases (shared/src/artifact/bundle.rs:182 onward) pass with `seeded_mock` (:163) compressing `one_feature_fgb_bytes()` and `sample_shard_bytes()` before insertion and `entry` (:131) hashing the compressed bytes. No new committed sample file, and the round trip is then exercised by every case rather than by one added test.

Unit tests in `ingestion`, run with `cargo test -p ingestion`:

- `build_hashed_path` renders `world-50m.tmp-<uuid>.fgb.br` as `world-50m-<sha>.fgb.br` and still renders a single-extension temporary name correctly. Written and observed failing against the current `rsplit_once('.')` implementation before the rewrite.
- The compress-and-hash step's returned byte count equals `fs::metadata` of the file it wrote, and its digest equals a fresh digest of that file.

Integration tests in `ingestion`, run with `cargo test -p ingestion --test artifact_integration` and `--test publish_integration`. The publish target needs a live Postgres at `TEST_DATABASE_URL` and cannot run under `SQLX_OFFLINE`.

- Every manifest entry's `size_bytes` equals the published file's length on disk. The existing assertion at ingestion/tests/artifact_integration.rs:148 compares the manifest against the same `byte_count` field that produced it, which is why the geometry undercount shipped; replacing it with a `fs::metadata` comparison is the assertion that catches it.
- The real-FlatGeobuf case decompresses the published geometry, re-parses it, and counts features. This is what catches an unflushed writer, and the existing case at :276-286 only asserts that the FGB parses.
- The downsampled variant's geometry is byte-identical to the complete variant's and carries the same digest and byte count, confirming one encode per build.
- Uploaded shard and geometry bytes are byte-identical to the source files. ingestion/tests/publish_integration.rs:57-59 asserts only that the destinations exist, so nothing there would notice an upload path that transformed bytes.

Build checks:

- `cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown`.
- A release wasm size comparison before and after, so the decoder's contribution is reported rather than guessed.
- Not `cargo test -p shared --features render`: no render-gated code is touched.
- No `#[wasm_bindgen_test]` anywhere in this feature. Brotli does not diverge between wasm32 and the host, so the browser harness would add cost and no coverage, and the web crate's browser cases are collected by no runner in any case.

Script checks, run in order:

- `cargo run --release -p ingestion -- build`, then `publish local`, then `./scripts/build/sync-embedded-bundle.sh ./web/static/embedded_artifacts`.
- `./scripts/build/verify-site-tree.sh` passes against that tree.
- A negative case: place a plain `*.sqlite` in the embedded data directory and confirm the script fails with the message naming `sync-embedded-bundle.sh` rather than dying on a raw `sqlite3` exit.
- `./scripts/build/measure-site-budget.sh` reports first paint at approx. 479,554 bytes and second paint at approx. 1,191,107, agrees with each artifact's `size_bytes` in the manifest, and does not report 484.

The browser measurement is the one number this design rests on and does not have. With the app running locally, time `Bundle::open` warm and cold, before and after, so the decode cost and any OPFS read saving are measured together rather than argued. Record the result in [tasks.md](tasks.md) §Deviations. If the warm start is slower in a real browser, say so and reopen the placement rather than defending it.

## PR description

Compresses the artifacts in the producer and decodes them in `shared`, taking first paint from 1.65 MB to 0.48 MB and second paint from 6.02 MB to 1.19 MB.

Neither destination compresses these files for us. Cloudflare's edge compresses only content types on a fixed list holding no generic binary entry, and R2 does not compress at all, so a 1.5 MB FlatGeobuf geometry has been transferring whole. `Content-Encoding: br` is unavailable here because the browser would decode it before the client reads the body, failing every digest check.

Each shard and the geometry is now encoded at brotli quality 11 before the content-addressing digest is taken, named with `.br` appended, and decoded inside `Bundle::open` between the digest check and the parser. The manifest's `sha256` and `size_bytes` keep describing the file as served, so no manifest field changes meaning and none is added; because brotli decoding is deterministic, one digest over the compressed stream certifies the decoded bytes. Nothing under `web/src` changes, which puts the decode where the iOS and Android clients inherit it.

Along the way: the geometry writer now flushes before its size is read, and both of a manifest entry's numbers come from one pass over one buffer, which fixes a live 2,872-byte undercount in the published geometry's `size_bytes`. `build_hashed_path` no longer silently drops the inner extension from a double-extension filename. The embedded-shard build gate decodes before reading the SQLite header and refuses a tree holding a plain `*.sqlite`, and the perf-budget report stops asserting that the artifacts transfer whole.

`brotli` is a new dependency, approved for this purpose.

## Deviations

To be recorded in [tasks.md](tasks.md) §Deviations as the work lands.
