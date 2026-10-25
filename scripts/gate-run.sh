#!/bin/sh
# Usage: scripts/gate-run.sh <command> [<args>...]
# Runs the command with a run id (GATE_RUN, 6 characters) and a TMPDIR of its own
# ($TMPDIR/gate-run.<id>), then fails when the run left anything behind: a process
# that has GATE_RUN in its environment, a path in the run's TMPDIR, or a tmux socket
# named with the id. It stops and removes what it listed (nothing else), prints
# `gate-run: FAIL: ...` on stderr and exits 1; a clean run prints nothing. The
# command's own exit status is returned unchanged when it is not 0, and the check
# still runs and prints (docs/research/hygiene.md, "The Gate leak check", hs-d519).
#
# - Another run has another id and another directory: it never matches.
# - Linux: the processes are found by the exact `GATE_RUN=<id>` entry in
#   /proc/<pid>/environ of this user's processes, which survives setsid, nohup and
#   a tmux server (it inherits the starter's environment). The script itself, its
#   ancestors and its own descendants are not leftovers.
# - macOS (run on Darwin arm64) has no /proc: `ps -axww -E -o pid=,command=` prints the
#   environment, and the lines with ` GATE_RUN=<id>` are the run's processes (the
#   same rule). Pids that are no longer alive are skipped (the scan's own ps and
#   grep have exited when it looks at them). The hint uses `ps -E -p <pid>`.
# - Every leftover process is reported with the `NEXTEST_TEST_NAME` of its environment (the test
#   that started it, read before the process is stopped) and its parent chain's command lines.
# - GATE_RUN_PROCFS (default /proc) is where the Linux branch reads processes from; a test points
#   it at a fake directory, or at a path that does not exist to take the `ps` branch on Linux too.
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
procfs=${GATE_RUN_PROCFS:-/proc}
uid=$(id -u)
self=$$
tmux_dirs="/tmp/tmux-$uid"
[ -n "${TMUX_TMPDIR:-}" ] && [ "${TMUX_TMPDIR%/}/tmux-$uid" != "$tmux_dirs" ] && tmux_dirs="$tmux_dirs ${TMUX_TMPDIR%/}/tmux-$uid"

# ppid_of PID: the parent's pid (empty when the process is gone).
ppid_of() {
  if [ -r "$procfs/$1/stat" ]; then
    _l=
    { read -r _l < "$procfs/$1/stat"; } 2>/dev/null || return 0
    _l=${_l##*) }
    set -- $_l
    echo "${2-}"
  else
    ps -o ppid= -p "$1" 2>/dev/null | tr -d ' '
  fi
}

# related PID: PID is this script, one of its ancestors or one of its descendants.
related() {
  _p=$1
  while [ -n "$_p" ] && [ "$_p" -gt 1 ]; do
    [ "$_p" -eq "$self" ] && return 0
    _p=$(ppid_of "$_p")
  done
  _a=$self
  while [ -n "$_a" ] && [ "$_a" -gt 1 ]; do
    [ "$_a" -eq "$1" ] && return 0
    _a=$(ppid_of "$_a")
  done
  return 1
}

# run_pids: the pids of this user's processes that have GATE_RUN=<id> in their environment.
run_pids() {
  if [ -d "$procfs/self" ]; then
    for _e in "$procfs"/[0-9]*/environ; do
      _p=${_e#"$procfs"/}
      _p=${_p%/environ}
      { tr '\0' '\n' < "$_e" | grep -qxF "GATE_RUN=$GATE_RUN"; } 2>/dev/null && echo "$_p"
    done
  else
    ps -axww -E -o pid=,command= 2>/dev/null | grep -F " GATE_RUN=$GATE_RUN" | while read -r _p _rest; do echo "$_p"; done
  fi
}

cmdline_of() {
  if [ -r "$procfs/$1/cmdline" ]; then
    tr '\0' ' ' < "$procfs/$1/cmdline" 2>/dev/null | cut -c 1-100
  else
    ps -o command= -p "$1" 2>/dev/null | cut -c 1-100
  fi
}

# env_value PID NAME: the value of NAME in PID's environment (empty when it has none, or when the
# system does not show it: on macOS an Apple platform binary hides its environment).
env_value() {
  if [ -d "$procfs/self" ]; then
    tr '\0' '\n' < "$procfs/$1/environ" 2>/dev/null | grep -m1 "^$2=" | cut -d= -f2-
  else
    ps -E -p "$1" -o command= 2>/dev/null | tr ' ' '\n' | grep -m1 "^$2=" | cut -d= -f2-
  fi
}

# parents_of PID: the pid and command line of each ancestor of PID, up to pid 1, `<pid> <command> < ...`.
parents_of() {
  _q=$(ppid_of "$1")
  _out=
  while [ -n "$_q" ] && [ "$_q" -ge 1 ]; do
    _out="${_out:+$_out < }$_q $(cmdline_of "$_q")"
    [ "$_q" -gt 1 ] || break
    _q=$(ppid_of "$_q")
  done
  echo "$_out"
}

finish() {
  status=$1
  pids=
  report=
  count=0
  nl='
'
  # The tripwire's log (scripts/tripwire/tmux): any line means a test called the real tmux.
  if [ -s "$TMPDIR/tripwire.log" ]; then
    {
      echo "gate-run: FAIL: a test called the real tmux:"
      cat "$TMPDIR/tripwire.log"
    } >&2
    [ "$status" -ne 0 ] || status=1
  fi
  rm -f "$TMPDIR/tripwire.log"
  for p in $(run_pids); do
    related "$p" && continue
    if [ -d "$procfs/self" ]; then
      [ -d "$procfs/$p" ] || continue
    else
      kill -0 "$p" 2>/dev/null || continue
    fi
    pids="$pids $p"
    report="${report}  process $p (ppid $(ppid_of "$p")) $(cmdline_of "$p")$nl"
    _t=$(env_value "$p" NEXTEST_TEST_NAME)
    report="${report}    NEXTEST_TEST_NAME=${_t:-(not in its environment, or not shown by this system)}$nl"
    report="${report}    parents: $(parents_of "$p")$nl"
    count=$((count + 1))
  done
  sockets=
  for d in $tmux_dirs; do
    for s in "$d"/*-"$GATE_RUN"-*; do
      [ -S "$s" ] || continue
      sockets="$sockets $s"
      report="${report}  socket  $s$nl"
      count=$((count + 1))
    done
  done
  paths=
  for f in "$RUN"/* "$RUN"/.[!.]* "$RUN"/..?*; do
    [ -e "$f" ] || [ -L "$f" ] || continue
    paths="$paths $f"
    report="${report}  path    $f$nl"
    count=$((count + 1))
  done
  if [ "$count" -eq 0 ]; then
    rmdir "$RUN" 2>/dev/null
    return "$status"
  fi
  {
    echo "gate-run: FAIL: this run (GATE_RUN=$GATE_RUN) left $count things behind:"
    printf '%s' "$report"
    if [ -n "$pids" ]; then
      set -- $pids
      if [ -d "$procfs/self" ]; then
        printf '%s\n' "gate-run: the test that left them: grep the id in the process environment (NEXTEST_TEST_NAME), e.g. \`tr '\\0' '\\n' <$procfs/$1/environ | grep NEXTEST_TEST_NAME\`"
      else
        printf '%s\n' "gate-run: the test that left them: grep the id in the process environment (NEXTEST_TEST_NAME), e.g. \`ps -E -p $1 -o command= | tr ' ' '\\n' | grep NEXTEST_TEST_NAME\`"
      fi
    fi
  } >&2
  # Only the pids, sockets and paths listed above.
  for p in $pids; do kill -KILL "$p" 2>/dev/null; done
  for s in $sockets; do rm -f "$s"; done
  rm -rf "$RUN"
  echo "gate-run: stopped $(set -- $pids; echo $#) process(es) and removed $(set -- $sockets $paths; echo $#) path(s) of this run (nothing else was touched)" >&2
  [ "$status" -ne 0 ] || status=1
  return "$status"
}

"$@"
cmd_status=$?
finish "$cmd_status"
exit $?
