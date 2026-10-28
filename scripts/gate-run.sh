#!/bin/sh
# Usage: scripts/gate-run.sh <command> [<args>...]
# Runs the command with a run id (GATE_RUN, 6 characters) and a TMPDIR of its own
# ($TMPDIR/gate-run.<id>), then hands the leak check to `cpd gate finish` (docs/implementing.md,
# "The split: scripts/gate-run.sh and cpd gate finish"). The cpd is $GATE_RUN_CPD, else target/debug/cpd.
# When there is no cpd (the build of the command failed, so nothing ran) the run directory is removed and the command's
# status is returned.
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
  # No cpd: nothing can check the run's directory, so it is removed here (nothing ran, nothing leaked), silently.
  [ -x "$cpd" ] || { rm -rf "$RUN"; exit "$1"; }
  exec "$cpd" gate finish --run "$GATE_RUN" --status "$1"
}

# $1 the signal's name, $2 its number. The signal goes to the command alone, not to its process group: with no job
# control the wrapper shares that group, and the command (cargo-nextest, npm) stops its own tests on TERM, INT or
# HUP; what it leaves, `cpd gate finish` stops. An async command ignores INT (POSIX), so INT is passed on as TERM.
# A signal that comes before the launch, or between `&` and `pid=$!`, finds `pid` empty: nothing is killed or waited
# for, and what the command left is stopped by `cpd gate finish`.
cancel() {
  trap '' INT TERM HUP
  case $1 in INT) set -- TERM "$2" ;; esac
  if [ -n "$pid" ]; then
    kill -s "$1" "$pid" 2>/dev/null
    wait "$pid"
  fi
  finish $((128 + $2))
}

# The traps come before the command starts: a signal that arrived with none set would kill the wrapper (sh's default
# action) and `cpd gate finish` would never run.
pid=
trap 'cancel INT 2' INT
trap 'cancel TERM 15' TERM
trap 'cancel HUP 1' HUP

# The command runs in the background and the wrapper waits for it: a trap does not run while a foreground command
# does, and the runner cancels a step with INT, then TERM, then KILL. The command keeps the wrapper's stdin (an async
# command otherwise gets /dev/null): it is saved on fd 3 first because dash opens /dev/null on fd 0 in the child
# before the command's own redirections, so `<&0` there would copy /dev/null.
exec 3<&0
"$@" <&3 3<&- &
pid=$!
exec 3<&-

wait "$pid"
finish $?
