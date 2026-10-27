#!/bin/sh
# Usage: scripts/check-migrations-frozen.sh [<base>]
#
# Contract §4.5 N5: a migration file that is on <base> (default origin/main) is
# never edited, renamed or deleted. Fails, naming the files, when the diff from
# the merge base of <base> and HEAD to HEAD modifies, renames or deletes one
# under core/src/store/migrations/. A new file is fine.
#
# The exceptions (contract §4.5 N5, O3; hs-d661, hs-d697, hs-d700, hs-d704): a
# migration whose first line is `-- cpd-migration: breaking after <n>` may be
# renamed back to an older name a live store already applied (the restore,
# hs-d697), and its `breaking after <n>` header may be corrected in place (the
# header-only correction, hs-d700). In every case every line after the first
# stays byte-identical. A forward rename (to a newer name) of a migration that
# already exists on <base> is refused (hs-d704, hs-n531): only a migration the PR
# itself adds may be renumbered forward before it lands (the O3 renumber, hs-d661).
# One named one-off (decision pro-d120, Plan mig-order): 20261031T1000-deploy-kind.sql
# was never applied by any store and sorted before the applied pr-description, so
# it may be renamed forward, header-only, to a newer name.
# An edit, a delete, a rename of a legacy or additive migration and a body change
# all stay refused.
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

# The allowed exception: both the old and the new file declare a breaking
# migration and their bodies (every line after the first) are byte-identical. A
# rename may move back to an older name a live store applied (the restore) and,
# for a same-name modification, only the header changed.
allowed_header_only() {
  old=$1
  new=$2
  git cat-file -e "$mb:$old" 2>/dev/null || return 1
  git cat-file -e "HEAD:$new" 2>/dev/null || return 1
  git show "$mb:$old" | head -n 1 | grep -Eq '^-- cpd-migration: breaking after [0-9]+$' || return 1
  git show "HEAD:$new" | head -n 1 | grep -Eq '^-- cpd-migration: breaking after [0-9]+$' || return 1
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
      if git cat-file -e "$base:$old" 2>/dev/null; then
        # The migration is on the base: only the hs-d700 restore, to an older name, is
        # allowed. A forward rename is the hs-n531 hole; refuse it.
        if [ "$(printf '%s\n%s\n' "$old" "$new" | LC_ALL=C sort | tail -n 1)" = "$old" ] \
          && allowed_header_only "$old" "$new"; then
          continue
        fi
        # The one-off forward rename (pro-d120).
        if [ "$old" = "${dir}20261031T1000-deploy-kind.sql" ] && allowed_header_only "$old" "$new"; then
          continue
        fi
      elif allowed_header_only "$old" "$new"; then
        # The old path is not on the base: a migration this PR adds may be renumbered.
        continue
      fi
      bad="$bad$status	$old	$new
"
      ;;
    M)
      allowed_header_only "$old" "$old" && continue
      bad="$bad$status	$old
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
  echo "A migration on main is never edited, renumbered or deleted. A breaking migration on main may only be restored to an older name a live store applied, and its first line (the \`breaking after <n>\` header) may be corrected, when every line after the first is byte-identical (contract §4.5 N5/O3). A forward rename of a migration on main is refused (hs-d704), except the one-off 20261031T1000-deploy-kind (pro-d120): only a migration this PR adds may be renumbered to a newer name before it lands. Otherwise restore it and put the change in a new migration (scripts/new-migration.sh)." >&2
  exit 1
fi
