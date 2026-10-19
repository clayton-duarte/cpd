#!/bin/sh
# Runs `npm ci` when node_modules is missing or stale against package-lock.json
# (e.g. a worktree that merged a main with new dependencies); otherwise does nothing.
# npm writes node_modules/.package-lock.json at every install; it omits the root
# package and packages skipped as optional, so compare package entries, not bytes.
set -eu

if node -e '
const fs = require("fs");
let hidden;
try { hidden = JSON.parse(fs.readFileSync("node_modules/.package-lock.json", "utf8")).packages; } catch { process.exit(1); }
const lock = JSON.parse(fs.readFileSync("package-lock.json", "utf8")).packages;
for (const key of Object.keys(hidden)) {
  if (!lock[key] || lock[key].version !== hidden[key].version) process.exit(1);
}
for (const key of Object.keys(lock)) {
  if (key !== "" && !hidden[key] && !lock[key].optional) process.exit(1);
}
'; then
  exit 0
fi
echo "npm-fresh: node_modules is missing or stale, running npm ci" >&2
exec npm ci --no-audit --no-fund
