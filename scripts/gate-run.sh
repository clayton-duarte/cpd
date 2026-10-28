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

cpd=${GATE_RUN_CPD:-$(dirname "$0")/../target/debug/cpd}

# The leak check is the last step, on every way out: exec'd, so its status is the wrapper's.
finish() {
  [ -x "$cpd" ] || exit "$1"
  exec "$cpd" gate finish --run "$GATE_RUN" --status "$1"
}

# The command runs in the background and the wrapper waits for it: a trap does not run while a foreground command
# does, and the runner cancels a step with INT, then TERM, then KILL. `<&0` keeps the command's stdin (an async
# command otherwise gets /dev/null).
"$@" <&0 &
pid=$!

# $1 the signal's name, $2 its number. The signal goes to the command alone, not to its process group: with no job
# control the wrapper shares that group, and the command (cargo-nextest, npm) stops its own tests on TERM, INT or
# HUP; what it leaves, `cpd gate finish` stops. An async command ignores INT (POSIX), so INT is passed on as TERM.
cancel() {
  trap '' INT TERM HUP
  case $1 in INT) set -- TERM "$2" ;; esac
  kill -s "$1" "$pid" 2>/dev/null
  wait "$pid"
  finish $((128 + $2))
}
trap 'cancel INT 2' INT
trap 'cancel TERM 15' TERM
trap 'cancel HUP 1' HUP

wait "$pid"
finish $?
