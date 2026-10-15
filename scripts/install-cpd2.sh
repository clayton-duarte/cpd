#!/bin/sh
# Builds cpd2 in release mode and installs it as ~/.local/bin/cpd2 (R12).
# Safe to rerun: the binary is replaced atomically, even while it runs.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
dest_dir="${HOME:?HOME is not set}/.local/bin"

"$root/scripts/cargo.sh" build --release --locked --manifest-path "$root/Cargo.toml" -p cpd2

target_dir="${CARGO_TARGET_DIR:-$root/target}"
mkdir -p "$dest_dir"
tmp="$dest_dir/.cpd2.tmp.$$"
trap 'rm -f "$tmp"' EXIT
cp "$target_dir/release/cpd2" "$tmp"
chmod 755 "$tmp"
mv -f "$tmp" "$dest_dir/cpd2"
trap - EXIT

echo "installed $dest_dir/cpd2" >&2
