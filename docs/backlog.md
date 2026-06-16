# Backlog

Deferred work items captured for future pickup. Each entry: what, why deferred, what triggers picking it up.

This file is **not** a roadmap — items here have no committed timeline and no priority order. The roadmap and current sequencing live in `docs/product/product-plan.md`, per-feature spec docs under `specs/`, and the active branch list. Items move out of this file when they enter active work (become a spec, a branch, or an inline plan in a relevant architecture doc).

Add new entries at the bottom of the relevant section. When picking up an item, delete its entry here as part of the same PR that starts the work.

## Ingestion / producer

- **Decouple geometry from per-version artifact bundles.** The current shape re-uploads the full geometry file under every published `version_label`. Geometry changes rarely (Natural Earth boundary updates are infrequent; subnational additions happen in discrete v2+ steps), so re-publishing it every version wastes R2 storage and bandwidth. A future shape publishes geometry to its own content-addressed key shared across versions, with the per-version manifest referencing it by URL (absolute, or rooted somewhere other than the version directory). Affects the publish flow and the manifest's `relative_path` resolution rule; invisible to consumers as long as they resolve URLs from the manifest entry rather than string-formatting paths. **Trigger:** R2 storage cost or upload time becomes a real signal. See `docs/architecture/ingestion.md` and `docs/architecture/client.md` §Decisions still open for related design notes.

## Client

(none yet — open client-side design questions live in `docs/architecture/client.md` §Decisions still open until they earn deferral as concrete work)

## Infrastructure / ops

(none yet)
