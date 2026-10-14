# cpd-tui

The cpd cockpit: ratatui views (`tree`, `plan`, `widgets`, `log <session.jsonl> <job>`) that each run in a pane of a
tmux session. tmux draws the border and the one header of each pane (`scripts/tmux.conf`, `pane-border-format`); the
views draw only content.

## Build

    cd tui && ~/.cargo/bin/cargo build --release   # binary: target/release/cpd-tui
    ~/.cargo/bin/cargo test

## Start the cockpit

    tui/scripts/cockpit.sh

It builds the tmux server `tmux -L cpd` (session `cpd`) unless it runs, resuming each Lead's pi in a chat pane through
`resume-lead.sh`, and prints the attach command. It exports the configuration the views read:

| Variable | Meaning |
| --- | --- |
| `CPD_TUI_CONTEXT` | the Context's name (`cpd`) |
| `CPD_TUI_PLANS` | the Plans, space-separated workflow ids (`v1 lead-fork`) |
| `CPD_TUI_SOCKET`, `CPD_TUI_SESSION` | the tmux server and session |
| `CPD_TUI_STATE` | state dir (`/tmp/cpd-tui`): active Plan, cache |

With `CPD_TUI_CONTEXT` or `CPD_TUI_PLANS` unset, a view shows an error line naming it.

Do not run `cockpit.sh` while a Lead's pi runs elsewhere on the same session file.

## Checking views without the live cockpit

Start a scratch server (`tmux -L cpd-tui-check -f scripts/tmux.conf`) with `CPD_TUI_SOCKET=cpd-tui-check`,
`CPD_TUI_SESSION=check` and `CPD_TUI_STATE=/tmp/cpd-tui-check`; use `sleep 3600` panes with `@plan` and
`@agent_state` set by hand as stand-in chats.
