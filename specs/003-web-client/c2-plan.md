# Phase C2 plan — web client map chrome + interactions

Feature `003-web-client`, Phase C2. Stacks on C1 (landed on `master`). Covers the map chrome (legend,
controls, detail + source panels) and the click / hover / statistic-swap / year-scrub interactions.
Closes the C2-scoped parts of FR-011, FR-013, FR-014 and P1 acceptance scenarios 1–4 (except Scenario
2's on-map outline, deferred to C3 — see below).

Produced by the `plan-web-c2` planning workflow (survey → two competing bridging designs → synthesis),
adjusted for the owner's scope decisions.

## Decisions locked

- **Selection outline / hover highlight on the map → deferred to C3.** C2 wires the full selection +
  hover state and the detail panel, but the 1px red outline and hover highlight are drawn by a new
  renderer pass that lands with the camera/aspect work in C3. Scenario 2's outline is not visible until
  C3; clicking already drives the detail panel in C2.
- **Provenance depth: friendly source name + dates, no upstream change.** The source panel and the
  detail-panel attribution map `DataSourceKind` → a human name via i18n (same pattern as statistic
  names) and show revision / published / fetched dates + build version, all from the manifest's
  `source_revisions`. License name/URL is canonical-store-only and deferred (needs an upstream
  artifact-shape change).
- **Ranking table → deferred.** No C2 FR names it; revisit as its own slice.
- **Four sibling components**, not one monolithic `Controls`: `Controls` (statistic picker + year
  scrubber), `RegionDetailPanel`, `SourcePanel`, `Legend`. Deviates from FR-011's literal
  single-`Controls` grouping on feature-cohesion grounds.
- **Shared-crate changes adopted:** lift the `select_shard` "first authorized license class" policy to a
  shared helper both the renderer and the C2 resolvers call (so a panel's value provably matches the map
  fill); widen `hit_test::region_at_point` to return the region's `iso3` / `name_en` (a `RegionHit`) so
  selection resolution does not re-parse the whole geometry layer per click.
- **Year scrubber** renders over the full `ShardValues::period_range()` track even though the embedded
  bundle ships a single year (dragging off it renders "no data" per Scenario 4).

## Architecture: driver is the single source of truth

`FrameState` in the `DRIVER` thread-local stays authoritative (chosen over signals-as-truth). Controls
call imperative `driver::` free functions that mutate `FrameState`, `request_redraw`, and publish
read-only projection signals the panels render. Signals are a downstream projection, never a parallel
store.

Two disciplines grafted from the rejected signals-as-truth design:

- **Equality guard per mutator** — each `set_active_statistic` / `scrub_to_period` / `select_region_at` /
  `hover_region_at` compares its one field before mutating, skipping a redundant `request_redraw`.
- **Borrow-then-set** — a mutator method returns the projections to publish; the free-function wrapper
  drops the `DRIVER` borrow, then calls `signal.set`. No `signal.set` ever runs inside `with_borrow_mut`,
  closing the RefCell re-entrancy panic path.

Hover is render-only (no signal): the renderer is hover's consumer, and `request_redraw` already
coalesces `pointermove` bursts into one frame.

## Component tree

```
App
└─ MapView                     <main id="map-view">           (map/map.rs, edited)
   ├─ MapCanvas                <canvas> + status overlay        (map/canvas/canvas.rs, edited)
   ├─ Legend                   choropleth ramp, bottom-left      (map/legend.rs, new)
   ├─ Controls                 statistic picker + year scrubber  (map/controls.rs, new)
   ├─ RegionDetailPanel        top-left; name + value + source   (map/detail_panel.rs, new)
   └─ SourcePanel              provenance                        (map/source_panel.rs, new)
```

Projections (`SelectionView`, `LegendView`, `ProvenanceView`) are defined in `canvas.rs` (the reactive
layer); the driver depends on them, mirroring how `RenderStatus` is defined in `canvas.rs` and passed
into `driver::start`. Panels read them via `provide_context` / `expect_context` and never touch
`FrameState` or the bundle directly.

## State-bridging wiring

- Signals in `MapCanvas`: `render_status`, `selection: RwSignal<Option<SelectionView>>`,
  `legend: RwSignal<Option<LegendView>>`, `provenance: RwSignal<Option<ProvenanceView>>`. Write-halves
  are threaded into `driver::start`; read-halves go into context for the sibling panels.
- Driver gains: `surface_dimensions: SurfaceDimensions` (set at attach + in `resize`; hit-test needs
  it), the three `WriteSignal`s, and `click` / `pointermove` / `pointerleave` `Closure` fields (held for
  the page lifetime, like `resize_callback`).
- Mutator shape follows the equality-guard + borrow-then-set discipline above.
- Canvas events: `click` → `select_region_at(screen_point)`; `pointermove` → `hover_region_at`;
  `pointerleave` → `clear_hover`. `screen_point_from_event` converts `offset_x`/`offset_y` (CSS px) to
  device px (× `devicePixelRatio`) to match `surface_dimensions`.

**Coordinate-space contract (highest risk):** `offset_*` are CSS pixels; `surface_dimensions` are device
pixels. `region_at_point` computes ratios, so both operands must be the same space → device px on both
sides. Asserted by a headless-Chrome test at DPR 1.0 and 2.0 (C2.3), because the bug is invisible at DPR
1.0 and wrong on Retina.

## Sub-slices (linear stacked PRs)

Each branches off the previous; the first commit on each is the `>>> branch:` marker.

- **C2.1 — driver surface-dims + hit-test plumbing (no UI).** `driver.rs`: `surface_dimensions` field
  (set at attach + `resize`), `region_at` via `bundle_sender.borrow()`, `screen_point_from_event`.
  `shared`: widen `hit_test::region_at_point` → `RegionHit { region_code, iso3, name_en }`; lift
  `select_shard` to a shared helper. No user-visible change.
- **C2.2 — selection/hover mutators + canvas listeners + projection publish.** `SelectionView`, the
  select / hover / clear free functions + methods (borrow-then-set), the three listeners, extended
  `driver::start`. FrameState set + redraw requested; the on-map outline is C3.
- **C2.3 — headless-Chrome coordinate-space + selection test.** Click at a known offset resolves to the
  expected region at DPR 1.0 and DPR 2.0.
- **C2.4 — RegionDetailPanel + SourcePanel + SCSS + i18n.** `ProvenanceView` published at startup; the
  two panels; provenance via i18n source names.
- **C2.5 — Controls (statistic picker + year scrubber) + SCSS + i18n.** `set_active_statistic` /
  `scrub_to_period`; Scenario 3 + 4; instant, no animation (FR-014).
- **C2.6 — Legend + SCSS + i18n.** Samples `color::choropleth_fill` across the active statistic's
  `value_range()`; no-data swatch.

Panels (C2.4) intentionally precede controls (C2.5) so selection is visible before statistic/year
mutation is wired.

## New i18n keys

`legend.*` (title, no_data, low, high); `statistic.*` (tfr, test_alpha, picker_label); `scrubber.*`
(label, year_prefix); `detail.*` (value_label, no_data, source_lead_in); `source.*` (title,
build_version_label, created_label, revision_label, fetched_label, plus one friendly-name key per
`DataSourceKind`). Panel copy is full sentences ending with a period; control labels are label-style.

## Deferred to C3 (camera / renderer slice)

- The selection/hover renderer pass (the 1px outline + hover highlight).
- Viewport aspect correction + pan/zoom (see `plan.md` §"Deferred to C2: viewport aspect ratio"). The
  pointer listeners C2 installs are reused by C3's wheel/drag; C2's hit-test is forward-compatible
  because it takes `viewport` by value on each call.

## Risks

- Coordinate-space / DPR — mitigated by C2.3.
- Projection drift — the "every mutator republishes what it changed" invariant is convention, not
  compiler-enforced; a reviewer must hold it.
- Hover hit-test load — `region_at_point` runs per `pointermove` (bbox-indexed, cheap); the equality
  guard limits redraws to country crossings. Throttle-to-rAF only if profiling shows a problem.
