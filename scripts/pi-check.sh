#!/bin/sh
# Usage: scripts/pi-check.sh [<pi command>]   (default: pi)
# Checks that the adapter of this checkout (pi/) loads and activates in that
# pi, offline, in an empty temporary agent dir (docs/implementing.md,
# "Checking a pi version"). Exit 0 and one `pi-check: ok (<version>)` line,
# or exit 1 and `pi-check: FAIL: <step and why>` with the tail of pi's output.
# Needs tmux and node (step 3 runs pi's interactive mode, the only one that
# activates); a machine without them fails step 0, never skips (hs-d355).
#
# hs-d355, docs/implementing.md "Checking a pi version":
# - The Gate runs it: `npm run verify` calls `scripts/pi-check.sh
#   node_modules/.bin/pi`, the pi package.json pins, against this checkout's adapter.
# - The fake cpd holds no answers of its own: it records its argv, sleeps for
#   `events follow`, and runs `node pi/test/answers.ts <argv>` for the rest,
#   the same answers the adapter's tests use, so a field the adapter starts
#   requiring fails `npm test` and this check together.
# - It never starts a model turn: every pi command runs under `env -i` with an
#   allow-list (PATH LANG LC_ALL LC_CTYPE TERM when set, plus HOME TMPDIR
#   TMUX_TMPDIR GATE_RUN (when set) PI_CODING_AGENT_DIR PI_OFFLINE PI_SKIP_VERSION_CHECK CPD_SESSION CPD_HOME,
#   which it sets itself): no provider key, no proxy, no base URL.
# - It runs on macOS too: scripts/check-portable.sh covers this file.
# - A pass stays a pass (hs-d463; docs/implementing.md "Checking a pi version"): the
#   exit status is fixed before the cleanup runs and the cleanup cannot change it.
#   The cleanup kills the private tmux server and waits (2 s, then SIGKILL of the
#   pane's process group, 1 s) for its processes, retries `rm -rf` every 0.5 s for
#   at most PI_CHECK_CLEANUP_SECS (default 5), and on failure only warns
#   (`pi-check: warning: could not remove <tmp>`); it never prints FAIL.
# - Every tmux call is a UTF-8 client (hs-d463; contract §6.1): `tmux -u`, in the one
#   function `tmx`, whatever LANG/LC_ALL the caller has.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
adapter_dir="$root/pi"
pi_cmd=${1:-pi}

fail() {
  echo "pi-check: FAIL: $1" >&2
  if [ -n "${2:-}" ] && [ -s "$2" ]; then tail -n 20 "$2" >&2; fi
  exit 1
}

# Step 0, tools: step 3 needs tmux and the fake cpd needs node. A machine without them fails, never skips.
for tool in tmux node; do
  command -v "$tool" >/dev/null 2>&1 || fail "step 0 (tools): $tool is not installed (step 3 needs it)"
done

# The pane starts in another directory: a relative pi command (the Gate's node_modules/.bin/pi) is made absolute.
pi_bin=${pi_cmd%% *}
case $pi_bin in
  /*) ;;
  */*) pi_cmd="$(pwd)/$pi_bin${pi_cmd#"$pi_bin"}" ;;
esac

tmp=$(mktemp -d "${TMPDIR:-/tmp}/pi-check.XXXXXX")
socket="pi-check-${GATE_RUN:-x}-$$"
uid=$(id -u)
# tmux's socket is <TMUX_TMPDIR>/tmux-<uid>/<socket>; past 100 characters it nears sun_path (104 on macOS).
tmux_tmpdir="$tmp/tmux"
long_path=
if [ "$(printf '%s' "$tmp/tmux/tmux-$uid/$socket" | wc -c | tr -d ' ')" -gt 100 ]; then
  tmux_tmpdir=/tmp
  long_path=1
fi

shq() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }

# The allow-list (no model turn): what every pi command and the tmux client see, under `env -i`.
allow=
for name in PATH LANG LC_ALL LC_CTYPE TERM; do
  eval "isset=\${$name+1}"
  [ -n "$isset" ] || continue
  eval "value=\$$name"
  allow="$allow $name=$(shq "$value")"
done
# GATE_RUN (the Gate's run id, scripts/gate-run.sh) stays, so the run finds this check's processes; TMUX_TMPDIR is inside
# $tmp, so the server's socket is removed with it (tmux leaves a socket file when its server exits; hygiene.md bug 4);
# only when the socket path <TMUX_TMPDIR>/tmux-<uid>/<socket> would pass 100 characters (a long macOS TMPDIR) it is /tmp,
# which scripts/gate-run.sh scans, and the cleanup removes that socket file.
if [ -n "${GATE_RUN:-}" ]; then allow="$allow GATE_RUN=$(shq "$GATE_RUN")"; fi
allow="$allow HOME=$(shq "$tmp/home") TMPDIR=$(shq "$tmp") TMUX_TMPDIR=$(shq "$tmux_tmpdir") PI_CODING_AGENT_DIR=$(shq "$tmp/agent") PI_OFFLINE=1 PI_SKIP_VERSION_CHECK=1"
cleanenv() { eval "env -i $allow \"\$@\""; }
tmx() { cleanenv tmux -u -L "$socket" "$@"; }

# The process ids of the private server's panes, recorded while they live (tmux >= 3.8 prints none once a pane's tty closed).
pane_pids=
record_panes() { pane_pids="$pane_pids $(tmx list-panes -a -F '#{pane_pid}' 2>/dev/null || true)"; }

# True while a process of the group $1 lives (tmux makes a pane's process a session leader: its group id is its pid).
group_alive() { kill -0 -- "-$1" 2>/dev/null; }

# Waits, polling every 0.1 s, for at most $1 tenths of a second, until no pane's process group lives.
wait_panes() {
  waits=0
  while [ "$waits" -lt "$1" ]; do
    alive=0
    for pid in $pane_pids; do
      if group_alive "$pid"; then alive=1; fi
    done
    [ "$alive" -eq 1 ] || return 0
    waits=$((waits + 1))
    sleep 0.1
  done
  return 1
}

# The cleanup never changes the check's exit status (hs-d463): the status is saved first,
# every command here may fail, and the last line exits with it. It stops the private
# server and waits for its processes (a child still writing into $tmp made `rm -rf` fail
# `Directory not empty` on macOS), retries the removal, and only warns when it cannot.
cleanup() {
  status=$?
  set +e
  record_panes
  tmx kill-server >/dev/null 2>&1
  if [ -n "$long_path" ]; then rm -f "/tmp/tmux-$uid/$socket"; fi
  if ! wait_panes 20; then
    for pid in $pane_pids; do kill -9 -- "-$pid" 2>/dev/null; done
    wait_panes 10
  fi
  tries=0
  limit=$((${PI_CHECK_CLEANUP_SECS:-5} * 2))
  while :; do
    if err=$(rm -rf "$tmp" 2>&1); then break; fi
    tries=$((tries + 1))
    if [ "$tries" -ge "$limit" ]; then
      echo "pi-check: warning: could not remove $tmp: $err" >&2
      break
    fi
    sleep 0.5
  done
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

mkdir "$tmp/agent" "$tmp/bin" "$tmp/home" "$tmp/tmux"

version=$(cleanenv $pi_cmd --version 2>&1) || fail "step 1 (version): '$pi_cmd --version' failed: $version"
echo "$pi_cmd --version: $version"

# Step 2, load: RPC mode, no session variables; an extension that throws at load exits 1.
if ! cleanenv $pi_cmd --no-extensions --no-session -e "$adapter_dir" --mode rpc </dev/null >"$tmp/load.out" 2>"$tmp/load.err"; then
  fail "step 2 (load): pi exited non-zero loading $adapter_dir" "$tmp/load.err"
fi

# Step 3, activation: the adapter only activates in interactive mode (ctx.mode "tui"),
# so pi runs in a private tmux server. A fake cpd records its argv, sleeps for
# `events follow` and answers the rest from pi/test/answers.ts, the adapter's own test
# answers; pass when it was called with `events follow`.
calls="$tmp/calls"
: >"$calls"
cat >"$tmp/bin/cpd" <<FAKE
#!/bin/sh
echo "\$*" >>"$calls"
case "\$1 \$2" in
  "events follow") exec sleep 60 ;;
  *) exec node "$adapter_dir/test/answers.ts" "\$@" ;;
esac
FAKE
chmod 755 "$tmp/bin/cpd"

session=$(node -p 'crypto.randomUUID()')

tmx -f /dev/null new-session -d -x 120 -y 30 -c "$tmp/home" \
  "env -i $allow PATH=$(shq "$tmp/bin:$PATH") CPD_SESSION='$session' CPD_HOME=$(shq "$tmp/home") \
   $pi_cmd --no-extensions --no-session --session-id '$session' -e '$adapter_dir' 2>'$tmp/tui.err'; echo \"pi exited \$?\" >>'$tmp/tui.err'" \
  || fail "step 3 (activation): cannot start tmux"
record_panes

tries=0
until grep -q '^events follow' "$calls"; do
  tries=$((tries + 1))
  if [ "$tries" -gt 150 ] || ! tmx has-session 2>/dev/null; then
    tmx capture-pane -p >>"$tmp/tui.err" 2>/dev/null || true
    {
      echo "fake cpd calls:"
      cat "$calls"
      cat "$tmp/tui.err"
    } >"$tmp/step3.out"
    fail "step 3 (activation): the adapter did not run 'cpd events follow' within 15 s" "$tmp/step3.out"
  fi
  sleep 0.1
done

echo "pi-check: ok ($version)"
