# Feature Specification: Human Fertility Database ingestion, completed cohort fertility

**Feature Branch**: `hfd-ingestion`

**Created**: 2026-08-19

**Status**: Draft

**Input**: User description: "add data integrations, aimed at more statistics; HFD first, then Gapminder, Eurostat, OECD. Completed cohort fertility first, then the cohort change. The scrubber should not show one instant for a cohort even if the data is internally designed that way; it should highlight the entire range."

## Why this source, and why this statistic

The Human Fertility Database is the second data source and the first one that adds a statistic rather than more coverage of an existing one. `docs/data/sources-survey.md` §Indicator coverage matrix marks completed cohort fertility and tempo-adjusted TFR as unavailable from UN WPP and the World Bank and available from HFD; both are statistics the product names, and neither is reachable without this source. HFD's user agreement settles the licensing directly: the estimates HFD constructs are CC BY 4.0, while "this is not the case for the input data which remains under each provider's distribution license", and that input data "should not be used for commercial gain or re-published in any form without the explicit permission of the data owners (usually national statistical agencies)". So the outputs are ingestible and the inputs are not, and the distinction is a licence boundary rather than a scoping preference.

HFD is deliberately narrow. It covers roughly 38 developed countries against the World Bank's 180-plus, recomputing every indicator from national birth registers on one method so figures are comparable across countries and decades. It is depth where World Bank WDI is breadth, which is why the two coexist rather than compete: the source-preference merge picks one source per cell, and this feature is the first thing that exercises it.

## Scenarios

### Capturing completed cohort fertility

An operator runs the ingestion CLI for HFD. The adapter reads the cohort summary file for each country in scope, normalizes each (country, birth cohort, value) triple to the canonical schema, and upserts `statistic_value` rows attributed to HFD. A second run against unchanged upstream files writes nothing. A run against a revised file supersedes the affected rows and inserts replacements, leaving the prior rows as audit trail.

### Presenting a cohort without misstating it

A visitor selects completed cohort fertility. The period axis is labelled for a birth cohort rather than a calendar year, and the active value is drawn as the span the cohort covers rather than as a single instant. Selecting the statistic moves the available range back by decades, because a cohort's fertility is only complete once its members have finished childbearing; the interface presents that as the nature of the measure, not as stale data.

### Reading provenance off a cell

A visitor selects a region and sees which source supplied the value and whether it is final. The status travels from the canonical store through the shard to the client, so a provisional figure is never shown as a confirmed one.

### Edge cases

- A cohort has not completed childbearing, so HFD publishes no value: no row is written, and the region renders as no-data for that cohort.
- HFD's file carries a supplementary measure computed by age 40 alongside the completed measure: the age-40 variant is a different definition, not a less-final version of the same one, and is out of scope rather than stored with a different `data_status`.
- An HFD code names a subpopulation rather than a country (`DEUTE`, `DEUTW`, `GBRTENW`, `GBR_SCO`, `GBR_NIR`): no canonical region matches, and the run records a warning and continues.
- A value is missing, which HFD encodes as a dot: the cell is skipped with a warning rather than parsed as zero.
- Both HFD and World Bank WDI supply a cell for the same region, statistic, and period: the source-preference merge selects one, and the series never mixes sources.
- The upstream file's header row differs from the expected columns: the run fails with an error naming the file and the columns found, rather than reading a column by position.
- The download returns the registration page because the account has not accepted the agreement: the run fails with an error saying so, rather than parsing HTML as data.
- Upstream is unchanged since the last run: nothing is downloaded past the point that becomes knowable, and the report shows a run that wrote nothing.

## Requirements

### Phase A, ingestion

- **FR-001**: System MUST implement the `fetch_and_store(pool, options) -> Result<IngestReport, AppError>` adapter contract for HFD exactly as `docs/architecture/ingestion.md` §Adapter contract specifies, with the client and adapter split per the repository's per-source module convention: `hfd_client.rs` knows the wire format and nothing about the canonical store, `hfd_adapter.rs` converts to canonical rows and houses the orchestrator.
- **FR-001a**: `fetch_upstream` MUST authenticate before downloading. HFD serves its download URLs only to a logged-in account that has accepted the user agreement; an unauthenticated request returns the registration page with status 200, so the client MUST detect a non-archive response rather than trusting the status code.
- **FR-001b**: Credentials MUST come from `HFD_USERNAME` in the environment and `hfd.siegelzc.password` through `secr`, read at the point of use per `docs/architecture/secrets.md`. Neither may be logged, and a failed login MUST report that it failed without echoing either.
- **FR-001c**: System MUST download `tfr.zip`, the by-statistic archive holding period and cohort TFR for every country, rather than a per-country archive. One request covers roughly 38 countries.
- **FR-001d**: System MUST extract the archive in memory using the existing `zip` dependency, and MUST read only `XXXtfrVH.txt` members from it.
- **FR-001e**: Login failure, an unaccepted agreement, and an archive that cannot be parsed MUST be errors that stop the run, not warnings. They are configuration faults rather than data quirks.
- **FR-001f**: System MUST skip work when upstream has not changed: compare the last-modification date each file declares against the newest `data_source_publication.revision_label` for HFD, via the contract's `read_latest_publication_revision`, and write nothing for a file whose date is unchanged. `options.force_full_refetch` MUST override this.
- **FR-001g**: A run MUST make one login and one archive request. Re-fetching per country, or re-downloading an archive already known to be unchanged, is what HFD's request not to circulate stale copies asks us to avoid on their side as well as ours.
- **FR-002**: System MUST read completed cohort fertility from HFD's cohort summary file, `XXXtfrVH.txt`, where `XXX` is the HFD country code. The by-birth-order companion (`XXXtfrVHbo.txt`) is out of scope. Note for any later feature that ingests exposure-to-risk or fertility tables: the agreement's prescribed citation applies by its own terms to those, and would then have to be rendered verbatim, download date included.
- **FR-003**: System MUST parse HFD's output format as documented in HFD's own `formats.pdf`: space-delimited ASCII, two informational lines followed by the column header on the third line, and missing values encoded as a dot. Parsing MUST be a pure function over bytes, with no I/O.
- **FR-004**: System MUST resolve columns by header name read from the third line, never by ordinal position, so an upstream column addition or reordering cannot silently shift which value is stored.
- **FR-005**: System MUST derive the publication's `revision_label` from the last-modification date the file's second line carries, so every captured publication has a label taken from the upstream artifact rather than synthesized.
- **FR-006**: System MUST map HFD country codes to canonical regions via ISO 3166-1 alpha-3 where the code is a plain country code, and MUST treat a code that resolves to no canonical region as a warning that does not stop the run. HFD's national-total codes that are not bare alpha-3 (`FRATNP`, `DEUTNP`, `GBR_NP`) MUST map to their countries; its subpopulation codes MUST NOT.
- **FR-007**: System MUST encode a single-year birth cohort as `period_start` = 1 January of the cohort year and `period_end` = 1 January of the following year, per the encoding `statistic_value.period_start`'s column comment already documents for cohorts.
- **FR-008**: System MUST register HFD in `data_source` via a seed migration with `code='hfd'`, `license_class='attribution'`, `license_name='CC BY 4.0'`, and a `preference_rank` that wins over World Bank WDI for the cells both supply, because HFD recomputes from national registers on one method.
- **FR-008a**: The attribution MUST name HFD as the source or intermediary provider, per CC BY 4.0 and the agreement's request to "acknowledge the HFD as either the source or the intermediary provider of the data", identifying the Max Planck Institute for Demographic Research, the Vienna Institute of Demography, and `www.humanfertility.org`.
- **FR-008c**: System MUST record the date each file was downloaded, which the agreement asks for as "note the date for future reference". `data_source_publication.fetched` already satisfies this; the requirement exists so a later change does not drop it.
- **FR-008b**: System MUST NOT read, store, or republish any file HFD publishes under Input Data on its country pages. FR-002 names one output file; this requirement states the reason, which is that the inputs carry each provider's own licence and forbid commercial use and republication without the owner's permission.
- **FR-009**: System MUST register the statistic via a seed migration with `code='ccf'` and a name of "Completed cohort fertility". The code MUST NOT be `cfr`: HFD distributes a `cfr.zip` holding cumulative fertility rates, a different measure, and the collision would mislead every later reader.
- **FR-010**: System MUST add a nullable `released timestamp with time zone` column to `statistic`, and `read_all_statistic_kinds` MUST consider only released statistics. The column follows the schema's existing idiom for state changes, nullable timestamps rather than booleans as with `statistic_value.superseded`, and carries when the statistic began being offered for the same storage. It is named `released` rather than `published` because `data_source_publication.published` already means the upstream's own publication date.
- **FR-010a**: A statistic MUST NOT need a `StatisticKind` variant until it is released. Ingestion resolves statistics by `statistic.code`, so Phase A can land its seed row, its adapter, and real rows in `statistic_value` with no client knowledge of the statistic at all. This is what decouples the migration from the enum.
- **FR-010b**: A released statistic whose code has no `StatisticKind` variant MUST remain a hard error rather than being skipped with a warning. Releasing a statistic no client can render is a misconfiguration, and failing the artifact build is how it gets noticed; silently omitting it would publish a bundle missing a statistic someone expected.
- **FR-010c**: The existing `tfr` statistic MUST be released by the same migration that adds the column, so behaviour is unchanged for what already ships.
- **FR-011**: System MUST set `data_status` on every row it writes, using `final` for HFD's completed measure.
- **FR-012**: System MUST surface non-fatal upstream quirks as `IngestWarning` values on the report rather than as errors: an unmapped country code, a dotted value, a cohort with no value.
- **FR-013**: System MUST provide checked-in sample files under `ingestion/samples/hfd/` covering a happy path, a dotted missing value, an unmapped subpopulation code, and a cohort absent from the file. Samples MUST be replayable without network access.
- **FR-014**: System MUST cover parsing and normalization with tests written before the implementation, per Constitution Principle VII.
- **FR-015**: System MUST wire HFD into the CLI's `source` dispatch and the `all` orchestration.

### Phase B, presenting a cohort

Phase B ends by releasing the statistic. Until then Phase A's data sits in the canonical store, ingested and verifiable, without reaching a client.

- **FR-016**: The shard read path MUST carry `period_end` and `data_status` into the client's cell values. Both columns are already written into every shard and are discarded by the current five-column read; the range and the status the following requirements need come from the same change.
- **FR-017**: `StatisticKind` MUST distinguish a period measure from a cohort measure, and the period axis MUST take its label from that distinction rather than from a fixed string. The existing per-statistic colour transform is the precedent for varying presentation by statistic.
- **FR-018**: The scrubber MUST represent the active value as the span from `period_start` to `period_end` rather than as a single instant, for every statistic. An annual period renders as a one-year span and a multi-year cohort as a span of its width, so a five-year cohort is never drawn as a dot at its first year.
- **FR-019**: The region detail panel MUST show a cell's `data_status` when it is anything other than final, in the design's existing vocabulary and without decoration.
- **FR-020a**: The migration releasing `ccf` MUST be the last change in Phase B, so the statistic becomes visible only once the axis, the span, and the citation render correctly.
- **FR-020**: The client MUST show the attribution FR-008a describes for a cell attributed to HFD. Rendering the download date alongside it is optional: the agreement's citation form covers exposure-to-risk, death rates, and life tables rather than fertility indicators, and its date instruction asks only that the date be noted. The manifest carries `fetched` per source, so displaying it costs nothing if wanted, and it does make a stale copy identifiable.

### Key entities

- **HFD cohort datum**: one (country code, birth cohort, value) triple from a cohort summary file. Maps to one `statistic_value` row.
- **HFD publication**: one upstream release of a country's files, labelled by the last-modification date the file declares. Maps to one `data_source_publication` row.
- **HFD source registration**: one `data_source` row with `code='hfd'`, created by seed migration.
- **Completed cohort fertility**: one `statistic` row with `code='ccf'`, the number of children born to a woman of a given birth cohort by the end of her childbearing years.

## Success criteria

- **SC-001**: A run against the checked-in samples writes the expected `statistic_value` rows, and a second run against the same samples writes nothing.
- **SC-002**: A revised sample file supersedes the affected rows and inserts replacements, leaving the superseded rows readable with their original values, statuses, and publications.
- **SC-003**: For a region and cohort that both HFD and World Bank WDI cover, the published shard carries exactly one source for the cell, and it is the preferred one.
- **SC-004**: Selecting completed cohort fertility labels the axis for a birth cohort, and the active value reads as a span rather than an instant.
- **SC-005**: A cell whose status is not final is distinguishable in the region detail panel from one that is.
- **SC-006**: An unmapped subpopulation code produces a warning on the report and no rows, and the run's exit status stays zero.
- **SC-007**: A run against unchanged upstream downloads the archive once, writes nothing, and reports the run as having found no change.
- **SC-008**: A run with a wrong password fails with an error that names the login step and contains neither the username nor the password.

## What HFD asks of a republisher

HFD's agreement closes with a request rather than a licence term: "the preferable option is not to pass a copy of these data to other users. Instead, refer them to the HFD website, where they may download the data for themselves." Its stated reasons are to stop outdated or incorrect copies circulating, and to keep each user in reach of the source, processing, and citation documentation.

Eafora is an aggregator that republishes values through a CDN, so this is worth answering rather than skipping. CC BY 4.0 permits the redistribution, and the phrasing is a courtesy request, not a condition. The intent is nonetheless honourable within the existing design, and these are the ways it is honoured:

- Every published cell already carries its source and that source's revision, so a value is traceable to the HFD release it came from rather than floating free.
- The download date is recorded per publication, and the manifest carries it, so a published value can be dated even though the agreement asks only that the date be noted rather than shown.
- Re-ingesting on HFD's update cadence keeps the published copy current, which is the substance of the concern. A frozen copy would be the thing the request warns against.
- Attribution links to HFD, where the documentation the request wants users to reach lives.

## Assumptions

- HFD's estimates are CC BY 4.0 and its input files are not, per the user agreement quoted above. FR-002 reads a single output file and FR-008b forbids the inputs outright.
- Automated download appears permitted: HFD's `robots.txt` disallows only `/cgi-bin`, and the agreement speaks to redistribution and citation rather than to how files are retrieved. The account gate is enforced per user, confirmed by an unauthenticated request to `/File/Download/Files/zip/tfr.zip` returning the registration page.
- The login form's fields are not recorded here because they have not been read from the live form. The site is ASP.NET-shaped (`/Account/Auth`, `/Account/Login`), so an antiforgery token is likely and the plan must record what the form actually requires.
- Session handling needs a decision: `reqwest` is configured with `json` and `rustls-tls` only, so it has no cookie store. Either enable its `cookies` feature or carry the session cookie explicitly across the two requests. The second adds no dependency and keeps the exchange visible, which suits a two-request flow.
- The exact column headers of `XXXtfrVH.txt` are not recorded here because FR-004 requires reading them from the file. The plan should record them once a real file is in hand.
- HFD's cohort summary is published by single birth cohort. If a country's file uses multi-year cohorts, FR-007's encoding covers it and FR-018 is what keeps the presentation honest.
- Countries in scope are those whose HFD code maps to a canonical region under FR-006. The set is expected to be roughly 30 of HFD's 38 codes, the remainder being subpopulations.

## Scope cutoff

Out of scope, each for a stated reason:

- **Age-specific fertility rates.** A value per (region, period, age), and per birth order beyond that, which the shard's cell shape does not express and a choropleth cannot draw. It needs the region detail view's charts, which do not exist. Deferred until they do.
- **Tempo-adjusted TFR.** Fits the existing cell shape and needs neither of Phase B's changes, so it is a small follow-up rather than part of this feature. `XXXadjtfrRR.txt` is the file.
- **HFD's Short-Term Fertility Fluctuations series.** Monthly and quarterly counts at a short lag, and the fastest data any source in the survey offers. It is provisional by nature, so it wants FR-019 landed first, and it is a separate cadence of ingestion.
- **HFD subpopulations.** East and West Germany and the UK constituents have no canonical region. Adding them is a region-hierarchy decision, not an ingestion one.

## Constitution check

- **Principle II, source provenance**: directly served. Every row carries its source, publication, and status, and Phase B is what stops the client from dropping the last of those.
- **Principle III, Rust core**: parsing and normalization live in `ingestion`, the canonical schema in `shared`; the client changes read existing columns and add no logic of their own.
- **Principle VI, CDN-delivered data**: unchanged. HFD is ingested into the canonical store and published as artifacts like any other source.
- **Principle VII, test-first**: FR-014 covers parsing and normalization. Phase B's axis and span behaviour is target-agnostic and testable on the host.
- **Principle I, educational neutrality**: the statistic is presented with its definition and its date range; the reason completed fertility lags by decades is a property of the measure and is surfaced as such.
