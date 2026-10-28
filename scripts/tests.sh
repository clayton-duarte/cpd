#!/bin/sh
# The test commands of cpd's `[tests]` table (contract §7.7), which are `cpd test` (docs/commands.md §26):
#   scripts/tests.sh build            compile the Rust tests (nextest --no-run), then `pnpm run typecheck`
#   scripts/tests.sh list <name>      exit 0 only when that one test exists
#   scripts/tests.sh run <name>...    run the named tests; non-zero when any failed or did not run
# This is only the bootstrap: it builds cpd with scripts/cargo.sh, takes the newest target/*/cpd, and hands over.
# `cpd test` runs from the repo root, whatever the cwd.
set -eu

cd "$(dirname "$0")/.."

scripts/cargo.sh build -q -p cpd >&2 # a no-op when cpd is current: a cpd older than this source has no `cpd test`
cpd=$(ls -t target/*/cpd | head -n 1)
exec "$cpd" test "$@"
