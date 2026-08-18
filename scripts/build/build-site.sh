#!/usr/bin/env bash

# Builds the deployable site tree into target/site: a release build, the prerendered document, and a
# discovery document naming the production artifact repository.
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
readonly SITE_DIR="${REPO_ROOT}/target/site"
readonly SERVER_BINARY="${REPO_ROOT}/target/release/web"
readonly COMMITTED_DISCOVERY_PATH="${WEB_DIR}/static/discovery"
readonly PRODUCTION_REPOSITORY_BASE_URL="https://repository.eafora.org"

# Hashed filenames let the deploy serve /pkg/* as immutable. Set here rather than in the manifest
# because cargo-leptos only re-hashes on a full build, so a watch rebuild would serve stale assets.
export LEPTOS_HASH_FILES=true

# The committed discovery document names /repository, which is where `ingestion publish local` writes and
# what `cargo leptos watch` serves. A deploy has to name the artifact CDN instead, in the document it
# serves and in the value compiled into the wasm for the speculative fetch that races discovery.
export EAFORA_REPOSITORY_BASE_URL="${EAFORA_REPOSITORY_BASE_URL:-$PRODUCTION_REPOSITORY_BASE_URL}"

function fail {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

if ! command -v cargo-leptos > /dev/null 2>&1; then
    fail "cargo-leptos is not installed; run: cargo install cargo-leptos --locked"
fi

if ! command -v jq > /dev/null 2>&1; then
    fail "jq is not installed; run: brew install jq"
fi

printf 'building the release site\n'
printf '  repository base: %s\n' "$EAFORA_REPOSITORY_BASE_URL"
cd "$WEB_DIR"
cargo leptos build --release

if [[ ! -x "$SERVER_BINARY" ]]; then
    fail "the release build produced no server binary at ${SERVER_BINARY}"
fi

# Run in place: the render reads the content-hash file from the directory holding the binary.
printf '\nprerendering the document\n'
"$SERVER_BINARY" prerender

# Rewrites the one field rather than committing a second document, so the schema stays in one file.
printf '\nnaming the artifact repository in the discovery document\n'
jq --arg base_url "$EAFORA_REPOSITORY_BASE_URL" '.repository_base_url = $base_url' \
    "$COMMITTED_DISCOVERY_PATH" > "${SITE_DIR}/discovery"

printf '\n'
"${SCRIPT_DIR}/verify-site-tree.sh"
