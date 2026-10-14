#!/bin/sh
# Resume a Lead's pi session in this cockpit pane, after it has quit in its herdr pane, and keep the slot alive.
# Two pi processes on one session file would corrupt it, so this waits while pi still runs there.
# HERDR_ENV=1 switches on cpd's pi adapter, so the Lead keeps receiving cpd messages (job-merged,
# escalations, plan reviews); herdr-agent-state stays off without a herdr socket and pane.
# tmux-agent-state.ts publishes working/blocked/idle and the session's identity into this pane for the session tree;
# until pi starts (and for a pi that predates it) this script publishes the identity from the session file.
set -u
plan=$1 session=$2 herdr_pane=$3
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/publish-session.sh"
state() { tmux -L "${CPD_TUI_SOCKET:-cpd}" set -p -t "$TMUX_PANE" @agent_state "$1" 2>/dev/null; }
live() {
  herdr pane process-info --pane "$herdr_pane" 2>/dev/null \
    | jq -e '[.result.process_info.foreground_processes[]? | select(.name == "pi")] | length > 0' >/dev/null
}
publish_session "$session"
while :; do
  if live; then
    state in-herdr
    printf '\n  The Lead of %s still runs in herdr pane %s.\n\n' "$plan" "$herdr_pane"
    printf '  Quit pi there (/quit, or Ctrl+C twice) and LEAVE THAT PANE OPEN:\n'
    printf '  cpd still opens Reviewers and the watcher from it, inside herdr.\n\n'
    printf '  This pane resumes the session as soon as pi has quit there.\n'
    while live; do sleep 2; done
  fi
  state starting
  env HERDR_ENV=1 pi -e "$DIR/tmux-agent-state.ts" --session "$session"
  state stopped
  printf '\n  pi exited. Press Enter to resume the %s Lead here.\n' "$plan"
  read -r _ || exit 0
done
