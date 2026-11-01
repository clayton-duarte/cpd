# Glossary rename research: inventory and the artifact question

Attempt `research-docs-a1` of Job `research-docs`, Plan `glossary-rename`. Prior art: `docs/research/glossary-audit*.md` (18 part files; `glossary-audit.md` the summary, `glossary-audit-rename.md` the rename map), collected 2026-10-28 and now stale in places; every row here is refreshed against the current tree and marked so. Decided and not reopened (Context records `pro-d136`, `pro-d138`, 2026-10-28): the object is a Workflow (Plan > Workflows > Jobs); **PR means only the pull request** on GitHub; history keeps the old words; no alias, no shim. This report changes nothing else and decides nothing: the artifact question is the User's, at the Plan review. Parts 3–4 (the design, blast radius, staging, store migration, cost and order) belong to the second Job of this Plan and need this one.

This document speaks the target vocabulary: it says Workflow for the object wherever it points forward. A quote of current code, an identifier or a doc keeps the current words and says so.

## Part 1 — The inventory

The live surfaces still call a Workflow `pr`/`PR`/`prs`; `CONTEXT.md`'s transitional note says so (CONTEXT.md:16, refreshed). Every occurrence below is classified into exactly one:

- **(a)** user/agent-facing interface that must change — a command, flag, refusal, help or agent text.
- **(b)** internal identifier that could change — a Rust type, field, function, local, test name, internal JSON key, log file name.
- **(c)** stable external contract that changes only with a PROTOCOL bump — an Event kind, Event data field or command output the pi adapter reads. `core/src/events/kind.rs:37` states when PROTOCOL must rise; `pi/lib/event.ts:4-9` mirrors it.
- **(d)** history that stays — ADRs, `docs/archive/`, `docs/research/`, old Context records, applied migrations, stored Event rows and `cancel_reason` values, `core/src/store/schema.rs` legacy and fixture lists.

PROTOCOL's current value: **19** (`core/src/events/kind.rs:38`, mirrored `pi/lib/event.ts:9`). The audit summary said 17 (docs/research/glossary-audit.md:17); the audits' rename map recorded 17 (`core/src/events/kind.rs:38` with `pi/lib/event.ts:9`, docs/research/glossary-audit-rename.md:14); it has since moved to 19 — refreshed, current number 19. Nine Event kinds still carry the old word (`core/src/events/kind.rs:97-105`, refreshed; the audit-pi part counted 8 plus `pr-feedback`).

### 1.1 CLI commands

| Occurrence | Where | Class | Audit row | Status |
|---|---|---|---|---|
| `cpd pr add`, `pr edit`, `pr describe`, `pr cancel`, `pr start` | cli/src/interface.rs:69-73 | (a) | glossary-audit-cli.md (interface.rs:69-73), rename map row 1 (interface.rs:69-73) | refreshed, lines unchanged |
| `cpd pr open`, `pr rebase`, `pr push` | cli/src/interface.rs:74, 76, 77 | (a) (mixed sense: acts on the Workflow's Lead Branch and reaches its pull request) | rename map row 3 (interface.rs:74,76,77) | refreshed |
| `cpd pr poll`, `pr reply`, `pr post`, `pr merge` | cli/src/interface.rs:75, 79-82 | (a); `pr poll`/`post`/`merge` are the pull request's own verbs (mechanical-stay candidates), `pr merge` is a cockpit action (interface.rs:82) | rename map row 5 (interface.rs:75,79-81) | refreshed; `pr merge` now at :82 (new since the map) |
| `cpd pr view` | cli/src/interface.rs:78 | (a) (mixed sense: reads the pull request, selected by a Workflow key) | rename map row 4 (interface.rs:78) | refreshed |
| group help `Pr(pr::Command)` | cli/src/cli.rs:60 | (a) | rename map row 6 (cli/src/cli.rs:58-60) | refreshed, now :60 |
| `Review(pr::review::Command)` | cli/src/cli.rs:63 | (a) | newly found (the audit's rows named only `Pr(pr::Command)`) | new |
| run arms `Command::Pr(c) => pr::run(...)`, `Command::Review(c) => pr::review::run(...)` | cli/src/cli.rs:184-185 | (b) | new | new |
| protocol handshake field `"protocol"` (not the word, listed to pin the handshake) | cli/src/cli.rs:299, 405, 407 | (b) | — | new |

### 1.2 Flags

| Occurrence | Where | Class | Audit row | Status |
|---|---|---|---|---|
| `--pr <key>` flag, clap `long = "pr"`, struct field `pr: Option<PrKey>` | core/src/args.rs:37, 41-42 | (a) | rename map row 7 (args.rs:41) | refreshed |
| one-PR selection rule and its messages ("Plan {plan} has no PR: add one with cpd pr add, then pass --pr <key>."; "has {n} PRs …") | core/src/args.rs:49, 52, 64, 72 | (a) | new (the map cited the flag, not these strings) | new |
| `--pr` on `job add`/`job edit`, `policy show` (`pr_policy`, `pr_policy_resolved`, `pr_policy_in`, `policy_of_pr`) | core/src/pr/mod.rs:122, 272-287, 307 | (b) | rename map row 7 (core/src/job/mod.rs:135, core/src/pr/mod.rs:201) | refreshed; current lines differ (new) |
| `--until-pr` flag | core/src/incident.rs:41 | (a) | rename map row 8 (incident.rs:41) | refreshed |
| `--until-pr` parse error text | core/src/incident.rs:157-160 | (a) | new | new |
| stored column `until_pr`, Event data key `until_pr` | core/src/incident.rs:90, 102, 104, 175-176, 183, 223, 225; core/src/error.rs:161 (`until_pr_not_incident`) | (b) for the column, (a) for the refusal code | rename map row 8 | refreshed with current lines (new) |
| `end_if_until_pr_merged` function | core/src/incident.rs:314 | (b) | new | new |
| placeholder keys `pr-` prefixed when a key does not start with a letter (`migrated_pr_key`, cut to 24) | core/src/store/schema.rs:391-398 | (b) | new | new |

### 1.3 Refusal codes

All in `core/src/error.rs` unless noted. The codes are printed to the user and listed in `docs/commands.md` §1.3 and `docs/contract.md` §1.2's table, so they are (a); the enum variants are (b). Audit rows: glossary-audit-core.md (error codes), rename map (rows on `pr_not_ready` disagreement).

| Code (variant) | Where | Class | Status |
|---|---|---|---|
| `until_pr_not_incident` (`UntilPrNotIncident`) | core/src/error.rs:161 | (a) | refreshed (new line) |
| `base_pr_unknown`, `base_pr_unavailable`, `base_pr_other_repo` | core/src/error.rs:179-181 | (a) | refreshed |
| `pr_other_plan` (`PrOtherPlan`) | core/src/error.rs:182 | (a) | refreshed |
| `explore_pr_forbidden` (`ExplorePrForbidden`) | core/src/error.rs:184 | (a) | refreshed |
| `pr_start_failed` (`PrStartFailed`) | core/src/error.rs:288 | (a) | refreshed |
| `pr_required`, `invalid_pr_id`, `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed`, `pr_title_required` | core/src/error.rs:289-298 | (a) | refreshed |
| `pr_adopted`, `pr_not_closed`, `pr_not_open` | core/src/error.rs:307-309 | (a) | refreshed (new line; audit had no fixed line) |
| `pr_not_found` (`PrNotFound`, exit 4) | core/src/error.rs:342 | (a) | refreshed |
| docs count: `pr_locked` 26 lines, `pr_in_use` 22, `pr_start_failed` 16, `pr_other_plan` 15, `pr_ended` 13, `pr_not_ready` 11, `pr_adopted` 11, `pr_unknown` 10, `pr_unavailable` 10, `pr_posts` 32, `pr_needs` 29, `pr_number` 18, `pr_url` 13, `pr_poll_seconds` 12, `pr_needs_no_delete` 12 across `docs/contract.md` and `docs/commands.md` | docs/contract.md, docs/commands.md (grep counts, current tree) | (a) | refreshed (audit parts gave prose counts; these are current grep line counts) |

### 1.4 The store

| Occurrence | Where | Class | Audit row | Status |
|---|---|---|---|---|
| tables `prs` and `pr_needs`, their history triggers (`create_history_triggers(conn, "prs", true)`, `(conn, "pr_needs", false)`) | core/src/store/schema.rs:190-191 | (b) (the rename redesigns the store; see the second Job) | glossary-audit-schema.md; rename map (store rows) | refreshed |
| `prs` history trigger rebuilt at reopening (§4.5) | core/src/store/schema.rs:223 | (b) | new | new |
| `pr_posts` in the legacy `plan_deleted` match, `pr_needs`, `plan_reviews` | core/src/store/schema.rs:456 | (b) | new | new |
| tables list including `prs`, `pr_needs` | core/src/store/schema.rs:571-572 | (b) | new | new |
| no generated `_no_delete` for `pr_needs` | core/src/store/schema.rs:586, 597 | (b) | new | new |
| `plans` gains `pr_title`, `pr_review_decision`, `pr_checks` (migration comment) | core/src/store/schema.rs:155-156 | (b) | new | new |
| `FILL_PRS` marker string | core/src/store/schema.rs:179 | (b) | new | new |
| `fill_prs` legacy rebuild: `INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pr_seen, pr_polled_at, pr_title, pr_review_decision, pr_checks, all_merged_at, …)`, phase map `WHEN 'pr' THEN 'open' WHEN 'done' THEN 'merged' END` | core/src/store/schema.rs:367-379 | (d) (legacy rebuild of an applied migration) | new | new |
| applied migrations: `0006-pr-poller.sql`, `0011-pr-mergeable.sql`, `0013-pr-inbox.sql` (Legacy list) | core/src/store/schema.rs:54, 59, 61 | (d) | glossary-audit-schema.md | refreshed |
| applied dated migrations: `20261019T1648-pr-cancel-started.sql`, `20261019T2302-pr-cancel-open.sql`, `20261020T0045-pr-title.sql`, `20261020T0056-stacked-prs.sql`, `20261020T0223-event-pr-base-changed.sql`, `20261026T1945-pr-comments.sql`, `20261029T0900-pr-adopt.sql` | core/src/store/migrations/ | (d) | new (the audit's schema part predates several) | new |
| live row counts quoted by the rename map (`prs` 298, `pr_needs` 223, `jobs.pr` non-null 1741, `pr-%` Events 1120, `plan_reviews` 114) | docs/research/glossary-audit-rename.md:9-10, 13 | (d) (a snapshot of that day's store; not re-measured here) | as stated | carried over unverified |

### 1.5 Event kinds and their data

| Occurrence | Where | Class | Audit row | Status |
|---|---|---|---|---|
| kinds `pr-feedback`, `pr-added`, `pr-edited`, `pr-described`, `pr-cancelled`, `pr-started`, `pr-base-changed` | core/src/events/kind.rs:97-103 | (c) (the pi adapter reads the kind strings; PROTOCOL must rise) | glossary-audit-pi.md, rename map Event rows | refreshed |
| kinds `pr-opened`, `pr-merged` (where the Workflow object is meant: the object's status change is the payload's subject; the pull-request facts ride along in `number`/`url`) | core/src/events/kind.rs:104-105 | (c), sense contested in the audits (ct1: pull request; schema/core/tui/cm2: mixed) — noted, not decided | rename map "JSON output keys" and D1 | refreshed |
| PROTOCOL `pub const PROTOCOL: u32 = 19` with the mirror comment | core/src/events/kind.rs:37-38, 260 (assert) | (c) | glossary-audit.md:17 (said 17), rename map:14 | refreshed, now 19 |
| constructors `pr_feedback`, `pr_added`, `pr_edited`, `pr_described`, `pr_cancelled`, `pr_cancelled_superseded`, `pr_started`, `pr_base_changed`, `pr_opened`, `pr_merged` and their `json!` data keys (`"pr"`, `"fields"`, `"superseded_by"`, `"number"`, `"url"`, `"merged_at"`, `"head_ref"`, `"author"`, `"needs"`, `"appended"`) | core/src/events/typed.rs:228-286, 296-324 | (c) for the data keys the adapter reads, (b) for the Rust fns | glossary-audit-core.md | refreshed with current lines (new) |
| fixture round-trip list of the same kinds | core/src/events/typed.rs:564-568 | (b) | new | new |
| PROTOCOL comment `17 is \`pr-described\`` | pi/lib/event.ts:7 | (c) | new | new |
| adapter kind list `"all-merged", "pr-feedback"` and `"pr-added", "pr-edited", "pr-described", "pr-cancelled", "pr-started", "pr-base-changed", "pr-opened", "pr-merged"` | pi/lib/event.ts:17, 22 | (c) | glossary-audit-pi.md | refreshed |
| `export const PROTOCOL = 19` | pi/lib/event.ts:9 | (c) | glossary-audit.md:17 (17), rename map:14 | refreshed, now 19 |
| `PLAN_KINDS = ["plan-created", "plan-phase", "pr-opened", "pr-merged", "job-failed", "plan-done"]` | pi/lib/updates.ts:56 | (c) | rename map (updates.ts:56) | refreshed, same line |
| "A `pr-opened` Event of a Plan" (doc comment) | pi/lib/updates.ts:64 | (a) | new | new |
| parse comment "`cpd events list --kind pr-opened`" and its error text | pi/lib/updates.ts:108, 113 | (a) | new | new |
| user-facing strings `"PR" / "PRs"` in the Plans feed: `plural(e.length, "PR", "PRs")} opened`, `${plural(e.length, "PR", "PRs")} merged` | pi/lib/updates.ts:209-215 | (a) (the adapter's rendered text) | new | new |
| stored `pr-%` Event rows in every store | store (rows, not re-measured) | (d) | rename map:13 | carried over unverified |

### 1.6 TUI strings and the Dispatch column

69 files under `tui/src/` match `PR`/`prs` (grep count, current tree; audit-tui counted 63 of 170 files — refreshed, count moved).

| Occurrence | Where | Class | Audit row | Status |
|---|---|---|---|---|
| section title `PRs (Dispatch)` | tui/src/widgets/legend.rs:198 | (a) | glossary-audit.md (legend.rs:198), CONTEXT.md:186 ("Dispatch column") | refreshed, same line |
| legend rows for `Dot::Working(Some(Work::Dispatch))` | tui/src/widgets/legend.rs:180, 461 | (b) | new | new |
| flow doc "its first PR (`plan show`)", "The Job's PR (`jobs.pr`)… `None` exactly for an explore Job", "An explore Job belongs to the Plan, not to a PR: its node carries no PR (`JobHome::Plan`)" | tui/src/read/flow.rs:4, 190, 480 | (b) comments, (a) where they describe rendered text | new | new |
| pulse read doc "review and first PR (`plan show`)" | tui/src/read/mod.rs:8 | (b) | new | new |
| test fixtures `INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, …)` | tui/src/worked_on_tests.rs:48; tui/src/read/pulse_tests.rs:46, 58; tui/src/read/stacked_ready_tests.rs:31, 51; tui/src/read/working_pr_dot_tests.rs:49, 53, 59, 61; tui/src/read/elapsed_tests.rs:15, 33 | (b) | new (the tests part sampled only) | new |

### 1.7 The adapter protocol and prompts

| Occurrence | Where | Class | Audit row | Status |
|---|---|---|---|---|
| open-Job prompt: "This is the **open Job** of a PR: you bring the PR's Lead Branch up to date with its base… cpd, not you, then runs the Gate, pushes the Lead Branch and opens the pull request." | core/src/spec/worker.md:23 | (a) | glossary-audit-pi.md | refreshed, same line |
| update-Job prompt: "This is an **update Job** of a PR whose pull request is open…" | core/src/spec/worker.md:26 | (a) | glossary-audit-pi.md | refreshed, same line |
| open/update merge procedure text (`origin/<base>`, base PR merged) | core/src/spec/worker.md:37 | (a) | glossary-audit-pi.md | refreshed, same line |
| test rule naming "the PR's open Job" | core/src/spec/worker.md:78 | (a) | new | new |
| policy refusals text "the PR's policy does not resolve" (`owner_unset`, `owner_unconfirmed`, `policy_invalid`) | core/src/spec/worker.md:252 | (a) | new | new |
| `pi/lib/updates.ts` 24 matching lines (the Plans feed, §1.5 above) | pi/lib/updates.ts | (a)/(c) | rename map (updates.ts:56, 140, 145) | refreshed |
| `pi/lib/reminder.ts` (4 lines), `pi/lib/turns.ts` (3), `pi/lib/slack-parse.ts` (3), `pi/lib/testguard.ts`, `pi/lib/mainguard.ts`, `pi/lib/delivery.ts` (1 each) | pi/lib/ | (a)/(b) by line; not individually re-cited (the audit's pi part skimmed these; still true) | glossary-audit-pi.md | carried over unverified |
| `pi/durable-host/turn.ts` matches `PR`/`prs` | pi/durable-host/turn.ts | (b) | new | new |

### 1.8 The skills

| Occurrence | Where | Class | Audit row | Status |
|---|---|---|---|---|
| Lead skill description: "one deliverable, one or more PRs across one or more repos"; "deploying - the open Job that checks and opens each PR, monitoring its review and CI" | pi/skills/cpd-lead/SKILL.md:3 | (a) | glossary-audit-pi.md | refreshed, same line |
| Lead skill: `gh pr checks`, `cpd pr rerun --pr <key>`, `cpd pr push` / `cpd pr rebase`, "the PR's open Job" | pi/skills/cpd-lead/SKILL.md:30 | (a) | glossary-audit-pi.md | refreshed, same line |
| Lead skill: "cpd opens the PRs, you do not." `cpd pr open` is the repair | pi/skills/cpd-lead/SKILL.md:33 | (a) | new | new |
| Lead skill: follow-up registration "PR / Job", `--base <pr-key>`, `--needs <pr-key>`, `cpd pr add <key> --repo <path>…`, `pr add` in `dispatch` refused `needs_required` | pi/skills/cpd-lead/SKILL.md:42, 47 | (a) | new | new |
| Lead skill full match count: 75 lines | pi/skills/cpd-lead/SKILL.md (grep count) | (a) | glossary-audit-pi.md (gave prose counts) | refreshed |
| Manager skill: "a PR waiting for review"; "Plan / PR / Job"; `cpd policy show --pr <key>` "a PR's repo policy" | pi/skills/cpd-manager/SKILL.md:38, 45, 55 | (a) | glossary-audit-pi.md | refreshed |
| Manager skill: "one or more PRs… no PR needs another Plan's PR, and no PR's base is another Plan's PR"; "A small follow-up goes into its area's existing PR" | pi/skills/cpd-manager/SKILL.md:91, 108 | (a) | new | new |
| Manager skill full match count: 14 lines | pi/skills/cpd-manager/SKILL.md (grep count) | (a) | glossary-audit-pi.md | refreshed |

### 1.9 The docs

| Occurrence | Where | Class | Audit row | Status |
|---|---|---|---|---|
| transitional note: "until the rename lands, the CLI (`cpd pr add`, `--pr`), the tables (`prs`, `pr_needs`), the Events and the docs still call a Workflow \"PR\", and the cockpit's third column is still titled Deploy; this glossary's words are the target. The rename deletes this note." | CONTEXT.md:16 | (a); the note stays until the rename lands and the same change that renames deletes it — never edited here | glossary-audit-vocab.md | refreshed |
| "The one rule: **PR means only the pull request** on GitHub. Plan > Workflows > Jobs. History keeps the old words" | CONTEXT.md:18 | (a) | glossary-audit-vocab.md | refreshed, same line |
| `**Workflow**` entry, `**pull request**`, `**PR**` entries | CONTEXT.md:44-46, 208, 212 | (a) | glossary-audit-vocab.md | refreshed, same lines |
| retired cpd-1 entries `**Workflow (cpd 1)**`, `**workflow worktree**` | CONTEXT.md:417, 435 | (d) (history: retired entries keep their words) | glossary-audit-vocab.md | refreshed, same lines |
| `docs/contract.md` total: 764 matching lines for `PR`/`prs`/`pr` (grep count, current tree) | docs/contract.md | (a) | audit parts C1-C4 gave 335/460/597/239/444 token counts by range | refreshed (current whole-file grep count) |
| §2.5 "PRs in commands and views (R34)" | docs/contract.md:790 | (a) | glossary-audit-contract-1.md | refreshed, same line |
| §3.8 "PR statuses (R34)" | docs/contract.md:1227 | (a) | glossary-audit-contract-2.md | refreshed, same line |
| §7.6 "Pull requests and the PR rebase"; "Each command here works on **one PR**… `pr poll`, which has no `--pr`" | docs/contract.md:4837 | (a) | glossary-audit-contract-4.md | refreshed, same line |
| `docs/commands.md` total: 637 matching lines (grep count) | docs/commands.md | (a) | audit parts M1-M4b | refreshed |
| `--pr` rule text (R34): "`--pr <key>`… used by `job add`, `pr open`, `pr rebase`, `pr push`, `pr view`, `pr reply`, `pr rerun` and `pr post`… `pr_required` (exit 2)… `pr_not_found` (exit 4)… Exceptions… `pr poll` has no `--pr`… `pr add`, `pr edit`, `pr cancel` and `pr start` take the key as their positional argument" | docs/commands.md:104 | (a) | new (audit parts quoted it in ranges) | new |
| refusal rows `pr_title_required`, `jira_required` (whose `details` is `{plan, pr}`) | docs/commands.md:178-179 | (a) | new | new |
| `cpd pr reply` (§9.8a), `cpd pr post` (§9.8b) table rows | docs/commands.md:262-263 | (a) | new | new |
| `plan list` output "Each Plan lists its PRs (R34)… with `key`, `repo`, `status`, `pr` (`{number, url, opened_at}`…) and `pull_request`" | docs/commands.md:867 | (a) (JSON output the adapter reads → PROTOCOL bump) | new | new |
| `plan show` output "`prs` (R34, A14), every PR of the Plan… `base_pr`… `jira`… `needs`… `closed_unmerged`… `feedback_open`… `all_merged_at`, `start_failed_at`…" | docs/commands.md:886 | (a) | new | new |
| `docs/implementing.md`: module row "\| `pr` \| rs-pr \| `pr …` (§9): `pr open`, `pr poll`, `pr rebase`, and since R34 `pr add`, `pr edit`, `pr cancel`, `pr start` (§9.4–§9.7); owns the tables `prs` and `pr_needs`, the PR selection by `--pr` (§1.3) and the PR start" | docs/implementing.md:46 | (a) | glossary-audit-vocab.md (skimmed the file) | refreshed, same line |
| `docs/implementing.md`: migration rule "a breaking migration the PR itself adds…", test-harness rows (`cli/tests/pr/git_guard.rs`, test area `pr` → `pr*`, `merge*`, `git*`, `repo_hooks`) | docs/implementing.md:101, 113, 122, 129 | (b)/(a) by row | glossary-audit-vocab.md | refreshed, same lines |
| `docs/implementing.md` "until PR final", "Timers and which PR gives each the seam" | docs/implementing.md:138, 160-162 | (b) | new | new |
| `docs/architecture.md`: "`pr-poll.lock`" | docs/architecture.md:253 | (b) | new | new |
| `docs/interface.md`: "`cpd pr` \| Pull requests (commands.md §9)", rows `cpd pr add`…`cpd pr poll` | docs/interface.md:65-72 | (a) (generated from clap help — regenerate with `CPD_BLESS=1`, never hand-edit) | glossary-audit-vocab.md (Description column only) | refreshed, same lines |
| `README.md`: no match | README.md (grep count 0) | — | new (the audit's vocab part read it; current tree has none) | new |
| this file and the other `docs/research/glossary-audit*.md` parts | docs/research/ | (d) (history; never rewritten) | — | new |

### 1.10 What Part 1 does not claim

- Counts in `docs/contract.md` and `docs/commands.md` are whole-file grep line counts, not sense-classified token counts like the audit parts' per-range counts; the classified rows above carry the senses.
- Live store row counts are the rename map's snapshot (2026-10-28) and are marked unverified here; this Job read no live database.
- `pi/lib` beyond the lines cited was not re-swept line by line; the audit's pi part skimmed it and its counts still stand as upper bounds.

## Part 2 — The artifact question, no decision

CONTEXT.md's Workflow entry (CONTEXT.md:44-46, refreshed, current words):

> **Workflow** (entity, pro-d136, pro-d138):
> The part of a Plan in one repository: its repo, base branch, Lead Branch and Lead worktree, its Jobs, and the Workflows it needs. It ends in exactly one pull request, which cpd opens from its Lead Branch; it maps to one pull request but is not it. One repository may hold several Workflows of one Plan, and the Jira key of the work stays on the Workflow. cpd 1 used the word for a Plan (D3, retired); a GitHub Actions workflow is always written in full.
> _Avoid_: PR, sub-plan, track, slice, repo plan

The records: `pro-d136` and `pro-d138` (2026-10-28) decided the object is a Workflow, PR means only the pull request, and history keeps the old words (CONTEXT.md:5, :18, :44). The User has since said a Workflow delivers "an artifact (like a pr, or a research doc)". Two readings of that sentence are live; both are laid out below with their costs. **Neither is recommended here: the User decides at the Plan review.**

### Reading 1 — the artifact is always the pull request; a research doc is a Job's artifact, never a Workflow's

Under this reading CONTEXT.md stands exactly as quoted: every Workflow ends in exactly one pull request (CONTEXT.md:45), and the User's "research doc" names the artifact of an *explore Job* (`research`), which is a different object at a different level.

What each implies, cited:

- Explore Jobs stay Plan-level and PR-less, as contract §2.2 states: "An explore Job… belongs to the Plan, not to a Workflow (it has no PR)" (docs/contract.md:454-458, refreshed); CONTEXT.md:73 says the same ("It belongs to the Plan, not to a Workflow (it has no PR)… the artefact stays in the Plan's explore directory, and a Spec cites it by path").
- The cockpit already renders a Plan-level explore Jobs box, and an explore Job's Attempts are in no Workflow's cost (docs/contract.md:528, refreshed; decision pro-d237 unchanged).
- Refusals already forbid the mix: `explore_pr_forbidden` for `--pr` on `job add --kind scout|research` (core/src/error.rs:184, docs/contract.md:87), `explore_needs_mixed` (docs/contract.md:458), and `jobs.pr` is NULL for explore Jobs (docs/contract.md:404; tui/src/read/flow.rs:190).
- A research-doc Workflow therefore cannot exist under this reading: a Workflow's last Job is an `open` Job that opens its pull request (CONTEXT.md:57), and every started Workflow gets one (CONTEXT.md:57; engine E4).
- Costs: none in code or schema. The cost is vocabulary — the User's sentence does not describe a Workflow, so either the User rewords it or cpd's docs gloss "artifact of a Workflow = pull request; artifact of a research Job = the explore artefact" in CONTEXT.md. The word "artifact" has no entry in CONTEXT.md today (grep: no `**Artifact**`/`**artefact**` entry; the glossary uses "artefact" in prose, e.g. CONTEXT.md:73, docs/contract.md:87).

### Reading 2 — a Workflow can deliver a non-PR artifact, so a research-doc Workflow exists

Under this reading the User's sentence redefines the Workflow's deliverable: "ends in exactly one pull request" (CONTEXT.md:45) stops being a definition and becomes one case of a Workflow that ends in an artifact — a pull request when the work ships code, a research document when it researches.

What each implies, cited:

- A Workflow need not end in a pull request: the rule "It ends in exactly one pull request, which cpd opens from its Lead Branch; it maps to one pull request but is not it" (CONTEXT.md:45) would be rewritten, and with it contract §7.6's "Each command here works on **one PR**… Each PR has its own Gate run and its own pull request" (docs/contract.md:4837) and the phrase "ready and never a draft" attached to every open.
- The open-Job rule becomes conditional: "The last Job of every started Workflow… a Job of kind `open` that is Ready when every other Job of its Workflow is merged or cancelled… cpd then runs the Gate and opens the Workflow's pull request" (CONTEXT.md:57). A research-doc Workflow has no pull request to open, so cpd would need either no open Job (a third Workflow shape besides code and explore) or an open Job that produces no pull request — both new machinery.
- The Workflow's status system follows the pull request: `open`/`waiting`/`merged`/`cancelled` ride the pull request's lifecycle (CONTEXT.md:194 "squash-merged… The Workflow's status `merged` follows it"). A research-doc Workflow would need its own terminal status and its own Event, because `pr-merged` (`core/src/events/kind.rs:105`) reports a pull request fact and cannot mark a doc Workflow done.
- Explore Jobs blur into Workflows: today a research Job is deliberately PR-less, plan-level, fingerprint-exempt and invisible to plan review and the lock (docs/contract.md:457: "An explore Job is in no plan fingerprint… a Plan whose only Jobs are explore Jobs still fails the lock as a Plan with no `draft` Job does"). Promoting it to a Workflow would pull it into the fingerprint, the Plan review, the lock and the cost roll-up (docs/contract.md:528), reversing decisions pro-d210 and pro-d215 made when explore Jobs were added, and `pro-d237`'s cost rule.
- The store and the adapter follow: table `prs` and its columns (core/src/store/schema.rs:190-191, 155-156, 372-379), `jobs.pr` (docs/contract.md:404), `pr_needs` (CONTEXT.md:88) and the `pr-` Event kinds and their data keys (core/src/events/kind.rs:97-105, core/src/events/typed.rs:228-324, pi/lib/event.ts:17, 22, pi/lib/updates.ts:56) all assume one pull request per Workflow; a second artifact kind means either nullable-everything columns with a new meaning for NULL, or new tables, kinds and keys — and PROTOCOL rises either way.
- Costs: a contract rewrite (§2.2, §3.8, §7.6), new Workflow statuses and Events, engine rules (E4/E7), cockpit flow, a store migration, and a PROTOCOL bump; plus the reverse-reading risk that the word Workflow (deliberately returned from cpd 1, pro-d136) acquires a third sense, next to cpd 1's retired Plan sense (CONTEXT.md:417) and the GitHub Actions workflow (CONTEXT.md:228), which the glossary already guards against with "GitHub Actions workflow is always written in full" (CONTEXT.md:45, :228).

### What both readings share

Both readings are consistent with the decided rename itself (pro-d136, pro-d138): the object is a Workflow, `PR` means only the pull request, and no alias is invented. Reading 1 leaves cpd's object graph untouched; Reading 2 changes what a Workflow is. The rename's inventory (Part 1) is the same under both; only the second Job's design differs.

**No decision is recorded here.** The User decides at the Plan review.

## Part 3 — The rename design per change class

This is the design the Part 1 inventory (glossary-rename.md:7-9) feeds, class by class (glossary-rename.md:11-159). It assumes Reading 1 of Part 2 — a Workflow still ends in exactly one pull request (glossary-rename.md:180) — for every class below; Part 2's Reading 2 changes what a Workflow is and would add to this design, and it stays the User's (glossary-rename.md:201). Nothing here is decided: the per-command boundary (§3.10), the store migration (§3.11, proposed), the staging (Part 4) and the artifact question are the User's at the Plan review (glossary-rename.md:207, plan.md "Out of scope").

The one rule that splits every class: rename what names the **object**, keep what names the **pull request**, and leave history in the old words (CONTEXT.md:18, :44, :208, :212; glossary-rename.md:3). The rename is a **cutover**, not a transition: Context record pro-d138 (2026-10-28) decided no alias, shim or deprecation period, so the old object forms become usage errors (glossary-rename.md:3; docs/research/glossary-audit-usage.md:63, refreshed — the map's own row). Prior art is the rename map `docs/research/glossary-audit-rename.md` (its decisions R1-R21 and its order of work); every row reused below is marked **refreshed from the map**, and every row that moved is re-cited against the current tree. A quote of current code keeps the current words and says so (glossary-rename.md:5).

### 3.1 CLI commands

**Mechanism.** Split today's single `pr` group into two: `workflow` for the object and `pr` for the pull request. Today the group is `Pr(pr::Command)`, help "Pull requests (commands.md §9)" (cli/src/cli.rs:58-60), with run arm `Command::Pr(c) => pr::run(ports, c)` (cli/src/cli.rs:184); `review` is a separate group `Review(pr::review::Command)` (cli/src/cli.rs:63, run arm :185) and does not move. Move the five that act on the Workflow — `pr add`, `pr edit`, `pr describe`, `pr cancel`, `pr start` (cli/src/interface.rs:69-73) — to `cpd workflow add|edit|describe|cancel|start`. Keep the nine that reach the pull request or its Lead Branch — `pr open`, `pr poll`, `pr rebase`, `pr push`, `pr view`, `pr reply`, `pr rerun`, `pr post`, `pr merge` (cli/src/interface.rs:74-82) — under `pr`, pending §3.10. The old forms are not aliased (pro-d138); clap rejects an unknown subcommand, and a red test must pin the exit code and text (the map's `old forms after the rename` row, refreshed from the map; the exit code clap uses versus cpd's `usage` code is unverified).

**Blast radius.**
- `cli/src/interface.rs:69-82` (the 14-row command registry) and `cli/src/cli.rs:58-60, 63, 184-185` (groups and run arms).
- `docs/interface.md:65-74` and following — generated from the `cpd --help` tree, never hand-edited, regenerated with `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface` (docs/interface.md:3; docs/commands.md:232).
- `docs/commands.md` §9 heading "## 9. `pr`" (docs/commands.md:1814) and its §9.1-§9.10 sections (docs/commands.md:1818-2453): the map's row, refreshed; §9 numbers are kept because help and skills cite `§9.4`, `§9.5a` (glossary-audit-rename.md, refreshed from the map).
- Callers that build argv from these names: the TUI (tui/src/**, 69 files match `PR`/`prs`, glossary-rename.md:114) and the engine (`engine/src/watch/pr.rs`).
- Tests: the map counted 392 argv literals across 60 test files (glossary-audit-rename.md, refreshed from the map); `cli/tests/pr/**` alone is 34 files, 2752 matches (glossary-audit-rename.md:216, refreshed from the map).

**Staging.** The group split cannot land alone and stay honest: the new commands need the Workflow types and the store names of §3.2-§3.4, and a temporary alias is forbidden (pro-d138). It can land in the same change as the CLI/type layer while the store migration is a separate Job, because the first full Gate is the open Job and docs-tied tests may be red in between (glossary-audit-rename.md:215, refreshed from the map). See Part 4 §4.2 for the proposed shape.

### 3.2 Flags

**Mechanism.** `--pr <key>` becomes `--workflow <key>` everywhere it selects the object: the shared `PrArg` used by `pr open`, `pr rebase`, `pr push`, `pr view`, `pr reply`, `pr rerun` and `pr post` (core/src/args.rs:37-42; docs/commands.md:104), `job add`/`job edit` (core/src/job/mod.rs:133-134) and `pr describe` (core/src/pr/mod.rs:214). `--until-pr` of `incident start` (core/src/incident.rs:41, parse error text :157-160) becomes `--until-workflow`. Placeholder keys `<pr-key>`, `<pr>`, `open-<pr>` become `<workflow-key>`, `<workflow>` (glossary-audit-rename.md, refreshed from the map). `review submit --pr <url|owner/repo#n>` stays `--pr`, because it names the pull request (glossary-audit-rename.md, R5, refreshed from the map; docs/commands.md:5591 in the map).

**Blast radius.** The one clap struct `PrArg` (core/src/args.rs:38-42) feeds every selector, so its field rename is crate-wide; its selection messages carry the old words — "Plan {plan} has no PR: add one with cpd pr add, then pass --pr <key>." and "Plan {plan} has {n} PRs (…): pass --pr <key>." (core/src/args.rs:64, :72) — and are user text. The stored `incident.until_pr` column and Event data key follow (core/src/incident.rs:90, :102, :104, :245, :260). The TUI builds `--pr` argv (glossary-audit-rename.md, refreshed from the map, cites tui/src/widgets/mod.rs:259). The skill tests read `docs/commands.md` for every flag, so the docs move with the code (glossary-audit-rename.md, refreshed from the map).

**Staging.** Flag and command rename are one interface: a flag renamed without the command (or the reverse) is an interface no caller can use. Land together, in the same change as §3.1.

### 3.3 Refusal codes

**Mechanism.** The object codes become `workflow_*`: `pr_required`, `invalid_pr_id`, `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_ended`, `pr_closed` (contested, see below), `pr_title_required`, `pr_start_failed`, `pr_adopted`, `pr_not_closed`, `pr_not_open`, `pr_not_found` and the base pair `base_pr_unknown`/`base_pr_unavailable`/`base_pr_other_repo`, `pr_other_plan`, `explore_pr_forbidden`, `needs_other_pr`, `until_pr_not_incident` (core/src/error.rs:161, 179-184, 288-298, 307-309, 323, 342). The map's split (R13, refreshed from the map): `pr_not_ready` covers two meanings today — an unmet need and an unpostable pull request — so it splits into `workflow_not_ready` (needs unmet) and a pull request `pr_not_ready`; `pr_closed`/`pr_not_closed` stay (pull request state); `base_pr_*` become `base_workflow_*`; `needs_other_pr` becomes `needs_other_workflow`; `until_pr_not_incident` becomes `until_workflow_not_incident`.

**Blast radius.** `core/src/error.rs` and its exit-code tests (core/src/error.rs:492-594); the code table in `docs/commands.md` §1.2 (docs/commands.md:121-205) and `docs/contract.md` §1.2, with the current counts `pr_locked` 26 lines, `pr_in_use` 22, `pr_start_failed` 16, `pr_other_plan` 15, `pr_ended` 13, `pr_not_ready` 11, `pr_adopted` 11, `pr_unknown` 10, `pr_unavailable` 10 (glossary-rename.md:56, refreshed); and the hand-written triggers that `RAISE(ABORT, 'pr_locked')` and `needs_other_pr` (glossary-audit-rename.md, refreshed from the map) — but those trigger texts live in **applied** migrations and are recreated with new text by the §3.11 migration, while the stored old text stays as history.

**Staging.** The code strings are read by the pi adapter's error handling and by `docs/commands.md`; they can land with the code rename and the docs, and need no PROTOCOL bump on their own unless the adapter branches on a code string (the adapter reads `error.code` generically, pi/lib/cpd.ts — unverified which codes it special-cases).

### 3.4 The store

**Mechanism.** Rename the object tables `prs` → `workflows` and `pr_needs` → `workflow_needs`, and the object columns `jobs.pr` → `jobs.workflow`, `pr_needs.pr` → `workflow_needs.workflow`, `pr_posts.pr` → `pr_posts.workflow`, `slack_reactions.pr` → `slack_reactions.workflow`, `incident.until_pr` → `until_workflow` (glossary-audit-rename.md, refreshed from the map; core/src/store/schema.rs:190-191, :571-572). Keep the table `pr_posts` (it posts the pull request; map R8, refreshed from the map) and keep the pull request columns `pr_number`, `pr_url`, `pr_opened_at`, `pull_request` and the `pull_requests` table (map R9, refreshed from the map; core/src/store/migrations/20261102T1000-lead-branch-name.sql:16-38). The eight hand-written triggers become `workflows_*`, `workflow_needs_*`, `jobs_workflow_dependencies`, `dependencies_same_workflow` (glossary-audit-rename.md, refreshed from the map). The full proposed migration is §3.11.

**Blast radius.** Every SQL string in Rust holds these names (the map counts 94 files for `prs`/`pr_needs`/`jobs.pr`, schema part, refreshed from the map), so a missed one fails only at run time (`store_failed`). `SCHEMA_HASH` is pinned (core/src/store/schema.rs:113) and repinned by `scripts/repin.sh`. The legacy lists in `schema.rs` that name `prs`/`pr_needs` — the `plan_deleted` match (core/src/store/schema.rs:456), the legacy table list (core/src/store/schema.rs:571-572) and the no-delete determination (core/src/store/schema.rs:586, :597) — move with the code, while the applied migration files under `core/src/store/migrations/` never change (docs/implementing.md:101-102, the freeze rule). The Plan fingerprint canonical keys `pr`/`prs` (core/src/plan/fingerprint.rs:51, :70) are a **frozen hash input**: renaming them voids the stored 114 `plan_reviews` and 1741 Job fingerprints (map R12, refreshed from the map; glossary-rename.md:57).

**Staging.** The table rename is the one barrier: a store migrated by the new binary is refused by an older binary (`store_too_new`, core/src/store/verdict.rs:196; core/src/error.rs:70), so it must land in one change with the code that speaks the new names, and its `after <n>` name must sort after every migration on the base (§3.11). Nothing else can ship SQL text against the new names until it lands.

### 3.5 Event kinds and their data

**Mechanism.** The six object kinds `pr-added`, `pr-edited`, `pr-described`, `pr-cancelled`, `pr-started`, `pr-base-changed` (core/src/events/kind.rs:97-103) become `workflow-added`, `workflow-edited`, `workflow-described`, `workflow-cancelled`, `workflow-started`, `workflow-base-changed`. The two contested kinds `pr-opened`, `pr-merged` (core/src/events/kind.rs:104-105) are the map's R6, refreshed from the map: keep them with the data key `workflow`, or rename to `workflow-opened`/`workflow-merged`, or `pull-request-*` — the audits disagree on their sense (glossary-rename.md:81). `pr-feedback` stays (pull request). The object data keys rename: `pr` (Workflow key) of `job-added`, `job-merged`, `all-merged` and every `pr-*` kind becomes `workflow`; `prs` of `plan-phase`/`plan-done` becomes `workflows`; `from_pr`/`to_pr`/`until_pr` and the cause `until-pr` follow (glossary-audit-rename.md, refreshed from the map; core/src/events/typed.rs:228-324). New kinds go in with the **additive-insert path** against `events` and `event_kinds` (docs/implementing.md:99, step 3: `INSERT INTO event_kinds (name) VALUES ('<kind>');` plus `core/src/events/kind.rs`, `pi/lib/event.ts`, contract §5.2), old rows stay as history (map R7, refreshed from the map). `PROTOCOL` rises from **19** to **20** — verified current value 19 at core/src/events/kind.rs:38 and pi/lib/event.ts:9, mirrored by the comment at kind.rs:36-37 and event.ts:3-8, pinned by the test `kind.rs:260` (`assert_eq!(PROTOCOL, 19)`).

**Blast radius.** `core/src/events/kind.rs`, `core/src/events/typed.rs:228-324`, the fixture list `typed.rs:564-568`, `docs/contract.md` §5.2 (docs/contract.md:2595), the pi adapter's `KINDS` (pi/lib/event.ts:17, :22) and `PLAN_KINDS` (pi/lib/updates.ts:56), and `pi/lib/updates.ts` parse/read of `data.pr`/`data.prs` (`updates.ts:140-153`). The pi adapter rejects an unknown kind (its `KINDS` list is `as const`, pi/lib/event.ts:16-24), so it must learn the new kinds before they are emitted. Per Context record hs-n628 (2026-10-31, Plan `attempt-abort`): bump PROTOCOL only when the adapter reads the change, and two Plans bumping at once is the update Job's conflict to resolve, not a Worker's (hs-n628 text). Live counts at the time of the map: 295, 14, 48, 82, 223, 3 object Events; 198 `pr-opened`, 197 `pr-merged`, 41 `pr-feedback` (glossary-audit-rename.md, refreshed from the map, marked live counts of that day).

**Staging.** The new kinds, the adapter's list and the PROTOCOL bump are one atomic change (hs-n628; glossary-audit-rename.md, refreshed from the map). They may overlap the `attempt-abort` Plan, which also touches `core/src/events/kind.rs` and `pi/lib/event.ts` (glossary-rename.md:222-224, risks R3): the two PROTOCOL bumps collide and the update Job resolves the constant (hs-n628).

### 3.6 TUI strings and the Dispatch column

**Mechanism.** Rename the rendered text: the section title `PRs (Dispatch)` (tui/src/widgets/legend.rs:198) becomes `Workflows (Dispatch)`, and the legend labels around `Dot::Working(Some(Work::Dispatch))` (tui/src/widgets/legend.rs:180, :461) follow. Object-sense strings in the plan and flow reads — the flow doc's "The Job's PR (`jobs.pr`)… `None` exactly for an explore Job", "An explore Job belongs to the Plan, not to a PR: its node carries no PR (`JobHome::Plan`)" (tui/src/read/flow.rs:190, :480) and the pulse doc (tui/src/read/mod.rs:8) — become Workflow wording. Pull request strings stay.

**Blast radius.** 69 files under `tui/src/` match `PR`/`prs` (glossary-rename.md:114, refreshed; the audit-tui part counted 63 of 170 files — count moved). Test fixtures insert into `prs` (tui/src/worked_on_tests.rs:48; tui/src/read/pulse_tests.rs:46, :58; tui/src/read/stacked_ready_tests.rs:31, :51; tui/src/read/working_pr_dot_tests.rs:49, :53, :59, :61; tui/src/read/elapsed_tests.rs:15, :33; glossary-rename.md:122), so they break the moment the table is renamed (§3.11) and must land with the store or be rewritten to the new name.

**Staging.** The TUI is compiled against core, so its types and SQL fixtures move with the store/type change; its rendered strings can follow in the same or a later Job, but no Job after the store rename may still query `prs`.

### 3.7 The adapter protocol and prompts

**Mechanism.** The Worker prompts in `core/src/spec/worker.md` speak the object: the open-Job line "the **open Job** of a PR: you bring the PR's Lead Branch up to date… opens the pull request" (core/src/spec/worker.md:23), the update-Job line "an **update Job** of a PR whose pull request is open…" (worker.md:26), the merge procedure "the base PR merged…" (worker.md:37) and the policy-refusal line "the PR's policy does not resolve" (worker.md:252) become Workflow wording, keeping "pull request" where it means the GitHub object. The pi adapter's rendered Plans feed — `plural(e.length, "PR", "PRs")} opened`, `PR ${key} opened` (pi/lib/updates.ts:209-215) — becomes Workflow wording, and its parse of the turn-start text follows.

**Blast radius.** `core/src/spec/worker.md` is the Spec every Worker reads (glossary-rename.md:126-129); `pi/lib/updates.ts` (24 matching lines), `pi/lib/turns.ts`, `pi/lib/reminder.ts`, `pi/lib/slack-parse.ts`, `pi/durable-host/turn.ts` (glossary-rename.md:130-133, refreshed). The adapter text is pinned by `pi/test/updates.test.ts` (glossary-audit-rename.md, refreshed from the map). A live pi session loads its adapter at Session start, so a renamed prompt/adapter is only read at the next Session start or reload (glossary-audit-rename.md, cutover row, unverified on protocol mismatch).

**Staging.** This class may land **without** the pi adapter breaking only if it touches nothing the adapter reads (prompt prose), so it can be staged ahead of the kind rename; the moment it changes `pi/lib/updates.ts` text the adapter reads, it lands with §3.5 and the PROTOCOL bump (hs-n628).

### 3.8 The skills

**Mechanism.** Rewrite the Lead skill (`pi/skills/cpd-lead/SKILL.md`) and the Manager skill (`pi/skills/cpd-manager/SKILL.md`) to say Workflow for the object: the Lead's "one deliverable, one or more PRs across one or more repos" and "the open Job that checks and opens each PR" (SKILL.md:3), its `cpd pr rerun --pr <key>`, `cpd pr push`/`cpd pr rebase` and "the PR's open Job" (SKILL.md:30), "cpd opens the PRs, you do not" (SKILL.md:33), the follow-up registration `--base <pr-key>`/`--needs <pr-key>`/`cpd pr add` (SKILL.md:42, :47); the Manager's "a PR waiting for review", "Plan / PR / Job" and `cpd policy show --pr <key>` (SKILL.md:38, :45, :55). Pull-request phrases stay.

**Blast radius.** The Lead skill matches 75 lines, the Manager 14 (glossary-rename.md:141, :149, refreshed). The skill guard test `pi/skills/skill.test.ts:110` currently forbids a bare `workflows?` and must flip to forbid only the retired forms, or it fails the moment a skill says Workflow (glossary-audit-rename.md, refreshed from the map). The `references/pr-description.md` name stays because it is the pull request's description (map R20, refreshed from the map).

**Staging.** Skills are prose the adapter loads at Session start; they can be staged independently of the code, but the guard-test flip must land with the first skill that uses the new word, and the flip's offending set must match what the code ships (§3.9).

### 3.9 The docs

**Mechanism.** Rewrite the object rows in `docs/contract.md` (§2.5 "PRs in commands and views", docs/contract.md:790; §3.8 "PR statuses", :1227; §7.6, :4837), `docs/commands.md` (§1.3 `--pr` rule, docs/commands.md:104; the §1.2 refusal table; §9), `docs/implementing.md` (the `pr` module row describing tables `prs`/`pr_needs` and selection by `--pr`, docs/implementing.md:46; the migration and test-harness rows, :113, :122, :129), `docs/architecture.md` (`pr-poll.lock`, :253) and regenerate `docs/interface.md` with `CPD_BLESS=1` (docs/interface.md:3; docs/commands.md:232). CONTEXT.md's transitional note (CONTEXT.md:16) **stays** until the change that renames lands, which deletes it then (glossary-rename.md:151; Part 3's scope does not touch it, per the Job's out-of-scope).

**Blast radius.** `docs/contract.md` alone has 764 matching lines and `docs/commands.md` 637 (glossary-rename.md:154, :158, refreshed whole-file grep counts); the contract's DDL blocks are compared with the schema by a test (`cli/tests/store/store.rs`, cited in the map at :179, :191, refreshed from the map), so the doc and §3.11 move together. The docs are history for `docs/adr/**`, `docs/archive/**` and `docs/research/**` (CONTEXT.md:417, glossary-rename.md:155), never rewritten.

**Staging.** Docs are the natural first slice (they break no build), but `docs/contract.md`'s DDL block and `docs/interface.md` are machine-checked: the DDL block goes red until the migration lands and `interface.md` is regenerated after the CLI change (§3.1). So the docs can lead everywhere except those two ties, which follow the code.

### 3.10 The per-command boundary — options and costs, no decision

`cpd pr push` was widened by Context record hs-d801 (2026-11-01, Plan `preopen-push`) to a PR that is **not yet open**, so it pushes the Lead Branch before any pull request exists — the web-client deadlock, where the tdd submit check builds a branch-local binary but pre-open the branch cannot receive main-side fixes (hs-d801 text). That breaks the old "`pr push` names the pull request" reading for `push` and, by the same `Lead Branch` sense, for `open` and `rebase`. The pull request's own verbs are clear. Each command below is laid out with both choices and their cost; **none is decided here.** The map's R3 recommends keeping `open`/`rebase`/`push` and `view` under `pr` (glossary-audit-rename.md, R3, refreshed from the map); hs-d801 pushes `push` toward `workflow`.

| Command | What it acts on (current) | Stays `pr` — cost | Moves to `workflow` — cost |
|---|---|---|---|
| `pr open` | Pushes the Lead Branch and opens the pull request (docs/commands.md:1818, §9.1) | A Workflow push under a pull request verb; but its result is the pull request, and `pr_not_open`/`open_job_live` are its refusals (glossary-rename.md:21) | `workflow open` names the push it performs; but the command the Lead runs to open the pull request loses its pull request word, and the policy text `cpd pr open` (owner.toml:8) changes |
| `pr push` | Gates, then pushes the Lead Branch as it is, no rebase (docs/commands.md:2417, §9.9); widened pre-open (hs-d801) | A pull request verb that works before any pull request exists (hs-d801) | `workflow push` matches "acts on the Workflow's Lead Branch" (hs-d801); but `push_rejected` names `cpd pr rebase` as next (docs/commands.md:121) and the repair hints, Gate log `pr-<key>.<hook>.log` (core/src/pr/mod.rs:323, :326-327) and policy text (cpd.toml:29-30) all change |
| `pr rebase` | Rebases the Lead Branch onto the base and pushes; `--continue`/`--abort` (docs/commands.md:1943) | Same mixed sense as `push` | `workflow rebase` names the Lead Branch it rewrites; but splitting `--continue`/`--abort` from the command is worse (map's caution, refreshed from the map) |
| `pr view` | Reads the pull request, selected by a Workflow key (docs/commands.md:2251) | `pr view --workflow <key>`: the pull request word with a Workflow selector — honest | `workflow view` must render pull request facts under a Workflow verb |
| `pr poll` | Polls every open pull request; no `--pr` (docs/commands.md:1888, :104) | Right: it acts on pull requests | Wrong: it has no Workflow selector at all |
| `pr reply` | Replies to a feedback item of a pull request (docs/commands.md:2287) | Right: the pull request's feedback | It takes `--workflow <key>` only to find the pull request; the verb is the pull request's |
| `pr rerun` | Re-runs failed GitHub Actions runs of the pull request's head commit (core/src/pr/mod.rs:9; docs/commands.md:2375, §9.8c) | Right: GitHub's runs of the pull request | Renames a GitHub verb for a Workflow it does not change |
| `pr post` | Posts the pull request to Slack (docs/commands.md:2319) | Right: the pull request is announced; table `pr_posts` stays (map R8) | Same as `reply` |
| `pr merge` | Merges the pull request by `<owner>/<repo>#<n>` (docs/commands.md:2453; a cockpit action, glossary-rename.md:21) | Right: no Workflow selector | Renames a pull request verb |

What must be said whichever way the User decides: the selector is `--workflow <key>` on every command that keeps a Workflow argument (§3.2, map R4, refreshed from the map); `pr open`, `pr rebase` and `pr push` keep the same words in their refusal texts and policy references, so a move changes those texts and the three live policy files (owner.toml:7-8; cpd.toml:28-30; missionlane/owner.toml, §3.11).

### 3.11 The store migration — proposed

This is the smallest **breaking** migration that keeps cpd 2 idempotent and honest. It is proposed, not decided. Name: a `breaking` file with a timestamp that sorts after every migration name on the base, for example `20261103T0300-workflow-rename.sql` (its `after <n>` recounted per `scripts/new-migration.sh`, core/src/store/migrations/).

**Why it must sort after every name on main.** `scripts/check-migrations-frozen.sh` (the late rule, its header and "The late rule" block) refuses a breaking migration the PR adds that sorts bytewise (`LC_ALL=C`) before any migration name on the base, because a store that applied a later name refuses a breaking migration that sorts before it: the breaking migration is a barrier, and the store records an applied migration only by name, so its floor (the newest breaking name) is its oldest allowed schema (docs/implementing.md:112, Check 3). The newest names on current main are `20261103T0100-session-runner` and `20261103T0200-event-session-model-changed` (core/src/store/migrations/), so the file is re-stamped to sort after them at land time; `scripts/repin.sh` recounts `after <n>` (scripts/new-migration.sh; docs/implementing.md:112).

**What it does, in order.**
1. **Drop the hand-written triggers and the index.** The eight triggers that name the object are `prs_locked`, `prs_status`, `prs_shape_fixed`, `prs_head_ref_fixed` (core/src/store/migrations/20261102T1000-lead-branch-name.sql:61, :69, :73, :75), `pr_needs_locked` (core/src/store/migrations/20261021T1300-local-needs.sql:27), `pr_needs_no_delete` (core/src/store/migrations/20261102T1400-pr-restack.sql:18), `jobs_pr_dependencies` (core/src/store/migrations/20261101T0900-job-verify-kind.sql:50) and `dependencies_same_pr` (core/src/store/migrations/0004-cross-repo.sql:209); the index is `prs_head_ref_live` (20261102T1000-lead-branch-name.sql:58; earlier 20261029T0900-pr-adopt.sql:10). Drop them, so the rename does not leave trigger bodies pointing at a table that no longer exists.
2. **Drop the generated history triggers.** The `prs_hist_ins`/`prs_hist_upd`/`prs_no_delete`/`pr_needs_hist_*` triggers are generated from `history_tables` by the reconcile (`create_history_triggers`, core/src/store/schema.rs:421-427; `history_tables`, schema.rs:588-606) and are regenerated after the rename (§ step 6). Their old names would otherwise keep `SCHEMA_HASH` and the schema check wrong (glossary-audit-rename.md, refreshed from the map).
3. **`ALTER TABLE ... RENAME`.** `ALTER TABLE prs RENAME TO workflows` and `ALTER TABLE pr_needs RENAME TO workflow_needs` — a rename, not a rebuild, so **every row survives and no `history` row is written** (the map's persistence row, refreshed from the map). Rename the object columns `jobs.pr` → `jobs.workflow`, `pr_needs.pr` → `workflow_needs.workflow`, `pr_posts.pr` → `pr_posts.workflow`, `slack_reactions.pr` → `slack_reactions.workflow`, `incident.until_pr` → `until_workflow` (map, refreshed from the map; table shapes at 20261102T1000-lead-branch-name.sql:16-38 and 20261030T1000-pr-posts.sql:4). **Unverified:** the runner applies a migration with `foreign_keys` off and `legacy_alter_table` on (comment, 20261102T1000-lead-branch-name.sql:2-3), and under `legacy_alter_table` on `ALTER TABLE RENAME` does not rewrite references in triggers, views or **foreign-key definitions** that other tables hold to the renamed table; the map flags the same interplay "unverified" (glossary-audit-rename.md, risk 8, refreshed). If so, the fallback is to rebuild the referencing tables' FK clauses (`jobs`, `pr_needs`, `pr_posts`, `slack_reactions`) or run the rename with `legacy_alter_table` off; this is the migration's one real unknown and the red test of §4.2 must settle it.
4. **Update `history_tables`.** Rename the rows `prs` → `workflows` and `pr_needs` → `workflow_needs` (the lookup table, 20261018T0828-kind-tables.sql:13, :33), so the reconcile regenerates triggers under the new names and the no-delete set is recomputed (schema.rs:586, :597). The append-only `history` rows stay: their `tbl` values `prs`/`pr_needs` are history, and the reader that replays them needs a replay rule for the old names (the map's persistence row, refreshed from the map; contract §4.4, cited there).
5. **Recreate the hand-written triggers and the index** under the new names and text, with `workflow_locked` where `pr_locked` was (core/src/store/migrations/20261102T1000-lead-branch-name.sql:61-75 as the model), and `workflows_head_ref_live` for the index.
6. **Update the code-side name lists** that are not SQL: `core/src/store/schema.rs:456` (`plan_deleted` match), `:571-572` (legacy table list), `:586`, `:597` (no-delete), and every SQL string in Rust (§3.4).
7. **Repin the hash:** `scripts/repin.sh` writes `SCHEMA_HASH` (core/src/store/schema.rs:113) from the print test, and the schema check fails loudly when it is stale (scripts/repin.sh; docs/implementing.md:101, step 4).
8. **Rewrite the live `cancel_reason`.** The live column is shown to the User — `core/src/plan/render.rs:205-206` renders `cancelled: <reason>` and `core/src/plan/jobs.rs:144` puts it in the JSON output — so the stored `'pr cancelled'` values (written at core/src/pr/cancel.rs:107, :109) are rewritten to the new reason, e.g. `'workflow cancelled'`, in the same migration. **Stored Event rows stay:** the `job-cancelled` Events' `reason` is append-only history and is not rewritten (map R14, refreshed from the map). The reader that justifies the split is the render/JSON pair above; `jobs.cancel_reason` is a live field, the Event's `reason` is not.

**Idempotence.** A migration is applied once and recorded by name in `schema_migrations` (core/src/store/schema.rs:118-127); re-installing the same binary opens the store without re-applying (the store verdict, core/src/store/verdict.rs). `ALTER TABLE RENAME` is not re-run. `scripts/repin.sh` is idempotent by design (its header: "Rerunning changes nothing").

**Back-compat (what an older binary does).** A pre-rename binary that opens a store with a breaking migration it does not know refuses with `store_too_new` (core/src/store/verdict.rs:196; core/src/error.rs:70; the user-facing hint "merge main into this branch, then install again", core/src/install/mod.rs:150). So installing the rename is a hard cutover for every cpd binary that shares `~/.cpd/cpd.db`: the User installs the new build on a machine with no Attempt mid-submit, and the engine restarts and migrates (the map's cutover, refreshed from the map; the flow, docs/contract.md:1080). The migration is a barrier by construction, which is what makes the rename honest.

### 3.12 What must land together, and the Plans in flight

Concurrent active Plans touch the same files, and per Context record hs-n628 a conflict is the **update Job's**, not a Worker's: `preopen-push` (`cpd pr push`, contract §9.9, commands §9.9; hs-d801), `web-client`, `metrics`, `attempt-abort` (touches `docs/contract.md`, `docs/commands.md`, `core/src/events/kind.rs`, `pi/lib/event.ts` — hs-n628 text), `lead-tier-jobs`. The overlap set is `core/src/store/schema.rs` (`SCHEMA_HASH`, migration list), any new migration (the freeze check), `pi/lib/event.ts` and `core/src/events/kind.rs` (`PROTOCOL`), `docs/interface.md` (generated), `core/src/error.rs`, `tui/src/**`, `engine/src/watch/**`, `scripts/tripwire/allow.txt` (glossary-audit-rename.md, "Always overlapping", refreshed from the map).

What **must** land in one change: the store rename, its migration, the code that speaks the new table/column names and the `SCHEMA_HASH` repin (§3.4); the Event kinds, the pi adapter's `KINDS` and the `PROTOCOL` bump (§3.5, hs-n628); the CLI group with its flags and the regenerated `docs/interface.md` (§3.1-§3.2). What **can** land alone: the prompt prose that the adapter does not parse (worker.md), the skills and their guard flip, and the non-DDL docs. What must land in **one Plan's PRs or several** is Part 4's proposal, not a decision (glossary-rename.md:207).

## Part 4 — The cost and the order

This is a **proposal**; the User approves scope at the Plan review, so nothing here is decided (glossary-rename.md:207; plan.md "Out of scope"). Files and lines are the current tree's; counts are the Part 1 whole-file grep counts or the map's, marked.

### 4.1 The cost per class

| Class | Files and lines to change | What breaks | Can be staged alone (no pi adapter break) | Must land in one change |
|---|---|---|---|---|
| 3.1 CLI commands | `cli/src/interface.rs:69-82`, `cli/src/cli.rs:58-60, :63, :184-185`, `docs/interface.md` (regenerated), `docs/commands.md:1814-2453`, the TUI/engine argv callers | The command registry, the generated interface page, the clap help tests, the 392 argv literals (map, refreshed) | No: needs the Workflow types | The `workflow` group with §3.2 and §3.4's code names |
| 3.2 Flags | `core/src/args.rs:38-42, :64, :72`; `core/src/job/mod.rs:133-134`; `core/src/pr/mod.rs:214`; `core/src/incident.rs:41, :157-160`; the TUI argv builder | The one `PrArg` struct, every selector, the skill tests that read `docs/commands.md` | No, with §3.1 | With §3.1 |
| 3.3 Refusal codes | `core/src/error.rs:161, 179-184, 288-298, 307-309, 323, 342`; `docs/commands.md:121-205`; `docs/contract.md` §1.2; the recreated trigger texts | Error-code tests (`error.rs:492-594`), the docs tables, the trigger `RAISE` texts (recreated; old text stays as history) | Yes, with the code that raises them | With §3.1/§3.4 |
| 3.4 Store | The new migration + `core/src/store/schema.rs:113, :456, :571-572, :586, :597` + 94 SQL-text files (map) + `core/src/plan/fingerprint.rs:51, :70` | Every SQL string, the schema check, the fingerprint input; voids stored fingerprints if keys rename | No: the table rename is a barrier | Migration + code + repin + `history_tables` |
| 3.5 Events | `core/src/events/kind.rs:97-105, :38, :260`; `core/src/events/typed.rs:228-324, :564-568`; `docs/contract.md:2595`; `pi/lib/event.ts:9, :17, :22`; `pi/lib/updates.ts:56, :140-153` | The adapter's `KINDS`, `PLAN_KINDS`, the protocol test, `pi/test` | No | Kinds + adapter + PROTOCOL 20 (hs-n628) |
| 3.6 TUI | `tui/src/widgets/legend.rs:180, :198, :461`; `tui/src/read/flow.rs:190, :480`; `tui/src/read/mod.rs:8`; test fixtures inserting `prs` | The section title, the doc strings, every fixture that inserts `prs` | Text alone yes; fixtures with the store | Fixtures with §3.4; text may follow |
| 3.7 Prompts/adapter | `core/src/spec/worker.md:23, :26, :37, :252`; `pi/lib/updates.ts:209-215`; `pi/lib/turns.ts`, `pi/lib/reminder.ts`, `pi/lib/slack-parse.ts`, `pi/durable-host/turn.ts` (map) | `pi/test/updates.test.ts`; a live session reads it at reload | Prompt prose yes; adapter text no | Adapter text with §3.5 |
| 3.8 Skills | `pi/skills/cpd-lead/SKILL.md` (75 lines), `pi/skills/cpd-manager/SKILL.md` (14), `pi/skills/skill.test.ts:110` | The skill guard test until it flips | Yes (prose) | The guard flip with the first new word |
| 3.9 Docs | `docs/contract.md` (764 lines), `docs/commands.md` (637), `docs/implementing.md:46, :113, :122, :129`, `docs/architecture.md:253`, `docs/interface.md` | The contract DDL-vs-schema test, the generated interface page | Yes, except the two machine-checked ties | The DDL block with §3.4; `interface.md` with §3.1 |
| Policy (outside the repo) | `~/.cpd/policy/clayton-duarte/owner.toml:2, :7, :8`, `cpd.toml:22, :25, :28-30`, `missionlane/owner.toml:9, :16` | `policy_invalid` for a moved key (`jira_required`); the Lead reads old texts | User-edited, not a Job's (the map, refreshed) | Nothing in the repo; the User edits after install |

### 4.2 A proposed staging

The map's order of work (glossary-audit-rename.md, "Order of work", refreshed from the map) builds one Workflow, many Jobs, docs first, then red tests, then the implementation, with the four `ren-red-*` in parallel and the Rust chain serial. A leaner proposal, which the User weighs:

- **One Plan, one Workflow, many Jobs (map R2a, refreshed).** Shape `tdd` (plan.md D3: the rename PR is registered `shape tdd, needs research`). Jobs: `glossary`/`guard` (CONTEXT.md, README, the retired-term guard + the skill-test flip), the doc Jobs (disjoint line ranges of `docs/contract.md` and `docs/commands.md`), the four red-test Jobs (`ren-red-store`, `ren-red-cli`, `ren-red-wire`, `ren-red-policy`), then the serial Rust chain (`migration` → `sql` → `types` → `fields` → `wire` → `cli` → `module` → `policy` → `text`) with `pi` parallel to the Rust chain, then the test renames, then `final` (re-bless `docs/interface.md`, repin, guard green, clean remnants). The map lists each Job's files and red tests (glossary-audit-rename.md:213-233, refreshed from the map). The first full Gate is the open Job, so the docs-tied tests may be red in between (glossary-audit-rename.md:215).
- **Two Workflows, stacked, if the slice is wanted.** Workflow A: docs + guard + skills + prompts (nothing the adapter parses). Workflow B (base A, stacked): the store migration, the types, the events and the CLI. Cost: A leaves `docs/contract.md`'s DDL block and the guard red until B lands, so A's open Job cannot go green (`docs/interface.md` and the contract DDL test are machine-checked, §4.1). The map's R2b calls two stacked Workflows a first-Workflow-fails-its-Gate shape; quoted as the map's, refreshed.
- **`pi` parallel to the Rust chain** (map, refreshed): the adapter's `KINDS` and parse text touch only `pi/`, so `ren-impl-pi` runs beside the Rust chain once the contract §5.2 Event table is agreed; the PROTOCOL constant is the one meeting point (hs-n628).
- **Order across Plans.** The map's `overlaps`/"Always overlapping" rows and hs-n628 say the rename should land after the Plans above merge (map R1a, refreshed), because they hold `core/src/pr`, `docs/contract.md`, `docs/commands.md`, `core/src/events` and `pi/lib`. A conflict is the update Job's (hs-n628); the rename's own migration competes with every other migration for the newest name (§3.11).

### 4.3 A proposed migration and cutover order

1. **Answers first.** The User settles the per-command boundary (§3.10), the Event question (§3.5/R6), the error-code split (§3.3/R13), the fingerprint question (§3.4/R12) and the `cancel_reason` rewrite (§3.11); the map's R1-R21 are the menu (glossary-audit-rename.md:249-260, refreshed from the map).
2. **The Workflow lands green.** One change (or the stacked pair) carries the store migration, the code and the docs through the open Job's Gate; nothing else can speak the new names (§3.4).
3. **Back up.** `sqlite3 ~/.cpd/cpd.db ".backup <file>"` — cheap to undo (the map's cutover step 3, refreshed). The live store is where the rename is irreversible in practice.
4. **Install on a quiet machine.** No Attempt mid-submit: the schema check reads the store verdict, the engine restarts and migrates (docs/contract.md:1080; the map's cutover step 4). An older binary then refuses with `store_too_new` (core/src/store/verdict.rs:196).
5. **The User edits the policy files outside the repo** (§4.1), because a Job may not: `clayton-duarte/owner.toml:2, :7, :8`, `cpd.toml:28-30` and `missionlane/owner.toml:9, :16` (the map's persistence row, refreshed). A moved policy key gives `policy_invalid` (the exposure rules, core/src/policy/exposure.rs:14-26; core/src/policy/default.toml:11).
6. **Restart Leads and Managers** so the skills and the adapter reload at the next Session start (glossary-audit-rename.md, cutover step 6, refreshed; the protocol-mismatch behaviour is unverified).
7. **Check.** `cpd pr add` is a usage error, `cpd workflow add` works, `cpd plan show` prints `workflows` (the map's step 7, refreshed).

### 4.4 What stays unverified

Carried from the map's risks and Part 1's "What Part 1 does not claim" (glossary-rename.md:161-164): the `ALTER TABLE RENAME` behaviour under the runner's `legacy_alter_table` on (§3.11 step 3); the pi adapter's behaviour on a protocol mismatch; `pi/lib` beyond the cited lines; the repo policy files outside `~/.cpd/policy`; clap's exit code for an unknown subcommand versus cpd's `usage` code; live store row counts (the map's 2026-10-28 snapshot, not re-measured here); and whether the `attempt-abort` Plan bumps PROTOCOL to the same number (the update Job decides, hs-n628). `docs/research/glossary-audit-rename.md` remains the row-by-row source; this Part 3/4 refreshes its decisions R1-R21 and its order of work against the current tree and cites them where reused.
