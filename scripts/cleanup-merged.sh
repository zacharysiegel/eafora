#!/usr/bin/env bash
# scripts/cleanup-merged.sh — delete named branches from origin and locally, then prune.
# Use after a PR has been merged into master via rebase-and-merge (the project
# default); the original branch tip is no longer reachable from master, so
# `git branch -d` would refuse — this script uses `-D`. Run it once per merge,
# passing the names of branches whose PRs have just landed.
#
# Usage:
#   ./scripts/cleanup-merged.sh <branch-name> [<branch-name>...]
#
# Behavior:
#   1. Refuse to operate on `master` or any currently-checked-out branch.
#   2. Delete each named branch from origin (skip silently if remote already gone).
#   3. Delete each named branch locally with -D (force; rebase-and-merge leaves
#      branches unreachable from master, so safe-delete would always refuse).
#   4. Run `git remote prune origin` to clean up stale remote-tracking refs.
#
# Data race with GitHub's "Automatically delete head branches" setting:
# After a local-rebase + push-master flow (see scripts/pr-merge.sh), GitHub
# recognizes the PR as merged (head SHA equal to base) and auto-deletes the
# head branch on origin asynchronously. This races against the push --delete
# step here: ls-remote can return "branch exists" at check time, then GitHub
# deletes it, then push --delete fails with "remote ref does not exist" /
# "cannot lock ref". The script handles this by re-fetching with --prune on
# push --delete failure and accepting the case where the branches are already
# absent from origin. Only a genuine "remote branch is still there" failure
# causes the script to exit with an error.

set -euo pipefail

if [[ $# -eq 0 ]]; then
    echo "usage: $0 <branch-name> [<branch-name>...]" >&2
    exit 64
fi

if ! git rev-parse --git-dir > /dev/null 2>&1; then
    echo "error: not inside a git repository" >&2
    exit 1
fi

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)

for BRANCH_NAME in "$@"; do
    if [[ "$BRANCH_NAME" == "master" ]]; then
        echo "error: refusing to delete master" >&2
        exit 1
    fi
    if [[ "$BRANCH_NAME" == "$CURRENT_BRANCH" ]]; then
        echo "error: cannot delete branch '$BRANCH_NAME' while it is checked out" >&2
        exit 1
    fi
done

REMOTE_TARGETS=()
for BRANCH_NAME in "$@"; do
    if git ls-remote --exit-code --heads origin "$BRANCH_NAME" > /dev/null 2>&1; then
        REMOTE_TARGETS+=("$BRANCH_NAME")
    fi
done

if [[ "${#REMOTE_TARGETS[@]}" -gt 0 ]]; then
    if ! git push origin --delete "${REMOTE_TARGETS[@]}"; then
        # Race with GitHub's "Automatically delete head branches" setting:
        # when a PR is recognized as merged (head SHA equal to base), GitHub may
        # delete the head branch on origin between our ls-remote check above and
        # this push --delete. Confirm the branches are actually gone, then continue.
        echo "note: push origin --delete returned non-zero; checking whether the branches were already deleted by GitHub auto-delete..."
        git fetch --prune origin > /dev/null
        for BRANCH_NAME in "${REMOTE_TARGETS[@]}"; do
            if git ls-remote --exit-code --heads origin "$BRANCH_NAME" > /dev/null 2>&1; then
                echo "error: branch '$BRANCH_NAME' still exists on origin after delete attempt" >&2
                exit 1
            fi
        done
        echo "all named branches confirmed absent from origin; continuing with local cleanup."
    fi
fi

LOCAL_TARGETS=()
for BRANCH_NAME in "$@"; do
    if git show-ref --verify --quiet "refs/heads/$BRANCH_NAME"; then
        LOCAL_TARGETS+=("$BRANCH_NAME")
    fi
done

if [[ "${#LOCAL_TARGETS[@]}" -gt 0 ]]; then
    git branch -D "${LOCAL_TARGETS[@]}"
fi

git remote prune origin
