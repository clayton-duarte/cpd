# Board routing: audit of what cpd already built (Plan rfm-board-routing, research 3 of 4)

Research and architecture only. No code, test or measurement changes with this document.

## Contents

1. What cpd already built (this section)
2. Measurements (later Job)
3. Evaluation (later Job)
4. Roadmap (later Job)

## Method and sources

- Read `origin/main` at `77b80a89` (`git rev-parse origin/main`, 2026-10-30 EDT). Every `file:line` below is a line of that commit; paths are repo-relative.
- Code first, docs second. Where `docs/contract.md` (`contract`) and code disagree, the code line is cited and the disagreement is named.
- Context record ids come from `cpd context search` and `cpd context show --record <id>` (run in this worktree). The Lead's brief gave five ids; four were wrong (see 1.2).
- Section 7 was read whole from the extract `/tmp/cpd-rfm-board-routing/section7.md`.
- Not done: no `~/.cpd/cpd.db` read, no ledger numbers (Job `measure`), no evaluation of the proposal (Job `design`).
- Labels: `unverified` = not checked in code or by command; `guess:` = inference.

## Source

- Proposal: `/tmp/cpd-rfm-board-routing/proposal.md` (a copy of the User's file), "CPD Context: Retrieval-First Memory Proposal".
- Section 7 = the two sections "Agent coordination: message board improvements" and "Agent coordination: what a small local model can do", lines 205-251 of the copy (`/tmp/cpd-rfm-board-routing/section7.md`).
- Bullets of the first section, by name: **Typed messages**, **Failure classes with routed escalation** (plus its 7-row table), **Retry budgets and timeouts**, **Storm grouping**, **Digests, not raw dumps**, **Reference, don't copy**, **Structured completions**, **Stuck detection**.
- Second section: a 7-row job table (Failure summarizer and classifier, Message triage, Root-cause clustering, Inbox digest, Worker first-line responder, Plan coverage check, GitHub event summary), "Keep with the larger models", **Order of adoption**, **Metrics for this part** (4 bullets).
- Sibling Plans own the memory, curation and planning ladder. Section 7 touches them only through context entry ids and `entry_reads`; this document only cites those.

## 1. What cpd already built

**Answer.** cpd has no typed board and no model in the loop, but it already routes most failure causes by deterministic rules: a ladder with budgets, same-Tier provider retry, an auto-retry when a fix merges, a stuck-Job message grouped per PR, deduped engine errors, and a Manager digest with counts. What is absent is the board's schema (`kind`, `thread_id`, `status`, refs), cross-PR cause grouping, escalation timeout climb, ping-pong detection, and every small-model job.

### 1.1 Current mechanisms

| Mechanism | What it does today | Wakes? | Evidence |
|---|---|---|---|
| `session send` | One message to a Lead or Manager as a `session-message` Event. `--unblocks <reason>` is required. Always wakes: an idle target starts a turn, a busy one gets it after its turn. Guards: no same text to two Sessions or twice to one within 60 min (`broadcast`, `duplicate`); Plan must be live | Always | `core/src/session/send.rs:56-81` (guard), `:87-104` (write), `:172-201` (command); `core/src/session/mod.rs:205-219` (`--unblocks` required); `core/src/events/typed.rs:450-453`; contract `docs/commands.md:2792` ("The message always wakes") |
| Engine message to a Session | The engine writes the same Event from the User's side (`from` null): `tell_managers` (one live Manager) and `tell` (a Lead, for E20) | Always | `core/src/session/send.rs:111-130` |
| `board post` / `list` / `take` | A row in `board_posts` addressed to a Session (`manager`, a Plan id or a Session id), 1-2000 chars, no Event, never starts a turn. `take` marks all unread posts read; the adapter calls it at turn start | Never | `core/src/board.rs:84-105` (post), `:142-153` (take); `docs/commands.md:3048-3075` |
| Events and who they wake | A Plan Lead is turned by `job-merged` (only when `data.turn`), `job-failed`, `pr-feedback`, `plan-done`, `error`, `escalation`; `pr-feedback` and `escalation` are steered, the rest follow-ups. `attempt-retry` and every other kind add nothing | Per kind | `pi/lib/delivery.ts:11-21`; contract `docs/contract.md:2635-2653` |
| Manager delivery | A Manager is woken only by `session-message` to it; Plan Events reach it as the turn-start digest | Only by message | contract `docs/contract.md:2627`; `docs/commands.md:4011-4062` |
| Turn-start digests (contract 15.7) | Lead: new decision records by others (`- <id>: <first sentence>`), pending board posts in full, PRs opened of its Plan. Manager: records, board posts in full, `Plans:` (counts per Plan by kind), `Pending work:`, `Errors: N (code xK)` with the command for the rest. Cap is lines (`MAX_LISTED = 20`), not tokens. `escalation` Events are not counted | No (folded into a turn) | `pi/lib/updates.ts:35` (cap), `:56` (counted kinds), `:196-213` (Plans), `:226-240` (Pending work), `:242-250` (Errors), `:262-290` (Lead update); `docs/commands.md:4011-4062` |
| Worker Escalations (`ask` / `answer`) | `cpd ask` blocks the Worker, opens an `escalations` row, sets the Worker `waiting`; the Lead is turned (steered) by `escalation`. Expiry: `ask` itself at `timeouts.ask_minutes` (default 60, exit 5 `ask_timeout`); engine E1 expires when the asker pid is dead (`asker_dead`). No climb to the Manager | Lead, at once | `core/src/ask/question.rs:47-69` (timeout), `:145-150` (waiting); `engine/src/watch/escalations.rs:1-75` (E1); `docs/contract.md:1055-1062`, `:4115` |
| Retry ladder (Job/Attempt) | `crashed`, `no_submit`, `merge_conflict` are ladder causes: the Job is requeued one Tier up while automatic Attempts < `1 + retry_cap` (default 2). A driver that never started, or one ended by an install, is requeued at the same Tier and not counted. `provider_error` retries at the same Tier at most 3 times, 30 s apart. Every other cause fails the Job | Not until the Job fails | `core/src/attempt/ledger.rs:233-235`, `:239-242`, `:462-495` (end of a failed Attempt), `:517-541` (counts), `:545-575` (`next_rung`); `engine/src/watch/schedule.rs:25-64` (spawn back-off `min(2^k, 300)` s), `:247` (hold); `engine/src/watch/provider_hold.rs:10-31`; contract `docs/contract.md:889`, `:974-979` |
| Attempt causes (all) | `setup_failed`, `timed_out`, `crashed`, `no_submit`, `merge_conflict`, `cas_lost`, `dirty_target`, `tampered`, `gate_failed`, `gate_error`, `provider_error` (plus `merged`, `cancel_requested`) | See 1.5 | `core/src/attempt/ledger.rs:163-186`; schema in 1.4 |
| Inbox rules | `core/src/inbox/rules.rs` has no failure kind. Kinds it computes: `dialog` (a blocked Session asking the User), `loop-guard` (a Session blocked by the self-turn breaker), `plan-review`, `review-waiting`, `unread`, `curate-*`, `deploy`, `drafts-parked`, `models-changed`, `review-requested`, `pr-ready`. A stuck Job is explicitly no condition: `stuck_jobs` is read by E8 and sent to the Manager as a message. `job-stuck`, `question`, `pr-failing`, `pr-changes` open nothing | Inbox is for the User; no agent wake | `core/src/inbox/rules.rs:1-5`, `:67-91` (list), `:297` (dialog), `:318` (loop-guard), `:486-540` (stuck Jobs), `:1250` (test: a stuck Job opens no condition), `:1356`, `:1385` (tests: failed checks and changes-requested open nothing); `engine/src/watch/inbox.rs:12-40` |
| Stuck-Job message (E8) | A `failed` Job whose latest `job-failed` is older than `inbox.stuck_job_minutes` (default 10) and not being worked on (pro-d88) is told to the Context's Manager once per PR, one message listing each Job with its minutes and cause; sent again only after a newer failure | Manager, waking | `core/src/inbox/rules.rs:486-568`; `engine/src/watch/report.rs:154-213`; `core/src/job/worked_on.rs:1-47`; `docs/contract.md:4134` |
| Fix auto-retry | A merged non-open, non-update Job requeues failed `open`/`update` Jobs of its PR when no other newer Job is queued or running | No | `core/src/job/fix_retry.rs:11-39`; called at `core/src/attempt/ledger.rs:501-503` |
| Engine `error` Event dedup | One `error` Event per (step, scope, code) until it clears, and at most one per `watch.error_repeat_seconds` (default 600), also across engine restarts (reads the store). No message is sent for it | `error` turns a Lead; Manager sees a count | `engine/src/watch/report.rs:231-292` (in-memory and store windows), `:326` (test); `docs/contract.md:1134`, `:4129` |
| E20 parked drafts | The one engine wake of a Lead by rule: a message when a Plan has parked drafts and its Lead is idle; the message is the record it was sent | Lead, waking | `engine/src/watch/parked.rs:17-104`; `docs/contract.md:1100` |
| Loop guard | Blocks a Session's further self-started turns past `loop_guard_turns` (default 20) | Opens a red Inbox row | `core/src/session/mod.rs:54`; `core/src/inbox/rules.rs:318-331`; `docs/contract.md:4108` |

Note, contract vs code: `docs/contract.md:2602` says the engine's session-message has `wake` false, and `:2627` says the engine's step errors reach the Manager as messages. The code always wakes (`core/src/session/send.rs:87-104`, `:111-130`) and sends no message for a step error (`engine/src/watch/report.rs:430` test `a_step_error_is_an_event_and_sends_no_message`; `:231-292`). `docs/contract.md:2653` ("a turn always, whatever `data.wake`") agrees with the code.

### 1.2 The five items the brief listed

Ids checked with `cpd context show --record <id>`. The brief's ids did not match its Plan names; the right records are below.

| Brief | Verdict | What it does | Evidence | Covers (a)-(d) of the proposal |
|---|---|---|---|---|
| Worker Escalations dropped from Manager digests (pro-d30 / esc-digest, pro-p5) | pro-d30 is wrong ("No test limit"). Right: **pro-d24** (decision, 2026-10-23) and **pro-p5** (Plan, "Manager digest: drop Worker Escalations"); Plan name `esc-digest` is in pro-d24's text | The Manager's `Plans:` digest no longer counts Worker `escalation` Events: the Lead answers or asks the User, the Manager has no action | `pi/lib/updates.ts:56` (the six counted kinds; `escalation` is not one); `docs/commands.md:4011-4062`; `docs/contract.md:2627` | (c) digests, partly: routes by role, removes a class of noise. (a) typed routing, partly (by Event kind). Not (b) |
| Provider errors retry at the same Tier (pro-d99 / outage-retry) | pro-d99 is wrong (deploy-inbox). Right: **pro-d101** (no ladder climb) and **pro-d102** (Plan `outage-retry`: N=3, 30 s, same Tier) | A pi stop on `Connection error.`, `Request timed out.` or HTTP 5xx ends the Attempt `provider_error`; the Job is requeued at the same Tier at most 3 times, held 30 s by E3; the 4th fails the Job and the Lead decides | `core/src/driver/run.rs:122-136` (classify), `:811-814` (end); `core/src/attempt/ledger.rs:239-242`, `:462-495`, `:551-556`; `engine/src/watch/schedule.rs:247`; `engine/src/watch/provider_hold.rs:10-31` | (b) retry budget before anyone is woken: BUILT for this one class. Also the proposal's "flaky, transient infra -> engine retry" row, for provider outages |
| Engine auto-retries a failed open/update Job when its fix merges (pro-d92 / fix-retry) | pro-d92 is wrong (resources). Right: **pro-d89** (Plan `fix-retry`, 2026-10-27) | The Job whose fix merged is requeued at its last Tier with a `job-retried` Event; no Lead action | `core/src/job/fix_retry.rs:11-39`; `core/src/attempt/ledger.rs:501-503` | (b), partly: removes a wake (the Lead's idle gap) and a Manager nudge. Not a retry-for-flakiness |
| Suppress "not retried" nudges while a fix is underway (pro-d90 / fail-handled) | pro-d90 is wrong (build-cap). Right: **pro-d88** (Plan `fail-handled`, 2026-10-26 EDT) | A failed Job is "being worked on" while the Lead is `working`, a newer Job of its PR is queued or running, or a retry is queued; one predicate drives the TUI red loader and E8's stuck-Job message | `core/src/job/worked_on.rs:1-47`; `core/src/inbox/rules.rs:486-520` | (d) stuck detection, partly (the false-positive guard), and (b) |
| Repeated engine `error` Events deduped (pro-d187 / pr-and-bot-error-dedup) | pro-d187 confirmed (2026-10-29): bot detection plus restart dedup. Plan name `pr-and-bot-error-dedup`: unverified (not in the record text read) | One `error` Event per (step, scope, code) per window, window held across engine restart | `engine/src/watch/report.rs:231-292`; `docs/contract.md:1134` | (c) digests, partly: bounds the `Errors:` count. (d) none |

### 1.3 Where cpd is stronger than the proposal

| Proposal item | What cpd already does, deterministically | Where the proposal would be worse | Evidence |
|---|---|---|---|
| Wake derived by rule from kind, not chosen by sender | Wake is a table by Event kind (`LEAD_MESSAGES`), not sender input; `attempt-retry` never wakes | A `task`/`fyi` board message that wakes would add a wake path beside two that already exist (`session send` always, `board post` never). `session send` already refuses broadcasts and duplicates | `pi/lib/delivery.ts:11-21`; `core/src/session/send.rs:56-81` |
| Failures route by class | The class is the Attempt `cause`, a CHECK-constrained column, set by the code that sees the failure (driver, merge, E2), not inferred from a log | A model classifier reading a CI log infers a class cpd already stores | `core/src/attempt/ledger.rs:163-186`; `core/src/driver/run.rs:385-817`; `core/src/merge/mod.rs:148-423` |
| Retry budget "K before anyone is woken" | `retry_cap`, 3 provider retries, spawn back-off are in code; a retry appends `attempt-retry`, which wakes no one | A K per class in a message router duplicates the ledger's own counters (`automatic_attempts`, `provider_errors`) | `core/src/attempt/ledger.rs:517-541`; `pi/lib/delivery.ts:11-21` |
| Merge conflict goes to the Lead | A conflict with the base queues an `update` Job (no Lead wake); a conflict inside an Attempt climbs the ladder with the conflict files in the next Spec; the Lead is told only when the Job fails | The proposal's row wakes the Plan Lead on every cross-PR conflict | `core/src/pr/poll.rs:432-447`; `core/src/spec/mod.rs:71-72`; `core/src/attempt/ledger.rs:233-235` |
| Storm grouping | One stuck-Job message per PR; one `error` Event per (step, scope, code) per window, restart-safe | Grouping by embedding plus a pairwise model check where a key (PR, step, code) already exists | `core/src/inbox/rules.rs:540-568`; `engine/src/watch/report.rs:231-292` |
| Dedup and fix-retry | The engine retries a failed open/update Job when its fix merges; it does not ask anyone | Anything that messages a Lead "your fix merged, retry" adds a wake | `core/src/job/fix_retry.rs:11-39` |
| Escalation to a higher level | The stuck-Job message is the climb for failed Jobs: Lead first (job-failed turn), Manager after `stuck_job_minutes`, and only when not worked on | A time-based climb of every unacknowledged message would wake the Manager for Jobs a Lead is fixing (pro-d88 was written for exactly that false alarm) | `core/src/inbox/rules.rs:486-520`; Context record pro-d88 |
| Digest | The Manager already gets counts per kind plus an errors summary, with the command for the rest | A free-text digest drops the structure the counts keep. Note: cpd's cap is 20 lines, not tokens | `pi/lib/updates.ts:35`, `:196-250` |

### 1.4 Actual schema (columns only)

Source: `docs/contract.md` section 4.4 (`CREATE TABLE` blocks), which `docs/contract.md:2404` and the migrations in `core/src/store/migrations/` keep in step. The first migration holds the original columns (`0001-initial.sql:152`, `:196`, `:218`, `:242`; `board_posts` in `20261021T1200-board-posts.sql:4`); later migrations add the columns marked. `unverified` for any later migration's column not named in the contract block (the contract block, not a live database, is the source: no `~/.cpd/cpd.db` was read).

| Table | Columns, in order | Evidence |
|---|---|---|
| `board_posts` | `id`, `at`, `context`, `from_session`, `to_session`, `text`, `read_at` | `core/src/store/migrations/20261021T1200-board-posts.sql:4-12`; `docs/contract.md:1860-1869` |
| `events` | `id`, `txn`, `at`, `kind` (references `event_kinds(name)`), `context`, `plan`, `job`, `attempt`, `session`, `text`, `data` (JSON object) | `docs/contract.md:1752-1766`; original `0001-initial.sql:242-260` (kind was a CHECK list) |
| `attempts` | `plan`, `id`, `job`, `number`, `tier`, `origin`, `status`, `outcome`, `cause`, `branch`, `worktree`, `base_sha`, `driver_pid`, `worker_session`, `spec_path`, `submitted_sha`, `submitted_at`, `check_tail`, `merged_sha`, `nudged`, `stop_request`, `metrics` (JSON), `started_at`, `ended_at`, `runner`, `worker_pid` | `docs/contract.md:1615-1655`; original `0001-initial.sql:152-186` (no `runner`, `worker_pid`; `cause` lacked `gate_failed`, `gate_error`, `provider_error`) |
| `sessions` | `id`, `role`, `context`, `plan`, `attempt`, `name`, `file`, `forked_from`, `cwd`, `tier`, `tmux_pane`, `state`, `state_label`, `state_at`, `created_at`, `archived_at`, `title`, `titled_at`, `asleep_at`, `by_user`, `switch_failed` | `docs/contract.md:1719-1746`; original `0001-initial.sql:218-238` |
| `escalations` | `plan`, `id`, `attempt`, `number`, `question`, `status` (`open`/`answered`/`expired`), `answer`, `asker_pid`, `asked_at`, `answered_at`, `expired_at` | `docs/contract.md:1697-1713`; `0001-initial.sql:196-214` |

What the board schema lacks next to Section 7: no `kind`, `thread_id`, `status`, `refs`, `priority`, `wake`. `board_posts.read_at` is the only state. `session-message` has `data.{from, text, wake}`, and a `session send` one has `{from, text, unblocks}` instead of `wake`; neither has a thread or refs (`core/src/events/typed.rs:446-453`). `events` already holds refs as columns (`plan`, `job`, `attempt`, `session`).

### 1.5 Failure cause map (baseline for the proposal's failure classes)

`attempts.cause` values and what the code does next. "Fails the Job" appends `job-failed`, which turns the Lead (`pi/lib/delivery.ts:13`), is counted in the Manager digest (`pi/lib/updates.ts:56`, `:213`), and after `stuck_job_minutes` without work becomes one message per PR to the Manager (`core/src/inbox/rules.rs:486-568`).

| Cause | Set where | Next, in code | Wake | Evidence |
|---|---|---|---|---|
| `setup_failed` | driver: worktree or `pre-start` hook failed | Not a ladder cause: fails the Job | Lead (job-failed) | `core/src/driver/run.rs:385`, `:400-416`, `:454`, `:462`; `core/src/attempt/ledger.rs:233-235`, `:551-560` |
| `timed_out` | E2 sets `stop_request` after `attempt_minutes` (30; explore 20); the driver ends it | Fails the Job | Lead | `engine/src/watch/liveness.rs:215-240`; `core/src/driver/run.rs:710-714` |
| `crashed` | pane dead, pi stopped badly, driver died (E2), merge recovery | Ladder: requeue one Tier up while automatic Attempts < `1 + retry_cap`; driver never started or ended by install: same Tier, uncounted, E3 back-off | None until the ladder is spent, then Lead | `core/src/driver/run.rs:738-817`; `engine/src/watch/liveness.rs:160-180`; `core/src/merge/recover.rs:101`; `core/src/attempt/ledger.rs:462-495`, `:551-575`; `engine/src/watch/schedule.rs:25-64` |
| `no_submit` | idle twice without `attempt submit`, nudged once between | Ladder, as `crashed` | None until spent | `core/src/driver/run.rs:122-135`, `:806-810`; `core/src/attempt/ledger.rs:233-235` |
| `merge_conflict` | merge step 5: rebase conflicted | Ladder; next Spec lists the conflict files | None until spent | `core/src/merge/mod.rs:235-255`; `core/src/spec/mod.rs:71-72` |
| `provider_error` | pi stop on connection error, timeout or 5xx | Same Tier, at most 3 per Job since the last `job retry`, 30 s hold; 4th fails the Job | None until spent, then Lead | `core/src/driver/run.rs:131-134`, `:811-814`; `core/src/attempt/ledger.rs:239-242`, `:462-495`; `engine/src/watch/provider_hold.rs:10-31` |
| `cas_lost` | merge step 8: Lead Branch kept moving | Not a ladder cause: fails the Job | Lead | `core/src/merge/mod.rs:312` |
| `dirty_target` | named in the type and the CHECK; `rg` finds no producer outside the definition and `spec/mod.rs:81` | None found: unverified whether any path sets it | n/a | `core/src/attempt/ledger.rs:178`; `core/src/spec/mod.rs:81` |
| `tampered` | diff touches protected paths (merge, or earlier at `attempt submit`) | Fails the Job (the submit-time check refuses to the Worker instead) | Lead | `core/src/merge/mod.rs:148`, `:160`, `:207`, `:221`; `core/src/attempt/submit.rs:228` |
| `gate_failed`, `gate_error` | merge step 6: the quick check or Gate exited non-zero, or could not be judged | Not ladder causes (contract: "the Lead decides"): fails the Job | Lead | `core/src/merge/mod.rs:413-423`; `docs/contract.md:979` |
| `merged`, `cancel_requested` | merge succeeds, or cancel | Not failures. A merge also requeues failed open/update Jobs it fixes. An incident-mode stop requeues at the same Tier | none | `core/src/attempt/ledger.rs:395-420`, `:501-503`; `core/src/job/fix_retry.rs:11-39` |
| (not an Attempt cause) escalation | Worker `cpd ask` | Lead is steered; expires at `ask_minutes` or when the asker dies; no climb | Lead | `core/src/ask/question.rs:47-69`; `engine/src/watch/escalations.rs:1-75`; `pi/lib/delivery.ts:16` |
| (not an Attempt cause) PR conflict | E5 poll sees `mergeable` become `conflicting` | Queues an `update` Job; no Lead wake | none | `core/src/pr/poll.rs:432-447` |
| (not an Attempt cause) PR checks failed or review changed | E5 poll | `pr-feedback` Event: Lead steered | Lead | `pi/lib/delivery.ts:14`; `docs/contract.md:2573` |
| (not an Attempt cause) engine step error | any step | one `error` Event (deduped); Lead turned if about its Plan; Manager counts it | Lead (if its Plan) | `engine/src/watch/report.rs:231-292`; `pi/lib/delivery.ts:15` |

Proposal's 7-row table against this map: "lint, format, simple compile -> Worker retry with the error" has no engine path: those fail at merge as `gate_failed` and the Lead decides (the Worker sees a refusal only for the tamper, red-test-guard and kind checks at submit, `core/src/attempt/submit.rs:228-236`). "Flaky -> engine retry" exists only for provider and spawn failures; a flaky test is `gate_failed`. "Requirement conflict" and "cross-feature dependency" have no class, only the manual `board post --to manager` and `session send`.

### 1.6 Section 7 element -> what cpd has today

| Section 7 element | Status | What exists, and what is missing | Evidence |
|---|---|---|---|
| Typed messages: `kind` | PARTLY | Events are typed (`events.kind` references `event_kinds`; the adapter has a delivery table); board posts and `session-message` are free text with no kind | `docs/contract.md:1752-1766`; `pi/lib/delivery.ts:11-21`; `core/src/board.rs:84-105` |
| Typed messages: `thread_id` | ABSENT | no column or field; `rg thread_id` in `core`, `pi`, `docs` hits only `docs/research/rfm-curation-*` | `core/src/store/migrations/20261021T1200-board-posts.sql:4-12`; `rg -l 'thread_id\|entry_reads' core pi docs` |
| Typed messages: `status` open/acked/resolved | ABSENT for messages; PARTLY for Escalations (`open`/`answered`/`expired`) | `board_posts` has only `read_at`; `escalations.status` exists | `docs/contract.md:1697-1713`; `core/src/board.rs:147` |
| Typed messages: refs to entry ids, PRs, jobs | PARTLY | Events carry `plan`, `job`, `attempt`, `session` columns and texts end in a `Next:`/`See:` command; no refs on board posts; Lead update shows record ids | `docs/contract.md:1752-1766`; `pi/lib/updates.ts:262-290`; `core/src/attempt/ledger.rs:440-495` |
| Typed messages: wake derived by rule from kind and priority | PARTLY | Wake is by Event kind (BUILT); no priority; `session send` always wakes and `board post` never does, so the sender picks the channel | `pi/lib/delivery.ts:11-21`; `core/src/session/send.rs:87-104`; `core/src/board.rs:1-3` |
| Failure class: flaky or transient -> engine retry | PARTLY | provider errors, spawn failures and install kills retried by the engine; a flaky test is `gate_failed` and wakes the Lead | 1.5 rows `provider_error`, `crashed`, `gate_failed` |
| Failure class: lint, format, simple compile -> Worker retry with the error | PARTLY | submit-time refusals (tamper, red-test guard, kind check) go to the Worker; the quick check and Gates run at merge and fail the Job | `core/src/attempt/submit.rs:3`, `:228-236`; `core/src/merge/mod.rs:413-423` |
| Failure class: code bug or test failure -> Plan Lead | BUILT | `job-failed` turns the Lead | `pi/lib/delivery.ts:13` |
| Failure class: ambiguous spec, missing context -> Plan Lead | BUILT | Worker `ask` steers the Lead; `answer` returns to the Worker | `core/src/ask/question.rs:47-69`; `pi/lib/delivery.ts:16` |
| Failure class: merge conflict across PRs -> Plan Lead | PARTLY (cpd routes differently) | conflict with base -> auto `update` Job; conflict in an Attempt -> ladder; Lead on failure only. No class for a conflict between two PRs | `core/src/pr/poll.rs:432-447`; `core/src/merge/mod.rs:235-255` |
| Failure class: requirement conflict or scope change -> Context Manager | ABSENT as routing | no engine class; a Lead may `board post --to manager` or `session send` by hand | `core/src/board.rs:61-80`; `docs/commands.md:3048-3075` |
| Failure class: cross-feature dependency -> Manager relay | ABSENT | no relay path between managers; Manager sees touched files read-side in `Pending work` (`cpd context pending`) | `pi/lib/updates.ts:226-240`; `core/src/context/pending.rs:155` |
| Retry budgets (K before anyone is woken) | BUILT | `retry_cap` (2), 3 provider retries, spawn back-off; `attempt-retry` wakes no one. K is fixed per cause, not configurable per class (provider N is a constant) | `core/src/attempt/ledger.rs:239-242`, `:545-575`; `pi/lib/delivery.ts:11-21`; `docs/contract.md:4098` |
| Timeout: unacknowledged escalation climbs one level after N min | PARTLY | Failed Jobs climb Lead -> Manager after `stuck_job_minutes` (10) unless worked on; Escalations only expire at `ask_minutes` (60) with no climb; messages have no ack | `core/src/inbox/rules.rs:486-568`; `core/src/ask/question.rs:47-69`; `engine/src/watch/escalations.rs:1-75` |
| Storm grouping (12 jobs, one cause, one message) | PARTLY | One stuck-Job message per PR and one `error` Event per (step, scope, code) per window; 12 failures on 12 PRs stay 12 messages; grouping is by key, not cause | `core/src/inbox/rules.rs:540-568`; `engine/src/watch/report.rs:154-213`, `:231-292` |
| Digests with token cap, folded at next turn | PARTLY | Manager and Lead turn-start digests exist (counts, errors summary); cap is 20 lines, board posts are in full, and `job-failed` reaches a Lead as one turn per Event; no token cap | `pi/lib/updates.ts:35`, `:196-290`; `docs/commands.md:4011-4062` |
| Reference, don't copy: entry ids | PARTLY | Lead update shows `- <id>: <first sentence>` and the pointer to `cpd context show --record <id>`; board posts hold text | `pi/lib/updates.ts:262-290`; `docs/commands.md:4011-4020` |
| Reference, don't copy: log bundles as `entry_reads` | ABSENT | no `entry_reads` table or code in `core`, `pi`, `engine` (belongs to the memory sibling Plan) | `rg -l 'entry_reads' core pi engine docs` -> only `docs/research/rfm-curation-*` |
| Structured completions (proof artifacts in typed fields) | PARTLY | `job-merged` has `{sha, tier, kind, turn}`; explore Jobs carry `summary` and `artefact`; `attempts.check_tail` and `metrics` are stored. No screenshots, test output or contract status as typed proof fields | `docs/contract.md:2563`, `:2621`; `core/src/attempt/submit.rs:146-175`; schema 1.4 |
| Stuck detection: unacknowledged message | ABSENT | board and `session-message` have no ack and no age check | `core/src/board.rs:1-3`; `core/src/session/send.rs:87-104` |
| Stuck detection: two agents ping-pong | ABSENT | `session send` refuses same-text repeats in 60 min; loop guard counts one Session's self-started turns, not a pair | `core/src/session/send.rs:56-81`; `core/src/inbox/rules.rs:318-331` |
| Stuck detection: lead blocked on manager | ABSENT | `blocked` Leads show as Inbox `dialog` for the User; nothing relates a Lead to a Manager message | `core/src/inbox/rules.rs:297-316` |
| Stuck detection: failed Job nobody handles (not in proposal) | BUILT | E8 stuck-Job message | `core/src/inbox/rules.rs:486-568` |
| Small model: failure summarizer and classifier | ABSENT | causes are set by code; `check_tail` and the gate log are the raw tail; the spec gives a rule-built sentence per cause | `core/src/spec/mod.rs:60-95`; `core/src/merge/mod.rs:395-425` |
| Small model: message triage | ABSENT | no model reads a message; wake is a rule by kind | `pi/lib/delivery.ts:11-21` |
| Small model: root-cause clustering | ABSENT | grouping is by PR key, not embeddings (the curator's embeddings belong to a sibling Plan) | `core/src/inbox/rules.rs:540-568` |
| Small model: inbox digest | ABSENT as a model; a rule digest exists | counts and first sentences, no model | `pi/lib/updates.ts:196-290` |
| Small model: Worker first-line responder | ABSENT | `ask` goes to the Lead; nothing answers from the plan | `core/src/ask/question.rs:47-69` |
| Small model: Plan coverage check | ABSENT | `rg -n coverage core/src/plan` finds no file; no brief-to-PR coverage check found (other places unverified) | `rg -n coverage core/src/plan` (no match) |
| Small model: GitHub event summary | PARTLY (rule-built, no model) | the poll turns PR checks, reviews and comments into `pr-feedback` Events with Rust-built text | `docs/contract.md:2573`; `core/src/pr/poll.rs:432-447` |
| Adoption order, each stage in shadow mode | ABSENT for this part | no shadow mode for board routing; shadow and `off`/`on` exist for context curation (sibling Plan) | `docs/research/rfm-curation-models.md` (cited only) |
| Metrics: wake-ups per feature by role | ABSENT | no counter of wakes; Events and `session-message` rows could be counted (Job `measure`) | `core/src/session/send.rs:87-104`; `pi/lib/delivery.ts:11-21` |
| Metrics: tokens read per lead and manager turn | PARTLY | `attempts.metrics` holds tokens per Worker Attempt only | `core/src/metrics/mod.rs:1-12` |
| Metrics: share of escalations that reached a higher level than needed | ABSENT | no level or need marker on a message | schema 1.4 |
| Metrics: classifier agreement with a human label | ABSENT | no classifier, no label | n/a |

### Findings that change the later Jobs

- The brief's ids pro-d30, pro-d99, pro-d92 and pro-d90 point at other decisions; use pro-d24, pro-d101/pro-d102, pro-d89 and pro-d88.
- The proposal's classes map onto `attempts.cause`, a column cpd already fills by code. The classifier's job would be the one gap: `gate_failed` and `gate_error` carry the CI/test log, which is where bloat and misrouting sit.
- Three Section 7 stuck rules (ping-pong, unacknowledged message, Lead blocked on Manager) have no data to run on: board posts have no ack, thread or age field.
- `dirty_target` is declared but has no producer found by `rg`; contract line 979 and the code agree it is not a ladder cause.
