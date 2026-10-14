# Sourced by the launch scripts. Publish a session's identity into this tmux pane, as tmux-agent-state.ts does once pi
# runs: @session_id, @session_file, @session_name. For a session that started before the extension, the name is the
# last session_info entry of its file. Usage: publish_session <session file> (needs $TMUX_PANE).
publish_session() {
  file=$1
  id=$(head -1 "$file" | jq -r '.id // empty' 2>/dev/null)
  name=$(tac "$file" | jq -r 'select(.type=="session_info") | .name // ""' 2>/dev/null | head -1)
  tmux -L "${CPD_TUI_SOCKET:-cpd}" set -p -t "$TMUX_PANE" @session_id "$id" \; set -p -t "$TMUX_PANE" @session_file "$file" \; set -p -t "$TMUX_PANE" @session_name "$name" 2>/dev/null
}
