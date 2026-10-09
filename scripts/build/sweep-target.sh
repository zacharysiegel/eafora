#!/usr/bin/env bash

# Deletes build artifacts in the workspace's target directory that no build has used for SWEEP_AFTER_DAYS,
# keeping everything a recent build relies on.
#
# Usage:
#   ./scripts/build/sweep-target.sh

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
readonly SWEEP_AFTER_DAYS=14

cd "${REPO_ROOT}"

cargo_sweep_path=$(which cargo-sweep 2>/dev/null || true)
if test -z "${cargo_sweep_path}"; then
    echo "skipping the target sweep: cargo-sweep is not installed" >&2
    echo "  install: cargo install --locked cargo-sweep    (or run ./setup.sh)" >&2
    exit 0
fi

target_size_before=$(du -sh target 2>/dev/null | cut -f1 || true)

# cargo-sweep dates each build unit by the access time of its fingerprint files, which every build reads.
cargo sweep --time "${SWEEP_AFTER_DAYS}" "${REPO_ROOT}"

target_size_after=$(du -sh target 2>/dev/null | cut -f1 || true)
echo "target/: ${target_size_before:-absent} -> ${target_size_after:-absent}"
