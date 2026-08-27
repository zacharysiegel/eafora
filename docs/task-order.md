# Task order

Committed near-term work sequence. Order is load-bearing (later tasks depend on earlier ones). Detail for each item lives in `docs/backlog.md` (parked items, with triggers) or in the architecture doc / spec plan that introduces the work.

When a task is picked up, leave it here as **In progress**; delete it on the same PR that lands it on master.

`005-core-data`, `006-core-renderer`, `003-web-client`, `007-hfd-ingestion`, the region detail dock, manifest schema backtracking, and the year scrubber's capsule have landed on `master`. HFD shipped in four phases: ingestion, presenting a cohort as a range, period total fertility with per-cell source priority, and keeping every source's value in the shard so a reader can be shown an alternative. The per-phase record and its deviations are in `specs/007-hfd-ingestion/tasks.md`. The sources after it are Gapminder, then Eurostat, then OECD, per `docs/research/data-source-licensing.md` §Recommended ingestion roadmap. The web client shipped in phases 0a through E; the per-phase record is in `specs/003-web-client/plan.md` §Phasing for PRs and the phase plan documents beside it. Its follow-up surfaces (the SSG region and about routes, a styled 404 page, producer-side artifact compression) are in `docs/backlog.md` with their triggers rather than here.

## Sequence

1. **Artifact compression** — the CDNs compress nothing for us, so a cold fetch carries approx. 3.07 MB more than it needs to: the geometry compresses 3.4x and a statistic shard 13.0x, both measured. The producer encodes with brotli and `shared` decodes inside `Bundle::open`, so every platform gets it without a loader remembering to. Spec, plan, and tasks in `specs/009-artifact-compression/`. **In progress** (`artifact-compression`).
2. **`004-ios-client`** — the iOS surface: a UniFFI boundary, a SwiftUI shell, and the Metal surface. Spec, plan, and tasks in `specs/004-ios-client/`. **In progress** (`004-ios-client`), planning only so far.
   - Delivered in phases per `plan.md` §Phasing for PRs. Phases 0.1, 0.2, A, and B carry task breakdowns; C and D are scoped sketches that need one before being picked up.
   - Phase 0.1 is blocked on approving `uniffi`, the feature's one new Rust dependency. Phase D is blocked on an Apple Developer Program enrollment, which gates device installs, TestFlight, and Universal Links but nothing on the simulator.
   - Phase 0.2 moves the web client's load orchestration, discovery reconciliation, and version ranking into `shared`, since all three are platform-agnostic Rust that happens to live under `web/`. Doing it first is what stops the iOS client reimplementing 562 lines of it in Swift.
