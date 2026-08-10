# Task order

Committed near-term work sequence. Order is load-bearing (later tasks depend on earlier ones). Detail for each item lives in `docs/backlog.md` (parked items, with triggers) or in the architecture doc / spec plan that introduces the work.

When a task is picked up, leave it here as **In progress**; delete it on the same PR that lands it on master.

`005-core-data` and `006-core-renderer` have landed on `master`. The web client (`003-web-client`) is mid-delivery, in phases.

## Sequence

1. **`003-web-client`** — the browser client (WASM + Leptos shell + map view + OPFS cache + perf-budget). Planning artifacts merged; delivered in phases, full breakdown in `specs/003-web-client/plan.md` §Phasing for PRs. **In progress.**
   - Phase 0a (`shared` wasm32 canvas attach + `RendererBackends`) — landed.
   - Phase A (`web/` crate: cargo-leptos config + Leptos shell + `leptos_i18n` + Sass tokens) — landed.
   - Phase B (`OpfsArtifactCache` implementing `shared::artifact::ArtifactCache`, headless-Chrome tested) — landed.
   - Phase C1 (first paint: loader + canvas→wgpu + `MapCanvas`) — landed.
   - Phase C2 (chrome + interactions) — plan and sub-slices in `specs/003-web-client/c2-plan.md`. **In progress.**
     - C2.1 (`RegionHit` hit-test in `shared`) — landed.
     - C2.2 (selection/hover interaction layer + driver) — landed.
     - C2.4a (`RegionDetailPanel`: selection lifted to `MapView` via context, `SelectionView` enriched with statistic + period) — landed.
     - C2.4b (per-region source attribution: `shard_db` surfaces each cell's data source, the detail panel shows it) — landed. A standalone always-visible `SourcePanel` / global `ProvenanceView` is deferred; per-region attribution is the meaningful version, and per-statistic aggregation only matters once multiple sources coexist.
     - C2.3 (DPR hit-test coverage) — **In progress on `web-map-dpr-hit-test`.** Host scale-invariance test: `region_at_point` resolves the same region at DPR 1/2/3 because it normalizes the point against the surface dimensions. Not headless-Chrome, per the wasm-test convention (the ratio math is target-agnostic).
     - C2.5 (Controls: statistic picker + year scrubber) — landed. Forward-compatible no-ops today (one statistic, one embedded period); meaningful once Phase D loads the multi-year bundle or a second statistic exists.
     - C2.6 (Legend) — landed. Data-driven `LegendView`; the color scale's interpolation is a swappable `ColorScale.interpolator` field.
     - C2.7 (per-statistic color transform) — landed. `StatisticColorTransform::{Linear, PiecewiseCubicArctan}` selected by `transform_for(StatisticKind)`; TFR uses a C² curve inflecting at replacement (2.1); the legend samples the gradient through the transform and marks the inflection generically. Design in `specs/003-web-client/color-transform-design.md`.
   - Phase C3 (viewport camera/aspect + pan/zoom + the selection/hover renderer pass) — split out of C2 per `c2-plan.md` §Deferred to C3, detailed in `specs/003-web-client/c3-plan.md`. **In progress.**
     - C3.1 (viewport aspect correction: isotropic projected radians + `Viewport::fill_height`, prime-meridian-centered home view) — landed.
     - C3.2 (selection/hover render pass: per-country GPU identity, emphasis lift + outline) — landed.
     - C3.3 (manual pan/zoom + pinch) — landed. Pointer Events input; wheel-zoom toward the cursor, drag-pan, and two-finger pinch mutating the projected viewport; zoom-out clamped to the home latitude range, horizontal wrap re-normalized so long pans stay visible.
     - C3.4 (animated zoom-to-country `ViewportTransition`) — **In progress on `web-map-zoom-to-country`.** Re-tapping the already-selected country (a second tap on it, or a double-click) eases the viewport to frame it; a plain first selection does not move the map. A pure `ViewportTransition` in `shared::map::viewport_transition` (cubic ease-in-out, geometric height interpolation, antimeridian short-way) sampled by a self-scheduling rAF loop in the web driver; any manual gesture, press, or resize cancels the animation.
   - Phase D (browser fetch + discovery + speculative fetch + bundle hot-swap) — pending.
   - Phase E (perf-budget script + precompress + `wrangler` deploy config) — pending.
2. **`004-ios-client`** — Xcode project + xcframework + SwiftUI shell + file-system cache adapter + AASA deploy + TestFlight pipeline. Spec: `specs/004-ios-client/spec.md`. Off `master`; may run in parallel with the web phases. **Pending.**
