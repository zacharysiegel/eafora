# Specification Quality Checklist: Web client (WASM + Leptos + wgpu shell + OPFS cache)

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-06-22
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

> Notes on this section: the spec deliberately names Rust / Leptos / wgpu / OPFS / Cloudflare Workers Assets / `leptos_i18n` / wasm-bindgen / brotli — they are locked architectural decisions from `docs/architecture/client-web.md`, `docs/architecture/client.md`, `docs/architecture/overview.md`, and `.specify/memory/constitution.md` (Principle III mandates Leptos + WASM for the web shell; Principle IV mandates Singularity-stack parity). This spec is a per-feature implementation contract against those decisions, not a discovery doc. Re-deriving the framework choices in a feature spec would invert the constitution's "architecture is locked at the project level; features implement against it" structure. The "no implementation details" rule is honored at the level of: which library function to call, which file structure to use within a module, which test runner to wire up — those are plan-level decisions and are deferred to `/speckit-plan`.

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)

> Note on this item: SC-001 through SC-008 mix user-outcome metrics (first-paint time, post-deploy verification) with build-artifact metrics (bundle size, compile-time check). The latter are measurable system properties that map directly to user experience (bigger bundle → slower first paint), not implementation details. SC-007 names the build-script + provider-component shape because the "missing translation is a build error, not a runtime miss" property is itself the success criterion — restating it as "every translation key resolves at runtime" loses the compile-time-guarantee shape that is the whole point.

- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded

> §Scope cutoff explicitly names what's NOT in this feature (SSG routes, real-CDN-wired live fetch, Worker-based SQLite, design-token codegen, region index). Each item is paired with the architectural reference that documents WHY it's out of scope and the trigger / follow-up that would pull it in.

- [x] Dependencies and assumptions identified

> §Assumptions names: (a) `core/` crate prerequisites (the first `client.md` producer follow-up); (b) `ingestion build` emits the downsampled subtree as a prerequisite; (c) `repository.eafora.org` + `latest/manifest.json` are NOT prerequisites — the spec describes the same-origin stub path that lets the feature land before them; (d) pinned Leptos / wgpu / `leptos_i18n` versions and the rule for re-verifying API names if they shift; (e) Safari OPFS cutoff to verify against `caniuse.com`; (f) Cloudflare dashboard configuration sits outside the codebase; (g) `brotli` CLI on the build machine; (h) embedded-bundle staleness story.

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria

> Mapping (each FR-N has at least one acceptance scenario or success criterion that exercises it):
> - FR-001 / FR-002 / FR-003 → SC-008 (the build produces a tree that deploys and renders); SC-002 bounds artifact bytes, not client code, so it does not exercise these
> - FR-004 → exercised by P1 acceptance #1 (embedded bundle loads)
> - FR-005 withdrawn, so SC-002 no longer counts `.br` siblings; it counts what a client transfers, which for the shards is their full size
> - FR-006 → P4 acceptance scenarios (perf-budget report)
> - FR-007 / FR-008 / FR-009 → P1 acceptance #1 (page-shell + Leptos `App` mounts)
> - FR-010 / FR-010a → SC-007 (compile-time translation-key check)
> - FR-011 → P1 acceptance #1 + SC-006 (visual layout matches stub-desktop.html)
> - FR-012 → P1 acceptance #1, #5, #6 (wgpu surface acquisition + WebGPU / WebGL2 / no-adapter paths)
> - FR-013 → implicit in P1 (event-driven rendering doesn't have a directly-observable user-facing acceptance, but P1#2/#3/#4 exercise the input-handler paths that set the dirty flag); also explicitly observable as "no continuous GPU load while idle" in dev tools
> - FR-014 → P1 acceptance #3 (instant choropleth swap)
> - FR-015 → P1 acceptance #5 (`?renderer=webgl2` forces Backend::Gl)
> - FR-016 → P1 acceptance #6 (no-adapter fallback message)
> - FR-017 through FR-024 → P2 acceptance #1 through #5 (OPFS cache adapter contract)
> - FR-025 / FR-026 / FR-027 / FR-028 / FR-029 → P3 acceptance #1 through #5 (discovery + speculative fetch + verify + persist + hot-swap)
> - FR-030 / FR-031 → P3 acceptance #1, #2, #4 (hot-swap mechanics)
> - FR-032 / FR-033 / FR-034 → SC-008 (operator can `wrangler deploy` and the deployed URL works)
> - FR-035 / FR-036 / FR-037 / FR-038 / FR-039 → SC-006 (visual identity matches design)
> - FR-040 / FR-041 / FR-042 → SC-003 (OPFS cache adapter test) + general CI-passes property
> - FR-043 → directly observable: there is no Playwright suite / `npm run e2e` script in this PR

- [x] User scenarios cover primary flows

> P1 = first-paint render against the embedded bundle (the deliverable that closes the `client-web.md` §Follow-up work item). P2 = OPFS cache adapter persistence (the verifiable surface the live-fetch path is built on). P3 = discovery + speculative parallel fetch + hot-swap (the full live path; testable against a same-origin stub before the producer side stands up the real CDN). P4 = perf-budget report (the warning surface that protects the 2 MB / 8 MB artifact-byte caps).

- [x] Feature meets measurable outcomes defined in Success Criteria

> Every SC-N is verifiable from observable system state: first-paint time (SC-001), deployable byte count (SC-002), test result (SC-003 / SC-007), wall-clock time of the live-fetch flow (SC-004 / SC-005), side-by-side visual comparison (SC-006), HTTP 200 against a deployed URL (SC-008).

- [x] No implementation details leak into specification

> See note under §Content Quality item #1. The named technologies are locked architecture, not implementation. Within those constraints the spec resists naming specific function names where the architecture doesn't already lock them down — e.g. FR-041 says "the simulation mechanism (mock server, web_sys interception layer, fetch override) is a plan-level decision" rather than picking one.

## Notes

- All checklist items pass on first iteration.
- Zero `[NEEDS CLARIFICATION]` markers were emitted. Reason: every potentially-ambiguous decision had a load-bearing default in the architecture docs (`client-web.md`, `client.md`, `overview.md`, `design/README.md`) or in the saved feedback memory. Where the architecture doc itself flagged a "to verify" item (Leptos metadata keys, wgpu API name, Safari OPFS cutoff), the spec's §Assumptions section names the verification step rather than blocking on it.
- Spec is ready for `/speckit-clarify` (if reviewer surfaces ambiguity) or `/speckit-plan` (to begin implementation planning). Per `feedback_spec_and_plan_same_pr.md`, both land in the same PR.
