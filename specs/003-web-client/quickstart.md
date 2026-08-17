# Quickstart: building and running the web client

Assumes the `003-web-client` stack is checked out and the workspace builds. Commands are single-line
for copy/paste. Paths are relative to the repo root unless noted.

## Prerequisites (one-time)

```sh
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos --locked --version '^0.3'
brew install brotli jq
```

`brotli` and `jq` are invoked by the build scripts under `./scripts/build/`, and are build-host tools rather than workspace deps. `wasm-opt` is deliberately not installed here: cargo-leptos downloads and caches the binaryen version it pins on the first release build, which keeps the optimizer tied to the toolchain. That first build needs network access to `github.com` and its release-asset host.

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

## Build the deployable tree and measure the budget

```sh
./scripts/build/build-site.sh
```

```sh
./scripts/build/measure-site-budget.sh --no-build
```

Every script resolves its own paths, so none of them care about the current directory.

`build-site.sh` runs `cargo leptos build --release` and then `web export-shell`, in that order, because a build empties the site root and the shell document has to survive it. It also sets `LEPTOS_HASH_FILES=true`, which gives the assets content-hashed filenames so the deploy can serve them as immutable. That setting lives here rather than in `web/Cargo.toml` on purpose: cargo-leptos only re-hashes on a full build, so under `cargo leptos watch` an incremental rebuild would write unhashed files while the page still asked for the previous build's hashed names, and the browser would silently load stale wasm.

`measure-site-budget.sh` prints the first-paint and second-paint artifact totals against the 2 MB / 8 MB targets, and the client code alongside them uncapped. It marks a total at or above 90% of its target ` near cap` and one over it `*** OVER CAP ***` with a warning line, and it always exits 0: the targets are for a person to weigh, not a gate. Anything it cannot pin down (a missing shell document, or two `.wasm` files where a visitor fetches one) is reported as unmeasured rather than as a smaller number. With no arguments it runs `build-site.sh` first; `--no-build` reports on whatever is already in `target/site/`, which may be a debug build.

## Deploy

```sh
./scripts/build/deploy-site.sh --build
```

```sh
./scripts/build/deploy-site.sh --dry-run
```

Pure static asset serving from Cloudflare Workers Assets (`web/wrangler.toml` has `[assets] directory`, no `main`, no Worker). The script deploys whatever is already in `target/site`; `--build` produces it first. Either way it runs `./scripts/build/verify-site-tree.sh`, which refuses a tree with no shell document, a shell reference missing from disk, or a `/pkg/` reference whose name carries no content hash, since `_headers` serves that path as immutable for a year.

The first deploy needs `npx wrangler login` once, which opens a browser. The apex-domain routing to `eafora.org` is configured in the Cloudflare dashboard, outside the codebase.

## Common pitfalls

- **Blank canvas, no error**: the bundle is missing or empty — re-run the embedded-bundle sync. `Bundle::open` reads from the cache, so a bundle that never got fetched into OPFS yields nothing to draw.
- **`shared` won't link the wgpu types**: the web crate must depend on `shared` with `features = ["render"]`; the feature is off by default.
- **`cache: opfs unsupported` on older Safari**: expected on browsers without OPFS: the client hard-fails and renders the unsupported panel instead of the map (no fallback). Verify the exact Safari cutoff against caniuse.com.
- **WebGL2 forced path looks identical to WebGPU**: intended — the renderer is built to the WebGL2 feature set, so `?renderer=webgl2` output matches (FR-005 acceptance scenario 5).
