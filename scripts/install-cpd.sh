#!/bin/sh
# Builds cpd in release mode and installs it as ~/.local/bin/cpd (R12), and
# the pi adapter pi/ as ~/.local/share/cpd/pi, where cpd finds it by
# default: <dir of the binary>/../share/cpd/pi (contract §6.5, §8 pi_adapter).
# Safe to rerun: the binary is replaced atomically, even while it runs.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
dest_dir="${HOME:?HOME is not set}/.local/bin"
adapter_dir="$dest_dir/../share/cpd/pi"

db="${CPD_HOME:-$HOME/.cpd}/cpd.db"

# Refuse, before building or replacing anything, a build that carries a migration
# that is not on origin/main (contract §3.7, hs-d178 D4): a preview would apply it
# to the shared database before the PR merges. A file of the working tree that is
# added, modified or untracked counts; one that only origin/main has does not (the
# store check below decides that). Git only: it needs nothing from the new binary.
migrations=core/src/store/migrations
if ! git -C "$root" fetch --quiet origin main; then
  echo "cpd install refused: cannot fetch origin/main to check that this build's migrations are on it." >&2
  exit 1
fi
unmerged=
for file in "$root/$migrations"/*; do
  [ -f "$file" ] || continue
  name=$(basename "$file")
  if ! git -C "$root" cat-file -e "origin/main:$migrations/$name" 2>/dev/null ||
    ! git -C "$root" show "origin/main:$migrations/$name" | cmp -s - "$file"; then
    unmerged="${unmerged:+$unmerged, }$name"
  fi
done
if [ -n "$unmerged" ]; then
  echo "cpd install refused: this build carries migration $unmerged that is not on origin/main, and a preview would apply it to $db before the PR merges. Merge main into this branch, or wait until the PR merges, then install again." >&2
  exit 1
fi

"$root/scripts/cargo.sh" build --release --locked --manifest-path "$root/Cargo.toml" -p cpd

target_dir="${CARGO_TARGET_DIR:-$root/target}"

# Refuse, before replacing anything, a build that cannot open the database
# (contract §3.7, hs-d151): ask the new binary, with the caller's CPD_HOME, for
# its store verdict (`store`: null without a database, else `opens`).
if ! version=$("$target_dir/release/cpd" --version 2>&1); then
  echo "cpd install refused: $target_dir/release/cpd --version failed:" >&2
  echo "$version" >&2
  exit 1
fi
if ! printf '%s' "$version" | grep -q '"store": *\(null\|{\)'; then
  echo "cpd install refused: $target_dir/release/cpd --version reports no store verdict:" >&2
  echo "$version" >&2
  exit 1
fi
if printf '%s' "$version" | grep -q '"opens": *false'; then
  code=$(printf '%s' "$version" | sed -n 's/.*"refusal": *{[^}]*"code": *"\([^"]*\)".*/\1/p')
  message=$(printf '%s' "$version" | sed -n 's/.*"refusal": *{.*"message": *"\(\(\\.\|[^"\\]\)*\)".*/\1/p')
  echo "cpd install refused: this build cannot open $db: $message" >&2
  case $code in
    store_too_new) echo "merge main into this branch, then install again" >&2 ;;
    migration_changed) echo "the branch's migration differs from the one this store applied; restore it from origin/main or wait for the store to be repaired" >&2 ;;
    migration_order) echo "rename the branch's breaking migration to a newer name with scripts/new-migration.sh, then install again" >&2 ;;
  esac
  exit 1
fi

mkdir -p "$dest_dir"
tmp="$dest_dir/.cpd.tmp.$$"
trap 'rm -f "$tmp"' EXIT
cp "$target_dir/release/cpd" "$tmp"
chmod 755 "$tmp"
mv -f "$tmp" "$dest_dir/cpd"
trap - EXIT

# The adapter is replaced as a whole; tests are not part of the install.
mkdir -p "$(dirname "$adapter_dir")"
adapter_tmp="$adapter_dir.tmp.$$"
trap 'rm -rf "$adapter_tmp"' EXIT
mkdir -p "$adapter_tmp"
cp -R "$root/pi/." "$adapter_tmp/"
rm -rf "$adapter_tmp/test"
rm -rf "$adapter_dir"
mv "$adapter_tmp" "$adapter_dir"
trap - EXIT

echo "installed $dest_dir/cpd and $adapter_dir" >&2

# A running cockpit's views keep the old binary until restarted (commands.md
# §13.1): when the configured server has a cockpit with its main window, restart
# its views. No config, no server or no main: nothing to restart. Likewise a
# running engine keeps running the old code (contract §3.7): when the work
# session has its `watch` window, kill it and start the engine again.
if config=$("$dest_dir/cpd" config 2>/dev/null); then
  tmux_conf=$(printf '%s' "$config" | sed -n 's/.*"tmux":{\([^}]*\)}.*/\1/p')
  socket=$(printf '%s' "$tmux_conf" | sed -n 's/.*"socket":"\([^"]*\)".*/\1/p')
  cockpit=$(printf '%s' "$tmux_conf" | sed -n 's/.*"cockpit_session":"\([^"]*\)".*/\1/p')
  work=$(printf '%s' "$tmux_conf" | sed -n 's/.*"work_session":"\([^"]*\)".*/\1/p')
  if [ -n "$socket" ] && [ -n "$cockpit" ] &&
    env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null list-windows -t "=$cockpit" -F '#{window_name}' 2>/dev/null | grep -qx main; then
    "$dest_dir/cpd" tui --respawn >&2
  fi
  if [ -n "$socket" ] && [ -n "$work" ] &&
    env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null list-windows -t "=$work" -F '#{window_name}' 2>/dev/null | grep -qx watch; then
    echo "restarting the running engine (cpd watch) to run the new binary" >&2
    env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null kill-window -t "=$work:watch" >&2
    # The killed engine releases its lock a moment later; --ensure answers
    # "already" until then.
    tries=0
    while :; do
      out=$("$dest_dir/cpd" watch --ensure) || break
      case $out in *'"already":true'*) ;; *) break ;; esac
      tries=$((tries + 1))
      [ "$tries" -lt 40 ] || break
      sleep 0.25
    done
    echo "$out" >&2
  fi
fi
