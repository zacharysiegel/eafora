#!/usr/bin/env bash

# Applies the artifact repository's CORS policy, without which a browser blocks every shard fetch: the
# site and the artifacts are different origins, so a response arrives intact and is then discarded.
#
# Usage:
#   ./scripts/build/set-artifact-cors.sh
#   ./scripts/build/set-artifact-cors.sh --list      (report the applied policy, changing nothing)
#
# The bucket and account come from .env, which is also where the publisher reads them, so the names are
# not restated here. The policy itself is ingestion/r2-cors.json, beside the crate that owns the bucket.

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
readonly ENVIRONMENT_PATH="${REPO_ROOT}/.env"
readonly POLICY_PATH="${REPO_ROOT}/ingestion/r2-cors.json"

LIST_ONLY=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --list)
            LIST_ONLY=true
            shift
            ;;
        *)
            echo "usage: $0 [--list]" >&2
            exit 64
            ;;
    esac
done

function fail {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

function read_environment_value {
    local key_name="$1"
    local declared_value
    declared_value="$(grep -E "^${key_name}=" "$ENVIRONMENT_PATH" | tail -1 | cut -d= -f2- || true)"

    if [[ -z "$declared_value" ]]; then
        fail "${ENVIRONMENT_PATH} declares no ${key_name}"
    fi

    printf '%s' "$declared_value"
}

if ! command -v npx > /dev/null 2>&1; then
    fail "npx is not installed; run: brew install node"
fi

if [[ ! -f "$ENVIRONMENT_PATH" ]]; then
    fail "${ENVIRONMENT_PATH} is absent; see docs/system-dependencies.md"
fi

if [[ ! -f "$POLICY_PATH" ]]; then
    fail "${POLICY_PATH} is absent"
fi

BUCKET="$(read_environment_value "R2_ARTIFACT_BUCKET")"
# Answers the account prompt wrangler would otherwise raise when a token can reach several accounts.
CLOUDFLARE_ACCOUNT_ID="$(read_environment_value "R2_ACCOUNT_ID")"
export CLOUDFLARE_ACCOUNT_ID

cd "$REPO_ROOT"

if [[ "$LIST_ONLY" == false ]]; then
    printf 'applying %s to bucket %s\n' "$POLICY_PATH" "$BUCKET"
    npx wrangler r2 bucket cors set "$BUCKET" --file "$POLICY_PATH" --force
    printf '\n'
fi

printf 'applied policy for bucket %s\n' "$BUCKET"
npx wrangler r2 bucket cors list "$BUCKET"

# Cloudflare documents both, and either one makes a correct policy look like it did not take effect.
printf '\nA change can take up to 30 seconds to propagate, and a custom domain already serving traffic\n'
printf 'needs its cache purged before responses carry the new header.\n'
