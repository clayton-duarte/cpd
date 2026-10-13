#!/usr/bin/env bash
# The Gate for this repo (contract §1): typecheck plus all tests.
#
# One Gate at a time per machine. The real-bd tests start many short `bd` processes (~110 MB each);
# on the 8 GB MacBook Air three concurrent Gates swapped until `bd` calls hit their 120 s timeout,
# so correct Attempts failed at submit (Stage 2, 2026-10-13). Other Gates wait here instead.
# The lock is a directory (atomic mkdir) holding the owner's pid; a dead owner's lock is taken over.
set -euo pipefail
LOCK=/tmp/cpd-verify.lock
waited=""
while ! mkdir "$LOCK" 2>/dev/null; do
  owner=$(cat "$LOCK/pid" 2>/dev/null || true)
  if [ -n "$owner" ] && ! kill -0 "$owner" 2>/dev/null; then
    echo "verify.sh: taking over the Gate lock of dead pid $owner" >&2
    rm -rf "$LOCK"
    continue
  fi
  if [ -z "$waited" ]; then
    echo "verify.sh: waiting for the machine-wide Gate lock (pid ${owner:-?} in $(cat "$LOCK/where" 2>/dev/null || echo '?'))" >&2
    waited=1
  fi
  sleep 5
done
trap 'rm -rf "$LOCK"' EXIT
trap 'exit 143' TERM INT HUP
echo $$ > "$LOCK/pid"
pwd > "$LOCK/where"
[ -n "$waited" ] && echo "verify.sh: got the Gate lock" >&2
npx tsc --noEmit -p . && node --test
