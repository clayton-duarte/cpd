#!/bin/sh
# Installs node_modules from pnpm-lock.yaml (`pnpm install --frozen-lockfile`);
# pnpm does nothing when node_modules is already current.
set -eu

command -v pnpm >/dev/null 2>&1 || { echo 'pnpm-fresh: pnpm is not on PATH (package.json pins it with packageManager)' >&2; exit 127; }
exec pnpm install --frozen-lockfile
