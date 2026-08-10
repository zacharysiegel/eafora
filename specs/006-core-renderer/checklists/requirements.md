# Specification Quality Checklist: shared/ — renderer layer

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-06-22
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

> Notes: this is a Rust-internal renderer-layer spec. "Users" are the developers building 003-web-client + 004-ios-client (each platform's per-frame shell calls `Renderer::draw_frame`) and the operator who deploys the resulting app. Naming `wgpu` / WGSL / `raw-window-handle` / Miller cylindrical / FlatGeobuf is required because those are locked architecture decisions (`docs/architecture/overview.md` §wgpu rendering pipeline + §Projection + §Polygon representation; Principle III mandates wgpu); the spec implements against them rather than re-deriving them.

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)

> SC-001 through SC-006 are verifiable via build / test output (compile success, projection round-trip tolerance, test pass count) or via downstream-PR import resolution and manual visual checks once 003 / 004 ship.

- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded

> §Scope cutoff explicitly names what's NOT in this feature (per-platform FFI modules, `hover_scale` easing curves, GPU-based label rendering). Each cut item is paired with where it lives instead (003 / 004, v2+ animation work, future SDF / MSDF pipeline). The zoom-to-country `Camera` state machine, formerly cut to v1.5+, has since landed in `shared::map::camera` with the driver wiring in the web client (003).

- [x] Dependencies and assumptions identified

> §Assumptions names: 005-core-data is the direct parent branch; wgpu API names need verification at plan time (per architecture docs' "to verify" lists); flatgeobuf + geo crate point-in-polygon shape needs verification; the wgpu device limits floor is `downlevel_webgl2_defaults`; raw-window-handle UiKit variant shape needs verification (with `wgpu::Surface::from_metal_layer` as the fallback); choropleth color function caches min/max on the bundle at open time (flagged for 005-implementation-time confirmation).

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria

> Mapping:
> - FR-001 / FR-002 / FR-003 → SC-001 (cross-target build success)
> - FR-004 / FR-005 → P1 acceptance #1, #2, #3 (Renderer construction without surface)
> - FR-006 / FR-007 / FR-008 / FR-009 → P2 acceptance #1, #2, #3 (surface lifecycle attach / resize / detach)
> - FR-010 / FR-011 / FR-012 / FR-013 / FR-014 → exercised by P3 + P4 acceptance scenarios that consume them
> - FR-015 / FR-016 / FR-017 / FR-018 / FR-019 → P3 acceptance #1, #2, #3, #4 (draw_frame round-trip, choropleth color, selection outline, antimeridian wraparound, surface-error recovery)
> - FR-020 / FR-021 / FR-022 → P5 acceptance #1, #2, #3 (projection origin, round-trip tolerance, unclamped input)
> - FR-023 / FR-024 → P4 acceptance #1, #2, #3, #4 (hit-test inside-USA / ocean / hover-doesnt-affect-hit / antimeridian-wrap)
> - FR-025 / FR-026 / FR-027 → exercised by P3 + P4 via the bundle's geometry source
> - FR-028 / FR-029 / FR-030 → SC-002 (test pass + projection round-trip + cross-target coverage + shader compile-time validation)

- [x] User scenarios cover primary flows

> P1 = Renderer construction (the first phase of the two-phase setup). P2 = surface lifecycle (the second phase + view-lifecycle handling). P3 = draw_frame (the per-frame contract both clients consume). P4 = spatial hit-testing (the hover-scale-doesnt-affect-hit-test property + the antimeridian wraparound). P5 = Miller cylindrical projection (the pure-math foundation that powers everything else). Together they exercise every public symbol the per-platform implementation features (003 / 004) reach for.

- [x] Feature meets measurable outcomes defined in Success Criteria

> Every SC is verifiable from observable build / test output, projection-tolerance assertions, downstream import resolution, or manual visual checks against the design stubs.

- [x] No implementation details leak into specification

> The architecture's locked decisions are surfaced where required (Miller cylindrical, WGSL, `downlevel_webgl2_defaults`, FlatGeobuf R-tree, raw-window-handle); plan-level decisions (whether the headless wgpu test is `#[cfg]`-gated, whether the choropleth color function caches min/max on the bundle vs in the renderer, whether shaders are validated at build time via `build.rs` or at test time via `include_str!`-and-compile) are explicitly deferred.

## Notes

- All checklist items pass on first iteration.
- Zero `[NEEDS CLARIFICATION]` markers. Every potentially-ambiguous decision had a load-bearing default in the architecture docs or in the existing producer-side implementation.
- 006 stacks on 005-core-data per the constitution's §Branch per body of work rule because 006's `Renderer::new` takes a `tokio::sync::watch::Receiver<Arc<Bundle>>` that 005 introduces; if these were unrelated, both could branch off master, but the type dependency forces the stack.
- The `shared::ffi::wasm` and `shared::ffi::uniffi` modules are deliberately deferred to the per-platform implementation features (003 / 004) because the consuming code lives in those platforms and the FFI shapes are platform-specific.
- The hover-scale animation curve is deferred per the design doc's v1-no-animation rule; the `hover_scale` pipeline still ships (it renders the discrete hovered vs not-hovered visual state) but the easing curve is a v2+ concern.
- Spec is ready for `/speckit-clarify` (if reviewer surfaces ambiguity) or `/speckit-plan`. Per `feedback_spec_and_plan_same_pr.md`, both ship in the same PR.
