#!/bin/sh
# Builds cpd2 in release mode and installs it as ~/.local/bin/cpd2 (R12), and
# the pi adapter pi-v2/ as ~/.local/share/cpd2/pi, where cpd2 finds it by
# default: <dir of the binary>/../share/cpd2/pi (contract §6.5, §8 pi_adapter).
# Safe to rerun: the binary is replaced atomically, even while it runs.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
dest_dir="${HOME:?HOME is not set}/.local/bin"
adapter_dir="$dest_dir/../share/cpd2/pi"

"$root/scripts/cargo.sh" build --release --locked --manifest-path "$root/Cargo.toml" -p cpd2

target_dir="${CARGO_TARGET_DIR:-$root/target}"
mkdir -p "$dest_dir"
tmp="$dest_dir/.cpd2.tmp.$$"
trap 'rm -f "$tmp"' EXIT
cp "$target_dir/release/cpd2" "$tmp"
chmod 755 "$tmp"
mv -f "$tmp" "$dest_dir/cpd2"
trap - EXIT

# The adapter is replaced as a whole; tests are not part of the install.
mkdir -p "$(dirname "$adapter_dir")"
adapter_tmp="$adapter_dir.tmp.$$"
trap 'rm -rf "$adapter_tmp"' EXIT
mkdir -p "$adapter_tmp"
cp -R "$root/pi-v2/." "$adapter_tmp/"
rm -rf "$adapter_tmp/test"
rm -rf "$adapter_dir"
mv "$adapter_tmp" "$adapter_dir"
trap - EXIT

echo "installed $dest_dir/cpd2 and $adapter_dir" >&2
