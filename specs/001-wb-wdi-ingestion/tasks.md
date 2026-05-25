# Tasks: World Bank WDI ingestion CLI

**Feature**: 001-wb-wdi-ingestion | **Branch**: `003-wb-wdi-tasks` | **Date**: 2026-05-24

**Input**: [spec.md](spec.md), [plan.md](plan.md)

This feature is a backend ingestion CLI; the spec's three "user stories" (P1 scheduled, P2 manual, P3 provenance) all share the underlying adapter implementation, so the tasks below are organized by dependency phase rather than per user story. The `[US#]` tags map tasks to the user stories they ultimately deliver, for traceability.

---

## Phase 1: Setup

- [ ] T001 [P] [Setup] Initialize Cargo workspace at the repo root: `Cargo.toml` with `[workspace]` block and `members = ["ingestion"]`. Add `rustfmt.toml` (`max_width = 120`, `chain_width = 100`, `edition = "2024"`) per Constitution IV.
- [ ] T002 [Setup] Create `ingestion/Cargo.toml` with deps per plan §Technical Context: tokio, sqlx, reqwest, serde, serde_json, chrono, uuid, clap, log, env_logger, dotenvy, secr, minimer.
- [ ] T003 [P] [Setup] Add `scripts/setup-test-db.sh` per plan §Test harness design — drops + recreates `eafora_test`, applies migrations via dbmate.

## Phase 2: Foundational

Blocks all subsequent phases.

### Schema and reference data

- [ ] T004 [Foundational] Write `ingestion/db/migrations/<timestamp>_create_initial_schema.sql` — all seven tables (region, country, statistic, data_source, data_source_publication, statistic_value, artifact_version) with: inline NOT NULL / DEFAULT / FK / UNIQUE constraints; the partial unique index `statistic_value_current_per_source` for the `superseded is null` invariant; all `comment on column ...` statements per the architecture doc.
- [ ] T005 [Foundational] Write `ingestion/db/migrations/<timestamp>_seed_initial_data.sql` — UN M49 hierarchy (5 regions, 17 subregions, 7 intermediate regions); ISO 3166 country-level region rows + their country extension rows (approximately 200 entries); the `tfr` row in statistic; the `wb_wdi` row in data_source.
- [ ] T006 [Foundational] Apply migrations to `eafora`: `./dbmate.sh up && ./dbmate.sh dump`. Verify `ingestion/db/schema.sql` regenerates and reflects all 7 tables + indexes + comments.
- [ ] T007 [P] [Foundational] Apply same migrations to `eafora_test` via `./scripts/setup-test-db.sh`.

### Application scaffolding

- [ ] T008 [Foundational] Write `ingestion/src/error.rs` — `pub type AppError = minimer::Error` (or equivalent wrapper); `impl From<String> for AppError`; `impl From<reqwest::Error> for AppError` etc. as needed for ergonomic format-string error construction at failure sites per `feedback_error_strings_not_enums.md`.
- [ ] T009 [Foundational] Write `ingestion/src/db.rs` — `pub async fn create_pool() -> Result<PgPool, AppError>` reading `DATABASE_URL` from env via `dotenvy`, configuring `PgPoolOptions` (max_connections, timeouts), returning the pool.
- [ ] T010 [Foundational] Write `ingestion/src/main.rs` clap-builder CLI scaffolding: subcommand tree per plan §CLI structure (source, all, build, seed, publish); each dispatch helper is a stub returning `Err(AppError::from("not yet implemented"))` except the ones we wire in Phase 4. The `all` subcommand iterates over a hand-written list of registered adapters (just `wb_wdi` for now); each adapter's `AppError` is caught + logged, the loop continues.
- [ ] T011 [Foundational] Write `ingestion/src/lib.rs` — re-exports for tests (`pub mod world_bank_wdi`, `pub mod canonical`, `pub mod error`, `pub mod db`).

### Test harness

- [ ] T012 [P] [Foundational] Write `ingestion/tests/helpers/mod.rs` and `ingestion/tests/helpers/test_db.rs` — `pub async fn test_pool() -> &'static PgPool` returning a `LazyLock`-cached pool against `eafora_test`; `pub async fn with_rollback<F, Fut>(pool, f)` that wraps a closure in a transaction rolled back at teardown.
- [ ] T013 [P] [Foundational] Write `ingestion/tests/helpers/sample_loader.rs` — `pub fn load_wb_wdi_sample(name: &str) -> WdiResponse` reading `ingestion/samples/wb_wdi/<name>.json` and deserializing to the parser's input type.

### Shared canonical-store models and queries

- [ ] T014 [P] [Foundational] Write `ingestion/src/canonical/canonical_model.rs` — Rust types mirroring `region`, `country`, `statistic`, `data_source` rows.
- [ ] T015 [P] [Foundational] Write `ingestion/src/canonical/canonical_db.rs` — `find_country_by_iso3(pool, iso3) -> Option<Country>`, `find_statistic_by_code(pool, code) -> Option<Statistic>`, `find_data_source_by_code(pool, code) -> Option<DataSource>`. Used by every adapter's normalize step.
- [ ] T016 [P] [Foundational] Write `ingestion/src/canonical/canonical_api.rs` — placeholder CLI handlers for canonical-store inspection (sketch only; not wired in this feature).

### WB WDI types

- [ ] T017 [P] [Foundational] Write `ingestion/src/world_bank_wdi/world_bank_wdi_model.rs` — `WdiResponse` (paging metadata + rows tuple), `WdiPagingMetadata`, `WdiRow`, `ParsedRow` (intermediate after parse_response), `NormalizedRow` (intermediate after normalize). Pure type definitions.

### Sample files

- [ ] T018 [P] [Foundational] Author `ingestion/samples/wb_wdi/happy_path.json` — full WB WDI TFR response, approx. 200 countries × approx. 65 years.
- [ ] T019 [P] [Foundational] Author `ingestion/samples/wb_wdi/na_value.json` — subset including a (USA, 2024, value=null) row to exercise the NA-value warning path.
- [ ] T020 [P] [Foundational] Author `ingestion/samples/wb_wdi/unknown_country.json` — subset including a country code we don't have in our country extension table (`XKX` for Kosovo, or `YUG`) to exercise the unknown-code warning path.

**Checkpoint**: After Phase 2, the workspace builds (`cargo build -p ingestion`), the schema is applied to both DBs, and the canonical-store helpers compile. No business logic yet.

---

## Phase 3: WB WDI adapter helpers (TDD per Constitution VII)

All tasks in this phase contribute to **[US1, US2, US3]** — the adapter is the shared dependency.

### `parse_response` (pure function; FR-003, FR-013)

- [ ] T021 [Tests-first] Write unit tests for `parse_response` in `ingestion/src/world_bank_wdi/world_bank_wdi_api.rs`'s `#[cfg(test)] mod tests` block: happy-path response → expected `Vec<ParsedRow>` length and content; null `value` → row preserved with `value: None`; malformed shape (missing rows array) → `AppError`.
- [ ] T022 Implement `parse_response(raw: WdiResponse) -> Result<Vec<ParsedRow>, AppError>` to make T021 pass.

### `normalize` (DB-reading transform; FR-004, FR-008, FR-013)

- [ ] T023 [Tests-first] Write tests for `normalize` against the test DB: known country resolves to `region_id`; unknown country produces an `IngestWarning` with `region_code: None` and the raw code in the message; `period_start` / `period_end` computed as full calendar year `[YYYY-01-01, YYYY+1-01-01)`; `data_status` set to `'final'` for WB WDI's published values.
- [ ] T024 Implement `normalize(pool, parsed_rows) -> Result<(Vec<NormalizedRow>, Vec<IngestWarning>), AppError>` to make T023 pass. Reads `country` and `statistic` via `canonical_db` helpers.

### `read_latest_publication_revision` (DB read; FR-014 indirectly)

- [ ] T025 [Tests-first] Write tests: empty `data_source_publication` table → returns `None`; one publication exists → returns its `revision_label`; multiple publications → returns the one with the latest `fetched`.
- [ ] T026 Implement `read_latest_publication_revision(pool, data_source_id) -> Result<Option<String>, AppError>` to make T025 pass. Single sqlx query.

### `upsert_rows` (DB write; FR-005, FR-006, FR-007)

- [ ] T027 [Tests-first] Write tests in `world_bank_wdi_db` covering: (a) insert publication on first run; (b) ON CONFLICT DO NOTHING on re-fetch of same revision_label; (c) insert new statistic_value row when no current row exists for the natural key; (d) skip writes when current row matches new value + status; (e) **supersede + insert when current row's value differs** — the P3 invariant; (f) IngestReport counts (`values_added`, `values_revised`, `values_skipped`) sum to total rows considered.
- [ ] T028 Implement `world_bank_wdi_db.rs` SQL queries: `insert_publication_or_match(pool, ...) -> Uuid`; `find_current_value(pool, region_id, statistic_id, period_start, period_end, data_source_id) -> Option<StatisticValue>`; `insert_statistic_value(pool, ...) -> ()`; `set_superseded(pool, statistic_value_id, instant) -> ()`. Use `sqlx::query_as!` per Constitution V.
- [ ] T029 Implement `upsert_rows(pool, normalized_rows) -> Result<IngestReport, AppError>` orchestrating the four query helpers per the architecture doc's `upsert_rows` contract.

### `fetch_upstream` (HTTP I/O; FR-002)

- [ ] T030 Implement `fetch_upstream(options) -> Result<WdiResponse, AppError>` — reqwest GET against `https://api.worldbank.org/v2/country/all/indicator/SP.DYN.TFRT.IN?format=json&per_page=20000`; deserialize to `WdiResponse`; map errors to `AppError::from(format!("wb_wdi: ..."))`. Tested via integration tests against fixture-replay; live HTTP not exercised in unit tests.

### `fetch_and_store` orchestrator (FR-001)

- [ ] T031 Implement `fetch_and_store(pool, options) -> Result<IngestReport, AppError>` — calls the five helpers in order per the architecture doc's adapter contract code listing.

### Integration tests

- [ ] T032 Write `ingestion/tests/world_bank_wdi_integration.rs` exercising the full pipeline against `eafora_test` with `happy_path.json` sample: assert resulting `statistic_value` row count, assert `data_source_publication` has one new row, assert IngestReport's `values_added` matches.
- [ ] T033 Add integration scenarios using `na_value.json` (assert one IngestWarning, one fewer row inserted than expected) and `unknown_country.json` (assert one IngestWarning identifying the raw code).

**Checkpoint**: After Phase 3, the WB WDI adapter is implemented end-to-end and tested against `eafora_test`. The CLI subcommands still error with "not yet implemented" — Phase 4 wires them.

---

## Phase 4: Operational wiring

### Manual run path **[US2]**

- [ ] T034 [US2] Replace the `source wb_wdi` stub in `main.rs` with a dispatch that calls `world_bank_wdi::fetch_and_store(&pool, AdapterOptions { force_full_refetch })`. Log the IngestReport via `log::info!`.
- [ ] T035 [US2] Manual verification: `cargo run -p ingestion -- source wb_wdi` against a freshly-migrated `eafora` populates approximately 13,000 rows in `statistic_value`. Re-run shows zero writes.

### Scheduled run path **[US1]**

- [ ] T036 [US1] Replace the `all` stub in `main.rs` with the orchestration loop per plan §Implementation phases step 5: iterate `[wb_wdi]` (just one adapter for now); call each adapter's `fetch_and_store`; catch and log any `AppError`; continue to the next adapter on failure (per architecture doc §Scheduling error-isolation rule); after all adapters, log an aggregate report with per-adapter outcomes.
- [ ] T037 [P] [US1] Author `scripts/eafora-ingestion.plist.template` — launchd plist matching the architecture doc's `StartCalendarInterval` block (Mondays 03:00 local) with `ProgramArguments` invoking the ingestion binary's `all` subcommand.
- [ ] T038 [US1] Update `setup.sh` to render `eafora-ingestion.plist.template` into `~/Library/LaunchAgents/org.eafora.ingestion.plist` and load via `launchctl bootstrap gui/$(id -u)`. Per Constitution Principle VIII (workflow discipline), commit setup.sh changes alongside the plist template so the install is reproducible.

**Checkpoint**: After Phase 4, both manual and scheduled paths work end-to-end. **[US1]** + **[US2]** acceptance scenarios pass; **[US3]** is satisfied transitively because `upsert_rows` includes the supersede logic (validated by T027/T028/T029 + T032/T033).

---

## Phase 5: Polish

- [ ] T039 [P] [Polish] Measure line coverage on `parse_response` and `normalize` (`cargo llvm-cov` or equivalent); add tests until ≥90% per spec SC-005.
- [ ] T040 [P] [Polish] Manual run against the live WB WDI API (one shot, on dev machine) — verify the IngestReport's totals match plausibility (approximately 13,000 rows, near-zero warnings). Validates spec SC-001 and SC-006 (sub-5-second runtime).
- [ ] T041 [Polish] If implementation surfaced any divergence from `docs/architecture/ingestion.md`'s adapter contract, propose an architecture amendment in a follow-up PR (per Constitution: amend the architecture, don't deviate silently).
- [ ] T042 [Polish] Run `./scripts/cleanup-merged.sh` once this branch + the spec/plan branches are merged into master.

---

## Dependencies & execution order

### Phase dependencies

- **Phase 1 (Setup)**: no dependencies.
- **Phase 2 (Foundational)**: depends on Phase 1. **Blocks all of Phase 3+**.
- **Phase 3 (Adapter helpers, TDD)**: depends on Phase 2. Internal ordering: parse_response → normalize → read_latest_publication_revision → upsert_rows → fetch_upstream → fetch_and_store → integration tests. Within each helper, **tests-first** (Constitution VII).
- **Phase 4 (Operational wiring)**: depends on Phase 3 completion (specifically `fetch_and_store` working end-to-end against test DB). T034/T035 (manual path) and T036–T038 (scheduled path) are independent — can be done in either order.
- **Phase 5 (Polish)**: depends on Phase 4.

### Within each phase

- Tasks marked **[P]** can run in parallel — different files, no shared state.
- All other tasks within a phase are sequential.
- TDD tasks (T021/T023/T025/T027) MUST FAIL before their corresponding implementation task (T022/T024/T026/T028).

---

## Parallel example: Phase 2

```text
# These T-numbers all touch different files; can be drafted in any order:
T011 src/lib.rs
T012 tests/helpers/test_db.rs
T013 tests/helpers/sample_loader.rs
T014 src/canonical/canonical_model.rs
T015 src/canonical/canonical_db.rs
T016 src/canonical/canonical_api.rs
T017 src/world_bank_wdi/world_bank_wdi_model.rs
T018 samples/wb_wdi/happy_path.json
T019 samples/wb_wdi/na_value.json
T020 samples/wb_wdi/unknown_country.json
```

T004 (schema migration) blocks T006 (apply); T005 (seed migration) blocks T007 (apply test DB). T008/T009/T010 (error/db/main scaffolding) are mostly independent files but main.rs imports from error and db, so order: error → db → main.

---

## Implementation strategy

**MVP** is the full feature — there's no "MVP slice" smaller than "WB WDI capture works" because the entire feature serves a single capture flow. The phasing above is driven by dependency, not by carving out a smaller deliverable.

Per Constitution VIII, the work flows in branches/PRs (each phase or grouping ships as its own PR if substantial; Phase 2 might be one PR, Phase 3 one PR per helper, Phase 4 one PR for wiring, Phase 5 one PR or rolled into a prior). Per the saved memory `feedback_branch_per_body_of_work.md`, branches form a linear stack when serial.

---

## Notes

- `[P]` = different files, no dependencies — can run in parallel.
- `[US#]` = traceability tag mapping the task to the user story (or stories) it ultimately delivers; spec.md's three sections are P1/P2/P3.
- `[Setup]` / `[Foundational]` / `[Polish]` = phase tags for tasks that don't map to a single user story.
- Verify tests fail before implementing them.
- Commit after each task or logical group per `feedback_commit_push_cadence.md`.
- Architecture amendments (if any surface during implementation) ship as separate PRs per Constitution §Amendment procedure.
