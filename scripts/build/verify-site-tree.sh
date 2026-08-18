#!/usr/bin/env bash

# Checks that a site tree is safe to deploy, and says what is wrong when it is not.
#
# Usage:
#   ./scripts/build/verify-site-tree.sh [<site-dir>]
#
# Shared by build-site.sh, which runs it on what it just produced, and deploy-site.sh, which runs it on
# whatever tree it is about to upload.

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
readonly SITE_DIR="${1:-${REPO_ROOT}/target/site}"
readonly DOCUMENT_PATH="${SITE_DIR}/index.html"

# cargo-leptos inserts a 22-character base64url digest between the stem and the extension.
readonly HASHED_NAME_PATTERN='\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9]+$'

function fail {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

if [[ ! -d "$SITE_DIR" ]]; then
    fail "${SITE_DIR} does not exist; run ./scripts/build/build-site.sh"
fi

if [[ ! -f "$DOCUMENT_PATH" ]]; then
    fail "${DOCUMENT_PATH} is missing, so the deploy would serve no document at /; run ./scripts/build/build-site.sh"
fi

reference_count=0

while IFS= read -r referenced_path; do
    # A reference the tree cannot satisfy deploys as a 404 for the wasm or the stylesheet, which presents
    # as a blank page rather than as a failure.
    if [[ ! -f "${SITE_DIR}${referenced_path}" ]]; then
        fail "the document references ${referenced_path}, which is not in ${SITE_DIR}"
    fi

    # web/static/_headers serves /pkg/* as immutable for a year, which is only correct while a rebuild
    # changes the filename. A stable name would pin this build's asset in caches for that long.
    if ! printf '%s' "$referenced_path" | grep -Eq "$HASHED_NAME_PATTERN"; then
        fail "the document references ${referenced_path}, whose name carries no content hash, but _headers serves /pkg/* as immutable; build with ./scripts/build/build-site.sh"
    fi

    reference_count=$((reference_count + 1))
done < <(grep -o '/pkg/[A-Za-z0-9._-]*' "$DOCUMENT_PATH" | sort -u)

if [[ "$reference_count" -eq 0 ]]; then
    fail "the document references no /pkg/ assets, so it would load nothing"
fi

printf '%s: prerendered document present, %d referenced assets present and content-hashed\n' "$SITE_DIR" "$reference_count"
