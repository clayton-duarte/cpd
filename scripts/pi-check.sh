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
#   PI_CODING_AGENT_DIR PI_OFFLINE PI_SKIP_VERSION_CHECK CPD_SESSION CPD_HOME,
#   which it sets itself): no provider key, no proxy, no base URL.
# - It runs on macOS too: scripts/check-portable.sh covers this file.
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
socket="pi-check-$$"

shq() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }

# The allow-list (no model turn): what every pi command and the tmux client see, under `env -i`.
allow=
for name in PATH LANG LC_ALL LC_CTYPE TERM; do
  eval "isset=\${$name+1}"
  [ -n "$isset" ] || continue
  eval "value=\$$name"
  allow="$allow $name=$(shq "$value")"
done
allow="$allow HOME=$(shq "$tmp/home") TMPDIR=$(shq "$tmp") PI_CODING_AGENT_DIR=$(shq "$tmp/agent") PI_OFFLINE=1 PI_SKIP_VERSION_CHECK=1"
cleanenv() { eval "env -i $allow \"\$@\""; }
tmx() { cleanenv tmux -L "$socket" "$@"; }

cleanup() {
  tmx kill-server >/dev/null 2>&1 || true
  rm -rf "$tmp"
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
