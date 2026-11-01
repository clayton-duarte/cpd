# cpd 2: command reference

Status: **normative for every cpd 2 implementation Job**, under [contract.md](contract.md): where the two disagree, contract.md wins and this file is fixed. Vocabulary: [CONTEXT.md](../CONTEXT.md). This file uses the contract's names, states, tables and Event kinds exactly; "§n" without a prefix means contract.md §n and "cpd 1 §n" means `docs/archive/cpd1/cli-contract.md` §n.

It specifies every command of outline §3 (synopsis, flags, preconditions with their error and exit codes, effects, JSON output, idempotence), the pi adapter's protocol, the Worker contract, what the TUI reads and `doctor`.

Evidence labels as in contract.md: **verified** (run on the homeserver on 2026-10-14), **docs** (read in the named tool's documentation or source), **unverified** (a design assumption the implementation Job must check).

The commands serve the **CPD flow** (hs-n31): three stages, **Context** (Grounding, Grilling), **Plan** (Clarifying, Scoping, Splitting, Orchestrating) and **Dispatch** (Documenting, Red testing, Implementing, Verifying, Deploying). contract §3.9 maps each step to its phase and commands.

## Contents

1. [Conventions](#1-conventions)
2. [Command index](#2-command-index)
3. [`context`](#3-context)
4. [`plan`](#4-plan)
5. [`job`](#5-job)
6. [`attempt`](#6-attempt)
7. [`review`](#7-review)
8. [`ask` and `answer`](#8-ask-and-answer)
9. [`pr`](#9-pr)
10. [`session`](#10-session)
10a. [`board`](#10a-board)
11. [`events`](#11-events)
12. [`watch`](#12-watch)
13. [`tui`](#13-tui)
14. [`doctor`, `config`, `tiers`, `models`, `preset`](#14-doctor-config-tiers-models-preset)
15. [The pi adapter's protocol](#15-the-pi-adapters-protocol)
16. [The Worker contract](#16-the-worker-contract)
17. [What the TUI reads](#17-what-the-tui-reads)
18. [Changes from cpd 1](#18-changes-from-cpd-1)
19. [Amendments to contract.md](#19-amendments-to-contractmd)
20. [Sections changed by R19 and R20](#20-sections-changed-by-r19-and-r20)
21. [`inbox`](#21-inbox)
22. [`policy`](#22-policy)
23. [`slot`](#23-slot)
24. [`secret`](#24-secret)
25. [`incident`](#25-incident)
26. [`gate`](#26-gate)
27. [`prune`](#27-prune)
28. [`tag` and `version`](#28-tag-and-version)
29. [`janitor`](#29-janitor)
30. [`resource`](#30-resource)

## 1. Conventions

### 1.1 How each command is written

Every command section has the same parts:

- **Synopsis**, then a table of every flag. `<x>` is required, `[x]` optional, `a | b` exclusive. A repeated flag says so.
- **Preconditions**, checked in the order listed; the first that fails is the result. Each row gives the error `code` and the exit code (§1.2). The common refusals of §1.3 apply to every command and are not repeated.
- **Effects**: **rows** written (table and columns), **Events** appended (kinds of §5.2), **tmux** and **git** actions and files under `$CPD_HOME` (§1.4). "One transaction" means one write transaction with its `txns` row, its `history` rows and its Events (§4.2). External work (git, tmux, hooks, `gh`, pi) is never inside it.
- **Output**: one JSON example of success. Every failure is the envelope of §1.2, `{"ok": false, "error": {"code", "message", "details"}}`; `details` is listed where it carries something the caller acts on.
- **Idempotent**: what a rerun does. The default of §1.3 is `"already": true`; a command that differs says so.

A command is **internal** when cpd runs it by itself and neither an agent nor the User types it: a program cpd starts in a pane or detached, or a command only cpd's own parts (the engine, the pi adapter) run. Internal commands are hidden in clap (`hide = true`; a command under a hidden one is internal too), so they are absent from `--help` text but listed, marked, in the `tree` (§1.2). Today: `session run`, `attempt run`, `tui view` and its views, `plan decide` and `session state`. They are documented so their implementation Jobs have a contract.

### 1.2 Output and exit codes

Kept from §1.2 (D1). Every command prints exactly one JSON value on stdout. Five kinds of command print something other than one JSON value and are the only exceptions (amendment 10, §19):

| Command | What it prints | Why |
|---|---|---|
| `cpd events follow` | one Event per line (§5.4) | the one streaming command (§1.2) |
| `cpd gate finish` (§26) | the run's report on stderr and its exit status | it prints no JSON of its own; the status is the command's, or 64 when the run left something behind |
| `cpd session run`, `cpd tui view` (internal) | they run pi or draw a ratatui view in their pane | they are the programs inside cockpit panes (§6.2), not commands an agent calls. If they fail before taking the terminal, they print the failure envelope and exit with its code |
| `cpd tui action` (internal) | nothing, ever | the StatusBar's click binding and the `merge` popup's confirm (`cpd tui action merge <owner>/<repo>#<n> --client <client>`, hs-d473) run it with `run-shell -b`, which puts the active pane into view mode on a failure or any output (§13.2); it reports a failure with `display-message -c <client>` and a failed merge with the cockpit's one failure dialog, a `text` dialog (§13.2, §17 "No notes") |
| `cpd` (no arguments), `cpd tui --attach` | nothing on success: it replaces itself with `tmux attach` | the approved outline says `tui` "builds and attaches the cockpit"; without `--attach`, `cpd tui` prints JSON like every command. Bare `cpd` is §13.3 (hs-d259) |

`cpd --version` (like `--help`) prints one JSON value too: `{"ok": true, "version": "0.18.45-070ddee", "protocol": 18, "store": {"opens": true, "refusal": null, "unknown": [], "pending": []}}`. The `version` field is `<semantic version>-<commit>` where the semantic version is `0.<protocol>.<migrations>`: the commit is the short hash the build was given in `CPD_COMMIT` (set by `scripts/install-cpd.sh` and the Gate, so an installed binary reports the commit it was built from) and `dev` when the build was given none, e.g. `0.18.45-dev` (docs/implementing.md "The version string"; docs/versioning.md). `store` is the **verdict** of contract §4.5 ("The window") on `$CPD_HOME`'s database opened read-only, computed by the one function every guard uses (`store::verdict`): `null` when the database file does not exist; otherwise `opens` (`true` when this binary can open the store, migrating or converting it if needed), `refusal` (`null`, or `{"code", "message"}` with `code` one of `store_too_new`, `store_failed`, `migration_changed`, `migration_order`, §1.3) when `opens` is `false`, `unknown` (the migration names the store has applied that this binary lacks, in name order: inside the window they are all additive) and `pending` (the names of this binary's migrations the store has not applied, in the order they would be applied; for an unconverted store the legacy names it lacks, then every named one). `unknown` and `pending` are `[]` for a store that is not named yet and filled whenever the store is a named store, whether or not it opens. `protocol` is an integer, **18** today (1 until Plan manager added `session-message`, 2 until Plan inbox added the four `inbox-*` kinds, 3 until `session state idle --reply`, hs-d278, 4 until `session-titled`, hs-d257, 5 until `pr-base-changed`, hs-d324, 6 until `session state waiting`, hs-d303, 7 until `review-opened`/`review-closed`, hs-d496, 8 until `session-retiered`, hs-d506, 9 until `plan-outlined`, hs-d502, 10 until `job-merged`'s `data.turn` and `session-message`'s `data.unblocks`, hs-d514, 11 until `session-resumed`, hs-d552, 12 until `session report`, hs-d573, 13 until `incident-started` and `incident-ended`, hs-d634, 14 until `mcp-attached`, pro-d8, 15 until the adapter runs `session switch-failed` and `session switched` and the `error` Event carries the reporting Session, 16 until `pr-described`, pro-d94, 17 until `session-model-changed`: the tier-preset switch, the adapter acts on it at the Session's next idle, §14.6), raised whenever an Event kind, an Event `data` field the pi adapter reads, or the output of a command the adapter runs changes in a way the adapter depends on (contract §5.2 "The protocol number"); the adapter checks it (§15.2). `--version` never creates, migrates or writes the database and runs neither the schema check nor the conversion. **The guards read `store`:** a driver or a Lead's wrapper reloads into a binary only when `store` is `null` or `store.opens` is `true` (contract §3.4, §6.7) and `scripts/install-cpd.sh` refuses a build whose `store.opens` is `false` and a Lead Branch build that carries a migration file that is not on `origin/main` (contract §3.7).

Every `--help` prints `{"ok": true, "help": "<clap's text>"}`. The root help (`cpd --help`, `cpd -h`, `cpd help`) adds `tree`: the whole command tree, every command whatever its depth, in clap's order, clap's own `help` subcommand left out. `cpd <group> --help` and `cpd help <group>` print `help` only. Each node is `{"name", "about", ...}`: `name` the command's word, `about` its one-line description (clap's, which ends with its § here). A runnable node (no subcommand, or subcommands that are optional, as `tui`) also has `callers`, `cockpit` and `internal` (§2). A node with subcommands has `commands`, its children. Example (trimmed with `...`):

```json
{"ok": true, "help": "...", "tree": [
  {"name": "context", "about": "Contexts and their records (commands.md §3)", "commands": [
    {"name": "new", "about": "Create a Context (§3.1)", "callers": ["Lead"], "cockpit": true, "internal": false}
  ]},
  {"name": "ask", "about": "Ask the Lead a question and wait for the answer (Worker) (commands.md §8.1)", "callers": ["Worker"], "cockpit": false, "internal": false},
  {"name": "tui", "about": "Build the cockpit and start the engine (commands.md §13)", "callers": [], "cockpit": false, "internal": false, "commands": [
    {"name": "view", "about": "A view in a cockpit pane (internal; owns its terminal) (§13.2)", "commands": [
      {"name": "tree", "about": "The session tree (left)", "callers": [], "cockpit": false, "internal": true}
    ]}
  ]}
]}
```

### 1.3 Selection and common refusals

- **Plan:** `--plan <id>`, else `CPD_PLAN`, else `plan_required` (exit 2) (§1.3).
- **Context:** `--context <id>`, else the Context of the selected Plan when a Plan is selected (by `--plan` or `CPD_PLAN`), else `context_required` (exit 2). New: a Lead's pane has `CPD_PLAN` set (§6.5), so it never needs to name its Context.
- **Session:** `--session <id>`, else `CPD_SESSION`, else `session_required` (exit 2). `CPD_SESSION` is set in every pane cpd starts (§6.5).
- **Short Session ids** (hs-a3, hs-q3): wherever a command takes a Session (`--session`, `CPD_SESSION` and the argument of `session open`, `close`, `fork`, `park`, `rename`, `archive`, `unarchive` and `show`) the value is resolved **once, at the boundary, into a `SessionId`** (`core/src/session/resolve.rs`, `docs/implementing.md`), trying these forms in this order, the first that matches deciding:

  | # | Form | Resolves to |
  |---|---|---|
  | 1 | the full id (a Session whose id equals it, a UUID in practice) | that Session |
  | 2 | a **Plan id** | that Plan's Lead Session, `plans.lead_session` |
  | 3 | **`<plan>·<prefix>`** or **`<plan>.<prefix>`** (split at the first `·` or `.`; `.` because `·` is hard to type and Plan ids never contain either) | the Session of that Plan whose id starts with `<prefix>` (at least 1 character) |
  | 4 | a **bare id prefix of at least 4 characters** | the Session, of any Plan or none, whose id starts with it |

  Matching is exact and case-sensitive, over every Session, archived ones included (so `session unarchive` and `session history` can name them). A form that matches nothing falls through to the next; when none matches, `session_not_found` (exit 4). A form (3) or (4) that matches **more than one** Session is `session_ambiguous` (exit 3) with `details.candidates`, the matching ids and is never resolved by guessing. A value outside §2.1's input pattern is `invalid_session_id` (exit 2); one that is valid but shorter than 4 characters and no Plan id or form (3) is `session_not_found`. Output always carries the full id.
- **PR** (R34, §1.3): `--pr <key>`, within the selected Plan, used by `job add`, `pr open`, `pr rebase`, `pr push`, `pr view`, `pr reply`, `pr rerun` and `pr post`. It may be omitted when the Plan has exactly one PR that is not `cancelled`, which is then selected; with two or more, `pr_required` (exit 2); with none, `pr_required` too, its message naming `cpd pr add`. A key that is not one of the Plan's PRs is `pr_not_found` (exit 4). Exceptions, stated at each command: `pr poll` has no `--pr`, it polls every open pull request of the User (§9.2, hs-d96) and `job edit --pr` changes the Job's PR, so omitting it changes nothing (§5.2). `pr add`, `pr edit`, `pr cancel` and `pr start` take the key as their positional argument.
- **The Lead's Workflow** (contract §3.6, pro-d219, pro-d259): the PR of the Plan that the working Lead's current turn concerns, stored as `sessions.working_pr` and drawn as the Lead's dot (§13.2). Its definition is contract §3.6's. Only these commands name it, and only when the Plan's working Lead runs them: `job add`, `job edit`, `job cancel`, `job retry`, `job urgent`, `job approve-tests`, `answer`, `pr add`, `pr edit`, `pr describe` and `pr cancel`.
- **Job, Attempt, Escalation, Plan review:** a positional id, within the selected Plan (§2.1).

Common refusals, which apply to every command and are not repeated in the sections below:

| Code | Exit | When |
|---|---|---|
| `usage` | 2 | unknown flag, missing argument, flag value of the wrong shape, exclusive flags together |
| `plan_required`, `context_required`, `session_required` | 2 | the command needs one and none was selected |
| `pr_required` | 2 | the command needs a PR, `--pr` is omitted and the Plan does not have exactly one PR that is not `cancelled` (R34) |
| `supersedes_invalid` | 3 | `context decide` or `context note` named, in `--supersedes`, a record whose kind it may not supersede (§3.4); `details: {record, kind, allowed}` |
| `curation_not_pending` | 3 | `context retire` or `context docs` (§3.16, §3.17) named a record that has no `pending` retirement (or doc candidate): none, already decided, or not curated; `details: {record}`; nothing is written (hs-d551) |
| `user_only` | 3 | `context curation`, `context retire` or `context docs` (§3.15 to §3.17) ran with `CPD_SESSION` or `CPD_ATTEMPT` set in its environment: only the User decides what leaves the Context and what is documented, so no agent's Session runs them (hs-d551); the cockpit's popups and views run without those variables (§3.15) |
| `manager_missing` | 3 | `context docs --send` (§3.17) found no live Manager Session (role `manager`, not archived) for the Context; nothing is written |
| `args_invalid` | 2 | `context retire` or `context docs` (§3.16, §3.17) named one id in both of its lists |
| `session_ambiguous` | 3 | a short Session id (§1.3) matches more than one Session; `details.candidates` lists their ids |
| `push_rejected` | 3 | `pr push` found the remote branch moved (§9.9); `details.stderr` is git's |
| `base_not_found` | 4 | `pr add`, `pr edit` (§9.4): the base branch does not exist; `pr open` (§9.1): the base branch of a stacked PR, its base PR's Lead Branch, is not on the remote (never for a `merged` base PR: that PR is unstacked first, hs-d301); `details` `{pr, base, base_pr}` (hs-d324). Not returned by an `open` or `update` Job's submit: the merge of that Job finds it (contract §7.4 step 6) and ends the Attempt `failed` / `gate_error` with it as the detail |
| `base_conflict` | 3 | `pr open` (§9.1, contract §7.6 step 1) found that the gated commit conflicts with `origin/<base>`, freshly fetched (`git merge-tree`); `details` is `{pr, base, base_sha, sha, files, next}`, `files` the conflicted paths, `next` the commands the Lead runs, as written (hs-d490; it replaces hs-d329's `base_behind`, retired: a base that merely moved ahead refuses nothing). The merge of an `open` or `update` Job finds the same conflict at its step 6 (contract §7.4), not at submit: the Attempt ends `failed` / `merge_conflict` with the files in its detail |
| `pr_not_open` | 3 | `pr reply` (§9.8a) and `pr rerun` (§9.8c) found the PR not `open`; `details` is `{pr, status}` (pro-d68, pro-d155) |
| `feedback_not_found` | 3 | `pr reply` (§9.8a): `--to` is the `id` of no item in the PR's stored list; `details` is `{pr, to, open: [ids]}`; nothing is posted (pro-d68) |
| `no_failed_run` | 3 | `pr rerun` (§9.8c) found no GitHub Actions workflow run of the PR's head commit failed; `details` is `{pr, head_sha, runs: [{id, name, status, conclusion}]}`; nothing is re-run (pro-d155) |
| `pr_not_closed` | 3 | `pr cancel` (§9.6) found the `open` PR's pull request not stored `closed`; `details` is `{pr, number, url, state}`, the stored row's `state` (`open` or `merged`) (hs-d329) |
| `already_posted` | 3 | `pr post` (§9.8b) found a `pr_posts` row for the PR; `details` is `{pr, channel, ts, link, posted_at}` (pro-d65) |
| `post_disabled` | 3 | `pr post` (§9.8b) found `policy.pr.post.enabled` is false; `details` is `{pr, key: "pr.post.enabled"}` |
| `not_authored` | 3 | `pr post` (§9.8b) found the stored pull request is not `authored`; `details` is `{pr, number}` |
| `post_unconfigured` | 3 | `pr post` (§9.8b) found no channel configured; `details` is `{keys: ["pr.post.channel", "inbox.team_channel"]}` |
| `pr_not_ready` | 3 | `pr post` (§9.8b) found `postable::unready(&row)` is not empty; `details` is `{pr, number, url, reasons: [<as_str of each>, …]}`, the Unready reasons being `not_open`, `draft`, `conflicting`, `changes_requested`, `checks_failed`, `checks_pending`, `comments_unread`, `bot_comments` (pro-d64, contract §2.2, §8.3) |
| `base_pr_unknown`, `base_pr_unavailable`, `base_pr_other_repo`, `pr_other_plan` | 3 | `pr add` or `pr edit` (§9.4, §9.5) found a bad PR reference in `--base` or `--needs`: no such PR (`base_pr_unknown`, the long form `--base <plan>/<key>` only); a base PR that is `cancelled` or `merged`; a base PR of another repo (`base_pr_other_repo`, the long form only: a bare `--base <key>` of another repo is a branch, `base_not_found`); a PR of another Plan (`pr_other_plan`, the long form or `--needs`) (contract §2.2 "A PR reference" and "Validation of a reference", hs-d522); `details` as at §9.4 |
| `stacked_rebase` | 3 | `pr rebase` (§9.3) found the PR stacked on a base PR; `details` is `{pr, base, base_pr, next}` (contract §3.8 "Stacked PRs" step 4, hs-d325) |
| `attempt_running` | 3 | `pr rebase` (§9.3) or `pr rebase --continue` found an Attempt of the PR of any kind (work, open, update) running (pro-d103, pro-d105); `details` is `{pr, jobs: [keys], attempts: [ids]}`; nothing is changed, checked before the fetch; the message tells the Lead to steer the Worker to do the rebase instead |
| `needs_unavailable` | 3 | `job add` (A4, pro-d89): a named Job is `cancelled`, `failed`, a draft in a phase after plan, or an `open` or `update` Job; or in phase `dispatch` a failed Job has no fix appended yet (A4): that failed leaf refuses to queue a new Job that needs it; `details.jobs` lists them; may include `fix_job` with the Job key for the failed leaf |
| `invalid_context_id`, `invalid_plan_id`, `invalid_pr_id`, `invalid_job_id`, `invalid_attempt_id`, `invalid_escalation_id`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id` | 2 | an id does not match §2.1 |
| `config_missing`, `config_invalid` | 1 | §8.1; hs-d562, pro-d53, hs-d554 and hs-d506 add the Tier checks of contract §8.2 (§9 "Tier rules"): G0–G5 are `tier_invalid` (exit 2, below); `context_distill_tier` must be GX, not H0 or anything else, else `config_invalid` (hs-d562); `lead_tier`, `review_lead_tier` or `manager_tier` cannot be letter G ("must not be a G Tier..."), else `config_invalid`, or effort 5, else `config_invalid`; `open_tier` must be a Tier a Worker may have (§9), else `config_invalid`. Each with `details.key` |
| `tier_not_allowed` | 2 | `job add`, `job edit`, `job retry` (§5.1, §5.2, §5.4) and `job dispatch` (§5.5): the Job's Tier is outside the Worker range of contract §9 "Tier rules" (`H0`–`H3` and `S0`–`S3`); Workers use only these Tiers to keep cost down (pro-d82, hs-d554, contract §9 "Tier rules"); `details` `{tier, allowed, rule}`; also an `error` Event of engine E3 (contract §3.7) for a stored Job on such a Tier. `plan new --tier` (§4.1) and `session retier --tier` (§10.16) refuse a Lead's or the Manager's Tier of letter G or effort 5 the same way, `tier_not_allowed` with `details` `{tier, allowed: "O0-O4, S0-S4, H0-H4", rule: "hs-d506, hs-d562"}` (contract §9 "Tier rules", pro-d139, pro-d140, pro-d142) |
| `tier_invalid` | 2 | `job add`, `job edit`, `job retry` (§5.1, §5.2, §5.4), `plan new --tier` (§4.1) and `session retier --tier` (§10.16): a Tier that is not valid at all: bad syntax, `G0`–`G5` (§5, the Tier flag), or an effort the Tier's model lacks; `details.tier` and `details.efforts` for an effort. A Lead's `--tier` is checked before anything is written |
| `store_too_new` | 1 | the store has a breaking migration this binary does not know, or a later scheme (contract §4.5 "The window"; it keeps its code and exit); `details: {user_version, floor, unknown}`: the store's `user_version`, its floor (the name of its newest breaking migration, or `null`) and every applied name this binary lacks. A store with a numbered migration this binary lacks (`user_version` above the legacy set and below 1000) is the same code, with `unknown` empty |
| `migration_failed` | 1 | a migration, or the conversion, failed and rolled back (§4.5); `details: {from, to, sqlite_message}` |
| `migration_changed` | 1 | an applied migration this binary knows has another checksum than the store recorded (§4.5 R3); `details: {name, recorded, embedded}` |
| `migration_order` | 1 | a pending migration cannot be applied in this order, nothing written (§4.5 O3): a breaking one while the store has an applied migration that sorts after it or that this binary does not know; `details: {name, applied_after, unknown}` (names) |
| `store_tampered` | 1 | a write command, when the schema check fails (§4.3); read commands report it in `tamper` instead |
| `store_busy` | 5 | no write lock within `store_busy_ms` (§4.2); `SQLITE_BUSY` and `SQLITE_LOCKED` both |
| `internal` | 1 | a bug in cpd itself (for example a malformed Event built by a command), or a failed write to stdout other than EPIPE (no I/O error code exists). `store_failed` is for SQLite failures only |
| `context_not_found`, `plan_not_found`, `pr_not_found`, `job_not_found`, `attempt_not_found`, `escalation_not_found`, `review_not_found`, `session_not_found` | 4 | the selected or named object does not exist. (A Context record that names a missing record is `record_not_found`, exit 3, §4.6) |
| `no_merge_method` | 3 | `pr merge` (§9.10) or `pr post` (§9.8b) read the repo's settings and found squash, merge commit and rebase all off; `details` `{repo, number, squash, merge, rebase}` (pro-d254, pro-d261) |
| `merge_not_clean` | 3 | `pr merge` (§9.10) re-read the pull request and found it not `OPEN` or not `CLEAN`; `details` has `repo`, `number`, `state`, `merge_state` (hs-d272, hs-d274) |
| `required_check_not_passed` | 3 | `pr merge` (§9.10) re-read the pull request's head commit and a required check name is missing, pending or failed; `details` has `repo`, `number`, `missing`, `pending`, `failed` (lists of check names; hs-d743, hs-n571) |
| `mcp_unknown` | 4 | `mcp attach` (§24.1) named a server the machine policy does not list; `details: {name, machine, available}`, `available` the sorted server names (an empty array when none); nothing is written (pro-d8) |
| `item_not_found` | 4 | `inbox show`, `snooze`, `dismiss`, `claim` or `approve` named an Inbox item that does not exist |
| `item_closed` | 3 | `inbox snooze`, `dismiss`, `claim` or `approve` named an Inbox item that is closed (§21) |
| `not_claimable` | 3 | `inbox claim` (§21.7): the item is not `review-requested` or is dismissed (C4); `details.kind` |
| `not_approvable` | 3 | `inbox approve` (§21.13): the item is not `deploy`; `details.item`, `details.kind` |
| `review_context_unset` | 3 | `inbox claim` (§21.7): config `inbox.review_context` is unset (C5) |
| `post_not_found` | 3 | `inbox claim` (§21.7): the team review cache holds no primary post of the pull request (C4) |
| `not_loop_guarded` | 3 | `session resume` (§10.17): the Session is not a `lead` or a `manager`, or is archived, or is not `blocked` with label `φ: loop guard`; `details` `{state, label}` |
| `slack_auth` | 3 | a Slack helper (§21.6, §21.7) found no usable Slack sign-in in pi's MCP credentials; the message says to run `pi mcp login slack` |
| `slack_not_configured` | 3 | a Slack helper (§21.6, §21.7) found no `slack` server in `~/.pi/agent/mcp.json`; the message says to run `pi mcp add slack --url https://mcp.slack.com/mcp --exposure hidden` |
| `slack_failed` | 1 | a Slack helper (§21.6, §21.7) could not read the channel (E11), send a message (pr post), or timed out; check the channel before retrying |
| `slack_refused` | 1 | a Slack helper (§21.8) returned a Slack API error for a specific set of refusable names (invalid_name, bad_timestamp, message_not_found, channel_not_found, is_archived, thread_locked, not_reactable, too_many_emoji, too_many_reactions, no_item_specified); E17 parks the reaction with the reason; the Row is not retried until the policy changes (contract §3.7 E17) |
| `slack_unparseable` | 1 | a Slack helper (§21.6, §21.7) received an invalid payload (E11), response (post), or no messages (E11). Also the code of the engine's E11 `error` Event (contract §3.7) |
| `slack_helper_failed` | 1 | a Slack helper (§21.6, §21.7) could not load pi's MCP modules or other failure inside it. Also the code of the engine's E11 `error` Event (contract §3.7) |
| `spawn_failed` | 1 | a Slack helper (§21.6, §21.7) could not be started as a child process; `details` are `{argv, exit, stdout, stderr}` |
| `jira_auth` | 3 | the Jira helper `jira-transition.ts` (§21.12) found the `atlassian` connection needs a sign-in, a 401 or 403, or no site with the scopes `read:jira-work` and `write:jira-work`; the message says to run `pi mcp login atlassian`. `cpd_core::jira` (§21.12, J8) passes it on. It is only the machine-wide `error` Event code of engine E19 (§21.11), never a command's refusal |
| `jira_not_configured` | 3 | `cpd_core::jira` (§21.12, J8) found no `atlassian` server in the machine policy `policy/host.toml`; the message says to add `[mcp.servers.atlassian]` there. Only the machine-wide `error` Event code of engine E19 (§21.11), never a command's refusal |
| `jira_failed` | 1 | the Jira helper (§21.12) could not read the issue or the transitions, or move the ticket, for a reason a retry may cure (429, 5xx, a 400 on the transition, a timeout, text that is not the expected JSON, another tool failure); `details.tool` and `details.status_code` when known. Only the `error` Event code of engine E19 (§21.11), scoped to the Plan, never a command's refusal |
| `jira_refused` | 1 | the Jira helper (§21.12) refused the ticket for a reason a retry does not cure: `details.reason` is `site_ambiguous`, `issue_not_found` or `no_transition`. Engine E19 (§21.11) parks the row with the reason and one `error` Event; never a command's refusal |
| `jira_helper_failed` | 1 | the Jira helper (§21.12) could not load pi's MCP modules, or they are of another shape; `cpd_core::jira::answer` (J8) uses it for a helper error code it does not know. Only the `error` Event code of engine E19 (§21.11), never a command's refusal |
| `store_failed` | 1 | a write command failed on SQLite (contract §4.5); for `pr post` (§9.8b) means the message WAS posted and the link is in `details` |
| `archive_blocked` | 3 | `context archive`, `plan archive` or `session archive` found a live object (contract §2.2 "Archived"); `details.blockers` lists each, `details.next` says what to do |
| `archived` | 3 | the object (or its Context or Plan) is archived and the command does not work on archived objects; `details.next` is the unarchive command |
| `cas_lost` | 3 | a compare-and-swap found the state changed since it was read (§4.2); rerun the command |
| `illegal_transition` | 3 | a transition not in §3, refused by the command or by a trigger |
| `locked_job_tampered` | 3 | a Locked Job's fingerprint does not match (§2.3) |
| `pr_title_required` | 3 | `pr open` found a PR with no stored title, one that existed before migration `pr-title` (contract §7.6, hs-d285); the message says to run `cpd pr edit <key> --title "<text>"`. The merge of an `open` Job finds it too (contract §7.4 step 6), before anything is pushed, and ends the Attempt `failed` / `gate_error` with the detail; not returned by submit |
| `jira_required` | 3 | `pr open`, the open Job's start (§5.5, contract §3.2) or its merge found a PR whose policy sets `pr.jira_required` and whose stored `jira` is null, the PR not adopted (`head_ref` null) (pro-d127, §9.4, §9.5); the message says to run `cpd pr edit <key> --jira <KEY>`; `details` is `{plan, pr}` |
| `needs_required` | 3 | `pr add` in phase `dispatch` found neither `--needs` nor `--no-needs` for a PR that is not stacked (pro-d46); a PR added after the lock with no needs is started at once and cannot be linked later; for a stacked PR, the base PR is its link; `details` `{plan, pr, phase}` |
| `adopt_not_open` | 3 | `pr add --adopt` (§9.4): the pull request's `state` is not `OPEN`; `details` is `{number, url, state}` |
| `adopt_fork` | 3 | `pr add --adopt`: the pull request is from a fork (`isCrossRepository`); cpd cannot push to a fork; `details` is `{number, url}` |
| `adopt_cpd_branch` | 3 | `pr add --adopt`: the head branch equals the stored `lead_branch` of any PR of that repo; `details` is `{number, head_ref}` |
| `adopt_taken` | 3 | `pr add --adopt`: a PR of any Plan with the same repo and head branch is `waiting`, `dispatch` or `open`; `details` is `{head_ref, plan, pr}` |
| `adopt_foreign` | 3 | `pr add --adopt`: the pull request's author is not the User and `--confirm-author` is missing or differs (case-insensitively); the message tells the Lead to ask the User first; `details` is `{number, url, author, user, confirm: "--confirm-author <author>"}` |
| `pr_adopted` | 3 | `pr edit --repo` or `--base` on an adopted PR (§9.5): the repo and the base come from the pull request; `details` is `{pr, flag}` |
| `resource_undeclared` | 3 | `job add` or `job edit --spec` (§5.1, §5.2), `job dispatch` (§5.5) or `resource run` (§30) named a resource that no host, owner or repo policy file declares for the PR (contract §8.3 "The `resources` domain"); `details` `{job, pr, resource, declared, file}` (`resource run`: `{resource, repo, declared, file}`), `declared` the sorted VISIBLE names (the union over host, owner and repo), `file` the repo policy file's path whether or not it exists. Engine E3 raises it as one `error` Event for a queued Job whose name no policy declares any more (contract §3.7) |
| `resource_busy` | 3 | `job dispatch` (§5.5) or `resource run` (§30) found a pool with no free slot (contract §1.4 "The free slots of a pool"; for `job dispatch`, `workers` too); `details` `{job, resource, holders}` (`resource run`: `{resource, repo, holders, waited_minutes}`), `holders` the holders of contract §1.4 of the pool, command slots then Job tenants. Engine E3 never raises it: it skips the Job (contract §3.7) |
| `kind_reserved` | 3 | `job add --kind open` or `update` (§5.1): cpd adds those Jobs, never the Lead (contract §2.2 "Job kinds"); `details.kind` |
| `test_job_required` | 3 | an `impl` Job that is not a draft, or a Plan review or lock over one (§5.1, §4.6, §4.8), needs no `test` Job of its PR; `details.jobs` lists the `impl` Jobs |
| `tests_unconfigured` | 3 | `pr add --shape tdd` (§9.4), `job add --kind test` or `impl` (§5.1) or a `test` or `impl` Job's submit (§6.1) found that the PR's repo policy has no `[test-commands]` (no repo file, or a file without the table; contract §7.7); the message names `--shape docs`, `--shape config` and the table |
| `test_files_missing` | 3 | a `test` Job's submit (§6.1): `details.paths` lists the `test_files` entries that match no path the Job changed |
| `tests_build_failed`, `tests_not_red`, `tests_failed` | 3 | a `test` or `impl` Job's submit (§6.1, contract §7.7): the repo's `build` failed; the named tests passed where they must fail (`test`); they failed where they must pass (`impl`). `details` `{job, tests, tail, log}` |
| `tests_not_found` | 3 | a `test` or `impl` Job's submit (§6.1, contract §7.7): the repo's optional `[test-commands]` `list` command exited non-zero for a named test, so no test has that name (hs-d497); `details` `{job, tests, tail, log}`, `tests` the names that match nothing |
| `impl_job_required` | 3 | a `verify` Job that is not a draft, or a Plan review or lock over one (§5.1, §4.6, §4.8), needs no non-cancelled `impl` Job of its PR; `details` `{plan, job, jobs}`, `jobs` the PR's `impl` Jobs |
| `shape_violation`, `shape_incomplete` | 3 | the PR's stored shape (§9.4, contract §2.2 "Job kinds", hs-d497): `job add` or `job edit --kind` gave a kind the shape does not allow (`details` `{pr, shape, kind, allowed}`; for an `open` PR `{plan, pr, status: "open", shape, kind, allowed: ["work"]}`, whatever the shape); a `tdd` PR has no `test` or no `impl` Job that is not `cancelled`, at `plan review`, `plan lock` or `job cancel` typed by the Lead (`details` `{pr, missing}`); **`pr cancel` and `plan cancel` never refuse it**: cancelling a PR or a Plan is never refused by the shape check (§5.3, §9.6, §4.12, hs-d573) |
| `draft_parked` | 3 | a draft that contract §2.2 "A parked draft" calls illegal would be written after the lock: `job add --draft` or `job edit` (§5.1, §5.2) of a stub that waits for nothing, or `pr add --shape` (§9.4) with no `--jobs` for a PR that would start at once (hs-d731); `details` `{plan, job, pr, needs}` (`{plan, pr, jobs}` for `pr add`, `jobs` the stub keys). The message names the legal forms |
| `tests_error` | 1 | a `test` or `impl` Job's submit: the repo's `[test-commands]` commands could not run (killed at `gate_minutes`, killed by a signal, `sh` not startable, `hook_invalid`; `list` included); the Worker asks the Lead. `details` as above |
| `tests_tampered` | 3 | an `impl` Job's submit (§6.1) changed a path the `test_files` of a `test` Job it needs guard and the Lead has not approved (§5.9); `details.paths`, `details.jobs` |
| `lead_branch_moved` | 3 | a merge detail, not a code any command returns: the merge of an `open` or `update` Job (contract §7.4 step 5) found the Lead Branch's tip not in HEAD and ends the Attempt `failed` / `merge_conflict` with the detail `lead_branch_moved: <message>` (a ladder cause: the next Attempt starts from the newer tip). Still declared, so it stays in this table; submit never refuses with it |
| `head_moved` | 3 | `review submit` (§31.1) found the pull request's current head is not `--head`; `details` `{pr, expected, actual}`, both SHAs lowercase; nothing was posted (pro-d169) |
| `open_job_live` | 3 | `pr open` (§9.1) found the PR's `open` Job `draft`, `queued` or `running`; `details.job` (hs-d573) |
| `not_an_impl_job`, `path_not_guarded` | 3 | `job approve-tests` (§5.9): the Job is not of kind `impl`; `details.paths` lists the paths no `test_files` entry of its test Jobs guards |
| `host_approval_required`, `not_a_manager` | 3 | `cpd host repair` (§9.17): needs an approval of the calling Session (`host_approval_required`); Manager role only (`not_a_manager`, commands.md §9.17) |
| `git_failed`, `tmux_failed`, `gh_failed` | 1 | the tool exited non-zero where the command did not expect it; `details` has `argv`, `exit` and the last 60 lines of stderr. `pr poll` also returns `gh_failed` for an empty or non-JSON body, a search response with `errors` and a lookup with no `data` (contract §7.6, hs-d96) |
| `gh_rate_limited` | 1 | an `error` Event code of engine E5 (contract §3.7, §5.2; `details`: `remaining`, `reset_at`), reported while it skips polls because GitHub's rate limit is nearly used; never a command's refusal |
| `install_state_invalid` | 1 | engine E6 (contract §3.7) or a command read the install state file `$CPD_HOME/run/cpd-install.json` (§1.4) and found it invalid (not JSON, unknown fields, missing or type-mismatched fields); `details: {path, reason}` |
| `adapter_missing` | 1 | an `error` Event code of the Lead wrapper `session run` (contract §6.7 "Adapter watchdog", §10.8; `details`: `session`, `state_since`, `seconds`), appended when it restarts a pi whose adapter never came back; never a command's refusal |
| `durable_session` | 3 | `session park` (§10.5), `session history` (§10.12) or `session fork` (§10.3) named a Session whose `runner` is `durable` (contract §3.6): it has no pane, so there is nothing to park, show or fork. (`session open` no longer refuses a durable Session: it resumes it and waits for its host's socket, §10.4.) A state error, as `archived` and `in_chat_slot` are; refused before any write or tmux call. Message: `Session <id> runs on durable: it has no pane. Send it a message with cpd session send; cpd chat will open it.`; `details: {session}` |
| `session_restart_loop` | 1 | an `error` Event code of engine E2 (contract §3.6, §3.7; `details`: `session`, `name`, `seconds`), appended when E2 detects a Session's pane gone but E2 itself restarted it less than 10 minutes ago (hs-d725, hs-d727), to guard against restart loops; never a command's refusal |

### 1.4 Files written by commands

The `$CPD_HOME` layout is §1.4. Commands in this file write files the contract's layout does not list (amendment 4, §19); each is beside its peers and named so it cannot collide:

| File | Written by | Read by |
|---|---|---|
| `run/<plan>/lead-message.md` | `plan new` | the Plan's Lead `session run` (its first message) |
| `run/<context>/manager-message.md` | `context manager` (B3-A2) | the Manager's `session run` (its first message) |
| `run/<plan>/summary.md` | `plan render` (§4.5, hs-d502): the Plan summary of the Plan as it is, without a review | the User, the Lead |
| `run/<plan>/summary-<review>.md` | `plan review` (§4.6, hs-d502): a copy of the summary that a later render cannot change | the pi adapter, which shows it in the Lead's chat (§15.5) |
| `run/<plan>/feedback-<review>.md` | the pi adapter: the User's note from the chat dialog's Change, overwritten on a retry | `plan decide --feedback-file` |
| `run/<plan>/messages/<session>.md` | `session fork --message-file` copies the file here | that Session's `session run` |
| `run/titles/<session>.log` | engine E10: the output of the `session retitle` runs it starts (hs-d257, contract §1.4); `locks/title-<session>.lock` is the run's own lock (§10.14) | the User |

### 1.5 Text arguments

Free text (Job `--title`, `--reason`, Context record text, questions and answers) is one argument, 1 to 20 000 characters, with no NUL; else `usage`. Context and Plan titles (`--title` of `context new`, `context rename`, `plan new`, `plan rename`) are 1–200 characters with no control characters, else `usage` (exit 2). Names (`--name`, `session rename`) are 1–80 characters with no control characters, else `invalid_name` (exit 2) (cpd 1 §11 `session rename`, kept). Files given by path (`--spec`, `--brief-file`, `--feedback-file`, `--message-file`) must be readable UTF-8, else `file_unreadable` (exit 2). A PR's title (`--title` of `pr add` and `pr edit`, hs-d285) is 1–72 characters on one line, with no control character; else `usage`. A PR's Jira ticket (`--jira` of `pr add` and `pr edit`) is a Jira key matching `^[A-Z][A-Z0-9]+-[0-9]+$`, else `usage`.

## 2. Command index

The index is generated: [docs/interface.md](interface.md), rendered from the `tree` of `cpd --help` (§1.2) by the test `cli/tests/interface.rs`, which fails when the page and the binary differ, either way; `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface` rewrites the page; never edit it by hand.

**Definitions:**
- **callers** = the agents meant to run the command, any of `Lead`, `Manager`, `Worker`.
- **cockpit** = an action of the User's cockpit runs it (the cockpit is the User's interface over the same commands, §17 Actions).
- **internal** = §1.1.

Anyone may run any command; a command with no callers that is neither cockpit nor internal is the User's, at a shell (for example `doctor`). The callers and the cockpit flag are kept in the binary (`cli/src/interface.rs`), next to the clap tree. The table's Writes column (each command's Effects says what it writes) and Contract column (each description ends with its §) are not carried over (hs-d251).

**Documented here before the binary has them** (the policies, contract §8.3, hs-d288). The Job that implements each adds it to the binary's tree with these values and `docs/interface.md` is then regenerated; until then this table is their index entry:

| Command | Callers | Cockpit | Internal |
|---|---|---|---|
| `cpd context owner` (§3.13) | Manager, Lead | no | no |
| `cpd host approve` (§9.17) | | no | yes |
| `cpd host repair` (§9.17) | Manager | no | no |
| `cpd policy show` (§22.1) | Manager, Lead, Worker | no | no |
| `cpd gate finish` (§26) | none | no | no |
| `cpd job approve-tests` (§5.9) | Lead | no | no |
| `cpd plan outline` (§4.13) | Lead | no | no |
| `cpd session retier` (§10.15) | Manager | no | no |
| `cpd secret approve` (§24.7, internal) | none | no | yes |
| `cpd models refresh` (§14.5) | Manager, Lead | no | no |
| `cpd models show` (§14.5) | Manager, Lead | no | no |
| `cpd preset list` (§14.6) | Manager, Lead | no | no |
| `cpd preset show` (§14.6) | Manager, Lead | no | no |
| `cpd preset apply` (§14.6) | Manager | no | no |
| `cpd context curation` (§3.15, internal) | none | yes | yes |
| `cpd context retire` (§3.16, internal) | none | yes | yes |
| `cpd context docs` (§3.17, internal) | none | yes | yes |
| `cpd pr reply` (§9.8a) | Lead | no | no |
| `cpd pr post` (§9.8b) | Lead | no | no |
| `cpd session switch-failed` (§10.18a, internal) | none | no | yes |
| `cpd session switched` (§10.18b, internal) | none | no | yes |
| `cpd tag` (§26.1) | Lead | no | no |
| `cpd version sync` (§26.2) | Lead | no | no |
| `cpd resource list` (§30) | Manager, Lead, Worker | no | no |
| `cpd resource run` (§30) | Manager, Lead, Worker | no | no |
| `cpd context show` (§3.3) | Manager, Lead, Worker | no | no |
| `cpd context search` (§3.4) | Manager, Lead, Worker | no | no |

`context show` and `context search` gain `Worker` (§3.3, §3.4; Plan `explore-roles`): an explore Worker reads the Context with them, read-only. `context new` gains `--owner` (§3.1) and `pr add`, `plan new`, `pr open`, `pr push` and `pr rebase` read the policies (§9.4, §4.1, §9.1, §9.9, §9.3); their entries are unchanged. `job add` and `job edit` gain `--kind` (§5.1, §5.2) and `pr add` gains `--shape` (§9.4), hs-d340 and hs-d341.

## 3. `context`

A Context and its records (§2.2, §4.6). The records live only in the database and Contexts are machine-local (hs-a1). Every record write follows §4.6 exactly: one transaction that validates, inserts the `context_records` row and appends `context-record` `{record, kind, machine, seq}`. No lock file, no git, no file.

**Record ids.** `<machine>-<letter><n>` with `n` = 1 + the highest `n` already in this Context's records for this machine and letter (§2.1). The write transaction makes it race-free; only this machine writes its letter sequence.

**Retries.** A record write is recognised as a retry and returns the existing record with `"already": true`, when this machine's latest record in the Context has the same `kind`, `text`, `supersedes`, `question` and `by.session`. Any other write appends a new record; records are never edited.

### 3.1 `context new`

```
cpd context new <id> --title <text> --owner <login> [--jira <key>]
```

| Flag | Meaning |
|---|---|
| `<id>` | Context id (§2.1) |
| `--title <text>` | required; 1–200 characters, no control characters, else `usage` (§1.5) |
| `--owner <login>` | required: the GitHub owner (user or organisation) of every repo the Context's Plans work on, its policy key (contract §8.3, hs-d288); a login of contract §2.1, else `usage`; stored lowercased |
| `--confirm-fallback` | optional: if the owner is unknown, create `policy/<login>/owner.toml` as a relative symlink to `../clayton-duarte/owner.toml` and proceed; if the owner is already known, the flag changes nothing (idempotent); if `policy/clayton-duarte/owner.toml` does not exist, fail `fallback_missing`. Without the flag, an unknown owner fails `owner_unconfirmed` |
| `--jira <key>` | Jira Epic key, stored only (`^[A-Z][A-Z0-9]+-[0-9]+$`, else `usage`) |

| Precondition | Code | Exit |
|---|---|---|
| no Context row with this id, unless it has the same title, Jira key and owner | `context_exists` | 3 |
| the owner has a policy file, `$CPD_HOME/policy/<owner>/owner.toml` (contract §8.3; an empty file is the default policy), else with `--confirm-fallback` create it as a relative symlink to `../clayton-duarte/owner.toml`; without the flag, `owner_unconfirmed` if the owner is unknown (details: `owner`, `file`, `fallback`, `flag`); `fallback_missing` if the fallback owner's file does not exist; `details.owner` and `details.file` | `owner_unconfirmed` (1), `fallback_missing` (1) |
| that file, read as every resolve reads it, is valid; `details.file`, `details.key` | `policy_invalid` | 1 |

Effects:
- **Rows**, one transaction: `contexts` (`created_on` = `machine`, `owner`). **Event** `context-created` `{title, jira, created_on}`. Nothing else is written: no directory, no git.
- **tmux:** none.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null, "owner": "clayton-duarte", "created_at": "2026-10-14T21:10:00.000Z", "created_on": "hs"}}
```

Idempotent: yes. The same id, title, Jira key and owner return `"already": true`.

### 3.2 `context list`

```
cpd context list
```

No flags, no preconditions, no effects (one read transaction).

```json
{"ok": true, "contexts": [
  {"id": "building-cpd", "title": "Building cpd", "jira": null, "owner": "clayton-duarte", "archived_at": null,
   "records": 57, "open_questions": 2,
   "plans": [{"id": "cpd2", "title": "cpd in Rust", "phase": "dispatch", "lead_session": "596d007f-…", "archived_at": null}]}],
 "events_cursor": 48213}
```

`owner` is the GitHub login of the Context's owner (contract §2.2), or null for a Context created before the migration. `archived_at` (here and on each Plan) is null, or the time the object was archived (contract §2.2 "Archived"). Archived Contexts and Plans are listed too. `events_cursor` is the highest Event id visible in the same read transaction (§17). Idempotent: read-only.

### 3.3 `context show`

```
cpd context show [--context <id>] [--since <seq>] [--record <id>] [--topic <t>]
```

| Flag | Meaning |
|---|---|
| `--since <seq>` | only records whose index position `seq` is greater (exclusive, like `events --since`); what a Manager's turn-start update holds (§15.7); a Lead passes the `seq` it last read |
| `--record <id>` | print that one record in full (B3-A4): `records` has only it, `markdown` renders only it with its whole `text`. Exclusive with `--since` (`usage`). A record not in the Context: `record_not_found` (exit 3, §4.6) |
| `--topic <t>` | print every curated record in force of that topic, one per line `- [<id>] <short>` (contract §4.6, hs-d549, hs-d551), below. Exclusive with `--since` and `--record` (`usage`) |

**Callers** (Plan `explore-roles`, R10): the Lead and the Manager, as before and the **Worker**. An explore Worker (`scout` or `research`, §5.1) may run `context show` read-only, as its Spec's Deliverable section says; nothing enforces it (§2: anyone may run any command). Every other Worker's rule stays the Spec-only rule of the skill.

Effects: none. Output: the Context, its records in index order and `markdown`, the rendering of §2.2 "Context record" (title and Jira key; Plans; active decisions; superseded decisions with their successor; open questions; answered questions with their answers; active notes; superseded notes with their successor, labelled as a superseded decision is; a record retired by an approved retirement (contract §4.6 "Retirement", hs-d549, hs-d551) is labelled `retired (<reason> <target>)`, in the section of its kind, as a superseded one is labelled with its successor and is in no digest; each item with its id and date; briefs under their Plan, see below). With `--since`, `markdown` renders only those records, in the same order of sections.

**Briefs** (B3-A4): a record of kind `brief` is listed **short**, in `markdown` and in `records`: its id and the first line of its `text`, under its Plan (the Plan's section), never in full. In `records` its `text` is that first line and the entry has `"short": true`; its `plan` is `{id, machine}`. The full text is `cpd context show --record <id>`. `--record` is the only way to get a brief whole.

**Plans** (R34, A12): each `plan` record is listed with the repos of its Plan's PRs that are not `cancelled`, read from this machine's database (`prs.repo`) in the same read transaction, each repo once, in PR key order; a Plan of another machine, or one with no PR yet, shows none. The record itself carries no repo (§2.2; a `repo` in a record written before R34 was dropped by migration 0007's import, contract §4.5). In `records`, a `plan` record's `plan` is `{id, machine, jira}` and the entry has `repos`, the list just described.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null},
 "records": [{"seq": 42, "id": "hs-d7", "kind": "decision", "at": "2026-10-14T21:12:00.000Z", "machine": "hs",
              "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "One engine per machine.", "supersedes": "hs-d2"}],
 "markdown": "# Building cpd\n\n## Plans\n…"}
```

Idempotent: read-only.



**`--topic <t>`** (FX5 of hs-d549, hs-d551): prints every curated record in force of the topic `<t>` (the exact topic string of `record_curations`), oldest first, one per line `- [<id>] <short>`; the way to read what the digest leaves out. `markdown` is those lines and `records` the list `{id, short}`. FX5: an unknown topic (no curated record in force has that topic) is refused `topic_not_found` (exit 4, `details: {context, topic, topics}`, the topics in use). Read-only.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null}, "topic": "engine",
 "records": [{"id": "hs-d7", "short": "one engine per machine"}],
 "markdown": "- [hs-d7] one engine per machine"}
```

### 3.4 `context search`

```
cpd context search <query> [--context <id>]
```

Searches the Context's curated records by topic and short form against the query string (hs-d655: on-demand retrieval). The Context is `--context`, else the selected Plan's (§1.3).

| Precondition | Code | Exit |
|---|---|---|
| a Context was selected (§1.3) | `context_required` | 2 |
| the Context exists | `context_not_found` | 4 |

| Flag | Meaning |
|---|---|
| `<query>` | search string to match against topics and short forms of curated records. Punctuation in the query is text, not FTS5 syntax; the operators kept and the rewrite are in docs/research/context-holographic.md `## Search contract`. |

**Callers** (Plan `explore-roles`, R10): as `context show` (§3.3): the Lead, the Manager and the **Worker**. An explore Worker (`scout` or `research`) may run it read-only, as its Spec's Deliverable section says; nothing enforces it (§2).

Effects: none. Output: records with curated topics or short forms matching the query, in topic order, then index order. `markdown` renders results; `records` is the list of matching `{id, topic, short}` entries.

Idempotent: read-only.

### 3.5 `context decide`, `context note`, `context ask`

```
cpd context decide <text> [--supersedes <record>] [--context <id>]
cpd context note <text> [--supersedes <record>] [--context <id>]
cpd context ask <question> [--context <id>]
```

| Flag | Meaning |
|---|---|
| `<text>`, `<question>` | the record's `text` (§1.5) |
| `--supersedes <record>` | `decide` and `note`: the id of the earlier record this one replaces (recency wins, contract §2.2). A decision may replace a decision or a note of the Context; a note may replace a note only |

| Precondition | Code | Exit |
|---|---|---|
| `--supersedes` names a record of this Context | `record_not_found` | 3 |
| `--supersedes` on a `note` names a record that is not a `note`, or on a `decide` names a record that is neither a `decision` nor a `note` | `supersedes_invalid` | 3 |

`supersedes_invalid` has `details: {record, kind, allowed}`: the id named, its kind and the kinds the writing record may supersede (`["decision", "note"]` for `decide`, `["note"]` for `note`). Nothing is written. A record superseded by a decision or note leaves force and every digest, stays readable and `context show` labels it superseded, naming its successor (§3.3).

Effects: the record write above (one transaction), with kind `decision` (letter `d`), `note` (`n`) or `question` (`q`); `by` = `{session: CPD_SESSION or PI_SESSION_ID or null, plan: the selected Plan or null}`. **Event** `context-record`. No tmux.

```json
{"ok": true, "record": {"id": "hs-d8", "kind": "decision", "at": "2026-10-14T21:30:00.000Z", "machine": "hs", "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "…", "supersedes": "hs-d2"}, "seq": 58}
```

Idempotent: a retry is recognised (above); otherwise each call inserts a record, by design (records are never edited).

### 3.6 `context answer`

```
cpd context answer <question> <text> [--context <id>]
```

`<question>` is the record id of the question.

| Precondition | Code | Exit |
|---|---|---|
| `<question>` is a record of this Context of kind `question` | `record_not_found` | 3 |
| no answer in this Context's records names it yet | `question_answered` | 3 |

Effects: the record write with kind `answer` (letter `a`) and `question` = `<question>`; **Event** `context-record`.

```json
{"ok": true, "record": {"id": "hs-a2", "kind": "answer", "at": "…", "machine": "hs", "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "Yes, per machine.", "question": "hs-q3"}, "seq": 59}
```

Idempotent: a retry is recognised; a second, different answer is refused. Contexts are machine-local, so only this machine answers.

### 3.7 `context sync`

Removed by Plan manager B1 (hs-a1): Contexts are machine-local; records live in the database. The command is now unknown (`usage`, exit 2); engine E6 is removed with it (contract §3.7).

### 3.8 `context rename`

```
cpd context rename <id> --title <text>
```

Changes the Context's visible label (contract §2.2: ids are immutable, titles editable). `--title` is required, as `context new`'s: 1–200 characters, no control characters, else `usage` (§1.5).

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |

Effects: an unchanged title: `"already": true`, nothing written. Otherwise **rows**, one transaction: `contexts.title`; **Event** `context-renamed` `{title}`. Only the database changes.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd 2"}}
```

Idempotent: yes.

### 3.9 `context archive`

```
cpd context archive <id>
```

Archives the Context with its Plans, all their Sessions and its Manager (contract §2.2 "Archived").

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| no object of the cascade is live: no Session `working`, `waiting` or `blocked`, no Job `queued`, `running` or `draft` (§4.10, hs-d731), no open Escalation, no pending Plan review and no PR of a Plan of the Context that a PR of a Plan outside it names as base PR or need (a Context's Plans archive together; contract §2.2 "Archived"); `details.blockers` lists each as `{kind, id, reason}` | `archive_blocked` | 3 |
| no Lead of the cascade is in the chat slot; `details.next` = `cpd session park` | `in_chat_slot` | 3 |

Effects, in this order (all or nothing):
1. **Check** every object of the cascade. If any blocks, nothing is written and nothing is closed.
2. **tmux:** close every live Lead pane exactly as `session close` does (§10.9): `kill-pane`, `state = stopped`, `tmux_pane = NULL`, with its `session-state` and `session-moved` Events. pi stops; the session files stay.
3. **Rows**, one transaction that re-checks the rule first (`archive_blocked` if something became live meanwhile): `archived_at` = now on the Context, on each of its Plans and on each Session of the cascade, that is not archived yet. One **Event** per object changed: `context-archived` `{archived: true}`, `plan-archived`, `session-archived`.

```json
{"ok": true, "archived": [{"kind": "context", "id": "building-cpd"}, {"kind": "plan", "id": "cpd2"}, {"kind": "session", "id": "596d007f-…"}], "closed": ["%41"]}
```

`closed` lists the panes closed in step 2. Idempotent: yes; a Context that is archived already (and whose cascade has nothing to archive) returns `"already": true`, writes nothing and closes nothing.

### 3.10 `context unarchive`

```
cpd context unarchive <id>
```

Clears `archived_at` on the Context and, with the same cascade as `context archive`, on its Plans and Sessions.

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |

Effects: **rows**, one transaction: `archived_at = NULL` on the Context and every archived Plan and Session of the cascade; one **Event** per object changed (`context-archived`, `plan-archived`, `session-archived`, each with `{archived: false}`). No tmux: no pane is touched and `session open` resumes a Lead later (§10.4). No archive rule applies.

```json
{"ok": true, "archived": [{"kind": "context", "id": "building-cpd"}, {"kind": "plan", "id": "cpd2"}, {"kind": "session", "id": "596d007f-…"}], "closed": []}
```

`archived` lists the objects changed (their `archived_at` is now null); `closed` is always empty. Idempotent: yes; nothing archived: `"already": true`, nothing written.

### 3.11 `context manager`

```
cpd context manager [--context <c>] [--by-user]
```

| Flag | Meaning |
|---|---|
| `--by-user` | (hs-d542 A2) when registering a new Manager, stores the flag on the Session and writes no first message (§1.4); `plan new` and `context manager` use it (§4.1); omit it and the Manager starts with its first message, as today |

Gets the Context's **Manager** (B3-A1, B3-A2; CONTEXT.md "Manager"): registers and starts it when the Context has none, starts it again when it is stopped. The Context is `--context`, else the selected Plan's (§1.3).

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| the Context is not archived; `details.next` = `cpd context unarchive <c>` | `archived` | 3 |
| the cockpit's tmux session exists when a pane must be started; `details.next` = `cpd tui` | `no_cockpit` | 3 |

Effects, by the state of the Context's live (not archived) Manager Session:
- **None:** **rows**, one transaction: `sessions` (new UUID v4; `role = manager`; `context`; `plan` null; `name` = `<context>-manager`; `cwd` = the realpath of the caller's cwd; `tier` = config `manager_tier`, default `O5`; `runner` = config `manager_runner`, default `pi`, contract §6.19; `forked_from` null; `by_user` = 1 when `--by-user`, else 0; `stopped`); **Event** `session-registered` `{role, name, forked_from, tier}`. Then **tmux**, as `plan new` starts a Lead (§4.1 step 4): `new-window -d -t =cpd: -n ctx-<context> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -P -F '#{window_id} #{pane_id}' -- cpd session run <session>`, `@cpd_session` on the pane (no `CPD_PLAN`); one transaction: `sessions.tmux_pane`, **Event** `session-moved` `{pane, window, place: "parked"}`. When `by_user` is 1, no first message is written; when it is 0, its first message is written to `run/<context>/manager-message.md` (§1.4): `You are the Manager of Context <c> "<title>". Follow the cpd-manager skill.`, then the Context as `plan new` gives it (§4.1 step 3; contract §4.6 "Consumers", the same function): the digest and at most 20 uncurated records raw. **For a durable Manager** (`runner` `durable`, contract §6.19) the tmux step and its `session-moved` are skipped: the new Session is left live and `stopped` and the engine's hosts step starts its host at the next tick, with a null `pane`/`window` in the output.
- **`stopped` with no pane:** the tmux step alone, in the stored `cwd`: `session run` resumes pi on its session file (`pi --session <file>`, §6.5), with no first message. A durable Manager is started by the engine's hosts step instead, not here.
- **Otherwise** (it has a pane): nothing.

The command never moves the chat slot (hs-d103); `session open <manager>` does (§10.4).

```json
{"ok": true, "manager": {"session": "3c9a…", "name": "building-cpd-manager", "state": "stopped", "pane": "%47", "window": "@21"}, "started": true}
```

`started` is true when this call registered the Manager or started its pane. Idempotent: yes; a Manager with a pane returns `"started": false` and writes nothing.

### 3.12 `context pending`

```
cpd context pending [--context <c>] [--markdown]
```

The in-flight work of a Context (hs-d159, P1; CONTEXT.md "Pending work"): what its active Plans have started and not yet landed on the base. The Manager runs it before starting or routing work for an issue, to see whether something already fixes or changes it. The Context is `--context`, else the selected Plan's (§1.3).

| Flag | Meaning |
|---|---|
| `--markdown` | also print the result as markdown, instead of JSON |

| Precondition | Code | Exit |
|---|---|---|
| a Context was selected (§1.3) | `context_required` | 2 |
| the Context exists | `context_not_found` | 4 |

An archived Context is not refused: it has no active Plan to list unless one was left active.

Effects: none. One read transaction, no write, no subprocess (no `git`, no `gh`): everything comes from the database, so the GitHub state is the last one `pr poll` stored.

Output: `plans`, every Plan of the Context that is not `done` and not archived, in creation order, each with `id`, `title`, `phase`, `lead_session` and `prs`: its PRs that are not `merged` (cancelled ones included), in key order, each with `key`, `repo`, `base`, `status`, `lead_branch`, `pull_request` (`{number, url, state}` as stored by `pr poll`, or null before the PR is opened), `waiting_on` (hs-d521: blockers of this PR, absent when none) and `jobs`: the PR's Jobs that are not `cancelled` and not yet on the base, in `seq` order, with status `draft`, `queued`, `running`, `failed`, `merged` or `waiting` (hs-d523: display only, blocked by a dependency or PR that has not started). Here `merged` means on the Lead Branch and not yet on the base (§2.2 Job). Each Job has `key`, `status`, `title`, `touches` and `waiting_on` (hs-d521: the blocker keys, absent when none or status is not `waiting`).

```json
{"ok": true, "context": "building-cpd",
 "plans": [{"id": "pay-4521", "title": "Retry failed payments", "phase": "dispatch", "lead_session": "596d007f-…",
            "prs": [{"key": "api", "repo": "/home/clayton/projects/api", "base": "main", "status": "dispatch",
                     "lead_branch": "pay-4521/api", "pull_request": {"number": 212, "url": "https://github.com/acme/api/pull/212", "state": "OPEN"},
                     "jobs": [{"key": "retry-core", "status": "merged", "title": "Retry policy", "touches": ["src/retry.rs"]},
                              {"key": "retry-cli", "status": "waiting", "title": "Retry flag", "touches": ["src/cli.rs", "docs/commands.md"], "waiting_on": ["retry-core"]}]},
                    {"key": "old", "repo": "/home/clayton/projects/api", "base": "main", "status": "cancelled", "pull_request": null,
                     "waiting_on": [],
                     "jobs": []}]}]}
```

With `--markdown` the output is the same data as markdown: `# Pending work in <context>`, then per Plan `## <id> (<phase>): <title>`, per PR `### PR <key> (<repo>, base <base>): <status>` with its pull request (`#<n> <state> <url>`) or `no pull request` and when waiting, `waiting on <keys>` (§hs-d521), then one line per Job `- <key> <status>: <title> (<touches, comma-separated>)` or `- <key> waiting on <keys>: <title> (…)`. A Context with no active Plan prints `No pending work.`

Idempotent: yes; it only reads.

### 3.13 `context distill`

```
cpd context distill [--context <c>] [--batch <n>]
```

Curates one batch of the Context's records (contract §4.6 "The distiller curates" and "The distill run", hs-d549, hs-d551, replacing the digest-writing run of hs-d194 D1–D5; GXD1-GXD2 hs-d563: per-line acceptance, rejected lines stay uncurated): at most **N uncurated records** in force (FX4: N is `--batch` from 1 to 60, default 60, at most 49152 bytes of their text in batch.md, hs-d549, hs-d551) get a topic and a short form (for machines) and the run may propose retirements and flag documentation candidates, written by one-shot pi and **checked structurally by cpd**, with each line validated independently so invalid lines do not fail the whole output (GXD1). The curations are read by `context show --topic` for discovery and retrieval. Engine E9 starts it detached when the Context has uncurated records in force and the newest of them is 10 minutes old, or 20 or more exist (contract §3.7); one batch per run, so a backlog (751 records for `building-cpd`) is curated by one run per tick. When a run for a state (the highest uncurated seq and count) fails (distill_invalid only when no line passes, distill_failed after the whole ladder, or a crash), E9 starts the next run for the same state with the next smaller batch (60 → 30 → 15 → 5 → 1, FX4) and records the batch size with the state; after batch 1 fails, it stops until the state changes (a success curates at least one record, GXD1, or a new record) or the engine restarts (contract §3.7 E9). Anyone may run it on demand and it then runs in the caller's process and waits for pi. The Context is `--context`, else the selected Plan's (§1.3). Its §2 row is added by the Job that implements it, as §10.9's was, so that `every_documented_command_exists` holds on every commit.

| Precondition | Code | Exit |
|---|---|---|
| a Context was selected (§1.3) | `context_required` | 2 |
| the Context exists | `context_not_found` | 4 |
| no other distill run of this Context is alive: `locks/distill-<context>.lock` is free (taken without waiting, held until the run ends) | `distill_running` | 3 |

An archived Context is not refused; engine E9 never starts one for it.

| Flag | Meaning |
|---|---|
| `--batch <n>` | batch size from 1 to 60, default 60 (FX4 of hs-d549, hs-d551; the batch is at most `n` records and at most 49152 bytes of their text, contract §4.6 step 2) |

Effects (contract §4.6 "The distill run", steps 1–5, GXD1-GXD4 hs-d563):
- **Nothing to do**, `"already": true` and nothing written: no record in force is uncurated.
- **Skipped when not curable** (R4 of hs-d592): when the Context is archived, deleted, or under `context_curation_min_records` (§8.2), the command exits 0 before taking the lock, writes nothing, creates no Event and its JSON data is `{"context": "<c>", "skipped": "archived"}` for an archived Context, or `{"context": "<c>", "skipped": "young", "records": <n>, "floor": <N>}` for one under the floor (archived is checked first). This makes engine E9's runs skip non-curan Contexts without process, lock or Event (contract §3.7 R5, R8).
- **Otherwise**: the files `prompt.md`, `batch.md` (at most N uncurated records in force, oldest first, in full, where N is the batch size), `known.md` (the topics in use, then `[<id>] <topic> | <short>` for every curated record in force) and `docs.md` (for each batch record, the `<repo>:<path>` hits of `git -C <repo> grep -l -w <id> -- docs '*.md'` on each repo of the Context's Plans' PRs, run by cpd), then the distiller, `pi --mode json ...` at `context_distill_tier` (default `H0`; hs-d560, contract §4.6 step 4: a distill run has no ladder, one step only; contract §6.5 "Distiller", §8.2), JSON mode, read by cpd, killed after `timeouts.distill_minutes`. GXD3: when pi's stop reason is `length`, cpd keeps the text up to the last complete line (drops incomplete trailing line if any) and notes `cut: true`. **GXD1-GXD2 Per-line acceptance**: every line is validated independently; lines that pass stay curated, lines that fail are rejected (their record stays uncurated); the output lists all attempted lines with a `rejected` array (GXD2) of failures. One transaction calls `propose` with only the passing lines; the run succeeds when at least one line passes.
- **The checks** (contract §4.6 step 5, GXD1-GXD2 hs-d563): each line is validated independently; failures are collected (GXD2: listed in `rejected` array), not fatal to the whole output. For each line: valid UTF-8 JSON of the output shape; `id` in the batch (GXD1: missing records stay uncurated; id not in batch is ignored; id appearing twice means both lines rejected); field bounds (`topic` 1 to 40 characters matching `[a-z0-9-]{1,40}`, `short` 1 to 120 bytes with GXD4 100-byte rule hs-d563, `reason` one of three); `duplicate` target is a record in force not the record itself with seq < record's seq; `superseded` target in force not the record itself with seq > record's seq; `documented` target is one of that record's paths in `docs.md`. GXD4: prompt examples have short of 90–100 bytes, never say only `correction to <id>`. No repair turn, no `## Contradictions`, no cite-every-decision: removed (hs-d549, hs-d551).
- **Valid run** (GXD1: at least one line passes): one transaction calls `propose`: for each passing line, the curation and retirement proposal; for doc candidates the `doc_candidate` `pending` with hits as `doc_paths`; then appends the **Event** `context-curated` `{context, curated, rejected, retire, docs}` (contract §5.2; GXD2: `rejected` the count of rejected lines, 0 when all passed). The output carries `rejected: [...]` and when any line was rejected or output was cut, raw text kept as `<upto_seq>.jsonl.invalid`. The proposals take effect only when the User decides them (Inbox items `curate-retire` and `curate-docs`, §3.15–§3.17). **Invalid run** (no line passes or output could not be parsed): the last output is kept as `run/context-digests/<context>/<upto_seq>.jsonl.invalid` (one per Context, replacing the previous; a valid run removes it) and the run fails (hs-d560: no ladder, no `distill-tier` item).
- **Run log**: every run that started pi appends a line to `run/context-digests/<context>/runs.jsonl` (`started_at`, `seconds`, `model`, `tier` (the Tier of that pi run, `context_distill_tier`, hs-d560), `tokens_in`, `tokens_out`, `cost`, `context`, `upto_seq`, `result`, `kind`, always `distill`; contract §4.6 "The run log"; GXD2: when any line was rejected or output was cut, `<upto_seq>.jsonl.invalid` is kept), also when it failed.
- **tmux:** none. **git:** read-only (`git grep`).

Failures, after the lock is taken. Each appends **one** `error` Event `{source: "context distill", code, message, details}` with no `context`, `plan` or `session` set (`details.context` names it; so no Lead gets a turn, contract §5.3), changes no curation and returns the same code:

| Failure | Code | Exit | `details` |
|---|---|---|---|
| pi could not start, exited non-zero (an error, or an answer cut at the output limit), ran longer than `timeouts.distill_minutes`, or its JSON stream had no final assistant text (GXD3: length with no complete line fails as distill_failed) | `distill_failed` | 1 | `context`, `upto_seq`, `reason` (`spawn`, `exit`, `timeout` or `output`), `exit` (when it exited), `tail` (pi's last 60 stderr lines), `log` |
| the output fails any check above, after the last Tier of the ladder (GXD1: no line passes; GXD2: rejected lines listed in output; GXD3: output cut at length with no complete line) | `distill_invalid` | 1 | `context`, `upto_seq`, `errors` (every failure, each `{id?, line?, what}`; GXD2: rejected lines, each with id, line number from 1 and reason), `invalid` (the kept `.jsonl.invalid` file), `log` |

`log` is `run/context-digests/<context>/distill.log`, where the run writes its stderr (and, started by E9, its stdout too).

```json
{"ok": true, "context": "building-cpd", "distilled": true, "steps": ["H0"],
 "curated": 59, "rejected": 1, "retire": 4, "docs": 2, "remaining": 692,
 "run": {"seconds": 94.2, "model": "anthropic/claude-sonnet-4-5:medium", "tokens_in": 61234, "tokens_out": 7410, "cost": 0.43}}
```

`curated`, `rejected` (GXD2) and `docs` are the counts of the `context-curated` Event (contract §5.2); `retire` is the count of retirements proposed; `remaining` the records in force still uncurated. `steps` is present with `run` (hs-d538): the Tiers tried in order, e.g. `["H0", "H1"]`; `run` is that of the **last** step. `run` is present only when pi ran (`"distilled": true`); its fields are the run's `runs.jsonl` line without `started_at`, `context`, `upto_seq`, `result` and `kind` and `tokens_in`, `tokens_out` and `cost` may be `null`. On a failure the same figures are in the log line only, not in the Event.

Idempotent: yes. With no uncurated record it returns `"already": true` and runs nothing; a rerun after a failure runs the same batch again (on demand; E9 does not retry the same highest `seq`, contract §3.7).

### 3.14 `context owner`

```
cpd context owner <login> [--context <c>]
```

Sets a missing owner or changes the owner (contract §8.3, hs-d288): a Context created before `context new --owner` existed may have no owner, which the migration could not fill because its PRs had no pull request or pull requests of two owners (contract §4.5); a Context with an owner may change it, but only while no Plan of the Context has a PR row (PR rows are rows of `prs` of Plans not deleted, in any status). The Context is `--context`, else the selected Plan's (§1.3).

| Flag | Meaning |
|---|---|
| `<login>` | the owner, a login of contract §2.1, else `usage`; stored lowercased |
| `--confirm-fallback` | optional: if the owner is unknown, create `policy/<login>/owner.toml` as a relative symlink to `../clayton-duarte/owner.toml` and proceed; if the owner is already known, the flag changes nothing (idempotent); if `policy/clayton-duarte/owner.toml` does not exist, fail `fallback_missing`. Without the flag, an unknown owner fails `owner_unconfirmed` |

| Precondition | Code | Exit |
|---|---|---|
| a Context was selected (§1.3) | `context_required` | 2 |
| the Context exists | `context_not_found` | 4 |
| the same owner again (no write, no Event) | `already` | 0 |
| the Context has an owner other than this one and a PR row exists; details: `context`, `owner` (the current owner), `prs` (the PR rows as `<plan>/<key>`, sorted) | `owner_in_use` | 3 |
| the owner has a policy file, `$CPD_HOME/policy/<owner>/owner.toml`, else with `--confirm-fallback` create it as a relative symlink to `../clayton-duarte/owner.toml`; without the flag, `owner_unconfirmed` if the owner is unknown (details: `owner`, `file`, `fallback`, `flag`); `fallback_missing` if the fallback owner's file does not exist; `details.owner` and `details.file` | `owner_unconfirmed` (1), `fallback_missing` (1) |
| that file is valid; `details.file`, `details.key` | `policy_invalid` | 1 |

An archived Context is not refused: changing its owner changes no work.

Effects: **rows and Events**, one transaction with the checks and the UPDATE on `contexts.owner`. One `context-owner` Event `{owner}` appended for each change; none for `already`. Nothing else: the Context's existing PRs keep their `name` and from now on resolve `Repo(<owner>, <name>)` (or `Owner(<owner>)` while their `name` is null, contract §8.3). Sessions already started keep the policy text they were given for the old owner until they restart (contract §8.3 'Delivery to agents').

```json
{"ok": true, "context": {"id": "building-cpd", "owner": "clayton-duarte"}, "already": false}
```

Idempotent: yes; the same owner again returns `"already": true` and writes nothing.

### 3.15 `context delete`

```
cpd context delete <id> [--yes]
```

Deletes a Context: its Plans' rows, its records, its Manager and its run files, worktrees and local branches (hs-d527). The `contexts` row and the row of each of its Plans, stay as **tombstones** (`deleted_at` set, once only) and so do the Leads' Session rows, which the tombstoned Plans reference: all hidden from every list, view and lookup (contract §2.2 "Deleted", which has the design). Deletion is permanent and cannot be undone. Without `--yes` the command prints what would be deleted (a preview); with `--yes` it deletes.

| Flag | Meaning |
|---|
| `<id>` | Context id (§2.1) |
| `--yes` | perform the delete; without it, print the preview only |

| Precondition | Code | Exit |
|---|---|---|
| the Context exists and is not deleted; a deleted one is `already` (below) for this command and `context_not_found` for every other | `context_not_found` | 4 |
| **archived or asleep**: the Context is archived, or every one of its Plans is asleep or archived; `details: {blockers: [{kind, id}]}` lists what is not | `archive_blocked` | 3 |
| **nothing running**: no Job `queued` or `running`, no Attempt not `ended`, no Session `working`, `waiting` or `blocked`, every Escalation closed, no Plan review pending; `details.blockers` | `archive_blocked` | 3 |
| **no open PR**: every PR of every Plan is `merged` or `cancelled`; `details.blockers` | `archive_blocked` | 3 |
| **Manager not in chat**: the Context's Manager (§3.10) is not in the chat slot, as `context archive` requires; `details.next = cpd session park` | `in_chat_slot` | 3 |

Plans already deleted are not checked. Effects, with `--yes` only (**without it, no rows or files are written**):
1. **One transaction.** It re-checks the preconditions (race safety), then sets `deleted_at = now()` **first**, on every Plan not yet deleted and then on the Context (the triggers that guard the deletes read it, contract §4.4; a set `deleted_at` is never changed or cleared). Then it deletes the rows, each through the exception for a deleted Plan or Context and in an order that keeps the foreign keys whole:
   - The plannotator reviews and board posts of every Session of the Context (the Manager's and the Leads' included)
   - Every Inbox item of the Context, then every Event of the Context (those of its Plans, Plans deleted earlier included and those that have no Plan); Inbox items go first because they point to Events
   - For each Plan: its Escalations, Attempts, Dependencies, Jobs; its PR needs, PRs, Plan reviews; every Session of it but the Lead
   - Every `record_retirements` and `record_curations` row of the Context (hs-d549; they point to the records, so they go first), then every `context_records` row of the Context (a Plan delete keeps them; `context_records_immutable` still refuses every update)
   - The Manager Session
   - The Leads' Session rows stay (each tombstoned `plans.lead_session` references one), hidden with their Plan.
   Every `DELETE` writes a `history` row with the whole old row (§4.4); the tombstones are `update` rows. They stay and the tamper check stays clean (contract §2.2). Last, in the same transaction, **one `context-deleted` Event** is appended, with the counts; it is never deleted by this delete.
2. **Session files** (after commit): every Session's file, the Leads' included; files that do not exist are skipped.
3. **Run files** (after commit): `$CPD_HOME/run/<plan>/` of each Plan and `$CPD_HOME/run/context-digests/<context>/`, with the summaries, renders, reviews, messages, specs, logs and distill artifacts under them.
4. **Worktrees and local branches** (after commit): `rm -rf` every Attempt worktree and PR Lead worktree, each with its `target/` (this, with the run files, is where the disk comes from: the database file does not shrink); then the stored `lead_branch` of each PR and `cpd/<plan>--<attempt>` from their repos (not on the remote), then the directories of step 3. The paths come from the live rows when the transaction commits and on a rerun from their `before` copies in `history`.

**Rerun.** The rows go in one transaction and steps 2–4 after it, so a crash leaves a tombstoned Context with files on disk. A rerun with `--yes` on a deleted Context writes **nothing** to the store (no row, no Event), finishes whatever file step is left (a missing file, directory or branch is not an error) and returns `"already": true`. A preview of a deleted Context returns `"already": true` and what is left on disk, not the counts of the first preview, which only the Event keeps.

Output without `--yes` (preview) or with `--yes` after a delete:

```json
{"ok": true, "preview": true, "already": false, "plans": [{"id": "cpd2", "jobs": 47, "attempts": 121}], "jobs": 47, "attempts": 121, "sessions": 3, "worktrees": 11, "disk_freed_mb": 342}
```

`sessions` counts every Session of the Context, the Leads included (their rows stay, their files go). With `--yes` the first time: `"preview": false`, `"already": false`. Rerun with `--yes`: `"preview": false`, `"already": true` and `worktrees` and `disk_freed_mb` of what this run removed (0 when nothing was left). Preview of a deleted Context: `{"ok": true, "preview": true, "already": true, "left": {"worktrees": 0, "branches": 0, "files": 0}}`.

Idempotent: yes; a Context already deleted returns `"already": true` and writes nothing to the store.

**Tests that assume otherwise change with this design** (hs-d527; for delete-core to have approved): `context_delete.rs` expects no Session with `context = <id>`, but the Leads' rows stay (tombstoned Plans reference them) and `plan_rows` as under `plan delete` counts the Leads and the Plans' `plan-deleted` Events; the Manager and every other Session are what is gone.

### 3.16 `context curation` (internal)

```
cpd context curation [--context <c>] [--limit <n>]
```

Lists what the distill step proposed and the User has not yet decided (contract §4.6 "Retirement"; hs-d549, hs-d551): the pending retirements and the pending doc candidates of the Context. Called by the cockpit when the User clicks a `curate-retire` or `curate-docs` Inbox item (§13.2), to fill the checklist dialog (hs-d568, hs-d569). Internal (§1.1): hidden in clap, as `plan decide` is. An archived Context or one under the floor (`context_curation_min_records`, §8.2) shows no Inbox item, its pending proposals stay pending and the command still works when run by hand (R6 of hs-d592).

| Flag | Meaning |
|---|---|
| `--context <c>` | the Context (§1.3) |
| `--limit <n>` | at most n entries per list (hs-d569 CD4), where n >= 1 (default: no limit; all entries) |

| Precondition | Code | Exit |
|---|---|---|
| `CPD_SESSION` and `CPD_ATTEMPT` are not set in the environment: only the User decides (hs-d551). The cockpit's views and popups carry neither: cpd's tmux client removes `CPD_SESSION` and `CPD_ATTEMPT` along with `TMUX` and `TMUX_PANE` (contract §6.1 "The one place, in Rust"), so even a tmux server started from inside an agent never passes them on and only the panes of a Session (`session run`, contract §6.5) get `CPD_SESSION`, through their `Spawn` | `user_only` | 3 |
| a Context was selected (§1.3) | `context_required` | 2 |
| the Context exists | `context_not_found` | 4 |
| `--limit` is a positive integer (if given) | (parse error in validation) | 2 |

Effects: none. Output: `retire`, the first n `pending` rows of `record_retirements` (oldest first by `proposed_at`, then the record's `seq`); `retire_total`, the count of ALL pending retirements. `docs`, the first n records whose `doc_candidate` is `pending` (oldest first by `curated_at`, then `seq`); `docs_total`, the count of ALL pending doc candidates. Each entry has `record` (the id), `kind` (the record's kind, §2.2), `text` (the record's full text, hs-d569 CD2), `short` (its short form, `record_curations.short`; a proposal is always written with its curation, contract §4.6), `reason` (for retirements) and `target` (for retirements), `paths` (for doc candidates, the doc paths of `record_curations.doc_paths`, `[]` when none).

```json
{"ok": true, "context": "building-cpd", "retire_total": 56, "docs_total": 3,
 "retire": [{"record": "hs-d12", "kind": "decision", "text": "engine: run one per machine", "short": "engine: one per machine", "reason": "superseded", "target": "hs-d77"}],
 "docs": [{"record": "hs-d300", "kind": "decision", "text": "policies: per owner and repo", "short": "policy: owner and repo files", "paths": ["docs/contract.md"]}]}
```

Idempotent: read-only.

### 3.17 `context retire` (internal)

```
cpd context retire [--context <c>] [--approve <id,...>] [--reject <id,...>]
```

Decides pending retirements (contract §4.6 "Retirement"; hs-d549, hs-d551). Called by the cockpit when the User approves the checklist dialog of a `curate-retire` item (§13.2): `--approve` the ticked rows, `--reject` the unticked. Internal (§1.1): hidden in clap. Only the User decides: no agent runs it. An archived Context or one under the floor shows no Inbox item, its pending proposals stay pending and the command still works when run by hand (R6 of hs-d592).

| Flag | Meaning |
|---|---|
| `--context <c>` | the Context (§1.3) |
| `--approve <id,...>` | record ids (§2.1), comma-separated, whose pending retirement becomes `approved`: the record leaves force |
| `--reject <id,...>` | record ids whose pending retirement becomes `rejected`: the record stays in force and is never proposed again |

At least one of the two is required (`usage`).

| Precondition | Code | Exit |
|---|---|---|
| `CPD_SESSION` and `CPD_ATTEMPT` are not set in the environment (as §3.15) | `user_only` | 3 |
| a Context was selected and exists | `context_required` (2), `context_not_found` (4) | |
| no id is in both lists | `args_invalid` | 2 |
| every id has a `pending` retirement in the Context; `details: {record}` names the first that has none (none at all, already decided) | `curation_not_pending` | 3 |

Effects: **one transaction**, all or nothing (a refusal writes nothing). **Rows:** each named `record_retirements` row gets its `status`, `decided_at` and `decided_by` `user`. **Event** (one): `records-retired` `{context, approved: [id], rejected: [id]}` (contract §5.2). A retirement writes no Context record. The Inbox item `curate-retire` follows at the engine's next E8 pass (contract §3.7): its count drops and it closes when none is pending.

```json
{"ok": true, "context": "building-cpd", "approved": ["hs-d12"], "rejected": ["hs-d30"]}
```

Idempotent: no. A rerun finds the rows decided and is refused `curation_not_pending`.

### 3.18 `context docs` (internal)

```
cpd context docs [--context <c>] [--send <id,...>] [--dismiss <id,...>]
```

Decides pending doc candidates (contract §4.6 "Retirement"; hs-d549, hs-d551). Called by the cockpit when the User approves the checklist dialog of a `curate-docs` item (§13.2): `--send` the ticked rows, `--dismiss` the unticked. Internal (§1.1): hidden in clap. Only the User decides: no agent runs it. An archived Context or one under the floor shows no Inbox item, its pending proposals stay pending and the command still works when run by hand (R6 of hs-d592). A dismissed candidate is never proposed again (R9 of hs-d592).

| Flag | Meaning |
|---|---|
| `--context <c>` | the Context (§1.3) |
| `--send <id,...>` | record ids whose `doc_candidate` becomes `sent`; they go to the Context's Manager in **one** board post, below |
| `--dismiss <id,...>` | record ids whose `doc_candidate` becomes `dismissed`; a dismissed candidate is never proposed again |

At least one of the two is required (`usage`).

| Precondition | Code | Exit |
|---|---|---|
| `CPD_SESSION` and `CPD_ATTEMPT` are not set in the environment (as §3.15) | `user_only` | 3 |
| a Context was selected and exists | `context_required` (2), `context_not_found` (4) | |
| no id is in both lists | `args_invalid` | 2 |
| every id has a `pending` doc candidate in the Context; `details: {record}` | `curation_not_pending` | 3 |
| with `--send`: the Context has a live Manager Session (role `manager`, not archived) | `manager_missing` | 3 |

Effects: **one transaction**, all or nothing. **Rows:** each named `record_curations` row gets `doc_candidate` `sent` or `dismissed`. With `--send`, **one board post** (`board post` mechanics, §10a.1, `core/src/board.rs`) to the Context's Manager Session, `from_session` null (the User's side), listing each sent record's id, short form and paths and asking the Manager to plan a docs PR for them (it picks the post up at its next turn start, as every post and no turn starts, hs-d514). The text is cut at the board's limit of 2000 characters: the lines of the records that do not fit are named by id alone, so the post is always written. **Event** (one): `docs-decided` `{context, sent: [id], dismissed: [id]}` (contract §5.2); the board post appends none. With no Manager, `manager_missing` is refused before anything is written.

```json
{"ok": true, "context": "building-cpd", "sent": ["hs-d300"], "dismissed": [], "manager": "a41b…"}
```

Idempotent: no. A rerun finds the candidates decided and is refused `curation_not_pending`.

## 4. `plan`

A Plan (§2.2) and its phases (§3.1). Replaces cpd 1's `workflow` group (D3) and `plan` group.

### 4.1 `plan new`

```
cpd plan new <id> --context <c> --title <text> [--jira <key>] [--name <name>] [--brief-file <file>] [--by-user] [--tier <T>]
```

A Plan has no repo (R34, A12): its PRs are added later with `pr add` (§9.4) and its Lead starts in the caller's cwd, the directory `plan new` runs from (usually `~`; §6.4). `--repo` and `--base` are gone: either is `usage` (exit 2).

| Flag | Meaning |
|---|---|
| `<id>` | Plan id (§2.1) |
| `--context <c>` | required: the Plan's Context (D2) |
| `--title <text>` | required; 1–200 characters, no control characters, else `usage` (§1.5) |
| `--jira <key>` | Jira task key, stored only |
| `--name <name>` | the Lead's pi display name; default `<id>` |
| `--brief-file <file>` | the brief for the Lead (B3-A4): written as a Context record of kind `brief` and put into the Lead's first message in full |
| `--by-user` | (hs-d542 A2) when registering a new Lead, stores the flag on the Session and writes no first message (§1.4); `plan new` and `context manager` use it (§3.10); omit it and the Lead starts with its first message, as today |
| `--tier <T>` | the Lead's Tier (contract §2.2, §9): a Tier in the Tier flag syntax (§5: `O`, `S` or `H` with an effort, or `GX`), checked by the Lead rule (below). Omitted, the Tier is `review_lead_tier` when `--context` is the non-empty `inbox.review_context` (contract §8.2), else `lead_tier` (Effects, step 1) |

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| the Context has an owner (contract §8.3; null only for a Context made before the owner existed); the message names `cpd context owner <login> --context <c>` | `owner_unset` | 3 |
| `--tier` is not valid syntax (§5; `G0`–`G5` included), or an effort the catalog lacks for the Tier's model (the models of the Context, contract §8.3; the catalog is read only when `--tier` is given); `details.tier` and `details.efforts` for an effort | `tier_invalid` | 2 |
| `--tier` is letter G (`GX`) or effort 5, the Lead rule (contract §9); `details` `{tier, allowed: "O0-O4, S0-S4, H0-H4", rule: "hs-d506, hs-d562"}`, message `Tier <T> is not allowed for a Lead: Leads use only the Tiers O0-O4, S0-S4, H0-H4, never effort 5 and never G (hs-d506, hs-d562).` | `tier_not_allowed` | 2 |
| no Plan with this id, unless every flag matches it (then the result is `already`) | `plan_exists` | 3 |
| the Context is not archived; `details.next` = `cpd context unarchive <c>` | `archived` | 3 |

Effects, in this order:
1. **Rows**, one transaction: `sessions` (new UUID v4; `role = lead`; `context`; `plan = <id>`; `name`; `cwd` = the realpath of the caller's cwd; `tier` = the Lead's Tier (the `--tier` flag; else `review_lead_tier` when the Context is the non-empty `inbox.review_context`; else `lead_tier`; contract §2.2); `runner` = config `lead_runner`, default `pi` (contract §6.19); `forked_from` null; `by_user` = 1 when `--by-user`, else 0; `state = stopped`: the Lead is always fresh, never a fork, B3-A3); `plans` (`phase = grounding`, `lead_session` = the new Session); no `prs` row. **Events** `plan-created` `{title, jira, lead_session, forked_from: null, cwd}` and `session-registered` `{role, name, forked_from, tier}`.
2. **Context records:** the record write of §3 (one transaction) with kind `plan` (letter `p`), `text` = the title, `plan` = `{id, machine, jira}` (`jira` omitted when null); **Event** `context-record`. With `--brief-file`, a second record of kind `brief` (letter `b`), `text` = the file's text, `plan` = `{id, machine}`; **Event** `context-record`.
3. **First message**, when `by_user` is 0, written to `run/<id>/lead-message.md` (contract §4.6 "Consumers", hs-d655): a header (`You are the Lead of Plan <id> "<title>" in Context <c>. Phase grounding: follow the cpd-lead skill.`), the full brief (the file's text) if given, then the pointer line:
   ```
   Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>
   ```
   The Lead gets on-demand access to the Context through these commands. The Lead is always fresh (B3-A3): the grounded Lead and its forks are retired (contract §3.1), so there is no fork case. When `by_user` is 1, no file is written (hs-d542 A2).
4. **tmux:** start the server if it is not running (§6.1) and the cockpit session `cpd` if it is missing; then `new-window -d -t =cpd: -n park-<id> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -e CPD_PLAN=<id> -P -F '#{window_id} #{pane_id}' -- cpd session run <session>` and `set -p -t <pane> @cpd_session <session>`. One transaction: `sessions.tmux_pane`; **Event** `session-moved` `{pane, window, place: "parked"}`. If any step after `new-window` fails (`set-option -p @cpd_session`, the row or the Event write), the new window is killed (`kill-window`) before the error is returned (teardown G11). **For a durable Lead** (`runner` `durable`, contract §6.19) this step and its `session-moved` are skipped: no tmux, no pane. The Session is left live and `stopped`, and the engine's hosts step starts its host at the next tick (contract §6.19); the output `lead.pane` and `lead.window` are null.
5. In the pane, `session run` (§10.8) starts a fresh Lead; when `by_user` is 1, it waits for the User's first prompt before starting pi (contract §10.8 step 3, hs-d542 A2). A durable Lead has no pane and no `session run`: its host is the engine's (§6.19).

When the cockpit session is missing, plan new creates it with the new Lead's park window as its first window; cpd tui builds main later.

```json
{"ok": true, "plan": {"id": "pay-4521", "context": "payments", "title": "Retry failed payouts", "jira": null, "phase": "grounding", "lead_session": "8d0e…", "prs": []},
 "lead": {"session": "8d0e…", "name": "pay-4521", "forked_from": null, "cwd": "/home/clayton", "pane": "%41", "window": "@17"},
 "record": "hs-p4"}
```

Idempotent: yes. With the same flags it returns `"already": true` and redoes any step that did not happen: the `plan` record when the Context has none for this Plan, the `brief` record when `--brief-file` is given and the Context has none for this Plan and step 4 when the Lead is `stopped` with no pane (in the stored `cwd`). The caller's cwd is not a flag: a rerun from another directory is still `already` and keeps the stored `cwd`. `--tier` is one of 'the same flags' only when it equals the stored `sessions.tier` of the Plan's Lead session: then the result is `already`, as above; a different Tier is `plan_exists` (exit 3, as for the other flags). The `--tier` checks of the table run first, so a refused Tier is refused whatever the Plan's state. A rerun without `--tier` compares no Tier, whatever the Lead's stored Tier is. Different flags: `plan_exists`. The `--by-user` flag is part of 'the same flags': a rerun with or without it when the Plan exists is `plan_exists` (hs-d542 A1).

### 4.2 `plan list`

```
cpd plan list [--context <id>] [--phase <phase>]
```

`--phase` is one of §3.1's phases. Archived Plans are listed, with `archived_at` (null, or a time). Effects: none. Each Plan lists its PRs (R34) in key order, `cancelled` ones included, with `key`, `repo`, `status`, `pr` (`{number, url, opened_at}` or null) and `pull_request` (hs-d96: its row of contract §2.2 "Pull request", `{repo, number, url, title, draft, state, review_decision, checks, mergeable, merge_state, created_at, merged_at}`, or null before `pr open`).

```json
{"ok": true, "plans": [{"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust", "phase": "dispatch", "lead_session": "596d007f-…", "archived_at": null,
  "counts": {"draft": 0, "queued": 2, "running": 1, "merged": 2, "failed": 0, "cancelled": 0},
  "prs": [{"key": "cpd", "repo": "/home/clayton/projects/cpd", "status": "dispatch", "pr": null, "pull_request": null}]}],
 "events_cursor": 48213}
```

Idempotent: read-only.

### 4.3 `plan show`

```
cpd plan show [--plan <id>]
```

Effects: none; one read transaction, so the result is one consistent snapshot. It is the TUI's snapshot of a Plan (§17).

Output: every field of the Plan (§2.2), `outline` included (hs-d502: the outline object of §4.13, parsed from `plans.outline`, or `null`); `prs` (R34, A14), every PR of the Plan in key order, `cancelled` ones included, each with its fields (§2.2 PR: `key`, `repo`, `base`, `base_pr` (the base PR as `<plan>/<key>`, derived from `base`, or null: hs-d324), `jira` (the PR's Jira ticket, the key or null: pro-d127, B2), `needs` (PR references: a bare key of this Plan, `<plan>/<key>` of another), `status`, `lead_branch`, `lead_worktree`, `pr`, `pull_request` (the row, as in §4.2, `mergeable` and `merge_state` included, or null), `closed_unmerged` (boolean: true iff the closed-unmerged predicate of contract §3.8 holds for the PR; in `plan show` only, `plan list`, `pr poll` and `context pending` are unchanged), `feedback_open` (integer: the number of open feedback items in the row's stored list, contract §2.2 "Feedback item"; null when the list is NULL, never read; pro-d68), `all_merged_at`, `start_failed_at`, `created_at`, `started_at`, `merged_at`) and `jobs`, the keys of its Jobs in `seq` order; `jobs` with each Job's fields (Spec included, `pr`, `review` and `urgent` among them), `needs` and its Attempts (`id`, `number`, `tier`, `origin`, `status`, `outcome`, `cause`, `worker_session`, `started_at`, `ended_at`); `escalations` that are `open`; the latest Plan review; `counts`; `tamper` (the tamper report of §4.3: `{kind: "schema" | "fingerprint" | "history", …}`); `events_cursor`.

```json
{"ok": true,
 "plan": {"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust", "jira": null, "phase": "dispatch", "lead_session": "596d007f-…", "created_at": "…", "locked_at": "…", "done_at": null, "archived_at": null, "outline": null},
 "prs": [{"key": "cpd", "repo": "/home/clayton/projects/cpd", "base": "main", "base_pr": null, "jira": null, "needs": [], "status": "dispatch", "lead_branch": "cpd2/cpd", "lead_worktree": "/home/clayton/projects/cpd.cpd2.cpd",
          "pr": null, "pull_request": null, "all_merged_at": null, "start_failed_at": null,
          "created_at": "…", "started_at": "…", "merged_at": null, "jobs": ["contract-core", "contract-commands"]},
         {"key": "web", "repo": "/home/clayton/projects/web", "base": "main", "base_pr": null, "needs": ["cpd"], "status": "waiting", "lead_branch": "cpd2/web", "lead_worktree": null,
          "pr": null, "pull_request": null, "all_merged_at": null, "start_failed_at": null,
          "created_at": "…", "started_at": null, "merged_at": null, "jobs": ["web-client"]}],
 "jobs": [{"key": "contract-commands", "pr": "cpd", "title": "cpd 2 contract: every command", "status": "running", "tier": "S3", "appended": false, "needs": ["contract-core"], "fingerprint": "9f…", "retry_tier": null,
           "goal": "…", "context": "…", "touches": ["docs/commands.md"], "acceptance": ["…"], "out_of_scope": ["…"],
           "attempts": [{"id": "contract-commands-a1", "number": 1, "tier": "O3", "origin": "ladder", "status": "running", "outcome": null, "cause": null, "worker_session": "c1a2…", "started_at": "…", "ended_at": null}]}],
 "escalations": [{"id": "contract-commands-a1-q1", "attempt": "contract-commands-a1", "question": "…", "asked_at": "…"}],
 "review": {"id": "r2", "status": "approved", "fingerprint": "9f…", "current": true},
 "counts": {"draft": 0, "queued": 0, "running": 1, "merged": 1, "failed": 0, "cancelled": 0},
 "tamper": [], "events_cursor": 48213}
```

An explore Job (`scout` or `research`, §5.1) is in `jobs` like any Job, with `pr` null and its `kind`; `job show` (§5.7) shows its `summary` and `artefact`.

Idempotent: read-only. `tamper` non-empty is not an error (cpd 1 §11 `workflow show`, kept).

### 4.4 `plan phase`

```
cpd plan phase plan [--plan <id>]
```

The only phase moved by hand (cpd 1 §11 `workflow phase`, kept). Any other target is `illegal_transition` (exit 3).

| Precondition | Code | Exit |
|---|---|---|
| the phase is `grounding` (already `plan`: `already`) | `illegal_transition` | 3 |

Effects (B3-A3; no grounded Lead is created, hs-d129): **rows**, one transaction: `plans.phase = plan`; **Event** `plan-phase` `{from: "grounding", to: "plan"}`. No Session, no `context-grounded` Event, no tmux.

```json
{"ok": true, "plan": "pay-4521", "phase": "plan"}
```

Idempotent: yes; in phase `plan` it returns `"already": true` and writes nothing.

### 4.5 `plan render`

```
cpd plan render [--plan <id>]
```

Effects: writes `run/<plan>/plan.md` and `run/<plan>/summary.md` (§1.4), nothing else. Content of `plan.md`, kept from cpd 1 §11 `plan render` and grouped by PR (R34): title, Context and phase; a table of the PRs that are not `merged`, with key, repo, base and needs, each row muted and marked `cancelled` when the PR is `cancelled` (hs-d518), with its `status` and `cancel_reason` or `superseded by <ref>` when the Event carries `superseded_by`; and, when there are two or more, a Mermaid `graph LR` of their needs (cancelled PRs not in the graph, hs-d518); then the section `## Overlaps` (below); then one section per PR that is not `cancelled` (hs-d518), in the order of the needs graph (ties by key), headed `PR <key>`, with its repo, base and needs, a table of its Jobs with key, title, Tier, status, needs and touches, each row muted and marked `cancelled` when the Job is `cancelled` (hs-d518), with its `status` and `cancel_reason` (hs-d518), a Mermaid `graph TD` of its Jobs' Dependencies (cancelled Jobs not in the graph) and one subsection per Job that is not `cancelled` (hs-d518) with its Spec. The Tier column and each Job section's `Tier:` line show the Tier the Job is displayed at (contract §3.4a): the active Attempt's Tier, or `retry_tier` while queued at it after `job retry`, or the latest Attempt's Tier, or the stored `tier`. The `open` Job placeholder keeps `open_tier`. **Waiting display** (hs-d521): a Job or PR table row whose status is `waiting` (display only, hs-d523) shows the blockers as `waiting on <keys>` in its status column or as a separate line (hs-d521); in the Mermaid graph (hs-d518), a cancelled PR is omitted and in the render's graph of needs a cancelled PR appears neither as a node nor in edges; in the per-PR Job graph, cancelled Jobs do not appear. **The open Job's placeholder** (hs-d497): a PR that has not started has no `open` Job yet (engine E4 adds it at the start), so the render shows one for each such PR, as the last row of its Jobs table and as the last Job section: key `open-<pr>`, title `Open the pull request: <PR title>`, Tier `open_tier` (contract §8.2), status `—` (not a Job yet), needs `every other Job of the PR` and, in place of the Spec, `Added by cpd when the PR starts: merges the base only on a conflict, runs the Gate, pushes and opens the pull request ready.` It is not a row, is in no fingerprint and no Mermaid graph and is shown in every phase until the PR starts (once the PR has started the real `open` Job is shown as any Job, with its derived needs, contract §3.2). A Job's section is under its PR only. In a phase after `plan`, the Jobs that are not `draft` are Locked context: the render shows them as such and marks every `draft` Job **under review** except a `draft` `open` Job, which cpd added and the Lead does not review: it is shown as context, never `**under review**` (hs-d573) (in its table row and its section), because a review in these phases covers exactly the drafts (§4.6; User, hs-d173, hs-d174, hs-d179). In phase `plan` every Job is a draft and nothing is marked. A **stub draft** (no Tier or no Spec, §5.1) is shown as it is, visibly incomplete: `Tier: —` in its table row and section when it has no Tier and `Spec: not written yet` in place of its Spec subsection's goal, context, touches, acceptance and out of scope when it has none. GitHub-flavoured Markdown only: tables, code and Mermaid, no footnotes, no raw HTML (t4). It also writes the **Plan summary**, `run/<plan>/summary.md` (below, hs-d502). Output: the path, `summary` (the path of the summary), the plan fingerprint (§2.3: Jobs, PRs and the outline when set), the number of `draft` Jobs (`draft_jobs`, not counting a `draft` `open` Job: it is not under review, hs-d573), the number of PRs that are not `cancelled` and the overlaps.

**Kinds** (hs-d340, hs-d341). A Job's `kind` is a column of its row in the PR's table and a line of its section when it is not `work`; a `test` Job's section lists its `tests` and `test_files` and a Job's section lists its `resources` with the Spec when it has any. The PR's `open` Job, once the PR has started, is a Job of its PR like any other, with its derived needs (contract §3.2); before the start it does not exist, so the review of a Plan shows the template's Jobs and the `open` Job as `plan render`'s placeholder (§4.5, hs-d497), the one every PR will get.

**Overlaps** (B4-A3, hs-d78, hs-d172, hs-n201): the warning that another Plan is working on the same files. It is made here and so in the Plan review (§4.6) and nowhere else: `job add` does not warn. It looks at **open work** only, on both sides: the Jobs that are not `cancelled` (`draft`, `queued`, `running`, `failed`, or `merged` on its Lead Branch) of PRs that are not `merged` and not `cancelled`. A Job of a `merged` PR is on its base already and cannot clash with anyone's work any more, so the Jobs of this Plan's `merged` PRs are not checked. For each PR of this Plan that is not `merged` and not `cancelled`, every **other** active Plan (any Context; its phase is not `done` and it is not archived) is looked at when it has a PR in the same repo (the same `prs.repo` realpath as stored) that is not `merged` and not `cancelled`. A Job of that PR overlaps a Job of this PR when a path in the `touches` of the one overlaps a path in the `touches` of the other. Two paths overlap when they are equal, or when one is a directory (ends with `/`) that contains the other. Nothing else is compared (no globs, no normalisation). A Plan's own Jobs are never listed. The warning is only that: nothing is refused and no state changes.

The overlaps are grouped: **one overlap per (this Job, other Plan, other PR)**. It lists every Job of that other PR that overlaps this Job, with its status and the paths of this Job that overlap one of them, each path once, in the order of the Job's `touches`. A broad path (`core/src/`) or a shared file is one entry of a row, not a row of its own for every pair of paths and Jobs (hs-n201: on a busy Context that made thousands of rows and nobody could read them in a Plan review).

`plan.md` has the section `## Overlaps` after the PRs table (and its graph), before the PR sections: a table with one row per overlap, ordered by this Job's key, then the other Plan, then the PR; or, when there is none, the line `No other active Plan touches these files.` A Plan with no PR yet has no PRs table and no section.

**The Explore section** (Plan `explore-roles`, R3, R9). When the Plan has explore Jobs (`scout`, `research`, §5.1), `plan.md` has the section `## Explore` after `## Overlaps` and before the PR sections: a table with the columns exactly `Job | Kind | Tier | Status | Needs | Question | Artefact`, one row per explore Job in key order (`Needs` its needs joined by `, `, `—` when none; `Question` the first line of its `goal`, at most 100 characters; `Artefact` `run/<plan>/explore/<key>.md`); then, for each merged explore Job, a `### <key>` heading and its `summary`. Both are absent when the Plan has no explore Job. Explore Jobs are in no other block of the render: not the Jobs of a PR's table, not a PR's Mermaid graph.

```
## Explore

| Job | Kind | Tier | Status | Needs | Question | Artefact |
|---|---|---|---|---|---|---|
| `cache-key` | `scout` | H2 | merged | — | Where is the cache key built? | `run/pay-4521/explore/cache-key.md` |

### cache-key
The key is built in `core/src/cache.rs` from the repo path and the schema version.
```

```
## Overlaps

| Job | Other Plan | PR | Its Jobs (status) | Paths |
|---|---|---|---|---|
| `x` | `pay-4522` | `api` | `y` (queued), `z` (draft) | `core/src/x.rs`, `core/src/lib.rs` |
```

The columns are this Plan's Job, the other Plan, its PR, the Jobs of that PR that overlap this Job with their status (in the order they were created) and the paths of this Job that overlap them. `Paths` shows at most five paths and then `+N` for the N more, as in `` `a`, `b`, `c`, `d`, `e`, +2 ``. In the JSON output, `overlaps` lists the same rows, in the same order, as `{job, plan, pr, other_jobs: [{key, status}], paths}`: `other_jobs` are the Jobs of the other PR that overlap `job` and `paths` are all the paths of `job` that overlap them, deduplicated (only the table is capped); `[]` when there is none.

```json
{"ok": true, "path": "/home/clayton/.cpd/run/pay-4521/plan.md", "summary": "/home/clayton/.cpd/run/pay-4521/summary.md", "fingerprint": "9f86d081…", "draft_jobs": 6, "prs": 2,
 "overlaps": [{"job": "x", "plan": "pay-4522", "pr": "api", "other_jobs": [{"key": "y", "status": "queued"}, {"key": "z", "status": "draft"}], "paths": ["core/src/x.rs", "core/src/lib.rs"]}]}
```

Idempotent: yes (the same PRs and Jobs, the same outline and the same other Plans, render the same two files). Change: cpd 1's `--out` is dropped; the file has one place.

**The Plan summary** (hs-d502, D4; User, 2026-10-20, hs-d502 supersedes hs-d468). A short text cpd generates from the Plan and its outline (§4.13), so the User approves a Plan from a screenful in the Lead's chat instead of reading the render (§15.5). `plan render` writes it to `run/<plan>/summary.md` and `plan review` (§4.6) writes the same text, with the review's heading, to `run/<plan>/summary-<review>.md`. It is derived and holds no information the Plan lacks: it carries no Spec, no Mermaid and no row for a Locked Job. GitHub-flavoured Markdown only (§4.5 above). Deterministic: the same Plan, outline, overlaps, review and path give the same bytes.

**Vocabulary.** A Job **under review** is a `draft` Job that has a Tier and a Spec, a `draft` `open` Job excepted (cpd's, shown as context, hs-d573); a **stub draft** is a `draft` Job without one of them (§5.1). A Job is **locked** when it is neither `draft` nor `cancelled`. A PR **counts** when it is not `cancelled`. Keys sort bytewise.

**Layout.** The file is a sequence of **blocks**, in the order below, separated by exactly one blank line and ends with one newline. A block that has nothing to say is left out, except the first, the Goal, the Jobs block and the last. A block is one or more consecutive lines.

| # | Block | Lines |
|---|---|---|
| 1 | heading | `# <review title>`, then `Plan <plan id>: <Plan title> · review <review id>`. In `summary.md` (no review): `# Plan: <Plan title>`, then `Plan <plan id> · phase <phase>` |
| 2 | goal | `Goal: <goal>` from the outline; `Goal: —` when the Plan has no outline |
| 3 | PRs | `## PRs`, then for each PR that counts and has a Job under review, in key order, `- <key> (<repo name>, base <base>): <PR title>` and under it one line `  Done when: <line>` (two leading spaces) per line of the outline's `done_when[<key>]`, in order, none when the outline is null or has no entry for it. `<repo name>` is the PR's `name` (§2.2 PR), else the last path segment of its `repo`; `<base>` the PR's `base`; `<PR title>` the PR's `title`, else `—` |
| 4 | decisions | `## Decisions`, then `D<n> <question> → <answer>` per outline decision, `n` from 1 in the outline's order |
| 5 | assumptions | `## Assumptions`, then `A<n> <text>` per outline assumption, `n` from 1 |
| 6 | Jobs | `## Jobs (<n> under review, <m> locked)`, `n` the number of Jobs under review and `m` the number of locked Jobs, then a table with the columns `Job`, `Title`, `Tier`, `Needs`, one row per Job under review in PR key order, then Job key order: its key, its title, its Tier and its `needs` joined by `, ` (`—` when none). The table has its header and separator rows even with no Job under review. No locked Job has a row |
| 7 | order | `Order: <items>`, the items joined by `, `: one `<need> → <key>` for every need of a PR that counts, whose need is a PR that counts too, in key order of the PR and then of the need; then, as the last item, the PRs that count and neither need nor are needed by any other, joined by ` · ` (no such item when there is none). Then the line `Stacked on: <entries>`, one `<key> on <base PR>` per PR that counts and has a base PR (`base_pr` of `plan show`, §4.3: `<plan>/<key>`), in key order, joined by `, `; `none` when none is stacked. Both lines are left out when no PR counts |
| 8 | overlaps | `Overlaps: none` when §4.5's overlaps are none; else `Overlaps: <n>`, `n` the number of overlaps (the rows of §4.5's table), then one row per overlap, in §4.5's order and at most 5: `- <path>: <other plan>/<other pr>/<other job> (<status>)`, the first of the overlap's paths and the first of its other Jobs with its status, followed by ` +<k>` when the overlap has more (`k` = the other paths plus the other Jobs); when `n` is over 5, a last row `- +<n-5> more overlaps` |
| 9 | decided | `Decided: <ids>`, the outline's `decided` record ids joined by `, `; left out when there are none |
| 10 | later | `Later: <items>`, the work this review does not cover, joined by `, `: the keys of the stub drafts in key order, then `PR <key>` for each PR that counts and has no Job under review, in key order; left out when there are none |
| 11 | risks | `## Risks`, then `R<n> <text>` per outline risk, `n` from 1 |
| 12 | out of scope | `## Out of scope`, then `- <text>` per outline entry |
| 13 | last line | for a review: `Approve \| Change` (the pipe literal); for `summary.md`: `Full plan (<path>)`, the absolute path of `plan.md` |

Blocks 4, 5, 9, 11 and 12 come from the outline and are left out when it is null or the list is empty. The sizes are bounded by the outline's own limits (§4.13: at most 3 risks, 5 out-of-scope entries, 3 done-when lines per PR) and by the overlap rows above; the decisions, assumptions and Jobs are as many as the Plan has. A `done_when` key of a PR that was cancelled after `plan outline` is not shown (the block lists PRs that count).

```
# Plan review in chat
Plan plan-ux: Plan UX · review r2

Goal: Show a Plan review as a short summary in the Lead's chat, approved in one dialog

## PRs
- cpd (cpd, base main): Summary, review in chat and outline
  Done when: plan review writes summary-<review>.md from the outline
  Done when: the adapter shows it in chat with Approve and Change

## Decisions
D1 Where is the review shown? → In the Lead's chat

## Assumptions
A1 The outline is written by the Lead, not inferred

## Jobs (2 under review, 1 locked)
| Job | Title | Tier | Needs |
|---|---|---|---|
| ux-docs | Docs for the review in chat | S3 | — |
| ux-summary | Summary and outline | O3 | ux-docs |

Order: cpd
Stacked on: none

Overlaps: none

Decided: hs-d502, hs-d503

Later: ux-remeasure

## Risks
R1 pi's dialog API differs between modes

## Out of scope


Approve | Change
```

### 4.6 `plan review`

```
cpd plan review [--plan <id>] --title <text>
```

`--title` is **required** (hs-d291): it names what the drafts under review do, so the User sees it as the heading of the review, in the summary in the Lead's chat and in the full render. It has the rule of a PR's title: 1–72 characters, one line, no control characters; a bad or missing one is `usage` (exit 2). It is stored as `plan_reviews.title` (contract §2.2).

Requests the User's approval of the draft Jobs (§2.2 Plan review). In phase `plan` the review covers the whole plan, as before; in every phase after `plan` it covers **exactly the draft Jobs** the Lead added: its fingerprint is over those drafts only (§2.3; a `draft` `open` Job is not one of them, so engine E4 adding one never voids a pending review, hs-d573) and the rendered plan marks them under review and shows the Locked Jobs as context (§4.5). A Lead that planned several batches adds the later ones as drafts (`job add --draft`, §5.1) and asks for one review per batch. The Lead runs it directly; the pi adapter shows the summary in the Lead's chat, with the approval dialog, when the `plan-review` Event arrives (§15.5), so the review opens from any caller, in whatever phase the Plan is. The User approves or asks for a change.

Explore Jobs (`scout`, `research`) are in no review (Plan `explore-roles`, R3): they are never `draft`, are in no block of the review's copy (the Jobs block counts the Jobs under review and the locked Jobs of the PRs; the Explore section of §4.5 is not in the copy) and are in no fingerprint (§2.3), so adding one never voids a pending review.

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan`, or any phase after it (not `grounding`) | `illegal_phase` | 3 |
| at least one Job is `draft` (in a phase after `plan` too: the drafts are what is reviewed; a `draft` `open` Job does not count, it is not under review, hs-d573) | `no_draft_jobs` | 3 |
| every `draft` Job has its Tier and its Spec (hs-d241, R1: no stub draft is approvable); `details.jobs` lists `[{key, missing}]`, `missing` being `["tier"]`, `["spec"]` or both | `draft_incomplete` | 3 |
| every `impl` Job that is a `draft` needs at least one `test` Job of its PR (hs-d341; `details.jobs` lists the `impl` Jobs) | `test_job_required` | 3 |
| every `verify` Job that is a `draft` needs at least one `impl` Job of its PR that is not `cancelled` (pro-d80; `details` `{plan, job, jobs}`; checked after `test_job_required` and before `shape_incomplete`) | `impl_job_required` | 3 |
| every PR of shape `tdd` that has not opened has at least one `test` and one `impl` Job that is not `cancelled` (hs-d497, contract §2.2 "Job kinds"; `details` `{pr, missing}`) | `shape_incomplete` | 3 |
| the Plan has an outline (§4.13; hs-d502, D3); `details.next` = `cpd plan outline --file <outline.json>` | `outline_missing` | 3 |
| every PR that has a `draft` Job (under review, since the previous row passed) has an entry in the outline's `done_when`; `details.prs` lists the PR keys without one, in key order | `outline_incomplete` | 3 |

Effects:
- **Files:** renders the plan to `run/<plan>/plan.md` and `run/<plan>/summary.md` (§4.5) and copies the summary to `run/<plan>/summary-<review>.md`, so a later render cannot change what the User reviews. The summary copy is the Plan summary of §4.5 with the review's heading (block 1) and its last line (block 13) and no other difference; it is what the User reviews (§15.5). The copy opens with `# <review title>`, then one line `Plan <plan id>: <Plan title> · review <review id>`, then the rendered content (hs-d291); `plan.md`, made without a review (`plan render`, §4.5), keeps `# Plan: <Plan title>`. The reviewed file carries the `## Overlaps` section of §4.5, as it is at that moment, so the User sees the warning in the review: one row per open Job of this Plan and per other PR that overlaps it, short enough to read (hs-n201); it is a warning only and the review is requested all the same.
- **Closing other reviews:** every other pending review of the Plan (always older) closes `superseded` in the same transaction that inserts the new review request (the new `pending` Event comes after their `superseded` Events). Their Events say why: "Plan review r<k> of Plan P is superseded by r<k+1>." When the latest review is already pending for the current fingerprint (the `already: true` path) nothing is written, nothing closes.
- The refusals `outline_missing` and `outline_incomplete` come **before anything is written** (D3): no render, no file, no row, no Event.
- **Rows**, one transaction: `plan_reviews` (`id = r<k>`, `k` = 1 + the highest, `fingerprint` = the plan fingerprint (§2.3: it includes the outline), `title` = `--title`, `status = pending`). **Event** `plan-review` `{review, fingerprint, status: "pending"}`.

```json
{"ok": true, "review": {"id": "r3", "fingerprint": "9f86d081…", "status": "pending", "requested_at": "…"}, "summary": "/home/clayton/.cpd/run/pay-4521/summary-r3.md",
 "next": "The User reviews the plan summary in your chat and decides there. Stop and wait for the decision message."}
```

**Stubs** (hs-d241, R1). A draft without its Tier or its Spec (a stub, §5.1) cannot be approved, so the review is **not requested**: it is refused `draft_incomplete` (exit 3) with `details.jobs` = `[{"key": "api-1", "missing": ["spec"]}, …]`, in key order, `missing` holding `"tier"` and/or `"spec"`.  Nothing is rendered or written. The Lead completes each stub with `job edit --tier --spec` (§5.2) or cancels it, then asks again. The plan fingerprint covers a stub as it is (contract §2.3), so completing one changes it.

Idempotent: yes. When the latest review is `pending` for the current fingerprint, it returns that review with `"already": true` (its `path` and `summary` too) and writes nothing; the two refusals above are still checked first, so a Plan with no outline is refused even when a review made before the outline existed is pending. When the Jobs or PRs changed since, it requests a new review and closes the older one `superseded`; the older one stays in the database, closed.

### 4.7 `plan decide` (internal)

```
cpd plan decide <review> (--approve | --reject) [--feedback-file <file>] [--plan <id>]
```

Records the User's decision. Called by the pi adapter with the result of the chat dialog (§15.5); replaces cpd 1's `plan decision` (cpd 1 §11), whose `--pending` step is now `plan review`. The cpd-lead skill forbids the Lead to run it. Internal (§1.1): hidden in clap, run only by the pi adapter.

| Precondition | Code | Exit |
|---|---|---|
| the review is `pending` (decided the same way: `already`); a review with status `approved`, `rejected`, or `superseded` is refused `review_decided` | `review_decided` | 3 |

Effects: **rows**, one transaction: `plan_reviews.status`, `feedback` (the file's text, or null), `decided_at`. **Event** `plan-review` `{review, fingerprint, status, feedback}`.

In a phase after `plan`, an **approval** also queues the drafts the review covered (§2.2, §3.2), in the same transaction:
- **Rows:** every `draft` Job of the Plan becomes `queued` and gets its Locked-Job fingerprint (§2.3), as an appended Job has at `job add`; `appended` stays 1. A `draft` `open` Job is one of them: it is queued here with the others, so a PR's `open` Job is `queued` exactly when its other Jobs are (contract §3.7 E4, hs-d573) and the `jobs` of the Event lists it too. The engine schedules each as soon as it is Ready (§3.2). **A draft of an `open` PR** that becomes `queued` here is a `work` Job and reaches the pull request by its own merge (§5.1; contract §3.8 "Updating"); no `update` Job is queued.
- **Event:** the same `plan-review` Event, whose `data` gains `jobs: [key]`, the Jobs it queued, in key order. No new Event kind. `plan-phase` is not appended: the phase does not change.
- Only a **current** approval queues (*decided*: a stale one would queue drafts the User never saw): when the drafts changed since the review (`current` false), the decision is recorded and nothing is queued, `jobs` is absent and `next` asks for a new `cpd plan review`.
- A **rejection** queues nothing: the drafts stay drafts, editable, as in phase `plan`; the Lead revises them (`job edit`, `job add --draft`, `job cancel`) and asks again.
- In phase `plan`, `plan decide` writes only what it did above and `plan lock` (§4.8) works as before.

```json
{"ok": true, "review": {"id": "r3", "status": "approved", "fingerprint": "9f86d081…", "feedback": null, "decided_at": "…"}, "current": true, "next": "cpd plan lock"}
```

`current` says whether the review's fingerprint is still the plan fingerprint; `next` is `cpd plan lock` only when approved and current in phase `plan`; in a later phase an approved and current review has no `next` and its result lists `jobs` (the Jobs queued). Idempotent: yes, as stated.

### 4.8 `plan lock`

```
cpd plan lock [--plan <id>]
```

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan` (`dispatch`: `already`) | `illegal_transition` | 3 |
| at least one Job is `draft` | `no_draft_jobs` | 3 |
| every `draft` Job has its Tier and its Spec, as `plan review` requires (§4.6; `details.jobs` as there) | `draft_incomplete` | 3 |
| every `impl` Job needs at least one `test` Job of its PR, as `plan review` requires (§4.6; `details.jobs` as there) | `test_job_required` | 3 |
| every `verify` Job needs at least one `impl` Job of its PR that is not `cancelled`, as `plan review` requires (§4.6; `details` as there; after `test_job_required`) | `impl_job_required` | 3 |
| every PR of shape `tdd` has a `test` and an `impl` Job that is not `cancelled`, as `plan review` requires (§4.6; `details` as there) | `shape_incomplete` | 3 |
| the latest Plan review is `approved` for the current plan fingerprint (§2.3: Jobs and PRs) | `plan_not_approved` | 3 |
| at least one PR is not `cancelled` and each such PR has at least one `draft` Job; `details.prs` lists the empty PRs (none at all: `details.next` = `cpd pr add`) | `pr_empty` | 3 |
| the Dependency graph of the draft Jobs is acyclic and so is the graph of the PRs' needs and base PRs (`details.cycle`, qualified references) | `dependency_cycle` | 3 |
| for each PR with no needs and no base PR: its Lead worktree path does not exist, except a leftover of an interrupted lock (a worktree of the PR's repo on its Lead Branch), which is removed first; `details.pr` names the PR | `worktree_exists` | 3 |
| for each PR with no needs and no base PR: its Lead Branch and its parent path do not exist in its repo, except a leftover of an interrupted start (§2.1); `details.pr` | `branch_exists` | 3 |
| for each PR with no needs and no base PR: `origin/<lead branch>` does not exist, or it does not conflict with `origin/<base>` (`git merge-tree`, §7.2, hs-d490; `details.files` the conflicted paths); a repo with no `origin` has no remote check; `details.pr` | `branch_exists` | 3 |
| for each PR with no needs and no base PR: `pre-start` passed in its new Lead worktree (§7.3); `details.pr`, `details.tail`, `details.log` | `setup_failed` | 1 |

**Explore Jobs** (Plan `explore-roles`, R3) are invisible to the lock. A Plan whose only Jobs are explore Jobs has no `draft` Job and is refused `no_draft_jobs`, as a Plan with no `draft` Job is. An explore Job is `queued` from its `job add` and is not queued again here.

Effects (§3.1, §3.8, §7.2; R34, A8): every PR's `draft` Jobs are approved at once and the PRs with no needs and no base PR **start**, all or nothing; the PRs with needs and the **stacked** PRs (contract §2.2 "The base PR"; a base PR of the same Plan included: the lock orders no worktrees), stay `waiting` until engine E7 (or `pr start`, §9.7) starts them once their needs are `merged` and their base PR is `open`.
- **git**, for each PR with no needs and no base PR, in its repo (§7.2, R35 per PR branch, A5): `fetch origin <base>`; `ls-remote --exit-code --heads origin <lead branch>`. When it is on the remote and does not conflict with `origin/<base>` (`merge-tree --write-tree --name-only` exits 0; a base that merely moved ahead is no conflict, hs-d490): reuse it, `fetch origin <lead branch>` and `worktree add --no-track -b <lead branch> <lead worktree> origin/<lead branch>`. When it is absent: `worktree add --no-track -b <lead branch> <lead worktree> origin/<base>`. A repo with no `origin` (A3): no remote check and no push, `worktree add --no-track` from `<base>`. `--no-track`: a Lead Branch has no upstream, so nothing writes `.git/config` (hs-d243, contract §7.2). Then `pre-start` in the worktree, output in `run/<plan>/logs/lead-<key>.pre-start.log`. Every started PR's worktree is created and its `pre-start` run before anything is pushed; if one fails, every worktree and branch the lock created is removed, nothing is pushed, nothing in the database changes and the lock fails with that PR's refusal. When all have passed, each new Lead Branch is pushed, `push origin <lead branch>` from its Lead worktree (no `-u`, hs-d243). A failed push does not fail the lock: one `error` Event with code `push_failed` and `details.pr` is appended per failed PR and the next merge or `pr open` pushes again.
- **Rows**, one transaction, with a compare-and-swap on `phase = 'plan'` and on the review read: `plans.phase = dispatch`, `locked_at`; every `draft` Job of every PR `queued` with its Locked-Job fingerprint; for each started PR, with a compare-and-swap on `status = 'waiting'`: `status = dispatch`, `lead_worktree`, `started_at`. **Events** `plan-phase` `{from: "plan", to: "dispatch", jobs: [key], prs: [key]}` (`prs`: the PRs started) and one `pr-started` `{pr, repo, base, lead_branch, lead_worktree, base_sha, reused, pushed, by: "lock"}` per started PR. If the transaction fails, the worktrees and branches are removed.
- **Open Jobs:** the lock adds none. Engine E4 adds each started PR's `open` Job in the first tick after the lock (contract §3.7, hs-d340).
- **tmux:** `watch --ensure` (§12): starts `cpd watch` in `cpd-work:watch` when `locks/watch.lock` is free.

`prs` lists every PR that is not `cancelled`, in key order: a started PR with its Lead worktree, `base_sha`, `reused` and `pushed` (R35); a `waiting` one with its `needs` and null for the rest. Every row has `base_pr`: the base PR the PR still waits for, as `<plan>/<key>` (its base PR, contract §2.2 "The base PR", has not opened: it is `waiting` or `dispatch`), else null. It is what `details.base_pr` of `pr start`'s `pr_not_ready` is (§9.7), so a stacked PR the lock leaves `waiting` shows its base PR, where `needs` alone would be empty; a started PR has null.

```json
{"ok": true, "plan": "pay-4521", "phase": "dispatch", "jobs": ["api-schema", "worker", "ui"],
 "prs": [{"key": "api", "status": "dispatch", "needs": [], "base_pr": null, "lead_branch": "pay-4521/api", "lead_worktree": "/home/clayton/projects/pay.pay-4521.api", "base_sha": "4b825dc…", "reused": false, "pushed": true},
         {"key": "ui", "status": "waiting", "needs": ["api"], "base_pr": null, "lead_branch": "pay-4521/ui", "lead_worktree": null, "base_sha": null, "reused": null, "pushed": null},
         {"key": "ui2", "status": "waiting", "needs": [], "base_pr": "pay-4521/api", "lead_branch": "pay-4521/ui2", "lead_worktree": null, "base_sha": null, "reused": null, "pushed": null}],
 "watch": {"started": true, "pane": "%3"}}
```

If the engine cannot be started, the Plan stays locked and the result is `watch_start_failed` (exit 1) with `details.next` = `cpd watch --ensure` (cpd 1's `watch_pane_failed`, renamed). Idempotent: yes; in phase `dispatch` it returns `"already": true` and still runs `watch --ensure`.

### 4.9 `plan rename`

```
cpd plan rename [--plan <id>] --title <text>
```

Changes the Plan's title, the label the tree shows (contract §2.2). `--title` is required, as `plan new`'s: 1–200 characters, no control characters, else `usage` (§1.5).

| Precondition | Code | Exit |
|---|---|---|
| the Plan exists | `plan_not_found` | 4 |

Effects: an unchanged title: `"already": true`, nothing written. Otherwise **rows**, one transaction: `plans.title`; **Event** `plan-renamed` `{title}`; **tmux**: sets `@cpd_title` to the new title on the Session's pane (the Plan's Lead) when it has one **and the Lead has no dynamic `title`** (hs-d257, contract §6.1: the dynamic title wins; the rename still writes `plans.title` and appends its Event) (§6.2 of contract.md). The Context store's `plan` record keeps the old title. Archived Plans can be renamed.

```json
{"ok": true, "plan": {"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust, done"}}
```

Idempotent: yes.

### 4.10 `plan archive`

```
cpd plan archive [--plan <id>]
```

Archives the Plan with its Lead and its side forks (contract §2.2 "Archived").

| Precondition | Code | Exit |
|---|---|---|
| the Plan exists | `plan_not_found` | 4 |
| no object of the cascade is live: no Session `working`, `waiting` or `blocked`, no Job `queued` or `running`, no open Escalation, no pending Plan review and no PR of the Plan, not `merged` or `cancelled`, that a PR of another Plan (itself not `merged` or `cancelled`) names as base PR or need (`kind` `pr`, `id` `<plan>/<key>` of the PR named; contract §2.2 "Archived", hs-d324); `details.blockers` lists each as `{kind, id, reason}`. The phase does not matter: a Plan in `deploy` is archivable and the engine keeps polling its PR | `archive_blocked` | 3 |
| the Plan has no `draft` Job (hs-d731, contract §2.2 "Archived"); `details.blockers` has `{kind: "job", id: "<plan>/<key>", reason: "draft"}` for each and the message says `cpd plan cancel` abandons a Plan and `cpd job cancel` one draft | `archive_blocked` | 3 |
| neither the Lead nor a side fork is in the chat slot; `details.next` = `cpd session park` | `in_chat_slot` | 3 |

Since the migration `review-superseded` and its enforcement rule, a Plan that is `done` has no pending review: every pending review of the Plan is closed `superseded` when the Plan reaches `done`.

Effects, in this order (all or nothing):
1. **Check** every object of the cascade; nothing is written if any blocks.
2. **tmux:** close every live Lead pane as `session close` does (§10.9).
3. **Rows**, one transaction that re-checks the rule: `archived_at` = now on the Plan and each of its Sessions not archived yet. One **Event** per object: `plan-archived` `{archived: true}`, `session-archived`.

```json
{"ok": true, "archived": [{"kind": "plan", "id": "cpd2"}, {"kind": "session", "id": "596d007f-…"}], "closed": ["%41"]}
```

Idempotent: yes; already archived: `"already": true`, nothing written, nothing closed.

### 4.11 `plan unarchive`

```
cpd plan unarchive [--plan <id>]
```

Clears `archived_at` on the Plan and its Sessions.

| Precondition | Code | Exit |
|---|---|---|
| the Plan exists | `plan_not_found` | 4 |
| the Plan's Context is not archived; `details.next` = `cpd context unarchive <c>` | `archived` | 3 |

Effects: **rows**, one transaction: `archived_at = NULL` on the Plan and each archived Session of it; one **Event** per object (`plan-archived`, `session-archived`, each `{archived: false}`). No tmux and no archive rule.

```json
{"ok": true, "archived": [{"kind": "plan", "id": "cpd2"}, {"kind": "session", "id": "596d007f-…"}], "closed": []}
```

Idempotent: yes; not archived: `"already": true`, nothing written.

### 4.12 `plan cancel`

```
cpd plan cancel [--plan <id>] [--reason <text>]
```

Abandons a Plan (E1, hs-d203): cancels every Job that is not merged, closes the Plan's pull requests that are not merged, closes its Lead and its forks and archives it. cpd never closes an adopted pull request (§9.4) and never deletes its branch: step 5 leaves it open and the output lists it under `left_open`. Branches and worktrees of cancelled PRs are removed by the next `cpd janitor` pass (§29, pro-d70, pro-d71), every `watch.janitor_minutes` minutes (default 60) or by hand. `--reason` is kept in the Events' `reason`; default `plan cancelled`. The Plan may be in any phase, `done` included (nothing is left to cancel; it is then archived).

| Precondition or failure | Code | Exit |
|---|---|---|
| the Plan exists | `plan_not_found` | 4 |
| neither the Lead nor a side fork is in the chat slot; `details.next` = `cpd session park` | `in_chat_slot` | 3 |
| every running Attempt has ended within 60 s of its stop request (below); `details.attempts` lists those still running | `cancel_timeout` | 5 |
| each `gh pr close` succeeds, or its pull request is already closed or merged; `details` is `{argv, exit, stderr}` | `gh_failed` | 1 |

A Session `working`, `waiting` or `blocked` and a Job `running` do not block (unlike `plan archive`): the command stops them. The caller's own Lead is in that case.

Effects, in this order; each step is idempotent, so a rerun after any failure continues where it stopped:
1. **Check** the preconditions above that need no write.
2. **Plan reviews:** every pending review of the Plan becomes `superseded` with `decided_at` = now and `feedback` null, **Event** `plan-review` `{review, fingerprint, status: "superseded"}` (reason: "Plan review r<k> of Plan P is superseded: the Plan was cancelled.") The User sees the reason in the Event's `text` (contract §5.2) and the dialog closes in the pi adapter (§15.5).
3. **Jobs**, as `job cancel` (§5.3) for every Job not `merged` and not `cancelled` (the explore Jobs, `scout` and `research`, included: they have no PR, so step 5 does not touch them), with the reason, **but no Job is refused `shape_incomplete`**, in particular not the last `test` or `impl` Job of a `tdd` PR (cancelling a Plan is never refused by the shape check, hs-d573; the skip is an internal parameter of the path, never a flag, option or environment variable): `draft`, `queued` and `failed` become `cancelled` with **Event** `job-cancelled`; a `running` Job gets `job-cancel-requested` and engine E2 or the driver ends its Attempt `cancelled` at the next safe point (contract §3.4). The command then waits, polling, up to 60 s until no Attempt of the Plan is not `ended`. Dependents cannot start, as for `job cancel`; they are cancelled themselves by this step.
4. **Escalations:** each `open` Escalation of the Plan becomes `expired`, **Event** `escalation-expired`.
5. **Pull requests:** for each PR of the Plan with status `open` whose pull request is not `merged`, `gh pr close <number> --repo <owner/repo>`. The PR rows keep their status (`open`; PR statuses never end this way, contract §3.8); the next `pr poll` reads the closure and appends no `pr-feedback` for an archived Plan. A `waiting` PR has no pull request: nothing. A `merged` PR is never touched. A PR with `head_ref` (adopted, §9.4) is never closed: its pull request stays open and is listed under `left_open` (`[{pr, number}]`).
6. **tmux:** close every live Lead pane as `session close` does (§10.9).
7. **Rows**, one transaction: `archived_at` = now on the Plan and its Sessions, with **Event** `plan-archived` `{archived: true}` and `session-archived` per object, as `plan archive` (§4.10).

No new Event kind and no schema change. The command's own Events are the existing ones named above.

```json
{"ok": true, "cancelled": ["api", "ui"], "closed_prs": [{"pr": "ui", "number": 813}], "left_open": [{"pr": "card", "number": 3748}], "closed": ["%41"], "archived": [{"kind": "plan", "id": "cpd2"}, {"kind": "session", "id": "596d007f-…"}]}
```

Idempotent: yes (a rerun continues where it stopped). An archived Plan with nothing left to cancel: `"already": true`, nothing written. A rerun after a failure repeats steps 2 to 6 on what is left; Jobs already `cancelled` and pull requests already closed are skipped.

### 4.13 `plan outline`

```
cpd plan outline --file <outline.json> [--plan <id>]
```

| Flag | Meaning |
|---|---|
| `--file <outline.json>` | required; the outline, JSON (below); readable UTF-8, else `file_unreadable` (§1.5) |
| `--plan <id>` | the Plan (§1.3) |

Sets the Plan's **outline** (hs-d502, D1; contract §2.2 Plan `outline`): what the Lead has settled with the User and says in a Plan review's summary (§4.5), which cpd cannot derive from Jobs. It **replaces** the outline whole; there is no partial edit and no way to clear it. It works in **every phase**, so a Lead that plans a later batch writes the outline of that batch before it asks for its review (§4.6).

**The outline file**, JSON, parsed once at the boundary (§1.5). A JSON object; unknown fields are refused. Every value is a string, array or object as written here; every "line" is a string of 1–200 characters on one line, with no control character (so no line break and no tab):

| Field | Value | Rule |
|---|---|---|
| `goal` | line | required |
| `decisions` | `[{question, answer}]` | each `question` and `answer` a line; may be empty |
| `assumptions` | `[line]` | may be empty |
| `risks` | `[line]` | 0 to 3 |
| `done_when` | `{<PR key>: [line]}` | each value 1 to 3 lines; every key is a PR of the Plan that is **not `cancelled`** (checked against the Plan) |
| `out_of_scope` | `[line]` | 0 to 5 |
| `decided` | `[record id]` | Context record ids (§2.1) of the Plan's Context; each must exist there (any kind, superseded included), else `outline_invalid` |

Every field but `goal` may be absent; an absent one is stored as `[]` (`{}` for `done_when`). The stored form, `plans.outline`, is the **canonical JSON** of the object with all seven fields: keys sorted, no whitespace, strings as parsed. So an outline that says the same stores the same text, whatever its key order or spacing.

| Precondition | Code | Exit |
|---|---|---|
| the file is readable UTF-8 | `file_unreadable` | 2 |
| the file is a JSON object with no unknown field and every field has the shape and limits above. Checked in this order, the first failure being the result: not JSON, or not an object (`details.field` `$`); an unknown field (its name, the first in byte order); then `goal`, `decisions`, `assumptions`, `risks`, `done_when`, `out_of_scope`, `decided`, each element in order; `details.field` is the path of the failure, as `goal`, `decisions[1].answer`, `risks[3]`, `done_when.api[0]`, `decided[0]` | `outline_invalid` | 2 |
| the Plan exists | `plan_not_found` | 4 |
| the Plan is not archived (§1.3) | `archived` | 3 |
| every key of `done_when` is a PR of the Plan that is not `cancelled`; `details.field` = `done_when.<key>`, `details.key` = the key | `outline_invalid` | 2 |
| every id of `decided` is a record of the Plan's Context; `details.field` = `decided[<i>]`, `details.record` = the id | `outline_invalid` | 2 |

Effects:
- The same canonical JSON as stored: `"already": true` and **nothing written**, no row, no Event.
- Otherwise **rows**, one transaction: `plans.outline` = the canonical JSON. **Event** `plan-outlined` `{replaced}` (contract §5.2), `replaced` true when the Plan had an outline. Nothing else is written: no file, no tmux. The plan fingerprint changes (§2.3: it includes the outline), so a Plan review made before this write is no longer current and the Lead asks for a new one (§4.6).
- The outline is checked when it is written only: a PR cancelled afterwards leaves its `done_when` entry in it, which the summary does not show (§4.5) and `plan review` does not need (§4.6).

```json
{"ok": true, "plan": "pay-4521", "replaced": false,
 "outline": {"assumptions": ["The outline is written by the Lead"], "decided": ["hs-d502"],
             "decisions": [{"answer": "In the Lead's chat", "question": "Where is the review shown?"}],
             "done_when": {"api": ["plan review writes summary-<review>.md"]},
             "goal": "Show a Plan review as a short summary in chat", "out_of_scope": [], "risks": []}}
```

`outline` is the stored object. Idempotent: yes. The same file twice changes nothing and writes one Event only the first time (`"already": true` the second time, `replaced` absent).

### 4.14 `plan delete`

```
cpd plan delete <id> [--yes]
```

Deletes a Plan: its Jobs, Attempts and the other rows below, its Sessions but the Lead and its run files, worktrees and local branches (hs-d527). The `plans` row stays as a **tombstone** (`deleted_at` set, once only) and so does the Lead's Session row, which the tombstone references (`plans.lead_session`): both hidden from every list, view and lookup (contract §2.2 "Deleted", which has the design). The Context's records are kept. Deletion is permanent and cannot be undone. Without `--yes` the command prints what would be deleted (a preview); with `--yes` it deletes.

| Flag | Meaning |
|---|
| `<id>` | Plan id (§2.1) |
| `--yes` | perform the delete; without it, print the preview only |

| Precondition | Code | Exit |
|---|---|---|
| the Plan exists and is not deleted; a deleted one is `already` (below) for this command and `plan_not_found` for every other | `plan_not_found` | 4 |
| **archived or asleep**: the Plan is archived or asleep; `details: {blockers: [{kind, id}]}` lists what is not | `archive_blocked` | 3 |
| **nothing running**: no Job `queued` or `running`, no Attempt not `ended`, no Session `working`, `waiting` or `blocked`, every Escalation closed, no Plan review pending; `details.blockers` | `archive_blocked` | 3 |
| **no open PR**: every PR of the Plan is `merged` or `cancelled`; `details.blockers` | `archive_blocked` | 3 |
| **Lead not in chat**: the Plan's Lead Session (§3.1) is not in the chat slot; `details.next = cpd session park` | `in_chat_slot` | 3 |

Effects, with `--yes` only (**without it, no rows or files are written**):
1. **One transaction.** It re-checks the preconditions (race safety), then sets `plans.deleted_at = now()` **first** (the triggers that guard the deletes read it, contract §4.4; a set `deleted_at` is never changed or cleared). Then it deletes the rows, each through the exception for a deleted Plan, in an order that keeps the foreign keys whole:
   - The plannotator reviews and board posts of the Plan's Sessions (the Lead's included)
   - Every Inbox item of the Plan, then every Event of the Plan
   - Every Escalation, Attempt, Dependency and Job of the Plan
   - Every `jira_transitions` row of the Plan (§21.11), before the PRs it refers to
   - Every PR need, PR and Plan review of the Plan
   - Every Session of the Plan but the Lead (Workers and forks); the Lead's row stays, with its `plan` unchanged, hidden with the Plan
   - The Context's records stay (hs-d527)
   Every `DELETE` writes a `history` row with the whole old row (§4.4); the tombstone is an `update` row. They stay and the tamper check stays clean (contract §2.2). Last, in the same transaction, **one `plan-deleted` Event** is appended, with the counts; it is never deleted by this delete. The Plan's `pull_requests` rows are no Plan's: the poll deletes them once no PR points to them (§7.6 step 5).
2. **Session files** (after commit): every Session's file, the Lead's included; files that do not exist are skipped.
3. **Run files** (after commit): everything under `$CPD_HOME/run/<plan>/`: summaries, renders, reviews, messages, specs, logs and distill artifacts.
4. **Worktrees and local branches** (after commit): `rm -rf` every Attempt worktree and PR Lead worktree, each with its `target/` (this, with the run files, is where the disk comes from: the database file does not shrink); then the stored `lead_branch` of each PR and `cpd/<plan>--<attempt>` from their repos (not on the remote), then the directory of step 3. The paths come from the live rows when the transaction commits and on a rerun from their `before` copies in `history`.

**Rerun.** The rows go in one transaction and steps 2–4 after it, so a crash leaves a tombstoned Plan with files on disk. A rerun with `--yes` on a deleted Plan writes **nothing** to the store (no row, no Event), finishes whatever file step is left (a missing file, directory or branch is not an error) and returns `"already": true`. A preview of a deleted Plan returns `"already": true` and what is left on disk, not the counts of the first preview, which only the Event keeps.

Output without `--yes` (preview) or with `--yes` after a delete:

```json
{"ok": true, "preview": true, "already": false, "jobs": 47, "attempts": 121, "sessions": 3, "worktrees": 11, "disk_freed_mb": 342}
```

`sessions` counts every Session of the Plan, the Lead included (its row stays, its file goes). With `--yes` the first time: `"preview": false`, `"already": false`. Rerun with `--yes`: `"preview": false`, `"already": true` and `worktrees` and `disk_freed_mb` of what this run removed (0 when nothing was left). Preview of a deleted Plan: `{"ok": true, "preview": true, "already": true, "left": {"worktrees": 0, "branches": 0, "files": 0}}`.

Idempotent: yes; a Plan already deleted returns `"already": true` and writes nothing to the store.

**Tests that assume otherwise change with this design** (hs-d527; for delete-core to have approved): `plan_rows` of `cli/tests/flow/plan_delete.rs` counts Sessions with `plan = <id>` and Events with `plan = <id>`, but the Lead's row stays with its `plan` and the `plan-deleted` Event names the Plan; the Jobs, Attempts, PRs, the other Sessions and the other Events are what is gone.

## 5. `job`

Jobs, Specs and Dependencies (§2.2, §3.2). Kept from cpd 1 §11 "Jobs" except where stated.

**Spec file** (`--spec`), JSON, every field required, unknown fields refused (`spec_invalid`, exit 2, with the JSON path in `details`):

```json
{"goal": "…", "context": "…", "touches": ["docs/commands.md"], "acceptance": ["…"], "out_of_scope": ["…"]}
```

`touches` and `acceptance` have at least one non-empty string; `out_of_scope` may be empty. Change: cpd 1's `outOfScope` is now `out_of_scope`, the column's name.

**A `test` Job's Spec file** (kind `test`, hs-d341) has two more fields: `tests`, the named tests (one or more distinct one-line strings) and `test_files`, the files that hold the red tests (one or more repo-relative paths or, ending in `/`, directory prefixes; no `..`, no leading `/`, the rule of the `protected` list, contract §7.5):

```json
{"goal": "…", "context": "…", "touches": ["cli/tests/it/pr_open_job.rs", "core/src/pr/"], "acceptance": ["…"], "out_of_scope": ["…"],
 "tests": ["pr_open_job::merges_the_base", "pr_open_job::opens_the_pull_request"], "test_files": ["cli/tests/it/pr_open_job.rs"]}
```

Both are required for a `test` Job that is not a stub and refused for every other kind (`spec_invalid`, exit 2, with the JSON path in `details`). An `impl` Job names no tests: its tests are the union of its test Jobs' (contract §2.2 "Job kinds").

**The optional `resources` Spec field** (contract §2.2): a list of one or more distinct names, each matching `^[a-z][a-z0-9-]{0,31}$`; absent means none. It names the declared resources (contract §8.3 "The `resources` domain") the Job is the tenant of from its first Attempt until it is merged or cancelled (contract §3.4 "Tenancy"):

**Tier flag** (`--tier`): `^(O|S|H)([0-5])$` or exactly `GX`. `G0`–`G5` are `tier_invalid` (exit 2, `details.tier`), before any catalog check, with the message `invalid Tier "G0": G has no effort, write GX (hs-d562).` A Tier from the pattern with an effort the letter's model supports (§9) is valid; otherwise `tier_invalid` (exit 2), with `details.efforts` listing the valid ones. **For a Job's Tier, the rule of pro-d82 applies next** (the User's decision, 2026-10-26; contract §9 "Tier rules"): a Worker uses only H0–H3 and S0–S3, so the Tier must be `H0`–`H3` or `S0`–`S3`, else `tier_not_allowed` (an explore Job's range is its kind's, below) (exit 2, `details` `{tier, allowed: "H0-H3, S0-S3", rule: "pro-d82"}`), message `Tier <T> is not allowed for a Worker: Workers use only the Tiers H0-H3, S0-S3, to keep cost down (pro-d82, hs-d554)`. `job add`, `job edit`, `job retry` and `job dispatch` apply it, after `tier_invalid`; a stub draft's Tier is checked too. A store that already holds a Job on a refused Tier is not rewritten: §5.5 and contract §9 ("Stored Jobs and Plans") say what happens to it.

### 5.1 `job add`

```
cpd job add <key> --title <text> --tier <T> --spec <file> [--pr <key>] [--kind <work|docs|test|impl|verify|scout|research>] [--needs <k1,k2>] [--draft] [--urgent] [--plan <id>]
cpd job add <key> --draft --title <text> [--tier <T>] [--spec <file>] [--pr <key>] [--kind <work|docs|test|impl|verify|scout|research>] [--needs <k1,k2>] [--plan <id>]
```

`--tier` and `--spec` are required, except with `--draft`: a **stub draft** (hs-d241, R1) needs only `--title`; either or both of `--tier` and `--spec` may be given too. A Job that is not a draft still needs both (`usage`, exit 2). The Lead registers planned work with a stub the moment it is planned (hs-d239) and completes it later with `cpd job edit <key> --tier <T> --spec <file>` (§5.2). A follow-up PR is added the same way: `pr add <key> --repo … --needs …` (§9.4) plus stub drafts in it.

| Flag | Meaning |
|---|---|
| `<key>` | Job key (§2.1). Change: positional, like every other `job` command (cpd 1 had `--key`) |
| `--pr <key>` | the Job's PR (R34, A14), selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one. Not for `scout` or `research`: `--pr` is refused `explore_pr_forbidden` and an explore Job belongs to the Plan (below) |
| `--needs <k1,k2>` | Dependencies, comma-separated, no duplicates (`usage`); Jobs of the same PR only (A7) |
| `--draft` | add the Job as a `draft` in a phase after `plan` too (User, hs-d173, hs-d174, hs-d179: every planned batch is visible in the Plan view before it is approved): `status = draft`, `appended = 1`, no fingerprint; the engine never runs it and it becomes `queued` only at the approval of a Plan review of the drafts (§4.7). In phase `plan` every Job is a draft, so `--draft` changes nothing there. Its needs may be drafts. Without `--tier` and `--spec` it is a stub (above); `--draft` is the only way to add a Job without them. A Job that only carries out a recorded User decision, with no open design choice, is added without `--draft` (User, hs-d301, amending hs-d173, hs-d174, hs-d179); drafts are for design the User has not decided and when unsure it is a draft. **After the lock a stub that waits for nothing is refused** (`draft_parked`, the row below; contract §2.2 "A parked draft"). Not for `scout` or `research` (`explore_no_draft`): an explore Job is never a draft (below) |
| `--urgent` | add the Job with `urgent = 1` (contract §2.2): engine E3 starts it before every non-urgent Ready Job and may run it as one Attempt beyond `concurrency` (§3.7). Use it **only when the User explicitly asks**, usually for something that blocks all work or an incident. Not a Locked field and not in any fingerprint (§2.3); works with `--draft` and in every phase `job add` works in. To mark an existing Job, see §5.8 |
| `--kind <work\|docs\|test\|impl\|verify\|scout\|research>` | the Job's kind (hs-d341, contract §2.2 "Job kinds"); default `work`. `scout` and `research` are the explore kinds, below. `verify` adds checks (end-to-end, smoke, load, contract) and changes no product behaviour: it has no Spec section of its own (no `tests` or `test_files`, `spec_invalid`) and its submit runs only the tamper check; it needs an `impl` Job of its PR (`impl_job_required`). `open` and `update` are cpd's: `kind_reserved`. The PR's stored shape (§9.4) decides which kinds it may have (`shape_violation`, below). A `test` Job's Spec file has `tests` and `test_files` (§5), an `impl` Job needs a `test` Job of its PR and both need the repo's `[test-commands]` (below). |

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan`, `dispatch`, `deploy` or `done` (`done` only with `--draft` and then only into a PR that is not `merged` or `cancelled`: none exists until `pr add` has reopened the Plan); `grounding`: `illegal_phase` (except for the explore kinds, below) | 3 |
| the PR is not `merged` or `cancelled` (§3.2) | `pr_ended` | 3 |
| no Job with this key, unless its Locked fields (`pr` included) and needs are identical (then `already`) | `job_key_exists` | 3 |
| every `needs` key is a Job of the Plan | `needs_unknown` | 3 |
| every `needs` Job is in the same PR; `details.jobs` lists those that are not | `needs_other_pr` | 3 |
| no `needs` Job is `cancelled` or `failed`, because a Dependency on a `failed` Job is refused | `needs_unavailable` | 3 |
| without `--draft`, in a phase after `plan`: no `needs` Job is a `draft` (`details.jobs` lists them; its message says to add this Job with `--draft`, or to approve the drafts first), because a `queued` Job could start before the draft it needs exists | `needs_unavailable` | 3 |
| `--kind open` or `update`; `details.kind` | `kind_reserved` | 3 |
| no `needs` Job is an `open` or `update` Job (nothing waits for them: contract §2.2 "Job kinds"); `details.jobs` | `needs_unavailable` | 3 |
| `--kind test` or `impl`: the PR's repo policy has `[test-commands]` (contract §7.7; none when there is no repo file or the file lacks the table) | `tests_unconfigured` | 3 |
| every `touches` entry of the Spec is checked against the tree the Job will start from (the PR's Lead Branch head once the PR started, else its base, contract §2.1; a PR stacked on a base PR that is `waiting` or `dispatch` and is itself still `waiting` has no start tree yet, its base being that PR's Lead Branch, which does not exist until the base PR opens (contract §3.8 "Stacked PRs"): nothing is checked and the entries pass): a literal path must exist in that tree, or its parent directory must (a new file); a glob must match a file, or its fixed prefix directory must exist; cpd's own `open` and `update` Jobs are not checked (contract §3.7 E4, hs-d512) and neither is an explore Job (its `touches` is its artefact, below); `details: {missing: [entries], tree: <ref>}` | `touches_missing` | 3 |
| without `--draft`, an `impl` Job needs at least one `test` Job of its PR; `details.jobs` | `test_job_required` | 3 |
| without `--draft`, a `verify` Job needs at least one `impl` Job of its PR that is not `cancelled`; `details` `{plan, job, jobs}`, `jobs` the PR's `impl` Jobs | `impl_job_required` | 3 |
| with `--draft`, in phase `dispatch`, `deploy` or `done` (phase `plan` is never refused: the Lead composes the first batch there): the Job is not a stub that is not waiting, as contract §2.2 "A parked draft" defines it (hs-d731). Never refused: a full draft, a stub that waits, the `open` Job's draft. `details` `{plan, job, pr, needs}`; the message names the legal forms: give the stub a need on a `queued` or `running` Job of its PR (`--needs`); put it in a PR that waits for another PR; give it Tier and Spec and ask for the Plan review in the same turn; or, when it only carries out a recorded decision, add it queued (no `--draft`, hs-d301) | `draft_parked` | 3 |
| the PR's stored shape allows the kind, while the PR is `waiting` or `dispatch` (hs-d497, contract §2.2 "Job kinds"): `tdd` allows `docs`, `test`, `impl`, `verify`; `docs` allows `docs`; `config` allows `work`; `free` allows every kind; `details` `{pr, shape, kind, allowed}`. **A PR whose status is `open` allows only `work`** (pro-d68, contract §2.2 "Job kinds"): `docs`, `test`, `impl` and `verify` are refused whatever the shape, `details` `{plan, pr, status: "open", shape, kind, allowed: ["work"]}` | `shape_violation` | 3 |
| the Spec file has `tests` and `test_files` exactly when the kind is `test` and the Job is not a stub (§5) | `spec_invalid` | 2 |
| every string of the Spec (each field, each `touches` entry, each `acceptance` item and their `test` and `test_files` for a test Job) names only repo-relative paths, not the primary checkout of any PR's repo (contract §8.3); a match is a substring ending at the end of the string or before a non-letter, non-digit, `.`, `_` or `-` character; `details: {path, primary, pr}` names the JSON path, the root realpath (or `~`/`$HOME` prefix) and the PR | `spec_invalid` | 2 |
| `--tier` is a Tier a Worker may have: `H0`–`H3` or `S0`–`S3` (§5, pro-d82, hs-d554) and for an explore kind its range: `H1`–`H3` for `scout`, `S1`–`S3` for `research` (Plan `explore-roles`, R6; an `O` Tier, any `S` outside the kind's range, `H4`, or effort 5 is refused; a Tier that is not valid at all: `tier_invalid`; `details` name the kind and its range) | `tier_not_allowed` | 2 |
| the Spec's `resources` is valid for the kind (§5, JSON path `$.resources`) | `spec_invalid` | 2 |
| every name in the Spec's `resources` is declared by the host, owner or repo policy of the PR (contract §8.3 "The `resources` domain"); `details` `{job, pr, resource, declared, file}`. A stub draft has no Spec of the Lead's and no check (the `open` stub's Spec is cpd's and names no `resources`); `plan review` and `plan lock` do not check again | `resource_undeclared` | 3 |
| `--pr` with `--kind scout` or `research` | `explore_pr_forbidden` | 3 |
| `--draft` with `--kind scout` or `research` | `explore_no_draft` | 3 |
| `--kind scout` or `research` in phase `done` (with or without `--draft`): a message of its own, since an explore Job is added in `grounding`, `plan`, `dispatch` or `deploy` | `illegal_phase` | 3 |
| an explore Job's `needs` are all explore Jobs and no other Job needs an explore Job; `details.jobs` lists the Jobs on the other side. `needs_other_pr` does not apply to an explore Job, which has no PR | `explore_needs_mixed` | 3 |
| an explore Job's Spec has exactly the four fields of the explore Spec (below): `touches`, `tests`, `test_files` or `resources` is refused, with the JSON path in `details` | `spec_invalid` | 2 |

### Explore Jobs (`scout`, `research`)

Explore Jobs (Plan `explore-roles`, R1-R9; contract §2.2 "Explore Jobs") answer one question from this machine, or also from outside it for `research` and leave one Markdown artefact and a bounded summary. Only a Lead adds one. Together they are the explore kinds; a Job of one is an explore Job.

- **Belongs to the Plan.** No `--pr` (`explore_pr_forbidden`); `pr_required` is not raised, also when the Plan has two or more PRs. The Job's `pr` is null (`plan show`, `job show`).
- **No draft, no review, no lock.** The Job is `queued` at once, with its Locked-Job fingerprint, in phase `grounding`, `plan`, `dispatch` or `deploy`; `--draft` is `explore_no_draft`; in phase `done` it is `illegal_phase`. It is in no plan fingerprint and invisible to `plan review` and `plan lock` (§4.6, §4.8): adding one never invalidates an approved review.
- **The Spec is four fields**: `goal` (the one question), `context` (where to look and what a good answer contains; it may name absolute machine paths, the repo-relative rule is waived for these kinds), `acceptance` (what the file must answer and cite) and `out_of_scope`, all required. `touches`, `tests`, `test_files` and `resources` are `spec_invalid`. cpd derives and stores `touches` = `["run/<plan>/explore/<job-key>.md"]`, the artefact, a path relative to `$CPD_HOME`; the `touches_missing` check does not run for it.
- **Tier**: `scout` `H1`-`H3` (its ladder is `H1` > `H2` > `H3`), `research` `S1`-`S3` (`S1` > `S2` > `S3`). After the top rung a `no_submit` fails the Job; a `crashed` or `merge_conflict` there is retried at the same Tier under the cap; it never leaves its letter (`job retry` and §9 as for any Job).
- **Needs**: an explore Job needs only explore Jobs and no other Job needs one (`explore_needs_mixed`).
- **Deliverable and end.** The Worker writes one Markdown artefact at `$CPD_HOME/run/<plan>/explore/<job-key>.md` and runs `cpd attempt submit <attempt> --summary-file <path>` (§6.1), a summary of at most 1500 characters. On a pass the Attempt and the Job end `merged` with no git merge; the Lead's `job-merged` carries the summary, never the file (§15.3). `cpd job show` shows both (§5.7).
- **The Attempt** has no worktree, no `pre-start` and no Lead Branch. Its worktree is the explore directory `$CPD_HOME/run/<plan>/explore/`, created if missing, which is also the Worker's cwd. Its timeout is `timeouts.explore_minutes` (§14.2, default 20). It takes a Worker slot under `concurrency` like any Job. Its guards are §15.22.

### A Spec names no primary checkout

A Spec must name only repo-relative paths. A Worker works in its own worktree (contract §7.2), not the repo's primary checkout (contract §15.16: the adapter refuses any write into a primary checkout). When reading the latest main, the Worker runs `git fetch origin` and then `git show origin/main:<path>`. cpd checks every Spec string (`goal`, `context`, each `touches` entry, each `acceptance` item and for a `test` Job its `tests` and `test_files`), searching for the root realpath of each PR's repo (or `~`, `$HOME` or `${HOME}` as a prefix of it), as a substring ending at the end of the string or before a character that is not a letter, a digit, `.`, `_` or `-`: for example `<root>/docs/x.md`, `<root>` followed by a space, a quote, a backtick or a comma match; `<root>-old` and `<root>.wt/x` do not. The first match in field order (goal, context, touches, acceptance, out_of_scope, tests, test_files) and PR order is reported as `spec_invalid`, with `details: {path, primary, pr}` the JSON path, the root and the PR key. Nothing is written when the check refuses. **Explore Jobs** (Plan `explore-roles`, R5): the check does not run for an explore Job's Spec, whose `context` may name absolute machine paths; and the Spec of any other Job may cite the artefact `$CPD_HOME/run/<plan>/explore/<job-key>.md`, which this check does not refuse.

**A stub draft** stores `tier` and the Spec fields `goal`, `context`, `touches`, `acceptance` and `out_of_scope` as null for what was not given (a given `--spec` sets all five; `--tier` sets the Tier) and its `job-added` Event has `tier: null` when it has none. A draft that lacks its Spec or its Tier is **incomplete**: it is never queued or run (contract §3.2), a Plan review of it is refused `draft_incomplete` (§4.6) and so is `plan lock` (§4.8). `needs` and `pr` work as for any draft.

Effects: **rows**, one transaction: `jobs` (`pr` = the PR's key; `kind` (`work` by default); `tests` and `test_files` from the Spec file of a `test` Job; `resources` from the Spec file (null when absent); `urgent` = 1 with `--urgent` (else 0); `status = draft`, `appended = 0`, no fingerprint, in phase `plan`; `status = draft`, `appended = 1`, no fingerprint, with `--draft` in any later phase; `status = queued`, `appended = 1`, fingerprint stored, in a later phase without `--draft`); one `dependencies` row per need. **Event** `job-added` `{pr, kind, status, appended, tier, needs, urgent}`. A Job appended to a PR keeps the PR's `open` Job waiting until it is `merged` or `cancelled`: that Job's Ready rule counts every Job of the PR (contract §3.2). (Before hs-d340 the add also re-armed `all-merged`; that step and Event are retired, contract §3.7 E4.) No cycle is possible: nothing needs a new Job yet. An appended Job of a `waiting` PR is not Ready until its PR starts (§3.2); an appended draft is not Ready at all until its review is approved; one of an `open` PR (a `work` Job, above) reaches its pull request by its own merge (contract §3.8 "Updating"): **no `update` Job is queued**, no PR Gate runs. `pr push` (§9.9) is the repair after a `push_failed` and `pr rebase` (§9.3) the repair for the rest. And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

**Feedback fixes chain after the open Job** (pro-d103, pro-d105; A1–A5): when a PR is `open` and its `open` Job is `merged`, on an `open` PR a Lead appends only `work` Jobs (contract §3.8 "Updating") as feedback fixes. The chain rule (A2): the **leaves** are the non-cancelled Jobs of the PR that come after the merged `open` Job and that no other such Job needs; the `open` Job itself is a leaf if it has no Dependent Job yet, or once all Dependents end. A `--needs` arg given to a feedback-fix `job add` is merged (union, A3) with the leaves, so `--needs` may be empty or omit the leaves. Drafts and `update` Jobs are never leaves (A2): they never hold and a `queued` `update` Job would deadlock a fix Job that needs it. A queued Job that needs a Job that is not merged, if the needed Job is `failed` and no other appended Job of the PR needs it and is not itself `failed`, is refused `needs_unavailable` with a failed-leaf error: the pool of available Dependents is empty, the fix Job cannot go forward and adding to it deadlocks the chain (A4). The chain stays idempotent (A3): rerunning `job add` with the same fix and flags recomputes the leaves without that fix itself and without Jobs that transitively need it, so the second call appends the same `--needs` and exits `already`.

```json
{"ok": true, "job": {"key": "api-schema", "pr": "api", "kind": "work", "status": "draft", "appended": false, "tier": "S2", "needs": [], "urgent": false}}
```

Idempotent: yes, as stated (`--draft` and `--urgent` are not Locked fields: the call that found an identical Job returns `already` whatever its status).

### 5.2 `job edit`

```
cpd job edit <key> [--title <text>] [--tier <T>] [--spec <file>] [--pr <key>] [--kind <work|docs|test|impl|verify>] [--needs <k1,k2> | --no-needs] [--plan <id>]
```

**Completing a stub draft** (hs-d241, R1): `cpd job edit <key> --tier <T> --spec <file>` sets the Tier and the Spec of a draft that lacks them (a draft is editable, so this is an ordinary edit; `fields` lists `tier` and `spec`). Once the draft has both, it is complete and a Plan review may approve it (§4.6). The edit changes the plan fingerprint (contract §2.3), which covers a stub as it is.

**A queued Locked Job's Spec** (ADR-0014, User, Context record pro-d79; contract §3.2 "Locked"): `cpd job edit <key> --spec <file>` also works, in any phase after the lock, on a Job that is `queued`, has no Attempt row of any status (not even an ended one) and is not an `open` or `update` Job. `--spec` is the **only** flag such a Job takes: `--title`, `--tier`, `--pr`, `--kind`, `--needs` or `--no-needs` is refused whole with `job_locked` and nothing is written. The Lead only: a Worker (`CPD_ATTEMPT` set) is refused `not_a_lead`. The new Spec is checked as a draft's `--spec` is (`spec_invalid`, the no-primary-checkout rule, `touches_missing`; a `test` Job keeps its `tests` and `test_files`). One write transaction re-reads the Job's status and Attempts, updates the Spec columns, recomputes `jobs.fingerprint` (contract §2.3) and appends one `job-edited` Event whose `fields` lists the changed Spec columns and whose text says it is a Locked Job's edit; the `history` rows keep the old and the new text. There is no Plan review and no User approval: tell the User in chat what you changed. A change that alters nothing writes nothing (`"already": true`). An explore Job (`scout`, `research`) is `queued` from its add, so the same rule makes `--spec` its only flag and its Spec is the four-field one of §5.1 (Plan `explore-roles`).

**`--kind`** (hs-d341) changes a draft's kind, never to `open` or `update` (`kind_reserved`); a Job that is not a draft is Locked. The Spec's `tests` and `test_files` must fit the new kind (`spec_invalid`: a `test` Job that is not a stub has both, every other kind neither) and the rules of §5.1 apply as there (`tests_unconfigured`; `test_job_required` where an `impl` Job stops being a draft and `impl_job_required` where a draft becomes a `verify` Job (by `--kind verify` or `--needs`) and stops being one, both at the review or the lock; `shape_violation` when the PR's shape does not allow the new kind). `fields` lists `kind`.

A draft is editable in **every phase** (User, hs-d173, hs-d174, hs-d179), including one added after the lock, whose review may be pending: the edit changes the fingerprint of the drafts, so the pending review no longer matters (§4.6). `--needs` may name drafts. `--no-needs` removes every Dependency (cpd 1 had no way to). `--pr <key>` moves the draft Job to another PR (R34, A14); it is a field flag, so §1.3's default does not apply: without it the Job keeps its PR. Dependencies stay inside one PR (A7): a Job that changes PR keeps none of its old needs, so give `--needs` with Jobs of the new PR, or `--no-needs` and no other Job may need it.

| Precondition | Code | Exit |
|---|---|---|
| at least one field flag | `nothing_to_edit` | 2 |
| with `--tier`: the Tier is one a Worker may have, `H0`–`H3` or `S0`–`S3` (§5, pro-d82, hs-d554). A draft stored with a refused Tier before pro-d82 is now allowed; a draft stored with a refused Tier before hs-d554 is repaired by this edit; an edit that does not give `--tier` is not blocked by the Tier it holds | `tier_not_allowed` | 2 |
| the edit leaves no stub that is not waiting after the lock (hs-d731): any change of a draft (`--needs`, `--no-needs`, `--tier`, `--spec`, `--title`, `--kind`, `--pr`) is refused when the result is such a stub, by the rule and with the `details` and message of §5.1; phase `plan` is never refused and an edit that completes the Job (Tier and Spec) is allowed | `draft_parked` | 3 |
| the Job is `draft`, in whatever phase; a Job that is not `draft` is Locked; an `open` or `update` Job is cpd's and always refused (its Spec is cpd's), also while the `open` Job is a `draft` (hs-d573) | `job_locked` | 3 |
| `--pr` names a PR of the Plan | `pr_not_found` | 4 |
| that PR is not `cancelled` | `pr_ended` | 3 |
| the needs exist and are usable | `needs_unknown`, `needs_unavailable` | 3 |
| every Dependency after the edit links two Jobs of the same PR: the needs are in the Job's PR (the new one with `--pr`) and with `--pr` the Job keeps no old needs and no Job needs it; `details.jobs` lists the Jobs on the other side (§4.4 `dependencies_same_pr`, `jobs_pr_dependencies`) | `needs_other_pr` | 3 |
| the new needs make no cycle | `dependency_cycle` | 3 |
| with `--spec`: every `touches` entry is checked as in `job add` (§5.1), against the tree the Job will start from (contract §2.1, hs-d512; for a PR stacked on a `waiting` or `dispatch` base PR that is itself still `waiting` there is no tree yet, so nothing is checked, §5.1); `details: {missing: [entries], tree: <ref>}` | `touches_missing` | 3 |
| with `--spec`: the Spec names only repo-relative paths, not the primary checkout of any PR's repo (§5.1 "A Spec names no primary checkout"); `details: {path, primary, pr}` | `spec_invalid` | 2 |
| with `--spec`: the Spec's `resources` is valid for the kind (§5) | `spec_invalid` | 2 |
| with `--spec` of an explore Job: `touches`, `tests`, `test_files` or `resources` is refused, the explore Spec of §5.1 (the `touches_missing` check does not run for it) | `spec_invalid` | 2 |
| with `--spec`, or with `--pr` for a Job that has `resources`: every name is declared for the (new) PR, as in `job add` (§5.1); `details` `{job, pr, resource, declared, file}` | `resource_undeclared` | 3 |

Effects: **rows**, one transaction: the changed `jobs` columns; `dependencies` rows deleted and inserted to match (§4.4 allows deletion only for `draft`); with `--pr`, in the order §4.4 states: delete the old Dependencies, update `pr`, insert the new ones. **Event** `job-edited` `{fields: [name]}`, listing only the fields whose value changed (`pr` among them). And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "job": {"key": "api-schema", "status": "draft"}, "fields": ["tier", "needs"]}
```

The Event is `job-edited` in both cases. Idempotent: yes; when no value changes it writes nothing and returns `"already": true`.

### 5.3 `job cancel`

```
cpd job cancel <key> --reason <text> [--plan <id>]
```

| Precondition | Code | Exit |
|---|---|---|
| the Job is not `merged` | `job_merged` | 3 |
| the Job is not the last `test` or the last `impl` Job (not `cancelled`) of a PR of shape `tdd` that has not opened (hs-d497: add the replacement first); `details` `{pr, missing}`. Applies to `job cancel` typed by the Lead only: `pr cancel` (§9.6) and `plan cancel` (§4.12) cancel their Jobs without it (hs-d573) | `shape_incomplete` | 3 |

Effects (§3.2 "Cancel"):
- `draft` (in any phase: a draft added after the lock is cancelled like one in phase `plan`), `queued` or `failed`: one transaction, `status = cancelled`, `cancel_reason`, `cancel_requested_at`; **Event** `job-cancelled` `{reason}`.
- `running`: one transaction, `cancel_reason` and `cancel_requested_at` only; **Event** `job-cancel-requested` `{reason}`. Engine E2 sets the Attempt's `stop_request = cancelled` and the driver or E2 ends it `cancelled` at its next safe point (§3.4), which appends `job-cancelled`. No tmux action by this command.
- **Dependents (hs-d647)**: in the same transaction, every Job of the Plan that needs the cancelled Job, directly or transitively through any status, becomes `failed` (when `queued`: `failed_cause` is `its need <cancelled-key> was cancelled`, one `job-failed` Event) or `cancelled` (when a `draft`: `cancel_reason` the same string, one `job-cancelled` Event); one already `failed`, `merged` or `cancelled` is untouched. The result's `failed` and `cancelled` arrays name the Jobs this changed and are absent when empty. In the `running` case the dependents are failed in the transaction that makes the Job `cancelled` (the engine's, 3.4), not by this command.
- And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "job": "worker", "status": "cancelled", "cancel_requested": false, "failed": ["dep"], "cancelled": ["draft-dep"]}
```

Idempotent: yes; a `cancelled` Job, or one with a request already recorded, returns `"already": true` (the first reason is kept). Dependents of a `failed` Job stay `queued`: `job retry` recovers the need, so that wait is legal. **Cancelling a PR's `open` Job stands** (contract §3.7 E4, §3.8): engine E4 adds no fresh one, so the PR stays `dispatch` with no live `open` Job. It is not a way to start over: to open the PR later the Lead runs `pr open --pr <key>` (§9.1, the repair, not refused `open_job_live` since no `open` Job is live); to give it up, `pr cancel <key>` (§9.6), which also cancels every `draft`, `queued` or `failed` Job of the PR, its `open` Job included, with the PR and adds nothing back. `job retry` (§5.4) takes a `failed` Job only and runs it again at another Tier.

### 5.4 `job retry`

```
cpd job retry <key> --tier <T> [--plan <id>]
```

| Precondition | Code | Exit |
|---|---|---|
| `--tier` is a Tier a Worker may have, `H0`–`H3` or `S0`–`S3` (§5, pro-d82, hs-d554): **`job retry --tier` refuses a Tier outside the Worker range**, so a Job that failed on `S3` cannot be retried higher; its remedy is the same Tier or a new Job with a better Spec. For an explore Job the range is its kind's (`scout` `H1`-`H3`, `research` `S1`-`S3`) and `details` name the kind and its range. Checked first, so a refused Tier changes nothing | `tier_not_allowed` | 2 |
| the Job is `failed` (or `queued` with this `retry_tier` and no Attempt since: `already`) | `job_not_failed` | 3 |
| no `needs` Job is `cancelled` (`needs_unavailable`, exit 3, `details.needs` lists them): a Job held by a cancelled need can never start, so cancel it and add a replacement whose needs do not name the cancelled Job. A `failed` need is allowed: retrying recovers it | `needs_unavailable` | 3 |

Effects: **rows**, one transaction: `status = queued`, `retry_tier = <T>`, `failed_cause = null` (the Job's `tier` field is left unchanged). **Event** `job-retried` `{tier}`. The next Attempt has `origin = retry` and this Tier and the ladder restarts from it with a fresh cap (§9; for an explore Job the ladder stays in its kind's range, `scout` `H1` > `H2` > `H3`, `research` `S1` > `S2` > `S3` and after the top rung a `no_submit` fails the Job, while a `crashed` or `merge_conflict` there is retried at the same Tier under the cap, Plan `explore-roles`, R6; Attempts whose driver never started are never counted). The Plan view, `plan render` and `context pending` show the Tier the Job is displayed at (contract §3.4a), which is `retry_tier` while the Job is queued at it; `plan show` and `job show` print both `tier` and `retry_tier` as raw field values. And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "job": "worker", "status": "queued", "tier": "S4"}
```

Idempotent: yes, as stated. (The engine also retries a `failed` open or update Job of the same PR when its fix merges, contract §3.2 "Engine auto-retry on fix merge" (pro-d89); a Lead normally retries only when no fix is needed or when a fix did not restart the Job.)

### 5.5 `job dispatch`

```
cpd job dispatch <key> [--plan <id>]
```

Starts one Ready Job now: the same step as engine E3 (cpd 1 §11, kept).

| Precondition | Code | Exit |
|---|---|---|
| the Job is Ready (§3.2) | `job_not_ready` | 3 |
| an `open` Job's PR has a stored `description` and, if not adopted (`head_ref` null), is treated as not Ready: the Job is not started; the same refusal at engine E3 as for `tier_not_allowed`, one `error` Event, the Job starts at the first tick after the description is stored (§9.5a); an `update` Job and an adopted PR's open Job need none | `description_required` | 3 |
| an `open` Job's PR's policy sets `pr.jira_required` and the PR has no stored `jira` and, if not adopted (`head_ref` null), is treated as not Ready: the Job is not started; the same refusal at engine E3 as for `tier_not_allowed`, one `error` Event, the Job starts at the first tick after `cpd pr edit <key> --jira <KEY>` (§9.5); an `update` Job and an adopted PR's open Job need none (pro-d127) | `jira_required` | 3 |
| its fingerprint matches | `locked_job_tampered` | 3 |
| the driver lock is free (§3.4 step 1) | `cas_lost` | 3 |
| a linear Attempt of the Job's PR is live (pro-d52, contract §3.7 E3 item 1) | `cas_lost` | 3 |
| every resource of the Job's Spec is still declared for the PR (contract §3.4 "Resources of a Job"); `details` `{job, pr, resource, declared, file}` | `resource_undeclared` | 3 |
| the Job's acquisition has a free slot in every pool: `workers` and every resource of the Spec, all or none, decided by the starter under the pools' guards after the driver lock and before the Attempt row exists (contract §3.4 "Resources of a Job"); a Job that is already a tenant (a retry) needs none; `details` `{job, resource, holders}`. It changes nothing: no Attempt and the Job stays `queued` | `resource_busy` | 3 |
| fewer than `concurrency` Attempts are not `ended` (an urgent Job: fewer than `concurrency + 1`, contract §3.7) | `no_slot` | 3 |
| the Job's Tier for the Attempt is one a Worker may have (§5, pro-d53, hs-d554): a Job stored on a Tier outside the range before the rule is not started here, as engine E3 does not start it and raises one `error` Event per such Job (contract §3.7 E3) | `tier_not_allowed` | 2 |

Effects (§3.4 "The driver lock"): take `locks/attempt-<plan>--<attempt>.lock`; **rows**, one transaction: `attempts` (`starting`, Tier from §9, `origin`), Job `running` and the `jira_transitions` row `in_progress` of the Job's PR (§21.11, J4a: the first Attempt of any Job of the PR queues it, later ones change nothing; engine E3's start is the same); **Event** `attempt-started` `{number, tier, origin}`; then spawn `cpd attempt run <attempt> --plan <plan>` with `setsid`, fd 3 = the lock, stderr to `run/<plan>/logs/attempt-<attempt>.log` and record `driver_pid` (a data write, no Event). If the spawn fails, the command ends the Attempt `failed` / `crashed` with `driver_pid` still null (`attempt-retry` at the same Tier, not counted against `retry_cap`, §3.4) and returns `spawn_failed` (exit 1).

```json
{"ok": true, "attempt": "api-schema-a1", "tier": "S2", "origin": "ladder", "pid": 81234}
```

Idempotent: no. A second call finds the Job `running` and is refused `job_not_ready`.

### 5.6 `job list`

```
cpd job list [--status <status>] [--plan <id>]
```

`--status` is one of §3.2's statuses and may repeat. Each Job carries `urgent` (a boolean) and `kind`. Effects: none.

```json
{"ok": true, "jobs": [{"key": "api-schema", "pr": "api", "title": "…", "status": "merged", "tier": "S2", "appended": false, "kind": "work", "urgent": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee…"}]}
```

### 5.7 `job show`

```
cpd job show <key> [--plan <id>]
```

Effects: none. Output: every field of the Job (§2.2; `urgent`, a boolean; `kind` and `tests`, `test_files` and `test_approved` where the kind has them; `resources`, the Spec's list (null when it has none); `waiting_for`, present only for a `queued` Job that is not a tenant (contract §3.4 "Tenancy": it has no Attempt yet) and that lists resources, or whose Ready turn finds no free Worker slot (it then has the entry for `workers`, contract §8.3): an array of `{"resource":"<name>","scope":"host"|"owner"|"repo","slots":<n>,"holders":[…]}` for each pool it needs that has no free slot right now, `scope` and `slots` those of the resolved declaration (contract §8.3) and `holders` the holders of contract §1.4 of the pool (`{slot, holder}`, command slots then Job tenants with `slot` null; `[]` when every pool has a free slot; the key is absent for every other Job, a retried Job included, which holds its resources). A Job in `waiting_for` is skipped, in order, by E3 (contract §3.7 "the skip order"): it waits without holding anything; the `needs` of an `open` or `update` Job is the derived list of contract §3.2), `needs`, `needed_by` and every Attempt with all its fields (`check_tail`, `gate_hook`, `gate_command`, `gate_exit`, `metrics`) and its Escalations (cpd 1 §11, kept).

```json
{"ok": true, "job": {"key": "worker", "pr": "api", "status": "failed", "failed_cause": "timed_out", "tier": "S2", "urgent": false, "fingerprint": "9f…", "retry_tier": null, "…": "…"}, "needs": ["api-schema"], "needed_by": ["ui"],
 "attempts": [{"id": "worker-a1", "status": "ended", "outcome": "failed", "cause": "timed_out", "check_tail": null, "escalations": []}]}
```

For an explore Job (`scout`, `research`) it shows `summary` (the Worker's summary, null until the Job is `merged`) and `artefact` (`run/<plan>/explore/<key>.md`, relative to `$CPD_HOME`) and `pr` is null (Plan `explore-roles`, R9).

`tamper` is the tamper report of §4.3, as in `plan show` (empty when the Job is intact). Both are read-only.

### 5.8 `job urgent`

```
cpd job urgent <key> [--plan <id>]
```

Marks an existing Job **urgent** (contract §2.2): engine E3 starts urgent Ready Jobs first, then by `jobs.seq`, across Plans and may start one urgent Job beyond `concurrency` (§3.7). A Lead or Manager runs it **only when the User explicitly asks**, usually for something that blocks all work or an incident. `urgent` is not a Locked field and in no fingerprint (§2.3), so it is allowed on a Locked Job: a `queued` one.

| Precondition | Code | Exit |
|---|---|---|
| the Job exists in the Plan | `job_not_found` | 4 |
| the Job is `queued` or `draft`; a `running`, `merged`, `failed` or `cancelled` Job is refused (*decided*: the flag only matters to scheduling and E3 only starts `queued` Jobs; `job retry` makes a `failed` one `queued`, then mark it) | `illegal_transition` | 3 |

Effects: **rows**, one transaction: `jobs.urgent = 1`. **Event** `job-edited` `{fields: ["urgent"]}` (no new Event kind). Nothing else changes: status, fingerprint, `appended` and Dependencies stay as they are. There is no command to clear the flag. And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "job": {"key": "hotfix", "status": "queued", "urgent": true}, "fields": ["urgent"]}
```

Idempotent: yes; a Job that is already urgent writes nothing and returns `"already": true` with `"fields": []`.

### 5.9 `job approve-tests`

```
cpd job approve-tests <job> --path <p> [--path <p> …] --reason <text> [--plan <id>]
```

The Lead's approval for an `impl` Job to change a red test (hs-d341; contract §2.2 "Job kinds", §7.5). The files of the `test` Jobs an `impl` Job needs are guarded: its submit is refused `tests_tampered` (§6.1) when it changes one. A Worker that needs a red test changed asks the Lead (`cpd ask`); the Lead decides and, to allow it, runs this. **The Lead only**, through `CPD_ATTEMPT` (hs-d497): the command is refused `not_a_lead` when `CPD_ATTEMPT` is set, which cpd sets in every Worker's pane (§6.5). That is all the check there is: the command is **unauthenticated, like `cpd answer`** (a Worker that unsets the variable gets through) and the approval is visible afterwards in the `job-edited` Event (`fields: ["test_approved"]`, with the `--reason`).

| Flag | Meaning |
|---|---|
| `<job>` | the `impl` Job |
| `--path <p>` | repeated: a path or, ending in `/`, a directory prefix, that is guarded: equal to, or inside, an entry of the `test_files` of a `test` Job the `impl` Job needs |
| `--reason <text>` | why (§1.5); it is in the Event's `text` |

| Precondition | Code | Exit |
|---|---|---|
| `CPD_ATTEMPT` is not set | `not_a_lead` | 3 |
| the Job exists | `job_not_found` | 4 |
| its kind is `impl` | `not_an_impl_job` | 3 |
| its status is `queued`, `running` or `failed` | `illegal_transition` | 3 |
| every `--path` is guarded; `details.paths` lists those that are not | `path_not_guarded` | 3 |

Effects: **rows**, one transaction: `jobs.test_approved` becomes the union of its old value and the paths. **Event** `job-edited` `{fields: ["test_approved"]}`, its `text` naming the paths and the reason. It takes effect at the Job's next check, a running Attempt's included (contract §7.7). `test_approved` is not a Locked field and is in no fingerprint (§2.3). And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "job": {"key": "api-impl", "status": "running", "kind": "impl"}, "approved": ["cli/tests/it/pr_open_job.rs"], "fields": ["test_approved"]}
```

Idempotent: yes; paths already approved write nothing and a call with none new returns `"already": true`.

## 6. `attempt`

### 6.1 `attempt submit`

```
cpd attempt submit <attempt> [--summary-file <path>] [--plan <id>]
```

Run by the Worker, from its worktree. Preconditions are §7.3's list, in that order, after the Attempt's own state:

| Precondition | Code | Exit |
|---|---|---|
| the Attempt is `running` (already `merging` or later with `submitted_sha` = the worktree's HEAD: `already`) | `attempt_not_running` | 3 |
| the caller's cwd is inside the Attempt worktree, on the Attempt branch; for a linear Attempt, the worktree is the Lead worktree and the branch is the Lead Branch | `wrong_worktree` | 3 |
| clean tree, untracked files included | `dirty_tree` | 3 |
| at least one commit ahead of `base_sha`; **for an `open` or `update` Job none is required** (the base may not have moved, so there may be nothing to commit, hs-d340) | `no_commits` | 3 |
| the tamper check passes (§7.5; a protected path whose Attempt version equals the Lead tip's or `origin/<base>`'s, as a base merge brings it, is allowed); `details.paths` lists the protected paths touched that are not allowed | `gate_tampered` | 3 |
| (kind `impl`) it changes no path guarded by the `test_files` of a `test` Job it needs, unless `job approve-tests` (§5.9) approved it; `details.paths`, `details.jobs` (contract §7.5, §7.7) | `tests_tampered` | 3 |
| (kind `test`) every `test_files` entry matches a path the Job changed (`git diff --name-only --no-renames <lead branch>...HEAD`); `details.paths` lists the entries that match none | `test_files_missing` | 3 |
| (kinds `test`, `impl`) the PR's repo policy has `[test-commands]` (none when there is no repo file or the file lacks the table) | `tests_unconfigured` | 3 |
| (kinds `test`, `impl`) the repo's `build` exits zero; `details` `{job, tests, tail, log}` | `tests_build_failed` | 3 |
| (kinds `test`, `impl`; the repo's `[test-commands]` has `list`) the `list` command exits zero for every named test, run once per name with the variable `test`; `details` `{job, tests, tail, log}`, `tests` the names that match nothing (hs-d497) | `tests_not_found` | 3 |
| (kind `test`) the repo's `run` of the named tests exits non-zero: they fail, for a reason other than a build error | `tests_not_red` | 3 |
| (kind `impl`) the repo's `run` of the derived tests exits zero | `tests_failed` | 3 |
| (kinds `test`, `impl`) the commands ran (not killed, not `hook_invalid`) | `tests_error` | 1 |
| (kinds `open`, `update`) the PR's policy resolves (contract §8.3): its Context has an owner | `owner_unset` | 3 |
| its owner has a policy file; its policy files are valid | `owner_unconfirmed`, `policy_invalid` | 1 |

Effects:
- **One check span** (contract §7.7, hs-d679): the checks of the table run inside ONE span, `what` `checks`, in this order: for an `open` or `update` Job the policy read (the policy rows), then the tamper check, the red-test guard and the `test` or `impl` kind check. The Gate runs at the merge, not in the span (contract §7.4, hs-d679). The preconditions of §7.3 run before it.
- **Hooks:** none at submit. The quick check (`pre-commit`) and the Gate (`pre-merge`) run at the merge, not here (§7.3, §7.4 step 6, hs-d679), and their logs are written there.
- **Kind checks** (contract §7.7, hs-d340, hs-d341), for the kinds that have one: a `test` or `impl` Job's `[test-commands]` commands (`build`, then `run`; output in `run/<plan>/logs/<attempt>.tests-build.log` and `<attempt>.tests-run.log`; with `list`, once per name between them, `<attempt>.tests-list.log`). An `open` or `update` Job has none: its only check beyond the tamper check is the policy read above. Its title, ticket, closed-pull-request, base and Gate checks are the merge's and end the Attempt there, not here (contract §7.4 steps 5, 6 and 8b). The span holds `locks/check-<plan>--<attempt>.lock`; it appends `attempt-status` with `check` `start` (before the first check) and `end` (contract §5.2; on a pass in the transaction of the Rows below, on a refusal before the error returns), so the Attempt's timeout and the stall kill do not count it, the tamper check included. A refusal changes nothing else: the Worker fixes, commits and submits again and the checks run again.
- **Rows**, one transaction with a compare-and-swap on `status = 'running'` and on the HEAD checked: Attempt `merging`, `submitted_sha`, `submitted_at`; the Job stays `running`; the span's `end` Event is appended in the same transaction. Submit writes no Gate column: `check_tail`, `gate_hook`, `gate_command` and `gate_exit` are written by the merge's Gate, and only when it fails `gate_failed` (contract §7.4 step 6). **Event** `attempt-status` `{from: "running", to: "merging"}`.
- The driver sees `merging` at A5 and starts the merge (A8, §7.4). An `open` or `update` Job's merge does not rebase, pushes the Lead Branch and, for an `open` Job, opens the pull request and moves the PR in step 10's transaction (contract §7.4, §7.7): `pr-opened` and `plan-phase` follow its `job-merged`.

```json
{"ok": true, "attempt": "api-schema-a1", "submitted": true, "sha": "a1b2c3d…", "next": "Stop now. Do not make more changes."}
```

Idempotent: yes, as stated; a HEAD that moved after a successful submit is refused `attempt_not_running`.

**An explore Attempt** (`scout`, `research`; Plan `explore-roles`, R8). The Worker runs `cpd attempt submit <attempt> --summary-file <path>` from its cwd, the explore directory `$CPD_HOME/run/<plan>/explore/`, which is not a worktree. The rows above that name a worktree, a commit, a branch, a check or the Gate do not apply to it; `attempt_not_running` does. Then, in this order, each refused with exit 3 and nothing changed:

| Precondition | Code | Exit |
|---|---|---|
| there is no non-empty artefact at `$CPD_HOME/run/<plan>/explore/<job-key>.md` | `artefact_missing` | 3 |
| there is no `--summary-file`, or the file is missing or empty | `summary_missing` | 3 |
| the summary has more than 1500 characters, counted as characters and not bytes; `details` `{length, cap}`, `cap` 1500. It is never truncated: the Worker shortens it and submits again | `summary_too_long` | 3 |

Effects, for an explore Attempt that passes:
- **Checks that do not run:** the quick check, the tamper check, the red-test check, the kind check and the Gate. There is no worktree and no commit to check.
- **The atomic end** (one transaction, no compare-and-swap, so no merge-recovery case): the Attempt `ended` / `merged`; the Job `merged` with `jobs.summary` the summary; the SHA-256 (hex) of the artefact in the Job's `merged_sha`, the Attempt's `merged_sha` and `submitted_sha` and the `job-merged` Event's `sha` (no commit exists, so the existing CHECKs on merged rows hold). The `job-merged` Event's `data` is `{sha, tier, kind, summary, artefact, turn}` (text in §15.3).
- **A driver that died before the transaction** leaves an ordinary dead Attempt, which E2 ends `crashed`; the submit never stops the Worker (the driver's A9 does, or E2 for a dead driver).

### 6.2 `attempt read`

```
cpd attempt read <attempt> [--lines <n>] [--plan <id>]
```

A readable transcript (cpd 1 §11, kept): one line per assistant text line (`assistant: …`) and one per tool call (`tool: <name>`), in session order, from the Worker's session file. `--lines` (default 40) keeps the last n. A partial last line of a file being written is skipped.

| Precondition | Code | Exit |
|---|---|---|
| the Session is registered and its `file` is known and exists | `session_not_found` | 4 |
| every complete line is a JSON session entry | `session_malformed` | 1 |

```json
{"ok": true, "attempt": "api-schema-a1", "session": "c1a2…", "file": "/home/clayton/.pi/agent/sessions/…/…_c1a2….jsonl", "total": 412,
 "lines": ["tool: bash", "assistant: The schema compiles; committing."]}
```

Read-only.

### 6.3 `attempt show` and `attempt list`

```
cpd attempt show <attempt> [--plan <id>]
cpd attempt list [--job <key>] [--active] [--plan <id>]
```

`show` returns every field of the Attempt (§2.2), its Escalations and the paths it uses: `spec_path`, `check` (`run/<plan>/logs/<attempt>.pre-commit.log`, null when no quick check ran), `log` (`run/<plan>/logs/attempt-<attempt>.log`) and the Worker's session file. Its `attempt` carries `runner` (`pi` or `durable`) and `worker_pid` (null for `pi` and before a `durable` Worker starts) and so do the attempts of `job show` (contract §3.4 "The Worker runner and its seam", §4.4). The Gate fields `check_tail`, `gate_hook`, `gate_command` and `gate_exit` are set exactly when the Attempt ended `gate_failed` and are null otherwise, `gate_error` included (contract §2.2, §7.4 step 6). `list` returns `id`, `job`, `number`, `tier`, `origin`, `status`, `outcome`, `cause`, `started_at`, `ended_at`; `--active` keeps those not `ended`.

```json
{"ok": true, "attempts": [{"id": "api-schema-a1", "job": "api-schema", "number": 1, "tier": "S2", "origin": "ladder", "status": "ended", "outcome": "merged", "cause": "merged", "started_at": "…", "ended_at": "…"}]}
```

`attempt show`:

```json
{"ok": true, "attempt": {"id": "api-schema-a1", "job": "api-schema", "status": "merging", "tier": "S2", "…": "…"},
 "escalations": [], "paths": {"spec": "…/specs/api-schema-a1.md", "check": "…/logs/api-schema-a1.pre-commit.log", "log": "…/logs/attempt-api-schema-a1.log", "worker_file": "…"}}
```

Both are read-only.

### 6.4 `attempt run` (internal)

```
cpd attempt run <attempt> --plan <id>
```

The Attempt driver, steps A1–A9 of §3.4, started detached by E3 or `job dispatch` with the driver lock as fd 3. It starts at nice 19 and I/O class idle (§6.5 hs-d528), set at spawn so its process tree (the Worker's pi, cargo, rustc and every tool) inherits it. Not for agents.

| Precondition | Code | Exit |
|---|---|---|
| fd 3 is the driver lock file (same device and inode) | `driver_lock_missing`, printed to its log; nothing is written (§3.4 step 4) | 1 |
| the Attempt is `starting` | `illegal_transition` | 3 |

Effects: everything of §3.4 A1–A9, §7.2, §7.3, §7.4: the worktree and `pre-start`; the rendered Worker Spec (§16.1); the Worker, started by the Attempt's `runner` (contract §3.4 "The Worker runner and its seam"): for `pi` its window `cpd-work:<plan>--<attempt>` (`new-window` with the Worker's argv of §6.5, `@cpd_session`) and the Session rows (`session-registered`, `session-moved` `{place: "work"}`); for `durable` a child process with no window and no `session-moved`, its `worker_pid` stored, restarted and stopped as that section says; the nudge (`attempt-nudge`); the stall kill (a Worker's session file idle for `timeouts.stall_seconds` inside one tool call: pi's descendant processes are killed, pi is not, the Attempt goes on; one driver-log line, no Event; contract §3.4 A4); the merge, with no lock (hs-d243: rule S and rule P, contract §7.4); the end (`metrics` from the end report, the Worker's stop by runner, worktree removal on `merged`). Each status change is one transaction with the Event of §3.4.

Output, printed when it exits (stdout goes to its log): `{"ok": true, "attempt": "api-schema-a1", "outcome": "merged", "cause": "merged"}`. Unexpected failures append `error` `{source: "attempt run", code, message, details}` and end the Attempt `failed` / `crashed` when it is not already ended. Idempotent: no; a second driver cannot get fd 3.

## 7. `review`

Removed by R19 (no Job-level Reviewer).

### 7.1 `review decide`

Removed by R19 (no Job-level Reviewer).

## 8. `ask` and `answer`

Escalations from a Worker to its Lead (§2.2, §3.5). The Context's `context ask` / `context answer` (§3.4, §3.5) are records, not Escalations.

### 8.1 `ask`

```
cpd ask <attempt> <question> [--timeout-minutes <n>] [--plan <id>]
```

Run by the Worker; it blocks until the answer. `--timeout-minutes` defaults to `ask_minutes`.

| Precondition | Code | Exit |
|---|---|---|
| the Attempt is `running` | `attempt_not_running` | 3 |
| answered before the timeout | `ask_timeout` | 5 |

Effects:
1. **Rows**, one transaction: `escalations` (`<attempt>-q<k>`, `open`, `asker_pid` = its own pid); the Worker Session `waiting` with label `φ: question to the Lead` (it waits for its Lead, not for the User: hs-d303; contract §3.6). **Events** `escalation` `{escalation, question}` (the Lead's adapter delivers it with a turn) and `session-state`.
2. It polls its row every 500 ms (a read; cpd 1 §11, kept).
3. Answered: one transaction setting the Worker Session `working` (`session-state`); prints the answer. Timeout: one transaction, `status = expired`, `expired_at`, **Event** `escalation-expired` `{escalation, reason: "timeout"}`, Session `working`; prints `ask_timeout`. If the process dies, E1 expires it (`reason: "asker_dead"`) and, in the same transaction, sets the Worker Session `working` (`session-state`, `waiting` to `working`) unless another Escalation of the Attempt is still `open`.

```json
{"ok": true, "escalation": "api-schema-a1-q1", "answer": "Use the v2 field names."}
```

**How a Worker runs it** (pro-d274). As a call of its own, in the foreground, with the bash tool's `timeout` input set to 7200 seconds (above the default `ask_minutes`, 60 minutes, so that `cpd ask` ends its own wait), never under the `timeout` command, in a pipeline or in the background. While the Escalation is `open` the Worker's pi sits inside a silent tool call: the stall rule spares it (contract §3.4 A4) and an explore Worker's guard lets it wait (§15.22). A bash call that ends before the Escalation (a harness timeout, a kill) leaves it `open` until E1 expires it; the Worker has the tool's error, its Attempt is not failed and it is sent no text, and it asks again (a new Escalation).

Idempotent: no. Each call is a new Escalation; ids are never reused (§3.5).

### 8.2 `answer`

```
cpd answer <escalation> <answer> [--plan <id>]
```

Run by the Lead.

| Precondition | Code | Exit |
|---|---|---|
| the Escalation is `open` | `escalation_answered`, `escalation_expired` | 3 |

Effects: **rows**, one transaction: `status = answered`, `answer`, `answered_at`. **Event** `escalation-answered` `{escalation}`. The waiting `ask` sees it at its next poll. And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "escalation": "api-schema-a1-q1", "attempt": "api-schema-a1", "answered_at": "…"}
```

Idempotent: no; §3.5 refuses `answer` on an Escalation that is not `open`, so a second answer is `escalation_answered`.

## 9. `pr`

A Plan's PRs (R34, §2.2 PR, §3.8) and their pull requests, through `gh` (§7.6). `gh` failures are `gh_failed` (exit 1) with its stderr tail. Every command here works on **one PR**: `pr open`, `pr rebase`, `pr push`, `pr view`, `pr reply`, `pr rerun` and `pr post` select it by `--pr <key>` (§1.3: omitted, the Plan's one PR that is not `cancelled`); `pr add`, `pr edit`, `pr cancel` and `pr start` (§9.4–§9.7) take its key as their argument; `pr poll` has no `--pr`: it polls every open pull request of the User. `<repo>`, `<base>`, `<lead branch>` and `<lead worktree>` are the selected PR's (§7); `gh` runs with cwd = its Lead worktree, so it addresses the PR's repo.

### 9.1 `pr open`

**The repair** (hs-d340). A PR is opened by its `open` Job (contract §7.7), not by the Lead: engine E4 adds the Job to every started PR, its Worker merges the base and its submit runs the Gate, pushes and opens the pull request in its merge. `pr open` stays for what that cannot cover: a PR whose `open` Job is `failed` (the Lead decides between `job retry <job> --tier <T>` (§5.4) and this command), an engine that is down, or a PR whose `open` Job the Lead cancelled (`job cancel` stands, §5.3: E4 adds no new one, so this is how the PR is opened later; to give it up, `pr cancel`, §9.6). It is refused `open_job_live` while the PR has an `open` Job that is `draft`, `queued` or `running` (hs-d573) and its `jobs_not_done` counts every Job of the PR except its `open` and `update` Jobs. Nothing else here changes: the steps and refusals below are the ones the Job's submit and merge share (contract §7.6 steps 0, 1, 3, 4).

`pr open` belongs to the **Deploying** step of the Dispatch stage (contract §3.9): it runs the Gate, then opens the pull request. Review follows the opening, not the Gate. **The Gate runs only when the PR's `gate.merge` is `local` (for `pr open` and `pr push`) or `gate.pr` is `local` (for `pr rebase`)** (contract §8.3, hs-d292; the PR's policy key, `Repo(<owner>, <name>)`): with `none`, cpd's default (`core/src/policy/default.toml`), `pr open` runs no Gate, the repo's CI is the check and the two Gate rows below do not apply.

```
cpd pr open [--pr <key>] [--plan <id>]
```

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR to open (R34, A14), selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one. Only as the repair above |

| Precondition | Code | Exit |
|---|---|---|
| the PR is `dispatch` (`open` or `merged`: `already`; `waiting` or `cancelled`) | `illegal_transition` | 3 |
| the PR has no `open` Job that is `draft`, `queued` or `running`; `details.job` (hs-d340, hs-d573) | `open_job_live` | 3 |
| every Job of the PR but its `open` and `update` Jobs `merged` or `cancelled`, at least one `merged` | `jobs_not_done` | 3 |
| the PR has a stored title (hs-d285): a PR that existed before migration `pr-title` has none and there is no fallback to the Plan's title. The message says to run `cpd pr edit <key> --title "<text>"`. Checked before the Gate | `pr_title_required` | 3 |
| the PR has a stored description (§9.5a): a PR without one is refused until the Lead runs `cpd pr describe --pr <key> --file <body.md>`. The message says the same. Checked before the Gate | `description_required` | 3 |
| the PR's policy sets `pr.jira_required` (B4, pro-d127) and the PR has a stored `jira` (§9.4), unless the PR is adopted (`head_ref` set, exempt as for the description); the message says to run `cpd pr edit <key> --jira <KEY>`, then open it again. Checked before the Gate, after the description | `jira_required` | 3 |
| the newest pull request of the Lead Branch is not closed without a merge (a merged one is adopted, below); `details.number`, `details.url` | `pr_closed` | 3 |
| **stacked** (the PR's base PR, contract §2.2, is not `merged`: a merged one was unstacked first, below): the base PR's Lead Branch, `<base>`, is on the remote (`git ls-remote --exit-code --heads origin <base>`), checked before the fetch and the Gate; `details` is `{pr, base, base_pr}` (hs-d324) | `base_not_found` | 4 |
| after `git fetch origin <base>`, G (the Lead Branch's head, read once: the commit that is gated, pushed and opened) does not conflict with `origin/<base>` (`git merge-tree --write-tree --name-only`; checked before the Gate, with `gate.merge` and `gate.pr` are `none` as with `local`; hs-d490, contract §7.6 step 1). A base that merely moved ahead is no refusal. `details` is `{pr, base, base_sha, sha, files, next}`, `sha` = G, `files` the conflicted paths, `next` = `["git -C <lead worktree> merge --no-edit origin/<base>", "cpd pr open --pr <key>"]` | `base_conflict` | 3 |
| the PR's policy resolves (contract §8.3 "Validation", `details` as there): its Context has an owner | `owner_unset` | 3 |
| its owner has a policy file | `owner_unconfirmed` | 1 |
| its policy files are valid | `policy_invalid` | 1 |
| the `pre-start` passes before the Gate runs; `details.tail` is its last 60 lines, `details.log` the full log `run/<plan>/logs/pr-<key>.pre-start.log` | `gate_error` | 1 |
| the Gate passes (§7.3, contract §7.4, hs-d679); `details.tail` is its last 60 lines, `details.log` the full log | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |
| the PR's status is still `dispatch` at the transaction (another `pr open` committed first) | `cas_lost` | 3 |
| the caller is not an agent, OR the incident is off, OR the resolved PR's Plan is not held (hs-d643): `CPD_SESSION` set and non-empty names an agent; a held Plan is in the incident; `details` is `{pr, plan, incident}` | `incident_hold` | 3 |

The Lead worktree's local changes refuse nothing: the Gate does not run there (hs-d243; `dirty_target` is no longer a refusal of this command).

**A stacked PR** (hs-d324, hs-d325, hs-d301, contract §3.8 "Stacked PRs" step 7): when its base PR is `merged`, the command first **unstacks** it, **before the lookup and before every precondition below, `base_not_found` included** (its `base` becomes the base PR's own `base`, normally `main`, walking a chain of merged base PRs to its end; in its own transaction, **Event** `pr-base-changed` with `by` `pr open`, one per step, §5.2; the unstack is kept even when a later check refuses) and goes on with the new base, because GitHub deleted the merged base PR's branch: that branch is never looked up. The poll already unstacks such a PR when it records the base PR's merge (`by` `poll`, contract §7.6 step 6), so this is the case where no poll did yet. Otherwise `<base>` is the base PR's Lead Branch, so the conflict check, the Gate (its `target` the base PR's Lead Branch) and `gh pr create --base <base>` all run on top of it and GitHub will not land the stacked pull request before the base PR. The Lead Branch contains the base PR's work, because it was started from the base PR's head (§9.7); a base PR that has moved on since is merged in only when it conflicts (`base_conflict`, `next`; hs-d490). Opened ready, never a draft, like every PR.

**Ready, never a draft** (hs-d329): the pull request is opened only after the conflict check above and, with `gate.pr` is `local`, the Gate; `gh pr create` never has `--draft` and no cpd command makes a pull request a draft or ready (contract §7.6 "Ready, never a draft"). Under `ci` the conflict check still applies and the pull request opens ready (the Lead's reading of hs-d329: `ci` skips only the Gate).

**An adopted PR** (`head_ref` set, §9.4): the pull request is found by `gh pr list --head <head_ref> --state all`, the Gate runs as above and rule P pushes to `refs/heads/<head_ref>`, plain, never forced; `gh pr create` is never run, the pull request that exists is adopted (its own title kept). `push_rejected` (§9.9) names `cpd pr rebase` (the catch-up, §9.3) as the next step. Steps 0 and 3 and the merged check name `<head_ref>`, not the Lead Branch.

Effects:
- **Title** (hs-d285): the pull request is titled with the PR's stored title, passed through the one seam in `core/src/pr/` that turns it into the GitHub title (unchanged today; hs-d277 may change it later). The title no longer comes from the Plan's title and a Plan with several PRs no longer gets ` (<key>)` appended.
- **Body** (§9.5a): the pull request body is the PR's stored description, verbatim, set by `cpd pr describe`.
- **No lock** (hs-d243, contract §7): merges into the PR go on during the command; no R first (an unrecorded merge leaves its Job `running`, which `jobs_not_done` refuses).
- **Adopting a merged pull request** (before the Gate; contract §7.6 `pr open` step 0): `gh pr list --head <lead branch> --state all --json number,url,state`, the newest decides. When it is `MERGED`, the Gate and the push are skipped (hs-d267 forbids skipping the Gate only when *opening* a pull request where `gate.pr` is `local`, hs-d292): one transaction records the pull request (its `pull_requests` row, `state` `merged`; the PR's `pr_number`, `pr_url`, `pull_request`) and moves the PR `dispatch → open → merged` exactly as `pr poll` does when it sees a merge: `pr-merged` (no `pr-opened`); `plan-phase` `dispatch → deploy` when every PR that is not `cancelled` is then `open` or `merged` and `plan-done` (`deploy → done`) when every one is `merged`. Before the `plan-done` Event, every pending review of the Plan is closed `superseded`, reason "Plan review r<k> of Plan P is superseded: the Plan is done." The next `pr poll` cleans up the merged PR's worktrees. The output carries `"adopted": "merged"`. When it is `CLOSED` without a merge the command is refused `pr_closed` (reopen it on GitHub, or move the work to a new branch); when it is `OPEN`, it is adopted after the Gate and the push, as below.
- **Conflict check** (contract §7.6 step 1, hs-d490): `git -C <repo> fetch origin <base>` (no `origin`: skipped, `refs/heads/<base>` used), `B = rev-parse origin/<base>` read once, `G = rev-parse refs/heads/<lead branch>` read once, then `git merge-tree --write-tree --name-only B G`; exit 1 is `base_conflict` with nothing run, pushed or written, any other failure `git_failed`. No ancestry test: a base that moved ahead is not a refusal and starts no Gate. It also applies to the adoption of an open pull request, not to a merged one. B is not fetched again after the Gate.
- **Hooks:** with `gate.pr` is `local` (contract §8.3, hs-d292), the Gate runs in the Lead worktree at G: `pre-start` (output `run/<plan>/logs/pr-<key>.pre-start.log`), then `pre-merge` from the PR's policy (output `run/<plan>/logs/pr-<key>.pre-merge.log`), with the template variable `branch` = the Lead Branch. With `ci`, none. On `gate_failed` or `gate_error` nothing is pushed, no row is written and the PR stays `dispatch`: the Lead appends fix Jobs to it (`job add --pr <key>`) and runs `pr open --pr <key>` again once they are Merged (hs-d679, hs-d680).
- **Re-read:** after the Gate the PR is read again; when it is no longer `dispatch` (a concurrent `pr open` opened it), the first precondition answers (`already`) and nothing is pushed.
- **git:** rule P (contract §7.4): `git -C <lead worktree> push --porcelain origin G:refs/heads/<lead branch>` (no `-u`). The Lead Branch is usually on the remote already: the PR's start pushed it and every merge pushed it (§7.2, §7.4, R35), so this push is normally a no-op and it pushes again what an earlier `push_failed` left behind. A rejected ref (any `!` line, rule P) when the remote tip already contains G (a merge during the Gate pushed a descendant) is a success with `pushed` false; any other git failure is `git_failed`.
- **gh:** the lookup above (`--state all --json number,url,state,createdAt`, only considering pull requests whose `createdAt` is at or after the row's `prs.started_at`, except for an adopted PR); when it found an open pull request (an earlier run died before its transaction) that one is adopted; else `gh pr create --base <base> --head <lead branch> --title <title> --body <description>` (never `--draft`). When `gh pr create` fails, the `gh pr list` lookup runs again with the same filter and a pull request it finds (a concurrent `pr open` created it) is adopted; otherwise the failure stands. `<title>` is the PR's stored title (Title, above); `<description>` is the PR's stored description (§9.5a), verbatim.
- **Rows**, one transaction, with a compare-and-swap on the PR's `status = 'dispatch'`: the `pull_requests` row of the pull request (inserted from `gh`'s URL, `repo`, `number`, `url`, the title given to `gh`, `draft` 0, `state` `open`, `review_decision` and `checks` `none`, `mergeable` and `merge_state` `unknown`; kept as it is when the poll stored it first; hs-d96, D6), then the PR `status = open`, `pr_number`, `pr_url`, `pr_opened_at`, `pull_request` (the row's id). **Event** `pr-opened` `{pr, number, url}`. When every PR that is not `cancelled` is now `open` or `merged` (A10), the same transaction sets `plans.phase = deploy` and appends `plan-phase` `{from: "dispatch", to: "deploy", prs: [{key, number, url}]}` (§3.1); otherwise the Plan stays in `dispatch`.

`phase` is the Plan's phase after the command.

`sha` is G, the gated commit; `pushed` is false when the remote already had G or a descendant of it; `remote` is the remote branch's tip after the push, past G when merges that landed during the Gate were pushed (hs-d243).

```json
{"ok": true, "plan": "pay-4521", "phase": "dispatch", "pr": {"key": "api", "status": "open", "pr": {"number": 812, "url": "https://github.com/acme/pay/pull/812", "opened_at": "…"}}, "sha": "77aa01e…", "pushed": false, "remote": "77aa01e…"}
```

A merged pull request adopted (no Gate, no push), `phase` after the transition, `pr.status` `merged`:

```json
{"ok": true, "adopted": "merged", "plan": "pay-4521", "phase": "done", "pr": {"key": "api", "status": "merged", "pr": {"number": 812, "url": "https://github.com/acme/pay/pull/812", "opened_at": "…"}}}
```

Idempotent: yes, as stated.

### 9.2 `pr poll`

```
cpd pr poll [--plan <id>]
```

The PR poller (hs-d96, D8; contract §7.6): one poll of **all of the User's open pull requests**. Run by engine E5 every `pr_poll_seconds` as a subprocess, without `--plan` (§3.7); also by a Lead, right after `pr open` or whenever it needs the current status (D12).

| Flag | Meaning |
|---|---|
| `--plan <id>` | adds that Plan's PRs, each with its `pull_request` row, to the output (`--plan` or `CPD_PLAN`; neither: no `prs` key). It does not narrow the poll: every open pull request is polled anyway. There is no `--pr` |

| Precondition or failure | Code | Exit |
|---|---|---|
| the lock `locks/pr-poll.lock` within 60 s | `lock_timeout` | 5 |
| `--plan` names no Plan | `plan_not_found` | 4 |
| a `gh` call fails, times out (15 s each), prints an empty or non-JSON body, answers the search with `errors`, or answers the lookup with no `data`; `details` is `{argv, exit, stderr, call, transient, reason}`: `call` is `search` or `lookup`, `transient` is true for an HTTP 5xx (`gh`'s `HTTP 5xx` in its message), a 15 s timeout, or a cut response (`gh` exited non-zero with `unexpected end of JSON input` in its stderr and no complete stdout, hs-d354), false for everything else and `reason` is `HTTP <status>`, `timeout`, `unexpected end of JSON input` or null (contract §7.6 step 4, hs-d354) | `gh_failed` | 1 |

There is no phase precondition: a Plan in any phase gets the poll.

**A transient `gh_failed`** (`details.transient` true) is still exit 1 and writes nothing (a failed deploy read, which comes after the rows, leaves them): the command never retries, waits or keeps state, so a Lead's `pr poll` by hand fails at once. The tolerance is engine E5's (contract §3.7): it logs a `watch.log` line, polls again at the next due time and raises an `error` Event only after `watch.pr_poll_transient_minutes` (default 10) of the same call failing (the cut response counts, with `reason` `unexpected end of JSON input`), keeping the first failure's time in `run/pr-poll-failing.json`. Every other `gh_failed` is an `error` Event at once.

**A gh call with cwd `$CPD_HOME`:** `$CPD_HOME` is not a git repository, so a gh call cpd makes there names its repo itself: in the endpoint (`repos/<owner>/<repo>/...`), in GraphQL (`repository(owner:, name:)`) or with `--repo`. A bare `gh pr ...` and the `{owner}/{repo}` placeholders (which gh fills from the cwd's git repository) fail there. The calls this covers are the poll's, the deploy read's, `pr edit --title`'s, `pr describe`'s (§9.5a, which takes its repo from the PR's row) and the team review lookup's. Calls made in a Lead worktree or a repo checkout resolve through its `origin` and may rely on it.

Effects (contract §7.6 gives the query, the mapping and the caveats):
- **Lock:** `flock` on `locks/pr-poll.lock`, held for the whole poll, so two polls never overlap.
- **gh:** one `gh api graphql` call with two searches (`is:pr is:open author:@me archived:false sort:created-asc` and `review-requested:@me is:pr is:open archived:false sort:created-asc`, the oldest 100 each; every pull request is read with its `baseRefName`, for the base follow below and with `latestOpinionatedReviews(first: 20) { nodes { state author { __typename login } } }` after `reviewDecision`, pro-d158: one point more per poll, measured 2026-10-28, unverified beyond that one measurement; contract §7.6 step 1), then, for the pull requests where at least one holds (a) the PR is owned and neither search listed it, (b) `authored` is 1 and the `author:@me` search did not list it, or (c) `review_requested` is 1 and the `review-requested:@me` search did not list it, one direct read call with one alias per pull request (hs-d565, contract §7.6 step 3). No candidate, no call. **The feedback read** (pro-d68, contract §7.6 step 3a): every pull request also reads `updatedAt` (still 1 point); for each pull request that the rule of contract §7.6 step 3a makes due (pro-d231), ONE more `gh api graphql` call reads the review threads, issue comments and reviews of all such pull requests, with `viewer { login }` and the poll stores the items (contract §2.2 "Feedback item") in `feedback` with `updated_at`. More than 100 threads, comments, reviews or comments in a thread is `gh_failed` (`details` name the PR and the number; nothing is written); a pull request no Plan owns is never read. Stdout is parsed whatever the exit code (gh exits 1 when an alias is `NOT_FOUND`). The direct read adds `viewerDidAuthor` and `viewerLatestReviewRequest { id }` (both required); answers tell what changed. **The checks read** (pro-d75, contract §7.6 step 3b): for each pull request that is owned and whose checks rollup is `failed` with `pr.feedback.checks` `any`, or `pending` with a non-empty `pr.human_gated_contexts`, or (owned or not) `passed` or `none` with a non-empty `pr.required_checks` of the repo's policy (contract §7.6 step 3b's three triggers, hs-d743), ONE more `gh api graphql` call reads the checks: CheckRun and StatusContext nodes, with `startedAt` and `checkSuite { workflowRun { workflow { name } } }` on a CheckRun and `createdAt` on a StatusContext (pro-d230); it applies the newest-run reduction of contract §7.6 step 3b (pro-d230: the newest run of each check wins; the reduction needs every entry, so the pagination of step 3b stays), then the per-check mapping (FAILURE, TIMED_OUT, ACTION_REQUIRED, STARTUP_FAILURE, ERROR are failing; CANCELLED is not; pending checks never fail), with the human-gated rule of contract §7.6 step 3b (pro-d151, pro-d153) and its required-names rule (hs-d743) and stores the result in the PR's `checks` field (contract §7.6 step 3b, §2.2). The pending trigger makes this read every poll while the rollup stays `pending`: +1 point whatever the number of pull requests, the aliases sharing one call (unverified against the hourly budget). The same call reads the pull request's labels, `labels(first: 100) { nodes { name } }` beside `commits`, at 0 extra rate-limit points (measured, pro-d229: the query cost 1 with them), for the excused rule of contract §7.6 step 6 (pro-d216, pro-d229); a reply without `labels` is `gh_failed`. More than 100 contexts is `gh_failed` (details name the PR and the number; nothing is written); a pull request that is not owned, or matches no trigger, makes no such call.
- **The deploy read** (pro-d100; contract §7.6 step 8), after the rows below are written: for each workflow a repo policy file declares in `[deploys] workflows`, `gh api "repos/<owner>/<repo>/actions/workflows/<file>/runs?status=waiting&per_page=100"` and for each run of the User's (`triggering_actor.login` equal, case-insensitively, to `gh api user --jq .login`) one `gh api repos/<owner>/<repo>/actions/runs/<id>/pending_deployments`; the runs with a pending deployment the User can approve are written to `run/deploy-runs.json` (contract §1.4). A failure is `gh_failed` with `call` `lookup`, exit 1, the PR poll's own writes standing; the failed workflow keeps its earlier runs.
- **Rows**, one transaction for every change, none when nothing changed: `pull_requests` rows inserted or updated (`changed_at` = now) when a value changed: `authored` and `review_requested` set by searches that list the PR (hs-d565); the direct read updates them per what it found (§9.2, contract §7.6 step 4). The poll does not delete rows (hs-d565): a row stays while it is in either search or a PR points to it; when it is in neither and no PR points to it, both flags 0 and no owner stays. The `authored` rows are what the Dispatch column shows (§13.2, §17); the Inbox kinds read them all (§21).
- **Events**, in that transaction, for the Plan whose PR points to the pull request only: **Event** `pr-feedback` (data of contract §5.2) when the checks became `failed` (the newest-run reduction and the per-check mapping of step 3b, pro-d230: FAILURE, TIMED_OUT, ACTION_REQUIRED, STARTUP_FAILURE, ERROR; not CANCELLED or pending checks; the human-gated rule of step 3b only turns a pending check passed) and the PR's `pr.feedback.checks` is `any` (contract §8.3, hs-d292; with `none` failing checks raise nothing; a PR whose repo has no policy file is polled with the owner and default policy, contract §8.3 "Keys and resolution"), and the PR carries none of its resolved `pr.excused_labels` (pro-d216, pro-d229; contract §7.6 step 6), the review `changes_requested`, the state `closed` (from anything else, once per transition), the PR turning ready to post (reason `ready_to_post`, pro-d225; the rule is contract §5.2), the review `approved` by a human for a posted PR (reason `approved`; a bot's approval never sends it, pro-d158, contract §7.6 step 2) or **`comments`** (pro-d68: the row's new `feedback` list holds an item whose `last` is the `last` of no item of the old list, a NULL old list counting as empty; items that close tell nothing; the Event's `text` states the number of new and of open comments and says `cpd pr view`; `pr.feedback.checks` does not apply to it; contract §7.6 step 6); a conflict is no longer a reason (hs-d340: it queues an `update` Job, below). A pull request that became `merged`: the PR `status = merged`, `merged_at`, **Event** `pr-merged` `{pr, number, url, merged_at}` and every `draft` Job of the PR becomes `cancelled` in the same transaction (hs-d731: `cancel_reason` `pr merged`, one **Event** `job-cancelled` each, as `pr cancel` does for its drafts, §9.6; contract §3.2); when every PR that is not `cancelled` is then `merged`, the same transaction closes every pending review of the Plan `superseded` (reason "Plan review r<k> of Plan P is superseded: the Plan is done.") and appends `plan-done` `{prs: [{key, number, url, merged_at}]}`, once each time the Plan reaches `done`: again when a reopened Plan's last PR merges (§3.1, D2). The plan phase transitions to `done` after the reviews are closed. Pull requests no Plan owns get no Event, nor does one only in the review-request search: `pr-feedback` is unchanged, still only for the author's Plans.
- **The base follow** (hs-d325, hs-d326; contract §3.8 "Stacked PRs" step 8, §7.6 step 6): for a PR whose status is `open` and whose pull request a search lists `OPEN`, when `baseRefName` differs from the PR's `base`, the same transaction sets the PR's `base` to it and appends **Event** `pr-base-changed` `{pr, from, to, from_pr, to_pr, by: "poll", number, url}`. This is what happens when a base PR merges: GitHub deletes its branch and repoints the stacked pull request to `main` (or to the base PR's own base); the poll learns it and **cpd follows GitHub, it never retargets** (no command edits a pull request's base). The PR is then unstacked, or stacked on the next PR when the new base is that PR's Lead Branch (contract §2.2 "The base PR"). **No Gate, no draft, no `pr-feedback`, no status change**: the pull request's tree is unchanged, so a ready PR stays ready (hs-d326); a conflict with the new base queues an `update` Job (below). A base PR closed unmerged is not repointed by GitHub: nothing changes for the PR above it (contract §3.8 step 9). A poll that finds no base changed writes nothing for this.
- **Unstacking the PRs stacked on a merged PR** (hs-d301, contract §7.6 step 6): in the transaction that records a PR's merge, every `waiting` or `dispatch` PR whose `base` is the merged PR's Lead Branch (any Plan) gets the merged PR's own `base` and one `pr-base-changed` (`by` `poll`, `number` and `url` null) for its own Plan. No `pr-feedback`, no Gate, no start.
- **Starting the PRs that waited for it** (A9): `pr poll` does not start a PR itself. Engine E7, which runs after E5 in the same tick (§3.7), starts every `waiting` PR whose needs are now all `merged`  and whose base PR, if any, is `open`, from a fresh `fetch origin <base>` (a stacked PR from its base PR's head, §9.7); when `pr poll` was run by hand, the next tick does, or the Lead runs `pr start <key>` (§9.7).
- **A pull request closed without a merge** (A11, hs-d329): `pr-feedback` with `reasons: ["closed"]`, once; the poll never ends the PR: it stays `open` in cpd, every PR that needs it stays `waiting` and the Lead decides. A Lead may have closed it itself, to reopen it on GitHub once it is ready again (contract §7.6 "Ready, never a draft"): reopened, the same pull request is in the search again and the poll writes its row `state` `open`, with no Event for the reopening; the PR was `open` throughout. To give it up the Lead runs `pr cancel <key>` (§9.6).
- **A conflict** (hs-d340, hs-d497; contract §3.7 E5, §7.6 step 6): in the same transaction, for the PR that is `open` and points to the pull request, when `mergeable` becomes `conflicting` (from anything else, once per transition; a moved base with no conflict, `merge_state` `behind` included, queues nothing), the row's `state` is `open`, the Plan is not archived, the PR's policy resolves and the PR has no `update` Job that is `queued` or `running`: one `update` Job is inserted, with the Event `job-added` `{kind: "update", by: "poll", reason}`, `reason` `conflict`. No `pr-feedback`, no Gate here, nothing becomes a draft (hs-d329). A pull request that is a draft anyway is only stored (`draft`, `merge_state` `draft`): no Event.
- **After a PR is merged or cancelled** (hs-d517, pro-d70, pro-d71): the poll records the status and cleans nothing. When the poll records a merge, the same transaction queues the PR's `done` Jira transition (§21.11, J4c), as `cpd pr merge` does (§9.10). The PR's status is the queue (§3.8). `cpd janitor` (§29) removes merged and cancelled PRs' worktrees and branches, every `watch.janitor_minutes` minutes (default 60) or by hand.

Output: `polled`, the number of distinct pull requests in either search; `changed`, one entry for each pull request whose row was inserted or updated; `base_changed`, the `{plan, pr, from, to}` of each PR whose `base` followed GitHub (empty when none); `feedback`, `merged` and `done`, the `{plan, pr}` pairs that got `pr-feedback` or `pr-merged` and the Plans that became `done` (hs-d565); `update_jobs`, the `{plan, pr, job, reason}` of each `update` Job the poll queued (hs-d340); `rate_limit`, the search's `cost`, `remaining` and `reset_at` (E5 reads it, §3.7); and, only with a Plan, `prs`, one entry for each of its PRs in key order that is not `cancelled`, with its `status` and its `pull_request` row (or null).

```json
{"ok": true, "polled": 3,
 "changed": [{"repo": "acme/pay", "number": 813, "state": "open", "draft": false, "checks": "failed", "review_decision": "none", "mergeable": "conflicting", "merge_state": "dirty"}],
 "base_changed": [{"plan": "pay-4521", "pr": "ui", "from": "cpd/pay-4521/api", "to": "main"}],
 "feedback": [{"plan": "pay-4521", "pr": "ui"}],
 "update_jobs": [{"plan": "pay-4521", "pr": "ui", "job": "up-ui", "reason": "conflict"}],
 "merged": [{"plan": "pay-4521", "pr": "api"}],
 "done": [],
 "rate_limit": {"cost": 1, "remaining": 4987, "reset_at": "2026-10-20T11:00:00Z"},
 "prs": [{"key": "api", "status": "merged", "pull_request": {"repo": "acme/pay", "number": 812, "url": "https://github.com/acme/pay/pull/812", "title": "Retry failed payouts (api)", "draft": false, "state": "merged", "review_decision": "approved", "checks": "passed", "mergeable": "unknown", "merge_state": "unknown", "created_at": "…", "merged_at": "…"}},
         {"key": "ui", "status": "open", "pull_request": {"repo": "acme/pay", "number": 813, "url": "https://github.com/acme/pay/pull/813", "title": "Retry failed payouts (ui)", "draft": false, "state": "open", "review_decision": "none", "checks": "failed", "mergeable": "conflicting", "merge_state": "dirty", "created_at": "…", "merged_at": null}}]}
```

Idempotent: yes; the rows hold what was reported, so a rerun with no change on GitHub reports nothing new (`changed`, `base_changed`, `feedback`, `update_jobs` and `merged` empty or 0) and writes nothing.

### 9.3 `pr rebase`

```
cpd pr rebase [--pr <key>] [--plan <id>] [--continue | --abort]
```

Rebases the PR's Lead Branch onto its base and pushes (§7.6); never required for a base that merely moved ahead (hs-d490). **The repair**, not how fix Jobs reach an `open` PR (a fix Job's own merge does, contract §3.8 "Updating"): for a failed `update` Job, an engine that is down, or a conflict the Lead resolves itself. On a conflict the rebase **stays in progress in the PR's Lead worktree** and the command exits `rebase_conflict`; the Lead resolves the files and runs `--continue` (the rest of the rebase, the Gate and the lease push) or `--abort` (hs-d289; contract §7.6 "The conflict stop"). The Gate runs only when the PR's `gate.merge` is `local` (for `pr open` and `pr push`) or `gate.pr` is `local` (for `pr rebase`) (contract §8.3, hs-d292); with `ci` there is no Gate and the two Gate rows below do not apply.

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR to rebase, selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one |
| `--continue` | finish the rebase stopped at a conflict, then Gate and push (below); no other flag but `--pr`/`--plan`; not with `--abort` (`usage`, exit 2) |
| `--abort` | `git rebase --abort` in the Lead worktree: the Lead Branch back where it was |

| Precondition | Code | Exit |
|---|---|---|
| the PR is `open`, before the Gate and again after it | `illegal_transition` | 3 |
| the PR is not stacked (contract §2.2 "The base PR"): a stacked PR takes its base PR's new commits by merge and publishes with `pr push`, never by rebase (contract §3.8 "Stacked PRs" step 4, hs-d325); checked before the `fetch`, nothing changed; `details` is `{pr, base, base_pr, next}`, `next` = `["git -C <lead worktree> merge --no-edit origin/<base>", "cpd pr push --pr <key>"]` | `stacked_rebase` | 3 |
| `git merge-tree` reports no conflict and the rebase has no unexpected one; else the rebase is left **in progress** in the Lead worktree (nothing pushed, nothing else changed); `details.files` the conflicted paths, `details.worktree` its path, `details.next` the `--continue` command, `details.abort` the `--abort` command | `rebase_conflict` | 3 |
| no rebase is already in progress in the Lead worktree (a stop of an earlier run); `details.next`, `details.abort` as above | `rebase_in_progress` | 3 |
| the Lead worktree has no tracked local changes, which `git rebase` would refuse (checked only when a conflict needs the rebase there); `details.files` lists them | `lead_worktree_dirty` | 3 |
| guard U: an Attempt of the PR whose compare-and-swap is in the snapshot and whose driver lives ends within 60 s; `details.attempt` names it; rerun | `cas_lost` | 3 |
| the Lead Branch's compare-and-swap succeeds within 3 tries (merges kept moving it) | `cas_lost` | 3 |
| no Attempt of the PR of any kind (work, open, update) is running (pro-d103, pro-d105); `details` is `{pr, jobs: [keys], attempts: [ids]}`; the message tells the Lead to message the Worker to do the rebase instead; checked before the fetch, nothing is changed | `attempt_running` | 3 |
| the PR's policy resolves (contract §8.3 "Validation", `details` as there): its Context has an owner | `owner_unset` | 3 |
| its owner has a policy file | `owner_unconfirmed` | 1 |
| its policy files are valid | `policy_invalid` | 1 |
| the `pre-start` passes before the Gate runs; `details.tail` is its last 60 lines, `details.log` the full log `run/<plan>/logs/pr-<key>.pre-start.log` | `gate_error` | 1 |
| the Gate passes on the rebased Lead Branch (§7.3, contract §7.4, hs-d679); `details.tail` is its last 60 lines, `details.log` the full log | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |
| the caller is not an agent, OR the incident is off, OR the resolved PR's Plan is not held (hs-d643): `CPD_SESSION` set and non-empty names an agent; a held Plan is in the incident; `details` is `{pr, plan, incident}` | `incident_hold` | 3 |

The Lead worktree's local changes refuse nothing (hs-d243; `dirty_target` is no longer a refusal of this command).

Effects: no lock (hs-d243); merges into the PR go on during the command. §7.6 steps 1–9: `fetch origin <base>`; the snapshot `S`; `merge-tree`; a temporary rebase worktree detached at `S` for the rebase, giving `R'`; guard U (R for an Attempt of the PR with a dead driver whose compare-and-swap is in `S`; a wait of at most 60 s for one with a live driver); the lease `X0`, the tracking ref `refs/remotes/origin/<lead branch>` read then (what cpd last saw of the remote branch, its own pushes and those of the merges guard U waited for included, so a push cpd has not seen refuses instead of being overwritten; Escalation nl-pr-a1-q1); `update-ref -m "cpd: pr rebase <plan>/<key>" refs/heads/<lead branch> R' S`, back to the snapshot when the ref moved; rule S on the Lead worktree (a stop appends `error` `lead_worktree_stale` and does not fail the command); with `gate.pr` is `local` (contract §8.3, hs-d292) the Gate in the temporary rebase worktree, `pre-start` then `pre-merge` from the PR's policy (hs-d679), outputs to `run/<plan>/logs/pr-<key>.pre-start.log` and `pr-<key>.pre-merge.log`, then the temporary worktree removed; the PR re-read; `push --porcelain --force-with-lease=refs/heads/<lead branch>:<X0> origin R':refs/heads/<lead branch>`, then, when the Lead Branch moved past R' during the Gate, rule P with its tip (a failure of either push is `git_failed`, except a rejection of the first when the remote tip already contains R', which is a success: two `pr rebase` of the PR at once push the same R' and the second can lose the race inside the remote; Escalation nl-pr-a1-q2). On `gate_failed` or `gate_error` nothing is pushed; the rebased Lead Branch stays (the compare-and-swap moved it before the Gate) and the Lead appends fix Jobs to the PR, then runs `pr rebase --pr <key>` again once they are Merged. **Rows**: none; the Ledger does not record the Lead Branch's SHA, so there is no Event. On an unexpected rebase conflict: `git rebase --abort` in the temporary worktree, which is removed, then the conflict stop below.

```json
{"ok": true, "plan": "pay-4521", "pr": "api", "rebased": true, "old": "4b825dc…", "new": "77aa01e…", "pushed": true}
```

**An adopted PR** (`head_ref` set, §9.4) takes the **catch-up**: the command never rebases onto the base. It fetches `origin <head_ref>` and rebases the Lead Branch's commits that `origin/<head_ref>` lacks onto `origin/<head_ref>` (the machinery above: gate worktree, guard U, compare-and-swap of the Lead Branch, rule S, the Gate when `gate.pr` is `local`, `--continue` and `--abort`), then pushes with rule P: `git push --porcelain origin <sha>:refs/heads/<head_ref>`, no lease, no force; history stays linear and is never deleted. It is allowed in `open` and in `dispatch` once every Job of the PR but its `open` and `update` Jobs is `merged` or `cancelled` (`jobs_not_done` otherwise, as `pr open`). When `origin/<head_ref>` is already in the Lead Branch the outcome is `already`. The Lead runs it when an `open` or `update` Job ends `push_failed`, or when `pr open` or `pr push` is refused `push_rejected`; cpd never runs it by itself. A conflict of an adopted PR with its base stays the `update` Job's: its Worker merges the base (a merge commit) and cpd pushes plain.

Idempotent: yes; a base that has not moved gives `"rebased": false` and only the Gate (with `gate.pr` `local`) and the push run. A run that stops at a conflict is repeatable only through `--continue`/`--abort`: a second plain run answers `rebase_in_progress`.

**The conflict stop** (contract §7.6). On a conflict nothing is aborted: the Lead worktree holds the rebase, stopped at the first conflicting commit and the Lead Branch ref is unchanged until the rebase finishes. The error:

```json
{"ok": false, "error": {"code": "rebase_conflict", "message": "…", "details": {"files": ["src/a.rs"], "worktree": "/…/repo.pay-4521.api", "next": "cpd pr rebase --pr api --continue", "abort": "cpd pr rebase --pr api --abort"}}}
```

The Lead edits the files and runs `git -C <worktree> add <files>` (no `git commit`, no `git push`), then `--continue`.

### `pr rebase --continue`

Finishes the stopped rebase, then the Gate and the lease push of `pr rebase`. Order, each refusal changing nothing:

| Precondition | Code | Exit |
|---|---|---|
| `--continue` and `--abort` together | `usage` | 2 |
| the PR is `open`, before the Gate and again after it | `illegal_transition` | 3 |
| the PR's policy resolves (§8.3): `owner_unset` (3), `owner_unconfirmed` (1), `policy_invalid` (1), as above | as listed | |
| a rebase is in progress in the Lead worktree | `no_rebase_in_progress` | 3 |
| no conflict marker remains: no unmerged path, no `<<<<<<<` / `=======` / `>>>>>>>` line at column 0 in a changed file; `details.files` lists the paths, `details.next` the same command | `rebase_conflict` | 3 |
| the Lead Branch is still at the tip the rebase started from (a merge landed during the stop otherwise); `details.reason` `lead_branch_moved`, `details.tip`; the Lead runs `--abort`, then `pr rebase` | `cas_lost` | 3 |
| guard U (as above; `details.attempt`) | `cas_lost` | 3 |
| `GIT_EDITOR=true git rebase --continue` does not stop at the next conflicting commit; if it does, `details.files` are that commit's and the Lead repeats | `rebase_conflict` | 3 |
| `git rebase --continue` fails otherwise; `details.stderr`; the rebase stays in progress | `git_failed` | 1 |
| `gate_error` and `gate_failed` as the rows above; nothing is pushed, the rebased Lead Branch stays | `gate_error` / `gate_failed` | 1 / 3 |
| the push, as above: `--force-with-lease` against `X0`, `X0` read after guard U | `git_failed` | 1 |

Effects: no lock; guard U and the lease as `pr rebase` (the snapshot `S` is the tip the rebase started from, `rebase-merge/orig-head`); `GIT_EDITOR=true git -C <lead worktree> rebase --continue` (no editor; messages kept); `R'` the Lead worktree's `HEAD`; no rule S (that worktree was the one rebased); the Gate in the Lead worktree at `R'` and the push exactly as `pr rebase` (contract §7.4 R2, hs-d679), the Gate only when `gate.pr` is `local`. No `fetch`: the rebase ends on the `origin/<base>` it started from. **Rows**: none; no Event. Output as `pr rebase`, `"rebased": true`, `old` = `S`, `new` = `R'`. Idempotent: no; a second run is `no_rebase_in_progress`.

### `pr rebase --abort`

`git -C <lead worktree> rebase --abort`: the Lead Branch and the Lead worktree as before the stop. Refuses `no_rebase_in_progress` (exit 3) when none is in progress. It asks no PR status, runs no Gate and no `fetch`, pushes nothing, appends no Event; then rule S on the Lead worktree (a stop appends `error` `lead_worktree_stale`, never fails it). Output `{"ok": true, "plan": "pay-4521", "pr": "api", "aborted": true, "tip": "4b825dc…"}`. Idempotent: no; a second run is `no_rebase_in_progress`.

### 9.4 `pr add`

```
cpd pr add <key> --repo <path> --title <text> [--jira <KEY>] [--adopt <number> [--confirm-author <login>]] [--shape <tdd|docs|config>] [--jobs <file.json>] [--base <branch> | --base <key> | --base <plan>/<key>] [--needs <r1,r2> | --no-needs] [--plan <id>]
```

Adds a PR to the Plan, `waiting` (R34, A13, §3.8). In phases `grounding` and `plan` it is part of the plan fingerprint (§2.3), so the Plan needs a review that includes it; in `dispatch`, `deploy` and `done` it is **appended**, like `job add` and engine E7 starts it once its needs are `merged` (at once when it has none, §3.7). In `deploy` and `done` it also **moves the Plan back to `dispatch`** (hs-d87: a merged PR can hold a bug or a regression, so a Plan takes follow-up work at any time; no Plan review of the PR itself, D4; the Jobs added to it as drafts are reviewed, §4.6). A follow-up PR is registered at once, even when it cannot run yet (hs-d239, hs-d241): `pr add <key> --repo … --needs …`, then one stub draft per planned Job (`job add --draft --title …`, §5.1). With `--adopt` it adopts an existing pull request whose head branch does not equal the stored `lead_branch` of any PR of that repo: cpd works it like a Plan (Jobs that address review comments and failing CI checks) and pushes to the pull request's own head branch, never a second pull request (pro-n8).

| Flag | Meaning |
|---|---|
| `<key>` | PR key (§2.1), chosen by the Lead, e.g. `schema`, `web` |
| `--repo <path>` | required: the PR's repo; stored as the realpath of its main worktree. Several PRs of one Plan may share a repo. Its `origin` must be a GitHub repo of the Context's owner (below) |
| `--title <text>` | required (hs-d285): the PR's own title, which names its change and becomes the pull request's title and then, when the repo squash-merges, the squash commit's subject on `main`. 1–72 characters, one line, no control characters, else `usage` (§1.5). `pr add` is the only place a PR is created |
| `--base <key>` | **stack on a PR**, the bare form (hs-d324, hs-d325, A1, A2): the PR `<key>` of this Plan in the PR's repo (this Plan's own included; for `pr edit`, the new repo with `--repo`) becomes the **base PR** and the stored `base` is its Lead Branch, so the PR starts from that branch's head and its pull request targets it (contract §3.8 "Stacked PRs"). A PR key of the Plan in the repo wins over a branch of the same name; any other bare value is a branch name (`base_not_found`), so a bare key of a PR in another repo, of another Plan, or of no PR is a branch (contract §2.2 "A PR reference") |
| `--base <branch>` | the branch the pull request targets, for a value that is no PR of the Plan in the repo; default the repo's `origin` HEAD branch, else `main` (cpd 1 §11 `workflow init`, kept) |
| `--base <plan>/<key>` | **stack on a PR** (hs-d324, hs-d325): the PR `<key>` of Plan `<plan>` (this Plan's own included) becomes the **base PR** and the stored `base` is its Lead Branch, so the PR starts from that branch's head and its pull request targets it (contract §3.8 "Stacked PRs"). The value is a PR when it has the form `<plan>/<key>` and a Plan `<plan>` exists, else a branch (contract §2.2 "A PR reference") |
| `--needs <r1,r2>` | PR references (contract §2.2): keys of other PRs of the Plan (`<plan>/<key>` of the Plan itself is read as the bare key; another Plan's is refused `pr_other_plan`, hs-d522), that must be `merged` before this one starts, comma-separated, no duplicates (`usage`). Default: in `grounding` and `plan` none; in `dispatch` none: one flag is required (`needs_required`, pro-d46); in `deploy` or `done` every `merged` PR of the Plan. Within a repo, stack on the earlier PR with `--base <key>` instead; a need is for another repo's PR or a PR that must start from fresh `main` (contract §3.8 "Stacked PRs") |
| `--no-needs` | no needs, the way a PR with no needs is stated in `dispatch` when the default does not apply; exclusive with `--needs` (`usage`); in `grounding`, `plan`, `deploy` or `done` it changes the default to explicitly none |
| `--title <text>` | required (hs-d285): the PR's own title, which names its change and becomes the pull request's title and then, when the repo squash-merges, the squash commit's subject on `main`. 1–72 characters, one line, no control characters, else `usage` (§1.5). `pr add` is the only place a PR is created. Under `--adopt` it is a local label (it names the stub Jobs): cpd never sends it to GitHub, the pull request keeps its own title |
| `--jira <KEY>` | optional (pro-d127, B1): the PR's own Jira ticket, a key matching `^[A-Z][A-Z0-9]+-[0-9]+$` (else `usage`, exit 2, as `context new --jira`). One ticket per PR: the Plan's `jira` never fills it. Set here or by `pr edit <key> --jira <KEY>` (§9.5); needed before the PR opens when its owner's policy sets `pr.jira_required` (§9.1, §5.5) |
| `--adopt <number>` | adopt the existing pull request `<number>` of `--repo`'s `origin` (below). The PR's base is the pull request's `baseRefName` (it must exist as `origin/<base>`: `base_not_found`) and its head is `head_ref`, stored in `prs.head_ref` |
| `--confirm-author <login>` | the pull request's author, when it is not the User (`adopt_foreign`); accepted and does nothing when the author is the User. Needs `--adopt` |
| `--base <key>` | exclusive with `--adopt`. **Stack on a PR**, the bare form: the PR `<key>` of this Plan in the repo, as in the table above (contract §2.2 "A PR reference") |
| `--base <branch>` | exclusive with `--adopt` (`usage`, exit 2: the base is the pull request's). The branch the pull request targets; default the repo's `origin` HEAD branch, else `main` (cpd 1 §11 `workflow init`, kept) |
| `--base <plan>/<key>` | exclusive with `--adopt`. **Stack on a PR** (hs-d324, hs-d325): the PR `<key>` of Plan `<plan>` (this Plan's own included) becomes the **base PR** and the stored `base` is its Lead Branch, so the PR starts from that branch's head and its pull request targets it (contract §3.8 "Stacked PRs"). The value is a PR when it has the form `<plan>/<key>` and a Plan `<plan>` exists, else a branch (contract §2.2 "A PR reference") |
| `--needs <r1,r2>` | PR references (contract §2.2): keys of other PRs of the Plan (`<plan>/<key>` of the Plan itself is read as the bare key; another Plan's is refused `pr_other_plan`, hs-d522), that must be `merged` before this one starts, comma-separated, no duplicates (`usage`). Default: in phase `deploy` or `done` (the reopen, hs-d118), **every `merged` PR of the Plan**; in other phases none. Within a repo, stack instead (`--base <key>`); a need is for another repo's PR or a PR that must start from fresh `main` (contract §3.8 "Stacked PRs") |
| `--no-needs` | no needs, also in phase `deploy` or `done`; exclusive with `--needs` (`usage`) |
| `--shape <tdd\|docs\|config>` | the template of stub drafts the PR starts with (hs-d341, contract §2.2 "Job kinds"), default `tdd`: `<key>-docs` (kind `docs`), `<key>-tests` (`test`, needs `<key>-docs`), `<key>-impl` (`impl`, needs `<key>-tests`). **`docs`**: only `<key>-docs`; **`config`**: only `<key>-config`, kind `work`: a docs-only or config-only PR says so here and has no test Job. **Stored in `prs.shape` and enforced** (hs-d497; contract §2.2 "Job kinds"): the kinds the PR may have (`shape_violation` at `job add`, `job edit --kind`) and the ones a `tdd` PR must keep (`shape_incomplete` at `plan review`, `plan lock` and `job cancel` typed by the Lead); never changed afterwards. `pr cancel` of the whole PR is the way out and cancels the `test` and `impl` Jobs itself (§9.6, hs-d573) `pr_exists`'s comparison includes it |
| `--jobs <file.json>` | **after the lock** (phase `dispatch`, `deploy` or `done`; hs-d731): the template's Jobs as decided work. One JSON object keyed by the template suffix (shape `tdd`: `docs`, `tests`, `impl`; `docs`: `docs`; `config`: `config`), each value `{"tier": "<T>", "spec": <the Spec object `job add --spec` takes; a test Job's has `tests` and `test_files`>}`; every key of the shape is required and no other key is allowed. Before the lock it is refused `usage` (in phase `plan` every Job is a draft). Effects in "The template" below |

| Precondition | Code | Exit |
|---|---|---|
| the Plan is not archived | `archived` | 3 |
| `--repo` is inside a git repository (`git -C <path> rev-parse --git-common-dir`) | `not_a_git_repo` | 3 |
| the Plan's Context has an owner (contract §8.3); the message names `cpd context owner <login> --context <c>` | `owner_unset` | 3 |
| the repo is keyed from its `origin` (below); `details.url` (null when there is no `origin`) | `repo_unkeyed` | 1 |
| the repo's owner is the Context's; `details.owner` (the Context's), `details.repo_owner` | `owner_mismatch` | 3 |
| the owner has a policy file, `$CPD_HOME/policy/<owner>/owner.toml`; `details.owner` | `owner_unconfirmed` | 1 |
| the owner's and the repo's policy files are valid; `details.file`, `details.key` | `policy_invalid` | 1 |
| `--base` is a branch (a bare `--base <key>` that names no PR of the Plan in the repo is one too): it exists, `refs/remotes/origin/<base>`, or `refs/heads/<base>` when the repo has no `origin` | `base_not_found` | 4 |
| `--base <plan>/<key>`: the PR exists in that Plan (the long form only: a bare `--base <key>` is never this code, contract §2.2 "A PR reference"); `details` is `{plan, pr}` | `base_pr_unknown` | 3 |
| the base PR (`--base <plan>/<key>`) and every need are PRs of the Plan, not of another Plan; `details` is `{pr, reference, plan}` (the PR being added, the reference as given, the other Plan) | `pr_other_plan` | 3 |
| the base PR is not `cancelled` and not `merged` (name its base branch instead); a need is not `cancelled`; `details.pr` | `base_pr_unavailable` (base PR), `needs_unavailable` (need) | 3 |
| the base PR of the `<plan>/<key>` form is in the same repo as this PR (the same realpath; a bare `--base <key>` of another repo is a branch, §2.2 "A PR reference"); `details` is `{pr, repo, base_repo}` | `base_pr_other_repo` | 3 |
| a PR of an archived Plan is referenced only as a `merged` need; `details` is `{plan, next}`, `next` the `cpd plan unarchive` command | `archived` | 3 |
| the base PR is not also in `--needs` (a need is merged before this PR starts, a base PR is the implied need met when it is `open`) | `usage` | 2 |
| no PR of the Plan has this key, `cancelled` ones included, unless its repo, base, title, shape, needs and `jira` (compared only when `--jira` is given) equal the resolved flags, the default needs included (then `already`) | `pr_exists` | 3 |
| in phase `dispatch`, one of `--needs` or `--no-needs` is required; an existing PR key is compared first (`already` when the flags match, else `pr_exists`), so a rerun is never refused (idempotence, pro-d46) | `needs_required` | 3 |
| (`--adopt`) the pull request's state is `OPEN`; `details` is `{number, url, state}` | `adopt_not_open` | 3 |
| (`--adopt`) it is not from a fork (`isCrossRepository` false); `details` is `{number, url}` | `adopt_fork` | 3 |
| (`--adopt`) its head branch does not equal the stored `lead_branch` of any PR of that repo; `details` is `{number, head_ref}` | `adopt_cpd_branch` | 3 |
| (`--adopt`) no PR of any Plan with the same repo and head_ref is `waiting`, `dispatch` or `open`; `details` is `{head_ref, plan, pr}` | `adopt_taken` | 3 |
| (`--adopt`) its author is the User, or `--confirm-author` equals the author's login (case-insensitively); `details` is `{number, url, author, user, confirm: "--confirm-author <author>"}`; the message tells the Lead to ask the User first (pro-n4) | `adopt_foreign` | 3 |
| no PR of the Plan has this key, `cancelled` ones included, unless its repo, base, title, shape, head_ref, needs and `jira` (compared only when `--jira` is given) equal the resolved flags, the default needs included (then `already`) | `pr_exists` | 3 |
| every need is a PR of the Plan; `details.needs` lists the unknown keys | `needs_unknown` | 3 |
| no need is `cancelled`; `details.needs` lists them | `needs_unavailable` | 3 |
| the graph of PR needs and base PRs stays acyclic across every Plan (a PR stacked on `B` cannot be needed, directly or not, by `B`: it would wait for `B` to start while `B` waits for it to merge); `details.cycle` lists the references | `dependency_cycle` | 3 |
| the Lead Branch `<plan>/<key>` and its parent path `<plan>` do not exist in the repo (except a leftover of an interrupted start); `details.branch` names it | `branch_exists` | 3 |
| (`--shape tdd`) the PR's repo policy has `[test-commands]` (contract §7.7; none when there is no repo file or the file lacks the table); the message names `--shape docs`, `--shape config` and the table | `tests_unconfigured` | 3 |
| none of the Job keys the shape lays out exists in the Plan; `details.jobs` | `job_key_exists` | 3 |
| `--jobs` only after the lock (phase `dispatch`, `deploy` or `done`); in `grounding` or `plan` the template is unchanged | `usage` | 2 |
| the `--jobs` file is one JSON object with exactly the keys of the shape; each Job's Tier and Spec are checked as `job add` checks them (§5.1), with the code `job add` gives the same fault (`spec_invalid`, `tier_invalid`, `tier_not_allowed`, `touches_missing`, `resource_undeclared`, …); nothing is written when one is refused | the code of `job add` | 2 or 3 |
| after the lock, without `--jobs`: the PR waits for another PR (a need not `merged`, or a base PR not `open`: the condition under which engine E7 does not start it, §3.7), else the stubs would be parked drafts (contract §2.2 "A parked draft", hs-d731); `details` `{plan, pr, jobs}`, `jobs` the stub keys; nothing is written | `draft_parked` | 3 |

The five `adopt_*` rows are checked in the order above, after every other check of this command, with nothing written. `--confirm-author` without `--adopt` is `usage`.

**What `--adopt` reads** (A2), with the working directory the repo, so `gh` resolves the pull request through its `origin`: `gh pr view <number> --json number,url,state,isCrossRepository,headRefName,baseRefName,author` and `gh api user --jq .login`. The first answers `{"number":7,"url":"https://github.com/acme/pay/pull/7","state":"OPEN","isCrossRepository":false,"headRefName":"feat/x","baseRefName":"main","author":{"login":"alice"}}`. The author is the User when `author.login` equals the login of `gh api user`, compared case-insensitively (pro-n4: the User's own pull request is adopted without asking; another author's needs a warning and the User's confirmation first). A `gh` failure is `gh_failed`. The pull request's number is not stored: `pr open` finds it by head (`gh pr list --head <head_ref> --state all`, §9.1).

Any phase is allowed: `grounding`, `plan`, `dispatch`, `deploy` and `done`, so no `illegal_phase`. A new PR has no dependents, so it creates no cycle of its own; `dependency_cycle` is possible only through `pr edit` (§9.5).

**The template** (hs-d341). The shape's Jobs are **stub drafts** (§5.1: a title, no Tier and no Spec; the exception is the `open-<pr>` stub, below, which has Tier `open_tier` and cpd's fixed Spec from the insert): `jobs` rows with their `kind`, the titles `Docs: <PR title>`, `Red tests: <PR title>`, `Implement: <PR title>` and `Config: <PR title>`, `status = draft`, `appended` 0 before the lock and 1 after it (as `job add --draft` writes them), no fingerprint and for `tdd` the `dependencies` rows `<key>-tests → <key>-docs` and `<key>-impl → <key>-tests`. The Lead completes each (`job edit --tier --spec`, §5.2) or cancels the ones it does not need (never, one by one, the `test` and `impl` Jobs of a `tdd` PR: `job cancel` is refused `shape_incomplete`; `pr cancel` of the whole PR cancels them itself, §9.6, hs-d573) and adds the PR's other Jobs with `job add --pr`. The engine runs nothing of them until the batch is approved (§4.6, §4.8). **After the lock** (hs-d731) the stubs are laid out as above only for a PR that waits for another PR; **with `--jobs`** the non-open Jobs are written `queued` instead, as `job add` without `--draft` writes them (`appended` 1, Tier, Spec, fingerprint, `tests` and `test_files` of the `test` Job, the same titles and kinds and for `tdd` the same chain of `dependencies` rows), one `job-added` Event each and the PR's `open` Job stays the draft it is; without `--jobs` a PR that would start at once is refused `draft_parked` (the rows above). The PR's `open` Job is a **stub draft** laid out by the template, Tier `open_tier` (contract §8.2), no Dependencies, and a stored Spec: cpd's fixed text, written by `pr add` when it inserts the stub and never edited (`job edit` refuses it, `job_locked`); the Lead may cancel it. Engine E4 (§3.7) is a **fallback**: when the PR becomes `dispatch`, if it has no `open` Job at all, E4 adds a fresh one `queued`; a `draft`, `queued`, `running`, `failed` or `cancelled` one it leaves alone and it still promotes a `draft` opener to `queued` when no other draft of its PR remains (contract §3.7, hs-d573).

**The repo's key** (contract §8.3, hs-d288). `pr add` parses the repo's `origin` once, at the boundary: `git -C <repo> remote get-url origin`, which must be `https://github.com/<owner>/<name>` or `git@github.com:<owner>/<name>`, each with or without `.git`, `<owner>` and `<name>` valid by contract §2.1 (stored lowercased). Nothing else is accepted: no `origin`, another host, or an SSH host alias (`git@github-work:<work-owner>/x.git`) is `repo_unkeyed`; cpd never asks the network to resolve one. `<owner>` must be the Context's (`owner_mismatch`): one GitHub owner per Context and a Plan may span several repos of it. The PR stores `<name>` only; its owner is the Context's.

**Legacy Lead Branches** (superseded by pro-d112, migration `lead-branch-name`). A PR created before the migration keeps its stored `lead_branch` name unchanged; nothing is renamed on disk or on the remote (A3).

Effects: **rows**, one transaction: in `deploy` or `done`, `plans` (`phase = dispatch`, `done_at = null`); `prs` (`status = waiting`, `repo`, `name`, `base` (the base PR's `lead_branch` when stacked), `title`, `jira` (null without `--jira`), `shape`, `lead_branch = <plan>/<key>` (with `--adopt`, the local branch of the Lead worktree; and `head_ref`, the pull request's head branch, set here only: the trigger `prs_head_ref_fixed` refuses any later change with `pr_locked` and the partial unique index `prs_head_ref_live` allows one live PR per repo and head), `created_txn`, `created_at`; `lead_worktree` null); one `pr_needs` row per need; and the shape's stub drafts (below). **Events**, in this order: in `deploy` or `done`, `plan-phase` `{from: "deploy"|"done", to: "dispatch"}`; then `pr-added` `{pr, repo, base, base_pr, title, needs, appended, head_ref, author}` (`head_ref` and `author` null for a cpd PR), `appended` true in phases `dispatch`, `deploy` and `done`; then one `job-added` (`kind`, `status` `draft`) per stub draft. Engine E7 then starts the PR like any appended PR (at once when it has no needs). No tmux and no other git write: the Lead Branch and Lead worktree are created when the PR starts (§3.8, §7.2). And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "pr": {"key": "web", "repo": "/home/clayton/projects/web", "name": "web", "base": "main", "base_pr": null, "title": "Add the web client", "jira": null, "needs": ["api"], "status": "waiting", "lead_branch": "pay-4521/web", "head_ref": null, "appended": false, "jobs": ["web-docs", "web-tests", "web-impl"]}}
```

An adopted pull request (`--adopt 3748`): the same object with `"head_ref": "feat/EEC-3238/review-submit-polish"`, `"base": "main"`, `"lead_branch": "pay-4521/card"`; `plan show` (§4) carries `head_ref` in its PR objects too.

A PR stacked on a PR of its Plan, needing another (`--base pay-1/api --needs schema`): `"base": "pay-1/api", "base_pr": "pay-1/api", "needs": ["schema"]`.

Idempotent: yes, as stated; the same flags again, `--adopt` included, are `already` (the comparison includes head_ref and `--jira` when it is given). The comparison uses the resolved repo and base (a base PR by its Lead Branch) and needs (as references), so a rerun without `--base` matches a PR whose base was defaulted to the same branch.

### 9.5 `pr edit`

```
cpd pr edit <key> [--repo <path>] [--base <branch> | --base <key> | --base <plan>/<key>] [--needs <r1,r2> | --no-needs] [--title <text>] [--jira <KEY>] [--plan <id>]
```

Changes a `waiting` PR before the lock (A13) and, with `--title`, a PR's title in any phase (below). `--needs` replaces the whole list; `--no-needs` removes every need. `--base` and `--needs` take what `pr add` takes (§9.4): a branch or, to stack, a base PR by its bare key `<key>` (a PR of the Plan in the repo) or in the form `<plan>/<key>` and PRs of the Plan (contract §2.2 "A PR reference"); `--base <branch>` on a stacked PR unstacks it. A PR that another PR is stacked on keeps its repo: `--repo` is refused `pr_in_use` (`details.prs`), because the stacking is derived from the repo and the base and a change would silently unstack the PR above it (hs-d324). The PR's Jobs stay with it. A changed `--repo` keeps the stored base unless `--base` is given and that base must exist in the new repo; the new repo's `origin` is parsed as `pr add` parses it (§9.4 "The repo's key") and replaces `name`. After the lock, the link (`--needs` and `--base`) changes only by the restack (below) and is otherwise stated at `pr add` with a required flag (pro-d46, §2.2).

Changes a `waiting` PR before the lock (A13) and, with `--title`, a PR's title in any phase (below). `--needs` replaces the whole list; `--no-needs` removes every need. `--base` and `--needs` take what `pr add` takes (§9.4): a branch or, to stack, a base PR by its bare key `<key>` (a PR of the Plan in the repo) or in the form `<plan>/<key>` and PRs of the Plan (contract §2.2 "A PR reference"); `--base <branch>` on a stacked PR unstacks it. An **adopted** PR (`head_ref` set, §9.4) takes its repo and its base from the pull request: `--repo` or `--base` is refused `pr_adopted` (`details` is `{pr, flag}`), checked first. A PR that another PR is stacked on keeps its repo: `--repo` is refused `pr_in_use` (`details.prs`), because the stacking is derived from the repo and the base and a change would silently unstack the PR above it (hs-d324). The PR's Jobs stay with it. A changed `--repo` keeps the stored base unless `--base` is given and that base must exist in the new repo; the new repo's `origin` is parsed as `pr add` parses it (§9.4 "The repo's key") and replaces `name`. `pr edit` writes no Job row and changes no open or update Job's stored Spec: the Worker is told the live base, read from the PR row at A3 (contract §3.4).

Changes a `waiting` PR before the lock (A13) and, with `--title`, a PR's title in any phase (below). `--needs` replaces the whole list; `--no-needs` removes every need. The PR's Jobs stay with it. A changed `--repo` keeps the stored base unless `--base` is given and that base must exist in the new repo; the new repo's `origin` is parsed as `pr add` parses it (§9.4 "The repo's key") and replaces `name`.

**After the lock, the restack** (contract §3.8 "Stacked PRs" step 10; User, 2026-10-28 EDT): `cpd pr edit <key> --base <A> --needs <rest>` or `cpd pr edit <key> --base <A> --no-needs` restacks a `waiting` PR onto `<A>`, the one link edit the lock allows. `<A>` is read as `--base` is (contract §2.2 "A PR reference": a bare `<key>` or `<plan>/<key>`). It is allowed when the PR is `waiting` and not adopted; `--repo` is not given; `<A>` is a PR of the same Plan and repo that is not `cancelled` and not `merged` and the graph stays acyclic (the refusals of §2.2 "Validation of a reference", as before the lock); `<A>` is one of the PR's current needs or already its base PR; and the PR's needs after the edit are exactly its current needs without `<A>` (so `--no-needs` only when no need other than `<A>` is left). The result: `base` is `<A>`'s Lead Branch, the need on `<A>` is gone and the Event is `pr-edited` with `fields` `["base","needs"]` (the fields that changed). Run again it is `already` (exit 0, nothing written). `--title` and `--jira` combine with a restack. Every other `--repo`, `--base`, `--needs` or `--no-needs` after the lock is refused `pr_locked` and so is any such flag on a PR that is not `waiting`; `--base <A>` alone is `pr_locked` too. The table below gives the codes.

`--title <text>` (hs-d285) works in **every phase and every PR status but `merged` and `cancelled`** (a PR that is over; `cancelled` is the PR status that stands for a closed PR, §3.8): 1–72 characters, one line, no control characters, else `usage` (§1.5). The title is **not a Locked field**: it is outside the plan fingerprint (§2.3) and `prs_locked`'s columns (§4.4), so editing it never invalidates a review or a lock and the phase and status rules below (`pr_locked`, `illegal_transition`) apply to the other flags only. A `merged` or `cancelled` PR's title is history: `pr_ended`. When the PR has an **open** pull request, the command also retitles it on GitHub, through the seam of §9.1, with `gh api -X PATCH repos/<owner>/<repo>/pulls/<n> -f title=<text>`. It must **not** use `gh pr edit`: on 2026-10-19 that fails on this repo with "Projects (classic) is being deprecated … (repository.pullRequest.projectCards)", while the `gh api` PATCH works (both verified on #99). `pr describe` (§9.5a) does not use it either. A failing GitHub update is `gh_failed` and leaves the stored title unchanged.

`--jira <KEY>` (pro-d127, B1) works in the same phases and PR statuses as `--title`: **every phase and every PR status but `merged` and `cancelled`** (`pr_ended`), an adopted PR included. The key matches `^[A-Z][A-Z0-9]+-[0-9]+$` (the boundary type `Jira`, as `context new --jira`), else `usage` (§1.5). The ticket is **not a Locked field**: it is outside the plan fingerprint (§2.3) and `prs_locked`'s columns (§4.4), so setting it never invalidates a review or a lock. The same key again is `already`, with no transaction and no Event; a different key replaces the stored one; no flag clears it. The Plan's own `jira` never fills a PR's ticket: one ticket per PR and `cpd pr edit <key> --jira <KEY>` gives each PR its own. It changes no GitHub state. A PR whose owner's policy sets `pr.jira_required` (`cpd policy show --pr <key>`) cannot open without a ticket (`jira_required`, §9.1, §5.5).

| Precondition | Code | Exit |
|---|---|---|
| at least one field flag; the message names the flags, `--jira` among them | `nothing_to_edit` | 2 |
| the PR is not adopted when `--repo` or `--base` is given; `details` is `{pr, flag}` | `pr_adopted` | 3 |
| with `--title` or `--jira`, the PR is not `merged` or `cancelled` | `pr_ended` | 3 |
| for the flags other than `--title` and `--jira`: the phase is `grounding` or `plan`; after the lock a PR is Locked (§4.4 `prs_locked`) except for the restack (below); a restack needs a `waiting` PR that is not adopted and no `--repo` | `pr_locked` | 3 |
| after the lock, `--base <A>` without `--needs` or `--no-needs`, `--needs` or `--no-needs` without `--base`, or a restack whose `<A>` is not one of the PR's current needs or its base PR, or whose needs after the edit are not exactly its current needs without `<A>` | `pr_locked` | 3 |
| for the flags other than `--title` and `--jira`: the PR is `waiting` (a `cancelled` one is terminal) | `illegal_transition` | 3 |
| `--repo` is inside a git repository | `not_a_git_repo` | 3 |
| with `--repo`: the new repo is keyed from its `origin`, as for `pr add` (§9.4) | `repo_unkeyed` | 1 |
| with `--repo`: the new repo's owner is the Context's, as for `pr add` | `owner_mismatch` | 3 |
| the base exists in the PR's repo (the new one with `--repo`), as for `pr add` | `base_not_found` | 4 |
| a base PR, as for `pr add`: `base_pr_unknown`, `pr_other_plan`, `base_pr_unavailable`, `base_pr_other_repo` (against the new repo with `--repo`), `archived` (§9.4) | as there | 3 |
| every need is a PR (of the Plan, or of another by its reference), other than this one | `needs_unknown` | 3 |
| no need is `cancelled` | `needs_unavailable` | 3 |
| the graph of PR needs and base PRs stays acyclic across every Plan; `details.cycle` lists the references | `dependency_cycle` | 3 |
| with `--repo`: no PR of the Plan is stacked on this PR (it has no dependents by base); `details.prs` lists them | `pr_in_use` | 3 |
| with `--repo`: the PR's Lead Branch and its parent path do not exist in the new repo | `branch_exists` | 3 |
| with `--title` and an open pull request: the `gh api -X PATCH` succeeds; `details` is `{argv, exit, stderr}` | `gh_failed` | 1 |

Effects: **rows**, one transaction: the changed `prs` columns (`name` with `repo`; `base`, a base PR's Lead Branch when stacked; `title`; `jira`); `pr_needs` rows deleted and inserted to match (§4.4 allows deletion in `grounding` and `plan` and after the lock only of the need that is the base PR of the restack, §3.8 step 10). **Event** `pr-edited` `{pr, fields: [name]}`, listing only the fields whose value changed (`repo`, `base`, `needs`, `title`, `jira`). The plan fingerprint changes with `repo`, `base` and `needs`, never with `title` or `jira` (§2.3). With `--title` and an open pull request, the GitHub update runs before the transaction and a failure writes nothing. And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "pr": {"key": "web", "status": "waiting"}, "fields": ["base", "needs"]}
```

Idempotent: yes; when no value changes it writes nothing and returns `"already": true`.

### 9.5a `pr describe`

```
cpd pr describe --pr <key> --file <path> [--plan <id>]
```

Stores the PR's description, verbatim, which becomes the pull request body (the description is required before `pr open`). The `--file` argument is a filesystem path or `-` for stdin; the file must be readable UTF-8, else `file_unreadable` (exit 2). The description is stored non-empty after trimming and at most 65536 characters; else `description_invalid` (exit 3). The command is allowed in **every phase and every PR status but `merged` and `cancelled`** (a PR that is over; like `pr edit --title`, §9.5). When the PR has an **open** pull request, the command also updates it on GitHub first, with `gh api -X PATCH repos/<owner>/<repo>/pulls/<n> --input -`, the JSON `{"body": <the description>}` on stdin. `<owner>/<repo>` is `pull_requests.repo` of the PR's open pull request, never the cwd: the command runs with cwd `$CPD_HOME`, which is not a git repository (§9.2, the rule on a gh call with cwd `$CPD_HOME`). A repo that is not `owner/name` (each part only `[A-Za-z0-9_.-]`) is `gh_failed` and nothing is sent. It must not use `gh pr edit`, for the reason §9.5 gives for `--title`. A failing GitHub update is `gh_failed` and leaves the stored description unchanged. An **adopted** PR (`head_ref` set, §9.4) is refused `pr_adopted` (exit 3; its pull request's body is the Lead's to fill, so `describe` on it is disallowed), checked first. The PR's open Job does not start until the description is stored.

| Precondition | Code | Exit |
|---|---|---|
| the PR is not `merged` or `cancelled` | `pr_ended` | 3 |
| the PR is not adopted when it has an open pull request; `details.pr` | `pr_adopted` | 3 |
| `--file` is readable UTF-8 | `file_unreadable` | 2 |
| the description, trimmed, is non-empty and at most 65536 characters | `description_invalid` | 3 |
| with an open pull request: the `gh api -X PATCH repos/<owner>/<repo>/pulls/<n> --input -` call succeeds; `details` is `{argv, exit, stderr}` | `gh_failed` | 1 |
| with an open pull request: `pull_requests.repo` is not `owner/name`; nothing is sent; `details` is `{repo}` | `gh_failed` | 1 |

Effects: **rows**, one transaction: the PR's stored `description` column (inserted or updated, the description verbatim). **Event** `pr-described` `{pr}` only when the description changed (idempotent). With an open pull request, the GitHub update runs before the transaction and a failure writes nothing. And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "pr": {"key": "api", "status": "dispatch"}}
```

Idempotent: yes; when the description is unchanged it returns `"already": true` and writes nothing.

### 9.6 `pr cancel`

```
cpd pr cancel <key> [--superseded-by <plan>/<key>] [--plan <id>]
```

`--superseded-by <ref>`: the PR is `dispatch` and its work was carried by another PR, `<ref>` (`<plan>/<key>`, or a bare `<key>` of the Plan); its `merged` Jobs no longer hold it (hs-d301, contract §3.8 "Ending a `dispatch` PR whose work was superseded").

Three uses (contract §3.8) and a fourth with `--superseded-by`. Before the lock it removes a `waiting` PR from the Plan: `waiting → cancelled` (A13). After the lock it ends a started PR with nothing left to do, `dispatch → cancelled`, so a PR whose Jobs were all cancelled, or that never had one, cannot keep the Plan out of `done` (hs-n271); and an **`open`** PR whose pull request was closed on GitHub without a merge, `open → cancelled` (hs-d329). The PR keeps its key either way, so `pr add` cannot reuse it. Locked Jobs are never edited or deleted: only the PR's status moves. The PR's `draft`, `queued` and `failed` Jobs, of every kind, go with it (below, hs-d573), so they need not be cancelled first; for a `waiting` PR the needs on it still have to go (`pr edit`). `pr cancel` is never refused by the shape check: the last `test` and `impl` Jobs of a `tdd` PR are cancelled with it, which is how a `tdd` PR left in draft is removed (hs-n421, hs-d573); and after it no Job of the PR is `queued` or `running`, so `plan archive` is not blocked by it. The status decides the path; a `cancelled` PR is `already` in any phase; a `merged` PR is refused. A **closed-unmerged** PR (contract §3.8) shows in the Inbox (`pr-closed`), in `plan show` (`closed_unmerged`), in the Lead's turn-start line and in one wake, until it is cancelled or its pull request is reopened or merged; cpd never cancels it itself, and this command is how it is given up.

| Precondition | Code | Exit |
|---|---|---|
| the PR is `open`, `dispatch`, `waiting` or `cancelled` (`cancelled`: `already`, in any phase; `merged`: `illegal_transition`); a `dispatch` PR only after the lock; an `open` PR is in phase `dispatch` or `deploy` | `illegal_transition` | 3 |
| `--superseded-by` on a PR that is not `dispatch` (`cancelled`: `already`), or naming a superseding PR that is `cancelled` | `illegal_transition` | 3 |
| `--superseded-by` names a PR that is not in any Plan (`details.plan`, `details.pr`) | `pr_not_found` | 4 |
| `--superseded-by` names a PR that is not of a Plan of this Context | `pr_not_found` | 4 |
| `--superseded-by` names the PR itself | `usage` | 2 |
| **`waiting`, `dispatch`**: no Job of the PR is `running` and none is `merged` unless `--superseded-by` is given (with it `merged` Jobs are work another PR carried and do not hold it); a `draft`, `queued` or `failed` Job, of any kind, never holds it: this command cancels it (below, hs-d573). And no PR that is not `cancelled`, of any Plan, needs it or is stacked on it (contract §3.8 "Stacked PRs"); `details.jobs` lists the holding Jobs and `details.prs` the PRs, as references (`<plan>/<key>` for another Plan's) | `pr_in_use` | 3 |
| **`open`**: the PR's `pull_requests` row is `state = closed`, as the last poll stored it (no `gh` call: run `pr poll` first); `details` is `{pr, number, url, state}` | `pr_not_closed` | 3 |
| **`open`**: no PR of the Plan that is not `cancelled` needs it or is stacked on it (its Jobs are not checked: a `draft`, `queued` or `failed` Job never holds it and it is cancelled by this command, below; hs-d573); `details.prs` lists the PRs as references | `pr_in_use` | 3 |
| **`open`**: the PR is still `open` at the transaction | `cas_lost` | 3 |

Every check runs before the first write: a refused `pr cancel` writes nothing (hs-d573).

Effects, **`waiting`** and **`dispatch`**: **rows**, one transaction: the PR `status = cancelled` and every Job of the PR whose status is `draft`, `queued` or `failed`, of every kind (`docs`, `test`, `impl`, `work`, `open`, `update`), cancelled in that transaction with `status = cancelled`, the constant reason `pr cancelled` (also with `--superseded-by`), `failed_cause` cleared and `cancel_requested_at` set (one `job-cancelled` each; a Job already `cancelled` is untouched and gets no Event; one that is `running` is a live Job and refuses `pr_in_use`; contract §3.8, hs-d573); a `dispatch` PR also loses its `lead_worktree` (the Lead worktree) and `started_at` (the table allows neither on a `cancelled` PR). **Worktrees and branches are removed by the next `cpd janitor` pass** (every `watch.janitor_minutes` minutes, default 60, or by hand, §29; pro-d70, pro-d71: idle check, dirty or unpushed kept untouched with one `cleanup_failed` Event from the first janitor pass that sees it, later passes retry silently). **Event** `pr-cancelled` `{pr}` and `superseded_by` (the superseding PR as a reference: the bare key in the Plan itself, else `<plan>/<key>`) with `--superseded-by`. Before the lock the plan fingerprint changes (§2.3: it covers the PRs that are not `cancelled`). After the lock, when no PR of the Plan is left `waiting` or `dispatch` and one is not `cancelled`, the same transaction moves the Plan as `pr open` and the poll do: `dispatch → deploy` with `plan-phase` and `deploy → done` with `plan-done` when every PR left is `merged` (§3.1); the output then has `phase`. A Plan whose PRs are all `cancelled` stays in `dispatch`. And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

Effects, **`open`** (contract §3.8): **rows**, one transaction: the PR `status = cancelled` (and its `draft`, `queued` and `failed` Jobs, cancelled as above, in the same transaction; hs-d573), with `lead_worktree` (the Lead worktree), `started_at`, `pr_number`, `pr_url`, `pr_opened_at` and `pull_request` set to null (the table's checks tie them to the status; the old values stay in the history rows). **Events** `pr-cancelled` `{pr}`, its `text` naming the pull request, the Lead worktree and the Lead Branch, which the next `cpd janitor` pass removes unless the worktree is dirty or unpushed (§29); then, when every PR that is not `cancelled` is now `merged` and at least one is, the Plan `deploy → done` with `done_at` and `plan-done` (§3.1); otherwise the phase stays. The plan fingerprint does not change after the lock (§2.3). The output also carries `phase` and `released`, `{number, url, lead_worktree, lead_branch}` as they were. And, when the Plan's working Lead runs it, it names the PR as the Lead's Workflow (§1.3).

```json
{"ok": true, "pr": {"key": "web", "status": "cancelled"}, "phase": "done"}
```

`phase` is present only when the Plan moved.

An `open` PR given up, `phase` after the command:

```json
{"ok": true, "pr": {"key": "api", "status": "cancelled"}, "phase": "deploy", "released": {"number": 116, "url": "https://github.com/acme/pay/pull/116", "lead_worktree": "/home/me/projects/pay.pay-4521.api", "lead_branch": "pay-4521/api"}}
```

Idempotent: yes, as stated.

### 9.7 `pr start`

```
cpd pr start <key> [--plan <id>]
```

Starts a `waiting` PR whose needs are all `merged` and whose base PR, if it has one, is `open` (A9, §3.8, hs-d324): the Lead's retry after engine E7 failed to start it (the `error` Event with code `pr_start_failed`), once the cause is fixed. It does what E7 does.

**An adopted PR** (`head_ref` set, §9.4; the same for E7 and `plan lock`'s start): the Lead worktree path is as always; `git fetch origin <head_ref>` runs (a failure is `pr_start_failed` with `step` `remote`) and the local branch `<plan>/<key>` is cut from `origin/<head_ref>`. There is no conflict check against the base (a conflict with the base is the `update` Job's), nothing is pushed at the start and the User's own local branch of the head's name is never read or touched.

| Precondition | Code | Exit |
|---|---|---|
| the phase is `dispatch` | `illegal_phase` | 3 |
| the PR is `waiting` (`dispatch`, `open` or `merged`: `already`; `cancelled`) | `illegal_transition` | 3 |
| every need is `merged` (`details.needs` lists those that are not, as keys) and the base PR, if the PR is stacked, is `open` (`details.base_pr` is its reference when it has not opened); a `merged` base PR is not a refusal: the PR is unstacked first (below) | `pr_not_ready` | 3 |
| its Lead worktree path does not exist, except a leftover of an interrupted start (a worktree of the PR's repo on its Lead Branch), which is removed first | `worktree_exists` | 3 |
| its Lead Branch and its parent path do not exist in its repo, except a leftover of an interrupted start (§2.1) | `branch_exists` | 3 |
| `origin/<lead branch>` does not exist, or it does not conflict with `origin/<base>` (§7.2, hs-d490) | `branch_exists` | 3 |
| the worktree, the remote check and `pre-start` succeeded; `details` is `{pr, step, tail?, log?}`, as in `pr_start_failed` | `setup_failed` | 1 |

Effects (§3.8 "The start", §7.2):
- **Unstack** (contract §3.8 "Stacked PRs" step 7, hs-d301): when the PR's base PR is `merged`, a first transaction sets the PR's `base` to the base PR's own `base` (a chain of merged base PRs is walked to its end, one `pr-base-changed` per step) and appends `pr-base-changed` with `by` `pr start` (E7 does the same with `by` `engine`; the poll with `by` `poll` when it recorded the merge), before the start's checks, then the start goes on with the new base (the start point is `origin/<new base>`, never the merged PR's deleted branch). This is also why `pr_not_ready` is never about a `merged` base PR.
- **git**, in the PR's repo: `fetch origin <base>` (fresh, so the PR contains what its needs merged; for a **stacked** PR, `<base>` is its base PR's Lead Branch, fetched only when it is on the remote and the Lead Branch is created from the base PR's Lead Branch **head**, the local branch, not from `origin/<default branch>`, contract §7.2), the remote check and the reuse or creation of its Lead worktree exactly as `plan lock` (§4.8) does for one PR, then `pre-start` in the new Lead worktree, output in `run/<plan>/logs/lead-<key>.pre-start.log`; on failure the worktree and branch are removed, nothing in the database changes (`start_failed_at` keeps its value) and the result is `setup_failed`. After `pre-start` passed, a new Lead Branch is pushed (`push origin <lead branch>`, no `-u`; the worktree is added with `--no-track`, hs-d243); a failed push does not fail the start: one `error` Event with code `push_failed` and `details.pr`.
- **Rows**, one transaction, with a compare-and-swap on `status = 'waiting'`: `status = dispatch`, `lead_worktree` (the Lead worktree), `started_at`, `start_failed_at = null`. **Event** `pr-started` `{pr, repo, base, lead_branch, lead_worktree, base_sha, reused, pushed, by: "pr start"}`. The PR's `queued` Jobs are now Ready (§3.2) and engine E3 schedules them.

```json
{"ok": true, "pr": {"key": "ui", "status": "dispatch", "lead_branch": "pay-4521/ui", "lead_worktree": "/home/clayton/projects/pay.pay-4521.ui", "base_sha": "9c1e7a2…", "reused": false, "pushed": true}}
```

Idempotent: yes, as stated; a PR that E7 started meanwhile returns `"already": true`.

### 9.8 `pr view`

```
cpd pr view [--pr <key>] [--plan <id>]
```

The detail behind a `pr-feedback` (hs-d98, D10; contract §7.6): the checks, reviews and comments of one PR's pull request, which the poller does not store. Read-only.

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR to view, selected by §1.3's rule (omitted: the Plan's one PR that is not `cancelled`) |

| Precondition | Code | Exit |
|---|---|---|
| the PR is `open` or `merged` (`waiting`, `dispatch` or `cancelled`: it has no pull request), as `pr rebase` | `illegal_transition` | 3 |
| the PR's policy resolves (contract §8.3 "Validation", as `pr post`, §9.8b): its Context has an owner | `owner_unset` | 3 |
| its owner has a policy file | `owner_unconfirmed` | 1 |
| its policy files are valid (`details` as in §8.3) | `policy_invalid` | 1 |
| a `gh` call fails (§1.3) | `gh_failed` | 1 |

Effects: **gh**, in the PR's Lead worktree: `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url,baseRefName` and `gh api repos/{owner}/{repo}/pulls/<n>/comments`. No rows, no Event.

Output: `plan`; `pr`, the PR's `key`, `number`, `url`, `title`, `jira` (the PR's Jira ticket, the key or null: pro-d127, B2), `base` (the pull request's base branch on GitHub, `baseRefName`), `base_pr` (the base PR as `<plan>/<key>`, derived from the PR's stored `base`, or null: it differs from `base` only until the poll has followed a repoint, contract §3.8 "Stacked PRs" step 8), `state` (`open`, `closed` or `merged`), `review_decision` (`none`, `approved` or `changes_requested`; `approved` only on a human's approval in `reviews`, the rule of contract §7.6 step 2 and `pr view` knows a bot by its login alone, the `[bot]` suffix or `inbox.bot_reviewers`, because `gh pr view --json reviews` carries no actor type: a bot outside that list can show `approved` here while the poll stores `none`) and `checks` (`none`, `passed`, `pending` or `failed`; the newest-run reduction of contract §7.6 step 3b (pro-d230), then its per-check mapping: FAILURE, TIMED_OUT, ACTION_REQUIRED, STARTUP_FAILURE, ERROR are failing; CANCELLED is not; pending checks never fail; the summary applies the human-gated rule and the required-names rule of contract §7.6 step 3b with the PR's resolved `pr.human_gated_contexts` and `pr.required_checks` (hs-d743) and `pr view` resolves the PR's policy and refuses `owner_unset`, `owner_unconfirmed` and `policy_invalid` as `pr post` does, §9.8b); `checks`, one entry for each run of each check, `{name, conclusion, url?}`, every entry `gh pr view` lists, superseded ones included (pro-d230), with their conclusions unmodified (the reduction and the mapping apply to the summary only); `reviews`, `{id, author, state, body}`; `comments`, `{id, author, body}`; `inline_comments`, the pull request's review comments, `{id, author, path, body}`; `feedback` (pro-d68), the row's stored list of open feedback items (contract §2.2 "Feedback item"; null when the poll never read it), which is what `pr reply --to` names; and `post` (pro-d65, contract §2.2), null for an unposted PR, else `{channel, ts, link, posted_at, reaction, reacted_at, refusal}`; `reacted_at` and `refusal` are read from the queue row of (post channel, post ts, post emoji); `reacted_at` is null until the reaction succeeds, `refusal` is null until the reaction is refused (parking the row) and both are never set together. `reviews`, `comments` and `inline_comments` are as before and list everything, a bot's included.

```json
{"ok": true, "plan": "pay-4521",
 "pr": {"key": "ui", "number": 813, "url": "https://github.com/acme/pay/pull/813", "title": "Retry failed payouts (ui)", "jira": "PAY-813", "base": "main", "base_pr": null, "state": "open", "review_decision": "changes_requested", "checks": "failed"},
 "checks": [{"name": "ci/test", "conclusion": "FAILURE", "url": "https://github.com/acme/pay/actions/runs/1"}, {"name": "ci/lint", "conclusion": "SUCCESS"}],
 "reviews": [{"id": "PRR_kwDO…", "author": "reviewer", "state": "CHANGES_REQUESTED", "body": "Please add a test."}],
 "comments": [{"id": "IC_kwDO…", "author": "reviewer", "body": "Thanks for the fix."}],
 "inline_comments": [{"id": "1893", "author": "reviewer", "path": "src/payout.rs", "body": "This retry never backs off."}],
 "post": null}
```

Idempotent: yes (a read).

### 9.8a `pr reply`

```
cpd pr reply --pr <key> --to <item-id> (--body <text> | --body-file <path>) [--plan <id>]
```

Answers one feedback item (contract §2.2 "Feedback item"; pro-d68, Plan fb-path). **Callers:** Lead; not cockpit, not internal (§2). The Lead fixes each open item or decides not to and answers it either way, saying what it did or why not. It **never resolves a thread** (no flag, no `resolveReviewThread`): the reviewer does.

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR, selected by §1.3's rule |
| `--to <item-id>` | the `id` of an item in the PR's stored `feedback` list (`pr view` prints it) |
| `--body <text>`, `--body-file <path>` | the reply; exactly one of them |

| Precondition or failure | Code | Exit |
|---|---|---|
| not exactly one of `--body` and `--body-file`, or the body empty after trimming | `usage` | 2 |
| `--body-file` unreadable | `file_unreadable` | 2 |
| the PR is not `open` (`details` `{pr, status}`) | `pr_not_open` | 3 |
| `--to` is the `id` of no item in the PR's stored list (`details` `{pr, to, open: [ids]}`); nothing is posted | `feedback_not_found` | 3 |
| the `gh` call fails (§1.3) | `gh_failed` | 1 |

Effects: **gh**, one `gh api graphql` call, as the User's own GitHub login. For an item of kind `thread`: `addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: <id>, body})`. For kind `comment` or `review`: `addComment(input: {subjectId: <the pull request's node id>, body})`, the body prefixed by `Re: <item url>` and a blank line. **Every posted body ends with a blank line and the marker `<!-- cpd:reply -->`** (contract §2.2), so the next poll sees the answer and the item leaves the list within one poll. It writes nothing in the store and appends no Event.

Output: `{"ok": true, "replied": {"pr", "to", "url"}}`, `url` the posted comment's.

```json
{"ok": true, "replied": {"pr": "ui", "to": "PRRT_kwDO…", "url": "https://github.com/acme/pay/pull/813#discussion_r1893"}}
```

Idempotent: no (every run posts a comment).

### 9.8b `pr post`

```
cpd pr post [--pr <key>] [--plan <id>]
```

Posts a PR's pull request to Slack for review, so an agent never calls Slack tools by hand (pro-d64, pro-d65). **Callers:** Lead; not cockpit, not internal (§2). The Lead asks the User and posts only on the User's yes. A PR may be posted only once; a second attempt is refused `already_posted`. It reads the stored `pull_requests` row of the PR (the poll keeps it, every `watch.pr_poll_seconds`; `cpd pr poll` makes it fresh). Its only GitHub calls are the one read of check 8, before the send, and the auto-merge mutation after the send (Auto-merge, below). The cockpit's Inbox menu runs it too (**Post** on a `ready for user review` `pr-ready` row, §13.2), at once with no confirm popup, through the cockpit's command runner; a failure opens the error dialog titled `Post Pull Request`.

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR to post, selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one |

Check order; the first failure refuses and then nothing is sent and nothing is written:

1. the PR's status is not `open` (`details` `{pr, status}`) → `pr_not_open` (exit 3)
2. a `pr_posts` row exists for the PR (`details` `{pr, channel, ts, link, posted_at}`) → `already_posted` (exit 3)
3. the owner's policy resolves for the PR (`policy_invalid`, `owner_unset`, `owner_unconfirmed`, contract §8.3 "Validation") → refusal with listed exit codes (1 or 3)
4. `policy.pr.post.enabled` is false (`details` `{pr, key: "pr.post.enabled"}`) → `post_disabled` (exit 3). `pr.post.enabled` is set only by an owner or repo file (contract §8.3 "Key exposure", pro-d126): a machine file with a `pr.post` table makes check 3 fail `policy_invalid` before any post
5. the stored pull request is not `authored` (`details` `{pr, number}`) → `not_authored` (exit 3)
6. the channel is `policy.pr.post.channel` when not empty, else the machine config's `inbox.team_channel` when not empty, else (`details` `{keys: ["pr.post.channel", "inbox.team_channel"]}`) → `post_unconfigured` (exit 3)
7. `postable::unready(&row)` (contract §2.2, §8.3, the eight Unready reasons: `not_open`, `draft`, `conflicting`, `changes_requested`, `checks_failed`, `checks_pending`, `comments_unread`, `bot_comments`) is not empty (`details` `{pr, number, url, reasons: [<as_str of each>]}`) → `pr_not_ready` (exit 3)
8. ONE `gh api graphql` read of the repository and the pull request (the auto-merge read query: `autoMergeAllowed`, the pull request's `id`, `autoMergeRequest { enabledAt }`, `squashMergeAllowed`, `mergeCommitAllowed`, `rebaseMergeAllowed`): all three methods false (`details` `{repo, number, squash, merge, rebase}`) → `no_merge_method` (exit 3); a read that fails or cannot be parsed → `gh_failed` (exit 1, `details` `{argv, exit, stderr}`). Both refuse before the Slack send: nothing is sent and nothing is written

| Refusal | Exit | Details |
|---|---|---|
| `pr_not_open` | 3 | `{pr, status}` |
| `already_posted` | 3 | `{pr, channel, ts, link, posted_at}` |
| `policy_invalid` | 1 | `{key: "pr.post.*", …}` as contract §8.3 |
| `owner_unset` | 3 | contract §8.3 |
| `owner_unconfirmed` | 1 | contract §8.3 |
| `post_disabled` | 3 | `{pr, key: "pr.post.enabled"}` |
| `not_authored` | 3 | `{pr, number}` |
| `post_unconfigured` | 3 | `{keys: ["pr.post.channel", "inbox.team_channel"]}` |
| `pr_not_ready` | 3 | `{pr, number, url, reasons: ["draft", "changes_requested", …]}` |
| `no_merge_method` | 3 | `{repo, number, squash, merge, rebase}`: the repo allows no merge method (check 8) |
| `gh_failed` | 1 | the read of check 8 failed or was unparseable; `{argv, exit, stderr}` |
| `slack_auth` | 3 | the Slack sign-in in pi's MCP credentials is missing or invalid (§21.7); run `pi mcp login slack` |
| `slack_not_configured` | 3 | no `slack` server in `~/.pi/agent/mcp.json` (§21.7); run `pi mcp add slack --url https://mcp.slack.com/mcp --exposure hidden` |
| `slack_failed` | 1 | the helper could not send the message (timeout, network, Slack service failure) (§21.7); check the channel to see if the message was posted |
| `slack_unparseable` | 1 | the helper's response is not the expected JSON (§21.7); the message may have been posted |
| `slack_helper_failed` | 1 | the helper could not load pi's MCP modules (§21.7) |
| `spawn_failed` | 1 | the helper could not be started |
| `store_failed` | 1 | the store write after a sent message failed (§21.7, D5); the message WAS posted to the channel and the link is in `details` |

Effects: **Slack**, through the helper (§21.7), one `slack_send_message` call with the rendered message from `policy.pr.post.template` (contract §8.3), sent with `unfurl_app_links: false` (pro-d245), so Slack shows no GitHub preview under it. Rows: one `pr_posts` row (contract §4.4) and, in the same transaction, the `jira_transitions` row `in_review` of the PR (§21.11, J4b; queued only when the `pr_posts` insert is new); the only later write is `reacted_at` (PR `react`). `pr post` never calls Jira: engine E19 does (§21.11). Events: none (contract §5.1). Limits: a crash between Slack's answer and the row can post twice on a rerun; a timeout leaves the outcome unknown, check the channel; two simultaneous runs of the same PR could both send and the second's insert fails as `already_posted` (documented, not guarded); the stored row is up to `watch.pr_poll_seconds` old, `cpd pr poll` first makes it fresh and a PR the poll never read for comments is `comments_unread`.

Output: `{"ok":true,"plan":<id>,"pr":<key>,"number":<n>,"url":<the pull request's url>,"post":{"channel":"C…","ts":"1793040197.490209","link":"https://missionlane.slack.com/archives/C…/p1793040197490209","posted_at":<time>,"reaction":<emoji name or null>,"reacted_at":null},"message":<rendered text>,"auto_merge":{"state":<state>,"reason":<text or null>}}`. `post.reaction` is the policy's value at post time (never changed on a policy edit; the final emoji for the reaction is resolved from the current policy when E17 places it, contract §3.7 A1, §8.3). `reacted_at` is null when the post is created; it is set by engine E17 (§21.9) after the reaction is added to the queue and successfully delivered.

```json
{"ok": true, "plan": "pay-4521", "pr": "api", "number": 813, "url": "https://github.com/acme/pay/pull/813", "post": {"channel": "C02JU6C2Z1D", "ts": "1793040197.490209", "link": "https://missionlane.slack.com/archives/C02JU6C2Z1D/p1793040197490209", "posted_at": "2026-10-26T21:36:37.490209Z", "reaction": "git-merge", "reacted_at": null}, "message": "Retry failed payouts (api)\nhttps://github.com/acme/pay/pull/813", "auto_merge": {"state": "enabled", "reason": null}}
```

**Auto-merge** (pro-d117, pro-d254, pro-d261). The read of check 8 precedes the send; the Slack post and the `pr_posts` row still come first, and the mutation's outcome never undoes them. After them the command reuses that read (no second read) and reports `auto_merge` `{state, reason}`, `state` one of `enabled`, `deferred`, `not_allowed`, `refused`, `failed`, in this order of cases: (a) `autoMergeAllowed` is false, stacked or not: `not_allowed`, stored `none`, no mutation, no Event; `reason` is one line saying the repo does not allow auto-merge and cpd merges the PR when it is ready (the cockpit's Merge, or the Lead with `cpd pr merge` after a human approves); (b) the PR is stacked (its base is another `prs` row's Lead Branch): `deferred`, stored `pending`, no mutation; (c) the PR already has an auto-merge request: `enabled`; (d) otherwise the mutation `enablePullRequestAutoMerge` with `mergeMethod` the first allowed of squash, merge, rebase in upper case (§9.10): `enabled`, `refused` (GitHub's own error message; stored `refused`) or `failed` (gh failed; stored `pending`, retried by engine E18). `reason` is null for `enabled`. The post and the command's exit status never depend on the mutation's outcome.

Idempotent: no (every run tries to post).

### 9.8c `pr rerun`

```
cpd pr rerun [--pr <key>] [--plan <id>]
```

Re-runs the failed GitHub Actions workflow runs of a PR's head commit (pro-d155), so a Lead never runs `gh run rerun` itself and never asks the User to click Re-run failed jobs. **Callers:** Lead; not cockpit, not internal (§2). Use it when the log of a failed check shows an infrastructure or transient runner failure, not a code defect on the branch: a code defect needs a fix Job (§9.8, `pr-feedback`).

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR, selected by §1.3's rule |
| `--plan <id>` | the Plan, else `CPD_PLAN` (§1.3) |

Check order; the first failure refuses and then nothing is re-run:

1. the PR's status is not `open` → `pr_not_open` (exit 3, `details` `{pr, status}`); no `gh` call is made
2. `gh pr view` gives the head commit, `head_sha`, read fresh from GitHub
3. `gh run list --commit <head_sha>` lists the GitHub Actions workflow runs of that commit. A run is **failed** when its `conclusion` is `failure`, `timed_out` or `startup_failure`, compared case-insensitively (`gh` prints them in lower case, GraphQL in upper case). A run still running has an empty conclusion and is not failed. `cancelled`, `skipped`, `neutral`, `action_required`, `stale` and `success` are not failed.
4. no run is failed → `no_failed_run` (exit 3, `details` `{pr, head_sha, runs: [{id, name, status, conclusion}]}`, every run of the commit so the Lead can see why); nothing is re-run

| Precondition or failure | Code | Exit |
|---|---|---|
| the PR is not `open` (`details` `{pr, status}`); no `gh` call is made | `pr_not_open` | 3 |
| no GitHub Actions workflow run of the head commit is failed (`details` `{pr, head_sha, runs}`); nothing is re-run | `no_failed_run` | 3 |
| a `gh` call fails (§1.3); the loop stops there, `details` is `{argv, exit, stderr, rerun}`, `rerun` the runs already re-run, `[{id, name}]`, empty when it failed before the first re-run | `gh_failed` | 1 |

Effects: **gh**, as the User's own GitHub login, with cwd = the PR's Lead worktree, so `gh` addresses the PR's repo (§9):

1. `gh pr view <number> --json headRefOid`: `head_sha`, the head commit of the pull request, read fresh from GitHub, not from the stored row
2. `gh run list --commit <head_sha> --json databaseId,name,status,conclusion --limit 100`: the GitHub Actions workflow runs of that commit, as `gh` lists them (at most the first 100)
3. for each failed run, in the order listed: `gh run rerun <databaseId> --failed` (only its failed jobs)

It writes nothing in the store and appends no Event (contract §5.1).

Output: `{"ok": true, "pr": "<key>", "head_sha": "<sha>", "rerun": [{"id": <databaseId>, "name": "<run name>"}]}`, one entry for each run re-run, in listing order.

```json
{"ok": true, "pr": "api", "head_sha": "4c1e7b2f9a0d3e58", "rerun": [{"id": 12345678901, "name": "CI"}]}
```

Idempotent: no (each call re-runs what has failed). A second call right after the first finds those runs in progress (empty conclusion) and is refused `no_failed_run`.

### 9.9 `pr push`

```
cpd pr push [--pr <key>] [--plan <id>]
```

Publishes an `open` PR's Lead Branch with a plain push, after the Gate when the PR's `gate.merge` is `local` (for `pr open` and `pr push`) or `gate.pr` is `local` (for `pr rebase`) (hs-d139, hs-d292; contract §7.6, §8.3; with `ci` there is no Gate and the two Gate rows below do not apply). It does not rebase: use it (the repair, as `pr rebase`; a fix Job's own merge pushes for itself, contract §3.8 "Updating"; after a `push_failed` it is the command to run) when the branch carries a merge of the base, which `pr rebase` would drop (§9.3), when the remote has not moved and the Jobs Merged since only need to reach the pull request, or for a stacked PR that took its base PR's new commits. It checks nothing about the base (hs-d490): a Lead Branch that lacks a base that moved ahead is pushed as it is.

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR to push, selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one |

| Precondition | Code | Exit |
|---|---|---|
| the PR is `open`, before the Gate and again after it (merged during the Gate: nothing pushed) | `illegal_transition` | 3 |
| the `pre-start` passes before the Gate runs; `details.tail` is its last 60 lines, `details.log` the full log `run/<plan>/logs/pr-<key>.pre-start.log` | `gate_error` | 1 |
| the PR's policy resolves (contract §8.3 "Validation", `details` as there): its Context has an owner | `owner_unset` | 3 |
| its owner has a policy file | `owner_unconfirmed` | 1 |
| its policy files are valid | `policy_invalid` | 1 |
| the Gate passes (§7.3, as `pr open` step 2); `details.tail` is its last 60 lines, `details.log` the full log `run/<plan>/logs/pr-<key>.pre-merge.log` | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |
| the push is a `(non-fast-forward)` or `(fetch first)` rejection and the remote tip does not contain G (the remote moved: a `pr rebase` pushed, or someone else); `details.stderr` is git's stderr; the Lead runs `pr rebase`. Any other rejection or failure is `git_failed`, with git's reason in its message (rule P) | `push_rejected` | 3 |
| the caller is not an agent, OR the incident is off, OR the resolved PR's Plan is not held (hs-d643): `CPD_SESSION` set and non-empty names an agent; a held Plan is in the incident; `details` is `{pr, plan, incident}` | `incident_hold` | 3 |

The Lead worktree's local changes refuse nothing (hs-d243; `dirty_target` is no longer a refusal of this command).

For an **adopted** PR (§9.4) the push is `git push --porcelain origin G:refs/heads/<head_ref>`, plain, never forced; `push_rejected` names `cpd pr rebase` (the catch-up, §9.3) as the next step.

Effects: no lock and no R first (hs-d243, §7.4): `G = rev-parse refs/heads/<lead branch>`, read once; with `gate.pr` is `local`, the Gate in the Lead worktree at G: `pre-start` (output `run/<plan>/logs/pr-<key>.pre-start.log`), then `pre-merge` from the PR's policy (output `run/<plan>/logs/pr-<key>.pre-merge.log`) (hs-d679, hs-d680); the PR re-read; then rule P, `git -C <lead worktree> push --porcelain origin G:refs/heads/<lead branch>` (no `-u`, never `--force` or a lease). A rejection whose remote tip already contains G (a merge during the Gate pushed a descendant) is a success that pushed nothing. Any other git failure is `git_failed`. **Rows**: none; no Event.

```json
{"ok": true, "plan": "pay-4521", "pr": "api", "pushed": true, "sha": "77aa01e…", "remote": "77aa01e…"}
```

`sha` is G, the gated commit (the Lead Branch's tip when the command read it); `remote` is the remote branch's tip after the push, past G when merges that landed during the Gate were pushed (hs-d243). Idempotent: yes; a G the remote already has (git's porcelain status `=`, "up to date"), or has a descendant of, still runs the Gate (with `gate.pr` is `local`), pushes nothing and returns `"pushed": false`.

### 9.10 `pr merge`

```
cpd pr merge <owner>/<repo>#<n>
```

Merges a pull request on GitHub: the Dispatch column's merge button (§13.2; hs-d272, hs-d273, hs-d274), or the Lead after a peer approves a posted PR (pro-d64, §9.10). **Callers:** the User, through the cockpit (`cockpit` true); the Lead, after a `pr-feedback` with reason `approved` of a posted PR; none of the other agent roles and no guard against them calling it (hs-d248). It takes a pull request, not a Plan's PR key: it needs no Plan or Context and `<owner>/<repo>#<n>` is the form the Dispatch column shows. Per-owner merge rules (repo-policy, hs-d263) are later.

Steps:

1. Read the pull request fresh from GitHub: `gh pr view <n> --repo <owner/repo> --json state,mergeStateStatus,headRefOid,title,mergedAt`. The stored `pull_requests.merge_state` is not trusted: it can lag.
2. Refuse, failing loud, unless `state` is `OPEN` and `mergeStateStatus` is `CLEAN`.
2a. Read the methods the repo allows: `gh repo view <owner/repo> --json squashMergeAllowed,mergeCommitAllowed,rebaseMergeAllowed`. It runs after the OPEN/CLEAN check and before the merge, so a pull request that is not OPEN and CLEAN is refused `merge_not_clean` and this read is skipped. All three false: refused `no_merge_method`, no `gh pr merge`. A non-zero exit or an unparseable answer: `gh_failed`, no `gh pr merge`.
3. `gh pr merge <n> --repo <owner/repo> --<method> --match-head-commit <headRefOid>`, `<method>` being `squash`, `merge` or `rebase`. `--match-head-commit` makes GitHub refuse when the branch moved after step 1's check. The method is the first allowed of squash, merge, rebase (the `--squash`, `--merge`, `--rebase` flags; GraphQL `SQUASH`, `MERGE`, `REBASE`), read fresh on every call and never cached: a cached permission that goes stale merges the wrong way. The order: squash is cpd's default and gives one commit titled by the PR (pro-d117); a merge commit before rebase is the User's choice (pro-d261): a merge commit leaves the Lead Branch's commits unrewritten, and rebase is the last resort. A repo that allows one method gets that one. cpd never passes `--delete-branch` and never deletes a remote branch: the branch is deleted by the repo's own `delete_branch_on_merge` setting, which repoints a pull request stacked on it to the base before it deletes the branch (contract §3.8 "Stacked PRs" step 8; pro-d240). cpd's own clean-up never touches remote branches (§9.2 "After a PR is merged") and nothing in cpd runs `git fetch --prune`, so the local clean-up is unaffected.

**Effects** (hs-d550 B1): after `gh pr merge` succeeds, one store transaction: read the pull request back once with `gh pr view <n> --repo <owner/repo> --json state,mergeStateStatus,headRefOid,title,mergedAt` (the precheck, hs-d550 B1: `gh::view_for_merge`). When it reads `state` `MERGED` the store transaction writes: the `pull_requests` row of <owner>/<repo>#<n>, when one exists, gets `state` `merged` and `merged_at` GitHub's `mergedAt` (else the transaction's time, changed_at now); when a `prs` row points to it, exactly the poll's merge (contract §7.6 pr poll step 6: `prs` `merged` with `merged_at`, `pr-merged`, the PR's `draft` Jobs `cancelled` with reason `pr merged` and one `job-cancelled` each (hs-d731, §9.2), Plan `done` with `plan-done` when it was the last PR, the unstack of PRs stacked on it). The open `pr-ready` item of subject `pr:<owner>/<repo>#<n>` is closed (closed_at, `inbox-closed`, as E8's reconcile closes one). The command output's `stored` is true. When it reads anything else, or the read fails, nothing is written and `stored` is false. The command succeeds either way. No stored row: nothing is written and the merge still succeeds. (hs-d550 B2: the precheck reading MERGED writes the merge as (A) does, with GitHub's `mergedAt`.) A row already stored merged writes nothing (idempotent). (hs-d550 B1, C: a failing `cpd pr poll` after the merge no longer matters, the merge is already stored; `pr merge` now opens the store.) GitHub's search listed the PR after its merge (hs-n415: evidence of why B1 changed the contract).

4. **The setting read** (pro-d240): once the merge is stored, or the read-back said otherwise, run `gh repo view <owner/repo> --json deleteBranchOnMerge`. `true` adds nothing. `false` adds the warning `auto_delete_off`: the merged branch stays on GitHub, and a pull request stacked on it is not repointed to the base, so turn the repo's setting on. A failed or unparseable read adds the warning `auto_delete_unread`, whose `text` carries the `gh` error's message. This read never fails the command: the merge, the store write and the exit status are the same whatever it says. It is skipped when the merge is refused or fails.

| Precondition | Code | Exit |
|---|---|---|
| the argument is `<owner>/<repo>#<n>`, `n` a positive integer | `usage` | 2 |
| the re-check finds the pull request not `OPEN` or its `mergeStateStatus` not `CLEAN`; `details` is `{repo, number, state, merge_state}` (GitHub's words); when the precheck reads `MERGED` (hs-d526), it writes the merge transaction and is still refused | `merge_not_clean` | 3 |
| with a non-empty repo policy `pr.required_checks`, when any required name is missing, pending or failed on the head commit; `details` is `{repo, number, missing: [names], pending: [names], failed: [names]}`; reads the head's checks fresh from GitHub (a second `gh pr view --json statusCheckRollup`, only when the list is not empty) as `pr merge` decides | `required_check_not_passed` | 3 |
| the repo allows no method: `squash`, `merge` and `rebase` are all false in the read of step 2a (only reached when the pull request is `OPEN` and `CLEAN`); `details` is `{repo, number, squash, merge, rebase}`; the message is `<owner/repo> allows no merge method: squash, merge commit and rebase are all off in its settings, so cpd cannot merge #<n>. Allow one in the repo's settings.` | `no_merge_method` | 3 |
| each `gh` call succeeds (also when `--match-head-commit` makes GitHub refuse, and the methods read of step 2a: a non-zero exit or an unparseable answer); `details` is `{argv, exit, stderr}` | `gh_failed` | 1 |

The setting read of step 4 is never a `gh_failed`: a failed read is the `auto_delete_unread` warning, and the exit status does not change.

Output:

```json
{"ok": true, "repo": "<owner/repo>", "number": 31, "method": "squash", "stored": true, "warnings": []}
```

`method` is the method step 3 used: one of `squash`, `merge`, `rebase`.

`warnings` is always present: `[]` when there is nothing to say, else a list of objects `{"code": ..., "text": ...}` with the codes of step 4, `auto_delete_off` and `auto_delete_unread` (pro-d240). The former field that said whether the branch was deleted is gone: cpd does not report it; `warnings` reports the repo's setting.

**Rows** (hs-d550 B1): one store transaction, recorded immediately after `gh pr merge` succeeds (when the precheck reads `MERGED`, hs-d550 B2), never delayed to the next poll. The `pull_requests` row of <owner>/<repo>#<n>: when it exists, `state` becomes `merged` and `merged_at` is GitHub's `mergedAt` from the precheck's `gh pr view`, or the transaction's time when GitHub was not read or did not return it (changed_at now); when a `prs` row points to it, the PR `dispatch → open → merged` or `waiting → merged` with `merged_at`, Events `pr-merged`, the cancelled drafts' `job-cancelled` (above) and, when every PR that is not `cancelled` is then `merged`, the Plan `deploy → done` with `plan-done` (contract §7.6 poll step 6). The same transaction queues the PR's `done` Jira transition (§21.11, J4c), next to the merge reaction. The open `pr-ready` Inbox item of subject `pr:<owner>/<repo>#<n>` is closed in the same transaction (closed_at, `inbox-closed`, hs-d550 B1, hs-n404). When no row exists, nothing is written and the merge still succeeds. No Event otherwise.

Idempotent: (A), (B), (C) yes, by the rows: a row already `merged` is compared with GitHub and written only if a value changed; when none do (already merged with the same `merged_at`), nothing is written. A second call of (A) finds the pull request `MERGED` and is refused `merge_not_clean`; (B) and (C) would have written it the first time, so the second call writes nothing (idempotent). The refusal is the same: `merge_not_clean` with `state` `MERGED`.

### 9.11 `host approve` and `host repair`

**Manager only, internal commands.** The host Manager approves and executes a cpd checkout repair when the install is skipped (contract §3.7 E6).

```
cpd host approve
cpd host repair
```

**`cpd host approve`** (internal; written by the adapter's `cpd_host_approve` tool after the User's Allow once): creates a private approval file for the Session, valid 10 minutes, one repair. Output: `{"ok": true}`.

**`cpd host repair`** (Manager only; contract §3.7 E6): repairs the skipped cpd install by resetting the checkout's main branch to origin/main, saving the diff and old HEAD. Needs an approval of the calling Session. Steps, all `git -C $HOME/projects/cpd`: (a) `rev-parse HEAD` (old head); (b) `diff HEAD` and, when non-empty, write to `$CPD_HOME/run/cpd-install-backups/<ts>.patch` (0600; <ts> = UTC `YYYYMMDDTHHMMSSZ`); (c) `update-ref refs/cpd-host/<ts> <old head>`; (d) `checkout --force -B main origin/main`; (e) `rev-parse HEAD` (new sha). Untracked files, other branches and stashes are never touched; no fetch (the script fetched). The approval is consumed only after step (e) succeeds. A failing git step stops it with `git_failed` naming the argv. Output JSON: `{"ok":true,"saved":{"ref":"refs/cpd-host/<ts>","patch":"<path or null>"},"head":"<new sha>"}`. The pill clears at the next sync tick after a repair (contract §3.7 E6).

### 9.12 `cpd tui action host`

(internal): what a click on the red version pill runs when `@cpd_host_alert` is set. Reads the install state file; when it is not `skipped` it does nothing. Finds the live (not archived) Manager of Context `host`; none: reports on the client with `display-message -c <client>` `cpd: no host Manager: apply dotfiles`. Else: brings that Manager into the chat slot as `session open` does (a stopped or asleep Manager is resumed), then, when `$CPD_HOME/run/cpd-install-notified` does not hold this incident's `since`, sends the Manager one message (as the User, `session send` with `--unblocks "cpd install skipped"`) and writes `since` and a newline to that file atomically; a later click on the same incident only opens the Manager. Message text (fixed): `cpd install skipped: <reason> (since <since>). Checkout: ~/projects/cpd. Read it with git -C ~/projects/cpd status / diff / log origin/main..HEAD (read only; never write in the checkout). Show the User what a repair would discard, ask with the tool cpd_host_approve, then run cpd host repair.` (contract §3.7 E6, §1.4, commands.md §15.18).

### 9.13 `cpd tui action incident`

(internal): what a click on the incident pill runs (contract §6.1). Reads the incident's stored Lead (`incident.lead`); when it is null, resolves the `lead_session` of the alphabetically first Plan in `incident_plans`. Brings that Lead into the chat slot as `session open` does (a stopped or asleep Lead is resumed), sends it no message. With incident mode off it reports on the client with `display-message -c <client>` `no incident: nothing to open`; with no resolvable Lead `incidents require a lead`. It prints nothing on stdout or stderr and exits 0, as every `cpd tui action` does..

## 10. `session`

Sessions (§2.2, §3.6) and their tmux panes (§6). Every tmux call is `tmux -L <tmux.socket> …` with `TMUX`, `TMUX_PANE`, `CPD_SESSION` and `CPD_ATTEMPT` removed (§6.1; hs-d551); a failing call is `tmux_failed` (exit 1). A Session's **place** is the `place` of its latest `session-moved` Event (`chat`, `parked`, `work` or `none`), or `none` when it has none.

**The chat slot.** **Amends contract.md §6.2** (the Lead's answer to Escalation `contract-commands-a1-q1`): the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat`. A pane with `@cpd_view = chat` is the **placeholder**: it runs `cpd tui view chat`, which reads `No Lead here: pick a Plan in the session tree` and it holds the slot while no Lead is there. **Amends §6.2's list of `@cpd_view` values** with `chat`. Why: the slot must hold a pane before any Lead exists (the first `cpd tui` on a machine) and after its Lead is parked (`swap-pane` needs a pane to swap in). At most one placeholder exists and only ever in the slot: `session park` creates it there (§10.5) and `session open` kills it with its window when a Lead takes the slot (§10.4).

### 10.1 `session list`

```
cpd session list [--context <id>] [--plan <id>] [--role <role>] [--state <state>] [--archived]
```

Filters combine with AND; `--role` and `--state` may repeat. Without filters: every Session except archived ones (`archived_at` is null). With `--archived`: archived Sessions are listed too, live and archived together (hs-d516); the flag is given once (clap refuses it repeated). Here `--context` and `--plan` are filters only: neither falls back to `CPD_PLAN`.

Effects: none; one read transaction.

```json
{"ok": true, "sessions": [
  {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "runner": "pi", "state": "waiting", "state_label": "φ: Worker question", "state_at": "…", "archived_at": null, "asleep_at": null,
   "place": "chat", "tmux_pane": "%12", "file": "/home/clayton/.pi/agent/sessions/…_596d007f-….jsonl", "forked_from": null, "plan_lead": true, "tier": "O3", "cwd": "/home/clayton/projects/cpd", "title": "Dynamic session titles contract", "titled_at": "…"}],
 "events_cursor": 48213}
```

`runner` is `pi` or `durable` (contract §2.2): how the Session runs, set at creation from `lead_runner` or `manager_runner` (contract §6.19); a `durable` Lead or Manager has a null `tmux_pane` and `place` `none` by design (contract §3.6). `role` is `lead`, `worker` or `manager` (B3-A1; a Manager has `plan` null, `attempt` null and `plan_lead` false; `--role manager` lists them). `plan_lead` is true for the Session that is its Plan's `lead_session`. `title` and `titled_at` are the dynamic title (contract §2.2, hs-d257) and when it was written, both null until the first `session retitle`. `archived_at` and `asleep_at` are null unless the Session is archived or asleep. Read-only.

### 10.2 `session show`

```
cpd session show [<id>]
```

`<id>` defaults to the selected Session (`CPD_SESSION`); it accepts the short forms of §1.3. Output: the fields of `session list` (`runner` included) plus `created_at`, `registered_event` (the id of its `session-registered` Event, the pi adapter's first cursor, §15.2), `model` and `thinking` (the row's `tier` as pi is told it, below: hs-d506), `loop_guard_turns` (the config value, for every Session), `switch` (null or a failed switch record, below) and, for a Plan's Lead, `open_escalations` (ids). Effects: none.

`model` is from the policy's `[model]` table for the key of the row's `tier` (contract §8.3, §9) (`<provider>/<model-id>`) and `thinking` is the level of the tier's digit (`off`, `low`, `medium`, `high`, `xhigh`, `max`, contract §9): the two halves of the `--model <model>:<thinking>` argument `session run` passes (§10.8), computed by the one function both use. The models are read at each call, so a policy edit shows at once.

`switch` (DEFECT 2) is null when the Session's last model switch succeeded or has not been attempted; otherwise it is `{tier, code, message, at, next}` where `tier` is the `tier` the switch tried, `code` and `message` are from the adapter error, `at` is the RFC 3339 UTC time and `next` is the exact `cpd session run <id>` command that relaunches this Session's pi so it starts on the current `tier` (taken from contract §6.5, §10.4, §10.8 as they stand at the time `switch` is read; the docs Job picks and cites it).

```json
{"ok": true, "session": {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "runner": "pi", "state": "idle", "state_label": null, "place": "chat", "tmux_pane": "%12", "file": "…", "forked_from": null, "plan_lead": true, "tier": "O3", "model": "github-copilot/claude-opus-5.5", "thinking": "high", "loop_guard_turns": 20, "switch": null, "cwd": "/home/clayton/projects/cpd", "title": null, "titled_at": null, "created_at": "…", "registered_event": 40117},
 "open_escalations": ["contract-commands-a1-q1"]}
```

Read-only.

### 10.3 `session fork`

```
cpd session fork <source> [--plan <id>] [--name <name>] [--message-file <file>]
```

§6.7, §6.8. Registers a new Lead Session forked from `<source>` and starts it in a parked window; the waiting for the source to be idle happens in that window, so the command returns at once.

| Flag | Meaning |
|---|---|
| `<source>` | the Session to fork (the short forms of §1.3 are accepted); a Lead. This command is for **tangents the User starts** (a bug, an idea), B4; no Worker is a fork (pro-d163) |
| `--plan <id>` | the new Session's Plan; default the source's Plan. The fork is a **side session** of the Plan: the Plan's `lead_session` does not change, so Plan Events still go to the Plan's Lead only (§15.2). It is drawn in the tree **directly under its Context**, after the Context's Plans, never under the Plan or under `<source>` (B1, §13.2) |
| `--name <name>` | default `<source name> (fork)`, cut to 80 characters |
| `--message-file <file>` | its first message, copied to `run/<plan>/messages/<session>.md`; without it the fork starts with no message |

| Precondition | Code | Exit |
|---|---|---|
| the source is a `lead` Session | `not_a_lead` | 3 |
| the source's `runner` is `pi`; a `durable` source has no pane (contract §3.6), refused before any write or tmux call (§1.3) | `durable_session` | 3 |
| a Plan is known: `--plan`, else the source's Plan (a Manager has none) | `plan_required` | 2 |
| the source is not archived; `details.next` = `cpd session unarchive <source>` | `archived` | 3 |
| the cockpit's tmux session (`tmux.cockpit_session`) exists, checked before anything is written; `details.next` = `cpd tui` | `no_cockpit` | 3 |

Effects:
- **Rows**, one transaction: `sessions` (new UUID v4, `role = lead`, the Plan's Context, `forked_from = <source>`, `cwd` = the source's, `tier = lead_tier`, `stopped`). **Event** `session-registered`.
- **tmux:** `new-window -d -t =cpd: -n park-<label> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<id> -e CPD_PLAN=<plan> -- cpd session run <id>`; `@cpd_session`. One transaction: `tmux_pane`, **Event** `session-moved` `{place: "parked"}`.
- If the cockpit disappears between that check and `new-window`, the command fails with `tmux_failed` after the Session is registered (Escalation `rs-session-a1-q2`). If a later step fails (`@cpd_session`, the row or the `session-moved` write), the new window is killed (`kill-window`) before the error is returned (teardown G11).

```json
{"ok": true, "session": {"id": "a41b…", "name": "cpd rust lead (fork)", "plan": "cpd2", "forked_from": "596d007f-…", "pane": "%52", "window": "@23"}}
```

Idempotent: no; each call registers a new Session.

### 10.4 `session open`

```
cpd session open <id>
```

Brings a Lead, or a Context's Manager, into the chat slot (§6.4). `<id>` accepts the short forms of §1.3, so `cpd session open <plan id>` opens that Plan's Lead.

| Precondition | Code | Exit |
|---|---|---|
| the Session is a `lead` or a `manager` (B3-A2) | `not_a_lead` | 3 |
| it is not archived; `details.next` = `cpd session unarchive <id>` | `archived` | 3 |
| its `runner` is `pi` and the cockpit's `main` window and its chat slot exist; `details.next` = `cpd tui`. A `durable` Session has no pane and skips the chat slot and this check (contract §6.19) | `no_cockpit` | 3 |

Effects:
- **Durable** (`runner` `durable`, contract §6.19): no pane and never the chat slot, so `require_pane` is not applied. When the Session is asleep or labelled `host crash loop`, one transaction clears `asleep_at` and `state_label`; the command then waits for the host's socket at `$CPD_HOME/run/host/<id>.sock` to accept (retrying on ENOENT/ECONNREFUSED with a back-off, a 30 s deadline, else `host_timeout`, exit 5) and returns `{"session": <id>, "pane": null}`. The engine's hosts step starts the host; the command starts nothing and writes no `session-moved`.
- In the chat slot already: nothing; `"already": true`.
- **tmux:** when the Session has no live pane (`stopped`, or its pane is gone): first `kill-pane` on a dead pane of the Lead, if one is still there, so it never adds a second window; then `new-window -d -t =cpd: -n park-<label> … -- cpd session run <id>` first, which resumes it (`pi --session <file>`, §6.5) and `tmux_pane` is recorded; the pane's `@cpd_title` is set by the precedence of contract §6.1 (the dynamic `title`, else the Plan's title or the name). Then `swap-pane -d -s <its pane> -t <chat slot pane>` and `rename-window -t <window> park-<displaced label>` for the window that now holds the displaced Lead. When the displaced pane is the placeholder or a history pane (`@cpd_view` `chat` or `history`, §10.12), `kill-pane` on it instead, which closes the now-empty park window with it, so placeholders and history panes never pile up in park windows.
- If a step after the new window is created fails (`set-option -p @cpd_session`, the swap, the row or the `session-moved` write), that new window is killed (`kill-window`) before the error is returned (teardown G11).
- **Rows**, one transaction: clear `asleep_at` when the Session was asleep (hs-d515); **Events** `session-moved` `{place: "chat"}` for this Session and `{place: "parked"}` for the displaced one.

```json
{"ok": true, "session": "8d0e…", "pane": "%41", "displaced": "596d007f-…"}
```

Idempotent: yes.

### 10.5 `session park`

```
cpd session park [<id>]
```

Moves the Lead in the chat slot back into a parked window (§6.4). `<id>` defaults to the Session in the chat slot; it accepts the short forms of §1.3.

| Precondition | Code | Exit |
|---|---|---|
| the cockpit exists | `no_cockpit` | 3 |
| the Session is a `lead` | `not_a_lead` | 3 |
| its `runner` is `pi`; a `durable` Session has no pane (contract §3.6) and is never in the chat slot, refused before any write or tmux call (§1.3) | `durable_session` | 3 |

Effects:
- Not in the chat slot: nothing; `"already": true`. A history pane in the slot (§10.12) has no Lead to park: nothing; `"already": true`.
- **tmux** (**amends contract.md §6.4**, which says only that park moves the Lead into a parked window): `new-window -d -t =cpd: -n park-<label> -P -F '#{window_id} #{pane_id}' -- cpd tui view chat` (a new park window holding a placeholder), `set -p -t <placeholder> @cpd_view chat`, then `swap-pane -d -s <placeholder> -t <chat slot pane>`. The Lead keeps its pid and is now alone in `park-<label>`; the placeholder fills the slot.
- **Rows**, one transaction: **Event** `session-moved` `{place: "parked"}`.

```json
{"ok": true, "session": "8d0e…", "pane": "%41", "window": "@31"}
```

Idempotent: yes.

### 10.6 `session state` (internal)

```
cpd session state <working|waiting|blocked|idle|stopped> [--label <text>] [--reply] [--session <id>]
```

How the pi adapter reports its agent state (§3.6, D4). `waiting` (an agent waits for another agent) and `blocked` (the User has to act) say why: `--label` is required with them and refused with any other state (`usage`); 1–80 characters. `waiting` is entered from `working`, `idle` and `blocked` and left for `working`, `idle`, `blocked` or `stopped` (§3.6). `--reply` (hs-d278) is allowed only with `idle`, else `usage`: it says the run that ended answered a message the User wrote to this Session (§15.4). Internal (§1.1): hidden in clap, run only by the pi adapter.

| Precondition | Code | Exit |
|---|---|---|
| the transition is in §3.6's table | `illegal_transition` | 3 |

Effects: when the state and label are unchanged, nothing (`"already": true`, no Event, §3.6). Otherwise **rows**, one transaction: `state`, `state_label` (null unless `waiting` or `blocked`), `state_at`. **Event** `session-state` `{from, to, label}`, plus `reply: true` when `--reply` was given (contract §5.2); `reply` is written only on a real change, so a call that changes nothing (`"already": true`) records no reply. No tmux.

```json
{"ok": true, "session": "596d007f-…", "state": "waiting", "label": "φ: Worker question", "from": "working"}
```

Idempotent: yes.

### 10.7 `session rename`

```
cpd session rename <name> [--session <id>]
```

cpd 1 §11 `session rename`, kept; the queue file becomes an Event. `--session` accepts the short forms of §1.3.

Effects: unchanged name: `"already": true`, nothing written. Otherwise **rows**, one transaction: `sessions.name`. **Event** `session-renamed` `{name}`, addressed to the Session (`session` set), which its adapter applies with `pi.setSessionName` (§15.4). **tmux**: sets `@cpd_title` to the new name on the Session's pane when it has one **and the Session has no dynamic `title`** (hs-d257, contract §6.1: the title wins; the rename still writes `sessions.name` and appends its Event); nothing else.

```json
{"ok": true, "session": "596d007f-…", "name": "cpd rust lead"}
```

Idempotent: yes.

### 10.8 `session run` (internal)

```
cpd session run <id> [--adopt <pid>]
```

The program in every Lead pane (§6.2): owns its terminal (§1.2). Steps of §6.7:
1. If the Session's file exists, resume: `pi --model <model>:<thinking> --session <file>` with the Lead's `[-e …]` (§6.5). **The model is the row's current `tier`'s** (hs-d506): pi, given `--model`, uses it instead of the model its session file recorded (pi 1.3.0 `core/sdk.js`: the file's model is restored only when none is given) and applies the `:<thinking>` level (`main.js` `buildSessionOptions`); so a Session `session retier` moved (§10.15) and then restarted (a reload for a new pi, the watchdog, Enter after pi exited, `session open`) runs on its new Tier, with its whole conversation.
2. Else, if `forked_from` is set, wait until the source is idle (§6.7 step 1, every 2 s, showing the waiting line), then `pi … --fork <source file> --session-id <id> -n <name> [-- @<first message file>]`.
3. Else start a fresh Lead: `pi --model … --session-id <id> -n <name> -- @<first message file>` for a non-by-user Session, or `pi --model … --session-id <id> -n <name>` for a by-user Session (hs-d542 A2: no `@<file>` when `sessions.by_user = 1`; the User's first prompt carries the first message with the queued turns, §15). Steps 2 and 3 take `--model` from the row's `tier` **when pi starts**, not from the `tier` the row had at registration (hs-d506).

Before it starts pi (steps 1–3) every pi a `session run` starts (§19), a Lead's and a Manager's, there are no ports to assign.

**Reload on install** (hs-d33, hs-d38, hs-d49, hs-d151; §6.7 "Reload on install"; incident hs-n141, cause hs-n148). The wrapper detects a new install by the identity of its executable (§3.4 of the contract), checked at every iteration of its loop **before any store access**. On a new cpd it re-execs **at once**, with no idle wait, whatever the Lead's place or state and **keeps pi running**: pi is its child and survives the `exec`. It `exec`s the same path with the same argv plus the internal argument `--adopt <pid>` (**internal**: written only by the wrapper's own `exec`, not an interface of the User or an agent); the new `session run` skips steps 1–3, starts no pi, `waitpid`s that pid as its own child and continues the same loop on it (watchdog, archive on quit, `Press Enter to resume`). The adapter reloads itself at idle (hs-a5, §15.6); a cpd install does not restart pi. Before the `exec` it runs `<path> --version` and re-execs only when that succeeds and `store` is `null` or `store.opens` is `true` (§1.2); otherwise it does not `exec`, logs once per installed file (identity) `reload skipped: <path> cannot open the store: <store.refusal.message>` on the stderr of its pane and keeps running, pi untouched; a later install is checked again (contract §6.7). **A store error never ends a wrapper while its pi runs:** a failed read with a replaced cpd leads to the re-exec above; any other failed read is logged once on stderr and retried at the next poll, pi untouched. Why: wrappers that waited for the Lead's idle read the store after a newer cpd had migrated it, failed `store_too_new` and exited and six Leads were ended instead of reloaded (hs-n141, hs-n148). If pi exits on its own after an install, the wrapper `exec`s the new binary (no `--adopt`) before any store write and the existing paths run there.

**Transient store errors** (contract §6.7 "Reload on install"). The store accesses before pi starts (open, the Session's row, `sessions.file`, the port, a fork's source) and after it exits (`session state stopped`, the name for the resume prompt) are tried again while they fail with `store_busy`, `lock_timeout` (the store's setup lock), or SQLite's busy or locked as `store_failed`: 0.5 s, doubling to 5 s between tries; the pane says once `Waiting for the store (<what>): <code>: <message> Trying again.` A wrapper never ends for it. Any other error (`session_not_found`, `not_a_lead`, `wrong_pane`, a malformed row) ends it as in the table below.

**Restart for a new pi** (hs-d49). A change of pi (the identity of `<pi agent dir>/install/current-version`, else of the resolved `pi` executable) is the only reload that restarts pi and it waits for the Lead's next idle (state `idle`, last turn finished, session file quiet `fork_quiet_seconds`): the wrapper sends pi `SIGTERM`, waits for it to exit (`SIGKILL` `timeouts.pi_stop_seconds` (default 10) after `SIGTERM`) and `exec`s the same path with the same argv (no `--adopt`); the new `session run` takes step 1. A parked Lead restarts at once. The Lead in the chat slot waits until it is idle **and** parked, or idle for 10 minutes, so unsent typing is not lost. A `waiting` or `blocked` Lead is never idle. See contract §6.7 "Reload on install".

**Adapter watchdog** (hs-d141; contract §6.7 "Adapter watchdog"). While pi is alive, the wrapper checks the Session's state at every poll (2 s). When the state has been `stopped` for more than **60 s** (a constant of the wrapper, not a config key; measured from the `session-state` Event that set it, so a state the wrapper itself set after a pi exit does not count while pi is not running), it restarts pi on the session file: `SIGTERM`, wait (`SIGKILL` after `timeouts.pi_stop_seconds`, as for a reload), then step 1. In one transaction before the restart it appends one `error` Event `{source: "session run", code: "adapter_missing", message, details: {session, state_since, seconds}}` with `session` = this Session (and its `plan`, `context`), which the new adapter delivers once it follows (§15.3). It applies to every Lead and Manager, whatever the place and whatever the idle rules of the reload; a `stopped` that ends (`idle`) before 60 s is the adapter's own reload and is never touched. After the restart the 60 s start again; a broken install loops once per 60 s with one `error` Event per restart (no cap: each is a visible fact and a repaired install ends it).

The first message is the file `plan new` or `session fork` wrote (§1.4); for a Manager, `run/<context>/manager-message.md` (§3.10); none when resumed. Before it starts pi it writes the `git` wrapper `$CPD_HOME/bin/git` if it is missing or different (idempotent) and puts `$CPD_HOME/bin` first on pi's `PATH` (contract §6.5, hs-d310). It is passed as `@<path>` (its absolute path). In the same transaction it clears `switch_failed` (DEFECT 2: pi now runs the row's current Tier, so any earlier failed switch is stale). While pi runs, it watches for the session file (§6.5) and records `sessions.file` once it appears (a data write, no Event). When pi exits it runs `session state stopped` if the adapter did not, prints `pi exited. Press Enter to resume <name> here.` and resumes on Enter, except after a quit by the User (below).

**Auto-archive on the User's quit** (hs-d91, hs-d122). pi exits with status 0 on the User's `/quit` and Ctrl+D (pi 1.2.0, `interactive-mode.js`: `shutdown()` ends in `process.exit(0)`; verified by reading it). The wrapper tells that quit from its own: when pi exits with status 0 and the wrapper did not cause the exit (it sent no reload `SIGTERM`), the User quit. Then, in this order:
1. If the Lead is in the chat slot, `session park` it first (§10.5), so the placeholder takes the slot.
2. Archive: a side fork runs `session archive <id>` (§10.10); a Plan's own Lead runs `plan archive --plan <p>` (§4.10), the cascade, forks included. The archive **rows are written before** any pane is killed and the wrapper's own pane is killed **last**.
3. The wrapper's pane is closed with the archive (§10.10 step 2, §4.10).

If the archive is refused (`archive_blocked`, for example a Job running), nothing is archived: the pane prints the refusal's message and then `pi exited. Press Enter to resume <name> here.`, as before. A non-zero exit, a signal, or an exit the wrapper caused (a reload) keeps the behaviour above: no archive. So does a Manager's quit: a Manager is archived only with its Context (§3.8). The tests use the harness's fake pi, exiting 0 or 1.

| Precondition | Code | Exit |
|---|---|---|
| the Session is a `lead` or a `manager` | `not_a_lead` | 3 |
| its pane's `@cpd_session` is this Session | `wrong_pane` | 3 |


Idempotent: yes; a restart resumes (§6.7 step 3).

### 10.9 `session close`

```
cpd session close <id>
```

Ends a Lead's pane (`<id>` accepts the short forms of §1.3): pi stops and the Session's file stays, so `session open` resumes it later (§10.4). It is the tree's `x` key (§13.2). Added by cockpit parity (amendment 14, §19).

| Precondition | Code | Exit |
|---|---|---|
| the Session is a `lead` | `not_a_lead` | 3 |
| its place is not `chat`: the Lead in the chat slot is never closed; `details.next` = `cpd session park` | `in_chat_slot` | 3 |

Effects:
- No live pane (`tmux_pane` null, or the pane is gone): nothing; `"already": true`. A `durable` Session (`runner` `durable`, contract §3.6) has no pane to kill and takes this branch: it succeeds with `"already": true` and writes nothing, as a `pi` Session with no pane does. It leaves the Session's host running (contract §6.19): the host is stopped when the Session sleeps (E13) or is archived (E14, §10.10), not by close.
- **tmux:** `kill-pane -t <pane>`. A parked window that held only that pane closes with it.
- **Rows**, one transaction, compare-and-swap on `tmux_pane` (when engine E2 recorded the pane as gone first, nothing is left to write: `"already": true`): `state = stopped` with **Event** `session-state` `{from, to: "stopped", label: null}` unless it already was `stopped`; `tmux_pane = NULL`; **Event** `session-moved` `{pane: null, window: null, place: "none"}`, the rows E2 writes for a pane that is gone (contract §3.6).

```json
{"ok": true, "session": "8d0e…", "pane": "%41"}
```

Idempotent: yes.

### 10.10 `session archive`

```
cpd session archive <id>
```

Archives one Session (contract §2.2 "Archived"); `<id>` accepts the short forms of §1.3.

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` (a Worker's Session is archived only by its Plan's cascade) | `not_a_lead` | 3 |
| it is not its Plan's own Lead (`plans.lead_session`); `details.next` = `cpd plan archive --plan <p>` | `archive_blocked` | 3 |
| its state is not `working`, `waiting` or `blocked`; `details.blockers` lists it | `archive_blocked` | 3 |
| it is not in the chat slot; `details.next` = `cpd session park` | `in_chat_slot` | 3 |

Effects, in this order:
1. **Check** the rule above; nothing is written if it blocks.
2. **tmux:** a live pane is closed as `session close` does (§10.9). A `durable` Session (contract §3.6) has no pane: nothing is closed and `closed` is empty, and its host is stopped (SIGTERM, then SIGKILL after `timeouts.pi_stop_seconds`, contract §6.19); step 3 writes `archived_at` as for any Session.
3. **Rows**, one transaction that re-checks the rule: `sessions.archived_at` = now, `sessions.asleep_at` = null (hs-d515); **Event** `session-archived` `{archived: true}`.

```json
{"ok": true, "archived": [{"kind": "session", "id": "a41b…"}], "closed": ["%52"]}
```

Idempotent: yes; already archived: `"already": true`, nothing written, nothing closed.

### 10.11 `session unarchive`

```
cpd session unarchive <id>
```

`<id>` accepts the short forms of §1.3.

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` | `not_a_lead` | 3 |
| its Plan is not archived (nor its Context); `details.next` = `cpd plan unarchive --plan <p>` or `cpd context unarchive <c>` | `archived` | 3 |

Effects: **rows**, one transaction: `sessions.archived_at = NULL`, `sessions.asleep_at = NULL` (hs-d515); **Event** `session-archived` `{archived: false}`. No tmux: no pane is started; `session open` resumes the Lead (§10.4).

```json
{"ok": true, "archived": [{"kind": "session", "id": "a41b…"}], "closed": []}
```

Idempotent: yes; not archived: `"already": true`, nothing written.

### 10.12 `session history`

```
cpd session history <id>
```

Shows an archived Session's history, read-only, in the chat slot (hs-d91, hs-d122). It is the tree's click on an archived Session row (§13.2).

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` | `not_a_lead` | 3 |
| its `runner` is `pi`; a `durable` Session has no pane (contract §3.6), refused before any write or tmux call, so no Lead is parked and no history pane is made (§1.3) | `durable_session` | 3 |
| it has a session file (`sessions.file` set and readable) | `file_unreadable` | 2 |
| the cockpit's `main` window and its chat slot exist; `details.next` = `cpd tui` | `no_cockpit` | 3 |

Effects, in this order:
1. **Park:** if a Lead holds the chat slot, park it as `session park` does (§10.5): a new `park-<label>` window with a placeholder, `swap-pane`, **Event** `session-moved` `{place: "parked"}` for that Lead.
2. **tmux:** `respawn-pane -k -t <the slot's pane> -- cpd tui view history <id>` (§13.2), then `set -p -t <pane> @cpd_view history` and `set -p -t <pane> @cpd_history <id>` and `@cpd_session` unset. A placeholder or an earlier history pane is the slot's pane, so it is respawned in place.
3. **Rows:** none besides step 1's Event.

```json
{"ok": true, "session": "8d0e…", "pane": "%41", "parked": "596d007f-…"}
```

`parked` is null when no Lead held the slot. Idempotent: yes; rerunning for the same id respawns the slot's one pane, so exactly one history pane exists and no Lead is parked.

### 10.13 `session send`

```
cpd session send <id> (<text> | --file <f>) --unblocks <reason>
```

Sends one message to a Lead or a Manager Session (hs-d92, hs-d133, hs-d514). It appends an Event; the target's adapter delivers it (§15.3). The message always wakes: when the target is idle it starts a turn at once; a working, waiting or blocked target gets it after its running turn, never interrupted. It carries a required statement of what it unblocks (hs-d507, hs-d514). The command is not the User's only way to write one: the engine writes the same Event, from the User's side (`from` null), to a Context's Manager for each of its own step errors (contract §3.7, hs-d314) and for each stuck Job (contract §3.7 "A stuck Job", hs-d318) and for each repeated ask (contract section 3.7 "A repeated ask").

| Argument | Meaning |
|---|---|
| `<id>` | the target Session (the short forms of §1.3 are accepted) |
| `<text>` | the message, a `Text` (1–20 000 characters) |
| `--file <f>` | the message read from a file, same limits |
| `--unblocks <reason>` | what the message unblocks, 1–200 characters, one line (hs-d514); shown in the message as `Message from <sender> (unblocks: <reason>): <text>` |

Exactly one of `<text>` and `--file` (else `usage`).

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` or a `manager`; a Worker is refused with "Session <id> is a worker, not a Lead or a Manager." | `not_a_lead` | 3 |
| it is not archived; `details.next` = `cpd session unarchive <id>` | `archived` | 3 |
| only when the target is `stopped` with no live pane: the cockpit's tmux session exists; `details.next` = `cpd tui` | `no_cockpit` | 3 |
| only if the sender is an agent ($CPD_SESSION set; User exempt, hs-d514): the target is not a Lead of a Plan in phase `done` or `archived` (hs-d507); `details: {plan, phase}` | `plan_inactive` | 3 |
| only if the sender is an agent: the sender has not sent the exact same text (trimmed) to a different Session in the last 60 minutes (no broadcasts, hs-d507); `details: {session, at}` | `broadcast` | 3 |
| only if the sender is an agent: the sender has not sent the exact same text (trimmed) to this Session in the last 60 minutes (no duplicates, hs-d507); `details: {at}` | `duplicate` | 3 |

**A `durable` target** (`runner` `durable`, contract §3.6): the Event is appended as for any Session; the command starts no pane, wakes no pane and raises no error for the missing pane (so `no_cockpit` does not apply); the host's follower delivers it (contract §6.13, P1.4), `started` is false and `pane` null. A `stopped` Lead or Manager with `runner` `pi` is accepted: this call starts it (hs-d725) as a parked `cpd session run <id>` window (hs-d726, A1) and it gets the message when its adapter starts; the chat slot never moves. A live pane of the Session means it is starting and nothing is started.

Effects:
- **Rows**, one transaction: **Event** `session-message` with `session` = the target, `context` and `plan` = the target's, `data` = `{from, text, unblocks}`; `from` is `CPD_SESSION`, else `PI_SESSION_ID`, else null (the User).
- **tmux:** when the target is `stopped` with no live pane, the mechanism of §10.4 (starting a parked window with a pane, §6.4) applies: first `kill-pane` on a dead pane of the Session, if one is still there; then `new-window -d -t =cpd: -n park-<label> | ctx-<context> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -P -F '#{window_id} #{pane_id}' -- cpd session run <id>`, the pane title and `@cpd_session` set, `tmux_pane` recorded and **Event** `session-moved` `{place: "parked"}` appended, the chat slot never moving. With a live pane, nothing is started. Otherwise no tmux, and none for a `durable` target.

```json
{"ok": true, "event": 4812, "session": "a41b…", "from": "596d007f-…", "unblocks": "PR #210 opened", "started": true, "pane": "%47"}
```

`from` is null when neither variable is set. `unblocks` echoes the reason. `started` is true when this call started a pane. `pane` is the started pane, or null when none was started. Idempotent: no; each call is one message.

### 10.14 `session retitle` (internal)

```
cpd session retitle <session> --width <cols>
```

Writes the Session's **dynamic title** (contract §2.2, hs-d257): a glance reminder of what was **last happening** in its conversation (hs-d543: the input is the last 2 turns only and the length is the chat slot's width minus 8; no word count), written by an auxiliary agent (hs-n95: cpd never calls an LLM API itself, only pi, here as a one-shot run, `pi --print`, docs: pi `cli.md`). **Internal:** engine E10 starts it (contract §3.7) for each Lead or Manager Session after its turns end, at most once every 10 minutes; anyone may run it. `<session>` accepts the short forms of §1.3.

`--width <cols>` (required, hs-d543) is the width in columns of the chat slot pane (contract §6.2); the title's limit is **N = cols − 8, capped at 200** (a Title's maximum). `--width` missing, not an integer, or **below 9** is `usage` (§1.5). Engine E10 reads the width from tmux and passes it (contract §3.7); a later resize truncates the shown title as the header does today.

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` or a `manager`; a Worker is refused | `not_a_lead` | 3 |
| it is not archived; `details.next` = `cpd session unarchive <id>` | `archived` | 3 |
| `locks/title-<session>.lock` is free (step 1; held: `details.reason` is `running`, nothing written and the other run is the one that counts) | `title_failed` | 1 |

**Steps.**
1. Take `locks/title-<session>.lock` without waiting and hold it for the run's whole life, so a Session never has two runs at once; the kernel frees it when the run dies, so a crashed run leaves no stale lock (contract §1.4).
2. **The conversation text.** Read the Session's pi session file (`sessions.file`, JSONL) and take **the last 2 turns** (hs-d543): from the **second-to-last** message of role `user` to the end of the file (the whole file when it holds fewer than two `user` messages), the text of the messages of role `user` or `assistant` in it (tool calls, tool results and other entries are not messages here), in order; if their text exceeds **8 000 characters**, cut it so the **newest** characters are kept. With no such message (no `file` yet, or none in it), **skip**: print the JSON below with `"skipped": "no_messages"`, run no pi, append no Event, write nothing and set nothing. A skip is not an error.
3. **Run pi** once, in `run/titles/` (created when missing): `pi --print --no-session --no-extensions --model <model>:off -- "<prompt>"` where `<model>` is from the policy's `[model]` table's `G` key (contract §8.3, §9) and `:off` is pi's level for no thinking: the title writer needs only the model. The prompt asks for a reminder of **what was last happening** in the conversation, answered as **one line `TITLE: <what was last happening, at most N characters>`** (N of the usage above) and nothing else. pi's environment is `PI_SKIP_VERSION_CHECK=1` only, no `CPD_*` (contract §6.5, "Title writer", hs-d562). Its stderr is kept (its last lines are the `tail` below).
4. **Parse and check the answer** (hs-d356). The title is taken **only from an explicit, parseable answer**: **the only line that starts with `TITLE:`** (leading spaces allowed), the text after the marker, trimmed and unquoted. Lines without the marker are ignored, so a preamble before the marked line does no harm. The answer is **malformed**, never a title, when: no line has the marker, or two lines do; the marked text is empty; it is over **N characters** (N = cols − 8, at most 200; it is **never cut**: an over-long answer is not a title; there is no word cap); it reads as sentences (a `. ` inside it, or it ends in `.`, `…` or `:`); or it is not a Title (`core/src/args.rs`: 1–200 characters, no control characters). An answer that is empty or only whitespace is `empty`, not `malformed`.
5. **Write**, in one transaction: `sessions.title` and `sessions.titled_at` (now); **Event** `session-titled` `{title}` (context, plan if any, session). Then **tmux**: sets `@cpd_title` to the title on the Session's live pane, when it has one (a tmux failure does not undo the write: it is reported on stderr and the header is made right by the next start or move of the pane, which apply contract §6.1's precedence).

pi failing to start or exiting non-zero, or an answer that is empty or does not pass step 4, is a refusal `title_failed` (exit 1), with `details: {session, reason: "running" | "spawn" | "exit" | "empty" | "malformed" | "invalid", exit?, tail?}` (`tail`: the last lines of pi's stderr) and **writes nothing**: `titled_at` stays, so the Session stays due and the engine retries at the next tick, logs the failure and reports it (an `error` Event) only once that Session's titling has failed for 10 minutes in a row; a success clears it (contract §3.7 E10, hs-d356). It appends no `error` Event itself: the engine's `report` does.

```json
{"ok": true, "session": "596d007f-…", "title": "Dynamic session titles contract", "titled_at": "2026-10-20T09:12:41Z", "event": 4821, "pane": "%12"}
```

`pane` is the pane whose header was set, null when the Session has no live pane. A skip prints:

```json
{"ok": true, "session": "596d007f-…", "title": null, "skipped": "no_messages"}
```

(`"already"` is not used here: it means the same value was already stored and a retitle never compares.) Idempotent: yes: rerunning is safe and writes a new title each time (it never answers `"already"`).

### 10.15 `session first-message` (internal)

```
cpd session first-message [--session <id>]
```

**Internal:** the pi adapter runs it at turn start for a by-user Lead or Manager Session whose `cpd-first-message` custom message has not yet been delivered (hs-d542 A3). Renders the first message at call time with the same functions `plan new` and `context manager` use (§4.1 step 3, §3.10), writes it to the session file's destination (`run/<plan>/lead-message.md` or `run/<context>/manager-message.md`) and prints JSON for the adapter to add the message to the run (§15).

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` or a `manager`; a Worker is refused `not_a_lead` | `not_a_lead` | 3 |
| the Session is a Lead of a Plan or is a Manager (not a side fork) | `not_a_lead` | 3 |

Effects:
- **Files**, one write: render the first message (contract §4.6 "Consumers", hs-d194 D6) and write it to `run/<plan>/lead-message.md` (for a Plan Lead) or `run/<context>/manager-message.md` (for a Manager), overwriting if it exists (hs-d542 A3: a by-user Session has no first message written at registration, so this is the first write).
- **Rows:** none. No database write, no Event appended.
- **tmux:** none.

Output: JSON with the rendered message text and metadata for the adapter to add it to the run.

```json
{"ok": true, "session": "596d007f-…", "role": "lead", "text": "You are the Lead of Plan …", "file": "/home/clayton/.cpd/run/pay-4521/lead-message.md", "event": 4820}
```

`role` is `lead` or `manager`. `text` is the rendered first message content. `file` is the path where it was written. `event` is the highest Event id at rendering (hs-d542 A3: for the pi adapter to track delivered messages, §15.7).

Idempotent: yes; rerunning renders the message again (the same if no Events occurred, different if the Context's records or Plans changed); the adapter tracks delivery by `details.event` and the fork-safe rule (§15.2 step 4, §15.7).

### 10.16 `session retier`

```
cpd session retier [--all | <id> [--tier <T>]]
```

Moves live Leads and Managers to the Tier the config names for them now (hs-d506: the User sets `lead_tier`, `review_lead_tier` and `manager_tier` and every running Session must follow, not only those registered later; pro-d139, pro-d140). With `--tier <T>` it instead moves the one Session `<id>` to `<T>` (pro-d142, below). **A Session's `tier` is not fixed at registration** (contract §2.2 Session): this command is the one that changes it. Workers are not touched: a Worker's Tier is its Job's (contract §9).

| Argument | Meaning |
|---|---|
| `<id>` | one Session (the short forms of §1.3) |
| `--all` | every Lead and Manager that is not archived |
| `--tier <T>` | with `<id>` only: the Tier the Session moves to instead of its target (below). With `--all`, or without `<id>`, it is `usage` |

Exactly one of the two: neither, or both, is `usage` (exit 2). `--tier` with `--all` is `usage` too (a clap conflict: nothing is changed) and so is `--tier` without `<id>`.

**The target.** A Session of role `lead` (a Plan's Lead and a side fork alike) moves to the config's `review_lead_tier` when its Context equals the non-empty `inbox.review_context`, else to `lead_tier`; one of role `manager` to `manager_tier` (contract §8.2; pro-d139, pro-d140). A later retier without `--tier` moves a Session to its target again, so a `--tier` override is reset by it.

**`--tier <T>`** (pro-d142). The one Session `<id>` moves to `<T>` instead of its target, with no config change. `<T>` is checked before anything is written, in the order of `plan new --tier` (§4.1): a bad syntax, or an effort the catalog lacks for the Tier's model (the models of the Session's Context, contract §8.3; the catalog is read only when `--tier` is given), is `tier_invalid` (exit 2); letter G or effort 5 is `tier_not_allowed` (exit 2), with the `details` and message of §4.1 (the Lead rule applies to the Manager too). A refusal changes nothing. A Session already on `<T>` is unchanged (`already`).

| Precondition | Code | Exit |
|---|---|---|
| the Session exists (`<id>`) | `session_not_found` | 4 |
| it is a `lead` or a `manager`; a Worker is refused, its Tier is its Job's | `not_a_lead` | 3 |
| it is not archived; `details.next` = `cpd session unarchive <id>` | `archived` | 3 |
| `--tier` (with `<id>`): a bad syntax, or an effort the catalog lacks for the Tier's model (`details.efforts`) | `tier_invalid` | 2 |
| `--tier` is letter G (`GX`) or effort 5, the Lead rule (for a Lead and the Manager) | `tier_not_allowed` | 2 |

`--all` refuses nothing for a Session it passes over: archived Sessions and Workers are not part of it and are not listed.

**Effects**, one transaction for the whole command:
- **Rows:** for each selected Session whose `tier` differs from its target, `sessions.tier` = the target (the store's history trigger records the old row) and `sessions.switch_failed = NULL` (DEFECT 2: a new target is a new attempt; any earlier failed switch is stale).
- **Event** `session-retiered` `{from, to}` for each such Session, one each, addressed to it (`session` set, with its `context` and `plan` when it has one), appended in the same transaction. `from` and `to` are Tier strings (`O3`, `O2`). `text`: `Session <id> moved from O3 to O2.`
- A Session already on its target is **unchanged**: no row write, no Event. When no Session changes, nothing is written at all.
- **No tmux call and no pi restart.** A running pi switches itself (below); a Session with no pi keeps the new `tier` for its next start (§10.8).

**The adapter's switch** (contract §5.3, §6.5; commands §15.3 `session-retiered`). The Event reaches the adapter of the Session it names. **At the Session's next idle** (right away when it already is) the adapter reads `cpd session show` for the Session's `model` and `thinking` (§10.2), finds the model in pi's registry (`ctx.modelRegistry.find(<provider>, <model-id>)`, the provider being the part of `model` before its first `/`) and calls `pi.setModel(<that model>)` and `pi.setThinkingLevel(<thinking>)`, once per change and never while the Session works, waits or is blocked: no turn is interrupted and nothing is restarted and the conversation stays whole (docs: pi 1.3.0 `extensions.md`, `ExtensionAPI.setModel` "for the current session without changing the configured default"). Several Events before one idle make one switch, to what the row says at that idle. When the model is not found, `setModel` throws or answers false (DEFECT 2), the adapter runs `cpd session switch-failed --code model_unavailable --message "<text>"` in addition to its existing error report (§10.18); after `setModel` accepts and the thinking level is set it runs `cpd session switched`, so `session show` clears the failed switch record and the Lead sees success.

```json
{"ok": true, "retiered": [
  {"session": "596d007f-…", "role": "lead", "from": "O3", "to": "O2", "event": 48214},
  {"session": "9a41c0de-…", "role": "manager", "from": "O5", "to": "O4", "event": 48215}],
 "unchanged": ["0c77e1b2-…"]}
```

`unchanged` lists the ids of the selected Sessions already on their target. `"already": true` is added (and `retiered` is `[]`) when nothing changed. Per Session: `session` (id), `role`, `from`, `to` and `event` (the id of its Event).

Idempotent: yes; run again, nothing changes (`"already": true`). With `--tier`, a rerun is the same. A later `session retier` without `--tier` moves the Session to the config's target again: the override is not sticky.

### 10.18 `session report` (internal)

```
cpd session report --what <text> --code <code> --message <text> [--details <json>] [--session <id>]
```

Reports an adapter error to the Manager (hs-d573, M4). Its callers are: **the adapter of a Lead**, for every error it hits (not to its chat, M3), once per distinct (`what`, `code`), keyed by `host.report`'s memory in pi/lib/host.ts; and **the adapter of any role** (a Manager's or a Worker's too) for `mcp_register_failed` (pro-d226; §15.1 'Errors'), for every refused registration, with `--what 'mcp attach <name>'`, and for `skills_unreadable` (pro-d260; §15.23), with `--what 'skills check'`. `mcp_register_failed` is exempt from the memory, so each refusal is one call; `skills_unreadable` is not, so it is one call per start. No other code is called by a Manager's or a Worker's adapter. A failed call is one log line, `report failed:`, never reported again (L4, §15.1 'Errors', §15.6 'Log').

| Flag | Meaning |
|---|---|
| `--what <text>` | what failed (1–2000 characters); `code` and `message` are from the error; combined as `cpd adapter: <what>: <code>: <message>` in the Event's `text` |
| `--code <code>` | error code (the adapter's code, not a command's exit code): `^[a-z][a-z0-9_]{0,63}$`, else `usage` |
| `--message <text>` | error message (1–2000 characters), else `usage` |
| `--details <json>` | additional details as a JSON object, else `usage`; the report adds `what`, `session` and `plan` (null if the Lead's Plan is none) |
| `--session <id>` | the Session reporting the error; default `CPD_SESSION` (§1.3) |

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |

Effects: **Rows**, one transaction: `events` (one `error` Event, source `adapter`; context = the Session's Context; **session = the reporting Session, plan null** (DEFECT 1: so the Manager can identify which Session failed; the follower rule of §11.2 excludes adapter-source errors from any Session's follower, so a Lead does not see its own adapter errors); data `{source: "adapter", code, message, details}` with `details` = the given object plus `{what, session, plan}`; text `cpd adapter: <what>: <code>: <message>`). Not idempotent by design: every call appends an Event.

Output:

```json
{"ok": true, "event": "4823"}
```

### 10.18a `session switch-failed` (internal)

```
cpd session switch-failed --code <code> --message <text> [--session <id>]
```

Records a failed model switch (DEFECT 2): the adapter of a Lead runs this when `setModel` fails (the three `unavailable` paths), to record the failure for the Lead to see in `session show` and to help it relaunch pi.

| Flag | Meaning |
|---|---|
| `--code <code>` | error code (the adapter's code): `^[a-z][a-z0-9_]{0,63}$`, else `usage` |
| `--message <text>` | error message (1–2000 characters), else `usage` |
| `--session <id>` | the Session whose switch failed; default `CPD_SESSION` (§1.3) |

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` or a `manager`, never a Worker | `not_a_lead` | 3 |

Effects: **Rows**, one transaction: `sessions.switch_failed = {tier, code, message, at}`, where `tier` is the row's current `tier`, `code` and `message` are the given values and `at` is the current RFC 3339 UTC time. No Event. Idempotent: yes; rerunning with the same or different code/message overwrites the existing `switch_failed` (the latest failure stands).

Output:

```json
{"ok": true, "session": "596d007f-…"}
```

### 10.18b `session switched` (internal)

```
cpd session switched [--session <id>]
```

Records a successful model switch (DEFECT 2): the adapter of a Lead runs this after `setModel` accepts and the thinking level is set, to clear the failure record.

| Flag | Meaning |
|---|---|
| `--session <id>` | the Session whose switch succeeded; default `CPD_SESSION` (§1.3) |

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` or a `manager`, never a Worker | `not_a_lead` | 3 |

Effects: **Rows**, one transaction: `sessions.switch_failed = NULL`. No Event. Idempotent: yes; rerunning does nothing when `switch_failed` is already null.

Output:

```json
{"ok": true, "session": "596d007f-…"}
```


### 10.17 `session resume`

```
cpd session resume <id>
```

Resumes a Lead or a Manager Session that is blocked by the loop guard, appending an Event and delivering one message with a turn (L6 of hs-d552, hs-d571). `<id>` accepts the short forms of §1.3.

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` or a `manager` | `not_a_lead` | 3 |
| it is not archived; `details.next` = `cpd session unarchive <id>` | `archived` | 3 |
| it is `blocked` with the label `φ: loop guard`; `details` `{state, label}` | `not_loop_guarded` | 3 |

Effects:
- **Rows**, one transaction: nothing is written to the Session.
- **Event** `session-resumed` (session = the target, context and plan = the target's), `data` `{}`, `text` `Loop guard of Session <id> resumed.`. The adapter clears the trip (L5): empties the window, closes the blocking reason, appends the pi session entry `{tripped: false}` and delivers ONE message with a turn (the Event's text, `followUp`), so one turn starts.
- **tmux:** none.

```json
{"ok": true, "event": 4816, "session": "a41b…"}
```

`event` is the id of the `session-resumed` Event. Idempotent: no; each call is one Event. A second call finds the Session no longer blocked once the adapter has acted.

## 10a. `board` (hs-d514)

The message board: a SQLite table whose posts name who they are for (to), read by `cpd board list` and picked up at an agent's turn start by `cpd board take`. Posts are written by `cpd board post` and never start a turn (hs-d514). Important facts stay Context records.

### 10a.1 `board post`

```
cpd board post <text> --to <ref>
```

Post a message to the board. It appends no Event and never starts a turn (hs-d514). The message is addressed to a Session (the target's context is the message's context).

| Argument | Meaning |
|---|---|
| `<text>` | the message, a `Text` (1–2000 characters) |
| `--to <ref>` | the target: `manager` (the live Manager of the sender's Context, else `no_manager`), a Plan id (its live Lead Session; a Plan in phase `done` or `archived` is refused `plan_inactive`), or a Session id |

| Precondition | Code | Exit |
|---|---|---|
| `<text>` is 1–2000 characters | `usage` | 2 |
| the target Session exists (resolved from `<ref>`); when `<ref>` is `manager` and there is no live Manager, refusal `no_manager` | `session_not_found` | 4 |
| when `<ref>` is a Plan id, the Plan is not in phase `done` or `archived` (hs-d514); `details: {plan, phase}` | `plan_inactive` | 3 |

Effects:
- **Rows**, one transaction: **Row** `board_posts`: `at`, `context` (target's), `from_session` (sender's Session or null), `to_session` (target), `text`, `read_at` (null); `id` is auto-incremented.
- **Events:** none. It does not append any Event.
- **tmux:** none.

```json
{"ok": true, "session": "a41b…", "context": "inbox", "posted_at": "2026-10-21T15:30:00.000Z"}
```

Idempotent: no; each call is one post.

### 10a.2 `board list`

```
cpd board list [--session <id>] [--all]
```

List posts to a Session. By default, unread posts only (`read_at` null), oldest first (hs-d514).

| Argument | Meaning |
|---|---|
| `--session <id>` | the target Session (default: `$CPD_SESSION`); accepts short forms of §1.3 |
| `--all` | show all posts, including read ones (`read_at` is not null) |

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |

Effects:
- **Rows:** none.
- **Events:** none.
- **tmux:** none.

```json
{"ok": true, "session": "a41b…", "posts": [{"id": 142, "at": "2026-10-21T15:30:00.000Z", "from_session": "596d…", "text": "PR #207 opened", "read_at": null}, {"id": 143, "at": "2026-10-21T15:31:00.000Z", "from_session": null, "text": "Inbox item closed", "read_at": null}]}
```

Idempotent: yes; the same list each run.

### 10a.3 `board take` (internal)

```
cpd board take --session <id>
```

Return and mark as read the unread posts to a Session (internal, the adapter's message picker at turn start, commands.md §15.7, hs-d514). In the same transaction, the rows' `read_at` are set to `now`.

| Argument | Meaning |
|---|---|
| `--session <id>` | the target Session; accepts short forms of §1.3 |

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |

Effects:
- **Rows**, one transaction: **update** every `board_posts` row with `to_session` = the target and `read_at` null, setting `read_at = now`.
- **Events:** none.
- **tmux:** none.

```json
{"ok": true, "session": "a41b…", "taken": [{"id": 142, "at": "2026-10-21T15:30:00.000Z", "from_session": "596d…", "text": "PR #207 opened"}, {"id": 143, "at": "2026-10-21T15:31:00.000Z", "from_session": null, "text": "Inbox item closed"}]}
```

Idempotent: yes; the second call returns an empty list and updates nothing.

## 11. `events`

§5. Selection: `--plan <id>`, `--context <id>` or `--session <id>`, at most one (`usage`), or none for every Event; `--kind <kind>` may repeat (a kind not in §5.2 is `usage`). These flags are filters: they do not fall back to `CPD_PLAN` or `CPD_SESSION`.

`--session <id>` applies §5.3's rule, with "a Lead of Plan P" made precise (amendment 7, §19):
- for the Session that is its Plan's `lead_session`: Events with `plan` = its Plan; Events with `plan IS NULL AND context` = its Context; Events with `session` = itself;
- for any other Session (Workers, side forks): Events with `session` = itself;
- for a Manager (B3-A5): Events with `session` = itself only, so a Plan's Events never wake it (hs-d86). It reads them itself at turn start (§15.7).

### 11.1 `events list`

```
cpd events list [--plan <id> | --context <id> | --session <id>] [--kind <kind>]… [--since <id>] [--limit <n>]
```

`--since` is exclusive and defaults to 0; `--limit` defaults to 500 (1–5000). Effects: none.

```json
{"ok": true, "events": [{"id": 48214, "at": "…", "kind": "job-merged", "context": "building-cpd", "plan": "cpd2", "job": "contract-core", "attempt": "contract-core-a1", "session": null,
  "text": "Job contract-core merged at c0ffee1 (Tier O3). Next: cpd plan show", "data": {"sha": "c0ffee1…", "tier": "O3"}}], "next": 48214}
```

`next` is the id of the last Event returned, or `--since` when none; pass it as the next `--since`. Idempotent: read-only.

### 11.2 `events follow`

```
cpd events follow [--plan <id> | --context <id> | --session <id>] [--kind <kind>]… [--since <id>] [--until-idle]
```

§5.4, normative: one Event per line, in id order; without `--since`, only Events committed after it starts; polling `PRAGMA data_version` every `events.poll_ms`; ends on EPIPE, on kill, or when its stdout is closed (`POLLHUP` or `POLLERR` on fd 1, checked at each poll, so an idle stream whose reader is gone still ends), with exit 0, or with `--until-idle` once it has printed everything committed when it started.

**Stale Events** (pro-d214): a `--session` follower drops a `plan-done`, `pr-feedback`, `job-failed`, `escalation`, `error` or `attempt-nudge` Event whose condition a later Event or the PR's stored state proves over; the table, the fail-open rule and the cursor are in contract §5.4 and `--plan`, `--context`, unscoped follows and `events list` are unchanged.

**Adapter-source error Events** (DEFECT 1): a Lead's follower (`--session` with a Lead Session) and any other Session's follower (`--session` with any non-Lead Session) skip `error` Events whose `data.source` is `adapter`, so a Session does not see its own adapter error. The Manager can read all Events (no `--session` filter applied, or `--context` filter), including adapter-source errors with their `session` field naming which Session failed.

| Precondition | Code | Exit |
|---|---|---|
| the selected Plan, Context or Session exists | `plan_not_found`, `context_not_found`, `session_not_found` | 4 |
| `--since` is not above the highest Event id (a cursor from another database) | `cursor_ahead` | 3 |

**A migration in progress is waited out** (hs-n278; contract §4.2, §1.4): when it opens the store, or reopens it, while another cpd is migrating it, it waits, up to `MIGRATION_WAIT_CEILING` (10 minutes) and prints one line on stderr (`waiting for cpd pid <pid> to migrate the store <from> -> <to> (<command>)`), nothing on stdout; it does not fail `lock_timeout` for it, so an install never ends a follower.

**Any other store error ends it** (hs-d140; contract §5.4): `store_too_new` after an upgrade (the verdict of contract §4.5, which the follower recomputes when `PRAGMA schema_version` changes), `migration_changed`, `migration_order`, `store_busy`, `store_failed`, … is printed as the last line, as the failure envelope and the process exits with that code's exit (§1.2); it never stays alive on a store it cannot read. The pi adapter restarts a follower that ends (§15.2).

**Re-exec** (hs-d140; contract §5.4): between polls it checks its executable path with `core/src/host/exe.rs`'s `(device, inode, mtime)` check, as `session run` and `attempt run` do (hs-d55); when it changed it re-execs itself with the same argv except `--since`, set to the id of the last Event it printed (none printed: the cursor it started from), so no follower outlives an install. Each line:

```json
{"id": 48215, "at": "2026-10-14T21:41:02.114Z", "kind": "escalation", "context": "building-cpd", "plan": "cpd2", "job": "contract-commands", "attempt": "contract-commands-a1", "session": null, "text": "Worker contract-commands-a1 asks: … Answer: cpd answer contract-commands-a1-q1 \"<answer>\"", "data": {"escalation": "contract-commands-a1-q1", "question": "…"}}
```

Idempotent: read-only; the same `--since` replays the same lines.

## 12. `watch`

```
cpd watch [--ensure | --check]
```

(and the internal `cpd watch --restart-tmux`, `cpd watch --ensure --wait` and `cpd watch --mark-restart`, below)

The engine, one per machine (§3.7, §6.6, D5).

**Without `--ensure`:** runs the `@cpd_workers` step (below), then E1–E5, E7, E20, E21 and E8–E14 (E6 is removed; E4 is the `open` Jobs since hs-d340, no longer the completion step; E7, the PR start, since R34; E20, the parked drafts, after E7 and before E8 (hs-d731, contract §3.7 "Parked drafts"); E21, the closed-pull-request wake, right after E20 (contract §3.7); E8, the Inbox, §21; E9, the Context curation, since hs-d194 (hs-d549, hs-d551); E10, the Session titles, since hs-d257; E11, the team review read, §21.5; E12, the Worker archive, since hs-d516; E13, the Sleep, since hs-d515 — for a durable Session the engine also stops its host (contract §6.19); E14, archive asleep Sessions, hs-d630 — a durable Lead's or Manager's host is stopped too (contract §6.19)) every `tick_seconds` (a decimal, minimum 0.1; default 5) in the foreground, in `cpd-work:watch`, logging to stderr and appending the same lines to `run/watch.log`.

E5 (the PR poll) runs at its due time, `pr_poll_seconds` after the previous poll ended (5 s by default), not at the next tick: the engine sleeps until the earlier of its next tick and E5's due time (contract §3.7). The pi stop grace is `timeouts.pi_stop_seconds` (default 10; contract §8.2).

| Precondition | Code | Exit |
|---|---|---|
| `locks/watch.lock` is free | `watch_running` | 3 |

Effects: **Events** `watch-started` and `watch-stopped` `{pid}`, one `session-message` to each Manager that is not archived when this engine follows an earlier one (how long none ran, contract §3.7) and every row and Event of E1–E5 and E7–E14 (E3 starts nothing while a tmux restart is pending and the engine starts that restart once nothing is in the way, §3.7) (§3.7; a PR that E7 fails to start gets its own `error` with code `pr_start_failed` and is not tried again, §9.7; a distill run E9 starts appends its own `error` when it fails, §3.12; a retitle run E10 starts (§10.14) appends its own `session-titled` when it succeeds and when it fails E10 reports one `error` with code `title_failed` for the Session). An unexpected failure in a step appends `error` `{source: "watch", code, message, details}` for the Plan it concerns and the loop continues; the same failure for the same object is appended once until it clears. It waits out a migration in progress when it opens the store (hs-n278; contract §3.7, §4.2): it does not fail `lock_timeout` and logs the one waiting line to `run/watch.log`. It runs until SIGTERM or SIGINT, or when its store outgrows the binary and it stands down (contract §3.7 "An engine whose store outgrew its binary stands down"), then prints `{"ok": true, "stopped": true}` and exits 0.
**The `@cpd_workers` step** (contract §6.1; User, hs-d149, hs-d198; hs-d749): on each tick the engine reads, in one read, `N` (the occupancy of the `workers` pool: its tenant Jobs, a `failed` Job and a retried `queued` Job included, contract §3.4 "Tenancy"), `M` (config `concurrency`), `W` (**waiting**: the Ready Jobs, §3.2, the ones E3 would start if a slot were free) and `Q` (**queued**: every Job with status `queued`, waiting ones included) and builds the StatusBar's texts: the long form `workers Q󰞘W󰞘N/M` and the short form `Q󰞘W󰞘N/M`; The status line measures their display widths using tmux's #{w:…} format modifier, so no width is computed in the engine. The workers' pill with its two caps `` and `` (the text's width plus 2; hs-d479, contract §6.1); plain text with no style (contract §6.1); the engine still computes no width (tmux's `#{w:…}` counts styles as 0 cells). It runs `tmux -L <socket> set -g @cpd_workers '<long>' ';' set -g @cpd_workers_short '<short>'` **only when a text changed** since the last tick it set (the first tick always sets them and so does the first tick after a tmux failure or a missing server: a restarted server starts without them). No row, no Event; a tmux failure of the step is no `error` Event while a tmux restart is pending and has not failed (§13.3: the restart takes the server away under the step), it is tried again at the next tick. The indicator is therefore up to one tick (`watch.tick_seconds`) late and stale while the engine is stopped; this deviates on purpose from §17's "no tmux option carries state".

**The `@cpd_runner` feed** (contract §6.1 "The runner pill"; pro-d199): in the same tick, the workers step reads `worker_runner` from `$CPD_HOME/config.toml` afresh (`Config::load`; the default `pi` when the line is absent) and sets `@cpd_runner` to `pi` or `durable` in the same tmux invocation as the workers texts (the `;`-joined `set -g` of each value that changed). It is part of the step's remembered state: it sets the option only when the value differs from the one it last set (the first tick always sets it and so does the first tick after a tmux failure or a missing server). A config that does not parse is an error of the step, reported through `engine.fail`; the step then sets nothing: no value is substituted for the unreadable one and the option keeps whatever it held until the file parses again.

Idempotent: a second one is refused.

**E3: Linear vs parallel Attempts** (pro-d52, hs-d680, hs-d682, hs-d683; contract §3.7 E3, §7.2, §7.4, A1). When E3 schedules a Ready Job of a PR each pass, it derives which Attempt runs as the linear one (the PR's only live Attempt for its whole life, reusing the PR's Lead worktree on its Lead Branch) and which as parallel siblings (each with its own worktree): by the rule of contract §3.7 E3, (1) if the PR has a live linear Attempt, no Ready Job starts; (2) if the PR has only live parallel Attempts, every Ready Job is parallel; (3) if no Attempt is live, one Ready Job is linear and the rest are parallel. E3 passes this allocation to the driver starter, which takes the driver lock and enforces it atomically before starting, ensuring no two Attempts can share the Lead worktree.

**With `--ensure`:** starts the engine when it is not running and returns at once. It fails and creates nothing when `$CPD_HOME` does not exist (hs-d519 3c/5, docs/research/hygiene.md, bug 3c/5).

| Precondition | Code | Exit |
|---|---|---|
| `$CPD_HOME` exists | `watch_start_failed` | 1 |
| the window started and its process took the lock within 10 s | `watch_start_failed` | 1 |

Effects: probe `locks/watch.lock` (open and `flock(LOCK_EX | LOCK_NB)`, released at once). Free: start the server and `cpd-work` session if needed (§6.1), set the server options and hooks again on a running server (§6.1), respawn a dead `watch` pane in its window (killing the window of any other dead one), else `new-window -d -t =cpd-work: -n watch -c $CPD_HOME -P -F '#{pane_id}' -- cpd watch`. No rows. The engine itself appends its stderr to `run/watch.log` (§1.4), so no shell is needed.

```json
{"ok": true, "started": true, "pane": "%3"}
```

Idempotent: yes; a running engine gives `"started": false` with `"already": true`.

Every `--ensure`, plain or `--wait`, takes `locks/watch-ensure.lock` before it probes the engine's lock and holds it until it has started the engine, so two calls never start two windows. A plain call waits for it 30 s at most (a supervising call holds it up to 15 s + 10 s + 2 s) and then fails with `watch_start_failed` (`details.next` = `cpd watch --ensure`); it still answers `already` when an engine runs.

**With `--check`** (hs-d463; contract §3.7 "The install's engine order"): verifies and **writes nothing**, what `--ensure` and the engine's start depend on: no row, no Event, no migration, no window, no pane, no lock taken, no tmux option set, no server started, no directory made. In this order, failing at the first, each with its own error and exit code (§1.2): (a) the config loads (`config_missing`, `config_invalid`); (b) the store opens, from the **read-only verdict** of `cpd --version` (§1.2; `store::verdict`, contract §4.5) on the database opened read-only: it reads `user_version`, `schema_migrations` and this binary's list and **never migrates or writes**; the database file, its WAL and `locks/store.lock` are not written, created or locked (no `Store::open`, which migrates). No database is no refusal; a verdict that opens, with every pending migration known to this binary, passes; a refusal is its own `store_too_new`, `store_failed`, `migration_changed` or `migration_order`. The migration happens only in the new engine's own start, after the old engine is retired, so a failed check leaves the old engine working against an unmigrated store (hs-d463); (c) `tmux -V` parses and is at least 3.6 (`tmux_failed`); (d) the tmux listing: with the server of `<socket>` running, `list-panes -a -F` with the format of the liveness call (§6.3), as a UTF-8 client (§6.1) and every line parses (`tmux_failed`); with no server it is skipped; (e) `$CPD_HOME/locks` and `run` exist or can be made (their nearest existing ancestor is a writable directory) and `locks/watch.lock`, only if it exists, opens read-write, with no `O_CREAT` and no `flock`, so nothing is created or locked (`store_failed`); a missing file passes, `--ensure` creates it. Idempotent; no bound of its own.

```json
{"ok": true, "checked": ["config", "store", "tmux", "listing", "home"], "tmux": {"version": "3.8", "server": "running"}}
```

`"server"` is `"running"` or `"none"`. `cpd install` runs it with the new binary before it stops anything (§14.4, contract §3.7).
**`--mark-restart` (internal; hs-d471).** What `cpd install` runs right before it retires the old engine (contract §3.7 "The install's engine order", step 3); not an interface of the User or an agent. It writes `run/install-restart.json`, `{"by": "install-cpd.sh", "at": <now>, "engine_pid": <pid>}` (`pid` of the latest `watch-started` Event, the engine about to be retired), by rename of a temporary file, replacing any earlier marker. No Event, no row, no lock, no tmux call. It does not need the engine to run.

| Precondition | Code | Exit |
|---|---|---|
| the home has a `watch-started` Event | `watch_not_started` | 3 |

```json
{"ok": true, "marked": true, "engine_pid": 4242}
```

Idempotent: a second one rewrites the same marker with a new `at`. The next engine consumes the marker at its start: a restart back within 10 s (`PLANNED_RESTART_QUIET_SECS`) whose marker names the previous engine's pid is logged only (`watch-started`'s `text` and `run/watch.log`) and sends no `session-message` to the Managers; past 10 s, or with no valid marker, they hear it as before (contract §3.7 "A planned restart", hs-d471). The tests are listed there.

**`--ensure --wait` (internal; hs-d328).** What the tmux server's hooks run when the engine's pane died or its window is gone (contract §6.1, §6.6); not an interface of the User or an agent. As `--ensure`, with these differences:

- a held lock with no live pane in the `watch` window (its window is gone, its process is stopping) is waited for, 15 s at most, then the engine is started; with a live pane it answers `already` at once;
- it takes `locks/watch-ensure.lock` first, as every `--ensure` does and waits for it without a limit and holds it until it is done, so only one runs at a time; after a failed start it waits 2 s under it, so a failing engine is tried once every few seconds;
- a dead engine pane is respawned in its window (`respawn-pane -e CPD_HOME=…`), the other dead `watch` windows are killed;
- it sets the server options and hooks again on a running server (§6.1), as `cpd tui` does: **`--ensure` does it too**, so every start of the engine leaves the hooks in place;
- it appends one line `supervision: …` to `run/watch.log` with the outcome. Its own output is dropped by the hook.

The engine it starts says how long it was down to each Manager (contract §3.7).

**`--restart-tmux` (internal; hs-d294).** Started by the engine, detached, when a restart of the tmux server is pending and nothing is in the way (contract §3.7, §6.1; §13.3); it is not an interface of the User or an agent. It restarts the server onto the installed tmux as §13.3 describes and exits 0, also when it did nothing (the server needs no restart, it is not safe now, another run holds `locks/tmux-restart.lock`). It prints one JSON value, `{"ok": true, "outcome": "…"}` and logs to stderr, which the engine appends to `run/tmux-restart.log`. A failed restart exits with its failure and keeps the reason in `run/tmux-restart.pending`.

## 13. `tui`

### 13.1 `tui`

```
cpd tui [--attach] [--respawn]
```

Builds the cockpit (§6.1, §6.2) unless it exists and starts the engine.

Effects, each only when missing:
- **tmux:** the server with §6.1's options; the session `cpd` with window `main` (`new-session -d -s cpd -n main -x 215 -y 62`), split into the tree (left, 25 %), the plan view (centre top) above the chat slot (centre bottom) and the widgets (right, 25 %), with the centre column 50 %; the plan view is 25 % of its height over the chat slot's 75 % (reset-layout §13.2); each pane runs `cpd tui view <tree|plan|widgets>` with `@cpd_view` set, with no StatusBar pane (the StatusBar is the session's status line, below); the chat slot holds the placeholder (§10) or, when one exists, the Plan Lead whose latest `session-moved` Event with `place: "chat"` is the newest, moved in as `session open` does. "Plan Lead" here means any Lead with a Plan, as for `session open` (side forks included, a Context's grounded Lead never) and one that `session open` refuses stays where it is while the placeholder keeps the slot (Escalation `tui-cockpit-a1-q3`). The session `cpd-work`.
- **Layout:** right after building a new `main`, `tui` runs reset-layout once (§13.2). On an existing `main` it adds every pane of the layout that is missing (the `tree`, `plan`, `widgets` or `chat` placeholder, each running its view with `@cpd_view` set; the User killed it, or a build stopped half-way) where it goes in the layout above and then runs reset-layout, so the added panes are taken from the layout and the other panes get their sizes back (amendment 15, §19; tui-W5). A chat slot that holds a Lead is not missing, so no placeholder is added beside it; a placeholder added opens the last chat Lead as a new `main` does. On an existing `main` that still has a `statusbar` pane (a cockpit built before the StatusBar moved to the status line) it kills that pane once (`kill-pane`) and then runs reset-layout, so the other panes take the freed row and get their sizes back (amendment 15, §19). The same for a `worker` pane left by a cockpit built before the Worker pane was removed (pro-d63): `cpd tui` kills it once, then runs reset-layout and listing panes skips it quietly, as for the StatusBar.
- **tmux options:** every option, the border format, the root `MouseDown1Status` binding and the conditional `pane-died` hook of §6.1. `host` sets them at every server start, so a server started by `plan new` or `watch --ensure` has them too; `cpd tui` sets them again on a running server (rerunning is safe). It also restores tmux's default `MouseDown1Pane` binding (`select-pane -t = ; send-keys -M`), which replaces cpd's earlier override.
- **The status line:** `status on` (hs-d567; hs-d561 had `2`), `status-style default` and `status-format[0]` (the `─` line, the StatusBar, §13.2), set with `-t <cockpit session>` once the session exists, never `-g`, so `cpd-work` has no status row (contract §6.1).
- **`--respawn`:** `respawn-pane -k` on every live pane of `main` that has `@cpd_view` (the views and the chat placeholder), so each runs the installed binary again; a Lead or engine pane is never touched. `scripts/install-cpd.sh` runs `cpd tui --respawn` when a cockpit exists. (Why: `cpd tui` respawns only dead panes, so a reinstall would never reach a running view.) Bare `cpd` does it too when it finds a view on an older binary (§13.3). While a preview is on (`@cpd_preview` is set on the server, §13.2), the views keep the preview build, so `--respawn` prints one line on stderr: `a preview is on (<value>); its views keep the preview build until 'preview-tui.sh off'` (hs-n264). Nothing is printed when it is unset and the value's JSON is unchanged.
- `watch --ensure` (§12).
- **tmux version** (hs-d294): when the server runs an older tmux than the installed one, marks a restart pending (`run/tmux-restart.pending`, §13.3) and says so on stderr; it never restarts the server itself, since it may run inside it. The engine does it once nothing is in the way.
- No rows of its own; moving a Lead appends `session-moved`.

| Precondition | Code | Exit |
|---|---|---|
| with `--attach`: stdin and stdout are a terminal | `not_a_terminal` | 3 |

```json
{"ok": true, "built": true, "socket": "cpd", "session": "cpd", "attach": ["tmux", "-L", "cpd", "attach", "-t", "cpd"], "watch": {"started": true, "pane": "%3"}}
```

With `--respawn` the value also has `"respawned": ["%1", "%2", …]`, the panes restarted. With `--attach` it then replaces itself with `tmux -L cpd attach -t cpd` and prints nothing (§1.2); bare `cpd` is the same attach with the follow-ups of §13.3. Idempotent: yes; an existing cockpit gives `"built": false`. A pane of the layout that has died is respawned (`respawn-pane`, amendment 9, §19), never duplicated and one that is gone is added again, so a second run adds nothing; a view pane that dies while the cockpit runs is respawned at once by the `pane-died` hook (§6.1).

### 13.2 `tui view` (internal)

```
cpd tui view <tree|plan|widgets|chat|legend>
cpd tui view logs [--event <id>]
cpd tui view merge <owner>/<repo>#<n> --client <name>
cpd tui view history <id>
cpd tui view dialog <confirm|input|text> …
```

The ratatui programs in the cockpit's panes (§6.2). They own their terminal (§1.2), make no writes of their own (two exceptions: the `plan` view's scroll offsets and the `tree` view's folds, below) and change state only by running `cpd` commands (`session open`, `plan new`, `session close`, `session park`, …) as subprocesses. What they read is §17.

A view in a pane never exits on a key: the `pane-died` hook (§6.1) starts a view again whenever its program exits, so the widgets view has no `q`. A view that fails (at start or later) prints its error, waits 3 s and then exits, so the hook does not start it again in a tight loop. Without a terminal the error is returned at once.

**Cockpit parity** (User, 2026-10-15). The cockpit matches the running cpd 1 cockpit exactly: layout, elements, text, colours, keys, mouse and behaviour. The reference was cpd 1's cockpit spike, `tui/` on branch `main` before the switch-over (`tui/src/*.rs`, `tui/README.md`; `tui/` now holds the Rust cockpit crate `cpd-tui`); the checklist of every difference is [cockpit-parity.md](archive/cockpit-parity.md). Where this section states a deviation, it is deliberate and has its reason.

| View | What it is | Keys and mouse |
|---|---|---|
| `tree` | the **Context column**: Contexts, their Plans' Lead Sessions and counters | ↑↓ and j/k move the cursor, Enter opens the Lead in the chat slot (on an Inbox row it jumps to the item's cause), `n` a new Plan, `x` closes a Lead's pane, `s` and `d` snooze and dismiss the selected Inbox item, `?` the legend; the popups of `n` and `x` are the `dialog` view's; clicks and the right-click menu as below. The tree's main loop may write a frame trace to its log (§17 "Frame trace") when `CPD_TUI_TRACE` is set. |
| `plan` | the **Plan view**: one box per PR of the chat slot's Plan, each holding the flowchart of its Jobs; with a Manager in the chat slot, it shows **Pulse** (one row per Plan of the Context) | ←→↑↓ and h/j/k/l select, Enter opens a Job's Worker, Esc clears the selection; the wheel scrolls (3 rows, 4 columns); clicks as below; no `q`. The view remembers each Plan's scroll offset and restores it when a Lead of that Plan comes back to the chat slot (a Plan Lead and its side forks share it), clamped to the chart, which may have changed size; the selection is not kept (reset). The offsets also survive a restart of the view: they are kept in `run/tui/plan-scroll.json` (below; User, hs-d197, hs-d254). With Pulse, a left click on a row opens that Plan's Lead and the wheel scrolls the rows (3 rows a notch). The plan view's main loop may write a frame trace to its log (§17 "Frame trace") when `CPD_TUI_TRACE` is set. |
| `widgets` | the **Dispatch column**: the machine (hostname, CPU, GPU and memory) and the Inbox (§13.2 below; pro-d130) | no keys; a left click on a `pr-ready` row of the Inbox opens its url (on the label) or its owning Plan's Lead in the chat slot (on the rest of the row); a right click opens the Inbox item's menu; the wheel over the Inbox scrolls it; mouse capture on |
| `history` | an archived Session's history, read-only: the Session's pi file drawn with the session renderer the history view owns (`tui/src/history/render.rs`); its top line is `<name> (archived, read-only)`; **its content area is one row shorter than the pane, so the pane's last row is empty** (User, hs-d567; contract §6.1 "The blank row", Q2). Started by `session history` (§10.12) in the chat slot | `q` or Esc quits to the placeholder text; ↑/k, ↓/j, PageUp, PageDown and the wheel (3 rows) scroll; End or `G` follows the end again; no other keys |
| `chat` | the chat slot's placeholder (§10) | none |
| `legend` | the key legend, in a popup | ↑↓, j/k, PgUp/PgDn and the wheel scroll it when it is taller than its popup; any other key closes it |
| `logs` | the Event log, in a popup; `--event <id>` opens it at that Event | ↑↓, j/k, PgUp/PgDn and the wheel scroll; End or `G` goes to the bottom, where it follows again; `q` or Esc closes it |

| `dialog` | the **dialog kit's** popup (below; hs-d295, hs-d297), of one kind: `confirm` (a question, two buttons), `input` (a labelled text box, contract §6.1.1), `text` (scrollable read-only text), `checklist` (tickable rows, hs-d549), `select` (radio list, a cursor row picked with Enter or [OK]; hs-d-tui-docs) or `form` (a labelled text box and a labelled radio list, pro-d114); its colours are contract §6.1.1's (title pill, body 7, the first button the blue block, the others 15, an input's refusal yellow, its border focused blue 12 and unfocused white 15, its cursor the blue block) | Enter or `y`, or a click on the first button, confirms; Esc, `n`, `q` or a click on the second cancels; `input`: Enter accepts, Esc cancels; `select` and `form`: ↑↓ and j/k move the cursor, Enter or a click on [OK] confirms, Esc or [Cancel] cancels; `checklist`, `select` and `form`: the cursor row is styled like checklist's, PgUp/PgDn and the wheel scroll; mouse capture on |

**`plan`: the scroll offsets file** (User, hs-d254). The `plan` view keeps each Plan's scroll offset in `$CPD_HOME/run/tui/plan-scroll.json` (contract §1.4), a JSON object `{"<plan id>": [x, y]}`, so that the offsets survive a restart of the view (every install restarts the views, `cpd tui --respawn`). This is a deliberate exception to "the views make no writes of their own" (the `tree` view's folds file is the other, below): the file is the view's own state, not Ledger state, as the Worker pane is the view's own layout (§17 "Actions"). Nothing else reads it.
- **Read** once, at the view's start. A missing file is an empty map. A file that cannot be read or is not that object (invalid JSON, an id that is not a Plan id, an offset that is not two non-negative integers) is not an error: the view starts with an empty map, logs `scroll positions reset: <reason>` (no note, §17 "No notes") and replaces the file at its next write. The view never exits because of this file.
- **Written** atomically (a temporary file in the same directory, then a rename; the directory and its parents are created) whenever an offset has changed, at most once per second and again as soon as the chat slot's Plan changes. A failed write is logged (§17 "No notes") and retried at the next write; it never ends the view.
- **Not cleaned up**: archived or unknown Plans stay in the file, harmlessly. Offsets are stored as left, not clamped: clamping stays at draw.
- The selection is never stored (reset at every restart and switch, above).

**Design rule** (User, Plan tui-visuals, 2026-10-17; hs-d124; refined by the powerline look, hs-d266 and hs-d322, 2026-10-19). Top-level layout separators (the tmux pane borders) are colour 7 when unfocused (interactive: they can be dragged) and blue when focused (hs-d531, hs-d534; contract §6.1.1); separators inside a slot are colour 8. The pane headers are tmux's (contract §6.1), **with a rounded cap on each side** and, by the title rule (User, hs-d475; hs-d478): the text in **Title Case**, **no padding** inside the pill, **a read-only title pill (colour 7 inverted: text in the terminal's background colour, caps colour 7), blue on the focused pane (colour 4 inverted: text in the terminal's background colour, caps colour 4)** (hs-d534, replacing hs-d530's 8 and 7; contract §6.1.1) and the pill starting **three columns from the pane's edge**, like the in-pane titles. `` (U+E0B6, nf-ple-left_half_circle_thick, left) and `` (U+E0B4, nf-ple-right_half_circle_thick, right) are one cell each on the normal background: `━━━Context━━━━━━━━━━━━`. The `━` before the pill is part of the border line and takes the line's colour (7 unfocused, blue focused), not the pill's. This is the powerline style of the User's zsh theme (agnoster) and needs the MesloLGS Nerd Font that dotfiles installs on every machine (`font-meslo-lg-nerd-font`, `~/dotfiles/home/.chezmoidata/packages.yaml`). A section title inside a slot is §17's "Section titles" rule (hs-d475, hs-d478, hs-d480), e.g. ` ──PRs───────────3 ` (` ╭─Inbox───…───35 ` for a collapsible widget's title): the title in Title Case as a read-only title pill (colour 7 inverted: text in the terminal's background colour, caps colour 7, `palette::pill_static()`, hs-d534) with no padding spaces, a muted line (colour 8) to the right edge, an optional count at its end as a pill too, the same colours as the title, inside the slot's one column of padding on each side (hs-d476), its left cap three columns from the edge (hs-d478). A slot's first section has no title, so no two titles stack: the pane header is the slot's title. **Pills** have the same rounded caps (the Plan view's Job pills, the legend's sample pill, the StatusBar's items): a pill is its block with a cap on each side, in the pill's background colour as their foreground (white for an interactive pill, grey for a read-only title, blue for the focused pane's title, the meaning's colour for a coloured one; hs-d534, contract §6.1.1), so the cockpit has one look, rounded.

**The chat slot's header** (hs-d257, contract §6.1). The border header of a Lead's or a Manager's pane is tmux's, from the pane option `@cpd_title`, not a view's: it shows the Session's **dynamic title** (`sessions.title`, written by `session retitle`, §10.14, started by engine E10 after the Session's turns end, at most every 10 minutes) when it has one, else what it showed before: the Plan's title for a Plan's Lead, the Session's name for a side fork or a Manager. It changes when a new title is written, not on a timer; a pane that gets no title yet shows the fallback. The text is title-cased by `Tmux::set_pane_title` (the first letter of each space-separated word upper-cased, the rest unchanged; hs-d475), which writes `@cpd_title`; the store's titles are unchanged. **The tree keeps showing ids** (hs-d101, "Labels" below): a title is never drawn in it.

**The dialog kit** (User, hs-d295, hs-d297; `tui/src/dialog/`). Every cockpit confirmation and input is a **tmux popup** running `cpd tui view dialog <confirm|input|text|checklist|select|form> …`, built on this one kit. The kit has six kinds: **confirm** (a question with two pill buttons, for example `[Archive] [Cancel]`; used only by Delete, pro-d111), **input** (a labelled text box, contract §6.1.1), **text** (scrollable read-only text), **checklist** (a scrollable list of rows the User ticks, with the pill buttons `[Approve] [Later]`; hs-d549, hs-d551), **select** (a scrollable radio list, the cursor row picked with Enter or [OK], with the pill buttons `[OK] [Cancel]`; hs-d-tui-docs) and **form** (a labelled text box and a labelled radio list, pro-d114, below).
- **The caller keeps its actions.** It opens the popup with `display-popup -E`, which returns when the popup closes, then reads the **result file** the view wrote at the path the caller named with `--out <path>` (a temporary file the caller makes and removes): *confirmed* or *cancelled* and for `input` and `form` the typed value and choice. The view runs no command and holds no caller logic; the caller runs its `cpd` commands (§17).
- **Style.** A dialog is one rounded frame and the popup is exactly that frame (hs-d578, amending hs-d569 CD1, hs-d475 to hs-d478 and hs-d298's section-title form): row 0 is `╭─`, the title as a pill, `─` up to `╮`; rows 1 to H-2 are `│`, the inside, `│`; row H-1 is `╰`, `─` × (W-2), `╯`. The title appears there and nowhere else (no tmux title, no header rows). A title wider than `W - 6` cells is cut to `W - 6` and the top row keeps at least one `─` before `╮`; with no room for a pill (`W - 6 < 1`) none is drawn. The frame's corners, walls and lines (the `─` fill of the top row included) are colour 8 (muted); the title pill is the read-only pill (text in the terminal's background colour, in colour 7, caps colour 7 on the terminal background, `palette::pill_static()`), as every title pill. The view draws the frame itself and tmux draws none (`-B`, below), so a dialog cannot be dragged by its border any more. Buttons and keys are the `merge` popup's: a click on a button, Enter or `y` confirms; Esc, `n` or `q` cancels. In `input`, Enter accepts and Esc cancels and `y`, `n` and `q` are typed text. Mouse capture is on. **Scrolling** is ↑↓, j/k, PgUp/PgDn and the **mouse wheel** and End or `G` goes to the bottom (in `input`, the keys that are text stay text). A scrolling popup draws the scroll cues right-aligned (§17, "Scroll cues").
- **Size.** The popup is opened with `display-popup -E -B -w W -h H <command>` (`popup_args` in `tui/src/dialog/mod.rs`): `-B` means tmux draws no border and there is no `-T`, so the program gets a pane of the full `W` × `H` (tmux 3.8: `stty size` 6 42 for `-w 42 -h 6`). The inside is 40 to 76 columns (`popup_size`: `(natural + 2).max(title chars + 6).clamp(40, 76)`); the popup is the inside plus 2 wide (the walls), capped at the newest client's width minus 2 (at least 1; `newest_client`, as the `logs` popup does) and `body + 5` rows tall (the top and bottom borders and the three template rows below; it was `body + 5 + 2` with tmux's border), capped at the client's height (vertical margin 0). The `confirm` natural width is unchanged (`max(widest line, accept label + 6 + 8)`).
- **The frame and its rows** (hs-d578, template hs-n437; the Box rule, contract §6.1.1 "Boxes"). A popup of `H` rows with a body of `b` rows has `H = b + 5`: row 0 the top border, row 1 blank, rows 2 to `H-4` the body (`H - 5` rows), row `H-3` blank, row `H-2` the buttons, row `H-1` the bottom border. The two blank rows are spacing between elements, not padding. **Margin and padding:** the inside is `W - 2` cells wide and its first and last cell are the padding: content starts at popup column 2 (wall at 0, padding at 1) and ends at column `W - 3` (padding at `W - 2`, wall at `W - 1`), so the content width is `W - 4`; no padding row above or below. The popup has a margin of one column on each side (at most `client width - 2` wide, at least 1, so a column of the cockpit stays on each side) and 0 vertically. **The body:** `confirm` and `text`: each row is the padding cell, the row's text (wrapped or cut at `W - 4`), blank to the padding cell. `checklist`: entries as below, at width `W - 4`. `input`: the input box (the dialog's body rect, contract §6.1.1) is the box_widget, drawn from popup column 2 to column `W - 3` (the dialog's padding cells form the input's margin; `tw = (W - 2) - 6`, starting on popup row 2), with refusal rows directly under it (up to 2, the note colour, a leading space, cut at `W - 4`), the rest blank. **The buttons row** (row `H-2`): buttons are pills, the cap U+E0B6, the label, the cap U+E0B4, so `[Archive]` is nine cells. The accepting button (`confirm` the caller's label, `input` `OK`, `checklist` `Approve`; `text` `Close`, which is alone) is flush left: its left cap is at popup column 2. The other button (`Cancel`, `Later`) is flush right: its right cap is at column `W - 3`. Nothing is drawn between them. Colours (hs-d534: interactive pill 15, selected 12): the accepting button is colour 12 inverted (text in the terminal's background colour), caps colour 12; the other is colour 15 inverted (text in the terminal's background colour), caps colour 15. The whole pill, caps included, is the hit target. **The scroll position** of a scrolling body (`1-5/20`) stays on the buttons row, colour 8, right-aligned inside the padding (its last character at popup column `W - 3`) and only when at least one blank cell lies between it and the pill on its left and, if there is a right pill, between it and that pill; else omitted.
- **Drawn.** In every drawing `[` and `]` stand for the pill's caps, U+E0B6 and U+E0B4, one cell each (so a pill is as wide as its drawing) and `█` is the cursor cell. Each drawing is a whole popup, 42 columns wide, exactly as the User sees it.

`confirm` (title `Archive`, question `Archive cpd-ui?`, accept label `Archive`; 42 × 6):
```
╭─[Archive]──────────────────────────────╮
│                                        │
│ Archive cpd-ui?                        │
│                                        │
│ [Archive]                     [Cancel] │
╰────────────────────────────────────────╯
```

`input` (title `New Plan`, prompt `Name:`, validator `plan-id`, the text `sketch` typed; 42 × 10; the two rows under the box are the reserved refusal rows, a refusal draws in them):
```
╭─[New Plan]─────────────────────────────╮
│                                        │
│ ╭─[Name]─────────────────────────────╮ │
│ │ sketch█                            │ │
│ ╰────────────────────────────────────╯ │
│                                        │
│                                        │
│                                        │
│ [OK]                          [Cancel] │
╰────────────────────────────────────────╯
```

`text` (title `session open l-ship`, body `no_cockpit: No cockpit.`; 42 × 6):
```
╭─[session open l-ship]──────────────────╮
│                                        │
│ no_cockpit: No cockpit.                │
│                                        │
│ [Close]                                │
╰────────────────────────────────────────╯
```

`checklist` (title `Retire records of cpd`, two entries `r-1` and `r-2`, the cursor on `r-1`; 42 × 10):
```
╭─[Retire records of cpd]────────────────╮
│                                        │
│ [ ] r-1 — duplicate r-0                │
│     use sqlite as the store            │
│                                        │
│ [ ] r-2 — documented docs/a.md         │
│     wire format                        │
│                                        │
│ [Approve]                      [Later] │
╰────────────────────────────────────────╯
```
- **`checklist`** (hs-d549, hs-d551; hs-d568, hs-d569): `cpd tui view dialog checklist --title <t> --items <json file> --out <file>`. `--items` is a JSON file, an array of `{"id": <record id>, "text": <full text>, "detail": <text>}` (hs-d569 CD2; `text` the record's full text; `detail`: `<reason> <target>` for a retirement, the doc paths joined by `, ` for a doc candidate; the caller builds it from `cpd context curation`, §3.15); a file that is not that shape is refused at the view's start, with the envelope of §1.2. The view draws a scrollable list of entries, one per array element, each of which is a header line `[ ] <id> — <detail>` (`[x]` when ticked) followed by the record's full `text`, wrapped to the body's width and indented by four columns; one empty row separates two entries (hs-d569 CD2). The cursor is an ENTRY: ↑↓ and j/k move one entry at a time, **Space toggles the cursor entry, a click on any line of an entry toggles that entry**, `a` toggles all entries (all ticked when any unticked, else all unticked), PgUp/PgDn and the wheel scroll the entries and after the cursor moves the window shows at least the cursor entry's header line. The buttons are `[Approve] [Later]` (hs-d569 CD3: `[Later]` decides nothing; Esc, `q` or a click on `[Later]` close with `approved: false`, every entry stays pending). Enter or a click on `[Approve]` approves (ticked entries decided). The **result file** is `{"approved": bool, "ticked": [id], "unticked": [id]}`, the ids in entry order (on `[Later]` `approved` is false and the two lists are what was on screen; the caller ignores them). Colours are contract §6.1.1's, title pill 7, frame 8, body 7, the first button the blue pill 12, the other the white pill 15. Mouse capture is on. The view runs no command: the caller does (the Inbox click table below).
- **`select`** (new): `cpd tui view dialog select --title <t> --prompt <p> --option <o> [--option <o>…] --out <path>`. `--option` is repeated, in the order shown; at least one; each one non-empty single line without control characters, distinct, at most 70 characters; otherwise `args_invalid` like an unreadable checklist file. The view draws a scrollable list of rows, one per option, `(x) <option>` for the cursor row (the choice) and `( ) <option>` for the others, styled as checklist's cursor entry. Keys: ↑↓ and j/k move the cursor one option and stop at both ends (no wrap); Enter or `y` confirms; Esc, `n` or `q` cancels. Mouse: a click on an option row moves the cursor there and does not close; a click on [OK] confirms, on [Cancel] cancels; the wheel and PgUp/PgDn scroll the body when it is taller than the popup and a cursor move scrolls so that the cursor row stays visible. The **result file** is `{"confirmed": true, "value": "<the option>"}` or `{"confirmed": false}`. Popup size: natural width = the widest of the prompt and `(x) ` + each option; body rows = the prompt's rows + one per option. The buttons are `[OK]` (first, left) and `[Cancel]` (second, right). Example, title `New Context`, prompt `Owner:`, options `clayton-duarte` and `other`, a popup of 42 × 8:
```
╭─[New Context]──────────────────────────╮
│                                        │
│ Owner:                                 │
│ (x) clayton-duarte                     │
│ ( ) other                              │
│                                        │
│ [OK]                          [Cancel] │
╰────────────────────────────────────────╯
```



- **The input box** (contract §6.1.1). The prompt is the box's label (trailing whitespace, then one trailing `:`, then trailing whitespace trimmed again; `Title: ` gives `Title`). There is no prompt row: the prompt moved into the box's top border. The box is always focused (border 12). The box starts on popup row 2 and is `W - 2` cells wide, with the refusal rows directly under it (up to 2, colour 8, a leading space, cut at `W - 4`; the frame and rows are "The frame and its rows" above) and its label pill follows the border's colour (12 here, focused). When the text needs more rows than available, the box shows consecutive rows from `start = max(0, cursor row + 1 - avail)` where `avail = max(H - 5 - 2 - REFUSAL_ROWS, 1)` and cursor row is `cursor / tw` (`tw = (W - 2) - 6`, `W` the popup's width; contract §6.1.1), the body being `H - 5` rows. The popup is sized so the box always fits the validator's longest accepted value (Plan id 24, title 200, name 80): `input_popup_size(client, title, prompt, initial, validator)` in `tui/src/dialog/mod.rs` computes the inside width and the height `body + 5` and caps both by the client (width minus 2, at least 1; height; no more `adds 2`; contract §6.1.1 has the worked sizes).
- **Validation** (`input`). The caller passes the check by name, from a closed set: `plan-id` (the Plan id of the `n` key, contract §2.1), `title` (`Title`, 1–200 characters) and `name` (`Name`, 1–80 characters). A refused value shows its reason **inside the popup**, which stays open and the User keeps typing; only an accepted value is written to the result file.

**`tree`.**
- **Top row and side margins.** The view keeps one blank row at its top and one blank column on each side; its content starts one row lower, inside the inner area (the pane minus one column left and right), so the tree never touches the pane's edges. Everything the view draws sits inside it: the cursor's blue row (contract §6.1.1), the message row at the bottom (a command's refusal or result, never an input) and the right-click menu's clamping.
- **Rows** (User, hs-d337, hs-d339; replaces hs-d309's `┌` / `└` shapes). There is **no indent** and **no in-pane `Context` title** (the pane border's CPD mark, contract §6.1, is enough; hs-d729, superseding the view-title part of hs-d671). Each Context is a collapsible block (§17, "Collapsible blocks") drawn by the one shared widget (`tui/src/fold.rs`). **Always:** the title row is `╭` + the Context's glyph + its name (bold), for example `╭󰽢 building-cpd`, with **no Plan count** (main's count of a folded Context stood for the Plans folding hid; a fold hides no live Plan, hs-d344) and each content row, one per live Lead Session, is `├─` + the row's glyph + its text, straight against the branch, for example `├─󰽢 cross-repo       3⋅1⋅0⋅0⋅92`, with the counters right-aligned. **The toggle shows and hides only the archived rows** (hs-d344): live Plans and Sessions always show, folded or expanded. **Folded:** the title row, the live rows, then the closing row `╰󰬦 expand`. **Expanded:** the title row, the live rows, the archived rows (below, colour 8), then the closing row `╰󰬬 collapse`. A Context with **nothing to fold** (no archived rows; hs-d477) has **no closing row** and no toggle: its **last row's branch is `╰─`** instead of `├─`, for example

```
╭󰽤 homeserver
├─󰽤 b2-budget …
╰─󰽧 speedtest …
```

The Context block's rows (title, `├─` rows, closing row) end at column `W - 2`, the row area's last cell (column `W - 1`, the pane's margin, stays blank; there is no right pad): the counters are right-aligned to it and the name is cut with `…` before them and before the cost (see Cost counters; the branch rows' glyph placement is unchanged). The Context tree is the one place that leaves the rail rule's `W - 4` right edge (pro-d186); contract §6.1.1 "The rail fold" keeps the rule for every other rail. A Context with archived rows keeps its closing row, `╰󰬦 expand` / `╰󰬬 collapse` and its live rows keep `├─`. A LIVE Context with **no row at all** under it ends with the clickable row ` ╰󰬪 new` — the muted `╰` and glyph and the word `new` in colour 15 (clickable text) — and a left click on that row opens the New Plan input popup for that Context (the same action as `n` and the context menu's New Plan...). An ARCHIVED Context with **no row at all** keeps the plain muted terminator ` ╰󰬪` (U+F0B2A, colour 8, no word, no toggle; hs-d574, replacing hs-d459 and the plain closing `╰─` line). Each Context block is a collapsible widget and so has **one empty row above and one below** (hs-d476; the view's blank top row is the first block's and the body has no vertical spacing). The closing row is drawn once per Context, never twice, by the shared helper (§17, "Collapsible blocks"; hs-d349), its lines colour 8 and its toggle (arrow and word) colour 7 (contract §6.1.1, hs-d534). The content rows are the Plans' Lead Sessions and every Session forked by `session fork` as siblings under the Context (B1, contract §6.8: a fork is a sibling under the Context, never nested under a Plan or under the Session it came from, whatever its `plan` or `forked_from`), ordered together at both levels by dot-urgency tier (red, yellow, magenta, cyan, shiro, then archived last), within a tier by creation (`created_at`, newest first; hs-d686, pro-d48); forks show no Job counters (the counters belong to the Plan Lead's row); then, for an expanded Context, **its archived Plans and Sessions, after the live ones, colour 8** (contract §6.1.1), with the same `├─` rows (hs-d339). There is **no archived `󰇘` row and no separate archived toggle**: the closing toggle is the archived rows' (hs-d344). Row: `├─`, glyph, one space, name (cut with `…` before the counters) and the four Job counters `planned⋅active⋅failed⋅done` right-aligned and **only the nonzero roles are drawn** (a zero role draws no number and no separator; a Plan with every role zero, for example all its Jobs cancelled, shows no counters; cancelled Jobs are not counted; no counters without a Plan; hs-d636). **planned** is `draft + queued` (all not-started work: a Plan with 3 drafts and 7 queued shows 10, not 3), colour 13 (magenta, `Meaning::Planning`) while it has any Job in `draft`—a draft appended after the lock included (hs-d173, hs-d179)—else 15 (the queued colour, `Meaning::NotStarted`). **active** is `running + merging`, colour 14 (cyan, `Meaning::InProgress`), or 11 (yellow, `Meaning::Review`) when any of it is landing (**landing** counts the Jobs whose active Attempt is checking or merging, contract §6.1.1, hs-d604). **failed** is 9 (red, `Meaning::Failed`, always). **done** is `merged`, 10 (green, `Meaning::Done`). hs-d636 replaces hs-d623's draft-takes-the-first-slot rule and the five-counter row; hs-d580 keeps the role colours and amends audit Q3, hs-d303 for the counters. On an ASLEEP Lead's row every counter is colour 8 whatever the count, its dot and name staying 7; separators `⋅` colour 8. **A row has no icon** (hs-d337, which replaced hs-d266's tree icons): the corner or branch, the dot glyph, the name. **The selected row** (User, hs-d351, 2026-10-20) is the Plan or Session row whose Session is in the chat slot, or a Context row whose Manager is in the chat slot: its **name** (the text after the dot glyph, up to the counters) is drawn in blue (`palette` blue, §17, "Blue") and blue **beats the muted colour of archived and asleep rows**; the rest of the row (the branch, the glyph, the counters, the PR status) keeps its colours and the name is not bold for being selected. There is no marker in column 0. The glyph is one of `` U+E3E3 (moon_alt_new: the Session needs you, coloured by case), `` U+E3E3, U+E3C8..U+E3E2 (the 28 moon_alt phases, section 17; the Lead is working: a loader of 28 frames, one every 50 ms, on one shared clock, so every spinner turns together; red 9 while its Plan has a failed Job being worked on or an open Worker Escalation (hs-d581, pro-d200), else the Work colour of pro-d200 ("The Lead dot rule", §17): magenta 13 while the Lead plans (any draft Job, grounding, a follow-up), yellow 11 for pull-request, deploy and landing work, cyan 14 for dispatch, white 15 with no Plan (superseding the phase colours of hs-d622 and hs-d624), §6.1.1), `` U+E3D5 (moon_alt_full: idle, nothing needed) or `` U+E3D5 (pi stopped, colour 8). A Session row's glyph looks at the Session's own state first (a working Session shows the spinner) and its `󰽢` comes only from its open, visible Inbox items (§17); a Context row shows the glyph of its Manager Session (hs-d215): a `󰽢` only from the Manager's own Inbox items (hs-d217), `󰽤` (colour 8) when the Context has none. Archived rows are colour 8 on every mark; asleep rows colour 7 except their Job counters, 8 and neither animates. The cockpit needs a Nerd Font for these symbols (https://www.nerdfonts.com/cheat-sheet). The rule, the colours and the precedences are §17's (The tree's dot rule) (hs-d622, hs-d624).

**Cost counters** (decisions pro-d179 and pro-d186; contract §2.2 "Session cost"). A Context row and a Plan row draw their `cost_usd` (when it is not `None`) as `cpd_core::metrics::format_cost` text, drawn beside the name or id, never at the right: on a Plan row one space after the name, on a Context row one space after the id. The Job counters stay right-aligned and unchanged, ending at the row's last cell, column `W - 2`. The cost is always colour 8 (`palette::muted_text()`, `Color::Indexed(8)`), never bold, on every row. With `width` the row area's width, `c` the width of the Job counters text and `k` the width of the cost text (`format_cost`, never a literal): a Plan's name is cut with `…` to `width - 6 - R` cells, where `R` is the reserve to its right, `1 + k` when the cost is drawn, plus `c + 1` when the Plan has counters (drawn or too narrow to be drawn), so a cut name leaves 3 blank cells before the counters; a Context's id is cut with `…` to `width - 4 - k` cells, so `3 + id + 1 + k <= width` and the cost ends at the row's last cell when the id is cut. A Plan with no nonzero counter draws the cost after the name and nothing at the right; a Context row draws nothing at the right. With no cost the row is as before. **By default the tree draws no cost on Context and Plan rows** (pro-d189): the state is a flag of the view's process, toggled by the `$` key (the StatusBar's costs button sends it), so a restart of the view or of the cockpit turns it off and nothing is stored. With the costs off a Plan name is cut only by its counters (it takes the room of the hidden cost, `R` is `c + 1` or 0) and a Context id only by the row's end; the rules above apply while the costs are on. The Workflow box footer's cost is not affected and is always drawn (pro-d194). Examples, with `row_of` (the helper of the tree's row tests, which pads the left text so that the right text ends at the row's last cell, no `RIGHT_PAD`):

- E1: Plan `ship`, last row of its Context, counters `2⋅2⋅5`, cost 0.12, width 44: `row_of("╰─{WORKING} ship $0.120", "2⋅2⋅5", 44)`.
- E2: Plan `ship`, counter `1`, cost 1234.0, width 30: `row_of("╰─{IDLE} ship $1.23k", "1", 30)`.
- E3: Plan `ship`, no counter, cost 0.004, width 30: `╰─{IDLE} ship $0.004` (nothing after it); with cost 0.12: `╰─{IDLE} ship $0.120`.
- E4: Context `alpha`, cost 12.30, width 30: `╭{IDLE} alpha $12.30` (nothing after it).
- E5 (cut name): Plan named `abcdefghijklmnopqrstuvwx`, counters `2⋅2⋅5`, cost 0.12, width 30: the name keeps 30 - 6 - 7 - 6 = 11 cells (k=6), `abcdefghij…`; `row_of("╰─{IDLE} abcdefghij… $0.120", "2⋅2⋅5", 30)` (3 blank cells before the counters).
- E6 (cut id): Context whose id is 30 `a`, cost 12.30 (k=6), width 30: the id keeps 30 - 4 - 6 = 20 cells, 19 `a` and `…`; the row is `╭{IDLE} ` + 19 `a` + `… $12.30` and fills the 30 cells exactly.

**Narrow rows.** The counters are drawn when `c + 5 <= width` (`MIN_LEFT`), as before. The cost is drawn only when the Plan has no counters or its counters are drawn and the cut name keeps at least one cell: `width - 6 - (1 + k) - (c + 1 if it has counters) >= 1`; otherwise the cost is dropped and the name is cut as if it had no cost. The cost goes first and never makes the counters disappear. A Context row draws its cost when `k + 5 <= width` (the id keeps at least one cell), else not. E7: Plan `ship`, counter `1` (c=1), cost 0.12 (k=6, `$0.120`): at width 16 the name keeps 16 - 6 - 7 - 2 = 1 cell, `…` and the row is `row_of("╰─{IDLE} … $0.120", "1", 16)`; at width 15 the cost is dropped and the row is `row_of("╰─{IDLE} ship", "1", 15)`; the counters alone are drawn from width 6 (the row ends with ` 1`, no `$`) and neither is drawn at width 5. E8: a Plan `ship` with a cost and no counters draws the cost at width 14 (the name keeps 14 - 6 - 7 = 1 cell, `…`) and not at width 13; a Context with cost 0.12 draws it at width 11 (the id keeps 11 - 4 - 6 = 1 cell, `…`) and not at width 10.

**Muted rows.** An archived row keeps its cost in colour 8, as its other marks. An asleep Plan row keeps its cost in colour 8: the muted restyle skips the cost cells as it skips the counters. The selected-name blue is the name's alone and the cost is never blue; the cursor's reversed block covers the whole row. A Context row's cost is never bold, though its title is.

**Rows without a cost.** Fork, Manager, Worker and toggle rows draw no cost, though each Session node carries its own cost (`SessionNode.cost_usd`, the read model's data, not a row element). A tree whose every cost is `None` draws what it drew before, except that its rows now end at column `W - 2` (pro-d186).
- **Cursor.** Keyboard only: a click never moves or shows it. It moves between Sessions only (Context rows are not selectable) and, after the last Session row, on into the Inbox section's item rows (below; the header, the cue lines, the up-to-date row and the blank rows are skipped by the cursor and the cursor goes back up from the first item row to the last Session; there is no toggle row, pro-d147), is drawn as the blue block (colour 4 inverted: text in the terminal's background colour, contract §6.1.1) across the full row only while the pane has keyboard focus (`focus-events`, §6.1) and after an arrow key; Enter, `n` and `x` act on it only while it shows.
- **Inbox section** (hs-d165, hs-d167, hs-d168, hs-d298, hs-d337, hs-d339, hs-d345 to hs-d349, pro-d147, pro-d177; contract §2.2 "Inbox item", commands §21). The right column (the `widgets` view, below) shows, directly under the machine lines, the Inbox, the **single source of "needs you"**: the open, visible items (not snoozed, not dismissed) of **every** Context, in the order of `cpd inbox list` (red, yellow, magenta, blue, cyan, white; oldest first; `engine-down` first, when no live Manager exists). Its section header is the **flush form** of §17 (`flush_title`, pro-d147, which supersedes pro-d141 and the collapsible, rail-and-toggle Inbox of hs-d337 and hs-d476) and the Inbox is **flat and bottom-aligned**: no `│` rail, no closing row, no fold state, no 3-item limit; an item is one list row or two, by its content. **Rows**, top to bottom, of a section of `height` rows: row 0 blank (spacing above the header); row 1 the header; row 2 the ▲ cue line; rows 3 to `height - 2` the list, as many rows as the items' heights add up to (1 or 2 each, below), or with no item one muted row ` up to date, drink some water` (column 0 blank, the text from column 1); the last row, the pane's last row, the ▼ cue line. **The header** spans the full pane width from column 0: `───` (three muted `─`, columns 0 to 2), the title pill `Inbox` (caps U+E0B6 and U+E0B4, the static grey pill `palette::pill_static()`, its left cap at column 3), a muted `─` fill, the count pill `<n>` (`n` the count of visible open items; the same grey pill) and one muted `─` at column `W - 1`; no padding cell on either side. At `W` = 30 with 2 items it is 3 + 7 (the title pill) + 16 `─` + 3 (the count pill) + 1 = 30 cells. The header's column 0 `─` is the first cell of the pane, next to tmux's `┃` border cell; tmux draws its `┃` in the middle of its own cell, so half a cell lies between the vertical line and the header and the two do not join (pro-d157; docs/research/inbox-pane.md). **An item row** has one blank column on each side: it is drawn at columns 1 to `W - 2` (width `W - 2`) as ` <glyph> <label> … <age> `: the tier glyph at column 1 (the glyph in its tier's colour, bright, the row is clickable; §6.1.1; its tier glyph, or for three kinds its own, below), a space, the label cut with `…` so that it never reaches the age, the age right-aligned and ending at column `W - 2` (`42s`, `5m`, `3h`, `2d`: the largest whole unit of `age_seconds`), columns 0 and `W - 1` blank; for example ` \u{F12F4} cc-api-orc-apollo#2309 merg… 5h` over `   Add retries to the payment client` (two rows, below) and ` \u{F0362} cpd#129 merge conflict       18m` (a one-row item; the escape stands for the glyph, written here as its code point; the first glyph is yellow, the second red). An `unread` item is ` \u{F0366} cpd replied 2m` (white glyph) over `   Variable-height Inbox rows` (its Row 2, the Plan's title); a one-row item stays one row, with no blank row under it. **An item is one row or two** (pro-d171, amended by pro-d177; contract §2.2 "Inbox item"): its height is 1 when its label has no Row 2 (no `\n`, or nothing after it) and 2 otherwise (one function, `item_height` in `tui/src/widgets/inbox.rs`). **Row 1** is the row above, unchanged: the first line of the stored `label`. **Row 2**, when it has one, is its second line, in muted colour 8 (`palette::muted_text()`): column 0 blank, columns 1 and 2 the `LEAD_WIDTH` (2) spaces, the subtitle from column 3 (under the label), cut with `…` so that it ends at most at column `W - 2`; an item with no Row 2 takes one row and no blank row follows it. The state dots and spinner glyphs stay reserved for the Context tree (hs-d483). The Inbox's order is that of `cpd inbox list`. **Hits:** the header row is `Hit::Header` (no action); both rows of a two-row item and the only row of a one-row item are `Hit::Item(i)` (a left click jumps to its cause, a right click opens its menu) and the `Link` widget covers Row 1's label only; the spare blank row of a capped list, the cue lines and the up-to-date row are no hit; there is no toggle. **The tier glyphs** (hs-d483; User, 2026-10-20; decided work, hs-d301). The urgency levels are named by their colours and rise in urgency white < cyan < blue < magenta < yellow < red (hs-d570 added the notice tier between white and magenta as cyan; pro-d16 names it cyan: blue means focus and selection only, amended by pro-d58 for the blue Inbox tier for external support, `review-requested`, between cyan and magenta). The glyph comes from the item's **urgency** (its tier), never from its kind and is drawn in the urgency's existing colour, **with three exceptions by kind** (pro-d98, pro-d100; User, 2026-10-27; an exception to hs-d483 for these three kinds only): `deploy` draws U+F15A9 (`\u{F15A9}`, yellow tier), `pr-ready` draws U+F12F4 (`\u{F12F4}`, yellow tier; it was U+F069A) and `review-requested` draws U+F12F2 (`\u{F12F2}`, blue tier; it was U+F0368); each is coloured by the item's tier colour as before (`palette::color_clickable(...)`, `palette::tier_span`). Every other kind keeps its tier glyph. The six tier glyphs are named once, in `tui/src/palette.rs` beside the urgency colours:

| Urgency (colour) | Glyph: code point and escape | Meaning |
|---|---|---|
| white | U+F0366 (`\u{F0366}`) | an unread reply |
| cyan | U+F06F1 (`\u{F06F1}`) (hs-d570, pro-d16) | a cpd notice: curation, nothing blocks; also a change in the models a provider offers (hs-d570) |
| blue | U+F0368 (`\u{F0368}`) (pro-d58) | external support (the blue tier's glyph; its only kind today, `review-requested`, draws its own, U+F12F2) |
| magenta | U+F173A (`\u{F173A}`) (hs-d570: back from U+F06F1) | a decision before the work runs (a Plan review, a question while the Plan is still planning) |
| yellow | U+F069A (`\u{F069A}`) | pull-request work (the yellow tier's glyph; its two kinds today, `pr-ready` and `deploy`, draw their own: `deploy` U+F15A9 in yellow; `pr-ready` U+F12F4 is drawn in the colour of its own urgency, red `Meaning::Failed`, yellow `Meaning::Review`, cyan `Meaning::Notice` or white `Meaning::NotStarted`, pro-d130) |
| red | U+F0362 (`\u{F0362}`) | a blocker only the User can clear (a dialog while the Plan runs, `engine-down`, `engine-error`, `pr-closed`) |

The header's lines are colour 8 and its two pills are read-only title pills (colour 7 inverted: text in the terminal's background colour, `palette::pill_static()`, hs-d475, hs-d480, hs-d534); the items are not (their own tier glyphs and colours; the label is 15, the age and the Row 2 subtitle 8, contract §6.1.1); the up-to-date row and the cues are colour 8. **The Inbox's size and position** (pro-d147, pro-d177 for the heights, replacing pro-d130's top placement and the share rule of hs-d307). The `widgets` column is: row 0 blank, row 1 ` <hostname>` (bold), row 2 the CPU/GPU/memory line, then `rest`, the rows below. The Inbox section sits at the **bottom** of `rest`: its first row is `rest.bottom() - height`, so it grows upward as items arrive and the rows between the usage line and the section are blank. `height` = list rows + 4, where list rows = the sum of the items' heights (1 or 2 each, below), or 1 for no item (the up-to-date row), capped at `rest.height`; when capped, list rows = `height - 4` and the items drawn are those from the scroll offset on whose heights add up to at most the list rows, stopping at the first one that does not fit; the rows left (at most 1, when the next item has two rows) stay blank above the ▼ line and an item never splits across them; fewer list rows than the next item's height show no item, only the ▼ cue. The items scroll with the mouse wheel (`WHEEL_ROWS` items a notch) over the section only; the scroll offset counts items, the ▲ and ▼ counts are items and the largest offset is the smallest one from which the heights of the remaining items add up to at most the list rows (every item when none does). Rows that do not fit are cut from the bottom of the section. The offset is view state only (starts at 0, a refresh keeps it, nothing is written) and is clamped by `Window` (§17 "Scroll cues"); the keyboard cursor on an item scrolls the list to keep it visible. **Worked examples** (a 30 x 12 column, machine `homeserver` at 23% CPU and 41% memory; rows 0 to 2 are the blank row, ` homeserver` and the usage line): **0 items**: `rest` is rows 3 to 11 (9 rows); the section is 5 rows, rows 7 to 11: row 7 blank; row 8 the header with count 0; row 9 blank (the ▲ line); row 10 ` up to date, drink some water`; row 11 blank (the ▼ line); rows 3 to 6 are blank. **2 items**, each with a Row 2: the section is 8 rows, rows 4 to 11: row 4 blank; row 5 the header (count 2); row 6 blank (▲); rows 7 and 8 item 1 (Row 1, Row 2); rows 9 and 10 item 2; row 11 blank (▼). **12 items**, each with a Row 2: the section is capped at the 9 rows of `rest`, rows 3 to 11, with 5 list rows: 2 items fit and 1 list row is blank. Scrolled to 0: row 3 blank; row 4 the header (count 12); row 5 blank (▲); rows 6 to 9 items 1 and 2; row 10 blank; row 11 `▼ 10 more` (ends at column 28). Scrolled to 3: row 5 `▲ 3 more` (ends at column 28); rows 6 to 9 items 4 and 5; row 10 blank; row 11 `▼ 7 more`. Scrolled to the maximum (10): row 5 `▲ 10 more`; rows 6 to 9 items 11 and 12; row 10 blank; row 11 blank (no ▼). **3 items** of heights 2, 1, 2 (5 list rows, the section is 9 rows, rows 3 to 11): row 3 blank; row 4 the header (count 3); row 5 blank (▲); rows 6 and 7 item 1 (Row 1, Row 2); row 8 item 2 (one row, nothing under it); rows 9 and 10 item 3; row 11 blank (▼). **6 items** of heights 1, 2, 2, 1, 1, 1 (8 list rows, capped at the 5 list rows of `rest`): scrolled to 0: row 5 blank (▲); row 6 item 1; rows 7 and 8 item 2; rows 9 and 10 item 3; row 11 `▼ 3 more`. Scrolled to 1: row 5 `▲ 1 more`; rows 6 and 7 item 2; rows 8 and 9 item 3; row 10 item 4; row 11 `▼ 2 more`. Scrolled to the maximum (2): row 5 `▲ 2 more`; rows 6 and 7 item 3; row 8 item 4; row 9 item 5; row 10 item 6; row 11 blank (no ▼). **5 items** of heights 1, 1, 2, 2, 2 (7 list rows, capped at 5): scrolled to 0: row 6 item 1; row 7 item 2; rows 8 and 9 item 3; row 10 blank (item 4 has two rows and one row is left); row 11 `▼ 2 more`. **The tree's cue lines and layout.** The tree (the left pane; its header is the tmux pane title) is laid out inside the pane's one blank column on each side (width `W - 2`, from column 1): row 0, the blank row under the title, is the ▲ line; the list starts at row 1; the pane's last row, or the row above the error footer when the snapshot has an error, is the ▼ line; the error footer stays below the ▼ line. A pane with fewer than 3 rows beyond the footer has no ▼ line: the list takes the rows after row 0. `no Contexts` is the list's first row. The tree scrolls inside its list rows: the wheel over it, 3 rows a notch and the keyboard cursor (a Session row moving out of view scrolls it back; the wheel does not move the cursor), with the cue rule of §17's "Scroll cues" (hs-d348, pro-d147). Examples: a 43 x 12 pane with 11 tree rows: row 0 blank (the ▲ line); rows 1 to 10 the first 10 tree rows; row 11 `▼ 1 more` (ends at column 41). A 43 x 31 pane with 61 tree rows (a Context and 60 Plans), scrolled by one wheel notch (3 rows): row 0 `▲ 3 more`, rows 1 to 29 the 4th to the 32nd tree row, row 30 `▼ 29 more`; scrolled to the end (32 rows hidden above): row 0 `▲ 32 more`, rows 1 to 29 the last 29 tree rows, row 30 blank. **The empty Inbox** (hs-d487, amended by pro-d147): with no open, visible item the list is exactly one muted row ` up to date, drink some water` (colour 8; column 0 blank, the text from column 1), between the ▲ and ▼ cue lines, under the header with the count pill at 0 (the 0-items example above). The row is no hit: a click on it does nothing.
- **Clicks on an Inbox item** jump to its cause (the cockpit never decides; each jump runs a command or is a link):

  | Kind | A left click on the row |
  |---|---|
  | `dialog`, `unread` | `session open <the item's Session>`, then `select-pane` on the chat slot: as a click on that Session's tree row (an archived Session: `session history`, as there) |
  | `plan-review` | a `Link` widget: the label is the Link's text, the cue `󱆭` follows and a plain left click on them opens the item's `url` when present; with no `url` the label draws with no link and a plain click jumps to the Lead instead (no url, no link drawn, the row's jump rule applies). An item with no `url` still jumps; if that jump fails the failure is a `text` dialog (§17 "No notes") |  
  | `loop-guard` | `cpd session resume <the item's Session>` in the background with no dialog (hs-d552, hs-d571: resumes a Lead or Manager blocked by the loop guard; L9); a refusal shows the failure dialog |
  | `drafts-parked` (hs-d731, hs-d733) | jumps to the Plan's Lead Session, as `plan-review` with no `url` does: no link is drawn and no command runs; the contract's rule (§2.2) gives the item its label `<plan>: <N> parked drafts` |
  | `pr-failing`, `pr-changes` (history rows only: hs-d470 dropped the kind, E8 opens none; no check or review state opens an Inbox item, hs-d470) | nothing; no jump, no click action |
  | `pr-ready` (the User's open authored pull request, in every state; the row's colour is the state, contract §2.2 "The authored pull request rule") | the glyph U+F12F4 is drawn in the item's urgency colour. The label is a `Link` widget (the label is the Link's text, the cue `󱆭` follows): a plain left click on it opens the item's `url` (the GitHub pull request page). A click on the rest of the row runs `session open <lead>`, then `select-pane` on the chat slot when a Plan owns the pull request (its Lead Session) and does nothing when none does. While that pull request's merge is running (the merge marker, below, hs-d495) the glyph cell is the yellow spinner and a click does nothing |
  | `pr-closed` (Plan closed-unmerged; contract §2.2 "Inbox item") | no link is drawn; a left click on the row runs `session open <the item's Lead Session>`, then `select-pane` on the chat slot, as a click on the rest of a `pr-ready` row does (Go to Lead); with no Lead it does nothing |
  | `review-requested` | a `Link` widget: the label is the Link's text, the cue `󱆭` follows and a plain left click on them opens the item's `url` (the GitHub pull request page); with no `url` the label draws with no link and a plain click does nothing |
  | `deploy` (pro-d100, A8) | a `Link` widget, as the link kinds: a plain left click on the label opens the item's `url`, the workflow run's page, in the default browser. Its right-click menu starts with **Approve**, which runs `cpd inbox approve <id>` at once (§21.13; the menu below) |
  | `engine-down` (shown only when no live Manager exists, below) | `cpd watch --ensure` (§12; idempotent) and says nothing (the result goes to the log only, never stderr; a failure is a `text` dialog); no popup (the StatusBar's `󰧭` button still opens the logs) |
  | `engine-error` (history rows only: hs-d314 dropped the kind, E8 opens none; the engine's errors are recorded in Events, counted in the Manager's turn-start digest, hs-d514, contract §3.7) | nothing: no jump, no popup |
  | `job-stuck` (history rows only: hs-d318 dropped the kind, E8 opens none; a stuck Job goes to the Context's Manager as a message, contract §3.7 "A stuck Job") | nothing: no jump, no popup |
  | `curate-retire` (hs-d549, hs-d551; hs-d568, hs-d569 CD4; hs-d592) | Exists only for a curable Context (R6 of hs-d592). Runs `cpd context curation --context <the item's Context> --limit=10` (§3.15; hs-d569 CD4: cockpit asks for 10) and opens the `checklist` dialog (§13.2) with title `Retire records of <context>` (with suffix ` (<shown> of <total>)` when total > 10, hs-d569 CD4) and one entry per row of `retire` (hs-d569 CD2: full `text`, header line `[ ] <id> — <reason> <target>`); on **Approve** runs `cpd context retire --context <context> --approve <ticked ids> --reject <unticked ids>` (§3.16), deciding only the shown entries; on **[Later]** (hs-d569 CD3: the button says it) or Esc/q runs nothing, every entry stays pending for the next opening. With nothing pending the dialog is not opened and nothing runs. A refusal of either command is a `text` dialog (§17 "No notes"). The click decides nothing by itself; dismissing the item (`d`) decides nothing either |
  | `curate-docs` (hs-d549, hs-d551; hs-d568, hs-d569 CD4; hs-d592) | Exists only for a curable Context (R6 of hs-d592). As `curate-retire`, with the entries of `docs` (hs-d569 CD2: full `text`, header line `[ ] <id> — <paths>`; `detail` the paths joined by `, `), title `Documentation candidates of <context>` (with suffix ` (<shown> of <total>)` when total > 10, hs-d569 CD4) and on **Approve** `cpd context docs --context <context> --send <ticked ids> --dismiss <unticked ids>` (§3.17: the ticked go to the Manager as one board post, the unticked are dismissed for good), deciding only the shown entries; on **[Later]** (hs-d569 CD3) or Esc/q nothing runs |
  | `models-changed` (pro-d13, pro-d16) | opens a read-only `text` dialog (§17, the dialog kit) titled `Models: <provider>` with the announced ids of the item's `changes` read from `run/models-<provider>.json` (the provider and `changes.at` are in the item's subject `models:<provider>/<changes.at>`; the announced part and its filter are contract §2.2 "The models-changed rule"): a heading `New (<N>)` and one id per line, then a heading `Gone (<M>)` and one id per line, a part with no ids omitted. No command runs and nothing is decided; Esc/q/Enter closes it. A file that no longer has that `changes` (a newer change replaced it) shows `changed again; the item closes at the next tick` in the dialog. The kind has no `url`, no OSC 8 link and no target, so the simpler rule of the other kinds (copy the `url`, or nothing) is not used: the ids are the point |
  | `question` (history rows only: hs-d306 (1) dropped the kind, E8 opens none) | `session open` of the Context's Manager Session (`cpd context manager --context <c>`, then `session open <manager>`, as a click on a Context row) |

  The cockpit writes no OSC 8 escape sequences in the Inbox rows. The four link kinds (`plan-review`, `pr-ready`, `review-requested`, `deploy`) draw `<label> 󱆭` as a `Link` widget; a plain left click on any part of the output opens the url in the default browser (macOS `open` only, through the `Runner` port, §13.2 Links). For `plan-review` the url is always null (§ D and G of decision hs-d738); for `pr-ready` and `review-requested` the url is the pull request's GitHub page; for `deploy` it is the workflow run's page (`https://github.com/<owner>/<repo>/actions/runs/<id>`, GitHub's `html_url`). An item with no `url` draws no link and a plain click acts according to its kind (no link, no open action).
- **A stopped engine restarts itself** (hs-d328, hs-d363, replacing hs-d318's restart by the tree; contract §3.7 "Who restarts a stopped engine"). **The supervisor restarts it** (contract §6.6: the `pane-died` and `window-unlinked` hooks, a failed start retried and logged to `run/watch.log`); **the tree runs no `cpd watch --ensure` by itself**, because the supervisor covers every case a tree running in the same tmux server could see and the tree's own restart raced it (it found the engine already running and left `engine was down: … found it running` on screen). A snapshot (§17) that reads the engine running, or a restart that `found it running`, shows nothing. The tree only **speaks** and only when the engine **stays down past the supervisor's window**: when `locks/watch.lock` has been free on **every** snapshot for **60 s** (view state: the time of the first free read, forgotten at the first held one), the view sends, once per downtime, to **the one Manager whose Session state changed most recently** (hs-d514: the live Manager that was last active; a Session with role `manager`, not archived; `session send <manager> --wake`), `The cpd engine has been down for 60 s and its supervisor has not restarted it; see run/watch.log, start it with cpd watch --ensure.`: the view has no `CPD_SESSION`, so `from` is null (the User's side) and `--wake` wakes the Manager (hs-d514); with no live Manager nothing is sent. It shows no note (hs-d469, replacing hs-d364 (2)'s "short note"): the message to the Manager is the whole of it and the view logs it. **The `engine-down` item shows in the Inbox section only when no live Manager exists** (with one, the message handles it: no red for what an agent handles, hs-d303); its click, the User's own explicit action (`Action::EnsureWatch`), is the one of the table above and its reply goes to the log only, never to stderr (§17: a view never writes to its terminal outside its frame).
- **Clicks.** A click on a live Session opens it in the chat slot and focuses the chat (`session open`, then `select-pane`); a click on an **archived Session** row (a Plan row's Lead or a fork) runs `session history <id>` (§10.12), then focuses the chat; a Context is a collapsible block (§17, "Collapsible blocks"; hs-d337) whose title row is the Context row: a click on the closing toggle row (`╰󰬦 expand` / `╰󰬬 collapse`) shows or hides its archived rows and a click on the title row, elsewhere on a live Context row, runs `cpd context manager --context <c> --by-user` (hs-d542 A1), then `session open <manager>`, then `select-pane` on the chat slot (B3-A6). For a by-user Session the Manager starts with no first message and waits for the User's first prompt to deliver it (hs-d540, hs-d542 A2, §15.7a). Clicks are ignored while a dialog popup is open.
- **Folds** (User, hs-d260). The folds survive a restart of the view: which Contexts have their archived rows hidden and which archived Contexts are expanded (the toggle shows and hides only archived rows, hs-d344; live Contexts start expanded and archived ones folded, hs-d339; the cursor and the scroll position are not kept). They are kept in `$CPD_HOME/run/tui/tree-folds.json` (contract §1.4), a JSON object `{"collapsed": ["<context id>", …], "expanded": ["<context id>", …], "archived": true}`: a Context in `collapsed` has its archived rows hidden (its closing row reads `╰󰬦 expand`) and one in `expanded` has them shown (`╰󰬬 collapse`); the `archived` key is the archive row's fold (absent or false = folded, true = expanded), starts folded; a live Context's own rows always show and a Context with no archived rows has no toggle. It is under the same exception as the Plan view's offsets file (above): the view's own state, not Ledger state, written and failing as that file is. **Read** once, at the view's start: a Context in `collapsed` has its archived rows hidden and one in `expanded` has them shown, both counting as known; the `archived` key toggles the archive row; ids that match no Context are harmless and a missing key is treated as false. A missing file is today's defaults. A file that cannot be read or is not that object is not an error: the view starts with the defaults, logs `fold state reset: <reason>` (no note, §17 "No notes") and replaces the file at its next write. **Written** atomically (temporary file, rename, parents created) whenever the folds have changed, at most once per second; only Contexts that exist and differ from their start are written, so unknown ids are dropped at the next write. A failed write is logged (`fold state not saved: …`) and retried; it never ends the view.
- **`n`** asks for the new Plan's id in an **`input`** popup (the dialog kit above; validation `plan-id`; its question and texts are the former inline prompt's, `Fork <label> into a new Plan, name:`; Enter starts, Esc cancels, an empty or invalid id is refused inside the popup) and runs `plan new` (§4.1) in the selected Session's Context: the Plan forks the Context's grounded Lead (R15), the one deviation from cpd 1's `n`, which forked the selected session. On a Context row it says `pick the session to fork`.
- **`x`** closes the selected Session's pane after a **`confirm`** popup (the dialog kit above) asking `Close <label>? pi stops; the session file stays`, preceded by `<label> is working.` when its state is `working`, with `[Close]` and `[Cancel]`; only Enter, `y` or a click on `[Close]` closes. It runs `cpd session close <id>` (§10.9). The Session in the chat slot is refused with `switch to another session first`, also when it moved there while the popup was open. On a Context row it does nothing. `<label>` is the Session's name.
- **`s` and `d`** act on the Inbox item under the cursor (they do nothing on a Session row): `d` runs `inbox dismiss <id>` at once; `s` opens the snooze choices, the menu's three `Snooze …` entries (↑↓ and Enter pick one, Esc closes), then runs `inbox snooze <id> <15m|1h|tomorrow>`. Enter on an item row acts as a plain click does (above). A refusal (`item_closed`: the item closed meanwhile) is a `text` dialog with the failure (§17 "No notes"). The item leaves the section when the Inbox-* Event reaches the view (§17).
- **`$`** toggles the cost counters of the Context and Plan rows (pro-d189, pro-d196; see Cost counters). It needs no cursor (like `?`), never shows or hides the cursor, works while the pane is not focused and an open menu takes it like every key.
- **`?`** opens the legend popup. The view has no other key that opens the menu or acts on it; what a key used to do is still a command.
- **Labels** (contract §2.2: ids are unique, immutable and need not be readable; the title is the visible label). A Context row shows the Context's **id**, a Plan Lead row the Plan's **id** and a fork row (a side Session) `<plan id>·<first 4 characters of its Session id>` (for example `cross-repo·7f3a`); a fork with no Plan keeps the fallback, the first 8 characters of its Session id. Archived rows follow the same rule. Titles and names no longer show in the tree (User, hs-d101; this supersedes the titles rule, hs-d63).
- **Archive folder row** (hs-d698, hs-d707). When at least one Context is archived, a single clickable row appears after the last live Context, one row, `󰬦 archive` folded (showing no archived Contexts) or `󰬬 archive` expanded (showing them). The row is colour 8 on every mark (branch, glyph, text), a hit target that toggles the archive row's fold (same as the two closing toggle buttons, but the whole row is the target); its fold is persisted in `tree-folds.json` under the `archived` key. Expanded, the archive row shows every archived Context and Plan and every archived Manager Session, as muted blocks with the same structure as live ones (each Context's archived Plans and Sessions inside the collapsible block that usually holds the live rows, below them and then the Context's archived Manager rows); they are muted (colour 8 on every mark, hs-d534), they have no indent, they render exactly as today but inside the archive row with no separate rows around them and a click on an archived Session row runs `session history <id>` (above). The archive row is folded by default and appears only when it has archived Contexts to show; old files without the `archived` key still load. Its fold is view state only: no command, no write. Every row of an archived object carries `archived_at` from the read model (§17).
- **Right-click menu** (**every item is a `cpd` command**: the tree only runs commands). A right click on an Inbox item row opens the menu with its four entries (table below); a right click elsewhere opens the menu of its row. A right click opens a menu, drawn by the view (ratatui) over the tree at the click and clamped inside the pane. ↑↓ and Enter pick an item, Esc closes it; a click picks an item; a click outside closes it. There is **no key** that opens the menu: `n`, `x` and `?` stay as above. The tree has **no inline prompt row** (hs-d295, hs-d297): the `…` items (**New Context…**, **New Plan…**, **New session…**, **Rename…**, **Fork…**) type their text in an **`input`** popup of the dialog kit, with the questions, starting values and texts of the former prompt kept verbatim (`Title: ` for a Context's or Plan's title, `Name: ` for a Session's name); **Rename…** starts with the current text; validation is `title` for Contexts and Plans, `name` for Sessions. **Close** asks in a **`confirm`** popup first, as `x` does. **Archive** asks in a **`confirm`** popup, `Archive <label>?` with `[Archive]` and `[Cancel]` (hs-d297; this supersedes hs-d90's "Archive runs at once, with no `y/N`"; the rest of hs-d90 stands). The commands each item runs and the opening of the new Lead below, are unchanged. The items by row:

  | Row | Items | Command each runs |
  |---|---|---|
  | Inbox item | Snooze 15 min, Snooze 1 h, Snooze until tomorrow 09:00, Dismiss | `inbox snooze <id> 15m`, `inbox snooze <id> 1h`, `inbox snooze <id> tomorrow`, `inbox dismiss <id>` (§21.3, §21.4) |
  | Context | New Plan…, Rename…, Copy id, Archive, Delete | `plan new` (§4.1), `context rename` (§3.7), `context archive` (§3.8), `context delete` (§3.14) |
  | Plan | New session…, Rename…, Copy id, Archive, Delete | `session fork <the Plan's Lead>` (§10.3), `plan rename` (§4.9), `plan archive` (§4.10), `plan delete` (§4.14) |
  | Fork (side Session) | Open, Rename…, Fork…, Copy id, Close, Archive | `session open` (§10.4), `session rename` (§10.7, `--session <id>`), `session fork` (§10.3), `session close` (§10.9), `session archive` (§10.10) |
  | empty tree area | New Context… | `context new` (§3.1) |
  | any archived row | Rename…, Copy id, Unarchive and Delete on a Plan or Context row (hs-d527; also on a live Plan row whose Lead is asleep and a live Context row whose Plans are all asleep or archived, which then have Delete last) | `context rename` (§3.7), `plan rename` (§4.9) or `session rename` (§10.7), `Copy id` (no cpd command; below) and `context unarchive` (§3.9), `plan unarchive` (§4.11) or `session unarchive` (§10.11); the rename commands accept archived objects (F4) |

  **Copy id** (hs-d102) is offered on every row that has an id: Context, Plan, fork and archived rows. It copies the row's shown id (see Labels) to the User's clipboard with `tmux -L <socket> set-buffer -w -- <id>`; tmux sends it to the outer terminal by OSC 52, which works over ssh when the terminal allows it (tmux's default `terminal-features` give `xterm*` the `clipboard` feature). Whether it reaches the User's Ghostty on the Mac is **unverified** until the User tries it. It says nothing. It is the one item that runs no `cpd` command: it changes no Ledger state.

  **Opening the new Lead** (hs-d103). After **New Plan…**, **Fork…** or **New session…** succeeds, the tree runs `session open <the new Session id>` (from the command's JSON: `plan new`'s `lead.session`, `session fork`'s Session id), then focuses the chat slot, as Open does. The commands themselves never move the chat slot.

  **Archive of the chat slot's Lead** (F3, hs-d90). Archive on a Session row (a Plan row's Lead, or a fork) whose Lead holds the chat slot, or a Context row whose Manager holds the chat slot, cannot go straight to the archive command, which refuses with `in_chat_slot` (§10.10, unchanged). The tree first frees the slot, then archives: it opens the next live Session in tree order, excluding every Session the archive cascade archives (`session open`, then focuses the chat); if none follows, the previous one; if there is none at all, it runs `session park`, so the placeholder holds the slot. Then it runs the archive command. Only the tree does this two-step; the CLI keeps its refusal.

  A refusal (`archive_blocked`, `in_chat_slot`, `archived`, …) is a `text` dialog with the failure (§17 "No notes").
- **New Plan… and New Context…** ask a title in an `input` popup (validation `title`); the tree derives the id. The id is a lowercase ASCII slug of the title: each run of other characters becomes `-`, leading non-letters are dropped and it is cut to fit the id pattern (Plan `^[a-z][a-z0-9-]{1,23}$`, Context: contract §2.1); `-2`, `-3`, … are appended while the id is taken in the tree's snapshot. New Plan… runs `plan new <id> --context=<c> --title=<title> --by-user` (hs-d542 A1): no repo, a Plan has none (R34, A12; its PRs are added with `pr add`). For a by-user Session, pi starts with no first message and waits for the User's first prompt to deliver it (hs-d540, hs-d542 A2, §15.7a). **New Context… is ONE dialog** (pro-d114, superseding hs-d297): a `form` dialog (§13.2, the dialog kit) titled `New Context`, with title input (label `Title:`) and owner selection (label `Owner:`, options the policy files' stems sorted, the first selected at start, the owner list as the tree derives it). Enter or [OK] confirms (only when the title passes validation) and runs `context new <id> --title=<title> --owner=<chosen>`; Esc or [Cancel] cancels and nothing runs. The cockpit takes no "Another owner" choice (pro-d114); the login box and confirm are gone. With no policy file the refusal is a `text` dialog `No owner policy: add $CPD_HOME/policy/<owner>/owner.toml`, running nothing. A title no id can be made of is refused in a `text` dialog after [OK], nothing runs.
- **Right button.** The view relies on tmux 3.4's default root `MouseDown3Pane` binding: believed to pass the click to a pane whose app captures the mouse (`mouse_any_flag`) and to show tmux's own menu otherwise (**unverified**). cpd adds no binding for it (§6.1, §13.1 unchanged).

**`plan`.**
- **No top row, no tabs** (User, hs-d102). The PR boxes start at the top of the pane, inside the margin below; the view draws no Context id, no tab and no separator line. Switching Leads is the tree's (Enter or a click). With no Lead in the chat slot, the message row reads ` no Lead session in the centre` (colour 8). With a Manager in the chat slot, the pane shows Pulse (one row per Plan of the Context), not the Plan view. With the placeholder or a history pane in the slot, the message shows.
- **Chart:** the Plan's Jobs in a flowchart of boxes (reference `flow.rs`, ported), laid out when a snapshot arrives or the pane width changes, and at no other time; at a frame change only spinner glyph cells change, never the layout. Each PR box is a `BoxWidget<Full>` (the box widget, `tui/src/box_widget.rs`), a full box with rounded corners (`Borders::ROUNDED`, hs-d474), whose body is the flow chart of that PR's Jobs; the box's title row is drawn by the widget and reads ` ╭─ <key> · <repo name> 󱦰 <base> · <pull request> ` (one `─` after the corner, then the key, B5), and its body contains the chart; the chart has no padding of its own (the widget's one column of padding is the only one). Inside a blank margin of 1 column and 1 row (`CHART_MARGIN`; the Box rule, contract §6.1.1 "Boxes": one blank column each side, one blank row above the first box and below the last, the canvas) that belongs to the chart's canvas, not to the scroll window (which is the whole body, so scrolled content reaches the pane's edges; at scroll (0, 0) nothing touches them; this is the one place where the pane clips a box); the layout test asserts the margin of the container at scroll (0, 0) only and the walls of the boxes the pane shows whole. A PR with no Jobs shows an empty box (` no Jobs `, colour 8). Cancelled Jobs shown as a colour 8 pill (hs-d518, hs-d534). Pulse's `MARGIN` (2 columns, the **Row layout** and **Buckets** bullets) is unchanged, as are the chart's message row's: Pulse rows and that row are not boxes. Each Job is a pill: `` (U+E0B6), the compact body `<icon> <key>[│#<n>]│<tier>` (hs-d474: no padding inside the caps and none before the `│`; pro-d62: `#<n>` is the Job's Attempt count n, shown only from the second Attempt on, between the key and the tier, so the tier is always the last cell of the pill; e.g. `󰗠 titles-docs-2│S3` and, from the second Attempt, `󰗠 titles-docs-2│#2│S3`; the label is `#<n>`, while Attempt ids in commands, logs and the store stay `aN`; the keys of a column are padded so the right edges align). The `<tier>` is the Tier a Job is shown at, derived by contract §3.4a: the Tier of its active Attempt, or its `retry_tier` while queued at that Tier after a `job retry`, or the Tier of its latest Attempt, or its stored `tier`. After a `job retry --tier <T>`, a Job shows `<T>` until an Attempt starts at that Tier. The `│` (U+2502) in the pill's style (User, Plan tui-visuals), then `` (U+E0B4). The two caps are one cell each, so the pill is two cells wider than its text, and the layout's widths follow from the text; they are drawn as a **coloured pill** (hs-d534, contract §6.1.1: the status colour's base colour as the background (a pill that is not clickable takes the base index, contract §6.1.1 rule (3); pro-d63), text in the terminal's background colour (inverted), the icon included, caps the same colour as `fg`: draft 5, queued 7, running 6, landing 3, merged 2, failed 1, cancelled 8; from `palette::pill(…)`) on the normal background, so the pill has rounded ends (hs-d266; mockup: `✓ header-reverse│S2`, the `✓` standing for the status icon). A `running` Job's icon spins `` (the 28 moon_alt phases, one frame every 50 ms on the shared clock, as listed in §17 section 17 "The tree's dot rule"); every other status, landing `󱑤` included, stays static; the `queued` icon stays `󰝦` U+F0766 (nf-md-circle_outline, hs-d235; the Lead's assumption in hs-d579: it is not a dot of the tree). A `cancelled` Job's pill is colour 8; colours and icons are unchanged (User, hs-d567). A pill's colour shows the status again (hs-d534, which replaces hs-d530's single interactive colour; the icon shows it too) and the Job pills take the base colours and the tree's counters the bright pairs (they sit in clickable rows), the tree's failed counter staying always red (hs-d580, §13.2). A `failed` Job's pill is **red in every case** (hs-d586 refined by pro-d88): with the static glyph U+F0159 while the Job is not being worked on, and the moon loader (the 28 frames of §17 section 17, one frame per 50 ms on the shared clock `palette::frame`) while it is, each failed pill by its own Job (hs-d586 refined by pro-d88). A `cancelled` Job's pill is colour 8 with the static U+F0159. A Job pill is read-only (pro-d63): a click on it and every key do nothing, no pill is selected and no pane opens. **Job connector lines** (R1, R5): a Job's connector line leaves from the right cap of its own pill (the cell right after the pill's last cell, where the left arm of a `─` or `┤` points) and enters the Job it feeds at the cell just left of that pill's left cap (where the right arm of a `─` or `├` points), with an arm pointing toward the cap. Pill widths do not change: a `cancelled` Job's pill is as wide as itself and never widens a live pill; a live pill is as wide as the widest live pill of its column. A card of several stacked Jobs with the same needs is held by a bracket on each side: the left bracket (one cell left of the pills' left caps) and the right bracket (one cell right of the pills' right caps) touch every pill of the card. **Every connector cell touches what it joins** (R5): in the drawn view, scrolled or not, every connector cell's arms lead to another connector cell, to a pill's cap (Job lines) or to a `┬` / `┴` on a box border; no line ends in a blank cell, in the title's text or on a plain `─`. The glyphs and their arms: `─` left,right; `│` up,down; `╭` down,right; `╮` down,left; `╰` up,right; `╯` up,left; `├` up,down,right; `┤` up,down,left; `┬` left,right,down; `┴` left,right,up; `┼` all four. Messages are one row at the bottom of the pane: `no Plan yet`, `loading… (first fetch)`, or the note.
- **PR boxes** (R34, A15; contract §2.5). The chart is split into **one box per PR** that is not `cancelled`, each a bordered block (`Borders::ALL`, colour 8, with rounded corners `╭ ╮ ╰ ╯` instead of `┌ ┐ └ ┘`, hs-d474; every other line and junction, `─`, `│`, the `┬` and `┴` where the needs links join a box, stays) holding the cards and Dependency connectors of that PR's Jobs, laid out inside it as the single chart was. A PR with a base PR or a need of another Plan carries one more line under its title, `on <plan>/<key>` (the base PR, a reference printed bare for this Plan's own) and `needs <plan>/<key>` for each need of another Plan, colour 8; a same-Plan base PR is also an arrow, dashed, and orders the boxes as a need does; a reference to another Plan has no box and no arrow (contract §2.5, hs-d324). The box's title, on its top border, reads ` <key> · <repo name> 󱦰 <base> · <pull request> `: the PR key in bold (colour 7, the title's read-only text; contract §6.1.1); the repo's directory name (the last component of `prs.repo`); its base; then `#<number> <state>` once the PR has a pull request, where `<state>` is `open`, `merged`, or `closed` while the pull request is closed without a merge (`pull_request.state` is `closed`, A11), in the palette's PR state colours (contract §6.1.1, hs-d534; the box title is a hit target, so they are the bright pairs: `open` is 15, and 11 only when the PR is ready to merge, `merged` 10; reference `pr_state`; `closed` is cyan (`Meaning::Waiting`'s colour) while the Lead handles it, and **red only when the Plan's Lead has an open, visible `dialog` item**, it asks the User about it; a closed pull request no Plan owns is red, but no box draws one; audit Q4, hs-d303); `<state>` reads, instead of `open`, when the pull request is `open` and its `mergeable` is `conflicting` (`unknown` is not a conflict; `merged` and `closed` never read either word), **the conflict rule** (hs-d532, written once here; §13.2's PRs column and the legend refer to it): `updating` while the PR has an update Job (`jobs.kind = 'update'`, key `up-<pr>`, hs-d490, hs-d340) that is not ended (`status` `queued` or `running`), in cyan, whether that Job is `running` or `queued` (both are the Lead's agent activity, hs-d534: the two colours of hs-d532 are one); `conflict`, in cyan too (not red: every box is a pull request a Plan owns, so its Lead fixes the conflict, hs-d302, hs-d303), only when no such update Job exists. For example ` api · api-repo 󱦰 main · #12 updating 󱆭 ` and ` api · api-repo 󱦰 main · #12 conflict 󱆭 ` (the state word cyan). The update Job comes from `tui/src/read/flow.rs`'s `Pull`; the colours only from `tui/src/palette.rs`. Else the PR's status (`waiting`, `dispatch`) colour 8. **The Lead's dot** (contract §3.6, "The Lead's Workflow", the display rule; hs-d335, pro-d217, pro-d219, pro-d259): the box of the PR that the Plan's Lead names as its Workflow (`sessions.working_pr`) draws the Lead's working dot while the Lead is `working` and the PR is neither `merged` nor `cancelled`, **whether or not the PR has a pull request**: with one, the dot follows the state word, a space between: `#<number> <state> <dot>`; without one, it follows the status word: `dispatch <dot>`. The dot is the tree's spinner (``, the 28 frames of §17 section 17, one frame per 50 ms on the shared clock, `palette::frame`), in the Lead's Work colour of pro-d200 ("The Lead dot rule", §17), or red when that Workflow has a failed Job or an open Escalation of one of its Attempts (§17). No other box draws the dot or the red loader, and a Plan whose Lead names no Workflow draws none. The view reads the dot from `PrNode::lead_dot` (`tui/src/read/flow.rs`), which is `Some` exactly when the box draws it. Once the PR has a pull request, the title ends with the `Link` widget (Links, below; pro-d60), which draws `#<number> <state>` (and the dot, which is not part of the link) as underlined text, then one space, then the cue `󱆭`: the whole `#<number> <state> <dot> 󱆭` appears as the underlined state and cue, the whole output clickable, and a plain left click opens `pull_requests.url` in the default browser (pro-d60). The PR key, the repo, the base and the spaces around the link are not linked; a `cancelled` PR (hs-d518) shows `cancelled`, and a PR that is `waiting` (display status only, hs-d523) is drawn colour 8 (User, hs-d567). The boxes are laid out **top to bottom** in the order of the needs graph, one row per level: a box sits below every box it needs; boxes of the same level sit side by side in that row, by key (hs-d115). A merged PR's level is 0 with no needs, else one more than the highest level among its needs. A PR that is not merged (any status but `merged`) is at the level max(1 + the highest level among its needs, M + 1), where M is the highest level of any merged PR (no floor when none is merged): **every box that is not merged is drawn below every merged box**, so the work in progress sits together, close below the finished work, instead of sharing a wide row with it (User, 2026-10-18). Each need is a plain box-drawing line from the needed box's bottom border to the needing box's top border: a `┬` on the bottom border, a line with `╭╮` corners, and a `┴` reaching the top border ON the needing box's top border; there is no arrowhead, the direction being implicit (the needed box sits above; hs-d638), and the ┬ exit is unchanged while the bottom-border text does not cover the box's middle column (pro-d175, pro-d179; see the counter and the cost below). The landing `┴` is on the box's middle column, or when the title covers that column, on the first `─` after the title. Inside a box the Jobs still flow left to right, and a box is as wide as the largest of its own chart, its title plus 4 cells and, when it shows one, its bottom-border text (the counter, and the cost after it when there is one) plus 4 cells: `╭`, `─`, the title with its own leading and trailing space, at least one `─`, `╮`. **The active-time counter** (User, Context records pro-d175 and pro-d182, 2026-10-29 EDT; Plan workflow-counter, PR counter; the code and this bullet still call a Workflow a PR): the bottom border of each box reads ` <duration> ` (a space before and after), in `palette::muted_text()` (the border's style), written on the bottom border row from column box.x + 2, right after `╰─`: `╰─ 1h·2m·3s ───────╯`. The duration omits leading zero units and joins them with `·` (U+00B7, one cell wide): under 60 s `<S>s` (`0s`, `42s`); 60 s to 3599 s `<M>m·<S>s` (`1m·0s`, `2m·3s`, `12m·0s`); 3600 s and more `<H>h·<M>m·<S>s` (`1h·0m·0s`, `1h·2m·3s`). Hours are never rolled into days: 90061 s is `25h·1m·1s`. The counter is hidden, a plain `╰──────╯` bottom border, before the first Attempt of the Workflow starts (`elapsed_seconds` is `None`). The box is at least as wide as the counter text with its two spaces, plus 4 (`╰`, `─`, ` <duration> `, at least one `─`, `╯`). A need line that leaves a box (the `┬` on its bottom border) lands on the box's middle column, or, when the bottom-border text covers that column, on the first `─` after the text, as a need line lands on the top border on the first `─` after the title (`land`). The active time (`PrNode.elapsed_seconds`, `tui/src/read/flow.rs`; `None` until a Job of the Workflow has an Attempt) is the length of the union of the Attempt intervals of every Job of the Workflow (every Job with `job.pr == pr.key`, of any kind: work, docs, test, impl, open, update; follow-up Jobs after the pull request opened included), except explore Jobs, whose Attempts count in no Workflow's cost and no Workflow's active time, each interval being [`started_at`, `ended_at`, or now when the Attempt has not ended], MINUS the union of the Plan's Lead's `blocked` intervals, in whole seconds rounded down. The Lead's `blocked` intervals come from the `session-state` Events of `plans.lead_session` (`json_extract(data, '$.to')`, in id order): a transition to `blocked` opens one that lasts until the next transition to another state, or until now when there is none. `waiting` is never subtracted, and every Workflow of the Plan loses the same Lead `blocked` time. A queued Job that waits for a resource slot or a dependency has no Attempt, so that wait is excluded by itself. `plan_boxes_at(store, plan, now_ms)` (`now_ms` Unix milliseconds) reads it with the snapshot's own clock; `plan_boxes(store, plan)` keeps its signature and uses the store's clock; the plan view's snapshot (`snapshot_timed`, `tui/src/plan/mod.rs`) passes its own `now_ms`, so a running Attempt counts up to the snapshot's `now`. The counter is static between reads (pro-d182): the view has no timer, no per-second redraw and no per-second re-read, so it is the time as of the last snapshot. The view also re-reads its snapshot when a `pull_requests` row changed (see Rates), with the Event-driven coalescing as before: a conflict clears with no Event (`mergeable` changes silently; only becoming `conflicting` appends `pr-feedback`), so without that a box would keep `conflict`; a change of the update Job's status (`job-started`, `job-merged`, `job-failed`, `job-cancelled`, `job-added`; Events with `plan = p`) re-reads it too, which moves `updating` to `conflict` or back. It reads on no timer: the counter is the time as of the last read (pro-d182). **The cost** (User, Context record pro-d179 item 3, 2026-10-29 EDT; Plan workflow-cost-box, PR cost-box): when the Workflow has cost data, the bottom-border text is ` <duration> · <cost> `, the counter and the cost joined by ` · ` (a space, U+00B7, a space) inside the one pair of outer spaces; `<cost>` is `cpd_core::metrics::format_cost` of `PrNode.cost_usd` (contract §2.2 "Session cost", "Formatting": `$0.450`, `$4.500`, `$0.004`, `$1.23k`: always 5 cells after the `$`, contract §2.2), for example `╰─ 1h·2m·3s · $0.450 ───╯`. `footer_text` in `tui/src/plan/boxes.rs` is still the only place that builds the text, and it drives the box's width, the exit `┬` and the drawing: the whole text is written in `palette::muted_text()` from column box.x + 2, the box is at least as wide as the whole text plus 4, and a need line leaves on the middle column or, when the whole text covers it, on the first `─` after the whole text (in a box 26 cells wide the middle column is 12: ` 5s ` leaves the exit on column 12, and ` 5s · $0.450 ` covers it and moves the exit to column 15). A segment with no value is left out: with no cost data the text is ` <duration> ` exactly as before (no ` · `, and never `$0.000` for a missing cost), and with neither the border is plain; a cost of exactly zero (`Some(0.0)`) is drawn `$0.000`. `PrNode.cost_usd` (`tui/src/read/flow.rs`; `None` until some Attempt of the Workflow has cost data) is the Workflow's cost of contract §2.2 "Totals": the total of the costs of every Attempt of every Job of the Workflow, except explore Jobs, whose Attempts count in no Workflow's cost and no Workflow's active time, each by the rule of contract §2.2 "An Attempt's cost" (the stored `attempts.metrics` `$.cost_usd`, else the own cost of the Attempt's Worker Session file, the two never added); the Lead's and its side forks' own costs are in the Plan row's cost only. It is read by `plan_boxes_at` with the rest of the snapshot, so it is static between reads like the active time (pro-d182): no timer and no per-second re-read, and a running Worker's cost is its cost as of the last read.

  ```
  ╭ schema · api 󱦰 main · #31 open ────╮
  │  󰗠 types│S1 ── 󰗠 gql│S2         │
  ╰────────────────┬─────────────────╯
          ╭───────┴───────╮
          │               │
  ╭ web · web 󱦰 main ┴─╮ ╭ docs · api 󱦰 main ┴─╮
  │  󰽧 page│S1        │ │  󰝦 readme│H2     │
  ╰───────────────────╯ ╰─────────────────────╯
  ```

  A PR with no Jobs shows an empty box (` no Jobs `, colour 8). A Plan with one PR shows one box; a Plan with no PR yet shows the message `no PR yet: cpd pr add` (colour 8). The wheel and the scroll offsets apply to the whole canvas.

- **The explore box** (Plan `explore-roles`; display only, no schema change). A Plan with explore Jobs (§5.1) has one box titled `explore` below its PR boxes: its Job pills, with the Tier each is shown at (contract §3.4a), the key and the status and the connector lines of needs as for a PR. It has no `open` Job row and no pull-request or branch footer. A Plan with no explore Job has no such box. It draws no cost counter: the cost of an explore Attempt is in the Plan's cost only (contract §2.2 "Session cost", decision pro-d237). The same Jobs are in `plan render`'s `## Explore` section (§4.5).
- **Pulse** (User, hs-d229, hs-d232): with a Manager in the chat slot, the pane shows Pulse instead of the Plan view: one row per Plan of the Manager's Context, with a sparkline of the Plan's activity, so a stalled Plan stands out. It draws no icon: no dot, no `󰗠` (User, hs-d327). Reads only what the store holds; no write.
  - **When.** The newest `session-moved` Event with `data.place = 'chat'` (over all Sessions) belongs to a Session with role `manager`. With a Lead or a side fork in the slot the Plan view is unchanged; so it is with the placeholder or a history pane (message ` no Lead session in the centre`). The pane's tmux header stays ` Plan `.
  - **Rows.** The Plans of that Context that are not archived and either not done, or done (`plans.done_at`) inside the window (the n buckets shown, see Buckets). In tree order, a Plan row as the tree's Plan row: by tier (its Lead's dot, the Plan row's tier of the tree's rule in §17), then `plans.created_at` newest first (hs-d686, pro-d48).
  - **Row layout,** inside the Plan view's margin (MARGIN: 2 columns left/right, 1 row top; kept at 2 because Pulse rows are not boxes): `<plan id padded to the longest id>  <sparkline>  <age right-aligned in 9>`. The id is plain text. No icon: no dot before the id, no `󰗠`, no merge-count column (User, hs-d327); the sparkline takes the width they freed. The row is 13 + id_width + n columns.
  - **Buckets.** 5 minutes each, aligned to wall-clock multiples of 5 min since the Unix epoch; the last one is the current, partial bucket. The window is n buckets ending now, n = max(12, pane width - 2*2 (margin) - (13 + id_width)). If n < 12, 12 is used and the line is cut at the pane's edge.
  - **Activity** (what a bucket counts). The Events of the Plan (`events.plan`) of kinds plan-phase, plan-review, plan-outlined, plan-done, job-added, job-edited, job-merged, job-failed, job-retried, job-cancel-requested, job-cancelled, attempt-started, attempt-status, attempt-retry, attempt-nudge, all-merged, pr-feedback, pr-added, pr-edited, pr-cancelled, pr-started, pr-opened, pr-merged, escalation, escalation-answered, escalation-expired, error; plus Lead turns: `session-state` Events with `data.to = 'working'` whose Session has role `lead` (the Plan's Lead and its side forks). Not counted: session-moved, session-registered, session-renamed, session-archived, session-message, inbox-*, context-*, plan-created, plan-renamed, plan-archived, watch-*.
  - **Kinds** (User, hs-d327). Each counted Event has a kind of activity, by its Event kind; the kinds are what the bars are coloured by:

    | Kind | Colour (bright pairs: the row is clickable; contract §6.1.1, hs-d534) | Events |
    |---|---|---|
    | error | red (9) | `error`, `job-failed` |
    | plan / ask | magenta (13) | `plan-phase`, `plan-review`, `plan-done` |
    | PR opened | cyan (14) | `pr-opened` |
    | escalation | cyan (14) | `escalation`, `escalation-answered`, `escalation-expired` |
    | merged | green (10) | `job-merged`, `pr-merged` |
    | other | cyan (14) | every other counted Event, Lead turns included |

    A bucket holding several kinds takes the first in this order: **error, plan / ask, PR opened, escalation, merged, other** (the precedence the colours had, red, magenta, yellow, blue, green, white; the Manager's assumption, accepted with hs-d327, unchanged: only the colours of three kinds moved with hs-d534, so the kind decides, not the colour).
  - **Bars.** A bucket with activity: a bar `▁▂▃▄▅▆▇█` of level ceil(count * 8 / max) (1..8), where max is the largest bucket count over ALL rows (one scale, so rows compare; counts and scale as before hs-d327), in the colour of the bucket's kind (Kinds above): red, magenta, cyan or green, each the bright pair (`Color::LightRed`, `LightMagenta`, `LightCyan`, `LightGreen`). A bucket with no activity: a `▁` in colour 8, so the line never breaks. A row with fewer buckets than the line's n is blank on the left. No `󰗠` and no distinction for a running Attempt.
  - **Age.** Minutes since the Plan's latest activity Event (any time, not only the window); `0m` while one of its Attempts is not ended (status starting, running or merging). The age text follows the tree's rule below (with minutes converted to seconds). Colour 8. Blank when the Plan has no activity Event at all.
  - **Quiet.** A Plan is quiet when all four hold: it has queued or running Jobs (drafts do not count); no Attempt of it is not ended; its latest activity is 30 min old or more (no activity at all also counts); its Lead's dot is not a needs-you dot (`Dot::needs_you()` false). The age cell then reads `quiet <age>` (e.g. `quiet 41m`) in colour 8, like the age (hs-d534: it was the Review colour). The 30 minutes are a constant, not a config key.
  - **Clicks and wheel.** A left click on a row runs the Plan view's `Actions::open_lead(<the Plan's lead_session>)` (session open, then focus the chat slot); a failure is a `text` dialog (§17 "No notes"). The wheel scrolls the rows (3 rows a notch, clamped). No keys, no selection.
  - **Re-read.** A new Pulse snapshot on any Event of the Manager's Context (`events.context`) and on `session-moved`, coalesced as other views; also every 60 s (the window moves and ages tick; a running Attempt is not an Event) and on pane resize (the bucket count changes). Between reads the rows do not change: a row has no spinner.
  - **Escalation bar.** Cyan, agent activity (hs-d534 replaces hs-d352's blue: blue is focus and selection only, contract §6.1.1).
  - **Empty.** A Manager whose Context has no row shows ` no Plans yet` (colour 8) in the message row.

    Example, a 60-column pane, id_width 10, so n = 60 - 4 - 23 = 33 buckets (colours by kind, as above; a `▁` in colour 8 is an empty bucket):

    ```
      cross-repo  █▃▁▁▄▁▁▁▁▁▁▂▁▂▁▁▄▁▁▃▁▁▁▂▁▁▁▁▅▁▁▂▇         4m
      manager     ▁▁▁▁▁▃▁▁▁▁▅▁█▄▁▁▃▁▇▅▃▁▂▁▁▁▁▁▁▁▁▁▁         3m
      quality     ▁▄▃▁▁▁▁▁▁▁▄▄▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁  quiet 41m
    ```

**`widgets`** (reference `dispatch`), top to bottom: the **Dispatch column** is the machine and the Inbox (pro-d130, placed by pro-d147; the PRs section is removed: no `PRs` title, no two-line PR rows, no `no open PRs` line, no PR scroll cues, no merge icon, no `active` blue block, no Lead spinner on a PR line, no PR right-click menu). Rows: row 0 blank, row 1 ` <hostname>` (bold), row 2 the CPU/GPU/memory line, then `rest`, the rows below, whose bottom holds the Inbox section (§13.2 "Inbox section"; its first row, empty, is `rest.bottom() - height`, its header the next row and its last row is the pane's last row); the Inbox is bottom-aligned and grows upward and the rows between the usage line and the section are blank. There is no git and no Jira section (hs-d257). The pane has no title (contract §6.1, hs-d729).
1. the machine (no header): its hostname in bold (` <hostname>`; the row is colour 7), then ` <cpu icon> N%  <gpu icon> A% B%  <mem icon> M%` (hs-d257, hs-d333), for example `  23%   61% 0%   41%` under ` homeserver`, with the CPU icon `nf-oct-cpu` (U+F4BC), the GPU icon (U+E266) and the memory icon (U+EFC5). The GPU part has one utilization % per GPU, in the order the tool lists them, one space between a GPU's numbers and two spaces between the parts; a GPU that reports no number is `–%` in its place. When no GPU is found (neither tool is available, or its output has no GPU) there is no GPU part at all and the row is `  N%   M%`; a missing tool is an empty list, never an error. All numbers are integers, drawn in colour 7 (read-only text, contract §6.1.1; no thresholds); the load average is not shown. CPU % is the share of non-idle time across all CPUs over the last 2 seconds, so the view keeps its previous CPU sample and shows `–%` until a second sample exists; a number that cannot be read is `–%` too, which is no error of the view (as the hostname, tui-W10). Memory % is used / total. GPU % is the GPU's utilization. All are re-read every 2 seconds. On Linux the CPU comes from two reads of the `cpu` line of `/proc/stat` (idle = idle + iowait) and the memory from `/proc/meminfo` (`MemTotal - MemAvailable`, over `MemTotal`); both files are read (a file read, never a process) through the sampler's reader, which tests script and what a test does not script is unreadable (`–%`). On macOS the CPU is the sum of `ps -A -o %cpu=` divided by `sysctl -n hw.ncpu`, capped at 100 (a point-in-time figure, so it needs no earlier sample) and the memory is `vm_stat`'s pages active + wired down + occupied by compressor, times the page size `vm_stat` prints, over `sysctl -n hw.memsize`. The GPUs come, on Linux, from `nvidia-smi --query-gpu=index,utilization.gpu --format=csv,noheader,nounits -lms 2000`, one long-running process for the view's life (one integer per line, one GPU per line; a line that is no integer is `–`) and on macOS from `ioreg -r -d 1 -w 0 -c IOAccelerator` (each accelerator's `"PerformanceStatistics"` dictionary has `"Device Utilization %"=N`; an accelerator without that key is `–`). The macOS parse is unverified on hardware: it is tested on a captured sample only;



**A merge in progress** (User, hs-d495, hs-d550 B4–B5; the marker design is the Lead's, the User may correct it). From the confirm until the background request ends (`cpd tui action merge`, below), **every place that shows that pull request's merge button shows the cockpit's spinner (`palette::SPINNER`, framed by the shared clock as for a running Job) instead and it cannot be clicked**: the Inbox's `pr-ready` row (its glyph cell shows the yellow spinner, a click does nothing) and the right-click menu's **Merge** entry (shown as `Merging`, not selectable). On success the merge's `cpd pr poll` clears the pull request from the lists; on failure the indicator goes and the failure dialog opens (hs-d469 `dialog::show_error`). **One marker is the source every view reads:** `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json` (contract §1.4), one file per pull request. The views (the cockpit's Inbox and the tree's) read the markers on each snapshot and a restarted view reads the same files, so all views and restarts agree. **B5** A pull request is merging (spinner shown) when its marker's pid is alive, or when its marker has `"merged": true` and its pull request's `pull_requests` row is `open`; a marker with `merged: true` whose row is not open and a marker without it whose pid is dead, are ignored.

The menu is drawn as a rounded box (the Box rule, contract §6.1.1 "Boxes", by `tui/src/menu.rs` using box_widget), at the click, clamped inside the pane less one blank column on each side, as the tree's menu is; ↑↓ and Enter pick, Esc closes; a click picks an item; a click outside closes it. The generic menu drawing is shared between the tree's right-click menu and the Inbox's (`tui/src/menu.rs`, shared with `tui/src/tree/menu.rs`, hs-d9: reused, not copied). There is no key that opens it.

**The Inbox's right-click menu** (Dispatch column). Claim is the first entry on `review-requested` rows only and runs `inbox claim <id>` (§21.7, review-requested items only); the other entries are the Snooze and Dismiss entries. **Approve** is the first entry on `deploy` rows only (pro-d167), then the Snooze and Dismiss entries (no Claim): it runs `cpd inbox approve <id>` (§21.13) at once through the cockpit's command runner, with no confirm popup (pro-d111); a refusal or failure opens the failure dialog titled with the command words, exactly as a refused `Claim` does (`dialog::report`). No other kind has Approve. **A `pr-ready` row's menu** (pro-d130), in this order, each entry only when it applies: **Merge** (status `ready to merge`; shown busy as **Merging** while the pull request's merge marker is live and then not selectable) runs `cpd tui action merge <owner>/<repo>#<n> --client <name>` at once, no confirm popup (pro-d111); **Post** (status `ready for user review`) runs `cpd pr post --pr <key> --plan <plan>` (§9.8b) at once, no confirm popup, through the cockpit's command runner, where `<key>` is the key of the `prs` row of the item's Plan that points to the pull request, read from the store when the entry is picked (the cockpit waits for the Slack call, shows no spinner and a failure opens the error dialog titled `Post Pull Request`); **Go to Lead** (a Plan owns the pull request) runs `cpd session open <lead>`, then `select-pane` on the chat slot; **Copy Link** (the item has a `url`) runs `tmux set-buffer -w -- <url>` (no `cpd` command); then **Snooze 15 min**, **Snooze 1 h**, **Snooze until tomorrow 09:00** and **Dismiss**. The status is the label's tail, parsed by `cpd_core::inbox::pr::Status::of_item`; the cockpit never re-derives a state from the pull request row. **A `pr-closed` row's menu** (Plan closed-unmerged) has **Go to Lead**, **Copy Link** (its `url`) and the Snooze and Dismiss entries, and no Merge, Post or Claim. Every other kind's menu is unchanged.

**The PRs section** was removed by pro-d130: its pull requests are Inbox rows (above, `pr-ready`) and the Inbox scrolls as §13.2 "Inbox section" says.

The column has no engine line, no slots, no Escalations, no Events and no footer hint. The Event log is the `logs` view (R36, below).

**Links** (User, pro-d60; hs-d187, hs-d191). The cockpit links a page through the `Link` widget (pro-d60, 2026-10-26: cpd's cockpit links are one reusable ratatui widget, never OSC 8 terminal escape sequences). the Plan view's PR box title draws the PR's state `#<number> <state>` (and the Lead's working dot when the Lead works on it) as underlined text, then one space, then the cue, the whole output clickable. The Inbox's link-kind rows (the four kinds `plan-review`, `pr-ready`, `review-requested` and `deploy`, §13.2 Clicks on an Inbox item) draw the item's label as the link's text. A click on any other cell does what it did before. The click rule: a plain left click (any modifiers) on a link's cell calls `link::open_url(&*ports.runner, url)`, which opens the url in the default browser. The widget API: `Link` takes a url and optional text; `Link::new(url)` draws the cue only (one cell); `Link::text(line)` sets the text (drawn with `Modifier::UNDERLINED` added to each span's style, its colours kept) and outputs `<text> 󱆭` (the space unlined, the cue in `palette::clickable_text()`, not underlined); `Link::width()` returns the output's width in cells (1, or text's width + 2). `Links` holds the (Rect, url) of every link drawn on the frame; `Links::draw(link, area, buf)` renders into `area` and records the rect it drew with the url; `Links::at(pos)` returns the url at that cell or None. The widget never holds an escape character; output is ordinary cells. `open_url` refuses with an `Err` a url that does not start with `http://` or `https://` or holds a byte outside 0x21..=0x7e; otherwise it runs `open` with the url as its one argument through the `Runner` and reports a non-zero exit or a runner failure as an `Err` whose message names `open`, the url and what `open` said on stderr, using the `ErrorCode` of `tree/actions.rs`.

**The StatusBar** (R36, User decision; moved to tmux's status line by the User, Plan tui-visuals, hs-d149, hs-d179) is **not a view and has no pane**: it is the one row of tmux's status line on the cockpit session (contract §6.1), as **one dark grey (colour 8) `─` line across the full width** at the bottom of the client, drawn by tmux from `status-format[0]` (one status row; hs-d567: the chat slot's last row, the one above the line, is empty by its occupant, so the chat's content never touches the line; contract §6.1 "The blank row" has the rules Q1-Q4, the evidence and the 20-row mock-up). There is no `statusbar` view, no `@cpd_view statusbar` and no process behind it; it takes no keyboard focus (a click on it is not a click on a pane) and draws no border title. **Its left side holds action buttons only; its right side holds read-only information.**
- **Left, in order** (hs-d479): the line starts with one `─`, then four buttons, `󰨝` (U+F0A1D md-view_dashboard_outline, reset layout), `󰨁` (U+F0A01 md-map_legend, legend), `󰧭` (U+F09ED md-text_box_outline, logs) and `` (U+EEC1 nf-fa-comment_dollar, costs; Context records pro-d189, pro-d195), Material Design (hs-d235) and Font Awesome, **joined by a single `─`**; the leading `─` and the joins are **outside the click ranges** (a range also owns the cell after it, so each join is the cell the button before it owns; a space inside a button lies inside that button's own range and moves no join or other range; the click columns with the runner unset, the check that the space shifts no click, are reset 1-4 (owns 5), legend 6-9 (owns 10), logs 11-14 (owns 15) and costs 16-19 (owns 20), hs-d730, pro-d296). Each is a **pill** (hs-d266): the left cap `` (U+E0B6), the glyph followed by one space on a white block, no other padding (the space is inside the block: the terminal draws a Nerd Font glyph full size only with an empty cell after it, hs-d730, pro-d296) (interactive, hs-d534; all four buttons share the one base colour 7, drawn bright 15 as a clickable pill, pro-d195, so the four look alike and the costs button looks the same whether costs show or not: `#[fg=colour15]#[reverse]`, `#[noreverse]#[default]`, no `reverse`), the right cap `` (U+E0B4), the caps in `fg=colour15` on the normal background (contract §6.1.1 has the exact format); a button is 4 cells wide (the two caps, the glyph and its space), the left group 20 cells (the leading `─`, four buttons of 4 cells and 3 joins). Each is its own `#[range=user|…]` (`reset`, `legend`, `logs`, `costs`) wrapping the whole pill, caps included and a click runs `cpd tui action <range> --client <name>` (below). Tooltips are not built (the User dropped them, hs-d179). Mockup: `─󰨝 ─󰨁 ─󰧭 ─ ───…───inbox 8─workers 6·0·4/6─`.
- **Runner pill** (pro-d199), right after the costs button, while the tmux server option `@cpd_runner` is set (the engine sets it from `worker_runner`, §12; `RUNNER_OPTION` in `core/src/host/tmux.rs`): a single `━` join and then a **clickable pill with text only**, the option's value `pi` or `durable` between the caps, with no icon and no padding; it is drawn like a button (base 7 drawn 15, inverted, caps 15; contract §6.1.1). Its `#[range=user|runner]` wraps the whole pill and a click runs `cpd tui action runner --client <name>` (below). Nothing is drawn while the option is unset. The left group is then 20 cells plus `#{n:@cpd_runner}` + 3.
- **Preview marker** (hs-n264, hs-d479), right after the buttons and the runner pill: while the tmux server option `@cpd_preview` is set (`scripts/preview-tui.sh on` sets it to `<worktree basename> <short sha>`, `off` unsets it), a single `─` join and then a **non-interactive pill** (yellow, hs-d534: colour 3 inverted: text in the terminal's background colour, caps colour 3; it was grey from hs-d530 to hs-d534) with no padding: the left cap ``, `preview: <value>`, the right cap ``, so a preview never passes for the installed cpd; nothing when it is unset. The group is then `─󰨝 ─󰨁 ─󰧭 ─preview: <value>───…` and the fill is shorter by the pill's width (`#{n:@cpd_preview}` + 12).
- **The fill** (User, hs-d479): a `─` (U+2500) in **dark grey, colour 8** (no `reverse`) fills the **free width** between the left group and the right group, across the whole row, so the row has no blank gap; the line ends with one closing `─`. Its width is `#{client_width}` minus the left group (20 cells, plus the preview's) minus the right group (`#{w:@cpd_workers}` or `#{w:@cpd_workers_short}`, the form drawn, plus 1 for the closing `─`).
- **Right**, in order: the workers pill, a single `─` join, the version pill, then the closing `─`, **each item its own pill** with no padding (hs-d479; the joins and the closing `─` are outside the click ranges). There is no Inbox pill (hs-d541, the User's decision: the Inbox is the tree's section only). The workers and version pills use the tmux `pill!` shape: `#[fg=colour8]U+E0B6#[reverse]<text>#[noreverse]U+E0B4#[default]`, where the text is inverted (text in the terminal's background colour) and the caps are colour 8 (non-interactive; hs-d561 supersedes hs-d534's 7 for the workers and version pills, contract §6.1.1); the pill has one text colour: the text carries none. The workers text (User, hs-d198, hs-d749) is `workers Q󰞘W󰞘N/M`, where `Q` is the number of `queued` Jobs, `W` is the number of Ready Jobs (a `queued` Job whose Dependencies are all `merged` and whose PR has started, §3.2), `N` is the number of Attempts not `ended`, and `M` is config `concurrency`. The text is plain with no colours. **The form is chosen per client**: the long form when the fill would still be at least 1 cell with it (`#{client_width}` at least the left group's 20 cells (plus the preview's) + `#{w:@cpd_workers}` + 1 for the closing `─` + 1 for the fill), else the short form `Q󰞘W󰞘N/M`, same plain text; the status line draws the pill of the form it chooses per client with `#{?#{e|>=:#{client_width},<20 + preview + #{w:@cpd_workers} + 2>},#{@cpd_workers},#{@cpd_workers_short}}`; the fill's width uses `#{w:@cpd_workers}` or `#{w:@cpd_workers_short}` the same way. Both texts are fed by the engine (§12), not computed by the status line: up to one tick late and stale while the engine is stopped (the deviation in §17). The old failure note is gone: a failed click reports with `display-message -c <client>` (below).

**`cpd tui action merge <owner>/<repo>#<n> --client <name>`** (internal; hs-d473, hs-d495): what the `merge` popup's confirm starts, with `run-shell -b`, after the popup has closed. It runs `cpd pr merge <owner>/<repo>#<n>` (§9.10), then `cpd pr poll`, in the background. The merge marker (Lead's design, User may correct it; contract §1.4):
- **File:** `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json`, `{"pid": <the action's pid>, "at": <RFC 3339 time>, "merged": <true when the merge succeeded, absent otherwise>}`, written atomically (temporary file, then rename).
- **Writer and remover:** only this action (**B4**). It writes the marker first (the initial `{"pid", "at"}`), before `cpd pr merge`; on success rewrites it atomically with `"merged": true` after the merge and then runs `cpd pr poll`; then removes it when the pull request's `pull_requests` row is not `open` (merged, closed, or no row); removes it before opening the failure dialog on failure; and removes it on any error path, so no exit leaves it behind. **B5**: a marker with `merged: true` whose pull request's row is not open and a marker without it whose pid is not alive, are ignored; a second `cpd tui action merge` for a pull request that is merging (`merged` field true or pid alive) does nothing and logs `merge already running`.
- **Readers:** the views, on each snapshot (above); a marker whose pid is not alive is stale and ignored.

**`cpd tui action <reset|legend|logs|costs|runner|host|incident> --client <name>`** (internal) is what a click on a StatusBar button runs (the `MouseDown1Status` binding, contract §6.1); **`cpd tui action merge <owner>/<repo>#<n> --client <name>`** is what the `merge` popup's confirm starts (hs-d473, below). Not a view, it owns no terminal; it runs the action **on the client named** (`#{client_name}` of the clicking client), so a popup opens on that client only, whatever other clients are attached.
- `reset` runs reset-layout (below); `legend` opens the legend popup, `logs` the logs popup (each `display-popup -c <name> …` sized to that client), `host` opens the host Manager (as the host action, §9.12, the same park/swap as the incident action below) and `incident` opens the incident's Lead (the same park/swap as the host action, no message sent; a failure is reported on the client).
- `costs` sends the literal key `$` to the tree pane, the pane of `cpd:main` whose `@cpd_view` is `tree` (`send-keys -t <pane> -l $`): **`cpd tui action costs --client <name>`** prints nothing and exits 0; with no live tree pane it fails like every other action (the error is shown on the client with `display-message -c`).
- `runner` flips the Worker runner (contract §6.1 "The runner pill", §8.1, §8.2): **`cpd tui action runner --client <name>`** reads `$CPD_HOME/config.toml` and calls `cpd_core::config::toggle`, which takes the effective `worker_runner` (the default `pi` when the line is absent), flips it (`pi` ↔ `durable`), rewrites ONLY the top-level `worker_runner` line atomically (a temporary file in the same directory, then a rename) and returns the new value. The action then runs `set -g @cpd_runner` to that value and `refresh-client -S -c <name>`. It prints nothing and exits 0. It opens no store, touches no Attempt and sends no signal: a started Attempt keeps its runner and only Attempts created after the click follow it (contract §3.4). A config that does not parse is refused by `toggle` (`config_invalid`); the file is left untouched and the failure is shown on the client with `display-message -l -c <name>`.
- **It prints nothing, on stdout or stderr**: tmux's `run-shell -b` puts the active pane into view mode when its command fails (research `statusbar-no-row.md` 2c) and shows any output of it in view mode (tmux(1), `run-shell`). On a failure (an unknown action, whose message lists `costs` among the actions, a client that has gone, a tmux error) it shows the message with `display-message -c <name> '<message>'` and exits 0; the binding also appends `2>/dev/null`.

**`legend`** is what every colour and icon of the cockpit means, from the palette (contract §6.1.1, hs-d534: each swatch and sample is drawn by the same palette call as the element it shows, so the legend shows the colours the views show; its title is a read-only title pill, its section titles and words colour 7, its column header and scroll position colour 8). It opens in a tmux popup (`display-popup -E -T ' legend ' -w <w> -h <h>`, sized to the legend and at most the client's size) from the tree's `?` key or the StatusBar's legend button (`cpd tui action legend`), because the columns are narrower than the legend; its title is drawn in the pill section-title style, the plain form (§17, the dialog kit's, hs-d297, hs-d475, hs-d478): ` ──Legend: …───`; when it is taller than its popup, ↑↓, j/k, PgUp/PgDn and the **mouse wheel** scroll it (mouse capture on; the kit's scrolling); any other key closes it. It has no `@cpd_view` pane of its own. The Job states it lists include **landing** (the sample is the Job pill's, the base yellow 3, drawn by the same palette call; the counter beside it is the bright 11; a Job is landing from its first check to the end of its merge, contract §6.1.1). Its section titles are `Jobs: Plan Pills And Context Counters`, `Session Dots`, `Inbox` and `Pulse` (Title Case, hs-d475; the popup's own tmux title ` legend ` is tmux's and unchanged); the Job statuses it lists are those the chart draws, so `cancelled` is not among them, The Job statuses it lists are those the chart draws, so `cancelled` is not among them and `running` is listed with the spinner frames `` (the 28 frames of §17 section 17). `Session Dots` lists each glyph (`` U+E3E3, `` U+E3E3, U+E3C8..U+E3E2 in each of the four Work colours of pro-d200 (magenta 13, cyan 14, yellow 11, white 15; the phase colours of hs-d622 and hs-d624 are superseded), then `` U+E3E3, U+E3C8..U+E3E2 in red 9 (a Plan's failed Job is being worked on or a Worker Escalation is open, one frame every 50 ms, hs-d581, pro-d200), `󱡓` U+F1853, `` U+E3D5, `` U+E3D5) and each `` colour (red, yellow, magenta, cyan, white) with its meaning: red 9 `the Lead is fixing a failed Job or answering a Worker's question`, yellow 11 `the Lead is working on a pull request (open, conflict, deploy, landing)`, magenta 13 `the Lead is planning (grounding, drafts, a follow-up)`, cyan 14 `the Lead is working alongside running Jobs`, white 15 `a working Session with no Plan`, as §17's dot rule gives them (the same colours mark the Inbox's items; the Inbox's items draw their own tier glyphs and three kinds their own glyphs, below; the legend says so in one line), then the precedence line `red > yellow > magenta > cyan > white >  > 󱡓 >  > `, drawn with the real glyphs and colours. `Inbox` (hs-d483; between `Session Dots` and `Pulse`) lists the six tiers, one line each, in the order red, yellow, magenta, blue, cyan, white and the three kind lines below them: the tier's glyph in its colour, then its meaning: `\u{F0362}` (U+F0362, red) `a blocker only you can clear`, `\u{F069A}` (U+F069A, yellow) `pull-request work`, `\u{F173A}` (U+F173A, magenta) `a decision before the work runs` (hs-d546, hs-d547), `\u{F0368}` (U+F0368, blue, pro-d58) `external support: a review asked of you`, `\u{F06F1}` (U+F06F1, cyan) `a cpd notice: curation, models changed; nothing blocks`, `\u{F0366}` (U+F0366, white) `an unread reply`; then **three more lines, one per exception kind** (pro-d98, pro-d100), each with the kind's glyph in its tier colour and its meaning: `\u{F15A9}` (U+F15A9, yellow, `deploy`) `a deploy waits for your approval`, `\u{F12F4}` (U+F12F4, yellow, `pr-ready`) `a pull request of yours, coloured by its state`, `\u{F12F2}` (U+F12F2, blue, `review-requested`) `a review asked of you`; the Inbox rows draw no state dot. Its lines say what needs the User and what only waits for an agent (hs-d303): `red: a dialog in dispatch or deploy, a stuck Job, an engine error, your own broken pull request that nobody is fixing` (an Escalation is no Inbox item, so it is never red; the phase is `deploy`, not `pr`; a pull request a cpd Plan owns is not red, hs-d302); `yellow: ready to post, ready to merge, review requested`; `magenta: a Plan review decision, a dialog question while planning` (hs-d570 changed meaning); `blue: review requested` (pro-d58); `cyan: a cpd notice: curation, models changed (hs-d570, pro-d16)`; `waiting: an agent waits for another agent (a Worker for its Lead), not for you` (the `waiting` dot, between `working` and `idle`); the `white` line is unchanged. The `Meanings → colour` table lists `waiting on an agent` in `Meaning::Waiting`'s colour, cyan (`Color::Cyan`: not blue, which means focus only, §17 "Blue"; neither the red of a failure nor the yellow of attention; the User's decision of 2026-10-20, which resolves hs-d303 against hs-d352 and cyan is agent activity in hs-d534). Its Jobs list shows one failed pill row: red loader: a failed Job being worked on (the same pill with the moon frames, one frame every 50 ms, hs-d586); no stuck row, as E8 opens no `job-stuck` item (hs-d318). Its counters row lists the four roles in the tree's order `planned⋅active⋅failed⋅done`, with a sample of one of each (planned 1 magenta 13, active 1 cyan 14, failed 1 red 9, done 1 green 10); the tree draws only nonzero roles and a failed counter is always red (hs-d580, hs-d636). Its cost row lists the cost counters of the Context and Plan rows (decision pro-d179): the plain text `cost`, the sample `format_cost(0.12)` (`$0.120`) in colour 8 (`palette::muted_text()`) and in colour 8 the words `$0.004 ⋅ $1.23k ⋅ omitted when unknown`. Its PR states list is `open`, `merged`, `closed` (waiting: `its Lead handles it`), `closed` (red: `its Lead asks you`) `updating` (cyan, both: `its update Job is fixing the conflict`, `its update Job is queued`; the rule of §17 PR boxes, hs-d532) and `conflict` (red: the one of a pull request no Plan owns; an owned one with no update Job is drawn cyan, as the plan view's box titles do). Its sample pill has the same form, `` + `<icon> key│S1` + ``, drawn as the Job pill is (hs-d534, pro-d63: the status colour's base colour, text in the terminal's background colour (inverted), caps the same; one sample per status); the legend's titles are read-only title pills (colour 7 inverted: text in the terminal's background colour). Pulse section (hs-d327): `▂▄▆` = `activity per 5 min, first colour wins` (a slot takes the first colour below); then one `▄` per kind in the precedence, red `an error, or a Job failed`, magenta `a Plan or ask Event`, cyan `a PR opened`, cyan `an escalation`, green `a Job or a PR merged`, cyan `any other activity` (the colours are the bars', §13.2 Pulse); a `▁` in colour 8 = `no activity`; `quiet 41m` (colour 8) = queued or running work with no activity for 30 min. Three entries a row (the popup is at most 80 % of the client tall).

**`merge`**: there is no confirm popup and no merge icon (pro-d111, pro-d130); the action is started from the Inbox's `pr-ready` menu (**Merge**) only, with tmux `run-shell -b`. **The `merge` action** (hs-d473, hs-d312) is the background run, in its own process, owned by the tmux server, with none of the popup's. It prints nothing, on stdout or stderr and exits 0 whatever happens, as every `cpd tui action` does (`run-shell -b` puts the active pane into view mode on a failure or any output, above):
- It runs `cpd pr merge <owner>/<repo>#<n>` (§9.10), which records the merge in the store immediately (hs-d526), then `cpd pr poll` (§9.2) to refresh (the poll's result window is unchanged: no second merge by the poll itself, just refresh of the stored merge). A failed merge runs no poll. **Nothing shows on success**: the Inbox shows the merge after the poll, which is already stored.
- **A failed merge** is the cockpit's one failure rule (§17 "No notes (a)"): a `text` dialog of the dialog kit (§13.2), the same one every failure opens, never a second dialog, opens on that client (`<name>`) with `merge <owner>/<repo>#<n>: <code>: <message>`, the `<code>` and `<message>` those of the failed command's error (for example `merge_not_clean`) and a line with the same text goes to the merge's log, `$CPD_HOME/run/tui/merge.log` (hs-d364: the log of the `merge` view that started the action).
- **A poll that fails after a merge** is **a log line only**, in the same file, `pr poll after merge <owner>/<repo>#<n>: <code>: <message>` and no dialog: the pull request did merge and the next periodic poll refreshes the column (hs-d473's first reading, a dialog for it, is replaced by §17 "No notes").
- **When the dialog cannot open** (the client has gone, tmux fails), the error does not vanish: the log line is written **first**, before the dialog is tried, so it is always there; then the dialog is tried; if that fails, the action shows the message with `display-message -c <name> '<message>'`, as every `tui action` does for its own failures and if that fails too the log line is what is left. Why the log first: it needs no client and no tmux, so it is the one place that cannot fail for the reasons the other two can.
- **A second confirm while a merge runs** (the icon shows while the stored `merge_state` is still `'clean'`, until the poll after the merge): the second action runs `cpd pr merge`, which reads GitHub fresh (§9.10 step 1). Either the first merge is done and it is refused `merge_not_clean` (`details.state` `MERGED`), or `gh` is still merging and GitHub accepts one of the two and refuses the other. One pull request is merged once; the refused run's error opens the failure dialog. Acceptable, not harmful: the User asked twice and the dialog says why the second did nothing.

**`logs`** (R36) is the **Event log**: every Event (§5), one line each: its local time of day (`HH:MM:SS`, colour 8, bare with no zone per §1.3 pro-d42), then its `text`, or its kind when the `text` is empty, an `error` Event in the failed colour (red 1; the dialog is read-only), except one **addressed to a Lead**, in `Meaning::Waiting`'s colour (cyan 6): the Lead handles it (audit Q5, hs-d303). An `error` is addressed to a Lead when it names a Plan (`plan` set) and its `data.source` is not the engine's own `watch`, or its `data.code` is `pr_start_failed` or `push_failed` (the engine reports those for a Lead, with source `watch`); the engine's own step errors (`source` `watch`, contract §5.2) and any `error` that names no Plan stay red. It opens in a tmux popup (`display-popup -E -T ' logs '`, at most 80 % of the client's width and height) from the StatusBar's `󰧭` button (`cpd tui action logs`, on the clicking client; no Inbox click opens it: `engine-down` starts the engine and the engine's errors go to the Manager, hs-d314) and has no `@cpd_view` pane of its own. With `--event <id>` (an Event id of §5) the view opens **at that Event**: its line is selected (the blue block, colour 4 inverted: text in the terminal's background colour, contract §6.1.1) and scrolled into view and it does not follow until the User scrolls to the bottom; an id outside the latest 500 Events is read on its own too. Without it, it opens at the bottom. The newest Event is last; the view follows live while it shows the bottom and stops following when the User scrolls up (it follows again at the bottom). Its title is drawn in the section-title style, the plain form (§17; hs-d297, hs-d475, hs-d478): ` ──Logs───…`. ↑↓, j/k, PgUp/PgDn and the **mouse wheel** scroll and End or `G` goes to the bottom, where it follows again (mouse capture on; the kit's scrolling, hs-d297), with the live follow and `--event` behaviour above unchanged; `q` or Esc closes it. **Margin** (the Box rule, contract §6.1.1 "Boxes"; the popup's contents are a widget in the space inside tmux's border): every row has 1 blank column on each side, so the text starts at column 1 of the popup's inside and ends at column `W - 2`.

**Reset-layout** is not a CLI command. `cpd tui action reset` and `cpd tui` run it. It runs `resize-pane` on the panes of `<cockpit>:main` found by `@cpd_view`, in this order:

| # | `@cpd_view` | Size |
|---|---|---|
| 1 | `tree` | `-x 20%` |
| 2 | `widgets` | `-x 30%` |
| 3 | `plan` | `-y 30%` |

The chat slot is never resized; it takes what is left. **The chat slot's last row (hs-d567; contract §6.1 "The blank row", Q1-Q4):** the status line is one row (`status on`), so the window is as tall as before hs-d561. The layout divides the window into columns: tree 20%, middle 50%, widgets 30%. The middle column's rows divide as: Plan view 30%, chat slot 70%. The `resize-pane` calls and these sizes are fractions of the window. The last row of the chat slot's pane is empty **by its occupant**, not by tmux: a Lead's or Manager's pi (the adapter, §15.1), the history view (§13.2); the placeholder draws one line at the top and needs nothing. In a 20-row client: row 1 the pane borders, rows 2-19 the panes (row 19, the chat slot's last row, empty), row 20 the `─` line; the tree, the Plan view and the widgets reach row 19. Code and tests: `core/src/host/tmux.rs` and `tui/src/cockpit.rs`, with the tests that pin the status options or a pane's height. The StatusBar's 󰨝 click runs it through `cpd tui action reset`; `cpd tui` runs it once, right after building a new `main` and after killing a leftover `statusbar` pane of an existing one (§13.1).

Refusal before drawing: an unknown view or Attempt is `usage` or `attempt_not_found`. Idempotent: they only read.

### 13.3 `cpd` with no arguments

```
cpd
```

User decision hs-d259 (2026-10-19), Plan pi-pin, PR `bare`. **Bare `cpd` opens the cockpit.** In a terminal it attaches that terminal to the cockpit (`tmux -L <socket> attach -t <cockpit_session>`, the `attach` of §13.1) after doing every follow-up an update needs, so the User needs no other command after an install. It owns its terminal (§1.2): nothing on success.

**Follow-ups**, in this order:

1. **Engine:** `cpd watch --ensure` (§12).
2. **Cockpit:** what `cpd tui` does (§13.1): build it when missing, respawn a dead pane of its layout, remove an older cpd's StatusBar pane or `worker` pane (pro-d63) and set the server options and the status line. One implementation, shared with `cpd tui`: bare `cpd` calls it, never a copy.
3. **Views on an old binary:** when the panes of the views run an older cpd than the installed one, `cpd tui --respawn` (§13.1).
4. **tmux version:** when the cockpit's tmux server runs an older tmux than the installed client (`tmux -L <socket> display -p '#{version}'` against `tmux -V`; hs-d166), restart the server onto the installed tmux **when it is safe**, reopen every Lead and Manager Session that had a pane (same pi session file, same place: the chat slot, its parked window, a Context's window), then attach. When it is not safe, **hold, drain, restart** (User, hs-d294):
   - **Safe** means: no Attempt `starting`, `running` or `merging` and no Lead or Manager Session mid-turn (state `working`; a `blocked` Session has a dialog open, so it is also not safe). Restarting stops every pi in the server; Leads and Managers resume from their session files.
   - **Safe now:** bare `cpd` restarts the server itself, in its own process (it runs in a terminal outside the server): it records every Lead and Manager Session with a live pane and its place (`chat`, or parked: its `park-<label>` window, or a Manager's `ctx-<context>` window); `kill-server`; `cpd tui` (the cockpit on the installed tmux, which also runs `watch --ensure`; the last chat Lead is back in the chat slot); then each Session in the same place: the one of the chat slot as `session open` does, the others in a new window running `cpd session run <id>`, which resumes the Session's pi file, each with `session-moved`. It then attaches with the installed tmux and says in one line how many Sessions it reopened.
   - **Not safe:** it **marks a restart pending** (the file `$CPD_HOME/run/tmux-restart.pending`, JSON `{"at", "server", "installed", "failed"}`; no store change), attaches at once with the **server's own tmux binary** (its path from the server pid: on Linux `/proc/<pid>/exe`, on macOS read with `libproc`), because a newer client cannot attach to an older server (`open terminal failed: not a terminal`, hs-n224) and prints one line saying so and that the restart happens once nothing is in the way. `cpd tui` does the same marking when it finds the server older (it never restarts the server itself, since it may run inside it) and `tui --attach` attaches with the server's own binary then as well.
   - **While a restart is pending, the engine starts no new Attempt** (E3's hold, contract §3.7); running Attempts finish normally. It is a planned hold, not an error: `cpd doctor` shows it as the `tmux_restart` check ("tmux restart pending", §14.1). Once none runs (no Attempt `starting`, `running` or `merging`) and no Lead or Manager Session is `working` or `blocked`, the engine starts the restart **detached** (`cpd watch --restart-tmux`, internal; `setsid`, its log `run/tmux-restart.log`), because the restart kills the server the engine itself runs in. The detached process takes `locks/tmux-restart.lock`, checks again that it is safe, does what "safe now" does, clears the pending file and logs the result. Then the engine, started again by `cpd tui`, schedules as usual. The User reconnects with bare `cpd`.
   - **A failed restart** keeps the pending file with `failed` (`at`, `code`, `reason`) and the engine that runs again appends one `error` Event (`source: "watch"`, `code` of the failure, `details: {at, server, installed, log}`); a failed restart holds nothing. The next bare `cpd` finding the server still older tries again (a failed file is marked afresh); finding it as new as the installed tmux, it says the failure in one line and removes the file. Never from inside the cockpit's own server.

Rerunning is safe: a restart already pending keeps its time and a server that is not older is left alone. Internal command: `cpd watch --restart-tmux` (not an interface of the User or an agent; it conflicts with `--ensure`).

`scripts/restart-tmux.sh` (PR `tmux`) is gone: its checks (the refusal while an Attempt runs, the attach hint with the server's binary) are the binary's own.

**Assumptions, confirmed by the User in the Plan review:**

- **A1** Without a terminal on stdin and stdout, bare `cpd` keeps today's behaviour: the JSON usage error `usage`, exit 2. It does no follow-up.
- **A2** Inside the cockpit's own server (`$TMUX` names its socket), bare `cpd` does not nest an attach: it does the follow-ups that are safe from inside, never the tmux restart nor the marking of one (it would kill the pane it runs in) and prints one line saying how to reach the cockpit.
- **A3** Each follow-up's failure is printed in one line on stderr and the attach still happens when the cockpit exists. When the cockpit does not exist and could not be built, bare `cpd` prints the failure value and exits with its code.
- **A4** Nothing new in the store: no table, column or Event kind.

## 14. `doctor`, `config`, `tiers`, `models`, `preset`

### 14.1 `doctor`

```
cpd doctor
```

Checks the machine (cpd 1 §11 `doctor`, kept in shape). Each check has `name`, `ok`, `severity` (`error` or `warn`) and when relevant `found`, `expected` and `message`. Any failing `error` check makes the result `doctor_failed` (exit 1) with every check in `details.checks`; `warn` checks never fail it.

**Minimum versions.** Each minimum is the version verified on this machine (the homeserver, 2026-10-14), because the contract's tmux, git, gh and pi behaviour was verified on exactly these (contract.md, evidence labels). Older versions may work; that is unverified, so doctor refuses them.

| Tool | Command run | Output found on 2026-10-14 | Minimum | Parsed as | What cpd needs from it |
|---|---|---|---|---|---|
| git | `git --version` | `git version 2.43.0` | `2.43.0` | the first `x.y.z` | `worktree add`, `update-ref` with an old value, `merge-base --is-ancestor`, `merge-tree --write-tree --name-only` (git 2.38 or later, docs), `read-tree -m -u` (§7) |
| tmux | `tmux -V` | `tmux 3.4` | `3.6` | `x.y`, ignoring a letter suffix (`3.4a`); `next-…` builds pass | `new-window`/`split-window -e`, argv commands after `--`, `swap-pane`, `pane-border-lines`, `extended-keys`, `#{pane_dead_status}` (§6); **3.6 because** older tmux can lose a pane's `SIGCHLD`, so `pane-died` never respawns a cockpit view (contract §6.1; fixed in tmux 3.6, tmux/tmux#4559) |
| pi | `pi --version` | `1.2.0` | `>= 1.2.0 < 2.0.0` | `x.y.z` | `--fork`, `--session-id`, `-n`, `--exclude-tools`, `--no-skills`, `--approve`, `--no-mcp`, `--tools +name`; the catalog of §9 (cpd 1 §11: pi updates itself, so a range) |
| gh | `gh --version` | `gh version 2.45.0 (2025-07-18 Ubuntu 2.45.0-1ubuntu0.3)` | `2.45.0` | the first `x.y.z` | `pr create`, `pr list --json`, `pr view --json …statusCheckRollup…`, `api` (§7.6) |
| pi's Node | `<pi node> --version`, the binary §9 chooses | `v22.23.2` (`/usr/local/bin/node`; this machine has no `pi-node` directory) | none: it must run | | runs the catalog module of §9 |

**Deadline.** Each probe `doctor` runs (the five commands of the table, `<pi node> --version`, the catalog module of §9 and `gh auth status`) has a deadline of 10 s. A probe that runs past it is a failed check whose `message` names the tool, its argv and the deadline (contract §1.2 `tool_timeout`); `doctor` goes on with the next check and still reports every check.

Checks, in order:

| Name | Severity | Passes when |
|---|---|---|
| `config` | error | `config.toml` exists and is valid (§8) |
| `store` | error | the store verdict (§1.2 `--version`, contract §4.5) says the database opens and it migrates and passes the schema check (§4.3); otherwise `found` is the verdict's refusal (`code`, `message`). Unknown additive migrations (`store.unknown`) are not a failure and have no check of their own: `--version` reports them (hs-d178 D1) |
| `tamper` | error | the tamper report (§4.3) is empty; `found` lists it |
| `git`, `tmux`, `pi`, `gh` | error | found on `PATH` at or above the minimum above |
| `gh_auth` | warn | `gh auth status` exits 0 (needed only by `pr` commands; verified on 2026-10-14: logged in through `GH_TOKEN`) |
| `pi_node` | error | pi's Node runs |
| `tiers` | error | each model in the policy's `[model]` table is in pi's catalog (contract §8.3, §9) and `lead_tier`, `review_lead_tier`, `manager_tier`, `context_distill_tier` and `open_tier` are valid Tiers that obey the Tier rules (§9, hs-d562, pro-d82, hs-d554 and hs-d506: `context_distill_tier` `GX` only (hs-d562: the auxiliaries' Tier); `open_tier` a Tier a Worker may have (`H0`–`H3` or `S0`–`S3`); effort 0–3) |
| `holographic` | error | when `context_engine_enabled = true`, resolve the bridge script and the Python interpreter from the config (or their defaults, §8.2), run `{"cmd":"ping"}` under `context_engine_timeout_ms` and report the engine version; when `context_engine_enabled = false`, passes with `"disabled": true` (hs-d677); no index read or build |
| `pi_adapter` | error | the `pi_adapter` directory has a `package.json` |
| `worker_runner` | error | for `pi`, passes without looking at anything; for `durable`, passes when `<pi_adapter>/durable-worker/main.ts` exists and pi's Node is at least 22.19.0 (the `engines.node` of the runner's package, contract §6.12): `found` is the Node version, `expected` is `>= 22.19.0` (contract §3.4 "The Worker runner and its seam") |
| `host_runner` | error | when `lead_runner` and `manager_runner` are both `pi`, passes without looking at anything; when either is `durable`, passes when `<pi_adapter>/durable-host/main.ts` exists and pi's Node is at least 22.19.0 (contract §6.19): `found` is the Node version, `expected` is `>= 22.19.0` |
| `lead_packages` | warn | reports, for each `lead_pi_packages` entry and the adapter, whether a Lead gets it by `-e` or from which settings file (§6.5); warns when an entry is neither declared nor resolvable |
| `watch` | warn | the engine runs when any Plan is in `dispatch` or `deploy` |
| `tmux_restart` | warn | present only while `run/tmux-restart.pending` exists: it passes (`message` "tmux restart pending", `found` the times and versions) while the restart waits, a planned hold of E3 (contract §3.7) and not a failure; it fails, with the reason, when the restart failed (§13.3) |

```json
{"ok": true, "checks": [
  {"name": "git", "ok": true, "severity": "error", "found": "2.43.0", "expected": ">= 2.43.0"},
  {"name": "tmux", "ok": true, "severity": "error", "found": "3.4", "expected": ">= 3.4"},
  {"name": "pi", "ok": true, "severity": "error", "found": "1.2.0", "expected": ">= 1.2.0 < 2.0.0"},
  {"name": "gh", "ok": true, "severity": "error", "found": "2.45.0", "expected": ">= 2.45.0"}]}
```

Change from cpd 1: no `node`, `bd`, `wt` or `herdr` checks (§1.1); `gh` now has a minimum. Idempotent: yes; it writes nothing except a migration on first open.

### 14.2 `config`

```
cpd config [--migrate]
```

Without `--migrate`, prints the effective configuration: defaults merged with `config.toml` (§8), each value with its source. Refusals: `config_missing`, `config_invalid` (exit 1, `details.key` = the key path), among them the Tier rules of hs-d562, pro-d53, hs-d554 and hs-d506 (contract §8.2, §9 "Tier rules"): `lead_tier`, `review_lead_tier` or `manager_tier` with effort 5 or letter G, `context_distill_tier` must be GX (hs-d562), `open_tier` outside the Worker range (§9) and `worker_runner`, `lead_runner` or `manager_runner` other than `"pi"` or `"durable"` (contract §8.2: each key is validated and printed, with its source like every key; `lead_runner` and `manager_runner` default to `pi` and are independent, contract §6.19). Defaults: `lead_tier` `O2`, `review_lead_tier` `S3` (pro-d139), `manager_tier` `O4`, `context_distill_tier` `GX` (hs-d562: the Tier every distill run uses, one step only, no ladder, hs-d560), `context_curation_min_records` `25` (hs-d592: a Context is curable when it holds at least this many records; an integer >= 1), `loop_guard_turns` `20` (hs-d552, hs-d571: an integer >= 1; the window size for the loop guard's turn counter). `timeouts.explore_minutes` `20` (Plan `explore-roles`: the active-time limit of an explore Attempt, `scout` or `research`, used instead of `timeouts.attempt_minutes`; an integer >= 1, else `config_invalid` with `details.key`). It reads only the config files and never opens the store, so it never waits on the setup lock (hs-n278); `cpd tiers` and every command that opens the store wait out a migration in progress (contract §4.2). No effects.

With `--migrate`, reads `$CPD_HOME/config.toml`, applies the ordered, additive migrations (contract §8.1 "Config migrations", hs-d722; the migrations include `distill-tier-gx` and `drop-test-concurrency` (hs-d722, hs-d679) and a third migration (hs-d738)), validates the result (the same checks as plain `config` above) and writes the file only when it changed and only when the result is valid. The file is untouched on any refusal. Refusals: `config_missing` (exit 1) when the file is missing; `config_invalid` (exit 1, `details.key`) when the migrated file is still invalid after the migrations; any other code the migrations may return. It prints `{"ok":true,"path":"<path>","migrated":["<name>",...]}` with an empty array when nothing changed. Used only by `cpd install` before replacing the binary (contract §3.7 "The config migration and validation", commands.md §14.4 step 3).

```json
{"ok": true, "path": "/home/clayton/.cpd/config.toml",
 "config": {"version": 1, "machine": "hs", "concurrency": 3, "retry_cap": 2, "lead_tier": "O2", "timeouts": {"attempt_minutes": 30, "...": "..."}, "watch": {"tick_seconds": 5, "...": "..."}, "events": {"poll_ms": 200}, "tmux": {"socket": "cpd", "cockpit_session": "cpd", "work_session": "cpd-work"}},
 "sources": {"version": "file", "machine": "file", "host": "default", "concurrency": "default"}}
```

`sources` has one entry per leaf key. Idempotent: read-only.

### 14.3 `tiers`

```
cpd tiers
```

Kept from cpd 1 §7 and §11; output of §9.

| Precondition | Code | Exit |
|---|---|---|
| pi is found as §9 says | `pi_not_found` | 1 |
| the catalog module ran and printed valid JSON | `catalog_failed` | 1 |
| each configured model is in the catalog | `tier_model_unknown` | 1 |

It opens the store (the catalog cache), so it waits out a migration in progress (hs-n278; contract §4.2): one line on stderr, the one JSON value at the end and `lock_timeout` only past `MIGRATION_WAIT_CEILING`.

Effects: the cache is keyed by `install/current-version` and the fingerprint of `<agentDir>/models.json` (`models_sha`: lowercase hex SHA-256 of its bytes, `""` when the file does not exist; any other read error is `catalog_failed` with the path in `details`), so the key is `(pi_version, models_sha, letter, model)` (pro-d26, Plan bifrost, PR catfp; contract §4.4, §9). When either differs from the cached rows, one transaction inserting `tier_catalog` rows (append-only; old rows stay) (a cache: no Event). No tmux, no git.

```json
{"ok": true, "pi_version": "1.2.0", "tiers": {"O": {"model": "github-copilot/claude-opus-5.5", "efforts": [0, 1, 2, 3, 4, 5]}, "S": {"model": "github-copilot/claude-sonnet-5.5", "efforts": [0, 1, 2, 3, 4, 5]}, "H": {"model": "github-copilot/claude-haiku-4.5", "efforts": [0, 1, 2, 3]}, "G": {"model": "github-copilot/gemini-3.8-flash", "efforts": []}}}
```

The `efforts` above are an illustration, not verified values. Idempotent: yes.

### 14.4 `install`

```
cpd install --root <checkout>
```

Installs the cpd built from `<checkout>`: the binary and the pi adapter, then restarts what runs the old binary. The User's command at a shell, run by `scripts/install-cpd.sh` and, through it, by dotfiles' install and sync; no agent runs it and no cockpit action does (callers none, cockpit no, internal no: §2). `scripts/install-cpd.sh` is the bootstrap: `CPD_COMMIT=$(git rev-parse --short HEAD) scripts/cargo.sh build --release --locked -p cpd`, then `exec target/release/cpd install --root <checkout>`. `--root` is the checkout (a git worktree with `pi/`, `scripts/` and `core/src/store/migrations/`); the release binary is `<target>/release/cpd` (`$CARGO_TARGET_DIR` or `<root>/target`). The home is `$CPD_HOME` (default `~/.cpd`), the destination `~/.local/bin/cpd` and the adapter `~/.local/share/cpd/pi`.

Steps, in order (implementing.md "`cpd install`" has each with its message; contract §3.7 "The adapter swap" and "The install's engine order" fix the rules):

1. **Preflight.** `~/.local/share/cpd/pi` is absent, a symlink or a directory; else refused.
2. **The migration check.** `git fetch --quiet origin main` (a failed fetch refuses); every file of `core/src/store/migrations/` must be on `origin/main` with the same content; else refused, listing the names.
3. **The verdict of the built binary.** `<target>/release/cpd --version` must succeed, carry a `store` field and not say `store.opens` false; else refused with the refusal's message and the fix of its code. Nothing migrates.
4. **The config migration and validation.** `<target>/release/cpd config --migrate` reads `$CPD_HOME/config.toml`, applies the migrations (contract §8.1 "Config migrations", hs-d722; the migrations include `distill-tier-gx` and `drop-test-concurrency` (hs-d722, hs-d679) and a third migration (hs-d738)), validates the result and writes the file only when it changed and only when the result is valid. The file is untouched on any refusal. Prints `{"ok":true,"path":"<path>","migrated":["<name>",...]}` and each applied migration as a stderr line `cpd install: config.toml migrated: <name>`. Refusals: `config_missing` (the file is absent), `config_invalid` (the migrated file is still invalid, `details.key` names it). The migration happens before the binary is replaced, so that if it fails, the config is not mutated.
4a. **The adapter packages.** Runs after step 4 and before step 5, so a failure leaves the binary, the adapter link and the config as they are. It reads `<root>/package.json`. When the file is absent, or has no `dependencies` key or an empty one, it does nothing and runs no pnpm (today the root package.json has no `dependencies`, so no machine runs pnpm at install). Otherwise `<root>/pnpm-lock.yaml` must exist; else refused: `cpd install refused: package.json has dependencies but the checkout has no pnpm-lock.yaml`. It creates `~/.local/share/cpd` if needed, copies `package.json`, `pnpm-lock.yaml` and, when present, `pnpm-workspace.yaml` there byte for byte and runs `pnpm install --prod --frozen-lockfile` with that directory as working directory, bounded at 10 minutes, output captured. Result: `~/.local/share/cpd/node_modules` holds the production packages only (no devDependencies); the adapter's `pi.<id>` finds them (implementing.md "The adapter swap"). A non-zero exit, a timeout, or a pnpm that cannot start (not installed) refuses the install (the refusals table below). Idempotent: the same package.json and lockfile give the same result.
5. **The binary.** Copied to `~/.local/bin/.cpd.tmp.<pid>`, `chmod 755`, `rename`d over `~/.local/bin/cpd` (atomic, also while it runs).
6. **The adapter swap.** `pi/` minus `test/` is built as `pi.tmp.<pid>`, renamed to `pi.<id>`, linked as `pi.link.<pid>` (a relative symlink) and `rename`d over `pi` in one `rename(2)`; a real `pi/` directory is first renamed to `pi.legacy` (the one non-atomic moment, restored if the swap fails); `pi/package.json` must exist; then `pi.<id>` and `pi.legacy` that are neither current nor displaced, are not protected by a live pin (`pi.pin.<pid>` of a live pid, whose target is a version name; contract §3.7 "The pin") and are older than 60 minutes and `pi.tmp.<pid>` and `pi.link.<pid>` of dead pids and the pins of dead or unparseable pids, are removed (a failure only warns); the cleanup never touches `<share>/node_modules`, `<share>/package.json` or `<share>/pnpm-lock.yaml`. Prints `installed <bin> and <link>` on stderr.
7. **The follow-ups, on the new binary:** `cpd config`; `cpd watch --check` (§12); `cpd tui --respawn` (§13.1) when the cockpit has its `main` window; when the work session has its `watch` window, `cpd watch --mark-restart` (§12), `tmux kill-window -t =<work>:watch`, the wait for `locks/watch.lock` to be free (0.25 s steps, `CPD_INSTALL_LOCK_WAIT_SECS`, default 60), `cpd watch --ensure` and the check that the lock is held. A failure of `config` or `watch --check` leaves the engine and the views alone and skips the rest; any other failure prints and goes on. Each failure is one line `cpd install follow-up failed: <text>` on stderr (texts: contract §3.7).
8. **The tag.** `scripts/tag-version.sh` (docs/versioning.md): a failure warns with `tag-version:` and does not change the exit.

| Precondition | Code | Exit |
|---|---|---|
| `--root` is given | `usage` | 2 |
| Step 4a: `<root>/package.json` has `dependencies` and `<root>/pnpm-lock.yaml` is absent: `cpd install refused: package.json has dependencies but the checkout has no pnpm-lock.yaml` | `install_failed` | 1 |
| Step 4a: `pnpm install --prod --frozen-lockfile` exits non-zero, times out (10 minutes) or cannot start (pnpm not installed): `cpd install refused: pnpm install --prod --frozen-lockfile failed in <share>:`, then its captured stdout and stderr, trimmed (for a pnpm that cannot start, the error) | `install_failed` | 1 |

Every refusal (steps 1 to 4a, nothing replaced), failure (steps 5 and 6) and failed follow-up (step 7) is a message on stderr as above and **exit 1**; the exit is 1 at the end when any follow-up line was printed.

Output: the install's progress and every message above go to stderr as human text, as the script's did (a Lead's commit hooks and dotfiles read the exit code and the text, not JSON); the one JSON value on stdout is `{"ok": true, "binary": "<path>", "adapter": "<path>", "version": "<cpd --version of the built binary>"}` on success and the error value (§1.2) with exit 1 otherwise (the messages are the script's own; every refusal, failure and failed follow-up is `install_failed`, exit 1). Effects: writes only `~/.local/bin`, `~/.local/share/cpd` and what the follow-ups write (`run/install-restart.json`, the engine's own files); no row, no Event. Idempotent: yes, safe to rerun, also by two installs at once (contract §3.7 "The adapter swap"). The command-tree test `cli/tests/tooling/interface.rs` lists it (callers none, cockpit no, internal no); `docs/interface.md` is regenerated from the tree.

### 14.5 `models`

```
cpd models refresh
cpd models show [--provider <provider>]
```

The model lists of the providers pi can use (contract §3.7 E15, Context decisions pro-d13, pro-d17, pro-d20). **cpd has no provider list and names no key**: the providers come from pi, through the helper `pi/lib/models-list.ts` and `--provider` only names one of the files already cached. Callers: Manager, Lead, or the User at a shell (§2). Neither command opens the store, writes a row or appends an Event.

**`models refresh`** reads the lists now, whatever their age. It runs the helper exactly as E15 does (no argument, under pi's Node, its own process group, a deadline of 120 s, the engine's environment inherited; contract §3.7 E15 for the answer, its validation, the error codes and the file), then, under `locks/models.lock`, computes each provider's diff and writes `run/models-<provider>.json` whole (`.tmp` and rename), by the same code as the engine, so a refresh and the engine never interleave. A provider whose list is empty or failed writes nothing and keeps its last good file. A first run, or a run after a change of `auth`, reports no change.

| Precondition | Code | Exit |
|---|---|---|
| pi is found as §9 says | `pi_not_found` | 1 |
| the helper starts | `spawn_failed` | 1 |
| the helper answers within 120 s | `tool_timeout` | 5 |
| the answer is the JSON of contract §3.7 E15 | `models_helper_failed` | 1 |
| pi can use a provider the contract lists | `models_no_provider` | 1 |
| the helper's whole run did not fail | the helper's code (`models_refresh_failed`, …) | 1 |
| every provider listed has a good, non-empty list | the provider's code (`models_key_unset`, `models_unauthorized`, `models_unreachable`, `models_tls`, `models_timeout`, `models_unparseable`, `models_empty`, `models_refresh_failed`) | 1 |
| `locks/models.lock` is free within `store_busy_ms` | `lock_timeout` | 5 |

When one provider fails and others do not, **the good ones are written first and the command then fails** with the failing provider's code (the first, in the helper's order) and `details.providers`: the same array as `ok`'s, a failed entry being `{provider, error: {code, message, details}}`. Nothing is hidden and nothing good is lost. No key is ever in an output, a message or `details`.

Effects: `run/models-<provider>.json` written for each provider that has a good list; the helper's own effect on pi (`models-store.json`'s `checkedAt` for `github-copilot`). No row, no Event, no tmux, no git. The engine's next E15 is not due for `watch.models_discovery_seconds` after the newest `polled_at`.

```json
{"ok": true, "pi_version": "1.3.0", "providers": [
  {"provider": "bifrost", "polled_at": "2026-11-02T09:00:00Z", "count": 41, "changes": {"at": "2026-11-02T09:00:00Z", "appeared": ["acme/new-model"], "went_away": []}},
  {"provider": "github-copilot", "polled_at": "2026-11-02T09:00:01Z", "count": 11, "changes": null}]}
```

`changes` is the diff of this run when it is not empty, else `null` (a list that did not change, a first run and a change of `auth` all print `null`). The ids above are an illustration, not verified values. Idempotent: yes (the same lists write the same ids and move `polled_at`).

**`models show`** prints the cached lists and runs nothing (no helper, no network, no lock). `--provider <provider>` prints only that provider's file. A provider with no file and a home with no file at all, is an empty `providers` array, not an error. A file it cannot parse is skipped and named in `warnings` (`{provider, file, code: "models_unparseable"}`); the command still succeeds.

| Precondition | Code | Exit |
|---|---|---|
| `--provider` is a file-name-safe token (`^[A-Za-z0-9._-]+$`) | `usage` | 2 |

```json
{"ok": true, "providers": [
  {"provider": "bifrost", "polled_at": "2026-11-02T09:00:00Z", "age_seconds": 3600, "pi_version": "1.3.0", "auth": "custom", "count": 41, "ids": ["…"], "changes": {"at": "2026-11-02T09:00:00Z", "appeared": ["acme/new-model"], "went_away": []}}],
 "warnings": []}
```

`ids` is the file's sorted list, `count` its length, `auth` the file's `source.auth`, `changes` the file's last `changes` (`null` when it has none), `age_seconds` the time since `polled_at`, at the call. The providers are in name order. Idempotent: yes, read-only.

### 14.6 `preset`

```
cpd preset list
cpd preset show <name>
cpd preset apply <name>
```

The tier-preset switch (D1–D5 of Plan `tier-presets`): a named set of four model refs — the `O`, `S`, `H` and `G` keys of a `[presets.<name>]` table of `$CPD_HOME/config.toml` (contract §8.1, §8.2) — applied to the machine by rewriting the `[model]` table of its host policy. `list` and `show` are read-only and their callers are `[Manager, Lead]`; `apply` writes and its caller is `[Manager]` (§2). None of the three writes `config.toml`: presets are the User's, managed by dotfiles. A provider is the text before the first `/` of a ref. `apply` writes a single-provider table (step 4), as contract §8.3 "One provider per table" holds of any written model table.

**`preset list`** prints every preset with the provider of its `O` ref. No precondition, no effect (one read of `config.toml`, no store):

```json
{"ok": true, "presets": [
  {"name": "fast", "provider": "github-copilot"}]}
```

`presets` is sorted by name, `[]` when the config has none. Idempotent: read-only.

**`preset show <name>`** prints the preset's four refs and the current host `[model]`:

| Precondition | Code | Exit |
|---|---|---|
| `<name>` is a `[presets.<name>]` table of `config.toml` | `preset_unknown` | 4 |

```json
{"ok": true, "name": "fast",
 "O": "github-copilot/claude-haiku-4.5", "S": "github-copilot/claude-sonnet-5.5",
 "H": "github-copilot/claude-haiku-4.5", "G": "github-copilot/gemini-3.8-flash",
 "host": {"O": "github-copilot/claude-opus-5.5", "S": "github-copilot/claude-sonnet-5.5",
          "H": "github-copilot/claude-haiku-4.5", "G": "github-copilot/gemini-3.8-flash"}}
```

`host` is the machine scope's `[model]` refs read from `$CPD_HOME/policy/host.toml`, or the built-in default of contract §8.3 when the file has no `[model]` table. No registry is read and nothing is validated: `show` reports, it never checks. Idempotent: read-only.

**`preset apply <name>`** validates the preset and, when it differs from the current host `[model]`, rewrites the `[model]` table of `$CPD_HOME/policy/host.toml`, then records the live Sessions whose model changed. Steps, in order; **every refusal happens before any write: nothing is written and no Event is appended**:

1. **The name.** `<name>` must be a `[presets.<name>]` table of `config.toml`, else `preset_unknown` (exit 4).
2. **The host file.** `$CPD_HOME/policy/host.toml` must exist and hold a `[model]` table, else `policy_invalid` (exit 1, `details.file`, `details.key` `model`): there is nothing to rewrite.
3. **Equality.** When the preset's four refs equal the current host `[model]` refs of step 2, `apply` is a no-op: no write, no Event, `"already": true` (idempotent).
4. **The one-provider invariant.** The preset's four refs must have one provider, the invariant contract §8.3 holds of a written model table; else `policy_invalid` (exit 1, `details.file`, `details.key` `model`), before anything external runs.
5. **The registry** (D3). The command lists pi's **full** registry on this machine — every provider pi can use, the built-in providers included, each with its offered model ids — by running the model-discovery helper (`pi/lib/models-list.ts`, the helper §14.5's `models refresh` and the engine's E15 run with no argument) in its `--all` mode, through the host runner, under pi's Node, in its own process group, with the deadline of `models refresh`'s (120 s). **The cached `run/models-<provider>.json` files are never the source**: they exclude the built-in providers, so the registry is always listed at apply time. The registry listing's own failures are the helper's codes (`pi_not_found`, `spawn_failed`, `tool_timeout` exit 5, `models_helper_failed`, …), writing nothing. Then:
   - a ref whose provider is not in the registry refuses `preset_provider_unconfigured` (exit 3);
   - a ref that is not among its provider's offered ids refuses `preset_model_unoffered` (exit 3).
6. **The write** (D4). The `[model]` table of `host.toml` is rewritten: the section header `[model]` and its `O`/`S`/`H`/`G` lines are replaced by the preset's four refs, in that order, and every other byte of the file is unchanged. The write is atomic: a temporary file in the same directory, then a rename over `host.toml` (`write_atomic`). `host.toml` stays the **only** place `[model]` is written (pro-d25): no owner or repo policy file and no dotfiles template.
7. **The live Sessions** (D5). After the write, the command recomputes the model each live (not archived) Lead and Manager Session resolves at its tier's letter — the `model` of `cpd session show` (§10.2), i.e. the ref contract §8.3's resolution gives the letter of the Session's stored tier, so a letter whose effective ref an owner or repo file overrides is unchanged. For every Session whose model differs under the pre-write and the post-write host `[model]`, ONE `session-model-changed` Event is appended, scoped to it (`context` and `plan` when it has them; `data` `{from, to}` = the resolved refs under the pre-write and the post-write `host.toml`), all in one store transaction, in the same style as `session-retiered` (contract §5.2, §5.3). A Session whose letter's model is unchanged gets none. Nothing else is written for Sessions.

| Precondition | Code | Exit |
|---|---|---|
| `<name>` is a `[presets.<name>]` table of `config.toml` (step 1) | `preset_unknown` | 4 |
| `host.toml` exists and holds a `[model]` table; `details.file`, `details.key` `model` (step 2) | `policy_invalid` | 1 |
| the preset's four refs are one provider — the invariant of contract §8.3 for the written table; `details.file`, `details.key` `model` (step 4) | `policy_invalid` | 1 |
| the registry can be listed: pi is found as §9 says | `pi_not_found` | 1 |
| the registry can be listed: the helper starts | `spawn_failed` | 1 |
| the registry can be listed: the helper answers within 120 s | `tool_timeout` | 5 |
| the registry can be listed: the answer is the JSON of contract §3.7 E15, in `--all` mode | `models_helper_failed` | 1 |
| the preset's provider is in the registry (step 5a) | `preset_provider_unconfigured` | 3 |
| every ref is among its provider's offered ids (step 5b) | `preset_model_unoffered` | 3 |

`apply` opens the store (it appends the Events of step 7; the no-op of step 3 opens nothing and writes nothing), so the write path waits out a migration in progress as every store-opening command does (contract §4.2). The file write of step 6 is not a store write: only the Events of step 7 write rows, each with the usual `txns` and `history` rows of a transaction.

```json
{"ok": true, "preset": "fast", "host": "/home/clayton/.cpd/policy/host.toml",
 "already": false,
 "changed": [
   {"session": "596d007f-…", "role": "lead", "from": "github-copilot/claude-opus-5.5", "to": "github-copilot/claude-sonnet-5.5", "event": 48220}]}
```

`host` is the path of the rewritten file; `changed` lists one entry per Session whose model changed, with the Session's id and role and its Event's `from`, `to` and `event` (the Event's id). When step 3 finds the host already on the preset, the output is `{"ok": true, "preset": "fast", "already": true, "changed": []}`. Idempotent: yes — a rerun whose preset already equals the host `[model]` is the no-op above.

## 15. The pi adapter's protocol

The pi adapter is the TypeScript package `pi/` (decisions: "the pi adapter stays TypeScript"), loaded in every pi session cpd starts (§6.5). It is the only pi-specific code: it turns Events into messages (§5.3), reports agent states (§3.6) and runs the Plan review in the Lead's chat (§15.5). It calls only `cpd` commands; it never reads the database and never types into a pane (cpd 1 §15, kept).

### 15.1 Package and activation

- **Package:** `pi/package.json` with `"keywords": ["pi-package"]` and `"pi": {"extensions": ["./extension.ts"], "skills": ["./skills"]}`; host packages are `peerDependencies: "*"` (cpd 1 §15, kept). The skill `cpd-lead` is the cpd 2 skill.
- **Location:** `pi/`, installed to `~/.local/share/cpd/pi` by `cpd install` (§14.4). (History: before the switch-over it lived beside cpd 1's `pi/`, in a second directory.)
- **Activation:** at pi's `session_start`, only when `ctx.mode` is `tui` and `CPD_SESSION` is set. Change from cpd 1: cpd 1 activated in any herdr or tmux pane (`HERDR_ENV`, `TMUX_PANE`); now cpd marks its own panes with `CPD_SESSION` (§6.5), so a pi the User starts by hand is left alone.
- **The empty last row** (User, hs-d567; contract §6.1 "The blank row", Q2-Q3): in its `session_start` handler (`pi/lib/adapter.ts`), the adapter calls `ctx.ui.setStatus("cpd-pad", "")` (`ctx.mode` `tui`; it knows nothing of the chat slot, so a parked pane has the extra empty row too). pi's footer draws one more line, empty, when its extension statuses are not empty, so pi's last row is empty; a `/reload` runs `session_start` again and sets it again and a resize needs nothing (verified in real pi, contract §6.1 evidence). The key `cpd-pad` is ours; another extension's status text would share the line.
- **Identity:** `CPD_SESSION` must equal `ctx.sessionManager.getSessionId()`. If not, the adapter delivers one error message (below) and stays inactive: it would otherwise report another Session's state.
- **Calls:** one-shot commands through `pi.exec("cpd", args)`, whose stdout is parsed as the JSON envelope at the boundary (a non-JSON stdout, or `ok: false`, is an error with the code). Each call passes a timeout of 30 s and the abort signal of the adapter's run (aborted at `session_shutdown`); a call that times out or is aborted is reported like a failed call (the error delivery below, code `tool_timeout` for a timeout). The follower is a child process (`node:child_process` `spawn`, argv, stdin `/dev/null`) read line by line. Every argv the adapter builds is one cpd's own grammar accepts (the clap tree of `cli/src/cli.rs`, as `cpd <command> --help` shows it): a flag where the command takes a flag, a positional only where it takes one. A `usage` refusal of an argv the adapter built is an adapter defect, never a condition: it is reported like any adapter error (the error delivery below), and a read that fails with it never renders a polite line in a message (§15.7 (4)). The guard is `docs/implementing.md` "The fake cpd refuses what cpd refuses".
- **Errors** (hs-d552; the adapter-only codes `protocol_mismatch`, `mcp_register_failed` and `skills_unreadable` are delivered by this rule, contract §1.2; `mcp_register_failed` and `skills_unreadable` are also reported, below) for a **Manager** or **Worker**: delivered to the Session as `pi.sendMessage({customType: "cpd", content: "cpd adapter: <what>: <code>: <message>", display: true, details: {kind: "error", code, …}}, {triggerTurn: false, deliverAs: "followUp"})`: no turn started, shown as a follow-up. An error identical to the last one shown in this Session (same `what` and `code`) is not shown again until a different error has been shown; one memory per Session, cleared at reload. For a **Lead** (hs-d573 cause A): one line appended to the adapter log `$CPD_HOME/run/adapter/<session>.log` (15.6 'Log'): `<time> error: <what>: <code>: <message>`, and reported to the Manager with `cpd session report` (§10.18), once per (`what`, `code`) by memory (`host.report`): the same error again is neither logged again nor reported again until a different one has been. The Manager's error Event carries `session` = the reporting Session (DEFECT 1: so the Manager can see which Session failed), and `plan` = null, so the Lead does not receive its own error back (§11.2). A failed report is one log line, as L4 below says. The adapter never throws into pi.
- **A refused MCP registration** (pro-d226; `mcp_register_failed`, §5.3, §15.3), for **every role**. **L1:** it is reported with `cpd session report --what 'mcp attach <name>' --code mcp_register_failed --message <pi's message> --details {"name","message"} --session <id>`, one `error` Event; a Manager or a Worker also gets the chat message above, and a Lead gets its log line (above) and no chat. **L2:** `what` is `mcp attach <name>`, which names the server. A refused registration is **never deduplicated**: `mcp_register_failed` bypasses `host.report`'s memory (`lastError`) in both its deliveries, the chat message and the report (and a Lead's log line), and it neither sets nor clears that memory, so the dedupe of every other error is unchanged. The same server refused twice in a row is two reports, and so are two servers refused in a row. Each report is tied to one Event id (handled at most once per run) or to one start, so it cannot flood. **L3:** the registrations of the replay at start (§15.2 step 5a) are reported the same way, one report per refused server. **L4:** a failed `session report` call is one log line, `report failed: <code>: <message>` (§15.6 'Log'), never delivered and never reported again, for every role (no recursion); the next refusal tries again. **L5:** only `mcp_register_failed` skips the memory. `skills_unreadable` (pro-d260, §15.23) is the second code reported to the ledger for every role, and, unlike `mcp_register_failed`, it is not exempt from the memory. Every other adapter error keeps its role's delivery and its memory exactly as above.

### 15.2 Start-up and the cursor

At `session_start`, in this order:
0. **Protocol check** (hs-a4): `cpd --version`, whose `protocol` (§1.2) is compared with the number the adapter was built for (5 today). Equal: nothing is said. Different: one message to the Session, `cpd adapter: protocol mismatch: protocol_mismatch: cpd speaks protocol N, this adapter M; reload pi (/reload)`, with `details: {kind: "error", code: "protocol_mismatch", cpd: N, adapter: M}`, delivered like the other errors (§15.1), once per installed cpd binary (the identity of the file `cpd` resolves to), not once per follower restart. The adapter keeps following Events either way; it never types into a pane. A `--version` that fails or lacks `protocol` is an ordinary error with its code, once per error code.
1. `cpd session show <id>`: role, Plan, Context, `plan_lead`, `registered_event`, name. A failure is delivered as an error message once per error code and the call is retried with the follower's back-off (1 s, doubling to at most 30 s) until it succeeds; the adapter does nothing else until then and `session_shutdown` ends the retries. A refusal that cannot pass (`usage`, `session_required`, `invalid_session_id`, `session_ambiguous`, `session_not_found`, `plan_not_found`, `context_not_found`, `cursor_ahead`) is delivered once and not retried: the adapter stays inactive.
2. `cpd session state idle --session <id>` (`stopped → idle`, §3.6); then `working` if `ctx.isIdle()` is false.
3. **Name:** if pi's session name differs from the Session's `name`, `pi.setSessionName(name)`: a `session-renamed` missed while pi was down still applies.
4. **Cursor** (§5.3 "No loss, no duplicates"): the greater of `registered_event` and the **highest** `details.id` of the `cpd` custom messages in the session's entries whose `details.delivered_to` is this Session's id (not the last: delivery is not in id order, `pr-feedback` is steered and the rest are follow-ups). The adapter puts `delivered_to` into every message it delivers (amendment 5, §19). Why: a fork's file starts with its source's messages, whose Event ids belong to the source's stream; counting them could skip Events addressed to the fork.
5. **Blocking reasons** (Plan Lead only; a Manager has none): one per `open` Escalation of its Plan (`cpd plan show`), and one for the Plan's **latest** Plan review when it is pending and this session opened it (its `cpd-plan-review` entries of either surface, §15.5; a pending chat entry also reopens its dialog, §15.5 "Recovery"): the latest is the one `cpd plan show` reports (`review.id`), and an undecided older review never opens a reason (hs-d224).
5a. **MCP replay** (pro-d226). pi keeps a registration only in memory, so the adapter registers the Session's attached servers again at every start. It makes ONE call, `cpd events list --session <id> --kind mcp-attached --limit 5000` (an existing command with existing flags: `protocol` is unchanged). The read is the full history: the staleness filter of pro-d214 applies only to `events follow`, never to this read, and the read does not depend on the cursor. It keeps the Events whose `session` is its own id; for each distinct `data.name` it keeps the Event with the highest id; and it registers those with `attachServer` (contract §6.11) in ascending id order, using the entry exactly as the Event holds it (a later edit of the machine policy applies at the next `cpd mcp attach`, never at replay). It runs at every `session_start` (startup, resume, new, fork, reload). A fork has none, because its Events name the source Session. A failed read (`events list` fails, or its output is not a list of valid `mcp-attached` Events) is one adapter error (§15.1) with what `events list (mcp-attached)` and the code of the failure: start-up goes on with no replay and no retry, and the next start reads again. A page of exactly 5000 Events is the adapter error `mcp_replay_truncated`: it registers nothing and counts as a failed read, because a page is the oldest 5000 and its latest Event per name could be missing. A server pi refuses during replay is reported as §15.1 'Errors' and §15.3 say for a live Event (L3, pro-d226): one `mcp_register_failed` error Event per refused server, for every role, never deduplicated, and the other servers still register.
5b. **Skill check** (pro-d260; a Lead or a Manager only): the adapter compares its skill's text with the Session's baseline and, when it changed, tells the Session at its next turn, as §15.23 says.
6. **Follow:** spawn `cpd events follow --session <id> --since <cursor>`.

The cursor in memory advances after each line is handled, delivered or not. Only delivered messages persist it. The Run also remembers the Event ids it has delivered and skips a repeated one. Step 5a (pro-d226) adds every own `mcp-attached` Event that its read returned to that memory: each counts as handled for this Run, whether it registered or was superseded by a later Event of the same name, so the follower's later delivery of the same Event id (it restarts from a cursor that delivered messages keep below the attach Event) registers nothing. An Event id the replay did not read registers as today, including a re-attach of a name already registered (pi replaces the same name, and pi's MCP extension reconnects a replaced server), so `cpd mcp attach` again stays the recovery for a server that is missing. After a restart the Events handled without a message (`escalation-answered`, `escalation-expired`, `session-renamed`, `plan-review`) are seen again; every handler below is idempotent for that reason.

**The follower ends** (EOF, exit, or a failure line): the adapter restarts it with `--since <in-memory cursor>` after 1 s, then backing off to at most 30 s. A failure line or a non-zero exit is delivered as an error message once per error code, until a line is read again. A failure line whose code is one of the refusals of step 1 is permanent: it is delivered once and the follower is **not** restarted (every other code, for example `store_busy` or `store_too_new`, is retried without end). A well-formed Event (§5.2 fields) of a kind the adapter does not know is skipped silently: no message and the cursor moves past it (an adapter that does not know a kind cannot need it, so an install that adds a kind does not flood older adapters). A malformed line (not JSON, or §5.2 fields missing or of the wrong type) is delivered as an error message and skipped and the cursor moves past it, so one bad line cannot stop delivery. Before it spawns the follower again, the adapter repeats the protocol check of step 0; the message is sent only when the installed binary is a different one from the last that mismatched.

**store_too_new when the installed cpd is newer** (hs-d7): at each follower run start, the adapter records the identity of the file `cpd` resolves to (the installed cpd identity, from `installedIdentity()` in pi/lib/cpd.ts). A failure line whose code is `store_too_new` is **not delivered** if the installed cpd identity differs from the identity recorded at this follower's start: the follower was running the old cpd binary when it was started and a newer cpd is now installed. In that case, the follower is restarted as usual (which runs the new cpd from PATH) and the restart delay resets to 1 s. When the installed cpd identity is the same as the one recorded at the follower's start (no install has happened since), `store_too_new` is delivered as today: once per error code, until a line is read again (the code is retried without end, as per the second sentence).

At `session_shutdown` the follower is killed.

### 15.3 What each Event does

Events arrive in id order; the follower already applied §11's `--session` rule and dropped the Events whose condition no longer held (§11.2) and an id already delivered in this activation is never delivered twice, so a Lead receives its Plan's Events and a Worker or side fork only those addressed to it. Delivery is `pi.sendMessage({customType: "cpd", content: event.text, display: true, details: {...event, delivered_to: <id>}}, {triggerTurn, deliverAs})` (§5.3), where `deliverAs` is `"steer"` for the **urgent kinds** and `"followUp"` for every other message (below).

| Kind | Plan Lead | Worker, side fork |
|---|---|---|
| `job-merged`, `job-failed`, `plan-done`, `error` | message, **turn**, `followUp`. The `job-merged` of an `open` Job is the Lead's word that the pull request is open (`data.number`, `data.url`, hs-d340) | |
| `all-merged` | ignored: no longer appended (hs-d340, contract §3.7 E4); the adapter still knows the kind, because an old Event may be read | |
| `error` with code `pr_start_failed` | the Lead answers with `cpd pr start <key>` once the cause is fixed | |
| `pr-feedback` (every `reason`: checks failed, changes requested, closed) | message, **turn**, **`steer`**. Carries the PR's key in `data.pr` | |
| `attempt-retry`, `job-cancelled` | message, no turn, `followUp` | |
| `pr-started`, `pr-merged` | message, no turn, `followUp` (R34: a started PR's Jobs are now scheduled; a merged PR may start the PRs that need it, §5.3) | |
| `pr-base-changed` | message, no turn, `followUp` (informational, hs-d325: the PR's `base` followed GitHub, as after its base PR merged; no Gate is needed and the PR stays ready, hs-d326). `data` is in contract §5.2. Raises `protocol` to 5 | |
| `escalation` | message, **turn**, **`steer`**; opens the blocking reason `escalation:<id>` labelled `φ: Worker question` | |
| `escalation-answered`, `escalation-expired` | closes `escalation:<id>`; no message | |
| `plan-review` with `status: pending` | opens it in the Lead's chat (§15.5): the summary as a `cpd-plan-summary` message (no turn) and the dialog; no `cpd` message for the Event itself. A review with a higher id than another pending review of this session **closes** that one's `plan-review:<review>` reason: only the latest review of a Plan blocks the Lead (§15.5 "Latest only") | |
| `plan-review` with `status: approved` or `rejected`, for a pending entry of this session | a decision made outside this adapter (§15.5 "Deciding elsewhere"): closes `plan-review:<review>` and an open dialog of it; message `cpd-plan-decision`, **turn**, `followUp` (own custom type, not `cpd`). For any other review: ignored | |
| `plan-review` with `status: superseded` | closes `plan-review:<review>` and an open dialog of it (the Plan is done or cancelled, §3.1); **no message, no turn**. The reason closes by the normal E8 rule (contract §3.7 E8) when the Inbox item that followed the pending status closes | |
| `plan-outlined` | ignored: **no message, no turn** (the Lead wrote the outline itself, §4.13); it only **learns the kind** (`pi/lib/event.ts`, delivery `none`), because an unknown kind costs an error turn (hs-n122). Adding the kind raises `protocol` to 10 (contract §5.2) | ignored |
| `session-renamed` (`session` = itself) | `pi.setSessionName(name)` and `ctx.ui.notify("cpd: session renamed to \"<name>\"")`; no message, no turn | same |
| `attempt-nudge` (`session` = itself) | | message with `data.prompt` as content, **turn** |
| `session-message` (`session` = itself) | message, `followUp`, **a turn always, whatever `data.wake`** (hs-d514, hs-n406); for every Lead (Plan Lead, side fork) and the Manager (the `--session` rule delivers Events addressed to a Session to it). Content `Message from <sender> (HH:MM ZZZ): <text>`, `<sender>` = the sender Session's name and id (`<name> (<id>)`), or `the User` when `data.from` is null. The time is the local time when the message was sent (format §1.3): `HH:MM ZZZ`, 24-hour clock, zero-padded, no seconds, one space before ZZZ where ZZZ is the zone abbreviation (letters only) or numeric offset (§1.3); rendered from the stored `at` (UTC, ISO 8601) by the pi adapter from `pi/lib/time.ts`. Example for an instant 2026-10-25T22:29:00.000Z in America/Toronto: `Message from the User (18:29 EDT): ...`. An idle Lead starts a turn at once; a working, waiting or blocked one gets it after its running turn, never interrupted | |
| `session-resumed` (`session` = itself) | for its own Session while tripped by the loop guard: the adapter clears the trip (L5), appends the pi session entry `{tripped: false}`, and delivers **one message** with a turn (the Event's text from `session resume`, `followUp`). While not tripped, does nothing (no message, no turn). For every Lead and a Manager. When the Session reloads, the trip survives (L7) | same | **the switch** (hs-d506, §10.15): no message, no turn. The adapter notes that its Session's Tier changed; at its next idle (right away when it already is idle, as `agent_settled` and `ctx.isIdle()` say it) it reads `cpd session show` for `model` and `thinking`, finds the model with `ctx.modelRegistry.find(provider, id)` and calls `pi.setModel(model)` and `pi.setThinkingLevel(thinking)`, once. Not while `working`, `waiting` or `blocked`. A model the registry lacks, or a `setModel` that answers false (no authentication for its provider), is one error message (§15.1, delivered as every adapter error is, with a turn: the Session tells the User; code `model_unavailable`, `details: {model, thinking}`) and sets nothing, not even the level; the next `session-retiered` tries again. The same for a failed `session show`. The adapter names no record id and reads no config: the mapping is `session show`'s. For every Lead and a Manager (a Worker never gets one). Adding the kind raises `protocol` | same |
| `session-model-changed` (`session` = itself) | **the switch of the model** (the tier-preset switch, §14.6), the same path as `session-retiered`'s: no message, no turn. At the Session's next idle (right away when it already is idle, as `agent_settled` and `ctx.isIdle()` say it) the adapter reads `cpd session show` for `model` and `thinking`, finds the model with `ctx.modelRegistry.find(provider, id)` and calls `pi.setModel(model)` and `pi.setThinkingLevel(thinking)`, once, never while the Session works, waits or is blocked. A model the registry lacks, or a `setModel` that answers false (no authentication for its provider), is one error message (§15.1) and nothing is set; the next `session-model-changed` tries again. For every Lead and a Manager (a Worker never gets one). Adding the kind raises `protocol` to 18 | same |
| `mcp-attached` (`session` = itself) | **the registration** (pro-d8, contract §5.3): no message, no turn. The adapter calls `pi.registerMcpServer(data.name, data.server)` (`attachServer`, contract §6.11). An Event for another Session registers nothing. The registration survives a reload and a restart through step 5a of §15.2 (pro-d226): an own Event that the replay read registers nothing when the follower delivers it again (the cursor counts delivered messages only, §15.2), and a new Event always registers, so `cpd mcp attach` again is still the recovery for a server that is missing. A registration pi refuses (it throws) is one error per refusal (pro-d226; §15.1 'Errors'; `what` `mcp attach <name>`, code `mcp_register_failed`). For **every role** the adapter runs `cpd session report --what 'mcp attach <name>' --code mcp_register_failed --message <pi's message> --details {"name","message"} --session <id>`, so the ledger holds one `error` Event for it. A Manager's or a Worker's adapter also delivers it in chat, `details: {kind: "error", code, name, message}` with pi's message. It is exempt from the once-per-(`what`, `code`) memory of both deliveries, so each refusal is delivered and reported, the same server twice in a row included, and it neither sets nor clears that memory (L2). A failed report is L4 of §15.1; every other adapter error keeps its delivery and memory (L5). With no such Event the adapter registers no server. For every role. Adding the kind raises `protocol` | same |
| `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived` | ignored: no message, no turn | ignored |
| `session-titled` | ignored: **no message, no turn** (hs-d257): the pane header is set by `session retitle` itself, through tmux (§10.14), so the adapter has nothing to do; it only **learns the kind** (the list of kinds in `pi/lib/event.ts`, with delivery `none`), because an unknown kind costs an error turn (hs-n122). Adding the kind raises `protocol` (contract §5.2, "The protocol number") | ignored |
| every other kind | ignored | ignored |

**An explore Job's `job-merged`** (Plan `explore-roles`, R9). The Plan Lead gets it as any `job-merged` does: a message, with a turn, `followUp`. `data.turn` is always true, so the Lead learns of it. Its text is exactly three lines, built in Rust with the other `job-merged` texts (core/src/attempt/ledger.rs):

```
Job <key> (<kind>) merged: findings in $CPD_HOME/run/<plan>/explore/<key>.md.
Summary: <summary>
Next: cite that path in the Specs that need it; add another explore Job when the summary is not enough. cpd job show <key> --plan <plan>
```

The Event's `data` carries `summary` and `artefact` (the path relative to `$CPD_HOME`) besides `sha`, `tier` and `kind`. The Lead gets the summary and never the file. The adapter renders `event.text` unchanged and reads neither field, so the protocol number does not change.

**Steered and follow-up** (User, hs-d276; evidence hs-n241). Two kinds are urgent and are delivered with `deliverAs: "steer"`: **`pr-feedback`, whatever its `reason`** and a Worker's **`escalation`**. pi injects a steering message after the current tool call instead of after the run (pi 1.3.0 `dist/core/agent-session.d.ts` `sendCustomMessage(…, {deliverAs: "steer" | "followUp" | "nextTurn"})`; `pi.sendMessage` takes the same option, `dist/core/extensions/types.d.ts` `ExtensionAPI.sendMessage`). An idle Lead is unchanged: `triggerTurn` starts a turn. **Every other message the adapter delivers is a `followUp`**: the Event kinds above, the adapter's errors (§15.1), `attempt-nudge` and the `cpd-plan-decision` message. The same rule holds for a Manager, if it is ever sent one of the two urgent kinds (today it gets neither, §11). Why: every Lead message used to be a follow-up and pi's default `followUpMode` `"one-at-a-time"` delivers one follow-up per finished run, oldest first, so a busy Lead got PR #80's conflict notice 19 minutes late and a `job-merged` 56 minutes late (hs-n241).

**Other notices** stay follow-ups, delivered as the User's pi settings say: `followUpMode` (and `steeringMode`, for steered ones) `"all"` delivers every queued message together, pi's default `"one-at-a-time"` one per finished run. cpd does not set them (the User set both to `all` in the global pi settings on 2026-10-19; contract §6.5 "Queue modes are the User's").

**The Manager** (B3-A5) is a Session of role `manager`: it has no Plan, so §15.2 step 5 (blocking reasons) and §15.5 (the Plan review and the command `/plan-review`) do not apply to it and nothing is opened or registered for them. Its follower follows only Events addressed to it (§11), so of this table only `session-message` and `session-renamed` reach it: a Plan's Events never wake it. It learns of them through the digest at turn start (§15.7), which only a Manager gets.

This is §5.3's table; the `plan-review` row is the Plan review protocol §5.3 leaves to this file (amendment 6, §19). A blocking reason opened twice or closed when not open changes nothing (cpd 1 `blocked.ts`, kept), which makes replays harmless.

### 15.4 Agent states: `session state`

The adapter keeps three facts and reports the state they give (the model of `tmux-agent-state.ts`, §3.6):

| Fact | Set by |
|---|---|
| `active` | true at pi's `agent_start`; false at `agent_settled` when `ctx.isIdle()` is true |
| its own reasons | §15.3 (`escalation:<id>`, kind **agent**), §15.5 (`plan-review:<review>`, the Plan's latest review only, kind **human**) and L4 of hs-d552, hs-d571 (`loop-guard`, kind **human**, when the loop guard trips), each with a label |
| blocking dialogs | pi's `ui_prompt_start` and `ui_prompt_end` (pi 1.2.0 `ExtensionAPI`, emitted around every blocking extension UI prompt: `select`, `confirm`, `input`, `editor`, `custom`; depth-counted, so nested prompts give one pair) open and close **one** reason of kind **human**, labelled `dialog` (or `verdict: <option>`, the paragraph below). **Exceptions:** the review chat's own `select` and `editor` calls (from `pi/lib/review-chat.ts` `runDialog`) open no `dialog` reason; they are represented by `plan-review:<review>` (the Plan's latest review, kind **human**) instead. This is implemented through a counter of the adapter's own prompts: each `runDialog` call increments the counter before opening the dialog and decrements it in a finally block; a `ui_prompt_start` or `ui_prompt_end` that arrives while the counter is above 0 opens no reason. Every other prompt (any other extension's dialog and an `ask_user_question` with no Verdict question) still opens `dialog` as today. A **Worker's** dialogs are not reported (hs-d303, audit Q6): a Worker never talks to the User, it asks its Lead with `cpd ask` |

**Verdict dialogs** (pro-d150, User, 2026-10-28 EDT; FR7 of Plan `eec-codereview`; assumptions A3 and A5 accepted). A Verdict question's dialog is named for its recommended option, so the Inbox shows two rows, Row 1 `<who> verdict` and Row 2 `<option>` (pro-d171; contract §2.2 `dialog`, §3.7 E8); the Session's reason label `verdict: <option>`, which the adapter writes, is unchanged. **The rule.** The adapter inspects an `ask_user_question` `tool_call` that it did **not** refuse (§15.13: G2 and G4 passed). In `input.questions`, in order, it takes the first question whose `header` is a string matching `/^verdict$/i` (an exact match, no trimming) and whose `options[0].label` is a string that is not empty after the normalisation below; a question with the header but no usable first option is skipped and the search goes on. **The normalisation:** replace every run of whitespace or control characters by one space and trim; then remove one trailing ` (Recommended)`, case-insensitively; then trim again. That text is the option. When a question is found, the reason that pi's `ui_prompt_start` opens (kind **human**) is labelled `verdict: <option>`; with none, `dialog` as today. The label is cut at **80 characters in all** (LABEL_MAX, the `--label` limit of §10.6, which also refuses control characters, hence the normalisation): the whole label, `verdict: ` included, is cut. **The pending label** is held from the `tool_call` until the last open prompt ends (the `ui_prompt_end` that brings the depth to 0) or the `tool_result` of an `ask_user_question` call arrives, whichever is first; a reason that `ui_prompt_start` opens while one is held takes it. So the label of a later dialog that no `ask_user_question` opened is `dialog`. A refused call holds none. A Worker's dialogs still open no reason (unchanged).

Every reason has a **kind** (hs-d303, audit J2): **human** (the User has to act: `dialog` or `verdict: <option>`, `φ: plan review`, `φ: loop guard`) or **agent** (another agent handles it: `φ: Worker question`). Reported state: `blocked` with the label of the most recently opened **human** reason while any is open; else `waiting` with the label of the most recently opened **agent** reason while any is open; else `working` while `active`; else `idle`. So a Worker question opened during a dialog does not replace the dialog's label and closing the dialog with the question still open reports `waiting`. On every change it runs `cpd session state <state> [--label <label>] --session <id>`. Calls are serialised: at most one in flight and only the latest wanted state is sent next, so states never arrive out of order. A refused call (`illegal_transition`, for example after the engine marked the Session `stopped`) is reported as an error once and the next change is tried normally. At `session_shutdown`: `session state stopped`, waited for at most 2 s; `session run` reports it as well if pi exits without it (§10.8).

**Reply runs** (hs-d278; contract §2.2 `unread`, §3.6). The adapter also records whether the current run is a **reply run** and reports it when the run's end moves the Session to `idle`: `cpd session state idle --reply --session <id>`; any other call omits `--reply`. A run is a reply run when it started at pi's `before_agent_start` with `event.messageType` `"user"` (a prompt the User submitted, or `pi.sendUserMessage()`), or with `"custom"` and the adapter's own message for a `session-message` Event whose `data.from` is null (`cpd session send` with no sender: "Message from the User"), or when pi's `input` event with `source: "interactive"` arrived during it (the User typed, also as a steer or follow-up). A run started by an engine Event, or by another Session's message, is not a reply run unless the User also typed during it. The flag is cleared when the run starts and read at the `idle` report; an older adapter sends no `--reply`, so its Sessions are never unread until it reloads (protocol 4, §1.2). by `ask` itself and back to `working` when it returns (§8.1); its adapter is mid-turn meanwhile and reports nothing.

**Names.** On pi's `session_info_changed`, when pi's name differs from the last name the adapter knows, it runs `cpd session rename <name> --session <id>`, so a `/name` typed in pi reaches the TUI. The `session-renamed` it causes comes back and changes nothing.

### 15.5 The Plan review in chat

Decision hs-d502 (User, 2026-10-20; supersedes hs-d468). A Plan review shows the User a **short summary** in the Lead's chat and asks for the decision in one **dialog**: `Approve` or `Change`. `plan decide` by the adapter stays the only approval record (§4.7); nothing else approves.

**What pi gives** (docs, pi 1.3.0; verified in `docs/extensions.md` and `dist/core/extensions/types.d.ts` and in `dist/modes/interactive/interactive-mode.js` for the behaviour of `select`):
- `ctx.ui.select(title, options, opts?)` returns the chosen option string, or `undefined` when the User presses Esc **or** when `opts.signal` (an `AbortSignal`, `ExtensionUIDialogOptions`) aborts; an already aborted signal returns `undefined` at once. So a caller tells its own abort from Esc by its own controller's `signal.aborted`.
- `ctx.ui.editor(title, prefill?)` returns the text, or `undefined` when cancelled. **It has no `opts`, so no signal**: an open editor cannot be closed by the adapter.
- `ctx.ui.notify(message, type?)` shows a notice in pi's chat area and adds nothing to the model's context. `pi.sendMessage(message, options)` adds a message to the session, drawn by the renderer of its `customType` (§15.9). `pi.registerCommand(name, {description, handler(args, ctx)})` registers a `/name` command.
- A dialog is not tied to a pi event handler: the adapter already calls `host.ctx.ui.notify(…)` from its follower's event handling (`handle.ts`, `startup.ts`) with the `ExtensionContext` it kept at `session_start`. `host.ctx` is set at `session_start` and cleared at shutdown; a reload makes a new one. It exists only in the TUI (`start` returns before it for any other mode, `startup.ts`), which is why "no `host.ctx`" below means "no UI". Dialogs share pi's editor slot (the User cannot type to the Lead while one is open) and pi emits `ui_prompt_start` and `ui_prompt_end` around each, which the adapter already reads for its state `dialog` (or `verdict: <option>`, §15.4). **The review chat's own dialogs** (the `select` and `editor` that `runDialog` in `pi/lib/review-chat.ts` opens) **open no `dialog` reason**; they are already represented by `plan-review:<review>`. Every other dialog still opens its reason, labelled as §15.4 says, as today. The adapter implements this through a counter: each `runDialog` call increments the counter before the dialog opens and decrements it in a finally block; when `ui_prompt_start` or `ui_prompt_end` arrives while the counter is above 0, no reason is opened.
- Unverified: how a dialog opened by an adapter while the Lead's turn runs is drawn is pi's (the dialog takes the editor's place); the Job that implements this checks it with a real pi.

**Requesting.** The Lead runs `cpd plan review` directly. The command returns its `next` text **at once**, without waiting for the User (an error from cpd, such as missing outline, is reported at once); the pi adapter shows the summary and opens the dialog when the `plan-review` Event arrives.

**Opening**, on a `plan-review` Event with `status: pending` for its Plan, **whatever the Plan's phase** (a review of the drafts of a running Plan, §4.6, opens exactly like one in phase `plan`; the adapter does not read the phase and its list of Event kinds does not change), or from the tool:
1. Skip it when the session already has a `cpd-plan-review` entry for this review, or when it is not the Plan's latest review (`cpd plan show`, `review.id`): a replayed or superseded request.
2. **Without `host.ctx`** (no UI), or when `run/<plan>/summary-<review>.md` does not exist (a review made before cpd wrote it), the review stays pending and no dialog opens. Otherwise:
3. Read `run/<plan>/summary-<review>.md` (§4.6). A failed read is an error message with the cause (turn); nothing is written and the review stays `pending`.
4. Show it: `pi.sendMessage({customType: "cpd-plan-summary", content: <the file's text>, display: true, details: {plan, review}}, {triggerTurn: false})`. The message is drawn **expanded**, always open (§15.9) and stays in the session, so `/reload` and a resume show it again.
5. `pi.appendEntry("cpd-plan-review", {plan, review, surface: "chat", status: "pending"})`; open the blocking reason `plan-review:<review>` labelled `φ: plan review`; close the reasons of this session's older reviews ("Latest only").
6. Open the **dialog**, below. It runs **detached**: the handler that opens it (the follower's, the tool's or the command's) does not wait for it, so Events keep being read and handled, the cursor keeps moving and the tool returns at once.

**The dialog**, with `host.ctx.ui` (one at a time per session; opening a newer review aborts the older review's dialog, below, whose entry stays pending as every superseded entry does):
- `ctx.ui.select("Plan review <review>: <review title>", ["Approve", "Change"], {signal})`, `signal` of an `AbortController` the adapter keeps for this review.
- **Approve** → "Deciding" below with `cpd plan decide <review> --approve --plan <plan>`.
- **Change** → `ctx.ui.editor("What should change?")`. A note that is empty after trimming, or a cancelled editor, **returns to the select** (nothing is recorded). Otherwise the note is written to `$CPD_HOME/run/<plan>/feedback-<review>.md` (overwritten on a retry; no temporary directory is left behind) and "Deciding" runs `cpd plan decide <review> --reject --feedback-file <f> --plan <plan>`.
- **Esc** (`undefined` and the controller's signal not aborted): nothing is recorded, the review stays `pending` and `plan-review:<review>` stays open. The adapter says so once with `host.ctx.ui.notify("Plan review <review> is still waiting: /plan-review reopens it")` (a notice, not a message to the model). **Without `host.ctx`** (no UI) the review stays `pending`.

**`/plan-review`**, a pi command the adapter registers once at load (`pi.registerCommand("plan-review", {description, handler})`; for a Plan Lead only, as the tool is): it reopens the dialog of this session's **pending chat review**, the Plan's latest review when the last `cpd-plan-review` entry for it has `surface: "chat"` and `status: "pending"` and `cpd plan show` reports it `pending`; it sends no new summary. With none, `ctx.ui.notify("cpd: no plan review is waiting in chat")` and nothing else. With a dialog already open for it, nothing.

If the summary was shown and the dialog was left with Esc, only `/plan-review` (or a restart, "Recovery") brings the dialog back; the summary message stays in the chat.

**Latest only** (hs-d224, bug hs-n158). The reason `plan-review:<review>` follows only the Plan's **latest** review, because nobody decides a superseded review and entry `approved`, `rejected` or `superseded` are closed (`plan lock` and the Plan inbox read only the latest, contract §3.1, §4.6): it exists while the review is `pending` **and** is the Plan's latest review. Review ids are `r<k>`, compared by `k`.
- On a `plan-review` Event with `status: pending`, every reason of a review with a lower `k` closes, before the new review is opened, **and the dialog of such a review is closed** (its `AbortController` aborts; a closed dialog records nothing).
- On a `plan-review` Event with `status: superseded`, the reason closes and, if open, **the dialog is closed** the same way.
- At start-up (§15.2 step 5) only the review `cpd plan show` reports (`review.id`) can open a reason, when this session has a pending entry for it; the entries of older reviews open nothing. When `plan show` fails (an error message, §15.2), the highest `k` among this session's entries stands for it.
- The entries of superseded reviews stay in the entry list; only the blocking reason (and the dialog) closes. A reason closed twice, or one that is not open, changes nothing, so replays are harmless.

**Deciding**, for a pending entry, on the dialog's `Approve` or `Change`:
1. Write the feedback, if any, to `$CPD_HOME/run/<plan>/feedback-<review>.md` (the dialog's note; overwritten on a retry; no temporary directory is left behind); run `cpd plan decide <review> --approve | --reject [--feedback-file <f>] --plan <plan>`. `review_decided` means another caller decided it: the adapter reads the decision with `cpd plan show` and continues with it. A failed `plan decide` is an error message (turn); the entry stays pending and, for the dialog, the select opens again.
2. Append the entry with `status` `approved` or `rejected` (the entry keeps its `surface`); close `plan-review:<review>`.
3. Tell the Lead with `pi.sendMessage({customType: "cpd-plan-decision", …}, {triggerTurn: true, deliverAs: "followUp"})` (cpd 1 `decisionText`, kept; **the text is unchanged**): `Plan review r3 was APPROVED.` plus the feedback, then `Next: cpd plan lock` in phase `plan`, or in a later phase that the drafts are queued and no lock is needed (the Event's `data.jobs` lists them), or for a rejection `Revise the plan (job edit, job add, job cancel), then ask for review again.`; when `current` is false, it says the Jobs changed since that review and asks for a new one. The custom type differs from `cpd`, so this message never moves the cursor.

**Deciding elsewhere**, on a `plan-review` Event with `status` `approved` or `rejected` for a pending entry of this session: any caller other than this adapter decided the review (`cpd plan decide` from the CLI, the User or a script) and the database already holds the decision, so the adapter does **not** run `plan decide`. Otherwise the Lead would stay `blocked` and never be told. It does what "Deciding" does after its own `plan decide`:
1. Read `cpd plan show` for `current` (and for the feedback when the Event's `data.feedback` is not a string).
2. Append the entry with the status; close `plan-review:<review>`; **abort the review's open dialog** (`AbortController.abort()`, which pi honours for `select`, "What pi gives"): the select closes and records nothing. An **editor** open in `Change` cannot be aborted (it has no signal): it stays until the User closes it and its result is **ignored** because the entry is no longer pending (no `plan decide`, no new select).
3. Tell the Lead with the same `cpd-plan-decision` message (`triggerTurn`, followUp), with the Event's `data.feedback`, plus a last line saying the review was decided outside this adapter.

A failure of `plan show` is an error message and leaves the entry pending. An Event for a review with no pending entry (replayed, another Plan's, already decided) changes nothing. A later answer of the dialog for an entry decided this way is ignored, because the entry is no longer pending; one that arrives while this path runs is ignored too, so the Lead is told once.

**Recovery** at `session_start`: for each pending entry,
- `surface: "chat"`, the last entry of the Plan's **latest** review when `cpd plan show` reports that review `pending` (§15.2 step 5 has opened its reason): the dialog **reopens** and the summary is **not** sent again (it is in the session). A review `plan show` reports `approved`, `rejected`, or `superseded` is not reopened; its `plan-review` Event, replayed from the cursor, is "Deciding elsewhere" (for superseded: treated as decided, so the reason closes and no dialog opens, like approved or rejected from the Event handler).


### 15.6 Reload after an install

Decision hs-d141 (research: `docs/research/pi-reload.md`), superseding its first answer; contract §5.3 "Reload after an install" and §6.7. After `cpd install` (§14.4) the adapter reloads its own pi (`ctx.reload()`: extensions, skills, prompts, themes, context files) without the User typing and without typing into a pane. Nothing reaches the model.

**Detection.** At `session_start` the adapter records the identity `(dev, ino, mtimeMs)` of its own directory (`dirname` of `pi/lib/`, from `import.meta.url`; contract §8 `pi_adapter`) with `statSync`. An `unref`'d timer polls it every 5 seconds (a constant of the adapter, not a config key) and is cleared at `session_shutdown`. The install replaces the directory as a whole (`rm -rf`, `mv`), so a new package has a new identity. `ENOENT` is the install in progress, not a change: the poll waits. A different identity of an existing directory is an install. The cpd binary is not watched (it changes first, before the directory is complete). The adapter runs only in `tui` mode (§15.1), so no other mode polls.

**The command.** The adapter registers `cpd-reload-after-install`. pi runs an extension's command with a full `ExtensionCommandContext` only through `pi.sendUserMessage("/cpd-reload-after-install", {expandPromptTemplates: true})` (pi 1.2.0 `agent-session.js` `prompt()`); without the flag the text goes to the model and pi's own `/reload` cannot be reached this way. The handler checks the guard, then `await ctx.reload()`; nothing after it uses the old `ctx`. The new instance records the new identity at its `session_start`, which is why a successful reload is not repeated.

**The guard**, in this order, checked by the adapter before it triggers the command and again in the handler (the handler is authoritative; only it has `ctx` and a User typing the command by hand is harmless):
1. `ctx.isIdle()` (mid-turn pi's `ctx.reload()` silently does nothing yet resolves, so it cannot be left to pi);
2. `ctx.hasPendingMessages()` is false;
3. the adapter's own state (§15.4) is `idle`: not `working`, `waiting` or `blocked`, so no Worker question, Plan review or `herdr:blocked` signal is open (pi's `isIdle()` does not see dialogs and a reload closes an open dialog and loses its answer);
4. `ctx.ui.getEditorText() === ""` (no unsent typing).

**Retries.** A refused reload (the guard fails at the trigger or in the handler) does nothing and is **not** handled: the recorded identity stays the old one, so the next tick checks the guard again and triggers again; there is no limit and no backoff beyond the poll interval. At most one trigger is in flight. **At settle:** a Lead that gets a message every few seconds is idle for gaps shorter than the 5 s interval, so no tick may ever see it idle (`docs/research/pi-reload.md` "Why a busy Lead did not reload"). So the adapter also decides at pi's `agent_settled` (pi has no automatic work left), after its own state is updated: for a package that **already passed pre-validation** and is still the directory's identity it runs the same guard and, when it passes, the same trigger. It adds no guard, does not shorten the stability or validation steps (a directory not yet validated waits for its ticks) and keeps one trigger in flight.

**Logged.** Nothing is delivered to the Session and no `cpd` command is run for it. On a normal reload after an install, the wrapper prints nothing on re-exec and the adapter notifies nothing to the pane; the status line's version pill (#210, @cpd_version) is the only notice (hs-d533). When a reload fails, the adapter notifies with `ctx.ui.notify` (level `error`, `cpd: adapter update not loaded: <reason>`, `cpd: cannot write the reload log: …`, or the long refusal `cpd: a new adapter is installed; reload when free (/reload)`), the wrapper logs `reload skipped: …` and pi's "Reload failed: …" is pi's own message. `ctx.ui.notify` is not written to the session file, so the notice is not evidence afterwards: the log below is.

**Log** (hs-d141, finding of `docs/research/pi-reload.md` "Why a busy Lead did not reload"). Every reload decision is one line appended to `$CPD_HOME/run/adapter/<session id>.log` (`CPD_HOME` default `~/.cpd`, `CPD_SESSION`; created with its directory at the first line; one file per Session, never rotated, small because a waiting or refusing line is not repeated while it is the last one). A line is `<ISO-8601 UTC time> <text>`; the texts, in the order they occur:
- `start: <reason>`: the adapter started (`session_start`; `reload` after a reload, `startup` or `new`, `resume`, `fork` otherwise);
- `error: <what>: <code>: <message>` (hs-d573): adapter error logged for a Lead Session, once per (`what`, `code`), keyed by `host.report`'s memory; the next different error is logged; if the error is a second occurrence, nothing is logged (G1's memory prevents it). For a Manager or a Worker the line exists only for `mcp_register_failed` (pro-d226, §15.1 'Errors'), and for every role it is written at each refusal, since that code is exempt from the memory;
- `report failed: <code>: <message>` (hs-d573, L4 of pro-d226): the `cpd session report` command to the Manager failed with that code, for every role, never delivered and never reported again; the next different error tries again, and a refused MCP registration (no memory) at its next refusal;
- `poll: watching <dir> (<identity>) every <ms> ms`: the poll began, with the identity it will compare with;
- `poll: the adapter directory is missing (an install in progress); waiting`;
- `poll: the identity changed <old> -> <new>; not stable yet` (also `changed again`);
- `poll: <identity> is stable; validating the package`, then `poll: pre-validation passed` or `poll: pre-validation failed: <reason>` (the reason of the visible error above);
- `poll: refused: busy|pending|state|typing|not active`: the guard refused at a tick or at a settle;
- `poll: trigger sent (/cpd-reload-after-install, at a tick|nudge)`;
- `command: refused: busy|pending|state|typing`: the handler's guard refused; `command: reloading`; `command: ctx.reload() returned` (a successful reload is followed by `stop` and the new instance's `start: reload`, so this line may come after them); `stop`: `session_shutdown`;
- `poll: not reloaded <n> s after the package validated; told the User`;
- `poll: failed: <message>`.

A line that cannot be written is notified once per run (`ctx.ui.notify`, level `error`, `cpd: cannot write the reload log: …`; not a message, so no turn) and never stops a reload.

**A long refusal.** When a package that passed pre-validation has not been reloaded **more than 10 minutes** after it passed (the guard refused every time; a constant of the adapter), the adapter notifies the User **once per install**: `cpd: a new adapter is installed; reload when free (/reload)`. A package that does not validate never produces it (`/reload` would be wrong for it; it has its own visible error). The wrapper's restart (below) is the fallback after the same refusal.

**Pre-validation** (hs-d141, the User's decision of 2026-10-18; research finding A, `docs/research/pi-reload.md` "Live verification"). A failed load leaves pi running with no adapter and no message, so the adapter proves the new package loads **before** it triggers the command. At each poll, once the identity differs from the recorded one, in this order; the first that fails ends the poll with no reload:
1. **Stable.** The new identity `(dev, ino, mtimeMs)` is equal on **two consecutive polls** (the install has finished; the first poll that sees a new identity only remembers it). `ENOENT` clears the remembered identity.
2. **Entry files exist:** `extension.ts` and `package.json` in the directory.
3. **pi itself loads the new package** (hs-d255 (a); the bug it replaces: a plain `node` + `jiti` load could not resolve `@earendil-works/pi-tui`, which pi provides to extensions at runtime, so every validation after #61 failed `Cannot find module`), in a child process, never in the live pi: `PI_OFFLINE=1 pi --no-extensions --no-session -e <dir> --mode rpc` with stdin `/dev/null`. `pi` is the one on `PATH`, the binary cpd starts the Session with (same version). Its `PI_CODING_AGENT_DIR` is a scratch empty directory under the OS temp directory (no User settings, sessions or credentials; with no model it needs no network), also its working directory and removed afterwards; the User's sessions and settings are never read or written. Exit 0: the extension loads. Exit 1: pi's stderr says why (`Failed to load extension "<path>": …`, also `Extension does not export a valid factory function`). The child has a timeout of **60 s**, which tolerates a loaded machine; its process group is killed at the timeout and **a timeout is a refusal** (`load timed out after 60000 ms`) retried at a later poll like any other, not a failure of the adapter. **Why a child:** pi's load runs the new module's top-level code and its factory; in the live pi that would be a second load of a package that may throw or hang. The factory runs in the child too, but the adapter activates only in `tui` mode (§15.1), so in `--mode rpc` it starts no follower and runs no `cpd` command. **Why pi itself:** it is the loader that will load the package, with its own aliases and virtual modules, so it cannot pass what pi rejects or fail what pi accepts (verified live: `docs/research/pi-reload.md` "Pre-validation with pi itself").

When a step fails: **no reload**; the recorded identity stays, so the check runs again at a later poll (no limit, the poll interval); and **one visible error**: `ctx.ui.notify("cpd: adapter update not loaded: <reason>", "error")` (`<reason>` is the first line of pi's stderr, `load timed out after <n> ms`, or `entry files missing`), shown once per distinct reason until a poll passes (a poll that passes re-arms it). Step 1 failing is not an error: it only waits. No `cpd` command is run and nothing reaches the model; the notice is the adapter's own, not an Event. Only when all three steps pass does the guard (above) run and the command trigger. A reload that still fails after a passing pre-validation (a load that differs from the child's) is caught by the wrapper's watchdog (§10.8 "Adapter watchdog").

**Relation to the wrapper's restart (§10.8; contract §6.7 "Reload on install").** The in-process reload is first. The wrapper's `SIGTERM` + `exec` stays as the fallback for what it cannot do: a pi without interactive UI, a reload refused for a long time (the wrapper's own idle rules: parked, or idle for 10 minutes) and an adapter that fails to load (the reload left pi without it; pre-validation above makes that rare and the wrapper's watchdog, §10.8, recovers it). Both implement hs-d49 with the same guard in spirit (never with unsent typing, never `blocked`); when the adapter reloads first, pi keeps its process and the wrapper's executable check still sees the new `cpd` binary and may restart pi at its own safe point, which is harmless (the new `session run` resumes the session file).

### 15.7 Context updates at turn start

Batch B2 (hs-d79, hs-d132; narrowed by hs-d209, D1; redesigned by hs-d514). A Manager sees what other Sessions wrote to its Context and what its Plans did, without polling. A Lead gets one short update with new records and pending board posts, not Jobs or other Events.

**Leads: short update with no turn start** (hs-d514, hs-d507). At pi's `before_agent_start`, for a Session of role `lead` (a Plan Lead, a side fork), one `cpd-context` message when anything is new since its cursor, holding only: (1) new decision records in force by other Sessions (`by.session` ≠ this Session), each as `- <id>: <first sentence>` (the rest: `cpd context show --record <id>`); (2) the pending board posts to it (from `cpd board take`), each in full with its sender; (3) the PRs of its Plan opened since, each as `PR <key> opened: <url>`; (4) the closed-unmerged line of each PR of its Plan that is closed-unmerged (contract §5.3), on every turn while it holds, also when (1) to (3) are empty. The lines are read at each turn with `cpd plan show --plan <its Plan>`. When that read fails, the update has one line `could not read the Plan's PRs: <the first line of the error's message>` for a condition (`plan_not_found`, a busy store, a timeout). A `usage` refusal is a defect (§15.1 'Calls'): the update has no line for it, the adapter reports it with `cpd session report` (§10.18: `--what 'closed-unmerged PR line'`, `--code usage`, the refusal's message and details), once per run while it repeats (the (what, code) in a row rule of §15.1), and (1) to (3) of the update are delivered as if the read had worked. Nothing about Jobs merged or other Events. No turn is started. The cursor is the highest Event id of a new `context-record`, a new `board_posts`, or a new `pr-opened` Event of the Plan.

Format:

```
Context building-cpd: 2 decisions · 1 board post · 1 PR opened
- hs-d514: Quiet agents, the design in full.

Board:
- Manager (2026-10-21 11:30 EDT): PR #207 opened

PRs:
- PR rebase opened: https://github.com/earendil-works/cpd/pull/211
```

Summary of the collapsed row (§15.9): `cpd-context: <n> decisions · <n> board posts · <n> PRs opened`, zero parts omitted; every part always shown.

**Managers: compressed digest** (hs-d514, hs-d86; narrowed by hs-d507). At pi's `before_agent_start`, for a Session of role `manager`, one `cpd-context` message holding: (1) records (as Leads see them, in full); (2) pending board posts (from `cpd board take`), each in full with sender; (3) a `Plans:` section, one line per Plan with Event counts by kind in a fixed order (no per-Job lines), e.g. `- cross-repo: 2 PRs opened (#210, #211), 1 Job failed, done`. The counted kinds are `plan-created`, `plan-phase`, `pr-opened`, `pr-merged`, `job-failed` and `plan-done`; an `escalation` Event is a Worker's `cpd ask` to its Lead (contract §5.2) and is never counted, because the Lead answers it or asks the User and the Manager has no action on it (decision pro-d24); (4) a `Pending work:` section, one line per active Plan with Job counts, e.g. `- inbox (dispatch): PR mainfix 2 running, 1 queued; PR prlink open #207`; (5) a new line `Errors: <n> (<code> x<k>, ...): cpd events list --kind error --since <id>` (hs-d573) counting `error` Events since the cursor, for its Context (including adapter errors from Leads, §15.1) and the machine-wide ones.

Format:

```
Context building-cpd: 2 new records since your last turn.
- hs-d514 decision (2026-10-21 UTC): Quiet agents, the design in full.

Board:
- User (2026-10-21 11:30 EDT): Inbox item closed

Plans:
- cross-repo: 1 PR opened (#212), 1 Job failed, done

Pending work:
- inbox (dispatch): PR mainfix 2 running, 1 queued; PR prlink open #207
- agent-comms (plan): PR quiet-core 1 running

Errors: 3 (gh_failed x2, pr_start_failed x1): cpd events list --kind error --since 4800
```

Summary of the collapsed row (§15.9): `cpd-context: <n> records · <n> board posts · <n> Plans · <n> errors`, zero parts omitted.

**Cursor**, in Event ids (the highest of all reads, `context-record` or board or Plan Events or errors): 
1. Start = the Session's `registered_event` (its first message already held the records up to then).
2. Then the greatest `details.event` of the session file's `cpd-context` messages whose `details.delivered_to` is this Session: the fork-safe rule of §15.2 step 4.
3. Also the greatest `details.event` of the session file's `cpd-first-message` messages whose `details.delivered_to` is this Session (hs-d542 A3: for a by-user Session, the first message is delivered only once, tracked by its Event id).

**Reading.** For records: `cpd events list --context <c> --kind context-record --since <cursor>`, then `cpd context show --context <c> --since <seq>` for the full text. The adapter skips its own records. For board posts: `cpd board take --session <id>` (internal, not a Lead command). For Plan Events: `cpd events list --context <c> --kind plan-created,plan-phase,pr-opened,pr-merged,job-failed,plan-done --since <cursor>`. For errors: `cpd events list --context <c> --kind error --since <cursor>` and `cpd events list --kind error --since <cursor>` (the machine-wide ones). The message's `details` is `{event: <highest Event id read>, delivered_to: <id>, records: [ids], posts: <n>, plans: <n>, errors: <n>, error_codes: {<code>: <count>}}`. The message is returned when any section has something; nothing is added if none do. No schema change.

**Failure.** A failed read adds no message and leaves the cursor. It is reported once as an error message (§15.1, no turn) and again only after a read has succeeded.

### 15.7a First message of a by-user Session

Decision hs-d540 (User): when the User creates a Lead or Manager from the tree, pi starts with no first message and runs no turn; the role's first message is delivered with the User's first prompt, in the same turn, computed when that prompt is sent. Decision hs-d542 (User): (1) the tree passes `--by-user` to `plan new` and `context manager`; without it a command keeps today's behaviour. (2) Until the User's first prompt a by-user Session runs no turn at all: every message or Event that would start one is queued (pi's `sendMessage` `deliverAs` 'nextTurn', `triggerTurn` false) and goes with that prompt's run, with the first message. (3) The first message is hidden in the chat (`display` false); the User sees only their own prompt.

**Handler** (contract §10.8, hs-d542 A3). The pi adapter, for a by-user Session whose session file holds no `cpd-first-message` custom message delivered to it (`details.delivered_to` = this Session, the fork-safe rule of §15.2 step 4/§15.7), adds at the first run's `before_agent_start` one custom message `{customType: 'cpd-first-message', content: text, display: false, details: {event, delivered_to}}`; pi places handler messages after the User's prompt and the queued nextTurn messages (pi 1.3.0 `agent-session.js` `_prepareRun`). Until it is delivered, the adapter starts no turn: every `sendMessage` that would use `triggerTurn` true uses `deliverAs` 'nextTurn' instead. A failed first-message read is reported once as an error message (as §15.1 'Failure') and retried at the next run; no message is added.

**Reading.** The adapter calls `cpd session first-message --session <id>` (internal, §10.15), which renders the first message and returns the text, the file it was written to and the highest Event id at rendering. The adapter reads the text and delivers it as a `cpd-first-message` custom message, with `details.event` = the returned Event id and `details.delivered_to` = this Session, so the read model and later runs can recognize it.

### 15.8 Turn-end reminder

Batch register (hs-d241, R2; the rule is hs-d239: no planned work lives only in a Lead's chat). A heuristic that nudges a Lead to register what it announced. It lives in its own module of the adapter (`pi/lib/reminder.ts`), with a few lines in `adapter.ts`.

**When.** At pi's `before_agent_start`, for a Session of role `lead` only (never a Manager, never a Worker). It never starts, wakes or interrupts a turn: it only adds a message to the run that is starting.

**The check**, made at the end of each run of the Lead (so it concerns the Lead's **last assistant message of that run**) and held for the next run. The reminder is due when both hold:

1. **It talks of future work**: the message contains, case-insensitively, one of these fixed phrases (substring match):

   `next batch`, `next step`, `next steps`, `next pr`, `follow-up`, `follow up`, `followup`, `later`, `then i will`, `then i'll`, `we still need`, `still need to`, `still to do`, `todo`, `to do next`, `once that merges`, `once it merges`, `after that merges`, `after it merges`

   and the pattern `after … merges`: `after`, then 1 to 60 characters that are not a newline or a full stop, then `merge` or `merges` (all as whole words). This list is the whole definition; extending it is a change to this section.
2. **It names no registered key**: it contains, as a whole word (case-sensitive), no Job key and no PR key of the Lead's Plan, as `cpd plan show` lists them at the moment of the check, whatever their status (draft, queued, running, merged, failed, cancelled alike).

A check that fails, whether `cpd plan show` returns an error or the message cannot be read, adds nothing: the reminder is not due and nothing is reported. A message with no assistant text is not checked.

**What.** When due, the next `before_agent_start` of that Lead returns one message, `customType: "cpd-reminder"`, `display: false` (never shown to the User), with this `content`:

```
cpd reminder (hs-d239): your last message spoke of further work but named no registered Job or PR. Register it now (cpd job add <k> --draft --title "…", or cpd pr add …) and name its key.
```

It is added **at most once per run** and once it has been added the reminder is not due again until a later run ends with a message that fails the check. It is never delivered to a Session that is not a `lead` and never starts a turn (`triggerTurn: false`, as §15.1's messages).

**Not done** (R3, *decided*): `pr open` does not ask for a checked statement; the reminder is the whole enforcement of this heuristic. The skills carry the rule (R4).

### 15.9 Rendering cpd's messages in a pi pane

Decision hs-d286 (User, 2026-10-19; it restyles hs-d221). **Display only:** every message below still reaches the model with its full `content` and `details`; only what the chat window draws changes. Nothing here changes a delivery (§15.1, §15.3), a cursor or an Event.

**Which messages.** Every `customType` the adapter sends with `display: true`: `cpd` (Events §15.3, Session messages and the adapter's errors §15.1), `cpd-context` (§15.7), `cpd-skills` (§15.23), `cpd-plan-decision` and `cpd-plan-summary` (§15.5). A message with `display: false` (`cpd-reminder`, §15.8) is never drawn, so it has no renderer. A `customType` the adapter starts to display later is rendered by the same rule, its summary being the first line of its `content` (as an Event's, below). The adapter registers one pi message renderer per type, once at load (`pi.registerMessageRenderer`), from one module, `pi/lib/render.ts`; the renderer reads `options.expanded` and `options.outputPad` from pi.

**The look is pi-compact-tools 0.16.6's tool row, copied.** The adapter does not import or vendor that extension (it exposes no API and draws tool rows only); it draws the same rows, from the same glyphs, theme tokens, bold and dim, laid out as the extension's source lays them out (`extensions/compact-tools.ts`, `compact-tools-layout.ts`, `compact-tools-claude.ts`, `compact-tools-codex-rows.ts`, `compact-tools-palette.ts`, `compact-tools-core.ts`). The message keeps the `customMessageBg` background of hs-d221 behind the rows (the extension's rows sit on pi's tool background, which a message has no part in). **Colours** (hs-d534): the theme tokens named in this section (`dim`, `muted`, `toolOutput`, `customMessageLabel`, `customMessageBg`, …) are mapped to the palette's indices 0-15 by the Dango pi theme (contract §6.1.1: cpd's blocks magenta, lines and rails 8, toggles 7, content 7, tool pills by status in base colours, the Thinking pill cyan, a message's background 0); that theme and its pinning are Plan `pi-theme`'s and until it lands the tokens keep the user's own theme.

**Which style: the extension's, read once.** The extension has four styles: `compact`, `claude`, `codex`, `off` (`DISPLAY_STYLES`, `compact-tools-types.ts`). Its **default is `compact`** (`DEFAULT_CONFIG.style`, `compact-tools-config.ts`) and that is what applies when no config exists, which is the case on hs today. The adapter reads the style **once, at `session_start`**, as the extension's `loadConfig` does and keeps it for the run (a later `/compact-tools` switch shows in the adapter's rows after a `/reload`):

1. `compact-tools.json` in pi's agent directory (`getAgentDir()`, `~/.pi/agent` unless pi is told otherwise);
2. then, only when the project is trusted (`ctx.isProjectTrusted()`), `<cwd>/.pi/compact-tools.json`, which overrides the first.

Only the `style` key matters; the other keys are the extension's own. A file that is missing is skipped. A file that does not parse, or a `style` that is not one of the four, is handled **exactly as the extension handles it**: one line `[compact-tools] <path>: <message>` on stderr and the value from the file before it (at first the default `compact`) is kept. The adapter adds no fallback of its own. With `off` the adapter registers **no** renderer and pi draws its own default box with the full `content` (the User turned the extension's rows off).

**The pill and the first sentence** (hs-d537, amended by the User 2026-10-22): the pill holds **only** `φ`, the cpd mark rendered bold (only the glyph is bold, not the colon or text that follows it). After it come one space and the summary's **first sentence**, **outside** the pill, text 7, as a tool row's title follows its pill: the summary up to the first `.`, `:` or line end, cut at **60 characters** with `…` (59 characters and `…`) only when that sentence is longer. A long sentence wraps onto continuation rows aligned like a wrapped tool title (`renderToolCall`). Example: ` ╭─[φ] Worker of Job chatfix-impl-misc (Attempt chatfix-impl-misc-a…` then ` ╰󰬦 expand`. The pill is magenta: `ESC[35m` U+E0B6 `ESC[7m` TEXT `ESC[27m` `ESC[35m` U+E0B4 `ESC[39m` (contract §6.1.1, hs-d545). The full summary is the title row's text where it is wider than that, below.

**The summary per kind** (what follows `cpd` on the collapsed row). One line, every newline and run of whitespace collapsed to one space and the ends trimmed:

| Message | Summary |
|---|---|
| `cpd`, a Session message (Event `session-message`) | `Message from <sender>`: the sender Session's name, or its id when it has no name; `Message from the User` when `data.from` is null |
| `cpd`, any other Event | the Event's `text`: its first line, cut to **160 characters** (159 and `…`) as the extension cuts a custom tool's summary (`CUSTOM_SUMMARY_MAX_LENGTH`); a row that is still wider than the content width `W - 6` wraps at it (ending at column `W - 4`, contract §6.1.1 "The rail fold"), as the extension's does, it is not cut again |
| `cpd`, an adapter error (`details.kind` is `error`) | `adapter: <what>`: the `<what>` of `cpd adapter: <what>: <code>: <message>` (§15.1) |
| `cpd-context` | its counts line, `cpd-context: 6 records · 17 Plan Events · 10 Plans pending` (§15.7) |
| `cpd-skills` (§15.23) | `Skill <skill> changed`: the first sentence of its `content` (`Skill cpd-lead changed`) |
| `cpd-plan-decision` | `Plan review <id> approved` or `Plan review <id> rejected` (`<id>` the review's id, as in §15.5) |
| `cpd-plan-summary` | `Plan review <id> summary` (`<id>` = `details.review`); never folded, see "No summary" |

**Collapsed, style `compact`** (the default; hs-d339 restyles it to §17's fold look). Two rows:

```
╭⦁ Message from cross-repo
╰󰬦 expand · 14:30
```

- The title row is `╭⦁ <title>`: `⦁` (U+2981) is painted with the theme token **`customMessageLabel`**, pi's colour for a custom message's label (violet/magenta in pi's themes; magenta, 5, in the Dango theme, hs-d534), so it never reads like bash's green `⦁`; it is a fixed colour that carries no status. The title (`Message from cross-repo`, the summary below) is bold.
- `╭`, `│`, `╰─`, `expand` and `collapse` are the muted chrome tone: the colour of `dim`, painted with `theme.fg("dim", …)` (colour 8 in the Dango theme, 7 for the words `expand` and `collapse`; contract §6.1.1, hs-d534).
- A title longer than the content width `W - 6` wraps (contract §6.1.1 "The rail fold"), its continuation rows behind `│ ` (muted); it is never cut.

**Expanded, style `compact`.** The title row stays. Under it comes the full `content`, plain text (not Markdown), every line `toolOutput`, each behind `│ ` (muted) and wrapped to the content width `W - 6` (ending at column `W - 4`, contract §6.1.1 "The rail fold"), with no blank rail row between the head and the first one; then the closing row ` ╰󰬬 collapse · HH:MM`. An empty `content` shows no body.

**Styles `claude` and `codex`**, when the config names them, follow the same rule with the extension's own chrome:
- `claude`: head ` ⦁ cpd(<summary>)`: `⦁` as above, `cpd` `toolTitle` bold, `(<summary>)` `toolOutput` (`claudeTitle`). Collapsed, one row under it, `╰─ expand` (muted, the shared widget's line). Expanded, the full `content` in `toolOutput` from that prefix on, later lines at six spaces (`CLAUDE_INDENT`), wrapped to the pane, then nothing else.
- `codex`: head ` • cpd <summary>` (`CODEX_BULLET`, painted bold with the magenta token above; `cpd` bold; the summary plain). Collapsed, one row under it, `╰─ expand` (muted, the shared widget's line). Expanded, the full `content` dim from `   └ ` on, later lines at five spaces (`CODEX_INDENT`), wrapped to the pane, then ` ╰─ collapse` (muted).

**The rail and the pill colours** (hs-d557 items 2 and 5; contract §6.1.1). The rail `│ ` is colour 8 (`ESC[90m`) on every row, expanded or not, whatever the content or the width. The pill is read-only: magenta (caps and block 5, text in the terminal's background colour (inverted)), never a colour from 8-15; the lead `╭─` and the closing row's `╰` are 8, the toggle `󰬦 expand` / `󰬬 collapse` is 7. Every row starts with one space (` ╭─`, ` │ `, ` ╰`), the column the tool and command rows share (contract §6.9).

**The fold** (hs-d339; replaces hs-d309's `┌` / `└` shapes and `Click to expand`). A message is a collapsible block of §17 ("Collapsible blocks"), the same look as the tree's: `╭⦁ title`, then `│ ` content lines, then ` ╰󰬦 expand · HH:MM` / ` ╰󰬬 collapse · HH:MM` (`pi/lib/render.ts`). The timestamp (HH:MM 24-hour zero-padded, machine-local time) is from `CustomMessage.timestamp`. Only that line toggles. When the row has no room for ` · HH:MM` the time is omitted.

**The expand action** is the extension's: **a left click** on the ` ╰󰬦 expand · HH:MM` / ` ╰󰬬 collapse · HH:MM` line toggles that one message and **Ctrl+O** (pi's `app.tools.expand`) expands or collapses every message and tool row at once. Pi hands the renderer `options.expanded` for Ctrl+O; a click is the renderer's own: it wraps its rows in pi-tui's `MouseRegion` that flips the message's own state, as pi does for a tool row (`createResultRegion`) and the extension does for its group row. The state is per rendered message and follows Ctrl+O both ways (the extension's group rule, `compact-tools-grouping.ts`); it is not persisted, so a reloaded session shows every message collapsed.

**No summary** (`cpd-context` whose `details` lack the counts, §15.7; and always `cpd-plan-summary`, hs-d502: the User reads it in the chat to decide, so it is never folded and Ctrl+O changes nothing for it; its content is Markdown, drawn as plain text like every message). There is nothing short to say, so the row is always open: the title `╭⦁ cpd-context` (for `cpd-plan-summary`, its summary above) and the full `content` as in "Expanded", ending with the terminator ` ╰󰬪` (contract §6.9 "The terminator": colour 8, no text, no toggle), never a bare `│`.

### 15.10 Policy text

At `session_start` the adapter runs `cpd policy show` with no flag (§22.1) and, at each run, adds one system prompt section, `cpd-policy`, with the `text` of every domain whose `audience` holds the output's `role`, labelled by domain and `source`. contract §8.3 "Delivery to agents" is the rule, including a failed `policy show` (an error message, no policy text).

### 15.11 A Manager or a Lead never blocks

Decision (User, 2026-10-19). Neither a Manager nor a Lead blocks on a wait: no sleep, polling loop or long wait in its own turn. Anything that takes time runs in the background and reports back as a message, or is the job of the Lead or Worker that owns it. The adapter enforces it, so it is not left to the skill's prose.

This rule is extended to a Lead (User, 2026-10-25, pro-d39). Evidence: during incident gatemac (pro-d29) the Lead of Plan tests-mock ran `for i in 1 2; do git fetch ...; sleep 280; done` in its own turn; the 280 s duration trips the 30 s bound and is refused. A Lead's wait holds the User's conversation as a Manager's does. A Worker keeps its own rule (§15.12), unchanged.

**When.** At pi's `tool_call`, for a `bash` call (`event.toolName` is `bash`, `event.input.command` and `event.input.timeout`), of a Session of role `manager` or `lead`. The handler is registered once at load (`pi.on` has no unsubscribe) and reads the role of the active run at each call; for a Worker, or before `session_start` has read the role, it returns nothing. A call that matches nothing runs, with an omitted `timeout` set to the bound as **The omitted timeout** says.

**The bound** is **30 seconds**, one constant of the adapter (`pi/lib/noblock.ts`); there is no config key. The bound and the patterns are the same for both Manager and Lead.

**What is refused.** The call is refused when it waits longer than the bound, that is, when the command (`pi/lib/noblock.ts`, `blockingReason`) contains one of:

- `sleep N` with N > 30 s: any unit (`s`, `m`, `h`, `d`), decimals allowed (`sleep 30` passes; `sleep 31` and `sleep 1m` are refused); several arguments add up (`sleep 20 20`) and `sleep infinity` is refused;
- `timeout N …` with N > 30 s (same units; `timeout 20 cmd` passes, `timeout 60 cmd` is refused);
- `watch`;
- `tail -f` or `tail -F` (also `--follow`);
- `cpd events follow`;
- `gh run watch`;
- `gh pr checks --watch`;
- a `while` or `until` loop whose body sleeps (any `sleep`, whatever its duration);
- the bash tool's own `timeout` input above 30 (seconds).

A pattern counts where a command runs: a command word of a simple command (after `;`, `&`, `|`, a newline, a parenthesis, `do`, `then` and a leading `VAR=value`, `time`, `nohup`, `exec`, `command`, `sudo` or `env`), not an argument or quoted text (`grep watch file` passes). A heredoc body is data, not commands (Decision pro-d203, 2026-10-29): it is not judged, so `cat <<EOF` with `watch` in a backticked word or a prose line in its body passes. The operator is `<<` or `<<-` outside quotes; `<<<` is a here-string, not a heredoc and skips nothing. The delimiter is the next word, with its quotes taken off (`EOF`, `'EOF'`, `"EOF"` and `\EOF` are one delimiter); a word ends at whitespace or at `;`, `&`, `|`, `<`, `>`, `(`, `)` or a newline. The body starts on the line after the first newline outside quotes that follows the operator and ends at the first line that equals the delimiter exactly: for `<<-` its leading tabs are removed first and for `<<` a line with leading spaces or tabs, or with more text, does not end it. The terminator line belongs to the heredoc. The rest of the operator's own line and everything after the terminator line are judged as before (`cat <<EOF | grep x` and `cat <<EOF; sleep 60` still judge their commands). A heredoc with no terminator line runs to the end of the call. Several heredocs on one line take their bodies one after the other, in the order of their operators. Not judged: an unquoted delimiter's body still expands `$(...)` and backticks in the shell and the guard does not judge them. A duration that is not a literal (`sleep $n`) is not judged.

A `for` loop is not a loop of this rule; the `sleep` in its body is judged by its own duration like any `sleep`, so `for i in 1 2; do git fetch origin; sleep 280; done` is refused (`sleep 280`) and `for i in 1 2; do sleep 5; done` passes.

**The omitted timeout.** Decision pro-d202 (2026-10-29). pi's `bash` tool has no default timeout: a call with no `timeout` waits as long as the command runs. Evidence: two Leads (rfm-memory-retrieval, rfm-curation-models) ran `grep -rl` and `find` across the home directory without `timeout` and hung on `~/Library/Group Containers` for 1 h 32 min. So when a Manager's or a Lead's `bash` call is not refused (`blockingReason` returns `undefined`) and `event.input.timeout` is `undefined`, the adapter sets `event.input.timeout = BOUND_SECONDS` (30) in place, before the call runs, so that no foreground `bash` call of a Manager or a Lead can ever hang past 30 seconds. An explicit `timeout` of 30 or less (for example `timeout: 10`) is kept unchanged; an explicit `timeout` above 30 is refused as before (`bash timeout <n>`). The bound is still the one constant `BOUND_SECONDS` of `pi/lib/noblock.ts`; there is no config key. A call that is refused is not mutated. A Worker's call is unaffected: its `timeout` stays as given, an omitted one omitted.

**The refusal** is `{block: true, reason}` (pi's `ToolCallEventResult`), with the reason on one line naming the rule by role:

```
reasonText(matched, role) {
  if (role === "manager") return `cpd: a Manager never blocks: a wait over 30 s runs in the background or is the owning Lead's or Worker's job; refused: ${matched}`;
  if (role === "lead") return `cpd: a Lead never blocks: a wait over 30 s runs in the background or is left to the engine's Events; refused: ${matched}`;
}
```

`<what matched>` is the matched command (`sleep 1m`, `watch`, `tail -f`, `cpd events follow`, `gh run watch`, `gh pr checks --watch`, `timeout 60`, `while … sleep`) or, for the tool's input, `bash timeout 120`.

**Not affected:** a Worker, whatever it runs; every other tool; every `cpd` command's own timeout. A Worker's `bash` call keeps its `timeout` as given (an omitted one stays omitted) and so does any call before `session_start` has read the role. A Lead's command that the User explicitly asked for and that waits over the bound runs in the background, as section 15.20 says.

### 15.12 A Worker runs targeted tests

Decision (User, 2026-10-19). A Worker runs only the tests its change touches; the full suite is the Gate's, run by the PR's `open` Job (contract §7.7; `pr open` is its repair). Several full suites and Gates at once loaded a host to 205 on 16 cores and every Gate failed on timing. The Spec's acceptance names the targeted tests (the area binaries, `--test <area>` of docs/implementing.md's area table and the modules the Job changes, plus its new tests), never a green full suite and the adapter enforces the rule, as §15.11 does for the Manager.

**When.** At pi's `tool_call`, for a `bash` call (`event.toolName` is `bash`, `event.input.command`), of a Session of role `worker` only. It is the same handler as §15.11's, which consults each guard by role (`pi/lib/testguard.ts`, `unfilteredTestRun`); for a Lead, a Manager, or before `session_start` has read the role, this guard returns nothing.

**What is refused.** The call is refused when a command of it runs a full unfiltered test suite, that is:

- `cargo test` or `cargo nextest run`, also through `scripts/cargo.sh`, with or without `timeout N` in front, when it has `--workspace`, or has none of `-p` / `--package`, `--test`, `--lib`, `--bin`, `-E` / `--filterset`, or a positional test-name filter (also after `--`);
- `npm test` or `npm run test` with no file argument (a word after `--`, or after the script name, that is not an option);
- `npm run verify` (the Gate), whatever its arguments;
- `pnpm test`, `pnpm t` or `pnpm tst` and `pnpm run test` or `pnpm run-script test`, with no file argument (a word after `--`, or after the script name, that is not an option);
- `pnpm run verify` or `pnpm run-script verify` and the bare `pnpm verify` (pnpm runs a script by its bare name), whatever its arguments.

The npm rules stay as they are (older branches still use npm).

**Allowed**, for example: `scripts/cargo.sh nextest run -p cpd --test flow plan_overlap::`, `scripts/cargo.sh build -p cpd --test flow`, `cargo test -p cpd-core spec::`, `node --test pi/test/x.test.ts`, `npm test -- pi/test/x.test.ts`, `pnpm test -- pi/test/x.test.ts`, `pnpm test pi/test/x.test.ts`, `pnpm run typecheck`, `pnpm install --frozen-lockfile`, `npm run typecheck`, `cargo build`.

A pattern counts where a command runs, as in §15.11 (a command word of a simple command, not an argument, quoted text or a heredoc body, which §15.11 skips: `grep "cargo test" f` passes).

**The refusal** is `{block: true, reason}`, with the reason on one line naming the rule:

```
cpd: a Worker runs only the tests its change touches; the full suite is the Gate's, run by the PR's open Job; refused: <what matched>
```

`<what matched>` is the command word and its subcommand, with `--workspace` when it was given (`cargo.sh test --workspace`, `cargo nextest run`, `npm test`, `npm run verify`). For pnpm the matched text is `pnpm test` (for `test`, `t` and `tst`), `pnpm run test` (for `run test` and `run-script test`) and `pnpm run verify` (for `run verify`, `run-script verify` and the bare `verify`).

**Not affected:** a Lead or a Manager, whatever it runs; every other tool; the Gate itself.

### 15.13 Questions stay short

Decision (User, 2026-10-20, hs-d502, which supersedes hs-d468's rule that a Plan with drafts never uses `ask_user_question`; hs-d503 keeps the 1500-character limit; hs-d548 adds guards that questions cite only what the User has seen). `ask_user_question` is for a few short choices and a Plan's open choices go to the User through it, as rounds with a recommended option, **in every phase and whatever the Plan's Jobs**; the Plan review is a summary and a dialog in the chat (§15.5). The adapter enforces what stays of the rule with guards, as §15.11 and §15.12 do for their rules. **The guard G1** **was deleted** (hs-d738): its code, its message and its tests; the adapter no longer reads `cpd plan show` for it. The guards G2 and G4 stay; G3 is deleted.

**When.** At pi's `tool_call`, for an `ask_user_question` call (`event.toolName`; the input is `{questions: [{question, header, options: [{label, description, preview?}], multiSelect?}]}`), in the same handler as §15.11 and §15.12, with the guard in its own module.

**G2, a long question.** For any Session (Lead, Manager, side fork), the call is refused when its text is over **1500 characters**. The text counted is the sum of every question's `question` and `header` and every option's `label`, `description` and `preview`. The limit is one constant, with no config key. The reason, on one line:

```
cpd: ask_user_question is for a few short choices; write anything larger to a file outside the repo and point to it in chat; refused: <n> characters (limit 1500)
```

The refusal is `{block: true, reason}`.

The tool's own description, its guidelines and the descriptions of its `question` and `preview` fields state the same limit before the call (contract §6.10; User, 2026-10-23, hs-d593); G2 and the number stay here.

**G4, labels stated nowhere** (hs-d553; hs-d548 part 1 and G3 superseded by hs-n425). A question must state what it asks about: every label must occur in the call's text. This guard applies to Sessions of role `lead` or `manager` (side forks of those roles included), not workers. The refusal comes before the User sees the question.

**Assumptions B1–B5.** B1: 'The call states a label' means the label token (\b[A-Z][0-9]+\b, as today's A3) occurs in the call's text: every question's `question` and `header`, every option's `label`, `description` and `preview`. A range X<m>–X<n> (en dash U+2013 or '-', same letter, m<n) in the call states its two ends only; its middle members count as cited but not stated, so they must occur as tokens somewhere in the call or the visible chat. Consequence: only the middle members of a range can be refused. B2: 'Visible chat' is unchanged (old A2): the whole branch up to the call: assistant `text` blocks (those of the calling message before the call included), `user` messages, `toolResult` text, `custom_message` entries with display true; never thinking blocks or tool-call inputs. A label is shown when its token occurs in the call's text or in the visible chat. B3: The turn logic of the old A1 goes away entirely: nothing is read about where the turn starts. B4: Roles unchanged (old A5): G4 applies to Sessions of role lead or manager (side forks of those roles included), not workers. B5: G4's reason, one line, exactly: `cpd: state what you ask about in the question itself (its text, or an option's description or preview); refused: <labels, comma-space separated, in order of first citation> stated nowhere` for example `...; refused: D2, D3, D4 stated nowhere`.

**G4 process.** The call is refused when it cites a label (stated or not; a range cites all its members) that is not shown. Cited but not stated labels form the refusal. The reason:

```
cpd: state what you ask about in the question itself (its text, or an option's description or preview); refused: <labels, comma-space separated, in order of first citation> stated nowhere
```

Examples: (1) a call citing D1–D5 and stating D1 and D5 only: refused `D2, D3, D4 stated nowhere`. (2) a call stating D1, D2, D3, D4, D5 individually: passes. (3) a call citing A4 with no chat text at all: passes. (4) D2 shown in a user message, but the call cites D1–D5 with D1 and D5 stated: only D3, D4 named in refusal (D2 is shown). The refusal is `{block: true, reason}`.

**G3 is deleted** (hs-d553 based on hs-n425): no more 'no visible text in this turn' check. Its code, reason, tests and docs are removed. The rule now: prose an agent writes next to a tool call lands in hidden thinking, not the chat, so 'write it in the chat first, then ask' cannot work; the agent must state what it asks about in the call itself.

**Not affected:** a Worker (it has no such tool); Sessions of role `worker`; every other tool.

**The ask pill.** The `[ask_user_question]` pill of the tool row (contract §6.1.1, §6.9) is magenta: `ESC[35m` U+E0B6 `ESC[7m` TEXT `ESC[27m` `ESC[35m` U+E0B4 `ESC[39m` exactly while the tool is executing and has no result, the window in which its closing row is ` ╰󰬦 choose` (the question is open and the User must decide). Reason: a question is a decision before the work runs and that is magenta. Before the execution (arguments streaming) the pill is the running cyan 6 like any tool; after the result it is 2 (success) or 1 (failure). Every other tool's pill is unchanged.

**The question widget's header.** The widget is cpd's own copy of the package `@juicesharp/rpiv-ask-user-question` (`pi/ask-user-question/`, contract §6.10; only Leads and Managers have the tool). In single-question mode it draws the question's `header` as `theme.bg("selectedBg", " <header> ")` (a padded block, blue in our theme). The adapter turns that call into a read-only title pill, as it does for the jump hint (contract §6.1.1 "The jump hint"): caps U+E0B6 and U+E0B4, no padding spaces, `[Assumptions]` as the bytes `ESC[37m` U+E0B6 `ESC[7m` `Assumptions` `ESC[27m` `ESC[37m` U+E0B4 `ESC[39m` (colour 7 inverted: text in the terminal's background colour, caps 7: grey, as every in-pane title, §17 "Section titles"; never clickable). Assumption the User may correct: grey, not magenta; the colour is one constant. Other `selectedBg` uses (the selectors) are untouched.

**The ask dialog never hangs.** (hs-n605, User, Plan `ask-dialog-stuck`; fork-only fix: no pi-coding-agent change, no cpd adapter change.) The cpd adapter's event-driven plan-review dialog (`pi/lib/review-chat.ts` `runDialog` -> `ctx.ui.select`) mounts into pi's single editor slot and can displace the inline `ask_user_question` questionnaire, clearing the slot without ever settling the questionnaire's `ctx.ui.custom()` promise, which hangs the tool's `await mount()` until the pane is closed. The widget (cpd's copy of the package, contract §6.10) must never hang: it either renders, or resolves a loud non-blocking result the model can act on. An inline mount wraps the mounted component in a render guard: every `RENDER_GUARD_TICK_MS` (1000 ms) a tick invalidates the mounted component, so pi-tui renders it, and when the component's render has not been called for `RENDER_GUARD_LOST_TICKS` (3) consecutive ticks (~3 s), the slot was taken by another dialog (pi's one editor slot) and the mount resolves a loud result. The render guard applies to the inline mount only: the overlay stacks and cannot be displaced. Every mount, inline and overlay, is bounded at `MOUNT_BOUND_MS` (2 hours): a mount that neither resolves nor is displaced resolves loud at the bound. Both resolutions return an error-shaped result (`answers: [], cancelled: true`, `isError` true) with the distinct new error codes `dialog_displaced` and `dialog_stuck` (added to `QuestionnaireError` in `pi/ask-user-question/tool/types.ts`) and the model-facing sentences, quoted verbatim:

code `dialog_displaced`:

```
Error: the question dialog disappeared before the user answered (another dialog took the single dialog slot). The user never saw the questions — do NOT treat this as a decline. Ask the questions as plain chat text instead, without using this tool.
```

code `dialog_stuck`:

```
Error: the question dialog did not settle within 2 hours and was closed. The user never saw the questions — do NOT treat this as a decline. Ask the questions as plain chat text instead, without using this tool.
```

The model then re-asks the questions as plain chat text. Esc keeps cancelling a rendered question (unchanged); the guard stops when the mount settles.

**The question widget's tab bar** (User, 2026-10-22, Plan `pi-theme`; the colours are the User's, the Submit rule and the cut are the Lead's assumptions). With two or more questions the widget (cpd's copy of the package, contract §6.10; the tab bar rules below do not change) shows a tab bar. Ours is one row: one leading margin column, then pills joined by a single `─` in colour 8, no spaces: `[D4 plain pi]─[D5 key source]─[D6 machine allow]─[D7 cut]─[Submit]`. Pill, colour n (§6.1.1, title rule hs-d475): `ESC[`<code for colour n>`m` U+E0B6 `ESC[7m` `<label>` `ESC[27m` `ESC[`<code for colour n>`m` U+E0B4 `ESC[39m` (inverted), no padding spaces. Colour code: `ESC[3nm` for n in 0-7, `ESC[9+(n-8)m` for n in 8-15. Separator: colour 8 as indexed SGR `ESC[90m` `─` `ESC[39m`. Colour by state, in priority order: current question blue 4 (selection wins, even if answered); answered green 2; unanswered 7 (grey). The last pill `Submit`: blue 4 when current, else green 2 when every question is answered, else 7. No arrows `←`/`→`, no `■`/`□`/`✓` marks, no terminator after `[Submit]`. A line wider than the pane is cut at the pane width, never wrapped; the empty second row of the tab bar stays. Pills are not clickable (keyboard-driven widget). The line is produced from the widget's own props (`tabs`: label, answered, active; `submit`: active, allAnswered) and the width by a render hook recognising the `TabBar` component by class name and props shape, installed idempotently and removed at `session_shutdown`; the hook does not edit the widget's files and no text pattern is matched. The single-question header pill is unchanged.

### 15.14 Mentions of Leads and Managers (hs-d544)

User decision hs-d544 (2026-10-22, owner Plan `pi-theme`). Display only. In the pane of a **Lead or a Manager** (the adapter knows its Session's role; never a Worker's), a mention of another Lead or Manager in the **assistant's answer text** is drawn magenta: colour 5, text and not a block, `ESC[35m`…`ESC[39m` (contract §6.1.1).

- **What is scanned:** the assistant's answer text (the text parts of its message, not thinking and not tool rows), **outside code**: inline code spans and fenced code blocks are left alone (decision of this Plan: a quoted command is not a mention).
- **A mention** is a whole token (bounded by non-word characters, `[A-Za-z0-9_-]` counting as word characters, so `cpd2-x` does not match `cpd2`): a Session UUID or its 8-character prefix, a Plan id, or a Context id, of a Lead or Manager that exists, live or archived. Plan ids that are plain words (`distill`, `yield`, `delete`) light up in ordinary prose too; the User accepted that.
- **The set of ids** comes from the commands the adapter runs: `cpd session list --archived --role lead --role manager` (§10.1: `id`, so its 8-character prefix and `plan`), `cpd plan list` (§4.2: `id`, archived ones included) and `cpd context list` (§3.2: `id`, archived included). A Plan or Context id counts only when a Lead (or Manager) Session exists for it.
- **Refresh:** at `session_start` (§10.8) and on Events of these kinds: `session-registered`, `plan-created`, `context-created`, `plan-deleted`, `context-deleted`, `session-renamed`, `plan-renamed`, `context-renamed`, `session-archived`, `plan-archived`, `context-archived` (hs-n423); every other Event refreshes nothing. A failed command keeps the previous set and shows the adapter's error as §15.1 does, once per error code, until a different error is shown. A Lead's follower only gets its Plan's Events (§11), so ids created elsewhere show after the next such Event or restart.
- **Drawing:** pi renders the assistant's markdown itself, so the adapter paints on the rendered lines: the assistant render override of `pi/compact-tools/extensions/compact-tools-silent.ts` (`RenderOverride`, `assistant`) wraps each rendered line's matching tokens in `ESC[35m`…`ESC[39m`, outside code. Idempotent: the same text gives the same lines.

### 15.15 The loop guard (hs-d552, hs-d571)

**L3 Counting** (the pi adapter; for a Lead and a Manager only). A *turn* is a pi agent run. It is **the User's** only when pi's `before_agent_start` event has `messageType: "user"` (a prompt the User typed). Every run a cpd message starts (`messageType: "custom"`: Events, a `session-message` with `from` null, `attempt-nudge`, a Plan review decision) is a **self-started turn**. The adapter keeps the start times of the self-started turns of the last 600 000 ms (a sliding window) from one clock, `options.now` (default `Date.now`).

**L4 Trip** (the adapter; trigger on the pi adapter, L4 of hs-d552). When a self-started turn starts and the window then holds MORE than `loop_guard_turns` turns (with the default 20: the 20th does not trip, the 21st does), the breaker trips. That 21st turn runs, pi has started it; from then on: (a) every message the adapter delivers with `triggerTurn: true` is sent with `{triggerTurn: false, deliverAs: "nextTurn"}` instead, through the one gate `gated` of `pi/lib/delivery.ts` that §15.7a's first-message hold already uses, so deliveries keep queueing, keep their order and keep moving the cursor but start nothing; (b) the adapter opens the blocking reason `loop-guard`, kind **human**, label `φ: loop guard` (§15.4); (c) it appends the pi session entry `cpd-loop-guard` `{tripped: true, at, turns}`.

**L5 The User's prompt** (`before_agent_start` with `messageType: "user"`) always runs. It empties the window; and when the breaker is tripped it also clears the trip: the reason closes, the entry `{tripped: false}` is appended and deliveries trigger turns again. Nothing else resumes it: no timeout.

**L6 Resume from the Inbox**. `cpd session resume <id>` (§10.17) appends the Event `session-resumed`. On it, for its own Session while tripped, the adapter empties the window, clears the trip as L5 and delivers ONE message with a turn (the Event's text), so one turn starts and sees every delivery queued meanwhile. While not tripped it does nothing: no message, no turn. The turn it starts is a self-started turn (the window holds 1).

**L7 Reload** (L7 of hs-d552). The trip survives a pi reload. At `session_start`, before the follower starts, the adapter reads its pi session's last `cpd-loop-guard` entry; `tripped: true` re-opens the reason and the gate at once. The window is not persisted: a reload empties it.

**Sends that use the gate** (every send of a message with `triggerTurn: true`): `session-message` Events, `attempt-nudge`, the adapter's errors, the Plan review decision message. All of these go through `gated` when the breaker is tripped, so they queue without starting turns.

### 15.16 A primary checkout stays put (hs-n436, hs-d606, hs-d608)

Decision (User, 2026-10-22, hs-n436; it applies hs-d573, fix the cause and hs-d248, a guard only for behaviour seen in practice, which this is; hs-d606 and hs-d608 widen it to every tool). No agent changes the files, the index or HEAD of a repo's primary checkout, with any tool: every repo of the machine, every role (Lead, Manager, Worker, side forks), while a run is active. On 2026-10-21 Leads ran `cd ~/projects/cpd && git checkout -q --detach origin/main` to read the latest main; HEAD stayed detached, dotfiles-sync installs cpd only from a checkout on `main` and hs took no new cpd for 14 hours. On 2026-10-24 two Workers (Plans anim-perf and sync) changed `docs/contract.md` and `docs/commands.md` of cpd's primary checkout with the `edit` tool by absolute path (their Specs named it); the checkout went dirty and dotfiles-sync, which fast-forwards only a clean `main`, stopped installing cpd. A change reaches a primary checkout only through a merged PR and the sync that fast-forwards it. The adapter enforces the rule, as §15.11 and §15.12 do for theirs, so it is not left to the skills' prose.

**A primary checkout** is the main working tree of a git repository: the directory that holds a `.git` *directory* (the directory of a Plan's repo, `prs.repo`). A linked worktree (a Lead worktree, an Attempt worktree, a throwaway one made with `git worktree add`) holds a `.git` *file* and is not one. A directory is *in* a primary checkout when the nearest `.git` entry at or above it is a directory: a subdirectory of a primary checkout is in it; a subdirectory of a linked worktree is not, even when that worktree sits inside a primary checkout; a directory with no `.git` at or above it, or one that does not exist, is in none. For a write target, a path that does not exist is in the primary checkout of its nearest existing ancestor (a write creates the missing directories) and a path below a checkout's `.git` is in it. Only the file system is read (`lstat`), never `git` or `cpd`.

**When.** At pi's `tool_call`, of a Session of **every role** (Lead, Manager, Worker, side forks) while a run is active (`session_start` has read the role), in the one handler of §15.11 and §15.12 and before their per-role guards. The code is `pi/lib/mainguard.ts`: `primaryCheckoutMove(command: string, cwd: string, home: string): string | undefined`, `primaryCheckoutWrite(toolName: string, input: unknown, cwd: string, home: string): string | undefined` (each returns the refused call as the reason shows it, or `undefined`; the adapter passes the context's `cwd` and `os.homedir()`) and `reasonText(matched: string): string`. Its tokenizer is the one §15.11 and §15.12 share (`pi/lib/shell.ts`), not a third copy. The handler (`pi/lib/adapter.ts`), while `host.run` is set: `edit` and `write` call `primaryCheckoutWrite`; `bash` calls `primaryCheckoutMove`; any other tool is not judged by this guard and a call that matches nothing returns nothing.

**What is refused.** A call is refused for three kinds: a git subcommand, a bash write form and an `edit` or `write` call.

**Git.** A git command whose subcommand is one of **seventeen** — `checkout`, `switch`, `reset`, `rebase`, `merge`, `pull`, `stash`, `restore`, `clean`, `commit`, `cherry-pick`, `revert`, `am`, `apply`, `rm`, `mv` and `add` (they change files, the index or HEAD) — with any arguments (`clean -n` and `apply --check` included), run in a directory that is in a primary checkout. The subcommand is the word as written: `merge-base` and `merge-tree` are other subcommands and pass. The matched text is `git <subcommand> in <root>`. Everything else passes, among it `fetch`, `log`, `show`, `diff`, `status`, `grep`, `rev-parse`, `ls-files`, `branch`, `tag`, `config`, `remote`, `update-ref`, `merge-base`, `merge-tree`, `worktree add` and `worktree remove`: a ref move (`update-ref`, `branch -f`) does not dirty a diff and stays not judged.

**The `edit` and `write` tools.** For the pi tools `edit` and `write` the adapter calls `primaryCheckoutWrite(toolName: string, input: unknown, cwd: string, home: string): string | undefined`. It returns `undefined` for any other tool, for an `input` without a non-empty string `path` (no other field is read) and for a path that is not in a primary checkout; otherwise the matched text `<tool> <path> in <root>`, for example `edit <abs path> in <root>`. The path is resolved as pi resolves tool paths: one leading `@` dropped; `~` and `~/…` from `home`; an absolute path as it is; any other relative to `cwd`; then normalised lexically (`.` and `..`; never `realpath`, so a symlink into a primary checkout is not followed: not judged). In the matched text `<path>` is that absolute path and `<root>` the root of the primary checkout.

**Bash writes.** `primaryCheckoutMove(command, cwd, home)` keeps its name and signature and now also returns the matched text `<form> <path> in <root>` of a write form. The command is read as today (the simple commands of the shared tokenizer, the prefixes, the `cd` tracking, the current directory); each simple command is judged for its git form and its write forms in one walk and the first match in order is returned. The write forms and the words each judges:

- **Redirection.** A word made of optional digits, then `>`, `>>` or `>|`, then optionally the target (`>f`, `>>f`, `2>f`, `>|f`); an operator word alone takes the next word of the same simple command. `&>f` is read as `>f`, because the tokenizer ends a command at `&`. A redirect with no target word in its simple command (`2>&1`: the `&` ends the command) is not judged. `<form>` is the operator without its digits (`>`, `>>`, `>|`). Reads (`<`, `<<`, `<<<`) are not writes.
- **Command words** (after the prefixes; the word or a path ending in `/<word>`; `<form>` is the word without its directory): `tee`, `rm`, `touch`, `truncate` and `mv` judge every operand (a word not starting with `-`); `cp`, `install` and `ln` judge the last operand and the value of `-t <dir>` or `--target-directory=<dir>` (their other operands are sources that are only read and pass); `sed` and `perl` with an in-place flag (a word that is `-i`, starts with `-i`, is `--in-place`, starts with `--in-place=`, or is a short-option cluster ending in `i` such as `-ni` or `-pi`) judge every operand; `dd` judges the value of an `of=<path>` operand; `patch` is judged by the directory it runs in (the current directory, or the value of `-d <dir>` or `--directory=<dir>` resolved from it), its operands are not read.
- **Resolving a target word.** As a directory word of this section (`~`, `~/…`, `$HOME`, `${HOME}`, relative to the current directory, lexical), extended: a word with `*`, `?`, `[` or `{` is judged by its literal leading directory, the segments before the first segment that holds one of them (`<root>/docs/*.md` is judged as `<root>/docs`); a word that holds `$` or a backtick, a `~user` and `-` cannot be resolved and is not judged; a relative word is not judged when the current directory is unknown. A resolved target is in a primary checkout by the definition above. Only the first matching target of a command is reported; `<path>` is the resolved absolute path (for a glob its literal directory, for `patch` the directory).
- **Everything that only reads passes:** `cat`, `grep`, `ls`, `head`, `sed -n`, `cp` from a checkout, `git show`, `echo rm <path>`, `grep 'rm <path>' f`.

**Where a command runs.** The call is read as its simple commands (the tokenizer of §15.11: split at `;`, `&`, `|`, a newline, parentheses and backticks, quotes taken off; heredoc bodies are skipped, §15.11; a command is where a command word stands, so `echo git checkout` and `grep "git reset" f` pass). The command word is `git`, or a path ending in `/git`, after the prefixes of §15.11 (`FOO=1`, `time`, `nohup`, `exec`, `command`, `sudo`, `env`, `do`, `then` and the like). The directory starts as `cwd`. A `cd <dir>` (the first word that does not start with `-`; none means `home`) sets it for every later command of the call, in whatever order the separators put them, a subshell's `cd` included: the guard errs towards refusing. `git`'s global options before the subcommand are skipped as the `git` wrapper of contract §6.5 skips them (`-c <k=v>`, `--git-dir <d>`, `--work-tree <d>`, `--namespace <n>`, `--super-prefix <p>`, `--config-env <k=v>`, their `--opt=value` forms and flags such as `--no-pager`, `-p` and `--bare`), except `-C <dir>`, which changes the directory for that command only, each `-C` relative to the one before it. A directory word is resolved as the shell would: `~`, `~/…`, `$HOME` and `${HOME}` (alone or followed by `/…`) from `home`, a relative word from the current directory, then normalised lexically (`.` and `..`, no `realpath`). A word that still holds `$`, a backtick, `*`, `?`, `[` or `{`, a `~user` and `cd -` cannot be resolved: that command and every later one until a `cd` that resolves, is **not judged** (the throwaway form `cd $(mktemp -d)` must pass).

**Not judged**, because the guard stops mistakes and not a determined bypass: `sh -c '…'`, `xargs`, scripts and interpreters (python, node, make, cargo, npm: a build writing its ignored output dir included), editors, `rsync`, `tar`, `unzip`, `curl -o`, `wget -O`, `mkdir`, `rmdir`, aliases and functions, `sed` without an in-place flag, symlinks, a `$VAR` or command-substitution path, `GIT_DIR`, `--git-dir`, `--work-tree` (they do not change the directory the guard judges) and `patch` operands.

**The refusal** is `{block: true, reason}` for the three kinds, with `reasonText(matched)` giving this one line (the four read alternatives stay; the way to change files is new, before `refused:`):

```
cpd: no agent moves HEAD or changes the files of a repo's primary checkout; fetching is allowed, then read origin/main with 'git show origin/main:<path>', 'git log origin/main' or 'git diff origin/main', or in a throwaway 'git worktree add $(mktemp -d) origin/main' that you remove afterwards; to change files, work on a branch in a worktree ('git worktree add -b <branch> <dir> origin/main', or the worktree of your Job) and let a PR carry the change; refused: <matched>
```

`<matched>` is `git <subcommand> in <root>` for a git command (the refused subcommand and the root of the primary checkout, the directory that holds its `.git` directory, an absolute path as resolved above, never a `realpath`), `<form> <path> in <root>` for a bash write form and `<tool> <path> in <root>` for an `edit` or `write` call.

**Examples.** The Session's `cwd` and `home` are `/home/clayton` and the example repo's root is `~/projects/cpd` (`<root>`). Refused: `edit` and `write` of `<root>/docs/x.md` by absolute path, by `~/…`, by a `@`-prefixed path and by a relative path from a subdirectory; `write` of `<root>/new/dir/f.md` (the directories do not exist); `echo x > <root>/f`; `cat <<EOF >> <root>/NOTES.md`; `echo x | tee <root>/f`; `sed -i s/a/b/ <root>/f`; `rm -rf <root>/src`; `rm <root>/docs/*.md`; `cp /tmp/a <root>/b`; `mv <root>/a /tmp/b`; `touch <root>/f`; `cd <root> && patch -p1 < /tmp/x.diff`; `git -C <root> commit -m x`; `cd <root> && git add .`. Passes: the same tools and commands in a linked worktree (also one inside the primary checkout) and outside any repository; `cat <root>/f`; `cp <root>/a /tmp/b`; `cmd 2>&1`; `cmd > /dev/null`; `git -C <root> fetch origin`; `git -C <root> show origin/main:README.md`; `echo x > $OUT`; `rm $DIR/f`. Today's git-only refused forms stay refused: `cd ~/projects/cpd && git checkout -q --detach origin/main`, `git -C ~/projects/cpd switch -c x`, `cd ~/projects/cpd/src; git reset --hard origin/main`, `git -C ~/projects/cpd rebase origin/main`, `git -C ~/projects/cpd merge origin/main`, `git -C ~/projects/cpd pull`, `git -C ~/projects/cpd stash`; today's git-only passes stay: `git -C ~/projects/cpd log origin/main`, `git -C ~/projects/cpd show origin/main:README.md`, `git -C ~/projects/cpd diff origin/main`, `git -C ~/projects/cpd status`, `git -C ~/projects/cpd grep -n x origin/main`, `git -C ~/projects/cpd worktree add $(mktemp -d) origin/main`, `git -C ~/projects/cpd merge-base HEAD origin/main`; each command of the refused list run in a linked worktree (`cd ~/projects/cpd.pay.api && git merge origin/main`); every command outside a repository.

**The skills.** Where the cpd-lead and cpd-manager skills tell an agent it may read a repo, they name the allowed forms only: `git fetch origin`, then `git show origin/main:<path>`, `git log origin/main`, `git diff origin/main`, or a throwaway `git worktree add $(mktemp -d) origin/main` removed afterwards; and they say that HEAD is never moved in a primary checkout. No skill file (a `SKILL.md` or a file of its `references/`) contains `git checkout`, `git switch`, `git reset`, `git pull`, `git stash` or `--detach`. The tests of this section read the skill files for both.

**The `git` wrapper stays.** `$CPD_HOME/bin/git` (contract §6.5, hs-d310) still refuses `git stash` in every directory of every Session, for its own reason (`refs/stash` is shared by every worktree). In a primary checkout this guard answers first, with the reason above.

### 15.17 Secrets in the pi adapter

Decision (User, 2026-10-22, accepted assumptions A1–A5). Agents never call 1Password's `op` directly; the pi adapter refuses it and offers `cpd secret` commands instead (contract §8.4, commands §24). cpd holds no `op` token and never signs in; the 1Password app's per-use prompt is the approval. The adapter enforces the raw-`op` guard as §15.11 and §15.12 do for theirs and provides the `cpd_secret_approve` tool for a Lead or Manager to request a secret outside the `cpd` and `homeserver` vaults.

#### The raw-op guard

**When.** At pi's `tool_call`, for a `bash` call, of a Session of **every role** (Lead, Manager, Worker, side forks) while a run is active, in the one handler of §15.11 and §15.12 and before their per-role guards. The code is `pi/lib/secretguard.ts`: `secretBypass(command: string): string | undefined` (the refused command as the reason shows it, or `undefined`; the adapter passes the command text from pi) and `reasonText(matched: string): string`. Its tokenizer is the one §15.11 and §15.12 share (`pi/lib/shell.ts`), not a third copy. For any other tool and for a call that matches nothing, the handler returns nothing.

**What is refused.** A simple command (the tokenizer of §15.11: split at `;`, `&`, `|`, newline, parentheses and backticks; quotes taken off; a command is where a command word stands, so `echo op read` and `grep "op read" f` pass) whose command word, after the prefixes of §15.11 (`FOO=1`, `time`, `nohup`, `exec`, `command`, `sudo`, `env`, `do`, `then` and the like), is `op` or a path ending `/op`, with any arguments: the matched text is `op` followed by its first argument when it has one (`op read`, `op item`, `op`); and the command `cpd secret approve` (the matched text `cpd secret approve`): approvals are the User's, written only by the adapter after its dialog. Every other `cpd secret ...` command passes.

**Not judged**, because the guard stops mistakes and not a determined bypass: `op` run through `sh -c '...'`, `xargs`, a script, an alias or a function.

**The refusal** is `{block: true, reason}`, with the one-line reason:

```
For op or path ending /op with arguments:
cpd: agents do not call 1Password's op; use cpd secret get|list|create|edit|delete (commands.md §23); refused: <matched>

For cpd secret approve:
cpd: approvals are the User's: ask with the tool cpd_secret_approve (commands.md §15.17); refused: cpd secret approve
```

`<matched>` is the command word (`op`, `/path/to/op`, `op read`, `op item list`) or `cpd secret approve`.

**Examples.** Refused: `op account get`, `op item list --vault cpd`, `/usr/bin/op read item/my-db-password`, `op`, `cpd secret approve --action get --vault cpd --item x`. Passes: `cpd secret get my-item -f password --vault cpd`, `cpd secret list --vault cpd`, `cpd secret create my-item --vault cpd`, `cpd secret edit my-item --vault cpd`, `cpd secret delete my-item --vault cpd`, `echo op` (argument), `grep "op read" file` (grep's argument).

#### The tool `cpd_secret_approve`

**When.** At pi's `tool_call`, registered once at load like `cpd_plan_review`, called from the adapter's start-up where `registerTool` is. The code is `pi/lib/secret.ts`: `registerSecretTool(host: Host): void`.

**Role and vault.** It is for a Lead or Manager only: any other role (Worker), or no active run, is refused with the CpdError `not_a_lead` and a message `the tool cpd_secret_approve is for Lead and Manager Sessions only`. The vaults `cpd` and `homeserver` are refused with the reason `these vaults need no approval; use cpd secret get|list|create|edit|delete (commands.md §23)`.

**Parameters:** `action` (one of `get`, `list`, `create`, `edit`, `delete`), `vault`, `item` (every action but `list`), `field` (required for `get`, `create` and `edit`), `reason` (one line, at most 200 characters: why the agent needs it).

**The dialog.** pi's `ui.select` run as the Plan review's chat dialog (facts.ownPrompt, so the agent state shows the Session waiting on the User), titled:

```
<session name> asks to <action> in vault <vault>: item <item>, field <field>. Reason: <reason>
```

with the options `Allow once` and `Deny`. **`Allow once`:** the adapter runs `cpd secret approve --action <action> --vault <vault> [--item <item>] [--field <field>]` (commands §24.7) atomically, writing `$CPD_HOME/run/secret/approve/<uuid>.json` with `{session, action, vault, item, field, at}`. The tool returns the text `Approved for one command in the next 10 minutes: run it now.` **`Deny`:** nothing is run and the tool returns `The User denied it. Do not try again.` **No dialog available** (no UI): an error `no_ui` saying the cockpit chat is needed.

#### A Worker's way

A Worker that needs a secret for an Attempt escalates to its Lead with `cpd ask <attempt-id> "<question>"`, naming the vault, item, field and why. The Lead reads the escalation, then grants the Worker's access for one use with:

```
cpd secret allow <item> --attempt <attempt-id> --vault <vault> --field <field>
```

This writes `$CPD_HOME/run/secret/allow/<attempt-id>.json` (contract §8.4). The Lead then answers the Escalation with `cpd answer <esc-id> "<answer>"`. The Worker runs `cpd secret get <item> --vault <vault> --field <field>` (or the other command the Lead approved). The allowance lasts until the Attempt ends (the store says); every Worker never gets another vault. For a Worker to request a secret from a different vault than `cpd` or `homeserver`, the Lead must first call `cpd_secret_approve` and have the User approve it through the dialog, then allow the Worker.

### 15.18 The pi tool `cpd_host_approve`

**Manager only** (internal): a pi tool the adapter runs on behalf of the host Manager after the User's dialog choice. Parameters: `reason` (one line, at most 200 characters; the reason the repair is needed, from the install state file). Dialog `Allow once` / `Deny` (buttons; no title given by pi, so the tool provides it in the message). Button text: the dialog says `Allow once: approve the repair. It discards tracked changes of ~/projects/cpd and resets main to origin/main after saving the diff and the old HEAD.` (brief, from the spec). On `Allow once`: run `cpd host approve` and tell the Manager to run `cpd host repair` now (the message unblocks it). On `Deny`: say so and not to retry (contract §3.7 E6). The adapter refuses an agent's bash `cpd host approve` with the reason `cpd: approvals are the User's: ask with the tool cpd_host_approve (commands.md §15.18); refused: cpd host approve` (model: §15.17).

### 15.19 The compaction ratio (hs-d660)

pi's compaction default `compaction.reserveTokens: 16384` makes a model without a per-model reserve compact only at `contextWindow - 16384` (~98% full), so models with a large window (the OpenRouter DeepSeek models) fill up and never compact. The fix is one pi setting, `compaction.reserveTokensRatio`: a fraction of the model's own context window reserved for the response.

The setting is read from pi's settings.json (global and trusted-project, merged by pi's SettingsManager). cpd adds no file and no key of its own.

The module is `pi/lib/compaction-ratio.ts`, loaded by the adapter (`extension.ts`, section 15.1). `pi/compact-tools` is NOT involved. The module hooks `SettingsManager.prototype.getCompactionReserveTokens`.

For a model with a positive contextWindow: `reserveTokens = Math.round(contextWindow * reserveTokensRatio)`, so compaction triggers at `used > contextWindow - reserveTokens = contextWindow * (1 - reserveTokensRatio)` (pi's `shouldCompact`). The value is derived on every call from the model in hand; it is never written to the session file or to the settings.

Precedence, per model key `provider/modelId`, highest first: (1) `compaction.modelOverrides["<key>"].reserveTokens` (an explicit absolute value, wins over every ratio); (2) `compaction.modelOverrides["<key>"].reserveTokensRatio` (a ratio for that model); (3) `compaction.reserveTokensRatio` (a ratio for every model); (4) `compaction.reserveTokens` (an explicit absolute value for every model, used only when no ratio applies); (5) the built-in ratio `0.1`. The default behaviour is therefore to compact at 90% of the window, whatever the model.

A model whose contextWindow is absent or not a positive number keeps pi's own value (the model override or pi's default). `branchSummary.reserveTokens` is a separate setting and is unchanged.

A ratio outside (0, 1), or one that is not a finite number, is refused: `getCompactionReserveTokens` throws an Error whose message names the exact key (`compaction.reserveTokensRatio`, or `compaction.modelOverrides["<key>"].reserveTokensRatio`). There is no silent fallback.

The ratio applies in every cpd pi session; a plain pi that does not load the adapter is not covered.

### 15.20 A Lead runs a command the User explicitly asks for (pro-d56)

Decision (User, 2026-10-26 11:53 EDT, relayed by the Lead of Plan preco-home-buttons): a Lead runs a command the User explicitly asks it to run. Confirmed and scoped by decision pro-d57 (User, 2026-10-26 12:09 EDT, accepted assumptions A1–A4): A1 the adapter's guards stay for a Lead; A2 only an explicit request counts, never an inferred one; A3 the rule covers Leads only, a Manager's rules are unchanged; A4 the Lead skill's standing bans stay (tmux, wt, git push, product code and commits and what belongs to cpd itself: `cpd plan decide`, `cpd watch`).

**The rule.** A Lead runs a command the User explicitly asks it to run and does not refuse it because of its role (decision pro-d56, confirmed and scoped by pro-d57, User, 2026-10-26 11:53 and 12:09 EDT). The User asked in words, in the Lead's conversation, for the Lead to run a command and the Lead runs it.

**Explicit only.** The User asked, in words, in the Lead's conversation; a Lead never infers a request from the context, from a Worker's Escalation or from a Manager's message that does not quote the User.

**Leads only.** A Manager's and a Worker's rules are unchanged. A Manager asks a Lead; a Worker runs its own turn; neither is given this latitude.

**What stays.** Every adapter guard applies to the request: `pi/lib/noblock.ts` (§15.11, a bash call that waits over 30 s), `pi/lib/testguard.ts` (§15.12, a Worker's full test suite), `pi/lib/askguard.ts` (§15.13, a question's length and labels), `pi/lib/loop-guard.ts` (§15.15, a loop that blocks), `pi/lib/mainguard.ts` (§15.16, a primary checkout's files), `pi/lib/secretguard.ts` (§15.17, raw `op` and `cpd secret approve`; `op` is answered with `cpd secret`), `pi/lib/hostapprove.ts` (§15.18, `cpd host approve`), . So do the Lead skill's standing bans: tmux, wt, git push and force-push, product code and commits and what belongs to cpd itself (`cpd plan decide`, `cpd watch` except `--ensure`; those last two are skill bans, not adapter guards). When one stops a request, the Lead tells the User what was refused and what to do instead.

**A command that does not exit within 30 seconds** (a dev server, a watcher, a long build) runs in the background: its own process group, its output in the log `/tmp/cpd-<plan>/<name>.log`, its process group id in `/tmp/cpd-<plan>/<name>.pid` (`<plan>` the Plan's id, `<name>` a short name), because the adapter refuses a bash call that waits over the bound (§15.11) and a foreground one would hold the User's conversation. The Lead then tells the User, in one message, the URL or port, the log path and how to stop it.

**A dev server that uses a declared resource** (for example the port an e2e run needs, `cpd resource list`, §30) is started with the prefix `cpd resource run <name> --`, so it holds a slot while it runs and a Job that lists the resource is skipped by E3 and starts when a slot is free, instead of failing on the port. **Nothing is left orphaned:** the Lead stops what it started (the process group, so the children end too) when the User says they are done, when the User asks it to stop and at `plan-done` if it still runs.

**Where it runs.** Where the User says; for a Plan's branch, in the PR's Lead worktree (`lead_worktree` of `cpd plan show`).

**Enforcement.** No guard is added: the adapter already lets a background start through (pi's bash tool starts each command in its own process group and returns when the shell exits) and the rule is prose in the cpd-lead skill, pinned by tests. State the exact shell commands NOT here: they belong to the cpd-lead skill, which a later Job of this PR writes and this section must not repeat them (DRY).

### 15.21 Calls a codemode script makes (hs-d736)

When a codemode script runs in a Session and calls a pi tool nested inside the script, pi 1.3.0 emits a `tool_call` event with `parentToolCallId` set to the script's tool call id and the adapter's handlers receive the nested call (read from pi source `dist/main.js` tool_call handler; confirmed with pi 1.3.0). **Nested calls are judged identically to direct calls**: a nested call goes through the same guard (`pi/lib/*.ts`, §15.11–15.20) that a direct call from a message goes through (assumption A6: the adapter's handlers are called for both direct and nested calls and the handlers do not check `parentToolCallId` to exempt nested calls; verified from the adapter code in 2026-10, cpd.git~1:pi/lib/index.ts `tool_call` handler). No nested-call escape clause, no reduced guards, no lifted restrictions: the adapter treats a bash call the adapter refuses directly the same way whether it is called directly in a message or nested in a script—it is refused. The Worker's `--exclude-tools` list (contract §6.6 "How cpd starts pi" Worker row) remains not callable from a script, because pi's tool exclusion (pi source `dist/main.js` checks a tool before calling its handler) reaches nested calls. When a guard reads the call's place in the session (G4 of section 15.13, which reads the chat up to the call), it takes the place of the parent call (`parentToolCallId`), because a nested call id is never in the transcript. **Evidence** (read and ran from docs/research/codemode-default.md with pi 1.3.0): nested calls reach the adapter's `tool_call` handlers with `parentToolCallId` set; they are rejected by guards the same way direct calls are; pi's exclusion blocks them before the handlers.

### 15.22 An explore Worker's guards (Plan `explore-roles`, R11)

A `scout` or `research` Worker (§5.1, "Explore Jobs") runs under a **guard profile**: pattern refusals in its `tool_call`, keyed on `CPD_JOB_KIND`, which its environment carries for explore Workers only. One pure function is shared by the pi runner (`pi/lib/adapter.ts`) and the durable runner (`pi/durable-worker/guards.ts`, §6.12); the durable runner's `workerVerdict` applies it after its other Worker guards, so both refuse the same calls. **It is not an OS sandbox.** It matches patterns in the command text and in the tool name, which stops mistakes; it does not follow what a program that a command runs does and it does not stop a determined agent. An **unknown `CPD_JOB_KIND` value fails loud**: a `usage` error (exit 2) in both runners, never a fallback to no profile.

A refusal's text starts `φ: ` and names the rule: `φ: <rule>: <what was refused>`, for example `φ: network: curl is refused for a scout Worker`. The rules are `network`, `write`, `scope`, `secret` and `timeout`.

| Rule | Applies to | Refused |
|---|---|---|
| `write` | both | `edit` and `write` outside `$CPD_HOME/run/<plan>/explore/` and the OS temp dir |
| `write` | both | a redirection or `tee` from bash into a path inside a git repository |
| `write` | both | from bash, in a git repository: `git add`, `commit`, `checkout`, `switch`, `reset`, `restore`, `rebase`, `merge`, `pull`, `push`, `fetch`, `stash`, `clean`, `apply`, `cherry-pick`, `revert`, `rm`, `mv`, `worktree` |
| `write` | both | from bash, aimed inside a git repository: `sed -i`, `rm`, `mv`, `cp`, `mkdir`, `touch`, `chmod`, `ln` |
| `timeout` | both | a bash call with no `timeout`, or with one over 30 s (`BOUND_SECONDS`, pro-d202), except a lone `cpd ask` (below), which needs a `timeout` input above 0 and has no upper bound (pro-d274) |
| `scope` | both | `find`, `grep -r` or `rg` whose search root is `/`, `/Users`, `/home`, `~`, `$HOME` or `${HOME}` (a whole-machine scan) |
| `network` | `scout` | `curl`, `wget`, `ssh`, `scp`, `sftp`, `nc`, `ftp`, `telnet`, `dig`, `nslookup`; `git fetch`, `pull`, `push`, `clone`, `ls-remote`; `gh`; `npm`, `pnpm`, `yarn`, `pip`, `brew`; `cargo install`, `publish`, `search` |
| `network` | `scout` | a tool whose name starts `mcp__`, or names web search or web fetch |
| `secret` | `research` | a literal secret in a bash command's argv: a token with the prefix `ghp_`, `github_pat_`, `sk-`, `xox` or `AKIA`; a `Bearer` token; `-u user:password`; `--password` or `--token` with a literal value. A value read from stdin or from a variable is fine |

**The lone `cpd ask`** (pro-d274). A lone `cpd ask` is a bash command line that the shared tokenizer `simpleCommands` (pi/lib/shell.ts) splits into exactly ONE simple command, whose command word, after the prefixes and variable assignments `strip` (pi/lib/explore.ts) skips and with any directory taken off, is `cpd` and whose first argument is `ask`. `timeout 600 cpd ask ...`, `cpd ask ...; ls`, `cpd ask ... && ls`, `cpd ask ... | cat` and `cpd answer ...` are NOT a lone `cpd ask`. For a lone `cpd ask` the `timeout` rule still demands a bash `timeout` input that is a number above 0 (a call with no timeout input stays refused) but sets no upper bound; every other explore rule applies to it unchanged. The durable runner applies the same function (§6.12), so both runners treat a lone `cpd ask` alike.

`git log`, `show`, `diff` and the other read-only commands stay allowed for both kinds. A `research` Worker may reach outside the machine (web, curl, attached MCP servers, registries, upstream docs): the profile has no `network` rule for it. The first `write` row is the one `edit` and `write` calls meet: they are refused outside the explore directory and the OS temp dir.

The explore Worker also may run the read-only `cpd context search` and `cpd context show` (§3.3, §3.4), as its Spec's Deliverable section says (R10); nothing enforces that.

### 15.23 A Session is told its skill changed (Plan `skill-fresh`)

Decision pro-d260 (User, 2026-10-30; measured basis pro-n225: a reload refreshes the skills listing, never a body already read). A running Lead or Manager is told, at its next turn, which of its skill files changed since its baseline. The adapter TELLS the Session; it does not re-feed the body.

**Who.** The adapter of a Lead (a Plan Lead or a side fork, role `lead`) and of a Manager (role `manager`). Never a Worker, which runs without skills. The skill is `cpd-lead` for role `lead` and `cpd-manager` for role `manager`. Only that skill's directory is checked.

**Directory.** `<adapter path>/skills/<skill>`, where `<adapter path>` is the path §15.6 watches: the `pi` link beside the running version directory, or the adapter's own directory when there is no such link. It is the location pi lists for the skill. The code and its tests take it from the same `options.reload.dir` the §15.6 poll uses, or from `adapterPath()` when that is unset.

**Digest.** A map from path to the lowercase hex sha256 of the file's bytes, for every regular file under the directory, recursively. The key is the path relative to the directory, with `/` separators. Symlinks and other non-regular entries are skipped. A directory that cannot be read, or that holds no `SKILL.md`, is unreadable (`skills_unreadable`, below).

**Baseline.** One custom entry, `{customType: 'cpd-skills', data: {skill: <name>, files: <digest>}}`, appended with `pi.appendEntry`; it survives a reload and a restart (A2). The Session's baseline is the LAST valid `cpd-skills` entry whose `skill` is the Session's skill, read from its CURRENT branch only (`ctx.sessionManager.getBranch()`, not the whole session file): a baseline on an abandoned branch describes text that this branch's model never read. An entry of that branch is invalid when its `data.skill` is not a string, or its `data.files` is not an object of 64-hex strings. An invalid entry is reported once, `host.deliver('session entries', CpdError('entry_invalid', '<n> cpd-skills entries of this session do not parse and were skipped'))`, where `<n>` is the number of invalid entries, and is ignored.

**Start** (step 5b of §15.2, which runs at every `session_start`: startup, resume, new, fork and reload, A3). For a Lead or a Manager the adapter reads the digest of its directory, then:
- (a) **unreadable:** it reports `skills_unreadable` (below), appends nothing, tells nothing, and goes on starting;
- (b) **no baseline:** it appends the baseline entry with the digest just read and says nothing: the Session has just read the current text itself, or reads it next;
- (c) **the digest equals the baseline's files:** it does nothing and appends nothing;
- (d) **different:** the Run holds a pending notice (the changes against the baseline), and nothing is appended yet.

**Changes.** For each path in the union of the two maps, sorted ascending by path: `added` (only in the new digest), `removed` (only in the baseline), `modified` (in both, with different hashes). A path with the same hash in both is no change.

**Delivery.** The pending notice goes out in a `before_agent_start` handler, registered once per adapter, the first time a Run holds a notice, as the reminder's is (§15.8); pi has no unsubscribe, so a Session with nothing pending has no such handler. The handler returns `{message: {customType: 'cpd-skills', content, display: true, details: {skill, dir, changes: [{file, change}]}}}` and, at that moment, appends the new baseline entry (the digest read at start) and clears the pending notice. It sends no message with `triggerTurn`: it starts no turn and wakes no idle Session (D2, A4). It is at most once per Run. If the Session ends before a turn, no baseline was written, so its next start compares against the same baseline and says the change again, with whatever changed since, or says nothing when the text is back at the baseline (idempotent).

**Content.** One line, exactly this shape:

`Skill <skill> changed: <path> <change>, <path> <change>. Re-read <dir>/SKILL.md and each file named above before you rely on them.`

`<change>` is one of `modified`, `added`, `removed`, in the order of **Changes**; `<dir>` is the **Directory** above, with no trailing slash. There is no cap on the list: a skill directory holds a handful of files. Example: `Skill cpd-lead changed: SKILL.md modified, references/spec-files.md added. Re-read /opt/cpd/pi/skills/cpd-lead/SKILL.md and each file named above before you rely on them.`

**Error `skills_unreadable`** (an adapter-only code, no exit code; contract §1.2). `what` is `skills check`; `message` is `cannot read <dir>: <reason>` (`<reason>` is the error's message) or `<dir> holds no SKILL.md`. It goes through `host.report` like every §15.1 error (a Lead: a log line and the ledger; a Manager: a chat message) and, like `mcp_register_failed`, it is reported to the ledger with `cpd session report` for EVERY role, so a Manager's reaches the ledger too (§10.18). Unlike `mcp_register_failed`, it is subject to the once-in-a-row memory of `host.report`; the memory is cleared at reload, so each start reports it once.

**Rendering.** `cpd-skills` is one of the types of §15.9 'Which messages': `display` true, one renderer, and its summary is the first sentence of `content`, by the existing rule: here `Skill cpd-lead changed`.

## 16. The Worker contract

### 16.1 The Worker Spec

Rendered by the driver at A3 to `run/<plan>/specs/<attempt>.md` from a template compiled into the binary, so nothing in a repo or worktree can change what Workers are told. For an `open` or `update` Job the PR's base, repo, Lead Branch and key are read from the PR row at A3, after the unstack of A2 (contract §3.4 A3), not from the stored Spec, which keeps the text written at the insert. Content, kept from cpd 1 `templates/worker.md` (cpd 1 §16) with the changes marked:

| Section | Content |
|---|---|
| Title | `# Worker Spec: <job>, Attempt <n>` |
| Who you are | Attempt `<n>` of Job `<job>` (`<title>`) in Plan `<plan>` (cpd 1's name for Plan); attempt id, Tier, worktree path |
| Rules | work only in this worktree and touch only the listed files; commit your work; before submitting, run the quick check yourself: the commands of `pre-commit` from the PR's policy (`[hooks]`, contract §7.3), listed verbatim, or, when the policy has no `pre-commit` hook, the line `This repo defines no quick check.` (change: cpd 1 said `./verify.sh`; R20: no fallback to `pre-merge`); never edit the protected paths, listed: `.cpd/` and each entry of the PR's `protected.paths` (§7.5; change: cpd 1 hard-coded `verify.sh`); never ask the User anything; tear down everything it started before it submits (private tmux servers, background processes, temporary homes, a `cpd watch` it started; hs-d34, hs-d39); if continuing would require guessing, `cpd ask <attempt> "<question>"`, which blocks until the Lead answers (change: no `--wf`, the pane has `CPD_PLAN`); run alone, in the foreground, with the bash `timeout` input at 7200 (pro-d274); one blocker at a time; commit a skeleton within 10 minutes, then at least every 20 minutes; run only commands that exit: `timeout` around every test and measurement, no synthetic benchmarks, never `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run` or any other command that does not exit (hs-n97, hs-n107); run only the tests the change touches, never the full suite (the Gate's); do not stop until you have submitted |
| Job | Goal, Context, Files you may touch (`touches`), Acceptance, Out of scope (or "Nothing is listed. Still do only what the Goal and Acceptance require.") |
| Resources | only when the Job's Spec lists `resources`: a line naming each resource the Job is the tenant of (contract §3.4 "Tenancy") and that `cpd resource run <name> -- <command>` run in the Attempt takes nothing more and runs the command at once (§29) |
| Previous Attempts | only when the Job has ended Attempts: for each, number, Tier, outcome and cause and what explains it: the conflicting files (`merge_conflict`), the failed Gate's hook and log (`gate_failed`, `gate_error`), pi stopped on a provider error with its message (`provider_error`); the provider was down or unreachable, the Job was retried at the same Tier, or one sentence for every other cause (change: cpd 1 showed only feedback) |
| Answers already given | new: every `answered` Escalation of earlier Attempts, question and answer, so a Worker does not ask again |
| Finish | `cpd attempt submit <attempt>`; on success stop at once; on a refusal, what each code means: `wrong_worktree` (run it from the worktree), `dirty_tree`, `no_commits`, `gate_tampered` (revert the protected paths), `attempt_not_running` (the Attempt was stopped: stop) |

**The `cpd ask` line** (pro-d274). The template adds, right after the sentence `Ask one real blocker at a time. Do not ask what this Spec or the code already answers.` (two spaces of indent, inside the same list item, for every kind of Job including explore), exactly this sentence: "Run `cpd ask` as a call of its own, in the foreground, with the bash tool's `timeout` input set to 7200 (seconds). Never put it under the `timeout` command, in a pipeline or in the background: it ends only when the Lead answers or the wait expires."

**Gate causes in Previous Attempts.** For an Attempt that ended `failed` / `gate_failed` or `gate_error`, the entry names the failed Gate's hook (`pre-commit` or `pre-merge`) and log, read from the Attempt's end Event `detail` or, for a requeued Attempt, `feedback`. When the detail has the hook and log:

- **`gate_failed`, hook and log known:** "The Gate (`<hook>`) failed at its merge. Its whole output is in this log; read it and fix what it reports:"
- **`gate_failed`, neither known:** "The Gate failed at its merge."
- **`gate_error`, hook and log known:** "The Gate (`<hook>`) could not be judged at its merge: it was killed, timed out or could not run, which is a problem of the Gate, not a failing check of your work. Its output so far is in this log, if the hook started:"
- **`gate_error`, neither known:** "The Gate could not be judged at its merge: it was killed, timed out or could not run, which is a problem of the Gate, not a failing check of your work."

When the log is known it follows as one indented list item holding the absolute path in backticks, like the files of `merge_conflict`.

**The kinds that have checks** (hs-d340, hs-d341; contract §2.2 "Job kinds", §7.7) differ from the table in these rows; a `docs` Job, a `verify` Job and a `work` Job have no row of their own.
- **`test`**: *Who you are* says it writes red tests for the Job's named tests and that another Worker implements them later. *Rules* add: write the named tests, in the files of `test_files`, from the docs alone; add stubs (empty or panicking implementations) so that the branch builds; do not implement; the submit builds the branch and runs the tests, which must fail. *Job* gets a subsection **Tests** that lists `tests` and `test_files`. *Finish* adds the refusals `test_files_missing`, `tests_unconfigured` (the repo's policy has no `[test-commands]`: ask the Lead), `tests_build_failed` (the stubs do not make it build), `tests_not_found` (a named test does not exist: fix the name or write the test), `tests_not_red` (a named test passes: make it fail for the right reason, for example against the stub) and `tests_error` (the check could not run: ask the Lead).
- **`impl`**: *Who you are* names the `test` Jobs it builds on. *Rules* add: make the listed tests pass (the union of those Jobs' `tests`, derived); never change a guarded file (the `test_files` of those Jobs, listed; the paths the Lead approved are listed apart, read when the Spec is rendered); a test that is wrong is a question (`cpd ask`), not an edit; the submit builds the branch and runs the tests, which must pass. *Job* gets the same **Tests** subsection: the derived tests and the guarded files. *Finish* adds `tests_unconfigured`, `tests_build_failed`, `tests_not_found` (ask the Lead), `tests_failed`, `tests_tampered` (revert the guarded paths, or ask) and `tests_error`.
- **`open`** and **`update`**: *Who you are* says it brings the PR's Lead Branch up to date with `<base>` and that cpd runs the Gate, pushes and opens the pull request itself. *Rules* replace "commit your work" with: `git fetch origin <base>`, then, **only if the Lead Branch conflicts with it** (`git merge-tree --write-tree --name-only origin/<base> HEAD` exits 1; a base that merely moved is no reason, hs-d490), `git merge --no-edit origin/<base>` in the worktree; resolve every conflict keeping both sides' intent; commit the merge and the code it needs (recomputed hashes, tests adapted to the base's new rules, a renumbered migration); run the quick check; submit, also when there was nothing to merge (an `update` Job is queued by a conflict only, so it merges the base). It does not push, does not run `gh`, does not run the Gate and does not touch the pull request. The base, repo and Lead Branch it names are read from the PR row at A3, so a base changed after the Job was inserted (`pr edit --base`, an unstack, GitHub's repoint) is the base the Worker is told. *Job* is cpd's fixed text for the kind. *Finish* adds the policy codes (`owner_unset`, `owner_unconfirmed`, `policy_invalid`: ask the Lead), the only refusals of submit beyond the common preconditions and the tamper check (contract §7.7), and drops `no_commits`. A title, ticket, closed pull request, base conflict or Gate failure is found at the merge, which ends the Attempt there (contract §7.4), so the Spec does not list them. The *Rules* row also says the Worker does not push, run `gh`, run the Gate or touch the pull request. The rows of the Spec for a kind are rendered from the Job's kind and its stored facts (`tests`, `test_files`, the guard of an `impl` Job, `test_approved`), as the Spec is rendered (A3); a `work` or `docs` Job's Spec is unchanged except that *Rules* name the PR's `open` Job, not `pr open`, as the place of the full suite.

The Worker is started as §6.5 says, with the first message `Read <spec path> and follow it exactly. Your attempt id is <attempt>.` and `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT` in its environment and `PATH` with `$CPD_HOME/bin` first, where the `git` wrapper refuses `git stash` (contract §6.5, hs-d310).

**Nudge** (A5, once): an `attempt-nudge` Event to the Worker's Session with `data.turns` = the Worker's assistant-turn count when nudged (a resumed driver reads its nudge baseline from it) and `data.prompt` = `You ended your turn without running cpd attempt submit <attempt>. Finish the Spec and submit, or run cpd ask <attempt> "<question>" if you are blocked.` (cpd 1 §12 step 7, kept), delivered with a turn (§15.3).

**Submit preconditions:** §6.1, in §7.3's order.

### 16.2 The review files and the brief

Removed by R19 (no Job-level Reviewer).

### 16.3 The verdict

Removed by R19 (no Job-level Reviewer).

### 16.4 Worker latitude

A Worker may change a file outside its `touches` when a test named in its acceptance cannot pass without it (hs-d512, contract §16 "The Worker contract"). The Worker lists each such file and why in its commit message, instead of escalating with `cpd ask`. Examples: a migration that only this Job should create, a helper script the test uses, a stub of a new module. The Lead reads the commit message at the Worker's submit and approves or escalates; the test must pass without editing the guide.

## 17. What the TUI reads

Outline §8 and D4: the views take one **snapshot** with a read command, then **follow Events**; there is no timer polling of the CLI, no cache directory and no tmux option carrying state, **with deliberate exceptions**: `@cpd_workers`, the StatusBar's workers text, which the engine sets, with the workers' short form and the width (below; contract §6.1) and `@cpd_runner`, the runner pill's text, which the engine sets from `worker_runner` and a click also sets (pro-d199; contract §6.1). A view reads the database **in process**, through the read model (`tui/src/read/`): the same queries as the snapshot commands named below, one read transaction per snapshot, no subprocess and no `gh`. (An earlier draft had each view run `cpd` as a subprocess; the implementation does not and one source of truth is the store.) Reads take no lock and never wait for writers (§4.2); cpd 1's `workflow show` took 58 s under load (research.md), which is why the spike needed a cache.

**Redraw rule, pacing and reads** (hs-d341). The measured causes of stutter in docs/research/anim-perf-plan.md and anim-perf-tree.md lead to four rules, the same everywhere the tree and plan views loop:

- **R1: Redraw rule.** The tree has one function that names every spinner its drawing code emits: `View::animated()` counts working and fixing Leads, a Context row's own dot (its Manager's) when working or fixing and merging Inbox items. Archived Contexts and Managers asleep (rows drawn muted) never turn and count for nothing. The plan view's spinners are a running Job's pill and a PR box title's Lead dot (`PrNode::lead_dot`, on the box the Lead's Workflow names; contract §3.6) and one function names them all with one line per spinner, so a spinner cannot be drawn without making the view redraw. While a spinner is on screen, the view draws once per 50 ms frame. With no spinner on screen, the view draws only when something changed (an Event, a size change, a clock tick, or input), never on a bare timer.
- **R3: Clock-aligned pacing.** While a spinner is on screen, the loop wakes at each 50 ms boundary of the wall clock (`wait = 50 ms - (unix ms % 50)`, between 1 and 50 ms) and draws once per frame. With no spinner on screen, the loop does not wake for the clock and blocks on its channel for a signal. A draw that takes longer than a frame is followed by the next frame's draw, never by a burst of catch-up draws: at most one draw per frame.
- **R4: Reads never block the draw.** No read (the snapshot, the Event follower, the open pull request read, the engine lock probe, the merge marker read) runs on the thread that draws. All views, including the Dispatch column, run their reads on a reader thread; the loop takes a finished result without waiting. At most one read runs at a time; a request made while one runs waits and only the newest waiting request is kept. A read that fails keeps the last snapshot on screen and the view shows its error. The first snapshot is taken before the terminal is set up. The end of an action, a popup, or a Worker-pane call wakes the loop like a read result.

**Titles** (hs-d257). No view follows `session-titled`: the one place a Session's dynamic title is shown is the chat slot's pane header, which tmux draws from `@cpd_title` (§13.2, contract §6.1), set by `session retitle` itself. A view that wants the title reads `sessions.title` from `session list` or `session show` (§10.1, §10.2). `session-titled` is in the Event log (`logs`) like every Event and in no view's follow list.

**Gap-free start.** Every snapshot command returns `events_cursor`, the highest Event id in the same read transaction. The view then runs `cpd events follow --since <events_cursor>`, so no Event between the snapshot and the follow is lost or seen twice.

| View (`@cpd_view`) | Snapshot (the read of the command named) | Events it acts on | What it updates |
|---|---|---|---|
| `tree` (Context column) | `cpd context list`, `cpd session list` and each Session's own cost: the `cost_usd` of `cpd_core::metrics::parse_file` on its session file (fork-safe, contract §2.2 "Session cost"), read by the snapshot itself with the tree's reads, not by a command and cached per file by its `(mtime, length)` (a file whose stamp is unchanged is not parsed again; a file that cannot be parsed keeps its last good cost) | `context-created`, `context-grounded`, `plan-created`, `plan-phase`, `plan-done`, `session-registered`, `session-state`, `session-moved`, `session-renamed`, `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived`, `inbox-opened`, `inbox-closed`, `inbox-snoozed`, `inbox-dismissed`, `inbox-claimed` (and the `context-record`, Job, Attempt, PR, Escalation and Plan review kinds of `tui/src/tree/kinds.rs`: every Event that moves a row the snapshot reads) | Contexts, their Plans' Lead Sessions, then the Sessions forked by `session fork` as siblings under their Context (B1), each with its label (an id, §13.2 "Labels"; the dynamic `title` of a Session is never drawn in the tree, hs-d257), state, place, `archived_at` (archived objects are in the read model), Job counters (Plan Leads only) and **dot** (rule below). Inside the archive row, after the archived Plans and forks: every archived Manager Session as a muted row (colour 8, fg 8), named `<context>-manager`, with its own dot (rule below) drawn in colour 8, inside each Context's block within the archive row. The Context row's own dot comes only from its live Manager. The **Inbox section** (§13.2) shows the open, visible items (snoozed and dismissed ones are hidden; `engine-down` included, when no live Manager exists), in its order, with each item's `kind`, `urgency`, `label`, `session`, `plan`, `event`, `url` and `opened_at`; and each Session's state and place for the glyphs the Inbox does not give (the spinner, `󰽤`). The Inbox, not the Sessions' state, says who needs the User, so the snapshot no longer reads the Plan's pending review or its pull requests' checks for the dots. Two changes of the Inbox have no Event: a snooze ends by the clock, so the tree also re-reads when the earliest snooze of an open item ends (`snoozed_until`, read with each snapshot) and the engine stops or starts (`engine-down`, never stored), so with each poll of the Events it looks at the engine's lock (§1.4) and re-reads when that changed. It re-reads on no timer. It redraws once per 50 ms frame only while a spinner is on screen (§17 redraw rule) and every second while the Inbox section shows items (an item's age, from `opened_at`). Also read and written: `run/tui/tree-folds.json`, the Contexts' folds and the archive row's fold, read once at the view's start and rewritten when a fold changes (§13.2, hs-d260), the view's own state |
| `plan` (centre top) | `cpd plan show --plan <p>` for the Plan of the Lead in the chat slot (its `prs`, each PR's key, repo, base, needs, status, `pr`, `pull_request` (its `state`, for a closed pull request) and `jobs`, for the PR boxes, §13.2); when a Manager is in the chat slot, the Context's Plans read through `read::tree()` with each Plan's Lead SessionNode's dot, Job counts, done_at and archived_at; the activity Events of the window, each with its kind of activity for the bar's colour (§13.2 Kinds) and each Plan's latest activity Event; the Attempts not ended or ended inside the window | every Event with `plan = p` (the `pr-*` kinds and `pr-feedback` included: they change a box's title or add a box); `session-moved` with `place: "chat"` (the slot changed: switch Plan and take a new snapshot); when a Manager is in the slot, every Event of the Manager's Context and `session-moved` | one box per PR that is not `cancelled`, top to bottom by needs and linked by them, the merged ones first and the others below them, each with the flowchart of its Jobs, Attempts and Tiers (R34, A15); with a Manager, Pulse (one row per Plan of the Context). Also read: the Plan's latest Plan review (its status and number)  and each pull request's `url`, for the `󱆭` links (§13.2). Also read: each pull request's `mergeable` and whether its PR has an update Job not ended (`jobs.kind = 'update'`, `queued` or `running`) with its status, for the `conflict`/`updating` title (§13.2, hs-d532) and the Plan's Lead Session's `state`, for the working dot beside the state word (§13.2; hs-d335; the re-read at least every 2 s picks up a change of `state`; see Rates for the redraw rule). Also read per PR: `elapsed_seconds`, the Workflow's active time for the counter on its box's bottom border (§13.2, pro-d175), from `plan_boxes_at` on the snapshot's own clock. Also read per PR: `PrNode.cost_usd`, the Workflow's cost for the cost after the counter (§13.2, pro-d179; contract §2.2 "Totals"), from `plan_boxes_at` with the same snapshot: the stored `attempts.metrics` of the PR's Attempts and, for an Attempt with none, its Worker Session's file read through the same per-file cache of `(mtime, length)` as the tree's costs. A conflict clears with no Event (only becoming `conflicting` appends `pr-feedback`), so the view also re-reads its snapshot when a `pull_requests` row changed (see Rates), with the same coalescing and the Plan view on no timer (Pulse: see Rates). Not drawn, so not read: open Escalations. Also read and written: `run/tui/plan-scroll.json`, the Plans' scroll offsets, read once at the view's start and rewritten when an offset changes or the Plan changes (§13.2, hs-d254), the view's own state. |
| `widgets` (Dispatch column) | the host's CPU %, one utilization % per GPU and memory % (`/proc/stat`, `nvidia-smi --query-gpu=index,utilization.gpu --format=csv,noheader,nounits -lms 2000`, one long-running process for the view's life and `/proc/meminfo`; on macOS `ps -A -o %cpu=`, `sysctl -n hw.ncpu`, `ioreg -r -d 1 -w 0 -c IOAccelerator`, `vm_stat` and `sysctl -n hw.memsize`), sampled every 2 s with no Event, the CPU over the time since the last sample (it needs no read of the store); and the Inbox (§21). On each refresh the view reads only the **mark** of the Inbox's open items (`inbox_mark`, `tui/src/read/tree.rs`: the open rows' own columns and the number of live Managers; one cheap read, no snapshot). It reads the Inbox itself (its items, the engine lock, `read::snooze_end_at`) only when the mark moved (an item opened, closed, snoozed, dismissed, claimed, relabelled or re-linked in place, which appends no Event, as an in-place label change of a `pr-ready` row does, contract §2.2 "The authored pull request rule"; a Manager appearing or going changes the mark too), the engine lock moved, a snooze ended, a failed read is retried, or a click or a menu pick forced it; an idle view takes no snapshot (tui-F1); the merge markers (§13.2); no `pull_requests` rows | the refresh triggers of the Inbox (`inbox-opened`, `inbox-closed`, `inbox-snoozed`, `inbox-dismissed`, `session-state`, `plan-created`, `session-registered`, `session-moved`) and the 2 s sample | the machine's name, CPU %, GPU % and memory % and the Inbox. No PRs, engine, slots, Escalations or Events |
| the StatusBar (tmux's status line; no view, no pane) | the slots, config `concurrency` and the queue: the tenants of `workers` (contract §3.4 "Tenancy"), `concurrency`, the Ready Jobs and the `queued` Jobs, read **by the engine** (§12), not by a view | the engine's tick (`watch.tick_seconds`): it runs `tmux set -g @cpd_workers '<text>'` (and `@cpd_workers_short`, `#{w:@cpd_workers}`, `#{w:@cpd_workers_short}`, `@cpd_version`) only when a value changed | `workers Q󰞘W󰞘N/M`, or `Q󰞘W󰞘N/M` on a narrow client (User, hs-d198; hs-d749). **A deliberate deviation** from "no tmux option carries state" (User, hs-d149): the options hold display strings and a width, up to one tick late and stale while the engine is stopped; nothing reads them back |
| `logs` (popup of the StatusBar's `󰧭` button, via `cpd tui action logs`) | the latest 500 Events (the `events list` default), oldest first | every Event: it follows live while the view shows the bottom (R36) | the Event log |
| `history` (chat slot, an archived Session) | the Session's file, named by `cpd session history <id>` (its `sessions.file`) | none: the view is static and re-reads nothing | the Session's pi file itself, read-only, once |
| `chat` (placeholder) | none | none | the static text `No Lead here: pick a Plan in the session tree` |

**A view never writes to its terminal outside its frame** (hs-d363). The `tree` printed `engine was down: …` with `eprintln!` into its own terminal and broke the drawn frame. A view's lines go to its log file `$CPD_HOME/run/tui/<view>.log`, appended, one timestamped line each; what the User must see or confirm is a dialog, below. No `eprintln!`, `eprint!`, `print!` or `println!` appears in the view modules (`tui/src/`, except the code that runs before or after the alternate screen, such as `cockpit.rs`, `cpd tui`'s own CLI output and the view runner's restart message printed after it leaves the alternate screen); a test in the style of `palette::tests::no_colour_outside_the_palette` pins it.

**No notes** (User, hs-d469, 2026-10-20; decided work, hs-d301; replaces hs-d364 (2)'s "short note" and en-impl2's in-frame notes). No view shows a transient note line: not `already in focus`, `copied <url>: …`, `engine down 60 s+: Managers told`, `fold state reset: …`, `scroll positions reset: …`, `no review page yet`, nor the result of an action. It applies to every view: the tree and its Inbox, the Plan view, the Dispatch column and the Worker and history views.
- **What the User must see or confirm is a dialog**: the dialog kit's popup, `cpd tui view dialog <confirm|input|text|checklist|select> …` (§13.2 "The dialog kit").
- **Otherwise nothing shows** and the details go to the view's log `$CPD_HOME/run/tui/<view>.log` (hs-d364).

**Frame trace** (hs-d588; opt-in). The `tree` and `plan` views write a frame trace to their log file `$CPD_HOME/run/tui/<view>.log` when the environment variable `CPD_TUI_TRACE` is exactly `1` (read once at the view's start; unset, empty, `0`, `true`, anything else: off and an off trace costs nothing, writes no line and touches no file). Each draw of the view's main loop appends ONE line after the draw, its own cost not counted in any field. The line is exactly `trace gap_us=<n> read_us=<n> layout_us=<n> draw_us=<n> frame=<n>`, every `<n>` a non-negative integer, all fields required:
  - `gap_us`: microseconds from the START of the previous draw to the START of this one; 0 on the view's first draw.
  - `read_us`: microseconds this view's loop thread spent inside reads (store snapshots, the Event follower, the open pull request read, the engine lock probe, the merge marker read) since the PREVIOUS draw ended; then the sum starts again at 0.
  - `layout_us`: microseconds spent laying out the content the draw shows, apart from drawing it, since the previous draw ended, summed like `read_us`: the plan view's `boxes::layout` (at a snapshot and when the width changes, per R2); 0 in the tree view, whose rows are built by the draw itself.
  - `draw_us`: microseconds the draw itself took (`terminal.draw`, which builds the tree's rows and renders both views).
  - `frame`: the spinner frame the draw used (`palette::frame`, 0..7).
The line is prefixed with an RFC 3339 time by the existing view log mechanism (commands.md §17 "No notes"). Why it exists: while a spinner is on screen the loaders must advance every 50 ms (hs-d616), so `gap_us` near 50000 is healthy only while an animation is on screen, since a view with none now idles between signals; a gap that is longer, with the cause in `read_us`, `layout_us` or `draw_us`, is a stutter.
- **(a) A failure is something the User must see** (Lead's decision, which the User may correct): a click or key whose command fails (for example `session open` refused) opens a `text` dialog with `<what failed>: <code>: <message>` and a log line too. A success says nothing. Clicking an Inbox item already in focus does nothing and says nothing.
- **(b) A view that cannot read its snapshot** (the store unreadable) is a lasting state of the view, not a transient note: it keeps its error line. So do the lasting texts of a view with nothing to show (`no Plans yet`, `no Lead session in the centre`).
- **(c) `palette::note()` and `Ui::Note` stay only while something else uses them** (the chat's tool lines use `Ui::Tool`); otherwise they are deleted.

**Rates.**
- A view learns of a commit from a **file watcher on the store's directory**: it signals on a data write, a create or a remove of `cpd.db` or `cpd.db-wal` and nothing else; `cpd.db-shm`, `config.toml`, `locks/` and `run/` do not signal, nor do access events, so a view's own reads cause no signal. A **safety read** starts when 2 s have passed since the last read started, for the facts the store does not announce: the engine's lock, the end of a snooze, the merge marker.
- A watcher that cannot start is shown in the view's error row as `no commit watcher: <reason>; polling every <events.poll_ms> ms` and the view polls at `events.poll_ms`, never silently; `events.poll_ms` (default 200 ms) is the fallback period of the safety read and the period of `cpd events follow`.
- A **read** is the follow poll (the Events since the cursor and the `pull_requests` mark) and, when it found a change, the snapshot, in one job on the reader thread, at most one per view per 50 ms frame: a commit that arrives while a read is out, or within the frame, is one read at the frame's end. Nothing reads when nothing committed except the safety read.
- A read result is drawn on arrival; only animation frames stay aligned to the 50 ms boundaries. A view with no animation, no age text on screen and no commit wakes only for a signal and the safety read.
- On an Event of a kind it shows, a view re-runs its snapshot (it never applies an Event's `data` itself), coalesced to at most one snapshot per view per 50 ms frame, whatever the Event rate.
- **Pulse** re-reads every 60 s (the window moves and ages tick; a running Attempt is not an Event) and on pane resize (the bucket count changes), coalesced as other views.
- The **Dispatch column** follows `session-state` Events (a Lead's dot is a fact it draws) and its `Follow` reads on the reader thread like the others. Each view follows Events with no selection and filters by kind itself, through `cpd events follow` or the same poll in process (§5.4): one `PRAGMA data_version` read per `events.poll_ms`, which costs no I/O while nothing changes (§5.4). The follow also reads `count(*)` and `max(changed_at)` of `pull_requests` with each poll of the Events to detect row changes (the poll sets `changed_at` with every change, contract §4.4) and says the rows moved when they differ from the last poll (the first poll says so).
- A view draws only when its model, its error, its size (a resize is an input event), its input or a clock changed, never on a bare tick. While a spinner is on screen the view draws once per 50 ms frame; with no spinner it draws only on change (§17 redraw rule). The spinners: a working dot in the tree, including the red loader (hs-d581), a turning failed pill in the plan view (hs-d586), or in Pulse, a running Job's pill in the plan view; the tree redraws every second while its Inbox section shows an item's age and the Dispatch column when the CPU, GPU or memory % it shows changed (sampled every 2 s; on Linux the GPUs come from one long-running `nvidia-smi -lms 2000`, so no tool is spawned per sample).
- Clocks such as "running 4 m" are redrawn every second from timestamps already read, without a read. The spinner (`` on the tree's dots, on a `running` Job's pill, on a turning failed pill with the moon loader (hs-d586), beside the state of a PR whose Lead works on the Dispatch row and in the Plan view's box title) is redrawn every 50 ms, while one is on screen, from data already read, without a read.
- If a follower ends, the view takes a new snapshot and follows again from its `events_cursor`.

| View | Clocks needed besides a signal | Safety read covers |
|---|---|---|
| tree | the next whole second while the Inbox shows items (their age text); the next 50 ms frame while a spinner is on screen (a working or fixing Lead, a merging Inbox item) | the engine's lock (`engine-down`), the end of a snooze, the merge marker |
| plan | the next 50 ms frame while a spinner is on screen (a running Job's pill, a failed one's moon loader while the Lead works, a PR box title's Lead dot); Pulse's 60 s refresh when Pulse shows; no per-second clock (Pulse's ages are in minutes) | (covered by the 50 ms and 60 s frame rates) |
| widgets | the next 50 ms frame while a Lead works or a clean PR's merge runs; the 2 s sample of CPU, GPU and memory | the merge marker |

A view with no clocks on screen wakes only for a signal and the safety read.

**Blue** (User, hs-d351, hs-d352, 2026-10-20; decided work, hs-d301; amended pro-d58). **Blue means focus and selection**, always colour 4, named once in `tui/src/palette.rs`; the whole Dango palette is contract §6.1.1 (hs-d534). It has these uses and nothing else is blue: the focused pane's border and title (`pane-active-border-style fg=blue`, §6.1); **the selected row** (hs-d351), the Context tree's selected Context or Plan name, which draws in blue while nothing else of the row changes, beats the muted colour of archived and asleep rows and has no chat-slot marker (§13.2); and every cursor and selection, which is a **blue block** (colour 4 inverted: text in the terminal's background colour) in place of reverse video (hs-d534): the tree's and the Inbox's cursor, the selected Job pill, the selected entry of a menu, the default dialog button, the dialog input's cursor, the Logs dialog's opened-at line, tmux's menus and copy mode. **Pulse's escalation bar is no longer blue**: hs-d352 made an escalation a focus change and drew it blue; hs-d534 replaces that, an escalation is agent activity and its bar is cyan (§13.2 Pulse). Later uses of focus and selection take the same blue. **The Inbox has a new blue tier** (pro-d58): external support for `review-requested` items, palette 4 (base blue) and 12 (bright blue), tier glyph U+F0368 (its kind `review-requested` draws its own, U+F12F2, §13.2), fills no Session dot (§17 "Which items fill which row", contract §4.4), between cyan and magenta in order. **This amended the earlier rule**: hs-d570 drew the notice tier blue, pro-d16 renamed it cyan and said "blue means focus and selection only"; pro-d58 adds the blue tier for external support, keeping focus use of blue in the interface.

**Collapsible blocks** (User, hs-d337, hs-d339, hs-d344, hs-d349, 2026-10-20; Plan pr-links; replaces hs-d309's `┌` / `└` shapes). Every block that folds has one look, drawn by **one shared widget** (`tui/src/fold.rs`), not one copy per view:
- **Rows:** the title row is `╭` + the row's glyph + the name, for example `╭󰽢 building-cpd`; each content row is `├─` + its glyph + its text, straight against the branch with **no indent**, for example `├─󰽢 cross-repo       3⋅1⋅0⋅0⋅92`, the counters right-aligned. (Where content has no glyph of its own, the content row is `│ ` and the row; pi's rows are the same, `│ `.) **Right edge:** every row follows the rail rule (contract §6.1.1 "The rail fold"): column 0 blank, rail at column 1, content from column 3 to column `W - 4`, columns `W - 3` to `W - 1` blank; text wrapped or cut with `…` and right-aligned elements (the counters) end at column `W - 4`. The Context tree's rows are the exception: they end at column `W - 2` (pro-d186; §13.2 "Rows"). Any row the widget wraps keeps its own glyphs and colours (hs-d345); the fold only adds lines (colour 8).
- **Closing row** (hs-d349; replaces the word-only `╰─ expand` / `╰─ collapse`): **folded**, `╰󰬦 expand` (U+F0B26 md-chevron_down_circle); **expanded**, `╰󰬬 collapse` (U+F0B2C md-chevron_up_circle); **one** closing row per block (a pi tool row has five forms, `expand`, `collapse`, loader, terminator and `choose` for `ask_user_question`: contract §6.9 "The five closing rows"). A block that cannot fold and has no row to close it (a Context with no rows at all) ends with the terminator ` ╰󰬪` (U+F0B2A md-chevron_right_circle, colour 8, no word, no toggle, hs-d574, replacing hs-d477's and hs-d459's plain `╰─`); a **LIVE** Context with no rows instead ends with ` ╰󰬪 new` — the muted `╰` and glyph and the word `new` in colour 15 (clickable text) — and a left click on that row opens the New Plan input popup for that Context (the same action as `n` and the context menu's New Plan...). An **ARCHIVED** Context with no rows keeps the plain muted terminator. `plan new` is refused in an archived Context (`archived`), which is why the row is absent there. A tree Context block with **nothing to fold** (no archived rows, hs-d477) has no closing row: its last row's branch is `╰─` instead of `├─`, for example `╭󰽤 homeserver` / `├─󰽤 b2-budget …` / `╰─󰽧 speedtest …`. The `╰─` of a last row's branch (hs-d477) is a content row's branch, not a closing row and is unchanged.
- **What the toggle hides** (hs-d344): in the tree's Context blocks **only the archived rows**. Live Plans and Sessions always show. Folded: the live rows, then `╰󰬦 expand`. Expanded: the live rows, then the archived rows in colour 8 (hs-d339, hs-d534), then `╰󰬬 collapse`. A Context with no archived rows has no closing row: its last row is `╰─` (hs-d477).
- **Muted vs not** (contract §6.1.1, hs-d534): the lines (`╭`, `├─`, `│`, `╰`) are muted, colour 8 (`palette::fg(Meaning::Nothing)`, or pi's rail colour in the Dango theme); the arrows and the words `expand` / `collapse` are the toggle, colour 7; the content is not muted.
- **Spacing** (hs-d476; a box's margin and padding are the Box rule, contract §6.1.1 "Boxes", hs-d578; `tui/src/layout_tests/` pins the spacing): a collapsible section widget (title row, content rows, closing row; `tui/src/fold.rs`) has a margin of **one empty row above and one below**; its body has no vertical spacing.
- **Toggling:** only the closing row toggles; the title row keeps its own action (opening the Manager or the Session, for example).
- **Where it applies:** the tree's Contexts, with their archived Plans and Sessions inside the block (hs-d339) cpd's messages in pi (§15.9; `╭⦁ title`; pi's own rendering is not changed by hs-d349 and keeps hs-d339's closing row until a Job says otherwise) and any popup or Plan view panel that folds (none does yet). **Manager's assumption** (the User may restrict it): the arrow toggle is the same for **every** fold block, the tree's Context blocks included, from the one shared helper. It replaces the archived `󰇘` row toggle and hs-d309's `┌` / `└` shapes.

**Section titles** (User, hs-d298, hs-d308, hs-d337, hs-d346, hs-d349, hs-d475 to hs-d478, hs-d480; **section look v4**). Every section title inside a pane (the Inbox, the Dispatch column, the Plan view's in-pane titles if any, logs, legend) is drawn by **one helper**, `tui/src/section.rs`, as a **pill** on a line, in **one of two forms** (the plain form and, since pro-d147, the flush form), each putting the pill's left cap three columns from the container's edge. One rule (hs-d475, hs-d480), the User's example `Section Or Panel Title`:
- the title is its text in **Title Case, every word capitalised**, with **no padding spaces** inside the pill, **a block between the rounded caps** U+E0B6 `` (left) and U+E0B4 `` (right) and **never clickable**;
- **in-pane titles are always a read-only title pill** (hs-d534, which replaces hs-d530's 8 and 7, hs-d349's unmuted pill and the reversed muted block): colour 7 inverted: text in the terminal's background colour, caps in colour 7 as foreground, from `palette::pill_static()`;
- **every title's left cap sits three columns from its container's edge** (hs-d478): the one column of padding, two cells of lead (`──`), then the cap U+E0B6 (the flush form has no padding column: three `─` from column 0, then the cap);
- **the Inbox's header** (pro-d147) is the **flush form**, drawn by `flush_title(title: &str, count: Option<usize>, width: usize) -> Line<'static>` in `tui/src/section.rs`: `───`, the pill, a muted `─` fill, the count pill (hs-d480; the same grey pill, `palette::pill_static()`), one muted `─`, **no padding cells**, so it spans the full width from column 0 to column `W - 1`: `───Inbox────…────35─`. A narrow width degrades like the plain form: the count is dropped first, then the title is cut, then the pill; at width 3 or less it is `─` repeated `width` times. Its column 0 `─` is the first cell of the pane, next to tmux's `┃` border cell; tmux draws the `┃` in the middle of its cell, so half a cell lies between them and they do not join (pro-d157; docs/research/inbox-pane.md). There is no collapsible form and no `rail_title`: no title uses them any more (they are removed);
- **any other title** (logs, legend, the Dispatch column's other sections) is the **plain form**, drawn by the helper's `title`: the padding, a muted `──`, then the pill, so its cap lines up with the flush form's: ` ──Logs────…────`, ` ──PRs───────3 `, ` ──Section Or Panel Title───`. **Manager's assumption** (hs-d478: the User may correct it): the `──` lead for the plain titles; the User's rule is only that the caps line up;
- **content rows** of a fold are `│ ` and then the row, with its own icon and colours; the **closing row** is the fold's (`╰󰬦 expand` or `╰󰬬 collapse`, above); the Inbox is not a fold;
- the lines are muted (colour 8), a fold's toggle arrow and words are colour 7, the pill is a read-only title pill (colour 7 inverted: text in the terminal's background colour);
- **spacing** (hs-d476, replacing hs-d448's; a box's margin and padding are the Box rule, contract §6.1.1 "Boxes", hs-d578): every **panel** has **one column of padding on the left and on the right** and its titles sit inside it, so a title row **starts at column 1 and ends one column before the right edge**; a **collapsible section widget** (title row, content rows, closing row; `tui/src/fold.rs`) has a margin of **one empty row above and one below**; a **section title outside a collapsible widget** has one empty row above and one below (the Inbox's flush header has one empty row above and the cue line below it); a section's **body has no vertical spacing**;
- **Manager's assumption** (the User may restrict it): the same title pill for **every** in-pane section title, not only the Inbox's. It replaces hs-d337's `┯━━ Inbox ━━ 32 ━━` and the heavy `━` titles.
- Not covered: the tmux pane titles (the tree pane's CPD mark and a Lead's header, contract §6.1; hs-d729 removed the Context, Plan and Deploy titles, superseding the view-title part of hs-d671) and the popups' tmux titles (` legend `, ` logs `) are Plan tui-visuals', unchanged here; the dialog kit's title is the frame's pill (§13.2, hs-d578), not a section title.

**Scroll cues** (User, hs-d348; amended by pro-d147, 2026-10-28, which supersedes pro-d141). **Only the tree and the Inbox draw scroll cues** (`rg 'cue\(' tui/src` finds no other caller); the Plan view, the popups, logs, the legend and the rest of the Dispatch column draw none and get none. A scrolling list is, top to bottom: a ▲ line directly under the list's header, the list rows, a ▼ line directly under the last list row. **Both lines are always reserved**, also when the list fits: then they are blank. When rows are hidden above, the ▲ line holds `▲ <n> more` (`n` = rows hidden above); when rows are hidden below, the ▼ line holds `▼ <n> more`. A cue is drawn by **one shared helper**, `dialog::scroll::cue` in `tui/src/dialog/scroll.rs` (muted, colour 8, right-aligned), in a row of width `W - 2` that starts at column 1, so a cue **ends at column `W - 2`** (column `W - 1` is blank), for the tree and the Inbox alike (the `W - 4` of the rail rule, contract §6.1.1 "The rail fold", is for rail folds only). A cue line never holds a list row and is no click target. No spacing is doubled: where the layout already has a blank row in that place, that row is the cue line. **Window arithmetic**, for `count` list items and `rows` list rows (the two cue lines are not in `rows`): `max_scroll = count - rows` (0 when `count <= rows`); `first` = the scroll clamped to `max_scroll`; `shown = min(count - first, rows)`; hidden above = `first`; hidden below = `count - first - shown`. One shared implementation: `Window::new(count, rows, scroll)` and `Window::max_scroll(count, rows)` in `tui/src/widgets/column.rs`, the case where every item is one row (the tree). For the Inbox an item is one row or two (pro-d177, §13.2): the same `Window` struct is built by `Window::weighted(heights, rows, scroll)` and its largest offset is `Window::max_scroll_weighted(heights, rows)` (`heights` the items' heights in order; `Window::new` and `Window::max_scroll` are the case of every height 1): `max_scroll` = the smallest offset from which the heights of the remaining items add up to at most `rows` (`count` when none does); `first` = the scroll clamped to it; `shown` = the longest run of items from `first` whose heights add up to at most `rows`; hidden above = `first`; hidden below = `count - first - shown`; the list rows left after `shown` (at most 1) are blank, above the ▼ line; the cues count items. The tree's and the Inbox's layouts are §13.2's.

**The age text rule** (hs-d341). One rule for all surfaces: `pub fn age_text(seconds: i64) -> String` in `tui/src/read/age.rs`. Under 60 s `Ns`, under 3600 s (1 hour) `Nm`, under 86400 s (1 day) `Nh`, else `Nd`; a negative age is `0s`. The tree's Inbox shows each item's age from `opened_at`; Pulse shows each Plan's age from its latest activity Event, calling `age_text(minutes * 60)` to convert.

**The PR state word rule** (hs-d341). One rule for all surfaces: `pub struct PrLook { pub state: (&'static str, Meaning), pub conflict: Option<(&'static str, Meaning)> }` and `pub fn pr_look(facts: &PrFacts) -> PrLook` in `tui/src/read/pr_look.rs`. The `state` word is: `draft` in `Meaning::Planning` when the PR is open and a draft; `open` in `palette::pr_open(clean)` when open and not a draft, yellow only when **ready to merge** (contract section 2.2), else colourless; `closed` in `palette::PR_CLOSED_ASKED` (red) when closed and the Lead asked to close it; else the word and `palette::pr_state(state)` (`merged` green, or `closed` cyan if still being handled). The `conflict` word is `Some` when the PR is open and `conflicting`: `updating` in cyan when an update Job runs or is queued, else `conflict` in cyan (the Lead fixes it); `None` otherwise. The plan view's PR box title shows `conflict` when present, else `state` and now also `draft`; the Dispatch row shows `state` and appends `conflict` after review words when present.

**The tree's dot rule.** One loader for pi and the cockpit (hs-d616, a trial; replaces hs-d579 and the eight nf-md moons at 100 ms): the tree's working dots and a running Job's pill use the 28 moon_alt phases described in the table, one frame every 50 ms, on one shared clock, so every spinner turns together. History: eight nf-md moons at 100 ms (hs-d579), replaced by the 28 moon_alt phases at 50 ms (hs-d616, a trial: the User keeps or reverts it after reading docs/research/moon-loader-cost.md). The static dots follow: needs-you is moon_alt_new (U+E3E3, frame 0), idle and stopped are moon_alt_full (U+E3D5, frame 14), waiting is U+F1853 (unchanged). The filled dot means the Session needs you and the hollow dot means nothing is needed; in moon_alt the new moon U+E3E3 is the filled disc and the full moon U+E3D5 the hollow ring (reverse of the earlier nf-md family), hs-d620; a non-turning loader glyph (an archived working or fixing dot, a running Job's still icon) is the hollow U+E3D5. The filled needs-you glyph comes **only from open, visible Inbox items** and shows on the tree's rows only; the Inbox's own rows draw their tier glyphs instead (hs-d483) (hs-d165, hs-d167: the Inbox is the single source of "needs you"; it replaces the per-case rules the tree derived from Session states, Plan phases, Plan reviews and pull request checks and cpd 1's `@asking`, `@settled_at` and `@seen_at` pane options). The other glyphs are derived from the read model with no write. Nothing is written by the tree.

**The Lead dot rule** (hs-d581, hs-d622, hs-d624; superseded by pro-d200, which replaces the phase colours of hs-d622 and hs-d624). The plan view's PR box title (on the box the Lead's `working_pr` names, contract §3.6) and the Dispatch row each show the Lead's working state as one dot: the 28-frame spinner on the shared clock, in one of five colours chosen by the work the Lead is doing (pro-d200, the User's decision; the static phase colour it replaces ignored the open PRs and the drafts). Working is agent activity, contract §6.1.1. The colour of a working Lead, by precedence: (1) **red 9** (`Meaning::Failed`, `Dot::Fixing`) while its Plan has a failed Job being worked on (`PlanHead::fixing`, pro-d88) or an open Worker Escalation (`PlanHead::open_escalations > 0`), decided alone as the failed case always was (the Lead's own red Inbox item still gives the static red dot; on a PR box the test is per Workflow, below); (2) **magenta 13** (`Meaning::Planning`) while the Plan has any draft Job (`counts.draft > 0`), even while PRs are open; (3) **yellow 11** (`Meaning::Review`; git, PR and gate operations) while the phase is deploy, or any PR of the Plan (not only the first) is open, or has `pull_requests.mergeable = 'conflicting'`, or has an open or update Job queued or running, or any Attempt is landing (`JobCounts.merging > 0`); (4) **magenta 13** again for the phases grounding, plan and done (grounding, researching, questioning, planning, or planning a follow-up on a done Plan), never green 10 for done and never white 15 for grounding; (5) **cyan 14** (`Meaning::InProgress`) for the phase dispatch with none of the above (running or queued work Jobs); (6) **white 15** (`Meaning::NotStarted`, clickable text) for a working Session with no Plan (a Manager or a side fork). The API (crate `tui`): `Work { Planning, Dispatch, Review }` in `tui/src/read/dot.rs` (Planning 13, Dispatch 14, Review 11), and `Work::of(phase, drafts, git) -> Work` with exactly that precedence (drafts gives Planning; else git or deploy gives Review; else grounding, plan or done give Planning; else Dispatch). `Dot::Working(Option<Work>)` (`None` is white 15); `Dot::working(failed, work) -> Dot` returns `Dot::Fixing` when `failed` and `Dot::Working(work)` otherwise; `Dot::of(state, item, failed, work) -> Dot` calls `Dot::working(failed, work)` when `state = 'working'`, else returns the Dot from the item's urgency or the static glyph for the state, ignoring `work`. `PlanHead` (`tui/src/read/shared.rs`) gives `failing()` (`fixing || open_escalations > 0`) and `work()` (`Work::of(phase, counts.draft > 0, git_active || counts.merging > 0)`), where `git_active` is true when any PR of the Plan is open, has `mergeable` `conflicting`, or has an open or update Job queued or running (one more query in `plan_heads`, whatever the number of Plans). The tree's `fill_dots` uses `failing()` and `work()`; the plan view's PR box title uses `Dot::working(failed, lead_work)`, where `failed` is the box's own Workflow's check (a failed Job of that Workflow, or an open Worker Escalation of one of its Attempts; contract §3.6, pro-d259). The tree computes and shows this same rule for every Lead's dot. The static (needs-you) dots, the Inbox tiers and the state glyphs are unchanged. The plan view's PR box title shows this dot only on the box that the Lead's `working_pr` names (contract §3.6), and never on a box whose PR is `merged` or `cancelled`, whatever the colour rule above yields (pro-d217, pro-d219 A4, pro-d259); a merged or cancelled box draws neither the dot nor the red loader. The colour precedence itself is unchanged; only its red condition is per Workflow.

| Glyph | Meaning | Style |
|---|---|---|
| `` U+E3E3 (moon_alt_new) | the row has an open, visible Inbox item (below) | the colour of its most urgent item, bright (the row is clickable; contract §6.1.1): red 9, yellow 11, magenta 13, cyan 14, white 15 |
| `` U+E3E3, U+E3C8, U+E3C9, U+E3CA, U+E3CB, U+E3CC, U+E3CD, U+E3CE, U+E3CF, U+E3D0, U+E3D1, U+E3D2, U+E3D3, U+E3D4, U+E3D5, U+E3D6, U+E3D7, U+E3D8, U+E3D9, U+E3DA, U+E3DB, U+E3DC, U+E3DD, U+E3DE, U+E3DF, U+E3E0, U+E3E1, U+E3E2 (moon_alt_new, moon_alt_waxing_crescent_1..6, moon_alt_first_quarter, moon_alt_waxing_gibbous_1..6, moon_alt_full, moon_alt_waning_gibbous_1..6, moon_alt_third_quarter, moon_alt_waning_crescent_1..6; the 28 moon_alt phases as described in section 17) | the Lead is working (`state = working`); 28 frames, one every 50 ms, looping, on one shared clock (`frame = (unix ms / 50) % 28`), so every spinner turns together; red 9 while its Plan has a failed Job being worked on or an open Worker Escalation (hs-d581, pro-d200), else the Work colour of pro-d200 ("The Lead dot rule", §17): magenta 13 while the Lead plans, yellow 11 for pull-request, deploy and landing work, cyan 14 for dispatch, white 15 with no Plan (superseding the phase colours of hs-d622 and hs-d624) | the bright pair of the Work colour in the tree (working is agent activity, contract §6.1.1, hs-d534; pro-d200); red 9 only when the Plan has a failed Job being worked on or an open Worker Escalation (hs-d581, pro-d200) |
| `󱡓` U+F1853 (nf-md-circle_opacity, swapped by hs-d582) | the Session waits for another agent, not for the User (`state = waiting`: a Worker for its Lead's answer, a Lead for a Worker's question, hs-d303); not a filled dot, so never one of the four colours | `Meaning::Waiting`, cyan (`Color::Cyan`; not blue, which means focus only, §17 "Blue"; the User's decision of 2026-10-20, resolving hs-d303 against hs-d352; bright 14 in the tree, contract §6.1.1) |
| `` U+E3D5 (moon_alt_full) | idle, nothing needed | colour 15 (clickable colourless text, contract §6.1.1) |
| `` U+E3D5 | pi stopped (`state = stopped`) | colour 8 (it was dim) |

| `󰽢` colour | Which item (contract §2.2 gives each kind its urgency) |
|---|---|
| red | the most urgent item is red: `dialog` while its Plan is in `dispatch` or `deploy`, `job-stuck`, `pr-closed` |
| yellow | yellow: `pr-ready`, `review-requested`, `deploy` (`deploy` links no Session: it fills no dot) |
| magenta | magenta: `dialog` in `grounding`, `plan`, `done` or with no Plan, `plan-review`, `drafts-parked` (hs-d731) |
| cyan | cyan: `curate-retire`, `curate-docs`, `models-changed` (cpd notices, hs-d549, hs-d551; pro-d16) |
| white | white: `unread` (contract §2.2 gives its rule: a finished turn that answered the User, hs-d278; this table does not define it) |

**Which items fill which row.** A **Session** row's `󰽢` is the most urgent open, visible item **linked to that Session** (`inbox_items.session`): `dialog` and `unread` for any Lead, fork or Manager; and, for a **Plan's own Lead** (`plan_lead`, never a fork), also the items of its Plan: `plan-review`, `job-stuck` and the Yellow or Red `pr-ready` items and the `pr-closed` items of the Plan's pull requests (an item whose `plan` is the Plan); Cyan and White `pr-ready` items fill no Session dot (pro-d130). A **Context** row's `󰽢` comes **only from its Manager Session's own items** (`dialog` and `unread` linked to the Manager; hs-d215, hs-d217): it is not the colour of the Context's most urgent Session and the Context's other items (`job-stuck`, `plan-review`, `question`, `pr-*` and the items of its Leads and forks) **do not colour the Context row**; they show only in the Inbox section (§13.2). `engine-down` is linked to no Session, so it fills no dot. A hidden (snoozed or dismissed) item fills no dot and when it closes (its cause ends) the dot falls back to the glyph rules below at once. **Own state first** (hs-d183): a `working` Session shows the spinner whatever its items; a Plan's own Lead that is working while its Plan has a failed Job shows the loader in red (hs-d581; or an open Worker Escalation, pro-d200) and only in that case its own open visible red Inbox item keeps the static red `󰽢` (precedence); a working Lead with no failed Job or open Worker Escalation shows its spinner in the colour of its Work (pro-d200, "The Lead dot rule", §17); any other state shows its most urgent item's `󰽢`, else `󰽡` in the waiting colour (`waiting` with no item) or `󰽤` (idle or blocked with no item) or `󰽤` in colour 8 (stopped). **A Context row** (hs-d215) is its Manager Session's dot by the same rule, the Session with `role = 'manager'` in that Context, not archived (one per Context, created the first time the Context row is opened, hs-d154); a Context with no Manager yet, or only an archived one, shows `󰽤` (stopped, colour 8). It is no longer the most urgent Session of the Context. **Archived rows** keep their glyph, drawn in colour 8 (`Meaning::Nothing`; asleep rows in 7, contract §6.1.1), never animated. **Archived Manager rows** appear inside their Context's collapsible archived section, after the archived Plans and forks, each muted (colour 8), named `<context>-manager`, with its own dot (rule below, hs-d630, hs-d633).

**Row ordering (hs-d686, pro-d48).** At both levels of the tree (Contexts and within each Context live Plans and forks), pro-d48 replaces hs-d686's last-activity second key with creation, newest first. Rows are ordered first by dot-urgency tier, then by creation (`created_at` descending, newest first).

- **Tier:** red (anko) 0 > yellow 1 > magenta 2 > cyan 3 > shiro 4. Shiro is the white Inbox dot (hs-d483) and every row with no Inbox item: a working row (the spinner), an idle row, a waiting row, a stopped row and the red loader. Archived rows are last, whatever their dot.
- **Within a tier:** the row's creation key, newest first. For a Plan row it is `plans.created_at`, for a fork row `sessions.created_at` (the Session's own creation, assumption A1 of the Decision) and for a Context row `contexts.created_at`. Events never reorder: a newer Event never moves a row. A tie (same tier and same created_at) keeps the read model's creation order (assumption A2, stable sort): Contexts by id; Plans before forks, each as the read model lists them (oldest first). So a hand-built tree whose rows all share one created_at keeps the order it was built in.
- **Level 1, Contexts:** a Context row's tier is its live (not archived) Manager's dot; `shiro`/white with no live Manager. Contexts are ordered by their tier and creation, newest first.
- **Level 2, within a Context:** its live Plans and forks, one list (a fork can outrank a Plan, since they share the same ordering rule). A Plan row's tier is its Lead Session's dot; a fork row's tier is the fork Session's dot. Both are ordered together by tier and creation, newest first, live rows above archived ones; archived Plans, then stray forks, then archived Managers, are grouped in that order (hs-d339), muted (colour 8), each group in creation order. A Manager never has a live row (only archived Managers within the Context block).

**Precedence.** A row shows its state's spinner when working, else the most urgent item's `󰽢` when it has one, else its state's glyph; the order of the dots is `red > yellow > magenta > cyan > white > red loader > working > waiting > idle > stopped`. Archived rows take no Inbox item.

**Actions.** The views change state only through commands: selecting a Lead runs `cpd session open <id>`; a click on a Context row runs `cpd context manager --context <c>` first (§13.2); the new-Plan key runs `cpd plan new …` after its `input` popup; the close key runs `cpd session close <id>` after its `confirm` popup; each item of the tree's right-click menu runs the command §13.2 names, after its dialog popup where it has one (the tree opens `cpd tui view dialog …` with `display-popup -E`, reads the result file, then runs the command; hs-d295, hs-d297); a click on the rest of a `pr-ready` row of the Inbox runs `cpd session open <lead>`, then `select-pane` on the chat slot (nothing when no Plan owns it) and the row's right-click menu (§13.2) runs `cpd tui action merge <owner>/<repo>#<n> --client <name>` with `run-shell -b` (**Merge**; it runs `cpd pr merge <owner>/<repo>#<n>` (§9.10) and, once it succeeds, `cpd pr poll` (§9.2; hs-d312) in the background and opens the failure dialog (§17 "No notes") when the merge fails), `cpd pr post --pr <key> --plan <plan>` (**Post**, §9.8b, no confirm), `cpd session open <lead>` (**Go to Lead**) or `tmux set-buffer -w -- <url>` (**Copy Link**, no `cpd` command); parking runs `cpd session park`; the dialog popups, like the legend, logs and `merge` popups, are the views' own layout, not Ledger state: they hold no state and write only their result file. The `plan` view also writes its own scroll offsets to `run/tui/plan-scroll.json` and the `tree` view its folds to `run/tui/tree-folds.json` (§13.2), the views' own state, not Ledger state. The StatusBar's clicks run `cpd tui action` (§13.2): `󰨝` click click resets the layout (`resize-pane`, the view's own layout, not Ledger state), its legend button opens the legend popup and its `󰧭` button the logs popup (§13.2). A click on an Inbox item runs `cpd session open <session>` (then `select-pane`, as a Session row; for a `plan-review` item, the Plan's Lead), opens the logs popup at the item's Event, or (for the link kinds §13.2 and `review-requested`, §21) is handled by the `Link` widget. The tree and the Inbox list each scroll with the wheel inside the rows their column gives them (§13.2); `s` and `d` and the right-click menu's entries run `cpd inbox snooze` and `cpd inbox dismiss` (§21).

Change from the spike and cpd 1's cockpit: the tree no longer discovers Sessions from tmux pane options (`@session_id`, `@agent_state`) or `leads/<id>/workflow` and the plan view no longer reads `/tmp/cpd-tui/cache/`; both read the database through commands.

## 18. Changes from cpd 1


| cpd 1 | cpd 2 | Why |
|---|---|---|
| `--wf`, `CPD_WF`, `workflow_required` | `--plan`, `CPD_PLAN`, `plan_required` | D3 (§1.3) |
| `workflow init` | `plan new` (also starts the Lead); since R34 it takes no `--repo` or `--base`: each repo of the Plan is a PR, added with `pr add` | D2, D3, R34 |
| `workflow show`, `workflow phase plan` | `plan show`, `plan phase plan` | D3 |
| `lead start`, `lead attach` | removed: `plan new` registers the Lead and starts it with `session run` | cpd starts every Lead itself, so there is nothing to attach (§6.5) |
| `plan decision --pending / --approved / --rejected` | `plan review`, then `plan decide` | the Plan review is a row with its own id and Event (§2.2) |
| `plan render --out` | `plan render` (one path) | one source of truth |
| `job add --key <k>` | `job add <k>` | like every other `job` command |
| Spec field `outOfScope` | `out_of_scope` | the column's name |
| `attempt submit` runs the Gate (`wt hook pre-merge`, exit 64) | runs the preconditions and the tamper, red-test and kind checks; the quick check (`pre-commit` only, none when the repo has no `pre-commit`) and the Gate run at the merge, a failure ends the Attempt `gate_failed` (§7.4 step 6); no refusal of submit for them | §7.3, §7.4 step 6, R20, hs-d679 |
| the Gate runs at every merge, on the rebased Attempt and again on the Lead | the Gate runs only at `pr open` and `pr rebase`, before the push; refusals `gate_failed`, `gate_error` | §7.4, §7.6, R19 |
| a Reviewer per Attempt; `review decide`; Job status `in_review`; Attempt statuses `reviewing`, `awaiting_lead`; outcome `approved` | removed: a submitted Attempt goes straight to `merging`; the Job stays `running`; outcome `merged`; the review is the PR's own, on GitHub | §3.2, §3.4, R19 |
| `watch --wf` per Workflow | `watch`, `watch --ensure`, one per machine | D5 |
| Lead queue files, `herdr:blocked` emitted to herdr | Events; `session state` | D4 |
| — | new: `context …`, `session list/show/fork/open/park/state/run`, `events list/follow`, `tui`, `config` | the model and the tmux host |
| `doctor` checks node, `bd`, `wt`, `herdr` | checks git, tmux, pi, gh with minimum versions | §1.1 |

## 19. Amendments to contract.md

Applied to contract.md by Job contract-amend.

This file specifies the following, which contract.md does not state or states otherwise. contract.md is not edited by this Job; the Lead appends a follow-up Job that applies this list. Each item names the contract.md section it changes.

Rows 22–27 were added by Job `docs` of Plan `tree-menu` (renames, archive and the tree's right-click menu); they are applied to contract.md itself. Rows 28–31 were added by Job `docs-followups` of Plan `tree-menu` (PR `followups`); rows 28 and 29 are applied to contract.md itself. Rows 12–21 were added by Job `parity-contract` (cockpit parity, User, 2026-10-15 and R36); the checklist behind them is [cockpit-parity.md](archive/cockpit-parity.md). Rows 12–14 and 16 are also applied to contract.md itself; rows 15 and 17–21 state what contract.md leaves to this file.

| # | contract.md section | Amendment | Where here |
|---|---|---|---|
| 1 | §6.2, chat slot rule | the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat` (Escalation `contract-commands-a1-q1`, answered by the Lead) | §10 |
| 2 | §6.2, `@cpd_view` values | new value `chat`: the placeholder pane running `cpd tui view chat`, text `No Lead here: pick a Plan in the session tree`; at most one, only ever in the slot | §10, §13.2, §17 |
| 3 | §6.4, `session park` | park creates a new park window holding a placeholder, then `swap-pane`s the Lead and the placeholder; `session open` kills the placeholder pane with its window when a Lead takes the slot | §10.4, §10.5 |
| 4 | §1.4, `$CPD_HOME` layout | two more files: `run/<plan>/lead-message.md`, `run/<plan>/messages/<session>.md` | §1.4 |
| 5 | §5.3, the cursor | the adapter adds `delivered_to` to every message's `details` and the cursor counts only messages delivered to this Session, never at or below its `session-registered` Event | §15.2 |
| 6 | §5.3, delivery table | `plan-review` with `status: pending` opens the Plan Lead's adapter (no message); listed today under "every other kind: ignored" | §15.3, §15.5 |
| 32 | Removed (hs-d738) | plannotator removed. Decision hs-d738 (User, 2026-10-30): plannotator, herdr and pi's notify extension are not used any more and removed. (a) cpd sets no `PLANNOTATOR_*` variable (`PLANNOTATOR_PORT`, `PLANNOTATOR_REMOTE`, `PLANNOTATOR_URL_HOST`) in any pi launch (§6.5, §15). (b) Worker's excluded tools: `ask_user_question,cpd_plan_review` (§16); `cpd_plan_review` tool is removed. (c) `plannotator_ports` config key removed (§14.2); config migration `drop-plannotator-ports` removes it from existing config.toml (comment-preserving, idempotent), the third migration after `distill-tier-gx` and `drop-test-concurrency`; `lead_pi_packages` defaults to `[]`. (d) Store breaking migration `drop-plannotator` deletes `review-waiting` Inbox rows and `plannotator_reviews` table; rebuilds `inbox_items` without `review-waiting` kind in check; Event kinds `review-opened` and `review-closed` stay valid but marked retired (§2.2). (e) `cpd review` has only `submit` subcommand; `open`, `close`, `list` removed (§31). (f) `cpd plan review` writes `run/<plan>/plan.md`, `run/<plan>/summary.md`, `run/<plan>/summary-<review>.md` only (no `plan-<review>.md`); output has `review`, `summary`, `next` (no `path`); summary's last line: `Approve | Change` (§4.6). (g) pi adapter: Plan review dialog offers `Approve`, `Change` only (no `Open full plan`); summary shown with no link; no plannotator tracker, guard, or event listener; `askguard` message updated (§15). Removed terms appearing only in this note: `review-waiting`, `plan-<review>`, `url_host`, `review host`, `Open full plan`, `plannotator_reviews`, `plannotator_ports`, `PLANNOTATOR_*` | §1.4, §2.2, §4.6, §6.5, §13.2, §14.2, §15, §16, §31 |
| 7 | §5.3 and §5.4, `--session` selection | "a Lead of Plan P" is the Session that is `plans.lead_session`; any other Session, side forks included, follows only Events addressed to itself | §10.3, §11 |

| 9 | §6.3, tmux commands | `respawn-pane`, for a dead pane of the cockpit's layout | §13.1 |
| 10 | §1.2, output | `session run`, `tui view` and `tui --attach` own their terminal instead of printing one JSON value (the outline's "builds and attaches the cockpit") | §1.2 |
| 11 | §1.2 table of codes | the error codes this file introduces: `context_required`, `session_required`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id`, `context_exists`, `question_answered`, `not_a_git_repo`, `base_not_found`, `plan_exists`, `illegal_phase`, `no_draft_jobs`, `review_decided`, `dependency_cycle`, `worktree_exists`, `watch_start_failed`, `spec_invalid`, `tier_invalid`, `job_key_exists`, `needs_unknown`, `needs_unavailable`, `nothing_to_edit`, `job_merged`, `job_not_failed`, `job_not_ready`, `spawn_failed`, `attempt_not_running`, `session_malformed`, `driver_lock_missing`, `jobs_not_done`, `rebase_conflict`, `not_a_lead`, `grounded_lead`, `no_cockpit`, `wrong_pane`, `cursor_ahead`, `not_a_terminal`, `doctor_failed`, `pi_not_found`, `catalog_failed`, `file_unreadable`, `invalid_name`; most are cpd 1's (cpd 1 §11 and its `src/`) | §1.3 and each command |
| 12 | §6.1, global options | the options `focus-events on`, `pane-active-border-style fg=blue` and `pane-border-format` (titles, Title Case, unpadded, a grey pill with black text and a blue one with black text when focused, hs-d475, hs-d534 (replacing hs-d530's): a Lead's `@cpd_title` and another pane's command, `Worker · <job> (Read-Only)`; none for the tree's CPD mark, the plan and widgets panes, Leads without a title, the chat placeholder and the StatusBar), the root `MouseDown1Pane` binding and the `pane-died` hook that respawns only a pane with `@cpd_view`: all set at every server start, not only by `cpd tui` | §13.1 |
| 13 | §6.2, §6.3 | a Worker view pane carries `@cpd_view worker`, `@cpd_job` and `@cpd_attempt`, opens with `split-window -h -d -l 50%` and retargets with `respawn-pane -k`; tmux commands `respawn-pane -k`, `set-hook` and `display-popup` | §13.2 |
| 14 | §6.4, §3.6, §5.2, §1.2 | `session close <id>`: kills a Lead's pane, the Session is `stopped` with `session-moved {place: "none"}`, the Lead in the chat slot refused (`in_chat_slot`, exit 3) | §10.9 |
| 15 | §6.3, reset-layout | `cpd tui --respawn` restarts every live view; a StatusBar added to an existing `main` is followed by reset-layout; `cpd tui` applies §6.1's options to a running server | §13.1 |
| 16 | §2.2 Plan, §4.5, §7.6 | `pr poll` stores the PR's title, review decision and checks summary in `plans.pr_title`, `pr_review_decision` and `pr_checks`, added by migration 2 | §9.2 |
| 17 | §6.2 (the views) | the Dispatch column is the machine's hostname and load, the PRs of the Plans in the cockpit, git (future) and Jira (future); no engine, slots, Escalations or Events | §13.2, §17 |
| 18 | §6.2 (the views) | the plan view's top row is ` <context id> ▸ ` and the tabs only; one tab per Lead Session of the Context with a pane; the tree's keys are ↑↓/jk, Enter, `n`, `x`, `?` and its cursor, clicks and prompts are the reference's | §13.2 |
| 19 | §6.2 (the views) | what each view reads, in process; the tree's dot (needs you) rule from Events; the plan row no longer reads Escalations, the Plan review or the PR | §17 |
| 20 | §6.2 (the views) | the legend opens from the tree's `?` and the StatusBar button, not from the Dispatch column; its section titles say Context and it lists no `cancelled` | §13.2 |
| 21 | §6.2 (the views), R36 | the StatusBar's left side holds action buttons only (`󰨝`, `󰨁`, `󰧭`) and its right side read-only information; the Event log is the `logs` view, a popup the `󰧭` button opens, not part of the Dispatch column | §13.2, §17 |
| 22 | §2.2 Context, Plan, Session | `archived_at` on the three; "Archived" (the archive rule, the cascade, the `archived` refusals); titles editable, ids immutable | §3.7–§3.9, §4.9–§4.11, §10.10–§10.11, §1.3 |
| 23 | §4.5 | Migration 3: `archived_at` on `contexts`, `plans`, `sessions`; `events` recreated with the five new kinds | §3.2, §4.2, §10.1 |
| 24 | §5.2 | Event kinds `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived` | §3.7–§3.9, §4.9–§4.11, §10.10–§10.11, §17 |
| 25 | §5.3, delivery table | the adapter ignores the five kinds: no message, no turn | §15.3 |
| 26 | §2.2 "Archived", §1.3 table of codes | refusals `archive_blocked` and `archived` (exit 3); `in_chat_slot` for archive | §1.3, §4.1, §10.3, §10.4 |
| 27 | §6.2 (the views) | the tree: labels are titles and names, `…` rows for archived objects, the right-click menu | §13.2, §17 |
| 28 | §6.2, chat slot rule and `@cpd_view` values | the chat slot is the pane of `cpd:main` that has `@cpd_session`, or `@cpd_view` `chat` or `history`; new value `history`, a pane running `cpd tui view history <id>` with `@cpd_history <id>`; at most one placeholder or history pane, only in the slot; the pane title table lists `history` | §10.4, §10.5, §10.12, §13.2, §17 |
| 29 | §6.3, §6.4 | tmux commands: `respawn-pane -k` also puts the history view in the slot; `set -p` also sets `@cpd_view history` and `@cpd_history`; `session open` kills a history pane like the placeholder; `session history` parks a Lead in the slot first; `session run` archives on the User's quit | §10.4, §10.8, §10.12 |
| 30 | §2.2 "Archived", §6.2 (the views) | the tree: archived rows drawn at their live depth, normal guides, `Meaning::Nothing` colour, no blank row; a click on `…` toggles, a click on a guide does nothing; Archive (confirmed in a popup since hs-d297, which supersedes hs-d90's no `y/N`); Archive of the chat slot's Lead opens another Session or parks first; archived rows offer Rename… and Unarchive; a click on an archived Session runs `session history` | §13.2 |
| 31 | §2.2 "Archived", §6.7 | auto-archive when pi exits with status 0 and the wrapper did not cause it: park if in the chat slot, then `session archive` or (a Plan's own Lead) `plan archive`; a refusal archives nothing and keeps the resume prompt | §10.8 |
| 32 | §10.13, §10.4, §3.6, §3.7 | `session send` to a `stopped` Lead or Manager starts it (A1) as a parked window with `started` and `pane` in the JSON (A3); contract §3.6 describes a pane that is gone, contract §3.7's E2 restarts a Session when its pane is gone (A4, hs-d726, hs-d727, hs-d725), with a 10-minute guard against restart loops (A4, session_restart_loop) | §10.13, §10.4 |

Row 15's StatusBar rule now covers every missing pane of the layout, not only the StatusBar: [§13.1](#131-tui) (tui-W5).

## 20. Sections changed by R19 and R20

Job `contract-gate-pr` applied the User's decisions R19 (no Job-level Gate or Reviewer: the Gate runs only at `pr open` and `pr rebase`; the review is the PR's own, on GitHub) and R20 (at submit, preconditions 1–4 of §7.3 plus the quick check, `pre-commit` only, with no fallback to `pre-merge`). No section number changed; sections that emptied keep their heading. The Jobs that implement these sections read this list.

**Renames and removals**, everywhere they appeared:

| Was | Now |
|---|---|
| Job status `in_review` | removed: a Job stays `running` until `merged`, `queued` again, `failed` or `cancelled` |
| Attempt statuses `reviewing`, `awaiting_lead` | removed: `attempt submit` moves an Attempt from `running` to `merging` |
| Attempt outcome `approved` | renamed `merged` (outcome `merged`, cause `merged`) |
| Attempt outcome `rejected`, cause `review_rejected` | removed |
| Attempt causes `gate_failed`, `gate_error` | removed from the merge; `gate_failed` (exit 3) and `gate_error` (exit 1) are now refusals of `pr open` and `pr rebase` |
| driver steps A6, A7 | removed; A8 and A9 keep their numbers |
| §7.4 step 6 (the Gate at merge) | removed; steps 7–10 keep their numbers |
| §7.6 `pr rebase` steps 1–4 | steps 1–5: the Gate is the new step 4, the push is step 5 |
| columns `jobs.review_tier`, `attempts.reviewer_session`, `verdict`, `verdict_feedback`, `verdict_by`, `merge_tail`, `review_metrics`; Session role `reviewer` | removed |
| Event kind `review-needed`; `attempt-nudge` for a Reviewer | removed |
| config keys `default_review_tier`, `review_mode` | removed |
| flags `--review-tier` (`job add`, `job edit`), `--reviewer` (`attempt read`, `tui view worker`); command `review decide`; error code `not_awaiting_review` | removed |
| files `run/<plan>/reviews/` (brief, `.diff`, `.check.txt`, `.gate.txt`) | removed; new `run/<plan>/logs/pr.pre-merge.log` |
| quick check falls back to `pre-merge` | no fallback: no `pre-commit` table means no quick check and it passes |

**contract.md:** the status and sources lines; §1.1, §1.2, §1.4, §2.2 (Job, Attempt, Session), §2.3, §2.4, §3.2, §3.4, §3.6, §3.7 (E4), §4.4, §5.1, §5.2, §5.3, §6.2, §6.3, §6.4, §6.5, §7.3, §7.4, §7.6, §8.2, §9, §10 (R13, R17 superseded for cpd 2, R19, R20).

**commands.md:** the introduction and contents; §1.5, §2, §4.2, §4.3, §4.5, §5 (the Tier flag), §5.1, §5.2, §5.3, §5.6, §5.7, §6.1, §6.2, §6.3, §6.4, §7 and §7.1 (emptied), §9.1, §9.3, §11, §13.2, §14.1, §14.2, §15.3, §16 (now "The Worker contract"), §16.1, §16.2 and §16.3 (emptied), §17, §18, §19 (amendment 11), §20 (new).

**CONTEXT.md:** Dispatch, Ledger, Session, Reviewer (removed), Gate, Quick check, Merged.

## 21. `inbox`

Contract §2.2 "Inbox item", §3.7 E8, §4.4, §5.2. The Inbox is the single source of "needs you": one stored item per thing that needs the User, opened and closed by the engine (E8) from its cause, never by a command. The commands read the items and hide them, except `inbox approve` (§21.13), which approves a `deploy` item's run and closes the item. Area `core/src/inbox/`. This section describes the core kinds (`dialog`, `plan-review`, `loop-guard`, `drafts-parked` (magenta, label `<plan>: <N> parked drafts`, links the Plan and its Lead, opened from engine E20's wake, contract §2.2, §3.7 "Parked drafts"; hs-d731, hs-d733), `unread` and `engine-down`, which is not stored; `engine-error` is no longer opened, hs-d314 and nor is `job-stuck`, hs-d318: a stuck Job is a message to the Context's Manager, contract §3.7 "A stuck Job") and the PR poll's (`pr-ready` and `pr-failing`, `pr-changes`, which are no longer opened, hs-d470) `pr-closed` (red: a closed-unmerged PR; contract §2.2, §3.8), and the team review poll's (`review-requested`, §21.5; and the PR poll's deploy read's, `deploy`, one item per deploy run that waits for the User's approval, from `run/deploy-runs.json`, contract §2.2 "The deploy rule", §7.6 step 8: it links no Plan, Context, Job or Session and its `url` is the run's page;  contract §2.2 gives their conditions, urgency and labels and their `url` is the pull request's page).

An item as printed:

```json
{"id": 12, "kind": "job-stuck", "urgency": "red", "label": "inbox/core-docs failed", "subject": "job:inbox/core-docs", "context": "building-cpd", "plan": "inbox", "job": "core-docs", "session": null, "record": null, "event": 48311, "url": null, "opened_at": "2026-10-14T21:40:02.114Z", "closed_at": null, "snoozed_until": null, "dismissed_at": null, "claimed_at": null, "age_seconds": 1260}
```

`age_seconds` is `closed_at` (or now, for an open item) minus `opened_at`. `dismissed_at` and `claimed_at` are null except for hidden items; `dismissed_at` is set when `inbox dismiss` hides an item for the rest of its life; `claimed_at` is set when `inbox claim` hides a `review-requested` item. The item `engine-down` has `id: null`, `subject: "engine"` and no links. An item's `label` is one row or two (`<row1>`, or `<row1>\n<row2>`; contract §2.2 "Inbox item", pro-d171): `inbox list` and `inbox show` print it as stored, with its `\n`; every Event text and the `inbox claim` message name the item by its one-line form, `<row1>: <row2>` (just `<row1>` when Row 2 is empty).

### 21.1 `inbox list`

```
cpd inbox list [--context <c>] [--all] [--closed] [--limit <n>]
```

| Flag | Meaning |
|---|---|
| `--context <c>` | keep the items of that Context (`engine-down` has none, so it is dropped) |
| `--all` | also list the snoozed, dismissed and claimed items, each with `hidden: "snoozed"`, `"dismissed"` or `"claimed"` |
| `--closed` | list closed items instead of open ones, newest `closed_at` first |
| `--limit <n>` | with `--closed`: at most n items; default 50 (1–5000) |

| Precondition | Code | Exit |
|---|---|---|
| `--all` and `--closed` are not given together and `--limit` comes only with `--closed` | `usage` | 2 |
| `--context` names a Context that exists | `context_not_found` | 4 |

Effects: none; it takes no lock (§4.2). Open visible items come in the order of §2.2 (red, yellow, magenta, blue, cyan, white; oldest `opened_at` first), with `engine-down` first when `locks/watch.lock` is free **and no live Manager Session exists** (role `manager`, not archived; hs-d318, hs-d363: with one, the tree tells it when the engine stays down 60 s, §13.2). A snoozed item whose `snoozed_until` has passed is visible again. `hidden` counts the open items now hidden by snooze, dismissal or claim, whether or not `--all` lists them; when several apply, the item's `hidden` is `dismissed`, then `claimed`, then `snoozed`. `events_cursor` is the highest Event id, so a follower can start from it. Each item's `label` may hold one `\n` (Row 1, Row 2; the intro of this section).

```json
{"ok": true, "engine": {"running": true}, "items": [ {"id": 12, "kind": "job-stuck", "...": "..."} ], "hidden": {"snoozed": 1, "dismissed": 0, "claimed": 0}, "events_cursor": 48320}
```

Idempotent: read-only.

### 21.2 `inbox show`

```
cpd inbox show <id>
```

| Precondition | Code | Exit |
|---|---|---|
| the item exists (open or closed) | `item_not_found` | 4 |

Effects: none. Output `{"ok": true, "item": {…}}`, the item as above (its `label` with its `\n` when it has two rows; the intro of this section). Idempotent: read-only.

### 21.3 `inbox snooze`

```
cpd inbox snooze <id> <15m | 1h | tomorrow>
```

`15m` and `1h` are that long from now; `tomorrow` is the next 09:00 in local time (the offset read with `date +%z`, which GNU and BSD `date` both print). Any other value is `usage` (exit 2).

| Precondition | Code | Exit |
|---|---|---|
| the item exists | `item_not_found` | 4 |
| the item is open | `item_closed` | 3 |

Effects, one transaction: **rows** `inbox_items.snoozed_until`; **Event** `inbox-snoozed` `{item, until}`, whose text names the item by its one-line form (the intro of this section). The item returns at its rank when the time passes; its `opened_at` is kept.

```json
{"ok": true, "item": {"id": 12, "snoozed_until": "2026-10-14T22:40:02.114Z", "…": "…"}}
```

Idempotent: snoozing again replaces the time and appends another Event.

### 21.4 `inbox dismiss`

```
cpd inbox dismiss <id>
```

| Precondition | Code | Exit |
|---|---|---|
| the item exists | `item_not_found` | 4 |
| the item is open | `item_closed` | 3 |

Effects, one transaction: **rows** `inbox_items.dismissed_at`; **Event** `inbox-dismissed` `{item}`, whose text names the item by its one-line form (the intro of this section). For a `review-requested` item, the dismissal holds for the pull request forever (C1, contract §2.2 "The team review rule"): an open or closed `review-requested` item with `dismissed_at` or `claimed_at` set suppresses new items for that pull request. For any other kind, the item stays hidden for the rest of its life; because an item lives only while its cause holds, a new cause is a new item and shows again. For `models-changed` the cause is one `changes` of one provider (subject `models:<provider>/<changes.at>`): dismissing it hides that notice and the provider's next change is a new subject, a new item, shown again.

```json
{"ok": true, "item": {"id": 12, "dismissed_at": "2026-10-14T21:41:02.114Z", "…": "…"}}
```

Idempotent: yes. An item already dismissed returns `"already": true` and writes nothing, with no Event.

### 21.5 The team review cache (engine E11)

Not a command. The team review read (Context decision "Team review Inbox", User, 2026-10-19; contract §2.2 "The team review rule", §3.7 E11): engine step E11 runs the Slack helper (§21.6) every `watch.team_review_seconds` (contract §8.2, default **1800**) after the end of the previous run, when `inbox.team_channel` is set and caches what it read in `run/team-review.json`. **It writes no row and appends no Event of its own** (a failure is reported, below): the file is a derived cache (contract §1.4), like a Context digest.

- **Detached.** The helper runs on its own thread with a deadline of 120 s (§1.2 `tool_timeout`, `details.tool` `node`) and a later tick collects its answer: E11 never holds the tick. `locate_pi` (contract §9) gives the entry and `PI_CODING_AGENT_DIR`; `pi` that is not found is `pi_not_found`. The helper's argv is the one of §21.6, cwd `$CPD_HOME`, under pi's Node.
- **The file**, written to `run/team-review.json.tmp` and renamed, so a reader never sees half of it: `{"version": 1, "polled_at": <time>, "channel": "C02JU6C2Z1D", "read": 74, "posts": [ … ]}`, `posts` as the helper printed them (§21.6: `repo`, `number`, `ts`, `ts_raw`, `author`, `primary`, `reactions`, `claim`, `eyes_me`, `thread_tags_me`, newest `ts` first). An answer with no `posts` or no `read` is refused (`slack_helper_failed`) and the file stays.
- **Failures** leave `run/team-review.json` **as it was**: the last good copy stays (hermes `fetch_slack`, `refresh.py:59-93`). Each is E11's one `error` Event per code (contract §3.7; `source` `watch`, the code and `details` the helper's, so `slack_auth` says `pi mcp login slack`); the Manager receives the Event (hs-d314) and no Inbox item opens.
- **What reads it.** The Inbox rule, in E8 (contract §2.2 "The team review rule", hs-d320): the status of each primary post's pull request from one memoised `gh api graphql` call, with the Slack overrides from `claim`, `eyes_me` and `thread_tags_me` (hermes `classify` 264-341, `refresh.build` 435-466); only `NEEDS_REVIEW` opens an item. It is not E11's: E11 calls no GitHub. A missing file is "unknown": the open items stay.

### 21.6 The helper `slack-channel.ts`

`pi/lib/slack-channel.ts` in cpd's `pi/` package, beside the adapter (contract §8 `pi_adapter`). Not a cpd command: engine E11 (§21.5) runs it, detached. It calls the Slack MCP server's tools **through pi's own MCP client and token store**, so cpd has no HTTP client (hs-d107) and never reads, copies or logs the credential (as hermes' `slack_fetch.py:21-28` did through Hermes' own storage). Evidence and the rejected alternatives: `docs/research/team-review.md`.

```
node <adapter>/lib/slack-channel.ts --coding-agent <…/@earendil-works/pi-coding-agent/dist/index.js> --channel <C…> --me <U…>
```

Environment: `PI_CODING_AGENT_DIR` (the agent directory of the pi release, contract §9). cwd is `$CPD_HOME`; the helper hands it to the connection, which sends it to the server as its root (`extensions/mcp/runtime.js:149`). Constants in the helper, each with its source: tool `slack_read_channel`, server entry `slack` of `<agentDir>/mcp.json` (the global file only; a project file is never read), `limit` 200 (`refresh.py:56`), window 7 days (`refresh.py:626-641`), 45 s for the channel read (`slack_fetch.py:183`), 30 s for each thread or reaction read (`slack_fetch.py:211, 324`), at most 8 of those at once (`refresh.py:353`).

**Entry.** The helper answers only when it is the script `node` was started with and it decides by real path: the installed adapter path is a symlink (`<share>/pi` to `pi.<id>`, docs/implementing.md "The adapter swap") and Node resolves symlinks in a module's `import.meta.url` but not in `process.argv[1]`, so comparing the two as they are never matches there and the script would exit 0 with no output. Reached through a symlink to the script or to a directory that holds it, the helper runs as when reached by its real path. A module that only imports the helper does not run it.

What it does:
1. **Loads pi's modules** by file URL from the directory of `--coding-agent`: `extensions/mcp/config.js` (`loadMcpConfig`) and `extensions/mcp/runtime.js` (`McpServerConnection`, `McpOAuthCredentialStore`, `createDefaultTransport`). pi's `package.json` does not export them, so no bare import works; contract §9's embedded module imports pi the same way. It builds the connection as `pi mcp list` does (`extensions/mcp/cli.js:67-76`) and **closes it before exiting**, so a refreshed token is saved. A failure here, or a changed API, is `slack_helper_failed`.
2. **Reads the channel**: `callTool("slack_read_channel", {channel_id, limit: 200})`; the text blocks joined by a newline (`slack_fetch.py:164-180`). Not signed in (the connection's state `needs-auth`, pi's message `requires sign-in`, or a failure text containing `invalid_token`, `unauthorized`, `invalid_auth` or `token_expired`, `slack_fetch.py:55, 91-93`) is `slack_auth`. A timeout, a result with `isError`, an empty text or any other failure is `slack_failed`.
3. **Validates before it answers** (hermes `fetch_slack`, `refresh.py:82-89`): the payload is unwrapped (`unwrap` and `_unescape`, `refresh.py:96-142`: the `{"result": …}` envelope, sometimes truncated, JSON-escaped) and must contain at least one `=== Message from `; else `slack_unparseable` (the bytes read in `details.bytes`), which keeps E11 (§21.5) from replacing its last good file with what looks like a quiet channel.
4. **Parses** (`parse_messages`, `refresh.py:145-194`): blocks start at `=== Message from `; a block with no `Message TS: <digits.digits>` is skipped; posts older than 7 days are skipped; the poster is the text before the first `(` and then before `<`; the PRs are every `github.com/<owner>/<repo>/pull/<n>` in the block, de-duplicated, trailing `.,)>|` stripped from the repo (`extract_prs`, `classify.py:74, 105-115`); the **primary** PR is the first of them; the newest posting of a PR wins. `reactions` are the names of the block's `Reactions: eyes (1), …` line, without counts (`parse_reactions`, `classify.py:118-127`).
5. **The claim** (`slack_claim`, `classify.py:141-174`): `claim` is `{kind: "claimed", reason: "claimed with :eyes: in Slack"}` when the reactions include `eyes` (`CLAIM_REACTIONS`); else, after the app's footer line `*Sent using* <@U…|Name>` is removed from the block (`APP_ATTRIBUTION_RE`, `classify.py:82-84`), when the block mentions someone (`<@U…|`): null if `--me` is among them (being named is the request), else `{kind: "claimed", reason: "assigned to someone in Slack"}`; else null.
6. **Two more reads, for the Slack overrides of the Inbox rule (contract §2.2)**, each failing soft as in hermes: for every distinct message `ts` among the posts, `slack_read_thread {channel_id, message_ts}`; `thread_tags_me` is whether the replies mention `--me` outside an app footer (`mentions_me`, `classify.py:130-138`); a failed read is false (`thread_text`, `slack_fetch.py:211-231`). For each distinct message `ts` of the posts whose `claim` is not null and whose `reactions` include `eyes` (the only line that is read; hermes asked for any reaction and found nothing on the others), `slack_get_reactions {channel_id, message_ts}`; the `eyes` line is read as `:eyes: × 2 — Ryan Case (U039194B7D2), clayton (U04DB4MTUMR)` (the em dash, `slack_fetch.py:353-364`; the `{"result": "…"}` envelope unwrapped first); `eyes_me` is true when `--me` is the only user id there (a peer's claim wins, `claimed_by_me`, `refresh.py:251-281`), false when a peer is there, null when the read failed or found no one (the caller then reads the claim as a peer's). Hermes matches display names; the helper matches the Slack user id in the parentheses (unverified against the live server).

Output, one JSON value on stdout (contract §1.2), progress on stderr:

```json
{"ok": true, "channel": "C02JU6C2Z1D", "read": 74,
 "posts": [{"repo": "acme/pay", "number": 812, "ts": 1760951522.0113, "ts_raw": "1760951522.011300",
            "author": "Ryan Case", "primary": true, "reactions": ["eyes"],
            "claim": {"kind": "claimed", "reason": "claimed with :eyes: in Slack"},
            "eyes_me": false, "thread_tags_me": false}]}
```

`posts` is newest `ts` first, one entry per PR; `read` is the number of `=== Message from ` blocks in the payload (hermes `scanned`); `ts_raw` is Slack's string, kept exact (`refresh.py:174-176`).

| Failure | Code | Exit |
|---|---|---|
| a flag is missing or malformed, or `PI_CODING_AGENT_DIR` is not set | `usage` | 2 |
| the `slack` server is not in `<agentDir>/mcp.json`; the message: `run: pi mcp add slack --url https://mcp.slack.com/mcp --exposure hidden` | `slack_not_configured` | 3 |
| Slack needs a sign-in (step 2); the message: `run: pi mcp login slack` | `slack_auth` | 3 |
| the channel read failed, timed out, or answered `isError` or no text (step 2) | `slack_failed` | 1 |
| the payload holds no message (step 3) | `slack_unparseable` | 1 |
| pi's modules did not load or no longer have the shape the helper uses (step 1; after a pi upgrade); `details.message` is the thrown error | `slack_helper_failed` | 1 |

A helper that exits 0 and prints no result is not one of these refusals: E11 reports it as `spawn_failed`, whose message is `<argv> exited 0 but printed no result.` (any other exit with no result keeps `<argv> printed no result.`) and whose `details` are `{argv, exit, stdout, stderr}`, the last two the tail of what the child printed.

Writes nothing: no file, no Slack message, no reaction (hermes' `post_message`, `add_reaction` and `mark_merged` are not ported); the message is posted by the helper `slack-post.ts` (§21.10) and the reaction is queued by the engine step (§21.9). The credential is read and refreshed inside pi's modules; the helper holds no token.

**The helper is no Session** (contract §6.11): `--no-mcp` and `cpd mcp attach` (§24.1) do not reach it. It keeps reading the `slack` entry of `<agentDir>/mcp.json`, which therefore stays until a later Plan moves the helper (pro-d10).

**What the User runs once** (`pi mcp login` needs a browser, so it is theirs; cpd never runs it): `pi mcp add slack --url https://mcp.slack.com/mcp --exposure hidden`, then `pi mcp login slack`, then `pi mcp list` shows `slack: connected`. `--exposure hidden` keeps the user-level entry from handing the Slack tools to every Lead and Worker (`docs/research/team-review.md` §3). If Slack refuses pi's dynamic client registration, the User adds `--oauth-client-id` (pi's `pi mcp add` options; unverified for Slack).

### 21.7 `inbox claim`

```
cpd inbox claim <id>
```

| Precondition | Code | Exit |
|---|---|---|
| the item exists | `item_not_found` | 4 |
| the item is open | `item_closed` | 3 |
| the item is not already claimed | (returns ok with `"already": true`, writes nothing) | 0 |
| the item is `review-requested` and not dismissed (C4) | `not_claimable` | 3 |
| config `inbox.review_context` is set (C5) | `review_context_unset` | 3 |
| the Context exists and is not archived (C4) | `context_not_found` | 4 |
| the Context has a live Manager Session (C4) | `no_manager` | 3 |
| the team review cache holds a primary post of the pull request (C4) | `post_not_found` | 3 |

Effects, one transaction: **rows** `inbox_items.claimed_at` = now; **Event** `inbox-claimed` `{item}` (C4), whose text names the item by its one-line form `<row1>: <row2>` (the intro of this section); one `session-message` Event to the Context's Manager (C4), worded to ask the Manager to start a review of the pull request and to name the item by that one-line form, containing the URL and the Slack post's channel and `ts_raw`; **queue** one `slack_reactions` row with `emoji` = `eyes`, `channel` = the team review cache's channel, `ts` = the post's `ts_raw`, `queued_at` = now, `reacted_at` = NULL (the queue step drains it). Output: `{"ok": true, "item": {…, "claimed_at": "…"}, "manager": "<session id>"}` (C4). That Event is appended as `session send` appends it (§10.13): for a Manager whose `runner` is `durable` it starts no pane and its host delivers it (contract §6.13, P1.4; unverified in the code: the Event is written by `inbox claim` itself, not through `session send`).

Idempotent: yes. An item already claimed returns `"already": true` and writes nothing, with no Event.

### 21.8 The helper `slack-react.ts`

`pi/lib/slack-react.ts` in cpd's `pi/` package, beside `slack-channel.ts` (§21.6) and `slack-post.ts` (§21.10, contract §8 `pi_adapter`). Not a cpd command: engine E16 (§21.9) runs it, detached. It calls the Slack MCP server's tools **through pi's own MCP client and token store**, so cpd has no HTTP client (hs-d107) and never reads, copies or logs the credential.

```
node <adapter>/lib/slack-react.ts --coding-agent <…/@earendil-works/pi-coding-agent/dist/index.js> --channel <C…> --ts <message ts> --emoji <name>
```

Environment: `PI_CODING_AGENT_DIR` (the agent directory of the pi release, contract §9). cwd is `$CPD_HOME`; the helper hands it to the connection. Constants in the helper, each with its source: tool `slack_add_reaction`, server entry `slack` of `<agentDir>/mcp.json` (the global file only; a project file is never read), deadline 30 s for the reaction call (matched with the E16 deadline of §3.7).

**Arguments:** validated before connecting: `--channel <C…>` matches `^C[A-Z0-9]{8,}$`, `--ts <ts>` matches `^[0-9]{10}\.[0-9]{6}$`, `--emoji <name>` matches `^[a-z0-9_+-]{1,100}$` and is without colons (Slack's internal format); anything else is `usage` (exit 2).

What it does:
1. **Loads pi's modules** by file URL from the directory of `--coding-agent`, reuses `connect`, `Refusal`, `parseArgs` and `CallTool` from `slack-channel.ts` (§21.6). A failure is `slack_helper_failed`.
2. **Adds the reaction**: `callTool("slack_add_reaction", {channel_id, message_ts, emoji})` with a 30 s deadline. Not signed in is `slack_auth`. A timeout or any other failure is `slack_failed`.
3. **Reads the answer** and validates it: on `ok`, the call succeeded; on an error whose text contains `already_reacted` (the reaction is already there), the call failed with that error, but the state is the desired one (idempotent success, `"already": true`); any other error is `slack_failed`.

Output, one JSON value on stdout (contract §1.2):

```json
{"ok": true, "channel": "C02JU6C2Z1D", "ts": "1793040197.490209", "emoji": "git-merge", "already": false}
```

or `"already": true` when the reaction was already there.

| Failure | Code | Exit |
|---|---|---|
| no `--channel`, `--ts`, or `--emoji`, or arguments malformed, or emoji has colons | `usage` | 2 |
| the `slack` server is not in `<agentDir>/mcp.json` | `slack_not_configured` | 3 |
| Slack needs a sign-in (step 2) | `slack_auth` | 3 |
| the add-reaction call returned a Slack API error for one of: invalid_name, bad_timestamp, message_not_found, channel_not_found, is_archived, thread_locked, not_reactable, too_many_emoji, too_many_reactions, no_item_specified (step 2) | `slack_refused` | 1 |
| the add-reaction call failed or timed out (step 2) | `slack_failed` | 1 |
| pi's modules did not load (step 1) | `slack_helper_failed` | 1 |
| the helper could not be started | `spawn_failed` | 1 |

Writes one Slack reaction (or confirms one already present) and nothing else. The credential is read and refreshed inside pi's modules; the helper holds no token. The caller (engine E16, §21.9) writes the `reacted_at` column after a successful reaction.

### 21.9 The Slack reaction queue (engine E17)

Not a command. A queue of Slack reactions appended by every step that wants to add a reaction to a Slack post: today the merge reaction (Plan pr-post, PR `react`, User decision pro-d64 step 7) and the Claim reaction (Plan review-intake, PR `claim`). Engine step E17 drains the queue, running the helper (§21.8) once per tick for the oldest queued reaction that is due.

**The table** (migration `slack-reactions`, additive; migration `20261102T1300-slack-reaction-refusal`, breaking after 73): `id INTEGER PRIMARY KEY`, `channel TEXT NOT NULL` (CHECK length 1..20), `ts TEXT NOT NULL` (CHECK length >= 1), `emoji TEXT NOT NULL` (CHECK length 1..100, no colons), `queued_at TEXT NOT NULL`, `reacted_at TEXT` (NULL until the helper succeeded), `plan TEXT` (NULL for unlinked; set for merge reactions), `pr TEXT` (NULL for unlinked; set for merge reactions; both plan and pr NULL or both set by CHECK), `refused_at TEXT` (NULL unless the row is parked; both refused_at and refusal NULL or both set by CHECK), `refusal TEXT` (the Slack error name, for example `invalid_name`; both NULL or both set); UNIQUE (channel, ts, emoji); CHECK (reacted_at IS NULL OR (refused_at IS NULL AND refusal IS NULL)); STRICT. It is a data table: it appends no Event, belongs to no Plan and is not a history table. Queue states: pending (queued_at set, reacted_at and refused_at NULL), reacted (reacted_at set), refused (refused_at set, reacted_at NULL). A row is never deleted.

- **When:** each tick, when incident mode holds (hs-d643), log `E17: holding: incident mode is on` and do nothing; otherwise, when a helper run is in flight, collect its answer; otherwise, pick the oldest queue row with `reacted_at` NULL whose last failure in this engine's life is at least `watch.error_repeat_seconds` old (the step remembers failures per row id, in memory, so one row that keeps failing never blocks the rows behind it; a restart retries at once). With none, clear the step with `engine.clear(Step::E17, &About::Machine)` and nothing else.
- **The helper call:** `cpd_core::slack::call(&config, "slack-react.ts", &[("channel", channel), ("ts", ts), ("emoji", emoji)], None, <120 s deadline>)` (§3.7 "Deadline", §1.2 `tool_timeout`), run detached on its own thread (never holding the tick), answer read with `cpd_core::slack::answer` at a later tick.
- **On success** (`ok: true`, `already` true or false): one store transaction sets that row's `reacted_at` to the transaction's time where it is still NULL (`UPDATE slack_reactions SET reacted_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND reacted_at IS NULL`), no Event and the step is cleared.
- **On `slack_refused`** (contract §3.7 A4): the row is parked in one transaction: set `refused_at` to now and `refusal` to the error name (for example `invalid_name`) and append one `error` Event (`source` `watch`, code `slack_refused`, scoped to the Plan when the row has one, `details` `{id, emoji, reason, channel, ts}` plus `{plan, pr, file}` for a merge reaction; `file` the policy file that sets `post.merged_reaction` for the PR). The row is not retried by E17 but re-checked each tick: when the policy names a non-empty emoji different from the row's emoji, the row is reopened (cleared `refused_at` and `refusal`, updated `emoji`, returned to pending state); when the policy names no emoji, the row is parked with refusal `policy_empty`.
- **On any other failure:** `engine.fail(Step::E17, About::Machine, failure)`: one `error` Event per code (`source` `watch`, reported again only when the code changes or after `watch.error_repeat_seconds` has passed; hs-d514; the codes are the helper's: `usage` (malformed call, unverified), `slack_not_configured`, `slack_auth`, `slack_failed`, `slack_helper_failed`, or the spawn codes `pi_not_found`, `spawn_failed`, `tool_timeout`), no Inbox item and **`reacted_at` stays NULL**. The same row is tried again `watch.error_repeat_seconds` after the end of the failed run: a reaction is never lost, only late.
- **What it does not do:** Reactions are added only by this step, never by E11 or any other operation. A parked row is not retried until the policy changes (for a merge reaction) or manually reopened (not a command today).

### 21.10 The helper `slack-post.ts`

`pi/lib/slack-post.ts` in cpd's `pi/` package, beside `slack-channel.ts` (§21.6) and `slack-react.ts` (§21.8, contract §8 `pi_adapter`). Not a cpd command: `pr post` (§9.8b) runs it and later engine E16 (§21.9) runs `slack-react.ts` the same way. It calls the Slack MCP server's tools **through pi's own MCP client and token store**, so cpd has no HTTP client and never reads, copies or logs the credential.

```
node <adapter>/lib/slack-post.ts --coding-agent <…/@earendil-works/pi-coding-agent/dist/index.js> --channel <C…>
```

Environment: `PI_CODING_AGENT_DIR` (the agent directory of the pi release, contract §9); `CPD_HOME`, `CPD_PLAN`, `CPD_SESSION` and `CPD_ATTEMPT` are passed; others from the caller are removed. cwd is `$CPD_HOME`; the helper hands it to the connection. Constants in the helper, each with its source: tool `slack_send_message`, server entry `slack` of `<agentDir>/mcp.json` (the global file only; a project file is never read), `unfurl_app_links: false` (source: decision pro-d245, a PR posted to Slack is the title and the link with no GitHub preview; fixed in the helper, there is no policy key), message size 1 to 5000 bytes (UTF-8), 30 s for the send call (contrast hermes' 45 s read).

**Entry.** The helper answers only when it is the script `node` was started with, same as `slack-channel.ts` (§21.6).

What it does:
1. **Loads pi's modules** by file URL from the directory of `--coding-agent`, reuses `connect`, `Refusal` and `CallTool` from `slack-channel.ts` and its `McpServerConnection`. A failure is `slack_helper_failed`.
2. **Reads stdin** (UTF-8, up to 5000 bytes): the message text; no message or over 5000 is `usage` and nothing is sent.
3. **Sends the message**: `callTool("slack_send_message", {channel_id, message, unfurl_app_links: false})` with a 30 s deadline. The explicit `false` asks Slack for no rich preview of the GitHub link, so the post is the rendered title and link and nothing else. The Slack tool has no other preview switch, and whether Slack then shows a plain link preview of its own is not decided by this helper. Not signed in is `slack_auth`. A timeout, `isError`, or any other failure is `slack_failed`.
4. **Reads the answer** and validates it: the real tool answers `{"message_link":"…","message_context":{"message_ts":"<ts>","channel_id":"<C…>"}}`. An invalid result, or a channel_id that does not match the channel asked, is `slack_unparseable` with `details.message` that may have been posted, check the channel.

Output, one JSON value on stdout (contract §1.2):

```json
{"ok": true, "channel": "C02JU6C2Z1D", "ts": "1793040197.490209", "link": "https://missionlane.slack.com/archives/C02JU6C2Z1D/p1793040197490209"}
```

| Failure | Code | Exit |
|---|---|---|
| no `--channel`, or stdin over 5000 bytes, or empty | `usage` | 2 |
| the `slack` server is not in `<agentDir>/mcp.json` | `slack_not_configured` | 3 |
| Slack needs a sign-in (step 3) | `slack_auth` | 3 |
| the send timed out or failed (step 3) | `slack_failed` | 1 |
| the answer is not the expected JSON, or channel_id does not match | `slack_unparseable` | 1 |
| pi's modules did not load (step 1) | `slack_helper_failed` | 1 |
| the helper could not be started | `spawn_failed` | 1 |

Writes one Slack message and nothing else. The credential is read and refreshed inside pi's modules; the helper holds no token. The caller (§9.8b) writes the `pr_posts` row after a successful send.

### 21.11 The Jira transition queue (engine E19)

Not a command. A queue of Jira status moves, one row per PR and target, for the PR's own ticket (`prs.jira`, §9.4, pro-d127). `in_progress` is queued when the first Attempt of any Job of the PR starts, `in_review` when the PR is posted (§9.8b), `done` when the PR is recorded merged. The status names are the policy's (§22.1: `pr.jira.in_progress`, `pr.jira.in_review`, `pr.jira.done`); a target whose name is empty has no transition. The policy's `pr.jira.estimate_fields` (§22.1) names the Jira fields the helper sets to 1 before a `done` move (§21.12, step 5). The three steps that queue only append a row, in their own transaction: they never read policy and never call Jira. Engine step E19 drains the queue through the helper (§21.12). An outage never blocks or fails a step.

**The table** (migration `<timestamp>-jira-transitions`, additive; contract §4.4): `jira_transitions`, a data table: no history triggers, no Event kind and a row is never deleted but by `plan delete` (§4.14). Its columns: `id` (INTEGER PRIMARY KEY); `plan` and `pr` (both NOT NULL, a foreign key to `prs(plan, key)`); `target`, one of `in_progress`, `in_review`, `done`; `queued_at`, the transaction's time; `settled_at` and `outcome`, `outcome` one of `moved`, `unchanged`, `skipped`; `refused_at`, `refusal` (the reason) and `refused_with` (`<ticket>|<status name>` as it was refused). UNIQUE (`plan`, `pr`, `target`): queueing a target twice is a no-op (`ON CONFLICT DO NOTHING`). STRICT. CHECKs: `settled_at` and `outcome` are both NULL or both set; `refused_at`, `refusal` and `refused_with` are all NULL or all set; a row is never settled and parked at once.

Queue states, each row in exactly one:
- **PENDING**: `settled_at` and `refused_at` NULL.
- **SETTLED**: `settled_at` and `outcome` set: `moved` (the ticket was moved), `unchanged` (it was already at or past the target), `skipped` (the policy names no status for the target).
- **PARKED**: `refused_at`, `refusal` and `refused_with` set.

**Who queues** (J4), each in the transaction of the step itself, `queued_at` the transaction's time:
- **`in_progress`**: the transaction that starts an Attempt (`insert` in `core/src/driver/start.rs`, §5.5), for the PR of the Attempt's Job. The first Attempt of any Job of the PR queues it; later ones are no-ops.
- **`in_review`**: the transaction of `cpd pr post` (§9.8b, `core/src/pr/post.rs`) that inserts `pr_posts`, only when that insert is new: a refused or repeated post queues nothing.
- **`done`**: every transaction that records a PR merged, next to the queue of the merge reaction (`reactions::queue_merge`): the merge of `core/src/pr/poll.rs` (`pr poll`, §9.2 and `cpd pr merge`, §9.10, which goes through it) and the already-merged branch of `core/src/pr/open.rs` (§9.1).

A PR with no ticket still gets its rows: they wait, PENDING and are never read.

- **When:** each tick, when incident mode holds (like E17 and E18, hs-d643), log `E19: holding: incident mode is on` and do nothing. Otherwise: first the reopen check of the bullet below, for every PARKED row; then, when a helper run is in flight, collect its answer; otherwise pick the oldest PENDING row (`next_due`, lowest `id`) whose PR has a ticket and is not `cancelled`, skipping a row that failed in this engine's life less than `watch.error_repeat_seconds` ago (the step remembers failures per row id, in memory, so a restart retries at once). A row of a PR with no ticket is never picked. With none, clear the step with `engine.clear(Step::E19, &About::Machine)` and nothing else.
- **Policy:** the PR's policy is resolved (`policy_of_pr`). An error is E17's merge-row failure path: `engine.fail(Step::E19, About::Plan(<plan>), failure)` and the row is tried later. When the target's name is empty, the row is settled in one transaction, `mark_settled(Skipped)`: no helper and the step goes on to the next row.
- **The helper call:** `cpd_core::jira::call(&config, &home, <the request: ticket, target, the three names of the policy and estimate_fields, the policy's pr.jira.estimate_fields>, <120 s deadline>)` (§21.12), run detached on its own thread (never holding the tick) and its answer read by `cpd_core::jira::answer` at a later tick, as E17 does (§21.9).
- **On ok** (`moved` or `unchanged`): one transaction, `mark_settled(id, Moved | Unchanged)` and the step is cleared. No Event, no Inbox item. Settling clears the row's Report key (R3 below), as does the `skipped` of the Policy bullet.
- **On `jira_refused`:** one transaction, `refuse(...)` with the helper's `details.reason` (`site_ambiguous`, `issue_not_found` or `no_transition`) and its `message` and the ticket and the status name the run used: the row is parked with `refused_with` `<ticket>|<status>` and one `error` Event is appended (`source` `watch`, code `jira_refused`, scoped to the Plan, `details` `{id, plan, pr, ticket, target, status, reason}`). Its text names the ticket, the PR, the Plan, the status and the reason and says nothing is tried again until the PR's ticket (`cpd pr edit <pr> --jira <KEY>`) or the policy key `pr.jira.<target>` changes. Parking clears the row's Report key (R3 below).
- **On any other failure** (the rules R1-R5 of contract §3.7 E19; the row is never parked by them):
  - **R1, a row failure:** any failure of the helper run, of its start, or of the store write that settles or parks the row, whose code is not `jira_auth`, `jira_not_configured` or (the parking path) `jira_refused`: `jira_failed`, `jira_helper_failed`, `spawn_failed`, `pi_not_found`, `tool_timeout`, and a store error under its own code. The row is not parked and stays PENDING; it is skipped for `watch.error_repeat_seconds` after the failed run's end and retried, so no move is lost: only late.
  - **R2, its Event:** one `error` Event, `source` `watch`, scoped to the row's Plan (`plan` set, no Job or Session), `code` the failure's own, `data.message` exactly `<ticket> could not be moved to "<status>": <the failure's message>` (`<status>` the policy's status name for the target), `data.details` the failure's own details plus `plan`, `pr`, `ticket`, `target`, `status` (E19's keys win a clash). Its text: `The engine failed at E19 (Jira transitions) for the <target> move of PR <pr> of Plan <plan>: <data.message> It tries again at every tick. See: cpd plan show --plan <plan>`.
  - **R3, dedupe:** the Report key is (E19, Plan, PR, target): one Event per code until the row ends (it settles `moved`, `unchanged` or `skipped`, or is parked), and ending the row clears the key. A different code on the same row is a new Event; another row's failure, with the same code in the same Plan, is its own Event. An engine restart forgets the open keys, so a row still failing is reported once more, unless the store holds an Event of the same step, scope and code appended less than `watch.error_repeat_seconds` ago (the Report's existing restart lookup).
  - **R4, the machine-wide codes:** `jira_auth` and `jira_not_configured` stay ONE machine-wide `error` Event each (no Plan), as today: `engine.fail(Step::E19, About::Machine, failure)`, reported again only when the code changes or after `watch.error_repeat_seconds` has passed (hs-d514). A row that settles or is parked clears that machine key as today.
  - **R5:** no Inbox item. The Event reaches the Plan's Lead exactly as any Plan-scoped `error` Event does (hs-d514); nothing about delivery changes.
- **Reopen:** each tick, for every PARKED row, the PR's ticket and policy are read again. When the target's name is non-empty and `<ticket>|<name>` differs from `refused_with`, `reopen(id)` clears `refused_at`, `refusal` and `refused_with` and the normal path runs the row. A policy that does not resolve for a parked row is `engine.fail(Step::E19, About::Plan(<plan>), failure)`. There is no command for it.
- **What it does not do:** no Inbox item and no Event on success; no reaction to a changed ticket after its row settled (a PR whose ticket is replaced after its rows settled is not moved again); no ticket creation or linking (Plan pr-ticket); no change to who may post (Plan post-scope); no `pr view` or `plan show` field for the queue.

### 21.12 The helper `jira-transition.ts`

`pi/lib/jira-transition.ts` in cpd's `pi/` package, beside `slack-channel.ts` (§21.6) and `slack-react.ts` (§21.8, contract §8 `pi_adapter`). Not a cpd command: engine E19 (§21.11) runs it, detached, through `cpd_core::jira::call`. It calls the Atlassian MCP server's tools **through pi's own MCP client and token store**, so cpd has no HTTP client and never reads, copies or logs the credential. The shapes it reads are the real ones of `docs/research/atlassian-mcp.md` (its sections 2 to 6, read on `origin/main`), not guesses.

```
node <adapter>/lib/jira-transition.ts --coding-agent <…/@earendil-works/pi-coding-agent/dist/index.js>
```

The request is ONE JSON object on stdin:

```json
{"server": {"url": "https://mcp.atlassian.com/v1/mcp/authv2", "exposure": "deferred"}, "ticket": "PAY-813", "target": "in_review", "names": {"in_progress": "In Progress", "in_review": "In Review", "done": "Done"}, "estimate_fields": ["customfield_10016"]}
```

- `server`: the `atlassian` entry of the machine policy (`[mcp.servers.atlassian]` in `$CPD_HOME/policy/host.toml`, §22.1 `machine`), with pi's key names, as `mcp attach` hands it to pi (§24.1); `url` is required. The entry must not set `enabled` to `false`: pi takes a server as on unless its `enabled` is `false` (read from pi's source; not run).
- `ticket`: a key of the `--jira` pattern `^[A-Z][A-Z0-9]+-[0-9]+$` (§9.4).
- `target`: `in_progress`, `in_review` or `done`; `names[target]` is non-empty; the other two names may be `""`.
- `estimate_fields`: a list of strings, always present (`[]` when the policy sets none, §22.1 `pr.jira.estimate_fields`); each a Jira custom field id matching `^customfield_[0-9]+$`, no id twice. The helper acts on it only for `target` `done` (step 5).

Anything else is `usage` (exit 2), before connecting.

Environment: `PI_CODING_AGENT_DIR` (the agent directory of the pi release, contract §9). cwd is `$CPD_HOME`; the helper hands it to the connection. The connection is built from `server` and never from `~/.pi/agent/mcp.json`, whose `atlassian` entry is `enabled: false`.

Constants, each with its source: the four tools `getAccessibleAtlassianResources`, `getJiraIssue`, `getTransitionsForJiraIssue` and `transitionJiraIssue` (research §1); 30 000 ms for each call (the research's calls took 0.6 to 2.0 s); 120 s for the whole run, held by E19 (§21.11).

What it does:
1. **Loads pi's modules** by file URL from the directory of `--coding-agent`, reusing `loadPi` and `resultText` of `slack-channel.ts` (§21.6) and builds the connection: `new McpServerConnection({ entry: { name: "atlassian", config: server }, cwd, createTransport: createDefaultTransport, credentials: new McpOAuthCredentialStore(), onTools: () => {} })`, closed before it exits. A failure to load them, or modules of another shape, is `jira_helper_failed`.
2. **Finds the site**: `getAccessibleAtlassianResources {}` answers a JSON array of `{id, url, scopes}`. It keeps the entries whose `scopes` include both `read:jira-work` and `write:jira-work` (the Confluence entry of the same site does not count). None: `jira_auth`. More than one distinct `id`: `jira_refused` `site_ambiguous` (only one site is verified, research §2). Else `cloudId` is that `id`.
3. **Reads the status**: `getJiraIssue {cloudId, issueIdOrKey: ticket, fields: ["status"]}`. The status name is `fields.status.name` and its category `fields.status.statusCategory.key` (research §3). A 404 is `jira_refused` `issue_not_found`. Then the never-backwards rule, in this order; an unchanged answer makes no further call:
   - the name equals `names[target]`: `unchanged`, `why` `at_target`;
   - the category key is `done`: `unchanged`, `why` `done_category`;
   - the name equals the name of a LATER target (the order is `in_progress` < `in_review` < `done`; only non-empty names count): `unchanged`, `why` `past_target`.
4. **Finds the transition**: `getTransitionsForJiraIssue {cloudId, issueIdOrKey: ticket}` answers `{transitions: [{id, name, to: {name}}]}`. The FIRST entry whose `to.name` equals `names[target]` exactly is taken. Never match on `name` or `id`: from `In Review`, `Done` is offered twice, id 31 and id 6 (named `PR Approved`) and every status offers a transition to itself (research §4, §8). None: `jira_refused` `no_transition`.
5. **Writes the estimate**: only for `target` `done` with a non-empty `estimate_fields`, and only after the transition was found (so an `unchanged` answer and a `no_transition` refusal write nothing; `in_progress` and `in_review` never write). `getJiraIssue {cloudId, issueIdOrKey: ticket, fields: estimate_fields}`. A field whose key is ABSENT from `fields` is skipped: the project does not carry it (`fields` itself may be absent). A field that reads `null` is empty; a field with any other value, 0 included, is left alone. When any field is empty: ONE `editJiraIssue {cloudId, issueIdOrKey: ticket, fields: {<each empty id>: 1}}`, the number 1; a non-error answer is success and its text is not parsed. Then step 6.
6. **Moves the ticket**: `transitionJiraIssue {cloudId, issueIdOrKey: ticket, transition: {id}}`, the id as a string. Success is `isError` false and `{"success": true, "statusName": …}`: `outcome` `moved`, `from` the name read in step 3, `to` `statusName` (research §5).

Failure classes are read from a tool's `isError` text (JSON `{error: true, message, statusCode}`, or plain text) and from the connection.

| Condition | Code | Exit |
|---|---|---|
| stdin is not the request above (not JSON, bad ticket, unknown target, empty `names[target]`, no `server.url`, no `estimate_fields`, `estimate_fields` not a list, a string in it that is not a custom field id, or a repeated id), refused before connecting | `usage` | 2 |
| pi's modules did not load, or are of another shape (step 1) | `jira_helper_failed` | 1 |
| sign-in missing: connection state `needs-auth`, or the text holds one of `invalid_token`, `unauthorized`, `invalid_auth`, `token_expired`, `requires sign-in`, or `statusCode` 401 or 403 on any call; or no site with the Jira scopes (step 2). Message: `Atlassian needs a sign-in. Run: pi mcp login atlassian` | `jira_auth` | 3 |
| more than one distinct `id` with the Jira scopes (step 2) | `jira_refused` `site_ambiguous` | 1 |
| `statusCode` 404 on the issue read (step 3) | `jira_refused` `issue_not_found` | 1 |
| no transition whose `to.name` is `names[target]` (step 4) | `jira_refused` `no_transition` | 1 |
| a failed read or edit of the estimate (step 5), not a sign-in failure: `details.tool` `getJiraIssue` or `editJiraIssue`, `details.status_code` when known; the move is not tried and the row stays PENDING (§21.11) | `jira_failed` | 1 |
| everything else a retry may cure: 429, 5xx, a 400 on the transition, a timeout, text that is not the expected JSON, any other tool failure; `details.tool` and `details.status_code` when known | `jira_failed` | 1 |

A `jira_refused` always carries `details.reason`. The spawn codes (`spawn_failed` and `tool_timeout` from the 120 s deadline) are raised by `cpd_core::jira`, not by the helper (J8).

Output, one JSON line on stdout, as the Slack helpers do (contract §1.2):

```json
{"ok": true, "ticket": "PAY-813", "target": "in_review", "outcome": "moved", "from": "In Progress", "to": "In Review"}
```

or, when the ticket is already at or past the target:

```json
{"ok": true, "ticket": "PAY-813", "target": "in_progress", "outcome": "unchanged", "status": "In Review", "why": "past_target"}
```

A failure is the envelope `{"ok": false, "error": {"code", "message", "details"}}` with the exit of the table, for example `{"ok": false, "error": {"code": "jira_auth", "message": "Atlassian needs a sign-in. Run: pi mcp login atlassian", "details": {}}}`.

**What the User runs once:** `pi mcp login atlassian`, from a shell on the machine. It opens a browser for the Atlassian sign-in; cpd never runs it and cpd never reads or writes pi's token store `mcp-auth.json`. The shell form needs an `atlassian` entry with the same url in `~/.pi/agent/mcp.json` (§24.1 "Signing in"); the helper never reads that file. Rerunning the helper is safe: a ticket already at or past the target answers `unchanged`.

### 21.13 `inbox approve`

```
cpd inbox approve <id>
```

Approves the pending deployments of a `deploy` item (contract §2.2 "The deploy rule"; pro-d167). It mirrors `inbox claim` (§21.7): `Command::Approve(DismissArgs)`, `run_with` opens `Env::open_with(ports)` and calls `approve_in(env, id)`, which uses `env.ports.runner` for `gh` and `env.ports.clock` for the time. It never rejects a deployment. The cockpit's **Approve** entry runs it (§13.2).

| Precondition, in this order | Code | Exit |
|---|---|---|
| the item exists | `item_not_found` | 4 |
| the item is open | `item_closed` | 3 |
| the item is `deploy` (a snoozed or dismissed one too); `details` `{item, kind}` | `not_approvable` | 3 |
| its subject is `deploy:<owner>/<repo>#<run_id>` with `<run_id>` a u64; else the database holds an invalid row and nothing else is done | `store_failed` (a bad row) | 1 |
| READ: `gh api` answers within 15 s with a JSON array of `{"environment": {"id": <u64>, "name": "<str>"}, "current_user_can_approve": <bool>, …}` (other members ignored); a failed call, a timeout or JSON of another shape | `gh_failed` | 1 |
| WRITE (only when some entry is approvable): the POST answers within 15 s; a failed call | `gh_failed` | 1 |

A `gh_failed` has the `details` of §1.3 (`argv`, `exit`, the last 60 lines of stderr). Its item stays open and nothing is written.

Effects, in this order:

1. **READ**: `gh api repos/<owner>/<repo>/actions/runs/<run_id>/pending_deployments`, argv exactly `["api", "repos/<owner>/<repo>/actions/runs/<run_id>/pending_deployments"]`, cwd the cpd home root, deadline 15 s, as the PR poll's deploy read (contract §7.6 step 8; `core/src/pr/deploys.rs`). It validates the answer at the boundary. `approvable` is the entries with `current_user_can_approve` true, sorted by (environment name, environment id).
2. **WRITE** (when `approvable` is not empty): `gh api --method POST repos/<owner>/<repo>/actions/runs/<run_id>/pending_deployments --input -`, argv exactly `["api", "--method", "POST", "repos/<owner>/<repo>/actions/runs/<run_id>/pending_deployments", "--input", "-"]`, deadline 15 s, `GH_PROMPT_DISABLED=1` and `NO_COLOR=1`, stdin the JSON object `{"environment_ids": [<id>, …], "state": "approved", "comment": ""}` with the ids in `approvable` order (`Call.stdin`, as `post_review` sends its body, contract §7.6).
3. **CLEAN UP**, also when `approvable` was empty (nothing is posted: the run no longer waits for the User, so its stale item clears at once):
   - **File** `run/deploy-runs.json` (contract §1.4): when it is a valid version-1 cache (the `Cache` of `core/src/pr/deploys.rs`), the entries whose `repo` is `<owner>/<repo>` and `run_id` is `<run_id>` are removed; `version`, `polled_at` and every other run stay as they are; it is rewritten atomically (`write_atomic`) only when an entry was removed. A missing, unreadable, invalid or not-version-1 file is left byte for byte as it is, with no error. So E8's next reconcile does not reopen the item before the next `pr poll`.
   - **Rows and Event**, one transaction (`store.write("inbox approve", …)`): the item is re-read; when it is still open, `inbox_items.closed_at` is set to now and the Event `inbox-closed` is appended with the text `Inbox item <id> closed: <row1>: <row2>.`, the one-line form of the intro of this section (contract §5.2; the close of E8's reconcile, by the one helper `close_item` of `core/src/inbox/model.rs`). A deploy item has no links, so the Event has none. When E8 closed the item meanwhile, nothing is written and the command still succeeds.

```json
{"ok": true, "id": 12, "repo": "acme/pay", "run_id": 40508749055, "environments": ["production", "staging"]}
```

`environments` is the names of the approved entries, in `approvable` order. When nothing was approvable (nothing was posted), it is `[]` and `"already": true` is added:

```json
{"ok": true, "id": 12, "repo": "acme/pay", "run_id": 40508749055, "environments": [], "already": true}
```

Idempotent: yes. A failure before the WRITE writes nothing, so a rerun is safe. A failure after the POST (the cache or the close) leaves the item open; a rerun finds nothing approvable, because GitHub no longer lists the run's environments as waiting for the User and finishes the clean-up with `"already": true`. A rerun after success is `item_closed` (exit 3).

## 22. `policy`

The policies (contract §8.3, hs-d288): cpd's compiled-in default and the owner and repo files under `$CPD_HOME/policy/`, resolved for one policy key, most specific scope first per domain. Area `core/src/policy/`. The files are written by hand or by the User's dotfiles, never by a command: `policy` only reads.

### 22.1 `policy show`

```
cpd policy show [--pr <key> [--plan <id>] | --context <c> | --owner <login> [--repo <name>]]
```

Prints the resolved policy of one key: every domain with its value and its source. The pi adapter runs it with no flag at a Session's start to build the Session's policy section (contract §8.3 "Delivery to agents"); a Lead runs it with `--pr` to read a PR's repo policy, which never reaches its own section.

| Flag | Meaning |
|---|---|
| (none) | the caller's Session (`CPD_SESSION`, else `session_required`, §1.3): a Manager or a Lead resolves `Owner` of its Context's owner; a Worker (its Session's Attempt, also `CPD_ATTEMPT` in its pane) resolves `Repo` of its Job's PR |
| `--pr <key>` | that PR of the selected Plan (`--plan`, else `CPD_PLAN`; §1.3): `Repo(<Context owner>, <PR name>)`, or `Owner(<Context owner>)` for a PR whose `name` is null (contract §8.3 "Keys and resolution") |
| `--context <c>` | that Context: `Owner(<its owner>)` |
| `--owner <login>` | that owner: `Owner(<login>)`; a login of contract §2.1, else `usage` |
| `--repo <name>` | with `--owner` only (else `usage`): `Repo(<login>, <name>)`; a name of contract §2.1, else `usage` |

`--pr`, `--context` and `--owner` are exclusive (`usage`).

| Precondition | Code | Exit |
|---|---|---|
| with no flag, a Session was selected | `session_required` | 2 |
| the Session, Plan, PR or Context exists | `session_not_found`, `plan_not_found`, `pr_not_found`, `context_not_found` | 4 |
| the Context (the Session's, the PR's Plan's, or `--context`) has an owner; the message names `cpd context owner <login> --context <c>` | `owner_unset` | 3 |
| the key's owner has `policy/<owner>/owner.toml`, also when its repo file exists; `details.owner` | `owner_unconfirmed` | 1 |
| every file the key reads is valid TOML with known domains, keys, types and values and a valid file name; `policy/host.toml` sets no `pr.post` table (pro-d126, contract §8.3 "Key exposure"); `details.file`, `details.key` | `policy_invalid` | 1 |

Effects: none. One read transaction for the key's rows, then the compiled default and the key's files, read in order (`policy/host.toml` when it exists, `policy/<owner>/owner.toml`, `policy/<owner>/<repo>.toml` for a `Repo` key when it exists: the file is optional), each parsed once. No subprocess. `check_file_names`: the file `host.toml` is the machine scope; a directory whose name is a valid owner login must contain only `owner.toml` and files with valid repo names ending in `.toml`, else `policy_invalid` (§8.3).

Output: `key`, `{owner, repo}` (`repo` null for an `Owner` key); `role`, the Session's role (`manager`, `lead` or `worker`) with no flag, else null; `files`, the realpath of each file read in order (machine, owner, repo), only the files that exist (a repo without a file has no repo scope and prints the owner's policy); and `policy`, one entry per domain of contract §8.3, in its table's order, each with `source`, `audience` (the roles its `text` reaches), `keys` (its typed keys, each with its `value` and `source`; `{}` for a domain that has none) and `text`. The repo-only domains (contract §8.3) and `resources` print like the others: `audience` `[]`, `text` `""`, `source` `default` or the file's (`machine:`, `owner:` or `repo:<path>`; `resources` only may come from any of the three); the `keys` of `hooks` are `pre-start`, `pre-commit` and `pre-merge`, each `value` a list of blocks, each block a list of `{"name": <string or null>, "command": <string>}` (`[]` when the hook is not set); of `test-commands`, `build`, `run` and `list` (a string, or null when not set); of `protected`, `paths` (a list of strings, `[]` by default); of `resources`, two keys: `names`, the visible resource names, sorted (the union over host, owner and repo, contract §8.3 "The `resources` domain"; `[]` and source `default` when none is declared) and `declared`, a list sorted by name of `{"name", "scope", "slots", "source"}`, one per name (`scope` `host`, `owner` or `repo`, the declaring level whose own declaration won; `slots` its slots; `source` the path of its file; `[]` when none). The domain's `source` and the `source` of both keys are that of the narrowest file that declares a resource, `default` when none does. A domain's `source` is `default`, `machine:<path>`, `owner:<path>` or `repo:<path>`; a typed key left out of a set domain has its domain's source followed by ` (key default)` and its value is cpd's default (contract §8.3 "Precedence and replace"). The adapter keeps the domains whose `audience` holds `role` and whose `text` is not empty. The `pr` domain's keys include `pr.jira.in_progress`, `pr.jira.in_review` and `pr.jira.done` (§21.11): strings, `""` by default, which is no transition; `pr.jira.estimate_fields` (§21.12): a list of strings, `[]` by default, the Jira custom field ids the helper sets to 1 before a `done` move; `pr.human_gated_contexts` (contract §8.3): a list of strings, `["UI Review", "UI Tests"]` by default, `[]` = none; `pr.excused_labels` (contract §8.3, pro-d216, pro-d229): a list of strings, `[]` by default, the label names that excuse a failing check from waking the Lead, placed right after `pr.human_gated_contexts` in the key order; and `pr.required_checks` (contract §8.3, hs-d743): a list of strings, `[]` by default (hs-d525), the CheckRun or StatusContext names that must pass on the head before a PR is ready to merge.

```json
{"ok": true, "key": {"owner": "clayton-duarte", "repo": "cpd"}, "role": "worker",
 "files": ["/home/clayton/.cpd/policy/host.toml", "/home/clayton/.cpd/policy/clayton-duarte/owner.toml"],
 "policy": {
   "gate": {"source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml", "audience": ["lead", "worker"],
           "keys": {"merge": {"value": "local", "source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml"},
                    "pr": {"value": "local", "source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml"}},
           "text": "These repos run no GitHub Actions: …"},
   "pr": {"source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml", "audience": ["lead", "worker"],
          "keys": {"feedback.checks": {"value": "none", "source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml"},
                    "pr.human_gated_contexts": {"value": ["UI Review", "UI Tests"], "source": "default"}, "pr.excused_labels": {"value": [], "source": "default"}, "pr.jira.in_progress": {"value": "", "source": "default"}, "pr.jira.in_review": {"value": "", "source": "default"}, "pr.jira.done": {"value": "", "source": "default"}, "pr.jira.estimate_fields": {"value": [], "source": "default"}},
          "text": ""},
   "deploy": {"source": "default", "audience": ["lead"], "keys": {}, "text": ""},
   "code": {"source": "default", "audience": ["lead", "worker"], "keys": {}, "text": ""},
   "tests": {"source": "default", "audience": ["lead", "worker"], "keys": {}, "text": ""},
   "monitor": {"source": "default", "audience": ["lead", "worker"], "keys": {}, "text": ""},
   "instructions": {"source": "default", "audience": ["manager", "lead", "worker"], "keys": {}, "text": ""},
   "hooks": {"source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml", "audience": [],
             "keys": {"pre-start": {"value": [], "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"},
                      "pre-commit": {"value": [[{"name": null, "command": "pnpm run typecheck || exit 64"}]], "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"},
                      "pre-merge": {"value": [], "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"}}, "text": ""},
   "test-commands": {"source": "default", "audience": [],
                     "keys": {"build": {"value": null, "source": "default"}, "run": {"value": null, "source": "default"}, "list": {"value": null, "source": "default"}}, "text": ""},
   "protected": {"source": "default", "audience": [], "keys": {"paths": {"value": [], "source": "default"}}, "text": ""},
   "resources": {"source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml", "audience": [],
                 "keys": {"names": {"value": ["shared-port", "staging"], "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"},
                          "declared": {"value": [{"name": "shared-port", "scope": "host", "slots": 2, "source": "machine:/home/clayton/.cpd/policy/host.toml"},
                                                 {"name": "staging", "scope": "repo", "slots": 3, "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"}],
                                       "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"}}, "text": ""}},
   "deploys": {"source": "default", "audience": [], "keys": {"workflows": {"value": [], "source": "default"}}, "text": ""}}}
```

**The machine object.** Every form of the command (`--pr`, `--context`, `--owner`, `--owner --repo` and no flag) also carries a top-level `machine` object, because every form reads the machine file (`$CPD_HOME/policy/host.toml`, contract §8.3; it is also a full policy scope, read as the "Effects" above say and the `mcp` table is the only part this object reports): `{"name": "<machine>", "mcp": {"source", "servers"}}`. `name` is the config key `machine`; `source` is `default` or `machine:<realpath>` (contract §8.3 "MCP servers of the machine scope"); `servers` maps each server name to its entry with the keys as written in the file (a key left out is absent; an entry carries `headers`, a table of header name to literal value, when the file sets it and `oauth.clientName`, a string verbatim, when the file sets it). An invalid machine file is `policy_invalid`, with the same exit and `details` (`file`, `key`) as an invalid owner file and fails every form. `markdown` gains a section `## Machine <name>` listing each server's name, `url` and `exposure`, or `No MCP servers.`. Example, on machine `pro`, `machine` only:

```json
{"machine": {"name": "pro", "mcp": {
  "source": "machine:/home/clayton/.cpd/policy/host.toml",
  "servers": {"slack": {
    "url": "https://mcp.slack.com/mcp",
    "description": "Slack: read channels and threads, post messages and manage reactions.",
    "exposure": "hidden",
    "oauth": {"clientId": "1601185624273.8899143856786", "callbackUrl": "http://localhost:3118/callback",
              "scope": "channels:history chat:write reactions:write"},
    "toolExposure": {"slack_read_channel": "direct", "slack_read_thread": "direct", "slack_add_reaction": "direct",
                     "slack_get_reactions": "direct", "slack_search_emojis": "direct",
                     "slack_read_user_profile": "direct", "slack_send_message": "direct"}}}}}}
```

A Fullstory entry, which carries `headers`:

```json
{"machine": {"name": "pro", "mcp": {
  "source": "machine:/home/clayton/.cpd/policy/host.toml",
  "servers": {"fullstory": {
    "url": "https://api.fullstory.com/mcp/fullstory",
    "description": "Fullstory: session replay and analytics.",
    "exposure": "deferred",
    "headers": {"MCP-Protocol-Version": "2025-06-18"}}}}}}
```

A Figma entry, which carries `oauth.clientName`:

```json
{"machine": {"name": "pro", "mcp": {
  "source": "machine:/home/clayton/.cpd/policy/host.toml",
  "servers": {"figma": {
    "url": "https://mcp.figma.com/mcp",
    "description": "Figma: design files, components and variables.",
    "exposure": "deferred",
    "oauth": {"clientName": "Claude Code"}}}}}}
```

Idempotent: yes; it only reads. It is also the check to run after editing a policy file.

## 23. `secret`

Wraps 1Password's CLI `op` to manage secrets in Secure Notes. cpd enforces who may do what, in which vault and when. Agents never call `op` (the pi adapter refuses it). Agents run `get` and `list` only; the Lead runs all commands. Workers on a permitted Attempt run `get` under an allowance. A Lead or Manager needs an approval for most vaults. cpd holds no token and never signs in; the 1Password app's per-use prompt (where it runs) is the approval. Values never appear in argv, output, Events, logs, files or the store. Area `core/src/secret/`.

### 23.0 What `cpd secret` is

**Caller checks** (contract §8.4 "Who the caller is"):

1. `CPD_SESSION` unset or empty: `secret_session_required` (exit 3), for every command, `approve` included.
2. Session unknown: `session_not_found` (exit 4).
3. `CPD_ATTEMPT` set and non-empty, or the Session's role `worker`: a **Worker**. Otherwise `lead` or `manager`.

**Rules** (contract §8.4, checked in order; the first that fails is the result):

1. Session (above).
2. Role check:
   - `get`: Lead, Manager, Worker. Else `secret_role_denied` (exit 3).
   - `list`, `create`, `edit`, `delete`: Lead, Manager only. Else `secret_role_denied` (exit 3).
   - `allow`: Lead only. Else `secret_role_denied` (exit 3).
   - `approve`: Lead, Manager. Else `secret_role_denied` (exit 3).
3. Vault check (`cpd`, `homeserver` only):
   - A Worker is refused any other vault with `secret_vault_denied` (exit 3).
   - `allow` for another vault is refused `secret_vault_denied` (exit 3).
   - `approve` for `cpd` or `homeserver` is refused `usage` (exit 2; needs no approval).
   - A Lead or Manager in another vault goes to step 4.
4. Grant check (contract §8.4):
   - A Worker's `get` needs an **allowance** for its Attempt, vault, item and field. Missing or expired: `secret_grant_required` (exit 3), `details` `{grant: "allowance", action: "get", vault, item, field, next: "ask the Lead with cpd ask"}`.
   - A Lead or Manager in another vault needs an **approval** for its Session, action, vault, item and field. Missing or expired: `secret_grant_required` (exit 3), `details` `{grant: "approval", action, vault, item, field, next: "approve with cpd_secret_approve"}`.
5. op.

**Also**: `allow` needs the Attempt to belong to the Lead's own Plan (`attempt_not_found`, exit 4) and not to have ended (`secret_attempt_ended`, exit 3).

**Constants** (hardcoded, no config):

- **Allowed vaults**: `cpd`, `homeserver`.
- **Account**: `my.1password.com` (every `op` call carries `--account my.1password.com`).
- **Approval lifetime**: 10 minutes.

**Grants** (files under `$CPD_HOME/run/secret/`; directories 0700, files 0600, written atomically):

- **`allow/<attempt-id>.json`**: a JSON array of `{vault, item, field}`. Written by `allow`, idempotent. Valid while the Attempt exists and has not ended (the store says), never consumed.
- **`approve/<uuid>.json`**: `{session, action, vault, item, field, at}` (`item` and `field` null where the action has none). Written only by `approve` after the User chooses Allow. The first command of that Session with the same action, vault, item and field, within 10 minutes of `at`, removes the file and runs; consumed even when op then fails. An older file is ignored and removed.

**op calls** (all with `--account my.1password.com`). **Evidence** (contract §1.3):

- `get`: `op item get <item> --vault <v> --fields label=<f> --reveal`; stdout is the value, minus one trailing newline. **docs** (op 2.41.1, 2026-10-22).
- `list`: `op item list --vault <v> --format json`; keep `id` and `title` from each entry. **docs**.
- `create`: first `op item get <item> --vault <v> --format json` (success: `secret_exists`); then `op item create --vault <v> -`, JSON template on stdin: `{"title": <item>, "category": "SECURE_NOTE", "fields": [{"id": <f>, "label": <f>, "type": "CONCEALED", "value": <value>}]}`. **unverified** (no Worker has a signed-in op).
- `edit`: `op item get <item> --vault <v> --format json` (missing: `secret_not_found`); set the field's `value` or append the new field; pipe the whole JSON to `op item edit <item> --vault <v>` on stdin. **unverified**.
- `delete`: `op item delete <item> --vault <v> --archive` (archival: recoverable in 1Password). **unverified**.
- **Authentication**: cpd never signs in, holds no token, passes no `--session`. The 1Password app's per-use prompt (where it runs) is the approval; a headless machine needs a signed-in op already. **unverified**.
- **Errors**:
  - `op` not on `PATH`: `secret_op_missing` (exit 1).
  - op exits non-zero with stderr containing (case-insensitively) `not signed in`, `not currently signed in` or `session expired`: `secret_not_signed_in` (exit 3; the message tells the User to sign in with op).
  - stderr containing `isn't an item`, `isn't a vault`, `isn't a field` or `could not be found`: `secret_not_found` (exit 4).
  - any other op failure: `secret_op_failed` (exit 1), `details` `{exit, stderr}` (last 60 lines). **unverified**.

**Error codes** (new; one per row in the table below):

| Code | Exit | When |
|---|---|---|
| `secret_session_required` | 3 | `CPD_SESSION` unset or empty |
| `secret_role_denied` | 3 | caller's role not allowed for the command |
| `secret_vault_denied` | 3 | vault not allowed for the caller (Worker, or `allow` in another vault) |
| `secret_grant_required` | 3 | no allowance or approval for the action; `details.next` says what to do |
| `secret_attempt_ended` | 3 | Attempt has ended |
| `secret_exists` | 3 | `create` on an existing item |
| `secret_not_signed_in` | 3 | op not signed in or session expired |
| `secret_not_found` | 4 | item, vault or field not found |
| `secret_op_missing` | 1 | `op` not found on `PATH` |
| `secret_op_failed` | 1 | op exited non-zero (not the specific errors above) |

### 23.1 `get`

```
cpd secret get <item> --vault <v> --field <f>
```

Gets the value of one field of one Secure Note item.

| Argument | Meaning |
|---|---|
| `<item>` | item name, 1–200 chars, no control chars, no leading `-` |
| `--vault <v>` | vault name (required); `cpd` or `homeserver` for Workers, any for Lead/Manager (but needs approval) |
| `--field <f>` | field label (required); 1–200 chars, no control chars, no leading `-` |

| Precondition | Code | Exit |
|---|---|---|
| name format (item, vault, field; or `<f>` syntax like `--flag` at all) | `usage` | 2 |
| Session and role (§24.0) | `secret_session_required`, `secret_role_denied`, `session_not_found` | 3, 3, 4 |
| Vault allowed and grant if needed (§24.0) | `secret_vault_denied`, `secret_grant_required` | 3, 3 |
| `op` found and responds | `secret_op_missing`, `secret_not_signed_in` | 1, 3 |
| item and field exist | `secret_not_found`, `secret_op_failed` | 4, 1 |

Effects: none. One `op item get` call; no write to the store, no Event, no file (the value enters the caller's session or transcript by design).

Output:

```json
{"ok": true, "vault": "cpd", "item": "deployment", "field": "password", "value": "secret"}
```

Idempotent: yes; every run gets the current value.

### 23.2 `list`

```
cpd secret list --vault <v>
```

Lists items in a vault (id and title only, no values).

| Argument | Meaning |
|---|---|
| `--vault <v>` | vault name (required); `cpd` or `homeserver` for Workers, any for Lead/Manager (but needs approval) |

| Precondition | Code | Exit |
|---|---|---|
| name format (vault) | `usage` | 2 |
| Session and role (§24.0) | `secret_session_required`, `secret_role_denied`, `session_not_found` | 3, 3, 4 |
| Vault allowed and grant if needed (§24.0) | `secret_vault_denied`, `secret_grant_required` | 3, 3 |
| `op` found and responds | `secret_op_missing`, `secret_not_signed_in` | 1, 3 |
| vault exists | `secret_not_found`, `secret_op_failed` | 4, 1 |

Effects: none. One `op item list` call.

Output:

```json
{"ok": true, "vault": "cpd", "items": [
  {"id": "abc123def456", "title": "deployment"},
  {"id": "xyz789uvw012", "title": "slack-webhook"}
]}
```

Idempotent: yes.

### 23.3 `create`

```
cpd secret create <item> --vault <v> --field <f>
```

Creates a Secure Note item with one concealed field. Value on stdin only. An empty value or a terminal stdin is `usage` (exit 2).

| Argument | Meaning |
|---|---|
| `<item>` | item name (required); 1–200 chars, no control chars, no leading `-` |
| `--vault <v>` | vault (required); `cpd` or `homeserver` for Workers, any for Lead/Manager (but needs approval) |
| `--field <f>` | field label (required); 1–200 chars, no control chars, no leading `-` |

| Precondition | Code | Exit |
|---|---|---|
| name format (item, vault, field) | `usage` | 2 |
| stdin is not a terminal and not empty (minus one trailing newline) | `usage` | 2 |
| Session and role (§24.0) | `secret_session_required`, `secret_role_denied`, `session_not_found` | 3, 3, 4 |
| Lead or Manager only (not Worker) | `secret_role_denied` | 3 |
| Vault allowed and grant if needed (§24.0) | `secret_vault_denied`, `secret_grant_required` | 3, 3 |
| `op` found and responds | `secret_op_missing`, `secret_not_signed_in` | 1, 3 |
| item does not exist | `secret_exists`, `secret_op_failed` | 3, 1 |
| op succeeds | `secret_op_failed` | 1 |

Effects: one `op item get` to check (missing is OK; success means exists), then one `op item create`. No store write, no Event.

Output:

```json
{"ok": true, "vault": "cpd", "item": "deployment", "field": "password", "created": true}
```

Idempotent: no; `create` on an existing item is `secret_exists` (exit 3).

### 23.4 `edit`

```
cpd secret edit <item> --vault <v> --field <f>
```

Sets the value of one field of an item. If the field doesn't exist, it is added. Value on stdin only (same preconditions as §24.3).

| Argument | Meaning |
|---|---|
| `<item>` | item name (required); 1–200 chars, no control chars, no leading `-` |
| `--vault <v>` | vault (required) |
| `--field <f>` | field label (required); 1–200 chars, no control chars, no leading `-` |

| Precondition | Code | Exit |
|---|---|---|
| name format (item, vault, field) | `usage` | 2 |
| stdin is not a terminal and not empty (minus one trailing newline) | `usage` | 2 |
| Session and role (§24.0) | `secret_session_required`, `secret_role_denied`, `session_not_found` | 3, 3, 4 |
| Lead or Manager only (not Worker) | `secret_role_denied` | 3 |
| Vault allowed and grant if needed (§24.0) | `secret_vault_denied`, `secret_grant_required` | 3, 3 |
| `op` found and responds | `secret_op_missing`, `secret_not_signed_in` | 1, 3 |
| item exists | `secret_not_found`, `secret_op_failed` | 4, 1 |
| op succeeds | `secret_op_failed` | 1 |

Effects: one `op item get` (missing: `secret_not_found`), then one `op item edit`. No store write, no Event.

Output:

```json
{"ok": true, "vault": "cpd", "item": "deployment", "field": "password", "edited": true}
```

Idempotent: no (running it again sets the value again, which is OK semantically, but is not reported as `already`).

### 23.5 `delete`

```
cpd secret delete <item> --vault <v>
```

Archives an item (recoverable in 1Password).

| Argument | Meaning |
|---|---|
| `<item>` | item name (required); 1–200 chars, no control chars, no leading `-` |
| `--vault <v>` | vault (required) |

| Precondition | Code | Exit |
|---|---|---|
| name format (item, vault) | `usage` | 2 |
| Session and role (§24.0) | `secret_session_required`, `secret_role_denied`, `session_not_found` | 3, 3, 4 |
| Lead or Manager only (not Worker) | `secret_role_denied` | 3 |
| Vault allowed and grant if needed (§24.0) | `secret_vault_denied`, `secret_grant_required` | 3, 3 |
| `op` found and responds | `secret_op_missing`, `secret_not_signed_in` | 1, 3 |
| item exists | `secret_not_found`, `secret_op_failed` | 4, 1 |
| op succeeds | `secret_op_failed` | 1 |

Effects: one `op item delete --archive` call. No store write, no Event.

Output:

```json
{"ok": true, "vault": "cpd", "item": "deployment", "archived": true}
```

Idempotent: no; a second run on an archived item is `secret_not_found` (exit 4).

### 23.6 `allow`

```
cpd secret allow <item> --attempt <id> --vault <v> --field <f>
```

Grants a Worker's Attempt read access to one field of one item (contract §8.4).

| Argument | Meaning |
|---|---|
| `<item>` | item name (required); 1–200 chars, no control chars, no leading `-` |
| `--attempt <id>` | Attempt id (required); the Attempt must belong to the Lead's own Plan |
| `--vault <v>` | vault (required); must be `cpd` or `homeserver` (no other vault) |
| `--field <f>` | field label (required); 1–200 chars, no control chars, no leading `-` |

| Precondition | Code | Exit |
|---|---|---|
| name format (item, field, vault, Attempt id) | `usage` | 2 |
| Session and role (§24.0) | `secret_session_required`, `secret_role_denied`, `session_not_found` | 3, 3, 4 |
| Lead only (not Manager or Worker) | `secret_role_denied` | 3 |
| Vault is `cpd` or `homeserver` (not another) | `secret_vault_denied` | 3 |
| Attempt exists and belongs to the Lead's own Plan | `attempt_not_found` | 4 |
| Attempt has not ended | `secret_attempt_ended` | 3 |

Effects: one transaction; append or update `$CPD_HOME/run/secret/allow/<attempt>.json` atomically. It is a JSON array of `{vault, item, field}` (new entries appended, duplicates ignored). No Event, no store write.

Output:

```json
{"ok": true, "attempt": "job-name-a1", "vault": "cpd", "item": "deployment", "field": "password", "already": false}
```

Idempotent: yes; an entry already listed returns `"already": true`.

### 23.7 `approve` (internal)

```
cpd secret approve --action <get|list|create|edit|delete> --vault <v> [--item <i>] [--field <f>]
```

Records the User's approval for a Lead or Manager to use another vault. Called by the pi adapter after the User chooses Allow in the `cpd_secret_approve` tool. Internal; not for manual use.

| Argument | Meaning |
|---|---|
| `--action <get|list|create|edit|delete>` | the action (required) |
| `--vault <v>` | vault (required); must not be `cpd` or `homeserver` (no approval needed for those) |
| `--item <i>` | item name (required for get, create, edit, delete; forbidden for list); 1–200 chars, no control chars, no leading `-` |
| `--field <f>` | field label (required for get, create, edit; forbidden for list and delete); 1–200 chars, no control chars, no leading `-` |

| Precondition | Code | Exit |
|---|---|---|
| name format (action, vault, item, field) | `usage` | 2 |
| `--item` present iff action is not `list` | `usage` | 2 |
| `--field` present iff action is `get`, `create` or `edit` | `usage` | 2 |
| `CPD_SESSION` is set and the Session exists | `secret_session_required`, `session_not_found` | 3, 4 |
| caller is Lead or Manager (`secret_role_denied`) | `secret_role_denied` | 3 |
| vault is not `cpd` or `homeserver` | `usage` | 2 |

Effects: one transaction; write `$CPD_HOME/run/secret/approve/<uuid>.json` atomically, where `<uuid>` is a new v4 UUID. The file holds `{session, action, vault, item, field, at}` (`item` and `field` null as needed). The file is consumed when the first matching command of that Session runs within 10 minutes of `at` (contract §8.4).

Output:

```json
{"ok": true, "action": "get", "vault": "vault-name", "item": "secret-item", "field": "secret-field", "expires_at": "2026-10-22T21:10:00.000Z"}
```

Idempotent: no; every call writes a new file.


## 23. `incident`

Incident mode (hs-d643, user decision hs-d643; supersedes hs-d634): Plan-scoped hold on scheduling. One Manager-only command to start and end, one any caller may read. A Manager starts with one or more Plans (no machine-wide mode).

### 23.1 `incident start`

```
cpd incident start --plan <id> [--plan <id>...] [--reason <text>] [--until-pr <plan>/<key>]
```

| Flag | Meaning |
|---|---|
| `--plan <id>` | required, repeatable at least once; names an existing Plan in this Context |
| `--reason <text>` | optional; 1 or more characters, explanation of the incident; required only on a fresh start |
| `--until-pr <plan>/<key>` | optional; PR key in format `<plan>/<key>` of one of the named Plans; incident ends when this PR merges |

| Precondition | Code | Exit |
|---|---|---|
| `CPD_SESSION` is set, valid and a Manager Session (role `manager`, not archived) | `manager_only` | 3 |
| at least one `--plan` is present | `usage` | 2 |
| when incident is off, `--reason` is present and at least 1 character | `usage` | 2 |
| no `--plan` argument, or every one names an existing Plan in the store | `plan_not_found` | 4 |
| `--until-pr` is absent, or its value is a PR of one of the named Plans and exists | `pr_not_found` | 4 |
| `--until-pr` is absent, or its value's Plan is one of the named Plans | `until_pr_not_incident` | 3 |
| `--until-pr` is valid JSON and `<plan>/<key>` format, or not present | `usage` | 2 |

**Effects:** one transaction. **Fresh start** (when incident is off): insert one row in `incident` table (`id = 1`, `started_at` now, `started_by` the Manager's Session id, `reason` the text, `started_txn` the transaction id, `lead` the `lead_session` of the first `--plan` argument's Plan, `until_plan` and `until_pr` as specified or null); insert one row per named Plan in `incident_plans` table (plan, started_txn); for every running Attempt in the same transaction, if `stop_request` is null set it to `'cancelled'`; append `incident-started` Event with `data` including `plans` array; return `changed: true`. **Update on a live incident** (when incident is on): replace `incident_plans` with exactly the named Plans; when the set changed, recompute `lead` from the new first Plan's `lead_session`; when the set or `until_pr` changed, update `until_plan`, `until_pr` and `lead` and append `incident-plans` Event with the new `plans` array; when all are unchanged, append no Event; return `changed: true` when the Event is appended, `changed: false` when unchanged; run no stop-request sweep. The `started_at`, `started_by` and stored `reason` stay unchanged on an update. When `--until-pr` is absent and the stored `until_plan` is not in the new set, clear both `until_plan` and `until_pr`. The incident and incident_plans tables are registered in `history_tables` (no_delete 0).

**Output examples:**

Fresh start:
```json
{"ok": true, "changed": true, "incident": {"started_at": "2026-10-14T21:10:00.000Z", "by": "3c9a…", "reason": "database migration in progress", "plans": ["plan-a", "plan-b"], "until_pr": null}}
```

Update with changed set:
```json
{"ok": true, "changed": true, "incident": {"started_at": "2026-10-14T21:10:00.000Z", "by": "3c9a…", "reason": "database migration in progress", "plans": ["plan-c"], "until_pr": null}}
```

Update with unchanged set and until_pr:
```json
{"ok": true, "changed": false, "incident": {"started_at": "2026-10-14T21:10:00.000Z", "by": "3c9a…", "reason": "database migration in progress", "plans": ["plan-a", "plan-b"], "until_pr": null}}
```

Fresh start with auto-end:
```json
{"ok": true, "changed": true, "incident": {"started_at": "2026-10-14T21:10:00.000Z", "by": "3c9a…", "reason": "fixing PR ci", "plans": ["plan-a"], "until_pr": "plan-a/pr-1"}}
```

**Idempotent:** yes; when the set and `until_pr` are already as named, returns `changed: false` with the existing incident.

### 23.2 `incident end`

```
cpd incident end
```

| Precondition | Code | Exit |
|---|---|---|
| `CPD_SESSION` is set, valid and a Manager Session (role `manager`, not archived) | `manager_only` | 3 |

Effects: one transaction; delete the `incident` row and every row in `incident_plans`; append `incident-ended` Event with `data` including `cause: "manual"` and `by` the Manager Session id.

Output:

```json
{"ok": true, "changed": true, "incident": null}
```

Idempotent: yes; incident mode already off returns `"changed": false` with `incident: null`.

### 23.3 `incident status`

```
cpd incident status
```

No flags, no preconditions. Any caller may read.

Effects: none (one read transaction).

Output when off:

```json
{"ok": true, "on": false, "incident": null}
```

Output when on:

```json
{"ok": true, "on": true, "incident": {"started_at": "2026-10-14T21:10:00.000Z", "by": "3c9a…", "reason": "database migration in progress", "plans": ["plan-a", "plan-b"], "until_pr": null}}
```

Output when on with auto-end:

```json
{"ok": true, "on": true, "incident": {"started_at": "2026-10-14T21:10:00.000Z", "by": "3c9a…", "reason": "fixing PR ci", "plans": ["plan-a"], "until_pr": "plan-a/pr-1"}}
```

Idempotent: yes; read-only.

## 23. `gate`

The Gate's leak check (hs-d519, docs/research/hygiene.md "The Gate leak check", Plan tests-mock pro-d35). Area `core/src/gate/` and `cli/`.

### 23.1 `gate finish`

```
cpd gate finish --run <id> --status <n>
```

| Flag | Meaning |
|---|---|
| `--run <id>` | the run id (`GATE_RUN`, 6 characters). Required |
| `--status <n>` | the exit status of the run's command. Required |

Run by `scripts/gate-run.sh` after its command, with `TMPDIR` the run's directory. It lists what the run left: the processes that have `GATE_RUN=<id>` or `TMPDIR=<run dir>` in their environment (`Spawn::with_env`, both keys, counted once; the match on `TMPDIR` is exact equality, so the machine-wide sccache server, whose `TMPDIR` is the parent directory and another run's directory are not leftovers; pro-d155), the tmux sockets named with the id (`/tmp/tmux-<uid>` and `$TMUX_TMPDIR/tmux-<uid>`), the paths under `$TMPDIR` and the lines of `$TMPDIR/tripwire.log`. A clean run prints nothing, removes the empty directory and exits with `--status`. Otherwise it prints `gate-run: FAIL: …` on stderr, `SIGKILL`s only the listed pids, removes only the listed sockets and the run's directory and exits with `--status` when that is not 0, else 64. Another run's id never matches. Not covered (pro-d155): a process whose cwd is inside the run's directory (the Spawn port has no cwd reader), a process whose `TMPDIR` is a directory inside the run's directory but not the run's directory itself and a process that cleared both `GATE_RUN` and `TMPDIR`. Like `slot`, it prints no JSON of its own; a usage error is the envelope on stderr. Idempotent: yes.

## 23. `prune`

```
cpd prune [--apply] [--drop <lead-branch>]... <repo>...
```

The successor of `scripts/prune-worktrees.sh` (hs-d138, hs-d143, hs-d144): the clean-up of leftover worktrees whose work reached `origin/main`, which the Manager runs by hand after a Plan. The script shrinks to `exec cpd prune "$@"`. A User's command at a shell: no callers, not cockpit, not internal.

Flags and arguments:
- `--apply`: do the removals. Without it nothing changes (apart from `git fetch origin`) and the command prints what it would do.
- `--drop <lead-branch>`: repeatable. A worktree whose Lead Branch is one of the values is removed whether or not it is merged (rule `remove dropped`), unless it is protected.
- `<repo>...`: one or more repos, at least one. A missing repo argument or an unknown flag is a usage error, exit 2.

**Protected set.** Before anything else the command reads the store (no `cpd` child, no `jq`): for every Plan that is not done, the `lead_branch` of each PR whose status is not merged and `cpd/<plan>--<attempt id>` for each Attempt of a Job of such a PR. If the store cannot be read the command exits 2 before touching anything. `git` and `gh` (authenticated) go through the `Runner`.

**Rules.** Per worktree (the main one is skipped), the first rule that holds wins:

```
keep   active       the branch is protected (also with --drop)
keep   dirty        `git status --porcelain` is not empty (untracked files included)
remove dropped      the Lead Branch L is one of the --drop values (merged or not)
remove              HEAD is an ancestor of origin/main
remove              a merged PR P has a head branch equal to L, starting with `L/`, or,
                    for an Attempt branch of Plan `<plan>`, starting with `<plan>/` (A6);
                    HEAD is an ancestor of P's head commit and P reached main (its base
                    is main, or the base is the head of a merged PR that reached main,
                    at most 5 levels)
keep   not merged   otherwise
```

L is the branch itself, except an Attempt branch `cpd/<plan>--<attempt>` (it starts with `cpd/` and its remaining text has no `/`), whose L is `cpd/<plan>` (the text before the first `--`), so a key containing `--` is never cut. A detached HEAD has no L. Removal deletes the worktree and its local branch, never a remote branch.

Effects per repo: `git fetch origin`, `gh pr list --state merged --limit 500` of the repo's `origin` slug, `git worktree list --porcelain`; with `--apply`, for each removal `git worktree remove --force`, `git branch -D` (only when no other worktree has the branch) and finally `git worktree prune`.

Output: one line per worktree, `keep <path> (<branch|detached>): <reason>`, `remove <path> (<branch|detached>)` or `remove <path> (<branch>): dropped`, `failed <path>: <error>`, then `<repo>: <n> remove, <m> keep`.

Exit codes: 0 unless a command failed; a failure is printed and the run goes on, exit 1 at the end. 2: usage, or the protected set could not be read.

Tests: in-process on `Fakes` (`FakeRunner` git and gh routes, a temp store), the names of the old `cli/tests/tooling/prune_script.rs` kept: `a_failed_command_is_reported_and_exits_non_zero`, `keeps_what_cpd_still_runs`, `matches_a_legacy_lead_branch_renamed_locally`, `drop_removes_clean_unmerged_worktrees_of_the_named_lead_branch` and `a_failing_cpd_exits_2_and_removes_nothing` (now: an unreadable store); new test names: `prune_matches_a_parallel_attempt_to_a_merged_plan_slash_key_pr` and `prune_keeps_a_key_with_a_double_dash_whole`. The command is listed in `cli/tests/tooling/interface.rs`.
## 23. `tag` and `version`

The release plumbing of docs/versioning.md, in Rust over `Ports` (`Runner` for git, std `fs` for files). `scripts/tag-version.sh` and `scripts/sync-version.sh` are thin: each is `exec cpd tag` / `exec cpd version sync "$@"` and keeps only what must exist before cpd does. Their old exec tests are replaced by in-process tests on `Fakes` (a `FakeRunner` git route; a temp dir for the files) and `scripts/check-portable.sh` still guards the two scripts.

### 23.1 `cpd tag`

`cpd tag` takes no arguments. It tags the commit of `origin/main` that last changed the workspace version of `Cargo.toml` with the annotated tag `v<version>` and pushes it. It is idempotent: a rerun does nothing or finishes the push.

Steps, each a git call through the `Runner`: `fetch --quiet origin main`; `show origin/main:Cargo.toml` (the first `version = "` under `[workspace.package]` is `<version>`); `ls-remote --tags origin refs/tags/v<version>` (output means the tag exists: exit 0, nothing more); `log -1 --first-parent -G'^version = ' --format=%H origin/main -- Cargo.toml` (the commit `<sha>`, whose `Cargo.toml` must carry `<version>`); then, when the local tag is absent, `tag -a v<version> <sha> -m v<version>` (a local tag that exists must point at `<sha>`); then `push --quiet origin refs/tags/v<version>`. A failed push is accepted only when `ls-remote --tags origin refs/tags/v<version>^{}` already answers `<sha>`.

Output (stderr, one line each; the stream the script used):

| Case | Line | Exit |
|---|---|---|
| tagged | `tag-version: tagged v<version> on <sha>` | 0 |
| the remote has the tag | `tag-version: v<version> exists; nothing to do` | 0 |
| fetch fails | `tag-version: cannot fetch origin/main` | 1 |
| no `Cargo.toml` on origin/main | `tag-version: cannot read Cargo.toml on origin/main` | 1 |
| no workspace version | `tag-version: no workspace version in Cargo.toml on origin/main` | 1 |
| `ls-remote` fails | `tag-version: cannot list the tags of origin` | 1 |
| `log` fails | `tag-version: cannot find the commit that set v<version>` | 1 |
| `log` is empty | `tag-version: no commit on origin/main changed the version` | 1 |
| the commit has another version | `tag-version: commit <sha> has version '<at>', not <version>` | 1 |
| the local tag points elsewhere | `tag-version: local tag v<version> points at <have>, not <sha>` | 1 |
| `tag -a` fails | `tag-version: cannot create v<version>` | 1 |
| the push fails and the remote lacks `<sha>` | `tag-version: push of v<version> failed` | 1 |

Every failure is a `tag-version:` line; there is no JSON. `scripts/install-cpd.sh` runs it last and only warns on a failure (docs/implementing.md "Releases"). Callers: Lead (§2).

### 23.2 `cpd version sync [--print]`

cpd's version is `0.<PROTOCOL>.<migrations>`: `PROTOCOL` is the `pub const PROTOCOL: u32 = <n>;` of `core/src/events/kind.rs`, `<migrations>` the count of `.sql` files in `core/src/store/migrations/`.

- `cpd version sync --print` prints the version (for example `0.13.45`) on stdout and writes nothing. Exit 0.
- `cpd version sync` rewrites the first `version = "` under `[workspace.package]` of `Cargo.toml` and the `version` of the four workspace crates (`cpd`, `cpd-core`, `cpd-engine`, `cpd-tui`; never a package with a `source`) in `Cargo.lock`, then prints `sync-version: <version>`. Rerunning changes nothing.
- Any other argument is a usage error: `usage: cpd version sync [--print]`, exit 2.
- Failures print a `sync-version:` line on stderr and exit 1, writing nothing: `sync-version: no 'pub const PROTOCOL: u32 = <n>;' line in <kind.rs>.`, `sync-version: no .sql file in <dir>.`, `sync-version: no [workspace.package] in <Cargo.toml>.`, `sync-version: no version line under [workspace.package] in <Cargo.toml>.`

The test that fails when `Cargo.toml` differs from the derived version (docs/implementing.md "The version string") stays; `cpd version sync` is what fixes it.

## 23. `test`

The logic behind `scripts/test-changed` and `scripts/tests.sh`, which become thin (`exec cpd test …`). The selection and the name classification are pure over `git diff` and `cargo metadata` output, read through the `Runner`; the commands start `cargo` through `scripts/cargo.sh` (the bootstrap that builds cpd). Callers: Manager, Lead, Worker; not cockpit, not internal. The command tree is pinned by `cli/tests/tooling/interface.rs` and [docs/interface.md](interface.md) is regenerated.

### 23.1 `test changed`

```
cpd test changed [--dry-run] [<base>]
```

Runs the Rust tests the changed files can reach: exactly the selection of docs/implementing.md "The scoped loop" (changed files from the merge base of `<base>`, default `origin/main`, to the working tree and untracked files; crate and reverse dependencies as `-p`; `cli/tests/<area>/<module>.rs` as `--test <area> -E 'test(/^<module>::/)'`; root-level inputs as `--workspace`; docs, `pi/`, `web/` and other scripts select nothing). `--dry-run` prints the one command on stdout and runs nothing, exit 0. Nothing selected: `test-changed: nothing to run` on stderr, exit 0. Otherwise the exit status is nextest's. `scripts/test-changed [--dry-run] [<base>]` stays as `exec cpd test changed "$@"`.

### 23.2 `test run`

```
cpd test run <name>...
```

Runs the named tests and exits non-zero when any failed or did not run. A name is a pi name, `pi/test/<file>.test.ts::<title>` (the file's path, `::`, the exact `node:test` title; run with `node --test --test-name-pattern=<title> <file>` and refused when the title is in no `test(` or `it(` call of the file), a web name, `web/<file>.test.ts(x)::<title>` (the file's path, `::`, the exact vitest title; run through the web package's vitest, `pnpm --filter cpd-web-client exec vitest run --testNamePattern <title> <file>`), or a Rust name as nextest filters it (run with `nextest run --workspace --no-fail-fast`). All run filtered, so none takes a test slot. Runs from the repo root, whatever the cwd. No name is a `usage` error.

### 23.3 `test list` and `test build`

```
cpd test list <name>
cpd test build
```

`list` exits 0 only when that one test exists: a pi name when its title is in a `test(` or `it(` call of its file (nothing is started), a web name when the web package's vitest answers it from collection mode (`pnpm --filter cpd-web-client exec vitest list --reporter=json <file>`: an existing test lists as found whether it passes or fails, and only an absent title or a file that does not collect is refused), a Rust name when `nextest list --workspace --message-format oneline <name>` prints a test. `build` runs `nextest run --workspace --no-run`, then `pnpm run typecheck` and, when `web/package.json` exists, typechecks the web client (it skips the web client when the file does not exist), and stops at the first failure; its status is that of the step that failed. Both run from the repo root, whatever the cwd.

### 23.4 The `scripts/tests.sh` contract

`.config/cpd.toml` `[tests]` keeps calling `scripts/tests.sh build`, `scripts/tests.sh list <test>` and `scripts/tests.sh run <tests>` (contract §7.7); that argument contract does not change. The script is only the bootstrap: it builds cpd with `scripts/cargo.sh build -p cpd` (a no-op when cpd is current, so a stale cpd never answers), takes the newest `target/*/cpd` and runs `exec cpd test <command> "$@"`; `scripts/test-changed` does the same for `cpd test changed`. A process's output reaches the caller when that process ends, not line by line: the runner returns it whole.

## 24. `mcp`

Attaches an MCP server to a Session (contract §6.11; pro-d2, pro-d8). The servers a machine may use are listed in the `[mcp]` table of its machine file, `$CPD_HOME/policy/host.toml` (contract §8.3 "MCP servers of the machine scope"; `cpd policy show`, §22.1). Dotfiles render that file per machine: its `[model]` table first, then the machine's `cpd.policy` text, which holds the `[mcp.servers.*]`. Area `core/` and `cli/`.

### 24.1 `mcp attach`

```
cpd mcp attach <name>
```

| Argument | Meaning |
|---|---|
| `<name>` | a server of this machine's policy; it matches the server-name rule of contract §8.3 |

The caller's Session is selected as §1.3 says (`--session <id>`, else `CPD_SESSION`). Any role may run it.

| Precondition | Code | Exit |
|---|---|---|
| `<name>` is given and matches the server-name rule (contract §8.3) | `usage` | 2 |
| a Session is selected | `session_required` | 2 |
| the Session exists | `session_not_found` | 4 |
| the machine file, `$CPD_HOME/policy/host.toml`, is valid TOML and its `[mcp]` table has known keys and types (contract §8.3 "MCP servers of the machine scope"; the file's other domains are the policy scope's and an invalid one is `policy_invalid` too); `details.file`, `details.key`; nothing is appended | `policy_invalid` | 1 |
| the machine file's `[mcp]` lists `<name>`; `details` `{name, machine, available}` (`machine` is the config key `machine`), `available` the sorted server names (an empty array when none); the message names the machine and lists what is available, or says none | `mcp_unknown` | 4 |

**Effects**, one transaction:
- **Event** `mcp-attached` (contract §5.2) addressed to the caller's Session (`session` set, with its `context` and `plan` when it has one), `data` `{name, server}`: `server` is the entry exactly as the machine policy gives it (pi's key names; a key left out is absent). `text`: `Server <name> attached to Session <id>.`
- Nothing else: no row, no file, no tmux call.

**The adapter** of that Session registers the server with pi (§15.3 `mcp-attached`, contract §5.3). The attach is for that Session only: never for another Session and never for a later one, and never done by default (pro-d2). cpd saves nothing of its own: the `mcp-attached` Events are the record (pro-d226). The adapter registers the Session's attached servers again at every start (§15.2 step 5a), so a reload or a restart keeps them. There is still no list and no detach command (pro-d8): `cpd policy show` lists what the machine allows. An attachment lasts as long as the Session (pro-d226): an archived Session runs no adapter, so it registers nothing, and when it runs again (`session unarchive`, then `session open` or `session resume`) its adapter replays; `session retier` changes the model, not the registrations; a fork has none, because its Events name the source Session. The server's `direct` tools appear in that Session as `mcp__<name>__<tool>` on a later model call of the same turn, not the call right after the attach (the adapter registers the server when it delivers the Event, then pi connects), and `tool_search` never finds the `direct` tools of a `hidden` server.

```json
{"ok": true, "attached": {"name": "slack", "session": "596d007f-…", "event": 48301}}
```

Idempotent in effect: running it again appends another Event and the adapter registers the same name again, which pi accepts as a replacement.

#### Signing in

A server that uses OAuth needs the User to sign in once on the machine (pro-d10): in that Session's pi with `/mcp login <name>`, or from a shell with `pi mcp login <name>`. The shell form works while the user-level `~/.pi/agent/mcp.json` has an entry with the same name and URL, because pi keys the credential by server name and URL. Tokens live in pi's own `mcp-auth.json`; cpd never reads or writes one (hs-d107, pro-d4). A running Session uses new credentials on its next turn (pi's rule) and an attached server that needs a sign-in shows pi's own "requires sign-in" report.

## 29. `janitor`

```
cpd janitor
```

Removes the worktrees and branches of merged and cancelled PRs (pro-d70, pro-d71) and of the ended Attempts of merged Jobs on live PRs (contract §7.2). One pass, then exits. It runs once every `watch.janitor_minutes` minutes (config key, default 60) via engine step E16 (§3.7) and may be run by hand. The User calls it, not Leads or Workers (§2).

| Flag | Meaning |
|---|---|
| (none) | `cpd janitor` takes no flags |

| Precondition or failure | Code | Exit |
|---|---|---|
| the store can be read (as `prune`, §27) | see §27 for store errors | 2 or 1 |

**Effects** (pro-d70, pro-d71, pro-d72; contract §7.1, §7.2):

The pass scans every PR of every Plan (archived included) whose status is `merged` or `cancelled` that still has on disk an Attempt worktree, its Lead worktree, or its aside leftover (a directory `<path>.removing` from a failed removal, §4 below). It also scans the PRs whose status is `dispatch` or `open` for the ended Attempts that contract §7.2 'Ended Attempts of merged Jobs on a live PR' sweeps. It does not start a pass when another is running: the lock `locks/janitor.lock` (exclusive, non-blocking) is held by another; it prints `{"ok":true,"skipped":"running"}`, exits 0 and touches nothing.

For each such PR (**pending PR**), in this order:

1. **Idle check**: every Attempt of the PR's Jobs is `ended` and its driver lock is free (probed without creating the file, as `merged_prs_to_clean` does today, contract §7.2). For a swept Attempt of a `dispatch` or `open` PR (contract §7.2 'Ended Attempts of merged Jobs on a live PR') the check is that Attempt's (`ended`, lock free), not the PR's. If not, the outcome is `pending`, nothing is touched, no Event for this PR in this pass.
2. **Kill Attempt windows**: `tmux kill-pane` on every pane of this machine's Attempt windows of the PR's Jobs (as today, `core/src/pr/poll.rs` `clean_up`).
3. **Discard parallel Attempt worktrees**: per Attempt of the PR's Jobs whose worktree exists on disk (a linear Attempt runs in the Lead worktree): removed as contract §7.2 'Removal' says, forced: rename aside, prune, branch -D, delete the aside; an Attempt's leftover `<path>.removing` counts as on disk.
4. **Rule S** on the Lead worktree (contract §7.4: fetch, merge base): its local branch stays up to date for the dirty and unpushed check (§4 below).
5. **Remove the Lead worktree** (§4 below).

**§4. Removal of a Lead worktree** (pro-d72; refusals keep it whole or mark it for retry):

When the Lead worktree is dirty (output of `git --no-optional-locks status --porcelain` not empty, untracked files included) or unpushed (`git rev-list --no-merges HEAD --not --remotes=origin` not empty, **unless** `HEAD` is the pull request's recorded head commit or an ancestor of it, as the poll checks today with `headRefOid`; a squash-merged PR whose branch GitHub deleted is cleaned up; contract §7.2 "The recorded head and the unpushed rule"), the outcome is `kept` with the reason and nothing is touched. Otherwise, in this order, each step idempotent (a stop at any point leaves the worktree whole or marked for retry):

(a) **Rename**: `<path>` to `<path>.removing` (atomic rename, a sibling in the same directory; PR keys have no dot, so no collision; a failing rename is `kept` with the error as reason).
(b) **Prune**: `git worktree prune` (the registered directory is gone, so git drops it).
(c) **Delete branch**: `git branch -D <lead branch>` when it exists.
(d) **Delete directory**: `std::fs::remove_dir_all(<path>.removing)`.

A stop (kill, crash, failing call) at (a) leaves the worktree whole and untouched. A stop at (b), (c) or (d) leaves the worktree gone (`<path>` absent) with `<path>.removing` as the marker (the directory is present from the rename, empty of tracked files). A pass that finds `<path>` absent and `<path>.removing` present retries (b), (c), (d) again; every step is idempotent. A failure in (b), (c) or (d) is outcome `failed`: the path is gone and later passes retry (a contract violation: the marker stays; the worktree can never be recovered). Renamed but uncleaned paths show in the log (§5).

**§5. Output and Log**:

One JSON value: `{"ok":true,"outcomes":[...]}` or `{"ok":false,"error":{...}}` (store unreadable only). `outcomes` is one entry per pending PR path scanned (paths of Attempt worktrees, Lead worktrees and aside leftovers), in the order they were found. Each entry:

```json
{"plan": "pay-4521", "pr": "api", "status": "cancelled", "path": "/home/clayton/projects/pay.pay-4521.api", "outcome": "removed", "reason": null}
```

or, when not removed:

```json
{"plan": "pay-4521", "pr": "api", "status": "merged", "path": "/home/clayton/projects/pay.api", "outcome": "kept", "reason": "unpushed"}
```

or:

```json
{"plan": "pay-4521", "pr": "api", "status": "merged", "path": "/home/clayton/projects/pay.api.removing", "outcome": "failed", "reason": "permission denied"}
```

`status` is `cancelled`, `merged`, `dispatch` or `open` (the last two for an Attempt swept under contract §7.2 'Ended Attempts of merged Jobs on a live PR'), read from the PR's status in the store. `outcome` is one of: `removed` (the path was deleted whole), `kept` (the path remains untouched, worktree dirty or unpushed), `pending` (the PR is not idle yet), or `failed` (the path was renamed but not fully deleted; the next pass retries). `reason` is present only for `kept` and `failed` (the dirty/unpushed rule, git's stderr tail, or a file error message); it is a short string suitable for an Event.

The pass writes a log to `run/janitor.log`, appended (new lines only, one per pass, format `<timestamp> <pass id> <status>` where `<status>` is `ok`, `locked`, or an error; the file is not parsed, so its format may change). The engine's watch.log reads events from that file (§12 "The watch loop").

**§6. Events**:

One `error` Event `cleanup_failed` per `kept` or `failed` path, only from the first pass that sees it with no `cleanup_failed` Event of that PR yet (later passes retry silently, as `fresh` does today for a cancelled PR; a merged PR now works the same way). Never for `pending` or `removed` paths. **Text** (pro-d72):

- `kept`: `The clean-up of the <status> PR <pr> of Plan <plan> kept <path> untouched: <reason>`
- `failed` (after rename at step (a) in §4): `The clean-up of the <status> PR <pr> of Plan <plan> removed <path>, but not everything: <reason>. The next janitor pass retries.`
- `failed` (Attempt worktree discard at step 3 in Effects): `The clean-up of the <status> PR <pr> of Plan <plan> did not remove <path>: <reason>. The next janitor pass retries.`

Each Event also has `details`: `{plan, pr, status, outcome, path, reason}`.

**§7. Exit codes**:

- `0` on success (outcomes list may be empty or contain `pending`, `kept`, `failed` and `removed`).
- `2` when the store cannot be read (as `prune`, §27: `store_too_new`, `migration_failed`, `migration_changed`, `migration_order`).
- `1` for other store errors (as `prune`, §27: `store_failed`, `store_tampered`).
- `1` when a git or tmux call fails unexpectedly (details `{argv, exit, stderr}`); the pass continues for the next PR.

**Tests**: `janitor_pass_removes_cancelled_and_merged_pr_worktrees_and_branches`, `janitor_pass_finishes_what_a_killed_pass_left`, `janitor_command_is_in_the_help_tree`.

Idempotent: yes; rerunning with nothing to clean prints `ok` with an empty outcomes list and writes nothing; rerunning after a `kept` or `failed` path retries (may succeed on rerun if the worktree was cleaned by hand; `kept` paths remain `kept`, as they are not idle yet or always dirty).

## 30. `resource`

Resources with slots, declared at host, owner or repo level (contract §8.3 "The `resources` domain", §1.4 "The resource locks and the holder record", §3.4 "Resources of a Job" and "Tenancy"; Plan `repo-lock`). Both commands derive the repo from the **current directory**: the git worktree that contains it, its `origin` keyed as `pr add` does (`repo_unkeyed`, exit 1, when it cannot), then that repo's policy (whose visible resources are the union of contract §8.3) (the owner and policy refusals of any resolve: `owner_unconfirmed`, `policy_invalid`). A directory outside a git worktree is `usage` (exit 2). The files are in contract §1.4; no command releases another tenant's slot.

### 30.1 `resource list`

```
cpd resource list
```

Effects: none. Output: `repo` (`<owner>/<name>`) and `resources`, one entry per visible resource sorted by name (`[]` when none is declared): `name`; `scope` (`host`, `owner` or `repo`, the declaring level, contract §8.3); `slots`; `free`, the free slots of the pool (contract §1.4: `slots` minus the Job tenants minus the held command slots, never below 0); and `holders`, an array of the holders only, free slots have no entry: first one `{slot, holder}` per held command slot in slot order, `holder` the `command` record of contract §1.4 (each slot read with the shared try-lock probe), then one `{slot: null, holder}` per Job tenant by `jobs.seq`, `holder` the derived shape `{by:"attempt", plan, job, attempt, at}` of the Job's latest Attempt, a Job holder having no slot number. The list includes the built-in `workers` (contract §8.3): `scope` `host`, `slots` = `concurrency` and `holders` one `{slot: null, holder}` per tenant Job (a `failed` Job included and more than `slots` while an urgent Job holds the slot beyond).

```json
{"ok": true, "repo": "clayton-duarte/card-dashboard", "resources": [
  {"name": "e2e-port", "scope": "repo", "slots": 1, "free": 0, "holders": [{"slot": 1, "holder": {"by": "command", "pid": 4242, "command": "npm run dev", "session": "9f1c…", "attempt": null, "at": "2026-10-26T21:40:00.000Z"}}]},
  {"name": "shared-port", "scope": "host", "slots": 2, "free": 1, "holders": [{"slot": null, "holder": {"by": "attempt", "plan": "card-dash", "job": "e2e-1", "attempt": "e2e-1-a1", "at": "2026-10-26T21:41:00.000Z"}}]}]}
```

Idempotent: yes; it only reads.

### 30.2 `resource run`

```
cpd resource run <name> [--wait <minutes>] -- <command> [<arg>...]
```

Takes a slot of the resource, then runs the command holding it. It decides the pool's free slots under the pool's guard, the same count the starter uses (contract §1.4 "The free slots of a pool"), so a command and a Job's start cannot both take the last slot. The resource is the one a human takes (the User, or a Lead running a command the User asked for, §15.20).

| Argument | Meaning |
|---|---|
| `<name>` | a resource visible to the repo: declared by the host, owner or repo policy; `workers` is reserved and refused `usage` (exit 2) |
| `--wait <minutes>` | a positive integer; absent means 0. Tries again every second until the minutes have passed, by the Ports' clock (so tests never sleep) |
| `-- <command> [<arg>...]` | the command and its arguments, run from the current directory with the current environment, never through a shell |

| Precondition | Code | Exit |
|---|---|---|
| a command follows `--`; `--wait` is a positive integer | `usage` | 2 |
| the directory is in a git worktree whose `origin` can be keyed | `repo_unkeyed` | 1 |
| the repo's policy resolves | `owner_unconfirmed`, `policy_invalid`, … | 1 |
| `<name>` is declared; `details` `{resource, repo, declared, file}` | `resource_undeclared` | 3 |
| the pool has a free slot (contract §1.4: `slots` minus the Job tenants minus the held command slots), taking under the guard the lowest free slot file by a non-blocking exclusive try-lock, tried again every second for `--wait` minutes; `details` `{resource, repo, holders, waited_minutes}` (`holders` the holders of the pool, as in `resource list`; `waited_minutes` the minutes given, 0 when absent). The command never starts | `resource_busy` | 3 |
| the command can be executed (after the lock is held; the process and the lock are gone together) | `spawn_failed` | 1 |

Effects: after it holds a slot it writes the command holder record (contract §1.4: `pid` its own process id, `command` the argv joined by single spaces, `session` from `CPD_SESSION` and `attempt` from `CPD_ATTEMPT`, null when unset), then **replaces itself with the command** (`exec`):

- the command keeps the process id (the record's `pid`), the stdio and the signals;
- it inherits the slot file's lock descriptor, so the slot stays held for as long as the command or any descendant that keeps the descriptor lives, which is what a dev server on a port needs;
- a signal to the process or to its group (`kill -TERM -- -<pgid>`) stops both the command and the hold;
- the command's output is the output: `run` prints no JSON value once it has exec-ed (a command that owns its terminal, contract §1.2) and the exit status is the command's;
- if the exec fails, `spawn_failed` (exit 1) and the lock goes with the process.

**Re-entrant for an Attempt whose Job is a tenant of the pool.** When `CPD_ATTEMPT` is set and the Job of that Attempt is a tenant of the pool (contract §3.4 "Tenancy"), `run` takes nothing more and execs the command at once: the Job already holds a slot and must not wait on itself or take a second.

**Hooks.** A repo may write `cpd resource run <name> --` in a hook command (contract §7.3); hooks run without `CPD_*` variables, so `CPD_HOME` is the default. The Gate and the hooks take no resource by themselves.

Idempotent: no. A run while every slot is held is `resource_busy`.

## 31. `review`

Reviews on GitHub, posted by cpd commands (§2.2; pro-d107, pro-d108, pro-d110, pro-d169).

### 31.1 `review submit`

```
cpd review submit --pr <url | owner/repo#n> --head <sha> --event APPROVE|REQUEST_CHANGES --input <review.json> [--slack <channel>/<ts>]
```

Posts a GitHub review and, with `--slack`, queues a reaction to a Slack message (the same mechanism as `pr merge`, §21.9). **Callers:** the Lead, the Manager; not the cockpit or other agent roles. It takes a pull request URL or PullRef form, like `pr merge` (§9.10).

Steps:

1. Validate arguments: `--head <sha>` is required (a missing flag is `usage`, exit 2) and must be exactly 40 ASCII hex digits, either case: not 39 or 41, not empty, no `0x` prefix, no whitespace, no branch name, no abbreviated sha. cpd lowercases it once here; every later use (the head check, the POST's `commit_id`, `details.expected`, `review.commit_id`) is that lowercase value. `--event` must be exactly `APPROVE` or `REQUEST_CHANGES`. `--input` is a file holding the review body as a JSON object (GitHub's REST `POST /repos/{owner}/{repo}/pulls/{n}/reviews` body), containing `body` (optional string) and `comments[]` (optional array of `{path, line, side, body}`); it has no `commit_id` key, the head is `--head`. When a file is unreadable, not JSON, carries its own `event` key or its own `commit_id` key, or is not an object, refuse `usage` (exit 2). With `--slack`, `<channel>/<ts>` must match the pattern `^C[A-Z0-9]{8,}$/^[0-9]{10}\.[0-9]{6}$` or refuse `usage` (exit 2). Every refusal of this step happens before any `gh` call, with nothing queued.
2. Read the pull request's current head with one `gh` call, a GET: `gh api repos/<owner>/<repo>/pulls/<n>` (argv exactly `api` and `repos/<owner>/<repo>/pulls/<n>`: no `--method`, no `--jq`, no stdin; `<owner>/<repo>` and `<n>` come from `--pr`, so a URL and `owner/repo#n` name the same call). Its stdout is the pull request's JSON object; its `head.sha` string is the current head, lowercased. A non-zero exit, a signal, a timeout, stdout that is not JSON, or an object without a string `head.sha` is `gh_failed` (exit 1). If the current head is not `--head`, refuse `head_moved` (exit 3) with `details` `{pr, expected, actual}` (`pr` is `<owner>/<repo>#<n>` even when `--pr` was a URL; `expected` is `--head` and `actual` the current head, both lowercase) and a message that names both SHAs. Both refusals post nothing and queue nothing; the GET is the only `gh` call made.
3. Post the review to GitHub: `gh api --method POST repos/<owner>/<repo>/pulls/<n>/reviews --input -`, with the review JSON on stdin: the input object, with `"event"` added from `--event` and `"commit_id"` set to the lowercase `--head`, so GitHub pins the review to that commit even when a push lands after step 2. GitHub's failures are relayed as `gh_failed` (exit 1, `details.stderr` the tail). cpd checks no content: no refusal for failed checks, out-of-diff comments, or empty body (GitHub enforces). An answer without `id` or `html_url` is `gh_failed` with message "review may have been posted; check the pull request".
4. On success and only with `--slack`, queue the reaction: one store transaction calls `cpd_core::pr::reactions::queue(tx, channel, ts, emoji)` with emoji `github-check-mark` for APPROVE, `github-changes-requested` for REQUEST_CHANGES. A store failure after GitHub accepted is that error with `details.review` added: the review stands.

| Precondition | Code | Exit |
|---|---|---|
| `--head` is present and is exactly 40 hex digits, either case | `usage` | 2 |
| `--event` is exactly `APPROVE` or `REQUEST_CHANGES` | `usage` | 2 |
| `--input` file is readable, is JSON, is an object and has neither an `event` key nor a `commit_id` key | `usage` | 2 |
| `--slack`, if present, matches `^C[A-Z0-9]{8,}$/^[0-9]{10}\.[0-9]{6}$` | `usage` | 2 |
| the pull request's current head can be read (`gh api repos/<owner>/<repo>/pulls/<n>`, a GET; its `head.sha` is a string) | `gh_failed` | 1 |
| the current head is `--head` (`details` `{pr, expected, actual}`) | `head_moved` | 3 |
| the review is posted; `details` has `stderr`, the `id` and `html_url` on success | `gh_failed` | 1 |

Effects:

- Posts a GitHub review on the pull request with the given event, body and `commit_id`. The review is pinned to `commit_id`, so a push after the head read still leaves the review on the reviewed commit.
- If `--slack` is provided, queues a reaction to the Slack message in the store after the review is posted (§21.9).
- The review verdict does not remove the Claim's :eyes: (A5).

Output:

```json
{"ok": true, "review": {"id": 123456, "url": "https://github.com/<owner>/<repo>/pull/<n>#pullrequestreview-123456", "state": "APPROVED", "commit_id": "<head>"}, "reaction": null}
```

With `--slack`:

```json
{"ok": true, "review": {"id": 123456, "url": "https://github.com/<owner>/<repo>/pull/<n>#pullrequestreview-123456", "state": "APPROVED", "commit_id": "<head>"}, "reaction": {"channel": "C123ABC45", "ts": "1234567890.123456", "emoji": "github-check-mark", "already": false}}
```

**Not idempotent** (A4): a rerun posts a second review. The queue row (with `--slack`) is unique per `channel+ts+emoji`, so the same reaction is queued once and placed once by the engine's slack-react step (§21.9).

**What it does not do** (pro-d108, pro-d169): cpd checks one thing about the pull request, its head (pro-d169) and no content of the review. No refusal for failed or pending checks, comments outside the diff, an empty body, or any other condition. GitHub's own refusal is relayed. Review skill rules (FR4) and COMMENT reviews are out of scope.

See §21.9 for the queue and the reaction step.
