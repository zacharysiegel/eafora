#!/usr/bin/env bash

# Builds the deployable site tree into target/site: a release build, then the prerendered document.
#
# Usage:
#   ./scripts/build/build-site.sh
#
# Two orderings matter and are the reason this is a script rather than two commands in a doc. A build
# empties the site root, so the document must be written after it, never before. And the hashed-filename
# setting has to reach both processes, or the document would reference names that are not on disk.

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
readonly WEB_DIR="${REPO_ROOT}/web"
readonly SERVER_BINARY="${REPO_ROOT}/target/release/web"

# Hashed filenames let the deploy serve /pkg/* as immutable. Set here rather than in the manifest
# because cargo-leptos only re-hashes on a full build, so a watch rebuild would serve stale assets.
export LEPTOS_HASH_FILES=true

function fail {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

if ! command -v cargo-leptos > /dev/null 2>&1; then
    fail "cargo-leptos is not installed; run: cargo install cargo-leptos --locked"
fi

printf 'building the release site\n'
cd "$WEB_DIR"
cargo leptos build --release

if [[ ! -x "$SERVER_BINARY" ]]; then
    fail "the release build produced no server binary at ${SERVER_BINARY}"
fi

# Run in place: the render reads the content-hash file from the directory holding the binary.
printf '\nprerendering the document\n'
"$SERVER_BINARY" prerender

printf '\n'
"${SCRIPT_DIR}/verify-site-tree.sh"
