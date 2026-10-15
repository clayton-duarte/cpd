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

# A running cockpit's views keep the old binary until restarted (commands.md
# §13.1): when the configured server has a cockpit with its main window, restart
# its views. No config, no server or no main: nothing to restart. Likewise a
# running engine keeps running the old code (contract §3.7): when the work
# session has its `watch` window, kill it and start the engine again.
if config=$("$dest_dir/cpd2" config 2>/dev/null); then
  tmux_conf=$(printf '%s' "$config" | sed -n 's/.*"tmux":{\([^}]*\)}.*/\1/p')
  socket=$(printf '%s' "$tmux_conf" | sed -n 's/.*"socket":"\([^"]*\)".*/\1/p')
  cockpit=$(printf '%s' "$tmux_conf" | sed -n 's/.*"cockpit_session":"\([^"]*\)".*/\1/p')
  work=$(printf '%s' "$tmux_conf" | sed -n 's/.*"work_session":"\([^"]*\)".*/\1/p')
  if [ -n "$socket" ] && [ -n "$cockpit" ] &&
    env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null list-windows -t "=$cockpit" -F '#{window_name}' 2>/dev/null | grep -qx main; then
    "$dest_dir/cpd2" tui --respawn >&2
  fi
  if [ -n "$socket" ] && [ -n "$work" ] &&
    env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null list-windows -t "=$work" -F '#{window_name}' 2>/dev/null | grep -qx watch; then
    echo "restarting the running engine (cpd2 watch) to run the new binary" >&2
    env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null kill-window -t "=$work:watch" >&2
    # The killed engine releases its lock a moment later; --ensure answers
    # "already" until then.
    tries=0
    while :; do
      out=$("$dest_dir/cpd2" watch --ensure) || break
      case $out in *'"already":true'*) ;; *) break ;; esac
      tries=$((tries + 1))
      [ "$tries" -lt 40 ] || break
      sleep 0.25
    done
    echo "$out" >&2
  fi
fi
