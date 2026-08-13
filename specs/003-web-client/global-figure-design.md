# Global figure in the detail panel's empty state

## Problem

When no region is selected, the detail panel is blank. Fill it with the world-aggregate figure for the active statistic and year (see `docs/backlog.md`). World Bank WDI publishes a "World" aggregate for the TFR indicator in the same `country/all` response the client already fetches, but the WDI client drops every aggregate row and the canonical store models only countries, so the World figure is fetched then discarded today.

## Phase 0 (prerequisite refactor): unify the region key on `region.code`

The shard row key and the geometry-to-shard join key are named `iso3` / `region_iso3`, but they are semantically a region key: the client only does string equality on them (`renderer.rs`, `driver.rs`), never parsing ISO 3166-1. For every country the seed derives `region.code = alpha_3.to_lowercase()` (and `country.iso3 = alpha_3`), so `region.code` and the value-join key are the same identifier in different case. Keying the shard by `country.iso3` (uppercase) was only a shortcut that held while every region was a country; the canonical key was `region.code` all along. The `world` region (a `region.code`, no `iso3`) makes carrying both untenable, and an `iso3`-overuse audit confirmed the misnomer is confined to this one chain.

Change: use the canonical lowercase `region.code` as the single region key for both selection and the value lookup, and drop the `iso3`-as-key. `iso3` survives only as the genuine `Country.iso3` attribute (World Bank ingest, Natural Earth `canonical_iso3`, `find_country_by_iso3`, the `country.iso3` column and its migrations), all unchanged.

Sites (from the audit):

- Ingestion value records: `CandidateValue` / `CandidateValueProjection` / `ResolvedValue.region_iso3` become `region_code`; the shard-build SQL keys by `region.code` (join `statistic_value` to `region` instead of to `country`), which also lets non-country regions into the shard.
- Shard: `schema.rs` `COL_REGION_ISO3 = "region_iso3"` becomes `COL_REGION_CODE = "region_code"`; the `sqlite.rs` writer bind; the `ShardValues` API (`value` / `cell` params, `read_shard` locals) keyed by `region_code`. The `by_region` field name is already correct.
- Render / hit-test carriers: drop `CountrySpan.iso3`, `CountryMesh.iso3`, `RegionHit.iso3`, `SelectionView.iso3`. The renderer's value lookup uses `span.region_code`, which it already carries for selection; the web driver's `resolve_selection_view` takes `region_code`.
- FlatGeobuf feature: `region_code` is already the join key (`FEATURE_COLUMN_REGION_CODE`); `FEATURE_COLUMN_ISO3` becomes vestigial and is dropped. Verify nothing displays `iso3` before dropping it (the audit found it is key-only).

This is behavior-preserving (the map colors the same regions; selection and hover are unchanged), but the shard key values change from uppercase ISO3 to lowercase slugs, so the producer and consumer ship together and the embedded artifact is rebuilt. Standalone PR.

## Canonical model (Phase 1)

Add one supranational region via a migration: `code = 'world'`, `name_en = 'World'`, `level = 'world'` (a new level value), `parent_region_id = null` (standalone, not wired as parent of the five M49 regions), `m49_code = '001'` (UN M49's code for World). No `country` row, no geometry.

## Ingestion (Phase 1)

The World row arrives as `countryiso3code = "WLD"`: it passes the empty-code parse filter, then is dropped in `normalize` today as an unknown country. Special-case `WLD` so it resolves to the world region and its per-year TFR is stored as a normal `statistic_value`. Every other aggregate keeps being dropped, unchanged. `WLD` stays confined to the WDI adapter; downstream everything uses the canonical `world`.

## Artifact (Phase 1)

With the shard keyed by `region.code` via the region join (Phase 0), the world region is included automatically once it has values, keyed `world`. The shard is per-year, so the World figure tracks the year scrubber like any region. No further shard change is needed. World has no geometry, so the map never draws it.

## Web (Phase 2)

When the selection context is `None`, the driver publishes a World view-model (the `world` value for the active statistic and year, read from the shard the client already loads), and `RegionDetailPanel` renders it as default content, for example "World · Total fertility rate · 2024 · 2.3" with the same source attribution, instead of being blank. Use a dedicated default view-model rather than overloading `SelectionView`, so the panel distinguishes "nothing selected, showing World" from "a region is selected."

## Delivery

Three stacked PRs, ingestion before clients:

1. Phase 0: unify the region key on `region.code` (standalone refactor; regenerate the embedded artifact).
2. Phase 1: seed the world region and keep `WLD` in the WDI adapter (canonical, ingestion, artifact).
3. Phase 2: render the World figure in the detail panel's empty state (web).

## Testing

- Phase 0: existing shard, render, hit-test, and artifact tests move to `region_code`; the map renders identically (host `cargo test` plus `cargo check` for wasm and ssr).
- Phase 1: a WDI test asserts `WLD` resolves to the world region; the existing "unknown country still dropped" test stays green; the shard integration test asserts a `world` entry.
- Phase 2: the empty state renders the World value (component test or manual verification in Chrome).

## Rejected alternatives

- A dedicated global field in the manifest: the manifest is not period-indexed, so it could not follow the year scrubber. The shard already carries per-year values.
- Computing a population-weighted mean from the country rows: needs population weights we do not ingest and would not match WB's official figure (per the backlog).
- Keying World by WB's `WLD`: source-specific; the canonical key is `region.code` (`world`).
- Capturing all WB aggregates now: deferred. We full-pull the entire WDI set on every build, so the aggregates are re-capturable anytime, and modeling WB's aggregate taxonomy as canonical regions is speculative until a comparison feature needs it.
