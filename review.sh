#!/usr/bin/env bash
# Serve the thing under review: the daemon + Vite dev server together.
#
# The reviewer should never have to learn a new URL. http://localhost:$CPD_REVIEW_PORT
# (4000 by default) is the address. This script frees that port, starts the
# daemon, waits for it to answer /api/health, starts Vite, and tears both
# processes down together on exit/interrupt. If the daemon never comes up it
# fails loudly instead of silently serving a backend-less app.
#
#   ./review.sh                      # serve the current branch
#   ./review.sh integration/phase-b  # serve a specific ref
#
# Env vars (all optional, defaults shown):
#   CPD_REVIEW_PORT=4000   # port Vite (and the reviewer's browser) use
#   CPD_DAEMON_PORT=4317   # port the daemon listens on
#   CPD_DAEMON_URL=http://localhost:$CPD_DAEMON_PORT  # proxy target
set -euo pipefail

CPD_REVIEW_PORT="${CPD_REVIEW_PORT:-4000}"
CPD_DAEMON_PORT="${CPD_DAEMON_PORT:-4317}"
export CPD_DAEMON_PORT
export CPD_DAEMON_URL="${CPD_DAEMON_URL:-http://localhost:$CPD_DAEMON_PORT}"

REF="${1:-}"

cd "$(dirname "$0")"

# Free the review port, whatever is holding it.
if lsof -ti "tcp:$CPD_REVIEW_PORT" >/dev/null 2>&1; then
  echo "freeing port $CPD_REVIEW_PORT"
  lsof -ti "tcp:$CPD_REVIEW_PORT" | xargs -r kill -9 2>/dev/null || true
  sleep 1
fi

if [ -n "$REF" ]; then
  echo "checking out $REF"
  git checkout -q "$REF"
fi

# Job control: each background job below gets its own process group, so we
# can kill the whole group (pnpm's wrapper shell + the real node child it
# execs) instead of just the top PID, which would leave an orphaned daemon.
set -m

DAEMON_PID=""
VITE_PID=""

kill_group() {
  pid="$1"
  kill -0 "$pid" 2>/dev/null || return 0
  kill -9 -- "-$pid" 2>/dev/null || kill -9 "$pid" 2>/dev/null || true
}

cleanup() {
  trap - EXIT INT TERM
  [ -n "$VITE_PID" ] && kill_group "$VITE_PID"
  [ -n "$DAEMON_PID" ] && kill_group "$DAEMON_PID"
  # Belt-and-suspenders: node --watch's supervisor can re-spawn a child that
  # outlives the group kill above (see package.json's `daemon` script).
  pkill -9 -f 'daemon/src/index.ts' 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "starting daemon on http://localhost:$CPD_DAEMON_PORT"
pnpm daemon &
DAEMON_PID=$!

# Fail loudly: never serve a dataless app silently.
ATTEMPTS=30
until curl -fsS "http://localhost:$CPD_DAEMON_PORT/api/health" >/dev/null 2>&1; do
  ATTEMPTS=$((ATTEMPTS - 1))
  if [ "$ATTEMPTS" -le 0 ]; then
    echo "ERROR: daemon did not answer /api/health on http://localhost:$CPD_DAEMON_PORT within 15s" >&2
    echo "refusing to serve an app with no backend." >&2
    exit 1
  fi
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo "ERROR: daemon process exited before it came up" >&2
    exit 1
  fi
  sleep 0.5
done

REF_NAME="$(git rev-parse --abbrev-ref HEAD)"
SHORT_SHA="$(git rev-parse --short HEAD)"
echo "serving $REF_NAME ($SHORT_SHA)"
echo "  review URL: http://localhost:$CPD_REVIEW_PORT"
echo "  daemon URL: http://localhost:$CPD_DAEMON_PORT"

pnpm dev --port "$CPD_REVIEW_PORT" --strictPort &
VITE_PID=$!

wait "$VITE_PID"
