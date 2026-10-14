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
grouped under its Context (the repo directory name of the session's own project directory: the `cwd` of its session
file's header line, read once per `@session_file`, else the pane's cwd when the file is missing or has no `cwd`), labelled by the pi name, else the Workflow id,
else the first 8 characters of the session id. The plan view shows the Workflow of the session in the centre slot
(`no Workflow yet` when unattached); its tabs are that Context's sessions. Switching swaps the session's pane into the
centre slot.

## Session actions

Keys on a session row of the Context view; each opens one inline prompt at the bottom of the pane (`src/prompt.rs`), and
clicks are ignored while it is open.

| Key | Action |
| --- | --- |
| `n` | Fork the session into a new Plan. Asks for the name (Enter starts, Esc cancels, an empty name is refused), then opens the hidden window `park-<name>` (name made safe for tmux, in the source's cwd) running `scripts/fork-lead.sh <source @session_file> <new uuid> <name>` with no note file, so pi starts the fork with no first message. The fork waits for the source's turn to end; it shows in the tree at once, labelled by its name; the centre does not switch. On a Context row: "pick the session to fork". |
| `x` | Close the session's chat pane after `Close <label>? pi stops; the session file stays (y/N)` (`<label> is working.` first when its `@agent_state` is `working`). Only `y` closes: `tmux kill-pane` ends pi, the session file stays so it can be resumed, and a parked window that held only that pane closes with it. The session in the centre slot is refused ("switch to another session first"), also when it was swapped there while the prompt was open. On a Context row: nothing. |

The scripts directory is found from the binary's own location, as `cockpit.sh` finds the binary: `<binary>/../../../scripts`
(the binary is `tui/target/<profile>/cpd-tui`, the scripts are `tui/scripts`).

Do not run `cockpit.sh` while a Lead's pi runs elsewhere on the same session file.

## Plan view

The Plan view draws the Workflow as a vertical flowchart (`src/flow.rs`, a pure layout the view blits into the frame):
one-row pills ` <icon> <key> <tier>[ aN] ` in black on the status colour, two rows per level (pills, then edges in dim
box-drawing). A Job sits one level below the deepest Job it needs; a level is ordered by its parents' positions. An edge
that skips levels runs in a lane to the right of every pill. Keys are truncated with `…` (down to 6 characters) when a
level is too wide; beyond that the chart scrolls horizontally. Cancelled Jobs are hidden (`N cancelled hidden` in the
header line).

| Input | Does |
| --- | --- |
| ↑ ↓ / k j | select the nearest Job on the level above / below |
| ← → / h l | select the previous / next Job on the level |
| mouse wheel | scroll vertically (wheel left/right: horizontally) |
| Enter, click on a pill | open the Worker's read-only chat for that Job |

The selected pill is bold, between `▸ ◂`. The view scrolls to keep the selection visible when it moves by key; the
refresh keeps the scroll offset and holds the selection by Job key.

## Dispatch: PR status

The `dispatch` view lists the PR of each Plan in the cockpit (the Workflows of the chat sessions, as the Context view
finds them), under the homeserver load: the Plan's label, `#<number>`, the state (open, merged, closed), the checks
(✓ all passed, ✗ any failed, … pending, – none), the review (approved, changes requested, no review) and the PR title,
cut first when the column is narrow. A Plan with `pr: null` shows `no PR` (dropped when the pane has no room).

**Interim source.** cpd records only `workflow.pr = {number, url, openedAt}` (from the cached `cpd workflow show`), so
the live state comes from the read-only `gh pr view <number> --json state,statusCheckRollup,reviewDecision,mergeable,url,title`,
run in `workflow.repo`. This stays until cpd reports PR state, which is the cpd-rust Lead's data contract.

`gh` runs in a thread of the view, never in the draw loop, at most once per 60 s per PR, and its answer is cached in
`$CPD_TUI_STATE/cache/pr-<workflow>.json`. When a call fails, the last known state shows dimmed with its age.

## Checking views without the live cockpit

Start a scratch server (`tmux -L cpd-tui-check -f scripts/tmux.conf`) with `CPD_TUI_SOCKET=cpd-tui-check`,
`CPD_TUI_SESSION=check` and `CPD_TUI_STATE=/tmp/cpd-tui-check`; use `sleep 3600` panes with `@session_id`, `@session_name` and
`@agent_state` set by hand (and `CPD_HOME` pointing at a scratch dir with `leads/<id>/workflow`) as stand-in chats. To check `n`, put a fake `pi` (a script that records its arguments and sleeps) first on the scratch
server's `PATH` and give the source a scratch session file; to check `x`, use `sleep 3600` stand-in panes.
