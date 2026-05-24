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
- UUIDv7 primary keys named `id`; timestamps named `created` / `modified` / `deleted` (no `_at` suffix), all `timestamp with time zone`. (Memory: db schema conventions)
- minimer for errors; secr for secrets; statics via `LazyLock`. (Constitution IV)

## Workspace placement

The ingestion binary lives at the workspace root in its own crate:

```
eafora/
├── core/                       # data models, math, projection, FFI surfaces (no sqlx)
├── ingestion/                  # this document's subject
│   ├── Cargo.toml              # depends on core, sqlx, reqwest, tokio, clap, ...
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

### CLI structure (clap builder API)

CLI arg parsing and dispatch use **clap**'s **builder** API (not the derive macros). Matches Constitution Principle V's explicit-over-implicit preference — the command tree is constructed imperatively, with each subcommand's arguments visible at the call site rather than derived from struct attributes.

```rust
// ingestion/src/main.rs
use clap::{Arg, ArgAction, ArgMatches, Command};

fn build_cli() -> Command {
    Command::new("ingestion")
        .subcommand_required(true)
        .subcommand(
            Command::new("ingest-source")
                .about("Run a single source adapter")
                .arg(Arg::new("source").required(true).help("source code (e.g. wb_wdi)"))
                .arg(Arg::new("force-full-refetch").long("force-full-refetch").action(ArgAction::SetTrue)),
        )
        .subcommand(Command::new("run-all").about("Run every registered source adapter"))
        .subcommand(
            Command::new("build-artifacts")
                .about("Build CDN artifacts from the current canonical store")
                .arg(Arg::new("output-dir").required(true))
                .arg(Arg::new("version-label").required(true)),
        )
        .subcommand(Command::new("seed-samples").about("Load checked-in sample responses into the canonical store"))
        .subcommand(
            Command::new("upload-artifacts")
                .about("Upload a previously-built artifact set to R2")
                .arg(Arg::new("version-label").required(true)),
        )
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let matches: ArgMatches = build_cli().get_matches();
    match matches.subcommand() {
        Some(("ingest-source",    sub_matches)) => dispatch_ingest_source(sub_matches).await,
        Some(("run-all",          _))           => dispatch_run_all().await,
        Some(("build-artifacts",  sub_matches)) => dispatch_build_artifacts(sub_matches).await,
        Some(("seed-samples",     _))           => dispatch_seed_samples().await,
        Some(("upload-artifacts", sub_matches)) => dispatch_upload_artifacts(sub_matches).await,
        _                                        => unreachable!("subcommand_required guarantees a match"),
    }
}
```

Each subcommand has a `dispatch_*` helper that reads its specific arguments from `ArgMatches` and calls into the relevant feature module (`world_bank_wdi::fetch_and_store(...)`, `artifact::build_artifacts(...)`, etc.). The `dispatch_*` helpers live alongside `main` in `main.rs` for the run-all orchestration case, or — if a dispatch grows non-trivial — in the relevant feature module's `_api.rs`.

Do not introduce `#[derive(Parser)]`, `#[derive(Subcommand)]`, or any clap derive macro. If a clap helper accepts both forms, pick the builder variant.

## Canonical PostgreSQL store

### Conventions

All tables follow the Singularity-inherited conventions:

- Primary key column is named `id`, type `uuid`, populated by application code as UUIDv7 (lexicographic time ordering is useful for `order by id desc` scans).
- Timestamps are named `created` / `modified` (not `_at` suffix); both are `timestamp with time zone not null default now()`.
- Soft-delete is `deleted` (`timestamp with time zone`, nullable; no `_at` suffix, matching the `created` / `modified` convention). The default query path filters `deleted is null`; hard deletes are reserved for migrations.
- Foreign keys are stored as plain `uuid` references (e.g. `region_id uuid not null references region (id)`); no ORM relationship layer (constraint stays at the DB level; resolution stays in service code).
- snake_case throughout; SQL keywords lowercase; trailing semicolons on their own line.
- `IF NOT EXISTS` on all DDL where applicable for migration idempotency.

### Tables

#### `region`

The unified administrative-geographic hierarchy. Every geographic entity — supranational groupings, countries, and (in v2+) subnational divisions — is a row in this table, linked via `parent_region_id`. v1 ships UN M49 groupings plus countries; subnational levels are added when subnational data lands.

UN M49 (see [unstats.un.org/unsd/methodology/m49/](https://unstats.un.org/unsd/methodology/m49/)) contributes four of the current levels:

- **`region`** (5 nodes): Africa, Americas, Asia, Europe, Oceania.
- **`subregion`** (17 nodes): one or more per region (Northern Africa, Sub-Saharan Africa, Northern America, Latin America and the Caribbean, Central Asia, Eastern Asia, South-eastern Asia, Southern Asia, Western Asia, Eastern Europe, Northern Europe, Southern Europe, Western Europe, Australia and New Zealand, Melanesia, Micronesia, Polynesia).
- **`intermediate_region`** (7 nodes; M49 uses this level only under two subregions): under **Sub-Saharan Africa** — Eastern Africa, Middle Africa, Southern Africa, Western Africa; under **Latin America and the Caribbean** — Caribbean, Central America, South America.
- **`country`** (~200 nodes): ISO 3166-1 entities; M49 also numbers these (USA='840', DEU='276'). Each country row's `parent_region_id` points at its *deepest* applicable M49 region (Brazil → South America; France → Western Europe; USA → Northern America; Egypt → Northern Africa, which has no intermediate level).

Future levels (`subnational_1`, `subnational_2`, etc.) attach via the same `parent_region_id` mechanism when subnational data is introduced.

Country-specific metadata (ISO 3166 codes) lives in a 1:1 extension table `country` keyed on `region.id`; see below.

```sql
create table if not exists region (
  id               uuid                     not null primary key,
  code             text                     not null unique,
  name_en          text                     not null,
  level            text                     not null,
  parent_region_id uuid                              references region (id),
  m49_code         text                              unique,  -- text vs int leaves room for a non-M49 taxonomy if §Boundary recognition's alt-taxonomy clause is exercised; nullable to accommodate future subnational levels that have no M49 equivalent
  created          timestamp with time zone not null default now(),
  modified         timestamp with time zone not null default now()
);

comment on column region.code             is $$human-readable slug ('americas', 'south_america', 'sub_saharan_africa', 'usa', 'germany')$$;
comment on column region.level            is $$'region' | 'subregion' | 'intermediate_region' | 'country' | (future subnational levels: 'subnational_1', 'subnational_2', ...)$$;
comment on column region.parent_region_id is $$null only for top-level region nodes (Africa, Americas, Asia, Europe, Oceania); every other row including countries has a parent$$;
comment on column region.m49_code         is $$UN M49 numeric code as text (preserves leading zeros like '021'); also populated for country-level rows (USA='840', DEU='276'); nullable for future non-M49 levels (subnational) that have no M49 equivalent$$;
```

Bootstrapped from M49 (groupings + countries) in seed migrations; not ingested per-cycle. Hierarchical queries ("all countries in the Americas") use a recursive CTE:

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
select region.id, region.code, region.name_en, country.iso3
    from region
    join region_descendants on region.id = region_descendants.id
    join country            on country.region_id = region.id
order by country.iso3 asc
;
```

The inner join to `country` acts as the filter — only country-level region rows have a corresponding country extension, so the result is "all countries in the Americas." To get all regions at every level under Americas (countries, intermediate regions, subregions), drop the country join.

#### `country` (1:1 extension of `region` at `level = 'country'`)

Country-specific metadata. Strictly 1:1 with `region` rows where `level = 'country'`: the PK and the FK are the same column (`region_id`). `name_en` lives on `region` (covers all levels); only the ISO codes are country-specific. Bootstrapped from ISO 3166-1 in the same seed migration that inserts the country-level region rows; not ingested per-cycle.

```sql
create table if not exists country (
  region_id uuid                     not null primary key references region (id),
  iso3      text                     not null unique,
  iso2      text                     not null unique,
  created   timestamp with time zone not null default now(),
  modified  timestamp with time zone not null default now(),
  deleted   timestamp with time zone
);

comment on column country.region_id is $$both PK and FK to region.id; enforces the strict 1:1 extension shape (every country row corresponds to exactly one region row at level='country', and vice versa)$$;
comment on column country.iso3      is $$ISO 3166-1 alpha-3 ('USA', 'DEU', 'JPN')$$;
comment on column country.iso2      is $$ISO 3166-1 alpha-2 ('US', 'DE', 'JP')$$;
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

comment on column statistic.code is $$short identifier used downstream ('tfr', 'cbr', 'asfr_15_19'); stable across versions, renaming is a migration event$$;
```

#### `data_source`

Publishers of the data. Per Constitution Principle II, every datum traces back to a row here.

```sql
create table if not exists data_source (
  id               uuid                     not null primary key,
  code             text                     not null unique,
  name_en          text                     not null,
  homepage_url     text                     not null,
  license_class    text                     not null,
  license_name     text                     not null,
  license_url      text                     not null,
  attribution_text text                     not null,
  preference_rank  int                      not null,
  created          timestamp with time zone not null default now(),
  modified         timestamp with time zone not null default now()
);

comment on column data_source.code             is $$short identifier ('wb_wdi', 'eurostat_demo_fer', 'hfd')$$;
comment on column data_source.license_class    is $$one of: public_domain | attribution | attribution_sa | noncommercial$$;
comment on column data_source.license_name     is $$e.g. 'CC BY 4.0', 'Open Government Licence v3.0'$$;
comment on column data_source.attribution_text is $$exact display string for UI citations$$;
comment on column data_source.preference_rank  is $$drives data-source-preference merge; lower wins; ties broken deterministically by data_source.id$$;
```

The license fields are denormalized onto `data_source` rather than a separate `license` table because a source's license is effectively a property of the source. If a source changes its license, that's a schema-and-data event documented in the relevant migration, not a runtime swap.

#### `data_source_publication`

One row per publication event we've captured from a source. Sources publish in batches (WB WDI's quarterly drops, Eurostat's weekly releases, HFD's annual updates); each batch is a "publication." This table normalizes the publication metadata out of `statistic_value` — a single fetch that produces 12,000 `statistic_value` rows produces ONE row here, not 12,000 redundant copies of the revision label.

```sql
create table if not exists data_source_publication (
  id             uuid                     not null primary key,
  data_source_id uuid                     not null references data_source (id),
  revision_label text                     not null,
  published      timestamp with time zone,
  fetched        timestamp with time zone not null,
  created        timestamp with time zone not null default now(),
  modified       timestamp with time zone not null default now(),
  unique (data_source_id, revision_label)
);

comment on column data_source_publication.revision_label is $$the source's own revision label for this publication event (WB WDI '2024-Q4', Eurostat '2026-w20', HFD '2025-12', WPP 'WPP-2024-rev1'); sources without native versioning get a synthesized label (response payload hash or fetch date); read by the adapter's read_last_seen_revision step for incremental fetches; aggregated per-source into the manifest's data_source_versions_jsonb at artifact-build time$$;
comment on column data_source_publication.published      is $$source's own publication timestamp where derivable (often only a year or version label, hence nullable)$$;
comment on column data_source_publication.fetched        is $$wall-clock instant our adapter captured this publication$$;
```

Publications are append-only — once captured, the row stays as an audit trail. If a source republishes under the same `revision_label` (a re-fetch with no upstream change), the existing row is matched on `(data_source_id, revision_label)` and no insert happens. If the source publishes a new revision, a new row is inserted, and `statistic_value` rows are updated to point at it (the old publication row stays in this table).

#### `statistic_value`

The fact table. One row per `(region_id, statistic_id, period_start, period_end, data_source_id)`. `region_id` can point at any level — country (the common case for v1), subnational region (when subnational data lands in v2+), or supranational grouping (for stored aggregates like an EU-wide TFR). When multiple sources publish the same datum, all rows are kept; the merge happens at artifact-build time. Periods are half-open intervals (inclusive start, exclusive end); see the column comments for examples. Each row points at the `data_source_publication` it was captured from; when the source publishes a revision, the row is updated in place to point at the new publication.

```sql
create table if not exists statistic_value (
  id                         uuid                     not null primary key,
  region_id                  uuid                     not null references region (id),
  statistic_id               uuid                     not null references statistic (id),
  period_start               date                     not null,
  period_end                 date                     not null,
  value                      double precision         not null,
  data_source_id             uuid                     not null references data_source (id),
  data_source_publication_id uuid                     not null references data_source_publication (id),
  data_status                text                     not null,
  created                    timestamp with time zone not null default now(),
  modified                   timestamp with time zone not null default now(),
  unique (region_id, statistic_id, period_start, period_end, data_source_id)
);

comment on column statistic_value.region_id                  is $$points at any level — country (common in v1), subnational (v2+ when subnational data lands), or supranational grouping (for stored aggregates)$$;
comment on column statistic_value.period_start               is $$inclusive lower bound: calendar year 2024 → '2024-01-01'; Q1 2024 → '2024-01-01'; 2020-2025 cohort → '2020-01-01'$$;
comment on column statistic_value.period_end                 is $$exclusive upper bound: calendar year 2024 → '2025-01-01'; Q1 2024 → '2024-04-01'; 2020-2025 cohort → '2025-01-01'$$;
comment on column statistic_value.data_source_id             is $$denormalized from data_source_publication.data_source_id for the natural-key unique constraint and fast filtering by source; the upsert path keeps the two in sync$$;
comment on column statistic_value.data_source_publication_id is $$points at the publication event this row's value was captured from; updated in place when the source publishes a revision (the previous publication row stays in data_source_publication as audit trail)$$;
comment on column statistic_value.data_status                is $$one of: final | provisional | preliminary | projection | imputed | interpolated$$;
```

`data_status` values:

| Value             | Meaning |
|-------------------|---------|
| `final`           | Source's authoritative value; not expected to revise |
| `provisional`     | Published as preliminary; subject to revision in a future publication cycle |
| `preliminary`     | Early estimate published before the final value — covers both rapid Eurostat-style flash releases (T+1 month) and national statistical offices' first publications (T+3 months) |
| `projection`      | Model output for future years (UN WPP projections, scenario forecasts) |
| `imputed`         | Filled in by Eafora's ingestion via a documented method (rare; flagged) |
| `interpolated`    | Straight-line or model-based estimate between known years (Eafora-generated) |

#### `artifact_version`

Records each artifact that has been **published to the CDN** (one row per successful upload — NOT per local build). Used for reproducibility ("what data did the client see at version 2026-05-18?"), rollback, and answering "what's the latest available version?" from clients. Local builds that haven't been uploaded leave no row here. If we ever need to track every build attempt (failed uploads, dry-run builds, etc.), that's a separate `build_attempt` table to add — not v1.

```sql
create table if not exists artifact_version (
  id                         uuid                     not null primary key,
  version_label              text                     not null unique,
  artifact_created           timestamp with time zone not null default now(),
  manifest_sha256            text                     not null,
  manifest_url               text                     not null,
  data_source_versions_jsonb jsonb                    not null,
  notes                      text
);

comment on column artifact_version.version_label              is $$ISO date of the scheduled build (e.g. '2026-05-18'); disambiguating suffix added if two builds land the same day$$;
comment on column artifact_version.manifest_sha256            is $$content hash of manifest.json$$;
comment on column artifact_version.manifest_url               is $$CDN URL of manifest.json$$;
comment on column artifact_version.data_source_versions_jsonb is $$snapshot of every data_source's data_source_revision at build time: {"wb_wdi": "2024-Q4", "hfd": "2025-12"}; used to attribute artifact contents to upstream snapshots and to let clients detect when re-fetching is worthwhile$$;
```

### Migrations

Migrations live in `ingestion/db/migrations/` as dbmate timestamped SQL files (`20260524123000_create_country.sql`, etc.). Each file has `-- migrate:up` and `-- migrate:down` sections. The `dbmate.sh` wrapper at the workspace root runs migrations and then re-runs `cargo sqlx prepare --workspace` to refresh the offline cache. Per the Singularity convention, `dbmate.sh` is the only supported way to apply migrations locally.

Seed data (country list from ISO 3166, statistic definitions, source records) lives in `ingestion/db/migrations/` as ordinary INSERT migrations rather than a separate seed mechanism — this keeps the canonical reference data versioned and reproducible across dev/CI/prod.

## Per-source adapters

### Adapter contract

Every source adapter exposes one entrypoint that orchestrates a fixed pipeline of named helper functions:

```rust
pub async fn fetch_and_store(
    pool: &PgPool,
    options: AdapterOptions,
) -> Result<IngestReport, AppError> {
    let last_seen_revision: Option<String> = read_last_seen_revision(pool).await?;
    let raw_response: RawResponse = fetch_upstream(&options, last_seen_revision.as_deref()).await?;
    let parsed_rows: Vec<ParsedRow> = parse_response(raw_response)?;
    let normalized_rows: Vec<NormalizedRow> = normalize(pool, parsed_rows).await?;
    let report: IngestReport = upsert_rows(pool, normalized_rows).await?;
    Ok(report)
}
```

Each helper is a separate named function inside the source's feature module; the orchestrator only sequences them. The helper contracts:

- **`read_last_seen_revision(pool)`** → `Option<String>`. Queries `select revision_label from data_source_publication where data_source_id = $1 order by fetched desc limit 1` to find the most recent publication this adapter has captured. `None` means "first run, fetch everything".
- **`fetch_upstream(options, since)`** → source-specific `RawResponse`. Makes the HTTP request(s) via reqwest. Honors `options.force_full_refetch`; uses `since` to request only-changed data when the source's API supports it.
- **`parse_response(raw)`** → `Vec<ParsedRow>`. Deserializes the source-specific response into intermediate types defined in `<source>_model.rs`. Pure function — no I/O, no DB access.
- **`normalize(pool, parsed_rows)`** → `Vec<NormalizedRow>`. Joins to `region` (via country.iso3 for country-level data) / `statistic` by code, computes `period_start` / `period_end` from the source's time-period encoding, attaches `data_status` and `data_source_revision`. Reads from the DB to resolve foreign-key IDs but does not write.
- **`upsert_rows(pool, normalized_rows)`** → `IngestReport`. First INSERTs the run's `(data_source_id, revision_label, fetched)` into `data_source_publication` (ON CONFLICT DO NOTHING; obtains the publication's id whether newly inserted or matched against an existing row). Then bulk-UPSERTs into `statistic_value` matched on the natural key `(region_id, statistic_id, period_start, period_end, data_source_id)`, setting `data_source_publication_id` to the publication's id. Returns counts of inserted / updated / unchanged `statistic_value` rows plus any non-fatal warnings.

Adapters are independent of each other. Adding a new source is one new feature module (`<source>_api.rs`, `<source>_db.rs`, `<source>_model.rs`) plus a migration that inserts the `data_source` record.

### `AdapterOptions` and `IngestReport`

```rust
#[derive(Debug, Clone)]
pub struct AdapterOptions {
    pub force_full_refetch: bool,                       // ignore last-seen revision; refetch everything
}

#[derive(Debug)]
pub struct IngestReport {
    pub source_code: String,
    pub started: DateTime<Utc>,
    pub finished: DateTime<Utc>,
    pub values_inserted: u64,
    pub values_updated: u64,
    pub values_unchanged: u64,
    pub upstream_revision: String,
    pub warnings: Vec<IngestWarning>,
}

#[derive(Debug)]
pub struct IngestWarning {
    pub region_code: Option<String>,
    pub statistic_code: Option<String>,
    pub period_start: Option<NaiveDate>,
    pub message: String,
}
```

`IngestReport` is logged at the end of each run; warnings are surfaced but not fatal (e.g. "source returned 'NA' for a country/year we expected; skipping" is a warning, not an error).

### Error model

`AppError` is `minimer::Error` for the ingestion binary. Adapter code builds errors as formatted strings at the failure site rather than defining per-source concrete enum variants — matches Singularity's pattern, avoids variant boilerplate, and is sufficient because nothing in the ingestion pipeline pattern-matches on adapter errors (they only flow to logs and surface in the run-level outcome).

```rust
// ingestion/src/world_bank_wdi/world_bank_wdi_api.rs
let url: &str = "https://api.worldbank.org/v2/country/all/indicator/SP.DYN.TFRT.IN?format=json&per_page=20000";
let response: reqwest::Response = reqwest::get(url).await
    .map_err(|err| AppError::from(format!("wb_wdi: HTTP GET {url} failed: {err}")))?;
let status: reqwest::StatusCode = response.status();
if !status.is_success() {
    return Err(AppError::from(format!("wb_wdi: HTTP {status} from {url}")));
}
```

The function never panics on upstream-data quirks; everything is either a recoverable warning (continues; see `IngestWarning`), a per-row drop (warning + skip), or an `AppError` that aborts the run.

Concrete enum variants are only justified when callers actually need to pattern-match on the error class — e.g. a retry layer that branches on transient-vs-permanent, or an FFI boundary that needs typed errors for Swift / Kotlin. Neither applies to ingestion adapters (which run server-side and either succeed or get logged-and-retried-on-next-schedule). If a concrete variant ever IS justified, promote it onto `AppError` itself rather than introducing a per-source enum hierarchy.

When v3+ adds actix-web HTTP handlers to this binary (currently dormant), the error model gains an HTTP-response mapping layer: copy Singularity's `LobbyError` pattern (implements actix-web's `ResponseError` trait to convert `AppError` variants into HTTP status + JSON body). v1's CLI-only ingestion doesn't need this; flag for whichever spec introduces the HTTP server mode.

### Adding a new source

The mechanical steps for any new source:

1. Add a migration inserting a row in `data_source` with the source's code, license, attribution string, and `preference_rank` (see §Source-preference merge for ranking).
2. Create `ingestion/src/<source_code>/` with the three-file lobby triplet.
3. Implement `fetch_and_store` (the orchestrator) and its five named helpers (`read_last_seen_revision`, `fetch_upstream`, `parse_response`, `normalize`, `upsert_rows`) in `<source_code>_api.rs`.
4. Implement source-specific SQL in `<source_code>_db.rs`.
5. Define source-specific types and parsing in `<source_code>_model.rs`.
6. Register the adapter in `main.rs`'s `run-all` subcommand handler and `ingest-source` dispatch.
7. Write tests against checked-in sample responses in `ingestion/samples/<source_code>/`.

### First source: World Bank WDI

The first concrete adapter (and the first `/speckit-specify` feature) is World Bank WDI for TFR:

- Endpoint: `https://api.worldbank.org/v2/country/all/indicator/SP.DYN.TFRT.IN?format=json&per_page=20000`
- Response shape: JSON array `[paging_metadata, [rows...]]` where each row has `country.id`, `date` (year), `value`, etc.
- Coverage: ~200 countries, years ~1960–latest-published.
- License: CC BY 4.0 → `license_class = 'attribution'`.
- `data_source.code = "wb_wdi"`; `preference_rank = 90` (lowest priority among fertility-data sources because WB aggregates from elsewhere).

The full implementation lives in the feature spec; this section documents only what's relevant to the ingestion architecture (the adapter is a normal instance of the contract above).

## Source-preference merge

When multiple sources publish a value for the same `(region, statistic, period_start, period_end)`, all rows stay in `statistic_value`. The merge into a single "what does the user see?" value happens at **artifact-build time**, not at ingestion time. This keeps every source's contribution intact in the canonical store for reproducibility, license accounting, and rollback.

### Merge rule

For each `(region, statistic, period_start, period_end)` cell published into the artifact:

1. Select all candidate rows from `statistic_value`.
2. Filter by license-class eligibility (the base shard only considers rows whose `data_source.license_class` is `public_domain` or `attribution`; the `share_alike` shard adds rows of class `attribution_sa`; the `noncommercial` shard adds rows of class `noncommercial`).
3. Among eligible candidates, pick the row with the lowest `data_source.preference_rank`. Ties (allowed — `preference_rank` is not unique) break by the lower `data_source.id`, which gives a stable arbitrary ordering when two sources sit at the same priority.
4. If the picked source's `data_status` is `provisional`/`preliminary` AND a lower-priority source has a `final` value for the same `(period_start, period_end)` whose `period_end` is within the last 2 years, prefer the `final` value. (Don't show stale "final" data when fresher "preliminary" data exists, and don't show preliminary data when a high-quality final value is available.)

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

- **Source**: Natural Earth 1:50m Cultural Vectors (`ne_50m_admin_0_countries.zip` for v1; subnational comes from `ne_10m_admin_1_states_provinces.zip` in v2+). A specific Natural Earth release is pinned in code (e.g. `5.1.2`) for build reproducibility.
- **License**: Public domain (`license_class = 'public_domain'` in the `data_source` record).
- **Pipeline**: Natural Earth is fetched at artifact-build time from a pinned URL (`https://naciscdn.org/naturalearth/...`), processed in-memory (project to WGS84 — already is — and join to `country.iso3` via Natural Earth's `ADM0_A3` field), and emitted as FlatGeobuf (with R-tree spatial index built in) directly to the artifact output directory. **Nothing is staged into Postgres; nothing is checked into git; no persistent cache between builds.** At weekly cadence with a ~3 MB upstream zip, fetching fresh per build is trivial.

Two-tier shard model (per License-segmented shards in the overview) does not apply to geometry: Natural Earth is public domain, so geometry ships in the base FlatGeobuf without segmentation.

## Artifact builder

### Entrypoint

```rust
pub async fn build_artifacts(
    pool: &PgPool,
    output_dir: &Path,
    version_label: &str,
) -> Result<LocalArtifactBuild, AppError> {
    let candidate_values: Vec<CandidateValue> = read_candidate_values(pool).await?;
    let data_source_versions: BTreeMap<String, String> = collect_data_source_versions(&candidate_values);
    let merged_values: Vec<MergedValue> = apply_source_preference_merge(candidate_values);
    let sqlite_shards: Vec<ShardOutput> = emit_sqlite_shards(&merged_values, output_dir).await?;
    let geometry_shard: ShardOutput = emit_geometry_flatgeobuf(output_dir).await?;
    let hashed_outputs: HashedOutputs = compute_content_hashes(sqlite_shards, geometry_shard).await?;
    let manifest_output: ShardOutput = emit_manifest(&hashed_outputs, version_label, &data_source_versions, output_dir).await?;
    Ok(LocalArtifactBuild {
        version_label: version_label.to_string(),
        hashed_outputs,
        manifest_output,
        data_source_versions,
    })
}
```

`build_artifacts` is local-only: it produces files on disk and returns a `LocalArtifactBuild` describing what was built. **It does not write to the `artifact_version` table.** That row is inserted by `upload_artifacts_to_r2` after the files are published to the CDN — so `artifact_version` records published artifacts, not local builds (see the `artifact_version` table intro above).

Each helper is a separate named function inside the `artifact` module; the orchestrator only sequences them. The helper contracts:

- **`read_candidate_values(pool)`** → `Vec<CandidateValue>`. SELECTs from `statistic_value` with joins to `data_source` (for `license_class` and `preference_rank`) and `statistic` (for `code`). Returns every candidate row that could appear in some shard. For v1's data volume (~tens of thousands of rows) the whole set fits in memory; if it grows substantially, stream into a chunked merge.
- **`collect_data_source_versions(candidates)`** → `BTreeMap<String, String>`. Reduces candidates to one entry per source (`data_source.code` → max `data_source_revision`). The result is what goes into `manifest.json`'s `data_source_versions_jsonb` field and the `artifact_version.data_source_versions_jsonb` column. Pure function — no I/O.
- **`apply_source_preference_merge(candidates)`** → `Vec<MergedValue>`. Groups candidates by `(region_id, statistic_id, period_start, period_end, license_class)` and applies the merge rule from §Source-preference merge (lowest `preference_rank` wins; data-status overrides where applicable). Pure function — no I/O.
- **`emit_sqlite_shards(merged, output_dir)`** → `Vec<ShardOutput>`. Groups merged values by `(statistic_code, license_class)`, opens a SQLite file per group under `output_dir/data/`, writes rows, returns metadata for each file (path + size; not yet content-hashed).
- **`emit_geometry_flatgeobuf(output_dir)`** → `ShardOutput`. Downloads the pinned Natural Earth release from `naciscdn.org`, processes the shapefile in-memory to the FlatGeobuf shape we ship (joined to `country.iso3` by `ADM0_A3`), writes to `output_dir/geometry/`. Single file, no license-class split — Natural Earth is `public_domain`.
- **`compute_content_hashes(shards, geometry)`** → `HashedOutputs`. SHA-256 every output file, rename from `*.tmp-<uuid>` to `*-<sha8>.<ext>` only after hashing succeeds, returns paths + full hashes for the manifest. The manifest itself is excluded — it has a stable filename (`manifest.json`).
- **`emit_manifest(hashed, version_label, data_source_versions, output_dir)`** → `ShardOutput`. Builds `manifest.json` from `hashed` + `version_label` + `data_source_versions`, writes to `output_dir/manifest.json`, computes its own SHA-256 (for the eventual `artifact_version.manifest_sha256` column). Filename is NOT content-hashed because clients fetch by stable URL with a short cache.

`LocalArtifactBuild` is a plain owned struct holding everything `upload_artifacts_to_r2` needs to publish + record the artifact_version row:

```rust
pub struct LocalArtifactBuild {
    pub version_label: String,
    pub hashed_outputs: HashedOutputs,
    pub manifest_output: ShardOutput,
    pub data_source_versions: BTreeMap<String, String>,
}
```

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

Filenames are `<statistic_code>-<license_class>-<sha8>.sqlite` (with `base` covering `public_domain` + `attribution`). Content-hash suffix uses the first 8 hex chars of SHA-256 for filename brevity; the full hash is in the manifest.

### Manifest format

```json
{
  "version": "2026-05-18",
  "artifact_created": "2026-05-18T03:00:00Z",
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

The client loads the manifest first, then fetches whatever shards its license class permits. The base shard is always present; non-base shards may be missing for a given statistic if no rows in that class exist.

### License-class shard mapping

| `data_source.license_class` value | Shard          |
|-----------------------------|----------------|
| `public_domain`             | `base`         |
| `attribution`               | `base`         |
| `attribution_sa`            | `share_alike`  |
| `noncommercial`             | `noncommercial`|

v1 emits only `base` shards (the only seeded source is WB WDI). The other shards activate when a source with the corresponding license class lands. Clients identify their distribution context and `ATTACH DATABASE` each authorized shard; query results union across attached databases.

### Content hashing and immutability

Every output file is content-hashed. The hash is computed from the file's bytes after all writes complete; the file is renamed from `*.tmp-<uuid>` to `<name>-<sha8>.<ext>` only after hashing succeeds. The manifest's hashes are computed last and reference the renamed paths.

CDN cache headers (set at upload time, not in the artifact itself):

- `manifest.json`: `Cache-Control: public, max-age=300` (short-cached)
- All other artifact files: `Cache-Control: public, max-age=31536000, immutable`

### R2 upload

Uploads happen via a separate CLI step (`ingestion upload-artifacts <version_label>`) so the build can be inspected locally before publishing, or chained inline via `build-artifacts --upload`. The upload orchestrator is responsible for both publishing the files AND inserting the `artifact_version` row — the row's existence means "fetchable from the CDN," so it MUST follow a successful upload:

```rust
pub async fn upload_artifacts_to_r2(
    pool: &PgPool,
    build: LocalArtifactBuild,
) -> Result<ArtifactVersion, AppError> {
    upload_files_to_r2(&build.hashed_outputs, &build.manifest_output).await?;
    let artifact_version: ArtifactVersion = insert_artifact_version(
        pool,
        &build.version_label,
        &build.manifest_output,
        &build.hashed_outputs,
        &build.data_source_versions,
    ).await?;
    Ok(artifact_version)
}
```

Helper contracts:

- **`upload_files_to_r2(hashed, manifest)`** → `()`. Uploads every file in `hashed.shards` + `hashed.geometry` + `manifest` to R2 via reqwest against the S3-compatible API. Credentials come from `secr`-encrypted secrets. Uploads are idempotent — content-hashed filenames mean re-uploading the same file is a semantic no-op (PUT overwrites the same bytes at the same key).
- **`insert_artifact_version(pool, version_label, manifest_output, hashed, data_source_versions)`** → `ArtifactVersion`. INSERTs into `artifact_version` with `version_label`, `manifest_sha256`, `manifest_url` (now resolves to the actual CDN URL because upload just succeeded), and `data_source_versions_jsonb`. Returns the inserted row.

If `upload_files_to_r2` fails, `insert_artifact_version` is not called — the canonical store stays consistent with "rows iff fetchable." The local build remains on disk for a retry.

## Postgres hosting

Postgres runs as a launchd-managed host service via Homebrew. The same recipe applies to every machine that has a canonical store — the user's Mac mini (v1 production hosting) and every developer machine — so the install is identical across environments. `setup.sh` is responsible for executing it.

`setup.sh` (at the repo root) installs Postgres via Homebrew (`brew install postgresql@17`) and configures it as a launchd-managed service. The plist template at `scripts/eafora-postgres.plist.template` is rendered into `~/Library/LaunchAgents/org.eafora.postgres.plist`, then loaded via `launchctl bootstrap gui/$(id -u)`, so Postgres starts at login and on demand. The default database is `eafora` on port 5432; `DATABASE_URL` is `postgresql://localhost/eafora` (set via `dotenvy` from `.env`).

This is a deviation from Singularity's Podman Compose setup (Constitution Principle IV; recorded in v1.3.3). Accepted because v1 ships on a personal Mac mini plus one developer machine — host-installed Postgres removes the Podman dependency and the compose-file plumbing at the cost of portability that doesn't matter at this scale. Containerization may return when cloud deployment lands post-v2.

## Scheduling

### v1: Mac mini M1 + `launchd`

A `launchd` plist (template at `scripts/eafora-ingestion.plist.template`, installed by `setup.sh` to `~/Library/LaunchAgents/org.eafora.ingestion.plist` on the Mac mini) triggers `ingestion run-all` on a schedule:

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

### Seeding the canonical store

A `seed-samples` CLI subcommand populates the canonical store with checked-in sample data:

```sh
cargo run -p ingestion -- seed-samples
```

This loads sample responses from `ingestion/samples/<source_code>/` and replays them through each adapter's normalize-and-insert path. The result is a fully-populated canonical store with the same shape production would have, but with fixed test data.

`seed-samples` does NOT run migrations — that's dbmate's job. The expected workflow is `./dbmate.sh up` first (which applies schema migrations including the seed-data migrations for `country`, `statistic`, and `data_source` reference rows), then `cargo run -p ingestion -- seed-samples` to fill in the sample `statistic_value` rows on top of that schema. `setup.sh` chains them on first-time setup; manual re-seeding after a schema change runs them in that order.

### Running an adapter locally

```sh
cargo run -p ingestion -- ingest-source wb_wdi
```

A full WB WDI run is ~200 countries × ~65 years × ~1 statistic ≈ 13k rows, which is under a second — fast enough to iterate without needing per-country or per-period filters.

### Producing artifacts locally

```sh
cargo run -p ingestion -- build-artifacts ./build-output 2026-05-18
```

Writes `manifest.json` + `geometry/` + `data/` under `./build-output/`. No upload. The artifacts can be served via `python -m http.server` in a pinch (with `Content-Encoding: br` headers ad-hoc'd in front of `nginx` or `caddy` if compression-aware testing is wanted), or pointed at directly from the web client via a local file URL.

## Testing strategy

Per Constitution Principle VII, the ingestion-side TDD-required surfaces are:

- **Per-source normalization** (`<source>_model.rs` parsing functions): every sample response → expected canonical-shape output, exhaustively.
- **Source-preference merge** (`artifact/merge.rs` — the per-cell merge logic): all combinations of `(data_status, preference_rank, period_end)` exercised against the merge rule.
- **Artifact diffing** (used by `build-artifacts` to decide whether a build is no-op): trivial cases (no canonical changes) and tricky cases (rows updated but resulting artifact bytes unchanged) covered.
- **Error mapping** (per-source error → `AppError` → log line): each variant gets a test.

Integration tests use the seeded canonical store via `seed-samples`, exercise `fetch_and_store` against the sample responses (no live HTTP), and assert on the resulting `statistic_value` rows and on the artifact output.

Non-TDD surfaces (still tested, but the test-first discipline is relaxed):

- HTTP wiring (reqwest configuration, timeout/retry policy)
- launchd plist generation
- CLI argument parsing

## Open questions

(None for v1 — the doc has converged on concrete answers for everything previously parked here.)

## Things to verify

1. **dbmate's behavior with the Singularity convention for `cargo sqlx prepare`**: confirm that `dbmate.sh`'s wrapper around `sqlx prepare --workspace` works against a workspace with both `ingestion/` and `core/` crates.
2. **Cloudflare R2 S3-compatible API surface**: confirm reqwest + `aws-sigv4` (or equivalent signature crate) work without the AWS SDK. R2's S3 compatibility is documented but corner cases (multipart upload thresholds, presigned URLs) deserve a spike before relying on them.
3. **Natural Earth license verification**: the dataset is widely described as public domain ("free of charge for any use"), but the actual Natural Earth website's license page should be confirmed and quoted in the seeded `data_source` row.

## Follow-up work

- First `/speckit-specify` feature spec: `specs/NNN-world-bank-wdi-ingestion/` — implements the WB WDI adapter against this plan's contract.
- Per-platform client plans (`docs-architecture-client-{web,ios,android}`) — these depend on the manifest + shard format locked here.
- A `docs-architecture-secrets` mini-plan documenting which secrets the ingestion binary needs (R2 credentials initially; nothing else through v2) and how `secr` integrates with the launchd entrypoint.
- A `scripts/build-fallback.sh` script (referenced from the overview's §Workspace and crate layout) that consumes a real artifact build and downsamples it into the per-platform bundled-fallback resources. Defer until at least one client shell exists.

