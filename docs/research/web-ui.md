# A web UI for cpd with cockpit parity: findings and open choices

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

### 3.1 How a cockpit view learns of a change today

### 3.2 The ways a browser can get updates

### 3.3 What the host socket protocol v1 carries for a web client

### 3.4 The three bridge options A, B and C

### 3.5 The protocol changes C1 to C5

## 4. The shared read model

### 4.1 The crate graph

### 4.2 Every file of tui/src/read/

### 4.3 The three parallel paths against the cockpit's reads

### 4.4 Parity is not pinned

### 4.5 Options for one model

## 5. Stack options and the recommendation

### 5.1 The decisions of a stack

### 5.2 Cost by stack class

### 5.3 The recommendation
