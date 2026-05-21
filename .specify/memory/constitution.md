<!--
SYNC IMPACT REPORT
==================
Version: 0.0.0 → 1.0.0 (initial ratification)
Modified principles: N/A (initial ratification)
Added principles:
  - I. Educational neutrality (NON-NEGOTIABLE)
  - II. Source provenance (NON-NEGOTIABLE)
  - III. Rust core, native UI shells
  - IV. Singularity convention parity
  - V. Explicit over implicit
  - VI. CDN-delivered data, no live data API through v2
  - VII. Test-first for core logic
  - VIII. Workflow discipline
Added governance subsections:
  - License
  - Versioning
  - Boundary recognition
  - Application code language
  - Git workflow
  - Tooling discipline
  - Amendment procedure
  - Compliance review
Removed sections: N/A
Templates requiring updates:
  - .specify/templates/plan-template.md       (⚠ pending — Constitution Check section to align with the 8 principles below; updated in this branch if needed)
  - .specify/templates/spec-template.md       (⚠ pending — confirm scope sections accommodate provenance and constitution-check references; updated in this branch if needed)
  - .specify/templates/tasks-template.md      (⚠ pending — task categorization should reflect TDD-for-core-logic and explicit-over-implicit; updated in this branch if needed)
  - .specify/templates/checklist-template.md  (⚠ pending — confirm checklist categories align with the 8 principles; updated in this branch if needed)
Follow-up TODOs:
  - License revisit before any public source release (Governance §License)
  - Replace placeholder `eafora` crate (currently 0.0.x on crates.io) with a real crate from inside the monorepo when workspace structure lands (Governance §Versioning)
  - Confirm GitHub repo settings preserve empty commits during "rebase and merge" (the spec-kit-bootstrap PR initially merged without its marker; resolved manually by the owner this branch). If the setting drops empty commits, the marker convention is unenforceable for merged PRs.
-->

# Eafora Constitution

## Core Principles

### I. Educational neutrality (NON-NEGOTIABLE)

Eafora is a data visualization and aggregation tool. The product MUST NOT contain editorial copy, opinion, or advocacy. UI text MUST be limited to labels, units, source attribution, and standardized indicator definitions. Users learn by exploring data and following links to primary sources (including, but not limited to, Wikipedia and the original publishing source for each datum).

**Rationale**: Aligns with the stated educational and research mission; protects funding optionality; reduces app-store and PR risk associated with politically charged content.

### II. Source provenance (NON-NEGOTIABLE)

Every numeric value displayed in the UI MUST be traceable to a named source, an original retrieval timestamp, and the source's original license. Derived or computed values MUST carry a documented derivation that names the input sources and the formula. Per-source license obligations MUST be tracked at ingestion time and respected at display time (attribution strings, redistribution clauses, share-alike requirements where applicable).

**Rationale**: Credibility is the moat. The "leading aggregator" goal requires receipts; users and funders will judge Eafora on the integrity of its provenance trail.

### III. Rust core, native UI shells

Application logic MUST live in Rust crates. This includes — but is not limited to — data models, indicator math, projection geometry, hit-testing, ingestion, normalization, artifact building, and the WebGPU/Metal/Vulkan rendering pipeline (`wgpu`).

UI MUST be platform-native: Leptos with WASM on the web, SwiftUI on iOS, Jetpack Compose on Android. The Rust core is consumed by clients via WASM bindings on the web and via UniFFI on iOS and Android. Cross-platform UI frameworks that defeat the native-quality goal (e.g. Flutter, React Native, Dioxus-everywhere) MUST NOT be introduced.

**Rationale**: Matches the locked architectural decision documented in this project's brainstorming history. The discipline of a Rust core with native shells is itself a design lesson Eafora exists to teach the owner, in preparation for a parallel and more ambitious project.

### IV. Singularity convention parity

Library, tooling, build, and convention choices MUST default to those used in the owner's parallel project at `/Users/singularity/singularity` (the "Singularity" project). Deviations from Singularity's choices MUST be justified in the relevant spec or plan. New third-party dependencies (Rust crates, build tools, linters, test frameworks, CI services, etc.) MUST be discussed with the owner before adoption.

Locked picks confirmed for Eafora (which match Singularity except where noted):
- HTTP server: **actix-web**
- HTTP client: **reqwest**
- Async runtime: **tokio** with `features = ["full"]`
- Database: **PostgreSQL** via Podman Compose, **sqlx** for queries (using `query_as!` and the offline cache), **dbmate** for migrations
- Serialization: **serde** with **rmp-serde** when MessagePack is appropriate
- Logging: **log** + **env_logger** with the message format `"<message>; [<data>] [<data2>]"`
- Configuration: **dotenvy** for env loading; statics via `LazyLock`
- Secrets: **secr** (the owner's crate)
- Errors: **minimer** (the owner's crate; not yet in Singularity but will be adopted there too — Eafora uses it from the start)
- Rust edition: **2024**, `rustfmt` configured to `max_width = 120`, `chain_width = 100`, `remove_nested_parens = true`

**Rationale**: Consistency between Eafora and Singularity is itself a goal of building Eafora. Lessons learned in one project must transfer cleanly to the other.

### V. Explicit over implicit

HTTP routes MUST be defined imperatively in actix-web. Route attribute macros (`#[get]`, `#[post]`, `#[route(...)]`, etc.) MUST NOT be used. The route tree MUST mirror the module tree: every feature module that owns routes exposes one `pub fn configurer(config: &mut web::ServiceConfig)`, and parent modules compose with `.configure(child::configurer)`.

Database queries MUST be hand-written SQL via `sqlx::query_as!` (or equivalent compile-time-validated SQLx macros). Object-relational mapping libraries that hide queries MUST NOT be adopted.

RPC frameworks (gRPC, tonic, GraphQL, JSON-RPC frameworks, Cap'n Proto, Leptos `#[server]`, tRPC-style code generation, or any RPC-shaped abstraction that hides the wire) MUST NOT be adopted without explicit owner approval per spec. The default network shape is HTTP with JSON bodies, called via reqwest on every client.

**Rationale**: The owner wants to see the wire. Magic that hides HTTP, SQL, or RPC is harder to debug, harder to reason about for performance, harder to adapt when the abstraction breaks, and harder to learn from. Eafora is partly a learning project; explicitness preserves the lessons.

### VI. CDN-delivered data, no live data API through v2

Through v2, data MUST be delivered to clients as versioned, immutable artifacts hosted on a CDN. Geometry artifacts MUST use **PMTiles**. Indicator-data artifacts MUST use **SQLite**. Both MUST be produced by a server-side ingestion pipeline (Rust, scheduled or manual through v1) that writes to a canonical PostgreSQL store and emits the artifacts to the CDN with content-hashed filenames.

Through v2, clients MUST NOT depend on a live application server for hot-path data. A live API for user contributions, interactive search, semantic Q&A, or other online-only features MAY be introduced from v3+ via its own spec, **with no obligation to also support those features without a live server**. Offline operability is a side effect of the v1-v2 architecture, not a product guarantee — it MUST NOT be promised in marketing copy or used to constrain v3+ design.

**Rationale**: Minimizes ops burden during the solo nights/weekends build; keeps the canonical data store available to back a future live API without re-architecting; makes "v3+ adds online-only features" a clean spec rather than a backwards-compatibility crisis.

### VII. Test-first for core logic

The Rust core MUST follow test-first development for: indicator math, projection math, hit-testing, ingestion normalization, source-merge conflict resolution, artifact diffing, and error mapping. Tests MUST be written and reviewed before implementation; the Red-Green-Refactor cycle MUST be respected.

UI shell code (Leptos components, SwiftUI views, Jetpack Compose composables, wgpu shaders, layout code, animation curves) is exempt from strict TDD. UI code SHOULD have integration or visual-regression tests where they add value, but is not held to the test-first discipline.

**Rationale**: TDD pays off where logic is verifiable in isolation. Rigid TDD on rendering produces ceremony without value, and would impede the wgpu-shader learning that motivates this project.

### VIII. Workflow discipline

Per-feature work MUST flow through Spec-Kit: `/speckit-specify` → `/speckit-clarify` (when ambiguous) → `/speckit-plan` → `/speckit-tasks` → `/speckit-implement`. Per-feature artifacts MUST live under `specs/NNN-slug/`.

Project-wide research and architecture documents (product plan, data-source survey, architecture overview, per-platform implementation plans, monetization research) MUST live in `docs/`, outside the Spec-Kit per-feature flow. Project-wide docs are not subject to the per-feature workflow but MUST be kept consistent with this constitution.

Conventions and operating rules that govern the project MUST be recorded in this constitution (under Governance), not in ad-hoc files like `CONTRIBUTING.md`. Spec-Kit's constitution is the canonical home for project-governing rules.

Superpowers skills (brainstorming, TDD, verification, systematic debugging, code review, parallel-agent dispatch, worktree isolation) MAY be layered on top of Spec-Kit where they do not conflict with Spec-Kit. When the two systems give conflicting guidance, Spec-Kit wins.

**Rationale**: Locked workflow decision. Avoids the ad-hoc planning that scatters context across the repo and frustrates future sessions.

## Governance

### License

Eafora is currently licensed proprietary, "all rights reserved." This MUST be revisited before any public source release; license selection (permissive, copyleft, source-available) MUST be a deliberate decision recorded in a spec, not an accidental one.

### Versioning

Any published Rust crate from this monorepo MUST follow Semantic Versioning. The `eafora` crate placeholder currently published on crates.io as `0.0.1` MUST remain at `0.0.x` until replaced by a real crate from inside the monorepo, at which point it MUST be re-versioned cleanly. Internal monorepo crates that are never published MAY use any version scheme but SHOULD use SemVer for consistency.

### Boundary recognition

Cartographic boundaries default to lines recognized by the United States government. The data layer MUST be designed so that an alternate boundary set (e.g. for India-specific or China-specific distribution) can be swapped in without changes to the rendering code. v1 ships a single source. Per-locale boundary swapping is deferred until a real distribution need exists.

### Application code language

Committed application code MUST be Rust. Where Rust is impractical for a specific subsystem (none currently identified), Kotlin, Java, or Go MAY be considered with explicit owner approval per spec.

**Python MUST NOT be used for any code committed to this repository.** Exceptions:
- Python-implemented tools that are unavoidable and invisible to the project (e.g. `uv` and `specify-cli` for GitHub Spec Kit) are acceptable when used as installed tooling, not extended.
- Ad-hoc Python written by the AI assistant for one-shot agentic tasks (data exploration, throwaway scripts in `/tmp`) is acceptable as long as it never lands in the repository.

**Rationale**: The owner has stated a strong preference against Python in committed code (dynamic typing, interpreter version churn, dependency-resolution friction). Rust is the project's default; deviations require deliberate decision.

### Git workflow

Eafora's git workflow is opinionated. Every rule in this section MUST be followed.

**Branch per body of work.** Each cohesive deliverable (a tool scaffold, a doc draft, a spec implementation) MUST be developed on its own short-lived branch off `master`. When phases are serial and depend on each other, branches MUST form a linear stack: each phase branched from the previous one's head, not independently from `master`. Branch names are short kebab-case describing the deliverable (`spec-kit-bootstrap`, `constitution`, `docs-product-plan`).

**Branch marker as the first commit.** Every new branch MUST begin with an empty marker commit whose subject is exactly:

```
>>> branch: <branch-name>
```

This marker compensates for the "rebase and merge" strategy used to land PRs into `master`, which otherwise erases PR boundaries from `master`'s history. To find boundaries in any `git log` view, search the `less` pager with `/>>> branch:` then step with `n` and `N`.

The marker MUST be created by running `./scripts/branch-init.sh <branch-name>` from the repo root. The script creates the branch, places the canonical empty commit, and pushes with upstream tracking. Manual creation of marker commits is permitted only when the script is unavailable (e.g. detached environments).

**Commit cadence.** Commits MUST be small and frequent — one per meaningful checkpoint. Phased work MUST NOT accumulate uncommitted changes across phases.

**Push cadence.** Every commit MUST be pushed immediately after creation. Working trees MUST NOT be left with unpushed commits at the end of a session.

**Commit message style.** Commit subject lines MUST fit on a single line. Multi-line commit message bodies are forbidden. The subject line is the entire message.

**Staging.** Files MUST be staged by explicit path. `git add -A` and `git add .` are forbidden — they accidentally include sensitive files and editor noise. Before every commit, `git status` MUST be inspected to verify only intended files are staged.

**Attribution.** Commit messages and PR descriptions MUST NOT include attribution lines (no "Co-Authored-By: Claude…", no "Generated with Claude Code", no AI assistant tags).

**Pull requests.** When a body of work is complete, a GitHub Pull Request MUST be opened via `gh pr create`. The owner reviews PRs in IntelliJ via its GitHub integration. Direct merges to `master` are forbidden; the PR step MUST NOT be skipped.

**PR assignment.** Immediately after creating a PR, the assignee MUST be set to `zacharysiegel` via `gh pr edit <number> --add-assignee zacharysiegel`.

**PR description.** Every PR description MUST cover (a) the problem motivating the change, (b) the solution at a high level, and (c) a brief test plan. Use markdown formatting.

**Stacked PRs.** When branches are stacked, the PR for each phase targets its parent branch, not `master`. After the parent merges, GitHub will retarget the child PR to `master` automatically.

**Merge strategy.** PRs MUST land in `master` via "rebase and merge". This linearizes history; the branch-marker commits preserve PR boundaries. The repository's GitHub settings MUST be configured to preserve empty commits during rebase-merge so the marker convention is enforceable.

**Force-push policy.** Force-pushes to feature branches MUST use `--force-with-lease` (never plain `--force`) to avoid clobbering remote updates. Force-pushes to `master` are forbidden except for the owner's manual workflow corrections.

**No `--amend` for cleanups.** When a commit is wrong or incomplete, add a new commit. Do not amend a published commit unless the owner explicitly requests it.

**No skipping hooks.** Pre-commit hooks MUST NOT be skipped (`--no-verify` is forbidden) unless the owner explicitly requests it. Hook failures indicate real issues to fix.

### Tooling discipline

**Prefer scripts over LLM orchestration.** When a task is mechanical, deterministic, and likely to be repeated (creating a branch with a marker, running a setup sequence, building an artifact), a checked-in script MUST be written rather than relying on the AI assistant to orchestrate it through tool calls each session.

**Script location.** Utility scripts live under `scripts/` (not at the repo root). Scripts MUST use `#!/usr/bin/env bash` and `set -euo pipefail`.

**Dogfood new scripts.** A newly written script MUST be exercised on the current task before it is considered complete. The first usage validates that it works.

**Reference scripts from the constitution or relevant spec.** When a script implements a convention, the convention's section in this document MUST name the script. Currently registered scripts:
- `scripts/branch-init.sh` — creates a new branch from current HEAD with the canonical marker commit and pushes with upstream tracking. (See Git workflow §Branch marker.)

**Third-party dependencies.** Adding a new third-party Rust crate, build tool, linter, test framework, CI service, or comparable tooling MUST be discussed with the owner before the dependency is committed. Defaults are inherited from Singularity (see §IV).

### Amendment procedure

Constitution amendments are versioned via SemVer:

- **MAJOR**: principle removal, principle redefinition that inverts intent, or governance changes that materially alter how the project is run.
- **MINOR**: principle addition, or material expansion of an existing principle's scope or a governance subsection.
- **PATCH**: clarifications, wording tightening, typo fixes, non-semantic refinements.

Every amendment MUST run the propagation checklist over `.specify/templates/*.md` and update any inconsistent references. Every amendment MUST update the Sync Impact Report at the top of this file.

### Compliance review

Every spec produced via `/speckit-specify` MUST include a "Constitution Check" section that names which principles apply to the proposed work and explains how the design honors them. Specs that violate a principle MUST either propose a constitution amendment or be revised.

---

**Version**: 1.0.0
**Ratified**: 2026-05-21
**Last amended**: 2026-05-21
