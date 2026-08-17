#!/usr/bin/env bash
# precompress-site.sh: write brotli-quality-11 .br siblings for the compressible files in a built site tree.
#
# Cloudflare Workers Assets serves a file's .br sibling to clients whose Accept-Encoding includes br
# and the original otherwise. Quality 11 is brotli's maximum and is too slow to run per request at
# the edge, so the deploy ships q11 output compressed here.
#
# This runs after the build instead of through cargo-leptos's own --precompress flag. That flag
# compresses the site tree before the server binary is compiled, so anything written to the tree
# afterward gets no .br sibling; it also writes a .gz sibling for every file and applies no extension
# filter, so it compresses already-compressed types too.
#
# Usage:
#   ./scripts/build/precompress-site.sh [<site-dir>]
# e.g.
#   ./scripts/build/precompress-site.sh
#   ./scripts/build/precompress-site.sh ./target/site
#
# Behavior:
#   1. <site-dir> defaults to <repo-root>/target/site.
#   2. Compress .wasm, .js, .css, .html, .json, .fgb, and .sqlite. Every other extension is left
#      alone, which is what excludes the already-compressed types (.png, .jpg, .woff2).
#   3. Leave a file alone when its .br sibling is at least as new, so re-running costs nothing.
#   4. Exit non-zero when brotli fails, or when the tree holds no compressible file at all.

set -euo pipefail

function required_program {
    if ! which "$1" 1>/dev/null 2>&1; then
        echo "The \`$1\` program is required" >&2
        echo "  install: $2" >&2
        exit 1
    fi
}
required_program "brotli" "brew install brotli"

if [[ $# -gt 1 ]]; then
    echo "usage: $0 [<site-dir>]" >&2
    exit 64
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SITE_DIR="${1:-$REPO_ROOT/target/site}"

if [[ ! -d "$SITE_DIR" ]]; then
    echo "error: $SITE_DIR does not exist; run 'cargo leptos build --release' first" >&2
    exit 1
fi

compressed_count=0
up_to_date_count=0
source_bytes=0
compressed_bytes=0

while IFS= read -r -d '' source_path; do
    case "$source_path" in
        *.wasm | *.js | *.css | *.html | *.json | *.fgb | *.sqlite)
            ;;
        *)
            continue
            ;;
    esac

    compressed_path="${source_path}.br"

    if [[ -f "$compressed_path" && ! "$source_path" -nt "$compressed_path" ]]; then
        up_to_date_count=$((up_to_date_count + 1))
    else
        # brotli refuses to write over an existing .br, and the test above already found it stale.
        brotli -q 11 --keep --force "$source_path"
        compressed_count=$((compressed_count + 1))
    fi

    # brotli can fail to write its output file and still exit zero.
    if [[ ! -f "$compressed_path" ]]; then
        echo "error: brotli produced no $compressed_path" >&2
        exit 1
    fi

    source_bytes=$((source_bytes + $(wc -c < "$source_path")))
    compressed_bytes=$((compressed_bytes + $(wc -c < "$compressed_path")))
done < <(find "$SITE_DIR" -type f -print0)

if [[ $((compressed_count + up_to_date_count)) -eq 0 ]]; then
    echo "error: no compressible file under $SITE_DIR; run 'cargo leptos build --release' first" >&2
    exit 1
fi

printf '%-18s %s\n' "site dir:" "$SITE_DIR" >&2
printf '%-18s %s\n' "compressed:" "$compressed_count" >&2
printf '%-18s %s\n' "up to date:" "$up_to_date_count" >&2
printf '%-18s %s\n' "source bytes:" "$source_bytes" >&2
printf '%-18s %s\n' "brotli q11 bytes:" "$compressed_bytes" >&2
