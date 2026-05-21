#!/usr/bin/env bash
# branch-init.sh — create a new git branch from current HEAD with the canonical
# empty marker commit (`>>> branch: <name>`) and push it.
#
# The marker compensates for "rebase and merge" erasing PR boundaries from
# master's history. Search for boundaries in `git log` with `/>>> branch:` then
# n/N in less.
#
# Usage:
#   ./branch-init.sh <branch-name>
#
# Behavior:
#   1. Refuse if the working tree has uncommitted changes.
#   2. Refuse if a branch with the given name already exists locally or on origin.
#   3. Create and check out the new branch from the current HEAD.
#   4. Create an empty commit with subject `>>> branch: <branch-name>`.
#   5. Push the branch with `-u origin <branch-name>` so tracking is set up.

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <branch-name>" >&2
    exit 64
fi

BRANCH_NAME="$1"

if [[ -z "$BRANCH_NAME" ]]; then
    echo "error: branch name must be non-empty" >&2
    exit 64
fi

if ! git rev-parse --git-dir > /dev/null 2>&1; then
    echo "error: not inside a git repository" >&2
    exit 1
fi

if ! git diff-index --quiet HEAD --; then
    echo "error: working tree has uncommitted changes; commit or stash first" >&2
    exit 1
fi

if git show-ref --verify --quiet "refs/heads/$BRANCH_NAME"; then
    echo "error: local branch '$BRANCH_NAME' already exists" >&2
    exit 1
fi

if git ls-remote --exit-code --heads origin "$BRANCH_NAME" > /dev/null 2>&1; then
    echo "error: remote branch 'origin/$BRANCH_NAME' already exists" >&2
    exit 1
fi

git checkout -b "$BRANCH_NAME"
git commit --allow-empty -m ">>> branch: $BRANCH_NAME"
git push -u origin "$BRANCH_NAME"
