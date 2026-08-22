# Tasks: Human Fertility Database ingestion, completed cohort fertility

**Feature**: 007-hfd-ingestion | **Branch**: `hfd-ingestion` | **Date**: 2026-08-19

**Input**: [spec.md](spec.md), [plan.md](plan.md)

Organized by the plan's three phases, each landing as its own PR on a linear stack. Phase A is complete; its checkboxes record what was done, with deviations noted at the end.

---

## Phase A: ingestion

### Schema

- [x] T001 Add `StatisticKind::Ccf` and resolve the statistic through the enum rather than a code string.
- [x] T002 Skip a statistic code the artifact build cannot parse with a warning rather than failing the build.
- [x] T003 Write `ingestion/db/migrations/<timestamp>_seed_hfd_and_completed_cohort_fertility.sql` seeding the `hfd` data source (CC BY 4.0, priority ahead of the World Bank, attribution naming HFD and both institutes) and the `ccf` statistic. Clear dependent rows in the down migration.
- [x] T004 Apply the migration to `eafora` and `eafora_test`; commit the regenerated `ingestion/db/schema.sql` and `.sqlx` cache.

### Parsing

- [x] T006 Write the failing tests for `parse_cohort_file` first: the publication date, every data row, the completed measure rather than the age-40 one, an absent value, a non-alpha-3 code, columns resolved by name rather than position, and the five rejection cases.
- [x] T007 Add `ingestion/src/hfd/hfd_model.rs` with `ParsedHfdPublication` and `ParsedHfdStatisticValue`.
- [x] T008 Implement `parse_cohort_file` and `CohortFileColumns` in `hfd_client.rs`, resolving each column by header name.
- [x] T009 Extend `ingestion/samples/hfd/tfrVH.txt` with a country whose every cohort is absent, and restore the CRLF endings HFD serves.
- [x] T010 Add in-memory archive tests for `read_cohort_members`, covering member selection and bytes that are not an archive.

### Normalization

- [x] T011 Add `IngestWarningKind::NoValuesForRegion`, and rename `UnknownCountry` to `UnrecognizedRegionCode` for what it actually reports.
- [x] T012 Write `ingestion/src/hfd/hfd_adapter.rs`: the HFD-code alias table, `resolve_region`, `normalize_row`, `group_by_code`, and `normalize`.
- [x] T013 Implement `fetch_and_store` with the revision check after the download, and `should_skip_run` extracted as a pure function so its branches are testable.
- [x] T014 Write `ingestion/tests/hfd_integration.rs` covering region mapping, both warning kinds, an absent value dropped without a warning, the cohort period encoding, a first run, an unchanged second run, and a revision that supersedes while keeping the original readable.
- [x] T015 Unit-test every branch of `should_skip_run`, including the force override.

### Wiring

- [x] T016 Add `DataSourceKind::HumanFertilityDatabase`, register it in `REGISTERED_SOURCES` and `run_source`, and update the CLI help.
- [x] T017 Add the client's HFD attribution label, which the exhaustive match in the detail panel requires.
- [x] T018 Run the adapter against live HFD and verify the counts reconcile against the upstream file.

---

## Phase B: presenting a cohort

- [x] T019 Carry `period_end` and `data_status` into `CellValue`, changing both target-gated `read_shard` implementations.
- [x] T020 Add a period-versus-cohort distinction on the enum, with the axis label and the slider's accessible name taken from it.
- [x] T021 Render the active value as a span from `period_start` to `period_end` for every statistic.
- [x] T022 Show a cell's `data_status` in the region detail panel when it is not final.
- [x] T023 Select `hfd` as the source for `ccf`, without which the artifact build refuses a statistic that has values and no configured source.

### Deviations from the plan, Phase B

- **`ShardValues` gained a period-ending lookup.** The plan had the span come from the active cell, but the scrubber is global while a cell is per-region, and the active period can name one no region has a value for. A period's ending is a property of the statistic's data, so it is indexed by period start on the shard.
- **`CellView` was extracted rather than a field added twice.** `SelectionView` and `GlobalView` already duplicated four cell-derived fields and the panel took six parameters; a status would have made it seven. Both views now embed one struct, which is also what `decode_cell` returns instead of a widening tuple.
- **A `source_choice` row is seeded for `ccf`.** Removing the release column let `ccf` into the artifact build, which refuses a series that has candidate values and no configured source. Caught by building against the real store rather than by any test, because the test database holds no `ccf` values and so never reaches that series.
- **The client cannot show HFD's full attribution.** The spec asks for it, but the manifest carries only `revision`, `published`, and `fetched` per source; `attribution_text` lives in Postgres and is never serialized into a bundle. The panel names the source, which is what a bundle supports today. Carrying the full attribution needs a producer change of its own.

---

## Phase C: period total fertility and per-cell source priority

- [x] T024 Carry the source's priority onto `CandidateValue` from the join the candidate query already makes.
- [x] T025 Group candidates per `(region, statistic, license_shard_class, period)` cell rather than per series, renaming `SeriesKey` to `CellKey`.
- [x] T026 Choose the lowest-priority-rank source per cell, ties broken deterministically so a rebuild is stable.
- [x] T027 Delete `SourceChoice`, `SourceChoiceEntity`, `read_source_choices`, and the resolver; rewrite its tests, which encode the replaced rule.
- [x] T028 Write the migration dropping `source_choice`.
- [x] T029 Read `tfrRR.txt` from the archive as well, reusing the parser with a different column set.
- [x] T030 Normalize period total fertility against the existing `tfr` statistic.
- [x] T031 Reconcile the 39 codes in `tfrRR.txt` against `tfrVH.txt`'s 37.
- [x] T032 Integration-test the join: a region both sources cover where HFD stops earlier keeps its World Bank values for the later periods, with no gap.
- [x] T033 Drop the test-only `StatisticKind` and `DataSourceKind` variants, which two real variants of each now make unnecessary.

### Deviations from the plan, Phase C

- **The tie-break is the source, not its id.** The schema's comment promised `data_source.id`, but a candidate carries its `DataSourceKind` rather than the row id, and ordering on the kind is equally deterministic without threading a `Uuid` through purely to break a tie that no two seeded sources currently produce. The migration updates the catalog comment to match.
- **`fetch_upstream` returns the archive rather than parsed files.** Two members are now read from one download, so extracting them belongs to the caller; `read_member` takes the member by name instead of sweeping for a suffix.
- **The 39-versus-37 code question closed without work.** The extras are Croatia and South Korea, both plain alpha-3, and the cohort file's codes are a strict subset of the period file's, so no alias entry was needed.
- **The five unrecognized codes now warn twice per run**, once per statistic, because each file is normalized separately. Each warning is a true statement about that file; deduplicating them would mean carrying warning state across two normalizations.

## Deviations from the plan

- **The parser lives in `hfd_client.rs`, not `hfd_model.rs`.** The plan put it in the model file on the grounds that it is pure and belongs beside its types. The repo's own convention is stronger: `_model.rs` files hold type definitions and nothing else, and wire-format knowledge belongs to the client. `hfd_model.rs` holds only the two parsed types.
- **`should_skip_run` replaced `is_already_captured`.** The plan described the skip inline. Folding the force-refetch override into one pure function made all its branches testable without the network.
- **`data_source_publication.published` carries HFD's declared date.** The plan did not mention it, and the first implementation passed null while parsing the date anyway, leaving the field unread. The column's own documentation asks for the upstream date where derivable, and it is derivable here.
- **`IngestWarningKind` gained two variants, not one.** The plan named a no-values warning. A separate `SubpopulationCode` variant was needed so `DEUTE` does not report as a country missing from the seed, which is a different and misleading fact.
- **The client changed in Phase A.** Adding the source and statistic enum variants broke exhaustive matches in the detail panel, the labels, and the colour transform, so the HFD attribution, the statistic name and unit, and the colour curve landed here rather than in Phase B. Compiler-forced, not scope creep. Completed cohort fertility shares the period measure's colour transform: same unit, and replacement level is meaningful for both.
- **The `statistic.released` column was added and then removed.** It gated the artifact build so Phase A could ship alone. Once ingestion resolved statistics through the enum, the variant had to exist in Phase A, so the flag became the only thing withholding the statistic; landing the two phases together removes the need for it entirely. The column existed only to support a PR structure.
- **Two warning kinds became one.** A separate kind for HFD's territory codes implied the problem was their being subnational, which the region schema can represent; the actual fact is that no canonical region matches, which is what `UnknownCountry` already meant. Renamed to `UnrecognizedRegionCode`, shared with World Bank WDI, and the hardcoded list of five codes deleted, since they warn correctly by falling through the region lookup.
- **A pre-existing broken test was fixed in passing.** `publish_integration.rs` did not compile on `master`, having missed a parameter added to `write_manifest` by `c1f85b8` (eafora). Left broken it would hide real failures.
- **The samples are byte-faithful.** The plan did not raise line endings. The checked-in sample now carries the CRLF that HFD actually serves.
