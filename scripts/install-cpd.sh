#!/bin/sh
# The bootstrap (it must exist before cpd does): builds cpd in release mode with the
# checkout's short commit, then hands over to `cpd install`, which is the install
# (docs/implementing.md "The install"). Safe to rerun. scripts/check-portable.sh
# guards this file: it runs on Linux and on macOS.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
CPD_COMMIT=$(git -C "$root" rev-parse --short HEAD) "$root/scripts/cargo.sh" build --release --locked --manifest-path "$root/Cargo.toml" -p cpd
exec "${CARGO_TARGET_DIR:-$root/target}/release/cpd" install --root "$root"
