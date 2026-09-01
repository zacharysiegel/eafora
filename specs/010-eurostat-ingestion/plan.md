# Implementation Plan: Eurostat ingestion, mean age at childbirth and at first birth

**Branch**: `010-eurostat-ingestion` | **Date**: 2026-09-01 | **Spec**: [spec.md](spec.md)

## Summary

Adds Eurostat as the third source, two statistics measured in years, and an estimated data status. The adapter contract, the publication model, the append-with-supersede revision strategy, the report shape, and per-cell resolution by source rank are all settled and demonstrated twice over, so this plan covers only what is specific to Eurostat: a JSON-stat 2.0 parse driven by flat-index arithmetic, an observation flag read as a character set, an alpha-2 region lookup that does not exist yet, the migration, and the client's first statistic that is not measured in children per woman.

Two phases for the country-level work, one PR each, on a linear stack, then two sketched phases for the subnational work that follows.

- **A, canonical vocabulary and presentation.** The enums, the seed migration, and the client changes that let a statistic measured in years be drawn honestly. No adapter, so no published output changes.
- **B, the adapter.** The client, the parse, the flag mapping, the normalization, the samples, the tests, the CLI wiring, and the first publish carrying Eurostat.
- **C, sketch: subnational regions with NUTS-2 values.** Needs a probe of the regional datasets before it can be planned.
- **D, sketch: subnational geometry.** Needs a boundary-source download before it can be planned.

Phase A precedes B because the compiler forces it to: adding a `StatisticKind` variant breaks five exhaustive matches in the web crate and one in the shared crate, and ingestion resolves statistics through the enum, so the variant must exist before the adapter that writes to it. HFD learned this the expensive way, adding and then removing a column whose only purpose was to let its two phases ship separately. The cost of the ordering is that Phase A's presentation choices have no real data to be seen against; they are verified by host unit tests, and the visual confirmation happens in Phase B when the first shard exists. That is stated here so it is not discovered as a gap in review.

## Corrections to the surrounding documents

Places where a document this feature touches is already wrong. Code is canonical; each of these is a doc fix that belongs in the phase that makes it observable, with one exception: the two entries below that name landed feature specs are recorded here and NOT edited in place. A shipped spec is a dated record of what was true when it shipped, and rewriting it would make the record disagree with its own deviations section.

- **`docs/architecture/ingestion.md` §Geometry ingestion names a subnational source that does not fit.** It says subnational geometry comes from `ne_10m_admin_1_states_provinces.zip`, which is provinces rather than NUTS regions and does not nest with them. `docs/architecture/geometry.md` supersedes it: boundaries come from EuroGlobalMap under the EuroGeographics Open Data Licence, reconciled with Natural Earth by subtraction. Phase D corrects the older line.
- **`docs/architecture/ingestion.md` §Current preference ranking is stale.** Its table says HFD 10, Eurostat 30, World Bank 90; the shipped seeds are HFD 50 and World Bank 100, and this feature seeds Eurostat at 40. The table should be corrected in Phase A rather than left as a second ranking on the record.
- **§Merge rule step 4 is unimplemented and now reachable.** Resolution is by rank alone. Eurostat is the first source to publish a non-final status, so a Eurostat provisional value will outrank a World Bank final one for the same year. Either the clause goes or it gets implemented; leaving both on the record is the thing to avoid. Carried as an open question, not decided here.
- **The `revision_label` column comment guesses a week-numbered form for Eurostat.** There is no such field on the wire. The response's `updated` timestamp is the only revision signal, and the comment should be corrected in Phase A's migration.
- **The `data_status` catalog comment enumerates the statuses and there is no constraint behind it.** No CHECK, no enum type, no domain, in Postgres or in the shard DDL. So `estimated` will insert cleanly with no code change at all, and the first failure would be at artifact-build time in the parse back out. The comment is the only database-side artifact to update, and the Rust `TryFrom` arm is the only thing that actually enforces the set.
- **`specs/008-manifest-schema-backtracking/spec.md` §Scope cutoff misstates where an unknown key fails.** It says an unknown statistic or licence-class code fails after the manifest parses. It fails during the parse, on the version-gated path, which an existing test in `shared/src/artifact/manifest.rs` proves. FR-034's reasoning depends on the true behaviour; the spec's wording is not authority for it.
- **The manifest carries per-source attribution.** `007-hfd-ingestion` recorded that it does not and that the client can only name a source by label. `Manifest::source_attribution` at `shared/src/artifact/manifest.rs:73` carries the attribution text, licence name, licence URL and homepage, filled from the `data_source` row by `ingestion/src/artifact/artifact_db.rs:101`. Eurostat's acknowledgement therefore reaches a reader with no producer change, and the HFD note is simply out of date.

## Module layout

```text
ingestion/src/eurostat/
├── mod.rs                  # declarations and re-exports only
├── eurostat_model.rs       # EurostatResponse, EurostatDimension, EurostatCategory,
│                           # ParsedEurostatPublication, ParsedEurostatObservation, GeoCodeAlias
│                           # (per docs/conventions/types.md; mirrors ParsedHfdPublication and
│                           #  ParsedWdiPublication in the sibling sources)
├── eurostat_client.rs      # the request, the JSON-stat parse, the index arithmetic
└── eurostat_adapter.rs     # geo aliasing, the flag mapping, normalize, fetch_and_store
```

No `eurostat_db.rs`. Phase one resolves regions through the generic canonical lookups and has no source-specific SQL, so the file would be an empty stub.

The one placement question is the flat-index arithmetic, and it belongs to the client: it is knowledge of how JSON-stat addresses a cell, which is wire format. The flag-to-status mapping belongs to the adapter, because `DataStatus` is a canonical type and the client may not name one. Both existing clients hold to that line, importing nothing from `canonical::`, and both adapters are the only places a status is chosen.

The region-resolution outcome follows the shape HFD already uses, an enum whose variants carry either the resolved identifier or the warning to report, named for what it decides rather than for its shape. The parse lives in `eurostat_client.rs` rather than in the model file, following the correction HFD's Phase A recorded: `_model.rs` holds type definitions and nothing else.

`should_skip_run` moves out of `ingestion/src/hfd/hfd_adapter.rs:97` into a new `ingestion/src/adapter/adapter.rs`, with its four unit tests, and `ingestion/src/adapter/mod.rs` gains the declaration and re-export. Eurostat is its second consumer, and duplicating it across sibling source modules is the smell the convention names. `adapter/mod.rs` currently holds only declarations, so the primary content goes in the parallel-named file per the module convention.

## The parse target

The response's shape, verified against six live captures rather than from the specification:

- `updated`, a string timestamp whose offset carries no colon. The revision label is the raw string; `published` is it parsed with an explicit format.
- `value`, an object keyed by the decimal string of a flat index. Sparse (5,358 of 11,310 for the phase-one extraction), unordered, never null-valued, and an object even when the slice is dense.
- `status`, an object over the same key space holding the raw flag strings. Absent entirely when nothing in the slice is flagged, so it deserializes with a default. Not a subset of `value`'s keys: one cell in the phase-one extraction is flagged and valueless.
- `id`, the dimension names in stride order. `size`, their cardinalities, positionally aligned.
- `dimension`, keyed by dimension name, each holding a label and a category with an index map from code to position and a label map from code to name.
- `extension`, Eurostat-specific and unmodelled except as a cross-check while probing. Its per-response status label block is a display aid and must not be read as an enumeration of legal codes.

Unknown fields are ignored, which is serde's default, so the deny-unknown-fields attribute must not be added.

The arithmetic. With `n` dimensions, the last varies fastest and a dimension's stride is the product of the sizes to its right. A flat index decomposes as `position[k] = (flat_index / stride[k]) % size[k]`, and each position maps back to a code by inverting that dimension's index map. The inversion is materialized once per dimension as a positional vector and validated as a bijection onto the declared size; every response captured satisfies it, and a break in it would mis-attribute every observation in the run rather than fail.

Two traps worth naming, both verified: category positions are not request order (Eurostat sorts the indicator codes alphabetically), and iteration must be driven by `value`'s keys, because a `status`-driven loop invents an observation for the flagged valueless cell and panics if the value lookup is unwrapped. A test asserting the parsed count equals the observation count, not the flag count, is what pins the second.

The flag precedence is one ordered table scanned for membership, not nested conditionals. Every atomic code is a single character and every concatenation is an ordered run of those characters, so a character-membership test is sound and no tokenizer is needed. Eight codes occur in this dataset, seven for the phase-one indicators, at a maximum length of three.

## Data model

One migration, in Phase A, plus the two catalog-comment corrections it carries. `ingestion/db/schema.sql` is regenerated by dbmate and committed with it.

### Phase A: `seed_eurostat_and_mean_age_statistics`

- `data_source`: the Eurostat row at `preference_rank=40`, `license_class='attribution'`, with the licence name, licence URL, homepage URL and attribution text FR-001 requires. The licence name renders as the anchor text of the licence link in the detail panel, so it has to read as a short label. The attribution text is rendered verbatim; both existing rows shape it as publisher, product, then the licence name in a trailing parenthetical, and Eurostat's adds the acknowledgement clause its reuse notice asks for.
- `statistic`: the two rows, both `units='years'`, both with `name_abbreviated_en`. Column list is `(code, name_en, name_abbreviated_en, units)`. `description` was dropped by `20260825120000_drop_statistic_description.sql`, and `name_abbreviated_en` was added not-null by `20260621120000_add_statistic_abbreviated_name.sql`, so neither the initial seed nor the HFD seed is a usable column-list template.
- The `statistic_value.data_status` catalog comment, widened to include the new status.
- The `data_source_publication.revision_label` catalog comment, correcting its Eurostat example.

The down migration clears dependents explicitly, keyed on each parent separately, because another source may later supply these statistics and Eurostat may later supply others. It must not copy the HFD seed's `source_choice` deletes: that table does not exist at this migration's point in the timeline, so referencing it would break a down migration.

Apply with `./scripts/db/dbmate.sh up`, never a bare `dbmate`. The wrapper carries the URL, the migrations and schema paths, and re-runs `cargo sqlx prepare --workspace -- --all-targets`, which the new alpha-2 lookup needs before anything compiles offline.

No migration in Phase B. No selection rows anywhere: `source_choice` is gone and resolution is by rank.

## Phase A steps

Each numbered step is a commit.

1. **`DataStatus::Estimated`,** with its string form and its parse arm, in `shared/src/canonical/canonical_model.rs:152-189`. The parse arm is not optional: the value is written as text and parsed back at `ingestion/src/canonical/canonical_entity.rs`, `ingestion/src/artifact/artifact_model.rs:62`, and both target-gated shard readers in `shared/src/sqlite/shard_db.rs`. An existing test already asserts an unknown status is rejected, which is exactly the failure a missing arm produces.
2. **`DataSourceKind` and `StatisticKind` variants,** with their codes and parse arms, at `shared/src/canonical/canonical_model.rs:76-148`. Both enums derive `Ord` and both key a manifest map, so declaration order is a wire-visible decision; the new statistics sort after the two existing ones. Both new statistics report the period temporal basis, which is what keeps the scrubber's axis label reading as a year.
3. **The seed migration,** as §Data model describes, applied to both databases through the wrapper script, with the regenerated schema dump and sqlx cache committed.
4. **The colour transform arms** in `shared/src/map/color.rs:100`. A range-normalized transform for the two new statistics, whose inflection is already `None`, which is what makes the legend draw a plain bar with no marker. The existing curve's own doc comment states it is keyed to absolute values, and the measured saturation (positions 0.9941 at 24, 0.9954 at 30, 0.9957 at 32) is why it cannot be reused. The scale's polarity doc comment justifies red-at-the-low-end as the fertility direction and stops being true of every statistic it serves; it needs rewording either way, and the polarity decision itself is open.
5. **The chart's reference line becomes genuinely optional.** `reference_value` at `web/src/map/detail_panel.rs:484` returns `None` for the two new statistics, and the geometry carries whether a reference exists so the line and its label can be hidden. They cannot simply be omitted: the file's own comments record that Safari logs an invalid empty attribute value for every attribute Leptos removes when it tears an SVG element down, which is why the cursor hides behind a visibility class and the marker behind a zero radius. Follow the cursor's pattern, with the matching style rule beside the existing one. Without this step the `None` branch falls back to the plot's baseline and draws a dashed line on top of the solid axis with an empty label.
6. **The legend caption and the label lookups.** `reference_caption` at `web/src/map/labels.rs:40` returns `None` for the two new statistics; `statistic_label`, `statistic_unit` and `statistic_description` gain arms; `source_label` and the two status functions in the detail panel gain theirs. Every one of these is an exhaustive match, so the compiler names the site; what it cannot name is the locale key each one needs.
7. **The locale strings** in `web/locales/en.json`: three per new statistic in the `statistic` block, one in the `source` block, one in `detail.status`. The unit strings are lowercase fragments matching their neighbours, because they render as a standalone line under a value and as a rotated axis title; the status sentence is a full sentence, matching its neighbours. `web/build.rs` generates the i18n module from this file, so a missing key is a compile error rather than a runtime blank.
8. **The doc corrections** listed in §Corrections, in the same commit as the migration where they are catalog comments and separately where they are prose.

## Phase B steps

Test-first for the parse, the flag mapping, and the normalization, per Constitution Principle VII.

1. **Extract the skip decision.** `should_skip_run` and its four tests move from `ingestion/src/hfd/hfd_adapter.rs:97` to a new `ingestion/src/adapter/adapter.rs`, re-exported from `adapter/mod.rs`. Pure motion, no behaviour change, so the existing tests are the verification.
2. **`find_country_by_iso2`** in `ingestion/src/canonical/canonical_db.rs`, copied from the alpha-3 lookup at `:10-26` with the predicate changed. The column is not-null and carries a unique constraint (`country_iso2_key`), so at most one row can match. Nothing in Rust reads `iso2` today. This is the one genuinely new generic query the feature needs, and the reason `cargo sqlx prepare` is mandatory rather than incidental.
3. **The live capture probe.** An ignored test that fetches the phase-one extraction and writes it to a named temporary path, printing the destination, so the checked-in samples are trimmed from a real response. Eurostat needs no credential, so this is cheaper than HFD's equivalent and there is no reason to skip it. Capture three responses: the phase-one extraction, a small dense multi-dimensional slice, and a single unflagged cell.
4. **The wire and parsed types** in `eurostat_model.rs`. The observation and flag maps deserialize straight into integer-keyed maps; serde parses a stringified-integer object key into an integer map key, verified by a compiled probe, so no manual key parsing is needed. Ordered maps rather than hashed ones, so log output and test assertions are stable. The flag stays a string on the parsed value, because the client may not name a canonical type.
5. **The parse, tests first.** The publication from `updated`, including the colon-less offset. Dimension resolution by name. The positional inversion of each category index map, with its bijection check. The stride decomposition. Iteration over the observation map. A rejection case per failure: a missing dimension, an index that is not a bijection, a body that is not the expected document. The count assertion that pins the flagged-valueless cell.
6. **The request.** One request per run through the shared HTTP client, with the dataset, format, language, country-level geo filter and three indicator filters. A non-success status fails with the status and the URL in the message; a body that is not the expected document fails naming the dataset and the cell limit, following the precedent of the existing guard that catches a login page served under status 200. No adapter options parameter: like HFD, Eurostat cannot narrow the request by revision, and the existing World Bank client's unused options parameter is the shape to avoid rather than to copy.
7. **The flag mapping, tests first.** One ordered table of character to status, scanned for membership, in `eurostat_adapter.rs`. Tests cover every atomic character, at least the three concatenations that occur most in the real data, an absent flag, and a flag of only unmodelled qualifiers.
8. **The geo resolution.** The alias table as one const array of paired code and target, following the HFD alias table's shape at `ingestion/src/hfd/hfd_adapter.rs:20-29`. The exclusion table for the codes that survive the country-level filter without naming a country. A resolved-or-warned outcome enum, as HFD has at `:31-34`, since the outcome is per code rather than per row.
9. **`normalize`,** mirroring `ingestion/src/hfd/hfd_adapter.rs:114`: the statistic resolved once through its kind, the rows walked, warnings collected alongside accepted values. The caller filters the parsed observations by indicator code before each of the three calls, so `normalize` stays indicator-agnostic and keeps HFD's signature.
10. **`fetch_and_store`.** The nine steps both existing adapters share, in their order, with the skip step included: begin the transaction, resolve the source row, read the latest publication, fetch, parse, skip or continue, normalize three times, record, attach warnings, commit. One transaction per run.
11. **The integration test** against `eafora_test` in a per-test transaction rolled back at teardown, following `ingestion/tests/hfd_integration.rs`: alias resolution, the excluded code, an unresolvable code's warning, the three-statistic fan-out, a flag reaching the stored status, and the added, skipped, and superseded triple. Source-specific helpers stay private in the test file; only generic helpers live under `tests/helpers/`.
12. **The CLI wiring.** The registered-sources slice, the dispatch match arm, the import, and the source argument's help text. The match is exhaustive so the compiler names it; the slice and the help text are not, and omitting the slice entry silently drops Eurostat from the all-sources run.
13. **The first real run and the first publish.** Run the adapter against live Eurostat, reconcile the counts against the response, build, publish locally, re-sync the embedded tree, and verify the site tree. The manifest schema version decision (open question 4) lands here if it lands at all, together with the embedded re-sync FR-035 requires.

## Phase C, sketch: subnational regions with NUTS-2 values

Probed on 2026-09-01, so the facts below are measured rather than assumed.

- **`demo_r_find2`** carries NUTS-2, **`demo_r_find3`** NUTS-3. Both take the same `geoLevel` parameter the country-level request uses, and it returns one level cleanly: 345 four-character codes and 1,615 five-character codes respectively, with no mixing. The level-mixing hazard recorded against these datasets applies to an unfiltered request, not to a level-filtered one.
- **Both carry three indicators**, `TOTFERRT`, `AGEMOTH` and `MEDAGEMOTH`. Mean age at first birth is country-level only, so one of the two statistics Phase A added cannot be published subnationally at all, and a NUTS layer for it would be empty rather than sparse.
- **They add a `unit` dimension** the country-level dataset has none of, with members `NR` and `YR`. Every `TOTFERRT` observation sits under `NR`, so the request pins the unit rather than treating it as a varying dimension.
- **Extraction sizes are comfortable.** NUTS-2 is 24,150 cells for 8,962 observations over 1990 to 2024; NUTS-3 is 38,760 cells for 15,901 observations over 2013 to 2024. Both are far inside the 500,000-cell synchronous ceiling, so each stays one request.
- **The same flag attribute**, with the same characters Phase B already maps: NUTS-2 returned `b` 107, `p` 81, `e` 40, `ep` 17; NUTS-3 returned `e` 168, `b` 158, `p` 101. No new character appears, so the precedence table needs no change.
- **NUTS-2 spans 37 country prefixes**, every one of them among the 48 codes the country-level extraction already resolves, so every subnational region has a seeded parent to hang from and no country needs adding.

Two consequences for the order of work. Mean age at first birth stays country-level, so the subnational phase publishes two statistics rather than three. And a NUTS code's parent is its own prefix, which every seeded country already answers to, so the hierarchy is derivable from the codes themselves without a second source.

What is already known about the shape:

- The region table is the unified hierarchy and already anticipates this: `region.level`'s catalog comment reserves subnational levels, `parent_region_id` is the only linkage needed, and `m49_code` is nullable precisely for levels that have no M49 equivalent. `statistic_value.region_id`'s comment already says it points at any level.
- The `country` table is a strict one-to-one extension of country-level regions, so a NUTS region gets no row in it. Its code namespace is therefore the region code, and the alpha-2 lookup Phase B adds does not serve subnational resolution; a NUTS lookup is a separate query.
- A NUTS region's parent is its country at NUTS-2 and its NUTS-2 parent at NUTS-3, which makes the hierarchy three levels deeper than anything the seed populates today.
- The shard keys values by region code, and the client resolves a cell by region code, so subnational values need no shard schema change. What they need is geometry keyed the same way, which is Phase D.
- Nothing in the client scopes a region set by level. The rank rows read "lowest of 217", the legend's range covers every drawn value, and the picker offers a statistic without saying at what level it exists. A layer mixing countries and NUTS-2 regions would rank them against each other and colour them on one scale, which is wrong in both cases. Deciding how a level is selected and how a mixed layer is prevented is the substance of the phase, alongside the region model itself.

Doing NUTS-2 before NUTS-3 is the right order regardless of what the probe finds: it is roughly an order of magnitude fewer regions, it is the level Eurostat's own coverage notes describe as more complete, and it exercises the whole path (region model, ingestion, geometry, level selection) at a size where a mistake is cheap to see.

## Phase D, sketch: subnational geometry

Boundary coverage was probed on 2026-09-01 through the WFS the Open Maps for Europe viewer uses, rather than the 507 MB download behind its email form. Findings:

- The layer is `ome:egm_wfs_nuts_3` and there is no NUTS-2 layer, so NUTS-2 geometry has to be derived by unioning NUTS-3 polygons on their four-character prefix.
- It reports 16,114 features, which are polygon parts and not regions: Norway contributes 4,751 and Sweden 3,695, both coastlines of thousands of islands. A distinct-region count is still unknown, because the service began returning 504s under repeated paging.
- **It covers 35 of the 37 countries Eurostat publishes NUTS regions for. Montenegro and Türkiye are absent.** Türkiye is the substantive gap, at 26 NUTS-2 regions that would carry values with no polygon to draw them on; Montenegro is one region at each level.

Both gaps were then closed on 2026-09-01 without a commercial licence, so the phase does not have to choose:

- **Montenegro needs no subnational geometry.** Eurostat publishes it as one region at each level, `ME00` and `ME000`, so the country outline already in the layer is its NUTS geometry.
- **Türkiye comes from Natural Earth's admin-1 layer**, public domain like the countries file already used. It carries exactly 81 Turkish provinces against Eurostat's 81 NUTS-3 regions, and the two sets match one to one by name with nothing left over on either side, which follows from Turkish NUTS-3 being defined on the provinces themselves. Grouping those 81 on the four-character prefix yields exactly the 26 NUTS-2 regions Eurostat publishes.

The join has to be a committed table of 81 rows rather than a derivation, because the two sources use unrelated identifiers: Natural Earth carries ISO 3166-2 (`TR-34`), Eurostat a NUTS code (`TR100`). It is generated once by matching names with diacritics stripped.

Boundaries were checked by area rather than assumed to agree. The 81 provinces sum to 781,300 km2 against Türkiye's published 783,562, a difference of 0.3%, so they tile the country without overlaps or holes. Per-province deviations are larger and follow a recognizable pattern: inland provinces sit within one or two percent, while small and coastal ones swing further (İstanbul -9.8%, Yalova -18.0%, Trabzon +8.2%, İzmir +8.7%), with neighbours erring in opposite directions. That is 1:10M generalisation moving coastlines and short shared borders, and official figures that sometimes count inland water. It is not evidence of a different unit set.

What this does not establish: that Natural Earth's provinces align with EuroGlobalMap's NUTS-3 polygons along the Turkish border, where the two sources meet. The subtraction in `geometry.md` handles that seam the same way it handles every other one, so it is a case the existing design already covers rather than a new problem.


Also not planned in detail, and for a reason that cannot be resolved by reading: **the phase's shape depends on whether the chosen boundary source's finest optional layer covers every country ingested, and only downloading it settles that.** EuroGlobalMap's optional NUTS_3 layer is the candidate the owner has in mind; nothing in this repository mentions it, and its per-country coverage is a property of the distribution rather than of its documentation. If it covers every country, one layer serves both NUTS levels by dissolving upward. If it does not, either the countries it misses fall back to the coarser level (a mixed-resolution layer, with a visible seam and a per-country record of which level a feature represents) or a second source fills the gaps (a join problem, and two boundary generalizations meeting along a shared border).

What is settled, and what is not:

- The repository's committed subnational geometry decision names a Natural Earth provinces layer for v2 and later, in `docs/architecture/ingestion.md` §Geometry ingestion. Provinces are not NUTS: for several member states the two hierarchies do not nest, so a provinces layer cannot carry NUTS values. That decision predates any subnational statistic and needs revisiting in this phase rather than being followed.
- The existing geometry pipeline's shape does carry over unchanged: fetched at build time from a pinned release, processed in memory, joined to the canonical region key, emitted as FlatGeobuf with its spatial index, nothing staged in Postgres and nothing committed. A NUTS layer differs only in the key it joins on and in the volume.
- Geometry is public domain today and ships in the base shard unsegmented. A boundary source under any other licence changes that, and the licence of whatever is chosen is a gating question for the phase, not a detail of it.
- Feature volume is the other unknown with a real consequence: the current world layer is one feature per country at 1:50m, and a NUTS-3 layer is over a thousand features at a much finer generalization. Whether it fits the client's first-paint and second-paint budgets is measurable with the existing budget script once a candidate file exists.

## Test plan

- **Parsing**: host unit tests in `eurostat_client.rs` against the three checked-in samples, plus inline malformed documents for the rejection cases. No database, no network.
- **Flag precedence**: host unit tests in `eurostat_adapter.rs`, no database. This is where the owner's ordering decision is pinned, and where the forecast character is exercised at all, since it does not occur in this dataset.
- **The skip decision**: the four existing tests, moved with the function.
- **Normalization and recording**: integration tests against `eafora_test`, each in a transaction rolled back at teardown for MVCC isolation. Run `./scripts/db/setup-test-db.sh` first.
- **Live**: an ignored capture probe, run by hand. It is a probe, not a gate.
- **Presentation**: the chart's scale is arithmetic and gets a host test asserting a series with no reference occupies the plot rather than the top fifth of it, as the counterpart to the existing test that asserts the reference is held inside the range. The colour transform's saturation claim is arithmetic too and can be asserted directly. The rest of the presentation work is shell code, exempt from strict test-first, and verified by looking at it in Phase B when a shard exists.
- **No browser harness.** Nothing here diverges between wasm32 and the host; `cargo check` against the wasm target plus the host tests is the coverage, per the standing rule.
- **The whole ingestion suite** before handoff, since the artifact and publish integration tests exercise the shard writer that now has to round-trip a new status.

## Deviations

To be recorded in [tasks.md](tasks.md) §Deviations from the plan as the work lands, per the repository's convention.

## PR description, Phase A

Adds Eurostat's source registration, two statistics measured in years, and an estimated data status, with the client vocabulary each needs.

Mean age of women at childbirth and mean age of women at first birth are the first statistics not measured in children per woman, so the presentation decisions that were previously implicit become explicit: the colour ramp normalizes against the observed range instead of pivoting at the replacement rate, which every plausible mean age saturates past; the history chart draws no reference line where no threshold exists, and no longer forces a fertility value into the vertical range of a series measured in years; the legend captions no inflection where there is none. Eurostat's observation flags are read into a data status, which needed a variant for values a statistical agency reports as estimated.

Scope is the canonical vocabulary and the presentation only. Neither new statistic has values yet, so no shard and no manifest key changes.

## PR description, Phase B

Ingests Eurostat's country-level fertility indicators, and reads each observation's published quality flag into its data status.

One request per run against the fertility-indicators dataset covers three indicators for 48 countries over 1960 to 2024. The JSON-stat response addresses observations by a flat index, so parsing resolves each dimension by name and each category position from the response's own index maps rather than from the order the request asked for; observations are enumerated from the value map, because the flag map is not a subset of it and can name a cell that has no value. Eurostat's two non-ISO geo codes are aliased, and the metropolitan-France code is excluded rather than double-counted against France. At preference rank 40 Eurostat now supplies the published total fertility figure for the countries it covers, while a year only the World Bank covers still emits.

## The adapter trait, revisited

`docs/architecture/ingestion.md` says the orchestrator becomes a shared trait once the pipeline shape proves stable. HFD's plan answered no on the evidence of two sources; Eurostat is the third and does not change the answer, but it does change one thing in the reasoning.

Eurostat's pipeline diverges from the World Bank's at the same point HFD's does: the revision label is inside the response, so the skip decision happens after the fetch and the fetch takes no options. That is now two of three sources sharing a shape the fourth does not, which is a reason to extract the shared *step*, and Phase B does exactly that with the skip decision. It is not a reason to extract the orchestrator: what the three sources hold in common is an ordering, and the ordering is nine lines that read correctly in full at each site. A hand-written match arm per source stays the preferred shape, and the doc's standing intention should be amended to say so.
