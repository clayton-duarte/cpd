#!/bin/sh
# Usage: scripts/check-migrations-frozen.sh [<base>]
#
# Contract §4.5 N5: a migration file that is on <base> (default origin/main) is
# never edited, renamed or deleted. Fails, naming the files, when the diff from
# the merge base of <base> and HEAD to HEAD modifies, renames or deletes one
# under core/src/store/migrations/. A new file is fine.
#
# The one exception (contract §4.5 N5, O3, hs-d661): a migration whose first line
# is `-- cpd-migration: breaking after <n>` may be renamed to a newer name (the
# new name sorts after the old, LC_ALL=C) when every line after the first is
# byte-identical. An edit, a delete, a rename of a legacy or additive migration,
# a body change and a rename to an older name all stay refused.
#
# Without <base> it first fetches origin/main (a failed fetch fails the check: it
# cannot know).
set -eu

dir=core/src/store/migrations/

if [ $# -ge 1 ]; then
  base=$1
else
  git fetch --quiet origin main
  base=origin/main
fi

mb=$(git merge-base "$base" HEAD)

tmp=$(mktemp "${TMPDIR:-/tmp}/check-migrations-frozen.XXXXXX")
tmp2=$(mktemp "${TMPDIR:-/tmp}/check-migrations-frozen.XXXXXX")
trap 'rm -f "$tmp" "$tmp2"' EXIT

# The rename is the allowed O3 renumber: the old file's first line is a breaking
# declaration, the new name sorts after the old, and the bodies (every line after
# the first) are byte-identical.
allowed_renumber() {
  old=$1
  new=$2
  git cat-file -e "$mb:$old" 2>/dev/null || return 1
  git cat-file -e "HEAD:$new" 2>/dev/null || return 1
  first=$(git show "$mb:$old" | head -n 1)
  printf '%s\n' "$first" | grep -Eq '^-- cpd-migration: breaking after [0-9]+$' || return 1
  [ "$new" != "$old" ] || return 1
  newest=$(printf '%s\n%s\n' "$old" "$new" | LC_ALL=C sort | tail -n 1)
  [ "$newest" = "$new" ] || return 1
  git show "$mb:$old" | tail -n +2 > "$tmp"
  git show "HEAD:$new" | tail -n +2 > "$tmp2"
  cmp -s "$tmp" "$tmp2" || return 1
  return 0
}

changed=$(git diff --name-status -M --diff-filter=MDR "$mb" HEAD -- "$dir")
bad=
while IFS='	' read -r status old new; do
  [ -n "$status" ] || continue
  case $status in
    R*)
      allowed_renumber "$old" "$new" && continue
      bad="$bad$status	$old	$new
"
      ;;
    *)
      bad="$bad$status	$old
"
      ;;
  esac
done <<EOF
$changed
EOF

if [ -n "$bad" ]; then
  echo "migrations frozen check failed: $dir files that exist on $base were changed (M modified, D deleted, R renamed):" >&2
  printf '%s' "$bad" >&2
  echo "A migration on main is never edited or deleted. A breaking migration may be renamed to a newer name when its body is unchanged and only its first line (the \`breaking after <n>\` count) changes (contract §4.5 N5/O3); otherwise restore it and put the change in a new migration (scripts/new-migration.sh)." >&2
  exit 1
fi
