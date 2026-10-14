# cpd-tui

The cpd cockpit: ratatui views (`context`, `plan`, `dispatch`, `worker <session.jsonl> <job>`) that each run in a pane of a
tmux session. tmux draws the border and the one header of each pane (`scripts/tmux.conf`, `pane-border-format`); the
views draw only content.

## Build

    cd tui && ~/.cargo/bin/cargo build --release   # binary: target/release/cpd-tui
    ~/.cargo/bin/cargo test

## Start the cockpit

    tui/scripts/cockpit.sh

It builds the tmux server `tmux -L cpd` (session `cpd`) unless it runs, resuming each Lead's pi in a chat pane through
`resume-lead.sh`, and prints the attach command. Nothing lists the sessions: the views discover them.

| Variable | Meaning |
| --- | --- |
| `CPD_TUI_SOCKET`, `CPD_TUI_SESSION` | the tmux server and session |
| `CPD_TUI_STATE` | state dir (`/tmp/cpd-tui`): cache |
| `CPD_HOME` | cpd's home (default `~/.cpd`): `leads/<sessionId>/workflow` attaches a session to a Workflow |

## Sessions

A chat pane publishes its session in tmux options: `@session_id`, `@session_file`, `@session_name`
(`tmux-agent-state.ts` at `session_start` and on `session_info_changed`; `resume-lead.sh` and `fork-lead.sh` before pi
starts, the name then coming from the file's last `session_info`). The Context view lists every pane with a `@session_id`,
grouped under its Context (the repo directory name of the pane's cwd), labelled by the pi name, else the Workflow id,
else the first 8 characters of the session id. The plan view shows the Workflow of the session in the centre slot
(`no Workflow yet` when unattached); its tabs are that Context's sessions. Switching swaps the session's pane into the
centre slot.

Do not run `cockpit.sh` while a Lead's pi runs elsewhere on the same session file.

## Checking views without the live cockpit

Start a scratch server (`tmux -L cpd-tui-check -f scripts/tmux.conf`) with `CPD_TUI_SOCKET=cpd-tui-check`,
`CPD_TUI_SESSION=check` and `CPD_TUI_STATE=/tmp/cpd-tui-check`; use `sleep 3600` panes with `@session_id`, `@session_name` and
`@agent_state` set by hand (and `CPD_HOME` pointing at a scratch dir with `leads/<id>/workflow`) as stand-in chats.
