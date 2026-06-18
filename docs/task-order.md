# Task order

Committed near-term work sequence. Each entry is the next concrete deliverable in line; the order is load-bearing (tasks below depend on tasks above).

This is the complement to `docs/backlog.md`:

- **Task order** (this file): committed sequence — what to do next, in order.
- **Backlog** (`docs/backlog.md`): parked items with no committed timeline; each carries a trigger that would move it into this list.

When a task is picked up, leave it here as **In progress**. When it lands on master, delete it from this file as part of the same PR. Add new tasks at the bottom unless a real dependency forces an earlier slot.

## Sequence

1. **`docs-architecture-client-web`** — draft `docs/architecture/client-web.md`. The web-specific deep-dive companion to `client.md` (cross-cutting) and `ingestion.md` (producer side). Covers Leptos + WASM + cargo-leptos build, plain-CSS component layout, OPFS cache lifecycle implementation details, perf-budget enforcement (2 MB first-paint / 3 MB second-paint caps), WebGPU/WebGL2 fallback UX, hot-reload story.
2. **`docs-architecture-client-ios`** — draft `docs/architecture/client-ios.md`. The iOS-specific deep-dive companion to `client.md`. Covers SwiftUI + UniFFI + xcframework integration, MTKView host, Metal-via-wgpu surface acquisition, file-system cache contract, Xcode build pipeline pulling `ingestion build --downsampled` output. Develops in parallel with web per the parallel-development discipline.
3. **`/speckit.specify` for the web client** — first spec-kit feature implementing client behavior, sourced from the architecture docs above. Numbering will be `specs/00N-web-client/` (next available; see `specs/`). Spec scope to be brainstormed at kickoff.
4. **`/speckit.specify` for the iOS client** — second spec-kit feature, parallel to the web client spec.

