# Ingestion architecture

<!--
Status: draft, 2026-05-23. This document is the per-segment implementation plan for Eafora's ingestion service and canonical store, elaborating the §Ingestion + canonical store section of `docs/architecture/overview.md`. It does not relitigate cross-cutting decisions already locked in the overview or the constitution.

The first concrete feature spec under this plan will be the World Bank WDI ingestion CLI (the smallest end-to-end exercise of the canonical store + adapter contract + artifact builder). That spec will go through `/speckit-specify` and live at `specs/NNN-world-bank-wdi-ingestion/`.
-->

> **Implementation-state note (2026-06):** Specific function signatures, CLI invocations, and dependency choices in this document predate the artifact-builder/publish PRs (specs 002) and have drifted from what shipped. The high-level design (adapter → ingest → canonical store → build → publish, with the canonical store as the producer/consumer seam) is correct; the specifics that diverged:
>
> - **CLI shape**: `ingestion ingest source <code>` / `ingestion ingest all` / `ingestion build` / `ingestion publish local|cloudflare-r2|dry [<artifact-dir>] [--build]` (nested sub-subcommands on `ingest` and `publish`, not flat `ingestion source` / `ingestion publish --destination=`).
> - **Auto-generated version labels**: `YYYY-MM-DD+<surname>` from a Nobel-laureate list via `crate::version_label::generate`; not a CLI argument.
> - **`build` writes to `$EAFORA_ARTIFACTS_DIR/<version-label>/`**, not a positional output dir.
> - **Build report type**: `BuildReport` (was `LocalArtifactBuild`); fields are `artifact_dir`, `version_label`, `artifacts: Artifacts`, `data_source_revisions: BTreeMap<DataSourceKind, SourceRevision>`.
> - **Publish orchestrator**: `publish_artifacts(pool, &BuildReport, &ArtifactRepositoryKind) -> PublishReport`; destination is selected by enum variant, not a generic bound.
> - **`ArtifactRepository` trait**: two methods — `put_file(&self, key, source_path, content_type) -> impl Future<...> + Send` and `url(&self, key) -> String`. Three impls: `LocalArtifactRepository`, `CloudflareR2ArtifactRepository`, `DryArtifactRepository`.
> - **R2 client**: `aws-sdk-s3` 1.135 against the R2 endpoint (was planned as raw `reqwest` + `aws-sigv4`).
> - **`artifact_version` insertion**: non-clobbering precheck (`read_artifact_version_exists` returns AppError if the label is taken); not `ON CONFLICT (version_label) DO NOTHING`.
> - **`ingest all` does NOT chain into `build` / `publish`.** Each pipeline stage is its own subcommand the launchd plist invokes separately.
> - **Filesystem types live in `crate::filesystem`**: `FileReference`, `Hashed<T>`, `sha256_hex`, `read_bytes`, `load_hashed_file`, `filename_of`. The `crate::artifact::hashing` module retains only the artifact-build-specific tmp-to-content-hashed rename orchestration.
>
> Per-spec docs (`specs/002-artifact-builder/spec.md` and the `git log` on `impl-publish-flow`) are authoritative for the publish flow's current shape. This document continues to describe the design at the architecture level; treat the inline code samples below as illustrative of intent, not as current API.

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
- Singularity `lobby/` feature-triplet pattern: each feature module is `<feature>_db.rs` + `<feature>_model.rs` plus either `<feature>_api.rs` (if it hosts HTTP routes) or `<feature>_client.rs` (if it consumes an external HTTP API). (Constitution IV)
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
│       ├── main.rs             # tokio CLI entrypoint; dispatches subcommands (source, all, build, seed, publish); the all subcommand loops over the registered adapters inline
│       ├── lib.rs              # re-exports for tests
│       ├── error.rs            # minimer wiring + per-feature variant aggregation
│       ├── world_bank_wdi/     # one source = one per-source module
│       │   ├── mod.rs
│       │   ├── world_bank_wdi_client.rs    # external HTTP client + parse_response (knows the wire format, NOT the canonical store)
│       │   ├── world_bank_wdi_adapter.rs   # normalize (parsed → canonical) + fetch_and_store orchestrator
│       │   └── world_bank_wdi_model.rs     # WB WDI response types + ParsedWdiStatisticValue
│       ├── eurostat/                       # same shape per added source
│       ├── hfd/
│       ├── adapter/                        # cross-adapter types + generic helpers
│       │   ├── adapter_model.rs            # AdapterOptions, NormalizedStatisticValue, NaiveDatePeriod, NormalizeOutcome, IngestWarning
│       │   └── mod.rs
│       ├── ingest/                         # canonical-store writes (source-agnostic)
│       │   ├── ingest.rs                   # record_statistic_values orchestrator with append-with-supersede semantics
│       │   ├── ingest_db.rs                # publication insert/match, find_current_value, insert_statistic_value, set_superseded
│       │   ├── ingest_model.rs             # IngestReport, RecordOutcome
│       │   └── mod.rs
│       ├── canonical/                      # cross-cutting reads of the canonical reference tables
│       │   ├── canonical_db.rs             # find_country_by_iso3, find_statistic_by_code, find_data_source_by_code
│       │   └── canonical_model.rs          # Region, Country, Statistic, DataSource, StatisticValue + StatisticCode/DataStatus enums
│       ├── artifact/                       # artifact builder
│       │   ├── artifact_api.rs             # CLI handlers for artifact build / inspection
│       │   ├── artifact_db.rs              # queries that drive the build (read fact table)
│       │   ├── artifact_model.rs           # Manifest, ArtifactVersion, build options
│       │   └── writer/                     # output-shape writers
│       │       ├── flatgeobuf.rs           # geometry shard writer
│       │       ├── sqlite.rs               # per-statistic, per-license-tier shard writer
│       │       └── manifest.rs             # manifest.json builder + content hashing
│       └── geometry/                       # Natural Earth fetch + parse (sibling to artifact/; subnational lands here in v2+)
```

Through v2 the `ingestion/` binary is a CLI: `ingestion <subcommand>` — `source <code>`, `build`, `seed`, `publish`, etc. Used for manual invocation, `launchd` triggers, and local dev. Per-feature module layout follows the Singularity `lobby/` convention with two suffix variants — `<feature>_api.rs` for HTTP routes Eafora HOSTS (none in v1; reserved for v3+ HTTP server mode) and `<feature>_client.rs` for code that calls OUT to an external HTTP API. Per-source modules split further: `<source>_client.rs` owns HTTP + parse (knows the wire format but not the canonical store), `<source>_adapter.rs` owns normalize + the `fetch_and_store` orchestrator (knows the canonical store), and `<source>_db.rs` is reserved for source-specific SQL when needed (often absent — generic canonical-store writes live in `crate::ingest::ingest_db`).

### CLI structure (clap builder API)

CLI arg parsing and dispatch use **clap**'s **builder** API (not the derive macros). Matches Constitution Principle V's explicit-over-implicit preference — the command tree is constructed imperatively, with each subcommand's arguments visible at the call site rather than derived from struct attributes.

```rust
// ingestion/src/main.rs
use clap::{Arg, ArgAction, ArgMatches, Command};

fn build_cli() -> Command {
    Command::new("ingestion")
        .subcommand_required(true)
        .subcommand(
            Command::new("source")
                .about("Run a single source adapter")
                .arg(Arg::new("source").required(true).help("source code (e.g. wb_wdi)"))
                .arg(Arg::new("force-full-refetch").long("force-full-refetch").action(ArgAction::SetTrue)),
        )
        .subcommand(Command::new("all").about("Run every registered source adapter"))
        .subcommand(
            Command::new("build")
                .about("Build CDN artifacts from the current canonical store"),
        )
        .subcommand(Command::new("seed").about("Load checked-in sample responses into the canonical store"))
        .subcommand(
            Command::new("publish")
                .about("Upload a previously-built artifact set to R2")
                .arg(Arg::new("version-label").required(true)),
        )
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let matches: ArgMatches = build_cli().get_matches();
    match matches.subcommand() {
        Some(("source",    sub_matches)) => dispatch_source(sub_matches).await,
        Some(("all",          _))           => dispatch_all().await,
        Some(("build",  _))           => dispatch_build().await,
        Some(("seed",     _))           => dispatch_seed().await,
        Some(("publish", sub_matches)) => dispatch_publish(sub_matches).await,
        _                                        => unreachable!("subcommand_required guarantees a match"),
    }
}
```

Each subcommand has a `dispatch_*` helper that reads its specific arguments from `ArgMatches` and calls into the relevant feature module (`world_bank_wdi::fetch_and_store(...)`, `artifact::build_artifacts(...)`, etc.). The `dispatch_*` helpers live alongside `main` in `main.rs` for the all orchestration case, or — if a dispatch grows non-trivial — in the relevant feature module's `_api.rs` (when the feature hosts routes) or `_client.rs` (when it consumes an external API).

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

comment on column data_source_publication.revision_label is $$the source's own revision label for this publication event (WB WDI '2024-Q4', Eurostat '2026-w20', HFD '2025-12', WPP 'WPP-2024-rev1'); sources without native versioning get a synthesized label (response payload hash or fetch date); read by the adapter's read_latest_publication_revision step for incremental fetches; aggregated per-source into the manifest's data_source_versions_jsonb at artifact-build time$$;
comment on column data_source_publication.published      is $$source's own publication timestamp where derivable (often only a year or version label, hence nullable)$$;
comment on column data_source_publication.fetched        is $$wall-clock instant our adapter captured this publication$$;
```

Publications are append-only — once captured, the row stays as an audit trail. If a source republishes under the same `revision_label` (a re-fetch with no upstream change), the existing row is matched on `(data_source_id, revision_label)` and no insert happens. If the source publishes a new revision, a new row is inserted; the old publication row stays in this table indefinitely.

#### `statistic_value`

The fact table, **append-only** with respect to source revisions. Each row is "what publication X said about cell (region, statistic, period)." When a source publishes a revision of a previously-captured value, a new row is inserted for the new publication's view; the previous row's `superseded` timestamp is set to mark it as no longer current. Both rows stay in the table — `superseded is null` filters to the current view; the full set is the revision history.

`region_id` can point at any level — country (the common case for v1), subnational region (when subnational data lands in v2+), or supranational grouping (for stored aggregates like an EU-wide TFR). Periods are half-open intervals (inclusive start, exclusive end); see the column comments for examples.

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
  superseded                 timestamp with time zone,
  created                    timestamp with time zone not null default now(),
  modified                   timestamp with time zone not null default now(),
  unique (region_id, statistic_id, period_start, period_end, data_source_publication_id)
);

create unique index if not exists statistic_value_current_per_source
  on statistic_value (region_id, statistic_id, period_start, period_end, data_source_id)
  where superseded is null;

comment on column statistic_value.region_id                  is $$points at any level — country (common in v1), subnational (v2+ when subnational data lands), or supranational grouping (for stored aggregates)$$;
comment on column statistic_value.period_start               is $$inclusive lower bound: calendar year 2024 → '2024-01-01'; Q1 2024 → '2024-01-01'; 2020-2025 cohort → '2020-01-01'$$;
comment on column statistic_value.period_end                 is $$exclusive upper bound: calendar year 2024 → '2025-01-01'; Q1 2024 → '2024-04-01'; 2020-2025 cohort → '2025-01-01'$$;
comment on column statistic_value.data_source_id             is $$denormalized from data_source_publication.data_source_id; needed for the partial unique index that enforces 'at most one current row per cell per source'; the upsert path keeps the two in sync$$;
comment on column statistic_value.data_source_publication_id is $$points at the publication event this row's value was captured from; the row is never updated to point elsewhere — when the source revises, a NEW row is inserted with the new publication, and this row's superseded timestamp is set$$;
comment on column statistic_value.data_status                is $$one of: final | provisional | preliminary | projection | imputed | interpolated$$;
comment on column statistic_value.superseded                 is $$wall-clock instant when this row stopped being the current view of its (region, statistic, period, data_source_id) cell — i.e., when a newer publication for the same source produced a different value, this row got marked as historical. NULL means current (the row reflects the latest publication's view of the cell)$$;
```

Two unique constraints together encode the model:
- **Full unique on `(region, statistic, period, publication)`** — at most one row per "what this publication said about this cell" (prevents accidental duplicate inserts on re-fetch of the same publication).
- **Partial unique on `(region, statistic, period, source) where superseded is null`** — at most one CURRENT row per cell per source (the supersede flow maintains this invariant).

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

Migrations live in `ingestion/db/migrations/` as dbmate timestamped SQL files (`20260524123000_create_country.sql`, etc.). Each file has `-- migrate:up` and `-- migrate:down` sections. The `scripts/db/dbmate.sh` wrapper runs migrations and then re-runs `cargo sqlx prepare --workspace` to refresh the offline cache. Per the Singularity convention, `scripts/db/dbmate.sh` is the only supported way to apply migrations locally.

Seed data (country list from ISO 3166, statistic definitions, source records) lives in `ingestion/db/migrations/` as ordinary INSERT migrations rather than a separate seed mechanism — this keeps the canonical reference data versioned and reproducible across dev/CI/prod.

## Per-source adapters

### Adapter contract

Every source adapter exposes one entrypoint that opens a transaction and orchestrates a fixed pipeline of named helpers — fetch + parse in `<source>_client`, normalize in `<source>_adapter`, persistence in the source-agnostic `crate::ingest`:

```rust
pub async fn fetch_and_store(
    pool: &PgPool,
    options: AdapterOptions,
) -> Result<IngestReport, AppError> {
    let mut transaction = pool.begin().await?;

    let data_source = canonical_db::find_data_source_by_code(&mut *transaction, "wb_wdi").await?...;
    let last_seen_revision = ingest::ingest_db::read_latest_publication_revision(&mut *transaction, data_source.id).await?;

    let raw = world_bank_wdi_client::fetch_upstream(options).await?;
    let revision_label = raw.0.lastupdated.clone();
    let parsed = world_bank_wdi_client::parse_response(raw)?;

    let (normalized, warnings) = normalize(&mut *transaction, parsed).await?;

    let mut report = ingest::record_statistic_values(&mut *transaction, data_source.id, &revision_label, Utc::now(), normalized).await?;
    report.warnings = warnings;

    transaction.commit().await?;
    Ok(report)
}
```

Each helper contract:

- **`read_latest_publication_revision(executor, data_source_id)`** → `Option<String>` (in `ingest::ingest_db`). Queries the most recent `data_source_publication.revision_label` for this source. `None` means "first run."
- **`fetch_upstream(options)`** → source-specific `RawResponse` (in `<source>_client`). HTTP fetch via reqwest. Honors `options.force_full_refetch`.
- **`parse_response(raw)`** → `Vec<Parsed<Source>StatisticValue>` (in `<source>_client`). Deserializes the source-specific response into intermediate types defined in `<source>_model.rs`. Pure function — no I/O, no DB access. May silently drop rows that aren't statistic-shaped (e.g. WB WDI's regional aggregates with empty `countryiso3code` get dropped here).
- **`normalize(connection, parsed)`** → `(Vec<NormalizedStatisticValue>, Vec<IngestWarning>)` (in `<source>_adapter`). Joins to `region` (via `country.iso3` for country-level data) and `statistic` by code, computes the `NaiveDatePeriod` from the source's time encoding, attaches `DataStatus` and the appropriate statistic id. Reads from the DB to resolve foreign keys; never writes. Rows whose country isn't in the seed produce an `UnknownCountry` warning and are dropped from the normalized output; rows with `value: None` produce a `NotApplicableValue` warning and are dropped.
- **`record_statistic_values(connection, data_source_id, revision_label, fetched, normalized)`** → `IngestReport` (in `crate::ingest`, source-agnostic). First inserts the `data_source_publication` row (or matches an existing one with the same `(data_source_id, revision_label)`); then for each `NormalizedStatisticValue` looks up the current `superseded is null` row in `statistic_value` for `(region_id, statistic_id, period.start, period.end, data_source_id)`. If none → INSERT; counts `values_added`. If a current row exists with the same `value` and `data_status` → skip; counts `values_skipped`. If a current row exists with different `value` or `data_status` → UPDATE the old row's `superseded = now()`, then INSERT a new row pointing at the new publication; counts `values_revised`. Per-row classification is `RecordOutcome` (Added | Revised | Skipped); `record_statistic_value` (singular) is the per-row helper.

Adapters are independent of each other. Adding a new source is one new feature module (`<source>_client.rs`, `<source>_adapter.rs`, `<source>_model.rs`) plus a migration that inserts the `data_source` record. Source-specific SQL (rare) goes in `<source>_db.rs`; generic canonical-store writes stay in `crate::ingest::ingest_db`.

**Maintain a consistent adapter shape across sources.** Every adapter exposes the same pipeline (`read_latest_publication_revision`, `fetch_upstream`, `parse_response`, `normalize`, `record_statistic_values`) with the same return shapes, even when a given source could in principle be simpler. The discipline is intentional: when source #2 lands and the shape proves stable across two real examples, the orchestrator becomes a one-time refactor into a shared `Adapter` trait (the public surface is already trait-ready by convention). Premature trait extraction with only one example would risk locking in a shape that's wrong for source #2; deferring requires every new source to mirror WB WDI's shape so we keep the option open.

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
    pub values_added: u64,
    pub values_revised: u64,
    pub values_skipped: u64,
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
// ingestion/src/world_bank_wdi/world_bank_wdi_client.rs
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
3. Implement `fetch_and_store` (the orchestrator) and the per-source helpers (`fetch_upstream` + `parse_response` in `<source_code>_client.rs`; `normalize` in `<source_code>_adapter.rs`). Generic helpers (`read_latest_publication_revision`, `record_statistic_values`) live in `crate::ingest` and don't need re-implementing per source.
4. Implement source-specific SQL in `<source_code>_db.rs`.
5. Define source-specific types and parsing in `<source_code>_model.rs`.
6. Register the adapter in `main.rs`'s `all` subcommand handler and `source` dispatch.
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

1. Select all candidate rows from `statistic_value` where `superseded is null`.
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
- **Features Natural Earth does not attribute to a seeded country**: the release ships four, and the source's own `TYPE` field splits them in two. `ATC` (Ashmore and Cartier Islands) and `IOA` (Christmas Island and the Cocos (Keeling) Islands) are `Dependency` records carrying `SOVEREIGNT` "Australia", so they are aliased to `AUS` in `ADM0_A3_TO_CANONICAL_ISO3` and merge into Australia's feature, as Somaliland and Northern Cyprus merge into their sovereign. `KAS` (Siachen Glacier, `NOTE_BRK` "Claimed by Pakistan and India") and `ATA` (Antarctica, "Multiple claims held in abeyance by treaty") are `Indeterminate` records that name no sovereign, and they are dropped: attributing them would assert an attribution the source withheld.
- **The resulting gap near Kashmir**: dropping `KAS` leaves a wedge of bare substrate measuring approx. 50 by 80 km between the Indian, Pakistani and Chinese fills, visible at default zoom. A grid scan at 0.05 degrees over latitude 30 to 40 and longitude 70 to 82 found no other uncovered point in the region. The gap is accepted. Filling it means either emitting it under a region code no statistic matches, which paints a distinct tone and so asserts a distinguishable region, or folding it into a neighbour, which draws a boundary across a stretch other cartographers leave unmarked.

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
  "manifest_schema_version": 1,
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

`manifest_schema_version` is the FIRST key and is `1` for v1. Consumers reject manifests whose `manifest_schema_version` they don't recognize, so v2+ shape changes (added field, renamed key, restructured `statistics` map) get a typed parse failure rather than a silent misinterpretation. Producers always emit it; the version bumps when the on-the-wire shape changes in a way old clients can't handle. See `docs/architecture/client.md` §Manifest schema (consumer view) for the consumer-side guarantees and `specs/005-core-data/spec.md` for the parse-and-validate contract.

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

Uploads happen via a separate CLI step (`ingestion publish <version_label>`) so the build can be inspected locally before publishing, or chained inline via `build --upload`. The upload orchestrator is responsible for both publishing the files AND inserting the `artifact_version` row — the row's existence means "fetchable from the CDN," so it MUST follow a successful upload:

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

`setup.sh` (at the repo root) installs Postgres via Homebrew (`brew install postgresql@18`) and configures it as a launchd-managed service. The plist template at `scripts/eafora-postgres.plist.template` is rendered into `~/Library/LaunchAgents/org.eafora.postgres.plist`, then loaded via `launchctl bootstrap gui/$(id -u)`, so Postgres starts at login and on demand. The default database is `eafora` on port 5432; `DATABASE_URL` is `postgresql://localhost/eafora` (set via `dotenvy` from `.env`). PG18 is required for the built-in `uuidv7()` function used as the default for every table's primary key.

This is a deviation from Singularity's Podman Compose setup (Constitution Principle IV; recorded in v1.3.3). Accepted because v1 ships on a personal Mac mini plus one developer machine — host-installed Postgres removes the Podman dependency and the compose-file plumbing at the cost of portability that doesn't matter at this scale. Containerization may return when cloud deployment lands post-v2.

## Scheduling

### v1: Mac mini M1 + `launchd`

A `launchd` plist (template at `ingestion/eafora-ingestion.plist.template`, installed by `setup.sh` to `~/Library/LaunchAgents/org.eafora.ingestion.plist` on the Mac mini) triggers `ingestion all` on a schedule:

```xml
<key>StartCalendarInterval</key>
<dict>
  <key>Weekday</key>  <integer>1</integer>   <!-- Monday -->
  <key>Hour</key>     <integer>3</integer>
  <key>Minute</key>   <integer>0</integer>
</dict>
```

`ingestion ingest all` invokes every registered adapter sequentially (parallelism is unnecessary at v1's source count; cross-adapter dependencies are nil). **An error from one adapter does NOT block subsequent adapters** — the orchestrator catches the `AppError`, logs it as the failed adapter's outcome, and continues with the next adapter. The process exit status reflects whether any adapter failed (non-zero if at least one returned `AppError`, zero if all succeeded). `ingest all` does NOT chain into `build` or `publish` — those are separate subcommands the launchd plist invokes on its own schedule.

Manual invocation is always supported: `ingestion ingest source wb_wdi --force-full-refetch` re-runs a single adapter ignoring incremental state. Per Constitution §Tooling discipline, both the scheduled path and the manual path go through the same CLI subcommands; `launchd` calls the same binary the developer calls.

### v2+: managed compute

When the Mac mini becomes insufficient (HA, geographic distribution, or v3+ live API needs), migrate to a managed-cloud Postgres + a managed scheduled-job runner. The CLI shape doesn't change — only the launchd plist gets replaced with cron, systemd, AWS EventBridge, or whatever the post-migration platform offers. Deferred until forced.

## Local development

### Seeding the canonical store

> **Status (2026-06): deferred indefinitely; subcommand removed.** No `ingestion seed` subcommand currently exists in the CLI. The live path (`ingestion ingest source wb_wdi`) takes about a second against the WB WDI API and works fine for first-time setup — the offline / no-internet use case that motivated `seed` (CI without network access, demo machines, fully reproducible fixture data across developers) isn't currently pressing. Re-add when one of those cases becomes a real blocker; the design below is preserved as the intended shape.

A `seed` CLI subcommand populates the canonical store with checked-in sample data:

```sh
cargo run -p ingestion -- seed
```

This loads sample responses from `ingestion/samples/<source_code>/` and replays them through each adapter's normalize-and-insert path. The result is a fully-populated canonical store with the same shape production would have, but with fixed test data.

`seed` does NOT run migrations — that's dbmate's job. The expected workflow is `./scripts/db/dbmate.sh up` first (which applies schema migrations including the seed-data migrations for `country`, `statistic`, and `data_source` reference rows), then `cargo run -p ingestion -- seed` to fill in the sample `statistic_value` rows on top of that schema. `setup.sh` chains them on first-time setup; manual re-seeding after a schema change runs them in that order.

### Running an adapter locally

```sh
cargo run -p ingestion -- ingest source wb_wdi
```

A full WB WDI run is ~200 countries × ~65 years × ~1 statistic ≈ 13k rows, which is under a second — fast enough to iterate without needing per-country or per-period filters.

### Producing artifacts locally

```sh
cargo run -p ingestion -- build
```

Writes `manifest.json` + `geometry/` + `data/` under `$EAFORA_ARTIFACTS_DIR/<auto-generated-label>/`. No upload. The artifacts can be served via `python -m http.server` in a pinch (with `Content-Encoding: br` headers ad-hoc'd in front of `nginx` or `caddy` if compression-aware testing is wanted), or pointed at directly from the web client via a local file URL.

## Testing strategy

Per Constitution Principle VII, the ingestion-side TDD-required surfaces are:

- **Per-source normalization** (`<source>_model.rs` parsing functions): every sample response → expected canonical-shape output, exhaustively.
- **Source-preference merge** (`artifact/merge.rs` — the per-cell merge logic): all combinations of `(data_status, preference_rank, period_end)` exercised against the merge rule.
- **Artifact diffing** (used by `build` to decide whether a build is no-op): trivial cases (no canonical changes) and tricky cases (rows updated but resulting artifact bytes unchanged) covered.
- **Error mapping** (per-source error → `AppError` → log line): each variant gets a test.

Integration tests use the seeded canonical store via `seed`, exercise `fetch_and_store` against the sample responses (no live HTTP), and assert on the resulting `statistic_value` rows and on the artifact output.

Non-TDD surfaces (still tested, but the test-first discipline is relaxed):

- HTTP wiring (reqwest configuration, timeout/retry policy)
- launchd plist generation
- CLI argument parsing

## Open questions

(None for v1 — the doc has converged on concrete answers for everything previously parked here.)

Deferred-but-not-blocking ingestion work lives in `docs/backlog.md` §Ingestion / producer.

## Things to verify

1. **dbmate's behavior with the Singularity convention for `cargo sqlx prepare`**: confirm that `dbmate.sh`'s wrapper around `sqlx prepare --workspace` works against a workspace with both `ingestion/` and `core/` crates.
2. **Cloudflare R2 S3-compatible API surface**: confirm reqwest + `aws-sigv4` (or equivalent signature crate) work without the AWS SDK. R2's S3 compatibility is documented but corner cases (multipart upload thresholds, presigned URLs) deserve a spike before relying on them.
3. **Natural Earth license verification**: the dataset is widely described as public domain ("free of charge for any use"), but the actual Natural Earth website's license page should be confirmed and quoted in the seeded `data_source` row.

## Follow-up work

- First `/speckit-specify` feature spec: `specs/NNN-world-bank-wdi-ingestion/` — implements the WB WDI adapter against this plan's contract.
- Per-platform client plans (`docs-architecture-client-{web,ios,android}`) — these depend on the manifest + shard format locked here.

Deferred-but-not-blocking ingestion work (secrets mini-plan, geometry-decoupling, etc.) lives in `docs/backlog.md` §Ingestion / producer.

