#!/bin/sh
# Usage: scripts/new-migration.sh <slug> [additive|breaking]
#
# Creates core/src/store/migrations/<YYYYMMDDTHHMM>-<slug>.sql (the UTC minute
# now; contract §4.5 N1) with its declaration line (N3) and prints its path.
# The kind defaults to `additive`; a `breaking` one is `breaking after <n>`,
# n = how many migration files (legacy included) sort before the new name.
# Refuses a slug that is not [a-z][a-z0-9-]{0,39} and a name that exists.
set -eu
LC_ALL=C; export LC_ALL

usage() {
  echo "usage: scripts/new-migration.sh <slug> [additive|breaking]" >&2
  exit 2
}

[ $# -ge 1 ] && [ $# -le 2 ] || usage
slug=$1
kind=${2:-additive}

case $slug in
  [a-z]*) ;;
  *) echo "new-migration: the slug must start with a-z, got '$slug'." >&2; exit 2 ;;
esac
case $slug in
  *[!a-z0-9-]*) echo "new-migration: the slug may only hold a-z, 0-9 and '-', got '$slug'." >&2; exit 2 ;;
esac
if [ ${#slug} -gt 40 ]; then
  echo "new-migration: the slug is at most 40 characters, got ${#slug}." >&2
  exit 2
fi
case $kind in
  additive|breaking) ;;
  *) usage ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
dir=$root/core/src/store/migrations
[ -d "$dir" ] || { echo "new-migration: $dir does not exist." >&2; exit 1; }

name=$(date -u +%Y%m%dT%H%M)-$slug
file=$dir/$name.sql
if [ -e "$file" ]; then
  echo "new-migration: $file already exists." >&2
  exit 1
fi

if [ "$kind" = breaking ]; then
  # Names sort bytewise (N1): count the existing ones that sort before the new one.
  before=0
  for existing in "$dir"/*.sql; do
    [ -e "$existing" ] || continue
    other=$(basename "$existing" .sql)
    if [ "$(printf '%s\n%s\n' "$other" "$name" | LC_ALL=C sort | head -n 1)" = "$other" ] && [ "$other" != "$name" ]; then
      before=$((before + 1))
    fi
  done
  header="-- cpd-migration: breaking after $before"
else
  header="-- cpd-migration: additive"
fi

printf '%s\n' "$header" > "$file"
"$root/scripts/sync-version.sh" >&2
echo "$file"
