#!/bin/sh
# The test commands of cpd's `[tests]` table (contract §7.7): builds, lists and runs named Rust and pi tests.
#   scripts/tests.sh build            compile the Rust tests (nextest --no-run), then `npm run typecheck`
#   scripts/tests.sh list <name>      exit 0 only when that one test exists
#   scripts/tests.sh run <name>...    run the named tests; non-zero when any failed or did not run
# A name is a Rust name (as nextest filters it, e.g. `watch::e4_adds_an_open_job`) or a pi name,
# `pi/test/<file>.test.ts::<title>`: the file's path, `::`, the exact `node:test` title.
# Rust names run with nextest, pi names with `node --test --test-name-pattern=<title> <file>`. Both run
# filtered, so neither takes a test slot (hs-d464). Runs from the repo root, whatever the cwd.
set -eu

cd "$(dirname "$0")/.."
cargo=scripts/cargo.sh

usage() {
  echo "usage: scripts/tests.sh build | list <name> | run <name>..." >&2
  exit 2
}

# A pi name: `pi/test/<file>.test.ts::<title>`.
is_pi() {
  case "$1" in
    pi/test/*.test.ts::?*) return 0 ;;
    *) return 1 ;;
  esac
}
pi_file() { printf '%s\n' "${1%%::*}"; }
pi_title() { printf '%s\n' "${1#*::}"; }

# Whether the title is in a `test(` or `it(` call of the file, quoted ', " or `.
pi_has() {
  [ -f "$1" ] || return 1
  for q in "'" '"' '`'; do
    for call in 'test(' 'it('; do
      if grep -qF -e "$call$q$2$q" "$1"; then return 0; fi
    done
  done
  return 1
}

# The title as an anchored regular expression that matches that title only.
pi_pattern() {
  printf '^%s$' "$(printf '%s' "$1" | sed 's/[][\\.*+?^$(){}|]/\\&/g')"
}

list_one() {
  if is_pi "$1"; then
    file=$(pi_file "$1")
    title=$(pi_title "$1")
    if pi_has "$file" "$title"; then return 0; fi
    echo "scripts/tests.sh: no test titled '$title' in $file" >&2
    return 1
  fi
  "$cargo" nextest list --workspace --message-format oneline "$1" | grep -q .
}

cmd=${1:-}
[ -n "$cmd" ] || usage
shift

case "$cmd" in
  build)
    [ $# -eq 0 ] || usage
    "$cargo" nextest run --workspace --no-run
    npm run typecheck
    ;;
  list)
    [ $# -eq 1 ] || usage
    list_one "$1"
    ;;
  run)
    [ $# -gt 0 ] || usage
    status=0
    pairs=
    nl='
'
    n=$#
    while [ "$n" -gt 0 ]; do
      name=$1
      shift
      n=$((n - 1))
      if is_pi "$name"; then
        pairs="$pairs$name$nl"
      else
        set -- "$@" "$name"
      fi
    done
    # Now $@ holds the Rust names only (the loop rotated them past the pi ones).
    if [ $# -gt 0 ]; then
      "$cargo" nextest run --workspace --no-fail-fast "$@" || status=1
    fi
    IFS=$nl
    files=$(for p in $pairs; do pi_file "$p"; done | sort -u)
    for file in $files; do
      set --
      for p in $pairs; do
        [ "$(pi_file "$p")" = "$file" ] || continue
        title=$(pi_title "$p")
        if ! pi_has "$file" "$title"; then
          echo "scripts/tests.sh: no test titled '$title' in $file" >&2
          status=1
          continue
        fi
        set -- "$@" "--test-name-pattern=$(pi_pattern "$title")"
      done
      [ $# -gt 0 ] || continue
      node --test "$@" "$file" || status=1
    done
    exit "$status"
    ;;
  *) usage ;;
esac
