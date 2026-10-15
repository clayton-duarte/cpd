# cpd-tui

The cpd cockpit: ratatui views (`context`, `plan`, `dispatch`, `statusbar`, `worker <session.jsonl> <job>`) that each run in a pane of a
tmux session (`legend` runs in a popup). tmux draws the border and the one header of each pane (`scripts/tmux.conf`, `pane-border-format`); the
views draw only content.

## Conventions

Views use ratatui's widgets and layout as much as possible: `Layout`/`Constraint`, `Table`/`List` with their state for
selection and scrolling, `Paragraph`, `Block`, `Scrollbar`, `Line`/`Span` alignment, and `tui-tree-widget` for trees. Hand-written
drawing is only for what has no widget, with a comment saying why: the chart's line glyphs and its window copy, and the Context column's counters
layer (a right-aligned `Line` per tree row, since the tree widget draws one text per row).

**One separator.** `src/separator.rs` is the only line that separates areas: a one-row `Block` with `Borders::TOP`, in the
palette's dim "nothing" colour. The Plan view's tabs and the Dispatch sections (one between each pair of
homeserver, PRs, git and Jira, laid out with `Layout`) all use it.

## Palette and legend

`src/palette.rs` is the only place with colours and status icons: a `Meaning` (not started white, in progress green, waiting
on review yellow, done dark grey, shared with nothing, failed red, planning magenta, grounding white, nothing dark grey) and the mappings from Job status,
phase, PR state and PR checks to `(icon, Meaning)`. Every view takes both from it, so changing one changes it everywhere;
the test `no_colour_outside_the_palette` fails when another `src/*.rs` names a colour. Grounding shares white with "not
started", and done shares dark grey with "nothing" (the User's choice): the two deliberate exceptions to one colour per meaning.

| Job status | queued | running | merging | merged | failed | draft | unknown |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Icon | `○` | `◐` | `⇥` | `✓` | `✗` | `◌` | `?` |

`●` means only "needs you" (the Context dot); PR checks are `✓ ✗ … –`.

`cpd-tui legend` (`src/legend.rs`) draws the meanings, each Job status as a Plan pill and a Context counter, the Context dots
by phase, the PR states and the checks, built from the palette only. Any key or click closes it. A StatusBar left item
is the ` ? legend ` button (a click; the key `?` in Context does the same): it runs `tmux -L <socket> display-popup -E -T ' legend ' -w <w> -h <h>
'<binary> legend'`, sized to the legend and capped at the client's size, so it floats over the whole cockpit.

## StatusBar

`cpd-tui statusbar` (`src/statusbar.rs`) is a one-row pane across the full width at the bottom of window `main`
(`cockpit.sh`: `split-window -v -f -l 1`, `@view statusbar`; tmux draws its border line above it, with no title). It has two
areas, `left` items from the left edge in order and `right` items ending at the right edge in order, with the middle
expanding (a `Layout` of `Length` items around a `Fill`, spaced by `Layout::spacing`); an item is added with one entry on
either side. `statusbar::draw` returns the `Rect` of each clickable item.

| Side | Item | Does |
| --- | --- | --- |
| left | `⧉ ` (reversed; one cell wide, first) | a click resets the layout |
| left | ` ? legend ` (reversed) | a click opens the legend popup |
| right | `workers N/M`, shortened to `N/M` when narrow | none; re-read every 2 s |

`N` is the live slots (`<cpd home>/slots/slot-<i>.json` whose `pid` is alive), `M` is `concurrency` in
`<cpd home>/config.json`. Interim source, read directly from cpd's home until cpd2 provides the data (`src/workers.rs`).

**The ⧉ button resets the layout.** Nothing re-adjusts the layout on its own: window resizes and border drags stay until the
button is clicked. A click (or `cpd-tui reset-layout`, which `cockpit.sh` runs last, so a new cockpit starts there) runs
`resize-pane` with percentages of the window, in this order (`src/layout.rs`, the one place with the numbers; panes are found by
`@view` in window `main`; the chat takes what is left):

| Step | View | Size |
| --- | --- | --- |
| 1 | `statusbar` | 1 row |
| 2 | `context` | 20% width |
| 3 | `dispatch` | 20% width |
| 4 | `plan` (PlanView) | 20% height |
| 5 | `worker`, only when a Worker view is open | 30% width |

A click on the bar does not take keyboard focus: `tmux.conf` binds root `MouseDown1Pane` to pass the click on with
`send-keys -M` without `select-pane` when the pane's `@view` is `statusbar`; every other pane keeps tmux's default.

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
(`no Workflow yet` when unattached, shown in the chart area); its tabs are that Context's sessions. Switching swaps the
session's pane into the centre slot (`swap-pane -d`) and focuses it (`select-pane`), so typing goes to the chat; a tab click
and a tree click do the same (`switch_to`).

## Context column

The `context` view is a `tui-tree-widget` tree (`src/context.rs`; `TreeState` keeps the open Contexts, the selection, the
scrolling and the clicks):

    ● cpd                         a Context: dot, name (and the number of Plans inside when collapsed)
      ├ ○ v1             36⋅0⋅0⋅1⋅5
    ⏵ ├ ● cpd rust lead   0⋅1⋅0⋅0⋅1
      └ ○ cpd-ui          1⋅0⋅2⋅0⋅3

A session row is guide, dot, name (cut with `…` before the counters), and its Workflow's five Job counters at the right:
queued, running, merging, failed, merged (in phase `plan` the drafts come first). Cancelled Jobs are not counted; a session with no
Workflow shows no counters. Zeros are dark grey, the `⋅` separators grey.

Colours are the palette's: the dot's is the Workflow's phase (none dark grey, context white, plan magenta, dispatch green,
pr yellow, done grey); a counter's is its Job status (queued white, running green, merging yellow, failed red, merged grey;
zeros and the drafts of phase `plan` dark grey).

A Context's dot has the colour of the busiest phase inside (dispatch > plan > context > pr > done > none). 

**Cursor.** Context rows are not selectable; a click on one only collapses or expands it. The keyboard cursor moves between
sessions only (↑↓/jk) and is keyboard-only: a click never moves or shows it. It is drawn inverted across the full row only
while the pane has keyboard focus (`focus-events on` in `tmux.conf`, and the focus events the view receives), and shows
after an arrow key. Enter, `n` and `x` act on it only while it shows. Keys: Enter on the cursor's session switches the
centre to it; a click on a session switches and focuses it; `?` opens the legend; `n` and `x`
below. 

**⏵ marks the centre.** The `⏵` in the dot column marks the session in the centre slot; the tabs and the tree read it
from the same place, so they agree within the 250 ms loop.

**● needs you, ○ does not.** A session needs you when its Lead waits on the User, or finished a turn the User has not seen.
`tmux-agent-state.ts` publishes the pane options for it: `@asking` (1 while a tool named `ask_*` runs, and after a
`cpd_plan_review` or `plannotator` call until the next run starts: `agent_start` fires for the decision message cpd sends with
`triggerTurn`, which fires no `input`; the User typing clears it too) and `@settled_at` (epoch seconds of the last `agent_settled`).
The Context view stamps `@seen_at` on the chat pane in the centre at every refresh and when switching. A session needs you
when `@asking` is 1, or when it is idle and `@settled_at` is after `@seen_at` (never seen counts as 0). A pi that started
before the extension had these options keeps publishing neither, so it shows ○ until it restarts.

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

The Plan view draws the Workflow as GitHub-Actions-style cards, left to right (`src/flow.rs`, a pure layout the view blits
into the frame; the snapshot tests there pin it, the whole `cpd2` chart included, from `tests/fixtures/`):

      ○ a S1 ──╮
               ├── ○ b S1 ──┬── ○ c S1
      ○ x S1 ──┴────────────╯

                 ╭ ○ b S1 ╮
      ○ a S1 ────┤ ○ c S1 ├──── ○ d S1
                 ╰ ○ e S1 ╯

Above, `c` needs `b` and `x`, and the line from `x` skips a column on a free row. Below, `b`, `c` and `e` need `a` and share
a card; `d` needs the card.

- **Implied needs are not drawn:** a need that is also reachable through another need is dropped (transitive reduction).
- **Cards:** Jobs that need exactly the same Jobs share one card, stacked with no space between them; Jobs with no needs are a
  card each; cards in a column have one empty row between them. A card of several Jobs is held by a bracket on each side
  (`╭ ╮ │ ╰ ╯`); a single Job's entry and exit are plain line ends.
- **Columns:** a card's column is one past the deepest column of the cards it needs (cards with no needs are the first
  column). A column is ordered by the mean row of the cards it needs (ties: the Workflow's order) and centred vertically.
- **Pills:** every pill in a column has the column's width. The icon and key are to the left, the tier (and ` aN` from the
  second Attempt on) to the right, black on the status colour; colours and icons are the palette's.
- **Lines:** one entry and one exit per card, at its middle row. There is one vertical bus per gap between columns; every
  exit of a column and every entry of the next join it, and where lines meet they merge into one junction (`┤ ├ ┼ ┬ ┴`). A
  line that skips columns rides a row free of cards, the one nearest its exit row, from its own bus to the bus before its
  target (rows may be shared). Lines are dim, in the palette's "nothing" colour.
- **Padding:** two columns and one row (`Rect::inner(Margin)`) around the chart, so nothing touches the pane's edges; the
  chart scrolls inside it. Cancelled Jobs are hidden.

The top row holds only the Context's name and its tabs (`src/tabs.rs`, one per Lead session), over a separator line (a
`Block` with `Borders::BOTTOM`). The active tab is reversed and is its only marker (no `⏵`); a click on a tab switches to
it and focuses the chat. The Workflow title, the count of hidden Jobs and the refresh age are not drawn; `no Workflow yet`,
`loading…` and the notes show in the chart area.

Selection is per Job. Nothing is selected when the view opens, when a tab switches to another Workflow, or when the selected
Job disappears (cancelled or hidden); the view starts scrolled to the top left. A click on a pill selects it; the first
arrow or `hjkl` key selects the first pill of the first column and moves from there. Enter with nothing selected does nothing.

| Input | Does |
| --- | --- |
| ← → / h l | select the nearest Job (by row) in the previous / next column |
| ↑ ↓ / k j | select the previous / next Job in the same column |
| mouse wheel | scroll vertically (wheel left/right: horizontally) |
| Enter, click on a pill | open the Worker's read-only chat for that Job |
| Esc, click on the selected pill, click on empty chart space | clear the selection and close that Job's Worker view |

The selected pill is drawn bold and reversed. The view scrolls on both axes to keep the selection visible when it moves by
key; the refresh keeps the scroll offset and holds the User's selection by Job key (it never creates one).

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
`@agent_state` set by hand (`@asking`, `@settled_at`, `@seen_at` for the ● dot; and `CPD_HOME` pointing at a scratch dir with `leads/<id>/workflow`) as stand-in chats. To check `n`, put a fake `pi` (a script that records its arguments and sleeps) first on the scratch
server's `PATH` (verify with a probe window running `command -v pi`: windows do not take `PATH` from the server's environment, so point `default-shell` at a wrapper that exports it) and give the source a scratch session file; to check `x`, use `sleep 3600` stand-in panes.
