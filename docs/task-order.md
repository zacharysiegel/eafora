# Task order

Committed near-term work sequence. Order is load-bearing (later tasks depend on earlier ones). Detail for each item lives in `docs/backlog.md` (parked items, with triggers) or in the architecture doc / spec plan that introduces the work.

When a task is picked up, leave it here as **In progress**; delete it on the same PR that lands it on master.

`005-core-data`, `006-core-renderer`, and `003-web-client` have landed on `master`. The web client shipped in phases 0a through E; the per-phase record is in `specs/003-web-client/plan.md` §Phasing for PRs and the phase plan documents beside it. Its follow-up surfaces (the SSG region and about routes, a styled 404 page, producer-side artifact compression) are in `docs/backlog.md` with their triggers rather than here.

## Sequence

1. **`007-hfd-ingestion`** — the second data source and the first added statistic: completed cohort fertility from the Human Fertility Database, plus the client work that keeps a cohort from being drawn as a calendar instant. Spec: `specs/007-hfd-ingestion/spec.md`. **In progress** (`hfd-ingestion`).
   - Ordered ahead of the iOS client at the owner's direction: data integrations first. The sources after this one are Gapminder, then Eurostat, then OECD, per `docs/research/data-source-licensing.md` §Recommended ingestion roadmap.
2. **`004-ios-client`** — Xcode project + xcframework + SwiftUI shell + file-system cache adapter + AASA deploy + TestFlight pipeline. Spec: `specs/004-ios-client/spec.md`. Off `master`; may run in parallel with the web phases. **Pending.**
