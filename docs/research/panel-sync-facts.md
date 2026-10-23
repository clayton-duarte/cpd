# Panel sync: the derived facts shown on more than one surface

Job `facts`, Plan `sync`, step 1 (MEASURE). A code survey of origin/main at `c2953181`; no cockpit, engine or test was run. File:line
references are to that commit. **No latency or CPU number was measured here**: every timing below is read from config or code and is
labelled `unverified (config, not measured)`. Divergences marked *verified* were found by reading both derivations (and the quoted lines).

## 1. How each surface learns of a change

| Surface | Mechanism (read in code) | Delay after a store write |
|---|---|---|
| tree view | `events` poll every `events.poll_ms` = 200 (`core/src/config/file.rs:365`), a `pull_requests` mark, `engine_running` flock probe each poll (`tui/src/tree/mod.rs:214`); snapshot at most every 500 ms (`SNAPSHOT_EVERY`, `tree/mod.rs:80`) | up to ~0.7 s + frame boundary (100 ms). unverified (config, not measured) |
| plan view | same follower; `COALESCE` 500 ms (`tui/src/plan/mod.rs:57`); Pulse 60 s (`:67`) | same |
| widgets (Dispatch) column | same follower; `COALESCE` 500 ms (`tui/src/widgets/mod.rs:58`); reads only `open_prs` (`:72`, `:229`) | same |
| Inbox section (in the tree) | items are **stored** rows written by engine E8 (`engine/src/watch/inbox.rs`); the tree reads them in its snapshot | engine tick `watch.tick_seconds` = 5 (`core/src/config/file.rs:335`) + the view's ~0.7 s. unverified (config, not measured) |
| tmux status line (`workers N/M`) | engine step `workers` (`engine/src/watch/workers.rs:78`) sets `@cpd_workers*` only when the text changed; tmux formats re-read options at redraw (`core/src/host/tmux.rs:311`) | engine tick (5 s) + tmux redraw. unverified (config, not measured) |
| tmux pane titles | views: static text by `@cpd_view` (`tmux.rs:106`); Lead chat panes: `@cpd_title` = `sessions.title`, set by a retitle run, at most once per 10 min per Session (`engine/src/watch/titles.rs:27`, `core/src/plan/lead.rs:119`) | static / minutes |
| pi footer and messages | no live panel: the footer carries only an empty pad (`pi/lib/adapter.ts:34`); facts arrive as cpd messages from the follower and the Manager's `context pending` digest (`pi/lib/updates.ts:226-236`) | the pi follower's own Event delivery; the digest is a snapshot at send time |

Pull-request rows: `pr poll` writes `pull_requests` with no Event (contract §7.6); the views notice through the `count(*)`/`max(changed_at)`
mark (`tui/src/read/follow.rs:37`), the engine PR poll is every `watch.pr_poll_seconds` = 5 (`file.rs:336`).

## 2. The facts

Legend: **1 fn** = one derivation shared by the surfaces named; **N fns** = derived separately.

| # | Fact | Surfaces | Where each derives it | 1 or N | Observed divergence (verified unless marked) | Learns of change |
|---|---|---|---|---|---|---|
| F1 | Plan counters `3⋅1⋅0⋅0⋅92` (queued, running, merging, failed, merged) | tree Plan row; legend sample; pi `Pending work:` | tree/legend: `plan_heads` (`tui/src/read/shared.rs:318-343`) then `JobCounts::counters` (`:130`), drawn by `tree/rows.rs:266`; pi: `pendingSection` counts a `status` string over `context pending` Jobs (`pi/lib/updates.ts:232-235`) | tree and legend share; pi is separate | pi lists `["running","queued","failed","draft","merged"]` (`updates.ts:232`); the tree has `merging` as its own counter, from `j.status = 'running' AND EXISTS (... a.status = 'merging')` (`shared.rs:324`). In pi a merging Job is `running`. Also `context pending` rewrites a blocked queued Job to `waiting` (`core/src/context/pending.rs:127`), which pi's list never counts, so pi's `queued` is smaller than the tree's `queued` (`shared.rs:333`: stored `queued`, blocked ones included) and its `waiting` Jobs appear nowhere. Per PR in pi, per Plan in the tree | view: ~0.7 s; pi: when the digest is sent |
| F2 | Lead dot (working / waiting / idle / stopped / needs-you colour / red loader) | tree rows, Context row, Pulse rows; plan view PR box title; widgets PR row | tree + Pulse: `fill_dots` (`read/tree.rs:122`) → `Dot::of` (`read/dot.rs:57`); plan box: `palette::dot_span(Dot::Working…)` when `lead_working` (`tui/src/plan/boxes.rs:137`); widgets row: same (`tui/src/widgets/column.rs:175`) | tree+Pulse 1 fn; plan box and widgets row N (each a `s.state = 'working'` bool: `read/flow.rs:271`, `read/widgets.rs:219`) | the tree shows the **red loader** `Dot::Fixing` for a working Lead whose Plan has a failed Job (`dot.rs:57-65`, hs-d581); the box title and the Dispatch row show `Dot::Working` in that case (`boxes.rs:137`, `column.rs:175`). A pending Inbox item also changes the tree dot but never the two others | view poll; the item part waits for the engine tick |
| F3 | Failed Job colour / loader (red vs waiting vs turning) | tree counters; plan view Job pills; tree dot | counters: `count_style` always red for `failed` (`tui/src/palette.rs:465-471`); pills: `Live::of(job, lead_working)` → `FailedWorked` (`tui/src/plan/flow.rs:73-80`); dot: `Dot::of(..., failed)` | N (three rules, one input `lead_working` / `counts.failed`) | the tree counter stays red while the Lead works; the plan pill turns (loader). Same rule intent (hs-d586, hs-d581), written three times | view poll |
| F4 | PR state word and colour (`open` / `draft` / `conflict` / `updating` / `closed` / `merged`) | plan view PR box title; widgets PR row; legend | plan: `state_word` (`tui/src/plan/boxes.rs:107-117`); widgets: `pr_lines` (`tui/src/widgets/column.rs:154-190`); both read palette consts `PR_CONFLICT*`, `PR_UPDATING_*` and `pr_open` | N (two matches over the same inputs; palette holds only the constants) | different **placement**: the plan box *replaces* the state word (`PrState::Open if pull.conflicting => match pull.update {… None => palette::PR_CONFLICT_OWNED}`, `boxes.rs:108-112`); the widgets row *keeps* `open`/`draft` and **appends** ` conflict` (`column.rs:166-169,190`). Only widgets knows `draft` (`column.rs:178`: `if pr.draft {"draft"}`); `Pull` (`read/flow.rs:140-150`) has no draft field. Plan box adds `closed` red when `asked` (`boxes.rs:113`); widgets rows are `state = 'open'` only (`widgets.rs:224`) | view poll; PR rows have no Event, so the pr-mark, then 500 ms |
| F5 | PR update Job (`updating`) | plan box, widgets row | `read/flow.rs:303` and `read/widgets.rs:222` | N: the same `(SELECT j.status FROM jobs j … j.kind = 'update' AND j.status IN ('queued','running') ORDER BY j.status='running' DESC LIMIT 1)` pasted twice, with the same `update_state` parser (`flow.rs:152`) | no value divergence found today; a copy-drift risk. *unverified* beyond reading both SQLs | view poll |
| F6 | PR checks (icon, colour) | widgets row; legend; tree Plan head carries `checks` | widgets: `palette::checks_of(pr.checks, owned)` (`column.rs:164`); legend: `palette::checks` (`legend.rs:205`); plan box: **none** (the box draws no checks) | 1 fn in `palette.rs:511-526`, but the plan box does not show it | failed checks are `Waiting` colour when the PR is owned, red when not (`palette.rs:511-515`); a PR box the Plan owns shows nothing, so the same PR reads "failed" on the Dispatch column and silent on the plan view | view poll |
| F7 | "PR is ready to merge" | widgets row (yellow `open`, merge icon); plan box title (yellow `open`); Inbox `pr-ready` item; tree click target | views: `merge_state = 'clean'` → `clean` (`read/flow.rs:301`, `read/widgets.rs:211`) → `palette::pr_open(clean)` (`palette.rs:485`); engine: `pr_ready` SQL `r.authored = 1 AND r.draft = 0 AND r.merge_state IN ('clean') AND r.review_decision IN ('approved','none')` (`core/src/inbox/rules.rs:424-432`) | N (the views ignore `draft`-on-plan-box, `authored` and `review_decision`; the engine ignores nothing the views know) | views: `clean` alone turns `open` yellow (`palette.rs:485`: `if clean {Review}`); engine: also needs approved-or-none review. A PR with `changes_requested` and `clean` is yellow `open` in the views and has **no** Inbox item. Views learn at the pr-mark; the item appears after the next E8 tick | views ~0.7 s; Inbox: engine tick |
| F8 | Job pill state (draft / queued / waiting / running / merging / failed / merged) | plan view pills; tree counters; pi digest; engine `workers` | plan: `JobNode.status` + `waiting_on` from `Display::job_blockers` (`read/flow.rs:369-380`, `core/src/plan/display.rs:84`); tree: stored status only (`shared.rs:333-343`); pi: CLI rewrites to `waiting` (`pending.rs:127`); engine: `READY` SQL (`engine/src/watch/schedule.rs:62-75`) | N: three blocker rules (`Display::job_blockers`, `READY`, the stored status) | `waiting` means opposite things: engine `W waiting` = *Ready* Jobs that **can** start (`schedule.rs:61`, `workers.rs:54`); core/plan view `waiting` = a queued Job that **cannot** start (`pending.rs:66`). Whether `READY` and `Display::job_blockers` agree for every case is *unverified* (not compared exhaustively); the engine's `READY` also adds `p.phase IN ('dispatch','deploy')` and `kind` clauses | plan view ~0.7 s; engine tick |
| F9 | `workers N/M` slots held | tmux status line only (pane text); `read::slots`/`Slots::used` | engine: `HELD` (`engine/src/watch/workers.rs:32`); scheduler: `schedule.rs:89`; view model: `held_slots` (`read/widgets.rs:302-305`) | N: three copies of `SELECT … FROM attempts WHERE status <> 'ended'` | no divergence today (the doc comment `workers.rs:31` says "the same query"; nothing enforces it). `read::widgets`/`slots`/`engine` have **no live caller** in `tui/src` or `cli/src` (only `cli/tests/tui/tui_widgets.rs:210…` call `widgets`): the Dispatch column reads only `open_prs` | engine tick + tmux redraw |
| F10 | Engine running | tree (red `engine-down` item, dot); `read::engine`; tmux (nothing) | tree: `model::engine_running` flock probe on `locks/watch.lock` (`core/src/inbox/model.rs:256-270`), polled (`tree/mod.rs:214`); model: last `watch-started`/`watch-stopped` Event + `kill(pid,0)` (`read/widgets.rs:329-346`) | N (two definitions of "running"), only the first used live | the Event+pid rule says `Running` for a live pid that lost the lock, the flock rule says down; and a crashed engine leaves `@cpd_workers` at its last value in tmux (nothing clears it, `workers.rs:95-100` clears only the remembered state when the *server* is gone): *unverified* by a run | tree: next poll; tmux: never refreshed while the engine is down |
| F11 | Age / duration text | tree Inbox rows; Pulse `quiet <age>` cells; Logs clock | `tree/inbox.rs:159-165`: `s<60→"Ns", <3600→"Nm", <86400→"Nh", else "Nd"`; `plan/pulse.rs:56-65`: `m<60→"Nm", <48*60→"Nh", else "Nd"` | N | same 30-hour-old thing: tree `1d` (`s / 86400`), Pulse `30h` (`minutes < 48 * 60`); tree has seconds, Pulse has none | tree: ticks with its snapshot; Pulse: 60 s |
| F12 | Inbox: item set, count, urgency colour, dot source | tree Inbox section + dots; plan view `NeedsUser`; Pulse rows | stored rows from E8 (`rules.rs`, `engine/src/watch/inbox.rs`), read by `visible_items` (`read/tree.rs:115`); dot: `dot::urgency` (`read/dot.rs:87-96`); plan view: `NeedsUser::of` (`read/flow.rs:72-82`) | rows stored once (1 source); the view-side filters differ: `dot::urgency` (kind lists, `PLAN_KINDS`) vs `NeedsUser::of` (`Dialog` by session, `JobStuck` by plan) | `NeedsUser::stuck_jobs` still reads `JobStuck` although the rules say there is no `job-stuck` Inbox condition (`rules.rs:5-6`, `stuck_jobs` goes to the Manager); the field is documented dead (`flow.rs:63`). Item appears/disappears only after an engine tick; tree and plan view can show it at different 500 ms snapshots | engine tick + ~0.7 s |
| F13 | Lead's name / title | tree row label; widgets PR row `lead`; tmux pane title | tree: `session_label` (`tui/src/tree/rows.rs:260`) from `sessions.name`; tmux: `@cpd_title` from `sessions.title` (`core/src/plan/lead.rs:119-126`, `host/tmux.rs:104`) | N (two stored fields: `name`, `title`) | the tab label in the tree and the pane title of the same Lead come from different columns, refreshed at different times (title at most every 10 min, `titles.rs:27`). *unverified* whether they usually differ in practice | tree: view poll; pane: minutes |
| F14 | Plan phase / PR status (`plan`, `dispatch`, `deploy`; `waiting`, `dispatch`, `open`, `merged`) | tree counters (first counter = drafts in phase `plan`, else queued: `shared.rs:130-133`); plan view; pi digest `(phase)` | stored columns, so one source; the *counter's meaning* switches by phase in the tree only (`counters`), pi prints `draft` and `queued` as separate words (`updates.ts:232`) | stored (1), rendering N | the first tree counter is `draft` in phase `plan` and `queued` after, while pi always labels each | view poll |

Not facts of this table (stored or one-surface): Inbox `subject`/`label` wording (stored by E8), the Pulse buckets, CPU/GPU/mem % (sampled by the widgets view).

## 3. `tui/src/read/` as it is

`read/mod.rs:1-27` is the contract: typed read-only snapshots, one `Store::read` transaction each, a fixed number of queries whatever the
Ledger size, rows parsed once (`shared.rs`: `bad_row`, `id`, `value_enum`, `string_enum!`), each snapshot returning `events_cursor`
for `Follow` (`follow.rs`: `events` poll on `PRAGMA data_version`, plus the `pull_requests` mark, `follow.rs:37`).

| File | Function(s) | Facts with **one** function there | Surfaces that do not use it |
|---|---|---|---|
| `dot.rs` | `Dot::of`, `urgency` | F2 for the tree rows (`fill_dots`, `tree.rs:122`, "every view that draws a Lead's dot (the tree, Pulse) uses this one rule") | plan box, Dispatch row (own `Dot::Working` by `lead_working`) |
| `shared.rs` | `plan_heads`, `JobCounts::counters`, `PrState::of`, `Checks::parse`, `SessionNode` | F1 counters, PR `state` mapping, checks parse | pi (F1); tmux has no counters |
| `tree.rs` | `tree`, `visible_items`, `fill_dots` | F12 items, dots | `read/flow.rs` re-filters the same items in `NeedsUser::of` |
| `flow.rs` | `plan_boxes`, `flow`, `pr_nodes`, `job_nodes`, `update_state`, `waiting_on` | `update_state` (parser only), Job `waiting_on` via core's `Display` | `widgets.rs` pastes the update-Job SQL (`widgets.rs:222`) |
| `widgets.rs` | `open_prs`, `open_pulls`, `slots`, `widgets`, `held_slots`, `engine`, `escalations` | the Dispatch column's PR row (F4, F6, F7 inputs); `held_slots` (F9) and `engine` (F10) | **the engine and tmux** (own SQL); `slots`, `engine`, `widgets` unused by any live view |
| `pulse.rs`, `worker.rs`, `logs.rs` | their views | Pulse activity; Attempt detail; Event log | — |

Everything in `read/` returns facts as **inputs** (booleans such as `lead_working`, `conflicting`, `clean`, enums such as `PrState`, `Checks`);
the **word and colour** are decided later in `tui/src/plan/boxes.rs`, `tui/src/widgets/column.rs`, `tui/src/palette.rs` (F4, F6). So
`read/` is the right layer for facts but today holds only the part of F1, F2 (tree) and the parsers.

### What the engine stores and what the views derive

| Stored by the engine (a row or tmux option written on a tick) | Derived by each view on read |
|---|---|
| Inbox items (E8: `engine/src/watch/inbox.rs`, rules in `core/src/inbox/rules.rs`): `pr-ready`, `plan-review`, `dialog`, `unread`, `review-waiting` | Plan counters (F1), the dot after items (F2), PR word/colour (F4-F6), Job pill, ages (F11), the engine-down item (`read/tree.rs:218`, built in the view, not stored) |
| `@cpd_workers`, `@cpd_workers_short`, `@cpd_workers_width`, `@cpd_workers_short_width` (`workers.rs:104`) | — (the views never draw `workers N/M`; they have `Slots` but no caller) |
| `sessions.title` / `@cpd_title` (a retitle run, at most per 10 min) | — |
| Job/Attempt/PR rows (written by commands and `pr poll`, with Events, except `pull_requests`) | all of the above |

**Does that split explain the disagreement?** Partly, and only for facts that cross it. Verified by reading: items and tmux options change on
the engine's tick (5 s config), while a view derives on its next snapshot (~0.7 s config). So an Inbox-driven fact (F12, the item part of
F2, F7's Inbox side) and `workers N/M` (F9) can lag the views by up to one engine tick, and an Inbox item can *disappear* from the tree
only after E8 runs although the PR row it came from already changed (F7). Two views that both derive on read (plan box and Dispatch
row, F2/F4/F11) also disagree **with no staleness at all**: the cause there is two derivations, not the store/derive split. unverified: I
did not measure how many seconds each disagreement lasts; that is the step-1 measurement Job.

## 4. Proposal (not a decision): functions for `tui/src/read/`

Ordered by how often the surfaces diverge today (count of surfaces that derive it separately, then F-number). "No extra query" means every
input is already a column the view's snapshot reads (named); "extra read" would add a query to a view.

| Order | Function (name) | Inputs (already in a snapshot) | Output | Callers after | Cost |
|---|---|---|---|---|---|
| 1 | `pr_status(pull, owned, asked) -> PrStatus { word, meaning, draft, conflicting, update, clean, checks }` for F4, F5, F6, F7 views side | `Pull` (`state`, `conflicting`, `clean`, `update`, `number`) and `OpenPr` (`draft`, `checks`, `review_decision`, `lead`, `update`); `pull_requests` columns both queries already select | one value the plan box and widgets row draw (placement stays theirs; word and colour decided once) | `plan/boxes.rs` `state_word`, `widgets/column.rs` `pr_lines`, legend | **no extra query**; `draft` must be added to `Pull` — it is already `q.draft` in the widgets SELECT, one more column in the plan view's `pr_nodes` SELECT (`flow.rs:297-306`), no extra read |
| 2 | `lead_dot(session_state, failed_job, item) -> Dot` over `Dot::of` plus a `plan_lead_dot(plan)` | `sessions.state`, the Plan's `counts.failed` (`plan_heads`, already read by the plan view's neighbours: **unverified** that `plan_boxes` has `failed`; it has `jobs`, so the count is derivable from `job_nodes`), visible items | `Dot` | tree, Pulse (today), plan box, Dispatch row | no extra query for the plan box (`jobs` read); the Dispatch row has `lead_working` only, so it needs `failed` per Plan: extra read **unless** `open_pr_rows` adds one correlated subquery over `jobs` |
| 3 | `job_counters(JobCounts) -> Counters` plus a counting rule shared with pi | `plan_heads` result | the five numbers | tree, legend; pi's digest via the CLI | no extra query in `read/`; pi is a different tree |
| 4 | `job_state(job, display_blockers) -> JobState` (one definition of `waiting`) | `jobs` rows + `dependencies` (`Display::load`, already in `job_nodes`) | enum incl. `Waiting(blockers)` | plan pills, tree counters, engine `READY`/`W waiting`, pi | no extra query in views; the engine's `READY` is SQL and would need the same rule in a shared core function — *design step*, since `core/` not `tui/` |
| 5 | `age_text(seconds) -> String` | `opened_at` etc. | one rule for `Ns/Nm/Nh/Nd` | tree Inbox, Pulse | no extra query |
| 6 | `slots_held(conn)` and `engine_state(home, conn)` in `read/` or `core/`, used by view model and engine | `attempts` (`status <> 'ended'`), the lock file | `u32`, `Engine` | engine `workers`, scheduler E3, `read::slots` | no extra query; today's three SELECTs become one definition |
| 7 | `pr_ready(conn_row) -> bool` | `pull_requests.draft, authored, merge_state, review_decision` (the widgets SELECT already reads all four, `widgets.rs:210-211`; the plan view reads `merge_state` only) | `bool` | views' yellow `open`, engine `pr_ready` condition | views: plan view adds `draft`, `review_decision` columns (extra columns, not extra query); engine SQL moves to the same predicate |

### What tmux and pi cannot call

- **tmux format strings** (`core/src/host/tmux.rs:104-312`) cannot call Rust. Minimal thing they could share: the **stored string** the
  engine already writes (`@cpd_workers`, `@cpd_workers_short`; the word rules sit in `Counts::long/short`, `workers.rs:58-65`) — the
  strings and their widths are generated by one Rust function today, so the cheapest sharing is for the views to *call the same
  `Counts`* if the Dispatch column ever draws the numbers. Choosing is the design step's.
- **pi (TypeScript, `pi/lib/updates.ts`)** reads the CLI's JSON (`cpd context pending`, `events list`), not the Rust model. Minimal share:
  put the derived word in the JSON the CLI already prints (for example a `display_status` that already exists as `status: "waiting"` in
  `pending.rs`, plus counts) so pi counts what the CLI computed instead of its own `["running","queued","failed","draft","merged"]` list; or
  a written spec of the counters. Not chosen here.

## 5. Not reached

- **Measured numbers**: none. The handoff's "a view sees a change within ~0.7 s; Inbox up to ~5 s later; CPU 0.1-0.4 % per view; 5-10
  messages/s" (`/tmp/cpd-building-cpd/tui-architecture-handoff.md` §3) is **neither confirmed nor corrected** by this survey: the
  structure (200 ms poll, 500 ms coalesce, 100 ms frame, 5 s engine tick) is confirmed by the lines in §1; the numbers are for the
  measurement Jobs.
- Surfaces: the worker view (`tui/src/worker/`), the Logs view, the dialog and history views, the Context row's records/questions
  counts, escalation counts (`read/widgets.rs:349`), Plan review state, `running 4 m` clocks (I found no such text in `tui/src/worker/`
  or `plan/flow.rs`; *unverified* that it exists), tmux pill texts besides `workers` (`pill_texts.rs` is a test), pi's chat-footer
  contents other than the empty pad and pi's `cpd-context` summary line (`summaryLine`, `countsOf`).
- Facts not compared line by line: the engine `READY` against `Display::job_blockers` (F8), `Pulse` kinds against Inbox kinds,
  Session `state` labels in pi `agent-state.ts` against `Dot::glyph`.
- Form (caps, spacing, colour palettes of pills across ratatui, tmux and pi) is another Plan; only F2, F4 and F6 mention colour because the
  *fact* (which meaning) differs.
