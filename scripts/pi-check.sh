#!/bin/sh
# Usage: scripts/pi-check.sh [<pi command>]   (default: pi)
# Checks that the adapter of this checkout (pi/) loads and activates in that
# pi, offline, in an empty temporary agent dir (docs/implementing.md,
# "Checking a pi version"). Exit 0 and one `pi-check: ok (<version>)` line,
# or exit 1 and `pi-check: FAIL: <step and why>` with the tail of pi's output.
# Needs python3 and node (step 3 runs pi's interactive mode, the only one that
# activates, and that needs a tty: scripts/pi-check-pty.py gives it a pty); a
# machine without them fails step 0, never skips (hs-d355).
#
# hs-d355, docs/implementing.md "Checking a pi version":
# - The Gate runs it: `pnpm run verify` calls `scripts/pi-check.sh
#   node_modules/.bin/pi`, the pi package.json pins, against this checkout's adapter.
# - The fake cpd holds no answers of its own: it records its argv, tells the
#   runner on a fifo for `events follow` (then blocks on another fifo, no `sleep`),
#   and runs `node pi/test/answers.ts <argv>` for the rest, the same answers the
#   adapter's tests use, so a field the adapter starts requiring fails `pnpm test`
#   and this check together.
# - It never starts a model turn: every pi command runs under `env -i` with an
#   allow-list (PATH LANG LC_ALL LC_CTYPE TERM when set, plus HOME TMPDIR
#   GATE_RUN (when set) PI_CODING_AGENT_DIR PI_OFFLINE PI_SKIP_VERSION_CHECK CPD_SESSION CPD_HOME,
#   which it sets itself): no provider key, no proxy, no base URL.
# - It runs on macOS too: scripts/check-portable.sh covers this file and the runner.
# - A pass stays a pass (hs-d463; docs/implementing.md "Checking a pi version"): the
#   exit status is fixed before the cleanup runs and the cleanup cannot change it.
#   The runner kills pi's whole process group and waits for it before it returns (also
#   on SIGTERM and SIGINT); the cleanup then retries `rm -rf` every PI_CHECK_CLEANUP_PAUSE
#   seconds (default 0.5) for at most PI_CHECK_CLEANUP_SECS (default 5), and on failure
#   only warns (`pi-check: warning: could not remove <tmp>`); it never prints FAIL.
# - PI_CHECK_ACTIVATION_SECS (default 60) is how long step 3 waits for `events follow`.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
adapter_dir="$root/pi"
pty_py="$root/scripts/pi-check-pty.py"
pi_cmd=${1:-pi}

fail() {
  echo "pi-check: FAIL: $1" >&2
  if [ -n "${2:-}" ] && [ -s "$2" ]; then tail -n 20 "$2" >&2; fi
  exit 1
}

# Step 0, tools: step 3 needs python3 (the pty) and the fake cpd needs node. A machine without them fails, never skips.
for tool in python3 node; do
  command -v "$tool" >/dev/null 2>&1 || fail "step 0 (tools): $tool is not installed (step 3 needs it)"
done

# pi runs in another directory: a relative pi command (the Gate's node_modules/.bin/pi) is made absolute.
pi_bin=${pi_cmd%% *}
case $pi_bin in
  /*) ;;
  */*) pi_cmd="$(pwd)/$pi_bin${pi_cmd#"$pi_bin"}" ;;
esac

tmp=$(mktemp -d "${TMPDIR:-/tmp}/pi-check.XXXXXX")

shq() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }

# The allow-list (no model turn): what every pi command sees, under `env -i`.
allow=
for name in PATH LANG LC_ALL LC_CTYPE TERM; do
  eval "isset=\${$name+1}"
  [ -n "$isset" ] || continue
  eval "value=\$$name"
  allow="$allow $name=$(shq "$value")"
done
# GATE_RUN (the Gate's run id, scripts/gate-run.sh) stays, so the run finds this check's processes; TMPDIR is $tmp,
# so everything pi writes is removed with it.
if [ -n "${GATE_RUN:-}" ]; then allow="$allow GATE_RUN=$(shq "$GATE_RUN")"; fi
allow="$allow HOME=$(shq "$tmp/home") TMPDIR=$(shq "$tmp") PI_CODING_AGENT_DIR=$(shq "$tmp/agent") PI_OFFLINE=1 PI_SKIP_VERSION_CHECK=1"
cleanenv() { eval "env -i $allow \"\$@\""; }

# The runner of step 3, while it runs.
runner=

# The cleanup never changes the check's exit status (hs-d463): the status is saved first,
# every command here may fail, and the last line exits with it. It stops the runner (which
# kills pi's group and waits for it: a child still writing into $tmp made `rm -rf` fail
# `Directory not empty` on macOS), retries the removal, and only warns when it cannot.
cleanup() {
  status=$?
  set +e
  if [ -n "$runner" ]; then
    kill -TERM "$runner" 2>/dev/null
    wait "$runner" 2>/dev/null
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
    python3 "$pty_py" pause "${PI_CHECK_CLEANUP_PAUSE:-0.5}"
  done
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

mkdir "$tmp/agent" "$tmp/bin" "$tmp/home"

version=$(cleanenv $pi_cmd --version 2>&1) || fail "step 1 (version): '$pi_cmd --version' failed: $version"
echo "$pi_cmd --version: $version"

# Step 2, load: RPC mode, no session variables; an extension that throws at load exits 1.
if ! cleanenv $pi_cmd --no-extensions --no-session -e "$adapter_dir" --mode rpc </dev/null >"$tmp/load.out" 2>"$tmp/load.err"; then
  fail "step 2 (load): pi exited non-zero loading $adapter_dir" "$tmp/load.err"
fi

# Step 3, activation: the adapter only activates in interactive mode (ctx.mode "tui"),
# so pi runs on a pty (the runner). A fake cpd records its argv, writes to the `follow`
# fifo and blocks on the `hold` fifo (nobody writes it; the runner kills it) for
# `events follow`, and answers the rest from pi/test/answers.ts, the adapter's own test
# answers; pass when it was called with `events follow`.
calls="$tmp/calls"
: >"$calls"
mkfifo "$tmp/follow" "$tmp/hold"
cat >"$tmp/bin/cpd" <<FAKE
#!/bin/sh
echo "\$*" >>"$calls"
case "\$1 \$2" in
  "events follow") echo follow >"$tmp/follow"; exec cat "$tmp/hold" ;;
  *) exec node "$adapter_dir/test/answers.ts" "\$@" ;;
esac
FAKE
chmod 755 "$tmp/bin/cpd"

session=$(node -p 'crypto.randomUUID()')

eval "set -- $allow PATH=$(shq "$tmp/bin:$PATH") CPD_SESSION=$(shq "$session") CPD_HOME=$(shq "$tmp/home")"
python3 "$pty_py" run --seconds "${PI_CHECK_ACTIVATION_SECS:-60}" --follow "$tmp/follow" --cwd "$tmp/home" --stderr "$tmp/tui.err" --out "$tmp/pty.out" -- \
  env -i "$@" $pi_cmd --no-extensions --no-session --session-id "$session" -e "$adapter_dir" &
runner=$!
status=0
wait "$runner" || status=$?
runner=

case $status in
  0) ;;
  130) exit 130 ;;
  *)
    {
      echo "fake cpd calls:"
      cat "$calls"
      cat "$tmp/tui.err"
      tail -n +2 "$tmp/pty.out"
    } >"$tmp/step3.out" 2>/dev/null
    fail "step 3 (activation): the adapter did not run 'cpd events follow': $(head -n 1 "$tmp/pty.out" 2>/dev/null)" "$tmp/step3.out"
    ;;
esac

echo "pi-check: ok ($version)"
