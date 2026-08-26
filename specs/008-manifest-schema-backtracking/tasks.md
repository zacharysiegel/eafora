# Tasks: manifest schema backtracking

**Feature**: 008-manifest-schema-backtracking | **Branch**: `manifest-schema-backtracking` | **Date**: 2026-08-26

**Input**: [spec.md](spec.md), [plan.md](plan.md)

Organized by the plan's phasing. The three PRs this list first proposed became one, for the reason recorded in plan.md: the feature's contract is a filename that producer and client must build from the same function, and a split diff shows only one end of it. Test-first throughout for the pure functions, per Constitution Principle VII. Every numbered task is one commit unless it says otherwise.

Read `docs/conventions/logging.md` before writing the log lines and `docs/conventions/types.md` before naming anything.

---

## Prerequisite: repair the publish test target

Own PR, branched off `master`. Nothing in Phase A can be demonstrated until this lands.

- [ ] T001 `./scripts/git/branch-init.sh publish-integration-repair` from a clean `master`.
- [ ] T002 Confirm the breakage before touching anything: `cargo check -p ingestion --tests`. Expected: `E0308` at ingestion/tests/publish_integration.rs:237, `expected &BundleProvenance, found &BTreeMap<DataSourceKind, SourceRevision>`.
- [ ] T003 Fix the `write_manifest` call at ingestion/tests/publish_integration.rs:232-240 to pass a `&BundleProvenance`, reading the current signature at ingestion/src/artifact/writer/manifest.rs:17-24 rather than guessing the shape. Touches only ingestion/tests/publish_integration.rs.
- [ ] T004 `cargo check -p ingestion --tests`. Expected: clean.
- [ ] T005 `cargo test -p ingestion --test publish_integration` against a live Postgres at `TEST_DATABASE_URL`. Expected: all five tests pass. Do not attempt this under `SQLX_OFFLINE`; the target's own `delete from artifact_version` query is absent from the committed `.sqlx` cache.
- [ ] T006 Commit explicit paths. Subject: `repair the publish integration target's write_manifest call`. Push, then `gh pr create --assignee @me`.

---

## Phase A: the producer writes the pointer

Branch `manifest-schema-pointer`, stacked on the prerequisite branch. Rebase `--onto master` once the prerequisite squash-merges.

- [ ] T007 `./scripts/git/branch-init.sh manifest-schema-pointer`.

### The key

- [ ] T008 Write the failing test first, in `shared/src/artifact/manifest.rs`'s existing `#[cfg(test)] mod tests`: `schema_pointer_key(2)` equals the literal `"latest/manifest.2.json"`, and `schema_pointer_key(11)` equals `"latest/manifest.11.json"`. Use literals, not `MANIFEST_SCHEMA_VERSION`, so the assertion keeps pinning the wire contract after the constant bumps. Run `cargo test -p shared schema_pointer_key`. Expected: does not compile, the function does not exist.
- [ ] T009 Add `pub fn schema_pointer_key(manifest_schema_version: u32) -> String` to shared/src/artifact/manifest.rs, directly beside `MANIFEST_LATEST_KEY` at :20, returning `format!("latest/manifest.{manifest_schema_version}.json")`. Parameter typed `u32` to match `MANIFEST_SCHEMA_VERSION` at :13. Give it a doc comment stating only what the pointer is for and that it freezes when the constant bumps; do not restate the format string. Run `cargo test -p shared`. Expected: PASS.

### The upload

- [ ] T010 Write the failing assertions first, in `ingestion/tests/publish_integration.rs`:
  - In `publish_artifacts_uploads_every_file_to_local_repository_and_inserts_artifact_version` (:33), after the existing latest-pointer assertions at :61-63, assert the pointer exists at `destination_dir.path().join(manifest::schema_pointer_key(manifest::MANIFEST_SCHEMA_VERSION))` and is byte-equal to the versioned manifest and to `latest/manifest.json`. Build the key from the function, never from a literal, matching the file's existing idiom.
  - In `publish_local_keeps_only_the_two_newest_version_directories` (:96), after the third publish, assert the pointer still exists. This pins the property that a key inside `latest/` is exempt from retention.
  - Run `cargo test -p ingestion --test publish_integration`. Expected: both new assertions fail, because nothing writes the pointer.
- [ ] T011 Add the upload in `ingestion/src/artifact/publish.rs`, immediately **before** the existing `MANIFEST_LATEST_KEY` upload at :69, not after, so `latest/manifest.json` stays the last object a publish writes. Bind the key to a `String` with an explicit type annotation, reuse `&build_report.artifacts.manifest.path` and `bundle::CONTENT_TYPE_MANIFEST` exactly as :55 and :69 do, and log at the level and in the shape :70 uses (`<prose>; [key=...]`). Do not add a comment narrating the upload; if the before-not-after placement needs a note, state that constraint and stop. Run `cargo test -p ingestion --test publish_integration`. Expected: PASS.
- [ ] T012 Confirm the dry path needs no change: run a dry publish and check the added key appears in the output through the existing log at ingestion/src/artifact/repository/dry_artifact_repository.rs:15-20. No code change expected; if one is needed, it belongs here.
- [ ] T013 Update `ingestion/src/artifact/publish.rs`'s module doc to state the ordering invariant that `latest/manifest.json` is the last object written, if it does not already. One line.
- [ ] T014 Commit explicit paths. Subject: `publish a stable manifest pointer per schema version`. Push, then `gh pr create --assignee @me`.

---

## Phase B: the client reads it

Branch `manifest-schema-fallback`, stacked on `manifest-schema-pointer`. Rebase `--onto master` once Phase A squash-merges.

- [ ] T015 `./scripts/git/branch-init.sh manifest-schema-fallback`.

### Reading the version out of a document

- [ ] T016 Write the failing tests first, in `shared/src/artifact/schema_version.rs`'s test module: `read_schema_version` returns the found value for a document whose version does not match the reader's, errors for a document missing the field, errors for a field that is not an integer, and errors for bytes that are not JSON. Run `cargo test -p shared read_schema_version`. Expected: does not compile.
- [ ] T017 Extract `pub fn read_schema_version(bytes: &[u8], field_name: &str) -> Result<u64, AppError>` out of `require_schema_version` (shared/src/artifact/schema_version.rs:12-27), taking the `from_slice`, `get`, `as_u64`, and missing-field error verbatim, and reduce `require_schema_version` to a call plus the existing comparison. The `u64` return is forced by `as_u64` being serde_json's only unsigned accessor; the comment at :20-21 already records why the comparison widens rather than narrows, so do not restate it on the new function. Run `cargo test -p shared`. Expected: PASS, including shared/src/artifact/schema_version.rs:53-59 and shared/src/artifact/manifest.rs:174-184 unmodified. Editing either of those assertions means the extraction changed a message and is wrong.
- [ ] T018 Hoist the field name: add a `pub const` for `"manifest_schema_version"` in shared/src/artifact/manifest.rs beside the other manifest constants and use it at :84. Search the repository for every other occurrence of the literal and replace them all in this commit, not one at a time.

### The fallback decision

- [ ] T019 Write the failing truth table first, in shared/src/artifact/manifest.rs's test module, over `valid_manifest_json()` (:125-139) mutated by `replace`: one version above the reader yields `Some` holding the reader's own pointer key (not the found version's); the reader's own version, one below, a missing field, and a body that is not JSON all yield `None`. Interpolate `MANIFEST_SCHEMA_VERSION` into the `replace` needle the way :177 already does. Run `cargo test -p shared schema_fallback_key`. Expected: does not compile.
- [ ] T020 Add `pub fn schema_fallback_key(bytes: &[u8]) -> Option<String>` to shared/src/artifact/manifest.rs below `parse_manifest`, reading the version through `schema_version::read_schema_version` and returning `Some(schema_pointer_key(MANIFEST_SCHEMA_VERSION))` only when the found version is strictly greater than `MANIFEST_SCHEMA_VERSION`. Every other case, including an error from the read, yields `None`. Run `cargo test -p shared`. Expected: PASS.
- [ ] T021 Repair `parse_manifest_ignores_unknown_fields` (shared/src/artifact/manifest.rs:208-217) in this commit. Its `replace` needle is `"manifest_schema_version": 1,` while the fixture at :128 emits `2`, so no substitution happens and the test re-parses the untouched valid fixture, passing without exercising unknown-field tolerance at all. Follow the sibling pattern at shared/src/artifact/discovery.rs:73-85, which declares a separate literal carrying the extra field. Run `cargo test -p shared parse_manifest`. Expected: PASS, and confirm it now fails if `deny_unknown_fields` is temporarily added, so the repaired test actually tests something.

### The fetch and the branch

- [ ] T022 Generalize the manifest fetch in `web/src/client/fetch.rs`: add `pub async fn fetch_manifest_at_key(repository_base_url: &str, key: &str) -> Result<Vec<u8>, AppError>` holding the existing body from :53-62, and reduce `fetch_manifest` to a call passing `manifest::MANIFEST_LATEST_KEY`. The base-URL trimming and `HttpCacheMode::Reload` stay in one place; only artifact bodies use `Default` (:76). Run `cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown`. Expected: clean.
- [ ] T023 Add the branch in `web/src/client/load.rs`, inside `load_live_bundle` (:133-147), between `resolve_repository` and `open_fetched_live_bundle`. Bind the decision to a named `Option<String>` first rather than matching the call directly, then select the bytes: `None` keeps the resolved bytes; `Some(key)` fetches the pointer and, on an error, logs a warning and falls through to the resolved bytes so the version-mismatch error is what surfaces. Log the taken fallback at `info`, which a release build keeps per commit 442b02d (eafora). Do not touch `resolve_repository`, the `tokio::join!` at :154-158, or `open_fetched_live_bundle`. Keep the branch free of local state so phase 0.2 can re-parameterize it mechanically. Run `cargo check -p web --lib --no-default-features --features hydrate --target wasm32-unknown-unknown` and `cargo test -p web --lib`. Expected: clean, 48 tests still passing.
- [ ] T024 Confirm the untouched surfaces by reading them, not by assuming: web/src/map/canvas/driver.rs:867-873 still has its single error arm, no rank guard was added at :876-880, and web/src/version_rank.rs and `evict_stale_versions` are unmodified. No commit if nothing changed.
- [ ] T025 Manual end-to-end check, which no automated test in this repository can replace. Publish once into the local static tree with `scripts/build/publish-web-local.sh`, edit that tree's `latest/manifest.json` to carry a `manifest_schema_version` one above the constant while leaving the pointer alone, run `cargo leptos watch`, and confirm from the console that the pointer key is requested and the bundle loads with no notice. Keep the publish count within the two versions local retention keeps, or the shards the pointer names will be gone. Record the result in §Deviations.
- [ ] T026 Commit explicit paths. Subject: `fall back to the newest manifest this client's schema version can read`. Push, then `gh pr create --assignee @me`.

### Landing

- [ ] T027 On the last PR of the stack, delete the schema-version backtracking entry from `docs/backlog.md` §Client and delete its sequence item from `docs/task-order.md`, per the standing rule that a landed item leaves both files.
- [ ] T028 Self-review pass before requesting review: scan every touched file for em dashes, function-name prefixes in log messages, missing explicit type annotations on `let` bindings, doc comments that narrate what the code does or name a downstream effect, magic strings that should reuse the new constant, and the untouched-surface claims in T024. Fix what the pass finds in this branch.

---

## Deviations

- **One PR, not three.** The prerequisite, Phase A, and Phase B landed on one branch. The owner's objection was that the producer half has no observable effect and the two halves have to ship together; the sharper reason is that the shared filename is a contract neither split diff would show both ends of.
- **T017 through T021 grew.** Two dormant test defects surfaced while hoisting the field-name constant, both caused by a fixture pinning a schema version the constant had since moved past. `parse_manifest_ignores_unknown_fields` declared a `replace` needle that no longer occurred, so it re-parsed an unmutated fixture and asserted nothing; it now mutates a needle that exists and asserts the mutation happened. `manifest_json` in web/src/client/load.rs claimed schema 1, so every version it seeded failed to parse, leaving `version_labels_newest_first_orders_same_date_labels_by_artifact_created` ranking two unrankable versions and passing only because the labels happen to sort the intended way. Both fixtures now interpolate `MANIFEST_SCHEMA_VERSION` and cannot go stale on a bump.
- **T023's shape.** The branch became a named function, `readable_manifest_bytes`, rather than inline statements in `load_live_bundle`, so the fallback reads as one decision and phase 0.2 can move it whole.
- **T025 passed.** With the served tree's `latest/manifest.json` edited to claim schema 3 and a hand-made `latest/manifest.2.json` holding the real schema-2 manifest, the client logged `the repository is at a newer schema version; [pointer=latest/manifest.2.json]`, loaded the bundle, and showed no notice. The tree was restored afterward.
- **A one-character fix outside the feature.** An em dash in schema_version.rs's module doc, against the project's own rule, removed while editing that file.
