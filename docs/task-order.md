# Task order

Committed near-term work sequence. Order is load-bearing (later tasks depend on earlier ones). Detail for each item lives in `docs/backlog.md` (parked items, with triggers) or in the architecture doc that introduces the work.

When a task is picked up, leave it here as **In progress**; delete on the same PR that lands it on master.

## Sequence

1. **`005-core-data`** — extract types + manifest schema + cache trait + `Bundle` loader + `DistributionContext` from `ingestion/` into a new `core/` workspace member. Spec: `specs/005-core-data/spec.md`. Off `master`.
2. **`006-core-renderer`** — wgpu pipelines + WGSL shaders + Miller cylindrical projection + spatial hit-testing + `Renderer` lifecycle inside `core/`. Spec: `specs/006-core-renderer/spec.md`. Stacks on `005-core-data` (needs `Bundle` + watch-channel types). Also builds the cross-platform SQLite `Connection` bridge in `shared::sqlite` (deferred out of 005: `sqlite-wasm-rs` exposes no `Connection` type, so it needs a real wrapper — native `rusqlite` + wasm32 raw FFI — whose API is defined by the renderer's queries).
3. **`003-web-client`** — WASM bundle + Leptos page shell + map view + OPFS cache adapter + perf-budget script. Spec: `specs/003-web-client/spec.md`. Off `master`; gated on `005`+`006` merged before implementation begins.
4. **`004-ios-client`** — Xcode project + xcframework + SwiftUI shell + file-system cache adapter + AASA deploy + TestFlight pipeline. Spec: `specs/004-ios-client/spec.md`. Off `master`; gated on `005`+`006` merged before implementation begins. May run in parallel with `003`.
