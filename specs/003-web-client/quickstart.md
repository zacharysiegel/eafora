# Quickstart: building and running the web client

Assumes the `003-web-client` stack is checked out and the workspace builds. Commands are single-line
for copy/paste. Paths are relative to the repo root unless noted.

## Prerequisites (one-time)

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos --locked --version '^0.3'
cargo install wasm-opt --locked
brew install brotli
```

`wasm-opt` is downloaded and invoked by cargo-leptos on release builds; `brotli` is invoked by
`./scripts/build/precompress-site.sh` and `./scripts/build/measure-site-budget.sh`. Both are build-host tools, not
workspace deps.

## Populate the embedded bundle

The map cannot render without a bundle. `web/static/embedded_artifacts/` is gitignored and rebuilt
locally:

```sh
./scripts/build/sync-embedded-bundle.sh ./web/static/embedded_artifacts/
```

The script runs `ingestion build` when no build exists, then plain-copies
`$EAFORA_ARTIFACTS_DIR/latest/downsampled/` into the destination with `cp -R`. Until Phase 0b lands
`ingestion build`, hand-place a stub bundle (a `manifest.json`, one `geometry/*.fgb`, and one
`data/tfr-base-*.sqlite`) under `./web/static/embedded_artifacts/` instead.

## Publish the complete live tree

After the embedded sync, publish the complete bundle into the gitignored local repository the wasm fetches as `/repository`:

```sh
cargo run -p ingestion -- publish local --build --root ./web/static/repository --public-base-url /repository
```

`--build` is global on `publish`. `--root` and `--public-base-url` are on `local`. Restart or refresh `cargo leptos watch` so `/repository/latest/manifest.json` exists. First paint is still the embedded (or OPFS-cached) bundle; the year scrubber gains periods after the live swap.

A second publish of the same `version_label` is rejected by the `artifact_version` uniqueness check. Rebuild with a new label, or point `--root` at a fresh tree after deleting the `artifact_version` row, if you need to overwrite the local static files.

## Dev loop

```sh
cd web && cargo leptos watch
```

Serves the app with rebuild-on-change at the cargo-leptos dev port. Open the printed URL; the map
renders against the embedded bundle. Append `?renderer=webgl2` to force the WebGL2 backend for parity
testing (FR-015).

## Run the browser tests

The three browser-divergent surfaces (OPFS cache, fetch error mapping, canvas bridge) are covered by
headless Chrome:

```sh
cd web && wasm-pack test --headless --chrome
```

Cross-platform logic (manifest parse, SHA-256, license authorization, hit-test, projection) is tested
once in `shared` with host `cargo test` and is not re-run here.

## Release build, precompress, and measure the budget

```sh
./scripts/build/measure-site-budget.sh
```

```sh
./scripts/build/precompress-site.sh
```

Both run from the repo root and resolve their own paths, so neither cares about the current directory.
`measure-site-budget.sh` runs `cargo leptos build --release` itself and prints the first-paint and
second-paint totals against the 2 MB / 3 MB caps, appending ` near cap` to any total over approx. 90% of
its cap; it always exits 0 (the cap is a target, surfaced to reviewers, not a build gate). Pass
`--no-build` to report on the tree already in `target/site/` instead of rebuilding. Run
`precompress-site.sh` after it, since a build clears the site root: the script writes `.br` siblings for
the compressible asset types under `target/site/`.

## Deploy

```sh
cd web && wrangler deploy
```

Pure static asset serving from Cloudflare Workers Assets (`wrangler.toml` has `[assets] directory`,
no `main`, no Worker). The apex-domain routing to `eafora.org` is configured in the Cloudflare
dashboard, outside the codebase.

## Common pitfalls

- **Blank canvas, no error**: the bundle is missing or empty — re-run the embedded-bundle sync. `Bundle::open` reads from the cache, so a bundle that never got fetched into OPFS yields nothing to draw.
- **`shared` won't link the wgpu types**: the web crate must depend on `shared` with `features = ["render"]`; the feature is off by default.
- **`cache: opfs unsupported` on older Safari**: expected on browsers without OPFS: the client hard-fails and renders the unsupported panel instead of the map (no fallback). Verify the exact Safari cutoff against caniuse.com.
- **WebGL2 forced path looks identical to WebGPU**: intended — the renderer is built to the WebGL2 feature set, so `?renderer=webgl2` output matches (FR-005 acceptance scenario 5).
