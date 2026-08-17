#!/bin/zsh
# Runs the shared crate's wasm32 tests in headless Chrome via wasm-bindgen-test.
# wasm-pack needs a package manifest, so this runs from shared/, not the workspace
# root. See docs/system-dependencies.md for the toolchain.
#
# Extra arguments are forwarded to wasm-pack, e.g.:
#   ./scripts/test/test-wasm.sh -- parse_manifest    (filter to matching tests)

set -euo pipefail

function required_program {
    if ! which "$1" 1>/dev/null 2>&1; then
        echo "The \`$1\` program is required"
        echo "  install: $2"
        echo "  (see docs/system-dependencies.md)"
        exit 1
    fi
}
required_program "wasm-pack"    "cargo install wasm-pack"
required_program "chromedriver" "a build matching your Chrome major version, on PATH"

repo_dir=$(git rev-parse --show-toplevel)
cd "${repo_dir}/shared"
exec wasm-pack test --headless --chrome "$@"
