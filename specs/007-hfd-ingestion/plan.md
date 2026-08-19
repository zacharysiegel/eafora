# Implementation Plan: Human Fertility Database ingestion, completed cohort fertility

**Branch**: `hfd-ingestion` | **Date**: 2026-08-19 | **Spec**: [spec.md](spec.md)

## Summary

Adds HFD as the second real source and completed cohort fertility as the second statistic. The adapter contract, the publication model, the append-with-supersede revision strategy, and the `IngestReport` shape are all settled by `docs/architecture/ingestion.md` and demonstrated by World Bank WDI, so this plan covers only what is specific to HFD: an authenticated archive download, a fixed-width text parser, a region-code mapping with five deliberate exclusions, three migrations, and the client changes that let a cohort be presented as a range rather than an instant.

The work splits at a release gate. Phase A lands ingestion and real rows in `statistic_value` with no client involvement at all, because a statistic with `released is null` is invisible to the artifact build. Phase B teaches the client to render a cohort and then releases the statistic in its final migration. The two are separate PRs on a linear stack.

## Already built

`ingestion/src/hfd/hfd_client.rs` is written and verified against the live service: it signs in, downloads `tfr.zip`, and extracts the `tfrVH.txt` member. `ingestion/samples/hfd/tfrVH.txt` is a reduced copy of a real download. `ingestion/tests/hfd_live.rs` is the `#[ignore]`d probe that produced both.

What remains for Phase A is the parser, the normalization, the migrations, and the wiring.

## Corrections to the spec

Five things the spec asserts do not match the code. Code is canonical; the spec is what changes.

- **The merge is driven by `source_choice`, not by `preference_rank` ordering.** FR-008 asks for "a `preference_rank` that wins over World Bank WDI for the cells both supply," and SC-003 tests a cell both sources cover. In the implementation, `source_choice` holds one row per `(statistic, license_shard_class)` naming exactly one `data_source`, optionally overridden per region; `preference_rank` is a tiebreaker recorded on the source, not a query-time ranking. Nothing selects between two sources for one cell, by design, because a series must never mix sources. For `ccf` this is moot in a second way: World Bank WDI publishes no completed cohort fertility, so no cell is contested. **The deliverable is a `source_choice` row for `(ccf, base)` naming `hfd`**, and SC-003 is unfalsifiable as written and should be restated as "the shard carries exactly one source per cell."
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

Three migrations, dbmate-timestamped when written. `ingestion/db/schema.sql` is regenerated by dbmate afterward and committed with them.

### Phase A, one: `add_statistic_released`

Adds `released timestamp with time zone` to `statistic`, nullable, with a catalog comment stating that null means the statistic is not offered to clients. Sets `released = now()` for `tfr` so nothing about the shipped product changes (FR-010c).

Named `released` rather than `published` because `data_source_publication.published` already means the upstream's own publication date, and the two would be confusable in a query that joins both.

`artifact_db::read_all_statistic_kinds` gains `where released is not null`. That one predicate is what decouples a seed row from a `StatisticKind` variant: ingestion resolves statistics by `statistic.code` and never touches the enum, so an unreleased statistic can accumulate real rows while no client knows it exists. The `try_from` on the code stays a hard error for a released statistic with no variant (FR-010b) — releasing something no client can draw is a misconfiguration, and a failed artifact build is how it surfaces.

### Phase A, two: `seed_hfd_and_completed_cohort_fertility`

- `data_source`: `code='hfd'`, `license_class='attribution'`, `license_name='CC BY 4.0'`, `preference_rank=50` (below WDI's 100; lower wins), `attribution_text` naming HFD, the Max Planck Institute for Demographic Research, the Vienna Institute of Demography, and `www.humanfertility.org` per FR-008a.
- `statistic`: `code='ccf'`, `name_en='Completed cohort fertility'`, `name_abbreviated_en='CCF'`, units in the same idiom as `tfr`'s "children per woman", `released` left null.
- `source_choice`: `(ccf, 'base')` → `hfd`, selected by code in a `from statistic, data_source` insert like the existing `source_choice` seed, so no id is hardcoded. Class `base` because CC BY 4.0 is the attribution class, same as WDI.

`ccf` over `completed_fertility` is settled by FR-009 on the ground that `cfr` would collide with HFD's own `cfr.zip` of cumulative fertility rates. This migration is the last cheap moment to change it; after it lands, the code is in a shard filename and a client enum.

### Phase B, three: `release_completed_cohort_fertility`

`update statistic set released = now() where code = 'ccf'`. Last change in the phase (FR-020a).

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

## Test plan

- Parser and mapping: host unit tests in `hfd_model.rs` and `hfd_adapter.rs` against `ingestion/samples/hfd/tfrVH.txt`, which covers a plain code, a national-total code that maps, two codes that must warn, an absent `CCF`, and a country with no values at all (FR-013).
- Archive extraction: a unit test building a small zip in memory, so `read_cohort_members` is covered without a checked-in binary.
- Login: `read_antiforgery_token` against a captured form fragment, including the two malformed cases.
- Normalization and recording: integration tests against `eafora_test`, each in a transaction rolled back at teardown.
- Live: `ingestion/tests/hfd_live.rs` stays `#[ignore]`d. It is a probe, not a gate.
- SC-008 (a wrong password fails without echoing either credential) is asserted by reading the error's text in a unit test over the failure branch, not by a live attempt with a bad password.
- Phase B's axis, span, and status behaviour is target-agnostic and belongs in host tests plus `cargo check --target wasm32-unknown-unknown`. No browser harness.

## The adapter trait

`docs/architecture/ingestion.md` says the orchestrator becomes a shared trait once a second source proves the shape stable. This is that second source, so the decision is due, and the answer is to defer.

HFD's pipeline diverges at three points: `fetch_upstream` needs no options and cannot use them, the revision comparison happens after the download rather than before, and authentication has no analogue in WDI. A trait extracted now would either encode WDI's ordering and force HFD to work around it, or be so loose that it says nothing. Two sources that differ this much are evidence the shape is not yet stable, which is the condition the doc set. Gapminder and Eurostat are next and both are unauthenticated file downloads; if they land on one shape, extract then, generic over the source rather than `dyn`.

## PR description, Phase A

Adds the Human Fertility Database as a data source and completed cohort fertility as a statistic, ingested from HFD's cohort summary file.

The adapter authenticates against HFD, downloads the by-statistic archive once per run, parses the cohort file by column name, and maps HFD's 37 country codes onto 32 canonical regions; the five that name subpopulations rather than countries produce warnings. Adds a nullable `statistic.released` column, which the artifact build now filters on, so a statistic can be ingested and verified before any client knows it exists. `ccf` is seeded unreleased and stays invisible until the client can draw a cohort.

## PR description, Phase B

Presents a cohort as the range it covers rather than as a single instant.

The shard read path carries `period_end` and `data_status` into the client, both already written into every shard and previously discarded. The scrubber draws the active value as a span for every statistic, the period axis takes its label from whether the statistic is a period or a cohort measure, and the region detail panel shows a cell's status when it is not final. The final migration releases completed cohort fertility.
