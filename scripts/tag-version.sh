#!/bin/sh
# Tags the commit of origin/main that last changed Cargo.toml's workspace version
# with the annotated tag v<version> and pushes it: `cpd tag` (docs/commands.md 26.1).
# A dev tool: it needs a built cpd (the installed one, or this checkout's build).
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
for cpd in "$root/target/release/cpd" "$root/target/debug/cpd" "$(command -v cpd || true)"; do
  if [ -n "$cpd" ] && [ -x "$cpd" ]; then
    cd "$root"
    exec "$cpd" tag
  fi
done
echo "tag-version: no cpd is built; run scripts/cargo.sh build -p cpd, or scripts/install-cpd.sh" >&2
exit 1
