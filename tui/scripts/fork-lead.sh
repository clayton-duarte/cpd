#!/bin/sh
# Start a fork of a Lead session in this cockpit pane. It waits until the source session is idle (its last message
# is a finished assistant turn and the file has been quiet for 5 s), so the fork never copies a half-done turn.
# Later runs resume the fork by its id instead of forking again. The pane publishes the fork's identity (@session_id,
# @session_name, and @session_file once the file exists); tmux-agent-state.ts keeps it current after that.
# Usage: fork-lead.sh <source.jsonl> <id> <name> [note file]   (the note is the fork's first message; without it pi starts the fork with none)
set -u
src=$1 id=$2 name=$3 note=${4:-}
DIR=$(cd "$(dirname "$0")" && pwd)
state() { tmux -L "${CPD_TUI_SOCKET:-cpd}" set -p -t "$TMUX_PANE" @agent_state "$1" 2>/dev/null; }
last() { tac "$src" | jq -r 'select(.type=="message") | .message.role + ":" + (.message.stopReason // "")' 2>/dev/null | head -1; }
quiet() { [ $(( $(date +%s) - $(stat -c %Y "$src") )) -ge 5 ]; }
. "$DIR/publish-session.sh"
file() { ls "$HOME"/.pi/agent/sessions/*/*_"$id".jsonl 2>/dev/null | head -1; }
if [ -n "$(file)" ]; then
  publish_session "$(file)"   # an existing fork: its file knows its current (maybe renamed) name
  env HERDR_ENV=1 pi -e "$DIR/tmux-agent-state.ts" --session "$id"
else
  tmux -L "${CPD_TUI_SOCKET:-cpd}" set -p -t "$TMUX_PANE" @session_id "$id" \; set -p -t "$TMUX_PANE" @session_name "$name" 2>/dev/null
  state starting
  printf '\n  Waiting for the source Lead to finish its turn, then forking it as "%s"…\n' "$name"
  until [ "$(last)" = "assistant:stop" ] && quiet; do sleep 2; done
  if [ -n "$note" ]; then
    env HERDR_ENV=1 pi -e "$DIR/tmux-agent-state.ts" --fork "$src" --session-id "$id" -n "$name" -- "$(cat "$note")"
  else
    env HERDR_ENV=1 pi -e "$DIR/tmux-agent-state.ts" --fork "$src" --session-id "$id" -n "$name"
  fi
fi
while :; do
  state stopped
  printf '\n  pi exited. Press Enter to resume %s here.\n' "$name"
  read -r _ || exit 0
  env HERDR_ENV=1 pi -e "$DIR/tmux-agent-state.ts" --session "$id"
done
