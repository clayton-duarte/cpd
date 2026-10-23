#!/bin/sh
# Usage: scripts/sync-version.sh [--print]
#
# cpd's version is 0.<PROTOCOL>.<migration files>: PROTOCOL from
# core/src/events/kind.rs, the count of .sql files in core/src/store/migrations.
# --print prints it and writes nothing; no argument rewrites the workspace
# version in Cargo.toml and the four workspace crates in Cargo.lock.
set -eu

usage() {
  echo "usage: scripts/sync-version.sh [--print]" >&2
  exit 2
}

print=0
case $# in
  0) ;;
  1) [ "$1" = --print ] || usage; print=1 ;;
  *) usage ;;
esac

root=$(cd "$(dirname "$0")/.." && pwd)
kind=$root/core/src/events/kind.rs
dir=$root/core/src/store/migrations

fail() {
  echo "sync-version: $1" >&2
  exit 1
}

protocol=$(sed -n 's/^pub const PROTOCOL: u32 = \([0-9][0-9]*\);.*/\1/p' "$kind" 2>/dev/null | head -n 1)
[ -n "$protocol" ] || fail "no 'pub const PROTOCOL: u32 = <n>;' line in $kind."

n=0
for f in "$dir"/*.sql; do
  [ -e "$f" ] || continue
  n=$((n + 1))
done
[ "$n" -gt 0 ] || fail "no .sql file in $dir."

version=0.$protocol.$n
if [ "$print" = 1 ]; then
  echo "$version"
  exit 0
fi

toml=$root/Cargo.toml
lock=$root/Cargo.lock
grep -q '^\[workspace\.package\]' "$toml" || fail "no [workspace.package] in $toml."

tmp_toml=$(mktemp "${TMPDIR:-/tmp}/sync-version.XXXXXX")
tmp_lock=$(mktemp "${TMPDIR:-/tmp}/sync-version.XXXXXX")
trap 'rm -f "$tmp_toml" "$tmp_lock"' EXIT

awk -v v="$version" '
  /^\[/ { inws = ($0 == "[workspace.package]") }
  inws && !done && /^version = "/ { print "version = \"" v "\""; done = 1; next }
  { print }
  END { if (!done) exit 3 }
' "$toml" > "$tmp_toml" || fail "no version line under [workspace.package] in $toml."

# Buffer each [[package]] stanza, rewrite its version when it is a workspace crate.
awk -v v="$version" '
  function flush(   i, out) {
    for (i = 1; i <= n; i++) {
      out = buf[i]
      if (ws && !src && out ~ /^version = "/) out = "version = \"" v "\""
      print out
    }
    n = 0; ws = 0; src = 0
  }
  /^\[\[package\]\]/ { flush() }
  { buf[++n] = $0 }
  $0 ~ /^name = "(cpd|cpd-core|cpd-engine|cpd-tui)"$/ { ws = 1 }
  /^source = / { src = 1 }
  END { flush() }
' "$lock" > "$tmp_lock"

cat "$tmp_toml" > "$toml"
cat "$tmp_lock" > "$lock"
echo "sync-version: $version"
