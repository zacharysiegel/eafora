# Ingestion architecture

<!--
Status: draft, 2026-05-23. This document is the per-segment implementation plan for Eafora's ingestion service and canonical store, elaborating the §Ingestion + canonical store section of `docs/architecture/overview.md`. It does not relitigate cross-cutting decisions already locked in the overview or the constitution.

The first concrete feature spec under this plan will be the World Bank WDI ingestion CLI (the smallest end-to-end exercise of the canonical store + adapter contract + artifact builder). That spec will go through `/speckit-specify` and live at `specs/NNN-world-bank-wdi-ingestion/`.
-->

## Scope of this document

This document covers everything between **upstream data sources** and **CDN-published artifacts**:

- The canonical PostgreSQL store: schema, conventions, migrations
- The per-source adapter contract: signature, return type, error model
- Source-preference merge rules (when multiple sources cover the same datum)
- Geometry ingestion (Natural Earth, separate from statistic ingestion)
- The artifact builder: FlatGeobuf writer, SQLite shard writer, manifest writer, content-hashing, R2 upload
- Scheduling (Mac mini M1 + `launchd`)
- Local development: Postgres on the host (Homebrew + launchd), seeding, manual invocation
- Module layout within `ingestion/`
- Testing strategy for the TDD-required surfaces (per Constitution Principle VII)

Client-side artifact consumption (parsing, caching, querying) is **not** in scope — that's the per-platform client plans.

## Locked decisions referenced (not relitigated)

From the constitution and `docs/architecture/overview.md`:

- Rust core + actix-web binary; tokio with `features = ["full"]`; sqlx `query_as!` with offline cache; dbmate migrations; reqwest for outbound HTTP. (Constitution IV; overview §Ingestion)
- Singularity `lobby/` feature-triplet pattern: each feature module is `<feature>_api.rs` + `<feature>_db.rs` + `<feature>_model.rs`. (Constitution IV)
- Imperative actix-web routing via cascading `configurer` functions; no `#[get]`/`#[post]` macros. (Constitution V)
- Hand-written `sqlx::query_as!`; no ORM. (Constitution V)
- HTTP+JSON for any future client/API path; no RPC frameworks without explicit approval. (Constitution V)
- CDN-delivered immutable artifacts; FlatGeobuf geometry + SQLite statistics; content-hashed filenames; no live data API through v2. (Constitution VI as amended in v1.3.2)
- License-segmented SQLite shards via additive `ATTACH DATABASE` composition, not mutually-exclusive variants. (Overview §License-segmented SQLite shards)
- Mac mini M1 hosts Postgres + ingestion + artifact builder through v1; Cloudflare R2 + Pages for distribution; Cloudflare Tunnel dormant through v2. (Overview §Cost; project memory)
- UUIDv7 primary keys named `id`; timestamps named `created` / `modified` (not `_at` suffix), all `timestamp with time zone`; soft-delete is `deleted_at`. (Memory: db schema conventions)
- minimer for errors; secr for secrets; statics via `LazyLock`. (Constitution IV)

## Workspace placement

The ingestion binary lives at the workspace root in its own crate:

```
eafora/
├── core/                       # data models, math, projection, FFI surfaces (no sqlx)
├── ingestion/                  # this document's subject
│   ├── Cargo.toml              # depends on core, sqlx, reqwest, tokio, ...
│   ├── db/
│   │   ├── migrations/         # dbmate timestamped SQL files
│   │   └── schema.sql          # dbmate-generated cumulative schema
│   ├── samples/                # checked-in sample data for tests + local dev seeding
│   └── src/
│       ├── main.rs             # tokio CLI entrypoint; dispatches subcommands (ingest-source, run-all, build-artifacts, seed-samples, upload-artifacts); the run-all subcommand loops over the registered adapters inline
│       ├── lib.rs              # re-exports for tests
│       ├── error.rs            # minimer wiring + per-feature variant aggregation
│       ├── world_bank_wdi/     # one source = one feature module (the lobby/ triplet pattern)
│       │   ├── mod.rs
│       │   ├── world_bank_wdi_api.rs       # CLI handlers (lobby/-triplet position; actix-web route configurer would go here if/when an HTTP server mode is added post-v2)
│       │   ├── world_bank_wdi_db.rs        # sqlx queries scoped to this source's ingestion
│       │   └── world_bank_wdi_model.rs     # types (WDI API response shapes, normalization helpers)
│       ├── eurostat/                       # same shape per added source
│       ├── hfd/
│       ├── canonical/                      # cross-cutting reads of the canonical store
│       │   ├── canonical_api.rs            # CLI handlers for canonical-store inspection (lobby/-triplet position)
│       │   ├── canonical_db.rs             # shared sqlx queries
│       │   └── canonical_model.rs          # shared entity types
│       ├── artifact/                       # artifact builder
│       │   ├── artifact_api.rs             # CLI handlers for artifact build / inspection (lobby/-triplet position)
│       │   ├── artifact_db.rs              # queries that drive the build (read fact table)
│       │   ├── artifact_model.rs           # Manifest, ArtifactVersion, build options
│       │   ├── flatgeobuf_writer.rs        # geometry shard writer
│       │   ├── sqlite_writer.rs            # per-statistic, per-license-tier shard writer
│       │   └── manifest_writer.rs          # manifest.json builder + content hashing
│       └── geometry_ingest/                # Natural Earth ingestion (separate from statistic adapters)
```

Through v2 the `ingestion/` binary is a CLI: `ingestion <subcommand>` — `ingest-source <code>`, `build-artifacts`, `seed-samples`, `upload-artifacts`, etc. Used for manual invocation, `launchd` triggers, and local dev. The `_api.rs` filename in each feature triplet matches the Singularity `lobby/` convention and reserves the position for actix-web route configurers if v3+ introduces an HTTP server mode; no actix-web dependency is taken until that mode actually lands.

## Canonical PostgreSQL store

### Conventions

All tables follow the Singularity-inherited conventions:

- Primary key column is named `id`, type `uuid`, populated by application code as UUIDv7 (lexicographic time ordering is useful for `order by id desc` scans).
- Timestamps are named `created` / `modified` (not `_at` suffix); both are `timestamp with time zone not null default now()`.
- Soft-delete is `deleted_at` (`timestamp with time zone`, nullable). The default query path filters `deleted_at is null`; hard deletes are reserved for migrations.
- Foreign keys are stored as plain `uuid` references (e.g. `country_id uuid not null references country(id)`); no ORM relationship layer (constraint stays at the DB level; resolution stays in service code).
- snake_case throughout; SQL keywords lowercase; trailing semicolons on their own line.
- `IF NOT EXISTS` on all DDL where applicable for migration idempotency.

### Tables

#### `region`

UN M49 geographic taxonomy as a self-referential reference table. M49 has three levels (see [unstats.un.org/unsd/methodology/m49/](https://unstats.un.org/unsd/methodology/m49/)):

- **Region** (5 nodes): Africa, Americas, Asia, Europe, Oceania.
- **Subregion** (17 nodes): one or more per region (Northern Africa, Sub-Saharan Africa, Northern America, Latin America and the Caribbean, Central Asia, Eastern Asia, South-eastern Asia, Southern Asia, Western Asia, Eastern Europe, Northern Europe, Southern Europe, Western Europe, Australia and New Zealand, Melanesia, Micronesia, Polynesia).
- **Intermediate region** (7 nodes; M49 uses this level only under two subregions): under **Sub-Saharan Africa** — Eastern Africa, Middle Africa, Southern Africa, Western Africa; under **Latin America and the Caribbean** — Caribbean, Central America, South America.

Each country row points at its *deepest* applicable region (Brazil → South America; France → Western Europe; USA → Northern America; Egypt → Northern Africa, which has no intermediate level). Hierarchical queries ("all countries in the Americas") use a recursive CTE; see below.

```sql
create table if not exists region (
  id               uuid                     not null primary key,
  code             text                     not null unique,
  name_en          text                     not null,
  level            text                     not null,
  parent_region_id uuid                              references region (id),
  m49_code         text                     not null unique,
  created          timestamp with time zone not null default now(),
  modified         timestamp with time zone not null default now()
);
```

`level` is one of `'region'`, `'subregion'`, `'intermediate_region'`. `m49_code` is the UN M49 numeric code as text (preserves leading zeros like `"021"` for Northern America); kept as text rather than `int` so that a future non-M49 taxonomy could coexist in the same column space if Constitution §Boundary recognition's alternate-taxonomy clause is ever exercised. Bootstrapped from M49 in a seed migration; not ingested per-cycle.

Hierarchical descendant query:

```sql
with recursive region_descendants as (
    select id
        from region
        where code = 'americas'
    union all
    select region.id
        from region
        join region_descendants on region.parent_region_id = region_descendants.id
)
select country.*
    from country
    join region_descendants on country.region_id = region_descendants.id
order by country.iso3 asc
;
```

#### `country`

Canonical country reference. Bootstrapped from ISO 3166-1 in a seed migration, with each row joined to its deepest M49 region; not ingested per-cycle.

```sql
create table if not exists country (
  id         uuid                     not null primary key,
  iso3       text                     not null unique,
  iso2       text                     not null,
  name_en    text                     not null,
  region_id  uuid                     not null references region (id),
  created    timestamp with time zone not null default now(),
  modified   timestamp with time zone not null default now(),
  deleted_at timestamp with time zone
);
```

#### `statistic`

Statistic definitions (TFR, CBR, CDR, ASFR, mean age at first birth, etc.).

```sql
create table if not exists statistic (
  id          uuid                     not null primary key,
  code        text                     not null unique,
  name_en     text                     not null,
  description text                     not null,
  units       text                     not null,
  created     timestamp with time zone not null default now(),
  modified    timestamp with time zone not null default now()
);
```

`code` is the short identifier used everywhere downstream: `"tfr"`, `"cbr"`, `"asfr_15_19"`, `"mean_age_first_birth"`. Stable across versions; renaming is a migration event.

#### `source`

Publishers of the data. Per Constitution Principle II, every datum traces back to a row here.

```sql
create table if not exists source (
  id               uuid                     not null primary key,
  code             text                     not null unique,
  name_en          text                     not null,
  homepage_url     text                     not null,
  license_tier     text                     not null,
  license_name     text                     not null,
  license_url      text                     not null,
  attribution_text text                     not null,
  preference_rank  int                      not null,
  created          timestamp with time zone not null default now(),
  modified         timestamp with time zone not null default now()
);
```

- `license_tier` is one of `public_domain`, `attribution`, `attribution_sa`, `noncommercial` (see §License-tier shard mapping below).
- `attribution_text` is the exact display string for UI citations.
- `preference_rank` drives the source-preference merge: lower wins. See §Source-preference merge.

The license fields are denormalized onto `source` rather than a separate `license` table because a source's license is effectively a property of the source. If a source changes its license, that's a schema-and-data event documented in the relevant migration, not a runtime swap.

#### `statistic_value`

The fact table. One row per `(country_id, statistic_id, year, source_id)`. When multiple sources publish the same datum, all rows are kept; the merge happens at artifact-build time.

```sql
create table if not exists statistic_value (
  id                  uuid                     not null primary key,
  country_id          uuid                     not null references country (id),
  statistic_id        uuid                     not null references statistic (id),
  year                int                      not null,
  value               double precision         not null,
  source_id           uuid                     not null references source (id),
  data_status         text                     not null,
  retrieved_at        timestamp with time zone not null,
  source_published_at timestamp with time zone,
  source_revision     text,
  created             timestamp with time zone not null default now(),
  modified            timestamp with time zone not null default now(),
  deleted_at          timestamp with time zone,
  unique (country_id, statistic_id, year, source_id)
);
```

`data_status` is one of:

| Value             | Meaning |
|-------------------|---------|
| `final`           | Source's authoritative value; not expected to revise |
| `provisional`     | Published as preliminary; subject to revision in a future publication cycle |
| `preliminary`     | First-cut estimate from a national source (e.g. CDC NCHS pre-final) |
| `flash_estimate`  | Eurostat-style flash estimate for the most recent year |
| `projection`      | Model output for future years (UN WPP projections, scenario forecasts) |
| `imputed`         | Filled in by Eafora's ingestion via a documented method (rare; flagged) |
| `interpolated`    | Straight-line or model-based estimate between known years (Eafora-generated) |

`retrieved_at` is the wall-clock instant our adapter fetched the row. `source_published_at` is the source's publication timestamp where derivable (often only as a year or version label, hence nullable). `source_revision` is a free-form source-specific identifier (`"2024-Q4"`, `"WPP-2024-rev1"`, etc.) used for upstream-change detection between ingestion runs.

#### `artifact_version`

Records each build of CDN-published artifacts. Used for reproducibility ("what data did the client see at version 2026-w21?") and rollback.

```sql
create table if not exists artifact_version (
  id                    uuid                     not null primary key,
  version_label         text                     not null unique,
  built_at              timestamp with time zone not null default now(),
  manifest_sha256       text                     not null,
  manifest_url          text                     not null,
  source_versions_jsonb jsonb                    not null,
  notes                 text,
  created               timestamp with time zone not null default now()
);
```

`source_versions_jsonb` is a snapshot of every source's `source_revision` at build time: `{"wb_wdi": "2024-Q4", "eurostat_demo_fer": "2026-w20", "hfd": "2025-12"}`. Used by clients to detect when re-fetching is worthwhile (manifest comparison) and by us to attribute artifact contents to upstream snapshots.

### Migrations

Migrations live in `ingestion/db/migrations/` as dbmate timestamped SQL files (`20260524123000_create_country.sql`, etc.). Each file has `-- migrate:up` and `-- migrate:down` sections. The `dbmate.sh` wrapper at the workspace root runs migrations and then re-runs `cargo sqlx prepare --workspace` to refresh the offline cache. Per the Singularity convention, `dbmate.sh` is the only supported way to apply migrations locally.

Seed data (country list from ISO 3166, statistic definitions, source records) lives in `ingestion/db/migrations/` as ordinary INSERT migrations rather than a separate seed mechanism — this keeps the canonical reference data versioned and reproducible across dev/CI/prod.

## Per-source adapters

### Adapter contract

Every source adapter exposes one entrypoint:

```rust
pub async fn fetch_and_normalize(
    pool: &PgPool,
    options: AdapterOptions,
) -> Result<IngestReport, AppError>;
```

The function:

1. Reads the source's last-seen revision from the canonical store (`statistic_value.source_revision` max within this source) to decide what to fetch.
2. Makes HTTP requests via reqwest to the source's API or static endpoint.
3. Parses the response into intermediate types (source-specific, in `<source>_model.rs`).
4. Normalizes to the canonical `statistic_value` shape (joins to `country`/`statistic` by code).
5. Inserts new rows via `sqlx::query_as!`; updates existing rows where the source has revised them (matched on the `(country, statistic, year, source)` natural key).
6. Returns an `IngestReport`.

Adapters are independent of each other. Adding a new source is one new feature module (`<source>_api.rs`, `<source>_db.rs`, `<source>_model.rs`) plus a migration that inserts the `source` record.

### `AdapterOptions` and `IngestReport`

```rust
#[derive(Debug, Clone)]
pub struct AdapterOptions {
    pub force_full_refetch: bool,                  // ignore last-seen revision; refetch everything
    pub country_filter: Option<Vec<String>>,       // restrict to these ISO3 codes (dev/test)
    pub year_range: Option<(i32, i32)>,            // restrict to these years (dev/test)
}

#[derive(Debug)]
pub struct IngestReport {
    pub source_code: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub rows_inserted: u64,
    pub rows_updated: u64,
    pub rows_unchanged: u64,
    pub upstream_revision: String,
    pub warnings: Vec<IngestWarning>,
}

#[derive(Debug)]
pub struct IngestWarning {
    pub country_iso3: Option<String>,
    pub statistic_code: Option<String>,
    pub year: Option<i32>,
    pub message: String,
}
```

`IngestReport` is logged at the end of each run; warnings are surfaced but not fatal (e.g. "source returned 'NA' for a country/year we expected; skipping" is a warning, not an error).

### Error model

`AppError` is `minimer::Error` parameterized for the ingestion binary. Per-feature modules define their own concrete variants where matching is useful:

```rust
// ingestion/src/world_bank_wdi/world_bank_wdi_model.rs
#[derive(Debug)]
pub enum WorldBankWdiError {
    HttpFailed { url: String, status: u16 },
    UnexpectedSchema { path: String, message: String },
    UnknownIso3 { iso3: String, year: i32 },
}
```

These convert to `AppError` at the public boundary (`fetch_and_normalize`'s return). The function never panics on upstream-data quirks; everything is either a recoverable warning (continues), a per-row drop (warning + skip), or an `AppError` that aborts the run.

### Adding a new source

The mechanical steps for any new source:

1. Add a migration inserting a row in `source` with the source's code, license, attribution string, and `preference_rank` (see §Source-preference merge for ranking).
2. Create `ingestion/src/<source_code>/` with the three-file lobby triplet.
3. Implement `fetch_and_normalize` in `<source_code>_api.rs`.
4. Implement source-specific SQL in `<source_code>_db.rs`.
5. Define source-specific types and parsing in `<source_code>_model.rs`.
6. Register the adapter in `main.rs`'s `run-all` subcommand handler and `ingest-source` dispatch.
7. Write tests against checked-in sample responses in `ingestion/samples/<source_code>/`.

### First source: World Bank WDI

The first concrete adapter (and the first `/speckit-specify` feature) is World Bank WDI for TFR:

- Endpoint: `https://api.worldbank.org/v2/country/all/indicator/SP.DYN.TFRT.IN?format=json&per_page=20000`
- Response shape: JSON array `[paging_metadata, [rows...]]` where each row has `country.id`, `date` (year), `value`, etc.
- Coverage: ~200 countries, years ~1960–latest-published.
- License: CC BY 4.0 → `license_tier = 'attribution'`.
- `source.code = "wb_wdi"`; `preference_rank = 90` (lowest priority among fertility-data sources because WB aggregates from elsewhere).

The full implementation lives in the feature spec; this section documents only what's relevant to the ingestion architecture (the adapter is a normal instance of the contract above).

## Source-preference merge

When multiple sources publish a value for the same `(country, statistic, year)`, all rows stay in `statistic_value`. The merge into a single "what does the user see?" value happens at **artifact-build time**, not at ingestion time. This keeps every source's contribution intact in the canonical store for reproducibility, license accounting, and rollback.

### Merge rule

For each `(country, statistic, year)` cell published into the artifact:

1. Select all candidate rows from `statistic_value` where `deleted_at is null`.
2. Filter by license tier eligibility (a Tier 0/1 base shard only considers rows from Tier 0/1 sources).
3. Among eligible candidates, pick the row with the lowest `source.preference_rank`. Ties (which should not happen — ranks are unique within a statistic) break to the most recent `retrieved_at`.
4. If the picked source's `data_status` is `provisional`/`preliminary`/`flash_estimate` AND a lower-priority source has a `final` value for the same year that is fresher than 2 years old, prefer the `final` value. (Don't show stale "final" data when fresher "preliminary" data exists, and don't show preliminary data when a high-quality final value is available.)

### Current preference ranking (v1+)

| Rank | Source                                      | Coverage                          |
|-----:|---------------------------------------------|-----------------------------------|
|   10 | Human Fertility Database (HFD)              | ~25 countries, peer-reviewed     |
|   20 | National statistical offices (per-country)  | Per country (CDC NCHS, ONS, etc.) |
|   30 | Eurostat                                    | EU members                        |
|   60 | UN World Population Prospects (WPP)         | Global, estimates + projections  |
|   90 | World Bank WDI                              | Global, aggregates from above     |

The numeric gaps leave room for inserting new sources without renumbering. The ranking is a working policy — revisable per-statistic if a source proves authoritative for one indicator and not another (a `statistic_source_override` table is a v3+ option; v1 uses a single global ranking).

## Geometry ingestion

Geometry is ingested separately from statistic ingestion, with a different cadence (rarely changes) and a different source (boundary datasets, not statistical agencies).

- **Source**: Natural Earth 1:50m Cultural Vectors (`ne_50m_admin_0_countries.zip` for v1; subnational comes from `ne_10m_admin_1_states_provinces.zip` in v2+).
- **License**: Public domain (`license_tier = 'public_domain'` in the `source` record).
- **Pipeline**: `geometry_ingest::fetch_natural_earth(version) -> Result<(), AppError>` downloads the shapefile, projects to WGS84 (already is), joins to `country.iso3` via Natural Earth's `ADM0_A3` field, and writes geometry into a separate `country_geometry` table (or stores directly in a checked-in FlatGeobuf if the dataset is stable enough to bundle — open question, see below).

Two-tier shard model (per License-segmented shards in the overview) does not apply to geometry: Natural Earth is public domain, so geometry ships in the base FlatGeobuf without segmentation.

## Artifact builder

### Entrypoint

```rust
pub async fn build_artifacts(
    pool: &PgPool,
    output_dir: &Path,
    version_label: &str,
) -> Result<ArtifactVersion, AppError>;
```

Steps:

1. Read `statistic_value` from the canonical store grouped by `(statistic_code, license_tier)`.
2. Apply the source-preference merge per cell (see §Source-preference merge).
3. For each `(statistic_code, license_tier)` group, emit a SQLite shard via `sqlite_writer`.
4. Emit the geometry FlatGeobuf via `flatgeobuf_writer` (single file, no tier split).
5. Compute content hashes (SHA-256) for every output file.
6. Emit `manifest.json` via `manifest_writer`.
7. Insert an `artifact_version` row recording the build.
8. Return the `ArtifactVersion` for the caller to upload via the R2 client (separate `upload-artifacts` CLI step or chained in `build-artifacts --upload`).

### Output directory layout

```
output_dir/
├── manifest.json
├── geometry/
│   └── world-50m-<sha8>.fgb
└── data/
    ├── tfr-base-<sha8>.sqlite
    ├── tfr-noncommercial-<sha8>.sqlite      # only emitted when noncommercial-tier rows exist for tfr
    ├── cbr-base-<sha8>.sqlite
    └── ...
```

Filenames are `<statistic_code>-<license_tier>-<sha8>.sqlite` (with `base` covering Tier 0 + Tier 1). Content-hash suffix uses the first 8 hex chars of SHA-256 for filename brevity; the full hash is in the manifest.

### Manifest format

```json
{
  "version": "2026-w21",
  "built_at": "2026-05-21T14:00:00Z",
  "geometry": {
    "url": "geometry/world-50m-ab12cd34.fgb",
    "size_bytes": 4380000,
    "sha256": "ab12cd34..."
  },
  "statistics": {
    "tfr": {
      "base":          { "url": "data/tfr-base-ef56...sqlite",         "size_bytes": 89000, "sha256": "ef56..." },
      "noncommercial": { "url": "data/tfr-noncommercial-78ab...sqlite", "size_bytes":  4200, "sha256": "78ab..." }
    },
    "cbr": { "base": { "url": "...", "size_bytes": ..., "sha256": "..." } }
  },
  "source_versions": {
    "wb_wdi": "2024-Q4",
    "eurostat_demo_fer": "2026-w20",
    "hfd": "2025-12"
  }
}
```

The client loads the manifest first, then fetches whatever shards its license tier permits. The base shard is always present; non-base shards may be missing for a given statistic if no rows in that tier exist.

### License-tier shard mapping

| `source.license_tier` value | Shard          |
|-----------------------------|----------------|
| `public_domain`             | `base`         |
| `attribution`               | `base`         |
| `attribution_sa`            | `share_alike`  |
| `noncommercial`             | `noncommercial`|

v1 emits only `base` shards (the only seeded source is WB WDI). The other shards activate when a source with the corresponding license tier lands. Clients identify their distribution context and `ATTACH DATABASE` each authorized shard; query results union across attached databases.

### Content hashing and immutability

Every output file is content-hashed. The hash is computed from the file's bytes after all writes complete; the file is renamed from `*.tmp-<uuid>` to `<name>-<sha8>.<ext>` only after hashing succeeds. The manifest's hashes are computed last and reference the renamed paths.

CDN cache headers (set at upload time, not in the artifact itself):

- `manifest.json`: `Cache-Control: public, max-age=300` (short-cached)
- All other artifact files: `Cache-Control: public, max-age=31536000, immutable`

### R2 upload

Uploads happen via a separate CLI step (`ingestion upload-artifacts <version_label>`) so the build can be inspected locally before publishing. The R2 client uses reqwest against R2's S3-compatible API with credentials from `secr`-encrypted secrets. Upload is idempotent — content-hashed filenames mean re-uploading the same file is a no-op semantically.

## Scheduling

### v1: Mac mini M1 + `launchd`

A `launchd` plist (template at `scripts/eafora-ingestion.plist.tmpl`, installed by `setup.sh` to `~/Library/LaunchAgents/org.eafora.ingestion.plist` on the Mac mini) triggers `ingestion run-all` on a schedule:

```xml
<key>StartCalendarInterval</key>
<dict>
  <key>Weekday</key>  <integer>1</integer>   <!-- Monday -->
  <key>Hour</key>     <integer>3</integer>
  <key>Minute</key>   <integer>0</integer>
</dict>
```

`ingestion run-all` invokes every registered adapter sequentially (parallelism is unnecessary at v1's source count; cross-adapter dependencies are nil). It then calls `build-artifacts` if any adapter reported rows changed, and `upload-artifacts` if the build succeeded.

Manual invocation is always supported: `ingestion ingest-source wb_wdi --force-full-refetch` re-runs a single adapter ignoring incremental state. Per Constitution §Tooling discipline, both the scheduled path and the manual path go through the same CLI subcommands; `launchd` calls the same binary the developer calls.

### v2+: managed compute

When the Mac mini becomes insufficient (HA, geographic distribution, or v3+ live API needs), migrate to a managed-cloud Postgres + a managed scheduled-job runner. The CLI shape doesn't change — only the launchd plist gets replaced with cron, systemd, AWS EventBridge, or whatever the post-migration platform offers. Deferred until forced.

## Local development

### Postgres on the host

`setup.sh` (at the repo root) installs Postgres via Homebrew (`brew install postgresql@17`) and configures it as a launchd-managed service. The plist template at `scripts/eafora-postgres.plist.tmpl` is rendered into `~/Library/LaunchAgents/org.eafora.postgres.plist`, then loaded via `launchctl bootstrap gui/$(id -u)`, so Postgres starts at login and on demand. The default database is `eafora_dev` on port 5432; `DATABASE_URL` is `postgresql://localhost/eafora_dev` (set via `dotenvy` from `.env`).

This is a deviation from Singularity's Podman Compose setup (Constitution Principle IV; recorded in v1.3.3). Accepted because v1 ships on a personal Mac mini plus one developer machine — host-installed Postgres removes the Podman dependency and the compose-file plumbing at the cost of portability that doesn't matter at this scale. Containerization may return when cloud deployment lands post-v2.

### Seeding the canonical store

A `seed-samples` CLI subcommand populates the canonical store with checked-in sample data:

```sh
cargo run -p ingestion -- seed-samples
```

This runs all migrations (including the seed-data migrations for `country` and the initial `statistic`/`source` records), then loads sample responses from `ingestion/samples/<source_code>/` and replays them through each adapter's normalize-and-insert path. The result is a fully-populated canonical store with the same shape production would have, but with fixed test data.

### Running an adapter locally

```sh
cargo run -p ingestion -- ingest-source wb_wdi --country-filter USA,DEU,JPN --year-range 2000-2024
```

The filters keep the run small enough to iterate quickly. Without filters, a full WB WDI run is ~200 countries × ~65 years × ~1 statistic ≈ 13k rows, which is still under a second.

### Producing artifacts locally

```sh
cargo run -p ingestion -- build-artifacts ./build-output 2026-w21
```

Writes `manifest.json` + `geometry/` + `data/` under `./build-output/`. No upload. The artifacts can be served via `python -m http.server` in a pinch (with `Content-Encoding: br` headers ad-hoc'd in front of `nginx` or `caddy` if compression-aware testing is wanted), or pointed at directly from the web client via a local file URL.

## Testing strategy

Per Constitution Principle VII, the ingestion-side TDD-required surfaces are:

- **Per-source normalization** (`<source>_model.rs` parsing functions): every sample response → expected canonical-shape output, exhaustively.
- **Source-preference merge** (`artifact/merge.rs` — the per-cell merge logic): all combinations of `(data_status, retrieved_at, preference_rank)` exercised against the merge rule.
- **Artifact diffing** (used by `build-artifacts` to decide whether a build is no-op): trivial cases (no canonical changes) and tricky cases (rows updated but resulting artifact bytes unchanged) covered.
- **Error mapping** (per-source error → `AppError` → log line): each variant gets a test.

Integration tests use the seeded canonical store via `seed-samples`, exercise `fetch_and_normalize` against the sample responses (no live HTTP), and assert on the resulting `statistic_value` rows and on the artifact output.

Non-TDD surfaces (still tested, but the test-first discipline is relaxed):

- HTTP wiring (reqwest configuration, timeout/retry policy)
- launchd plist generation
- CLI argument parsing

## Open questions

1. **Geometry storage shape: in-DB or pre-built FlatGeobuf bundled in the repo?** Natural Earth's 1:50m countries dataset is ~3 MB raw, ~5 MB FlatGeobuf, stable across years. Bundling the FlatGeobuf directly in the repo (as `ingestion/data/world-50m.fgb`) and copying it to artifact output during build avoids the Postgres-as-staging step entirely for geometry. Storing in Postgres (PostGIS `geometry` column on a `country_geometry` table) makes the canonical store the single source of truth at the cost of operational complexity (PostGIS extension, larger backup, harder local-dev setup). Lean: **bundle the FlatGeobuf in-repo for v1**; switch to PostGIS if subnational geometry ever needs to be merged with custom overlays. Defer until v2.
2. **Per-statistic preference overrides.** The current `source.preference_rank` is global. Some sources may be authoritative for one statistic and not another (e.g. CDC NCHS for US TFR but not US migration). A `statistic_source_preference` association table would handle this. Defer until v2; for v1 the global ranking is sufficient.
3. **Revision detection granularity.** `source_revision` is stored per-row, but most sources publish a single version label that applies to the whole download. Should there be a `source_publication (source_id, revision_label, fetched_at)` table tracking publication-level metadata distinct from per-row metadata? Defer; first ingestion pass will reveal whether per-row revision tracking is overkill.
4. **Artifact build cadence vs. ingestion cadence.** Currently `run-all` calls `build-artifacts` if any adapter reported changes. Should artifact builds be debounced (e.g. only build at most once per day even if multiple ingestion runs report changes)? Probably yes once the cost of an artifact build becomes non-trivial; not a v1 concern.

## Things to verify

1. **dbmate's behavior with the Singularity convention for `cargo sqlx prepare`**: confirm that `dbmate.sh`'s wrapper around `sqlx prepare --workspace` works against a workspace with both `ingestion/` and `core/` crates.
2. **Cloudflare R2 S3-compatible API surface**: confirm reqwest + `aws-sigv4` (or equivalent signature crate) work without the AWS SDK. R2's S3 compatibility is documented but corner cases (multipart upload thresholds, presigned URLs) deserve a spike before relying on them.
3. **Natural Earth license verification**: the dataset is widely described as public domain ("free of charge for any use"), but the actual Natural Earth website's license page should be confirmed and quoted in the seeded `source` row.
4. **WB WDI rate limits**: the WB API has historically been generous (no documented rate limit) but a spike against the actual endpoint should confirm before the first scheduled run.

## Follow-up work

- First `/speckit-specify` feature spec: `specs/NNN-world-bank-wdi-ingestion/` — implements the WB WDI adapter against this plan's contract.
- Per-platform client plans (`docs-architecture-client-{web,ios,android}`) — these depend on the manifest + shard format locked here.
- A `docs-architecture-secrets` mini-plan documenting which secrets the ingestion binary needs (R2 credentials initially; nothing else through v2) and how `secr` integrates with the launchd entrypoint.
- A `scripts/build-fallback.sh` script (referenced from the overview's §Workspace and crate layout) that consumes a real artifact build and downsamples it into the per-platform bundled-fallback resources. Defer until at least one client shell exists.
