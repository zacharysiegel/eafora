# Feature Specification: World Bank WDI ingestion CLI

**Feature Branch**: `001-wb-wdi-ingestion`

**Created**: 2026-05-24

**Status**: Draft

**Input**: User description: "World Bank WDI ingestion CLI — first concrete adapter implementing the fetch_and_store contract from docs/architecture/ingestion.md; fetches TFR from World Bank, normalizes to canonical schema, upserts statistic_value rows"

## User Scenarios & Testing *(mandatory)*

### Scheduled weekly capture (P1)

The launchd-managed weekly schedule on the Mac mini invokes `ingestion all`, which dispatches the WB WDI adapter as one of its registered sources. The adapter fetches the latest TFR (Total Fertility Rate) values for every country WB WDI covers, identifies which are new vs. revised vs. unchanged relative to the canonical store, and persists changes. An IngestReport summarises the run for the operator. This is the entire reason the feature exists — without it, the canonical store goes stale and downstream artifacts freeze at v1's first manual seed.

**Acceptance Scenarios**:

1. **Given** the canonical store has WB WDI TFR data captured from publication `2024-Q4`, **When** WB releases publication `2025-Q1` and the next weekly schedule fires, **Then** new cells (those WB now publishes that we hadn't captured) are INSERTed; revised cells (different `value` from the `2024-Q4` capture) get a new row + the previous row's `superseded` set; a row for `2025-Q1` is inserted into `data_source_publication`.
2. **Given** the canonical store is empty (first-ever run), **When** the WB WDI adapter runs, **Then** every country/year datum WB publishes for TFR lands in `statistic_value` (~200 countries × ~65 years ≈ 13,000 rows) and one row is created in `data_source_publication`.
3. **Given** WB WDI has not published a new revision since the last run, **When** the adapter runs, **Then** zero rows are inserted/updated in `statistic_value`, zero rows are inserted in `data_source_publication`, and the IngestReport's `values_skipped` equals the row count.

---

### Manual run for development and operational debugging (P2)

A developer or operator runs `ingestion source wb_wdi` from a shell to capture WB WDI data outside the scheduled cycle. The behaviour is identical to the scheduled path; the IngestReport prints to stdout/stderr for direct review. Used during dev iteration (you can't iterate on an adapter you can only run weekly via launchd) and for operational catch-up after a failed scheduled run.

**Acceptance Scenarios**:

1. **Given** a developer machine with the canonical schema applied and the `data_source` row for wb_wdi seeded, **When** the developer runs `cargo run -p ingestion -- source wb_wdi`, **Then** the WB WDI API is fetched, rows are upserted, and the IngestReport prints to the terminal.
2. **Given** a previously-captured publication is the latest WB has, **When** the developer re-runs the same command, **Then** the IngestReport shows `values_skipped` only and no DB writes occur.
3. **Given** the `--force-full-refetch` flag is passed, **When** the adapter runs, **Then** it re-fetches without consulting `read_latest_publication_revision` and re-evaluates every row against the natural key.

---

### Provenance preserved across revisions (P3)

When WB WDI revises a previously-published value (the same `(country, statistic, period)` cell now reports a different number in a later publication), the adapter INSERTs a new `statistic_value` row pointing at the new publication, and sets the previous row's `superseded` timestamp. Both rows stay in `statistic_value` permanently — the new row is the current view (`superseded is null`); the old row is the audit trail. Constitution Principle II requires per-cell source provenance with retrieval timestamp and license; the append-with-supersede model keeps the full revision history of every value in the canonical store, not just publication metadata.

**Acceptance Scenarios**:

1. **Given** `statistic_value` has Germany TFR 2023 = 1.46 captured under publication `2024-Q4` (one row, `superseded is null`), **When** the adapter ingests publication `2025-Q1` in which Germany TFR 2023 = 1.44, **Then** the original row's `superseded` is set to the run's start time, and a NEW row exists with `value = 1.44` pointing at `2025-Q1` and `superseded is null`.
2. **Given** an artifact builder runs after such a revision, **When** the manifest is generated, **Then** the build reads from `superseded is null` rows only (so Germany TFR 2023 = 1.44), and `data_source_versions_jsonb` records `{"wb_wdi": "2025-Q1", ...}`.
3. **Given** a row was captured under publication `2024-Q4` and a newer revision came in under `2025-Q1`, **When** an operator queries `select value, data_source_publication_id from statistic_value where region_id = $1 and statistic_id = $2 and period_start = $3 order by created`, **Then** both rows are returned in publication order — the audit trail is queryable directly from the canonical store without consulting external snapshots.

---

### Edge Cases

- **Source returns `null`/`"NA"` for a country/year cell** — adapter logs an `IngestWarning` identifying the cell, skips that datum, continues with the rest of the response.
- **Source returns a country code we don't have in our `country` extension table** (e.g. Kosovo's `XKX`, historical codes like `YUG`) — adapter logs an `IngestWarning` identifying the raw code, skips the row, continues.
- **HTTP failure** — DNS resolution failure, TCP timeout, TLS error, or 5xx response: adapter returns `AppError`; no partial DB writes; canonical store stays consistent. The next scheduled run retries the full fetch.
- **Schema drift in WB WDI's response** — JSON shape doesn't match the parser's expectations (renamed field, removed metadata block): adapter returns `AppError` with a descriptive message identifying the path that failed to parse; canonical store stays consistent.
- **Same revision label, no upstream change** — `read_latest_publication_revision` finds the publication already exists; `fetch_upstream` is invoked anyway (WB WDI doesn't expose a cheap-poll endpoint); the response matches what's already stored; `upsert_rows` produces zero writes; `IngestReport.values_skipped` equals the row count.
- **Revision label format change** — WB changes how it labels publications (e.g., from `2024-Q4` to `2024-12`): the adapter's revision-label extraction logic needs updating; until then, a synthetic label (response payload hash) is used as a fallback so ingestion doesn't break.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST implement the `fetch_and_store(pool, options) -> Result<IngestReport, AppError>` adapter contract for the World Bank WDI source, exactly as specified in `docs/architecture/ingestion.md` §Adapter contract, including all five named helpers (`read_latest_publication_revision`, `fetch_upstream`, `parse_response`, `normalize`, `upsert_rows`).
- **FR-002**: System MUST fetch TFR data from WB WDI's API, specifically the `SP.DYN.TFRT.IN` series, in JSON format. The endpoint URL is the documented WB WDI API path and includes a `per_page` parameter sized to retrieve all rows in a single request.
- **FR-003**: System MUST parse the WB WDI JSON response (a paging-metadata-plus-rows structure) into intermediate types defined in `world_bank_wdi_model.rs`. Parsing MUST be a pure function (no I/O, no DB access).
- **FR-004**: For each parsed row, system MUST resolve the country via ISO 3166 alpha-3 lookup against the `country` extension table (joined to `region` for the `region_id`), resolve the statistic ID via `statistic.code = 'tfr'`, and compute `period_start` / `period_end` as full calendar year `[YYYY-01-01, YYYY+1-01-01)`.
- **FR-005**: System MUST INSERT a `data_source_publication` row for the WB WDI publication captured by this run, keyed on `(data_source_id, revision_label)` with `ON CONFLICT DO NOTHING`. The publication's `revision_label` MUST be derived from the WB API response (specifically the `lastupdated` field where present, or a synthesized label otherwise — see Assumptions).
- **FR-006**: For each normalized row, system MUST look up the current (`superseded is null`) `statistic_value` row matching `(region_id, statistic_id, period_start, period_end, data_source_id)`. If none exists → INSERT the new row pointing at the publication. If a current row exists with the same `value` and `data_status` → no writes. If a current row exists with different `value` or `data_status` → UPDATE its `superseded` to the run's start time, then INSERT a new row pointing at the new publication. The new row is the current view; the old row is permanent audit trail.
- **FR-007**: System MUST return an `IngestReport` containing `values_added` (new cells, no prior row), `values_revised` (cells where supersede + insert pair fired), and `values_skipped` (cells where the existing current row already matched) counts plus `upstream_revision` (the publication's revision label), `started` / `finished` timestamps, and any `IngestWarning` instances accumulated during the run.
- **FR-008**: System MUST surface non-fatal data quirks (NA values, unknown country codes, null TFR for a known country, etc.) as `IngestWarning` instances appended to the report — not as `AppError` returns. The run continues past each warning.
- **FR-009**: System MUST register WB WDI in `data_source` via a seed migration with `code='wb_wdi'`, `name_en='World Bank World Development Indicators'`, `homepage_url='https://datatopics.worldbank.org/world-development-indicators/'`, `license_class='attribution'`, `license_name='CC BY 4.0'`, `attribution_text='The World Bank: World Development Indicators'`, and `preference_rank=90`.
- **FR-010**: System MUST register the `tfr` statistic in the `statistic` table via a seed migration with `code='tfr'`, `name_en='Total fertility rate'`, `description` referencing the standard demographic definition, and `units='births per woman'`.
- **FR-011**: System MUST wire the WB WDI adapter into `main.rs`'s `source` subcommand dispatch (so `ingestion source wb_wdi` calls `world_bank_wdi::fetch_and_store(...)`) and into the `all` orchestration loop (so the weekly launchd trigger includes WB WDI).
- **FR-012**: System MUST provide checked-in sample WB WDI API responses under `ingestion/samples/wb_wdi/` covering at minimum: a happy-path response, an NA-value case, and an unknown-country-code case. Sample responses MUST be replayable by `seed` and by integration tests without live HTTP.
- **FR-013**: System MUST cover `parse_response` and `normalize` with TDD unit tests per Constitution Principle VII. Tests MUST be authored before implementation; the Red-Green-Refactor cycle MUST be respected.
- **FR-014**: System MUST treat WB WDI's JSON response as the source of truth for the run's revision label; if no native revision label is exposed, system MUST synthesize one (response payload SHA-256 truncated to 8-12 hex chars) so that the publication-table invariant — every captured publication has a label — is maintained.

### Key Entities

- **WB WDI TFR datum**: a single (country, year, value) tuple from the WB WDI API response. Each maps to one row in `statistic_value` (joined to `data_source_publication` for revision metadata).
- **WB WDI publication**: one batch release from WB WDI, identified by a revision label such as `'2024-Q4'`. Each maps to one row in `data_source_publication`. A typical run captures exactly one publication.
- **WB WDI source registration**: a single row in `data_source` with `code='wb_wdi'`. Created once via seed migration; referenced by every `statistic_value` row attributed to WB WDI.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After a fresh ingestion run on an empty canonical store, the `tfr` statistic has approximately 13,000 `statistic_value` rows (≈200 countries × ≈65 years), and one `data_source_publication` row exists for the WB WDI source.
- **SC-002**: A scheduled weekly ingestion run captures any new or revised WB WDI TFR values within one launchd cycle (≤ 7 days) of WB's publication of those values.
- **SC-003**: A re-run with no upstream change produces zero DB writes; the IngestReport shows `values_added = 0`, `values_revised = 0`, `values_skipped = N` where N is the row count for WB WDI in `statistic_value` (filtered to `superseded is null`).
- **SC-004**: For any artifact build that occurs after a successful WB WDI run, the manifest's `data_source_versions_jsonb` includes `"wb_wdi": "<the captured revision label>"`, and an operator can answer "which WB publication did this canonical row come from?" via a single join from `statistic_value` to `data_source_publication`.
- **SC-005**: The `parse_response` and `normalize` helpers achieve at least 90% line coverage in the test suite, exercising the happy-path response, every documented edge case (NA value, unknown country code, schema-drift detection), and the upsert idempotence property.
- **SC-006**: A full ingestion run (fetch + parse + normalize + upsert) completes in under 5 seconds on the Mac mini's network and database, with no manual intervention.

## Assumptions

- The architecture in `docs/architecture/ingestion.md` is locked. The canonical schema (`region`, `country`, `statistic`, `data_source`, `data_source_publication`, `statistic_value`, `artifact_version`) is applied via dbmate before this feature is exercised.
- WB WDI's API at `api.worldbank.org/v2/` remains publicly accessible without authentication. The TFR series code remains `SP.DYN.TFRT.IN`. The JSON response shape (paging metadata header + rows array) is stable enough that a single parser handles it; if it changes, schema drift is treated as an `AppError`.
- WB WDI's TFR data covers calendar-year periods. Each datum's period is a full calendar year (`YYYY-01-01` to `YYYY+1-01-01`); no quarterly or sub-annual TFR data is published. (This is documented for WB WDI as of the 2026-05-21 data-sources research; if WB later adds finer granularity, the adapter would need updating.)
- The CC BY 4.0 license terms documented in `docs/research/data-source-licensing.md` Part 1 remain stable; the attribution text used in the seed migration matches WB's published terms.
- The WB API's `lastupdated` field — present per the 2026-05-21 research — is what the adapter parses for the publication's `revision_label`. If `lastupdated` is missing or unstable, the adapter falls back to a synthetic label (response-payload SHA-256, truncated). The fallback is the third item under FR-014.
- The seed migrations registering `wb_wdi` and `tfr` are part of this feature's deliverable; they're not assumed to exist beforehand.
- This feature does NOT include the artifact-build path or R2 upload — those land separately via `build` / `publish` (which are out of scope here). This feature only populates the canonical store.

## Constitution Check

Per Constitution §Compliance review, this spec honors the binding principles as follows:

- **Principle I (Educational neutrality)**: not directly applicable — the feature ingests source data into the canonical store; no UI text or editorial copy is added. The `data_source.attribution_text` will be set to WB's literal attribution string ("The World Bank: World Development Indicators") per WB's terms-of-use; that's factual citation, not editorial.
- **Principle II (Source provenance — NON-NEGOTIABLE)**: directly served. Every WB WDI datum lands in `statistic_value` with a foreign key to a `data_source_publication` row carrying the WB revision label and our retrieval timestamp; the chain `statistic_value` → `data_source_publication` → `data_source` provides full provenance (publisher, license, revision, retrieval) for every cell.
- **Principle III (Rust core, native UI shells)**: applies — the adapter is a Rust module in the `ingestion/` binary. No UI, no FFI surface added.
- **Principle IV (Singularity convention parity)**: applies — uses reqwest, `sqlx::query_as!`, tokio per the locked picks; no new third-party dependencies introduced beyond what the architecture doc already names. The Postgres-via-launchd deviation already recorded in v1.3.3 is the host's responsibility, not this feature's.
- **Principle V (Explicit over implicit)**: applies — the adapter exposes itself as a CLI subcommand handler (no actix-web routes); SQL is hand-written via `sqlx::query_as!`; no ORM, no RPC framework, no `#[derive(Parser)]` macros for the clap config (clap builder API only, per the registered preference).
- **Principle VI (CDN-delivered data, no live API through v2)**: applies indirectly — this feature populates the canonical store that the artifact builder reads from. No client API is exposed; clients still consume only from CDN artifacts.
- **Principle VII (Test-first for core logic)**: directly applicable. `parse_response` and `normalize` are core logic surfaces and MUST follow Red-Green-Refactor. FR-013 codifies this; SC-005 measures it.
- **Principle VIII (Workflow discipline)**: this feature is the first `/speckit-specify` use; the spec lives at `specs/001-wb-wdi-ingestion/spec.md` per the convention.

No principle violations identified; no constitution amendments proposed.

