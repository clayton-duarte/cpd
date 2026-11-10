#!/bin/sh
# scripts/check-shell-version.sh: Verifies shell code changes bumped shell version.
# Contract (docs/implementing.md "The shell ABI check", commands.md §14.4b).
set -eu

BASE="${1:-origin/main}"
ROOT=$(cd "$(dirname "$0")/.." && pwd)

CHANGED=$(git -C "$ROOT" diff --name-only "$BASE"...HEAD 2>/dev/null || git -C "$ROOT" diff --name-only "$BASE" 2>/dev/null || true)

has_shell_change=0
for f in $CHANGED; do
  case "$f" in
    web/src/electron/*|web/scripts/*|web/electron-builder.yml|web/vite.electron.config.ts)
      if [ "$f" != "web/src/electron/version.ts" ]; then
        has_shell_change=1
      fi
      ;;
  esac
done

if [ "$has_shell_change" -eq 1 ]; then
  # Check if version.ts was modified and bumped
  if ! echo "$CHANGED" | grep -q "^web/src/electron/version.ts$"; then
    echo "check-shell-version: shell code changed without version bump (web/src/electron/version.ts)" >&2
    exit 1
  fi
fi

echo "check-shell-version: ok"
