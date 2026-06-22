# Specification Quality Checklist: core/ — data layer

**Purpose**: Validate specification completeness and quality before proceeding to planning
**Created**: 2026-06-22
**Feature**: [spec.md](../spec.md)

## Content Quality

- [x] No implementation details (languages, frameworks, APIs)
- [x] Focused on user value and business needs
- [x] Written for non-technical stakeholders
- [x] All mandatory sections completed

> Notes: this is a Rust-internal extraction spec — its "users" are the developer building the clients (003 / 004 / 006 and beyond) and the existing producer (ingestion). Naming `core/`, `serde`, `rusqlite`, `tokio::sync::watch`, the `ArtifactCache` trait shape is necessary because those decisions are locked at the architecture level (`docs/architecture/client.md` + `docs/architecture/overview.md`); the spec implements against them rather than re-deriving them.

## Requirement Completeness

- [x] No [NEEDS CLARIFICATION] markers remain
- [x] Requirements are testable and unambiguous
- [x] Success criteria are measurable
- [x] Success criteria are technology-agnostic (no implementation details)

> SC-001 through SC-006 are observable from CI output (build success, test pass rate) or from concrete byte-equality / import-resolution checks. SC-006 is forward-looking (verifiable when the 003 / 004 implementation PRs reach for the symbols) — flagged as "verifiable when those PRs go up; not gated on this PR's CI."

- [x] All acceptance scenarios are defined
- [x] Edge cases are identified
- [x] Scope is clearly bounded

> §Scope cutoff explicitly names what's NOT in this feature (wgpu pipelines + WGSL, `core::geometry`, `core::map::map_renderer::Renderer`, FFI modules, the bulk of canonical_model entity types). Each cut item is paired with where it lives instead (006-core-renderer, 003 / 004, ingestion-stays-in-place).

- [x] Dependencies and assumptions identified

> §Assumptions names: ingestion stays canonical for everything that doesn't move; stable AFIT requires rustc 1.75+; the existing manifest tests are the regression net for the type extraction; `Manifest`'s `Serialize` impl must be byte-equal to the producer's current output; rusqlite `bundled` on WASM is the v1 SQLite path with sqlite-wasm-rs as the fallback; entity types stay in ingestion; 006-core-renderer stacks on this branch.

## Feature Readiness

- [x] All functional requirements have clear acceptance criteria

> Mapping:
> - FR-001 / FR-002 / FR-003 / FR-004 → P1 acceptance #1, #2 (workspace member compiles for host + wasm32)
> - FR-005 / FR-006 → P1 acceptance #3, #4 (ingestion-side tests pass post-move; re-exports in place)
> - FR-007 / FR-013 → P1 acceptance #3 + SC-005 (producer-side regression net + byte-equal round-trip)
> - FR-008 / FR-009 → P5 acceptance #2 (mismatch surfaces the documented error message shape) + P5 acceptance #5 (sha256_hex stays consistent with producer-side output)
> - FR-010 / FR-011 / FR-012 → P2 acceptance #1 through #4 (parse round-trip, malformed rejection, path-traversal rejection)
> - FR-014 / FR-015 → P4 acceptance #1 through #4 (discovery parse round-trip, unknown schema_version rejection, sunset Option)
> - FR-016 / FR-017 → P3 acceptance #1, #2, #3 (trait shape + !Send compatibility + Rust-1.75-AFIT)
> - FR-018 / FR-019 / FR-020 → P5 acceptance #1, #2, #3 (Bundle::open round-trip; SHA-256 mismatch; license-scoped attach)
> - FR-021 / FR-022 → P6 acceptance #1, #2, #3 (DistributionContext slices; compile error on new variant)
> - FR-023 → P5 acceptance #4 (watch-channel hot-swap visible to the renderer's Receiver)
> - FR-024 / FR-025 → SC-004 (test suite passes 100%) + cross-target coverage

- [x] User scenarios cover primary flows

> P1 = workspace member exists + the type extraction lands cleanly. P2 = manifest parsing (the consumer-side contract). P3 = `ArtifactCache` trait (the cross-platform cache contract both clients implement). P4 = discovery document parsing (the v1 schema validation gate). P5 = `Bundle::open` (the loader both clients consume). P6 = license authorization (the `DistributionContext` matrix). Together they exercise every public symbol the clients reach for.

- [x] Feature meets measurable outcomes defined in Success Criteria

> Every SC is verifiable from observable build / test output, byte-equality assertions, or import-resolution checks at downstream PR time.

- [x] No implementation details leak into specification

> The architecture's locked decisions are surfaced where required (Rust 1.75 AFIT, `tokio::sync::watch`, `rusqlite`, BTreeMap-deterministic-serialization); plan-level decisions (whether `MockArtifactCache` is `#[cfg(test)]` or a feature gate; whether the bundled SQLite WASM path uses `rusqlite` or `sqlite-wasm-rs`; the exact `core/Cargo.toml` shape) are explicitly deferred to plan time.

## Notes

- All checklist items pass on first iteration.
- Zero `[NEEDS CLARIFICATION]` markers. Every potentially-ambiguous decision had a load-bearing default in the architecture docs or in the existing ingestion-side implementation.
- The `Region` / `Country` / `Statistic` / etc. entity-type move-or-stay decision deliberately defers to "stay in ingestion until a client needs them" rather than over-pulling on first pass; this keeps the extraction surface tight per the spec's stated motivation of unblocking 003 / 004.
- Spec is ready for `/speckit-clarify` (if reviewer surfaces ambiguity) or `/speckit-plan`. Per `feedback_spec_and_plan_same_pr.md`, both land in the same PR.
