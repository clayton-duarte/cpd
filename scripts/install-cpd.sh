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
# existing real directory to pi.legacy (two renames in one process, not atomic, once). Layout,
# migration, cleanup and what pi sees: contract §3.7 "The adapter swap";
# algorithm and tests: docs/implementing.md "The adapter swap".
#
# Engine order (hs-d463; contract §3.7 "The install's engine order", docs/implementing.md
# "The install's engine order"): the running engine is never stopped before the new
# binary is known to be able to start one. After the swap: `cpd config`, then
# `cpd watch --check` (read-only: config, store, tmux listing, the home); a failure
# prints "the engine was not touched" and runs no kill-window. Then `cpd tui --respawn`
# (its own check before its first respawn-pane -k), then retire the old engine
# (kill-window, wait for locks/watch.lock, bounded) and start the new one (`watch
# --ensure` once, then check the lock is held). A failure after the kill-window says
# "the old engine was stopped and no engine is running"; every failure prints
# `cpd install follow-up failed: ...` and the exit is non-zero at the end.
#
# Every tmux call is a UTF-8 client (hs-d463; contract §6.1): `tmux -u`, in the one
# function `tmx` below, whatever LANG/LC_ALL the caller has (a launchd agent sets only
# PATH, and without a UTF-8 locale tmux prints the -F tabs as `_`).
# scripts/check-portable.sh refuses a bare `tmux` in this file.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
# Every tmux call of this script, a UTF-8 client: `-u` (hs-d463). $socket is set before the first call.
tmx() { env -u TMUX -u TMUX_PANE tmux -u -L "$socket" -f /dev/null "$@"; }
dest_dir="${HOME:?HOME is not set}/.local/bin"
# <dir of the binary>/../share/cpd, where cpd finds the adapter; $adapter_dir is the symlink.
share="$HOME/.local/share/cpd"
adapter_dir="$share/pi"

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

# Preflight for the adapter swap, before anything is built or replaced (docs/implementing.md "The adapter swap", step 1).
if ! command -v python3 >/dev/null 2>&1; then
  echo "cpd install refused: python3 is required for the atomic adapter swap" >&2
  exit 1
fi
if { [ -e "$adapter_dir" ] || [ -L "$adapter_dir" ]; } && [ ! -L "$adapter_dir" ] && [ ! -d "$adapter_dir" ]; then
  echo "cpd install refused: $adapter_dir is neither a symlink nor a directory" >&2
  exit 1
fi

CPD_COMMIT=$(git -C "$root" rev-parse --short HEAD) "$root/scripts/cargo.sh" build --release --locked --manifest-path "$root/Cargo.toml" -p cpd

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
  message=$(printf '%s' "$version" | sed -En 's/.*"refusal": *\{.*"message": *"((\\.|[^"\\])*)".*/\1/p')
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
# one time $adapter_dir is missing between two renames in one python3 process
# (microseconds), and a failed second rename puts it back in that process. Afterwards $adapter_dir/package.json is checked, and versions
# that are neither current, nor the displaced one, nor younger than 60 minutes
# are removed, with the tmp directories and links of dead pids.
mkdir -p "$share"
adapter_tmp="$share/pi.tmp.$$"
adapter_link="$share/pi.link.$$"
version_name="pi.$(date -u +%Y%m%dT%H%M%S)-$$"
trap 'rm -rf "$adapter_tmp" "$adapter_link"' EXIT
mkdir "$adapter_tmp"
cp -R "$root/pi/." "$adapter_tmp/"
rm -rf "$adapter_tmp/test"
mv "$adapter_tmp" "$share/$version_name"
ln -s "$version_name" "$adapter_link"

displaced=
if [ -L "$adapter_dir" ]; then
  displaced=$(readlink "$adapter_dir")
elif [ -d "$adapter_dir" ]; then
  displaced=pi.legacy
fi

# One python3 process does the renames, so the migration's window is two system calls apart, not a process start-up.
if [ -L "$adapter_dir" ] || [ ! -e "$adapter_dir" ]; then
  swap_py='import os, sys; os.replace(sys.argv[1], sys.argv[2])'
  swap_failed="cannot swap the adapter link over $adapter_dir"
else
  # The migration: a rename of a symlink over a directory fails, and no portable call exchanges the two.
  rm -rf "$share/pi.legacy"
  swap_py='import os, sys
link, adapter, legacy = sys.argv[1:4]
os.rename(adapter, legacy)
try:
    os.replace(link, adapter)
except BaseException:
    os.rename(legacy, adapter)
    raise'
  swap_failed="cannot swap the adapter link over $adapter_dir; the old directory is back"
fi
if ! python3 -c "$swap_py" "$adapter_link" "$adapter_dir" "$share/pi.legacy"; then
  echo "cpd install failed: $swap_failed" >&2
  exit 1
fi
trap - EXIT

if [ ! -f "$adapter_dir/package.json" ]; then
  echo "cpd install failed: $adapter_dir has no package.json after the swap" >&2
  exit 1
fi

# Clean up (a failure is a warning, not a failed install): versions that are not current, not
# the displaced one and not modified in the last 60 minutes; scratch of pids that are gone.
current=$(readlink "$adapter_dir")
cleaned=1
for old in $(find "$share" -maxdepth 1 \( -name 'pi.legacy' -o -name 'pi.[0-9]*-[0-9]*' \) -mmin +60 2>/dev/null); do
  name=$(basename "$old")
  [ "$name" != "$current" ] && [ "$name" != "$displaced" ] || continue
  rm -rf "$old" || cleaned=0
done
for scratch in "$share"/pi.tmp.* "$share"/pi.link.*; do
  [ -e "$scratch" ] || [ -L "$scratch" ] || continue
  pid=${scratch##*.}
  kill -0 "$pid" 2>/dev/null || rm -rf "$scratch" || cleaned=0
done
[ "$cleaned" -eq 1 ] || echo "cpd install warning: could not remove every old adapter version under $share" >&2

echo "installed $dest_dir/cpd and $adapter_dir" >&2

# A running cockpit's views keep the old binary until restarted (commands.md
# §13.1), and a running engine keeps running the old code (contract §3.7). The
# order (hs-d463, contract §3.7 "The install's engine order"): verify with the new
# binary everything its start depends on (`cpd config`, `cpd watch --check`, which
# writes nothing), and only then respawn the views, retire the old engine and
# start the new one. A failure before the retirement leaves the engine and the
# views as they are; one after it says that the old engine was stopped. No
# follow-up is skipped silently (hs-n278): each failure prints what failed and
# why, the later follow-ups that can still run do, and the script exits non-zero
# at the end. cpd waits out a migration holding the setup lock (contract §4.2),
# so these run after it.
# How long to wait for the killed engine to release locks/watch.lock (tests set it).
lock_wait_secs="${CPD_INSTALL_LOCK_WAIT_SECS:-60}"
failed=0
# True when nobody holds the flock(2) lock on $1 (the lock cpd takes, core/src/lock.rs); macOS ships no flock command.
lock_free() {
  python3 -c 'import fcntl, sys
try:
    fcntl.flock(open(sys.argv[1], "a"), fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    sys.exit(1)' "$1"
}
fail() {
  echo "cpd install follow-up failed: $1" >&2
  failed=1
}
# The code and the message of a cpd error value, else its text as it is.
describe() {
  code=$(printf '%s' "$1" | sed -n 's/.*"error": *{[^}]*"code": *"\([^"]*\)".*/\1/p')
  message=$(printf '%s' "$1" | sed -n 's/.*"error": *{.*"message": *"\(\(\\.\|[^"\\]\)*\)".*/\1/p')
  if [ -n "$code" ] && [ -n "$message" ]; then printf '%s (%s)' "$message" "$code"; else printf '%s' "$1"; fi
}
untouched="the engine was not touched: the engine that runs, if one does, is the old one and keeps running; the views were not respawned"
errfile=$(mktemp)
trap 'rm -f "$errfile"' EXIT

if ! config=$("$dest_dir/cpd" config 2>"$errfile"); then
  fail "cpd config: $config $(cat "$errfile"); $untouched"
else
  tmux_conf=$(printf '%s' "$config" | sed -n 's/.*"tmux":{\([^}]*\)}.*/\1/p')
  socket=$(printf '%s' "$tmux_conf" | sed -n 's/.*"socket":"\([^"]*\)".*/\1/p')
  cockpit=$(printf '%s' "$tmux_conf" | sed -n 's/.*"cockpit_session":"\([^"]*\)".*/\1/p')
  work=$(printf '%s' "$tmux_conf" | sed -n 's/.*"work_session":"\([^"]*\)".*/\1/p')
  # Step 1, verify with the new binary, nothing stopped: config, store, tmux, its listing, the home.
  if ! checked=$("$dest_dir/cpd" watch --check 2>"$errfile"); then
    fail "cpd watch --check: $(describe "$checked") $(cat "$errfile"); $untouched"
  else
    # Step 2, the views: `cpd tui --respawn` checks before its first respawn-pane -k.
    if [ -n "$socket" ] && [ -n "$cockpit" ] &&
      tmx list-windows -t "=$cockpit" -F '#{window_name}' 2>/dev/null | grep -qx main; then
      if out=$("$dest_dir/cpd" tui --respawn 2>"$errfile"); then
        echo "$out" >&2
      else
        code=$?
        fail "cpd tui --respawn exited $code: $(describe "$out") $(cat "$errfile"); the views still run the old binary"
      fi
    fi
    # Step 3, retire the old engine, then start the new one.
    if [ -n "$socket" ] && [ -n "$work" ] &&
      tmx list-windows -t "=$work" -F '#{window_name}' 2>/dev/null | grep -qx watch; then
      echo "restarting the running engine (cpd watch) to run the new binary" >&2
      # Mark the restart as planned (hs-d471): the new engine then only logs a restart that is back within 10 s.
      if ! mark=$("$dest_dir/cpd" watch --mark-restart 2>"$errfile"); then
        fail "cpd watch --mark-restart: $mark $(cat "$errfile"); the engine restart is not marked as planned"
      fi
      if ! killed=$(tmx kill-window -t "=$work:watch" 2>&1); then
        fail "tmux kill-window $work:watch failed: $killed; the engine was not stopped and still runs the old binary"
      else
        # The killed engine releases its lock a moment later, possibly well after
        # (it finishes its tick, waits for a store write): wait for the lock
        # itself, bounded, then start the engine once and check that it holds the
        # lock (hs-n288). Nothing gives up silently.
        lock="${CPD_HOME:-$HOME/.cpd}/locks/watch.lock"
        mkdir -p "$(dirname "$lock")"
        waited=0
        while ! lock_free "$lock"; do
          waited=$((waited + 1))
          if [ "$waited" -gt $((lock_wait_secs * 4)) ]; then
            break
          fi
          sleep 0.25
        done
        if ! lock_free "$lock"; then
          fail "the old engine still holds $lock after ${lock_wait_secs}s: it was told to stop and has not; the new engine was not started"
        elif ! out=$("$dest_dir/cpd" watch --ensure 2>"$errfile"); then
          fail "cpd watch --ensure: $(describe "$out") $(cat "$errfile"); the old engine was stopped and no engine is running: run cpd watch --ensure, the server's hooks also retry it every few seconds"
        elif lock_free "$lock"; then
          fail "cpd watch --ensure answered $out, but no engine holds $lock: the old engine was stopped and no engine is running"
        else
          echo "$out" >&2
        fi
      fi
    fi
  fi
fi
# Last step: tag the version. It never fails the install; the next install retries.
if ! "$root/scripts/tag-version.sh"; then
  echo "the next install retries the tag" >&2
fi
[ "$failed" -eq 0 ] || exit 1
