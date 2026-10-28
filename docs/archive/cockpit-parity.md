# cpd 2 cockpit: parity checklist (an audit against the running cpd 1 cockpit)

**Inputs.** I compared these against each other:
- the reference (`main`: `tui/src/*.rs`, `tui/scripts/*`, `tui/README.md`);
- the target (`cpd.cpd-cpd2`: `cli/src/tui/**`, `cli/src/host/{tmux,layout}.rs`, `docs/`);
- the captures in `~/.cpd/plans/cpd-rust/parity/`.

**Caveats.**
- **The captured views run an old binary.** The cpd 2 plan capture shows `workers 0/3` on the tab row, but the current source draws only the PR there (`cli/src/tui/plan/mod.rs:170–171`), and `plan/` no longer has a workers module. So the captured panes were started before the last reinstall. `cpd2 tui` respawns only *dead* panes (`cockpit.rs` `respawn_dead`), so a reinstall never reaches a running view. Some of what the User sees is old code.
- **Not inferred:** where a row says "verify", I couldn't confirm it from source or captures alone.

## 0. The User's three points

**(a) Why the Dispatch column looks "totally broken".** cpd 2's `widgets` view isn't a port of the reference Dispatch column. It's a different design, written to `commands.md` §17 (line 1373: "PRs and their feedback, the engine's state") and to the `tui-widgets` Spec, which added Slots, Escalations and Events. The User's sections are missing (` homeserver`/` load`, PRs listed by Plan label, ` git`, ` Jira`). In their place are an engine line, slots, Escalations, a raw Events log that fills the column (in the capture, a burst of "The engine failed at E2" lines) and a footer the reference doesn't have.

**(b) The StatusBar.** The captured content is identical byte for byte:
- the same items and styling: reversed `⧉ ` and ` ? legend `, then dim `workers`, the count in colour 8, and the total dim;
- the same `bar::draw`;
- both right edges at the same column;
- a full-width split, `-v -f -l 1` (`host/tmux.rs:169`).

So a bar that "doesn't span" isn't reproduced by the code or this capture. Likely causes, unverified:
1. **A cockpit built before the StatusBar existed.** It gets the bar through `add_missing_statusbar` with no reset (`cockpit.rs`, A7), and without the border options until `cpd2 tui` reruns.
2. **Stale view panes**, as in the caveat above.
3. **A cockpit started by `plan new`.** Its first window is `park-…` and `main` is added later. Until `cpd2 tui` runs, it has none of the TUI options (border format, active border, mouse binding), because those are set only by `cpd2 tui` (`cockpit.rs` `TUI_OPTIONS`) and not at server start.

To verify: on the `cpd2` server, compare `#{pane_width}` of the `@cpd_view statusbar` pane with `#{window_width}` of `main`. The real differences found:
- **Workers count:** the "room" given to it (§1.4 table below).
- **Refresh:** event-driven at 500 ms with a 2-second floor, against a flat 2 seconds in the reference.
- **Data:** `M` is cpd 2's `concurrency` (3) against cpd 1's (10).

**(c) PlanView tabs.** Three real differences in the current source, plus one from the stale binary:
1. **Breadcrumb:** cpd 2 shows the Context **title** (`plan/mod.rs:77`: ` Building cpd: the CPD workflow tool ▸ `); the reference shows the short Context name (` cpd ▸ `).
2. **Right side:** cpd 2 adds a PR line (`plan/mod.rs:170–171`, drawn by `plan/tabs.rs` `draw`'s `right` parameter); the reference row holds only the name and the tabs (`tui/src/tabs.rs:24–46`).
3. **The stale binary also adds** `workers N/M` at the right; the current source has already dropped it.
4. **Tab set:** the reference has one tab per Lead *session* in the Context, labelled by pi name; cpd 2 has one per Plan Lead (`plan/mod.rs:61` `tab_label`). The tabs' styling is identical: active reversed and bold, ` label ` padding, one space between, separator in colour 8.

## 1. Parity checklist

Status values: **same**, **differs**, **missing**, **extra**.

### 1.1 Dispatch (reference `dispatch` / cpd 2 `widgets`)

| Element | Reference | cpd 2 | Status | Exact change |
|---|---|---|---|---|
| Section order | `main.rs:852–885`: homeserver, sep, PRs (`Min`), sep, git, sep, Jira | `widgets/column.rs:209` `draw`: Engine, Slots, sep, PRs, sep, Escalations, sep, Events | differs | Replace `column.rs` with the reference layout: `Layout::vertical([Length(2), sep, Min(1), sep, Length(2), sep, Length(2)])` |
| homeserver section | `main.rs:866`: ` homeserver` bold; ` load a b c` from `/proc/loadavg` (`:890`) | missing | missing | The title is the machine's hostname (` homeserver` on this machine); the load is the first 3 fields of `/proc/loadavg` (macOS: `sysctl -n vm.loadavg`) |
| Engine lines | none | `column.rs:88–99` | extra | remove |
| Slots lines | none (workers are in the StatusBar) | `column.rs:101–124` | extra | remove |
| Escalations lines | none | `column.rs:167–184` | extra | remove |
| Events log | none | `column.rs:186–199` | extra | remove |
| Footer hint ` ? legend   q quit` | none | `column.rs:23` `HINT` | extra | remove |
| ` PRs` title | `main.rs:867` bold | `column.rs:128` bold | same | — |
| A Plan's PR, line 1 | ` <label>` bold, where the label is the Lead session's name (as the tab) | ` <Plan title>` bold, cut with `…` | differs | Use the Lead session's name, as the tab label |
| A Plan's PR, line 2 | `  #N <state> <checks> <review decision>  <title dim, cut first>` (capture: `#2 merged ✓ no review  cpd-tui cockpit c…`) | `  #N <state> <checks> N reviews  N comments` | differs | Show the review decision (`approved` / `changes requested` / `no review`) and the PR title dim, cut first; drop the counts. Needs PR title and review decision in the store (§2) |
| State and checks colours | `pr_state`, `checks` (`palette.rs:154–172`) | `palette.rs:170–187` | same | — |
| A Plan without a PR | ` <label>` bold + `  no PR` dim on one line; listed after the Plans with PRs, only while there's room (`main.rs:870–879`) | `column.rs:127` (`with_idle`) | differs | Copy the order and room rule exactly |
| Empty | ` no Plans in the cockpit` dim (`main.rs:880–882`) | verify | verify | Same text |
| Which Plans | the Workflows of the chat sessions in the cockpit | Plans from `read::widgets` | differs | Plans whose Lead Session has a pane in the cockpit (chat slot or parked) |
| git section | ` git` bold / ` (future)` dim (`main.rs:883`) | missing | missing | add |
| Jira section | ` Jira` bold / ` (future)` dim (`main.rs:884`) | missing | missing | add |
| Separators | `separator::draw`, `Borders::TOP`, colour 8 | `column.rs:201` | same | — |
| Keys | `q` quits (`main.rs:~905`) | `q` (`widgets/mod.rs:81`), `?` legend (`:82`) | extra | Remove `?` (in the reference it's on the Context column and the StatusBar) |
| Refresh | every 2 s; PRs via `gh` with a 60 s cache | Events and changes of the pull request rows, coalesced to 500 ms, no timer | differs (source) | Keep the Event-driven refresh (data from the store); add the load every 2 s |
| Border title | ` Dispatch ` (`tmux.conf:11`) | ` Dispatch ` (`cockpit.rs` `BORDER_FORMAT`) | same | — |
| Reference test to port | `main.rs:1053–1062` exact lines | — | — | Port as a snapshot test |

### 1.2 PlanView (`plan`) and tabs

| Element | Reference | cpd 2 | Status | Exact change |
|---|---|---|---|---|
| Top row | ` <context> ▸ ` dim, then tabs (`tabs.rs:24–46`) | same, plus the `right` part (`plan/tabs.rs` `draw`) | extra | Remove the `right` parameter and its drawing; drop `right_parts` (`plan/mod.rs:170`) |
| Context label | the Context's name (` cpd `) | the Context **title** (`plan/mod.rs:77`) | differs | Use the Context **id** (` building-cpd `), consistent with the tree root |
| Tabs | one per Lead session of the Context; label: pi name, else Workflow id, else 8 characters of the id | one per Plan Lead (`plan/mod.rs:61`) | differs | One per Lead Session of the Context with a pane in the cockpit (Plan Leads and side forks); label: the Session name, else the Plan id, else 8 characters |
| Active tab | reversed and bold, the only marker | same | same | — |
| No centre | ` no Lead session in the centre` dim | same (`plan/tabs.rs`) | same | — |
| Separator under the tabs | `Borders::TOP` block, colour 8 | same | same | — |
| Chart area | `body.inner(Margin{2,1})` (`main.rs:710`) | same (`plan/mod.rs:152`) | same | — |
| Messages | `no Workflow yet`, `loading… (first fetch)`, the note (`main.rs:661–665`) | `message()` (`plan/mod.rs:91`) | differs (text) | `no Plan yet` / note; keep the one-row message area at the bottom of the window (`main.rs:712`) |
| Flow layout | `flow.rs` (cards, buses, brackets, transitive reduction, column width, `aN`) | `plan/flow.rs` port | verify | The captured chart starts one row lower in the reference: verify against `tui/tests/fixtures/cpd2-jobs.json` / `cpd2-chart.txt` Superseded (pro-d62, pro-d63, 2026-10-26 EDT): the label is now `#N`, before the tier. |
| Pill style | the status colour inverted (palette::pill; captured as `ESC[7m` on `ESC[38;5;8m`) | same | — |
| Selection keys | ←→/hl, ↑↓/kj, Enter, Esc, clicks (`main.rs:743–744`) | same (`plan/mod.rs:256–267`) | same | — Superseded (pro-d62, pro-d63, 2026-10-26 EDT): removed; a Job pill is neither clickable nor selectable, so there is no selection and no keys. |
| `q` | not handled | quits (`plan/mod.rs:264`) | extra | remove (a dead layout pane stays dead in cpd 2) |
| Wheel | 3 rows / 4 columns (`main.rs:773–776`) | same (`plan/mod.rs:298–301`) | same | — |
| Tab click | switches the centre and focuses the chat | opens that Lead in the chat slot | same (verify the focus) | `select-pane` on the chat after `session open` |
| Border title | ` Plan ` | ` Plan ` | same | — |

### 1.3 Context column (`context` / `tree`)

| Element | Reference | cpd 2 | Status | Exact change |
|---|---|---|---|---|
| Row format | guide, dot, name, right-aligned counters `q⋅r⋅m⋅f⋅d` (README "Context column") | same (capture) | same | — |
| Context row | dot (busiest phase) + name bold; Plan count when collapsed | `tree/rows.rs:201–205` | same (verify the count) | — |
| `⏵` on the centre session | bold, column 0 | same | same | — |
| Name of the centre row | bold | bold | same | — |
| Dot colours | `palette::phase` (`palette.rs:123–131`) | `palette.rs:133–141` | same | — |
| ● needs you | `@asking`, or idle with `@settled_at` after `@seen_at` (`main.rs:239–246`, `:349`) | `State::Blocked` only (`tree/rows.rs:251`) | differs | ● = blocked, or (idle, not in the chat slot, and its latest `session-state → idle` is newer than its latest `session-moved {place: chat}`), derived from Events, no writes |
| Cursor | keyboard only; inverted full row while focused, after an arrow; Context rows not selectable | `tree/view.rs`, focus at `tree/mod.rs:104–108` | verify | Match README "Cursor" exactly |
| Keys: ↑↓/jk, Enter | same | `tree/view.rs:114`, `:153` | same | — |
| `n` | fork the selected session into a new Plan (prompt for a name) (`main.rs:445`) | `plan new` prompt (`tree/view.rs:133`) | differs (mechanism) | Keep the key and prompt text; the mechanism stays `plan new` (§4) |
| `x` | close the session's pane after `Close <label>? pi stops; the session file stays (y/N)`; refused for the centre (`main.rs:456`) | missing | missing | Add with the same prompt, running the new `cpd2 session close` (§2) |
| `?` | legend popup (`main.rs:451`) | missing | missing | add |
| `q`, `f`, `r`, `p`, Space | none | `tree/view.rs:113`, `:137`, `:141`, `:145`, `:149` | extra | Remove from the TUI (they stay CLI commands) |
| Clicks | a session switches and focuses; a Context row collapses or expands; prompts block clicks | verify | verify | Match README |
| Inline prompt | `prompt.rs`, one row at the bottom | `tree/prompt.rs` | verify text | Copy the prompt texts exactly |
| Border title | ` Context ` | ` Context ` | same | — |

### 1.4 StatusBar

| Element | Reference | cpd 2 | Status | Exact change |
|---|---|---|---|---|
| Pane | `split-window -v -f -l 1` (`cockpit.sh:20`) | `Split::Bottom` = `-v -f` (`tmux.rs:169`) | same | — |
| Items and drawing | `statusbar.rs` | `statusbar/bar.rs` (port) | same | — |
| Workers room | `workers.line(width)` (`main.rs:~560`) | `workers.line(room)` (`statusbar/mod.rs` `items`) | differs | Pass the full width, as the reference |
| Refresh | 2 s | 200 ms tick, 500 ms coalescing, 2 s floor (`statusbar/mod.rs:41–45`) | differs (source) | keep |
| Click without focus | `MouseDown1Pane` binding (`tmux.conf:17`) | `cockpit.rs` `TUI_OPTIONS`, only after `cpd2 tui` | differs | Set at server start (§1.8) |
| A missing bar | — | added without a reset (`cockpit.rs` `add_missing_statusbar`) | differs | Reset after adding it |

### 1.5 Worker view

Superseded (pro-d62, pro-d63, 2026-10-26 EDT): the Worker view and its pane are removed; the table below is history.

| Element | Reference | cpd 2 | Status | Exact change |
|---|---|---|---|---|
| Opening | `split-window -h -d -l 50%` beside the chat; `@view worker`, `@job`; retarget with `respawn-pane -k`; the same Job closes it (`main.rs:603–640`) | `Split::Right 50%` (`plan/actions.rs:71`), `@cpd_attempt` (`:18`) | differs | Also set `@cpd_job <job>`; check `-d` (no focus) and the toggle/retarget behaviour |
| Border title | ` Worker · <job> (read-only) ` (`tmux.conf:11`) | ` Worker (read-only) ` | differs | `BORDER_FORMAT`: ` Worker · #{@cpd_job} (read-only) ` |
| Rendering | `you` and its text cyan, 12 lines; `agent` green; `→ tool` yellow; `← last` dim; 160 characters (`main.rs:~915–957`) | `worker/render.rs:37–56`, `you` white | differs | `you` cyan: add the `Ui` palette (§1.7) |
| Keys | `q`/Esc, ↑/k, ↓/j, PageUp, PageDown, End/G, wheel 3 rows (`run_worker`, `main.rs`) | the same (`worker/view.rs`) | same | — |
| Follow | stays at the end until the User scrolls; re-reads every 2 s | every 500 ms on size or mtime | same / differs (source) | keep |

### 1.6 Legend popup

| Element | Reference | cpd 2 | Status | Exact change |
|---|---|---|---|---|
| Popup | `display-popup -E -T ' legend ' -w/-h`, capped at the client (`legend.rs:144–147`) | `widgets/legend.rs:191` | same | — |
| Intro | `Legend: … (any key closes)` (`legend.rs:26`) | same (`widgets/legend.rs:27`) | same | — |
| Section titles | "Jobs: Plan pills and Context counters"; "Context dots: … Workflow's phase" (`legend.rs:94–96`) | "…tree counters"; "Tree dots: … Plan's phase" (`widgets/legend.rs:122–124`) | differs | "Context counters", "Context dots"; keep "Plan's phase" (§4) |
| Job statuses listed | queued…unknown (`palette.rs:~120`) | adds `cancelled ⊘` (`palette.rs:108`) | extra | Drop `cancelled` from the legend (the chart hides cancelled Jobs) |
| Opened by | the StatusBar button and `?` in Context | the StatusBar button and `?` in widgets | differs | `?` moves to the tree |

### 1.7 Palette

| Element | Reference | cpd 2 | Status | Exact change |
|---|---|---|---|---|
| `Meaning` colours | `palette.rs:61–70` | `palette.rs:70–79` | same | — |
| `Ui` (you cyan, agent green, tool and note yellow) | `palette.rs:97–101` | missing | missing | Add `Ui` and `ui()` to `cli/src/tui/palette.rs` |
| Phase legend labels | `no Workflow`, `context` | `no Plan`, `grounding` | differs | keep (§4) |

### 1.8 Layout, sizes, borders and tmux

| Element | Reference | cpd 2 | Status | Exact change |
|---|---|---|---|---|
| Window | `new-session … -x 215 -y 62` (`cockpit.sh:16`) | `SESSION_WIDTH/HEIGHT` 215/62 (`tmux.rs:22–23`) | same | — |
| Split topology | context \| [plan / chat] \| dispatch, plus the bar (`cockpit.sh:17–20`) | the same tree (`cockpit.rs` `build`) | same | — |
| Reset order and sizes | statusbar 1, context 20%, dispatch 20%, plan 20%, worker 30% (`layout.rs:6–31`) | identical (`statusbar/layout.rs:13–19`) | same | — |
| Server options, part 1 | `tmux.conf:2–13` | `SERVER_OPTIONS` (`tmux.rs:33–42`) | same | — |
| Server options, part 2 | `focus-events on`, `pane-active-border-style fg=blue`, `pane-border-format`, `MouseDown1Pane` | only via `cpd2 tui` (`cockpit.rs` `TUI_OPTIONS`) | differs | Move them into `SERVER_OPTIONS`, so every server start (`plan new`, `watch --ensure`, `tui`) looks the same |
| `pane-died` respawn hook | global `respawn-pane` (`tmux.conf:14`) | none, by design | differs | `set-hook -g pane-died 'if -F "#{@cpd_view}" "respawn-pane"'`: only view panes respawn; Lead and engine panes stay dead for E2 |
| Border format | uses `@session_id`, `@view`, `@job` | uses `@cpd_session`, `@cpd_view` (worker title without the job) | differs | Add `#{@cpd_job}` |
| A new view binary | the panes keep the old binary | same | differs (ops) | `cpd2 tui --respawn`: `respawn-pane -k` on every live layout view; `install-cpd2.sh` runs it when a cockpit exists |
| Chat placeholder | none | `chat` view | extra | keep (§4) |

## 2. Contract changes, and data sources

**The contract changes.**
- **§6.1:** add `focus-events on`, `pane-active-border-style fg=blue`, the `pane-border-format` (titles: Context, Plan, Dispatch, ` Worker · <job> (read-only) `; none for Leads, the chat placeholder and the StatusBar), the root `MouseDown1Pane` binding, and the conditional `pane-died` hook. All are set at every server start.
- **§6.2:** a Worker pane carries `@cpd_view worker`, `@cpd_job`, `@cpd_attempt`. The sizes stay the reference's.
  Superseded (pro-d62, pro-d63, 2026-10-26 EDT): the Worker pane and its `@cpd_view worker`, `@cpd_job` and `@cpd_attempt` are removed.
- **§6.3:** add `respawn-pane -k` (Worker retarget; `tui --respawn`) and `set-hook`.
  Superseded (pro-d62, pro-d63, 2026-10-26 EDT): `respawn-pane -k` no longer retargets a Worker view; `tui --respawn` and `set-hook` stay.
- **§10 (new):** `cpd2 session close <id>`: kill the Lead's pane (pi stops, the session file stays), `stopped`, `session-moved {place: none}`. Refuses the Session in the chat slot.
- **§13.1:** `tui --respawn`; a StatusBar added to an existing `main` is followed by the reset.
- **§13.2, keys per view:**
  - tree: ↑↓/jk, Enter, `n`, `x`, `?`;
  - plan: ←→↑↓/hjkl, Enter, Esc, wheel, clicks;
    Superseded (pro-d62, pro-d63, 2026-10-26 EDT): now wheel and the remaining clicks only.
  - widgets: `q`;
  - worker: `q`/Esc, ↑↓/jk, PageUp/PageDown, End/G, wheel;
  - statusbar: clicks;
  - legend: any key.
- **§13.2, drawing:** the plan view's top row is only `<context id> ▸` and the tabs. The Dispatch sections are hostname with load, PRs, git (future), Jira (future).
- **§17:** the widgets row reads the cockpit's Plans with their PR (number, state, checks, review decision, title) plus the host load. No engine, slots, Escalations or Events. Its Events are `plan-created`, `plan-phase`, `pr-feedback`, `plan-done`.
- **§17:** the plan row drops "open Escalations, the latest Plan review" (not drawn).
- **§17:** replace "each view runs `cpd2` as a subprocess" with the in-process read model, as implemented.
- **§17:** the tree's ● rule, as in §1.3.
- **§7.6 / §9.2:** `pr poll` stores the PR's `title`, `review_decision` and checks summary, in a migration 2, since a real `cpd.db` now exists.

| Item | cpd 1 reads | cpd 2 reads |
|---|---|---|
| Sessions in the tree | tmux pane options (`@session_*`) and the workflow cache | the store (`sessions`, `plans`, `contexts`) |
| ● needs you | `@asking`, `@settled_at`, `@seen_at` | `session-state` and `session-moved` Events |
| Job counters and chart | the cached `cpd workflow show` | the store |
| PR state | `gh pr view` every 60 s per PR, cached | the store, written by `pr poll` (E5) |
| `workers N/M` | `slots/*.json` with live pids, and `config.json` | Attempts not ended, and `config.toml` `concurrency` |
| homeserver load | `/proc/loadavg` | `/proc/loadavg` (macOS: `sysctl vm.loadavg`) |

## 3. Job breakdown

Each Job fits 90 minutes. Each verifies with `scripts/cargo.sh test --workspace` and `npm run typecheck`, and commits every 20 minutes. Shared files have exactly one owner:
- `cli/src/tui/mod.rs`: `parity-tmux`;
- `cli/tests/command_index.rs`: `session-close`;
- `cli/src/tui/read/`: `parity-pr-data` and `parity-needs-you` (different files);
- `cli/src/tui/palette.rs`: `parity-palette-worker`.

| Key | Title | Tier | Needs | Files | Acceptance |
|---|---|---|---|---|---|
| `parity-contract` | Amend the contract for cockpit parity | S3 | — | `docs/contract.md`, `docs/commands.md` | Every change of §2 is stated; section numbers unchanged |
| `parity-tmux` | Server options at every start, conditional respawn hook, Worker title, `tui --respawn` | O3 | contract | `cli/src/host/tmux.rs`, `cli/src/tui/cockpit.rs`, `cli/src/tui/mod.rs`, `scripts/install-cpd2.sh`, `cli/tests/host.rs`, `cli/tests/tui.rs` | On a private socket: a server started by `plan new` has every option of `tmux.conf:2–17` (renamed); a killed view pane respawns, a killed Lead or engine pane doesn't; `tui --respawn` restarts views only; a bar added to an old `main` is followed by the reset |
| `session-close` | `cpd2 session close` | S3 | contract | `cli/src/session/`, `cli/tests/session.rs`, `cli/tests/command_index.rs` | §10 command with its refusals; integration test |
| `parity-pr-data` | `pr poll` stores PR title, review decision and checks; read model for Dispatch | S3 | contract | `cli/src/pr/`, `cli/src/store/` (migration 2), `cli/src/tui/read/widgets.rs`, `cli/tests/pr.rs` | Fake `gh`; the migration from version 1 is tested; Dispatch rows carry label, number, state, checks, decision, title |
| `parity-needs-you` | The ● rule from Events | S3 | contract | `cli/src/tui/read/shared.rs`, `cli/src/tui/read/tree.rs`, `cli/tests/tui_read.rs` | Table test: blocked; idle and unseen; idle and seen; in the slot |
| `parity-dispatch` | The Dispatch column as the reference | S3 | `parity-pr-data` | `cli/src/tui/widgets/column.rs`, `cli/src/tui/widgets/mod.rs`, `cli/tests/tui_widgets.rs` | The ported snapshot of `main.rs:1053–1062` matches exactly; the PR rows match the capture's format; `q` only |
| `parity-planview` | The tab row as the reference; Worker pane `@cpd_job` | S3 | contract | `cli/src/tui/plan/{mod,tabs,actions}.rs`, `cli/tests/tui_plan.rs` | The reference `tabs.rs` tests ported verbatim pass; `cpd2-jobs.json` reproduces `cpd2-chart.txt`; no `q` |
| `parity-tree` | Keys, prompts, clicks and cursor as the reference | O3 | `session-close`, `parity-needs-you` | `cli/src/tui/tree/*`, `cli/tests/tui_tree.rs` | Snapshot of the README "Context column" example; key table exact (`n`, `x`, `?`, Enter, ↑↓/jk); prompt texts exact; click rules |
| `parity-palette-worker` | `Ui` palette, Worker colours and keys, legend text, StatusBar room | S3 | contract | `cli/src/tui/palette.rs`, `cli/src/tui/worker/*`, `cli/src/tui/widgets/legend.rs`, `cli/src/tui/statusbar/mod.rs`, `cli/tests/tui_worker.rs` | `you` cyan; Worker render snapshot against a fixture session file equals the reference `render_session` output; legend titles exact, no `cancelled`; StatusBar snapshot equals `main.rs:1028`'s |

Separately, outside parity: a bug Job for the engine's repeated "failed at E2 (liveness)" error, every 5 seconds before its restart at 17:39:51 in the capture.

## 4. Not to copy from the reference

| Reference | Why not |
|---|---|
| Data sources: slot files, `config.json`, workflow caches, `/tmp/cpd-tui`, view-run `gh` with cache files, tmux options carrying state | cpd 2 reads its store (R1, D4) |
| The global `pane-died respawn-pane` hook | It would respawn Lead and engine panes, whose death E2 must see; use the `@cpd_view`-conditional hook |
| `cockpit.sh` (hard-coded `PLANS`/`FIRST`), `resume-lead.sh`, `fork-lead.sh`, `publish-session.sh`, `tmux-agent-state.ts` | Replaced by `cpd2 tui`, `plan new`, `session fork/open/run` and the adapter's `session state` |
| `n` forking the *selected* session | cpd 2's new Plan forks the Context's grounded Lead (R15); same key, cpd 2's mechanism |
| "Workflow" wording (legend, `no Workflow yet`, phase `context`) | cpd 2 has Plans; phase `grounding` |
| A literal ` homeserver` | Use the hostname: identical here, correct on the Macs |
| The tmux config file (`-f tmux.conf`) | cpd 2 sets options from `host` (one source) |
| No chat placeholder | cpd 2's cockpit can have no Lead; keep `chat` |

END OF AUDIT
