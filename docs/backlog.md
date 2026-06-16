# Backlog

Deferred work items captured for future pickup. Each entry: what, why deferred, what triggers picking it up.

This file is **not** a roadmap — items here have no committed timeline and no priority order. The roadmap and current sequencing live in `docs/product/product-plan.md`, per-feature spec docs under `specs/`, and the active branch list. Items move out of this file when they enter active work (become a spec, a branch, or an inline plan in a relevant architecture doc).

Add new entries at the bottom of the relevant section. When picking up an item, delete its entry here as part of the same PR that starts the work.

## Ingestion / producer

- **Decouple geometry from per-version artifact bundles.** The current shape re-uploads the full geometry file under every published `version_label`. Geometry changes rarely (Natural Earth boundary updates are infrequent; subnational additions happen in discrete v2+ steps), so re-publishing it every version wastes R2 storage and bandwidth. A future shape publishes geometry to its own content-addressed key shared across versions, with the per-version manifest referencing it by URL (absolute, or rooted somewhere other than the version directory). Affects the publish flow and the manifest's `relative_path` resolution rule; invisible to consumers as long as they resolve URLs from the manifest entry rather than string-formatting paths. **Trigger:** R2 storage cost or upload time becomes a real signal. See `docs/architecture/ingestion.md` and `docs/architecture/client.md` §Decisions still open for related design notes.
- **`docs-architecture-secrets` mini-plan.** Document which secrets the ingestion binary needs (R2 credentials initially; nothing else through v2) and how `secr` integrates with the launchd entrypoint. Currently the wiring works and is captured implicitly across `setup.sh`, `secrets.yaml`, and `ingestion/src/secrets.rs`, but no single doc explains the contract. **Trigger:** a second secret enters the system (e.g. a non-R2 upstream API key), or onboarding friction surfaces the gap.

## Client

(none yet — open client-side design questions live in `docs/architecture/client.md` §Decisions still open until they earn deferral as concrete work)

## Infrastructure / ops

- **Adopt Nix (or `devenv` / `flake.nix`) for reproducible local development.** The current bootstrap is `setup.sh` plus Homebrew, with most tool versions floating across `brew update` cadences (Postgres point release, dbmate, system OpenSSL/rustls linkage, etc.) and Rust pinned only by each developer's `rustup default`. Eafora is single-contributor today so the drift hasn't bitten, but a second contributor or a CI environment would surface it immediately. Decision criteria, candidate tools (flakes, `devenv`, `nix-direnv`, Brewfile fallback), and what to investigate are captured in `docs/research/nix-reproducible-dev.md`. **Trigger:** a second contributor needs to set up the repo, OR `setup.sh` breaks on a point-release float, OR CI gains a pinned-toolchain requirement.

## Product

- **`docs-product-funder-shortlist`.** Research a concrete list of named funders (pro-natal foundations, family-policy think tanks, "civic data" infrastructure funders) with each one's grant size, fit, and application cycle. Distinct from the strategic product plan; takes real work to get right. **Trigger:** funder conversations become a realistic near-term goal (e.g. v1 demo is presentable).
- **`docs-product-pitch-deck`.** Distill `docs/product/product-plan.md` into a 6–10 slide deck plus a 2-page proposal. **Trigger:** a real funder conversation is on the calendar.
- **`docs-product-launch-plan`.** v1.5 public-launch plan: who to contact, what to post where (Hacker News, demographic-Substack writers, journalists), what to have ready. **Trigger:** v1.5 is built.
