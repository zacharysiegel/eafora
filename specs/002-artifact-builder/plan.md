# Implementation Plan: Artifact builder + Cloudflare R2 publish

**Feature**: 002-artifact-builder

**Spec**: `specs/002-artifact-builder/spec.md`

## Technical Context

### Dependencies (additions to `[workspace.dependencies]`)

| Crate         | Purpose                                                                                                                                                             | Wildcard pin |
|---------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------|--------------|
| `rusqlite`    | Embedded SQLite writer for per-statistic shards. Bundled feature ships SQLite source so no system library is required.                                              | `0.32.*`     |
| `flatgeobuf`  | FlatGeobuf writer (the upstream library, MIT-licensed).                                                                                                             | `4.6.*`      |
| `geozero`     | Pulled in via `flatgeobuf`; provides the GeoJSON / WKB conversion the writer needs.                                                                                 | (transitive) |
| `shapefile`   | Reads the Natural Earth `.shp` from the downloaded zip.                                                                                                             | `0.7.*`      |
| `zip`         | Unzips the Natural Earth release in-memory (no temp files).                                                                                                         | `4.5.*`      |
| `sha2`        | SHA-256 hashing for content-hashed filenames + manifest.                                                                                                            | `0.10.*`     |
| `aws-sigv4`   | Signs the S3 PUT requests for the Cloudflare R2 store impl; thin enough to keep "explicit over implicit" intact (the raw `reqwest::Client` still issues the calls). | `1.3.*`     |

**Why these specific picks** (per `feedback_eafora_library_conventions.md` — ask before adding deps):

- `rusqlite` over `sqlx::sqlite` — sqlx pulls in async runtime baggage we don't need for a per-build local file writer; rusqlite is sync, simple, has a much smaller dep tree, and matches the "explicit over implicit" preference.
- `flatgeobuf` is the only viable FlatGeobuf writer in Rust.
- `aws-sigv4` (not the full `aws-sdk-s3`) — we want to see the actual HTTP request being built; the SDK hides too much.
- No `async-trait` — `ArtifactRepository` uses native async-fn-in-trait (stable since 2024 edition) + static dispatch via `<S: ArtifactRepository>` generic bounds. Avoids the boxing overhead and the macro dependency; the CLI dispatch monomorphizes per destination at the call site.
- No new test deps. Mocking the Cloudflare R2 store happens via a `Dryrun` `ArtifactRepository` impl that validates inputs without doing I/O (see Test harness design).

### Storage abstraction

Storage targets are an `ArtifactRepository` trait with native async-fn-in-trait + static dispatch. New destinations (GCS, in-process for tests, dry-run) drop in by implementing the trait; the publish orchestrator is generic over `S: ArtifactRepository` and monomorphizes per implementor:

```rust
pub trait ArtifactRepository: Send + Sync {
    /// Publishes one file under `key` (the path relative to the destination root,
    /// e.g. "data/tfr-base-ab12cd34.sqlite") with the given Cache-Control header.
    /// Returns the URL the file is now fetchable from (https://...cdn... for Cloudflare R2,
    /// file://... for local).
    async fn put(&self, key: &str, body: Bytes, cache_control: &str) -> Result<String, AppError>;

    /// Removes one file under `key`. Used by `delete_artifact`'s best-effort cleanup.
    async fn delete(&self, key: &str) -> Result<(), AppError>;
}

pub struct LocalArtifactRepository {
    pub published_dir: PathBuf,  // e.g. ${repo_dir}/published
    pub version_label: String,   // every put writes to <published_dir>/<version_label>/<key>
}

pub struct CloudflareR2ArtifactRepository {
    pub client: reqwest::Client,
    pub credentials: aws_sigv4::sign::v4::SigningParams,
    pub account_id: String,
    pub bucket_name: String,
    pub cdn_base_url: String,    // e.g. https://artifacts.eafora.org
    pub version_label: String,   // every put writes to <bucket>/<version_label>/<key>; URL returned is <cdn_base_url>/<version_label>/<key>
}

pub struct DryrunArtifactRepository {
    // Records every put/delete call for test assertions; no I/O.
    pub version_label: String,
    pub recorded: Mutex<Vec<RecordedOp>>,
}
```

**Path scheme (Option A — per-version namespace, symmetric across destinations)**: every key passed to `put` is namespaced under the repository's `version_label`. A `put("data/tfr-base-<sha>.sqlite", ...)` writes to `<published_dir>/<version_label>/data/tfr-base-<sha>.sqlite` for local, or `<bucket>/<version_label>/data/tfr-base-<sha>.sqlite` for Cloudflare R2. Deletion is trivial — `rm -rf` the local subdir, or list-and-delete the bucket prefix. Identical content-hashed shards across versions get duplicated; the dedup loss is ~5MB per version (free at our scale on both destinations per `project_eafora_r2_pricing.md`).

`upload_artifacts_to_cloudflare_r2` becomes `publish_artifacts<S: ArtifactRepository>(repository: &S, pool: &PgPool, build: &LocalArtifactBuild) -> Result<ArtifactVersion, AppError>` — generic, no `&dyn`. The trait + impls live in `ingestion/src/artifact/repository/{mod.rs, local.rs, cloudflare_r2.rs, dryrun.rs}`; `publish_artifacts` lives in `ingestion/src/artifact/publish.rs`.

The CLI's `dispatch_publish` parses the destination flag and monomorphizes per arm:

```rust
let artifact_version: ArtifactVersion = match destination {
    Destination::Local { published_dir } => {
        let repository: LocalArtifactRepository = LocalArtifactRepository { published_dir, version_label: version_label.clone() };
        publish_artifacts(&repository, &pool, &build).await?
    }
    Destination::CloudflareR2 => {
        let repository: CloudflareR2ArtifactRepository = construct_cloudflare_r2_store().await?;
        publish_artifacts(&repository, &pool, &build).await?
    }
    Destination::Dryrun => {
        let repository: DryrunArtifactRepository = DryrunArtifactRepository::new();
        publish_artifacts(&repository, &pool, &build).await?
    }
};
```

Three monomorphizations of `publish_artifacts` get compiled — small (~50 LOC each), no boxing per call, the trait methods inline into the surrounding code. `dispatch_delete` mirrors the same shape for `delete_artifact<S: ArtifactRepository>(...)`.

### CLI flags

```sh
# Defaults to --destination=local
ingestion publish <version-label> \
    [--destination=local|cloudflare-r2] \
    [--local-dir=<path>]              # required if --destination=local; defaults to ${repo_dir}/published
    [--dry-run]                       # constructs the store, calls put(), validates signing/paths but no actual I/O
```

### Module layout

```
ingestion/src/artifact/
├── mod.rs
├── artifact.rs                 # build_artifacts orchestrator + helpers in artifact_db / artifact_model that the orchestrator sequences
├── artifact_model.rs           # CandidateValue, MergedValue, LocalArtifactBuild, ShardOutput, HashedOutputs, ArtifactVersion (the canonical entity belongs here too since it's only used by this feature)
├── artifact_db.rs              # read_candidate_values + insert_artifact_version (sqlx queries)
├── source_priority.rs          # apply_source_priority — pure logic; TDD surface
├── content_hashing.rs          # compute_content_hashes — SHA-256 + the *.tmp.<uuid> → <name>-<sha8>.<ext> rename dance
├── publish.rs                  # publish_artifacts orchestrator (generic over S: ArtifactRepository)
├── writer/                     # output-shape writers (one file per output format)
│   ├── mod.rs                  # pub mod sqlite; pub mod flatgeobuf; pub mod manifest;
│   ├── sqlite.rs               # emit_sqlite_shards
│   ├── flatgeobuf.rs           # emit_geometry_flatgeobuf
│   └── manifest.rs             # emit_manifest — builds + hashes manifest.json
├── repository/
│   ├── mod.rs                  # ArtifactRepository trait; pub mod local; pub mod cloudflare_r2; pub mod dryrun;
│   ├── local.rs                # LocalArtifactRepository impl
│   ├── cloudflare_r2.rs        # CloudflareR2ArtifactRepository impl (raw reqwest + aws-sigv4)
│   └── dryrun.rs               # DryrunArtifactRepository impl for tests
└── geometry/                   # in-tree subdirectory for the Natural Earth processing; eventually lifts to a top-level geometry/ when subnational lands
    ├── mod.rs                  # pub mod natural_earth;
    └── natural_earth.rs        # pinned URL, zip extraction, shapefile → in-memory features
```

Per `feedback_no_per_source_test_helper_modules.md`: artifact-builder integration tests live in `ingestion/tests/artifact_integration.rs`; any helpers used only there inline as private fns.

### SQLite shard schema

```sql
create table statistic_value (
    region_iso3   text    not null,
    region_id     blob    not null,        -- UUID-as-blob for compactness + client-side index reuse
    period_start  text    not null,        -- ISO 8601 date 'YYYY-MM-DD'
    period_end    text    not null,
    value         real    not null,
    data_status   text    not null,
    data_source_code     text not null,
    data_source_revision text not null,
    primary key (region_iso3, period_start, period_end)
);
create index statistic_value_by_region on statistic_value (region_id);
```

`region_iso3` is for human-readable client queries; `region_id` is for the rare cross-shard joins. Period as text (ISO 8601) so client queries are naturally string-comparable across all SQLite versions without date-function dependencies.

### Manifest serialization

`serde_json` with a typed `Manifest` struct + serde derives — matches `serde_json` usage everywhere else. Field order in the JSON output is fixed (BTreeMap on statistic codes for deterministic builds). Pretty-printed with two-space indents for human readability.

### Cloudflare R2 upload mechanics

The Cloudflare R2 bucket is reachable via the S3 endpoint `https://<account-id>.r2.cloudflarestorage.com`. We construct each request manually:

```rust
let signed_request = aws_sigv4::http_request::sign(/* PUT, headers, body */, &credentials, &signing_settings)?;
let response = reqwest_client.put(url).headers(signed_headers).body(body).send().await?;
```

Credentials come from `secr`-encrypted secrets at keys (TBD; documented in setup.sh as the credentials get added):
- `cloudflare_r2.access_key_id`
- `cloudflare_r2.secret_access_key`
- `cloudflare_r2.account_id`
- `cloudflare_r2.bucket_name`

### Constitution alignment

- **Principle V (Explicit over implicit)**: hand-written SQL for `read_candidate_values` and `insert_artifact_version`; raw `reqwest` + `aws-sigv4` for S3 PUTs (no SDK); SQLite writes via `rusqlite::Connection::execute_batch` with hand-written DDL.
- **Principle VII (Test-first)**: `apply_source_priority`, manifest serialization, content hashing, SHA-256-from-bytes, and the `name.tmp.<uuid> → name-<sha8>.<ext>` rename helper are pure functions and follow Red-Green-Refactor.
- **Principle IV (Singularity convention parity)**: every new dep above is added to `[workspace.dependencies]` with the wildcard pin; consumer crates use `{ workspace = true }`.

### Module-layout decision

Single-project layout (the `ingestion/` workspace member, same as 001). Module layout above expands the existing per-feature pattern: each artifact concern is its own file under `ingestion/src/artifact/`. The `geometry/` subdirectory inside `artifact/` is the v1 home for Natural Earth processing; it lifts to a top-level `ingestion/src/geometry/` when subnational geometry support lands (per architecture doc line 74).

## Test harness design

- **Pure-logic tests** live in `#[cfg(test)] mod tests` blocks within their respective files (`source_priority.rs`, `content_hashing.rs`, `writer/manifest.rs`).
- **DB integration tests** live in `tests/artifact_integration.rs`. Each test opens a transaction on the existing `eafora_test` pool helper, exercises `read_candidate_values` / `insert_artifact_version`, and rolls back.
- **Cloudflare R2 integration test**: one test in `tests/artifact_integration.rs` runs `upload_artifacts_to_cloudflare_r2` with `--dry-run` on a real `LocalArtifactBuild` fixture (constructed from temp files via `tempfile`). Asserts: every shard's signed PUT URL is computed, the headers carry the right Cache-Control values, and `artifact_version` is NOT inserted (dry-run skips that step too).
- **End-to-end test**: one test runs the full `build` against `eafora_test` (which has the seeded canonical store but no values), inserts a small known fixture into `statistic_value`, runs the build, asserts the shard file exists and contains the expected rows when opened via `rusqlite`.

## Implementation phases

1. **Workspace setup** (Cargo deps): add the 7 crates above to `[workspace.dependencies]`; `ingestion/Cargo.toml` references each via `{ workspace = true }`.
2. **artifact_model + artifact_db** (T010-T013): types, `read_candidate_values`, `insert_artifact_version` scaffolded (returns dummy / not-yet-implemented until later phases).
3. **source-priority merge** (T014-T015, TDD): tests then implementation.
4. **SQLite shard writer** (T016-T018, TDD-light): tests cover the schema + row writing; FR-005 satisfied.
5. **FlatGeobuf geometry writer** (T019-T021): Natural Earth download (pinned URL, zip → shapefile in-memory), join to `country.iso3` table from DB, write `.fgb`.
6. **Content hashing + tmp-file rename** (T022-T023, TDD): tests then implementation.
7. **Manifest emission** (T024-T025, TDD): build the `Manifest` struct from `HashedOutputs`, serialize via `serde_json::to_string_pretty`, hash it, write to disk.
8. **build_artifacts orchestrator** (T026): chain phases 2-7; integration test against `eafora_test`.
9. **Cloudflare R2 publish** (T027-T031): `upload_files_to_cloudflare_r2` (signed PUTs via reqwest + aws-sigv4), `insert_artifact_version` with `ON CONFLICT (version_label) DO NOTHING`, `upload_artifacts_to_cloudflare_r2` orchestrator with the well-ordered publish flow (PUTs first with manifest LAST, then INSERT — partial failures leave orphan files but never a row that lies). `--dry-run` flag wires through `publish`.
10. **CLI wiring** (T032-T034): `dispatch_build`, `dispatch_publish`, `--upload` flag on build that chains the two.
11. **Polish** (T035-T038): coverage measurement on pure helpers, live build + dry-run publish timing per SC-002, architecture-doc amendments for any divergences, cleanup-merged.

## Phasing for PRs

This feature breaks naturally into three serial PRs, stacked linearly:

- **PR A** (`impl-artifact-build-local`): phases 1-8 — every artifact lands on disk; build CLI works; no Cloudflare R2 yet.
- **PR B** (`impl-artifact-publish-cloudflare-r2`): phase 9 — Cloudflare R2 upload + artifact_version recording; publish CLI works; `--dry-run` covers tests.
- **PR C** (`impl-artifact-cli-polish`): phases 10-11 — `build --upload` chained mode, coverage measurement, doc amendments.

Each PR includes its tasks block from `tasks.md` (subset), is reviewable independently, and stacks the next branch on the prior per `feedback_branch_per_body_of_work.md` and `feedback_serial_branches_must_stack.md`.

## Brief PR description (per `feedback_pr_description_style.md`, applies to PR A only — B and C get their own when cut)

> Implements `build_artifacts(pool, output_dir, version_label)` per `docs/architecture/ingestion.md` §Artifact builder: reads candidate values from the canonical store, applies the source-priority merge, emits per-statistic / per-license-class SQLite shards and a content-hashed FlatGeobuf geometry shard processed from the pinned Natural Earth release, writes `manifest.json` with full SHA-256 hashes referenced by content-hashed filenames. Adds the `build <output-dir> <version-label>` CLI dispatch. No Cloudflare R2 upload yet (lands in the stacked follow-up PR); the `artifact_version` table is unchanged by `build`.
