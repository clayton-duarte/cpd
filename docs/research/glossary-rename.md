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
