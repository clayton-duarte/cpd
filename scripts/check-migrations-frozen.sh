#!/bin/sh
# Usage: scripts/check-migrations-frozen.sh [<base>]
#
# Contract §4.5 N5: a migration file that is on <base> (default origin/main) is
# never edited, renamed or deleted. Fails, naming the files, when the diff from
# the merge base of <base> and HEAD to HEAD modifies, renames or deletes one
# under core/src/store/migrations/. A new file is fine. Without <base> it first
# fetches origin/main (a failed fetch fails the check: it cannot know).
set -eu

dir=core/src/store/migrations/

if [ $# -ge 1 ]; then
  base=$1
else
  git fetch --quiet origin main
  base=origin/main
fi

changed=$(git diff --name-status -M --diff-filter=MDR "$base...HEAD" -- "$dir")
if [ -n "$changed" ]; then
  echo "migrations frozen check failed: $dir files that exist on $base were changed (M modified, D deleted, R renamed):" >&2
  echo "$changed" >&2
  echo "A migration on main is never edited, renamed or deleted (contract §4.5 N5): restore it, and put the change in a new migration (scripts/new-migration.sh)." >&2
  exit 1
fi
