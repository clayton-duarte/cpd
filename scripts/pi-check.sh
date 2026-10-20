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

tmp=$(mktemp -d "${TMPDIR:-/tmp}/pi-check.XXXXXX")
socket="pi-check-$$"
cleanup() {
  tmux -L "$socket" kill-server >/dev/null 2>&1 || true
  rm -rf "$tmp"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

fail() {
  echo "pi-check: FAIL: $1" >&2
  if [ -n "${2:-}" ] && [ -s "$2" ]; then tail -n 20 "$2" >&2; fi
  exit 1
}

version=$($pi_cmd --version 2>&1) || fail "step 1 (version): '$pi_cmd --version' failed: $version"
echo "$pi_cmd --version: $version"

mkdir "$tmp/agent" "$tmp/bin" "$tmp/home"

# Step 2, load: RPC mode, no session variables; an extension that throws at load exits 1.
if ! env -u CPD_SESSION -u CPD_PLAN -u CPD_HOME -u CPD_ATTEMPT \
  PI_CODING_AGENT_DIR="$tmp/agent" PI_OFFLINE=1 PI_SKIP_VERSION_CHECK=1 \
  $pi_cmd --no-extensions --no-session -e "$adapter_dir" --mode rpc </dev/null >"$tmp/load.out" 2>"$tmp/load.err"; then
  fail "step 2 (load): pi exited non-zero loading $adapter_dir" "$tmp/load.err"
fi

# Step 3, activation: the adapter only activates in interactive mode (ctx.mode "tui"),
# so pi runs in a private tmux server. A fake cpd answers what the adapter calls
# at activation and records its argv; pass when it was called with `events follow`.
calls="$tmp/calls"
: >"$calls"
cat >"$tmp/bin/cpd" <<FAKE
#!/bin/sh
echo "\$*" >>"$calls"
case "\$1 \$2" in
  "session show") echo '{"ok":true,"session":{"plan":null,"plan_lead":false,"registered_event":0,"name":null}}' ;;
  "events follow") exec sleep 60 ;;
  *) echo '{"ok":true}' ;;
esac
FAKE
chmod 755 "$tmp/bin/cpd"

session=$(cat /proc/sys/kernel/random/uuid 2>/dev/null || uuidgen)
session=$(echo "$session" | tr 'A-F' 'a-f')

tmux -L "$socket" -f /dev/null new-session -d -x 120 -y 30 -c "$tmp/home" \
  "env PATH='$tmp/bin':\"\$PATH\" PI_CODING_AGENT_DIR='$tmp/agent' PI_OFFLINE=1 PI_SKIP_VERSION_CHECK=1 CPD_SESSION='$session' CPD_HOME='$tmp/home' \
   $pi_cmd --no-extensions --no-session --session-id '$session' -e '$adapter_dir' 2>'$tmp/tui.err'; echo \"pi exited \$?\" >>'$tmp/tui.err'" \
  || fail "step 3 (activation): cannot start tmux"

tries=0
until grep -q '^events follow' "$calls"; do
  tries=$((tries + 1))
  if [ "$tries" -gt 150 ] || ! tmux -L "$socket" has-session 2>/dev/null; then
    tmux -L "$socket" capture-pane -p >>"$tmp/tui.err" 2>/dev/null || true
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
