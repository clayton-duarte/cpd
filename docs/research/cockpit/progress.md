# Cockpit research: following progress

Job `insp-progress` (attempt `insp-progress-a1`), Plan `tui-visuals`, PR `inspiration`. Research only: no code, nothing here is decided. It builds on [survey.md](survey.md) (Job `insp-survey`) and goes deeper on one of the User's four pain points (hs-d275), **following progress**: how the best TUIs let you follow many moving things at once, without scrolling or switching.

Tags used in this file (the same as the survey's): **F** finding what needs him, **P** following progress, **N** getting around, **L** looks and polish. Effort is **S** (a day or less, one view), **M** (a few days, one view plus a command or a tmux hook) or **L** (a week or more, or crossing the engine and several views), each with a one-line reason. The Inbox (`docs/contract.md` §2.2, `docs/commands.md` §21), the powerline look (hs-d266) and dynamic titles (hs-d257) are in flight elsewhere and are not redone here.

## 1. Method, and what was verified

- **Reading.** Every tool was read through `gh` (README, docs folder, source files). Screenshots cannot be seen, so every layout is described from README, docs or source; what is **inferred** (from a file name, a config key, a code path) is marked so. Every claim carries its URL; the survey's sources are cited by the same URL and marked "(read in the survey)" where this file did not read them again.
- **New tools beyond the survey** (found in [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) and [awesome-tuis](https://github.com/rothgar/awesome-tuis)): [ilmari](https://github.com/bnomei/ilmari), [sqwatch](https://github.com/fedonman/sqwatch), [gentooplz](https://github.com/JustRoccat/gentooplz), [drydock](https://github.com/yetidevworks/drydock), [agx](https://github.com/brevity1swos/agx), [amtr](https://github.com/arian-shamaei/anthropometer), [hwatch](https://github.com/blacknon/hwatch), [forgetop](https://github.com/magna-nz/forgetop). Deeper reads of survey tools: [yazi](https://github.com/sxyazi/yazi) (the Lua of its status bar and task list), [flowrs](https://github.com/jvanbuel/flowrs) (the Gantt renderer and model), [bottom](https://github.com/ClementTsang/bottom) (graph time zoom).
- **Checked on this machine, not inferred:**
  - The repo locks `ratatui` 0.30.2 (`Cargo.lock`), and its widgets crate `ratatui-widgets` 0.3.2 in the local cargo registry has `Sparkline` (`sparkline.rs`), `Gauge` and `LineGauge` (`gauge.rs`), `BarChart` and `Chart`. So the survey's open point (I-11: "unverified that the pinned ratatui version has them") is settled for gauges and sparklines. `grep -n "Sparkline\|LineGauge\|Gauge" tui/src` finds none in use: the Pulse view draws its bars by hand (`tui/src/plan/pulse.rs`).
  - `tui/src/statusbar/layout.rs` resets the Plan view to `-y 20%` (and the tree and Deploy column to `-x 20%`). At 62 rows that is **about 12 rows** for the Plan view (`man tmux`, 3.8: `-y` may be "followed by `%` for a percentage of the window size"). So whatever must show without scrolling has about 12 rows and 127 columns to do it in.
  - tmux 3.8 (`man tmux`): `status-interval` is "every interval seconds" (default 15), so a tmux status line cannot animate a 100 ms spinner; a status-line segment is static between refreshes. `client-focus-in` and `pane-focus-in` hooks exist.
- **Not verified:** how any screen looks (no screenshots), that the User's Ghostty over ssh shows any given glyph (the cockpit already needs a Nerd Font, `docs/commands.md` §13.2), and every "data exists" statement marked unverified below.

## 2. Where the cockpit stands, and what "following progress" asks

What shows progress today (all from `docs/commands.md` §13.2 and §17):

| Place | What it shows | Gap |
|---|---|---|
| Context column | per Plan Lead a dot and **five counters** `queued⋅running⋅merging⋅failed⋅merged` | five numbers must be summed in the head to learn "how far"; no total, no bar |
| Plan view | one box per PR, a flow chart of Job pills; **about 12 rows tall** (see §1) | scrolls when PR boxes stack; a finished PR keeps its full box |
| Deploy column | machine load; the User's open PRs, one flat list, two lines each (`open ✓ approved …`) | shows PRs only from `open` on, so the long phase before (Jobs running) is invisible here |
| StatusBar | `workers N/M` | a count, not *which* Workers or what they are doing |
| Pulse | one row per Plan with a 5-minute activity sparkline, `✓N`, age, `quiet` | shown **only while a Manager is in the chat slot** (§13.2 "Pulse", "When"), so seeing every Plan means swapping the chat away from the Lead he is talking to |
| Event log | every Event, one line each | a popup (`▤`) he must open; no grouping, no "since I looked" |

The questions a User asks while he follows work, and which ideas answer them:

| Question | Ideas |
|---|---|
| Is it moving? What is each Worker doing right now? | P-01, P-06, P-10 |
| How far along is it? | P-03, P-02, P-05 |
| What is slow, stuck or burning retries? | P-10, P-09, P-04 |
| What changed since I looked? | P-07, P-08, P-13 |
| What is it waiting for, and what is next? | P-02, P-09, P-12 |

## 3. The ideas, ranked

Ranked by how much each makes the User more efficient at following progress, then by cheapness. Cockpit area: **Tree** (Context column), **Plan** (Plan view), **Deploy** (right column), **Bar** (StatusBar), **Pop** (popup or zoomed pane).

| Rank | Id | Idea | Source | Area | Pain | Effort |
|---|---|---|---|---|---|---|
| 1 | P-01 | Workers board: every running Attempt, its last line, always visible | ilmari, sqwatch, yazi, process-compose | Deploy | P, F | M |
| 2 | P-02 | PR lifecycle stepper (jobs, open, ci, review, merge) | drydock, forgetop, gh-dash | Plan, Deploy | P, F | S |
| 3 | P-03 | A gauge at every level: Plan row, PR box, status line | yazi, ratatui | Tree, Plan, Bar | P, F | S |
| 4 | P-04 | Overview without switching: Pulse as a toggle, with more columns and a time zoom | bottom, btop, drydock | Pop | P, N | M |
| 5 | P-05 | Fold finished work, expand on click | lazygit, bosun, savras | Plan | P, L | S |
| 6 | P-06 | A detail card that follows the selected Job | gh-dash, agx, sqwatch, yazi | Plan | P, N | M |
| 7 | P-07 | What changed since you looked: heat fade and a "since" strip | amtr, hwatch, ilmari | Plan, Pop | P, F | M |
| 8 | P-08 | The Event log as a pinned, grouped feed with a histogram | lnav, agx | Pop, Plan | P, F | S / M |
| 9 | P-09 | A time view (Gantt) of the Plan's Jobs and Attempts | flowrs | Pop | P | M |
| 10 | P-10 | Elapsed against typical: a stuck bar on running Jobs | gentooplz, Reeve, amtr | Deploy, Plan | P, F | M |
| 11 | P-11 | Slot strip and counts-only summary in the status line | ilmari, pueue | Bar | P | S |
| 12 | P-12 | Check stages on PR rows: which check is red | forgetop, gh-dash | Deploy | P, F | M |
| 13 | P-13 | Replay: scrub a Plan back in time | amtr, agx, Reeve | Pop | P | L |

Why this order:

- **P-01 first.** "What is each Worker doing" is today an answer he must walk the flow chart (or open a Worker pane) for. Four tools (ilmari, sqwatch, process-compose, yazi) make *the live list of things in flight* the main screen.
- **P-02 and P-03 next:** they cost one view each, change no layout and replace a mental sum with a glance.
- **P-04 and P-05** attack "without scrolling or switching" directly: the Overview removes the Manager swap; folding gives rows back in a 12-row pane.
- **P-07 to P-10, P-12 and P-13** go from "follow" to "understand" (what changed, what was slow, what is red) and cost more; P-11 is cheap but small.
- **Small first** (S, no dependency): P-02, P-03, P-05, P-11. The Inbox is **not** a dependency of any idea here.

Two full-size mockups (215×62) follow in §5: **Mockup A** (today's columns, upgraded with P-01, P-02, P-03, P-05, P-06, P-11) and **Mockup B** (a zoomed Overview with P-04, P-07, P-08 and P-09).

## 4. The ideas

### P-01. Workers board: every running Attempt, its last line, always visible

- **Pattern:** a live table of everything in flight: a state glyph, an age, and the latest output line under or beside it. Rows keep a stable order and the selection survives each refresh. [ilmari](https://github.com/bnomei/ilmari#what-the-radar-shows) lists every agent pane with `running`, `waiting-input`, `finished`, `terminated` or `unknown`, a recent output excerpt and git and CPU stats; inside a workspace "running panes appear first, waiting panes appear next, and finished, terminated, or unknown panes follow", and the most recent waiting pane is selected. [sqwatch](https://github.com/fedonman/sqwatch#features) has an auto-refreshing job table with colour-coded states, "your selection and cursor position survive each refresh", fetches on a background thread, a following stdout tail beside it, and panels that toggle and are remembered. [yazi](https://github.com/sxyazi/yazi/blob/HEAD/yazi-plugin/preset/components/tasks.lua)'s task list draws three rows per task: icon and title, a progress line, and a `┃` bar at the left, in a different style on the task under the cursor. [process-compose](https://github.com/F1bonacc1/process-compose/blob/HEAD/src/tui/proc-table.go) (read in the survey) puts icon, name, status, age and health in one table.
- **cpd mapping:** a new first section of the **Deploy column**, `──[ workers 3/4 ]──`, above the PRs; the machine line shrinks to one row. Each Attempt holding a slot is two rows: `<spinner> <plan>/<job>  <tier> a<n>  <age>` and the Worker's last line (`→ tool` or `← result`, the format the Worker view already draws, `docs/commands.md` §13.2 "worker"). Order: failed in the last N minutes first (with `↻ retry`, which runs `job retry`, §5.4), then working Attempts in start order (the order never changes under the cursor, see the survey's I-27). Click a row: open the Worker pane beside the chat (the existing action, §17 "Actions"). A header click cycles the scope: this Plan, this Context, all. The data is read already: the StatusBar counts Attempts not ended (§17), the Worker view re-reads a session file when its size or mtime changes (§17 "Rates"). This is the survey's I-14 (popup) made **persistent**, so no click is needed to see it. Serves **P**, **F**.
- **Effort:** M. A new section in the `widgets` view and a tail read of each running Worker's session file (the renderer is `tui/src/worker/render.rs`); no new command.
- **Mockup:** Mockup A, right column (§5).

```
 ──[ workers 3/4 ]──────────────────────
 ✗ schema/links  H2 a2   failed 2m
   exit 1 · cargo clippy         ↻ retry
 ⠹ schema/ui     S2 a2   7m ▰▰▰▰▱▱
   edit src/ui/Login.tsx
 ⠼ auth/session  S2 a1   3m ▰▰▱▱▱▱
   cargo test -p auth
 ○ 4 queued
```

### P-02. PR lifecycle stepper: jobs, open, ci, review, merge

- **Pattern:** show an item's whole path as a row of stage glyphs, each with its own state, instead of one summary word. [drydock](https://github.com/yetidevworks/drydock) makes each question its own column ("Release state is a separate axis from working state: a repo can be dirty and released") with the colour carrying the state ("so the rows worth acting on stand out without reading a word"). [forgetop](https://github.com/magna-nz/forgetop) lets you "drill into pipeline stages" of a CI run. [gh-dash](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/layout/pr.mdx) (read in the survey) gives `ci` and `reviewStatus` each their own one-icon column.
- **cpd mapping:** today a PR is one word in its box title (`open`, `merged`, `conflict`) and, from `open` on, a second line in the Deploy column (`open ✓ approved conflict …`). The PR's **whole** path is five facts already in the read model: its Jobs (all merged?), the PR opened, checks, review, merge (`prs`, `pull_requests` with `checks`, `review`, `mergeable`; §17). Draw them as one stepper: `▰▰▰▱▱ 2/3 jobs ▸ ○ open ▸ ○ ci ▸ ○ review ▸ ○ merge`. In the **Plan view** it sits on the box's **bottom border**, which is unused today except for the `┬` arrows; the label words show when the box is wide enough (about 46 cells, inferred) and glyphs only below that. In the **Deploy column** the second line becomes `✓ci ✗review ○merge` plus ` conflict` as now (Mockup A) (a PR is `open` there, so jobs and open are always done and are dropped). A red step is the thing to do; the User no longer has to infer it. Serves **P**, **F**.
- **Effort:** S. Pure rendering over fields the views already read; no command, no write.

```
 ┌─ schema · api → main ─────────────────────────────┐
 │  ✓ types │S1 ──── ✓ gql │S2 ──── ⠹ ui │S2│a2      │
 └─ ▰▰▰▱▱ 2/3 jobs ▸ ○ open ▸ ○ ci ▸ ○ review ▸ ○ merge ─┘

 ┌─ auth · api → main · #34 ─────────────────────────┐
 │  ✓ token │S1                                       │
 └─ ✓ 1/1 jobs ▸ ✓ open ▸ ✓ ci ▸ ✗ review ▸ ○ merge ─┘   changes requested
```

### P-03. A gauge at every level: Plan row, PR box, status line

- **Pattern:** a thin gauge with a count, in a fixed corner; red when anything failed; hidden when there is nothing to count. [yazi](https://github.com/sxyazi/yazi/blob/HEAD/yazi-plugin/preset/components/progress.lua)'s status bar has a gauge at its right end, at most 20 cells wide, drawn with `th.status.progress_normal` or, when `summary.failed` is not 0, `th.status.progress_error`; it returns nothing when `summary.total == 0`; its label is the percentage and `N left`. Each task in the task list shows `x/y`, and `x/y, N failed` when some failed ([tasks.lua](https://github.com/sxyazi/yazi/blob/HEAD/yazi-plugin/preset/components/tasks.lua)). ratatui ships it: `Gauge` (with `use_unicode` for finer steps) and `LineGauge` for a thin bar (`ratatui-widgets` 0.3.2 `gauge.rs`, in the local cargo registry, the version this repo locks).
- **cpd mapping:** four places, one rule (merged ÷ not-cancelled Jobs; red `✗N` when N Jobs are failed):
  1. **Tree**, each Plan Lead row: `▰▰▰▱▱ 6/10 ✗1` in place of the five counters (the legend keeps the counters' meaning; `2⋅1⋅0⋅1⋅6` becomes the hover or the right-click detail). 14 cells in place of about 9, in a column 43 cells wide.
  2. **Context row:** the sum over its Plans when collapsed.
  3. **Plan view, PR box title** (right corner, as in Mockup A) and the folded row of P-05.
  4. **StatusBar:** one aggregate segment `▰▰▰▱▱▱▱▱ 9/24 jobs ✗1`.
  Whether drafts count in the denominator is a product question (they are "planned", not "queued"); the numbers above count them. Serves **P**, **F** (the `✗N` is a "needs you" cue).
- **Effort:** S. The counters are already in the read model (§13.2 "Rows"); gauges are drawn in the views that exist.

```
 today                                  proposed
   ⏵ ⠋ schema        2⋅1⋅0⋅1⋅6            ⏵ ⠋ schema    ▰▰▰▱▱ 6/10 ✗1
     ● auth          4⋅1⋅0⋅0⋅1              ● auth      ▰▱▱▱▱ 1/5
     ○ search        3⋅0⋅0⋅0⋅0              ○ search    ▱▱▱▱▱ 0/3
```

### P-04. Overview without switching: Pulse as a toggle, with more columns and a time zoom

- **Pattern:** one dense screen of every item with a status column and a trend per row, reached by one click and left by one click. [bottom](https://github.com/ClementTsang/bottom/blob/HEAD/docs/content/usage/general-usage.md) selects a widget and `e` expands it to the whole terminal ("hide all other widgets"), `e` or `esc` returns; its graphs zoom their time range with `+` and `-`, from 30 s to 600 s, with `=` to reset ([cpu-graph.md](https://github.com/ClementTsang/bottom/blob/HEAD/docs/content/usage/widgets/cpu-graph.md)). [btop](https://github.com/aristocratos/btop#configurability) (read in the survey) has presets of boxes. [drydock](https://github.com/yetidevworks/drydock) tells you, in one live screen, what is uncommitted, unpushed and unreleased across every repo, with a time filter ("the last hour, day, week or month") over a column per question.
- **cpd mapping:** the Pulse view already is this table (a Plan per row, a sparkline per row, `✓N`, age, `quiet`), but it appears **only while a Manager is in the chat slot** (`docs/commands.md` §13.2 "Pulse", "When"), so looking at all Plans costs the chat slot. Make it reachable at any time: a `Plan | Pulse` toggle in the Plan view's header, or a click on the pane header that zooms the Plan pane into the Overview (tmux zoom, `resize-pane -Z`; the survey's I-03 and I-12). Add the columns the other ideas make cheap: the gauge (P-03), the PR chips (P-02 compressed), the last Worker line (P-01). Add a clickable window zoom `‹ 1h [3h] 12h ›` that changes the bucket size (bottom's `+`/`-`), and keep the rule that rows do not move under the cursor (order by `created_at`). Serves **P**, **N**. A `by Context | by status` toggle (survey I-26) belongs on the same header.
- **Effort:** M. `tui/src/plan/pulse.rs` exists; this is a toggle, extra columns, a window parameter, and a second place to open it.
- **Mockup:** Mockup B (§5).

### P-05. Fold finished work, expand on click

- **Pattern:** an accordion: the thing in focus gets the room, the rest collapses to one row that still carries the worst state of what is inside. lazygit's `expandFocusedSidePanel` ("increase the height of the focused side window; creating an accordion effect", [Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md), read in the survey). [bosun](https://github.com/yetidevworks/bosun) (read in the survey): a collapsed row "picks up a small accent dot when any background tab is busy". [savras](https://github.com/Marijusaj/savras#readme) (read in the survey): a header counts `1 needs input · 2 working · 4 done`.
- **cpd mapping:** in the Plan view a PR box whose Jobs are all merged (the PR is `open` or `merged`) folds to one row: `▸ ✓ core · api → main · #28 merged ↗  ▰▰▰▰▰ 3/3`. A click on `▸` expands it in place. A folded box with anything not green keeps its glyph (`✗`, `⠹`, `?`) on the folded row, so folding never hides a need. In a pane of about 12 rows, a finished PR's three or more rows go back to what is moving. The fold is view state only (no command, no write), as the tree's archived-rows fold already is (§13.2 "Archived rows"). A header button `fold done` folds every finished box at once. Serves **P**, **L**.
- **Effort:** S. A layout rule in `tui/src/plan/boxes.rs` plus a click target; no data.

```
 before (3 boxes, 11 rows)               after (core folded, 8 rows)
 ┌ core · api → main · #28 merged ┐       ▸ ✓ core · #28 merged ↗ ▰▰▰▰▰ 3/3
 │ ✓ a │S1 ─ ✓ b │S1 ─ ✓ c │S2   │       ┌ schema · api → main ───────────┐
 └────────────────────────────────┘       │ ✓ types ── ✓ gql ── ⠹ ui       │
 ┌ schema · api → main ───────────┐       └────────────────────────────────┘
 │ ✓ types ── ✓ gql ── ⠹ ui       │
 └────────────────────────────────┘
```

### P-06. A detail card that follows the selected Job

- **Pattern:** a preview that follows the selection, placed to the right, or below on a narrow terminal. gh-dash: `preview.position: auto` (read in the survey). [agx](https://github.com/brevity1swos/agx#what-it-shows) turns an agent session into a timeline of colour-coded steps (`[user]` cyan, `[asst]` green, `[tool]` yellow, `[result]` magenta) and, on selecting a result, shows "both the original tool call input and the response in one detail view". sqwatch has a following log tail beside its table; process-compose a log pane under its table (`LogProcHalf`, read in the survey).
- **cpd mapping:** the Plan view is 127 columns wide and its boxes are 33 to 51 wide, so the right side is empty. Put a card there for the selected Job (a click or the arrow keys): status and age, its Tier, one line per Attempt (`a1 failed 12m ago: clippy`), the Worker's last `→ tool` and `← result`, and four buttons: Open Worker, Retry, Cancel (`job cancel`, §5.3), Copy key. It merges the survey's I-05 (preview strip) and I-33 (a menu for pills). Today a Job pill opens a Worker on Enter (§13.2) and, as far as §13.2 lists, has no menu (inferred: none is described). Serves **P**, **N**.
- **Effort:** M. A new region (right of the boxes, or under them when narrow), a tail read of the Worker file, and a button row mapped to existing commands.
- **Mockup:** Mockup A, Plan view (§5).

### P-07. What changed since you looked: heat fade and a "since" strip

- **Pattern:** show *change* as something that decays. [amtr](https://github.com/arian-shamaei/anthropometer) draws live file traffic as "a 'now' view of what's being touched this instant (fading on a heat law)", and lets `←/→` rewind every view. [hwatch](https://github.com/blacknon/hwatch#readme) records each run of a command and lets you page through the history and the differences between runs. [ilmari](https://github.com/bnomei/ilmari#what-the-radar-shows) latches attention: its attention icon shows "only for an unacknowledged sticky attention latch" (that it clears on acknowledgement is inferred from the name); it also rings a bell when a pane moves into waiting or finished.
- **cpd mapping:** two small parts. (a) **Heat:** a pill, row or box whose status changed in the last 1 to 2 minutes is drawn one step brighter and fades in three steps; nothing else changes about it. The eye is drawn to the one thing that just moved among twenty. (b) **Since strip:** one line at the bottom of the Plan view or Overview, `since you looked 14:52 · 6 changes ▸`, which opens the Events since then as a short list, each line a click that goes to that Job or Lead (Mockup B, lower part). "Looked" is when the cockpit last had focus: tmux has the hooks `client-focus-in` and `pane-focus-in` (`man tmux`, 3.8). Where to keep the timestamp is open: `docs/commands.md` §17 says "no tmux option carrying state", and a view's memory is lost when it respawns. Serves **P**, **F**.
- **Effort:** M. The heat is S (a timestamp per Event already read); the "since" marker needs a place to live (view memory is S but forgets at a respawn).

```
 heat, 3 steps          ⠹ ui │S2│a2     ✗ links │H2│a2     ✓ gql │S2
                         bright+bold      bright (new)      normal (fades)
 since strip            ──[ since you looked 14:52 · 6 changes ▸ ]──
```

### P-08. The Event log as a pinned, grouped feed with a histogram

- **Pattern:** one time-ordered feed that merges many sources, with an index of what matters. [lnav](https://github.com/tstack/lnav#readme) "merges files by time into one view", "builds an index of errors and warnings", jumps with `e`/`E`, and has a histogram of messages over time on `i` ([docs.lnav.org](https://docs.lnav.org/en/latest/ui.html#hist)). agx colours each step by kind and has a `--live` mode with `--notify-on-idle 10m` ("OS notify when session hasn't grown for 10 minutes").
- **cpd mapping:** the Event log (§13.2 "logs") is a popup, one line per Event, oldest first. As a **pinned section** (the lower part of the Overview, Mockup B; or one row under the Plan view that opens it) it needs only three changes: (1) newest first, (2) bursts grouped (`3 Jobs merged in schema`; the Event kinds are already named in §13.2 "Activity"), (3) a rate strip, which is Pulse's bucket data (§13.2) reused, so a burst or a quiet stretch shows. Add `n`/`N` to jump to the next failed or needs-you Event, as the survey's I-21 has. Serves **P**, **F**.
- **Effort:** S for pinning and newest-first (the view exists); M with grouping.

```
 ──[ activity · schema · last 60 min ]───────────  ▁▂▁▃▄▃▁▁▅█▄▃▁▂
  15:08  ✗ schema/links  failed a2 (cargo clippy)
  15:07  ⠹ schema/ui     a2 started
  15:04  ✓ 3 Jobs merged in auth                  ▸ expand
```

### P-09. A time view (Gantt) of the Plan's Jobs and Attempts

- **Pattern:** a bar per row on a shared time axis. [flowrs](https://github.com/jvanbuel/flowrs/blob/HEAD/src/ui/gantt.rs) (an Airflow TUI in ratatui) draws a Gantt **column inside its task table**: one `▃` per cell, coloured by state, one segment per *try*, with "lead-in" phases first (scheduled, queued, then the running segment on top); a legend line (`■ scheduled`, `■ queued`, `■ running`, …) sits under the column. Its model ([gantt.rs](https://github.com/jvanbuel/flowrs/blob/HEAD/src/airflow/model/common/gantt.rs)) keeps per task a list of tries with `scheduled_when`, `queued_when`, `start_date`, `end_date`, `state`; the window starts at the earliest known time and ends at `now` while anything is running, else at the latest end. The [CHANGELOG](https://github.com/jvanbuel/flowrs/blob/HEAD/CHANGELOG.md) lists "add Gantt chart to task instances panel" and "show scheduled and queued phases".
- **cpd mapping:** the flow chart shows *dependency*; a lane per Job shows *when*, and where a Plan waited or burned retries. A `flow | time` toggle in the Overview (Mockup B): one lane per Job, one segment per Attempt (flowrs paints all tries of a task on one lane; the mockup gives each Attempt its own lane so that the cost of a retry stands out, which is a choice, not flowrs' behaviour), `·` for queued, `▃` for running, `✓` or `✗` where it ended, a `┊` for now, and a reason where a Job waits (`waits: PR web needs PR schema`). In the mockup the cause of a slow Plan is visible at once: `ui a1` ran 23 minutes and failed, and `links` failed twice after 23 and 30 minutes. The Attempts' `started_at` and `ended_at` are read already (Pulse uses them, §13.2 "Activity"); The `·` phase needs the time a Job became `queued`: the `jobs` table has only `created_at` (migration `0001.sql`), but every write is kept in the append-only `history` table with its transaction time (`txns.at`; `docs/contract.md` §1, "Every write is recorded in the append-only `history` table"), so the time of the update to `queued` is derivable (inferred; not run). Serves **P**.
- **Effort:** M. The renderer is small (flowrs paints a per-cell colour map, then joins runs of the same colour); the data join over `attempts` and `history` is the work.
- **Mockup:** Mockup B, lower half (§5).

### P-10. Elapsed against typical: a stuck bar on running Jobs

- **Pattern:** a bar that fills against an expectation and changes colour when it is exceeded, never a made-up percentage. [gentooplz](https://github.com/JustRoccat/gentooplz) ("Real progress, not guesses") parses `[123/456]`, `[ 45%]` and `Compiling` lines from each build's log tail and, when a build prints nothing structured, falls back to counting source against object files; it also shows "3 of 15" for the whole queue. [Reeve](https://github.com/Dancode-188/reeve#what-it-does) (read in the survey) has a 0 to 100 health score that "changes color as it drops. Green, amber, red", and rules such as `health_score < 30`. [amtr](https://github.com/arian-shamaei/anthropometer) labels "every quantity authoritative … or estimated, never blurred together".
- **cpd mapping:** a Worker has no `n/N`, so "how far" cannot be known; "how long compared to usual" can. On a running pill and on a Workers board row (P-01) draw a thin bar of *elapsed ÷ the median duration of finished Attempts of the same Tier*, labelled `(est.)`. Past 1.5 times it turns amber and gets a `!`; Pulse's `quiet` rule (30 minutes of no activity, a constant, §13.2) is the Plan-level cousin, and a Job-level rule may later open an Inbox item (the survey's I-37). Show nothing until there are three finished Attempts of that Tier (inferred: avoids a misleading bar on the first Plan). The data is in the store: `attempts.started_at` and `ended_at` (migration `0001.sql`) and the Job's `tier`, so the median is one query over finished Attempts joined to their Jobs (inferred; not run). Serves **P**, **F**.
- **Effort:** M. A read query, a bar, two thresholds (constants, not config, like the 30 minutes).

```
 ⠹ schema/ui     S2 a2   7m ▰▰▰▰▰▰▱▱▱▱ (est.)
 ⠴ login/form    S2 a1  14m ▰▰▰▰▰▰▰▰▰▰ ! slow: typical 9m
```

### P-11. Slot strip and counts-only summary in the status line

- **Pattern:** a summary that shows only the non-zero counts, the ones needing attention first. [ilmari](https://github.com/bnomei/ilmari#add-the-global-summary-to-the-status-line) publishes `@ilmari_status_summary` for tmux's `status-right`; it "uses the same configured glyphs as the popup and shows only nonzero counts: sticky attention first, then ordinary waiting, finished, running, terminated, and unknown states", plus per-window badges (`@ilmari_window_badges`). [pueue](https://github.com/Nukesor/pueue#features) has groups, each with "several tasks running in parallel" and a limit you set.
- **cpd mapping:** the StatusBar's right side says `workers 3/4`. Make it a strip with one cell per slot, `●●●○ 3/4 · 4 queued`, followed by the P-03 aggregate gauge and the failed count, and drop every zero (`· 0 failed` never shows). A click goes to the Workers board (P-01) or opens its popup form. Because the StatusBar is moving to tmux's status line (the Job's brief), note the limit: `status-interval` is in seconds (tmux 3.8 man page), so the strip is static glyphs and cannot spin at 100 ms like the tree's dots. Serves **P**.
- **Effort:** S. One format string over numbers the StatusBar reads already (Attempts not ended, `concurrency`; §17).

```
 ⧉ ? ▤ │ needs you: ● schema/review  ● login/question  +2 │ ▰▰▰▱▱▱▱▱ 9/24 jobs ✗1 │ ●●●○ 3/4 · 4 queued
```

### P-12. Check stages on PR rows: which check is red

- **Pattern:** a CI run shown as its stages, so "failing" says *what* fails. forgetop lets you "drill into pipeline stages" and "trigger runs" ([README](https://github.com/magna-nz/forgetop)); [glim](https://github.com/junkdog/glim) is a ratatui TUI for "monitoring GitLab CI/CD pipelines and projects" (the stage layout is in screenshots, not read); gh-dash's `w` key watches the checks of the selected PR (read in the survey).
- **cpd mapping:** the Deploy column shows one checks icon per PR (`✓`, `✗`, `◌`; `docs/commands.md` §13.2 "widgets"). The poll already asks `gh pr view --json …statusCheckRollup…` (§14.1 names it in the `gh` row). A second line `✗ci 2/9 red: lint, doctest` would say which, and the same text would fill the P-06 card for a PR. **Checked:** the per-check rollup is **not** stored: `pull_requests.checks` is a four-value summary (`none`, `passed`, `pending`, `failed`; migration `0006.sql`) and no table holds individual checks (the tables are in the migrations), so the poll would have to store the failing names (a column or a table). Serves **P**, **F**.
- **Effort:** M. A migration, and the `pr poll` step that already reads the rollup (§14.1) has to keep the failing names.

### P-13. Replay: scrub a Plan back in time

- **Pattern:** a timeline scrubber that re-renders the whole view at a past moment. [amtr](https://github.com/arian-shamaei/anthropometer): "`←/→` REPLAY re-renders every view at any past turn from the engine's own snapshot"; "a timeline scrubber holds the whole session". agx's "dual-cursor / heatmap / time-travel approach" and Reeve's `R` ("replays a finished trace like a DVR", read in the survey).
- **cpd mapping:** the Events are an append-only log (`events list --since`, §11.1), so "the Plan at Event N" is well defined. A scrubber under the Overview's time view (P-09): drag or click to 14:30 and the pills and gauges show their status then. It answers "what happened while I was away" more completely than P-07. The data is there: `history` keeps the before and after of every row change with its transaction time (`docs/contract.md` §1), so state at any past transaction can be rebuilt (inferred; not run). The cost is that the views read current rows (§17), so each view would need an as-of read. Serves **P**.
- **Effort:** L. As-of reads (or a replay of `history` into a scratch database) behind every view, plus the scrubber. Last, because P-07 and P-09 give most of the value; M for a Gantt-only scrubber, which needs only `attempts`.

## 5. Full-size mockups (215×62)

Both are 215 columns by 62 rows, drawn with the cockpit's real split: the tree and the Deploy column 43 columns each, the centre 127, two 1-column borders; the pane header on each pane's first row (inverted in the real cockpit) and the StatusBar on row 62. The data is one invented scenario (Context `api`, Plan `schema` in the chat slot) used by both. Glyphs are the ones that exist in the cockpit's palette or plain Unicode; the Nerd Font dots are shown as `●`, `○`, `⠋` for plain text. These are inventions to look at, not specifications.

### Mockup A: today's columns, upgraded

P-03 (gauges on the tree rows, the PR box titles and the status line), P-02 (the stepper is on the PR rows of the Deploy column; the box bottom border is shown in §4), P-05 (the folded `core` box), P-06 (the card at the right of the Plan view), P-01 (the workers section on top of the Deploy column) and P-11 (the status line). The Plan view is the 12 rows it has today; everything fits without scrolling, and the finished `core` PR takes one row.

```
 Context                                   │ Plan                                                                                                                          │ Deploy
                                           │                                                                                                                               │
 ▾ ●  api                                  │  ┌─ schema · api → main · dispatch ──── ▰▰▰▱▱ 2/3 ─┐  ┌─ docs · api → main ▰▰▱▱▱ 1/2 ─┐   ┌─ schema/ui · S2 · a2 ───────────┐ │ mbp-dev  cpu 38% ⣀⣤⣶⣿⣷⣤⣀⣠  mem 61%
   ⏵ ⠋ schema       ▰▰▰▱▱ 6/10 ✗1          │  │ ✓ types │S1 ──── ✓ gql │S2 ──── ⠹ ui │S2│a2     │  │ ✓ readme │H2 ── ✗ links │H2│a2│   │ ⠹ 7m · typical S2 ≈ 9m (est.)   │ │
     ● auth         ▰▱▱▱▱ 1/5              │  └────────────────────────┬────────────────────────┘  └───────────────────────────────┘   │ ▰▰▰▰▰▰▰▱▱▱                      │ │ ──[ workers 3/4 ]────────────────────────
     ○ search       ▱▱▱▱▱ 0/3              │                           │                                                               │ → edit src/ui/Login.tsx         │ │
 ▾ ○  web                                  │                           ▼                                                               │ ← ok · 214 lines                │ │ ✗ schema/links  H2 a2   failed 2m
     ● login        ▰▰▱▱▱ 2/6              │           ┌─ web · web → main  ▱▱▱▱▱ 0/2 ─┐                                               │ a1 failed 12m ago: clippy       │ │   exit 1 · cargo clippy         ↻ retry
 ▸ ○  ops                                  │           │ ○ page │S1 ── ○ list │S2      │                                               │                                 │ │ ⠹ schema/ui     S2 a2   7m ▰▰▰▰▱▱
 …                                         │           └───────────────────────────────┘                                               │ [Open Worker] [Cancel] [Copy]   │ │   edit src/ui/Login.tsx
                                           │                                                                                           └─────────────────────────────────┘ │ ⠼ auth/session  S2 a1   3m ▰▰▱▱▱▱
                                           │  ▸ ✓ core · api → main · #28 merged ↗  ▰▰▰▰▰ 3/3      (folded; a click opens it)                                              │   cargo test -p auth
                                           │                                                                                                                               │ ⠴ login/form    S2 a1  14m ▰▰▰▰▰▰ !
                                           │                                                                                                                               │   slow: typical 9m · waiting on model
                                           │ schema · Lead                                                                                                                 │ ○ 4 queued
                                           │                                                                                                                               │
                                           │  you    the docs job failed twice, what is going on?                                                                          │ ──[ PRs ]────────────────────────────────
                                           │                                                                                                                               │
                                           │  agent  → cpd job show docs/links                                                                                             │ cpd/api#34 auth                        ↗
                                           │         ← failed a2: cargo clippy -p docs exit 1 (unused import in src/links.rs)                                              │   ✓ci ✗review ○merge · changes req.
                                           │         The Worker added the import in a1 and removed its use in a2. I will retry with a note.                                │ cpd/web#12 login           conflict    ↗
                                           │         → cpd job retry docs/links --note "keep the import, it is used by the doctest"                                        │   ✗ci ○review ○merge
                                           │                                                                                                                               │ cpd/api#30 hotfix (no Plan)            ↗
                                           │  you    ok, and ping me when the schema PR is green                                                                           │   ✓ci ✓review ○merge
                                           │                                                                                                                               │
                                           │  ⠋ agent is working …                                                                                                         │ ──[ git ]────────────────────────────────
                                           │                                                                                                                               │
                                           │                                                                                                                               │ (future)
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
                                           │                                                                                                                               │
 ──[ Inbox 2 ]──────────────────────────── │                                                                                                                               │
                                           │                                                                                                                               │
 ● schema  review pending ▸                │                                                                                                                               │
 ● login   question ▸                      │  ┃ message the schema Lead…                                                                                                   │
 ⧉ ? ▤ │ needs you: ● schema/review  ● login/question  ● auth#34/changes  +1 │ ▰▰▰▱▱▱▱▱ 9/24 jobs ✗1 │ ●●●○ 3/4 · 4 queued
```

### Mockup B: the Overview, zoomed

P-04 (the Pulse table as a toggle with gauge, PR chips, last line and a window zoom), P-09 (the time view of the selected Plan: `ui` ran 23 minutes and failed, `links` failed twice, `page` and `list` wait on a PR), P-07 and P-08 (the "since you looked" feed), P-02 (the PR lines). Zoom is a click on the Plan pane's header (tmux zoom: `resize-pane -Z` toggles the active pane between zoomed and unzoomed, `man tmux`); the StatusBar stays. The empty rows are room for the Plan's other lists.

```
 Overview · zoomed (click the header to restore)   [ by Context | by status ]   window  ‹ 1h ›  [3h]  ‹ 12h ›

    PLAN          JOBS             PRs (current step)                                 ACTIVITY · 5 min buckets, last 3 h         AGE    LAST

  ▾ ● api
    ⏵ ⠋ schema      ▰▰▰▱▱ 6/10 ✗1   ⠹ schema · ⠹ docs · ○ web · ✓ core#28              ▁▂▁▃▄▃▁▁▅█▄▃▁▂▄▃▂▁▁▃▅▄▂▁▃▃▂▁▂▃▅▇▆▃   0m    ⠹ ui: edit src/ui/Login.tsx
      ● auth        ▰▱▱▱▱ 1/5       ✓ #34 open · ✗review (changes requested)           ▁ ▁▁▃▃▂ ▁▁▂▄▅▃▂▁▁▁▂▃▂▂▃▄▃▂▁▁▁▂▃▂▃▃▂▁    0m    ⠼ session: cargo test -p auth
      ○ search      ▱▱▱▱▱ 0/3       ○ waiting                                                                                    –    drafts: review pending ▸
      ✓ core        ▰▰▰▰▰ 3/3       ✓ #28 merged                                       ✓   ▁▃▆▇▃▁                                 2h    merged 14:12
  ▾ ○ web
      ● login       ▰▰▱▱▱ 2/6       ✗ #12 ci red · conflict · ○ next                   ▃▂▁  ▁▂▅▇▅▃▂▁▁▁▂▃▄▅▃▂▁▂▃▂▁▁▁▂▃▃▂▁▁▂   quiet 41m  ? asks: which test account?
  ▸ ○ ops
 ─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 ──[ schema · time view   14:00 → 15:10   10 Jobs · 10 Attempts · 3 failed ]──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
                        14:00                14:10                14:20                 14:30                14:40                 14:50                15:00                ┊
 core/a                 ··▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✓                                                                                                                                 ┊
 core/b                 ··▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✓                                                                                                                           ┊
 core/c                                    ······▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✓                                                                                                          ┊
 types S1               ····▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✓                                                                                                                    ┊
 gql S2                                                 ··▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✓                                                                            ┊
 ui S2 a1                                                                                       ···▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✗                        ┊
 ui S2 a2                                                                                                                                           ···········▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃
 readme H2              ····▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✓                                                                                                                           ┊
 links H2 a1                                     ··▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✗                                                                       ┊
 links H2 a2                                                                                         ····▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✗   ┊
 page S1                waits: PR web needs PR schema to merge                                                                                                               ┊
 list S2                waits: PR web needs PR schema to merge                                                                                                               ┊
                                                                                                                                                                           now
 legend  ▃ working   · queued   ✓ merged   ✗ failed   ┊ now   (bars cut at the window; a Job that waits shows why)

 ──[ since you looked 14:52 · 6 changes ]─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  15:08  ✗ schema/links  failed a2 (cargo clippy)          Lead retried with a note
  15:07  ⠹ schema/ui     a2 started
  15:04  ✓ auth/token    merged, PR auth#34 opened
  15:01  ● login         asks: which test account? ▸ answer
  14:58  ✗ schema/ui     failed a1 (cargo clippy)
  14:55  ✓ docs/readme   merged

 ──[ PRs of schema ]──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  schema   api → main   ⠹ dispatch   ▰▰▰▱▱ 2/3 Jobs    no pull request yet
  docs     api → main   ⠹ dispatch   ▰▰▱▱▱ 1/2 Jobs    ✗ links failed twice
  web      web → main   ○ waiting    ▱▱▱▱▱ 0/2 Jobs    needs schema
  core     api → main   ✓ merged     ▰▰▰▰▰ 3/3 Jobs    #28 ↗


















 ⧉ ? ▤ │ needs you: ● schema/review  ● login/question  ● auth#34/changes  +1 │ ▰▰▰▱▱▱▱▱ 9/24 jobs ✗1 │ ●●●○ 3/4 · 4 queued
```

## 6. Looked at and left out

- **Token and cost dashboards** ([claudectl](https://github.com/mercurialsolo/claudectl), [toktop](https://github.com/htin1/toktop), [LimitDeck](https://github.com/rockythink/limitdeck), [token-monitor](https://github.com/xzy9565/token-monitor)): about spend, not progress; a possible separate theme.
- **A context-window map** ([amtr](https://github.com/arian-shamaei/anthropometer)): excellent, but it shows what is inside one pi session, which is pi's job, not the cockpit's. Only its replay and its "estimated, never blurred" rule were taken (P-13, P-10).
- **Pixel-art office** ([pixtuoid](https://github.com/IvanWng97/pixtuoid)) and **web dashboards** (forgetop's browser view, bullmq-dash's web mode): look, not a way to follow progress in a terminal; the cockpit is a terminal.
- **Kanban boards** (kagan, Backlog.md; the survey's reasons): a Plan's PR boxes and Job flow are already the board; a column-per-status board would drop the dependency flow.
- **Per-process CPU/memory graphs** (btop, bottom, gentooplz's resource view): the load line exists, its sparkline is the survey's I-11 (now known to be buildable, §1), and Workers are pi sessions whose CPU is not the question.

## 7. Open points for the next Job to weigh

- P-03: whether draft Jobs count in the denominator (the numbers here count them).
- P-07: where the "last looked" time lives, given `docs/commands.md` §17 forbids tmux options for state.
- P-09 and P-10: the `queued` time and the per-Tier median are derivable from `history` and `attempts`, not stored as columns; the queries were not run.
- P-12: the poll must start keeping the names of failing checks (not stored today).
- Any idea that needs the Deploy column's width (P-01) should be checked against the real column: it is 43 columns, about 41 of text.
