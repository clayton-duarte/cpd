#!/bin/sh
# One-time cleanup of leftover worktrees whose work reached origin/main (hs-d138, hs-d143).
#
# Usage: scripts/prune-worktrees.sh [--apply] <repo>...
#
# Without --apply nothing changes (apart from `git fetch origin`) and the script prints what it
# would do. Needs git, gh (authenticated) and jq.
#
# Per worktree (the main one is skipped), the first rule that holds wins:
#   keep   dirty        `git status --porcelain` is not empty (untracked files included)
#   remove              HEAD is an ancestor of origin/main
#   remove              a merged PR P has a head branch equal to the Lead Branch L or starting
#                       with `L/`, HEAD is an ancestor of P's head commit, and P reached main
#                       (its base is main, or the base is the head of a merged PR that reached
#                       main, at most 5 levels)
#   keep   not merged   otherwise
# L is the branch, or for a branch containing `--` the part before the first `--`. A detached
# HEAD has no L. Removal deletes the worktree and its local branch, never a remote branch.
#
# Exit 0 unless a command failed; a failure is printed and the run goes on, exit 1 at the end.

set -eu

usage() {
    echo "usage: $0 [--apply] <repo>..." >&2
    exit 2
}

apply=0
if [ "${1-}" = "--apply" ]; then
    apply=1
    shift
fi
[ $# -gt 0 ] || usage
case "$1" in -*) usage ;; esac

for tool in git gh jq; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool not found" >&2
        exit 127
    }
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
status=0

# prune_repo <repo>: prints one line per worktree and a summary; returns 1 if a command failed.
prune_repo() {
    repo=$1
    failed=0
    removes=0
    keeps=0

    url=$(git -C "$repo" remote get-url origin) || return 1
    slug=$(printf '%s\n' "$url" | sed -E 's#/+$##; s#\.git$##; s#.*[:/]([^/:]+/[^/]+)$#\1#')

    git -C "$repo" fetch --quiet origin || return 1
    gh pr list --repo "$slug" --state merged --limit 500 \
        --json number,headRefName,baseRefName,headRefOid >"$tmp/prs.json" || return 1

    # One line per non-main worktree: path, HEAD, branch ("-" when detached), tab separated.
    git -C "$repo" worktree list --porcelain | awk -v RS= -F '\n' '
        NR > 1 {
            path = ""; head = ""; branch = "-"
            for (i = 1; i <= NF; i++) {
                if ($i ~ /^worktree /) path = substr($i, 10)
                else if ($i ~ /^HEAD /) head = substr($i, 6)
                else if ($i ~ /^branch refs\/heads\//) branch = substr($i, 19)
            }
            printf "%s\t%s\t%s\n", path, head, branch
        }' >"$tmp/worktrees" || return 1

    while IFS="$(printf '\t')" read -r wt head branch <&3; do
        label=$branch
        [ "$branch" != "-" ] || label=detached
        reason=

        if [ -d "$wt" ]; then
            if ! dirty=$(git -C "$wt" status --porcelain 2>&1); then
                echo "failed $wt: $dirty"
                failed=1
                continue
            fi
            [ -z "$dirty" ] || reason=dirty
        fi

        if [ -z "$reason" ]; then
            if git -C "$repo" merge-base --is-ancestor "$head" origin/main; then
                :
            elif merged_via_pr "$repo" "$branch" "$head"; then
                :
            else
                reason="not merged"
            fi
        fi

        if [ -n "$reason" ]; then
            echo "keep $wt ($label): $reason"
            keeps=$((keeps + 1))
            continue
        fi

        echo "remove $wt ($label)"
        removes=$((removes + 1))
        [ "$apply" -eq 1 ] || continue
        if ! err=$(git -C "$repo" worktree remove --force "$wt" 2>&1); then
            echo "failed $wt: $err"
            failed=1
            continue
        fi
        if [ "$branch" != "-" ] &&
            git -C "$repo" show-ref --verify --quiet "refs/heads/$branch" &&
            ! git -C "$repo" worktree list --porcelain | grep -qxF "branch refs/heads/$branch"; then
            if ! err=$(git -C "$repo" branch -D "$branch" 2>&1); then
                echo "failed $wt: $err"
                failed=1
            fi
        fi
    done 3<"$tmp/worktrees"

    if [ "$apply" -eq 1 ]; then
        git -C "$repo" worktree prune || failed=1
    fi
    echo "$repo: $removes remove, $keeps keep"
    return "$failed"
}

# merged_via_pr <repo> <branch> <head>: true when a merged PR of the branch's Lead Branch
# contains <head> and reached main.
merged_via_pr() {
    [ "$2" != "-" ] || return 1
    lead=${2%%--*}
    candidates=$(jq -r --arg lead "$lead" '
        . as $prs
        | def reached($base; $level):
            $base == "main"
            or ($level < 5
                and any($prs[] | select(.headRefName == $base); reached(.baseRefName; $level + 1)));
        .[]
        | select(.headRefName == $lead or (.headRefName | startswith($lead + "/")))
        | select(reached(.baseRefName; 0))
        | "\(.number) \(.headRefOid)"' "$tmp/prs.json") || {
        failed=1
        return 1
    }
    [ -n "$candidates" ] || return 1
    while read -r number oid; do
        if ! git -C "$1" cat-file -e "$oid^{commit}" 2>/dev/null; then
            git -C "$1" fetch --quiet origin "pull/$number/head" 2>/dev/null || {
                echo "failed $1: cannot fetch pull/$number/head" >&2
                failed=1
                continue
            }
        fi
        if git -C "$1" merge-base --is-ancestor "$3" "$oid" 2>/dev/null; then
            return 0
        fi
    done <<EOT
$candidates
EOT
    return 1
}

for repo in "$@"; do
    prune_repo "$repo" || {
        echo "failed $repo" >&2
        status=1
    }
done
exit "$status"
