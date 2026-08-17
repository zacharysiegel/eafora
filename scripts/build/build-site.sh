#!/usr/bin/env bash

# Builds the deployable site tree into target/site: a release build, then the static shell document.
#
# Usage:
#   ./scripts/build/build-site.sh
#
# Two orderings matter and are the reason this is a script rather than two commands in a doc. A build
# empties the site root, so the shell must be written after it, never before. And the hashed-filename
# setting has to reach both processes, or the shell would reference names that are not on disk.

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

# Run in place: the shell render reads the hash file from the directory holding the binary.
printf '\nwriting the shell document\n'
"$SERVER_BINARY" export-shell

# A reference the tree cannot satisfy would deploy as a 404 for the wasm or the stylesheet, which reads as
# a blank page rather than as a build failure. Cheap to catch here instead.
printf '\nchecking the shell references files that exist\n'
SITE_DIR="${REPO_ROOT}/target/site"
REFERENCE_COUNT=0

while IFS= read -r referenced_path; do
    if [[ ! -f "${SITE_DIR}${referenced_path}" ]]; then
        fail "the shell references ${referenced_path}, which is not in ${SITE_DIR}"
    fi

    REFERENCE_COUNT=$((REFERENCE_COUNT + 1))
done < <(grep -o '/pkg/[A-Za-z0-9._-]*' "${SITE_DIR}/index.html" | sort -u)

if [[ "$REFERENCE_COUNT" -eq 0 ]]; then
    fail "the shell references no /pkg/ assets, so it would load nothing"
fi

printf '  %d referenced assets are present\n' "$REFERENCE_COUNT"

printf '\nthe deployable tree is at %s\n' "$SITE_DIR"
