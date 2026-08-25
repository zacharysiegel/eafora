#!/usr/bin/env bash
# publish-local.sh: publish an artifact set into the web client's static repository tree.
#
# `ingestion publish local` writes object keys under a destination root and records an
# artifact_version row pointing at a public URL prefix. The two values below are the ones the
# web client's dev server and Cloudflare Pages build both serve, so they are not left to the
# caller: the destination is the client's static tree, and the prefix is the path that tree is
# mounted at. $EAFORA_LOCAL_REPOSITORY_ROOT (the CLI's own default root) is a scratch directory
# no client serves.
#
# The tree keeps every version published into it, because the client ranks the versions it finds
# and prefers the newest it can open.
#
# Usage:
#   ./scripts/build/publish-local.sh [--build]
#
#   --build  build a new artifact set first; without it, the newest existing build under
#            $EAFORA_ARTIFACTS_DIR is published

set -euo pipefail

DESTINATION_ROOT="./web/static/repository/"
PUBLIC_BASE_URL="/repository"

BUILD_FIRST=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --build)
            BUILD_FIRST=true
            shift
            ;;
        *)
            echo "usage: $0 [--build]" >&2
            exit 64
            ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

PUBLISH_ARGS=(publish local --root "$DESTINATION_ROOT" --public-base-url "$PUBLIC_BASE_URL")

if [[ "$BUILD_FIRST" == true ]]; then
    PUBLISH_ARGS+=(--build)
else
    # EAFORA_ARTIFACTS_DIR comes from the environment or the repo .env (the same variable `ingestion build` writes under).
    if [[ -z "${EAFORA_ARTIFACTS_DIR:-}" && -f ./.env ]]; then
        EAFORA_ARTIFACTS_DIR="$(grep -E '^EAFORA_ARTIFACTS_DIR=' ./.env | tail -1 | cut -d= -f2- || true)"
    fi
    if [[ -z "${EAFORA_ARTIFACTS_DIR:-}" ]]; then
        echo "error: EAFORA_ARTIFACTS_DIR is not set (neither in the environment nor ./.env)" >&2
        exit 1
    fi

    ARTIFACT_DIR="$EAFORA_ARTIFACTS_DIR/latest/complete"
    if [[ ! -d "$ARTIFACT_DIR" ]]; then
        echo "error: no build to publish at $ARTIFACT_DIR; pass --build" >&2
        exit 1
    fi

    PUBLISH_ARGS+=("$ARTIFACT_DIR")
fi

cargo run -p ingestion -- "${PUBLISH_ARGS[@]}"

echo "published into $DESTINATION_ROOT, served at $PUBLIC_BASE_URL" >&2
