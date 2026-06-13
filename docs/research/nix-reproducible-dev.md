# Nix for reproducible local development

## Status

Future investigation — not yet started. Capturing scope so it isn't lost.

## Question

Should Eafora adopt Nix (or `nix-direnv`, or `devenv`, or `flake.nix`) to pin and
reproduce the local development environment?

## Today's pain (the things Nix would replace)

The current bootstrap is `setup.sh` plus a few documented prerequisites. The
versions of these tools are not pinned anywhere machine-readable:

- **Postgres** — `setup.sh` installs `postgresql@18` via Homebrew. The exact
  point release floats with Homebrew. Two developers on different `brew update`
  cadences run different Postgres builds.
- **Rust toolchain** — no `rust-toolchain.toml` checked in. Each developer's
  `rustup default` picks the version. CI (when it exists) will need its own
  pin.
- **`secr`** — installed via `cargo install secr`. The version is whatever
  was on crates.io at install time. The secret store format is currently
  stable, but a future `secr` major version could break decryption.
- **`dbmate`** — installed via `brew install dbmate`. Same float problem as
  Postgres.
- **`sqlx-cli`** — installed via `cargo install sqlx-cli` for `cargo sqlx
  prepare`. Version drift could change the `.sqlx/` cache format.
- **AWS SDK toolchain** — only relevant when running R2 publishes; the
  `aws-sdk-s3` crate version is pinned in Cargo.toml, but the system OpenSSL /
  rustls version it links against on macOS is whatever the OS ships.
- **Natural Earth shapefile zip** — already pinned by sha256 inside
  `samples/natural_earth/`, so this is fine.

## What "good" looks like

A new contributor (or the user on a fresh machine) clones the repo, runs one
command, and gets the exact toolchain Eafora expects. CI runs the same
toolchain. No `brew update` lottery, no `cargo install --locked` boilerplate,
no "works on my machine."

## Candidates to evaluate

- **`nix` flakes (`flake.nix`)** — pure, reproducible, `nix develop` enters a
  shell with every pinned tool on PATH. Highest reproducibility; steepest
  learning curve.
- **`devenv.sh`** — Nix-backed, but with friendlier YAML/Nix-lite config.
  Includes Postgres-as-a-service (`processes` block) which would replace
  Homebrew-launchd entirely.
- **`nix-direnv`** — auto-activates the Nix shell when `cd`-ing into the repo.
  Quality-of-life on top of either of the above.
- **Stay with Homebrew + add a `Brewfile`** — pins via `brew bundle`. Lower
  ceiling on reproducibility (Homebrew formulas are themselves moving
  targets), but trivial to adopt and matches macOS muscle memory.

## What to investigate

1. Can `devenv` (or a flake) run a Postgres instance on a per-project basis
   without launchd? Eafora currently relies on `brew services start
   postgresql@18` and a port mutation in `postgresql.conf`. A per-project
   Postgres (with its own `data/` and port) would eliminate the global config
   edit.
2. How does `cargo` interact with Nix? Specifically: does the Nix-provided
   Rust toolchain play nicely with `cargo sqlx prepare`'s offline cache and
   with `cargo build --release` for the launchd-installed ingestion binary?
   Or do we keep `rustup` and only use Nix for the system-level deps?
3. Can `secr` (and `minimer`) be expressed as Nix derivations, or do we keep
   `cargo install` for owned crates and only Nix-manage the third-party
   deps?
4. macOS-only support is sufficient for v0.9. Linux dev would be a nice
   side-effect but is not a goal. Confirm chosen approach doesn't bake in
   anything Linux-specific that we'd later have to back out.
5. CI implications: GitHub Actions has first-class Nix support
   (`cachix/install-nix-action`). If we adopt Nix, the same `flake.nix`
   reproduces the CI environment.

## Decision criteria

Worth adopting if all of:

- One-command bootstrap from clean clone to working dev environment.
- Per-project Postgres (no `launchd` global state mutation).
- CI runs the same pinned toolchain via the same config file.
- Cost to adopt is < 1 day of work.

Worth deferring if:

- Nix-on-macOS is still flaky for our specific tool mix (Postgres-with-PostGIS
  potential future, Rust toolchain, AWS SDK linkage).
- Adoption requires us to rebuild `secr` / `minimer` as derivations and the
  effort isn't justified by the reproducibility gain.

## Out of scope (for the investigation, not for the project)

- Containerizing the Eafora ingestion binary itself for production deployment
  (that's a separate question — the local dev env and the production runtime
  are different problems).
- Replacing `cargo` itself.
