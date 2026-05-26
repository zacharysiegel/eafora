# Tasks: Artifact builder + R2 publish

**Feature**: 002-artifact-builder

**Spec**: `specs/002-artifact-builder/spec.md`

**Plan**: `specs/002-artifact-builder/plan.md`

Task ordering reflects the three-PR stacked sequence in plan.md §Phasing for PRs. Within each PR, [Tests-first] tasks land before their implementation counterparts per Constitution VII.

---

## PR A: Local build (phases 1-8)

### Phase 1: Workspace setup

- [ ] T001 [Setup] Add `rusqlite = { version = "0.32.*", features = ["bundled"] }`, `flatgeobuf = "4.6.*"`, `shapefile = "0.7.*"`, `zip = "4.5.*"`, `sha2 = "0.10.*"`, `aws-sigv4 = "1.3.*"` to root `Cargo.toml`'s `[workspace.dependencies]`. Add the `{ workspace = true }` references to `ingestion/Cargo.toml`.
- [ ] T002 [Setup] Run `cargo check -p ingestion` to confirm the new deps compile against the existing tree.

### Phase 2: Artifact model + DB scaffolding

- [ ] T003 [Foundational] Create `ingestion/src/artifact/mod.rs` with the `pub mod` declarations + `pub use` re-exports for the module's public types.
- [ ] T004 [Foundational] Write `ingestion/src/artifact/artifact_model.rs`: `CandidateValue`, `MergedValue`, `LicenseShardClass` (enum: Base | ShareAlike | NonCommercial; with `from_license_class(license_class: &str) -> LicenseShardClass` and `as_str(self) -> &'static str`), `ShardOutput`, `HashedOutputs`, `LocalArtifactBuild`. Use `crate::canonical::canonical_model::DataStatus` and `NaiveDatePeriod` rather than redefining.
- [ ] T005 [Foundational] Move `ArtifactVersion` from `canonical_model` (if it's there) to `artifact_model` — it's only used by this feature, no other module references it.
- [ ] T006 [P] [Foundational] Write `ingestion/src/artifact/artifact_db.rs::read_candidate_values(executor) -> Result<Vec<CandidateValue>, AppError>`. Single sqlx `query_as!` joining `statistic_value` → `data_source` → `statistic`, filtered to `superseded is null`.

### Phase 3: Source-preference merge (TDD)

- [ ] T007 [Tests-first] Write unit tests for `apply_source_preference_merge` in `ingestion/src/artifact/source_preference_merge.rs::#[cfg(test)] mod tests`: (a) single source → output identical to input; (b) two sources with different preference_rank — lower wins; (c) two sources with same preference_rank — tie broken by data_source_id; (d) data_status override (`final` beats `provisional` per the architecture's merge rule); (e) different license_class groups don't merge across.
- [ ] T008 Implement `apply_source_preference_merge(candidates: Vec<CandidateValue>) -> Vec<MergedValue>` to make T007 pass. Pure function — no I/O. Group by `(region_id, statistic_id, period.start, period.end, license_class)`; within each group apply the merge.

### Phase 4: SQLite shard writer

- [ ] T009 [Tests-first] Write unit tests for the SQLite schema in `ingestion/src/artifact/sqlite_writer.rs::#[cfg(test)] mod tests`: emit_sqlite_shards writes the expected `values` table schema; rows match the input MergedValues; per-statistic per-license-class file split is correct.
- [ ] T010 Implement `emit_sqlite_shards(merged: &[MergedValue], output_dir: &Path) -> Result<Vec<ShardOutput>, AppError>` to make T009 pass. Uses `rusqlite::Connection::open` against `data/<statistic_code>-<license_class>-tmp.<uuid>.sqlite`, batches inserts in a transaction, returns ShardOutput (path + size; not yet hashed).
- [ ] T011 [P] [Foundational] Add `collect_data_source_versions(candidates: &[CandidateValue]) -> BTreeMap<String, String>` to `source_preference_merge.rs`. Pure function — reduces candidates to `(data_source.code → max data_source_revision)`.

### Phase 5: FlatGeobuf geometry writer

- [ ] T012 [Foundational] Write `ingestion/src/artifact/geometry_ingest/natural_earth.rs`: pinned `NATURAL_EARTH_URL` const (`https://naciscdn.org/naturalearth/50m/cultural/ne_50m_admin_0_countries.zip`), `download_pinned_release(client: &reqwest::Client) -> Result<Vec<u8>, AppError>`, `extract_shapefile_from_zip(zip_bytes: &[u8]) -> Result<ShapefileBytes, AppError>` where `ShapefileBytes` carries the four-file shapefile components in memory.
- [ ] T013 Implement `emit_geometry_flatgeobuf(executor, output_dir) -> Result<ShardOutput, AppError>` in `ingestion/src/artifact/flatgeobuf_writer.rs`: downloads + extracts via T012, reads features via `shapefile` crate, joins each feature's `ADM0_A3` to the canonical `country.iso3` table via `canonical_db`, writes a FlatGeobuf with properties `{iso3, name_en}` and the feature's geometry to `geometry/world-50m-tmp.<uuid>.fgb`.
- [ ] T014 [Tests-first] Test for `emit_geometry_flatgeobuf` in `tests/artifact_integration.rs`: downloads + processes the live Natural Earth release; asserts ~250 features emitted; asserts every feature's `iso3` resolves to a known country (allowing for the documented misses like Kosovo's `KOS`). Live HTTP — gated behind `#[ignore]` and run manually as part of T039 timing.

### Phase 6: Content hashing

- [ ] T015 [Tests-first] Write unit tests for `compute_content_hashes` in `ingestion/src/artifact/content_hashing.rs::#[cfg(test)] mod tests`: (a) hashes match `sha2::Sha256` over the file's bytes; (b) the rename `name-tmp.<uuid>.<ext>` → `name-<sha8>.<ext>` happens after hashing succeeds; (c) idempotent re-hash returns the same bytes; (d) one file's hashing error doesn't leave others renamed.
- [ ] T016 Implement `compute_content_hashes(shards: Vec<ShardOutput>, geometry: ShardOutput) -> Result<HashedOutputs, AppError>` to make T015 pass.

### Phase 7: Manifest emission (TDD)

- [ ] T017 [Tests-first] Write unit tests for `emit_manifest` in `ingestion/src/artifact/manifest_writer.rs::#[cfg(test)] mod tests`: (a) the serialized JSON matches the architecture's §Manifest format byte-for-byte for a known input; (b) statistic codes are sorted alphabetically (BTreeMap); (c) license-class entries within a statistic are sorted alphabetically too; (d) the manifest's own SHA-256 is computed correctly.
- [ ] T018 Implement `emit_manifest(hashed, version_label, data_source_versions, output_dir) -> Result<ShardOutput, AppError>` to make T017 pass.

### Phase 8: build_artifacts orchestrator

- [ ] T019 Implement `build_artifacts(pool, output_dir, version_label) -> Result<LocalArtifactBuild, AppError>` in `ingestion/src/artifact/artifact.rs`, chaining T006 → T008 → T010 → T013 → T016 → T018 per the architecture doc's code listing. Opens a single transaction for the canonical reads.
- [ ] T020 [US2] Wire `dispatch_build` in `main.rs` to call `artifact::build_artifacts(&pool, output_dir, version_label)` and log the resulting `LocalArtifactBuild` summary.
- [ ] T021 Integration test in `tests/artifact_integration.rs`: insert a small set of `statistic_value` rows into a transaction-scoped fixture, run `build_artifacts` against a `tempdir()`, assert: one SQLite shard exists with the expected rows, manifest.json is well-formed, all referenced files exist on disk.
- [ ] T022 Manual verification of PR A: `cargo run -p ingestion -- build /tmp/eafora-build-test 2026-05-27` against the canonical store; verify the output directory matches SC-001 (one tfr-base SQLite shard, one geometry shard, one manifest.json; total size <5MB).

---

## PR B: R2 publish (phase 9)

- [ ] T023 [Tests-first] Write unit tests for the AWS sigv4 signing logic in `ingestion/src/artifact/publish.rs::#[cfg(test)] mod tests`: assert that a known input (PUT method, bucket/key, fixed timestamp, fixed credentials) produces the expected `Authorization` header per the AWS sigv4 spec.
- [ ] T024 Implement `upload_files_to_r2(client, credentials, bucket, region, hashed, manifest) -> Result<(), AppError>` in `publish.rs`: for each ShardOutput, construct a signed PUT request via `aws_sigv4::http_request::sign`, set the appropriate `Cache-Control` header (`immutable` for shards/geometry; `max-age=300` for manifest), and issue the request via `reqwest`. Idempotent — re-running with the same content-hashed keys is a no-op overwrite.
- [ ] T025 [Foundational] Write `ingestion/src/artifact/artifact_db.rs::insert_artifact_version(executor, version_label, manifest_output, hashed, data_source_versions) -> Result<ArtifactVersion, AppError>`. Single `query_as!` insert; `manifest_url` resolves to the CDN URL via a `cdn_base_url` parameter (TBD config).
- [ ] T026 Implement `upload_artifacts_to_r2(pool, build) -> Result<ArtifactVersion, AppError>` orchestrator: calls T024 first; only if all uploads succeed does it call T025. Per FR-002, the local build is not modified or deleted on failure.
- [ ] T027 Add `--dry-run` flag to publish: skips the actual network PUT (the signed request is constructed and validated, just not sent); skips the `insert_artifact_version` insert. Tests use this to exercise the full code path without network.
- [ ] T028 [US3] Wire `dispatch_publish` in `main.rs` to read the local build from disk (manifest + referenced files), pass it through `upload_artifacts_to_r2`. The `<version-label>` arg matches a previously-built directory's manifest version.
- [ ] T029 Integration test in `tests/artifact_integration.rs`: build a small LocalArtifactBuild fixture in `tempdir()`, run `upload_artifacts_to_r2` with `--dry-run` against an `eafora_test` transaction, assert: no row in `artifact_version` (dry-run skips); every signed request was constructed; no panics.
- [ ] T030 Document R2 secret keys: `r2.access_key_id`, `r2.secret_access_key`, `r2.account_id`, `r2.bucket_name` — add to `template.env` as commented placeholders and to `setup.sh` as a `secr` read step (deferred to real credentials existing).
- [ ] T031 Manual verification of PR B: live publish against a developer R2 bucket; assert the manifest URL returns HTTP 200 and the shards do too.

---

## PR C: CLI polish + meta (phases 10-11)

- [ ] T032 Implement the `--upload` flag on `build` that chains `build_artifacts` directly into `upload_artifacts_to_r2` without round-tripping through disk (avoids re-reading the manifest from disk; uses the in-memory `LocalArtifactBuild`).
- [ ] T033 Update `scripts/eafora-ingestion.plist.template` (or fork it) to invoke `build /tmp/eafora-build-<timestamp> <date> --upload` after the `all` subcommand. Decision: chain into the same launchd job, or use a second plist? Default: same job — failure of build/publish surfaces in the launchd log alongside the ingestion run.
- [ ] T034 Run `cargo llvm-cov -p ingestion` and verify the pure-function helpers (`apply_source_preference_merge`, `collect_data_source_versions`, manifest writer, content hashing) achieve ≥90% line coverage per SC-005.
- [ ] T035 [P] Live timing of `build + publish` end-to-end against the v1 canonical store; verify SC-002 (<60s).
- [ ] T036 [P] If implementation surfaced any divergence from `docs/architecture/ingestion.md` §Artifact builder or §R2 upload, propose an architecture amendment in a follow-up.
- [ ] T037 [P] Run `./scripts/cleanup-merged.sh` after PRs A, B, C all integrate.

---

## Dependencies & execution order

```text
PR A:
  T001-T002 (setup) →
    T003-T006 (model + db scaffolding) →
      T007-T008 (merge, TDD) ↘
      T009-T011 (sqlite, TDD)      ↘
                                     T019 (build_artifacts orchestrator) → T020-T022 (CLI + verify)
      T012-T014 (geometry)         ↗
      T015-T016 (content hashing) ↗
      T017-T018 (manifest, TDD)  ↗

PR B (stacks on PR A's branch):
  T023-T024 (sigv4 + uploads) → T025-T026 (insert + orchestrator) → T027 (dry-run) → T028-T029 (CLI + tests) → T030-T031 (secrets + live verify)

PR C (stacks on PR B's branch):
  T032 (build --upload) → T033 (plist) → T034-T037 (polish)
```

## Parallel example: phase 2

```text
# T004, T005, T006 touch different files; can be drafted in any order or in parallel:
T004 src/artifact/artifact_model.rs
T005 src/artifact/artifact_model.rs (StatisticValue move) — actually merges with T004; do them in one pass
T006 src/artifact/artifact_db.rs
```

## Implementation strategy

PR A is the bulk of new code (~700 LOC + tests). PR B is small (~200 LOC). PR C is mostly polish. The three-PR sequence keeps each review tractable. If a PR turns out too large at write-time, split by phase boundary (e.g. PR A1 = phases 1-6, PR A2 = phases 7-8).
