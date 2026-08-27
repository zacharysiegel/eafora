#!/usr/bin/env bash
# scripts/git/pr-integrate.sh — integrate a feature branch into master via the manual
# rebase flow that preserves `>>> branch: <name>` marker commits. Named "integrate"
# rather than "merge" because it rebases (not merges) the branch onto master.
#
# The GitHub "rebase and merge" UI button drops empty commits and would lose
# the markers, so this script encodes the canonical local-rebase + push + cleanup
# sequence per Governance §Git workflow §Merge strategy.
#
# Usage:
#   ./scripts/git/pr-integrate.sh <branch>
#   ./scripts/git/pr-integrate.sh --current                                   (use the currently
#                                                                         checked-out branch)
#   ./scripts/git/pr-integrate.sh <branch> --from <former-parent-branch>     (for a stacked branch
#                                                                         whose parent already merged)
#   ./scripts/git/pr-integrate.sh <branch> --budget                          (build the site and report
#                                                                         the perf budget)
#
# Behavior:
#   1. Validate: working tree is clean; <branch> exists locally and on origin.
#   2. Update master: `git checkout master && git pull --ff-only`.
#   3. Update branch atop latest master:
#        non-stacked: `git checkout <branch> && git rebase master`
#        stacked:     `git checkout <branch> && git rebase --onto master <former-parent> <branch>`
#   4. With --budget, build the site and report the perf budget when the branch touched anything the
#      deployed site is built from, standing in for the hosted CI check that does not exist yet. The report
#      itself never fails the integration, but a build failure does, before anything is pushed.
#   5. Force-push branch (`git push --force-with-lease`).
#   6. Fast-forward master to branch tip via `git rebase <branch>` (consistent
#      with the rebase-family preference; equivalent to `merge --ff-only`).
#   7. Push master (`git push origin master`).
#   8. Run `./scripts/git/cleanup-merged.sh <branch>` to delete the branch from
#      origin, locally, and prune.

set -euo pipefail

# Paths whose contents end up in the deployed site, so a change to them moves the perf budget.
readonly BUDGET_RELEVANT_PATH_PATTERN='^(web/|shared/|Cargo\.lock$|Cargo\.toml$)'

# There is no hosted CI to run this, so it runs at the one point every change passes through.
# TODO: move to a CI workflow, posting the report as a PR comment, once hosted CI exists.
function report_site_budget {
    local changed_paths
    changed_paths="$(git diff --name-only "master..$BRANCH")"

    if ! printf '%s\n' "$changed_paths" | grep -Eq "$BUDGET_RELEVANT_PATH_PATTERN"; then
        echo "    nothing that affects the site changed; skipping"
        return 0
    fi

    # Built separately from the report, which always exits zero by design, so that a branch that stops
    # compiling once rebased fails here rather than being force-pushed and fast-forwarded onto master.
    "$(dirname "$0")/../build/build-site.sh"
    "$(dirname "$0")/../build/measure-site-budget.sh" --no-build
}

function main {
    BRANCH=""
    FROM_BRANCH=""
    USE_CURRENT_BRANCH=false
    RUN_BUDGET_REPORT=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --from)
                shift
                if [[ $# -lt 1 ]]; then
                    echo "error: --from requires a former-parent branch argument" >&2
                    exit 64
                fi
                FROM_BRANCH="$1"
                shift
                ;;
            --current)
                USE_CURRENT_BRANCH=true
                shift
                ;;
            --budget)
                RUN_BUDGET_REPORT=true
                shift
                ;;
            -h|--help)
                awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
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
        echo "usage: $0 (<branch> | --current) [--from <former-parent-branch>] [--budget]" >&2
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

    if [[ -n "$FROM_BRANCH" ]]; then
        if ! git show-ref --verify --quiet "refs/heads/$FROM_BRANCH" && \
           ! git rev-parse --verify --quiet "$FROM_BRANCH" > /dev/null; then
            echo "error: former-parent ref '$FROM_BRANCH' not resolvable" >&2
            exit 1
        fi
    fi

    echo ">>> Updating master"
    git checkout master
    git pull --ff-only

    echo ">>> Updating branch '$BRANCH' atop master"
    git checkout "$BRANCH"
    if [[ -n "$FROM_BRANCH" ]]; then
        git rebase --onto master "$FROM_BRANCH" "$BRANCH"
    else
        git rebase master
    fi

    if [[ "$RUN_BUDGET_REPORT" == true ]]; then
        echo ">>> Reporting the perf budget"
        report_site_budget
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
}

main "$@"
