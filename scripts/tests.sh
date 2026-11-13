#!/bin/sh
# The test commands of cpd's `[tests]` table (contract §7.7), which are `cpd test` (docs/commands.md §26):
#   scripts/tests.sh build            compile the Rust tests (nextest --no-run), then `pnpm run typecheck`
#   scripts/tests.sh list <name>      exit 0 only when that one test exists
#   scripts/tests.sh run <name>...    run the named tests; non-zero when any failed or did not run
# Cargo builds and executes this checkout's cpd, including when its target directory is customized.
# `cpd test` runs from the repo root, whatever the cwd.
set -eu

cd "$(dirname "$0")/.."

for arg in "$@"; do
    shift
    case "$arg" in
        engine::*) set -- "$@" "${arg#engine::}" ;;
        core::*) set -- "$@" "${arg#core::}" ;;
        *) set -- "$@" "$arg" ;;
    esac
done

exec scripts/cargo.sh run -q -p cpd --bin cpd -- test "$@"
