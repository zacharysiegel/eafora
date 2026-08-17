#!/usr/bin/env bash

# Deploys the site to Cloudflare Workers Assets.
#
# Usage:
#   ./scripts/build/deploy-site.sh [--dry-run]
#
# Wraps the build rather than documenting it beside the deploy command, because web/static/_headers serves
# /pkg/* as immutable for a year. That is only safe for the content-hashed filenames build-site.sh
# produces, so a hand-run build followed by `wrangler deploy` would pin stale assets.

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
readonly WEB_DIR="${REPO_ROOT}/web"

DRY_RUN=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        *)
            echo "usage: $0 [--dry-run]" >&2
            exit 64
            ;;
    esac
done

"${SCRIPT_DIR}/build-site.sh"

printf '\ndeploying\n'
cd "$WEB_DIR"

if [[ "$DRY_RUN" == true ]]; then
    npx wrangler deploy --dry-run
else
    npx wrangler deploy
fi
