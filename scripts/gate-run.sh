#!/bin/sh
# Usage: scripts/gate-run.sh <command> [<args>...]
# Runs the command with a run id (GATE_RUN, 6 characters) and a TMPDIR of its own
# ($TMPDIR/gate-run.<id>), then hands the leak check to `cpd gate finish` (docs/implementing.md,
# "The split: scripts/gate-run.sh and cpd gate finish"). The cpd is $GATE_RUN_CPD, else target/debug/cpd.
# When there is no cpd (the build of the command failed, so nothing ran) the command's status is returned.
set -u

[ $# -gt 0 ] || { echo "usage: gate-run.sh <command> [<args>...]" >&2; exit 2; }

base=${TMPDIR:-/dev/shm}
[ -d "$base" ] || base=/tmp
RUN=$(mktemp -d "$base/gate-run.XXXXXX") || exit 2
GATE_RUN=${RUN##*.}
export GATE_RUN
TMPDIR=$RUN
export TMPDIR
# Node's compile cache would put a `node-compile-cache` directory in TMPDIR: a tool's cache, not a leak of a test.
NODE_DISABLE_COMPILE_CACHE=1
export NODE_DISABLE_COMPILE_CACHE

"$@"
status=$?
cpd=${GATE_RUN_CPD:-$(dirname "$0")/../target/debug/cpd}
[ -x "$cpd" ] || exit "$status"
exec "$cpd" gate finish --run "$GATE_RUN" --status "$status"
