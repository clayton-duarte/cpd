#!/bin/sh
# Previews a worktree's web UI: the Vite dev server of the worktree runs in the
# `web` window of the work session (localhost:5173, strictPort). Nothing is
# installed. Electron is never started or killed here; only the preview watchdog
# (scripts/preview-watchdog.mjs, a frozen copy outside the worktree) is started and stopped.
#
#   sh scripts/preview-web.sh on [<worktree>]   dev server from the worktree
#   sh scripts/preview-web.sh off               dev server on the primary checkout again, watchdog stopped
#   sh scripts/preview-web.sh reload [<worktree>]  typecheck, build:electron, then SIGUSR1 to the Electron of the health file
#   sh scripts/preview-web.sh status            prints '<worktree> <sha>' or 'no web preview'
#   sh scripts/preview-web.sh serve [<worktree>]   the worktree + deps + Vite steps of `on`, no watchdog, no printed command
#   sh scripts/preview-web.sh unserve            stops only that dev server (the `web` window)
#
# The default worktree is ${CPD_WEB_PREVIEW_WORKTREE:-<repo>/../cpd.preview.web}, created
# from origin/main on branch preview/web when missing, and never touched when present.
# `on` also installs the frozen watchdog into ${CPD_PREVIEW_DIR}/watchdog and starts it (pid in
# ${CPD_PREVIEW_DIR}/watchdog.pid, log in ${CPD_HOME:-~/.cpd}/run/preview-watchdog.log).
# The watchdog relaunches Electron with CPD_ELECTRON_DEV_URL=http://localhost:5173/electron.html (Vite HMR);
# when that URL fails to load, Electron loads the built renderer.
# `on` builds the Electron bundle when web/dist-electron/main.js is missing. While a preview is on, the
# tmux server option @cpd_web_preview holds '<name> <sha>'. State: ${CPD_PREVIEW_DIR:-~/.cache/cpd-preview}/web-state.
# Safe to rerun.
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd -P)
dir="${CPD_PREVIEW_DIR:-${HOME:?HOME is not set}/.cache/cpd-preview}"
state="$dir/web-state"
cpd_home="${CPD_HOME:-${HOME:?HOME is not set}/.cpd}"
wd_dir="$dir/watchdog"
wd_pid="$dir/watchdog.pid"
dev_url="http://localhost:5173/electron.html"

usage() {
  echo "usage: sh $0 on [<worktree>] | off | status | reload [<worktree>] | serve [<worktree>] | unserve" >&2
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

# serve <mode> <checkout>: the dev server of <checkout> in the web window, without the session identity.
# 'on' exports CPD_MODE=preview and CPD_CHECKOUT=<checkout>; 'off' sets CPD_MODE=production and removes CPD_CHECKOUT.
serve() {
  mode=$1
  checkout=$2
  if [ "$mode" = on ]; then
    set -- env CPD_MODE=preview "CPD_CHECKOUT=$checkout"
  else
    set -- env CPD_MODE=production env -u CPD_CHECKOUT
  fi
  if tmux_cmd list-windows -t "=$work" -F '#{window_name}' 2>/dev/null | grep -qx web; then
    tmux_cmd respawn-pane -k -t "=$work:web" -c "$checkout" -- "$@" \
      env -u CPD_SESSION -u PI_SESSION_ID -u CPD_SESSION_ID -u CPD_ATTEMPT_ID pnpm --dir "$checkout" --filter cpd-web-client run dev --host localhost
  else
    tmux_cmd new-window -d -n web -c "$checkout" -t "=$work:" -- "$@" \
      env -u CPD_SESSION -u PI_SESSION_ID -u CPD_SESSION_ID -u CPD_ATTEMPT_ID pnpm --dir "$checkout" --filter cpd-web-client run dev --host localhost
  fi
}

# stop_watchdog: SIGTERM to the pid of the pid file when alive; the file is removed either way.
stop_watchdog() {
  [ -f "$wd_pid" ] || return 0
  wpid=$(sed -n '1p' "$wd_pid")
  case $wpid in
    ''|*[!0-9]*) ;;
    *) if env kill -0 "$wpid" 2>/dev/null; then env kill -TERM "$wpid" 2>/dev/null || true; fi ;;
  esac
  rm -f "$wd_pid"
}

# start_watchdog <worktree>: installs the frozen copy and starts the watchdog detached.
start_watchdog() {
  stop_watchdog
  mkdir -p "$wd_dir" "$cpd_home/run"
  cp "$repo/scripts/preview-watchdog.mjs" "$repo/web/src/electron/health.ts" \
    "$repo/web/src/electron/watchdog.ts" "$repo/web/src/electron/restore-points.ts" "$wd_dir/"
  CPD_MODE=preview CPD_CHECKOUT="$1" CPD_ELECTRON_DEV_URL="$dev_url" nohup node "$wd_dir/preview-watchdog.mjs" --worktree "$1" --home "$cpd_home" \
    --relaunch pnpm --dir "$1/web" exec electron . >>"$cpd_home/run/preview-watchdog.log" 2>&1 </dev/null &
  echo "$!" >"$wd_pid"
}

reload() {
  worktree=${1:-}
  if [ -z "$worktree" ] && [ -f "$state" ]; then worktree=$(sed -n '1p' "$state"); fi
  worktree=${worktree:-${CPD_WEB_PREVIEW_WORKTREE:-$repo/../cpd.preview.web}}
  if ! checked=$(pnpm --dir "$worktree" --filter cpd-web-client run typecheck 2>&1); then
    printf '%s\n' "$checked" >&2
    echo "preview: typecheck failed; nothing built, nothing signalled" >&2
    exit 1
  fi
  pnpm --dir "$worktree" --filter cpd-web-client run build:electron >&2
  health="$cpd_home/run/preview-health.json"
  rpid=
  [ ! -f "$health" ] || rpid=$(sed -n 's/.*"pid":\([0-9][0-9]*\).*/\1/p' "$health" | sed -n '1p')
  if [ -z "$rpid" ]; then
    echo "preview: no pid in $health; is the Electron preview running?" >&2
    exit 1
  fi
  if ! env kill -0 "$rpid" 2>/dev/null; then
    echo "preview: Electron pid $rpid is not running" >&2
    exit 1
  fi
  env kill -USR1 "$rpid"
  echo "reloaded Electron pid $rpid from $worktree" >&2
}

on() {
  worktree=${1:-${CPD_WEB_PREVIEW_WORKTREE:-$repo/../cpd.preview.web}}
  read_config
  if [ ! -d "$worktree" ]; then
    git -C "$repo" fetch origin main >&2
    git -C "$repo" worktree add -B preview/web "$worktree" origin/main >&2
    pnpm --dir "$worktree" install --frozen-lockfile >&2
  fi
  if [ ! -f "$worktree/web/dist-electron/main.js" ]; then
    pnpm --dir "$worktree" --filter cpd-web-client run build:electron >&2
  fi

  serve on "$worktree"
  sha=$(git -C "$worktree" rev-parse --short HEAD)
  tmux_cmd set -g @cpd_web_preview "$(basename "$worktree") $sha"
  mkdir -p "$dir"
  printf '%s\n%s\n' "$worktree" "$sha" >"$state"
  start_watchdog "$worktree"
  echo "previewing $worktree @ $sha at http://localhost:5173; revert with: sh $0 off" >&2
  echo "Electron on it: CPD_MODE=preview CPD_CHECKOUT=$worktree CPD_ELECTRON_DEV_URL=$dev_url pnpm --dir $worktree/web run electron" >&2
}

off() {
  read_config
  serve off "$repo"
  tmux_cmd set -gu @cpd_web_preview
  rm -f "$state"
  stop_watchdog
  echo "dev server is back on $repo" >&2
}

# serve_only [<worktree>]: the worktree + deps + Vite steps of `on`, without the watchdog,
# the printed Electron command, `web-state` or `@cpd_web_preview`. Idempotent; used by the
# in-place WebUI Preview client on switching to the preview.
serve_only() {
  worktree=${1:-${CPD_WEB_PREVIEW_WORKTREE:-$repo/../cpd.preview.web}}
  read_config
  if [ ! -d "$worktree" ]; then
    git -C "$repo" fetch origin main >&2
    git -C "$repo" worktree add -B preview/web "$worktree" origin/main >&2
    pnpm --dir "$worktree" install --frozen-lockfile >&2
  fi
  if [ ! -f "$worktree/web/dist-electron/main.js" ]; then
    pnpm --dir "$worktree" --filter cpd-web-client run build:electron >&2
  fi
  serve on "$worktree"
}

# unserve: stops only the dev server of `serve_only` (the `web` window). It touches neither the
# watchdog, `web-state` nor `@cpd_web_preview`; safe to rerun.
unserve() {
  read_config
  if tmux_cmd list-windows -t "=$work" -F '#{window_name}' 2>/dev/null | grep -qx web; then
    tmux_cmd kill-window -t "=$work:web"
  fi
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
  reload) [ $# -le 2 ] || usage; reload "${2:-}" ;;
  serve) [ $# -le 2 ] || usage; serve_only "${2:-}" ;;
  unserve) [ $# -eq 1 ] || usage; unserve ;;
  *) usage ;;
esac
