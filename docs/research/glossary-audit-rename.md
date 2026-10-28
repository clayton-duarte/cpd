# Glossary audit: the rename map

## Scope and coverage

Consolidates the 18 part audits (`docs/research/glossary-audit-<part>.md`; short names used below: vocab, ct1, ct1b, ct2, ct3, ct4 = contract-1..4, cm1..cm4, cm4b = commands-1..4b, pi, schema, core, cli, tui, tests, usage). Decided and not reopened: pro-d136, pro-d138 (PR means only the pull request; the object is a Workflow; no alias, no shim; history keeps old words). Sense codes are the part codes. Rule cell: `mechanical` (object renamed, pull request stays) or `D (Rn)`: the Spec's D<n> is numbered Rn in `## Decisions for the User`.
Method: I read every part's tables for the identifier, command, string and fix-list material in full except the long `## PR occurrences` prose groups of the contract and commands parts (read only where they feed a row: unverified for the rest), then checked rows against the repo and the live machine.
Ten rows opened and confirmed by me (quote at the place): `cli/src/interface.rs:69-81` (13 `pr` rows), `core/src/args.rs:41` (`long = "pr"`), `core/src/pr/cancel.rs:114` (reason text), `core/src/policy/default.toml:7`, `engine/src/doctor/state.rs:92`, `pi/skills/skill.test.ts:110`, `core/src/events/kind.rs:38` with `pi/lib/event.ts:9` (PROTOCOL 17), `core/src/pr/mod.rs:313` (log file name), `core/src/home.rs:174` and `:490`, `pi/lib/updates.ts:56` with `pi/lib/turns.ts:32`, `core/src/plan/fingerprint.rs:51,70`, `core/src/job/open.rs:20`, `core/src/ids.rs:141`, `engine/src/watch/pr.rs:40`; also `~/.cpd/policy/**` and `~/.cpd/cpd.db` (read-only).
Where parts disagree (shown in the tables): `pr_posts` (schema, ct1, ct2: pull request; core, cm2: mixed); `pr-merged` (ct1: pull request; schema, core, tui, cm2, cm3, cm4: mixed); `pr_not_open` (ct1: pull request; core: object; schema: mixed); `pr_not_ready` (schema, core: object; cm2, ct1: two meanings); `pr_number|pr_url|pr_opened_at` (schema: pull request; core, tui, cm2: mixed); `pr view` and `pr describe` (schema: workflow; vocab, cm2, pi: stay or decision); inner `pr` key of `plan show` (schema: unchanged; cm2: split into `workflow` and `pull_request`).
Live counts differ from the schema part because the store grew since (now: `prs` 298 vs 295, `pr_needs` 223 vs 216, `jobs.pr` non-null 1741 vs 1717, `pr-%` Events 1120 vs 1101, `plan_reviews` 114 vs 112); I use my numbers and say so. Part counts are the parts' own and move up to 2%.
Not covered by any part (unverified): contents of repo policy files outside `~/.cpd/policy`; `pi/lib` beyond 56 hit lines (pi part skimmed it); the 4% of test files no part opened. The largest uncertainty is `object` counts in prose: they are upper bounds (pi, core sampled ~80% object).

## Rename map

Counts are tokens or rows as stated by the named part (unverified by me unless "live"). Hazard cites path:line.

### CLI nouns and flags

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| `cpd pr add` `edit` `cancel` `start` (`cli/src/interface.rs:69-73`) | `cpd workflow add` `edit` `cancel` `start` | object | 5 table rows (cli); 392 argv literals in 60 test files (tests) | mechanical | Old forms must be usage errors (pro-d138); tui and engine build argv from these |
| `cpd pr describe` (`interface.rs:71`) | `cpd workflow describe` | mixed | 6 skill uses (pi) | D (R3) | Text lands in the pull request body; refusals `pr_ended`, `pr_adopted` are Workflow states |
| `cpd pr open` `rebase` `push` (`interface.rs:74,76,77`) | stay under `pr` | mixed | 8 + 9 + 6 skill uses (pi); 69 literals in core (core) | D (R3) | Act on a Workflow's Lead Branch and reach its pull request; Gate text, commit message and retry hints name them |
| `cpd pr view` (`interface.rs:78`) | stays under `pr` | mixed | 6 skill uses (pi) | D (R3) | Reads the pull request, selected by a Workflow key |
| `cpd pr poll` `reply` `post` `merge` (`interface.rs:75,79-81`) | unchanged | pull request | 5 rows (cli); 22 uses in tui (tui) | mechanical (stays) | `pr merge` is a cockpit action (`interface.rs:81`); engine spawns `pr poll` (`engine/src/watch/pr.rs`) |
| group help `Pr(pr::Command)` (`cli/src/cli.rs:58-60`) | two groups: `workflow`, `pr` | mixed | 1 | D (R3) | Help text is generated into `docs/interface.md` (never edit by hand; `CPD_BLESS=1`) |
| flag `--pr <key>` (`core/src/args.rs:41`, `core/src/job/mod.rs:135`, `core/src/pr/mod.rs:201`) | `--workflow <key>` | object | 96 test uses (tests); 25 text uses (core); 27 mentions in commands §9 (cm2) | D (R4) | One clap struct `PrArg` feeds `pr *`, `job add`, `job edit`, `policy show`; tui argv `tui/src/widgets/mod.rs:259` |
| flag `--pr <url|owner/repo#n>` of `review submit` (docs/commands.md:5591) | stays `--pr` | pull request | 2 (schema) | D (R5) | Same token, two senses today; after the rename only one |
| flag `--until-pr` of `incident start` (`core/src/incident.rs:41`) | `--until-workflow` | object | 34 test uses (tests); 19 uses (schema) | mechanical | Stored in `incident.until_pr` and Event data; error `until_pr_not_incident` |
| placeholder `<pr-key>`, `<pr>`, `open-<pr>` | `<workflow-key>`, `<workflow>` | object | 9 in the Lead skill (pi) | mechanical | Skill tests read docs/commands.md for every flag (`pi/skills/skill.test.ts`) |
| `--base <plan>/<key>`, `--needs`, `--no-needs`, `--superseded-by`, `--adopt`, `--confirm-author`, `--shape`, `--title`, `--jira` | names unchanged; help text says Workflow | object | 0 name hits in cli part | mechanical (text only) | `--base` takes a branch or `<plan>/<key>` (docs/commands.md:1947) |
| external `gh pr <verb>`, GraphQL `is:pr` | unchanged | other | 17 + 4 (schema) | mechanical (stays) | GitHub's own words |

### JSON output keys

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| `pr` = Workflow key or `{key,status,...}` in outputs of add, edit, describe, cancel, start, open, push, rebase, post, reply, janitor (`core/src/pr/rebase.rs`, `open.rs`, `start.rs`) | `workflow` | object | 21 literals in `core/src/pr` (schema); 152 `"pr"` literals in core, mixed with SQL columns (core) | mechanical | Same token is the pull request object in `plan show` (next rows); read by the Lead as text; adapter use unverified |
| `prs` list of Workflows (`core/src/plan/show.rs:92`) | `workflows` | object | 25 literals (core); 83 test uses (tests) | mechanical | pi parses it (`pi/lib/updates.ts:140,145`); PROTOCOL bump |
| `pr` / `pull_request` inside each `plan show` entry (`PrRow::to_json`) | unchanged | pull request | 6 (schema) | D (R21) | cm2 proposes `workflow {...}` plus `pull_request {...}`; ct1 notes two fields name the pull request |
| `base_pr`, `from_pr`, `to_pr`, `needs_other_pr` | `base_workflow`, `from_workflow`, `to_workflow`, `needs_other_workflow` | object | 68 (schema); 30 test uses (tests) | mechanical | Event data of `pr-base-changed` (3 live Events) |
| `jobs[].pr`, `details.pr`, `done[{plan,pr}]` of poll | `workflow` | object | 1741 live non-null `jobs.pr` | mechanical | Job JSON is Locked and fingerprinted (R12) |
| `details.prs` of `policy_in_use` (`core/src/policy/mod.rs:189`) | `workflows` | object | 2 (schema) | mechanical | Printed in an error detail |
| `closed_prs`, `left_open [{pr,number}]` (docs/commands.md:1172,1188,1195) | `closed_workflows`, `[{workflow,number}]` | mixed | 3 (cm1) | D (R9) | Workflow key beside a pull request number |
| `released {number,url,lead_worktree,lead_branch}` of `pr cancel` | unchanged keys | mixed | 1 (cm2) | D (R9) | Pull request and Workflow fields in one object |
| Plan fingerprint canonical keys `pr`, `prs` (`core/src/plan/fingerprint.rs:51,70`) | unchanged (frozen hash input) | object | 114 live `plan_reviews`; 10 test uses (tests) | D (R12) | Renaming changes every stored fingerprint |
| `workflow` key of `run/deploy-runs.json` entries (docs/contract.md:142 per ct1) | unchanged | other | 1 | D (R19) | A GitHub Actions workflow |

### Database tables, columns, triggers, views and indexes

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| table `prs` (`core/src/store/schema.rs:446` `delete_owner`, `:567` legacy list) | `workflows` | object | live 298 rows; `history` 1075 rows; 78 SQL and 130 Rust-string uses (schema) | mechanical | Breaking migration (`store_too_new` for older binaries); 94 non-test files hold SQL text that fails at run time |
| table `pr_needs` | `workflow_needs` | object | live 223 rows; `history` 225; 67 uses (schema) | mechanical | Locked after the Plan lock (trigger `pr_needs_locked`); `needs` is fingerprinted |
| table `pr_posts` | table stays | pull request | live 6 rows; 23 uses (schema) | D (R8) | Keyed (plan, pr); core and cm2 say mixed |
| table `pull_requests`, `pull_requests_*` triggers | unchanged | pull request | live rows not recounted; 121 uses (schema) | mechanical (stays) | `prs.pull_request` references it, so the rename must keep the reference |
| column `jobs.pr`, `pr_needs.pr`, `pr_posts.pr`, `slack_reactions.pr` | `workflow` | object | live 1741 + 223 + 6 + 11 | mechanical | Column names are in `SCHEMA_HASH`; trigger bodies and `UPDATE OF` lists name them (ct2 1925, 1931, 1995) |
| column `incident.until_pr` | `until_workflow` | object | live 0 non-null | mechanical | Event data `incident-started` carries it too |
| columns `prs.pr_number`, `pr_url`, `pr_opened_at`, `prs.pull_request` | unchanged | pull request | 202 uses (schema) | D (R9) | Table is renamed, columns hold pull request data; core and tui say mixed |
| hand-written triggers `prs_locked`, `prs_status`, `prs_shape_fixed`, `prs_head_ref_fixed`, `pr_needs_locked`, `pr_needs_no_delete`, `jobs_pr_dependencies`, `dependencies_same_pr` | `workflows_*`, `workflow_needs_*`, `jobs_workflow_dependencies`, `dependencies_same_workflow` | object | 8 live | mechanical | Must be dropped and recreated; `jobs_locked` body names `pr` and is recreated too; `RAISE` texts hold `pr_locked` (ct2 1945) |
| generated triggers `prs_hist_*`, `prs_no_delete`, `pr_needs_hist_*` | regenerated by the K4 reconcile from `history_tables` | object | 12 (ct2) | mechanical | Old names must be dropped or `SCHEMA_HASH` and the check fail (ct2 1243) |
| index `prs_head_ref_live`; autoindexes of `prs`, `pr_needs` | `workflows_head_ref_live` | object | 1 + 4 live | mechanical | Autoindex rename by SQLite is unverified |
| `history_tables` rows `prs`, `pr_needs`; `history.tbl` values | rows renamed; history values stay | object | 3 live `history_tables` rows named `pr%` | mechanical | History is append-only and replayable (ct2 1827): readers need a replay rule for old `tbl` values |
| views | none exist | n/a | 0 live | mechanical | none |
| `prs_new`, `pr_needs_new`, `schema_migrations` names `*-pr-*`, `*-stacked-prs` | unchanged (applied history) | other | 75 live migrations | mechanical (stays) | Checksums of applied files; never edited |

### Event kinds and their data fields

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| `pr-added` `pr-edited` `pr-described` `pr-cancelled` `pr-started` `pr-base-changed` (`pi/lib/event.ts:22`) | `workflow-added` ... `workflow-base-changed` | object | 295, 14, 48, 82, 223, 3 live Events (schema, earlier counts) | mechanical | `events.kind` references `event_kinds`; additive insert + PROTOCOL bump; pi rejects an unknown kind |
| `pr-opened`, `pr-merged` (`pi/lib/updates.ts:56`, `pi/lib/turns.ts:32`) | stay | mixed | 198, 197 live (schema) | D (R6) | ct1 says pull request, others mixed; adapter counts and reads them |
| `pr-feedback`; Inbox kinds `pr-ready`, `pr-failing`, `pr-changes` | unchanged | pull request | 41 / 666 / 204 live (schema) | mechanical (stays) | Inbox subjects `pr:<owner>/<repo>#<n>` (948 live) name a pull request |
| data key `pr` (Workflow key) of `job-added`, `job-merged`, `all-merged`, every `pr-*` kind | `workflow` | object | 28 literals (schema); `job-added` 1717 | mechanical | pi reads `data.pr` of `pr-opened` (`pi/lib/updates.ts:110` per schema); constructors in `core/src/events/typed.rs` |
| data key `prs` of `plan-phase`, `plan-done` | `workflows` | object | 163 + 78 live | mechanical | Old rows keep `prs` |
| data keys `from_pr`, `to_pr`; `until_pr`; cause `until-pr` | `from_workflow`, `to_workflow`, `until_workflow`, `until-workflow` | object | 68; 19 (schema) | mechanical | `incident-ended` cause is stored |
| optional key `pr` of `plan-phase` (`core/src/events/kind.rs:158`) | delete | object | 0 live carry it | mechanical | Dead key (`typed.rs:645` says no command makes it); schema part |
| `by`/`source` strings `pr start`, `pr add`, `pr open`, `pr poll` (`core/src/job/open.rs:20`, `core/src/pr/start.rs:35`) | follow the command names | mixed | 145 + 27 + 8 + 5 live (schema) | D (R3) | Stored in Events; history keeps old values |
| error code `pr_start_failed` in `error` Events (`engine/src/watch/engine.rs:265`) | `workflow_start_failed` | object | 2 in tui (tui) | mechanical | tui colours log lines by it (`tui/src/logs/view.rs:144`) |
| `PROTOCOL` (`core/src/events/kind.rs:38`, `pi/lib/event.ts:9`) | 18 | other | 17 now | mechanical | Test pins 17 at `kind.rs:255`; rises with any new kind |

### Policy keys and tables

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| domain `[pr]`, `pr.feedback.checks`, `pr.post.*` (`core/src/policy/exposure.rs:14`) | unchanged | pull request | 71 key uses (schema); live in 2 owner files | D (R10) | Exposure table pins `pr.post` (owner and repo only) |
| `pr.jira_required` | `workflow.jira_required` | object | 3 (cm2); live `missionlane/owner.toml:9` | D (R10) | User file edit; errors as `policy_invalid` with `details.key` |
| `[gate] pr` (`core/src/policy/domain.rs:349`, `default.toml:7`) and alias `[pr] gate` | unchanged | mixed | 14 + 8 (schema); alias live in `clayton-duarte/owner.toml:3` | D (R11) | Docs disagree on spelling `pr.gate` vs `gate.pr` (vocab, cm1, pi) |
| role section header `## pr` (`pi/lib/policy.ts:22`) | follows the domain | mixed | 3 (schema) | D (R10) | Shown to every agent |
| `[deploys] workflows` (`core/src/policy/default.toml:54`) | unchanged | other | 68 uses (schema) | D (R19) | A GitHub Actions workflow; no machine file declares it |
| `watch.pr_poll_seconds`, `watch.pr_poll_transient_minutes` (`core/src/config/file.rs:332,337`) | unchanged | pull request | 24 test uses (tests) | mechanical (stays) | None in `~/.cpd/config.toml` (rg) |
| `pr_open_escalate_minutes` | unchanged | pull request | 4 (tests) | mechanical (stays) | Named after `pr open`; ct2 2289 calls it removed (unverified) |

### Environment variables

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| `CPD_TEST_PR_POLL` (`engine/src/watch/pr.rs:40`) | unchanged | pull request | 11-12 (schema, tests) | mechanical (stays) | Interval between pull request polls; listed in docs/contract.md:77 per schema |
| `PR_POLL_EVERY_TICK` test const | unchanged | pull request | 1 (tests) | mechanical (stays) | Test only |
| any variable naming the object | none exists | n/a | 0 (`CPD_[A-Z_]*PR` search, schema) | mechanical | `CPD_WF` is cpd 1 (skill test forbids it) |

### Files and directories under $CPD_HOME

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| `run/<plan>/logs/pr-<key>.<hook>.log` (`core/src/pr/mod.rs:309-313`) | `workflow-<key>.<hook>.log` | object | 117 live files | mechanical | Old logs stay unread; readers that glob `pr-*.log` unverified; docs/commands.md:4504 still says `pr.pre-merge.log` |
| `locks/pr-poll.lock` (`core/src/home.rs:172-174`) | unchanged | pull request | 1 | mechanical (stays) | Held by the engine's poll |
| `run/deploy-runs.json` | unchanged | other | 1 | D (R19) | Entries hold a `workflow` key |
| directory named `workflows/` | must not exist | other | 0 live | mechanical | `core/src/home.rs:490` forbids it (cpd 1 name); a Workflow is never a directory |
| `~/.cpd/policy/<owner>/*.toml` | content edited | mixed | 2 files hold `[pr]` | D (R10) | User files, outside the repo (see Persistence) |
| `run/<plan>/specs/*.md`, `plan.md`, Lead messages | unchanged (generated history) | object | many | mechanical (stays) | History keeps old words |

### Branch and worktree names

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| Lead Branch `<plan>/<key>`, legacy `cpd/<plan>/<key>`, `cpd/<plan>` | unchanged | other | 295 live `lead_branch` (schema) | mechanical (stays) | Built from Plan id and key, not from the word `pr`; CHECK pins the forms |
| Lead worktree `<repo>.<plan>.<key>` | unchanged | other | 207 live (schema) | mechanical (stays) | Same |
| commit message `cpd: pr rebase <plan>/<key>` (`core/src/pr/rebase.rs:112`) | follows R3 | mixed | 1 | D (R3) | Written into pushed git history |
| fixture branch `/pr{n}`, Plan ids `pr-post`, `nl-pr-a1-q1` | unchanged | other | 1 + 2 | mechanical (stays) | History and test data |

### Rust modules, types and functions

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| module `core/src/pr/` (31 files) | `core/src/workflow/` for add, edit, describe, cancel, start, row; rest stays | mixed | 1856 matches (schema) | D (R16) | `cpd_core::pr::start` is called from engine; tripwire allow list `scripts/tripwire/allow.txt:36,37,54` pins names |
| `engine/src/watch/pr.rs`, `tui/src/read/pr_look.rs` | unchanged | pull request | 1 each | mechanical (stays) | E5 polls pull requests |
| `PrKey`, `InvalidPrId` (`core/src/ids.rs:141`) | `WorkflowKey`, `InvalidWorkflowId` | object | 154 in core (core); 46 tui; 11 engine | mechanical | Error text label "PR key"; macro in `ids.rs` |
| `PrRow`, `PrSpec`, `PlanPrs`, `Pr`, `PrArg`, `PrSet`, `PrNode`, `PrStatus` | `WorkflowRow`, `WorkflowSpec`, `PlanWorkflows`, `Workflow`, `WorkflowArg`, `WorkflowSet`, `WorkflowNode`, `WorkflowStatus` | object | 167 + 15 (core); 60 + 47 (tui) | mechanical | tui `PrStatus::as_str` is the stored `prs.status` value (`tui/src/read/flow.rs:341` per tui) |
| `PrRef` | `WorkflowRef` for `core/src/pr/add.rs`; the `gh.rs` type stays | mixed | 17 (core) | D (R17) | Two different types share one name |
| `PrTitle`, `PrTitleRequired`, `describe_pr`, `pr_merged` bucket, tui `Pr`/`PrRow` | per row: rename when stored on the Workflow | mixed | 20 + 14 + 9 + 4 + 6 | D (R17) | Title serves both; ct2 says neither is a pull request |
| `Kind::PrAdded` `PrEdited` `PrDescribed` `PrCancelled` `PrStarted` `PrBaseChanged`, `By::PrStart` | `Workflow*`, `By::WorkflowStart` | object | 37 (schema); 8 (tui) | mechanical | Follows Event kinds |
| `Kind::PrOpened`, `PrMerged`, `PrFeedback`, `PrReady`, `PrFailing`, `PrChanges`, `PrState`, `PullRequest*`, `Pull`, `PR_*` consts, `merge_pr`, `create_pr`, `close_pr`, `head_pr`, `open_pr` | unchanged | pull request / mixed | 50 + 94 + 119 + 84 (core, tui) | mechanical (stays) / D (R6) | `PrOpened`, `PrMerged` follow R6 |
| `ErrorCode::Pr*`, `BasePr*`, `NeedsOtherPr`, `UntilPrNotIncident` (`core/src/error.rs:274-324`) | follow the codes | mixed | 87 (core) | D (R13) | Pinned by `core/src/error.rs` tests (474-560 per core) |
| functions and fields: `load_prs`, `known_prs`, `plan_prs`, `start_pr`, `startable_prs`, `find_pr`, `policy_of_pr`, `lead_branch_of_pr`, `pr_lead_worktree_path`, `.pr`, `.prs` | `*_workflow*`, `.workflow`, `.workflows` | object | 80 + 65 + 349 + 434 (core); 21 + 16 (tui) | mechanical | 233 Rust files by `\.prs?\b`; a field rename is crate-wide |
| locals and parameters `pr`, `prs`, test helpers `add_pr`, `locked_pr`, `fn pr(`, test names with `pr` | `workflow`, `workflows`, ... | object / mixed | 1288 bare `pr` (core); 409 test fn names, 269 `.pr(` calls (tests) | D (R17, R18) | A missed local compiles; only a ratchet test catches it |
| SQL text in Rust strings: `FROM prs`, `JOIN pr_needs`, `jobs.pr` | `workflows`, `workflow_needs`, `jobs.workflow` | object | 94 files, 183 with tests | mechanical | Fails only at run time (`store_failed`) |

### Error codes

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| `pr_in_use` `pr_locked` `pr_exists` `pr_ended` `pr_other_plan` `pr_not_found` `pr_required` `pr_empty` `pr_start_failed` `invalid_pr_id` `needs_other_pr` `until_pr_not_incident` `base_pr_unknown` `base_pr_unavailable` `base_pr_other_repo` `base_pr_not_open` | `workflow_*`, `base_workflow_*` | object | 45 (core); 94 `pr_*` test uses (tests) | mechanical | `RAISE(ABORT, ...)` in triggers carries `pr_locked`, `needs_other_pr`: trigger text and hash change |
| `pr_adopted`, `pr_not_open`, `pr_title_required` | `workflow_adopted`, `workflow_not_open`, `workflow_title_required` | mixed | 14 + 11 (core, schema) | D (R13) | Raised on a Workflow key about a state; core calls `pr_not_open` object, ct1 pull request |
| `pr_not_ready` | `workflow_not_ready` (needs unmet, `docs/commands.md:2137`); `pr_not_ready` stays for an unpostable pull request (`:2251`) | mixed | 5 (cm2) | D (R13) | One code, two meanings today |
| `pr_closed`, `pr_not_closed` | unchanged | pull request | 2 + 3 (cm2) | D (R13) | Pull request state |
| `adopt_not_open` `adopt_fork` `adopt_foreign` `adopt_cpd_branch` `adopt_taken` | unchanged | pull request / mixed | 5 (cm2) | mechanical (stays) | Names say `adopt`, not `pr` |

### pi adapter and skills

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| `KINDS` list (`pi/lib/event.ts:22`), `PLAN_KINDS` (`pi/lib/updates.ts:56`) | follow Event kinds | object / mixed | 10 (schema) | mechanical / D (R6) | `pi/test/event.test.ts` compares with contract §5.2 |
| `p.prs`, `p.pr` parse (`pi/lib/updates.ts:140-153`) | `workflows`, `workflow` | object | 3 | mechanical | Wire key; PROTOCOL 18 |
| turn-start text: `PR ${key} opened`, `PRs:`, `${plural(e.length,"PR","PRs")} opened` (`pi/lib/updates.ts:206,209,236,273,275`) | "Workflow <key>: pull request opened <url>" style | mixed | 7 | D (R6) | Pinned by `pi/test/updates.test.ts` lines 372-380, 443, 473, 490 (pi) |
| `reminder.ts:8,15`, `testguard.ts:7`, `mainguard.ts:8` strings, phrase `next pr` | Workflow wording | object / mixed | 5 (pi) | mechanical | Phrase list matches the Lead's own words; pinned by `pi/test/reminder.test.ts` |
| `pi/skills/cpd-lead/SKILL.md` | rewrite | object 271, pull request 60, mixed 85 | 419 prose + 168 command tokens (pi) | mechanical + D (R3) | Mixed rows need a sentence each; overlaps inbox-prs, stack-default |
| `pi/skills/cpd-manager/SKILL.md`, `references/spec-files.md`, `references/pr-description.md`, `core/src/spec/worker.md` | rewrite | object / mixed | 52 + 25 + 14 + 13 (pi) | mechanical + D (R20) | `pr-description.md` stays if R20a; worker.md is the Spec every Worker reads |
| `pi/skills/skill.test.ts:110` forbids `workflows?` | flip: forbid only the retired forms | other | 116 matches in file (tests) | mechanical | Fails the moment a skill says Workflow; also pins `### PR` (`:209`, R15) |
| `pr poll` strings, `gh pr checks --watch` (`pi/lib/noblock.ts:62`), `slack-parse.ts` `PR_URL_RE` | unchanged | pull request | 12 + 6 (pi) | mechanical (stays) | GitHub's words |

### Docs and generated files

| Old | New | Sense | Count | Rule | Hazard |
|---|---|---|---|---|---|
| `CONTEXT.md` entries PR (phase) :29, PR (step) :71, PR (entity) :111, Workflow :108, Avoid lines :109, :183 | delete :29; entity becomes Workflow; step per R15; Workflow gets a live entry | object 42 of 66 PR tokens | 66 (vocab) | mechanical + D (R15) | Overlaps inbox-prs; Avoid lines are an allowed place for retired terms |
| `README.md:7` levels table | pointer to CONTEXT.md, then Workflow level | object | 2 (vocab) | mechanical | One source of truth |
| `docs/contract.md` | rewrite per section | object 285+137 (§1-2), 436+147 (§3), 360 (§4), 128 (§5-6), 510 (§7-10) | tokens by part (ct1, ct1b, ct2, ct3, ct4) | mechanical + D | §4.5 legacy record (2111-2218) keeps its words (ct2); DDL blocks are compared with the schema by `cli/tests/store/store.rs:179,191` |
| `docs/commands.md` | rewrite; §9 split into `workflow` and `pr` | object 284 (§1-8), 388 (§9), 53 (§10-14), 36 (§15-20), 75 (§21-end) | tokens by part (cm1..cm4b) | mechanical + D (R3) | Keep the §9 number: help text and skills cite `§9.4`, `§9.5a` (`core/src/pr/mod.rs:81`); skill tests read this file |
| `docs/implementing.md`, `docs/ui-preview.md`, `docs/versioning.md`, `BACKLOG.md:16` | rewrite; versioning numbers (13, 45) already stale vs 17, 75 | object 25 + 2 + 0 + 1 | vocab | mechanical | `BACKLOG.md:16` "Remote workflows" now reads as the new object |
| `docs/interface.md` | regenerate | object 10 | vocab | mechanical | Generated by `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface`; never hand-edited |
| `docs/cockpit-parity.md` | history: move to `docs/archive` or leave | other | 11 cpd 1 Workflow (vocab) | D (R20 note) | Not under an allowed place today |
| `docs/adr/**`, `docs/archive/**`, `docs/research/**` | unchanged | history | n/a | mechanical (stays) | Allowed places of the guard |

## The command surface

Source: cm2 table (docs/commands.md:1731-2358), checked against `cli/src/interface.rs:69-81`. Recommended option column follows R3 (recommended a). Every command that changes or reads the Workflow takes `--workflow <key>` (R4), not a positional key, unless it takes a positional `<key>` today.

| Command (docs/commands.md) | Acts on | After the rename | Option and recommendation |
|---|---|---|---|
| `pr add` (9.4, :1933) | Workflow | `workflow add <key>` | Moves. `--adopt`, `--confirm-author` name a pull request but are flags of `add`: move with it |
| `pr edit` (9.5, :2015) | Workflow | `workflow edit <key>` | Moves |
| `pr describe` (9.5a, :2057) | both | decision | a) `workflow describe --workflow <key>` (recommended: the text is a Workflow column set before any pull request exists); b) stays `pr describe` (it becomes the pull request body) |
| `pr cancel` (9.6, :2081) | Workflow | `workflow cancel <key>` | Moves; `--superseded-by <plan>/<key>` moves with it |
| `pr start` (9.7, :2123) | Workflow | `workflow start <key>` | Moves |
| `pr open` (9.1, :1735) | both | decision | a) stays `pr open --workflow <key>` (recommended: its result is the pull request); b) `workflow open` (schema part); reading cm2: both fit |
| `pr rebase` (9.3, :1857) | both | decision | a) stays `pr rebase` (recommended: it publishes to the pull request); b) `workflow rebase` (its Lead Branch and worktree are the Workflow's) |
| `pr rebase --continue` (:1909) | both | follows `pr rebase` | Moves with it |
| `pr rebase --abort` (:1929) | Workflow's worktree | follows `pr rebase` | Asks no pull request; leans `workflow` (cm2) but splitting a command's modes is worse |
| `pr push` (9.9, :2270) | both | decision | a) stays `pr push` (recommended); b) `workflow push` |
| `pr view` (9.8, :2154) | pull request, selected via a Workflow | stays `pr view --workflow <key>` | Stays (cm2, vocab, pi agree; schema said move: its own mixed row says decision) |
| `pr poll` (9.2, :1805) | pull requests | unchanged | `--plan` adds that Plan's Workflows to the output |
| `pr reply` (9.8a, :2187) | Feedback item of a pull request | unchanged | `--pr` becomes `--workflow`; `--to <item-id>` stays |
| `pr post` (9.8b, :2219) | pull request announced in Slack | unchanged | `--pr` becomes `--workflow`; table `pr_posts` stays (R8) |
| `pr merge` (9.10, :2306) | pull request by `<owner>/<repo>#<n>` | unchanged | No Workflow selector |
| `host approve`, `host repair` (9.11, :2338) | no PR | unchanged | Not `pr` commands; section placement is outside this rename |
| `tui action host`, `tui action incident` (9.12, 9.13) | no PR | unchanged | Same |
| `--pr <key>` of `job add` (:1327), `job edit` (:1384), `policy show` (:4775) | Workflow | `--workflow <key>` | Command stays; defined by commands-1 and commands-4b parts |
| `--pr <url>` of `review submit` (:5591) | pull request | stays `--pr` | R5 (a recommended) |
| `--until-pr <plan>/<key>` of `incident start` | Workflow | `--until-workflow` | Mechanical |
| old forms after the rename | n/a | usage error | `cpd pr add|edit|cancel|start` and `--pr <key>` on moved commands exit as a usage error, no alias (pro-d138); clap rejects an unknown subcommand and flag: a red test must pin the exit code and text (unverified that clap's exit code equals cpd's `usage` code) |

## Persistence and wire

What is stored on this machine (live reads of `~/.cpd/cpd.db`, read-only) and what must migrate. Applied migrations are never edited (75 rows in `schema_migrations`, checksums stored).

| Item | Stored where | Safest way | Order |
|---|---|---|---|
| Tables `prs` 298 rows, `pr_needs` 223, `jobs.pr` 1741, `slack_reactions.pr` 11, `pr_posts.pr` 6, `incident.until_pr` 0 | `~/.cpd/cpd.db` | One new `breaking after <n>` migration (`scripts/new-migration.sh <slug> breaking`): drop the 8 hand-written triggers and the index, `ALTER TABLE ... RENAME` (RENAME avoids dropping a `history_tables` table, ct2 2077; the `legacy_alter_table` interplay is unverified), recreate triggers with new names and the `jobs_locked` body, `UPDATE history_tables`, let K4 regenerate the history triggers | 1 (before any SQL-text Job) |
| `SCHEMA_HASH` | `core/src/store/schema.rs` | `scripts/repin.sh` after the migration; `LEGACY` lists frozen (`schema.rs:567`) | 2 |
| `history` rows (`prs` 1075, `pr_needs` 225, `pr_posts` 8) | `cpd.db` (append-only) | Keep `tbl` values; add a replay rule for old names (ct2 1827) | with 1 |
| `events` with `pr-*` kinds (1120) and `event_kinds` (9 rows) | `cpd.db` (`events_no_update`) | Add new kinds with additive inserts (K1); old rows and old `event_kinds` rows stay as history; readers read new kinds only (R7a). Rewriting old Events means dropping an append-only trigger (R7b) | with 1; new kinds raise PROTOCOL once |
| Protocol | `core/src/events/kind.rs:38`, `pi/lib/event.ts:9` | 17 to 18 in one change with the kinds | with the Event Job |
| pi adapter in running pi sessions | loaded at Session start | New adapter and skills load at the next Session start or reload (unverified how a live session behaves on a protocol mismatch) | after install |
| `jobs.cancel_reason = 'pr cancelled'` (200 live rows) | `cpd.db`; written at `core/src/pr/cancel.rs:114`, read back by a test at `:377` (no production reader found by `rg`) | Rewrite in the same migration (R14a); Event `reason` values stay | with 1 |
| Plan fingerprints (114 `plan_reviews`, 1741 Job fingerprints) | `cpd.db` | Keep canonical keys `pr`, `prs` as a written, frozen hash input (R12a) | none |
| Policy files with the object: `~/.cpd/policy/clayton-duarte/owner.toml` (alias `[pr] gate` :3; text names `cpd pr rebase --pr <key>`, `cpd pr push --pr <key>`, `cpd pr open` :7-9) and `~/.cpd/policy/missionlane/owner.toml` (`jira_required` :9; text names `cpd pr add <key>`, `cpd pr edit <key>` :16) | User files, outside the repo | The User edits them (Jobs cannot): `jira_required` moves per R10; texts say Workflow. `clayton-duarte/cpd.toml:5` has a comment naming `pr open`. Other policy files (`host.toml`, `dotfiles.toml`, `card-dashboard.toml`) hold no hit (rg) | right after install |
| `~/.dotfiles` | n/a | `ls -A ~/.dotfiles/files` is empty; no policy or cpd text there | none |
| `~/.cpd/config.toml` | machine config | No `pr` key found (rg) | none |
| 117 Gate logs `run/<plan>/logs/pr-*.log`, Specs and Lead messages under `run/<plan>/` | files | Leave; only new logs use the new name | none |
| Branches and worktrees | git | Unchanged (no `pr` word in the forms) | none |

Cutover order: (1) answers to R1-R21; (2) the Workflow's pull request lands with a green Gate; (3) back up: `sqlite3 ~/.cpd/cpd.db ".backup <file>"` (86 MB; cheap to undo); (4) install on a machine with no Attempt mid-submit: `watch --check` reads the store verdict, the engine restarts and migrates (docs/contract.md:1080 flow); (5) edit the two policy files; (6) restart Leads and Managers so skills and the adapter load; (7) check: `cpd pr add` is a usage error, `cpd workflow add` works, `cpd plan show` prints `workflows`.

## Order of work

Repo shape: contract and commands docs first, then red tests for new behaviour, then the implementation. One Workflow, many Jobs (R2a). Names are proposals; each Job's red tests are listed. The Gate runs only at the open Job, so docs-tied tests (`cli/tests/store/store.rs:179`, `cli/tests/store/events.rs`, `pi/test/event.test.ts`, `pi/skills/skill.test.ts`) may be red in between. Mechanical test renames ride with the family that breaks them: a lib's unit tests cannot compile against unrenamed types; only `cli/tests/**` (separate binaries) can follow later.

| Key | Goal | Files | Needs | Red tests it needs |
|---|---|---|---|---|
| `ren-glossary` | CONTEXT.md: Workflow entry, delete PR phase entry, entity, Avoid lines; README pointer | 2 | R3, R15 | none (doc) |
| `ren-guard` | Guard test for retired terms (below) and flip `skill.test.ts:110` | 3 (new `cli/tests/tooling/retired_terms.rs`, its `mod` line, `skill.test.ts`) | `ren-glossary` | the guard fails listing offenders; skill test fails on today's text |
| `ren-doc-contract-1` | contract §1-2 (1-748) | 1 | `ren-guard` | guard count falls |
| `ren-doc-contract-2` | contract §3 (749-1205) | 1 | same | same |
| `ren-doc-contract-3` | contract §4 (1206-2419, not the legacy record 2111-2218): DDL, new migration entry, fingerprint note (R12) | 1 | same | `store.rs` DDL tie goes red |
| `ren-doc-contract-4` | contract §5-6 (2420-3490): Event table, PROTOCOL 18 | 1 | same | `events.rs` SPEC, `event.test.ts` go red |
| `ren-doc-contract-5` | contract §7-10 (3491-4258): Gate, policy domains | 1 | same | exposure table test goes red |
| `ren-doc-commands-1` | commands §1-8 (1-1730) | 1 | `ren-guard` | guard count falls |
| `ren-doc-commands-2` | commands §9 (1731-2358) split `workflow`/`pr`, keep §9 numbering | 1 | R3 | skill test reads it |
| `ren-doc-commands-3` | commands §10-14 (2359-3647) | 1 | same | guard |
| `ren-doc-commands-4` | commands §15-end (3648-5632), incl. dup/old fixes only if asked | 1 | same | guard |
| `ren-doc-skills` | Lead skill and its 3 references | 4 | contract and commands docs | skill tests |
| `ren-doc-misc` | Manager skill, `core/src/spec/worker.md`, `docs/implementing.md`, `ui-preview.md`, `versioning.md`, `BACKLOG.md` | 6 | same | skill tests |
| `ren-red-store` | New file `core/src/store/schema/workflow_rename_tests.rs`: old store with rows migrates, data and history kept, triggers work, `cancel_reason` rewritten, hash repinned | 2 | `ren-doc-contract-3` | fails: no migration |
| `ren-red-cli` | New file `cli/tests/flow/workflow_commands.rs`: new commands work; old `pr add|edit|cancel|start` and `--pr <key>` are usage errors; `plan show` has `workflows` | 2 | R3, R4 | fails: no `workflow` command |
| `ren-red-wire` | PROTOCOL 18, new kinds accepted, error codes (R13), JSON keys | 3 | `ren-doc-contract-4` | fails: old names |
| `ren-red-policy` | `workflow.jira_required` accepted, `pr.jira_required` is `policy_invalid`, exposure table | 2 | R10 | fails |
| `ren-impl-migration` | The breaking migration, `schema.rs` lists, `SCHEMA_HASH`, `store/mod.rs` trigger texts | 5 plus the `.sql` | all red-store | `ren-red-store` green |
| `ren-impl-sql` | SQL text in Rust strings, with the unit tests that fixture them | 94 | migration | store-backed tests |
| `ren-impl-types` | Types, ids, `ErrorCode` variants, `Kind` variants (src plus unit tests) | 71 (115 with `cli/tests`) | sql | build and unit tests |
| `ren-impl-fields` | `.pr`, `.prs`, functions, consts, locals (scripted, per crate order core, engine, cli, tui) | up to 221 | types | build and unit tests |
| `ren-impl-wire` | Event kinds and data keys, error code strings, JSON keys, PROTOCOL 18 | about 40 | fields, `ren-red-wire` | `ren-red-wire` green |
| `ren-impl-pi` | `pi/lib` kinds, parse, turn-start text, `pi/test` | 35 | `ren-doc-contract-4`; parallel to the Rust chain | `pi/test` |
| `ren-impl-cli` | `workflow` command group, `--workflow`, `--until-workflow`, interface table, tui and engine argv, bless `docs/interface.md` | about 12 | wire | `ren-red-cli` green |
| `ren-impl-module` | Move six files to `core/src/workflow/`, fix paths, `allow.txt` | about 20 | cli | build |
| `ren-impl-policy` | `workflow` domain, exposure, `default.toml`, `policy show`, `pi/lib/policy.ts` header | about 10 | R10, `ren-red-policy` | `ren-red-policy` green |
| `ren-impl-text` | Help, error and Event texts, tui legend and labels, engine log text, `dispatch or pr`, "workflow worktree" | about 90 | module | text tests |
| `ren-tests-pr` | Rename `cli/tests/pr/**` (34 files, 2752 matches) | 34 | text | area binary |
| `ren-tests-flow` | Rename `cli/tests/flow/**` | 51 | same | area binary |
| `ren-tests-rest` | `cli/tests/{common,engine,store,session,tooling,tui}/**` | about 50 | same | area binaries |
| `ren-final` | Re-bless `docs/interface.md`, repin check, guard green, clean `phase pr` leftovers | about 6 | all | guard and Gate |

Parallel: all `ren-doc-*` (disjoint line ranges of one file merge cleanly; Jobs that share `docs/contract.md` or `docs/commands.md` need a textual merge); the four `ren-red-*`; `ren-impl-pi` with the Rust chain; the three `ren-tests-*`. Serial: `ren-guard` before docs; `ren-impl-migration`, `-sql`, `-types`, `-fields`, `-wire`, `-cli`, `-module`, `-policy`, `-text`, then tests, then `ren-final`.
Largest Job: `ren-impl-fields`, up to 221 Rust files (349 files hold any pr word, 128 of them under `cli/tests`). It fits one Attempt because it is a scripted rule table (one `rg -l | xargs` pass per family, then compile and fix the `mixed` sites), not hand edits; zero-count `rg` checks prove completeness. If an Attempt runs out, the Lead splits it per crate: locals and parameters are file-local and independent. Docs Jobs have 1 file each, the largest region 1214 lines (§4).

Guard test (`ren-guard`): fails when a retired term appears in live text. Scanned: `CONTEXT.md`, `README.md`, `docs/*.md` (not adr, archive, research), `pi/skills/**`, `core/src/spec/*.md`, `core/src/context/*.md`, `docs/interface.md` and the help tree (the existing `cli/tests/tooling/interface.rs` walks it), non-test Rust string literals and `pi/lib` strings. Retired terms: `cpd pr (add|edit|cancel|start)` (and `describe` per R3), `--pr <key>` on commands that moved, tables `prs` and `pr_needs`, `base_pr`, `PrKey`, `PrRow`, `until-pr`, the six `pr-*` object kinds, the object error codes (list above), JSON `"prs"`, `pr-<key>.*.log`, the phrases `PR key`, `PR keys`, `Plan's PRs`, `PRs of the Plan`, `PR needs`, `base PR`, `waiting PR`, `PR status`, `workflow worktree`, `dispatch or pr`, `phase pr`, `pr.gate`, `sync Job`, and a bare `workflow` not preceded by `GitHub Actions` (allow `[deploys] workflows` and `workflow_runs`). The word PR cannot be grepped for sense, so the guard lists phrases and identifiers, not the word. Allowed places: CONTEXT.md `_Avoid_` lines, `docs/adr/**`, `docs/archive/**`, `docs/research/**`, applied migrations and `core/src/store/schema.rs` legacy lists and fixtures of old stores, `docs/contract.md` §4.5 legacy record, test fixtures that build an old store. Style to copy: `offenders(forbidden, allowed)` at `cli/tests/tooling/pill_sites.rs:65` (path:line: text, reason beside each allowed path).

## Overlaps with other active Plans

| Plan / branch (measured now with `git diff --name-only origin/main...HEAD`; Plan scope unverified) | Files that overlap the rename |
|---|---|
| inbox-prs | `docs/contract.md`, `docs/commands.md`, `CONTEXT.md`, `pi/skills/cpd-lead/SKILL.md`, `pi/skills/skill.test.ts`, `core/src/inbox/**`, `cli/tests/pr/pr_post_flow.rs`, `tui/src/**` (from the Spec) |
| jira-sync (`cpd.jira-sync.sync`; no diff against main yet) | docs, tests, impl of Jira moves: likely `core/src/pr/open.rs`, `pr.jira_required` policy (unverified) |
| stack-default (4 worktrees, 7-8 overlapping files each) | `core/src/pr/add.rs`, `cli/tests/pr/`, `docs/commands.md` §9.4, Lead skill `--base` text |
| adopt (`cpd.adopt.adopt.gate-5294`, 26 files) | `core/src/pr/` (8), `cli/tests/pr/` (6), `core/src/plan/` (3), `core/src/events/` (2), `core/src/store/` incl. a migration |
| italic (15 files) | `pi/lib/` (3), `pi/test/` (8) |
| bifrost provider (11 files) | `core/src/policy/` (7), `core/src/plan/` (1) |
| no-slots (6 files), review-item (4), update-hold (3), review-tier (2) | `core/src/pr/`, `cli/tests/pr/`, `CONTEXT.md`, `docs/`, one skill file |
| Always overlapping | `core/src/pr`, `core/src/plan`, `core/src/inbox`, `cli/tests/pr`, `docs/contract.md`, `docs/commands.md` (Spec); and from the parts: `core/src/store/schema.rs` (`SCHEMA_HASH`, migration list), any new migration (a breaking migration must sort after every name on origin/main: `scripts/check-migrations-frozen.sh`), `pi/lib/event.ts` and `core/src/events/kind.rs` (`PROTOCOL`), `docs/interface.md` (generated), `core/src/error.rs`, `tui/src/**`, `engine/src/watch/**`, `scripts/tripwire/allow.txt` |

## Decisions for the User

R1 Landing order: a) after the Plans above merge (recommended); b) first, they rebase; c) in slices.
R2 Shape: a) one Workflow, one pull request, many Jobs (recommended; docs-tied tests need docs and code together); b) two stacked Workflows (first fails its Gate).
R3 Commands: a) `workflow add|edit|describe|cancel|start`; `pr open|rebase|push|view|poll|reply|post|merge` (recommended); b) also open, rebase, push under `workflow`; c) view too.
R4 Selector on remaining commands: a) `--workflow <key>` (recommended); b) positional key.
R5 `review submit --pr <url>`: a) keep `--pr` (recommended); b) `--pull-request`.
R6 Event kinds `pr-opened`, `pr-merged`: a) keep, data key `workflow` (recommended); b) `workflow-opened`, `workflow-merged`; c) `pull-request-*`.
R7 Old Events and `event_kinds`: a) keep as history, add new kinds, read new only (recommended); b) rewrite old rows (drops the append-only trigger).
R8 Table `pr_posts`: a) keep, rename column `pr` to `workflow` (recommended); b) `workflow_posts`.
R9 Mixed shapes (`pr_number`, `pr_url`, `pr_opened_at`, `{pr,number}` pairs, `released`): a) keep pull request fields, rename the Workflow-key part (recommended); b) rename all.
R10 Policy: a) domain `pr` stays for feedback and post, `jira_required` moves to a new `workflow` domain (recommended); b) whole domain to `workflow`; c) keep `pr.jira_required`.
R11 `gate.pr` and the `[pr] gate` alias: a) unchanged (recommended); b) `gate.workflow`.
R12 Plan fingerprint keys `pr`, `prs`: a) frozen hash input, rename only Rust names (recommended; 114 reviews stay current); b) rename and re-review; c) rename and recompute in a Rust migration.
R13 Error codes: a) object `workflow_*`, split `pr_not_ready`, keep `pr_closed`, `pr_not_closed` (recommended); b) all `pr_*` renamed; c) keep mixed.
R14 Stored `cancel_reason` `pr cancelled`: a) rewrite in the migration (recommended); b) keep the stored value.
R15 Deploy step named PR: a) keep, it now means the pull request (recommended); b) rename.
R16 Module `core/src/pr`: a) split into `workflow` and `pr` (recommended); b) rename whole; c) keep.
R17 Rust locals and mixed names (`PrRef`, `PrTitle`): a) rename by script with a ratchet, per row (recommended); b) types, functions and fields only.
R18 Test names and dir `cli/tests/pr/`: a) rename object-sense names, dir stays (recommended); b) rename dir too.
R19 GitHub Actions `workflows` keys (`[deploys] workflows`, `deploy-runs.json`): a) keep, write the phrase in full (recommended); b) `actions_workflows`.
R20 `pr-description.md` and `docs/cockpit-parity.md`: a) keep the name (it is the pull request's description) and move cockpit-parity to archive (recommended); b) rename the file, leave cockpit-parity.
R21 Inner `pr` and `pull_request` keys of `plan show` and `pr view`: a) keep (recommended); b) unify to `pull_request`.

## Risks

1. Seven active branches touch the same files: a merge storm on `core/src/pr`, `docs/contract.md`, `docs/commands.md` (R1).
2. The breaking migration, `SCHEMA_HASH` repin and the migration-order check collide with any migration landed meanwhile; older binaries refuse the shared `~/.cpd/cpd.db` (`store_too_new`).
3. Docs-tied tests stay red until the last Job; the first full Gate is the open Job, so one late failure costs the whole chain.
4. Renaming the fingerprint keys would void 114 Plan reviews (R12).
5. Policy files and live Leads: wrong keys give `policy_invalid` (strictness unverified) and Leads hold old vocabulary until restarted; old commands are usage errors by decision.
6. `object` prose counts are upper bounds; a scripted rename can hit a pull request sense (`gh pr`, `pull_requests`, `PrState`): exclusions come from this map, and the guard checks phrases only.
7. A missed Rust local compiles; SQL text in strings fails only at run time, so `ren-impl-sql` needs store-backed tests.
8. Unverified: pi adapter behaviour on protocol mismatch, `ALTER TABLE RENAME` with `legacy_alter_table`, `pi/lib` beyond the hit lines, repo policy files outside `~/.cpd/policy`.
