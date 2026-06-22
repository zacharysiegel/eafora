# Specification Quality Checklist: iOS client (Xcode project + xcframework + SwiftUI shell + file-system cache)

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-06-22
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

> Notes on this section: the spec deliberately names Swift / SwiftUI / MTKView / Metal / UniFFI / XcodeGen / `URLSession` / Cloudflare Workers Assets — these are locked architectural decisions from `docs/architecture/client-ios.md`, `docs/architecture/client.md`, `docs/architecture/overview.md`, and `.specify/memory/constitution.md` (Principle III mandates SwiftUI for iOS; Principle IV mandates UniFFI for the FFI surface; the AASA-via-dedicated-Worker shape is locked in `client-ios.md`). This spec is a per-feature implementation contract against those decisions, not a discovery doc. Re-deriving the framework choices in a feature spec would invert the constitution's "architecture is locked at the project level; features implement against it" structure. The "no implementation details" rule is honored at the level of: which API call to make in `MapRenderer`, which file-layout convention to use within `Map/`, which test target to wire up — those are plan-level decisions and are deferred to `/speckit-plan`.

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)

> Note on this item: SC-001 through SC-009 mix user-outcome metrics (launch-to-first-paint time, deep-link routing) with build-process metrics (incremental build time, CI workflow runs) and provenance metrics (revision visible in two surfaces). The build-process metrics are measurable system properties that map directly to developer-experience outcomes; the provenance metrics map directly to support-engineer ability to symbolicate a crash report. SC-006 names the design stub file (`docs/design/stub-mobile.html`) — that IS the success-criterion vocabulary because the visual identity is defined relative to the stub.

- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded

> §Scope cutoff explicitly names what's NOT in this feature (live CDN fetch against real `repository.eafora.org`, SSG content surfaces beyond in-app sheets, `core/`-change watcher, design-token codegen, App Intents, push / iCloud / WidgetKit, Android). Each item is paired with the architectural reference that documents WHY it's out of scope and the trigger / follow-up that would pull it in.

- [x] Dependencies and assumptions identified

> §Assumptions names: (a) `core/` crate prerequisite (shared with the 003-web-client feature; either-can-block-on-it but they don't block each other); (b) `ingestion build --downsampled` prerequisite (same as web); (c) `repository.eafora.org` is NOT a prerequisite (same stub-shaped fallback path web uses); (d) Apple Developer Program enrollment (gates P4 only; everything else works without it); (e) Mac mini M1 CI environment with Xcode 16+ and App Store Connect API key secrets; (f) `uniffi-bindgen-swift` per-artifact invocation pattern; (g) wgpu Metal-from-`CAMetalLayer` API stability; (h) XcodeGen YAML schema stability; (i) `MTKView.isPaused` + `setNeedsDisplay()` semantics on iOS 18+; (j) `xcodebuild -exportArchive` direct-upload behavior; (k) AASA Worker route precedence; (l) Cloudflare-registered domain.

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria

> Mapping (each FR-N has at least one acceptance scenario or success criterion that exercises it):
> - FR-001 / FR-002 → P1 acceptance #1 (Xcode project builds end-to-end) + SC-001 (setup-to-first-paint under 10 min)
> - FR-003 / FR-004 / FR-005 → P1 acceptance #1 (Xcode pre-build phase produces the xcframework)
> - FR-006 / FR-007 → SC-001 (setup.sh prereq for the full first-paint flow)
> - FR-008 / FR-009 / FR-010 / FR-011 → P1 acceptance #1 implicitly (the FFI surface IS what the app builds against) + SC-009 (revision visible in two surfaces, including `eafora.revision()`)
> - FR-012 / FR-013 / FR-014 → P1 acceptance #1 (embedded bundle loads + renders)
> - FR-015 / FR-016 / FR-017 / FR-018 → P1 acceptance #1 + edge case "MTKView's CAMetalLayer is recreated" (attach/detach lifecycle)
> - FR-019 / FR-020 / FR-021 / FR-022 / FR-023 / FR-024 → P2 acceptance #1 through #5 (file-system cache adapter contract + iOS-purge recovery)
> - FR-025 / FR-026 / FR-027 / FR-028 / FR-029 → P3 acceptance #1 through #5 (discovery + speculative fetch + verify-in-Rust + persist + hot-swap)
> - FR-030 / FR-031 / FR-032 / FR-033 / FR-034 → P1 acceptance #2 (region detail sheet) + P4 acceptance #3, #4 (Universal Link routing to sheets) + SC-006 (visual identity)
> - FR-035 / FR-036 / FR-037 → SC-006 (visual identity matches design stubs)
> - FR-038 / FR-039 / FR-040 / FR-041 → Constitution Check item I + the localization-discipline note (no acceptance scenario directly tests it; the compile-time check is implicit in builds succeeding)
> - FR-042 / FR-043 → SC-009 (revision visible in two independent surfaces)
> - FR-044 / FR-045 / FR-046 / FR-047 / FR-048 → P4 acceptance #2, #3, #4 (AASA file + Universal Link routing)
> - FR-049 / FR-050 / FR-051 / FR-052 / FR-053 → P4 acceptance #1 (TestFlight-ready build) + SC-007 (CI uploads)
> - FR-054 → SC-003 (XCTest cache suite passes)
> - FR-055 / FR-056 / FR-057 / FR-058 → exercised by their respective XCTest suites; the test suites are themselves the deliverable
> - FR-059 → directly observable: there is no XCUITest target / `xcodebuild test` UI scheme in the project file

- [x] User scenarios cover primary flows

> P1 = first-paint map render against the embedded bundle on the simulator (the deliverable that closes the `client-ios.md` §Follow-up work item; reachable without Apple Developer Program enrollment). P2 = file-system cache adapter persistence + iOS purge recovery (the verifiable surface the live-fetch path is built on). P3 = discovery + speculative parallel fetch + hot-swap (the full live path; testable against a same-origin stub before the producer side stands up the real CDN). P4 = TestFlight-ready build + Universal Links (the end-to-end distribution path, gated on Apple Developer Program enrollment + AASA Worker deployment).

- [x] Feature meets measurable outcomes defined in Success Criteria

> Every SC-N is verifiable from observable system state: wall-clock build time (SC-001 / SC-002), test result (SC-003), live-bundle hot-swap latency (SC-004), cached-bundle first-paint latency (SC-005), side-by-side visual comparison (SC-006), CI workflow logs (SC-007), end-to-end Universal Link behavior on a real device (SC-008), revision visible in Settings + git (SC-009).

- [x] No implementation details leak into specification

> See note under §Content Quality item #1. The named technologies are locked architecture, not implementation. Within those constraints the spec resists naming specific function signatures where the architecture doesn't already lock them down — e.g. FR-055's `URLProtocol` interception is named because that's the standard iOS pattern for mocking `URLSession` in tests; FR-009's `WindowHandle` shape is named because the iOS architecture doc commits to that exact shape and the spec must propagate it.

## Notes

- All checklist items pass on first iteration.
- Zero `[NEEDS CLARIFICATION]` markers were emitted. Reason: every potentially-ambiguous decision had a load-bearing default in the architecture docs (`client-ios.md`, `client.md`, `overview.md`, `design/README.md`) or in saved feedback memory. Where the architecture doc itself flagged a "to verify" item (`uniffi-bindgen-swift` flags, wgpu Metal API name, MTKView `isPaused` semantics, `xcodebuild -exportArchive` upload flow, AASA Worker route precedence), the spec's §Assumptions section names the verification step rather than blocking on it.
- Branching note for review: the spec is on `004-ios-client` branched off `master` (NOT stacked on `003-web-client`). Rationale: the two features modify entirely different file trees (`web/` vs `ios/` + `tools/aasa-deploy/`); the only shared concern is the existence of `core/` and the existence of `scripts/sync-embedded-bundle.sh`, both of which are prerequisites that land before either feature begins implementation. Per the constitution's Git workflow §Branch per body of work, branches stack only when phases are serial and depend on each other — these are parallel deliverables.
- Spec is ready for `/speckit-clarify` (if reviewer surfaces ambiguity) or `/speckit-plan` (to begin implementation planning). Per `feedback_spec_and_plan_same_pr.md`, both land in the same PR.
