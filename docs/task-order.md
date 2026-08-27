# Task order

Committed near-term work sequence. Order is load-bearing (later tasks depend on earlier ones). Detail for each item lives in `docs/backlog.md` (parked items, with triggers) or in the architecture doc / spec plan that introduces the work.

When a task is picked up, leave it here as **In progress**; delete it on the same PR that lands it on master.

`005-core-data`, `006-core-renderer`, `003-web-client`, `007-hfd-ingestion`, the region detail dock, manifest schema backtracking, the year scrubber's capsule, and brotli artifact compression have landed on `master`. Compression took a cold fetch from 4,373,808 bytes of artifacts to 711,553, measured; the record is in `specs/009-artifact-compression/` and the ratios are in `docs/architecture/client-web.md` §Compression. HFD shipped in four phases: ingestion, presenting a cohort as a range, period total fertility with per-cell source priority, and keeping every source's value in the shard so a reader can be shown an alternative. The per-phase record and its deviations are in `specs/007-hfd-ingestion/tasks.md`. The sources after it follow `docs/data/sources-survey.md` §Recommended integration order, whose phase 1 is UN WPP, Eurostat, World Bank, HFD, and Our World in Data. World Bank and HFD have landed; WPP is blocked on its licence, which both the survey's own profile of it and `docs/research/data-source-licensing.md` §UN World Population Prospects call ambiguous, and which the same fertility indicators reach through WDI's clear CC BY meanwhile; OWID is a cross-check rather than a primary source, so each of its indicators is ingested from its upstream. That leaves Eurostat next. The web client shipped in phases 0a through E; the per-phase record is in `specs/003-web-client/plan.md` §Phasing for PRs and the phase plan documents beside it. Its follow-up surfaces (the SSG region and about routes, a styled 404 page, producer-side artifact compression) are in `docs/backlog.md` with their triggers rather than here.

## Sequence

1. **Eurostat ingestion** — the next data source, and the first to carry regions below country level: fertility at NUTS-2 and NUTS-3, plus fresher country figures than WDI holds for members. It needs a spec, and the subnational region model is the substance of it, since every region the store holds today is a country or the world.
   - Sources take priority over the iOS client from here.
   - Unblocking UN WPP is a written reply from `population@un.org` confirming reuse terms, which is an errand rather than a task; WPP jumps ahead of Eurostat if it arrives, being the wider source.

2. **`004-ios-client`** — the iOS surface: a UniFFI boundary, a SwiftUI shell, and the Metal surface. Spec, plan, and tasks in `specs/004-ios-client/`. **In progress** (`004-ios-client`), planning only so far, and parked behind the sources above.
   - Delivered in phases per `plan.md` §Phasing for PRs. Phases 0.1, 0.2, A, and B carry task breakdowns; C and D are scoped sketches that need one before being picked up.
   - Phase 0.1 is blocked on approving `uniffi`, the feature's one new Rust dependency. Phase D is blocked on an Apple Developer Program enrollment, which gates device installs, TestFlight, and Universal Links but nothing on the simulator.
   - Phase 0.2 moves the web client's load orchestration, discovery reconciliation, and version ranking into `shared`, since all three are platform-agnostic Rust that happens to live under `web/`. Doing it first is what stops the iOS client reimplementing 562 lines of it in Swift.
