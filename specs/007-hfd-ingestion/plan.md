# Implementation Plan: Human Fertility Database ingestion, completed cohort fertility

**Branch**: `hfd-ingestion` | **Date**: 2026-08-19 | **Spec**: [spec.md](spec.md)

## Summary

Adds HFD as the second real source and completed cohort fertility as the second statistic. The adapter contract, the publication model, the append-with-supersede revision strategy, and the `IngestReport` shape are all settled by `docs/architecture/ingestion.md` and demonstrated by World Bank WDI, so this plan covers only what is specific to HFD: an authenticated archive download, a fixed-width text parser, a region-code mapping with five deliberate exclusions, the migrations, and the client changes that let a cohort be presented as a range rather than an instant.

Four phases, one PR each, on a linear stack.

- **A, ingestion.** Real rows in `statistic_value` with no client involvement, because a statistic with `released is null` is invisible to the artifact build.
- **B, presentation.** The client learns to render a cohort, and the final migration releases the statistic.
- **C, source priority.** Each published cell resolves to the highest-priority source holding a value for it, replacing the configuration table that picked one source per series.
- **D, period total fertility.** HFD's period TFR, from a file already inside the archive A downloads. Depends on C.

C and D are independent of the cohort work and could land in either order relative to A and B, but D depends on C.

## Already built

`ingestion/src/hfd/hfd_client.rs` is written and verified against the live service: it signs in, downloads `tfr.zip`, and extracts the `tfrVH.txt` member. `ingestion/samples/hfd/tfrVH.txt` is a reduced copy of a real download. `ingestion/tests/hfd_live.rs` is the `#[ignore]`d probe that produced both.

What remains for Phase A is the parser, the normalization, the migrations, and the wiring.

## Corrections to the spec

Five things the spec asserts do not match the code. Code is canonical; the spec is what changes.

- **Source selection becomes per-cell priority ordering, replacing the selection table.** The spec asks for a priority that wins over World Bank WDI for the cells both supply, and describes a merge that picks one source per cell. The implementation does neither: `source_choice` holds a row per `(statistic, license_shard_class)`, optionally overridden per region, naming exactly one source for the whole series; `preference_rank` is recorded on the source and read by nothing. Selection now resolves per `(region, statistic, period)` cell by taking the highest-priority source holding a value for it, so a preferred source's coverage gaps are filled by the next source rather than becoming gaps in the published series. `source_choice` is deleted: the table, its entity, its query, and its resolver. Adding a source needs no selection config at all.
- **The helper is `read_latest_publication`, returning `Option<SourceRevision>`.** FR-001f and the architecture doc both name `read_latest_publication_revision` returning `Option<String>`. The doc predates the code. Use the real name; fix the doc separately rather than in this feature.
- **`NaiveDatePeriod::from_year` already produces FR-007's encoding.** It yields 1 January of the year to 1 January of the following year. No new constructor is needed for a single-year cohort, and none should be added; a multi-year cohort constructor arrives with the first source that has one.
- **A `statistic` insert must set `name_abbreviated_en`.** The column is `not null` and was added after the initial seed, so the `tfr` insert in the seed migration does not name it and is not a usable template.
- **`fetch_upstream` cannot honour `options.force_full_refetch`.** The revision label is the last-modification date printed inside the file, so it is unknowable until after the archive is downloaded and extracted. The conditional skip therefore lives in the adapter, after `fetch_upstream` returns, and `hfd_client::fetch_upstream()` takes no options. This is a real divergence from the shape the architecture doc asks every adapter to mirror, and it is the honest one: FR-001f's "nothing is downloaded past the point that becomes knowable" already concedes that one download is unavoidable.

## Module layout

```text
ingestion/src/hfd/
├── mod.rs                 # declarations and re-exports only
├── hfd_client.rs          # built: login, download, extract. Knows the wire format, not the canonical store
├── hfd_model.rs           # new: ParsedHfdPublication, ParsedHfdStatisticValue, the parser
└── hfd_adapter.rs         # new: normalize + fetch_and_store orchestrator
```

The parser is the one placement question worth stating. `parse_cohort_file` is pure text-to-intermediate-types with no knowledge of the canonical store, which puts it on the client's side of the split, but `hfd_client.rs` is already the HTTP and archive surface and the parser is the larger half of the file. It goes in `hfd_model.rs`, beside the types it produces, and `hfd_client.rs` keeps only the transport. That matches how `world_bank_wdi_model.rs` holds the response types while `parse_response` sits in the client, inverted, and the inversion is deliberate: WDI's parse is a serde deserialize of a few lines, and this one is a hundred.

## Data model

Four migrations across the four phases, dbmate-timestamped when written. `ingestion/db/schema.sql` is regenerated by dbmate afterward and committed with each.

### Phase A, one: `add_statistic_released`

Adds `released timestamp with time zone` to `statistic`, nullable, with a catalog comment stating that null means the statistic is not offered to clients. Sets `released = now()` for `tfr` so nothing about the shipped product changes (FR-010c).

Named `released` rather than `published` because `data_source_publication.published` already means the upstream's own publication date, and the two would be confusable in a query that joins both.

`artifact_db::read_all_statistic_kinds` gains `where released is not null`. That one predicate is what decouples a seed row from a `StatisticKind` variant: ingestion resolves statistics by `statistic.code` and never touches the enum, so an unreleased statistic can accumulate real rows while no client knows it exists. The `try_from` on the code stays a hard error for a released statistic with no variant (FR-010b) — releasing something no client can draw is a misconfiguration, and a failed artifact build is how it surfaces.

### Phase A, two: `seed_hfd_and_completed_cohort_fertility`

- `data_source`: `code='hfd'`, `license_class='attribution'`, `license_name='CC BY 4.0'`, `preference_rank=50` (below WDI's 100; lower wins), `attribution_text` naming HFD, the Max Planck Institute for Demographic Research, the Vienna Institute of Demography, and `www.humanfertility.org` per FR-008a.
- `statistic`: `code='ccf'`, `name_en='Completed cohort fertility'`, `name_abbreviated_en='CCF'`, units in the same idiom as `tfr`'s "children per woman", `released` left null.

No selection row. Priority ordering resolves `ccf` to HFD automatically, since nothing else supplies it.

`ccf` over `completed_fertility` is settled by FR-009 on the ground that `cfr` would collide with HFD's own `cfr.zip` of cumulative fertility rates. This migration is the last cheap moment to change it; after it lands, the code is in a shard filename and a client enum.

### Phase B: `release_completed_cohort_fertility`

`update statistic set released = now() where code = 'ccf'`. Last change in the phase (FR-020a).

### Phase C: `drop_source_choice`

Drops the `source_choice` table, which nothing reads once priority resolution replaces it. The down migration recreates the table and restores the one seeded row.

Phase D has no migration; it writes to the `tfr` statistic, which is already seeded and released.

## Phase A steps

Test-first throughout, per Constitution Principle VII. Each numbered step is a commit.

1. **`hfd_model.rs` types and parser, tests first.** `ParsedHfdPublication { revision_label, last_modified }` and `ParsedHfdStatisticValue { hfd_code, cohort_year, completed_cohort_fertility: Option<f64> }`. `parse_cohort_file(contents: &str) -> Result<(ParsedHfdPublication, Vec<ParsedHfdStatisticValue>), AppError>` resolves `Code`, `Cohort`, and `CCF` by name from the third line and fails naming the file and the columns found if any is absent (FR-004). `CCF40` is located and ignored (FR-004a). A field of `.` parses to `None` (FR-003). The last-modification date comes off the second line and becomes the revision label (FR-005).
2. **Region mapping.** A const table pairing each national-total code with its ISO 3166-1 alpha-3 (`DEUTNP`→`DEU`, `FRATNP`→`FRA`, `GBR_NP`→`GBR`) as one struct rather than two parallel arrays, and a const list of the five subpopulation codes. A bare three-letter code resolves through `canonical_db::find_country_by_iso3`. The subpopulation list exists for warning quality: without it `DEUTE` reports as an unknown country, which is wrong and would send a reader looking for a missing seed row.
3. **`IngestWarningKind::NoValuesForCountry`** and an `IngestReport` counter for cells absent upstream. FR-012a forbids warning per absent `CCF` — 364 of 1701 rows in the current release — so the count goes on the report and `log_report` prints it. The existing `values_skipped` cannot carry it; that means "unchanged since last run," a different fact.
4. **`normalize`,** mirroring `world_bank_wdi_adapter::normalize`: resolve the statistic by code, walk rows, return `(Vec<NormalizedStatisticValue>, Vec<IngestWarning>)`. `data_status: DataStatus::Final` on every row (FR-011). A mapped country that produced nothing warns once (Chile, in the current release).
5. **`fetch_and_store`.** Open the transaction, resolve `DataSourceKind::Hfd`, `fetch_upstream`, parse, compare the parsed revision label against `read_latest_publication` and return an empty report unless `options.force_full_refetch` (FR-001f), normalize, `ingest::record_statistic_values`, commit.
6. **`DataSourceKind::Hfd`** with code `hfd`, plus `REGISTERED_SOURCES` and the `run_source` match in `main.rs` (FR-015). The match is exhaustive by design, so the compiler names the site.
7. **Integration test** against `eafora_test` in a rolled-back transaction, following `world_bank_wdi_integration.rs`: the sample parses, normalizes, and records; a second run over the same input reports every cell skipped (SC-001); a revised sample supersedes and reinserts, leaving the prior row readable (SC-002); a subpopulation code warns and writes nothing with a zero exit (SC-006).

## Phase B steps

1. **Widen the shard read.** `CellValue` gains `period_end` and `data_status`; the two `read_shard` implementations select the columns they already write and discard (FR-016). Both target-gated submodules change identically, which is the sweep the render-feature test rule is about.
2. **`StatisticKind::Ccf`,** and a distinction on the enum between a period measure and a cohort measure, with the axis label taken from it rather than from a fixed string (FR-017). The per-statistic colour transform is the precedent for varying presentation this way.
3. **The scrubber renders a span.** `web/src/map/controls.rs` works in `i32` years off `period_start` throughout — `earliest_year`, `latest_year`, `active_year`, `thumb_proportion`. FR-018 applies to every statistic, so an annual period draws as a one-year span and the change is uniform rather than conditional on the statistic.
4. **`data_status` in the detail panel** when it is anything but final (FR-019), and the HFD attribution (FR-020). Both are existing i18n surfaces.
5. **The release migration.**

## Phase C steps, per-cell source priority

Independent of HFD, and a prerequisite for Phase D. Rewrites `ingestion/src/artifact/source_choice.rs`, which is deleted and replaced by `ingestion/src/artifact/source_priority.rs`.

1. **Carry the source's priority onto the candidate.** `CandidateValueProjection` and `CandidateValue` gain `data_source_preference_rank`, read from the join the candidate query already makes to `data_source`. Resolution then needs no second query and stays a pure function over the candidates.
2. **Group per cell, not per series.** The key becomes `(region_id, statistic_kind, license_shard_class, period)`. `SeriesKey` is renamed to `CellKey` and gains the period; it stays one struct passed whole rather than four parameters.
3. **Choose by priority.** Lowest `preference_rank` wins among the candidates for that cell, ties broken by `data_source_id` as the column comment already promises, so a rebuild of unchanged data produces an identical shard.
4. **Delete the selection machinery.** `SourceChoice`, `SourceChoiceEntity`, `canonical_db::read_source_choices`, the resolver's two maps, and the "no source_choice configured" error class all go. That error was the only way the artifact build could fail on editorial config, and nothing replaces it: priority is total, so every cell with a candidate resolves.
5. **Tests.** The existing resolver tests encode the rule being replaced and are rewritten, not amended: a cell held by two sources resolves to the higher-priority one; a period only the lower-priority source covers still emits, which is the behaviour the old tests asserted the opposite of; equal ranks resolve deterministically.

## Phase D steps, HFD period total fertility

HFD's `tfrRR.txt` is already inside the archive Phase A downloads, so this phase adds a parse and no new transport. It is worth doing only because Phase C landed: per-cell priority means HFD's 13 countries that reach 2024 or 2025 gain recency while the 26 whose series end earlier keep their World Bank values for the later periods.

1. **Read a second member from the archive.** The member filter becomes a list rather than one suffix, and `read_cohort_members` is renamed for what it now does.
2. **Parse `Code Year TFR TFR40`** with the parser Phase A wrote, which resolves columns by name and needs only the column set as a parameter. `TFR40` is ignored for the same reason `CCF40` is.
3. **Normalize against the existing `tfr` statistic,** which is already seeded and released, so this phase has no migration at all.
4. **Verify the join.** An integration test covering a region where both sources have values and HFD stops earlier: the cells HFD covers name HFD, the later cells name the World Bank, and the series has no gap between them.

The 39 codes in `tfrRR.txt` against `tfrVH.txt`'s 37 need reconciling when this phase starts; the extra two are unidentified.

## Test plan

- Parser and mapping: host unit tests in `hfd_model.rs` and `hfd_adapter.rs` against `ingestion/samples/hfd/tfrVH.txt`, which covers a plain code, a national-total code that maps, two codes that must warn, an absent `CCF`, and a country with no values at all (FR-013).
- Archive extraction: a unit test building a small zip in memory, so `read_cohort_members` is covered without a checked-in binary.
- Login: `read_antiforgery_token` against a captured form fragment, including the two malformed cases.
- Normalization and recording: integration tests against `eafora_test`, each in a transaction rolled back at teardown.
- Live: `ingestion/tests/hfd_live.rs` stays `#[ignore]`d. It is a probe, not a gate.
- SC-008 (a wrong password fails without echoing either credential) is asserted by reading the error's text in a unit test over the failure branch, not by a live attempt with a bad password.
- Phase B's axis, span, and status behaviour is target-agnostic and belongs in host tests plus `cargo check --target wasm32-unknown-unknown`. No browser harness.

## The adapter trait

`docs/architecture/ingestion.md` says the orchestrator becomes a shared trait once a second source proves the pipeline shape stable. This is that second source, and the answer is no: a hand-written match arm per source is the preferred shape, and the doc's paragraph should be amended to say so rather than left as a standing intention.

HFD is also evidence the extraction would have been wrong. Its pipeline diverges at three points: `fetch_upstream` needs no options and cannot use them, the revision comparison happens after the download rather than before, and authentication has no analogue in World Bank WDI. A trait would have encoded one source's ordering and forced the other around it.

## PR description, Phase A

Adds the Human Fertility Database as a data source and completed cohort fertility as a statistic, ingested from HFD's cohort summary file.

The adapter authenticates against HFD, downloads the by-statistic archive once per run, parses the cohort file by column name, and maps HFD's 37 country codes onto 32 canonical regions; the five that name subpopulations rather than countries produce warnings. Adds a nullable `statistic.released` column, which the artifact build now filters on, so a statistic can be ingested and verified before any client knows it exists. `ccf` is seeded unreleased and stays invisible until the client can draw a cohort.

## PR description, Phase B

Presents a cohort as the range it covers rather than as a single instant.

The shard read path carries `period_end` and `data_status` into the client, both already written into every shard and previously discarded. The scrubber draws the active value as a span for every statistic, the period axis takes its label from whether the statistic is a period or a cohort measure, and the region detail panel shows a cell's status when it is not final. The final migration releases completed cohort fertility.

## PR description, Phase C

Resolves each published cell to the highest-priority source that holds a value for it.

Selection previously picked one source per region and statistic from a configuration table, which made a preferred source's coverage gaps into gaps in the published series. Priority now resolves per `(region, statistic, period)` cell, so a source with narrow temporal coverage no longer truncates a series and adding a source needs no configuration. Drops the `source_choice` table.

## PR description, Phase D

Adds HFD's period total fertility, parsed from a file already inside the archive the HFD adapter downloads.

Thirteen countries gain a more recent figure than the World Bank publishes, one of them 2025. Countries whose HFD series ends earlier keep their World Bank values for the later periods, which per-cell source priority now permits.
