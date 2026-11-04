#!/bin/sh
# Previews a worktree's web UI: the Vite dev server of the worktree runs in the
# `web` window of the work session (localhost:5173, strictPort). Nothing is
# installed; Electron is never started or killed here.
#
#   sh scripts/preview-web.sh on [<worktree>]   dev server from the worktree
#   sh scripts/preview-web.sh off               dev server on the primary checkout again
#   sh scripts/preview-web.sh status            prints '<worktree> <sha>' or 'no web preview'
#
# The default worktree is ${CPD_WEB_PREVIEW_WORKTREE:-<repo>/../cpd.preview.web}, created
# from origin/main on branch preview/web when missing, and never touched when present.
# CPD_PREVIEW_ELECTRON=1 also builds the Electron bundle. While a preview is on, the
# tmux server option @cpd_web_preview holds '<name> <sha>'. State: ${CPD_PREVIEW_DIR:-~/.cache/cpd-preview}/web-state.
# Safe to rerun.
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd -P)
dir="${CPD_PREVIEW_DIR:-${HOME:?HOME is not set}/.cache/cpd-preview}"
state="$dir/web-state"

usage() {
  echo "usage: sh $0 on [<worktree>] | off | status" >&2
  exit 2
}

tmux_cmd() {
  env -u TMUX -u TMUX_PANE tmux -u -L "$socket" -f /dev/null "$@"
}

# Reads the socket and the work session from `cpd config` (no jq: this must run on macOS too).
read_config() {
  config=$(cpd config) || { echo "preview: cpd config failed" >&2; exit 1; }
  tmux_conf=$(printf '%s' "$config" | sed -n 's/.*"tmux":{\([^}]*\)}.*/\1/p')
  socket=$(printf '%s' "$tmux_conf" | sed -n 's/.*"socket":"\([^"]*\)".*/\1/p')
  work=$(printf '%s' "$tmux_conf" | sed -n 's/.*"work_session":"\([^"]*\)".*/\1/p')
  if [ -z "$socket" ] || [ -z "$work" ]; then
    echo "preview: no tmux socket or work session in cpd config" >&2
    exit 1
  fi
}

# serve <checkout>: the dev server of <checkout> in the web window, without the session identity.
serve() {
  if tmux_cmd list-windows -t "=$work" -F '#{window_name}' 2>/dev/null | grep -qx web; then
    tmux_cmd respawn-pane -k -t "=$work:web" -c "$1" -- \
      env -u CPD_SESSION -u PI_SESSION_ID -u CPD_SESSION_ID -u CPD_ATTEMPT_ID pnpm --dir "$1" --filter cpd-web-client run dev --host localhost
  else
    tmux_cmd new-window -d -n web -c "$1" -t "=$work:" -- \
      env -u CPD_SESSION -u PI_SESSION_ID -u CPD_SESSION_ID -u CPD_ATTEMPT_ID pnpm --dir "$1" --filter cpd-web-client run dev --host localhost
  fi
}

on() {
  worktree=${1:-${CPD_WEB_PREVIEW_WORKTREE:-$repo/../cpd.preview.web}}
  read_config
  if [ ! -d "$worktree" ]; then
    git -C "$repo" fetch origin main >&2
    git -C "$repo" worktree add -B preview/web "$worktree" origin/main >&2
    pnpm --dir "$worktree" install --frozen-lockfile >&2
  fi
  if [ "${CPD_PREVIEW_ELECTRON:-}" = 1 ]; then
    pnpm --dir "$worktree" --filter cpd-web-client run build:electron >&2
  fi

  serve "$worktree"
  sha=$(git -C "$worktree" rev-parse --short HEAD)
  tmux_cmd set -g @cpd_web_preview "$(basename "$worktree") $sha"
  mkdir -p "$dir"
  printf '%s\n%s\n' "$worktree" "$sha" >"$state"
  echo "previewing $worktree @ $sha at http://localhost:5173; revert with: sh $0 off" >&2
  echo "Electron on it: CPD_ELECTRON_DEV_URL=http://localhost:5173 pnpm --dir $worktree/web run electron" >&2
}

off() {
  read_config
  serve "$repo"
  tmux_cmd set -gu @cpd_web_preview
  rm -f "$state"
  echo "dev server is back on $repo" >&2
}

status() {
  if [ -f "$state" ]; then
    paste -sd' ' "$state"
  else
    echo "no web preview"
  fi
}

case ${1:-} in
  on) [ $# -le 2 ] || usage; on "${2:-}" ;;
  off) [ $# -eq 1 ] || usage; off ;;
  status) [ $# -eq 1 ] || usage; status ;;
  *) usage ;;
esac
