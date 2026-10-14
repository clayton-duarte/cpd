#!/bin/sh
# Build the cpd cockpit (tmux -L cpd, session cpd) unless it already runs, then print how to attach.
# The views discover the Lead sessions from the chat panes' tmux options; nothing lists them.
set -eu
DIR=$(cd "$(dirname "$0")" && pwd)
EXE="$DIR/../target/release/cpd-tui"
SOCK=cpd SESS=cpd
export CPD_TUI_SOCKET=$SOCK CPD_TUI_SESSION=$SESS CPD_TUI_STATE=/tmp/cpd-tui
PLANS="v1 lead-fork"   # the Leads this script starts; the views learn them from the panes
FIRST=lead-fork        # shown first: this conversation
T="env -u HERDR_ENV -u HERDR_PANE_ID -u HERDR_TAB_ID -u HERDR_WORKSPACE_ID -u HERDR_SOCKET_PATH -u PI_SESSION_FILE -u PI_SESSION_ID -u PI_CODING_AGENT tmux -L $SOCK -f $DIR/tmux.conf"
if $T has-session -t "$SESS" 2>/dev/null; then echo "cockpit already running: tmux -L $SOCK attach -t $SESS"; exit 0; fi
mkdir -p "$CPD_TUI_STATE"
lead() { jq -r ".lead.$2" "$HOME/.cpd/workflows/$1/workflow.json"; }
chat() { echo "$DIR/resume-lead.sh $1 '$(lead "$1" sessionPath)' $(lead "$1" paneId)"; }
$T new-session -d -s "$SESS" -n main -x 215 -y 62 "$EXE context"
LEFT=$($T display -p -t "$SESS:main.0" '#{pane_id}')
CHAT=$($T split-window -h -t "$LEFT" -l 80% -P -F '#{pane_id}' "$(chat $FIRST)")
RIGHT=$($T split-window -h -t "$CHAT" -l 25% -P -F '#{pane_id}' "$EXE dispatch")
PLAN=$($T split-window -v -b -t "$CHAT" -l 20% -P -F '#{pane_id}' "$EXE plan")
$T set -p -t "$LEFT" @view context; $T set -p -t "$PLAN" @view plan; $T set -p -t "$RIGHT" @view dispatch
for p in $PLANS; do
  [ "$p" = "$FIRST" ] && continue
  $T new-window -d -t "$SESS" -n "park-$p" "$(chat "$p")"
done
$T select-pane -t "$CHAT"
echo "cockpit ready: ssh -t homeserver tmux -L $SOCK attach -t $SESS"
