#!/bin/zsh
# Runs the shared crate's wasm32 tests in headless Chrome via wasm-bindgen-test.
# wasm-pack needs a package manifest, so this runs from shared/, not the workspace
# root. See docs/system-dependencies.md for the toolchain (wasm-pack, Chrome, and a
# chromedriver matching your Chrome major version).
#
# Extra arguments are forwarded to wasm-pack, e.g.:
#   ./scripts/test-wasm.sh -- parse_manifest    (filter to matching tests)

set -euo pipefail

repo_dir=$(git rev-parse --show-toplevel)

if ! which wasm-pack 1>/dev/null 2>&1; then
    echo "wasm-pack is required: cargo install wasm-pack"
    echo "  (see docs/system-dependencies.md)"
    exit 1
fi

# Locate chromedriver: an explicit CHROMEDRIVER wins; otherwise one on PATH (let
# wasm-pack find it); otherwise fall back to ~/.local/bin.
if test -z "${CHROMEDRIVER:-}"; then
    if ! which chromedriver 1>/dev/null 2>&1; then
        if test -x "${HOME}/.local/bin/chromedriver"; then
            export CHROMEDRIVER="${HOME}/.local/bin/chromedriver"
        else
            echo "chromedriver not found on PATH, in \$CHROMEDRIVER, or in ~/.local/bin"
            echo "  install a build matching your Chrome major version"
            echo "  (see docs/system-dependencies.md)"
            exit 1
        fi
    fi
fi

cd "${repo_dir}/shared"
exec wasm-pack test --headless --chrome "$@"
