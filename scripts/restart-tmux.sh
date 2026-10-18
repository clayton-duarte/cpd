#!/bin/sh
# Restarts the cpd tmux server on the installed tmux, once, after a tmux
# upgrade: a running server keeps the tmux it was started with. Run it by hand
# at a quiet moment, from a terminal outside that server (another terminal or
# ssh). Never called by cpd or the dotfiles. Safe to rerun: when the server
# already runs the installed version it does nothing (--force restarts anyway).
#
# It refuses, touching nothing, while an Attempt is starting, running or
# merging (its Worker lives in the server), and when run from inside the
# server. Otherwise: kill-server, `cpd tui` (rebuilds the cockpit and starts
# the engine), then it prints how to resume each Lead that had a pane.
# Needs cpd and jq on PATH.
set -eu

force=0
case ${1:-} in
  '') ;;
  --force) force=1 ;;
  *) echo "usage: scripts/restart-tmux.sh [--force]" >&2; exit 2 ;;
esac

config=$(cpd config)
socket=$(printf '%s' "$config" | jq -er '.config.tmux.socket')
cockpit=$(printf '%s' "$config" | jq -er '.config.tmux.cockpit_session')

installed=$(tmux -V)
installed=${installed#tmux }

if ! running=$(tmux -L "$socket" display -p '#{version}' 2>/dev/null); then
  echo "the tmux server -L $socket is not running: starting the cockpit with cpd tui"
  exec cpd tui
fi
echo "running server (-L $socket): tmux $running; installed: tmux $installed"
if [ "$running" = "$installed" ] && [ "$force" = 0 ]; then
  echo "the server already runs the installed tmux: nothing to do (--force restarts anyway)"
  exit 0
fi

# An Attempt's Worker lives in the server: restarting it would kill the Worker.
busy=
for plan in $(cpd plan list | jq -r '.plans[].id'); do
  found=$(cpd attempt list --plan "$plan" --active |
    jq -r --arg plan "$plan" '.attempts[] | select(.status == "starting" or .status == "running" or .status == "merging") | "  \($plan)/\(.id) (\(.status))"')
  [ -z "$found" ] || busy="$busy$found
"
done
if [ -n "$busy" ]; then
  echo "refused: these Attempts live in the server -L $socket; wait until they end, then run this again:" >&2
  printf '%s' "$busy" >&2
  exit 1
fi

# Killing the server from one of its own panes would kill this terminal.
case ${TMUX:-} in
  */"$socket",*)
    echo "refused: this terminal is inside the server -L $socket; run this from another terminal or over ssh." >&2
    exit 1
    ;;
esac

leads=$(cpd session list --role lead | jq -r '.sessions[] | select(.tmux_pane != null) | .id')
if [ -n "$leads" ]; then
  echo "Lead Sessions with a pane in the server, stopped by the restart:"
  for id in $leads; do echo "  $id"; done
fi

tmux -L "$socket" kill-server
cpd tui

for id in $leads; do echo "resume: cpd session open $id"; done
echo "attach: tmux -L $socket attach -t $cockpit"
