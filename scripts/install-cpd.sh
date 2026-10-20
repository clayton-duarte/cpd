#!/bin/sh
# Builds cpd in release mode and installs it as ~/.local/bin/cpd (R12), and
# the pi adapter pi/ as ~/.local/share/cpd/pi, where cpd finds it by
# default: <dir of the binary>/../share/cpd/pi (contract §6.5, §8 pi_adapter).
# Safe to rerun: the binary is replaced atomically, even while it runs.
#
# Runs on Linux and on macOS, the same lines (hs-d355; scripts/check-portable.sh
# refuses Linux-only calls in this file, and the Gate runs it). The adapter
# directory is a symlink, ~/.local/share/cpd/pi -> pi.<id>, swapped by one
# rename(2) (python3 os.replace); a first install after hs-d355 migrates an
# existing real directory to pi.legacy (two renames, not atomic, once). Layout,
# migration, cleanup and what pi sees: contract §3.7 "The adapter swap";
# algorithm and tests: docs/implementing.md "The adapter swap".
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

# The adapter is replaced as a whole; tests are not part of the install
# (hs-d355, docs/implementing.md "The adapter swap"). A version is built in
# pi.tmp.$$ ($$ keeps concurrent installs apart), renamed to pi.<id> (<id> =
# UTC YYYYmmddTHHMMSS-$$) and linked as pi.link.$$ -> pi.<id>; one rename(2)
# (python3 os.replace, never renameat2, mv or ln -sfn) puts the link over
# $adapter_dir, so the path is always the old or the new version, whole. A real
# directory there (installs before hs-d355) is first renamed to pi.legacy: that
# one time $adapter_dir is missing for microseconds, and a failed second rename
# puts it back. Afterwards $adapter_dir/package.json is checked, and versions
# that are neither current, nor the displaced one, nor younger than 60 minutes
# are removed, with the tmp directories and links of dead pids. Preflight,
# before anything is replaced: python3 exists, $adapter_dir is absent, a
# symlink or a directory.
mkdir -p "$(dirname "$adapter_dir")"
adapter_tmp="$adapter_dir.tmp.$$"
trap 'rm -rf "$adapter_tmp"' EXIT
mkdir -p "$adapter_tmp"
cp -R "$root/pi/." "$adapter_tmp/"
rm -rf "$adapter_tmp/test"
if [ -d "$adapter_dir" ]; then
  python3 -c 'import ctypes, sys
RENAME_EXCHANGE, AT_FDCWD = 2, -100
a, b = (p.encode() for p in sys.argv[1:])
if ctypes.CDLL(None, use_errno=True).renameat2(AT_FDCWD, a, AT_FDCWD, b, RENAME_EXCHANGE) != 0:
    e = ctypes.get_errno()
    sys.exit("renameat2 %s <-> %s: errno %d" % (sys.argv[1], sys.argv[2], e))' "$adapter_tmp" "$adapter_dir"
else
  mv "$adapter_tmp" "$adapter_dir"
fi
rm -rf "$adapter_tmp"
trap - EXIT

echo "installed $dest_dir/cpd and $adapter_dir" >&2

# A running cockpit's views keep the old binary until restarted (commands.md
# §13.1): when the configured server has a cockpit with its main window, restart
# its views. No server or no main: nothing to restart. Likewise a running engine
# keeps running the old code (contract §3.7): when the work session has its
# `watch` window, kill it and start the engine again. No follow-up is skipped
# silently (hs-n278): each failure prints what failed and why, the later
# follow-ups still run, and the script exits non-zero at the end. cpd waits out a
# migration holding the setup lock (contract §4.2), so these run after it.
# How long to wait for the killed engine to release locks/watch.lock (tests set it).
lock_wait_secs="${CPD_INSTALL_LOCK_WAIT_SECS:-60}"
failed=0
fail() {
  echo "cpd install follow-up failed: $1" >&2
  failed=1
}
errfile=$(mktemp)
trap 'rm -f "$errfile"' EXIT

if ! config=$("$dest_dir/cpd" config 2>"$errfile"); then
  fail "cpd config: $config $(cat "$errfile"); the views were not respawned and the engine was not restarted"
else
  tmux_conf=$(printf '%s' "$config" | sed -n 's/.*"tmux":{\([^}]*\)}.*/\1/p')
  socket=$(printf '%s' "$tmux_conf" | sed -n 's/.*"socket":"\([^"]*\)".*/\1/p')
  cockpit=$(printf '%s' "$tmux_conf" | sed -n 's/.*"cockpit_session":"\([^"]*\)".*/\1/p')
  work=$(printf '%s' "$tmux_conf" | sed -n 's/.*"work_session":"\([^"]*\)".*/\1/p')
  if [ -n "$socket" ] && [ -n "$cockpit" ] &&
    env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null list-windows -t "=$cockpit" -F '#{window_name}' 2>/dev/null | grep -qx main; then
    "$dest_dir/cpd" tui --respawn >&2 || fail "cpd tui --respawn exited $?: the views still run the old binary"
  fi
  if [ -n "$socket" ] && [ -n "$work" ] &&
    env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null list-windows -t "=$work" -F '#{window_name}' 2>/dev/null | grep -qx watch; then
    echo "restarting the running engine (cpd watch) to run the new binary" >&2
    if ! env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null kill-window -t "=$work:watch" >&2; then
      fail "tmux kill-window $work:watch failed: the engine still runs the old binary"
    else
      # The killed engine releases its lock a moment later, possibly well after
      # (it finishes its tick, waits for a store write): wait for the lock
      # itself, bounded, then start the engine once and check that it holds the
      # lock (hs-n288). Nothing gives up silently.
      lock="${CPD_HOME:-$HOME/.cpd}/locks/watch.lock"
      mkdir -p "$(dirname "$lock")"
      waited=0
      while ! flock -n "$lock" true; do
        waited=$((waited + 1))
        if [ "$waited" -gt $((lock_wait_secs * 4)) ]; then
          break
        fi
        sleep 0.25
      done
      if ! flock -n "$lock" true; then
        fail "the old engine still holds $lock after ${lock_wait_secs}s: the new engine was not started"
      elif ! out=$("$dest_dir/cpd" watch --ensure 2>"$errfile"); then
        fail "cpd watch --ensure: $out $(cat "$errfile"); the engine is not running"
      elif flock -n "$lock" true; then
        fail "cpd watch --ensure answered $out, but no engine holds $lock: the engine is not running"
      else
        echo "$out" >&2
      fi
    fi
  fi
fi
[ "$failed" -eq 0 ] || exit 1
