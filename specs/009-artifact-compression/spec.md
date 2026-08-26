# Feature Specification: artifact compression

**Feature Branch**: `artifact-compression`

**Created**: 2026-08-26

**Status**: Draft

**Input**: `docs/backlog.md` §Client, "Compress the artifacts in the producer, and decompress them in `shared`", picked up ahead of its recorded trigger because first paint is already at 82% of its cap.

## Why the artifacts arrive uncompressed

Neither destination compresses them. A probe deploy against Cloudflare Workers Assets confirmed the edge compresses a response only when its content type is on a fixed list, and that list holds no generic binary entry: no `application/octet-stream`, no `application/vnd.sqlite3`, no catch-all. A 1.5 MB FlatGeobuf file came back whole with no `Content-Encoding`. R2, which serves the live bundle, does not compress on its own either. `web/static/_headers` records the same shape of gap for a different reason: the edge's extension lookup has no entry for `.fgb` and none for `.sqlite`, which is why the two artifact rules state a Content-Type at all.

The bytes at stake are almost all of first paint. The embedded geometry is 1,576,240 of the 1,646,356-byte first-paint total, and the geometry compresses 3.35x while a statistic shard compresses 11.6x. `scripts/build/measure-site-budget.sh` counts both at full size and says so in its own header comment and in its printed notes, so the script that reports the budget is also the clearest statement of the problem.

The standard remedy is forbidden here. `Content-Encoding: br` would have the browser decode transparently before `web/src/client/fetch.rs:29` reads the body, so every digest check would then compare plain bytes against a compressed-bytes digest and no bundle would ever open. That leaves compressing in the producer and decoding in `shared`, which is also the only option that covers R2, the iOS client, the Android client, and any future destination, since none of them is an HTTP edge we can configure.

## The mechanism

The producer compresses each shard and the geometry once, at quality 11, before the content-addressing digest is taken, and writes the compressed file to disk under the name the manifest carries. The manifest's `sha256` and `size_bytes` therefore describe the file as served, which is what they already claim to describe, so no manifest field changes meaning and no field is added.

The reader decodes inside `Bundle::open`, at the two points where a verified buffer becomes a parsed object: after `filesystem::verify_sha256` at shared/src/artifact/bundle.rs:53 and before `geometry::parse_geometry_layer` at :54, and after `verify_sha256` at :65 and before `shard_db::read_shard` at :67. Nothing between the wire and those two lines changes. The OPFS cache holds the artifact exactly as served, `web/src/client/load.rs` fetches and stores it unchanged, and `is_already_cached` (web/src/client/load.rs:303) keeps re-hashing cached bytes against the manifest digest with exactly its current meaning.

One digest is enough because brotli decoding is deterministic: a bit-exact compressed stream implies bit-exact decoded bytes, so a digest over the compressed form transitively certifies what comes out of the decoder. That is the whole reason `ManifestEntry` needs no second digest and no second size.

The decode is unconditional at both sites. The reader does not sniff a filename or read a manifest field to decide whether to decode, so an artifact published in plain form fails loudly rather than being accepted quietly.

## Scenarios

### A cold first visit

The client fetches the embedded manifest and the two embedded files it names, verifies each digest, and caches the bytes as served. `Bundle::open` then decodes the geometry from 469,994 bytes back to 1,576,240 and the downsampled shard from 9,076 back to 69,632, parses both, and the map draws. The visitor transferred 479,554 bytes instead of 1,646,356 and paid one brotli decode of the geometry before the first frame, which `Renderer::new` (web/src/map/canvas/driver.rs:785) cannot be constructed without.

### A warm start

The cached bytes are read out of OPFS, hashed against the manifest, and decoded. Nothing is fetched. The session hashes 712 KB where it used to hash 4.37 MB and pays two decodes for it, which is a net reduction in startup CPU rather than an addition.

### The live upgrade

After the first redraw is scheduled, `upgrade_to_live_bundle` (web/src/map/canvas/driver.rs:845) fetches the live manifest and every file it names, verifies, caches, and opens. The decode happens in the same `Bundle::open` as the first-paint path, off the critical path for the first frame.

### A build of the site tree

`scripts/build/sync-embedded-bundle.sh` copies the producer's downsampled tree unchanged, so the embedded bundle is compressed for free. `scripts/build/verify-site-tree.sh` decodes each embedded shard before reading its SQLite header, and refuses a tree holding any plain `*.sqlite`, which is the only local signal that a stale tree was copied in.

### Inspecting an artifact by hand

`sqlite3` and `fgb info` and QGIS all fail on the file as written. The `.sqlite.br` and `.fgb.br` names say why, and `brotli -d` restores a correctly-named file by dropping one path segment.

### Edge cases

- A repository version published before this feature: its digest describes plain bytes, so `verify_sha256` passes and the decode then fails. `open_newest_cached_bundle` (web/src/client/load.rs:63) deletes the version, the live open reports an error, and the next visit repeats it. Pre-launch this is accepted, and the decode error must name the wrong-form cause rather than surfacing as an opaque decoder fault.
- A cached OPFS entry truncated by a crash or partially evicted: unchanged from today. `is_already_cached` hashes the stored bytes, answers false, and that one file is re-fetched. The compressed form does not weaken this, because the repair is driven by the digest rather than by the decoder erroring.
- A plain artifact sitting in the embedded data directory, because a stale tree was copied: the build fails and names `sync-embedded-bundle.sh`, rather than shipping a first paint the client cannot decode.
- An unflushed `BufWriter` in the geometry writer: today it leaves the manifest's geometry `size_bytes` short by 2,872 while the file itself is intact. Once the compressor reads that temporary file and nothing stats it, the same omission yields a truncated `.fgb` that compresses, hashes, publishes and verifies self-consistently. The writer flushes explicitly and a test re-parses the published geometry.
- A double extension through the content-addressing step: `build_hashed_path` (ingestion/src/artifact/hashing.rs:52) splits the extension with `rsplit_once('.')` at :56-58 and rejoins one extension at :69, so `world-50m.tmp-<uuid>.fgb.br` would keep only `br` and silently drop `.fgb`. It splits on `.tmp-` instead and carries the whole remaining extension chain.
- A brotli stream whose declared expansion is enormous: the digest is verified first, so the stream is authenticated against a manifest from our own origin, but the decoder still gets a ceiling on decoded size rather than trusting the stream.
- Two overlapping publishes: unchanged. They already race on `latest/manifest.json`, and every artifact filename is content-addressed, so the losing publish's files are not overwritten.

## Requirements

### The codec

- **FR-001**: Compression MUST use brotli through the pure-Rust `brotli` crate, one crate for encoding in `ingestion` and decoding in `shared`. It compiles for wasm32 with no C toolchain, which is what makes one crate viable on every target.
- **FR-002**: The crate MUST be declared in `[workspace.dependencies]` with a `major.minor.*` requirement resolved from `Cargo.lock`, and MUST be added to `shared`'s plain `[dependencies]` table rather than to either per-target table. The decoder is target-agnostic pure Rust, so a `#[cfg]` gate there would be noise per `docs/conventions/conditional-compilation.md`.
- **FR-003**: Encoding MUST be at quality 11. `BrotliEncoderParams::default()` is already quality 11 with a 22-bit window and no magic prefix, so the producer constructs the default and sets nothing. Quality 11 costs approx. 27x quality 9's time for approx. 17% smaller output, and the producer runs weekly.
- **FR-004**: Both directions MUST live in one module in `shared`, so the encoder's parameters and the decoder cannot drift apart across crates.
- **FR-005**: The decoder MUST bound the decoded size rather than trusting the stream's expansion ratio.
- **FR-006**: There MUST be exactly one codec. No negotiation, no gzip fallback, and no per-entry codec field.

### The digest and the manifest

- **FR-007**: The manifest's `sha256` MUST cover the compressed bytes, and `size_bytes` MUST be the compressed byte count, so a manifest entry keeps describing the file as served and the client verifies what it fetched before spending CPU on the decoder.
- **FR-008**: `ManifestEntry` (shared/src/artifact/manifest.rs:84), `validate_entry` (:120), `MANIFEST_SCHEMA_VERSION` (:13), and `schema_pointer_key` (:27) MUST be unchanged. No decompressed size, no decompressed digest, no codec field, no schema bump. Every field keeps its current meaning under FR-007.
- **FR-009**: `filesystem::verify_sha256` (shared/src/filesystem.rs:42) and all four of its call sites MUST keep hashing exactly the bytes they hash today.
- **FR-010**: The compression step MUST sit between the writers and `hashing::*`, so the content-addressing digest covers compressed bytes as FR-007 requires.
- **FR-011**: A manifest entry's `size_bytes` and its `sha256` MUST be derived from the same pass over the same bytes. Today they are two independent observations of the file, a `fs::metadata` stat taken by the writer (ingestion/src/artifact/writer/flatgeobuf.rs:134 and writer/sqlite.rs:80) and a later re-read in `hashing.rs:20` and :31, and they demonstrably disagree for the geometry.

### File naming

- **FR-012**: A compressed artifact MUST be named with `.br` appended to the name it carries today: `geometry/world-50m-<64 hex sha256>.fgb.br` and `data/<statistic>-<class>-<64 hex sha256>.sqlite.br`, the digest over the compressed bytes per FR-007.
- **FR-013**: The inner extension MUST be kept, so the artifact's real format stays visible and `brotli -d` restores a correctly-named file by dropping one segment.
- **FR-014**: `build_hashed_path` (ingestion/src/artifact/hashing.rs:52-70) MUST be rewritten to split the stem on `.tmp-` and carry the whole remaining extension chain, and `trim_tmp_uuid_segment` (:72-75) MUST be deleted. As written the function silently drops `.fgb` from `.fgb.br` rather than failing, which is a silent name corruption in the content-addressing step.
- **FR-015**: `CONTENT_TYPE_FLATGEOBUF` and `CONTENT_TYPE_SQLITE` (shared/src/artifact/bundle.rs:17-18) MUST be collapsed into one `application/octet-stream` constant. A brotli-wrapped SQLite file is not `application/vnd.sqlite3`, and the header that would describe the wrapper is the one FR-016 forbids. Collapsing rather than widening keeps `ArtifactRepository::put_file`'s signature unchanged.

### The prohibition on Content-Encoding

- **FR-016**: Nothing MUST set `Content-Encoding` on an artifact: not `web/static/_headers`, not the R2 `put_object` call, not any `ArtifactRepository` implementation. The browser decodes a marked body transparently before web/src/client/fetch.rs:29 reads it, so the digest over compressed bytes would fail on every file and no bundle would ever open. This is a stated prohibition rather than an absence, because the bytes genuinely are brotli and a future reader will be tempted.
- **FR-017**: `ArtifactRepository::put_file` (ingestion/src/artifact/repository/artifact_repository.rs:10) MUST keep its signature and gain no header parameter.

### The producer

- **FR-018**: The geometry MUST be encoded once per build. `copy_geometry_into` (ingestion/src/artifact/artifact.rs:234-247) copies the complete variant's geometry into the downsampled variant byte-for-byte and carries its digest and byte count forward, so encoding before the copy keeps that reuse valid rather than turning it into an assumption about encoder determinism, and saves approx. 1.0 s per build.
- **FR-019**: The geometry writer MUST flush explicitly before its bytes are read by anything. Once nothing stats the file, an unflushed tail is a truncated artifact rather than a wrong number.
- **FR-020**: `manifest.json` MUST stay uncompressed. It is the document carrying every other file's digest, `parse_manifest` is called on raw fetched bytes with no decode step available before it, and Cloudflare compresses `application/json` at the edge already.
- **FR-021**: The discovery document MUST stay uncompressed, for the same reason plus the Content-Type `web/static/_headers` assigns it.
- **FR-022**: The compressed file MUST be what lands on disk in the artifact directory. `sync-embedded-bundle.sh`'s plain copy and `measure-site-budget.sh`'s local-repository fallback both read that tree, and one form on disk is what keeps them honest.
- **FR-023**: Publish ordering (ingestion/src/artifact/publish.rs:33-78) MUST be unchanged, including that `latest/manifest.json` is the last object a publish writes.
- **FR-024**: The publish integration test MUST assert that uploaded shard and geometry bytes are byte-identical to the source files. Today it asserts only that the destinations exist (ingestion/tests/publish_integration.rs:57-59), so nothing would notice an upload path that transformed bytes.
- **FR-025**: A manifest entry's `size_bytes` MUST be asserted against the file's length on disk. The existing assertion compares the manifest against the same field that produced it, which is why the 2,872-byte geometry undercount shipped.

### The reader

- **FR-026**: The decode MUST happen inside `Bundle::open`, after the digest check and before the parser, at shared/src/artifact/bundle.rs:53-54 and :65-67.
- **FR-027**: The decode MUST be unconditional at both sites. No filename sniffing and no manifest field consulted, so a plain artifact fails loudly.
- **FR-028**: A digest match followed by a decode failure MUST be reported as a wrong-form artifact naming the relative path. It can only mean the producer wrote the wrong form, never corruption, and that distinction is the diagnostic.
- **FR-029**: Nothing under `web/src` MUST change. Not `fetch.rs`, not `load.rs`, not `cache.rs`, not `opfs.rs`. No decode at the fetch boundary and no codec awareness in the `ArtifactCache` trait (shared/src/artifact/cache.rs).
- **FR-030**: The OPFS cache MUST hold the artifact exactly as served. `is_already_cached` (web/src/client/load.rs:303-323) then keeps its per-file corruption repair with its current meaning, which is what the truncation handling at :300-302 and at :71-81 is built around.
- **FR-031**: Both readers MUST keep taking a fully materialized buffer. `rusqlite`'s `deserialize` needs one contiguous image, the read-only VFS needs random access, and `parse_geometry_layer` retains its owned buffer for the session because hit-testing re-queries it on every pointer move. No streaming decode.
- **FR-032**: `validate_shard_header` (shared/src/sqlite/schema.rs:77), `SCHEMA_VERSION` (:13), both `read_shard` bodies, and `ro_memory_vfs.rs` MUST be unchanged. The shard's own bytes and schema are untouched; only the envelope around them is new.
- **FR-033**: The `Bundle::open` tests MUST seed `MockArtifactCache` with compressed sample bytes and digests over those compressed bytes, so the round trip is exercised by every existing case rather than by one added test. No new committed sample file is needed.

### The build scripts

- **FR-034**: `scripts/build/verify-site-tree.sh` MUST decode each embedded shard before reading its SQLite header. Its `find` at :89 becomes `*.sqlite.br` and each match is decoded into a temporary file for the existing `pragma user_version` check at :82. Keeping `.sqlite` is not viable: `sqlite3` exits 26 on a brotli stream, and under the script's `set -euo pipefail` the command substitution kills the build before the message at :84 that names `sync-embedded-bundle.sh`.
- **FR-035**: `verify-site-tree.sh` MUST fail when the embedded data directory holds any plain `*.sqlite`. A plain artifact there means a stale tree was copied and would reach the client as a decode failure.
- **FR-036**: `verify-site-tree.sh` MUST check for the `brotli` program as it checks for its other dependencies, and `docs/system-dependencies.md:87-88` MUST record that the deploy gate now needs it rather than only the perf-budget report.
- **FR-037**: `scripts/build/measure-site-budget.sh` MUST stop excluding `*.br` from the embedded-directory sum at :214. That filter exists to avoid double-counting a precompressed sibling and would now exclude the artifacts themselves, silently reporting first paint as 484 bytes.
- **FR-038**: `measure-site-budget.sh`'s stated premise MUST be rewritten: the header comment at :10-14, the comment at :138-141, and the printed notes at :528-530, all of which assert that the geometry and the statistic shards transfer whole. The artifacts keep falling through to `wc -c`, which becomes the right answer by construction rather than by accident.
- **FR-039**: `measure-site-budget.sh` MUST NOT pipe an artifact through `brotli` a second time. Re-encoding an already-brotli stream inflates the reported figure.
- **FR-040**: `scripts/build/sync-embedded-bundle.sh` MUST NOT gain a compress step. Its `cp -R` carries whatever the producer wrote, and compression happens once, in the producer.
- **FR-041**: `sync-embedded-bundle.sh`'s `cargo run -p ingestion` MUST be changed to `--release`. The same encode is approx. 13.6x slower in a debug build, which turns an approx. 3.2 s step into approx. 45 s in a script a developer runs by hand.

### The documentation that becomes false

- **FR-042**: `ingestion/src/artifact/writer/flatgeobuf.rs:6-17` MUST be rewritten. Its module doc argues that compression belongs at publish time via `Content-Encoding: br` and that the local file is more useful as a plain `.fgb`. Both claims are refuted by the CDN probe and by this feature respectively, and the doc is the last place the refuted plan survives as advice. It is rewritten, not trimmed.
- **FR-043**: The compression figures at `docs/architecture/client-web.md:147-149` and its sample report at :158-170, the ratio claim at `docs/architecture/client.md:348`, and the "brotli compression at the CDN" assertion at `docs/architecture/overview.md:435` MUST all be corrected.
- **FR-044**: The stale `SHARD_FILENAME_EXTENSION` constant (shared/src/artifact/geometry.rs:28), referenced nowhere, and the doc at :19 claiming the final filename is `{stem}-{sha8}.fgb` MUST be reconciled with the new naming.

### Key entities

- **Compressed artifact**: a shard or the geometry, brotli-encoded at quality 11 in the producer, named with `.br` appended, described by the manifest at its compressed size and digest, cached in that form, and decoded once per `Bundle::open`.
- **Codec module**: the one place in `shared` holding both directions and the encoder's parameters, imported by `ingestion` for encoding and by every client for decoding.

## Naming the compressed artifact

The name is `world-50m-<64 hex sha256>.fgb.br`, not `world-50m-<64 hex sha256>.fgb`, and the choice is worth stating because the alternative is cheaper in exactly one place and worse everywhere else.

Keeping the plain extension would leave `verify-site-tree.sh`'s `find` matching and `measure-site-budget.sh`'s `.br` filter harmless, which is the whole of its advantage. Against that, a brotli stream under a `.sqlite` name makes `sqlite3` fail with "file is not a database" and tells the reader nothing about why, and it makes `verify-site-tree.sh` die on a raw driver exit rather than on the message that names the script which fixes the tree. The served Content-Type would keep asserting `application/vnd.sqlite3` over bytes that are not one, with no honest alternative available, since the header that would describe the wrapper is the one FR-016 forbids. And `curl -O` would produce a file whose name lies about its contents.

Appending `.br` costs one producer fix, which the feature wants anyway: `build_hashed_path` cannot round-trip a double extension as written, and the way it fails is silent truncation of the name rather than an error. Two scripts change, both in ways that were going to change regardless, because a compressed embedded shard breaks `verify-site-tree.sh` under either name and inverts `measure-site-budget.sh`'s premise under either name.

Two things propagate for free. The OPFS key is `artifacts/{version_label}/{relative_path}` built straight from the manifest (web/src/client/cache.rs:128-135), so the rename needs no cache-layer change. And `web/static/_headers` matches by directory, so `/embedded_artifacts/geometry/*` and `/embedded_artifacts/data/*` still match and only the declared Content-Type changes. Those two rules must stay separate: merging them into `/embedded_artifacts/*` would capture `manifest.json` and break the revalidation its stable name depends on.

## Budget effect

Every compressed figure below is the Rust crate's quality-11 output, which is 0.35% larger than the `brotli` CLI's for the geometry and 1.81% larger for a shard. The CLI numbers are what `docs/backlog.md` quotes and what `measure-site-budget.sh` computes today, so the two have never agreed. After this feature the script reads the real on-disk bytes and its artifact figures stop being an approximation of a different encoder's output.

First paint, against the 2,000,000-byte target: 1,646,356 bytes (82.3%) becomes 484 + 9,076 + 469,994 = 479,554 bytes (24.0%), saving 1,166,802. The manifest stays edge-compressed at 484 bytes. The embedded downsampled shard goes 69,632 to 9,076, which is 7.67x rather than the backlog's 13.0x, because 13.0x is the multi-year live shard. The geometry goes 1,576,240 to 469,994, which is 3.35x.

Second paint, against the 8,000,000-byte target: 6,020,164 bytes (75.3%) becomes 479,554 + 469,994 + 224,117 + 17,442 = 1,191,107 bytes (14.9%). Total artifact saving on a cold first visit is 4,829,057 bytes, approx. 4.83 MB. The backlog's approx. 3.07 MB is stale: it counted the geometry plus one 2,125,824-byte shard, and that shard is now 2,592,768 bytes with a second one alongside it.

Against those savings the wasm grows by approx. 212 KB raw and approx. 61 KB over the wire for the decoder and its static dictionary, so the honest net for a genuine first-time visitor is approx. 1.11 MB rather than approx. 1.17 MB. `measure-site-budget.sh` reports client code separately and uncapped, so its printed first-paint improvement will overstate the real one and the wasm delta has to be measured and reported alongside it.

Per-session CPU falls rather than rises. Every artifact is hashed three times per warm session, in `Bundle::open` on the cached version (web/src/map/canvas/driver.rs:754), in `is_already_cached` on the live path (web/src/client/load.rs:309), and in `Bundle::open` on the live version (web/src/client/load.rs:229). At the surveyed rates, hashing 712 KB three times instead of 4.37 MB saves 18.35 ms and two decodes cost 15.68 ms, so a warm session goes 21.91 ms to 19.25 ms and a cold visit 20.06 ms to 16.88 ms. The throughputs behind those figures were not re-measured for this spec, and the wasm decode multiple is an estimate; see the plan's risks.

The OPFS cache footprint falls from 8,749,996 bytes to 1,425,486 bytes across the two retained versions, approx. 6.1x. That is worth nothing against browser quotas, which are in the hundreds of megabytes to gigabytes per origin, and must not be claimed as a benefit of this feature.

Producer encode cost is approx. 3.2 s per weekly build with the geometry encoded once, or approx. 4.2 s if it is encoded per variant.

## Success criteria

- **SC-001**: A round trip through the codec module returns the input bytes exactly, for the committed geometry sample and the committed shard sample.
- **SC-002**: Decoding plain bytes reports a wrong-form artifact naming the relative path, rather than returning garbage or an opaque decoder fault.
- **SC-003**: A bundle whose cached bytes are correctly compressed but whose manifest digest is wrong fails with a digest mismatch, never with a decoder error, so the decoder provably never runs on unverified bytes.
- **SC-004**: `build_hashed_path` renders `world-50m.tmp-<uuid>.fgb.br` as `world-50m-<sha>.fgb.br`, and still renders a single-extension temporary name correctly.
- **SC-005**: Every manifest entry's `size_bytes` equals the length of the published file on disk, and its `sha256` equals a fresh digest of that file.
- **SC-006**: The published geometry decodes and re-parses as FlatGeobuf with the expected feature count, which is what catches an unflushed writer.
- **SC-007**: The downsampled variant's geometry is byte-identical to the complete variant's and carries the same digest and byte count, confirming a single encode per build.
- **SC-008**: Uploaded shard and geometry bytes are byte-identical to the source files, and the three manifest copies stay byte-identical to each other.
- **SC-009**: The seven existing `Bundle::open` cases pass with the mock cache seeded with compressed bytes.
- **SC-010**: `verify-site-tree.sh` passes against a freshly synced embedded tree, and fails with the message naming `sync-embedded-bundle.sh` when a plain `*.sqlite` is placed in the embedded data directory.
- **SC-011**: `measure-site-budget.sh` reports first paint at approx. 479,554 bytes, agreeing with each artifact's `size_bytes` in the manifest, and does not report 484.
- **SC-012**: A cold browser load draws the map and a warm reload draws it with no artifact fetches, with `Bundle::open` timed in both, before and after, so the wasm decode cost is measured rather than estimated.
- **SC-013**: The release wasm's size is reported before and after, so the decoder's contribution to the ledger is a measurement rather than a probe result.

## Assumptions

- `brotli` is a new third-party dependency. The repository's rule is to ask before adopting one; the owner approved this one explicitly, and it alone.
- Brotli decoding is deterministic, so a digest over the compressed stream certifies the decoded bytes. This is what removes the need for a second digest, and the whole placement rests on it.
- Pre-launch, there is no compatibility burden. Every already-published version becomes permanently unopenable, and that is accepted rather than mitigated.
- Only the complete variant is published to a repository; the downsampled tree is the embedded first-paint bundle. Both are compressed, and the geometry they share is encoded once.
- The producer runs weekly, so encode time is not a constraint at quality 11.
- Hosting the encoder in `shared` costs the wasm nothing, because nothing in a client's reachable graph calls it and link-time optimization drops it. This was checked with a cdylib probe rather than against the real release build, which is why SC-013 exists.
- The compressed sizes quoted throughout are measured, not estimated. The wasm decode latency and the OPFS read volume are the two figures that are not.

## Scope cutoff

Out of scope, each for a stated reason:

- **`Content-Encoding` at any layer.** Forbidden rather than deferred, per FR-016. The browser would decode transparently before the client reads the body and every digest check would fail.
- **A manifest schema bump or any new manifest field.** No decompressed size, no decompressed digest, no per-entry codec field. Under FR-007 every existing field keeps its meaning, so there is nothing to version.
- **Any change under `web/src`.** The decode is in `shared`, inside the function every platform calls, so no platform loader can forget it. Putting the decode at the fetch boundary or in the cache layer would push a transport concern into the storage interface and break both digest checks.
- **Streaming, worker-thread, or incremental decode.** Both readers need a fully materialized buffer, per FR-031, and the client has no worker infrastructure to move a decode onto.
- **Range-request artifact reading.** FlatGeobuf's HTTP-range streaming mode is unused; v1 downloads the whole geometry at startup.
- **A second codec, codec negotiation, or a gzip fallback.** One codec, decoded unconditionally.
- **Re-setting the 2.00 MB and 8.00 MB budget caps.** They stop discriminating at 24% and 15% of target, which this feature should surface rather than silently absorb. Restating them is a product judgment and belongs in `docs/architecture/client.md` §Web first-paint perf budget.
- **Publish ordering, the retention policy, and `VERSIONS_KEPT`.** Untouched. The unused `CACHE_CONTROL_MANIFEST` and `CACHE_CONTROL_SHARD` constants (shared/src/artifact/bundle.rs:22 and :24) are not wired up here either.
- **A plain sibling artifact written alongside the compressed one.** One form on disk, and `brotli -d` for the QGIS and `sqlite3` workflows. Two forms would double the disk footprint to serve a workflow one command already serves, and would let the two drift.
- **The shard's own schema.** `SCHEMA_VERSION`, `validate_shard_header`, both `read_shard` bodies, and `ro_memory_vfs.rs` are unchanged. Only the envelope is new.
- **A compress step in any script.** Compression happens once, in the producer. `sync-embedded-bundle.sh`, `deploy-site.sh`, and `set-artifact-cors.sh` gain none.
- **The `004-ios-client` phase 0.2 move of load orchestration into `shared`.** This feature deliberately touches none of the files that move (web/src/client/load.rs, web/src/live_resolve.rs, web/src/version_rank.rs), so the two can land in either order.
- **Automated coverage of the two web fetch-and-cache call sites.** They have no runner today, because `scripts/test/test-wasm.sh` changes into `shared/` before invoking `wasm-pack`, so web-crate browser cases are collected by nothing. This placement changes neither site, and extending the runner is its own question.

## Constitution check

- **Principle IV, Singularity convention parity**: one new third-party dependency, approved by the owner explicitly per the rule that requires asking first. Declared with the `major.minor.*` requirement form the workspace already uses.
- **Principle V, explicit over implicit**: directly served. The decode is two visible calls at two named lines, with no interception, no wrapper, and no conditional. The client does not consult a field to decide whether to decode, so there is no hidden branch to reason about.
- **Principle VI, CDN-delivered data**: unchanged in shape. The same immutable content-addressed artifacts on the same destinations, fetched over the same HTTP path, with a different envelope.
- **Principle VII, test-first**: a codec boundary is squarely inside "error mapping" and "artifact diffing". The round trip, the wrong-form error, the digest-before-decode ordering, and `build_hashed_path`'s extension chain are all pure functions with host tests written before the implementation.
- **Principle III, Rust core**: decisive for the placement. The decode lives in `shared` inside `Bundle::open`, so the iOS and Android clients inherit it on day one rather than reimplementing it in a platform load layer.
- **Principle II, source provenance**: unaffected. Compression is an envelope over bytes whose provenance the manifest already records.
- **Principle I, educational neutrality**: unaffected. No user-facing copy changes.
- **`docs/conventions/conditional-compilation.md`**: the dependency is declared outside both per-target tables, because the decoder is target-agnostic and a gate would be noise.
- **`docs/conventions/logging.md`**: any added log line is `<message>; [key=value ...]`, with the bracketed section omitted when there is no structured data.
- **`docs/conventions/types.md`**: no new wire type and no new enum. The codec module exposes two functions and one extension constant.
