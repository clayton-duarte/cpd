# A web UI for cpd with cockpit parity: findings and open choices

## Summary

**Verdict.** TODO

## 1. Scope, sources and labels

**Goal** (the User's requirement, record hs-d757, 2026-10-31). A browser UI for cpd with full cockpit parity: the tree, Plans, PR boxes, the Inbox, costs, the status line's pills, chat with Leads and the Manager through the durable host protocol, and every action the cockpit takes. It is bound to localhost; other devices reach it over Tailscale or an ssh forward, never a LAN or public listener. The research picks the stack. The build waits for durable-leads P1.5's host socket protocol.

The doc answers five requirements: the inventory of what the cockpit reads, does, and keeps terminal-only (section 2), how a view learns of a change and which state has no Event (3), the shared read model a web client would reuse (4), the stack options and the recommendation (5) and the cut into PRs and the risks (6 and 7), ending with earlier-sections' open choices (section 8). The later sections, the Appendix and the Summary are written by later Jobs of this Plan.

Out of scope (hs-n591, accepted assumptions A1–A5): product code (this is a research Plan; the build waits for durable-leads P1.5); a public or LAN deployment; any change to durable-leads' PRs (the asks went to the Manager as a board FYI, A4); recommending before P1.5 merges. Parity means the cockpit as the code draws it today (A5): the Dispatch column is host stats plus the Inbox, and no view draws checks or reviews.

Pinned versions and commits: cpd origin/main at `fdd9745d9` (where this research began; main later moved to `d55c00251` when PR #687, the host socket server, merged — say so in [section 2.4](#24-terminal-only-features-and-the-sessions-a-browser-can-reach)). Every `file:line` below is of `fdd9745d9` unless it names a package. 42 `file:line` cites were spot-checked against `git show fdd9745d9:<path>`; the one that did not match, inv-acts-tree's `incident_click.rs:117-125` (that file is 26 lines), is corrected here to `incident_click.rs:20`.

Labels, on every claim:

| Label | Meaning |
|---|---|
| read | a `file:line` of `fdd9745d9` or a docs section cite |
| measured | the command and the number |
| inferred | a judgement, stated as one; a size is `guess` unless derived from a cited line count |

Gap-table legend (the header used once, in section 2.3; ids G01, G02, … run on in that table's order, no duplicates across the five findings):

- `Capability lives today` is a `file:line` of the TUI, the engine, core or pi that currently holds or withholds it.
- `CLI-first fix` is the one-line (inferred) change a web client needs to render or run it — usually: add a field to an existing `cpd` command, or a `cpd` command that does not exist.
- `Size`: S under 300 non-test lines, M 300 to 1,000, L over 1,000; the value is a `guess`.

Evidence: the twelve finding files are at `/home/clayton/.cpd/run/web-ui/explore/`, one per finding; this document synthesises the five that cover sections 1 and 2 (`inv-reads-left.md`, `inv-reads-plan.md`, `inv-acts-tree.md`, `inv-acts-work.md`, `tui-only.md`). They are a machine-local source, as `docs/research/chat-render-cost.md` says of its own.

## 2. Inventory: what the cockpit reads and does

Sources: `inv-reads-left.md`, `inv-reads-plan.md`, `inv-acts-tree.md`, `inv-acts-work.md`, `tui-only.md` (all at `fdd9745d9`). Every "the `cpd` command returns X" is from `docs/commands.md` samples and core source; no data command was run against a live ledger, so the JSON reponses are **unverified at runtime** (the findings flag the same).

Cockpit breadth first, so the tables below are not a surprise. The **Dispatch column (right) draws only host stats and the Inbox** — no Jobs, Attempts, Escalations, PR boxes or Deploy view (`widgets/column.rs:119-140`; commands.md:3573, 4571). The **PR boxes live in the Plan view (centre)**. **`Deploy` is only an Inbox item kind**, and the cockpit's only deploy affordance is `inbox approve <deploy id>` (`widgets/inbox.rs:263-264,411`); hs-d757's "Deploy view" maps to nothing — the Inbox `deploy` kind is the whole of it, and hs-d757's "PR boxes" map to the Plan view of section 2.1.5. The reads that look like Dispatch data — `read::widgets` and `read::flow` (slots, engine, PlanCard, escalations, events, review) — **are drawn by no view**; only tests call them (`draw_tests.rs:192`; `explore_attempt_tests.rs:113-116`; commands.md:4570 "Not drawn, so not read: open Escalations"). **No view draws checks, reviews or comments lists**: the PR box and Inbox row show only a state word/status; the only checks/review reads are the undrawn `PolledPr` (`read/widgets.rs:102-107`) and `pr view` (which calls `gh` and the cockpit never draws). Where the code and docs disagree (for example the docs say `pr-closed` fills the Lead's red dot, code does not, `read/dot.rs:139,147-152`), parity follows the code and the document lists the disagreement in the gaps.

### 2.1 Reads

Columns: what is shown | producing function (path:line) | inputs | the `cpd` command that returns the same data, or none | gap id.

**Tree and dots** (left column; another read `tree_at` `read/tree.rs:231`, drawn by `tree/rows.rs`; every snapshot returns `events_cursor`):

| What is shown | Producing function (path:line) | Inputs | `cpd` command | Gap |
|---|---|---|---|---|
| Context row: id, title not drawn | `tree_at` contexts query `read/tree.rs:300-345`, `context_text` `tree/rows.rs:393` | `contexts`, `context_records` counts | `context list` (§3.2) | G13 (no `created_at`, the order key) |
| Context dot = its live Manager's dot, `Stopped` with none | `Dot::of` `read/dot.rs:113`, `dot::urgency` `:144` | `sessions` role=manager, visible Inbox linked to it | `session list --role manager` + `inbox list` + client-side `Dot::of` | G29 |
| Context place/muted (blue chat, asleep, archived) | `manager_place`/`manager_asleep` `read/tree.rs:290-295,331`, `rows.rs:200,372` | latest `session-moved` Event, `asleep_at`, `archived_at` | `session list` | — |
| Context order (dot tier then `created_at` desc; archived folded last) | `Dot::tier`/`order_key` `read/dot.rs:74,86`, `items_with_costs` `rows.rs:524-552` | dot, `contexts.created_at` | none (`context list` lacks `created_at`) | G13 |
| Plan row; lead dot; counters `planned⋅active⋅failed⋅done` | `plan_heads` `read/shared.rs:356`, `fill_dots` `read/tree.rs:186-195`, `JobCounts::counters` `shared.rs:140` + `SHOWN_STATUS` `:97`, `rows.rs:291` | `plans`, Lead state, visible Inbox, `jobs` GROUP BY status, "merging" split | `plan list` (§4.2) `counts` | G10 (`fixing`/`git_active`), G12 (`merging` split), G13 |
| Plan/Context cost `$x.xxx` | `cost::total` `read/cost.rs:77`, `plan_cost` `rows.rs:331`, `context_cost` `:347`, `format_cost` `core/src/metrics/mod.rs:219` | pi Session files (parsed `metrics::parse_file`, cached by mtime+len) + `attempts.metrics.$.cost_usd` | none | G01 |
| Fork row (id `<plan>·<4>` or 8 chars) + dot | `fork_row` `rows.rs:415`, `ContextNode::forks` `read/tree.rs:88` | `sessions` role=lead `plan_lead=false` | `session list` | — |
| Archived Plans/forks/Managers, fold toggle `╰`, archive row | `kids` `rows.rs:132`, `manager_row` `:421`, `archive_item` `:510`, `FoldsFile` `tree/mod.rs:12` | `archived_at`, fold state `run/tui/tree-folds.json` | `session list --archived` etc. | G31 |
| Spinner frame (28 moons, 50 ms) | `frame_now`/`palette::frame` `tree/mod.rs:96,280` | wall clock | none (client clock) | — |

Dot rule one-liner (`read/dot.rs:37` enum, `Dot::of:113`): red = most urgent linked item, yellow/magenta/cyan/white = that item's urgency (over the *visible* items, `inbox/model.rs:302`), fixing = `PlanHead::failing` (`read/shared.rs:345`), `Working(Some(_))` colour from `Work::of` (`read/dot.rs:21`), idle/stopped/waiting from `sessions.state`. `review-requested` (blue) fills no dot; `PrClosed` (`core/src/inbox/pr.rs:195`) is not in the dot's kinds (doc drift, G29).

**StatusBar pills and costs** (the tmux status line, not a view: `STATUS_FORMAT` `core/src/host/tmux.rs:339-366`; `read::slots` `read/widgets.rs:144` is documented "the StatusBar's whole model" but rg finds no caller):

| Pill / part | Producing function (path:line) | Inputs | `cpd` command | Gap |
|---|---|---|---|---|
| workers `Q󰞘W󰞘N/M` | engine step `engine/src/watch/workers.rs` (header `:1`, not read) | pool tenants N, config concurrency M, Ready W, queued Q | partial: N/M `resource list`, Q `job list --plan`, **W none** | G02 |
| runner `pi`/`durable` | engine feed; click `toggle_runner` `cockpit.rs:422` | `worker_runner` in config.toml | `config` (§14.2) | G07 (no command sets it) |
| preview | `scripts/preview-tui.sh` | tmux option | none | dev-only |
| version (red when alert) | `apply_status_line` `tmux.rs:798`, host alert `engine/src/watch/host_alert.rs:1` | `cpd --version`, install state file | `cpd --version`; alert none | — |
| incident `Incident Mode: <elapsed>` | engine workers step `workers.rs:31` | incident state, wall clock | `incident status` (§25.3) | G09 (lead), G15 (elapsed) |
| fill/junction `━ ┻`, widths | tmux format `tmux.rs:292-337` | `client_width` | none | — |
| 4 buttons reset/legend/logs/costs | static in `STATUS_FORMAT`; `costs` click flips a **view-local** flag | click = `cpd tui action` (`cockpit.rs:333-360`) | actions | G31 (`costs` state) |

The StatusBar shows no costs: the 4th button toggles the tree's own cost display (view state, `tree/view.rs:324`).

**Inbox with its 18 kinds** (right column section `widgets/inbox.rs`, drawn by `inbox::draw` `:457`; read `inbox_at` `read/tree.rs:151`; `cpd inbox list` `core/src/inbox/cmd.rs:182` is the same query):

| What is shown | Producing function (path:line) | Inputs | `cpd` command | Gap |
|---|---|---|---|---|
| Header + count; empty text | `section::flush_title`, `inbox.rs:457,476` | snapshot len | `inbox list` (`hidden{}`) | — |
| Row 1 glyph+label+age; row 2 subtitle | `item_line` `inbox.rs:424`, `palette::item_span` `palette.rs:458`, `age_of` `:136`, `R/age.rs:5`, `rows_of` `:415` | kind, urgency, label, `opened_at`, view clock | `inbox list` (`age_seconds` once) | G30 |
| Label as link (plan-review, pr-ready, pr-closed, review-requested, deploy) | `is_link` `inbox.rs:410`, `link_url` `:449` | `url` | `inbox list` | — |
| **Merging spinner** on a `pr-ready` row | `merges` `inbox.rs:45`, `marker::live_with` `merge/marker.rs:92` | `run/tui/merging/*.json` `{pid,at,merged}`, pid liveness | none | G32 |
| Hidden items (snoozed/dismissed/claimed) | `State::Visible` `core/src/inbox/model.rs:302` | snooze/dismiss/claim columns | `inbox list --all` | — |
| `engine-down` synthetic first | `engine_down_first` `read/tree.rs:133`, `engine_down` `:103` | flock probe `model.rs:273` | `inbox list` (`id:null`) | G18 |

All 18 kinds (`enum Kind` `core/src/inbox/model.rs:19-43`; opened/closed only by engine E8's `reconcile`):

| Kind | Opener (path:line) | Urgency | `cpd` | Gap |
|---|---|---|---|---|
| dialog | `rules.rs:297` | red in dispatch/deploy else magenta | `inbox list` | — |
| loop-guard | `rules.rs:318` | red | `inbox list` | — |
| plan-review | `rules.rs:392` | magenta | `inbox list` (link) | — |
| unread | `rules.rs:333` | white | `inbox list` | — |
| drafts-parked | `rules.rs:160` | magenta | `inbox list` | — |
| pr-ready | `core/src/inbox/pr.rs:140` `conditions`, `classify:100`, `Status` `:66` | red/yellow/cyan/white by status | `inbox list` | G25 (status is label text) |
| pr-closed | `pr.rs:195` | red | `inbox list` (link); fills no dot | G29 (doc drift) |
| review-requested | `rules.rs:530` | blue | `inbox list` (link) | — |
| curate-retire / curate-docs | `rules.rs:351` | cyan | `inbox list` | — |
| models-changed | `rules.rs:618` | cyan | `inbox list` | — |
| deploy | `rules.rs:94` | yellow | `inbox list` (link); approve = `inbox approve` | — |
| engine-down | never stored; `read/tree.rs:103-125` | red | `inbox list` (`engine.running`) | G18 |
| engine-error | `rules.rs:127,599` | red | `inbox list` | — |
| job-stuck | **not opened** (`rules.rs:46`); tells the Manager | red | `inbox list --closed` | — |
| question | **not opened** (`rules.rs:80`) | magenta | `inbox list --closed` | — |
| pr-failing, pr-changes | **not opened** (`model.rs:30-33`) | red | `inbox list --closed` | — |

The view reads only stored `inbox_items` (+ lock + merge markers); engine E8 is the only reader of `gh`, `run/team-review.json`, `run/deploy-runs.json`, model files and config (`rules.rs:56-65`).

**Event logs and history views**: `cpd tui view logs [--event id]` (StatusBar `logs` button) and `cpd tui view history <id>` (chat slot).

| What is shown | Producing function (path:line) | Inputs | `cpd` command | Gap |
|---|---|---|---|---|
| Latest 500 Events, oldest first | `read::logs` `read/logs.rs:19`, `Selection::resolve(..).fetch(..)` | `events`, `highest_id` | `events list` (needs the two-step) | G17 |
| Live append | `Follow::poll` `read/follow.rs:50`, `logs_after` `read/logs.rs:34` | highest id > cursor, poll 200 ms | `events follow` / polled `events list --since` | — |
| Line `HH:MM:SS` + text/kind, cut `…` | `line` `logs/view.rs:163`, `clock` `:135` | `Event.at/text/kind`, local zone | `events list` | — |
| Colour: error red; waiting when addressed to a Lead | `addressed_to_lead` `logs/view.rs:149` | `Event.plan`, `data.{source,code}` | `events list` | — |
| **Archived history transcript**: one block per `message` | `render` `history/render.rs:20`, `load` `history/mod.rs:99` | the pi `.jsonl` file at `sessions.file`, read once | **none** | G16 |

**Plan view with its PR boxes, Job pills and Pulse** (centre; `plan/mod.rs`, `read::flow.rs`):

| What is shown | Producing function (path:line) | Inputs | `cpd` command | Gap |
|---|---|---|---|---|
| Which Plan shows; Pulse when a Manager is in the slot | `chat_slot` `read/flow.rs:622`, `manager_slot` `read/pulse.rs:45` | newest `session-moved` `data.place='chat'` | none (derived) | G23 |
| Plan phase as the lead dot's colour | `PlanHead::work` `read/shared.rs:349`, `Work::of` `read/dot.rs:21` | phase, drafts, `git_active`, merging | `plan show` (`plan.phase`) | G10 |
| One box per PR (needs level, merged first, key order) | `pr_nodes` `read/flow.rs:339`, `levels` `boxes.rs:71`, `layout` `boxes.rs:215` | `prs`, `pull_requests`, `jobs` | `plan show` `prs[]` | — |
| Box title / state word / conflict / updating | `title_of` `boxes.rs:156`, `pr_look` `read/pr_look.rs:26`, `PrState::of` `shared.rs:170`, `conflicting()` `flow.rs:179` | prs.status, pull_request state/draft/mergeable, update Job | `plan show` (partial) | G03,G19,G20 |
| Lead's working dot on the box | `Dot::working` `read/dot.rs:92`, `working_pr`/`failed_prs` `flow.rs:303,377` | `sessions.working_pr`, phase, failed Jobs | none | G11 |
| Box bottom: elapsed + cost | `footer_text` `boxes.rs:141`, `elapsed_seconds` `read/active.rs:53`, `attempt_cost` `read/cost.rs:64` | attempts.started/ended/metrics, session files, clock | none (`attempt show` partial) | G01,G26 |
| Job pill: status icon, key, `#n`, Tier | `Live` `plan/flow.rs:37`, `pill_parts` `:143`, `JobNode` `flow.rs:470` | jobs status/tier, attempts, `shown_tier`, `SHOWN_STATUS` | `plan show`/`job show` jobs[] | G21,G12 |
| Status variants draft/queued/running/**merging**/merged/failed/worked/cancelled | `Live` `plan/flow.rs:37`, `SHOWN_STATUS` `read/shared.rs:97` | jobs.status, attempts.status, latest `attempt-status` Event, `worked_on` | stored status only | G12,G28 |
| Flow graph (stacked cards by needs, transitive reduction) | `reduced_needs` `plan/flow.rs:149`, `cards_of` `:174`, `column_of` `:202` | `dependencies`, shown needs | `plan render` Mermaid | — |
| Pulse: one row per Plan, 5-min bucket bars, age/quiet, Lead dot, merges | `read::pulse` `read/pulse.rs:151`, `Kind::of` `:94`, draw `plan/pulse.rs:55`, refresh 60 s `plan/mod.rs:64` | `events` bucketed, attempts, plans, sessions, visible Inbox, clock | `plan list` + `events list` (buckets none) | G22 |

**Dispatch column (right), as drawn today** (`widgets/column.rs:119-140`):

| What is shown | Producing function (path:line) | Inputs | `cpd` command | Gap |
|---|---|---|---|---|
| Hostname | `hostname_with` `widgets/mod.rs:556` | /proc, `$HOSTNAME` | none | G05 |
| CPU %, per-GPU %, memory % | `Sampler::sample_with` `widgets/host.rs:69`, `cpu_share` `:205`, `parse_meminfo` `:212`, `parse_nvidia_smi_line` `:241`; macOS tools | /proc/stat, /proc/meminfo, `nvidia-smi -lms 2000` | none | G05 |
| Inbox section header + count, rows, `engine-down` first, age | `inbox::draw` `widgets/inbox.rs:457`, `engine_down_first` `read/tree.rs:133`, `age_of` `:136` | visible items, clock | `inbox list` | G30 |
| `Deploy` item (link to the run URL) | `Kind::Deploy` `widgets/inbox.rs:263-264,411`, glyph | `inbox_items` kind deploy | `inbox list`; approve = `inbox approve` | — |
| `pr-ready` row with status in the label | `Entry::entries` `widgets/inbox.rs:359-384` via `pr::Status::of_item` | item + `pull_requests` (engine-polled) | `inbox list` | G24,G25 |
| **Merging spinner** on a `pr-ready` row | `merges` `inbox.rs:45`, `marker::live_with` `merge/marker.rs:92` | marker files, pid liveness | none | G32 |
| Scroll cues, empty text, error row | `Shape`/`Window` `widgets/inbox.rs:65-133`, error `column.rs:145` | item heights, read error | none | — |

No Jobs, Attempts, Escalations or Events are drawn in the Dispatch column (commands.md:4571).

### 2.2 Actions

Every action a web UI with cockpit parity must let a browsing User take, or know is pi-drawn. Columns: action | trigger | input | ends in | `cockpit:true`? | drawn by | gap id. `cockpit:true` is the flag of that command in `cpd --help` (cli table `interface.rs:21-111`); several commands the cockpit runs are `false` (gap G28). "drawn by" = the cockpit's ratatui/popup process, or the **pi** adapter inside a Lead/Manager's chat pane.

| Action | Trigger (path:line) | Input the cockpit asks | Ends in (argv; core fn) | `cockpit:true` | Drawn by | Gap |
|---|---|---|---|---|---|---|
| New Context | Empty-area menu `tree/menu.rs:83` | title + Owner radio (`ask.rs:133-138`); id derived client-side `tree/slug.rs:39` | `cpd context new <id> --title= --owner=` (`actions.rs:156`); core `context/mod.rs:324` | yes | cockpit | G06,G34 |
| New Plan | Context menu `menu.rs:79` or `n` | title (id derived `ask.rs:61`, max 24) | `cpd plan new <id> --context= --title= --by-user` (`actions.rs:151`) then `session open` | yes | cockpit | G34 |
| New session / Fork | Plan/Fork menu `menu.rs:80-81` | name (default `<label> (fork)`) | `cpd session fork <plan lead> --name=` (`actions.rs:164`) then open | yes | cockpit | G34 |
| Rename Context/Plan/Session | menu `view.rs:416,58,59` | title/name | `cpd context rename` / `plan rename` / `session rename` | yes | cockpit | — |
| Archive (Context/Plan/Session) | menu `menu.rs:79-81` | none (runs at once) | `cpd context archive` / `plan archive` / `session archive` | yes | cockpit | G33 (frees the chat slot first) |
| Unarchive | Archived menu `menu.rs:82` | none | `cpd context/plan/session unarchive` | yes | cockpit | G41 (Manager row) |
| Delete Plan/Context | Delete menu on archived/asleep `menu.rs:96` | confirm + a preview (own no-`--yes` reply) | `cpd plan delete <id> --yes` / `context delete --yes` (`actions.rs:64`) | yes | cockpit (confirm) | — |
| Open Session / Manager | Enter/click `view.rs:331,493` | none | `cpd session open` / `cpd context manager --by-user` then open | yes | cockpit | G34,G36 |
| Park | only a step of F3 (`actions.rs:92`) | none | `cpd session park` | yes | cockpit | G35 (no user trigger) |
| Close | `x` / Fork menu `view.rs:319`, `menu.rs:81` | none (refuses the chat slot) | `cpd session close` | yes | cockpit | — |
| History | Inbox click only (`widgets/inbox.rs:241`) | none | `cpd session history <id>` (a pane view) | yes | cockpit | G35,G16 |
| Send | automatic (engine-down 60 s, host pill) | none | `cpd session send <id> --unblocks -- <text>` | no (Lead/Manager) | cockpit | G35 |
| Resume | loop-guard Inbox click | none | `cpd session resume <id>` | yes | cockpit | G35 |
| Retier | none | — | `cpd session retier` | no (Manager) | none | G35 |
| Copy id | Copy id menu `view.rs:428` | none | tmux `set-buffer -w` (OSC 52), not a command | n/a | cockpit | G30 (client clipboard) |
| Runner toggle | StatusBar pill `tmux.rs:358` | none | `cpd_core::config::toggle` flips `worker_runner` (`cockpit.rs:422`, `core/src/config/runner.rs:74`) | no | cockpit | G07 |
| Host pill (alert) | red version pill `tmux.rs:364` | none | `host_click.rs:38`: SQL host Manager + `session open` + once-per-incident `session send` + marker | no | cockpit | G08 |
| Incident pill | StatusBar pill `tmux.rs:350` | none | `incident_click.rs:20`: `incident::lead` + `session open` | no | cockpit | G09 |
| Merge a `pr-ready` PR | Inbox menu `inbox.rs:371` (`merge::start` `merge/mod.rs:62`) | none (no confirm) | `cpd pr merge <owner>/<repo>#<n>` then `cpd pr poll` + a marker file; core `pr/merge.rs:67` | yes | cockpit | G32 |
| Post for review | Inbox menu `inbox.rs:372` | none | SQL `<pull>→<key>` lookup then `cpd pr post --pr --plan`; core `pr/post.rs:39` | no | cockpit | G27 |
| Go to Lead / Manager | Inbox click `inbox.rs:246-281` | none | `cpd session open <lead>` (+ `select-pane`) | yes | cockpit | G36 |
| Copy Link | Inbox menu | none | tmux `set-buffer -w` of `item.url` | n/a | cockpit | G30 |
| Open URL | click a link | none | OS opener `link.rs:116` | n/a | cockpit | — |
| Snooze 15m/1h/tomorrow | Inbox menu `inbox.rs:322` | a fixed choice | `cpd inbox snooze <id> 15m|1h|tomorrow` (`cmd.rs:322`) | no | cockpit | — |
| Dismiss | Inbox menu | none | `cpd inbox dismiss <id>` (`cmd.rs:339`) | no | cockpit | — |
| Claim (review-requested only) | Inbox menu `inbox.rs:309` | none | `cpd inbox claim <id>` (`cmd.rs:412`) | no | cockpit | — |
| Approve deploy | Inbox menu `inbox.rs:308` | none | `cpd inbox approve <id>` (`cmd.rs:368`) | no | cockpit | — |
| Answer an Escalation | **no cockpit UI** (the Lead agent answers) | — | Lead runs `cpd answer <esc> "<text>"` (`core/src/ask/mod.rs:53`) | no | none | G37 |
| Curate retire / docs | Inbox click `inbox.rs:261` → checklist popup | checklist `Approve`/`Later` | `cpd context curation` then `context retire [--approve=]` / `context docs` | no | cockpit (checklist) | — |
| models-changed | Inbox click `inbox.rs:265` | none (read-only) | `cpd models show` | no | cockpit | — |
| Plan review: Approve / Change | **pi** dialog `ctx.ui.select` (`pi/lib/review-chat.ts:50`) | select, then an editor for the note | pi adapter `cpd plan decide --approve/--reject --feedback-file` (`pi/lib/review.ts:129-167`); core `plan/review.rs:326` | no (internal) | pi | G39 |
| Secret approval | **pi** dialog `ui.select` (`pi/lib/secret.ts:49`) | `Allow once`/`Deny` | pi adapter `cpd secret approve --action --vault`; core `secret/mod.rs:361`; needs `CPD_SESSION` | no (internal) | pi | G38 |
| Host-repair approval | **pi** dialog `ui.select` (`pi/lib/hostapprove.ts:54`) | `Allow once`/`Deny` | pi adapter `cpd host approve`; core `host_repair.rs:94` | no (internal) | pi | G38 |
| ask_user_question | **pi** overlay `pi/ask-user-question/ask-user-question.ts:241` | the questionnaire | **no cpd command** (answer is the tool result in the pi process) | n/a | pi | G40 |

### 2.3 Gaps

The five findings' gap lists merged without duplicates, grouped by kind. Every row has a `file:line` cite (where the capability, or its lack, lives today). Sizes are a `guess`.

**Missing command** (no `cpd` command returns the datum; the fix is a new field or command):

| id | Gap | Capability lives today | CLI-first fix (inferred) | Size |
|---|---|---|---|---|
| G01 | No command returns Plan/Context/Session cost, which the view computes from pi Session files + stored `attempts.metrics` (falling back to a Worker file) | `read/cost.rs:77`, `read/tree.rs:307-318`, `metrics/mod.rs:219`; per-Workflow `read/cost.rs:64` | add `cost_usd` fields (or a `costs` read) over the same `metrics::parse_file` | M |
| G02 | StatusBar `workers Q/W/N/M`: W (Ready Jobs) has no command; no machine-wide workers read | `engine/src/watch/workers.rs` (not read) pushes a tmux string; `read/slots` `read/widgets.rs:144` unused | a command returning the four workers numbers | S |
| G03 | `ready_to_merge` boolean: `plan show` carries `merge_state` but not `base_ref`/`default_branch` | `core/src/pr/pulls.rs:51-56` vs `:104-111` | add `ready_to_merge` (or the two columns) to the pull JSON | S |
| G04 | Engine state Never/Running/Stopped/Crashed: no command prints it | `read/widgets.rs:238-256`; `watch --check` is a start pre-check | a `watch status`/state read | S |
| G05 | Host machine hostname/CPU/GPU/memory: no command (not Ledger data) | `widgets/host.rs:69-250`, `hostname_with` `widgets/mod.rs:556` | a `status`/host command | S |
| G06 | Owner list for New Context: `known_owners` has no CLI (`cpd policy` only `show`) | `tree/view.rs:392`, `core/src/policy/known.rs:10` | `cpd policy owners` | S |
| G07 | Runner toggle: `worker_runner` only flips via `cpd tui action runner` | `cockpit.rs:422`, `core/src/config/runner.rs:74` | `cpd config set worker_runner` | S |
| G08 | Host pill: alert-the-host-Manager + once-per-incident send + marker are TUI logic with no `cockpit:true` command | `tui/src/host_click.rs:38-99` | a `cpd` command (or compose `session open`+`send`) | S |
| G09 | Incident pill: the Lead to open is `incident::lead`, not in `incident status` | `incident_click.rs:20`; `incident status` commands.md:5656 | add the lead id to `incident status` | S |

**Command returns less** (the command exists but omits what the view derives):

| id | Gap | Capability lives today | CLI-first fix (inferred) | Size |
|---|---|---|---|---|
| G10 | Lead-dot colour inputs: `fixing`, `git_active`, merging count, open-Escalations per Plan are not in `plan list` | `read/shared.rs:314-337,345,387-397`; `Dot::Fixing`/`Work::of` `read/dot.rs` | add the fields to `plan list` | S |
| G11 | `sessions.working_pr` in no command's JSON; the box Lead dot cannot be rebuilt | writers only `core/src/session/working_pr.rs:16`; view `read/flow.rs:303` | add `working_pr` to `session list`/`show` | S |
| G12 | Derived shown status `checking`/`merging` (`SHOWN_STATUS`) and `worked_on` in neither counts nor stored status | `read/shared.rs:97-99`; `core/src/job/worked_on.rs` | add the derived status / `worked_on` | S |
| G13 | Tree order keys: `created_at` of Context/Plan/Session not in the list samples | `read/tree.rs:59`, `read/shared.rs:327` | add `created_at` to the lists | S |
| G14 | Context row Manager facts (dot/place/asleep) need `session list --role manager` joined client-side | `manager_place` `read/tree.rs:290` | document the join; or add the Manager field | S |
| G15 | StatusBar host-alert, incident elapsed (and preview) have no read command | `engine/src/watch/host_alert.rs:1`, workers step `workers.rs:31` | expose them as reads | S |
| G16 | No command returns a Session's conversation; archived-history rendering is view code | `history/render.rs:20`, `history/mod.rs:99`; only `session show` gives `file` | a transcript read | M |
| G17 | `events list` documents `--since` default 0, not the views' "latest 500"; needs a two-step | `read/logs.rs:19-30`; `--since`/`--limit` docs §11.1 | a `last N` flag or corrected default | S |
| G18 | `engine-down` is synthetic: `inbox list` id `null` vs the view's id 0 | `read/tree.rs:103-125`, `cmd.rs:152` | align the synthetic id | S |
| G19 | Box `waiting on <refs>` and `superseded by` not in `plan show` `prs[]` | `Display::pr_blockers` / `superseded_by` `core/src/plan/display.rs:97-146` | add the fields | S |
| G20 | A PR's update-Job state returned less directly (only derivable from `jobs[]`) | the view's choice `read/flow.rs:398-399` | add it as a PR fact | S |
| G21 | Shown Tier (`Display::job_tier`) used by `plan render`, not by `plan show`/`job list` | `core/src/plan/display.rs:89-95` | expose it on the job reads | S |
| G22 | Pulse: per-Plan 5-min buckets, kind, age, quiet, merges, Lead dot have no command | `read/pulse.rs:151-262`; `ACTIVITY` `:139`; `Kinds` `:94` | a `pulse`/activity read | M |
| G23 | "Newest chat Lead / Manager in the slot" is view logic over `session-moved` Events | `read/flow.rs:622`, `read/pulse.rs:45` | a `chat slot` read or field | S |
| G24 | Non-Plan authored PR state reaches the Inbox only as `inbox list` label text | `read/widgets.rs:193-217`; `pull_requests` rows | a command listing authored PRs | M |
| G25 | `pr-ready` status (`ready to merge` / `ready for user review`) is the tail of `label`, not a structured field | `widgets/inbox.rs:370`, `pr::Status::of_item` `core/src/inbox/pr.rs:77` | a status field on the item | S |
| G26 | `plan show` attempts omit `submitted_at`, `metrics`, Worker state; `review` omits `number` | `core/src/plan/show.rs:158-171,220` | return them | S |
| G27 | `pr post` has no `<owner>/<repo>#<n>` form; the TUI resolves the PR key by SQL | `widgets/mod.rs:259-266`; `core/src/pr/post.rs:26` | accept the pull form | S |
| G28 | Which menu entries apply is TUI logic over row kind and label status; the `cockpit` flag is unset for commands the cockpit runs | `Entry::entries`/`Choice::entries` `widgets/inbox.rs:304-382`; `interface.rs:81,106-109` | move the entry list to core; fix the flag | S |

**View-private logic** (dot rule, costs, merge markers, palette — a web client re-implements or reads a file):

| id | Gap | Capability lives today | CLI-first fix (inferred) | Size |
|---|---|---|---|---|
| G29 | The whole dot rule (`Dot::of`, `dot::urgency`, `Work::of`, `PLAN_KINDS`, order key) and the `pr-closed` doc drift are Rust-only, not in any command | `read/dot.rs:37-155`, `read/shared.rs:345,350`; `pr.rs:195` | add a computed `dot` field, or expose the rule | M |
| G30 | Item glyphs/colours and the ordering rank are palette/view code, not data | `tui/src/palette.rs:458-476`; `age` `read/age.rs:5` | mirror client-side (or a glyph field) | S |
| G31 | View state — fold file `run/tui/tree-folds.json`, the `$` cost toggle, per-Plan scroll — has no command (by design) | `tree/mod.rs` FoldsFile, `tree/view.rs:324`, `plan/mod.rs:343` | a web UI keeps its own (not a defect) | S |
| G32 | The only record a merge runs is the marker file `run/tui/merging/*.json` + pid liveness; two clients see two markers; no command lists merges | `merge/marker.rs:92`, `widgets/inbox.rs:45`, `read/widgets.rs:150` | a "merging" fact/command (or gate through a marker reader) | M |

**Composed flow only in the TUI**:

| id | Gap | Capability lives today | CLI-first fix (inferred) | Size |
|---|---|---|---|---|
| G33 | Archive refuses `in_chat_slot`; the TUI picks the next live Session (or parks) before archiving | `view.rs:437-461`, `actions.rs:77-84,201-209`; core refusal `plan/archive.rs:149` | a `--free-slot` flag on the archive commands | S |
| G34 | Composed flows: "create then open", click Context = `context manager --by-user` then open, click archived = unarchive outermost then open | `actions.rs:186-197,210-214`; `rows.rs:225-250` | keep client-side (documented); or a single command | M |
| G35 | No user trigger for Park, History (tree), Send, Retier; Retier has no cockpit trigger at all | `actions.rs` (no call sites); `core/src/session/retier.rs:49` | new surface (parity, not a defect) | S |
| G36 | Pane focus after Open/Go-to-Lead/Manager is tmux-only (`select-pane` on the chat slot) | `actions.rs:326-328`, `widgets/mod.rs:217` | inherent in a web UI (its own focus) | S |

**Actions only the Lead or a pi pane can take** (no UI answers an Escalation; approvals are bound to a Session identity):

| id | Gap | Capability lives today | CLI-first fix (inferred) | Size |
|---|---|---|---|---|
| G37 | No User UI answers an Escalation: the cockpit shows only a count; the Lead agent runs `cpd answer` | `tree/rows.rs:993-995`; `core/src/ask/mod.rs:53` | a list of open Escalations + a User answer surface | M |
| G38 | Secret and host-repair approvals are pi-`select` dialogs bound to a Session (`CPD_SESSION`); raw agent calls are guard-refused; no "approve as the User for session S" | `pi/lib/secret.ts:49`, `pi/lib/hostapprove.ts:54`; `secretguard.ts`/`askguard.ts`; core `secret/mod.rs:369`, `host_repair.rs:94` | socket `ui_request`/`ui_response` (host protocol, P1.6) | M |
| G39 | `plan decide` takes feedback as a file path under `$CPD_HOME`, is hidden/internal; the dialog (Approve/Change) and the follow-up to the Lead are adapter-side | `pi/lib/review.ts:129-167`; core `plan/review.rs:326`; `review-chat.ts:50` | accept text; deliver through `ui_request`/the `plan-review` Event | M |
| G40 | `ask_user_question` has no cpd command; the questionnaire lives only in the pi process | `pi/ask-user-question/ask-user-question.ts:241`; no `host.call` | socket `ui_request` questionnaire (P1.6) | L |
| G41 | A Manager row's Unarchive builds `session unarchive` but core `require_lead` refuses `not_a_lead` | `tree/view.rs:88-93,411`; `core/src/session/archive.rs:112-115,254` | let unarchive accept a Manager (unverified) | S |

Count per kind: **missing command 9, command returns less 19, view-private 4, composed flow 4, Lead/pi-only 5 = 41 gaps (G01–G41)**.

### 2.4 Terminal-only features and the Sessions a browser can reach

Two media are mixed in the cockpit: the ratatui views (tree, Plan, widgets, dialogs, history, logs, legend) are cell-drawn and read the store, so every drawing feature there has a web equivalent; and the **chat slot**, which for a `pi` Session is the live pi TUI in a tmux pane — that has **no cpd-owned data path to a browser today**. The one hard terminal-only part is the chat slot of a pi Lead in tmux.

**Features** (feature | where | web equivalent or none):

| Feature | Where | Web equivalent or `none` |
|---|---|---|
| tmux mouse capture, focus events, pane click | `core/src/host/tmux.rs:177-178,204` | DOM click/focus |
| Left click rows/links/pills | `tree/view.rs:351`, `plan/mod.rs:263`, `widgets/mod.rs:417`, `link.rs:3` | `<a>`/`<button>` running the same `cpd` command |
| StatusBar button click (`cpd tui action … --client <c>`) | `tmux.rs:205`, `cockpit.rs:334-358` | page buttons (`--client` is meaningless to a browser) |
| Right-click context menu | `menu.rs:1-4`, `widgets/mod.rs:406` | contextmenu event; touch needs a long-press (guess) |
| Wheel scroll with `▲ n more` cues | `tree/view.rs:366`, `widgets/inbox.rs:108`, `column.rs:46-64` | native scroll |
| Hover | none implemented (only asserted ignored, `legend.rs:644`) | CSS `:hover` optional |
| Pane-border drag, pane sizes | `cockpit.rs:303-304` | CSS layout |
| Text selection/copy in the pane | `DL §6.5` | browser selection |
| Link cells, never OSC 8 (decision pro-d60) | `link.rs:1,18`, `osc_8_guard_tests.rs:1-3` | `<a href>` |
| Popups/dialogs kit (confirm, input, text, checklist, select, form; file `--out`) | `popup.rs:1-40`, `dialog/mod.rs:4,346`, `dialog/pane.rs:29` | native web forms (the file handoff is terminal-only) |
| Chat slot = live pi TUI in a tmux pane | C §10.4, `tree/actions.rs:333` | **none** for the pane; the host + socket protocol is the intended path |
| pi transcript render (tool rows, balloons, thinking fold, markdown) | `DL §3.5` G53-G68 | none today (must render from data: session file or socket frames) |
| pi editor box (multi-line, `@file`, paste, external editor, `!cmd`) | `DL §3.6` G83-G88 | a web textarea; `@file`/`!cmd`/slash commands are client work |
| pi dialogs: ask_user_question, plan-review, secret/host approve | `pi/ask-user-question/ask-user-question.ts:241`, `pi/lib/review-chat.ts:50`, `secret.ts:49`, `hostapprove.ts:54` | for **durable**: socket `ui_request`/`ui_response` (P1.6, not built); for **pi**: none |
| Tree keys `n x ? $ j/k Enter` | `tree/view.rs:313-331` | buttons/keys (every action also has a click path) |
| pi chords (Ctrl+C/T/O/X, Alt+Enter, Shift+Tab …) | `DL §3.6` G81 | none reliably (browser/OS reserve some chords) |
| tmux-level chords, prefix, copy-mode | `tmux.rs:181-185` | none (ssh/tmux detail) |
| Moon spinner (Nerd Font `U+E3C8-E3E3`), 28 frames @50 ms | `palette.rs:405,480-486`, `plan/mod.rs:73-107` | CSS animation (state still from `sessions.state`); needs the font or SVG |
| Clipboard (tmux buffer + OSC 52) | `widgets/mod.rs:282-289` | `navigator.clipboard` (secure-context only; Tailscale HTTP origin may not qualify, guess) |
| Terminal/pane titles, session tick line | `tmux.rs:116,200,213`, `K:2874` | `document.title`, headings |
| Pane-death / respawn hooks | `tmux.rs:201-203`, `cockpit.rs:453` | none needed (a browser has no panes) |

**Sessions a browser can reach** — what a browser can stream, send, answer, and read, and by which command or message (`cpd` = a read/writing `cpd` CLI call; socket = host protocol v1, `pi/durable-host/protocol.ts:26-45`, which on `fdd9745d9` is specified and unit-tested but **not wired to a real socket** — main's head `d55c00251` merges that socket host, PR #687):

| Session | Stream (live conversation) | Send a message | Answer a dialog / UI request | Read history | Cannot |
|---|---|---|---|---|---|
| **pi Lead/Manager in tmux** | none as conversation; `cpd events follow --session` gives Events only | `cpd session send <id> (text\|--file) --unblocks <r>` (a `session-message` Event; a stopped target starts in a parked window, needs a cockpit) | none: dialogs (`ask_user_question`, plan review, secret/host approve) are `ctx.ui` in the pane; partial `cpd plan decide`, Worker `cpd answer` | `session show` `file` + parse the pi JSONL yourself (not a cpd command); `events list` is Events only | open/park/history (pane moves, `session open` etc.); no queryable open-dialog state |
| **durable Lead/Manager (host socket)** | **intended**: socket `attach` → `snapshot` then `events` frames `{epoch,seq}`. **Today not reachable** (window unmerged; nothing starts a durable Lead) | `cpd session send` (the host follower delivers); socket `submit` (not wired) | socket `ui_response`: not built (P1.6) | socket attach snapshot (whole history); `cpd` has none (`session history/open/park/fork` refuse `durable_session`) | no TCP: a browser cannot speak a unix socket — a localhost server must hold it (inference); `cpd chat` does not exist; a shared Rust fold is Q35-decided but not built |
| **Worker, pi (tmux window)** | none; `cpd events follow --plan/--context` | none (refused `not_a_lead`); via the Lead only | its Escalation: `cpd answer <id> "<text>"`; its own `cpd ask` blocks until then | `cpd attempt read <attempt> [--lines n]` (snapshot, not stream) | attach to its pane; `attempt read` is the only readable transcript |
| **Worker, durable (child process)** | none | none | `cpd answer` as above | none (`attempt read` explicitly out of scope for durable; the report and `run/<plan>/logs/worker-<attempt>.log` are files, not commands) | as a paneless Worker, neither TUI (K:1013) |
| **Archived Session history** | none | refused (`archived`) | none | TUI: `cpd session history <id>` is a pane view (`history/render.rs:18`); a browser reads `file` from `session show` and parses; **durable archived**: refused | durable Leads in `history` |

A browser that wants cockpit parity therefore must, today, act over the `cpd` CLI (every read/action of sections 2.1–2.2), follow `cpd events` for live data, read pi transcripts from the session file, and leave each pi Lead's pane dialogs and editor to the pane — until the durable host socket's `ui_request`/`ui_response` (P1.6) or a shared core fold (Q35) wires them to data.
## 3. Live data

Sources: `live-today.md`, `host-wire.md`, `front-end.md` (the last three figures in 3.2 are from URLs read 2026-10-31). Cites of `pi/durable-host/*` and `docs/contract.md` in 3.3–3.5 are of `d55c00251` (main after #687); every other `file:line` here is of `fdd9745d9`. 23 of the new cites were spot-checked against `git show fdd9745d9:<path>` or `git show origin/main:<path>` (the host-wire ones); the rest are as the findings give them.

### 3.1 How a cockpit view learns of a change today

**Answer first.** Two mechanisms drive every TUI view: (a) a *commit watcher* (`notify` inotify on `cpd.db`/`cpd.db-wal`) that only wakes the loop, and (b) a read in the view's own process that asks the store "which Events are after my cursor" (`Follow`). `cpd events follow` is a third, separate poller (`PRAGMA data_version`) used by the pi adapter, not the TUI. The Event stream alone is **not** enough to drive a full cockpit view: PR state, the engine-running flag, snooze ends, Pulse ages and the host samples change with no Event (3.1.1). Event stream + store re-read covers the rest.

| State | Change signal | Latency / cadence | Cite |
|---|---|---|---|
| Any store write (wake only) | inotify `Modify(Data/Any)`/`Create`/`Remove` of `cpd.db`/`cpd.db-wal`, non-recursive on the store dir → `Signal::Commit` | event-driven (kernel); a lost/`Err` event is ignored, the safety read covers it | `tui/src/read/watch.rs:11-21,29-45`; `tui/src/pump.rs:38-40` |
| Read pacing after a commit | `Schedule::take`: a wanted read waits until `FRAME` (50 ms) since the last read; one read in flight; `force` for own actions | ≤ 1 read / 50 ms; 50 ms coalescing | `tui/src/pump.rs:167-220`; `tui/src/palette.rs:481` |
| Safety poll (watcher present) | `Schedule` every `SAFETY` | 2 s | `tui/src/pump.rs:19`; `tui/src/tree/mod.rs:188`; `tui/src/plan/mod.rs:538` |
| Fallback when the watcher fails to start (or driven/test loop) | poll at `events.poll_ms` (default 200 ms, range 20–10,000); widgets shows "no commit watcher" | 200 ms | `core/src/config/file.rs:338`; `tui/src/widgets/mod.rs:320-324` |
| What changed (Events) | `Follow::poll_changes`: `highest_id`, then `events` with `id > cursor`, in one read txn; cursor moves only on `Ok`; `cursor_ahead` if the DB was replaced | per read (above) | `tui/src/read/follow.rs:61-79` |
| Plan view | snapshot retaken only if an Event `concerns` it: kind `session-moved`, or same plan, or same context, or (Pulse) context; or `pulls` changed; or `force`/retry | commit + ≤ 50 ms, else 2 s | `tui/src/plan/mod.rs:197-203,585-600` |
| Pulse (dots, 5-min buckets, ages, quiet) | same Event filter + timer `PULSE_REFRESH` (window slides, ages tick, a running Attempt is no Event) | 60 s timer | `tui/src/plan/mod.rs:64,125-131`; `tui/src/read/pulse.rs:30-32` |
| Tree (Contexts/Plans/Sessions, Inbox, dots) | any `Changes::any()` (Events or pulls), or engine lock flip, or snooze end, or retry | commit + ≤ 50 ms; 2 s safety | `tui/src/tree/mod.rs:250-270` |
| Widgets/Inbox strip | Events/pulls/Inbox mark; ages each 1 s; CPU/GPU/mem sample | 50 ms frame, 1 s, 2 s | `tui/src/widgets/mod.rs:89,313-330,473,525-527` |
| Logs view | own `Follow` poll at `events.poll_ms`, appends | 200 ms | `tui/src/logs/mod.rs:94-95` |
| Spinner/animation frames | clock `FRAME_MS` = 50, `(unix ms / 50) % 28` | 50 ms | `tui/src/palette.rs:481-486` |
| Background action results | `Waker::read_done` → `Signal::Read` | on completion | `tui/src/background.rs:115-119`; `tui/src/pace.rs:72-76` |
| Engine writes (source of nearly all of the above) | engine tick; PR poll | `watch.tick_seconds` default 5 s; `watch.pr_poll_seconds` default 5 s after the last poll ended | `core/src/config/file.rs:302-303`; `engine/src/watch/pr.rs:40-48` |
| `cpd events follow` (pi adapter, any external consumer) | poll `PRAGMA data_version` (+ `schema_version`) every `events.poll_ms`; query only when it changed; one batch of 500 repeats at once | ≤ 200 ms + query | `core/src/events/mod.rs:160-215`; `docs/contract.md:2765` ("polling, no notification channel") |

#### 3.1.1 State with NO Event or store commit

| State | Why no signal | How the TUI learns | Cite |
|---|---|---|---|
| PR checks, review decision, `mergeable`, state | `pr poll` writes `pull_requests` with no Event (contract §7.6, hs-d96) | `pulls_mark` = `count(*)`, `max(changed_at)` of `pull_requests` compared per poll (it is a commit, so the watcher wakes it; the Event list alone would miss it) | `tui/src/read/follow.rs:22-24,34-38,77` |
| Engine running / down (`engine-down` item, never stored) | flock on `locks/watch.lock`; no row | non-blocking shared flock probed per read (`engine_running`); a flip triggers a snapshot | `core/src/inbox/model.rs:267-287`; `tui/src/tree/mod.rs:250-262` |
| Inbox snooze ending | clock only | `min(snoozed_until)` → deadline / safety read | `tui/src/read/tree.rs:196-214` |
| Running Attempt age, Pulse ages, window slide | time-derived | `PULSE_REFRESH` 60 s | `tui/src/plan/mod.rs:64`; `docs/commands.md:3558` |
| Inbox item age text | time-derived | redraw each second from `opened_at` | `tui/src/widgets/mod.rs:343-345,525-527` |
| Spinner frame | clock | 50 ms | `tui/src/palette.rs:483-486` |
| Host CPU/GPU/memory % | sampled from the OS | every 2 s | `tui/src/widgets/mod.rs:89,473` |
| Marker files (`run/tmux-restart.pending`, `run/watch.log`, tmux option `@cpd_workers`) | files / tmux options, no Event | not read by the listed TUI views; the tmux status bar reads the option | `docs/commands.md:3232,3305` |
| Worker/session pid liveness | the engine detects dead panes and writes `attempt-status died`/`session-state stopped` (an Event, up to one tick late); the TUI never probes a pid | via Events | `docs/contract.md:2690` context; `engine/src/watch/liveness.rs:358` (unverified in detail: only header comments read) |

#### 3.1.2 Event kinds and their coverage (`core/src/events/kind.rs:76-143`)

| Group | Kinds | State covered |
|---|---|---|
| Context | context-created/-record/-grounded/-deleted, records-retired, docs-decided, context-curated/-renamed/-owner/-archived | tree rows, records, archive |
| Plan | plan-created/-phase/-review/-outlined/-done/-deleted/-renamed/-archived | Plan row, phase, review, outline |
| Job | job-added/-edited/-merged/-failed/-retried/-cancel-requested/-cancelled | Job status in the chart |
| Attempt | attempt-started/-status (from/to, `died`), attempt-retry, attempt-nudge | Attempt/Worker status |
| PR (cpd `prs`) | all-merged, pr-feedback, pr-added/-edited/-described/-cancelled/-started/-base-changed/-opened/-merged | PR boxes, cpd-side merge state |
| Escalation | escalation, escalation-answered, escalation-expired | asks |
| Session | session-registered/-state/-moved/-renamed/-titled/-retiered, mcp-attached, session-asleep/-resumed/-archived, session-message | Lead/Manager dots, the chat slot (`session-moved` `place=chat`, `tui/src/read/pulse.rs:49-52`), titles |
| Inbox | inbox-opened/-closed/-snoozed/-dismissed/-claimed | Inbox items |
| Engine/other | watch-started/-stopped, error, review-opened/-closed, incident-started/-plans/-ended | engine marks, errors, reviews, incident mode |

Coverage verdict (inferred from the two tables): every ledger row a view draws has an Event kind except the `pull_requests` table and the engine lock; those two are the set a web server must carry separately, exactly as the TUI does (`pulls_mark` + `engine_running` probe).

### 3.2 The ways a browser can get updates

| Way | What it needs | Covers | Misses |
|---|---|---|---|
| **In-process server** over the store's commit watcher plus an Event cursor, SSE with `Last-Event-ID` resume | `notify` (already a workspace dep, `Cargo.toml:19`), a `Follow`-style cursor (`events` after id, one read txn), `pulls_mark`, the engine-lock probe, the 60 s/2 s timers; an SSE endpoint that takes `Last-Event-ID` and resumes `--since` | exactly what the TUI sees, at TUI latency (inotify wake + 50 ms coalescing); the DB is the buffer, ids monotone, so a dropped browser reconnects with its last id with no gap or dup | nothing on the ledger side; chat needs the host socket (3.3); the snapshot for a first load needs a store read (no confirmed command gives snapshot+cursor atomically, `live-today.md` 4, unverified) |
| **Fan-out of `cpd events follow`** | one follower child per server (or the server reads the store like `Follow`); each line is already JSON with a monotone `id`; a browser that drops reconnects with its last id and the server resumes with `--since` or `events list --since` | all Events, back-pressured (500/line batch, nothing dropped; ends on EPIPE); `--since 0` replays all; same `--since` replays the same lines (idempotent) | nothing outside the stream: PR checks/review/mergeable, engine up/down, snoozes, ages, host samples (3.1.1); no heartbeat line, so an idle stream is silent; latency floor is `events.poll_ms` (200 ms) + query vs inotify wake + 50 ms; a replaced store makes `--since` fail `cursor_ahead` (clients must resnapshot) |
| **Timers** | a polling endpoint (any `cpd` read of section 2.1, or a store read) at a fixed cadence | everything a read returns | push semantics; latency = the timer; N clients = N polls |
| **Chat (host socket)** | a bridge to `$CPD_HOME/run/host/<session>.sock` holding `hello`/`attach` and reconnect; frames to the browser | the Session's conversation, live (3.3, 3.4) | the cockpit views (those are cpd CLI/core, not the host) |

**The connection limit.** EventSource over HTTP/1.1 — what a plain localhost or ssh-forward server speaks — is capped at 6 connections per origin in Chrome and Firefox ("Won't fix"; HTTP/2 negotiates up to 100 streams but needs TLS, a guess for Tailscale `serve`), MDN `EventSource`, read 2026-10-31 (from `front-end.md`). **One multiplexed stream per tab** (one EventSource carrying typed events: tree, plan, pr, inbox, chat) uses 1 of 6, leaves 5 for ordinary fetches/POSTs, and survives several tabs and devices per browser; htmx's per-panel `sse-connect` (one stream per element) would hit the cap across tabs. EventSource also reconnects itself (`error` event, `readyState` CONNECTING/OPEN/CLOSED, same MDN page).

**SSE + POST against WebSocket (inferred).** The evidence favours SSE for server→browser: it reconnects itself, `Last-Event-ID` is the resume cursor the TUI already uses (`--since`), and the direction is write-mostly-one-way; WebSocket has no auto-reconnect (MDN WebSocket, unverified read, `front-end.md`). It favours POST for browser→server: every action of section 2.2 already is a `cpd` command, stateless and retryable. A WebSocket earns its keep only inside the chat bridge (host frames arrive unsolicited and chat submits are frequent); even there, an SSE stream per chat slot plus POST `submit` reproduces the bridge, and the crate table of 5.2 prices the difference (dropping WS keeps the server deps at 4–6, `rust-http.md` 4).

### 3.3 What the host socket protocol v1 carries for a web client

All cites are of `d55c00251`; `C:n` = `docs/contract.md:n` (the real socket at C:4049), `pi/durable-host/<f>:n` as named. "Not in v1" rows are either planned (P1.6–P1.9) or a gap.

| Need | Carried by v1 (message, contract section) | Planned P1.6 to P1.9 | Gap |
|---|---|---|---|
| Attach and snapshot of the whole history | `attach` → `attached{mode,epoch,from_seq,head_seq,client_id}`, then a `snapshot` frame, then ring `events` frames, then `host_state` (`pi/durable-host/server.ts:88-99`; C:4018); the first snapshot is the whole history (Q5, `durable-leads.md` R:1191) | — | size unmeasured (Q5 residual, open): ~580 B per entry for a 30-entry faux snapshot, MB plausible for a real Lead (`host-wire.md` 1, derived, guess); one line ≤ 64 MiB (`protocol.ts:4`), a bigger snapshot is "a host defect" (C:4021) |
| Resume after a drop | `attach.cursor {epoch,seq}`; resume if `base.seq ≤ seq ≤ head` and the same epoch, else snapshot mode (`pi/durable-host/ring.ts:27-34`; C:3947) | — | a host restart is a new epoch = always a full snapshot (`bye{reconnect}` tells when to retry); the browser must keep `{epoch,seq}` of the last applied frame |
| Slow or dropped browser | host-side only: `bye{slow_client, reconnect:true}` past 8 MiB unsent (`pi/durable-host/server.ts:53-60`; `protocol.ts:4`) | — | the browser-side backlog is the bridge's to bound (no protocol help); a dropped unix link resumes by cursor if still in the ring |
| Several clients on one host | Q1: any client may command (`durable-leads.md` R:1143); `clients` count in `host_state`, `client_id` per connection (`server.ts:67-71,88-99`) | — | one tab = one connection works; shared tabs need the bridge to fan out and keep per-tab cursors itself |
| Send a message | `submit{req,text,when_busy followUp\|steer\|reject,request_id?}` → `result{submission_id,status}` or `error busy` (`pi/durable-host/protocol.ts:91-96`; C:4124) | — | text only; images are unsupported until P1.8 (G82, `durable-leads.md` R:7.2); every client submit is `reply:true` — any client is the User's hand (Q1, C:4124); idempotent resend by `request_id` |
| Abort | `abort{req,background}`, `abort_submission{req,submission_id}` → `result` (`pi/durable-host/server.ts:112-121`) | — | both are plain RPC |
| Answer UI requests (approvals, `ask_user_question`, Plan review) | — | P1.6: `ui_request`, `ui_resolved`, `ui_response`; `held_ui` > 0, replay at attach; today `held_ui: 0` and none are sent (C:4045); Q8 re-asks `ask_user_question` only; Q20 ask waits for a client | the `select/confirm/input/editor/questionnaire` shapes are not yet in the contract (C3); "not a v1 client" without `questionnaire` (`durable-leads.md` R:4.4) |
| Model and tier: show | via the fold: `SnapshotEvent.agent` and `agent_changed` (R:4.3); `host_state.agent` exists (`protocol.ts:51`) but `run.ts:222` never sends it | — | cpd tier is not a wire concept: a retier is a cpd Event delivered as an entry (`session-retiered`, P1.9, C:3859) |
| Model and tier: change | — | P1.8: `configure{model?,thinking_level?}`, `list_models` | `unknown_type` today; `list_models` result shape "inferred" (R:4.5 row 21) |
| Compact | — | P1.8: `compact{req,instructions?}` → `result{task_id}`; progress as `compaction_start/end` events | — |
| Fork | — | P1.8: `fork{req,at}`; Q7: a fork is a new Session with its own host | the web client must open a second socket/Session after a fork; session list and fork are cpd CLI/core, not the host |
| History paging | — | P1.8: `history{req,before?,limit=50}` → `result{entries,next?}`, newest first | only matters if the snapshot is windowed (C1); shape not yet in the contract (C4) |
| Markdown | client's (Q2): the host sends raw text of entries and deltas | — | the browser renders; no protocol part |
| Origin and auth on the socket | **gap, by decision**: unix socket 0600 in a 0700 dir, owner-checked (`net.ts:21-24,41,47`); no TCP in the host (Q34); same-user trust (Q6); a socket has no Origin | — | the web server must authenticate and check `Origin` on its own upgrades; without it any web page in the User's browser can drive a localhost server (RFC 6455 §10.2, from memory, unverified); a reachable bridge widens Q1 to every device on the tailnet/forward |
| Host not running (no socket) | **gap, outside the protocol**: the socket exists only while the host runs; Q33 rejected reading the store directly | `cpd session open` starting the host is P1.10 | a web view of a stopped/parked durable Session needs the engine to start the host first, or shows nothing |
| Ping / liveness | `ping` → `pong`, works before `hello` (C:4012 rule 4); `bye` on shutdown | — | the bridge maps to WS ping or its own timer |
| State pill | coarse: `host_state{state idle\|working\|waiting\|blocked,name?,clients,held_ui}`; main only ever reports `idle`/`working` (`run.ts:138`) | `blocked`/`waiting` need P1.6/P1.7 | — |

Outside the host protocol: the cockpit parity of hs-d757 (tree, Plans, PRs, Inbox, merges, reviews) is cpd CLI/core, not the host — the host serves one Session's conversation.

### 3.4 The three bridge options A, B and C

All three dial `$CPD_HOME/run/host/<session>.sock`, send `hello` (≤ 5 s or `hello_timeout`), `attach`, and own reconnect with `bye.reconnect`; the wire is NDJSON, so one line = one message in any option. Basics from `host-wire.md` 2 and the durable-leads decisions (read): **Q9** decided "A. The Rust client folds each `AgentEvent` batch onto the `SnapshotEvent`" and **Q35** decided "A. A shared Rust fold in `core`: one library used by the CLI tail and by `cpd chat`; the wire is unchanged" (`durable-leads.md` R:1238, R:1528).

| | A. Relay, browser folds (JS) | B. Server folds in Rust, serves views | C. Hybrid |
|---|---|---|---|
| Who folds | the browser, a second fold of ~20 event types in TS beside the Rust one (R:4.3); a Node spike client with a subset fold is 5,142 B (R:662), full size unknown (R:1236) | the server, the one Rust fold in `core` (Q35 A) | the server folds transcript state only; control-plane frames (`ui_*`, `host_state`, `error`, `bye`, `result`) relayed raw |
| Duplicated logic | highest: a TS reimplementation of the Rust fold; pi-durable exports no reducer (R:4.1) | none beyond the shared fold; the cost moves to a view schema that does not exist (each option's own) | smaller than B's schema (transcript only); no TS fold |
| Browser implements | the fold (incl. "whole message at `message_start`, replace at `message_end`"), cursor storage, snapshot-mode replace on any `snapshot` frame, dialogs from raw `ui_request`, markdown | render the view, apply patches, send commands, dialogs, markdown | render the transcript view, dialogs from raw `ui_request`, markdown |
| Server implements | least: auth + Origin check, session ↔ socket map, line ↔ message, backpressure, reconnect; needs a max message size ≥ the 64 MiB host line | most: fold, per-session state, patch/diff or full-view resend, resume across restarts, fan-out to N browsers | medium |
| Wire coupling | the browser is tied to pi-durable `AgentEvent` shapes, which pass through unchanged (Q4) and are "experimental" (R:1236); a pi-durable bump touches the Rust **and** TS folds | only the Rust fold tracks pi-durable; the browser is tied to the server's view schema | as B for the transcript |
| Fit with Q9/Q35 | Q9 A and Q35 A say a Rust client folds; A adds a second, non-Rust fold, which Q35's own evidence flags: "a web UI in another language would have to fold again until B is added" (R:1528) | Q35 A directly, if the web server is Rust (stack not yet chosen: unverified); hs-d753 CLI-first | compatible with Q35 A |
| Latency / size over the link | the whole snapshot and every raw batch cross the link; batches are small, the snapshot is the unmeasured big one | the server can send a trimmed/rendered view; the snapshot still crosses the unix hop | as B for the transcript |

Common to all three (not a bridge question): markdown is the client's (Q2) and approvals + Origin are the (web) server's job.

### 3.5 The protocol changes C1 to C5

From `host-wire.md` 3. **None needs a version bump**: adding an optional field or a new message type is not a version change (R:352). The durable-leads Lead is done and takes no message, so at 2026-10-31 14:14 EDT these were **sent** — not agreed — to the Manager as a board FYI (A4).

| # | Change a web client needs | Why | Version bump? | Status |
|---|---|---|---|---|
| C1 (conditional) | optional `attach` field `entries_limit` (number): in snapshot mode the host sends a `snapshot` whose `entries` holds only the last N, `attached` says `truncated:true`; older entries come from `history` (P1.8) | a cold attach over Tailscale/ssh pays the whole history (Q5 A; no cap; the 64 MiB line limit is unhandled, C:4021); windowing "needs no version bump" already noted (R:1191) | No for the field and `truncated` — but an old host **silently ignores an unknown `attach` field** (`parseLine` ignores unknown fields, C:3873), so the client would silently get the full snapshot: list a feature in `welcome` or require P1.8 | sent, conditional on the P1.5 snapshot measurement; whether folded events name entry ids not in a trimmed snapshot is unverified and must be checked first |
| C2 (conditional) | optional `hello.role: 'viewer'` that cannot `submit`, `abort` or `ui_response` (`error refused`) | Q1 (any client commands) plus a network-reachable bridge means every connected browser can answer `secret_approve`/`host_approve` (R:4.8); Q1's own text says "revisit with a `viewer` role" (R:1135-1141) | No (same silent-ignore as C1); alternative with no protocol change: the web server refuses to forward `ui_response` for those purposes unless the User re-authenticates | sent, only if the web server cannot enforce the alternative itself |
| C3 (request, P1.6) | write `ui_request` for every `purpose` including the `questionnaire` field schema as exact TS types in §6.14 (today only "inferred", R:4.5 row 23), and say `ui_request` frames replay after `host_state` with `replayed:true` | a JS client must render and answer them from the contract alone; the questionnaire has no pi shape (R:4.4) | No (new types) | sent |
| C4 (request, P1.8) | specify `history`, `list_models` and `configure` results as exact types (today "inferred", R:4.5 row 21), and say what `fork` returns (a new Session id the host does not serve, Q7) | the web client calls them; the fork result must tell it which socket to open next | No | sent |
| C5 (request, sock-host follow-up) | define what the host does for a line over `LIMITS.hostLine` (64 MiB): today "a host defect, not handled" (C:4021) | a bridge sets its message-size limit from this; a silent drop or crash on a big snapshot is undebuggable remotely | No | sent |

Not needed (checked): a `hello` capability for the web (the `client` string is enough, `protocol.ts:30`); a second transport or TCP in the host (Q34 decided against; the bridge is another process); `Origin` on the socket (a unix socket has none; the bridge checks it); server-side folding in the host (Q35 B, only if the web server is not Rust and B is chosen — then it is a new message family, still no bump).

## 4. The shared read model

Source: `core-model.md`; every `file:line` is of `fdd9745d9`. 14 of the new cites below were spot-checked against `git show fdd9745d9:<path>`.

**Answer first.** The whole read model except `pr_look.rs` and `watch.rs` is pure data with no ratatui, but it lives in the tui crate and no CLI command calls it. The CLI commands `plan show`, `session list` and `inbox list` run a *parallel* path in core. No test pins that the two agree. Moving the model to core with `Serialize` would give the TUI, the CLI JSON and a web server one source of truth — and would close, at once, most of the "command returns less" and "view-private" gaps of 2.3 (4.6).

### 4.1 The crate graph, one line each

| Crate | Depends on | Notes |
|---|---|---|
| `cpd-core` (`core/Cargo.toml:2`) | no workspace crate | ratatui only as a dev-dep, to unify features (`core/Cargo.toml:37-42`); `core/src/lib.rs:9-56` has `plan`, `inbox`, `session`, `metrics`, `events` — no `read`/`view` module |
| `cpd-engine` (`engine/Cargo.toml:2`) | `cpd-core` (`engine/Cargo.toml:13`) | — |
| `cpd-tui` (`tui/Cargo.toml:2`) | `cpd-core` (`tui/Cargo.toml:13`), ratatui (`tui/Cargo.toml:17`) | does not depend on engine or cli |
| `cpd` / cli (`cli/Cargo.toml:2`) | `cpd-core`, `cpd-engine`, `cpd-tui` (`cli/Cargo.toml:21-23`) | calls only `cpd_tui::{bare,run_with,...}` (`cli/src/cli.rs:142,156,190`); never `cpd_tui::read` |
| workspace | members core, engine, cli, tui (`Cargo.toml:3`) | `cpd_tui::read` is used outside tui only by a cli test (`cli/tests/pr/pr.rs:24,2666`) |

### 4.2 Every file of `tui/src/read/`

"Pure" = no ratatui/crossterm import in the file (verified by `rg ratatui` over non-test files: no hit; only `pr_look.rs:4` imports `crate::palette` and `watch.rs:6-9` imports `crate::pump`). No type derives `Serialize` (derives are Clone/Debug/PartialEq only, e.g. `widgets.rs:23`, `pulse.rs:37`); "serde-able" here means no UI types in the fields. Not run through a compiler: the classification is a read, not a build.

| File | Lines | Class | Shared with CLI/core? | What moving to core touches |
|---|---|---|---|---|
| `active.rs` | 72 | pure (interval union math, `time`) | TUI-private | leaf; needs `bad_row` from `shared.rs:22` |
| `age.rs` | 11 | pure (`age_text`) | TUI-private; CLI emits `age_seconds` (commands.md §13.2 has the same unit rule) | leaf |
| `cost.rs` | 79 | pure (process-global `LazyLock<Mutex>` cache, `cost.rs:5,14`) | calls `cpd_core::metrics::parse_file` (`cost.rs:9,58`): parsing shared, the (mtime, len) cache and the sums TUI-private | the cache is process state: a server keeps it, a CLI call does not need it |
| `dot.rs` | 378 | pure enums `Work`, `Dot`, `urgency` (`dot.rs:12,37,144`); docs name `palette::*` but the code has no ratatui | TUI-private (the Lead dot rule, commands.md:4645) | palette maps `Dot` → colour/glyph (view-bound, stays); `Dot`, `Work` move |
| `flow.rs` | 644 | pure structs `Flow`, `PlanBoxes`, `PrNode`, `JobNode`, `Pull`… (`flow.rs:35-258`), `flow`, `plan_boxes(_at)`, `chat_slot` | **partly**: reuses core `plan::jobs::load` (`flow.rs:471`), `load_prs` (`:387`), `plan_fingerprint` (`:278`), `shown_needs` (`:488`), `failed_worked_on` (`:317`); own SQL for attempts, escalations, review, pulls (`flow.rs:507-611`) which `core/src/plan/show.rs:~20-70` also writes as `serde_json::Value` | largest piece; `PrNode.lead_dot: Dot` pulls `dot.rs` along; needs `Serialize` |
| `follow.rs` | 80 | pure (`Follow`, `Changes`; uses `events` + rusqlite) | wraps `cpd_core::events` (`follow.rs:6`) | leaf; in-process cursor |
| `logs.rs` | 40 | pure (`logs`, `logs_after`) | wraps `cpd_core::events::Selection` (`logs.rs:6`); `events list` is the CLI twin (`mod.rs:15`) | leaf |
| `mod.rs` | 84 | glue: module list and re-exports (`mod.rs:27-84`) | n/a | re-exports follow the moves |
| `pr_look.rs` | 41 | **UI-bound**: returns `palette` meanings/colours (`pr_look.rs:4,29-38`); `PrFacts`/`PrState` are plain data | TUI-private | the words (`open`, `conflict`, `updating`) could move; the `palette::Meaning` mapping cannot (unverified whether `Meaning` alone is ratatui-free: palette.rs is 884 lines, not read) |
| `pulse.rs` | 302 | pure (`Pulse`, `PulseRow`, `Bucket`, `Kind`, `manager_slot`) | TUI-private; no CLI command | uses `shared.rs` `plan_heads`/`sessions`, `dot.rs` |
| `shared.rs` | 503 | pure row types and loaders: `SessionNode`, `PlanHead`, `Pr`, `JobCounts`, `plan_heads`, `sessions` (`shared.rs:272,356`) | **parallel** to `session list`: own SQL `sessions()` (`shared.rs:272`) vs core's `query` (`core/src/session/row.rs:279`) | foundation of tree/flow/pulse/widgets; needs clap `ValueEnum` of core types (`shared.rs:7,36`), already core's |
| `tree.rs` | 343 | pure (`Tree`, `ContextNode`, `PlanNode`, `inbox_at`, `inbox_mark`, `snooze_end`, `tree_at`) | **partly**: inbox rows come from core `inbox::model::list` (`tree.rs:128`, same as `core/src/inbox/cmd.rs:194-195`); own copy of `engine_down` as `Item` (`tree.rs:103`) beside the CLI's JSON one (`core/src/inbox/cmd.rs:150`) | pulls `shared.rs`, `dot.rs`, `cost.rs` |
| `watch.rs` | 46 | **UI/process-bound**: `notify` watcher plus `crate::pump::Waker` (`watch.rs:6-9`) | TUI-private | does not move; a server needs its own wake |
| `widgets.rs` | 303 | pure (`Widgets`, `Slots`, `Engine`, `PlanCard`, `PrRow`, `MachineEscalation`, `widgets`, `slots`, `open_pulls`) | TUI-private SQL; the CLI twin is `plan list` (`mod.rs:13`) | pulls `flow.rs` `open_escalation`, `tree.rs` helpers |
| `*_tests.rs` (13 files, 2162 lines) | `age_tests` 41, `cost_tests` 475, `dot_pr_tests` 77, `elapsed_tests` 203, `explore_attempt_tests` 181, `inbox_mark_tests` 37, `lead_dot_box_tests` 103, `look_tests` 197, `pulse_tests` 387, `stacked_ready_tests` 56, `watch_tests` 54, `workflow_cost_tests` 132, `working_pr_dot_tests` 219 | tests | n/a | `look_tests`, `lead_dot_box_tests`, `working_pr_dot_tests` use ratatui `Cell`/`Color` and `crate::plan::boxes` (519 lines UI-bound); the other 1643 lines test pure reads on fakes |

### 4.3 The three parallel paths against the cockpit's reads

| Command | CLI code | Cockpit code | Shared | Parity test |
|---|---|---|---|---|
| `plan show` | `core/src/plan/show.rs:76-100` (raw SQL → `serde_json::Value`) | `read::flow`/`plan_boxes` (`tui/src/read/flow.rs:275,292`) | only the core helpers listed in the `flow.rs` row; the escalation, attempt, review and PR queries are written twice | none found |
| `inbox list` | `core/src/inbox/cmd.rs:182-250` (`model::list` ×3 and an own `engine_down` JSON) | `read::inbox_at`/`tree_at` (`tui/src/read/tree.rs:151,231`) | `inbox::model::list` and `engine_running` (`core/src/inbox/model.rs:273`); ordering and the engine-down rule are restated | none found |
| `session list` | `core/src/session/row.rs:340-366` (`query` + `Row::list_json`) | `shared::sessions` (`tui/src/read/shared.rs:272`) | nothing | none found |

What the two disagree on (read, from `core-model.md` 3): the ordering and the `engine-down` rule are restated in two places (`tree.rs:103` vs `cmd.rs:150`); the escalation/attempt/review/PR queries exist twice with no test that the JSONs equal; everything the views *derive* — the shown status (`SHOWN_STATUS`), the dot rule, Pulse buckets, the chat slot — is in the read model only, so the CLI cannot even disagree, it simply lacks them (gaps G12, G29, G22, G23).

### 4.4 Parity is not pinned

The docs claim the equality — "the same queries as the snapshot commands" and "in the order of `cpd inbox list`" (`docs/commands.md:4555`; `tui/src/read/tree.rs:28,126`) — and those words are the only link. Facts: no read type derives `Serialize`, so no CLI command can print a read row as-is (`widgets.rs:23`, `pulse.rs:37`); `tui/src/read_source_tests.rs:6-22` is a *source scan* only — no process spawn, no write statement in non-test read files — and checks nothing about the CLI; `rg 'plan show|session list|inbox list'` over `tui/src/read/*_tests.rs` hits nothing; the only CLI test touching `cpd_tui::read` is `cli/tests/pr/pr.rs:2666`, which asserts on `read::widgets` and does not compare with CLI JSON. Not exhaustively verified over `cli/tests` (searched `cpd_tui` and `read::` only).

### 4.5 Totals (product files in `tui/src/read/`, 2926 lines; guess: classified by import/derive, not compiled)

| Bucket | Lines | Which |
|---|---|---|
| Already shared | **0 whole files**; only call-level reuse of core primitives (guess: ~15 of the 2926 lines are such calls) | `flow.rs`, `tree.rs`, `cost.rs`, `follow.rs`, `logs.rs` call core |
| Movable (pure data) | **2755** | active 72, age 11, cost 79, dot 378, flow 644, follow 80, logs 40, pulse 302, shared 503, tree 343, widgets 303 |
| Glue | 84 | `mod.rs` |
| View/process-bound | **87** | pr_look 41 (palette), watch 46 (pump, notify) |
| Test lines | 2162 | 1643 movable, 519 UI-bound |

Gaps a web server would hit today (read): read types have no `Serialize`; the process-global cost cache in `cost.rs:5,14` (process state, not a pure function); `engine_down` and the ordering duplicated between tui and core (4.3).

### 4.6 Options for one model that the TUI, the CLI JSON and the web server all read

Each option's effect on parity drift is **inferred**. Gap ids are the ones of section 2.3 whose "Capability lives today" is a `tui/src/read/` path (read-located) plus the ones whose data the moved queries compute.

**O1. Move `tui/src/read` to core, derive `Serialize`, and make the three CLI commands print the moved model.** One path, one source of truth (DRY); the CLI JSON becomes the read model, which is exactly the CLI-first rule of hs-d753 (the CLI is the first-class interface; a web UI is one client of it). The web server links the same types, so its views cannot drift from the TUI's; the derived facts (shown status, dot, Pulse, chat slot) become data, so the CLI stops "disagreeing" — it simply returns them. Contracts to serialize: a deliberate change to the JSON of `plan show`/`session list`/`inbox list` (ordering, derived statuses, dot field), regenerated `docs/interface.md`; the cost cache becomes server state (a server keeps it, a CLI call does not); moving touches `core/src/lib.rs` and `Cargo.toml`-adjacent files, which `docs/implementing.md:7` forbids Jobs to edit — the move is one deliberate decision, not a Job's side effect. Closes for the web **and** the CLI every read-located gap of 2.3: **G01** (cost), **G04** (engine state), **G10** (dot inputs), **G11** (working dot), **G12** (derived status), **G13** (`created_at` order), **G14** (Manager facts), **G17** (logs), **G18** (`engine-down`), **G22** (Pulse), **G23** (chat slot), **G29** (the dot rule), **G30** (the ordering rank), **G32** (merging read) — and, if the printed JSON keeps the field (a `guess`), part of the `plan show` gaps **G03, G19–G21, G25, G26** that `flow.rs` already computes.

**O2. Leave the model in tui; let the web crate depend on `cpd-tui`.** The web UI and the TUI share one model and cannot drift from each other, with zero moves and zero `Serialize` work at first; the CLI stays a third, parallel path, so its gaps remain open for agents and the CLI (the gaps of 2.3 stay open for the CLI). Costs: the web crate pulls ratatui transitively (`tui/Cargo.toml:17`) and is coupled to the TUI crate's internals; a `cpd-web` binary then links a UI crate, which is the layering the crate graph (4.1) exists to avoid — and hs-d753's CLI-first test fails on every gap the CLI does not close. Closes for the web the read-located list of O1; for the CLI, none.

**O3. A second read model written in the web crate.** Worst parity drift: three paths (core CLI, tui, web) with no test pinning any two (4.4), so a change drifts silently; it is a fourth copy of the same SQL (the 2,755 movable lines again) and leaves every gap of 2.3 open for the web too, because the web's copy has the same omissions as the CLI's. Cheapest to start, most expensive forever; only defensible as a throwaway spike.

Recommended (inferred): **O1** — it is the only one that satisfies hs-d753 (the CLI gains the read model) and converges the three consumers on one source of truth, with no new dependency (serde is already a workspace dependency) and no new toolchain. O2 is the reversible first step: a web crate that starts by linking `cpd_tui::read` (proving the views) and is then cut over to core's copy once `plan show`/`session list`/`inbox list` print it. The User decides in section 8 (A3).

## 5. Stack options and the recommendation

Sources: `repo-cost.md`, `rust-http.md`, `front-end.md`, section 3 above; package sizes are URLs (`unpkg.com`, `crates.io`, npm registry) read 2026-10-31. `cargo` was not installed on the machine that wrote `rust-http.md` (`cargo tree` never ran), so **every transitive count in 5.1 and 5.2 is `guess`**; direct crate counts come from the crates.io dependency API (2026-10-31). All `file:line` are of `fdd9745d9` unless named.

### 5.1 The decisions of a stack, with their options

**D1. Where the server lives.**

| Option | Install fit | Notes |
|---|---|---|
| A `cpd web` subcommand in the one cpd binary | none: `Install` copies one binary (`cli/src/install.rs:36-41`), and install-cpd.sh builds `-p cpd` only (`scripts/install-cpd.sh:10`) | `docs/research/binaries.md` (hs-q5) recommended one binary and priced separate binaries: more installs/version checks, a reload policy per binary, a version matrix (`binaries.md:6,113-114,129-133`); a long-lived server needs its own restart rule — nothing exists (guess, `repo-cost.md` 3) |
| A new workspace crate (fifth member, a library the cli calls) | none, if the subcommand stays in the cli binary (a library member, not a second binary) | `Cargo.toml:3` and the layout table (`docs/implementing.md:11-16`) change; `verify:build`/`verify:rust` pick a member up automatically via `--workspace` (`package.json:20-21`) |
| The engine | engine restart = web server restart | binds User browsing to the engine's lifecycle and watch loop; the engine is the store's writer, so serving reads there risks coupling watchdog/restart to interaction (inferred) |
| A separate Node process | a second long-lived artefact: its own copy, `pnpm install --prod` at `cpd install` (step 4a becomes active on every machine, 10 min bound), its own restart | two runtimes in prod, a second supply chain (5.2 class C); `pi/lib/cpd.ts` shows the spawn-cpd pattern such a server would need (`binaries.md:26`) |

**D2. The HTTP crate.**

| Stack | New packages (guess) | Async? | Maintained | SSE | WebSocket | Notes |
|---|---|---|---|---|---|---|
| axum 0.8.9 (crates.io, 2026-10-31) + tokio | 45–70 | yes — brings a runtime the workspace does not have (no `tokio`/async in core/engine/cli/tui, `rust-http.md` 1; `docs/implementing.md:253`) | active, 2026-04 release, org-maintained | built in | built in (feature `ws`) | the sync rusqlite `Store` needs a `spawn_blocking` seam (guess); SSE/WS/static/shutdown all included |
| hyper 1.12.0 + tokio + hyper-util | 25–40 | yes | active, 2026-10 | hand-written | none (add tokio-tungstenite) | most code among the async options: routing, SSE framing, upgrade, assets |
| tiny_http 0.12.0 | 4–6 | no — blocking, thread-per-connection, fits the workspace's `std::thread` style (`tui/src/pump.rs`, `engine/src/watch/*.rs`) | **stale**: last release 2022-10 | hand-written chunked | `Request::upgrade` + tungstenite (+10–15, guess) | SSE framing, route match, Host/Origin by hand; `tiny_http::TestRequest` for handler tests (docs.rs, 2026-10-31) |
| rouille 3.6.2 | 35–50 | no | stale, last release 2023-04 | `from_data` | built-in module | heavier deps (chrono, multipart, url, rand); serde_json/time already in the lock |
| hand-written on `std::net` | 0 | no | own code | by hand | by hand | an HTTP/1.1 parser and framing of our own on a security-sensitive boundary, for zero packages; against the "take the dependency, pinned" axiom (inferred — the document's own judgement, added beyond the finding `rust-http.md`) |

No stack is chosen by the findings. Cargo.lock today holds, of all the stacks' names, only `mio` (:1129, via `notify`), `libc` (:997), `base64` (:116) and `minijinja` (:1117) — no tokio, hyper, axum, tiny_http or tungstenite (verified at `fdd9745d9`).

**D3. The front end.**

| | A. Server-rendered fragments, htmx + SSE | B. Vanilla ES modules + Web Components | C. Small SPA + bundler |
|---|---|---|---|
| Build step | none for the browser: htmx is a static file; the server (Rust) renders HTML; Gate unchanged | none: files served as-is; a DOM tsconfig only if we ship TS (browser code outside `pi/` is unchecked today, `tsconfig.json:11`) | Vite/esbuild (or the Svelte compiler) in pnpm; a build step before `verify:build` embeds its output |
| Shipped JS | htmx 2.0.x min 52,182 B / 16,826 gz + SSE ext 2,853 / 1,073 gz (unpkg, 2026-10-31); own CSS; markdown rendered server-side: 0 B client | own code only (guess: tens of KB); marked 49,207 / 14,851 gz + DOMPurify 29,828 / 11,743 gz (unpkg, 2026-10-31) if markdown is client-side | framework bundle + marked + DOMPurify unless markdown is server-rendered |
| Live updates | `sse-connect`/`sse-swap` swap server HTML; the extension adds backoff reconnect over the browser's; one multiplexed stream needs one root element and one `sse-swap` per event name | native `EventSource` dispatching typed events to custom elements; we write the state/resync logic by hand — grows into a mini-framework (guess) | one `EventSource` feeding a store; finest-grained update; resync after reconnect is our code |
| Gate / repo impact | none new: fragment tests are Rust in-process (render fn over a fake store → assert HTML, like `Screen`); SSE handler tested with the in-memory store + `ManualClock`; htmx itself is external and untested (rule: only authored code, `implementing.md:225`) | Gate unchanged for plain JS; `node --test` on the pure modules (reducers, formatters, SSE parser) with `@sinonjs/fake-timers` (already a devDep, `package.json:32`); custom-element glue is thin and untested | new front-end toolchain in the Gate's critical path; components need a DOM emulator (new dev dep); bundler output is external and untested |

**D4. Where markdown is rendered and sanitised.**

| Where | How | Cost |
|---|---|---|
| Server (Rust) | render + sanitise once, in code we author and can test in-process (`implementing.md:225,234`); the sanitised HTML is the fragment; 0 B client JS for markdown; a strict CSP is the second line | two new crates (a markdown renderer and an HTML sanitiser; names guessed from memory — `pulldown-cmark`, `ammonia` — unverified, `front-end.md`) |
| Client (JS) | `marked` parses, `DOMPurify.sanitize()` before `innerHTML`; marked does not sanitise (guess from memory); re-parse per delta | marked + DOMPurify as pinned npm deps; their JS is external and untested by the Gate; the sanitising is the web's own code |

Q2 (durable-leads R:1155) fixes only the host's side — raw text on the wire; who turns it into HTML for this browser is the stack's choice (inferred).

**D5. How third-party JavaScript reaches the browser without vendoring.** The User's axiom is never to vendor or fork what upstream owns and to take the dependency, pinned; so no file from `htmx`, `marked` or `DOMPurify` is copied into the repo by hand. The options:

| Ships as | Needs a pnpm dep? | Needs a Gate build step? | Viable for `cpd install`? |
|---|---|---|---|
| pinned npm dependency (root `package.json` `dependencies` + `pnpm-lock.yaml`; `pnpm install --frozen-lockfile` is already Gate step 0, `package.json:14`) whose minified file the server embeds via one `scripts/` copy step before `verify:build`/`install-cpd.sh`'s `cargo build` | yes | yes — one script, and `scripts/check-portable.sh` must classify it (macOS: no `readlink -f`, `sed -i`, `timeout`, `sha256sum`…); a new stage in `verify:steps` goes where its cost puts it (`implementing.md:417`) | yes: the files are embedded into the binary at compile time; the binary stays the only installed artefact |
| pinned npm dependency served from `node_modules` at runtime | yes | no copy step | **no (inferred)**: `cpd install` copies only the binary (`install.rs:36-41`) and skips npm today (no `dependencies`, step 4a skipped, `repo-cost.md` 3); an installed binary would have no `node_modules` |
| CDN at runtime | no | no | yes but needs internet at page load, a CSP allow for the CDN origin, and every Tailscale/ssh-forward client fetching it — against a localhost-bound local-first tool (inferred) |

Per front-end approach: **A** needs the two htmx files only (pnpm dep + the copy step); **B** needs marked + DOMPurify only if markdown is client-side (same path; none for its own modules), or nothing if markdown is server-side; **C** needs the full bundler pipeline (pnpm deps + bundler build step + DOM tsconfig + `verify:node` globs + CI yml and `gate.test.ts` pin, `implementing.md:417`).

**D6. How the chat bridge is built.** From 3.3 and 3.4: the web server holds the host's unix socket (`$CPD_HOME/run/host/<session>.sock`), `hello` ≤ 5 s, `attach`, reconnect by cursor with `bye.reconnect`; frames reach the browser over **one SSE stream per chat slot**, and browser submits are **POST** (per 3.2, inferred — dropping WebSocket keeps the server at the 4–6-package class); the bridge is the Rust client of the host, folding nothing itself (Q9 A, Q35 A); markdown is raw on the wire (Q2) and rendered by D4; every upgrade and POST carries the server's Origin check (3.3, RFC 6455 §10.2 unverified).

### 5.2 Cost by stack class

Columns as in `repo-cost.md` 5. A new crate needs a recorded User decision: "Later Jobs may not add, remove or change dependencies or features" (`docs/implementing.md:383`; `Cargo.toml:10-12`), and the one precedent (`notify`) was a User decision recorded in a Plan (`implementing.md:405`). All sizes/times are `guess`.

| Stack class | New dependencies | Gate steps added | `cpd install` change | How our code is tested (no browser, no network, no sleep) | Risk |
|---|---|---|---|---|---|
| **A. Rust server in the workspace, static assets embedded** | one HTTP crate (none in the lock; only `mio` via `notify`, `Cargo.lock:1129`): ~4–6 packages (tiny_http class) or 45–70 (async); `include_str!` needs no crate; each new crate = a recorded User decision (`implementing.md:383,405`); a new member changes `Cargo.toml:3` | none new in principle: `verify:build` and `verify:rust` pick a member up via `--workspace`; hand-written HTML/JS/CSS adds none; build time +30–60 s sync, +1–3 min async cold (guess) | **none** as a subcommand of `cpd` (one binary, `install.rs:36-41`); a second binary needs new install steps and restart rules | handler as a pure function over request/response values; listener behind a `Ports` seam; `ManualClock`; no socket, no browser (`implementing.md:225,234`); JS (if any) under `node --test` with fake timers | low for repo rules; medium for dependency approval and the async-runtime question (no runtime today, `implementing.md:253`); asset JS untested by the Gate unless Node tests are added |
| **B. Same, server-rendered templates** | the same server crate; templates need **none**: `minijinja` 3 is already a workspace dependency (`Cargo.toml:16`, strict undefined already the policy, `implementing.md:394`); askama/maud would be new | same as A; no JS toolchain | same as A (templates `include_str!`-embedded; no files to install) | render test: state → HTML string asserted (like `Screen` text); handler as in A; no browser | lowest of the four for Gate and install; a push feel needs JS or SSE regardless; HTML-escaping and XSS are product risk, not repo-rule risk (no escaping policy read) |
| **C. Separate Node server** | a Node HTTP framework or only `node:http` (none new); npm deps go in root `package.json` `dependencies` (today none are installed at `cpd install`, 4a skipped) and need `pnpm-lock.yaml` + `allowBuilds` (`pnpm-workspace.yaml:1-5`); node ≥ 22.19 and pnpm 12.6.0 are already required (`package.json:6-9`) | `typecheck` include (`tsconfig.json:11`) and `verify:node` globs (`package.json:11`) must grow or the code is neither checked nor tested; `check-portable.sh` classifies any new script; the no-sleep scan applies; no new stage (guess) | **yes**: a second long-lived artefact with its own copy + `pnpm install --prod` (4a becomes active on every machine, 10 min bound), its own restart, and a way to reach cpd data (spawn the `cpd` CLI, `binaries.md:26`) | `node --test` with `@sinonjs/fake-timers`; `cpd` faked as `pi/test/answers.ts` does; real sockets forbidden | highest operational: two runtimes in prod, a second supply chain, script portability, install ordering (a failed pnpm blocks the whole `cpd install`; 4a runs before the binary swap) |
| **D. Front end built by a bundler** | bundler + framework in npm (vite/esbuild/webpack, transitive tree, build scripts needing `allowBuilds`), a second tsconfig with the DOM lib, plus whichever server serves it (A, B or C); lockfile churn | a build step before `verify:build` embeds or installs its output: a new `verify:web` (or inside `verify:build`) with a script classified by `check-portable.sh`; typecheck for the web tsconfig; `verify:node` globs; CI yml + `gate.test.ts` pin (`implementing.md:417`); output committed (stale risk, no precedent) or built in the Gate (tens of seconds, guess) | `install-cpd.sh` has one `cargo build` and no JS build (`install-cpd.sh:10`); the output must embed into the binary at compile time (build order bundler → cargo in `install-cpd.sh` **and** `verify:build`) or ship as files (new install step) | JS units under `node --test`; components need a DOM emulator (new dev dep); no real browser in the Gate | highest build risk: a new toolchain in the Gate's critical path, cold-cache cost, frozen-lockfile and build-script policy, two orders of build; UI tests stay the weakest (no browser permitted) |

### 5.3 The recommendation

**One stack (inferred — the document's judgement; the User decides in section 8, A3).** Against the axiom "fewest new dependencies and least new toolchain for the same parity" (`repo-cost.md` 5, `rust-http.md`, `front-end.md`):

- **Server**: a `cpd web` subcommand of the one cpd binary (D1; one binary, no install change, `install.rs:36-41`, `binaries.md:6,113-114`), implemented in a **new workspace crate `cpd-web`** (a fifth member, a library the cli calls; D1).
- **HTTP**: **tiny_http class (sync, thread-per-connection)** — the only stack that adds no runtime to a workspace with none (`rust-http.md` 1; `implementing.md:253`), at 4–6 new packages vs 45–70 for axum (guess), and it matches the codebase's `std::thread` concurrency (`tui/src/pump.rs`, `engine/src/watch/*.rs`). No WebSocket: **SSE + POST only** (3.2), which keeps the count at 4–6 instead of 15–20 with tungstenite (guess).
- **Front end**: **server-rendered fragments with htmx + SSE** (D3-A) — one multiplexed EventSource per tab (the 6-connection cap, MDN 2026-10-31), the only third-party JS being htmx + htmx-ext-sse (17,899 B gz, unpkg 2026-10-31), **shipped as pinned npm dependencies embedded by one classified copy step** (D5; never vendored); markdown **rendered and sanitised server-side in Rust** (D4) so no marked/DOMPurify JS ships at all and the sanitising is testable in-process.
- **Read model**: **section 4 O1** — `tui/src/read` moved to core with `Serialize`, `plan show`/`session list`/`inbox list` print it (closes G01, G04, G10–G14, G17, G18, G22, G23, G29, G30, G32 on the read side, and part of G03/G19–G21/G25/G26; no new dependency — serde is already in `Cargo.toml:13-30`).
- **Chat**: the D6 bridge — host socket held by `cpd-web`, frames over SSE, submits over POST.

Why the axioms pick it: **YAGNI** — nothing in hs-d757 needs a framework or a bundler, and the async stack buys scale a localhost cockpit will not use (guess); **KISS** — one runtime, one rendering path in Rust beside the TUI, one JS file; **convention over configuration** — take the pinned dependency rather than hand-roll HTTP (D2's std::net row) or a JS framework; **one source of truth** — O1 kills the parallel read paths of 4.3; **never vendor** — the htmx files are dependencies, not copies (D5); **make it work before fast** — a sync server on localhost is correct first, fast enough later (measured TUI latencies are 50 ms coalescing + sub-second polls, 3.1; a localhost HTTP hop adds unmeasured but small latency, guess).

The cost it accepts: three new crates (HTTP + markdown renderer + sanitiser) and two npm packages, each needing a recorded User decision (`implementing.md:383,405`); `tiny_http` is **stale** (last release 2022-10, crates.io); thread-per-connection is fine at localhost scale (guess); the CLI JSON of the three commands changes (O1's contract); SSE+POST chat latency is unmeasured; C1's real-snapshot size is still open.

What evidence would change it: `tiny_http`'s abandonment on a future toolchain (swap to hyper or axum behind the same handler seam — a library change, no protocol change); a measured SSE+POST chat latency over the spike's 2.6 ms unix-socket figure (measured, `durable-leads.md` Summary: event to first frame at the client median 2.60 ms; then a WebSocket bridge, +10–15 packages, guess); a real Lead's snapshot measured multi-MB (C1 windowing — still no new dependency); pnpm `allowBuilds` friction with the two JS files (then move markdown client-side or serve from a CDN — direction changes, not stack changes). Each is a measurement or a fact, not a belief; none is known today (all open items are labelled in 3.3, 3.5 and `host-wire.md` 4).

## 6. The cut into PRs

<!-- skeleton -->

## 7. Risks

<!-- skeleton -->

## 8. Open design choices

<!-- skeleton -->

## Appendix A. Evidence index

<!-- skeleton -->
