#!/bin/sh
# Previews a worktree's build in the running cockpit's views only, and puts the
# installed cpd back. Nothing is installed: the engine, the Leads and the
# Workers stay on the installed cpd; only the view panes of <cockpit>:main
# (commands.md §13) run the preview binary, which only reads the store.
#
#   sh scripts/preview-tui.sh on [<worktree>]   build, then swap the views
#   sh scripts/preview-tui.sh off               installed cpd in the views again
#
# While a preview is on, the tmux server option @cpd_preview holds '<worktree> <sha>'.
# The preview lives in ${CPD_PREVIEW_DIR:-~/.cache/cpd-preview}. Safe to rerun.
set -eu

self=$(cd "$(dirname "$0")" && pwd)/$(basename "$0")
dir="${CPD_PREVIEW_DIR:-${HOME:?HOME is not set}/.cache/cpd-preview}"
cpd_home="${CPD_HOME:-${HOME:?HOME is not set}/.cpd}"

usage() {
  echo "usage: sh $0 on [<worktree>] | off" >&2
  exit 2
}

tmux_cmd() {
  env -u TMUX -u TMUX_PANE tmux -L "$socket" -f /dev/null "$@"
}

# Reads the socket and the cockpit session from `cpd config` (no jq: this must
# run on macOS too) and exits 1 unless the cockpit has its main window.
require_cockpit() {
  config=$(cpd config) || { echo "preview: cpd config failed" >&2; exit 1; }
  tmux_conf=$(printf '%s' "$config" | sed -n 's/.*"tmux":{\([^}]*\)}.*/\1/p')
  socket=$(printf '%s' "$tmux_conf" | sed -n 's/.*"socket":"\([^"]*\)".*/\1/p')
  cockpit=$(printf '%s' "$tmux_conf" | sed -n 's/.*"cockpit_session":"\([^"]*\)".*/\1/p')
  if [ -z "$socket" ] || [ -z "$cockpit" ] ||
    ! tmux_cmd list-windows -t "=$cockpit" -F '#{window_name}' 2>/dev/null | grep -qx main; then
    echo "preview: no cockpit (no tmux server or no main window); start it with: cpd tui" >&2
    exit 1
  fi
}

# respawn_views <cpd binary>: restarts each live view pane of main running
# `<binary> tui view <view>`, always with an explicit command. Lead panes
# (@cpd_session) and worker panes stay.
respawn_views() {
  bin=$1
  panes=$(tmux_cmd list-panes -t "=$cockpit:main" -F '#{pane_id}|#{pane_dead}|#{@cpd_view}|#{@cpd_session}')
  worker_open=0
  while IFS='|' read -r pane dead view session; do
    { [ "$dead" = 0 ] && [ -z "$session" ]; } || continue
    case $view in
      worker) worker_open=1 ;;
      tree | plan | widgets | statusbar | chat)
        tmux_cmd respawn-pane -k -t "$pane" -c "$cpd_home" -e CPD_HOME="$cpd_home" -- "$bin" tui view "$view"
        ;;
    esac
  done <<PANES_END
$panes
PANES_END
  if [ "$worker_open" = 1 ]; then
    echo "a Worker view pane is open; left alone" >&2
  fi
}

on() {
  worktree=$(cd "${1:-$(dirname "$0")/..}" && pwd) || { echo "preview: no such worktree: ${1:-}" >&2; exit 1; }

  mkdir -p "$dir"
  CARGO_TARGET_DIR="$dir/target" "$worktree/scripts/cargo.sh" build --release --locked --manifest-path "$worktree/Cargo.toml" -p cpd
  tmp="$dir/.cpd.tmp.$$"
  trap 'rm -f "$tmp"' EXIT
  cp "$dir/target/release/cpd" "$tmp"
  chmod 755 "$tmp"
  mv -f "$tmp" "$dir/cpd"
  trap - EXIT

  # Refuse, changing nothing in tmux, a build that cannot read the store.
  out=$("$dir/cpd" plan list 2>&1) || true
  case $out in
    *'"ok":true'*) ;;
    *)
      echo "preview refused: $dir/cpd cannot read the store:" >&2
      echo "$out" >&2
      case $out in *store_too_new*) echo "merge main into the branch first." >&2 ;; esac
      exit 1
      ;;
  esac

  require_cockpit

  # The engine on the installed binary first; the preview's tui then finds it.
  cpd watch --ensure >&2
  "$dir/cpd" tui >&2
  respawn_views "$dir/cpd"

  sha=$(git -C "$worktree" rev-parse --short HEAD)
  # The status line shows it while set (commands.md §13.2); `cpd tui --respawn` warns.
  tmux_cmd set -g @cpd_preview "$(basename "$worktree") $sha"
  printf '%s\n%s\n' "$worktree" "$sha" >"$dir/state"
  echo "previewing $worktree @ $sha; revert with: sh $self off" >&2
  echo "a reinstall's cpd tui --respawn may keep the preview in the views; run off to be sure." >&2
}

off() {
  require_cockpit
  respawn_views cpd
  tmux_cmd set -gu @cpd_preview
  cpd tui >&2
  rm -f "$dir/state"
  echo "installed cpd is back in the views" >&2
}

case ${1:-} in
  on) [ $# -le 2 ] || usage; on "${2:-}" ;;
  off) [ $# -eq 1 ] || usage; off ;;
  *) usage ;;
esac
