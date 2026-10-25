#!/bin/sh
# The bootstrap (it must exist before cpd does): builds cpd in release mode with the
# checkout's short commit, installs it through `cpd install`, then installs the
# holographic bridge beside the pi adapter. Safe to rerun. scripts/check-portable.sh
# guards this file: it runs on Linux and on macOS.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
CPD_COMMIT=$(git -C "$root" rev-parse --short HEAD) "$root/scripts/cargo.sh" build --release --locked --manifest-path "$root/Cargo.toml" -p cpd
"${CARGO_TARGET_DIR:-$root/target}/release/cpd" install --root "$root"

# The bridge lives in cpd and is installed next to the adapter. The venv beside it is
# dotfiles': this script never touches it. cp then mv keeps the old bridge whole until
# the new one is complete (docs/research/context-holographic.md, `## Install (dotfiles)`).
bridge_src="$root/contrib/holographic/bridge.py"
bridge_dir="$HOME/.local/share/cpd/holographic"
mkdir -p "$bridge_dir"
bridge_tmp="$bridge_dir/.bridge.py.tmp.$$"
trap 'rm -f "$bridge_tmp"' EXIT
cp "$bridge_src" "$bridge_tmp"
chmod 644 "$bridge_tmp"
mv "$bridge_tmp" "$bridge_dir/bridge.py"
trap - EXIT
