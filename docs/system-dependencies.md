# System dependencies

What a contributor needs installed to build, test, and run Eafora. This is the
human-readable companion to `setup.sh`, which automates the macOS bootstrap
(`setup.sh` itself only checks for `brew`, `cargo`, and `dbmate`, then installs
PostgreSQL and applies migrations; the rest of the toolchain below is not
auto-installed).

> Naming note: filed as `system-dependencies.md` (kebab-case) to match the rest
> of `docs/`. Rename if you prefer the underscore form.

## Quick start (macOS)

1. Install the prerequisites `setup.sh` expects: `brew` (https://brew.sh),
   the Rust toolchain (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`),
   and `dbmate` (`brew install dbmate`).
2. Run `./setup.sh` (optionally `./setup.sh <master-secret>`). It generates
   `.env` from `template.env`, installs and starts `postgresql@18`, creates the
   `eafora` database, and applies migrations to both `eafora` and `eafora_test`.
3. For the WASM test toolchain and anything not covered by `setup.sh`, install
   the tools in the sections below.

## Rust toolchain

- `rustup` — the toolchain manager. Install via https://rustup.rs. The repo
  pins its channel in `rust-toolchain.toml` (currently `1.95`); `rustup`
  installs and selects that channel automatically on the first `cargo`
  invocation, so you do not pin it by hand.
- `cargo` / `rustc` — come with the toolchain.
- A C compiler / linker — `rusqlite` is built with its `bundled` feature, which
  compiles SQLite from C source. macOS: Xcode Command Line Tools
  (`xcode-select --install`). Linux: `gcc` or `clang` plus `make`.
- The `wasm32-unknown-unknown` target — required to build and test the `shared`
  crate for the web. Install with `rustup target add wasm32-unknown-unknown`.

## Database (ingestion tests and running the pipeline)

The `shared` crate builds and tests with no database (its `sqlx` queries use the
committed `.sqlx` offline cache). A database is needed only for the `ingestion`
crate's integration tests and for running the ingestion pipeline.

- PostgreSQL 18 — the canonical store. `setup.sh` installs `postgresql@18` via
  Homebrew and runs it as a launchd service. Manual: `brew install postgresql@18`
  (macOS) or your distro's `postgresql` package (Linux).
- PostgreSQL client tools — `createdb`, `dropdb`, `psql`. Ship with the server
  package; used by `setup.sh` and `scripts/db/setup-test-db.sh`.
- `dbmate` — the migration runner; applies `ingestion/db/migrations`. Install
  with `brew install dbmate` (or see https://github.com/amacneil/dbmate).
- `secr` — the secret-management CLI the owner maintains. `setup.sh` uses it
  (`secr key` to generate a master secret) and the binary decrypts `secrets.yaml`
  at runtime. Obtain it from the owner / its crate; confirm the install command.

## WASM tests (headless browser)

The `shared` crate's wasm32 tests run in a real headless browser via
`wasm-bindgen-test`.

- `wasm-pack` — drives the wasm test harness (installs `wasm-bindgen-cli` and the
  test runner on first use). Install with `cargo install wasm-pack`.
- Google Chrome (or Chromium) — the headless browser the tests run in.
- `chromedriver` — drives the headless browser. Treat it like any other tool: it
  must be on your `PATH`, and its **major** version must match the installed
  Chrome (`wasm-pack` finds it on `PATH` automatically). Homebrew's `chromedriver`
  cask tends to track the newest Chrome and skew ahead of a slightly older
  installed Chrome, so install the version-matched driver from Chrome for Testing
  instead. Resolve the URL for your Chrome milestone at
  https://googlechromelabs.github.io/chrome-for-testing/ , then install it into a
  directory on your `PATH` (the example uses `~/.local/bin`; pick any `PATH`
  directory). For example on Apple Silicon:

  ```sh
  cd /tmp && curl -sSL -o chromedriver.zip "https://storage.googleapis.com/chrome-for-testing-public/<version>/mac-arm64/chromedriver-mac-arm64.zip" && unzip -o chromedriver.zip && mkdir -p ~/.local/bin && mv -f chromedriver-mac-arm64/chromedriver ~/.local/bin/chromedriver && codesign --force --sign - ~/.local/bin/chromedriver && chromedriver --version
  ```

Run the wasm tests with the wrapper script, which runs from `shared/` (`wasm-pack`
needs a package manifest, not the workspace root) and forwards extra arguments to
`wasm-pack`:

```sh
./scripts/test/test-wasm.sh
```

It is equivalent to `cd shared && wasm-pack test --headless --chrome`.

## Web site build and deploy

- `brotli`: needed by the deploy gate, since `scripts/build/verify-site-tree.sh` decodes each embedded shard to
  read its schema version, and by the perf-budget report, which pipes the remaining assets through it to
  estimate transfer size. Neither writes a compressed file into the tree. Install with `brew install brotli`.
- `jq` — the same report reads the built manifests with it. Install with `brew install jq`.
- `node` — only for `npx wrangler`, which `scripts/build/deploy-site.sh` invokes to upload the site.
  Install with `brew install node`, then authenticate once with `npx wrangler login`.
- `wasm-opt` is deliberately absent: cargo-leptos downloads and caches the binaryen release it pins on
  the first release build, which keeps the optimizer tied to the toolchain rather than to Homebrew. That
  first build needs network access to `github.com` and its release-asset host.

## Contribution workflow

- `git` — version control; the `scripts/git/branch-init.sh` and
  `scripts/git/pr-integrate.sh` flows wrap it.
- `gh` — the GitHub CLI, used to open PRs (`gh pr create`). Install via
  https://cli.github.com.

## Spec-driven workflow (optional)

Per the working agreement in the repo root `CLAUDE.md`, feature work uses GitHub
Spec Kit.

- `uv` — Python tool runner used to install Spec Kit. Install via
  https://docs.astral.sh/uv/ (`brew install uv`).
- Spec Kit — `uv tool install specify-cli --from git+https://github.com/github/spec-kit.git@<tag>`
  (check Releases for the current tag).

## Configuration (not installed, but required)

- `.env` — generated by `setup.sh` from the committed `template.env`; holds
  `DATABASE_URL`, `TEST_DATABASE_URL`, and `MASTER_SECRET`. Not committed
  (`.gitignore`d); `template.env` is the committed source of truth.
- `MASTER_SECRET` — required by `setup.sh` (first argument or an existing `.env`
  value). Generate one with `secr key`. `secr` decrypts the committed
  `secrets.yaml` against this master secret at runtime.

## Notes

- Building does not require a database: the committed `.sqlx` offline cache
  satisfies `sqlx`'s compile-time query checks (`cargo build` / `cargo check`
  work offline). Running `ingestion`'s integration tests does require a database
  (`scripts/db/setup-test-db.sh` provisions `eafora_test`).
- Routine verification commands: `cargo test --workspace` (host) and
  `./scripts/test/test-wasm.sh` (wasm32).
- A Nix-based reproducible dev environment is under consideration; see
  `docs/research/nix-reproducible-dev.md`. It would subsume most of this list,
  but is not yet adopted.
