#!/usr/bin/env bash
# Always serve the thing under review on ONE port: 4000.
#
# The reviewer should never have to learn a new URL. http://localhost:4000 is
# the address, permanently. This script kills whatever is on 4000, checks out
# the ref you want to look at, and serves it there.
#
#   ./review.sh                      # serve the current branch
#   ./review.sh integration/phase-b  # serve a specific ref
set -euo pipefail

PORT=4000
REF="${1:-}"

cd "$(dirname "$0")"

# Free the port, whatever is holding it.
if lsof -ti "tcp:$PORT" >/dev/null 2>&1; then
  echo "freeing port $PORT"
  lsof -ti "tcp:$PORT" | xargs -r kill -9 2>/dev/null || true
  sleep 1
fi

if [ -n "$REF" ]; then
  echo "checking out $REF"
  git checkout -q "$REF"
fi

echo "serving $(git rev-parse --abbrev-ref HEAD) ($(git rev-parse --short HEAD)) on http://localhost:$PORT"
exec pnpm dev --port "$PORT" --strictPort
