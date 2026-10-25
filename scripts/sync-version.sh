#!/bin/sh
# Usage: scripts/sync-version.sh [--print]
#
# `cpd version sync [--print]` (docs/commands.md 26.2): cpd's version is
# 0.<PROTOCOL>.<migration files>. A dev tool: it needs a built cpd (the installed
# one, or this checkout's build).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
for cpd in "$root/target/release/cpd" "$root/target/debug/cpd" "$(command -v cpd || true)"; do
  if [ -n "$cpd" ] && [ -x "$cpd" ]; then
    cd "$root"
    exec "$cpd" version sync "$@"
  fi
done
echo "sync-version: no cpd is built; run scripts/cargo.sh build -p cpd, or scripts/install-cpd.sh" >&2
exit 1
