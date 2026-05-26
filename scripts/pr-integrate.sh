#!/usr/bin/env bash
# scripts/pr-integrate.sh — integrate a feature branch into master via the manual
# rebase flow that preserves `>>> branch: <name>` marker commits. Named "integrate"
# rather than "merge" because it rebases (not merges) the branch onto master.
#
# The GitHub "rebase and merge" UI button drops empty commits and would lose
# the markers, so this script encodes the canonical local-rebase + push + cleanup
# sequence per Governance §Git workflow §Merge strategy.
#
# Usage:
#   ./scripts/pr-integrate.sh <branch>
#   ./scripts/pr-integrate.sh --current                                   (use the currently
#                                                                         checked-out branch)
#   ./scripts/pr-integrate.sh <branch> --onto <former-parent-branch>     (for a stacked branch
#                                                                         whose parent already merged)
#
# Behavior:
#   1. Validate: working tree is clean; <branch> exists locally and on origin.
#   2. Update master: `git checkout master && git pull --ff-only`.
#   3. Update branch atop latest master:
#        non-stacked: `git checkout <branch> && git rebase master`
#        stacked:     `git checkout <branch> && git rebase --onto master <former-parent> <branch>`
#   4. Force-push branch (`git push --force-with-lease`).
#   5. Fast-forward master to branch tip via `git rebase <branch>` (consistent
#      with the rebase-family preference; equivalent to `merge --ff-only`).
#   6. Push master (`git push origin master`).
#   7. Run `./scripts/cleanup-merged.sh <branch>` to delete the branch from
#      origin, locally, and prune.

set -euo pipefail

BRANCH=""
FORMER_PARENT=""
USE_CURRENT_BRANCH=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --onto)
            shift
            if [[ $# -lt 1 ]]; then
                echo "error: --onto requires a former-parent branch argument" >&2
                exit 64
            fi
            FORMER_PARENT="$1"
            shift
            ;;
        --current)
            USE_CURRENT_BRANCH=true
            shift
            ;;
        -h|--help)
            grep '^#' "$0" | sed 's|^# \{0,1\}||'
            exit 0
            ;;
        -*)
            echo "error: unknown flag: $1" >&2
            exit 64
            ;;
        *)
            if [[ -z "$BRANCH" ]]; then
                BRANCH="$1"
                shift
            else
                echo "error: unexpected argument: $1" >&2
                exit 64
            fi
            ;;
    esac
done

if [[ "$USE_CURRENT_BRANCH" == true ]]; then
    if [[ -n "$BRANCH" ]]; then
        echo "error: --current cannot be combined with a positional branch argument" >&2
        exit 64
    fi
    BRANCH=$(git symbolic-ref --short HEAD 2>/dev/null || true)
    if [[ -z "$BRANCH" ]]; then
        echo "error: --current requires HEAD to be on a branch (detached HEAD)" >&2
        exit 1
    fi
fi

if [[ -z "$BRANCH" ]]; then
    echo "usage: $0 (<branch> | --current) [--onto <former-parent-branch>]" >&2
    exit 64
fi

if [[ "$BRANCH" == "master" ]]; then
    echo "error: refusing to operate on master" >&2
    exit 1
fi

if ! git rev-parse --git-dir > /dev/null 2>&1; then
    echo "error: not inside a git repository" >&2
    exit 1
fi

if ! git diff-index --quiet HEAD --; then
    echo "error: working tree has uncommitted changes; commit or stash first" >&2
    exit 1
fi

if ! git show-ref --verify --quiet "refs/heads/$BRANCH"; then
    echo "error: local branch '$BRANCH' does not exist" >&2
    exit 1
fi

if ! git ls-remote --exit-code --heads origin "$BRANCH" > /dev/null 2>&1; then
    echo "error: remote branch 'origin/$BRANCH' does not exist (cannot merge a branch that hasn't been pushed)" >&2
    exit 1
fi

if [[ -n "$FORMER_PARENT" ]]; then
    if ! git show-ref --verify --quiet "refs/heads/$FORMER_PARENT" && \
       ! git rev-parse --verify --quiet "$FORMER_PARENT" > /dev/null; then
        echo "error: former-parent ref '$FORMER_PARENT' not resolvable" >&2
        exit 1
    fi
fi

echo ">>> Updating master"
git checkout master
git pull --ff-only

echo ">>> Updating branch '$BRANCH' atop master"
git checkout "$BRANCH"
if [[ -n "$FORMER_PARENT" ]]; then
    git rebase --onto master "$FORMER_PARENT" "$BRANCH"
else
    git rebase master
fi

echo ">>> Force-pushing rebased '$BRANCH'"
git push --force-with-lease

echo ">>> Fast-forwarding master to '$BRANCH' tip"
git checkout master
git rebase "$BRANCH"

echo ">>> Pushing master"
git push origin master

echo ">>> Cleaning up '$BRANCH'"
"$(dirname "$0")/cleanup-merged.sh" "$BRANCH"

echo ">>> Done."
