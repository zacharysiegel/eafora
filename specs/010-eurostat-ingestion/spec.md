# Feature Specification: Eurostat ingestion, mean age at childbirth and at first birth

**Feature Branch**: `010-eurostat-ingestion`

**Created**: 2026-09-01

**Status**: Draft

**Input**: User description: "ingest Eurostat as a third data source. Phase one is country-level only: total fertility rate into the existing statistic, plus mean age of women at childbirth and mean age at first birth as two new ones. Eurostat outranks HFD and the World Bank. Observation flags become a real data status."

## Why this source, and why these statistics

Eurostat is the third data source and the first one whose values arrive with a published quality flag attached. `docs/data/sources-survey.md` §Recommended integration order puts it in phase 1 alongside the sources already landed, and `docs/task-order.md:11` names it as next. It is the highest-quality fertility data for the EU and EEA, collected under a statistical regulation with a standardized methodology across member states, and it publishes a country's figure earlier than the World Bank republishes it. For the 48 countries it covers, it is the better source at every period it holds, which is why it takes the lowest `preference_rank` of the three.

Two of the three statistics in this feature are new, and both are measured in years rather than in children per woman. That is the interesting part of the work. Everything the client knows about a statistic beyond its numbers is currently written as though every statistic were a fertility rate: the colour ramp pivots at the replacement rate of 2.1, the history chart draws a dashed reference line at 2.1 and forces that value into its vertical range, the legend captions its inflection "replacement", and the unit string is "births per woman". A mean age near 30 through the existing colour transform lands between position 0.994 and 0.996 of the ramp for every plausible value, so all of Europe paints one tone and the legend bar is a solid rectangle. This feature is therefore as much about the client's per-statistic vocabulary as it is about a new adapter.

The third statistic is the existing total fertility rate, and it is the reason the source ranking matters. Eurostat publishes a member state's provisional figure well before the World Bank's release cycle carries it, so a Eurostat cell replaces a World Bank cell for the same country and year while the countries Eurostat does not cover keep their World Bank values. Per-cell resolution by source rank already does this, landed with `007-hfd-ingestion` Phase C.

Eurostat is also the source that makes the observation-status machinery real. Every value in the store today is `final`, hardcoded at two adapter sites. Eurostat publishes an `OBS_FLAG` attribute per cell whose 42 codes are ordered concatenations of nine atomic characters, and the phase-one extraction carries 136 flag entries against 5,001 observations. Reading them is what stops a provisional figure from being displayed as a confirmed one, which is Constitution Principle II applied to a case where the source actually tells us.

## Behaviour

### A run captures the three indicators

An operator runs the ingestion CLI for Eurostat. The adapter issues one request for the `demo_find` dataset filtered to three indicators at country level, parses the JSON-stat 2.0 response, fans the observations out to three statistics by their indicator code, resolves each country's alpha-2 geo code to a canonical region, and upserts `statistic_value` rows attributed to Eurostat. A second run against an unchanged upstream release writes nothing. A run after a republication supersedes the affected rows and inserts replacements, leaving the prior rows readable as audit trail.

### A flagged observation keeps its status

A cell Eurostat marks provisional is stored as provisional, one it marks estimated as estimated, and one carrying a concatenated flag takes the strongest status the flag names. The status travels from the canonical store through the shard into the client, and the region detail panel says so in a sentence. A flag character the model does not represent changes nothing about the value.

### A Europe-only layer reads as a Europe-only layer

A visitor selects mean age at childbirth. The map paints the countries Eurostat covers with visible variation between them and leaves the rest of the world in the no-data tone. The legend runs from the lowest covered value to the highest, with no inflection marker, because no age is a threshold. The value reads in years, the chart's axis title says years, and the history chart draws no reference line.

### Edge cases

- A country and year Eurostat has not published: the observation key is simply absent from the response, so no row is written and nothing warns. This is the normal state of nearly half the cells in the extraction: 5,001 values against 9,555 addressable cells, verified from a country-level capture whose `size` is `[1, 3, 49, 65]`.
- A cell carrying a flag but no value: verified to exist, one such cell in the phase-one extraction (Kosovo, total fertility rate, 2015, flagged provisional with no value). It must produce no observation. Iterating the flag map rather than the value map would invent one.
- A geo code naming a subset of a country rather than a country: `FX` (Metropolitan France) survives the country-level filter and would double-count France. It is excluded by name.
- A geo code Eurostat publishes that is not an ISO 3166-1 alpha-2 code: `EL` for Greece and `UK` for the United Kingdom are aliased. `XK` for Kosovo already matches the seeded code and needs no alias.
- A geo code matching no canonical region: one warning, no row, the run continues and exits zero.
- Both Eurostat and the World Bank supply the same country, statistic, and year: Eurostat wins the published cell. A year only the World Bank covers still emits, so a member state's series is not truncated at Eurostat's last period.
- Eurostat's `updated` timestamp carries a UTC offset written without a colon (`+0200`), which the RFC 3339 parser rejects. Verified: an explicit format string is required.
- A dimension's category positions are not the order the request asked for; Eurostat sorts the indicator codes alphabetically. Positions are read from the response, never inferred from the request.
- A request too large for a synchronous response: above 500,000 cells Eurostat answers asynchronously, and above 5,000,000 it returns a SOAP fault with `faultcode` 413 and `EXTRACTION_TOO_BIG`. Phase one's extraction is 9,555 cells, so the fault is out of reach, but a non-JSON body under a successful status must fail with an error naming the limit rather than being parsed as data.
- The two new statistics are absent from the embedded first-paint bundle, because that bundle is downsampled to World Bank values anchored on the United States. The statistic picker offers one statistic on first paint and three once the live bundle swaps in.

## Requirements

### Phase A, canonical vocabulary and a client that can draw a statistic measured in years

- **FR-001**: System MUST register Eurostat in `data_source` via a seed migration with `license_class='attribution'`, a `license_name` short enough to read as an anchor's text, a `license_url`, a `homepage_url`, an `attribution_text` acknowledging the source as Eurostat's reuse notice requires, and `preference_rank=40`. Lower wins, so 40 places it ahead of HFD's 50 and the World Bank's 100. No selection row is seeded: `source_choice` was dropped and resolution is per `(region, statistic, period)` cell by rank.
- **FR-002**: System MUST register two statistics via the same migration, both with `units='years'` and a `name_abbreviated_en`. The insert's column list is `(code, name_en, name_abbreviated_en, units)`; `description` no longer exists on the table, so the HFD seed migration is not a usable template for the column list.
- **FR-003**: `DataSourceKind` MUST gain a Eurostat variant with a `code()` equal to the migration's `data_source.code` and a matching `TryFrom<&str>` arm. The code string is a wire contract: it keys the manifest's source maps and is written into every shard row.
- **FR-004**: `StatisticKind` MUST gain a variant per new statistic, each reporting `TemporalBasis::Period`, with `code()` and `TryFrom<&str>` arms matching the seeded `statistic.code`. Variant declaration order is load-bearing: the derived `Ord` fixes the manifest's statistic key order and the picker's option order, and an existing artifact test asserts the first shard of a build is `tfr`. The new variants MUST sort after `Tfr`.
- **FR-005**: `DataStatus` MUST gain an `Estimated` variant with `as_str()` and `TryFrom<&str>` arms. Both directions are required for a round trip: the value is written as text into Postgres and into the shard, and parsed back at four sites including both target-gated shard readers. A shard carrying a status the reader cannot parse is rejected whole.
- **FR-006**: The migration MUST update the `statistic_value.data_status` catalog comment, which is the only artifact in the database enumerating the legal values. There is no CHECK constraint, no enum type, and no domain, so the enum is enforced only in Rust and the comment is the only thing that can go stale.
- **FR-007**: The per-statistic colour transform MUST NOT apply the replacement-pivoted curve to a statistic measured in years. That curve is keyed to absolute values and saturates above approx. 4, mapping every plausible mean age to within 0.2 % of one end of the ramp. A transform normalized against the statistic's observed range is what makes a Europe-only layer legible.
- **FR-008**: A statistic with no meaningful threshold MUST draw no reference line on the history chart, and the absent case MUST render nothing. Absence is currently unreachable and its fallback draws a dashed line coincident with the chart's baseline carrying an empty label, so this requirement is about making the branch correct rather than about selecting it.
- **FR-009**: A reference line MUST NOT force its value into the chart's vertical range for a statistic that has none. Including the replacement rate in the range of a series spanning approx. 25 to 32 years compresses the whole series into the top fifth of the plot, where it reads as a flat line.
- **FR-010**: The legend MUST NOT caption an inflection for a statistic that has none. `legend_scale` already filters the transform's inflection to the value range and renders the marker and its caption only when one survives, so this requirement is about making that branch correct rather than about selecting it.
- **FR-011**: The client MUST carry a label, a unit, and a definition for each new statistic, a label for the new source, and a sentence for the new status, all as locale strings. The unit a reader sees comes from the locale file: `statistic.units` in the canonical store reaches neither the manifest nor the shard and is read by nothing, so it is producer-side documentation and MUST NOT be treated as the reader's unit.
- **FR-012**: User-facing copy added by this feature MUST be a full sentence ending in a period where the existing neighbours are (the status sentences and the statistic definitions) and a bare fragment where they are (the unit strings, which render as a standalone line under a value and as a rotated axis title).
- **FR-013**: Phase A MUST leave the published artifacts unchanged. The two statistics have no values until Phase B, and the artifact build already logs and skips a statistic with no candidate values, so no shard and no manifest key appears. This is what makes Phase A reviewable without the adapter.

### Phase B, the Eurostat adapter

- **FR-014**: System MUST implement the `fetch_and_store(pool, options) -> Result<IngestReport, AppError>` adapter contract as the two shipped adapters implement it, `ingestion/src/world_bank_wdi/world_bank_wdi_adapter.rs` and `ingestion/src/hfd/hfd_adapter.rs`, which are the contract; `docs/architecture/ingestion.md` §Adapter contract is stale against them and is corrected in this feature rather than followed, split per the repository's per-source module convention: the client knows the wire format and nothing about the canonical store, the adapter converts to canonical rows and houses the orchestrator.
- **FR-015**: `fetch_upstream` MUST issue one request per run, through the shared HTTP client, against the dissemination API's data endpoint for the `demo_find` dataset with `format=JSON`, `lang=EN`, a country-level geo filter, and one indicator filter per requested indicator. `format=JSON` is the only permitted value and yields JSON-stat 2.0. The geo-level parameter and an explicit geo filter are mutually exclusive.
- **FR-016**: The client MUST NOT trust a successful status. A SOAP fault body, or any body that is not the expected JSON document, MUST fail with an error naming the dataset and what was received, in the same spirit as the existing archive guard that catches a login page served under status 200.
- **FR-017**: Parsing MUST be a pure function over the response with no I/O, and MUST resolve dimensions by name from the response's own dimension-order array rather than by position, and category positions from the response's own index maps rather than from the order the request asked for. Verified: Eurostat returns indicator positions in alphabetical order regardless of request order.
- **FR-018**: Parsing MUST derive each observation's dimension positions from the flat index by the row-major stride rule, where the last dimension varies fastest and a dimension's stride is the product of the sizes to its right. It MUST validate that each category index map is a bijection onto the positions its declared size allows, and fail if it is not. Verified to hold in every response captured; a silent break in it would mis-attribute every observation in the run.
- **FR-019**: Iteration MUST be driven by the observation map's keys. The observation map is sparse, is emitted in no particular key order, and never carries a null placeholder for an absent cell; the flag map shares its key space but is not a subset of it. Iterating the flag map, or the cartesian product of positions, is what produces a phantom observation.
- **FR-020**: The flag map MUST be absent-tolerant. A response in which no cell is flagged omits the key entirely, verified against a single-cell response.
- **FR-021**: System MUST derive the publication's `revision_label` from the response's top-level `updated` string and `data_source_publication.published` from that string parsed to an instant. Verified: the offset is written without a colon, so an explicit format is required and the RFC 3339 parser fails. This is Eurostat's only revision signal; the `revision_label` column comment's suggestion of a week-numbered form for Eurostat is a guess that predates the wire format and MUST be corrected rather than synthesized toward.
- **FR-022**: System MUST skip writing when the parsed revision label matches the newest publication already recorded for Eurostat, and `options.force_full_refetch` MUST override the skip. Eurostat's revision label is inside the response, so the request is unavoidable and only the write is skippable. This makes Eurostat the second consumer of the same skip decision HFD introduced, so that decision MUST move to the shared adapter module rather than be copied.
- **FR-023**: System MUST parse the observation flag as a set of characters and map it to a `DataStatus` by precedence: forecast beats imputed beats estimated beats provisional. The break-in-series, definition-differs, not-significant, low-reliability, and missing-cannot-exist qualifiers are not modelled and are dropped; a cell carrying a value alongside one of them still stores that value. A flag naming only dropped qualifiers, and an absent flag, both yield `final`. The mapping MUST NOT depend on the per-response codelist label block, which carries only the codes that response happened to use and is a display aid.
- **FR-024**: The flag-to-status mapping MUST live in the adapter, not the client. The client may not name `DataStatus`, which is a canonical type; the flag crosses the boundary as an opaque string on the parsed value.
- **FR-025**: System MUST resolve each observation's geo code to a canonical region through a new alpha-2 country lookup. No such lookup exists; the four generic lookups are by ISO 3166-1 alpha-3, region code, statistic code, and source kind. The column is present, unique, and populated for every seeded country.
- **FR-026**: System MUST alias the two geo codes Eurostat publishes that are not ISO 3166-1 alpha-2 (`EL` to Greece's code, `UK` to the United Kingdom's) in one const table pairing each code with its target, following the existing HFD alias table's shape.
- **FR-027**: System MUST exclude by name the geo codes that survive the country-level filter without naming a country. Verified: the filter narrows the geo dimension from 58 positions to 49, dropping the aggregate codes and the whole-Germany code, and keeps `FX` (Metropolitan France), which is a subset of France and would double-count it. An excluded code MUST NOT warn; it is a known non-region rather than an unrecognized one. Any other unresolvable code MUST warn.
- **FR-028**: System MUST fan the parsed observations out to three statistics by their indicator code, resolving each statistic through `StatisticKind` rather than a bare code string. One response carries all three indicators, so the indicator code is a real varying dimension and must be carried on each parsed observation.
- **FR-029**: System MUST encode each observation's period as the calendar year it names, through the existing single-year period constructor.
- **FR-030**: System MUST surface non-fatal upstream quirks as warnings on the report rather than as errors: an unresolvable geo code, and a geo code that yielded no values at all. An absent observation MUST NOT warn; over half the addressable cells are absent and warning per cell would bury the warnings that matter.
- **FR-031**: System MUST provide checked-in sample responses under `ingestion/samples/eurostat/` covering the phase-one extraction, a small multi-dimensional slice whose every cell can be asserted by hand, and a response with no flagged cell. Samples MUST be replayable without network access, and MUST be captured from a live response rather than hand-written, because a JSON-stat document edited by hand can be internally inconsistent in a way a parser test cannot detect.
- **FR-032**: System MUST cover parsing, the flag precedence, and normalization with tests written before the implementation, per Constitution Principle VII.
- **FR-033**: System MUST wire Eurostat into the CLI's single-source dispatch and the all-sources orchestration, and MUST update the source argument's help text, which names the registered codes and is not compiler-checked.
- **FR-034**: The first publish carrying Eurostat's source keys and the two new statistic keys MUST be legible to a client older than it. The manifest's source and statistic maps are keyed on string-coded enums whose deserializer fails the whole document on an unrecognized key, and a same-version parse failure has no fallback: such a client keeps painting from its cached bundle and never updates again, indistinguishable from a healthy one. A manifest schema version bump makes the mismatch legible and routes the client to the version-pointed manifest it can read. See §Open questions for the decision this requirement depends on.
- **FR-035**: The embedded first-paint tree MUST be re-synced in the same change as any manifest schema version bump. It is parsed by the same reader, so a bump against a stale embedded tree leaves first paint with no bundle at all.

### Key entities

- **Eurostat observation**: one (indicator, geo, year) cell from the dataset, with its optional flag. Maps to one `statistic_value` row.
- **Eurostat publication**: one upstream release, labelled by the response's `updated` timestamp. Maps to one `data_source_publication` row.
- **Eurostat source registration**: one `data_source` row at `preference_rank` 40, created by seed migration.
- **Observation flag**: a string whose characters are a set of quality qualifiers, mapped to one `DataStatus` by precedence.
- **Mean age of women at childbirth**: one `statistic` row measured in years, the average age of women giving birth in a year across all births.
- **Mean age of women at first birth**: one `statistic` row measured in years, the same average restricted to first births.

## Success criteria

- **SC-001**: A run against the checked-in samples writes the expected `statistic_value` rows, and a second run against the same samples writes nothing.
- **SC-002**: A revised sample supersedes the affected rows and inserts replacements, leaving the superseded rows readable with their original values, statuses, and publications.
- **SC-003**: The parsed observation count equals the response's observation count, not its flag count, for a sample containing a flagged cell with no value.
- **SC-004**: Every atomic flag character, at least three concatenations, an absent flag, and a flag of only unmodelled qualifiers each map to the status the precedence rule names.
- **SC-005**: The two aliased geo codes resolve to their countries, the excluded code produces no row and no warning, and an unresolvable code produces one warning, no rows, and a zero exit status.
- **SC-006**: Each indicator's observations land on their own statistic, verified by three separate statistic lookups.
- **SC-007**: For a country and year both Eurostat and the World Bank supply, the published cell names Eurostat; a year only the World Bank covers still emits, so the series has no gap at Eurostat's boundary.
- **SC-008**: Selecting a mean-age statistic paints the covered countries in more than one distinguishable tone, and the legend bar is a gradient rather than a single tone.
- **SC-009**: A mean-age statistic's history chart draws no reference line, the legend shows no inflection caption, the value and the chart axis read in years, and the series occupies the plot rather than the top fifth of it.
- **SC-010**: A cell whose status is estimated is distinguishable in the region detail panel from a final one, and the shard round-trips the status.
- **SC-011**: A run against an unchanged upstream release writes nothing and reports the run as having found no change.
- **SC-012**: A client build older than the first bundle carrying Eurostat's keys either loads a bundle it can read or reports a legible version mismatch. It does not silently keep a stale bundle while appearing healthy.

## What Eurostat asks of a republisher

Eurostat's copyright and reuse notice authorizes reuse of statistical data for commercial and non-commercial purposes provided the source is acknowledged, and licenses the site's editorial content under CC BY 4.0. `docs/research/data-source-licensing.md` §Eurostat quotes both, and records that the exceptions are third-party material, logos and trademarks, and certain trade datasets sourced from outside the EU and EFTA. Fertility data is Eurostat-produced and falls under the default terms.

The acknowledgement is the whole of the obligation, and the existing design satisfies it: the attribution string, the licence name, the licence URL, and the homepage from the `data_source` row are carried into the manifest's per-source attribution map and rendered in the region detail panel beside the source's own values. That path was not available when HFD landed and is available now, so Eurostat's attribution reaches a reader without a producer change.

Two smaller points, both accommodated rather than requiring work. The survey notes Eurostat's API rate limits are undocumented and recommends bulk download over repeated API calls; one request per run, with a skip on an unchanged revision, is well inside any plausible limit and is why the dataset filter is applied server-side rather than by fetching the whole database. The survey also notes that Eurostat flags provisional data and that a consumer must read the status metadata to distinguish it from final, which is what FR-023 does.

## Assumptions

Facts verified live against the API and against captured responses, not assumed:

- The dataset holds fertility indicators at country level over 1960 to 2024, with dimensions for frequency, indicator, geo, and time, and no unit dimension.
- The three phase-one indicator codes exist in it, alongside nine others out of scope.
- `format=JSON` is the only permitted format value and yields JSON-stat 2.0.
- The geo-level parameter accepts an aggregate, country, and three NUTS levels, and is mutually exclusive with a geo filter.
- Under the country-level filter the geo dimension narrows from 58 to 49 positions, dropping the aggregates and the whole-Germany code and keeping `FX`, `EL`, `UK`, and `XK`.
- The observation map is an object keyed by stringified flat index, sparse, unordered, and never null-valued, even for a fully dense slice. Zero nulls across all 17,906 observations of the whole dataset.
- The flag map shares that key space, is omitted when nothing is flagged, and can key a cell the observation map does not.
- Eight of the codelist's 42 flag codes occur in this dataset, with a maximum length of three characters. Seven occur for the three phase-one indicators.
- The forecast character does not occur anywhere in this dataset, so that branch of the precedence rule is exercised only by a synthetic test input.
- The `updated` timestamp's offset is written without a colon.
- Category index maps are bijections onto their declared sizes in every response captured.

Assumptions proper:

- Automated access is permitted. The API is a documented public dissemination service with no credential, and the reuse notice speaks to redistribution and acknowledgement rather than to retrieval.
- The forecast character maps to the existing projection status. It is the only sensible fit among the modelled statuses, and it is an inference rather than a settled decision (see §Open questions).
- Every phase-one observation is an annual period, so the existing single-year period constructor is sufficient and no multi-year constructor is added.
- Coverage is EU, EFTA and candidate countries, so both new statistics render as Europe-only layers by nature rather than by an ingestion gap.
- Two decimal places on a displayed value are inherited from the fertility statistics and are a presentation choice for an age, not a defect (see §Open questions).

## Scope cutoff

Out of scope, each for a stated reason:

- **Subnational regions and NUTS-level values.** The region table holds only the world, five UN M49 regions, their subregions, and countries; the level column's comment already reserves subnational levels that nothing populates. Adding them is a region-model change with a geometry consequence, and it is the substance of the later phases sketched in the plan rather than a detail of this one.
- **The other nine indicators in the dataset.** Median age at childbirth, mean age at second through fourth-and-later births, births outside marriage as a percentage, and live births by birth order as percentages all fit the existing cell shape and are a small follow-up once the two new statistics have proven the years-valued presentation path. Nothing in this feature blocks them.
- **Age-specific fertility rates and births by mother's age or parity.** A value per region, period, and age, which the shard's cell shape does not express and a choropleth cannot draw.
- **The confidentiality attribute.** Its three codes have no counterpart among the modelled statuses, and conflating confidentiality with data quality would mislabel a cell. Deciding what a confidential cell means for a published artifact is a separate question from what a provisional one means.
- **Eurostat's quarterly and monthly provisional series.** A different cadence of ingestion, and provisional by nature; it wants the status work in this feature landed first.
- **The freshness override in the documented merge rule.** `docs/architecture/ingestion.md` §Merge rule step 4 says a provisional value from the preferred source should yield to a recent final value from a lower-priority one. Nothing implements it; resolution is by rank alone, in the shard's rank table and on the client's read. Eurostat is the first source that makes the clause reachable, since a Eurostat provisional figure will now outrank a World Bank final one for the same year. Implementing or retiring the clause is a resolution change touching every source, not a Eurostat change (see §Open questions).

## Decisions taken

Settled while reviewing the draft. Each was an open question; the reasoning is recorded so any of them can be reversed on its merits.

1. **Statistic codes and variant names.** Codes `mean_age_at_childbirth` and `mean_age_at_first_birth`; variants `MeanAgeAtChildbirth` and `MeanAgeAtFirstBirth`. Verbose, because neither has an established acronym the way TFR and CCF do, and the naming convention prefers spelled-out words over invented abbreviations.
2. **One `data_source` row for the publisher, `eurostat`.** `preference_rank` is a judgment about a publisher's authority, not about one of its tables, and per-dataset rows would duplicate the attribution and the licence across every later dataset. A later dataset needing its own rank is a later migration. The schema's own example, which names a dataset, is corrected in Phase A.
3. **The licence.** `license_name` is `Commission Decision 2011/833/EU`, `license_url` is Eurostat's copyright-notice page, which is where the terms are actually stated, and `attribution_text` credits the European Union as the rights holder with the reuse instrument named.
4. **`MANIFEST_SCHEMA_VERSION` bumps; the shard `SCHEMA_VERSION` does not.** An older deployed client fails during the manifest parse on an unknown source key, which is indistinguishable from a healthy client; the backtracking machinery exists precisely so it falls back to a manifest it can read instead. The shard's shape is unchanged, since `estimated` is a new value in an existing text column.
5. **Colour polarity for the mean-age statistics: most saturated at the highest value.** The ramp's saturated end marks the fertility-notable extreme, which for a mean age is later childbearing rather than earlier. The doc comment states the direction per statistic instead of asserting one direction for the scale.
6. **The chart's reference stays its own decision.** It is an editorial choice about a statistic; the transform's inflection is a scale parameter. The duplicated replacement-rate constant is worth removing on its own, so it lives once in `shared` and both sites read it, but the chart does not learn its reference from the colour scale.
7. **Displayed precision is per statistic:** one decimal for an age, two for a fertility rate. Eurostat publishes ages to one decimal, and rendering 28.40 asserts precision the source does not carry.
8. **The picker names a Europe-only statistic's coverage.** Selecting one repaints the world in the no-data tone, which reads as a broken map otherwise. The coverage note is a locale string per statistic, absent for the global ones.
9. **The downsampled bundle carries all three statistics.** The reference-year filter is extended to keep one year per statistic rather than only the World Bank's United States anchor, so the picker offers the same three statistics before and after the live swap. A picker that changes shape mid-load is the same class of defect as geometry that changes under a held-open page.
10. **The forecast character maps to `Projection`.** Unexercised by this dataset, so its test is synthetic.
11. **A dropped qualifier is counted in the report.** Sixty-four break-in-series entries is a fact about comparability worth surfacing in aggregate; per-cell warnings would drown the log.
12. **The documented freshness override is removed rather than implemented.** Rank alone decides, which is what rank 40 was chosen against. A documented behaviour nothing implements is worse than no documentation.

## Open questions

One fact no probe could establish, gating the geometry phase.

1. **Whether a single boundary source covers every NUTS region ingested.** EuroGlobalMap's NUTS layer is optional per country in its specification, so coverage is settled only by downloading the file and looking. Phase D depends on it; the rest of that phase's design is settled in `docs/architecture/geometry.md`.

## Constitution check

- **Principle II, source provenance**: directly served, and advanced. Eurostat is the first source whose published quality flag reaches the reader, and the first whose full attribution string is rendered from the bundle rather than named by label alone.
- **Principle I, educational neutrality**: the two new statistics are presented with their definitions, their unit, and their coverage. No copy asserts what a mean age at first birth should be, which is also why no reference line is drawn for it.
- **Principle III, Rust core**: parsing, the flag mapping, and normalization live in `ingestion`; the canonical vocabulary lives in `shared`; the client changes are per-statistic presentation decisions expressed in Rust and in locale strings.
- **Principle VI, CDN-delivered data**: unchanged. Eurostat is ingested into the canonical store and published as artifacts like any other source.
- **Principle VII, test-first**: FR-032 covers parsing, the flag precedence, and normalization. The presentation changes are shell code and are exempt from strict test-first, but the chart's scale is arithmetic and is tested.
- **Principle IV, dependency discipline and convention specs**: no new third-party dependency; the wire types follow the existing per-source model shape. Applicable convention docs: `docs/conventions/types.md` governs the three new enum variants and the new wire, parsed, alias and outcome types, and `docs/conventions/logging.md` governs every new log and error string. `docs/conventions/conditional-compilation.md` and `docs/conventions/shading.md` do not apply, since nothing is target-gated and no WGSL changes.
- **Principle V, explicit over implicit**: the flag precedence is one readable ordered table rather than nested conditionals, and the skip decision moves to a shared function rather than being copied into a second adapter.
