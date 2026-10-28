# Glossary audit: core/src (without store/, events/, policy/ and `*_tests.rs`)

Attempt `aud-core-a1`, Job `aud-core`, Plan `glassary`. Read and classify only: no source file was changed. Sense codes: `object` (the cpd object that becomes a Workflow), `pull request` (the GitHub pull request), `phase`, `step`, `mixed`, `other`. Where the sense cannot be told from the text the code is `mixed` and the After the rename is `decision`.

## Coverage

**Part.** `core/src/**/*.rs` except `core/src/store/**`, `core/src/events/**`, `core/src/policy/**` and test files. The file list, run from the repo root, is exactly:

```sh
FILES=$(rg --files core/src -g '*.rs' -g '!core/src/store/**' -g '!core/src/events/**' -g '!core/src/policy/**' -g '!*_tests.rs' -g '!**/tests/**')
```

It lists **232 files**. Every count below uses `$FILES` from that command. Counts are of the tree at the base of this Attempt; other Plans that merge changes in between can move them by a few percent (accept within 2%).

Judgement calls on the boundary (flag for the Lead):

- Files named `tests.rs` (`core/src/testsel/tests.rs`, `core/src/resource/tests.rs`, `core/src/tag/tests.rs`, `core/src/prune/tests.rs`, `core/src/gate/tests.rs`) do not match `*_tests.rs` and are not under a `tests/` directory (none exists in the part), so they are **in** this part. Only `core/src/prune/tests.rs` matches the PR patterns (its helper `NO_PRS`, `merged_prs`).
- Inline `#[cfg(test)] mod` blocks sit inside 78 non-test files and are counted with their file (the part is files, not code regions). The Counts section splits production code from inline test code.
- Files named `*_tests.rs` (for example `core/src/pr/poll_tests.rs`) belong to the test part and are not read.

**Directories of the part** (files in the part, PR-pattern token occurrences, files with at least one occurrence):

| Directory | Files | Occurrences | Files with occurrences |
|---|---|---|---|
| `core/src` | 19 | 266 | 9 |
| `core/src/ask` | 3 | 0 | 0 |
| `core/src/attempt` | 8 | 50 | 3 |
| `core/src/config` | 7 | 8 | 1 |
| `core/src/context` | 21 | 63 | 7 |
| `core/src/driver` | 7 | 98 | 5 |
| `core/src/fakes` | 2 | 0 | 0 |
| `core/src/fakes/git` | 8 | 0 | 0 |
| `core/src/fakes/git/route` | 3 | 0 | 0 |
| `core/src/fakes/tmux` | 9 | 0 | 0 |
| `core/src/gate` | 2 | 0 | 0 |
| `core/src/git/hooks` | 4 | 1 | 1 |
| `core/src/git` | 5 | 71 | 4 |
| `core/src/host` | 13 | 1 | 1 |
| `core/src/host/pi` | 5 | 0 | 0 |
| `core/src/host/tmux` | 2 | 0 | 0 |
| `core/src/inbox` | 7 | 341 | 6 |
| `core/src/install` | 3 | 2 | 1 |
| `core/src/job` | 14 | 431 | 11 |
| `core/src/merge` | 2 | 94 | 2 |
| `core/src/metrics` | 1 | 0 | 0 |
| `core/src/models` | 4 | 0 | 0 |
| `core/src/plan` | 23 | 750 | 19 |
| `core/src/pr` | 26 | 2341 | 26 |
| `core/src/prune` | 2 | 71 | 2 |
| `core/src/resource` | 3 | 12 | 2 |
| `core/src/review` | 1 | 0 | 0 |
| `core/src/secret` | 4 | 0 | 0 |
| `core/src/session` | 17 | 0 | 0 |
| `core/src/spec` | 1 | 5 | 1 |
| `core/src/tag` | 2 | 0 | 0 |
| `core/src/testsel` | 2 | 0 | 0 |
| `core/src/version_sync` | 2 | 0 | 0 |

**What was read.** Method: enumerate with `rg`, then read the lines around the families. I did **not** read any file of the part in full. Read as match lines with their context: every line that matches the pattern `T` below, in the 101 files that have a match (all of them, via the tables below). Skimmed for structure: `core/src/pr/mod.rs` lines 1 to 200 (module doc and clap help), `core/src/args.rs` lines 37 to 75 and 205 to 235, `core/src/error.rs` match lines, `core/src/ids.rs` line 141, `core/src/home.rs` lines 484 to 494. Not read: the 131 files with no match (no rename can touch them), and the bodies of the files with matches outside the matching lines (a sentence that names a PR without the token, for example "the Lead Branch's change", is not found: unverified).

**Token pattern `T`** (every use of the abbreviation, the type and identifier forms, and `pull request` prose; a superset of the Acceptance patterns `\bprs?\b`, `Pr[A-Z]`, `pr_`, `_pr\b`, `pull_request`, `PullRequest`):

```sh
rg -P -o -e '\w*(?:Pr[A-Z]|pr_|_prs?\b|pull_?request|PullRequest|PR_|_PRS?\b)\w*|\b(?:[Pp]r|PR)s?\b|\b[Pp]ull [Rr]equests?\b|\bpull-request' $FILES | wc -l   # 4605
```

`T` finds 4605 occurrences in 101 files. **Coverage check:** every one of those occurrences falls inside the match of at least one row of the next section; the uncovered list is empty (0 uncovered). Method: each row's pattern was run with `rg -P -o -n --column`; an occurrence is covered when a row match contains it (script not committed; the rule is: row spans contain the `T` span).

Other commands behind the tables:

```sh
rg -n -e '"[^"]*\b(PRs?|pull requests?|Pull requests?)\b[^"]*"' $FILES      # string literals with the abbreviation: 253 lines, 235 before inline test blocks
rg -n '^\s*///[^\n]*\b(PRs?|pull requests?)\b' $FILES | wc -l    # doc comments: 634
rg -n -i workflow $FILES                                       # collisions with the new term: see Wrong or stale
```

The strings table widens the literal search to `cpd pr <verb>`, `--pr`, `pr-<word>` and `gh pr`, keeps only code before an inline test block, and adds doc comments that are clap help (the doc line is followed by `#[arg`, `#[command` or an enum variant).

## Identifier families

Count: `R<n>` is the command in the list after the table (all use `$FILES`); the number is the `rg -P -o | wc -l` occurrences of that row's pattern alone; rows overlap, so `excl.` is the number of `T` occurrences the row owns when rows are applied in table order (first match wins). The sum of `excl.` is the total of `T`. Sense of a pattern row is the sense of **most** of its occurrences; sampled occurrences that differ are named in the Hazard cell.

| Family | Surface | Sense | Count | Files (top 5) | After the rename | Hazard |
|---|---|---|---|---|---|---|
| `pr-feedback` | Event kind (text/doc/comment) | pull request | R1: 6 (excl. 6) | `core/src/pr/poll.rs` (3), `core/src/pr/feedback.rs` (2), `core/src/pr/pulls.rs` (1) | unchanged | Event kind is stored in the events table (store part) and read by the pi adapter; spec names it pull request |
| `pr-started` | Event kind | object | R2: 3 (excl. 3) | `core/src/pr/start.rs` (3) | `workflow-started` | stored events.kind values, pi adapter and Lead/Manager skills match the kind |
| `pr-opened`, `pr-merged` | Event kind | mixed | R3: 9 (excl. 9) | `core/src/pr/open.rs` (8), `core/src/pr/poll.rs` (1) | decision | stored events.kind; the Event says a Workflow's pull request opened/merged (text "PR {key} (#n) ... is merged") |
| `pr-ready`, `pr-failing`, `pr-changes` | Inbox item kind (string and docs) | pull request | R4: 21 (excl. 21) | `core/src/inbox/model.rs` (9), `core/src/inbox/rules.rs` (8), `core/src/inbox/pr.rs` (3), `core/src/pr/merge.rs` (1) | unchanged | stored inbox rows (kind) and Slack/Inbox renderers; about the User's authored pull request |
| other kebab `pr-*` names | kebab names in strings/comments (e.g. lock file `pr-poll.lock`) | mixed | R5: 57 (excl. 18) | `core/src/inbox/model.rs` (10), `core/src/pr/open.rs` (9), `core/src/inbox/rules.rs` (8), `core/src/pr/poll.rs` (7), `core/src/pr/start.rs` (5) | decision | file names under $CPD_HOME/locks |
| `pr_not_found`, `pr_other_plan`, `pr_required`, `invalid_pr_id`, `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_start_failed`, `pr_not_open`, `needs_other_pr`, `until_pr_not_incident`, `base_pr_{unknown,unavailable,other_repo,not_open}` | error code (snake_case string) | object | R6: 45 (excl. 45) | `core/src/error.rs` (17), `core/src/pr/add.rs` (8), `core/src/pr/start.rs` (4), `core/src/pr/edit.rs` (4), `core/src/pr/mod.rs` (3) | `workflow_*` / `base_workflow_*` / `until_workflow_*` | printed in `error.code` (JSON contract, docs/commands.md), pinned by tests in error.rs and every command test; pi adapter and skills may match codes |
| `pr_closed`, `pr_not_closed`, `pr_title_required`, `pr_adopted` | error code (snake_case string) | mixed | R7: 14 (excl. 14) | `core/src/pr/open.rs` (5), `core/src/error.rs` (4), `core/src/job/open.rs` (2), `core/src/pr/describe.rs` (1), `core/src/pr/cancel.rs` (1) | decision | same as other codes; each says something about the Workflow's pull request (closed, adopted, title of the pull request to open) |
| `pr_status`, `pr_number`, `pr_url`, `pr_opened_at` | SQL column alias / local variable / struct field | mixed | R8: 52 (excl. 52) | `core/src/driver/start.rs` (9), `core/src/pr/open.rs` (8), `core/src/plan/prs.rs` (6), `core/src/plan/list.rs` (3), `core/src/plan/show.rs` (3) | decision | columns `prs.pr_number`, `prs.pr_url`, `prs.pr_opened_at` (store part owns the migration): hold the pull request's number/url/time inside the Workflow row; `pr_status` is the Workflow status |
| `pull_requests` table / `pull_request` column | SQL table, column, JSON key, Rust field | pull request | R9: 137 (excl. 137) | `core/src/inbox/rules.rs` (33), `core/src/pr/pulls.rs` (11), `core/src/plan/prs.rs` (11), `core/src/janitor.rs` (9), `core/src/plan/list.rs` (8) | unchanged | table written only by poll; applied migrations are history; `prs.pull_request` FK column name |
| `pr_posts` table | SQL table (+ local name) | mixed | R10: 12 (excl. 12) | `core/src/pr/posts.rs` (6), `core/src/pr/reactions.rs` (2), `core/src/inbox/pr.rs` (1), `core/src/plan/delete.rs` (1), `core/src/pr/pulls.rs` (1) | decision | migration + store part; row is keyed (plan, pr) and holds the Slack post of the pull request |
| `prs` table in SQL (`FROM\|JOIN\|UPDATE\|INTO\|DELETE FROM prs`) | SQL table | object | R11: 115 (excl. 115) | `core/src/pr/open.rs` (14), `core/src/pr/start.rs` (10), `core/src/pr/poll.rs` (8), `core/src/pr/add.rs` (6), `core/src/pr/cancel.rs` (6) | `workflows` | migration (new table name; applied migrations keep old), every query, CHECK/trigger names; store part owns schema |
| `PrKey`, `InvalidPrId` | Rust type (newtype id), error variant | object | R12: 154 (excl. 154) | `core/src/pr/open.rs` (20), `core/src/pr/poll.rs` (17), `core/src/pr/add.rs` (13), `core/src/pr/mod.rs` (13), `core/src/pr/rebase.rs` (11) | `WorkflowKey`, `InvalidWorkflowId` | ids.rs macro text "PR key" shown in errors; every signature |
| `PrRow`, `PrSpec`, `PlanPrs`, `Pr` | Rust struct (row of table `prs`; `Pr` is a local enum/struct in pr/add.rs and edit.rs) | object | R13: 167 (excl. 150) | `core/src/plan/render.rs` (16), `core/src/pr/open.rs` (13), `core/src/pr/add.rs` (13), `core/src/job/open.rs` (11), `core/src/pr/mod.rs` (11) | `WorkflowRow`, `WorkflowSpec`, `PlanWorkflows`, `Workflow` | fixture constructors in tests (`PrRow::fixture`) |
| `PrArg` | Rust struct for `--pr <key>` | object | R14: 15 (excl. 15) | `core/src/args.rs` (3), `core/src/job/add.rs` (2), `core/src/job/mod.rs` (2), `core/src/pr/post.rs` (2), `core/src/pr/reply.rs` (2) | `WorkflowArg` | clap flag `--pr` (flag rename = CLI contract) |
| `PrRef` | Rust struct: pr/add.rs = `<plan>/<key>` reference (object); pr/gh.rs = repo+number of a pull request | mixed | R15: 17 (excl. 17) | `core/src/pr/open.rs` (6), `core/src/pr/add.rs` (5), `core/src/pr/gh.rs` (3), `core/src/pr/mod.rs` (3) | decision | two different types share the name in pr/add.rs:101 and pr/gh.rs:111 |
| `PrTitle`, `PrTitleRequired` | Rust newtype/error variant: the title | mixed | R16: 20 (excl. 20) | `core/src/args.rs` (10), `core/src/pr/mod.rs` (3), `core/src/error.rs` (2), `core/src/merge/mod.rs` (2), `core/src/plan/mod.rs` (2) | decision | Workflow title is the pull request title; message "A PR title must be 1 to 72 characters" |
| `PrState`, `PullRequestState` | Rust enum: open/closed/merged of the pull request (pr/status.rs) | pull request | R17: 43 (excl. 43) | `core/src/pr/status.rs` (15), `core/src/pr/pulls.rs` (8), `core/src/pr/open.rs` (6), `core/src/pr/gh.rs` (6), `core/src/pr/poll.rs` (6) | unchanged | gh JSON mapping tests |
| `PullRequest`, `PolledPr`, `GhPr` | Rust struct: number, url, state, mergeable ... (pr/pulls.rs, pr/gh.rs, inbox/team.rs) | pull request | R18: 119 (excl. 51) | `core/src/pr/pulls.rs` (37), `core/src/inbox/team.rs` (26), `core/src/inbox/rules.rs` (18), `core/src/pr/gh.rs` (10), `core/src/pr/open.rs` (7) | unchanged | stored in `pull_requests`; mapping from gh JSON |
| `PrReady`, `PrFailing`, `PrChanges` | Rust enum variant of inbox `Kind` | pull request | R19: 50 (excl. 50) | `core/src/inbox/rules.rs` (32), `core/src/inbox/model.rs` (13), `core/src/inbox/pr.rs` (5) | unchanged | stored kind strings `pr-ready` etc. |
| error variants `Pr*` and `BasePr*`, `UntilPrNotIncident`, `NeedsOtherPr` | Rust `ErrorCode` variant (error.rs) | mixed | R20: 87 (excl. 83) | `core/src/error.rs` (38), `core/src/pr/start.rs` (7), `core/src/git/worktree.rs` (6), `core/src/pr/add.rs` (6), `core/src/args.rs` (5) | decision | object for most (see the code rows); `PrClosed`, `PrNotClosed`, `PrAdopted` are mixed; rename with the string |
| `enablePullRequestAutoMerge`, `addPullRequestReviewThreadReply` | GitHub GraphQL mutation names (external) | other | R21: 6 (excl. 6) | `core/src/pr/gh.rs` (5), `core/src/pr/auto_merge.rs` (1) | unchanged (GitHub's own) | external API: never rename |
| snake names containing `pull_request(s)` (functions, tests) | function / test name | pull request | R22: 168 (excl. 31) | `core/src/inbox/rules.rs` (43), `core/src/pr/gh.rs` (13), `core/src/pr/pulls.rs` (12), `core/src/plan/prs.rs` (11), `core/src/janitor.rs` (9) | unchanged | test names pin it |
| long snake sentences with `pr` (test names, > 30 chars) | test function name (sentence) | mixed | R23: 29 (excl. 18) | `core/src/inbox/rules.rs` (10), `core/src/job/open.rs` (4), `core/src/plan/render.rs` (4), `core/src/pr/open.rs` (2), `core/src/pr/pulls.rs` (2) | decision | test names; each sentence is object or pull request (decide per name when renaming) |
| `base_pr`, `base_prs`, `derived_base_pr`, `from_pr`, `to_pr`, `other_pr`, `of_pr`, `by_pr`, `titled_pr`, `docs_pr` | function / variable | object | R24: 109 (excl. 98) | `core/src/pr/add.rs` (34), `core/src/pr/start.rs` (23), `core/src/pr/open.rs` (12), `core/src/pr/poll.rs` (9), `core/src/plan/render.rs` (7) | `base_workflow`, ... | none beyond the code; `base_pr` is derived, never stored (comment pr/add.rs:192) |
| `head_pr`, `open_pr` | function (gh::head_pr = finds the pull request of a branch; inbox/rules.rs open_pr = test fixture GhPr) | pull request | R25: 33 (excl. 33) | `core/src/inbox/rules.rs` (28), `core/src/pr/open.rs` (4), `core/src/pr/gh.rs` (1) | unchanged | none |
| `until_pr`, `new_until_pr`, `stored_until_pr`, `parse_until_pr`, `end_if_until_pr_merged` | function / variable / flag (`--until-pr`) | object | R26: 37 (excl. 36) | `core/src/incident.rs` (34), `core/src/error.rs` (1), `core/src/pr/merge.rs` (1), `core/src/pr/poll.rs` (1) | `until_workflow` | CLI flag `--until-pr`, stored `incidents` value (store part); error code `until_pr_not_incident` |
| `load_prs`, `known_prs`, `all_prs`, `plan_prs`, `reviews_prs`, `close_prs`, `closed_prs`, `opened_prs`, `merged_prs`, `printed_prs`, `base_prs`, `models_of_pr`... | function / variable | object | R27: 80 (excl. 80) | `core/src/pr/add.rs` (9), `core/src/pr/poll.rs` (8), `core/src/prune/tests.rs` (7), `core/src/plan/cancel.rs` (6), `core/src/pr/open.rs` (6) | `load_workflows` ... | `merged_prs` in prune/tests.rs is a pull request fixture (prune part, gh list) |
| `pr_needs`, `pr_row`, `pr_fields`, `pr_values`, `pr_section`, `pr_blockers`, `pr_branch`, `pr_title`, `pr_log`, `pr_policy*`, `pr_poll_*`, `pr_lead_worktree_path`, `pr_open_job`, `pr_selection`, `pr_kinds`, `pr_tests`, `pr_of*`, `pr_base*`, `pr_repo`, `pr_keys` ... | function / variable / config key / test names | mixed | R28: 135 (excl. 120) | `core/src/pr/mod.rs` (14), `core/src/merge/mod.rs` (11), `core/src/pr/add.rs` (10), `core/src/inbox/rules.rs` (8), `core/src/plan/render.rs` (7) | decision | `pr_poll_seconds` and `pr_poll_lock` are about polling pull requests (pull request); `pr_policy*`, `pr_needs`, `pr_row`, `pr_open_job`, `pr_lead_worktree_path`, `pr_branch` are object; config key `watch.pr_poll_seconds` is in the user's config file (config/file.rs:332) |
| other `_pr` / `pr_` identifiers (functions, vars) | function / variable / test name | object | R29: 757 (excl. 62) | `core/src/pr/add.rs` (56), `core/src/inbox/rules.rs` (50), `core/src/pr/open.rs` (46), `core/src/incident.rs` (34), `core/src/pr/start.rs` (33) | `*_workflow*` | local; sampled 40: object except `find_pr`/`check_pr_live` (object), `close_pr_ready` (pull request, inbox item), `merged_via_pr` (pull request, prune), `merge_pr`/`create_pr`/`describe_pr`/`retitle_pr` (mixed: act on a Workflow's pull request) |
| `gh pr <verb>` | strings: argv/diagnostics of the GitHub CLI (`gh pr list/view/create/edit/merge/close`) | pull request | R30: 33 (excl. 33) | `core/src/pr/gh.rs` (15), `core/src/pr/open.rs` (5), `core/src/prune/mod.rs` (5), `core/src/plan/cancel.rs` (2), `core/src/prune/tests.rs` (2) | unchanged (gh's own noun) | gh CLI syntax; never rename |
| `cpd pr <verb>` | CLI noun in help/error/Event text (`cpd pr add/edit/start/open/rebase/push/poll/view/reply/describe`) | mixed | R31: 32 (excl. 32) | `core/src/pr/open.rs` (7), `core/src/pr/rebase.rs` (6), `core/src/pr/cancel.rs` (4), `core/src/attempt/ledger.rs` (2), `core/src/plan/lock.rs` (2) | decision | the noun covers Workflow verbs (add, edit, cancel, start, describe) and pull request verbs (poll, view, reply, post, merge); `pr open/rebase/push` act on both |
| `pr <verb>` command literal, Workflow verbs (`"pr add"`, `COMMAND = "pr edit"`, `"pr cancelled"`) | string const `COMMAND` (error `details.command`, `store.write` label) and reason text | object | R32: 49 (excl. 37) | `core/src/pr/start.rs` (9), `core/src/pr/cancel.rs` (8), `core/src/pr/add.rs` (6), `core/src/pr/mod.rs` (4), `core/src/args.rs` (3) | `workflow add` ... | `details.command` is JSON contract; reason `pr cancelled` is a stored jobs/events value (pr/cancel.rs:114) |
| `pr <verb>` command literal, pull request verbs (`"pr poll"`, `"pr view"`, `"pr merge"`, `"pr post"`, `"pr reply"`) | string const `COMMAND`, store.write label | pull request | R33: 56 (excl. 34) | `core/src/pr/gh.rs` (12), `core/src/pr/poll.rs` (5), `core/src/pr/mod.rs` (4), `core/src/pr/post.rs` (4), `core/src/pr/feedback.rs` (3) | unchanged | `details.command` is JSON contract |
| `pr <verb>` command literal, both (`"pr open"`, `"pr rebase"`, `"pr push"`) | string const `COMMAND`, retry hint text, Gate log label | mixed | R34: 69 (excl. 59) | `core/src/pr/open.rs` (21), `core/src/pr/rebase.rs` (17), `core/src/merge/mod.rs` (6), `core/src/pr/mod.rs` (6), `core/src/pr/push.rs` (4) | decision | `pr open` acts on a Workflow and opens its pull request; `pr rebase`/`pr push` act on the Lead Branch of a Workflow and push for its pull request |
| Inbox subject prefix `pr:<repo>#<n>` | string: stored `subject` of an Inbox item | pull request | R35: 19 (excl. 19) | `core/src/inbox/rules.rs` (14), `core/src/inbox/pr.rs` (2), `core/src/inbox/cmd.rs` (1), `core/src/inbox/model.rs` (1), `core/src/pr/mod.rs` (1) | unchanged | stored inbox.subject values; many test strings pin it (inbox/rules.rs) |
| policy key `pr.post.*`, `pr.jira_required`, `pr.protected` | policy-file key (TOML table `[pr]`) in error `details.key` and text | mixed | R36: 12 (excl. 12) | `core/src/pr/reactions.rs` (5), `core/src/pr/post.rs` (4), `core/src/pr/pulls.rs` (1), `core/src/pr/poll.rs` (1), `core/src/pr/view.rs` (1) | decision | users' policy files ($CPD_HOME/policy/*.toml) and the policy part; `pr.post.*` posts the pull request (pull request); `pr.jira_required` is a Workflow ticket rule |
| SQL alias `pr.<col>` on `pull_requests` | SQL alias | pull request | R37: 1 (excl. 1) | `core/src/pr/pulls.rs` (1) | unchanged | pr/pulls.rs:190 builds `pr.{c}` aliases |
| log file name `pr-<key>.<hook>.log` | file name under run/<plan>/logs | object | R38: 2 (excl. 2) | `core/src/pr/mod.rs` (2) | `workflow-<key>.<hook>.log` | existing logs on disk; docs |
| `until-pr` flag value / incident cause | CLI flag `--until-pr`, stored `cause` "until-pr" | object | R39: 4 (excl. 4) | `core/src/incident.rs` (4) | `until-workflow` | stored in the incident rows and `incident-ended` Event cause (events part) |
| Escalation ids `nl-pr-a1-q1` (history) | comment (a past Escalation id, history) | other | R40: 2 (excl. 0) | `core/src/pr/rebase.rs` (2) | unchanged (history) | history record |
| test job key `other-pr` | test data job key | object | R41: 1 (excl. 1) | `core/src/job/open.rs` (1) | `other-workflow` | test only |
| file path `pr/<file>.rs` in strings | string: path of a source file (env.rs list of commands that open the store) | other | R42: 1 (excl. 1) | `core/src/env.rs` (1) | follows the module rename | env.rs:96 table of paths |
| bot login `copilot-pull-request-reviewer` | string: external GitHub bot login | other | R43: 1 (excl. 1) | `core/src/config/file.rs` (1) | unchanged | external |
| `"pr"` string literal | JSON key (`.with("pr", ..)`, `p["pr"]`), SQL column (`"pr"` in pr_posts/jobs), clap long name | mixed | R44: 152 (excl. 152) | `core/src/pr/rebase.rs` (15), `core/src/pr/open.rs` (14), `core/src/pr/add.rs` (12), `core/src/pr/cancel.rs` (12), `core/src/pr/start.rs` (10) | decision | JSON detail keys are contract (docs/commands.md); `jobs.pr` and `pr_posts.pr` column; `p["pr"]["number"]` (plan/show.rs:132) is the pull request object nested under key `pr` |
| `"prs"` string literal | JSON key (`.with("prs", ..)`, lists of Workflow keys) | object | R45: 25 (excl. 25) | `core/src/plan/fingerprint.rs` (7), `core/src/context/pending.rs` (3), `core/src/plan/lock.rs` (3), `core/src/args.rs` (2), `core/src/plan/review.rs` (2) | `"workflows"` | JSON detail key in error details and plan show; pi adapter unverified |
| module path `pr::`, `mod pr`, `crate::pr` | Rust module (core/src/pr/) | mixed | R46: 35 (excl. 35) | `core/src/merge/mod.rs` (10), `core/src/driver/start.rs` (3), `core/src/inbox/pr.rs` (3), `core/src/attempt/submit.rs` (2), `core/src/driver/run.rs` (2) | decision | directory core/src/pr holds both: add/edit/cancel/start/row (Workflow) and gh/poll/pulls/merge/post/reply/view (pull request); `pub mod pr;` core/src/lib.rs:45 |
| `--pr` flag text | CLI flag in help/error text and `args.rs` | object | R47: 25 (excl. 25) | `core/src/pr/rebase.rs` (9), `core/src/args.rs` (4), `core/src/pr/push.rs` (2), `core/src/pr/feedback.rs` (2), `core/src/attempt/ledger.rs` (1) | `--workflow` | CLI contract (docs/commands.md §1.3), every Lead/Worker skill and spec |
| `.pr` field access / `pr:` field | Rust field / struct member (`job.pr`, `attempt.pr`, `PrArg.pr`) | object | R48: 349 (excl. 349) | `core/src/pr/poll.rs` (35), `core/src/driver/start.rs` (22), `core/src/inbox/team.rs` (20), `core/src/job/edit.rs` (20), `core/src/job/add.rs` (17) | `.workflow` | `jobs.pr` column mapping (row structs in store part) |
| `prs` bare (field/var/JSON/SQL) | Rust field / var (`plan.prs`), SQL alias, comment (`prs.status`) | object | R49: 434 (excl. 320) | `core/src/pr/gh.rs` (45), `core/src/plan/fingerprint.rs` (26), `core/src/pr/open.rs` (19), `core/src/plan/render.rs` (18), `core/src/pr/start.rs` (18) | `workflows` | `plan.prs` is the Plan row's field; `prs.shape`, `prs.head_ref` in comments name the table |
| `pr` bare (variable / param / SQL / comment) | Rust variable/param/field name, SQL column in queries, comment | object | R50: 1288 (excl. 719) | `core/src/pr/open.rs` (100), `core/src/pr/rebase.rs` (89), `core/src/job/open.rs` (70), `core/src/pr/poll.rs` (69), `core/src/pr/gh.rs` (64) | `workflow` | local; SQL column name `pr` in `jobs`, `pr_posts`, `attempts` queries needs migration |
| `pull request(s)` prose (lowercase) | comment / string (GitHub pull request) | pull request | R51: 230 (excl. 230) | `core/src/pr/open.rs` (39), `core/src/pr/gh.rs` (37), `core/src/job/open.rs` (18), `core/src/inbox/rules.rs` (11), `core/src/pr/poll.rs` (11) | unchanged | strings shown to Lead/Worker; pinned in tests of job/open.rs text |
| `Pull request #n` prose (capitalised) | string / comment | pull request | R52: 10 (excl. 10) | `core/src/pr/add.rs` (4), `core/src/attempt/ledger.rs` (1), `core/src/inbox/rules.rs` (1), `core/src/pr/mod.rs` (1), `core/src/pr/pulls.rs` (1) | unchanged | strings |
| `PRs` plural (prose/comment/string) | comment / string ("the Plan's PRs", "No PRs yet.") | object | R53: 80 (excl. 80) | `core/src/pr/add.rs` (9), `core/src/plan/render.rs` (8), `core/src/pr/start.rs` (8), `core/src/pr/cancel.rs` (6), `core/src/plan/display.rs` (5) | `Workflows` | pinned by render tests (plan/render.rs:405,410 "No PRs yet.", "## PRs") |
| `PR` followed by a placeholder or key (`PR {key}`, `PR {}`, `PR key`) | string (error / Event / render text) | object | R54: 171 (excl. 171) | `core/src/pr/cancel.rs` (18), `core/src/pr/add.rs` (15), `core/src/pr/open.rs` (15), `core/src/job/open.rs` (13), `core/src/pr/start.rs` (11) | `Workflow {key}` | texts pinned by tests; Event text in the engine log and pi |
| `PR` bare prose | comment and string ("the PR's base", "stacked PR") | object | R55: 754 (excl. 754) | `core/src/pr/add.rs` (70), `core/src/pr/open.rs` (70), `core/src/pr/start.rs` (60), `core/src/pr/mod.rs` (42), `core/src/pr/cancel.rs` (34) | `Workflow` | sampled 45 lines: 36 object, 3 pull request (e.g. prune/tests.rs:22), 4 mixed (curation.rs:323 "plan a docs PR"), 2 step; full list not classified (unverified) |
| `Pr` mixed-case bare | Rust identifier / comment | object | R56: 12 (excl. 0) | `core/src/pr/add.rs` (9), `core/src/pr/edit.rs` (2), `core/src/plan/prs.rs` (1) | `Workflow` | pr/add.rs `Pr` local enum |

Commands (run from the repo root after the `FILES=` line of Coverage):

```sh
rg -P -o -e '\bpr-feedback\b' $FILES | wc -l   # R1: 6
rg -P -o -e '\bpr-started\b' $FILES | wc -l   # R2: 3
rg -P -o -e '\bpr-(?:opened|merged)\b' $FILES | wc -l   # R3: 9
rg -P -o -e '\bpr-(?:ready|failing|changes)\b' $FILES | wc -l   # R4: 21
rg -P -o -e '\bpr-(?:\*|[a-z][a-z-]*)' $FILES | wc -l   # R5: 57
rg -P -o -e '\b(?:pr_(?:not_found|other_plan|required|exists|in_use|empty|locked|not_ready|ended|start_failed|not_open)|invalid_pr_id|needs_other_pr|until_pr_not_incident|base_pr_(?:unknown|unavailable|other_repo|not_open))\b' $FILES | wc -l   # R6: 45
rg -P -o -e '\bpr_(?:closed|not_closed|title_required|adopted)\b' $FILES | wc -l   # R7: 14
rg -P -o -e '\bpr_(?:status|number|url|opened_at)\b' $FILES | wc -l   # R8: 52
rg -P -o -e '\bpull_requests?\b' $FILES | wc -l   # R9: 137
rg -P -o -e '\bpr_posts\b' $FILES | wc -l   # R10: 12
rg -P -o -e '(?:(?<=FROM )|(?<=JOIN )|(?<=UPDATE )|(?<=INTO )|(?<=TABLE ))prs\b' $FILES | wc -l   # R11: 115
rg -P -o -e '\b(?:PrKey|InvalidPrId)\b' $FILES | wc -l   # R12: 154
rg -P -o -e '\b(?:PrRow|PrSpec|PlanPrs|Pr)\b' $FILES | wc -l   # R13: 167
rg -P -o -e '\bPrArg\b' $FILES | wc -l   # R14: 15
rg -P -o -e '\bPrRef\b' $FILES | wc -l   # R15: 17
rg -P -o -e '\bPrTitle(?:Required)?\b' $FILES | wc -l   # R16: 20
rg -P -o -e '\b(?:PrState|PullRequestState)\b' $FILES | wc -l   # R17: 43
rg -P -o -e '\b(?:PullRequest|PolledPr|GhPr)\b' $FILES | wc -l   # R18: 119
rg -P -o -e '\b(?:PrReady|PrFailing|PrChanges)\b' $FILES | wc -l   # R19: 50
rg -P -o -e '\b(?:PrNotFound|PrOtherPlan|PrRequired|PrExists|PrInUse|PrEmpty|PrLocked|PrNotReady|PrEnded|PrStartFailed|PrNotOpen|PrClosed|PrNotClosed|PrAdopted|PrStart|BasePr\w+|UntilPrNotIncident|NeedsOtherPr)\b' $FILES | wc -l   # R20: 87
rg -P -o -e '\b(?:enablePullRequestAutoMerge|addPullRequestReviewThreadReply)\b' $FILES | wc -l   # R21: 6
rg -P -o -e '\b\w*pull_?request\w*\b' $FILES | wc -l   # R22: 168
rg -P -o -e '\b(?=\w{31,})\w*(?:_prs?_|_prs?\b|\bprs?_)\w*' $FILES | wc -l   # R23: 29
rg -P -o -e '\b(?:base_prs?|derived_base_pr|from_pr|to_pr|other_pr|of_pr|by_pr|titled_pr|docs_pr|base_pr_\w+|check_base_pr_state|require_base_on_remote_pr)\b' $FILES | wc -l   # R24: 109
rg -P -o -e '\b(?:head_pr|open_pr)\b' $FILES | wc -l   # R25: 33
rg -P -o -e '\b\w*until_pr\w*\b' $FILES | wc -l   # R26: 37
rg -P -o -e '\b(?:load_prs|known_prs|all_prs|plan_prs|reviews_prs|close_prs|closed_prs|opened_prs|merged_prs|printed_prs|NO_PRS)\b' $FILES | wc -l   # R27: 80
rg -P -o -e '\b(?:pr_needs|pr_row|pr_fields|pr_values|pr_section|pr_blockers|pr_branch|pr_title|pr_log|pr_policy\w*|pr_lead_worktree_path|pr_open_job|pr_selection|pr_kinds|pr_tests|pr_of\w*|pr_base\w*|pr_repo|pr_keys|pr_has_live_linear|pr_poll_lock|pr_poll_seconds|pr_poll_transient_minutes|pr_section|pr_merged|pr_started|pr_opened|pr_edited|pr_described|pr_added|pr_cancelled\w*|pr_exists|pr_not_found|pr_feedback|pr_other_plan|pr_locked)\b' $FILES | wc -l   # R28: 135
rg -P -o -e '\b\w*(?:_prs?|pr_)\w*\b' $FILES | wc -l   # R29: 757
rg -P -o -e '(?<=gh )pr(?= [a-z]+)' $FILES | wc -l   # R30: 33
rg -P -o -e '(?<=cpd )pr(?= [a-z]+)' $FILES | wc -l   # R31: 32
rg -P -o -e '(?<![\w.-])pr(?= (?:add|edit|cancel|cancelled|start|describe)\b)' $FILES | wc -l   # R32: 49
rg -P -o -e '(?<![\w.-])pr(?= (?:poll|view|merge|post|reply)\b)' $FILES | wc -l   # R33: 56
rg -P -o -e '(?<![\w.-])pr(?= (?:open|rebase|push)\b)' $FILES | wc -l   # R34: 69
rg -P -o -e '(?<![\w.:-])pr(?=:[^ :])' $FILES | wc -l   # R35: 19
rg -P -o -e '(?<=["`\s(])pr(?=\.(?:post|jira|protected|hooks)\b)' $FILES | wc -l   # R36: 12
rg -P -o -e '\bpr(?=\.\{)' $FILES | wc -l   # R37: 1
rg -P -o -e '\bpr(?=-(?:\{|<key>))' $FILES | wc -l   # R38: 2
rg -P -o -e '(?<=until-)pr\b' $FILES | wc -l   # R39: 4
rg -P -o -e '(?<=nl-)pr(?=-a1)' $FILES | wc -l   # R40: 2
rg -P -o -e '(?<=other-)pr\b' $FILES | wc -l   # R41: 1
rg -P -o -e '\bpr(?=/[a-z_]+\.rs)' $FILES | wc -l   # R42: 1
rg -P -o -e 'pull-request' $FILES | wc -l   # R43: 1
rg -P -o -e '"pr"' $FILES | wc -l   # R44: 152
rg -P -o -e '"prs"' $FILES | wc -l   # R45: 25
rg -P -o -e '(?:\bcrate::pr\b|\bsuper::pr\b|\bmod pr\b|\bpub mod pr\b|\bpr::|\buse pr\b)' $FILES | wc -l   # R46: 35
rg -P -o -e '(?<![\w-])--pr\b(?![-\w])' $FILES | wc -l   # R47: 25
rg -P -o -e '(?:\.pr\b(?!\w)|\bpr: )' $FILES | wc -l   # R48: 349
rg -P -o -e '(?<![\w"])prs\b(?![\w"])' $FILES | wc -l   # R49: 434
rg -P -o -e '(?<![\w"-])pr(?![\w"-])' $FILES | wc -l   # R50: 1288
rg -P -o -e '(?<![\w`])pull requests?\b' $FILES | wc -l   # R51: 230
rg -P -o -e '\bPull requests?\b' $FILES | wc -l   # R52: 10
rg -P -o -e '\bPRs\b' $FILES | wc -l   # R53: 80
rg -P -o -e '\bPR (?=\{|key\b|keys\b)' $FILES | wc -l   # R54: 171
rg -P -o -e '\bPR\b(?! (?:\{|keys?\b))' $FILES | wc -l   # R55: 754
rg -P -o -e '\bPr\b' $FILES | wc -l   # R56: 12
```

Notes on the families:

- `PrRef` names two different types (`core/src/pr/add.rs:101` is the `<plan>/<key>` reference of the object; `core/src/pr/gh.rs:111` is a repo and number of a pull request): proposal, rename the first `WorkflowRef`, keep the second.
- Proposal for `mixed` module `core/src/pr`: split it, add/edit/cancel/start/row/describe go to `workflow`, gh/poll/pulls/merge/post/reply/view/review/comments/deploys stay `pr`; `pr open`/`pr rebase`/`pr push` need the User's decision (they act on a Workflow and push for its pull request). The User decides.
- The bare families (`pr`, `prs`, `PR`, `PRs`) are the long tail: sampled lines (45 of the `PR` family, 40 of the `pr_` family) were object in about 80% and the rest pull request or mixed; the lines I did not sample are classified by the pattern only (unverified).
- Phase and step senses: none found. `core/src/plan/phase.rs` has five phases (grounding, plan, dispatch, deploy, done) and no `pr` phase; no string says "PR step" (checked with `rg -n -i 'PR (step|stage|phase)'`).

## User-facing strings

Shown to is inferred from the code around the string (who receives the error, the Event or the file): unverified for rows not obviously for one reader. A row stands for every string with the same text once `{...}` placeholders are ignored; extra places are in Where. `help` rows are clap help (doc comments); the others are string literals before the file's inline test block. Quotes are the first words of the literal, raw.

| Where (path:line) | Text | Shown to | Sense | Fix |
|---|---|---|---|---|
| `core/src/args.rs:40` | `The PR [default: the Plan's one PR that is not cancelled]` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/args.rs:54` (+`core/src/job/edit.rs:173`, `core/src/pr/mod.rs:347`, 1 more) | `No PR {key} in Plan {plan}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/args.rs:64` | `Plan {plan} has no PR: add one with cpd pr add, then pass --pr <key>.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/args.rs:72` | `Plan {plan} has {} PRs ({}): pass --pr <key>.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/args.rs:231` | `A PR title must be 1 to {} characters on one line, with no control` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/attempt/kind.rs:60` | `The repo's policy has no [test-commands], so the tests of this Job cannot be checked.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/attempt/kind.rs:98` | `The impl Job {} needs no test Job of its PR, so it names no` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/attempt/ledger.rs:373` | ` Pull request #{} of PR {} is open: {}.` | Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/attempt/ledger.rs:385` | ` PR {key} still has {open} open review comments: cpd pr view --pr {key}, then` | Worker | object | PR/PRs → Workflow(s) |
| `core/src/context/curation.rs:323` | `Documentation candidates of Context {context}, sent by the User: plan a docs PR that documents` | Lead | mixed | decision: say Workflow or pull request per clause |
| `core/src/context/pending.rs:161` | `\n### PR {} ({}, base {}): {}\n\n` | Manager, Lead | object | PR/PRs → Workflow(s) |
| `core/src/context/pending.rs:172` | `no pull request\n\n` | Manager, Lead | pull request | unchanged |
| `core/src/driver/run.rs:316` (+`core/src/merge/mod.rs:90`, `core/src/pr/mod.rs:300`, 1 more) | `Job {} has the PR key {:?}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/run.rs:330` | `Reading the policy of the PR failed: {}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/start.rs:83` | `The workflow worktree on the PR's Lead Branch.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/start.rs:254` | `Job {key} has the PR key {:?}: {e}` | Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/start.rs:312` | `Attempt {attempt} of Job {key} cannot start: the workflow worktree of PR {} is held` | Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/start.rs:329` | `Attempt {attempt} of Job {key} cannot be linear: another Attempt of PR {} is live.` | Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/start.rs:345` | `PR {} of Plan {plan} has no workflow worktree.` | Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/start.rs:374` | `its PR {} is {pr_status}` | Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/start.rs:376` | `its PR {} is {pr_status}, and an open Job runs only while its PR is` | Worker | object | PR/PRs → Workflow(s) |
| `core/src/driver/start.rs:382` | `no other Job of its PR {} is merged: there is nothing to open` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/git/worktree.rs:291` | ``An adopted PR's Lead Branch: cut from `origin/<head_ref>`, which is the pull request's own branch;`` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/git/worktree.rs:386` | `The base PR's Lead Branch {base} is neither a local branch of this repo nor` | Worker | object | PR/PRs → Workflow(s) |
| `core/src/git/worktree.rs:413` | ``A stacked PR's start point: its base PR's Lead Branch, local or on `origin` (§7.2).`` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/git/worktree.rs:703` | ```git rev-list HEAD --not --remotes=origin` is not empty, and `HEAD` is not the pull request's`` | Lead, Worker | pull request | unchanged |
| `core/src/home.rs:174` | `pr-poll.lock` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/ids.rs:141` (+`core/src/inbox/pr.rs:163`, `core/src/inbox/rules.rs:457`, 1 more) | `PR key` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/inbox/cmd.rs:423` | `Please start a review of the pull request {url} in your Context {context}. It was` | Lead | pull request | unchanged |
| `core/src/inbox/model.rs:61` | `pr-failing` | Lead, Worker | pull request | unchanged |
| `core/src/inbox/model.rs:62` | `pr-changes` | Lead, Worker | pull request | unchanged |
| `core/src/inbox/model.rs:63` | `pr-ready` | Lead, Worker | pull request | unchanged |
| `core/src/inbox/rules.rs:533` | `GitHub could not read any pull request posted in the team channel.` | Lead, Worker | pull request | unchanged |
| `core/src/inbox/team.rs:128` | `The User's own pull request.` | Lead, Worker | pull request | unchanged |
| `core/src/inbox/team.rs:252` | `gh api graphql answered a pull request state {other:?}.` | Lead, Worker | pull request | unchanged |
| `core/src/inbox/team.rs:263` | `gh api graphql answered a pull request without isDraft.` | Lead, Worker | pull request | unchanged |
| `core/src/incident.rs:39` | ``The `<plan>/<key>` PR that ends the incident when it merges`` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/incident.rs:215` | `No PR {until_plan}/{key}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/janitor.rs:140` | `The clean-up of the {status} PR {pr} of Plan {plan} kept {path} untouched: {reason}` | log | object | PR/PRs → Workflow(s) |
| `core/src/janitor.rs:143` | `The clean-up of the {status} PR {pr} of Plan {plan} removed {path}, but not everything:` | log | object | PR/PRs → Workflow(s) |
| `core/src/janitor.rs:146` | `The clean-up of the {status} PR {pr} of Plan {plan} did not remove {path}: {reason}` | log | object | PR/PRs → Workflow(s) |
| `core/src/job/add.rs:159` (+`core/src/job/add.rs:213`) | `check_pr_live found the PR` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/add.rs:251` | `Job {job} takes the resource {missing}, which the policy of PR {pr} does not declare` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/add.rs:293` | `A review Job needs every Job of PR {pr}, and {} failed: retry or cancel` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/chain.rs:41` | `A Job added to PR {pr} chains after {}, which failed: retry or cancel it` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/edit.rs:48` | `Nothing to edit: pass --title, --tier, --spec, --pr, --kind, --needs or --no-needs.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/edit.rs:122` | `only its Spec (--spec) can change; title, Tier, PR, kind and Dependencies are Locked` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/edit.rs:216` | `Job {key} cannot change PR: {} need{} it.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:93` | `writes run/<plan>/reviews/<key>.md; it needs every Job of its PR so far` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:134` | `Move the Job to this PR (it keeps no old Dependency)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:292` | `The kind {kind} is cpd's: it adds the open Job when a PR starts and` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:324` | `PR {pr} has opened: a feedback fix is one work Job, not {kind}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:338` | `PR {pr} has the shape {}: it allows the kinds {}, not {kind}. The shape` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:355` | `The impl Job {} must need a test Job of its PR {}: its tests` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:370` | `The verify Job {} must need an impl Job of its PR {}: it checks` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:424` | `A Dependency can only link Jobs of the same PR: {} {} not in PR` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/mod.rs:438` | `PR {pr} of Plan {plan} is {}: Jobs are not added to or moved into` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/open.rs:20` (+`core/src/pr/add.rs:29`) | `pr add` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/open.rs:34` | `PR {} has no free {prefix} Job key` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/job/open.rs:47` | `Make the Lead Branch {lead} of PR {key} fit its base {base}, so that cpd` | Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/job/open.rs:64` (+`core/src/job/open.rs:116`) | `Nothing was pushed by the Worker and no pull request was created or edited by` | Lead, log | pull request | unchanged |
| `core/src/job/open.rs:67` | ``Pushing, `gh`, running the Gate or opening the pull request yourself: cpd does them`` | Worker | pull request | unchanged |
| `core/src/job/open.rs:78` | `Make the Lead Branch {lead} of PR {key} fit its base {base}, so that cpd` | Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/job/open.rs:84` | `The pull request of PR {key} exists already and is adopted: it is updated, never` | Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/job/open.rs:99` | `Bring the pull request of PR {key} up to date with its base {base}, so` | Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/job/open.rs:119` | ``Pushing, `gh`, running the Gate or editing the pull request yourself: cpd does them`` | Worker | pull request | unchanged |
| `core/src/job/open.rs:135` | `The poll saw the pull request conflict with its base.` | Lead, Worker | pull request | unchanged |
| `core/src/job/open.rs:175` | `Bring the pull request up to date: {}` | Worker | pull request | unchanged |
| `core/src/job/spec.rs:86` | `the Spec names {primary}, the primary checkout of the repo of PR {key}; a Worker` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/merge/mod.rs:324` | `{} kept moving during the merge (other merges, a pr rebase, or someone outside cpd);` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/merge/mod.rs:448` | `Plan {} has no PR {}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/merge/mod.rs:458` | `PR {} of Plan {} has no title: run cpd pr edit {} --title \"<text>\"` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/merge/recover.rs:149` | `Attempt {} is merged, but pushing {} to origin failed: {}. The next merge or` | Lead, log | mixed | decision: say Workflow or pull request per clause |
| `core/src/plan/delete.rs:340` | `PR {key} of Plan {plan} is {status}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/lock.rs:75` | `Plan {id} has no PR: add one with cpd pr add.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/lock.rs:77` | `Plan {id} has PRs without a draft Job: {}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/lock.rs:81` | `cpd pr add <key> --repo <path> --plan {id}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/mod.rs:82` | `Cancel the Plan's Jobs and pull requests, close its Lead, archive it (§4.12)` | Lead, Worker | pull request | unchanged |
| `core/src/plan/outline.rs:103` | `expected an object of PR key to lines` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/outline.rs:176` | `The outline's done_when names {key}, which is not a PR of Plan {} that is` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/prs.rs:65` | `PR {} has no started_at` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/prs.rs:66` | `PR {} has the started_at {started:?}: {e}` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/plan/prs.rs:123` | `Plan {plan} has a PR key {key:?}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/render.rs:23` | `Added by cpd when the PR starts: merges the base only on a conflict, runs` | Lead, User | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/plan/render.rs:25` | `every other Job of the PR` | Lead, User | object | PR/PRs → Workflow(s) |
| `core/src/plan/render.rs:40` (+`core/src/pr/add.rs:62`) | `Open the pull request: {}` | Lead, User | pull request | unchanged |
| `core/src/plan/render.rs:192` (+`core/src/plan/summary.rs:155`) | `PR {key}` | Lead, User | object | PR/PRs → Workflow(s) |
| `core/src/plan/render.rs:233` | `## PR {}` | Lead, User | object | PR/PRs → Workflow(s) |
| `core/src/plan/render.rs:365` | `\| Job \| Other Plan \| PR \| Its Jobs (status) \| Paths \|` | Lead, User | object | PR/PRs → Workflow(s) |
| `core/src/plan/render.rs:405` | `No PRs yet.` | Lead, User | object | PR/PRs → Workflow(s) |
| `core/src/plan/render.rs:410` (+`core/src/plan/summary.rs:119`) | `## PRs` | Lead, User | object | PR/PRs → Workflow(s) |
| `core/src/plan/review.rs:302` | `The outline of Plan {id} has no done_when for the PRs {}: add them with` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/shape.rs:33` | `PR {} has the shape tdd, which keeps a test Job and an impl Job` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/shape.rs:60` | `The impl Jobs {} must each need a test Job of their PR: their tests` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/shape.rs:81` | `The verify Jobs {} must each need an impl Job of their PR: they check` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/show.rs:79` (+`core/src/plan/show.rs:124`) | `a PR key` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/show.rs:120` | `a PR {field}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/plan/show.rs:132` | `a pull request number` | Lead, Worker | pull request | unchanged |
| `core/src/pr/add.rs:143` | ``A base PR named as `<plan>/<key>`: its Lead Branch becomes the base.`` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:222` | `PR {plan}/{key} is a PR of Plan {plan}, not of {}: needs and base PRs` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:252` (+`core/src/pr/add.rs:683`) | `Plan {plan} has no PR {base_key} to stack on.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:260` | `PR {} is in {}, not in {}: a PR stacks only on a PR` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:281` | `Plan {plan} has no PR {} to need.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:287` | `A PR cannot need the cancelled PR {}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:294` | `{key} is the base PR and a need: the base PR is already the implied` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:316` | `PR {} is {}: there is nothing to stack on, name its base branch instead.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:433` | `PR {key} added to Plan {plan} (repo {}, base {base}{with}), waiting. Next: cpd plan show` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:449` | `Job {} added to Plan {plan} as draft (the {} template of PR {key}). Next:` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:538` | `Plan {plan} already has a PR {} with other flags.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:561` | `Plan {plan} is in dispatch: say --needs <pr> to link PR {} to the PR` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:586` | `Pull request #{number} is {}: only an open pull request is adopted.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/add.rs:592` | `Pull request #{number} comes from a fork: cpd cannot push to a fork.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/add.rs:602` | `Pull request #{number} has the head branch {}: it is a cpd pull request.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/add.rs:616` | `The head branch {} is already adopted by PR {pr} of Plan {plan}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:627` | `Pull request #{number} is {}'s, not the User's ({}). Ask the User first; once they` | Lead, Worker | pull request | unchanged |
| `core/src/pr/add.rs:645` | `{repo} has no [test-commands] in its policy file {}, and a tdd PR needs it` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:667` | `Plan {plan} already has the Job {}, which the {} template of PR {} would` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:738` | `PR {} of Plan {plan} vanished while its template was being written` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/add.rs:791` | `Plan {plan} is back in dispatch: PR {key} was added.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/auto_merge.rs:15` | `GitHub accepted the request, or the pull request already had one.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/cancel.rs:25` | `pr cancel` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:65` | `PR {key} changed while it was cancelled: rerun the command.` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:75` | `PR {key} of Plan {plan} was cancelled. Next: cpd plan show --plan {plan}` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:122` | `Job {job} of Plan {plan} was cancelled: {reason}: PR {key} was cancelled. Next: cpd plan` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:142` | `Plan {plan} left phase dispatch while PR {key} was cancelled: rerun the command.` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:149` | `Every PR of Plan {plan} that is not cancelled is open or merged. The engine` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:172` | `Plan {plan} left phase deploy while PR {key} was cancelled: rerun the command.` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:176` (+`core/src/pr/open.rs:383`, `core/src/pr/poll.rs:471`) | `Every PR of Plan {plan} is merged: the Plan is done. See cpd plan show` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:208` | `PR {key} is {status}: --superseded-by cancels only a dispatch PR, after the lock.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:220` | `only a waiting PR is cancelled` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:221` | `an open PR is cancelled only in phase dispatch or deploy` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/cancel.rs:222` | `only a waiting, dispatch or open PR is cancelled` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:224` | `PR {key} is {status}: {only}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:251` | `--superseded-by names PR {key} itself.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:263` | `No PR {by_key} in Plan {by_plan} to be superseded by.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:268` | `PR {by_plan}/{by_key} is cancelled: its work cannot supersede PR {key}'s.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/cancel.rs:291` | `PR {key} of Plan {plan} is open without a pull request or a Lead worktree` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/cancel.rs:297` | `PR {key}'s pull request #{} is {state} as the last poll stored it: only a` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/cancel.rs:308` | `PR {key} is in use: PRs that need it, or are stacked on it, would` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/comments.rs:204` | `found no pull request` | Lead, Worker | pull request | unchanged |
| `core/src/pr/describe.rs:17` | `pr describe` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/describe.rs:54` | `PR {key} of Plan {plan} described. Next: cpd plan render --plan {plan}` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/describe.rs:84` | `PR {key} is an adopted pull request: its own body is kept, so it is` | Lead, log | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/describe.rs:89` | `PR {key} of Plan {plan} is {}: its description is history.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/edit.rs:25` | `pr edit` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/edit.rs:72` | `PR {key} is an adopted pull request: its repo and base come from the pull` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/edit.rs:101` | `PR {key} of Plan {plan} edited ({}). Next: cpd plan render --plan {plan}` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/edit.rs:138` | `PR {key} of Plan {plan} is {}: its title and ticket are history.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/edit.rs:149` | `PR {key} is {}: only a waiting PR is edited.` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/edit.rs:185` | `PR {key} keeps its repo: {} {} stacked on it.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/feedback.rs:44` | `PR {key} (#{number}) of Plan {plan} needs attention: {}.{counts} Read the details with cpd pr` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/gh.rs:217` | `gh printed the createdAt {:?} of pull request #{}: {e}` | Lead, Worker | pull request | unchanged |
| `core/src/pr/gh.rs:293` | `gh pr create printed no pull request URL.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/gh.rs:842` | `GitHub has no pull request {repo}#{number}.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/merge.rs:60` | `The pull request, as the Deploy column shows it` | Lead, Worker | pull request | unchanged |
| `core/src/pr/merge.rs:77` | `{repo}#{number} is {} and {}: only an OPEN pull request that is CLEAN is merged.` | Lead, log | pull request | unchanged |
| `core/src/pr/merge.rs:103` | `pr merge` | Lead, Worker | pull request | unchanged |
| `core/src/pr/mod.rs:81` | `Add a PR to the Plan (§9.4)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:83` | `Change a waiting PR before the lock (§9.5)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:85` | `Store the PR's description, which becomes the pull request body (§9.5a)` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/mod.rs:87` | `Remove a PR from the Plan before the lock; after it, end a PR with` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:89` | `Start a waiting PR whose needs are merged, after a failed start (§9.7)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:91` | `Push the Lead Branch and open the PR, refused only when it conflicts with its` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:93` | `Poll every open pull request of the User (§9.2)` | Lead, Worker | pull request | unchanged |
| `core/src/pr/mod.rs:99` | `Show the PR's checks, reviews and comments (§9.8)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:101` | `Reply to an open feedback item of the PR's pull request as the User, with` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/mod.rs:103` | `Post the PR's pull request to Slack for review (§9.8b)` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/mod.rs:105` | `Squash-merge a clean pull request on GitHub and delete its branch (§9.10)` | Lead, Worker | pull request | unchanged |
| `core/src/pr/mod.rs:137` | `The PR key, chosen by the Lead` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:140` | `The PR's repo: a path inside it` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:143` | `The PR's own title: names its change; 1-72 characters on one line` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:146` | `The PR's own Jira ticket key, e.g. ABC-123 (optional)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:149` | `The branch the pull request targets [default: origin's HEAD branch, else main], or <plan>/<key> to` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/mod.rs:152` | `PRs that must be merged before this one starts, comma-separated: a key of the Plan,` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:155` | `No needs, even on a reopened Plan (default there: every merged PR of the Plan)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:158` | `The template of stub drafts the PR starts with, and the kinds its Jobs may` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:161` | `Adopt the existing pull request <number> of the repo: its head branch is pushed to,` | Lead, Worker | pull request | unchanged |
| `core/src/pr/mod.rs:164` | `With --adopt: the login of the pull request's author when the User is not the` | Lead, Worker | pull request | unchanged |
| `core/src/pr/mod.rs:173` (+`core/src/pr/mod.rs:200`, `core/src/pr/mod.rs:212`, 1 more) | `The PR key` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:179` | `The new base branch, or <plan>/<key> to stack on that PR` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:188` | `The new title, in any phase while the PR is not merged or cancelled; 1-72` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:191` | `The PR's Jira ticket key, e.g. ABC-123: sets or replaces it, in any phase while` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:215` | `Cancel a dispatch PR whose merged work another PR carried: that PR, as <plan>/<key> (a` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/mod.rs:327` | `Plan {plan} is in phase {}: {command} needs phase grounding or plan, and after the` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:27` | `pr open` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/open.rs:44` | `Plan {plan} is held by incident mode: an agent cannot open, push or rebase PR` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/open.rs:76` | `PR {key} of Plan {id} vanished while it was being queued` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:98` | `PR {}'s base {}, the Lead Branch of {base_pr}, is not on origin: the pull` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/open.rs:123` | `Its Lead pushes it (cpd pr push): ask the Lead (cpd ask), then run {again}` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/open.rs:149` | `{} of PR {} conflicts with {remote} ({}) in {}: merge it into {into} and` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:230` | `The pull request {} of PR {key}'s Lead Branch {branch} is closed without a merge:` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/open.rs:246` (+`core/src/pr/open.rs:278`) | `gh printed the pull request URL {:?}, which is not https://github.com/<owner>/<name>/pull/<n>.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/open.rs:252` | `gh cannot find pull request {repo}#{}, which it just opened.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/open.rs:285` | `gh lists pull request {repo}#{} as merged but reads it {}.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/open.rs:288` | `gh cannot find pull request {repo}#{}, which it lists.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/open.rs:343` (+`core/src/pr/poll.rs:453`) | `PR {key} (#{number}) of Plan {id} is merged.` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:346` | `PR {key} of Plan {id} opened as #{number}: {url}. The engine polls it; to follow` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:362` | `Every PR of Plan {id} is open or merged. The engine polls them; to follow` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:364` | `Every PR of Plan {id} is open. The engine polls them; to follow them by` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:465` | `PR {key} of Plan {id} left status dispatch meanwhile: rerun the command.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:481` | `the selected PR is still in the Plan` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:485` | ``The PR is `dispatch`: go on, with its key.`` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:487` | ``The PR is `open` or `merged`: the success to print.`` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:533` | `PR {key} of Plan {plan} has its open Job {job} draft, queued or running: it` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/open.rs:558` | `{} Jobs of PR {key} of Plan {} are not Merged or cancelled yet: see` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:565` | `No Job of PR {key} of Plan {} is Merged: there is nothing to open` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/open.rs:581` | `PR {key} of Plan {plan} has no title: run cpd pr edit {key} --title \"<text>\"` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/open.rs:611` | `PR {key} of Plan {plan} has no description: write it (the repo's pull request template,` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/open.rs:635` | `PR {key} of Plan {plan} has no Jira ticket: its owner's policy sets pr.jira_required; run` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/poll.rs:35` | `pr poll` | Lead, Worker | pull request | unchanged |
| `core/src/pr/poll.rs:233` | `Another pr poll held {} for longer than {}s.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/poll.rs:337` | `its base PR {base} merged and GitHub repointed the pull request` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/poll.rs:392` | `The policy of PR {pr} (#{number}) of Plan {plan} did not resolve: {}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/poll.rs:395` | `cpd policy show --pr {pr} --plan {plan}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/poll.rs:397` | `{message} The poll sends this PR no pr-feedback until it does; every other PR is` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/poll.rs:523` | `PR {key} of Plan {id} changed while it was being polled: rerun the command.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/post.rs:50` | `PR {key} of Plan {id} is {}: pr post needs an open PR.` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/post.rs:63` | `PR {key}: posting is not enabled for its owner.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/post.rs:68` (+`core/src/pr/reply.rs:60`) | `an open PR has its pull_request row (prs CHECK)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/post.rs:71` | `Pull request {} of PR {key} is not the User's own.` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/post.rs:90` | `PR {key} is not ready to post: {}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/post.rs:125` (+`core/src/pr/post.rs:167`) | `pr post` | Lead, Worker | pull request | unchanged |
| `core/src/pr/post.rs:132` | `PR {key} has no post after its insert was refused` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/post.rs:156` | `the PR is stacked on another PR's branch: the request waits until it is unstacked` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/post.rs:188` | `PR {key} was already posted: {}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/pulls.rs:133` | `pull request {} has {what} {value:?}` | Lead, Worker | pull request | unchanged |
| `core/src/pr/pulls.rs:174` | `no pull request {id}` | Lead, Worker | pull request | unchanged |
| `core/src/pr/pulls.rs:227` | `a PR has {what} {value:?}` | Lead, Worker | pull request | unchanged |
| `core/src/pr/push.rs:17` | `pr push` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/push.rs:39` | `pr push --pr {key}` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/push.rs:56` | `The remote {remote_branch} does not contain the gated commit, so it moved: nothing was pushed.` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/reactions.rs:146` | `The merge reaction of PR {} of Plan {} on Slack message {} of {}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/rebase.rs:32` | `pr rebase` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/rebase.rs:112` | `cpd: pr rebase {id}/{key}` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/rebase.rs:132` | `{branch} kept moving during the rebase (merges into PR {key}); the compare-and-swap lost {} times:` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/rebase.rs:147` (+`core/src/pr/rebase.rs:232`, `core/src/pr/rebase.rs:299`) | `pr rebase --pr {key}` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/rebase.rs:173` | `Job {} of PR {key} has a running Attempt: steer that Worker to do the` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/rebase.rs:340` | `{} was pushed, but the merges after it ({}) were rejected: run cpd pr push` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/rebase.rs:386` | `{} was rebased onto origin/{}, but its push was rejected: the remote head moved again,` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/rebase.rs:455` | `cpd pr rebase --pr {key} {flag}` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/rebase.rs:483` | ``A rebase of PR {key} is already in progress in its Lead worktree: run `{}``` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/rebase.rs:492` | `No rebase of PR {key} is in progress in its Lead worktree.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/rebase.rs:501` | ``The Lead Branch of PR {key} moved during the stop (a merge landed): run `{}`,`` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/rebase.rs:610` | `Attempt {attempt} of PR {key} is on the Lead Branch but its merge has not` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/rebase.rs:627` | `PR {key} of Plan {plan} was {what}, but its Lead worktree {} still holds the` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/reply.rs:21` | ``The id of the open item to answer (`cpd pr view`, `feedback`)`` | Lead, Worker | pull request | unchanged |
| `core/src/pr/reply.rs:54` | `PR {} of Plan {id} is {}: pr reply needs an open PR.` | Lead, Worker | mixed | decision: say Workflow or pull request per clause |
| `core/src/pr/reply.rs:67` | `PR {} has no open feedback item {:?}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/review.rs:27` | `The pull request: its URL, or owner/repo#n` | Lead, Worker | pull request | unchanged |
| `core/src/pr/review.rs:53` | `--pr {text:?} is not https://github.com/<owner>/<repo>/pull/<n> or <owner>/<repo>#<n>.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/review.rs:103` | `gh answered the review without an id or html_url: the review may have been posted;` | Lead, Worker | pull request | unchanged |
| `core/src/pr/row.rs:18` | `PR {} of Plan {plan} has no Lead worktree` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/row.rs:30` | `PR {} of Plan {plan} is {}: {command} needs status {needs}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:27` | `pr start` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:157` (+`core/src/pr/start.rs:211`) | `The base PRs of {repo} form a cycle at {from}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:164` | `PR {key} of Plan {plan} changed while it was unstacked: rerun the command.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:191` | `PR {key} of Plan {plan} vanished while it was unstacked` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:291` | `The database, or the PR changed meanwhile.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:353` | `PR {} of Plan {plan} changed while it was started: rerun the command.` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:358` | `PR {} of Plan {plan} started: Lead Branch {} in {worktree}. Its Jobs are scheduled.` | Lead, log | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:365` | `The Lead Branch {} of PR {} of Plan {plan} was not pushed to origin:` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:413` | `PR {key} of Plan {plan} could not be started ({}): {}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:414` | `{message} The engine does not try again. Fix the cause, then: cpd pr start {key}` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:438` | `PR {key} of Plan {plan} is cancelled: it cannot be started.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/start.rs:459` | `PR {key} of Plan {plan} {}.` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/pr/status.rs:108` | `gh printed the unknown pull request state {other:?}.` | Lead, Worker | pull request | unchanged |
| `core/src/pr/view.rs:16` | `pr view` | Lead, Worker | pull request | unchanged |
| `core/src/pr/view.rs:30` | `an open or merged PR has its pull request (prs CHECK)` | Lead, Worker | object | PR/PRs → Workflow(s) (keep "pull request" words) |
| `core/src/pr/view.rs:58` | `an open or merged PR has its pull_request row (prs CHECK)` | Lead, Worker | object | PR/PRs → Workflow(s) |
| `core/src/prune/mod.rs:169` | `cannot read the output of gh pr list: {e}` | Lead, Worker | pull request | unchanged |
| `core/src/prune/mod.rs:170` | `cannot read the output of gh pr list: not a list` | Lead, Worker | pull request | unchanged |
| `core/src/prune/mod.rs:182` | `cannot read the output of gh pr list: a PR lacks a field` | Lead, Worker | pull request | unchanged |
| `core/src/resource/mod.rs:314` | `Job {key} of Plan {plan} lists resources but its PR {pr} has no repo` | Lead, Worker | object | PR/PRs → Workflow(s) |

## Wrong or stale

Everything with sense `object` in the two tables above is stale by decision pro-d136/pro-d138 (it must say Workflow): that is not repeated here. Below are the items that are wrong today or collide with the new meaning.

| path:line | what is wrong | why |
|---|---|---|
| `core/src/plan/mod.rs:82` | Help says "Cancel the Plan's Jobs and pull requests" | It cancels the cpd objects (`prs` rows, `plan cancel`), not GitHub pull requests; after the rename it must say Workflows (sense `object` written as `pull request`). |
| `core/src/pr/mod.rs:1` | Module doc starts "Pull requests (contract §7.6; commands.md §9), through `gh`" | The module also holds the Workflow commands add, edit, cancel, start, describe, which do not use `gh`. |
| `core/src/driver/start.rs:312`, `:345` | "the workflow worktree of PR {} ..." and "has no workflow worktree" (also `core/src/driver/run.rs:322`, `:338`, `:818`, `:825`, `:828`, `:832`, `:850`) | "workflow worktree" is the old name of the Lead worktree of a PR; after the rename Workflow is the object and this phrase would read "the Workflow's workflow worktree". Same word in comments `core/src/driver/start.rs:78`, `:83`, `:307`, `:308`, `:325`, `core/src/driver/run.rs:319`, `:742`, `:813`, `:838`, `:839`, `core/src/driver/mod.rs:68`, `core/src/git/worktree.rs:28`, `core/src/merge/recover.rs:163`. |
| `core/src/pr/deploys.rs:1`, `:2`, `:71`, `:181` | "the workflow runs that wait for the User's approval", "workflow" as the name of a GitHub Actions workflow (fields `workflow`, `workflow_runs`, fn `workflow_runs`, `workflows` at lines 24, 42, 79 to 122, 181 to 186) | A GitHub Actions workflow must always be written in full; bare "workflow" will now mean the cpd object. Also `core/src/inbox/model.rs:45` ("A workflow run waits for the User's approval"). The `[deploys] workflows` policy key is in the policy part. |
| `core/src/home.rs:6`, `:490` | Names `workflows/` as a cpd 1 directory that cpd 2 must not collide with (test `no_name_collides_with_cpd_1`) | cpd 1's Workflow was a name for Plan (retired by D3); the term returns with a new meaning, so the comment and test list need a note that it is the old directory, not the new object. `other`, history. |
| `core/src/pr/open.rs:44`, `:533`, `:565` | "open, push or rebase PR {pr}", "to open the PR by hand", "nothing to open a PR for" | `PR` here is both: it acts on a Workflow and opens its pull request (`mixed`). The sentence needs two words after the rename. |
| `core/src/context/curation.rs:323` | "plan a docs PR that documents these records" | Told to a Lead: a "docs PR" is a Workflow of shape docs that ends in one pull request (`mixed`). |
| `core/src/pr/cancel.rs:221`, `:222`, `:297` | "an open PR is cancelled only in phase dispatch or deploy" and "PR {key}'s pull request #{} is {state} ..." | Cancels a Workflow whose pull request is open or closed; the same word PR is used for the status (object) and the pull request state (`mixed`). |
| `core/src/pr/post.rs:50`, `core/src/pr/reply.rs:54` | "PR {key} ... is {status}: pr post needs an open PR" | Status `open` is a Workflow status but the command works on its pull request (`mixed`). |
| `core/src/pr/pulls.rs:227` | "a PR has {what} {value:?}" is the message for a bad `pull_requests` row | Here PR means the pull request, while `PrKey::parse` on the next line is the object: one function uses the word for both. |
| `core/src/prune/mod.rs:182` | "cannot read the output of gh pr list: a PR lacks a field" | PR means the pull request here (sense `pull request`); a global PR→Workflow replace would break it. Same for `core/src/prune/tests.rs:22` comment and `core/src/pr/gh.rs:868` comment ("never \"no PRs\"") and `core/src/inbox/team.rs:197`. |
| `core/src/inbox/rules.rs:457`, `core/src/inbox/pr.rs:163` | `id("PR key", &pr, PrKey::parse)` | The only inbox strings where PR is the object; the rest of `core/src/inbox` uses PR for the pull request. Rename by hand, not by pattern. |
| `core/src/pr/rebase.rs:323`, `:509` | Comments cite "Escalation nl-pr-a1-q2" and "nl-pr-a1-q1" | Past Escalation ids (history, contain `pr` as a Plan name `nl-pr`); not an identifier to rename. |
| `core/src/pr/mod.rs:309`, `:313` | Log file name `pr-<key>.<hook>.log` | A file name on disk and in docs; renaming leaves old logs unreachable. |
| `core/src/error.rs:274`-`:295`, `:324` | Error codes `pr_*`, `base_pr_*`, `needs_other_pr`, `until_pr_not_incident` | Printed in `error.code`, a JSON contract: renaming is a CLI change, pinned by `core/src/error.rs` tests (lines 474 to 560). |

## Counts

- Files: 232 in the part; 101 have at least one occurrence of `T` (read at the matching lines); 131 have none (not read); no file read in full.
- Rows per table: Identifier families 56; User-facing strings 273 (45 clap help, 228 literals); Wrong or stale 15.
- Occurrences of `T`: 4605 (4041 in production code, 564 in inline test blocks).
- Occurrences per sense (each `T` occurrence assigned once, by the first row that contains it; the sense of a row is a majority sense, so counts are by row, not by line):

| Sense | Occurrences | of which production code | of which inline tests |
|---|---|---|---|
| object | 3245 | 2965 | 280 |
| pull request | 699 | 504 | 195 |
| mixed | 653 | 565 | 88 |
| other | 8 | 7 | 1 |
| phase | 0 | 0 | 0 |
| step | 0 | 0 | 0 |
| total | 4605 | 4041 | 564 |

- Strings by sense: object 197, pull request 52, mixed 24.
- Identifier rows by sense: object 23, mixed 15, pull request 14, other 4.
