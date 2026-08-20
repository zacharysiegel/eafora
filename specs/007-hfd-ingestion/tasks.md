# Tasks: Human Fertility Database ingestion, completed cohort fertility

**Feature**: 007-hfd-ingestion | **Branch**: `hfd-ingestion` | **Date**: 2026-08-19

**Input**: [spec.md](spec.md), [plan.md](plan.md)

Organized by the plan's three phases, each landing as its own PR on a linear stack. Phase A is complete; its checkboxes record what was done, with deviations noted at the end.

---

## Phase A: ingestion

Order is load-bearing between T001 and T003: the artifact build reads every statistic code and hard-errors on one with no client enum variant, so seeding `ccf` before the release filter exists breaks every build, including the World Bank's.

### Schema

- [x] T001 Write `ingestion/db/migrations/<timestamp>_add_statistic_released.sql` adding a nullable `released timestamp with time zone` to `statistic`, with a catalog comment, and releasing `tfr` so shipped behaviour is unchanged.
- [x] T002 Add `where released is not null` to `artifact_db::read_all_statistic_kinds`.
- [x] T003 Write `ingestion/db/migrations/<timestamp>_seed_hfd_and_completed_cohort_fertility.sql` seeding the `hfd` data source (CC BY 4.0, priority ahead of the World Bank, attribution naming HFD and both institutes) and the `ccf` statistic, left unreleased. Clear dependent rows in the down migration.
- [x] T004 Apply both migrations to `eafora` and `eafora_test`; commit the regenerated `ingestion/db/schema.sql` and `.sqlx` cache.
- [x] T005 Verify the decoupling: an artifact build succeeds with `ccf` seeded and no `StatisticKind` variant.

### Parsing

- [x] T006 Write the failing tests for `parse_cohort_file` first: the publication date, every data row, the completed measure rather than the age-40 one, an absent value, a non-alpha-3 code, columns resolved by name rather than position, and the five rejection cases.
- [x] T007 Add `ingestion/src/hfd/hfd_model.rs` with `ParsedHfdPublication` and `ParsedHfdStatisticValue`.
- [x] T008 Implement `parse_cohort_file` and `CohortFileColumns` in `hfd_client.rs`, resolving each column by header name.
- [x] T009 Extend `ingestion/samples/hfd/tfrVH.txt` with a country whose every cohort is absent, and restore the CRLF endings HFD serves.
- [x] T010 Add in-memory archive tests for `read_cohort_members`, covering member selection and bytes that are not an archive.

### Normalization

- [x] T011 Add `IngestWarningKind::SubpopulationCode` and `NoValuesForRegion`, and `IngestReport::values_absent_upstream`.
- [x] T012 Write `ingestion/src/hfd/hfd_adapter.rs`: the national-total and subpopulation code tables, `resolve_region`, `normalize_row`, `group_by_code`, and `normalize`.
- [x] T013 Implement `fetch_and_store` with the revision check after the download, and `should_skip_run` extracted as a pure function so its branches are testable.
- [x] T014 Write `ingestion/tests/hfd_integration.rs` covering region mapping, both warning kinds, the absent-value count, the cohort period encoding, a first run, an unchanged second run, and a revision that supersedes while keeping the original readable.
- [x] T015 Unit-test every branch of `should_skip_run`, including the force override.

### Wiring

- [x] T016 Add `DataSourceKind::HumanFertilityDatabase`, register it in `REGISTERED_SOURCES` and `run_source`, and update the CLI help.
- [x] T017 Add the client's HFD attribution label, which the exhaustive match in the detail panel requires.
- [x] T018 Run the adapter against live HFD and verify the counts reconcile against the upstream file.

---

## Phase B: presenting a cohort

- [ ] T019 Carry `period_end` and `data_status` into `CellValue`, changing both target-gated `read_shard` implementations.
- [ ] T020 Add `StatisticKind::Ccf` and a period-versus-cohort distinction on the enum, with the axis label taken from it.
- [ ] T021 Render the active value as a span from `period_start` to `period_end` for every statistic, replacing the year-based scrubber arithmetic.
- [ ] T022 Show a cell's `data_status` in the region detail panel when it is not final.
- [ ] T023 Write the release migration for `ccf`, last in the phase.

---

## Phase C: period total fertility and per-cell source priority

- [ ] T024 Carry the source's priority onto `CandidateValue` from the join the candidate query already makes.
- [ ] T025 Group candidates per `(region, statistic, license_shard_class, period)` cell rather than per series, renaming `SeriesKey` to `CellKey`.
- [ ] T026 Choose the lowest-priority-rank source per cell, ties broken by source id so a rebuild is deterministic.
- [ ] T027 Delete `SourceChoice`, `SourceChoiceEntity`, `read_source_choices`, and the resolver; rewrite its tests, which encode the replaced rule.
- [ ] T028 Write the migration dropping `source_choice`.
- [ ] T029 Read `tfrRR.txt` from the archive as well, reusing the parser with a different column set.
- [ ] T030 Normalize period total fertility against the existing `tfr` statistic.
- [ ] T031 Reconcile the 39 codes in `tfrRR.txt` against `tfrVH.txt`'s 37.
- [ ] T032 Integration-test the join: a region both sources cover where HFD stops earlier keeps its World Bank values for the later periods, with no gap.

---

## Deviations from the plan

- **The parser lives in `hfd_client.rs`, not `hfd_model.rs`.** The plan put it in the model file on the grounds that it is pure and belongs beside its types. The repo's own convention is stronger: `_model.rs` files hold type definitions and nothing else, and wire-format knowledge belongs to the client. `hfd_model.rs` holds only the two parsed types.
- **`should_skip_run` replaced `is_already_captured`.** The plan described the skip inline. Folding the force-refetch override into one pure function made all its branches testable without the network.
- **`data_source_publication.published` carries HFD's declared date.** The plan did not mention it, and the first implementation passed null while parsing the date anyway, leaving the field unread. The column's own documentation asks for the upstream date where derivable, and it is derivable here.
- **`IngestWarningKind` gained two variants, not one.** The plan named a no-values warning. A separate `SubpopulationCode` variant was needed so `DEUTE` does not report as a country missing from the seed, which is a different and misleading fact.
- **The client changed in Phase A.** Adding the source enum variant broke the detail panel's exhaustive match, so the HFD attribution label landed here rather than in Phase B. This is compiler-forced, not scope creep.
- **A pre-existing broken test was fixed in passing.** `publish_integration.rs` did not compile on `master`, having missed a parameter added to `write_manifest` by `c1f85b8` (eafora). Left broken it would hide real failures.
- **The samples are byte-faithful.** The plan did not raise line endings. The checked-in sample now carries the CRLF that HFD actually serves.
