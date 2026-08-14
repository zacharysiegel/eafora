# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **Note for the next session**: this file was written as a one-shot handoff
> from the previous session that brainstormed the name and reserved external
> properties. It is not load-bearing. Once the project's actual conventions and
> architecture are established (after `specify init` and `/speckit.constitution`),
> rewrite this file freely — clobber any of the content below that no longer
> serves you. The decisions in `docs/product/brand.md` are the only thing worth preserving
> verbatim.

## Project

**Eafora** is an interactive global atlas for fertility data — TFR, effective TFR,
completed fertility rate, and related demographic indicators — built to raise
public awareness of the global fertility crisis and dispel common myths through
education.

The product is, at minimum, a web application; the build will also include
native iOS and Android apps. The owner is using this project to practice
multiplatform application development as preparation for a more ambitious
parallel project (a native game, currently a Rust macOS app, eventually mobile).

See `docs/product/brand.md` for naming, etymology, and external property reservations.

## Stage

Greenfield. The repository contains brand documentation only — no source code,
no tooling, no architecture yet. The next concrete step is initializing
[GitHub Spec Kit](https://github.com/github/spec-kit) and drafting a project
constitution.

## Working agreement

This project uses two complementary tool layers:

1. **Spec Kit** for artifact structure. Per-feature workflow:
   `/speckit.constitution` → `/speckit.specify` → `/speckit.clarify` →
   `/speckit.plan` → `/speckit.tasks` → `/speckit.implement`. Artifacts live in
   `specs/NNN-slug/{spec,plan,tasks}.md`. Templates and scripts live in
   `.specify/`; slash-command prompts live in `.claude/commands/speckit.*.md`.
   Both directories are committed.

2. **Superpowers skills** for behavioral discipline (brainstorming before
   design, TDD before implementation, verification before claiming done,
   debugging before fixing, code review, parallel-agent dispatch, worktree
   isolation). Layered on top of the Spec Kit flow.

The web client (and later native clients) must run fully on this machine: discovery, the live artifact tree, and first-paint embedded files are same-origin or local-directory. Developing or verifying a feature must not require `eafora.org` or `repository.eafora.org` to be up.

Project-wide research/positioning docs that don't fit the per-feature mold
(product plan, data-source survey, architecture overview) go in `docs/`.

## Conventions

Cross-cutting coding rules live in `docs/conventions/`. The README there is
the index. Read the relevant doc before writing code in that area:

- `docs/conventions/types.md` — Rust type naming (Model + Entity/Projection/Serial pairs,
  enum `Kind` suffix, `TryFrom<&str>` parsing, db.rs variable naming).
- `docs/conventions/logging.md` — log message format (`<message>; [key=value ...]`).
- `docs/conventions/shading.md` — WGSL matrix naming (`<source_space>_to_<destination_space>`).
- `docs/conventions/conditional-compilation.md` — gate target/feature-specific code in one
  `#[cfg]`-ed submodule (not per item), with a one-line WHY on each gate.

These take precedence over Singularity's conventions where they diverge (per
the doc's "Where Eafora diverges" notes). The constitution (`.specify/memory/constitution.md`)
holds principles; these docs operationalize them.

## Decisions locked

- **Name**: Eafora (Old English for *son, descendant, heir*).
- **Repository shape**: monorepo. All platforms and the data pipeline live in
  one repo. Subdirectory layout to be decided during `/speckit.plan` for the
  architecture spec.
- **Tooling**: Spec Kit + Superpowers (see above).
- **Crate placement**: the published `eafora` crate on crates.io is currently a
  proprietary placeholder (`v0.0.1`). Replace with a real crate from inside the
  monorepo once the workspace structure is decided.
- **Initial license**: most-restrictive proprietary "all rights reserved" for
  now. Revisit when public release approaches.

## Decisions still open

- Workspace layout for the monorepo (e.g. `web/`, `ios/`, `android/`, `backend/`,
  `data/`, plus shared Rust crates).
- Frontend technology choice for each platform — owner has expressed strong
  interest in Rust + WASM where practical, and in finding a coherent
  multiplatform strategy that can carry over to the parallel game project.
- Data sources to integrate (World Bank API is the obvious starting point; the
  goal is to merge multiple sources and become *the* leading aggregator).
- Whether a backend + database is required for v1, or whether v1 can ship with
  static data bundled in the app.
- Map projection details and rendering approach (preference: humped-projection
  world map; vector political boundaries; smoothed hover scaling without
  enlarging the input hit zone).
- Monetization model (no firm direction; possibilities include nonprofit
  funding, grants, sponsorships from pro-natal organizations).

## Immediate next steps for a new session

1. Verify `uv` is installed (`uv --version`).
2. Install Spec Kit:
   `uv tool install specify-cli --from git+https://github.com/github/spec-kit.git@v0.8.12`
   (check Releases for newer tag).
3. Initialize:
   `cd /Users/singularity/eafora && specify init . --integration claude`.
4. Inspect what was created. Commit `.specify/` and `.claude/commands/speckit.*.md`.
5. Run `/speckit.constitution` to draft project principles. Useful inputs:
   - Rust where practical, including frontend (WASM) where it doesn't fight the
     platform.
   - Multiplatform discipline — favor architectures whose lessons carry over to
     the owner's parallel game project, even when that introduces complexity
     not strictly necessary for Eafora itself.
   - Mission framing: education and myth-dispelling, not advocacy or alarmism.
6. Create the first feature spec via `/speckit.specify` for whichever surface we
   tackle first (likely the data ingestion pipeline or the web app's map view).
7. Update this `CLAUDE.md` as decisions land.

## Repository layout (current)

```
.
├── .claude/        # Claude Code session machinery (will gain commands/ once spec-kit is initialized)
├── .git/
├── CLAUDE.md       # This file
└── plans/          # Vestigial; will be removed in favor of spec-kit's specs/ and a docs/
```

## Owner preferences

The owner has extensive global preferences in `~/.claude/CLAUDE.md` covering
code style, commit hygiene, tool usage, and process conventions. Read them.
Notable items relevant here:

- Prefer Rust over Python for application work.
- Strong opinions on Java, SQL, and Rust style — see global file.
- Never include attribution lines in commits (no "Co-Authored-By", no
  "Generated with Claude Code").
- Never use `git add -A` or `git add .`. Stage by explicit path.
- When implementing a planned change, report deviations from the plan
  afterward and update the plan doc with notes about the deviations.

<!-- SPECKIT START -->
Active plan: `specs/003-web-client/d-plan.md`. Spec: `specs/003-web-client/spec.md`. Branch: `d-plan`. For additional context about technologies to be used, project structure, shell commands, and other important information, read these files first.
<!-- SPECKIT END -->
