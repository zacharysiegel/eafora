#!/usr/bin/env bash
# sync-embedded-bundle.sh — copy the newest downsampled bundle into a client's static-asset tree.
#
# `ingestion build` writes every version's two bundles to
# $EAFORA_ARTIFACTS_DIR/<version-label>/{complete,downsampled}/ and repoints
# $EAFORA_ARTIFACTS_DIR/latest at the newest version. This copies that build's
# downsampled/ subtree (a self-contained manifest + geometry + shards) into the
# destination, running `ingestion build` first if no build exists yet.
#
# Plain copy only: no symlink, no hardlink, no rsync, so the destination is a
# standalone tree the client build embeds verbatim.
#
# Usage:
#   ./scripts/sync-embedded-bundle.sh <destination-dir>
# e.g.
#   ./scripts/sync-embedded-bundle.sh ./web/static/embedded_artifacts

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <destination-dir>" >&2
    exit 64
fi

DESTINATION="$1"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# EAFORA_ARTIFACTS_DIR comes from the environment or the repo .env (the same variable `ingestion build` reads).
if [[ -z "${EAFORA_ARTIFACTS_DIR:-}" && -f "$REPO_ROOT/.env" ]]; then
    EAFORA_ARTIFACTS_DIR="$(grep -E '^EAFORA_ARTIFACTS_DIR=' "$REPO_ROOT/.env" | tail -1 | cut -d= -f2- || true)"
fi
if [[ -z "${EAFORA_ARTIFACTS_DIR:-}" ]]; then
    echo "error: EAFORA_ARTIFACTS_DIR is not set (neither in the environment nor $REPO_ROOT/.env)" >&2
    exit 1
fi

DOWNSAMPLED_DIR="$EAFORA_ARTIFACTS_DIR/latest/downsampled"

if [[ ! -d "$DOWNSAMPLED_DIR" ]]; then
    echo "no build found at $DOWNSAMPLED_DIR; running 'ingestion build' first" >&2
    (cd "$REPO_ROOT" && cargo run --quiet -p ingestion -- build)
fi

if [[ ! -d "$DOWNSAMPLED_DIR" ]]; then
    echo "error: $DOWNSAMPLED_DIR is still missing after the build" >&2
    exit 1
fi

# Replace the destination wholesale so stale content-addressed files never linger.
rm -rf "${DESTINATION:?destination must be non-empty}"
mkdir -p "$DESTINATION"
cp -R "$DOWNSAMPLED_DIR/." "$DESTINATION/"

echo "synced embedded bundle: $DOWNSAMPLED_DIR -> $DESTINATION" >&2
