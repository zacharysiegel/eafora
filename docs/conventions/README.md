# Coding conventions

Per-topic convention specs that govern the Eafora codebase. Read the relevant doc before writing new code in that area.

These docs are the source of truth. Per-feature plan.md / tasks.md should reference them rather than restating rules. Memory files (in `~/.claude/projects/-Users-singularity-eafora/memory/`) reference them as well.

## Index

- [`types.md`](types.md) — Type naming (Model + Entity/Projection/Serial pairs, `Kind` enum suffix, `TryFrom<&str>` parsing, conversion impl placement, db.rs variable-naming guidance).
- [`logging.md`](logging.md) — Log message format: `<message>; [key=value ...]` for messages with structured data; prose-only messages stand alone.
- [`shading.md`](shading.md) — WGSL/shader naming: transform matrices named `<source_space>_to_<destination_space>` (`object_to_world`, `view_to_clip`), never by pipeline role alone (`model_matrix`) or acronym (`mvp`).

## When to add a doc here

Add a convention doc when a rule:

1. Affects how multiple features get written (not a one-off architectural choice).
2. Has been called out in code review more than once.
3. Has tradeoffs worth recording so future-you doesn't re-derive them.

Don't add a doc just to centralize trivia that's already obvious from the code.

## Relationship to the constitution

The constitution (`.specify/memory/constitution.md`) holds principles. These docs hold rules that operationalize the principles for a specific topic. If a rule here conflicts with a constitutional principle, the constitution wins.
