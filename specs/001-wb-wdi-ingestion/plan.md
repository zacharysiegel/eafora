# Implementation Plan: World Bank WDI ingestion CLI

**Branch**: `002-wb-wdi-plan` | **Date**: 2026-05-24 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification at `specs/001-wb-wdi-ingestion/spec.md`

## Summary

Implements the `fetch_and_store` adapter contract from `docs/architecture/ingestion.md` for the World Bank World Development Indicators source, capturing TFR (`SP.DYN.TFRT.IN`) into the canonical store. The architecture doc has already locked the schema, the helper-function contract, the publication-table model, the append-with-supersede revision strategy, the merge rules, and the IngestReport shape. This plan focuses on the WB-WDI-specific deliverables: module layout, dependencies, migration files, sample-response files, the test harness, and the wiring into `main.rs`.

## Technical Context

**Language/Version**: Rust 2024 edition; `rustfmt` config `max_width = 120`, `chain_width = 100` (per Constitution Principle IV locked picks).

**Primary Dependencies**:

- `tokio` (with `features = ["full"]`)
- `sqlx` (with `runtime-tokio`, `postgres`, `uuid`, `chrono`, `json` features) using `query_as!` and the offline cache
- `reqwest` (with `rustls-tls`, `json` features)
- `serde` + `serde_json` for response parsing
- `chrono` for `NaiveDate` (period bounds) and `DateTime<Utc>` (run timestamps)
- `uuid` (with `v7` and `serde` features) for primary keys
- `clap` (4.x, builder API only — no derive macros per the registered preference)
- `log` + `env_logger` for the structured-log lines
- `dotenvy` for env-loading
- `secr` (user's crate) for secret resolution (R2 credentials in v3+; not used by this feature directly but the runtime is present)
- `minimer` (user's crate) for the `AppError` shape — see Constitution Principle IV

**Storage**: PostgreSQL via Homebrew + launchd on the host, per `docs/architecture/ingestion.md` §Postgres hosting. The dev / prod divergence is just `DATABASE_URL` pointing at the local instance; no environment-specific schema variation. Production hosting is the Mac mini.

**Testing**: `cargo test` for unit + integration. Integration tests use a dedicated `eafora_test` PostgreSQL database on the same host (separate from `eafora`); migrations are applied to it via `dbmate.sh` ahead of the test run; each integration test wraps its work in a transaction that's rolled back at teardown so tests don't pollute each other.

**Target Platform**: macOS (Mac mini M1 for prod, dev machines per developer); the binary is a CLI, not a server. No cross-compilation needed for v1.

**Project Type**: CLI binary inside the `ingestion/` workspace member.

**Performance Goals**: A full WB WDI ingestion run (fetch + parse + normalize + upsert) completes in under 5 seconds (spec SC-006). At ~13k rows fetched-and-considered per run, this targets ~2,500 rows/sec evaluation throughput including the network round-trip.

**Constraints**: No live API surface from this feature — the binary is invoked via CLI subcommand only. No editorial copy is added (Constitution Principle I). All upstream attribution must be displayable (the seeded `data_source.attribution_text` carries WB's required string).

**Scale/Scope**: ~13,000 rows per WB WDI ingestion run (~200 countries × ~65 years × 1 statistic). Per Constitution v1.3.3 the canonical store fits comfortably in a single Postgres on a Mac mini.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

The spec's Constitution Check (`spec.md` §Constitution Check) enumerates all eight principles and confirms no violations. The plan adds no new violations. The non-trivial points worth re-affirming at planning time:

- **Principle V (Explicit over implicit)**: clap builder API only, hand-written `sqlx::query_as!`, no ORM, no derive macros, imperative subcommand dispatch. The lobby/-triplet `world_bank_wdi_api.rs` file holds CLI handlers with no actix-web routes wired up; routes can land later if a v3+ HTTP server mode arrives.
- **Principle VII (Test-first for core logic)**: `parse_response` and `normalize` are core logic surfaces and follow Red-Green-Refactor. Tests authored before implementation; coverage measured against the spec's SC-005 (≥90% line coverage on those helpers).
- **Principle IV (Singularity convention parity)**: every dependency listed above is in Singularity's set or is `clap` (newly registered with the user's preference for the builder API; will be added to Constitution IV's locked-picks list in a follow-up amendment).

**Gate result: PASS** — proceed to design.

## Project Structure

### Documentation (this feature)

```text
specs/001-wb-wdi-ingestion/
├── spec.md              # Feature specification (already merged into master)
├── plan.md              # This file
├── checklists/
│   └── requirements.md  # Spec quality checklist (already merged)
└── (no separate research.md / data-model.md / contracts/ / quickstart.md)
```

The standard spec-kit phase outputs (`research.md`, `data-model.md`, `contracts/`, `quickstart.md`) are intentionally not generated for this feature: there are no `[NEEDS CLARIFICATION]` markers to research; the data model lives in `docs/architecture/ingestion.md` and would be redundant to restate; the helper contracts are in `docs/architecture/ingestion.md` §Adapter contract; and the developer quickstart is captured in this plan's §Local development section below. If a future feature surfaces genuinely new design questions, those phase artifacts would be generated then.

### Source Code (repository root)

```text
ingestion/                              # workspace member; new in this feature
├── Cargo.toml                          # depends: tokio, sqlx, reqwest, serde, serde_json, chrono, uuid, clap, log, env_logger, dotenvy, secr, minimer, core
├── db/
│   ├── schema.sql                      # dbmate-generated cumulative schema (regenerated after each migration)
│   └── migrations/
│       ├── YYYYMMDDHHMMSS_create_initial_schema.sql    # all 7 tables: region, country, statistic, data_source, data_source_publication, statistic_value, artifact_version (with CREATE INDEX for the partial unique on statistic_value's superseded-NULL rows, and all COMMENT ON COLUMN statements)
│       └── YYYYMMDDHHMMSS_seed_initial_data.sql        # all reference data: UN M49 region hierarchy + ISO 3166 country extension rows + 'tfr' statistic + 'wb_wdi' data_source
├── samples/
│   └── wb_wdi/
│       ├── happy_path.json             # full WB WDI TFR response, ~200 countries × ~65 years
│       ├── na_value.json               # subset including a (USA, 2024, value=null) row to exercise the NA-value warning
│       └── unknown_country.json        # subset including a country code we don't have in our country table to exercise the unknown-code warning
├── src/
│   ├── main.rs                         # tokio CLI entrypoint; clap builder; dispatches source / all / build / seed / publish; the all subcommand loops over registered adapters inline (one adapter's AppError does not block others — caught/logged/continue); calls db::create_pool() to obtain the PgPool that gets threaded through to every dispatch
│   ├── lib.rs                          # re-exports for tests
│   ├── error.rs                        # AppError = minimer::Error; format-string error construction at failure sites
│   ├── db.rs                           # PgPool bootstrap: reads DATABASE_URL from env, builds a PgPool with the project's pool-config defaults, returns it; symmetric with tests/helpers/test_db.rs but for the production binary
│   ├── world_bank_wdi/                 # the lobby/-triplet for this source
│   │   ├── mod.rs
│   │   ├── world_bank_wdi_api.rs       # CLI handler + fetch_and_store orchestrator + the five named helpers (read_latest_publication_revision, fetch_upstream, parse_response, normalize, upsert_rows)
│   │   ├── world_bank_wdi_db.rs        # sqlx queries scoped to this source's ingestion (publication INSERT, statistic_value lookup-current/INSERT/UPDATE-superseded)
│   │   └── world_bank_wdi_model.rs     # WB WDI response types (paging metadata, row shape), ParsedRow, NormalizedRow
│   └── canonical/                      # cross-cutting reads of the canonical store (used by future adapters too)
│       ├── canonical_db.rs             # shared sqlx queries (region/country/statistic lookups by code)
│       └── canonical_model.rs          # shared entity types
└── tests/
    ├── world_bank_wdi_integration.rs   # end-to-end against the test database, replaying samples/wb_wdi/*.json
    └── helpers/
        ├── mod.rs
        ├── test_db.rs                  # acquires the eafora_test pool; transaction-per-test scaffolding
        └── world_bank_wdi.rs           # WB-WDI-specific test helpers (sample loading, assertions)
```

**Structure Decision**: Single-project layout (Cargo workspace with `ingestion/` as one of multiple workspace members; `core/` and the per-platform clients land in later features). The `ingestion/` member is a CLI binary, not a library. The lobby/-triplet pattern within `src/world_bank_wdi/` follows the convention from `docs/architecture/ingestion.md` §Workspace placement: every per-source feature module is exactly three files (`<name>_api.rs`, `<name>_db.rs`, `<name>_model.rs`), each a feature concern, sharing nothing with sibling sources except the `AppError` and `IngestReport` types in `error.rs` / `world_bank_wdi/world_bank_wdi_api.rs`.

## Implementation phases

Sequential — each phase's output is depended on by the next:

1. **Schema + reference-data migrations** (`ingestion/db/migrations/`). Two migrations: `create_initial_schema.sql` (all 7 tables with CREATE INDEX for the partial-unique statistic_value index and all COMMENT ON COLUMN statements) and `seed_initial_data.sql` (UN M49 hierarchy + ISO 3166 country extension rows + the `tfr` statistic + the `wb_wdi` data_source). Validated via `./dbmate.sh up && ./dbmate.sh status` against a clean local DB.
2. **AppError + db bootstrap + main.rs CLI scaffolding** (`src/error.rs`, `src/db.rs`, `src/main.rs`). `db::create_pool()` reads `DATABASE_URL` and returns a configured `PgPool` (`max_connections`, timeouts as needed). Build the clap subcommand tree, dispatch shells for `source <code>` / `all` / `seed` / `build` / `publish` (the latter two stubbed since they're separate features), the all loop with per-adapter error isolation. No business logic yet; commands print "not yet implemented" except where they delegate.
3. **WB WDI types** (`src/world_bank_wdi/world_bank_wdi_model.rs`). Define the WB API response types (`WdiResponse`, `WdiPagingMetadata`, `WdiRow`), `ParsedRow`, `NormalizedRow`. Pure type definitions, no logic. Authored alongside the parse_response tests.
4. **WB WDI helper implementations** in TDD order per Constitution Principle VII:
   - `parse_response`: tests first (covering happy-path response, NA values, malformed shapes), then implementation.
   - `normalize`: tests first (covering known-country resolution, unknown-country warnings, period_start/period_end calculation, data_status assignment), then implementation. Reads `region`, `country`, `statistic` for FK resolution.
   - `read_latest_publication_revision`: tests first (covering empty store → `None`, populated store → max revision), then implementation. One sqlx query.
   - `upsert_rows`: tests first (covering insert-new, skip-unchanged, supersede+insert-revised, all-error-isolation behavior), then implementation. Drives both `data_source_publication` and `statistic_value`.
   - `fetch_upstream`: lighter testing (this is the I/O surface; we test the URL/header construction with a fixture-replay approach, but real HTTP timing tests are deferred). Reqwest call to the WB API with the pinned series code.
   - `fetch_and_store` orchestrator: integration test that wires all five helpers against a test DB and the happy-path sample, asserts on the resulting `statistic_value` rows + the IngestReport.
5. **Wire into main.rs**. Add `source wb_wdi` dispatch to `world_bank_wdi::fetch_and_store`. Add wb_wdi to the all loop. Verify by running both manually against a local Postgres seeded with sample data.
6. **Sample files**. Author the three sample JSON files described in §Project Structure. Verify each one drives the appropriate path through the parser + normalizer + upsert (happy → all rows captured; NA → warning + skip; unknown country → warning + skip).
7. **Integration tests** that exercise the full pipeline against the test DB. Per spec SC-005, ≥90% line coverage on `parse_response` and `normalize`.

## Test harness design

Per the spec's Functional Requirements + Success Criteria, this feature builds the test infrastructure that future adapters will reuse. The harness components are minimal and deliberately tied to real test needs (no speculative scaffolding):

- **Test database**: a separate `eafora_test` PostgreSQL database on the same host as `eafora`. `setup-test-db.sh` (a new script in `scripts/`) drops the database if it exists, recreates it, applies migrations via dbmate, and exits. Run before the first integration test of a session; safe to re-run.
- **Pool acquisition**: `tests/helpers/test_db.rs` exposes `pub async fn test_pool() -> PgPool` returning a connection pool against `eafora_test`. The pool is shared across all tests in a single `cargo test` invocation (one global LazyLock-style cache).
- **Per-test isolation**: each integration test acquires a connection, opens a transaction at the start, performs all DB operations within it, and rolls back at the end. The `tests/helpers/test_db.rs` module exposes `pub async fn with_rollback<F, Fut>(f: F)` that scaffolds this pattern.
- **Sample loader**: `tests/helpers/world_bank_wdi.rs` exposes `load_sample(name)` which reads `ingestion/samples/wb_wdi/*.json` files and deserializes them into the `WdiResponse` type used by the parser. Tests pass these to `parse_response` directly (skipping the HTTP fetch), then assert on the resulting `Vec<ParsedRow>`.
- **Assertion helpers**: `assert_ingest_report_eq(actual, expected)` for comparing IngestReport instances ignoring volatile fields (timestamps); `assert_value_at(pool, region_code, statistic_code, period_start, expected) -> ()` for spot-checking specific cells.

When the second adapter (e.g. Eurostat) lands, it'll reuse the test_db helper verbatim and add its own `tests/helpers/eurostat.rs` for source-specific helpers alongside its own `eurostat_integration.rs` test file.

## Local development

Developer setup, in order:

1. **Postgres**: `./setup.sh` (per `docs/architecture/ingestion.md` §Postgres hosting) installs Postgres via Homebrew and launches it via launchd; creates the `eafora` database. If Postgres is already running on 5432, the script errors out — see the architecture doc for the override path.
2. **Migrations**: `./dbmate.sh up` applies schema + seed migrations to `eafora` and regenerates `ingestion/db/schema.sql`.
3. **Test database**: `./scripts/setup-test-db.sh` creates `eafora_test` and applies the same migrations.
4. **Build**: `cargo build -p ingestion`.
5. **Run a manual ingestion**: `cargo run -p ingestion -- source wb_wdi`. Reads `DATABASE_URL` from `.env` (which `setup.sh` generated), hits the WB API, populates `statistic_value`. Logs the IngestReport.
6. **Run tests**: `cargo test -p ingestion`. Unit tests are pure-function; integration tests use the test DB.
7. **Run all adapters** (currently just wb_wdi): `cargo run -p ingestion -- all`.

## Complexity Tracking

> Fill ONLY if Constitution Check has violations that must be justified.

(No Constitution violations; this section intentionally empty.)
