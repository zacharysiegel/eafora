# Feature Specification: Artifact builder + Cloudflare R2 publish

**Feature Branch**: `002-artifact-builder`

**Created**: 2026-05-26

**Status**: Draft

**Input**: User description: "Artifact builder + Cloudflare R2 publish — reads the canonical store, emits FlatGeobuf geometry + per-statistic / per-license-class SQLite shards + content-hashed manifest into a local build directory, uploads to Cloudflare R2 and records an artifact_version row. Implements the design in docs/architecture/ingestion.md §Artifact builder and §Cloudflare R2 upload."

## Scenarios & Testing *(mandatory)*

### Scheduled artifact publication (P1)

After the weekly WB WDI ingestion run completes on the Mac mini, the launchd-managed schedule chains a build + publish: every value with `superseded is null` in `statistic_value` is rolled into the source-priority merge, emitted into SQLite + FlatGeobuf shards with content-hashed filenames, written to manifest.json, **delivered to the configured destination** (local disk OR Cloudflare R2 — toggleable per-invocation), and recorded in `artifact_version` with a destination-appropriate URL. **Pre-product-ship**: the periodic job uses the local-disk destination — artifacts accumulate on the Mac mini, Cloudflare R2 is not touched, no public-facing CDN traffic. **Post-ship**: the same job switches to the Cloudflare R2 destination by changing one flag in the launchd plist.

**Acceptance Scenarios**:

1. **Given** a canonical store with WB WDI TFR values (≈13k current rows) and the wb_wdi data_source row, **When** the chained `build` + `publish --destination=local --local-dir ${repo_dir}/published` runs, **Then** the local published directory contains `<version_label>/manifest.json` and the referenced content-hashed shards; one `artifact_version` row is inserted with `manifest_url=file://...<absolute_path>/manifest.json` and `data_source_versions_jsonb={"wb_wdi": "..."}`.
2. **Given** the same state but `publish --destination=cloudflare-r2` is invoked, **When** publish runs against a real Cloudflare R2 bucket with valid credentials, **Then** each shard + manifest is uploaded under its content-hashed key with the per-file `Cache-Control` headers; `artifact_version.manifest_url` resolves to the CDN URL; the destination switch from `local` to `cloudflare-r2` requires no source-code change — only the flag.
3. **Given** a successful local-destination publish completed for `2026-05-18`, **When** the canonical store changes (a WB WDI revision lands) and the next periodic build + local-publish runs, **Then** the new build dir contains shards whose content hashes differ from the prior build; the new manifest references the new hashes; a new `artifact_version` row records the new label; the prior version's directory + `artifact_version` row remain untouched (additive publishing).

---

### Manual local build for inspection (P2)

A developer runs `ingestion build <output-dir> <version-label>` from a shell to produce artifact files locally without publishing. Used to inspect the shard contents, diff against a prior build, verify the manifest, or stage a build before authorizing the publish step. `build` writes ONLY to disk and never inserts an `artifact_version` row — that table is the destination's "what's published?" record, not a per-build log.

**Acceptance Scenarios**:

1. **Given** a canonical store with current WB WDI values, **When** the developer runs `cargo run -p ingestion -- build ./tmp-build 2026-05-26-test`, **Then** the local directory contains the full artifact set (shards + geometry + manifest), the `artifact_version` table is unchanged, and the developer can inspect each SQLite shard via `sqlite3 ./tmp-build/data/tfr-base-*.sqlite`.
2. **Given** the local build directory already exists from a prior build, **When** the developer re-runs the same command, **Then** the new artifacts overwrite the prior ones (content-hashed filenames mean prior files with different hashes accumulate; the manifest references only the current build's hashes).

---

### Well-ordered publish (P3)

True cross-system atomicity (Postgres ↔ destination) isn't achievable without distributed transactions. Instead, publish is **well-ordered**: file PUTs happen first with the manifest LAST, then the `artifact_version` INSERT. At every intermediate point, a failure leaves a state that either has nothing visible (manifest not yet PUT, no row) or a state that's safely re-runnable (idempotent content-hashed PUTs + `ON CONFLICT (version_label) DO NOTHING` on the insert). The `artifact_version` row's existence implies the manifest URL is fetchable; partial-failure states can leave orphan files in the destination but never a row that lies.

When publish runs, file PUTs land first (manifest LAST so any partial-fail state has no entry point), then the `artifact_version` INSERT. The invariant the ordering achieves: **if an `artifact_version` row exists, the manifest URL it points to is fetchable from the destination**. The inverse (orphan files in the destination after a partial-failure publish, with no row pointing at them) is tolerated — orphans don't break anything since clients reach the version via the manifest URL only, and the manifest is the last file written. The invariant generalizes from the architecture's Cloudflare-R2-only framing: **`artifact_version` rows mean "fetchable from the destination recorded in `manifest_url`"** — `file://` for local destination, `https://...cdn...` for Cloudflare R2.

**Acceptance Scenarios**:

1. **Given** a Cloudflare R2 PUT fails partway through publish (network blip, 5xx from Cloudflare R2), **When** publish returns the error, **Then** `artifact_version` has no row for that version_label, AND the local build remains on disk so the developer can retry without re-running `build`. Any shards that succeeded upload before the failure remain in Cloudflare R2 as orphans — they're idempotently overwritten on retry (content-hashed key + same bytes = no-op PUT).
2. **Given** a local-destination publish where the copy of one shard fails (target disk full, permission denied), **When** publish returns the error, **Then** `artifact_version` has no row for that version_label; the published directory may contain a partial set but the missing manifest prevents any consumer from treating it as published.
3. **Given** every shard + manifest reaches the destination successfully, **When** `insert_artifact_version` then fails (DB connection drops, conflict on `version_label`), **Then** publish uses `ON CONFLICT (version_label) DO NOTHING` semantics — if a prior run already inserted an identical row (deterministic from canonical state, so the manifest_sha256 matches), the conflict is silently swallowed; otherwise the operator gets a clear AppError. Re-running publish is the recovery path either way.

---

### License-class shard partitioning (P4)

When sources with stricter license classes land (post-v1 — none today), values from each license class emit into their own shard. Clients identified as authorized for stricter classes `ATTACH DATABASE` the additional shards. v1's only data source (WB WDI, CC BY 4.0 → `attribution` license_class) maps to the `base` shard.

**Acceptance Scenarios**:

1. **Given** the canonical store has only WB WDI values, **When** build runs, **Then** only `data/tfr-base-<sha8>.sqlite` is emitted (no `share_alike` or `noncommercial` shards, because no sources of those license classes are seeded).
2. **Given** a future source with `license_class='noncommercial'` adds values for the `tfr` statistic, **When** build runs, **Then** two SQLite files are emitted: `data/tfr-base-<sha8>.sqlite` (the public_domain + attribution values) and `data/tfr-noncommercial-<sha8>.sqlite` (the noncommercial-license values); the manifest's `statistics.tfr` entry lists both.

---

### Deleting a published artifact (P5)

`ingestion delete <version-label> --destination=local|cloudflare-r2` removes a published artifact's `artifact_version` row AND best-effort removes its files from the destination. Operationally most important for **local storage**: the pre-ship periodic job accumulates one build per week (~5-10MB each), so over years a Mac mini's published-directory grows; deleting old versions reclaims space. Less critical for Cloudflare R2 — the free tier covers years of accumulation per `project_eafora_r2_pricing.md` — but the same flow handles both destinations uniformly. Like publish, delete is **well-ordered, not cross-system atomic**: the row deletion happens first (in a Postgres transaction), so the row is gone iff nothing else claims the manifest URL is fetchable; file deletes follow best-effort and may leave orphans (recoverable via re-run or future `fsck`).

**Acceptance Scenarios**:

1. **Given** an `artifact_version` row for `2026-05-18` published to local destination at `${repo_dir}/published/2026-05-18/`, **When** `ingestion delete 2026-05-18 --destination=local --local-dir ${repo_dir}/published` runs, **Then** the `artifact_version` row is DELETEd first; then every file under `${repo_dir}/published/2026-05-18/` is removed (including the version directory itself); subsequent `select * from artifact_version where version_label='2026-05-18'` returns zero rows.
2. **Given** an `artifact_version` row for `2026-05-18` published to Cloudflare R2, **When** `ingestion delete 2026-05-18 --destination=cloudflare-r2` runs, **Then** the row is DELETEd first; then every shard + geometry + manifest under that version's content-hashed keys is DELETEd from the bucket via signed S3 DELETE; the CDN stops serving the manifest URL (returns 404 after the 5-minute manifest cache TTL expires).
3. **Given** the row-delete succeeds but a file-delete fails partway (network blip mid-DELETE for Cloudflare R2; permission denied removing a file locally), **When** delete returns the per-file error(s), **Then** the `artifact_version` row stays gone (the row deletion already committed before file deletes started), the destination has orphan files for that version; recovery is operator-driven (manually delete the orphans) or a future `fsck` command. Re-running `delete <version-label>` after the row is already gone is a no-op (no row to find).

---

### Edge Cases

- **Cloudflare R2 credentials missing or invalid** — publish fails fast with an `AppError` naming the secret it tried to read; no Cloudflare R2 PUTs attempted; no `artifact_version` row.
- **Natural Earth download fails or 404s** — build aborts with an `AppError`; no partial shard set written. The pinned upstream release URL is the source of truth; a 404 means the release was moved/removed and the version pin needs updating.
- **Statistic has no rows in canonical store** — the corresponding SQLite shard is NOT emitted (zero-byte SQLite would be misleading); the manifest's `statistics` map omits that statistic entirely. **If the statistic IS present in the `statistic` table but has zero matching `statistic_value` rows**, the build emits a warning identifying the orphaned statistic (it was registered as a statistic Eafora expects to publish, but no source contributed values — usually a data-source-not-yet-wired situation worth flagging).
- **License-class shard with no rows** — that variant is NOT emitted; the manifest entry for the statistic omits that class.
- **Re-running `build` with an identical canonical store** — produces shards with identical content hashes (the build is deterministic from canonical state); manifest content hash also matches; safe to re-publish but semantically a no-op.
- **Two builds in flight against the same canonical state** — racing builds emit different `*.tmp-<uuid>` files; rename-to-hashed-name only happens after hashing succeeds; final files don't collide because the content hash is the same.
- **Manifest write succeeds but its own SHA-256 hash computation fails** — currently can't happen (we hash an in-memory buffer before writing); spec'd as an `AppError` in case the implementation changes to a streaming approach.
- **Existing `artifact_version` row with the same `version_label`** — unique constraint violation surfaces as an `AppError`; the publish step does not silently overwrite. Operator picks a new label or explicitly deletes the old row.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST implement `build_artifacts(pool, output_dir, version_label) -> Result<LocalArtifactBuild, AppError>` exactly per `docs/architecture/ingestion.md` §Artifact builder, including all seven named helpers (`read_candidate_values`, `collect_data_source_versions`, `apply_source_priority`, `emit_sqlite_shards`, `emit_geometry_flatgeobuf`, `compute_content_hashes`, `emit_manifest`).
- **FR-002**: System MUST implement `publish_artifacts(store, pool, build) -> Result<ArtifactVersion, AppError>` per `docs/architecture/ingestion.md` §Cloudflare R2 upload as amended by this spec: destination-agnostic via an `ArtifactRepository` trait, with `LocalArtifactRepository` and `CloudflareR2ArtifactRepository` as the two concrete impls (plus a `DryrunArtifactRepository` for tests). The local build MUST NOT be modified or deleted by publish; recovery from publish failure is re-running publish.
- **FR-002a**: The `ArtifactRepository` trait MUST define one method: `async fn put(&self, key: &str, body: Bytes, cache_control: &str) -> Result<String, AppError>` returning the URL the file is fetchable from. Composition over enum dispatch: new destinations (GCS, in-process, etc.) drop in by implementing the trait without changing `publish_artifacts`.
- **FR-003**: System MUST read candidate values via SELECT from `statistic_value` joined to `data_source` (for `license_class`, `preference_rank`) and `statistic` (for `code`), filtered to `superseded is null`. v1 holds the full set in memory (≈13k rows × growth headroom for additional statistics).
- **FR-004**: System MUST apply the source-priority merge per `docs/architecture/ingestion.md` §Source priority: group candidates by `(region_id, statistic_id, period_start, period_end, license_class)`; within each group, lowest `preference_rank` wins; ties broken deterministically by `data_source.id`; data-status overrides where applicable.
- **FR-005**: System MUST emit one SQLite shard per `(statistic_code, license_class)` group with at least one merged value. Filename pattern: `data/<statistic_code>-<license_class>-<sha8>.sqlite` where `<license_class>` is one of `base | share_alike | noncommercial` per the §License-class shard mapping table. Each SQLite file has one table per shard's schema (TBD in plan).
- **FR-006**: System MUST download the pinned Natural Earth 1:50m Cultural Vectors release (`ne_50m_admin_0_countries.zip`), process the shapefile in-memory (no temp-file extraction beyond what's needed to parse it), join geometries to `country.iso3` via Natural Earth's `ADM0_A3` field, and write a FlatGeobuf file at `geometry/world-50m-<sha8>.fgb`. The Natural Earth release version is pinned in code; bumping it is a code change, not a runtime config.
- **FR-007**: System MUST content-hash every shard + the manifest using SHA-256. Shard filenames use the first 8 hex chars (`-<sha8>`) suffix; the manifest carries the full hashes. The manifest filename itself is `manifest.json` with no hash suffix.
- **FR-008**: System MUST emit `manifest.json` matching the schema in `docs/architecture/ingestion.md` §Manifest format: `version`, `artifact_created`, `geometry`, `statistics` (nested by statistic code → license class → `{url, size_bytes, sha256}`), and `source_versions` (the `data_source_versions_jsonb` snapshot).
- **FR-009**: For the Cloudflare R2 store impl, system MUST upload every shard + manifest via the S3-compatible PUT API; credentials read from `secr` (key names listed in plan.md). Cache-Control headers: `public, max-age=31536000, immutable` on shards + geometry; `public, max-age=300` on `manifest.json`. For the local store impl, system MUST copy files into `<published_dir>/<version_label>/<key>` with no equivalent of Cache-Control (irrelevant for filesystem reads).
- **FR-010**: System MUST insert one `artifact_version` row only after every put succeeds, with `version_label`, `manifest_sha256`, `manifest_url` matching the URL the store's `put` call returned for the manifest (`https://<cdn>/...` for Cloudflare R2, `file://...` for local), and `data_source_versions_jsonb` matching the manifest's `source_versions`. The INSERT uses `ON CONFLICT (version_label) DO NOTHING` so re-runs after a transient DB failure don't double-insert; the row that survives is the one already there, which (by build determinism) is byte-equivalent to what the re-run would have written.
- **FR-011**: System MUST wire the build + publish flows into `main.rs`: `ingestion build <output-dir> <version-label>` runs local-only build; `ingestion publish <version-label> [cloudflare-r2|r2] [--local-dir=<path>] [--dry-run]` runs publish + record. Pre-product-ship the launchd job uses `cloudflare-r2`; post-ship it flips to `--destination=r2` via a one-flag edit to the plist template.
- **FR-012**: System MUST cover the pure-function helpers (`collect_data_source_versions`, `apply_source_priority`, manifest construction, content hashing) with TDD unit tests per Constitution VII. DB-touching helpers (`read_candidate_values`, `insert_artifact_version`) are covered by integration tests against `eafora_test`. Both store impls have integration tests: `LocalArtifactRepository` against a `tempdir()`, `CloudflareR2ArtifactRepository` against a real Cloudflare R2 bucket once (manual verification per FR-014); `DryrunArtifactRepository` is the test-time impl that exercises the publish orchestrator end-to-end without I/O.
- **FR-013**: System MUST treat the local build directory as the source of truth between build and publish — `publish` reads the manifest + shards from disk and uploads/copies what's there. The CLI MUST NOT re-execute `build_artifacts` inside publish; the operator runs `build` explicitly first.
- **FR-014**: System MUST verify the Cloudflare R2 store impl works end-to-end against a real Cloudflare R2 bucket at least once before PR B integrates (manual run; documented in task T034). After that one verification, the periodic launchd job uses the local destination until product ships.
- **FR-015**: System MUST implement `delete_artifact(store, pool, version_label) -> Result<DeleteReport, AppError>` and a `ingestion delete <version-label> --destination=local|cloudflare-r2` CLI dispatch. **Well-ordered, not cross-system atomic**: open a Postgres transaction, SELECT the row (to read the manifest URL needed for file enumeration), DELETE the row, COMMIT — then best-effort delete each referenced file. Per-file delete failures are accumulated into `DeleteReport.orphan_files` and logged as warnings; they don't fail the operation (the row is already gone; the orphans are a storage-cleanup concern, not a correctness one).
- **FR-016**: The `ArtifactRepository` trait MUST include `async fn delete(&self, key: &str) -> Result<(), AppError>` alongside `put`, implemented by `LocalArtifactRepository` (filesystem remove + empty-dir cleanup), `CloudflareR2ArtifactRepository` (signed S3 DELETE), and `DryrunArtifactRepository` (record + no-op).

### Key Entities

- **`LocalArtifactBuild`**: a plain owned struct returned by `build_artifacts`, holding `version_label`, `hashed_outputs` (every shard + geometry's path + size + hash), `manifest_output` (the manifest's path + size + hash), and `data_source_versions` (the BTreeMap snapshot). Passed to `upload_artifacts_to_cloudflare_r2` to publish.
- **`ShardOutput`**: per-file metadata (path, size, sha256). Used uniformly for SQLite shards, the geometry file, and the manifest.
- **`HashedOutputs`**: groups `ShardOutput`s by category (shards, geometry) for the manifest emission step.
- **`CandidateValue`**: a row read from `statistic_value` joined to its `data_source` + `statistic`, carrying everything the merge needs. Lifetime: in-memory only during a build run.
- **`MergedValue`**: a row produced by the source-priority merge, ready for SQLite shard emission. Carries `(region_id, statistic_id, period_start, period_end, license_class, value, data_status, data_source_revision)`.
- **`ArtifactVersion`**: the persisted row in `artifact_version`; one per successful publish.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After a fresh `ingestion build ./out 2026-05-26` against a canonical store with the v1 WB WDI data, the output directory contains exactly one SQLite shard for `tfr-base`, exactly one FlatGeobuf geometry shard, and exactly one `manifest.json`. Total shard size is under 200 KB; geometry shard is under 5 MB.
- **SC-002**: A scheduled `build + publish` cycle completes end-to-end (read canonical → emit shards → download + process geometry → upload to Cloudflare R2 → insert artifact_version) in under 60 seconds on the Mac mini.
- **SC-003**: For any artifact version `V`, the `manifest_url` recorded in `artifact_version` returns HTTP 200 from the CDN and the manifest's referenced shard URLs all return HTTP 200. ("`artifact_version` rows mean fetchable" invariant.)
- **SC-004**: For any `version_label`, two consecutive `build` runs (no canonical changes between) produce SQLite shards with identical SHA-256 content hashes. (Determinism — the build is a pure function of canonical state.)
- **SC-005**: The pure-function helpers (`collect_data_source_versions`, `apply_source_priority`, manifest serialization, SHA-256 hashing) achieve ≥90% line coverage in the test suite. DB and HTTP helpers are exercised by integration tests; their coverage is informational, not gating.
- **SC-006**: An operator can answer "what was published at version V?" via the `artifact_version` row alone — without consulting external snapshots — by following `manifest_url` to the manifest and `data_source_versions_jsonb` for the source revisions baked into that build.

## Assumptions

- The canonical schema (`region`, `country`, `statistic`, `data_source`, `data_source_publication`, `statistic_value`, `artifact_version`) is applied via dbmate before this feature runs.
- The 001-wb-wdi-ingestion feature is in place: the canonical store has WB WDI data and the `wb_wdi` `data_source` row exists.
- Natural Earth 1:50m Cultural Vectors is available at `naciscdn.org` at the pinned URL. The shapefile's `ADM0_A3` field continues to match ISO 3166-1 alpha-3 codes for the countries we ship; rare mismatches (Kosovo's `KOS` etc.) are tolerated as missing-geometry warnings, not blockers.
- Cloudflare R2 credentials (access key ID, secret access key, bucket name, account ID for the endpoint URL) are stored in `secr` under documented secret keys. Local-dev publish either uses a development bucket or skips the upload via a flag.
- For the upload integration test, the chosen approach is either (a) a mock S3 server (e.g. `minio` or a hand-rolled tiny mock) or (b) a `--dry-run` flag on publish that exercises the code path without actually PUTting. Decision deferred to plan.md.
- Per Constitution Principle V (explicit over implicit), the Cloudflare R2 upload uses raw `reqwest` against the S3 API rather than the `aws-sdk-s3` crate. The architecture doc names this preference.
- The SQLite schema inside each shard is a single `values` table per shard with columns sufficient for client-side queries by region + period; exact schema is a plan-level decision.
- The FlatGeobuf shape (one feature per country, properties limited to `iso3` + `name_en`, geometry as projected to WGS84) is what we ship for v1; geometry simplification is deferred — Natural Earth's 1:50m is already lightweight.
- This feature does NOT include the artifact-builder CLI's interaction with the geometry-ingest pipeline beyond the in-memory download + process; subnational geometry, statistic-specific geometry overlays, or anything past country boundaries is out of scope.

## Constitution Check

Per Constitution §Compliance review, this spec honors the binding principles as follows:

- **Principle I (Educational neutrality)**: not directly applicable — the feature emits structured data + geometry; no UI text or editorial copy.
- **Principle II (Source provenance — NON-NEGOTIABLE)**: directly served. Every shard row carries its source via the merge step's `data_source_id` lineage; the manifest's `source_versions` records the revision label of every source contributing to the build; the `artifact_version` row records the snapshot operators can trace any client-visible datum back to.
- **Principle III (Rust core, native UI shells)**: applies — pure Rust module in the `ingestion/` binary. No UI or FFI.
- **Principle IV (Singularity convention parity)**: applies — uses `reqwest`, `sqlx::query_as!`, tokio per the locked picks. New deps (SQLite library, FlatGeobuf library, SHA-256 hasher) are planned-level decisions and must be approved per `feedback_eafora_library_conventions.md`.
- **Principle V (Explicit over implicit)**: applies — raw `reqwest` for Cloudflare R2 (no SDK), hand-written SQL, manual file orchestration. No frameworks hiding the wire/disk operations.
- **Principle VI (CDN-delivered data, no live API through v2)**: directly served. This feature IS the path from canonical store to CDN; without it, the CDN-only delivery model in the architecture has no producer.
- **Principle VII (Test-first for core logic)**: applicable to the pure-function helpers (merge logic, content hashing, manifest serialization). FR-012 codifies this; SC-005 measures it.
- **Principle VIII (Workflow discipline)**: this is the second `/speckit-specify` feature; spec/plan/tasks land in the same PR per `feedback_spec_and_plan_same_pr.md`.

No principle violations identified; no constitution amendments proposed.
