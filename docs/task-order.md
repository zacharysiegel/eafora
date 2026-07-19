# Task order

Committed near-term work sequence. Order is load-bearing (later tasks depend on earlier ones). Detail for each item lives in `docs/backlog.md` (parked items, with triggers) or in the architecture doc / spec plan that introduces the work.

When a task is picked up, leave it here as **In progress**; delete it on the same PR that lands it on master.

`005-core-data` and `006-core-renderer` have landed on `master`. The web client (`003-web-client`) is mid-delivery, in phases.

## Sequence

1. **`003-web-client`** — the browser client (WASM + Leptos shell + map view + OPFS cache + perf-budget). Planning artifacts merged; delivered in phases, full breakdown in `specs/003-web-client/plan.md` §Phasing for PRs. **In progress.**
   - Phase 0a (`shared` wasm32 canvas attach + `RendererBackends`) — landed.
   - Phase A (`web/` crate: cargo-leptos config + Leptos shell + `leptos_i18n` + Sass tokens) — landed.
   - Phase B (`OpfsArtifactCache` implementing `shared::artifact::ArtifactCache`, headless-Chrome tested) — landed.
   - Phase C (canvas→wgpu surface + `MapView` first paint against the embedded bundle) — pending; needs Phase 0a + the Phase 0b downsampled bundle.
   - Phase D (browser fetch + discovery + speculative fetch + bundle hot-swap) — pending.
   - Phase E (perf-budget script + precompress + `wrangler` deploy config) — pending.
2. **`ingestion build` coupled bundle emission** — producer command that emits both bundles for one version in a single run: `$EAFORA_ARTIFACTS_DIR/<version-label>/complete/` (all periods and sources; publishes to the CDN) and `$EAFORA_ARTIFACTS_DIR/<version-label>/downsampled/` (World Bank WDI at the United States' most-recent reference year, full 1:50m geometry; embedded into clients), and updates a `$EAFORA_ARTIFACTS_DIR/latest` symlink pointing at the newest `<version-label>/`. The separate `--downsampled` flag was removed (web-client Phase 0b prerequisite; unblocks `scripts/sync-embedded-bundle.sh` and the real embedded/live bundle). Off `master`; independent of the web stack. Built first because a meaningful first paint needs real 1:50m geometry, which only the producer pipeline emits (a hand-stub would be 0b in disguise); Phase C consumes its output. **In progress on `coupled-bundle-build`.**
3. **`004-ios-client`** — Xcode project + xcframework + SwiftUI shell + file-system cache adapter + AASA deploy + TestFlight pipeline. Spec: `specs/004-ios-client/spec.md`. Off `master`; may run in parallel with the web phases. **Pending.**
