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
     - C2.4a (`RegionDetailPanel`: selection lifted to `MapView` via context, `SelectionView` enriched with statistic + period) — **In progress on `web-region-detail-panel`.**
     - C2.4b (`SourcePanel` + `ProvenanceView`) — pending; split out of C2.4.
     - C2.3 (DPR hit-test coverage) — pending; likely a host scale-invariance test rather than a headless-Chrome test.
     - C2.5 (Controls: statistic picker + year scrubber) — pending.
     - C2.6 (Legend) — pending.
   - Phase C3 (viewport camera/aspect + pan/zoom + the selection/hover renderer pass) — pending; split out of C2 per `c2-plan.md` §Deferred to C3.
   - Phase D (browser fetch + discovery + speculative fetch + bundle hot-swap) — pending.
   - Phase E (perf-budget script + precompress + `wrangler` deploy config) — pending.
2. **`004-ios-client`** — Xcode project + xcframework + SwiftUI shell + file-system cache adapter + AASA deploy + TestFlight pipeline. Spec: `specs/004-ios-client/spec.md`. Off `master`; may run in parallel with the web phases. **Pending.**
