# cpd 2: command reference

Status: **normative for every cpd 2 implementation Job**, under [contract.md](contract.md): where the two disagree, contract.md wins and this file is fixed. Vocabulary: [CONTEXT.md](../CONTEXT.md). This file uses the contract's names, states, tables and Event kinds exactly; "§n" without a prefix means contract.md §n, and "cpd 1 §n" means `docs/archive/cpd1/cli-contract.md` §n.

It specifies every command of outline §3 (synopsis, flags, preconditions with their error and exit codes, effects, JSON output, idempotence), the pi adapter's protocol, the Worker contract, what the TUI reads, and `doctor`.

Evidence labels as in contract.md: **verified** (run on the homeserver on 2026-10-14), **docs** (read in the named tool's documentation or source), **unverified** (a design assumption the implementation Job must check).

The commands serve the **CPD flow** (hs-n31): three stages, **Context** (Grounding, Clarifying, Specifying), **Plan** (Dividing, Sequencing, Approving, Dispatching) and **Deploy** (Mechanical checks, PR, Reviewing and Monitoring, Addressing). contract §3.9 maps each step to its phase and commands.

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
14. [`doctor`, `config`, `tiers`, `models`](#14-doctor-config-tiers-models)
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

## 1. Conventions

### 1.1 How each command is written

Every command section has the same parts:

- **Synopsis**, then a table of every flag. `<x>` is required, `[x]` optional, `a | b` exclusive. A repeated flag says so.
- **Preconditions**, checked in the order listed; the first that fails is the result. Each row gives the error `code` and the exit code (§1.2). The common refusals of §1.3 apply to every command and are not repeated.
- **Effects**: **rows** written (table and columns), **Events** appended (kinds of §5.2), **tmux** and **git** actions, and files under `$CPD_HOME` (§1.4). "One transaction" means one write transaction with its `txns` row, its `history` rows and its Events (§4.2). External work (git, tmux, hooks, `gh`, pi) is never inside it.
- **Output**: one JSON example of success. Every failure is the envelope of §1.2, `{"ok": false, "error": {"code", "message", "details"}}`; `details` is listed where it carries something the caller acts on.
- **Idempotent**: what a rerun does. The default of §1.3 is `"already": true`; a command that differs says so.

A command is **internal** when cpd runs it by itself and neither an agent nor the User types it: a program cpd starts in a pane or detached, or a command only cpd's own parts (the engine, the pi adapter) run. Internal commands are hidden in clap (`hide = true`; a command under a hidden one is internal too), so they are absent from `--help` text but listed, marked, in the `tree` (§1.2). Today: `session run`, `attempt run`, `tui view` and its views, `plan decide` and `session state`. They are documented so their implementation Jobs have a contract.

### 1.2 Output and exit codes

Kept from §1.2 (D1). Every command prints exactly one JSON value on stdout. Five kinds of command print something other than one JSON value, and are the only exceptions (amendment 10, §19):

| Command | What it prints | Why |
|---|---|---|
| `cpd events follow` | one Event per line (§5.4) | the one streaming command (§1.2) |
| `cpd gate finish` (§26) | the run's report on stderr, and its exit status | it prints no JSON of its own; the status is the command's, or 64 when the run left something behind |
| `cpd session run`, `cpd tui view` (internal) | they run pi or draw a ratatui view in their pane | they are the programs inside cockpit panes (§6.2), not commands an agent calls. If they fail before taking the terminal, they print the failure envelope and exit with its code |
| `cpd tui action` (internal) | nothing, ever | the StatusBar's click binding and the `merge` popup's confirm (`cpd tui action merge <owner>/<repo>#<n> --client <client>`, hs-d473) run it with `run-shell -b`, which puts the active pane into view mode on a failure or any output (§13.2); it reports a failure with `display-message -c <client>`, and a failed merge with the cockpit's one failure dialog, a `text` dialog (§13.2, §17 "No notes") |
| `cpd` (no arguments), `cpd tui --attach` | nothing on success: it replaces itself with `tmux attach` | the approved outline says `tui` "builds and attaches the cockpit"; without `--attach`, `cpd tui` prints JSON like every command. Bare `cpd` is §13.3 (hs-d259) |

`cpd --version` (like `--help`) prints one JSON value too: `{"ok": true, "version": "0.13.45-070ddee", "protocol": 13, "store": {"opens": true, "refusal": null, "unknown": [], "pending": []}}`. The `version` field is `<semantic version>-<commit>` where the semantic version is `0.<protocol>.<migrations>`: the commit is the short hash the build was given in `CPD_COMMIT` (set by `scripts/install-cpd.sh` and the Gate, so an installed binary reports the commit it was built from), and `dev` when the build was given none, e.g. `0.13.45-dev` (docs/implementing.md "The version string"; docs/versioning.md). `store` is the **verdict** of contract §4.5 ("The window") on `$CPD_HOME`'s database opened read-only, computed by the one function every guard uses (`store::verdict`): `null` when the database file does not exist; otherwise `opens` (`true` when this binary can open the store, migrating or converting it if needed), `refusal` (`null`, or `{"code", "message"}` with `code` one of `store_too_new`, `store_failed`, `migration_changed`, `migration_order`, §1.3) when `opens` is `false`, `unknown` (the migration names the store has applied that this binary lacks, in name order: inside the window they are all additive) and `pending` (the names of this binary's migrations the store has not applied, in the order they would be applied; for an unconverted store the legacy names it lacks, then every named one). `unknown` and `pending` are `[]` for a store that is not named yet, and filled whenever the store is a named store, whether or not it opens. `protocol` is an integer, **14** today (1 until Plan manager added `session-message`, 2 until Plan inbox added the four `inbox-*` kinds, 3 until `session state idle --reply`, hs-d278, 4 until `session-titled`, hs-d257, 5 until `pr-base-changed`, hs-d324, 6 until `session state waiting`, hs-d303, 7 until `review-opened`/`review-closed`, hs-d496, 8 until `session-retiered`, hs-d506, 9 until `plan-outlined`, hs-d502, 10 until `job-merged`'s `data.turn` and `session-message`'s `data.unblocks`, hs-d514, 11 until `session-resumed`, hs-d552, 12 until `session report`, hs-d573, 13 until `mcp-attached`, pro-d8), raised whenever an Event kind, an Event `data` field the pi adapter reads, or the output of a command the adapter runs changes in a way the adapter depends on (contract §5.2 "The protocol number"); the adapter checks it (§15.2). `--version` never creates, migrates or writes the database, and runs neither the schema check nor the conversion. **The guards read `store`:** a driver or a Lead's wrapper reloads into a binary only when `store` is `null` or `store.opens` is `true` (contract §3.4, §6.7), and `scripts/install-cpd.sh` refuses a build whose `store.opens` is `false`, and a Lead Branch build that carries a migration file that is not on `origin/main` (contract §3.7).

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
- **Short Session ids** (hs-a3, hs-q3): wherever a command takes a Session (`--session`, `CPD_SESSION`, and the argument of `session open`, `close`, `fork`, `park`, `rename`, `archive`, `unarchive` and `show`) the value is resolved **once, at the boundary, into a `SessionId`** (`core/src/session/resolve.rs`, `docs/implementing.md`), trying these forms in this order, the first that matches deciding:

  | # | Form | Resolves to |
  |---|---|---|
  | 1 | the full id (a Session whose id equals it, a UUID in practice) | that Session |
  | 2 | a **Plan id** | that Plan's Lead Session, `plans.lead_session` |
  | 3 | **`<plan>·<prefix>`** or **`<plan>.<prefix>`** (split at the first `·` or `.`; `.` because `·` is hard to type, and Plan ids never contain either) | the Session of that Plan whose id starts with `<prefix>` (at least 1 character) |
  | 4 | a **bare id prefix of at least 4 characters** | the Session, of any Plan or none, whose id starts with it |

  Matching is exact and case-sensitive, over every Session, archived ones included (so `session unarchive` and `session history` can name them). A form that matches nothing falls through to the next; when none matches, `session_not_found` (exit 4). A form (3) or (4) that matches **more than one** Session is `session_ambiguous` (exit 3) with `details.candidates`, the matching ids, and is never resolved by guessing. A value outside §2.1's input pattern is `invalid_session_id` (exit 2); one that is valid but shorter than 4 characters and no Plan id or form (3) is `session_not_found`. Output always carries the full id.
- **PR** (R34, §1.3): `--pr <key>`, within the selected Plan, used by `job add`, `pr open`, `pr rebase`, `pr push`, `pr view` and `pr reply`. It may be omitted when the Plan has exactly one PR that is not `cancelled`, which is then selected; with two or more, `pr_required` (exit 2); with none, `pr_required` too, its message naming `cpd pr add`. A key that is not one of the Plan's PRs is `pr_not_found` (exit 4). Exceptions, stated at each command: `pr poll` has no `--pr`, it polls every open pull request of the User (§9.2, hs-d96), and `job edit --pr` changes the Job's PR, so omitting it changes nothing (§5.2). `pr add`, `pr edit`, `pr cancel` and `pr start` take the key as their positional argument.
- **Job, Attempt, Escalation, Plan review:** a positional id, within the selected Plan (§2.1).

Common refusals, which apply to every command and are not repeated in the sections below:

| Code | Exit | When |
|---|---|---|
| `usage` | 2 | unknown flag, missing argument, flag value of the wrong shape, exclusive flags together |
| `plan_required`, `context_required`, `session_required` | 2 | the command needs one and none was selected |
| `pr_required` | 2 | the command needs a PR, `--pr` is omitted, and the Plan does not have exactly one PR that is not `cancelled` (R34) |
| `supersedes_invalid` | 3 | `context decide` or `context note` named, in `--supersedes`, a record whose kind it may not supersede (§3.4); `details: {record, kind, allowed}` |
| `curation_not_pending` | 3 | `context retire` or `context docs` (§3.16, §3.17) named a record that has no `pending` retirement (or doc candidate): none, already decided, or not curated; `details: {record}`; nothing is written (hs-d551) |
| `user_only` | 3 | `context curation`, `context retire` or `context docs` (§3.15 to §3.17) ran with `CPD_SESSION` or `CPD_ATTEMPT` set in its environment: only the User decides what leaves the Context and what is documented, so no agent's Session runs them (hs-d551); the cockpit's popups and views run without those variables (§3.15) |
| `manager_missing` | 3 | `context docs --send` (§3.17) found no live Manager Session (role `manager`, not archived) for the Context; nothing is written |
| `args_invalid` | 2 | `context retire` or `context docs` (§3.16, §3.17) named one id in both of its lists |
| `session_ambiguous` | 3 | a short Session id (§1.3) matches more than one Session; `details.candidates` lists their ids |
| `push_rejected` | 3 | `pr push` found the remote branch moved (§9.9); `details.stderr` is git's |
| `base_not_found` | 4 | `pr add`, `pr edit` (§9.4): the base branch does not exist; `pr open` (§9.1): the base branch of a stacked PR, its base PR's Lead Branch, is not on the remote (never for a `merged` base PR: that PR is unstacked first, hs-d301); `details` `{pr, base, base_pr}` (hs-d324) |
| `base_conflict` | 3 | `pr open` (§9.1, contract §7.6 step 1) or an `open` or `update` Job's submit (§6.1, contract §7.7) found that the gated commit conflicts with `origin/<base>`, freshly fetched (`git merge-tree`); `details` is `{pr, base, base_sha, sha, files, next}`, `files` the conflicted paths, `next` the commands the Lead runs, as written (hs-d490; it replaces hs-d329's `base_behind`, retired: a base that merely moved ahead refuses nothing) |
| `pr_not_open` | 3 | `pr reply` (§9.8a) found the PR not `open`; `details` is `{pr, status}` (pro-d68) |
| `feedback_not_found` | 3 | `pr reply` (§9.8a): `--to` is the `id` of no item in the PR's stored list; `details` is `{pr, to, open: [ids]}`; nothing is posted (pro-d68) |
| `pr_not_closed` | 3 | `pr cancel` (§9.6) found the `open` PR's pull request not stored `closed`; `details` is `{pr, number, url, state}`, the stored row's `state` (`open` or `merged`) (hs-d329) |
| `base_pr_unknown`, `base_pr_unavailable`, `base_pr_other_repo`, `pr_other_plan` | 3 | `pr add` or `pr edit` (§9.4, §9.5) found a bad PR reference in `--base <plan>/<key>` or `--needs`: no such PR; a base PR that is `cancelled` or `merged`; a base PR of another repo; a PR of another Plan (contract §2.2 "Validation of a reference", hs-d522); `details` as at §9.4 |
| `stacked_rebase` | 3 | `pr rebase` (§9.3) found the PR stacked on a base PR; `details` is `{pr, base, base_pr, next}` (contract §3.8 "Stacked PRs" step 4, hs-d325) |
| `invalid_context_id`, `invalid_plan_id`, `invalid_pr_id`, `invalid_job_id`, `invalid_attempt_id`, `invalid_escalation_id`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id` | 2 | an id does not match §2.1 |
| `config_missing`, `config_invalid` | 1 | §8.1; hs-d562, pro-d53, hs-d554 and hs-d506 add the Tier checks of contract §8.2 (§9 "Tier rules"): G0–G5 are `tier_invalid` (exit 2, below); `context_distill_tier` must be GX, not H0 or anything else, else `config_invalid` (hs-d562); `lead_tier` or `manager_tier` cannot be letter G ("must not be a G Tier..."), else `config_invalid`, or effort 5, else `config_invalid`; `open_tier` must be a Tier a Worker may have (§9), else `config_invalid`. Each with `details.key` |
| `tier_not_allowed` | 2 | `job add`, `job edit`, `job retry` (§5.1, §5.2, §5.4) and `job dispatch` (§5.5): the Job's Tier is outside the Worker range of contract §9 "Tier rules" (`H0`–`H4` and `S0`–`S3`); Workers use only these Tiers to keep cost down (pro-d53, hs-d554, contract §9 "Tier rules"); `details` `{tier, allowed, rule}`; also an `error` Event of engine E3 (contract §3.7) for a stored Job on such a Tier |
| `store_too_new` | 1 | the store has a breaking migration this binary does not know, or a later scheme (contract §4.5 "The window"; it keeps its code and exit); `details: {user_version, floor, unknown}`: the store's `user_version`, its floor (the name of its newest breaking migration, or `null`) and every applied name this binary lacks. A store with a numbered migration this binary lacks (`user_version` above the legacy set and below 1000) is the same code, with `unknown` empty |
| `migration_failed` | 1 | a migration, or the conversion, failed and rolled back (§4.5); `details: {from, to, sqlite_message}` |
| `migration_changed` | 1 | an applied migration this binary knows has another checksum than the store recorded (§4.5 R3); `details: {name, recorded, embedded}` |
| `migration_order` | 1 | a pending migration cannot be applied in this order, nothing written (§4.5 O3): a breaking one while the store has an applied migration that sorts after it or that this binary does not know; `details: {name, applied_after, unknown}` (names) |
| `store_tampered` | 1 | a write command, when the schema check fails (§4.3); read commands report it in `tamper` instead |
| `store_busy` | 5 | no write lock within `store_busy_ms` (§4.2); `SQLITE_BUSY` and `SQLITE_LOCKED` both |
| `internal` | 1 | a bug in cpd itself (for example a malformed Event built by a command), or a failed write to stdout other than EPIPE (no I/O error code exists). `store_failed` is for SQLite failures only |
| `context_not_found`, `plan_not_found`, `pr_not_found`, `job_not_found`, `attempt_not_found`, `escalation_not_found`, `review_not_found`, `session_not_found` | 4 | the selected or named object does not exist. (A Context record that names a missing record is `record_not_found`, exit 3, §4.6) |
| `merge_not_clean` | 3 | `pr merge` (§9.10) re-read the pull request and found it not `OPEN` or not `CLEAN`; `details` has `repo`, `number`, `state`, `merge_state` (hs-d272, hs-d274) |
| `mcp_unknown` | 4 | `mcp attach` (§24.1) named a server the machine policy does not list; `details: {name, machine, available}`, `available` the sorted server names (an empty array when none); nothing is written (pro-d8) |
| `item_not_found` | 4 | `inbox show`, `snooze`, `dismiss` or `claim` named an Inbox item that does not exist |
| `item_closed` | 3 | `inbox snooze`, `dismiss` or `claim` named an Inbox item that is closed (§21) |
| `not_claimable` | 3 | `inbox claim` (§21.7): the item is not `review-requested` or is dismissed (C4); `details.kind` |
| `review_context_unset` | 3 | `inbox claim` (§21.7): config `inbox.review_context` is unset (C5) |
| `post_not_found` | 3 | `inbox claim` (§21.7): the team review cache holds no primary post of the pull request (C4) |
| `not_loop_guarded` | 3 | `session resume` (§10.17): the Session is not a `lead` or a `manager`, or is archived, or is not `blocked` with label `φ: loop guard`; `details` `{state, label}` |
| `slack_auth` | 3 | the team review helper found no usable Slack sign-in in pi's MCP credentials (§21.6); the message says to run `pi mcp login slack` |
| `slack_not_configured` | 3 | there is no `slack` server in `~/.pi/agent/mcp.json` (§21.6); the message says to run `pi mcp add slack --url https://mcp.slack.com/mcp --exposure hidden` |
| `slack_failed`, `slack_unparseable`, `slack_helper_failed` | 1 | the helper could not read the channel, read a payload with no messages in it, or could not load pi's MCP modules (§21.6). Also the code of the engine's E11 `error` Event (contract §3.7) |
| `archive_blocked` | 3 | `context archive`, `plan archive` or `session archive` found a live object (contract §2.2 "Archived"); `details.blockers` lists each, `details.next` says what to do |
| `archived` | 3 | the object (or its Context or Plan) is archived and the command does not work on archived objects; `details.next` is the unarchive command |
| `cas_lost` | 3 | a compare-and-swap found the state changed since it was read (§4.2); rerun the command |
| `illegal_transition` | 3 | a transition not in §3, refused by the command or by a trigger |
| `locked_job_tampered` | 3 | a Locked Job's fingerprint does not match (§2.3) |
| `pr_title_required` | 3 | `pr open` found a PR with no stored title, one that existed before migration `pr-title` (contract §7.6, hs-d285); the message says to run `cpd pr edit <key> --title "<text>"` |
| `needs_required` | 3 | `pr add` in phase `dispatch` found neither `--needs` nor `--no-needs` for a PR that is not stacked (pro-d46); a PR added after the lock with no needs is started at once and cannot be linked later; for a stacked PR, the base PR is its link; `details` `{plan, pr, phase}` |
| `adopt_not_open` | 3 | `pr add --adopt` (§9.4): the pull request's `state` is not `OPEN`; `details` is `{number, url, state}` |
| `adopt_fork` | 3 | `pr add --adopt`: the pull request is from a fork (`isCrossRepository`); cpd cannot push to a fork; `details` is `{number, url}` |
| `adopt_cpd_branch` | 3 | `pr add --adopt`: the head branch starts with `cpd/`, so it is a cpd pull request; `details` is `{number, head_ref}` |
| `adopt_taken` | 3 | `pr add --adopt`: a PR of any Plan with the same repo and head branch is `waiting`, `dispatch` or `open`; `details` is `{head_ref, plan, pr}` |
| `adopt_foreign` | 3 | `pr add --adopt`: the pull request's author is not the User and `--confirm-author` is missing or differs (case-insensitively); the message tells the Lead to ask the User first; `details` is `{number, url, author, user, confirm: "--confirm-author <author>"}` |
| `pr_adopted` | 3 | `pr edit --repo` or `--base` on an adopted PR (§9.5): the repo and the base come from the pull request; `details` is `{pr, flag}` |
| `kind_reserved` | 3 | `job add --kind open` or `update` (§5.1): cpd adds those Jobs, never the Lead (contract §2.2 "Job kinds"); `details.kind` |
| `test_job_required` | 3 | an `impl` Job that is not a draft, or a Plan review or lock over one (§5.1, §4.6, §4.8), needs no `test` Job of its PR; `details.jobs` lists the `impl` Jobs |
| `tests_unconfigured` | 3 | `pr add --shape tdd` (§9.4), `job add --kind test` or `impl` (§5.1) or a `test` or `impl` Job's submit (§6.1) found that the PR's repo policy has no `[test-commands]` (no repo file, or a file without the table; contract §7.7); the message names `--shape docs`, `--shape config` and the table |
| `test_files_missing` | 3 | a `test` Job's submit (§6.1): `details.paths` lists the `test_files` entries that match no path the Job changed |
| `tests_build_failed`, `tests_not_red`, `tests_failed` | 3 | a `test` or `impl` Job's submit (§6.1, contract §7.7): the repo's `build` failed; the named tests passed where they must fail (`test`); they failed where they must pass (`impl`). `details` `{job, tests, tail, log}` |
| `tests_not_found` | 3 | a `test` or `impl` Job's submit (§6.1, contract §7.7): the repo's optional `[test-commands]` `list` command exited non-zero for a named test, so no test has that name (hs-d497); `details` `{job, tests, tail, log}`, `tests` the names that match nothing |
| `shape_violation`, `shape_incomplete` | 3 | the PR's stored shape (§9.4, contract §2.2 "Job kinds", hs-d497): `job add` or `job edit --kind` gave a kind the shape does not allow (`details` `{pr, shape, kind, allowed}`; for an `open` PR `{plan, pr, status: "open", shape, kind, allowed: ["work"]}`, whatever the shape); a `tdd` PR has no `test` or no `impl` Job that is not `cancelled`, at `plan review`, `plan lock` or `job cancel` typed by the Lead (`details` `{pr, missing}`); **`pr cancel` and `plan cancel` never refuse it**: cancelling a PR or a Plan is never refused by the shape check (§5.3, §9.6, §4.12, hs-d573) |
| `tests_error` | 1 | a `test` or `impl` Job's submit: the repo's `[test-commands]` commands could not run (killed at `gate_minutes`, killed by a signal, `sh` not startable, `hook_invalid`; `list` included); the Worker asks the Lead. `details` as above |
| `tests_tampered` | 3 | an `impl` Job's submit (§6.1) changed a path the `test_files` of a `test` Job it needs guard and the Lead has not approved (§5.9); `details.paths`, `details.jobs` |
| `lead_branch_moved` | 3 | an `open` or `update` Job's submit (§6.1) found the Lead Branch's tip not in HEAD; `details.next` is the merge to run (contract §7.7) |
| `open_job_live` | 3 | `pr open` (§9.1) found the PR's `open` Job `draft`, `queued` or `running`; `details.job` (hs-d573) |
| `not_an_impl_job`, `path_not_guarded` | 3 | `job approve-tests` (§5.9): the Job is not of kind `impl`; `details.paths` lists the paths no `test_files` entry of its test Jobs guards |
| `host_approval_required`, `not_a_manager` | 3 | `cpd host repair` (§9.17): needs an approval of the calling Session (`host_approval_required`); Manager role only (`not_a_manager`, commands.md §9.17) |
| `git_failed`, `tmux_failed`, `gh_failed` | 1 | the tool exited non-zero where the command did not expect it; `details` has `argv`, `exit` and the last 60 lines of stderr. `pr poll` also returns `gh_failed` for an empty or non-JSON body, a search response with `errors`, and a lookup with no `data` (contract §7.6, hs-d96) |
| `gh_rate_limited` | 1 | an `error` Event code of engine E5 (contract §3.7, §5.2; `details`: `remaining`, `reset_at`), reported while it skips polls because GitHub's rate limit is nearly used; never a command's refusal |
| `install_state_invalid` | 1 | engine E6 (contract §3.7) or a command read the install state file `$CPD_HOME/run/cpd-install.json` (§1.4) and found it invalid (not JSON, unknown fields, missing or type-mismatched fields); `details: {path, reason}` |
| `adapter_missing` | 1 | an `error` Event code of the Lead wrapper `session run` (contract §6.7 "Adapter watchdog", §10.8; `details`: `session`, `state_since`, `seconds`), appended when it restarts a pi whose adapter never came back; never a command's refusal |
| `session_restart_loop` | 1 | an `error` Event code of engine E2 (contract §3.6, §3.7; `details`: `session`, `name`, `seconds`), appended when E2 detects a Session's pane gone but E2 itself restarted it less than 10 minutes ago (hs-d725, hs-d727), to guard against restart loops; never a command's refusal |

### 1.4 Files written by commands

The `$CPD_HOME` layout is §1.4. Commands in this file write files the contract's layout does not list (amendment 4, §19); each is beside its peers and named so it cannot collide:

| File | Written by | Read by |
|---|---|---|
| `run/<plan>/lead-message.md` | `plan new` | the Plan's Lead `session run` (its first message) |
| `run/<context>/manager-message.md` | `context manager` (B3-A2) | the Manager's `session run` (its first message) |
| `run/<plan>/summary.md` | `plan render` (§4.5, hs-d502): the Plan summary of the Plan as it is, without a review | the User, the Lead |
| `run/<plan>/plan-<review>.md` | `plan review`: the full render | the pi adapter, which opens it in plannotator only when the User chooses "Open full plan" (§15.5) |
| `run/<plan>/summary-<review>.md` | `plan review` (§4.6, hs-d502): a copy of the summary that a later render cannot change, as `plan-<review>.md` | the pi adapter, which shows it in the Lead's chat (§15.5) |
| `run/<plan>/feedback-<review>.md` | the pi adapter: the User's note from the chat dialog's Change, or plannotator's feedback (§15.5), overwritten on a retry | `plan decide --feedback-file` |
| `run/<plan>/messages/<session>.md` | `session fork --message-file` copies the file here | that Session's `session run` |
| `run/<plan>/reviews/<job>.md` | a review Job's Worker (B2, contract §1.4) | the Lead (and the User) |
| `run/titles/<session>.log` | engine E10: the output of the `session retitle` runs it starts (hs-d257, contract §1.4); `locks/title-<session>.lock` is the run's own lock (§10.14) | the User |

### 1.5 Text arguments

Free text (Job `--title`, `--reason`, Context record text, questions and answers) is one argument, 1 to 20 000 characters, with no NUL; else `usage`. Context and Plan titles (`--title` of `context new`, `context rename`, `plan new`, `plan rename`) are 1–200 characters with no control characters, else `usage` (exit 2). Names (`--name`, `session rename`) are 1–80 characters with no control characters, else `invalid_name` (exit 2) (cpd 1 §11 `session rename`, kept). Files given by path (`--spec`, `--brief-file`, `--feedback-file`, `--message-file`) must be readable UTF-8, else `file_unreadable` (exit 2). A PR's title (`--title` of `pr add` and `pr edit`, hs-d285) is 1–72 characters on one line, with no control character; else `usage`.

## 2. Command index

The index is generated: [docs/interface.md](interface.md), rendered from the `tree` of `cpd --help` (§1.2) by the test `cli/tests/interface.rs`, which fails when the page and the binary differ, either way; `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface` rewrites the page; never edit it by hand.

**Definitions:**
- **callers** = the agents meant to run the command, any of `Lead`, `Manager`, `Worker`.
- **cockpit** = an action of the User's cockpit runs it (the cockpit is the User's interface over the same commands, §17 Actions).
- **internal** = §1.1.

Anyone may run any command; a command with no callers that is neither cockpit nor internal is the User's, at a shell (for example `doctor`). The callers and the cockpit flag are kept in the binary (`cli/src/interface.rs`), next to the clap tree. The table's Writes column (each command's Effects says what it writes) and Contract column (each description ends with its §) are not carried over (hs-d251).

**Documented here before the binary has them** (the policies, contract §8.3, hs-d288). The Job that implements each adds it to the binary's tree with these values, and `docs/interface.md` is then regenerated; until then this table is their index entry:

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
| `cpd context curation` (§3.15, internal) | none | yes | yes |
| `cpd context retire` (§3.16, internal) | none | yes | yes |
| `cpd context docs` (§3.17, internal) | none | yes | yes |
| `cpd pr reply` (§9.8a) | Lead | no | no |
| `cpd tag` (§26.1) | Lead | no | no |
| `cpd version sync` (§26.2) | Lead | no | no |

`context new` gains `--owner` (§3.1), and `pr add`, `plan new`, `pr open`, `pr push` and `pr rebase` read the policies (§9.4, §4.1, §9.1, §9.9, §9.3); their entries are unchanged. `job add` and `job edit` gain `--kind` (§5.1, §5.2) and `pr add` gains `--shape` (§9.4), hs-d340 and hs-d341.

## 3. `context`

A Context and its records (§2.2, §4.6). The records live only in the database, and Contexts are machine-local (hs-a1). Every record write follows §4.6 exactly: one transaction that validates, inserts the `context_records` row and appends `context-record` `{record, kind, machine, seq}`. No lock file, no git, no file.

**Record ids.** `<machine>-<letter><n>` with `n` = 1 + the highest `n` already in this Context's records for this machine and letter (§2.1). The write transaction makes it race-free; only this machine writes its letter sequence.

**Retries.** A record write is recognised as a retry, and returns the existing record with `"already": true`, when this machine's latest record in the Context has the same `kind`, `text`, `supersedes`, `question` and `by.session`. Any other write appends a new record; records are never edited.

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

Effects: none. Output: the Context, its records in index order, and `markdown`, the rendering of §2.2 "Context record" (title and Jira key; Plans; active decisions; superseded decisions with their successor; open questions; answered questions with their answers; active notes; superseded notes with their successor, labelled as a superseded decision is; a record retired by an approved retirement (contract §4.6 "Retirement", hs-d549, hs-d551) is labelled `retired (<reason> <target>)`, in the section of its kind, as a superseded one is labelled with its successor, and is in no digest; each item with its id and date; briefs under their Plan, see below). With `--since`, `markdown` renders only those records, in the same order of sections.

**Briefs** (B3-A4): a record of kind `brief` is listed **short**, in `markdown` and in `records`: its id and the first line of its `text`, under its Plan (the Plan's section), never in full. In `records` its `text` is that first line and the entry has `"short": true`; its `plan` is `{id, machine}`. The full text is `cpd context show --record <id>`. `--record` is the only way to get a brief whole.

**Plans** (R34, A12): each `plan` record is listed with the repos of its Plan's PRs that are not `cancelled`, read from this machine's database (`prs.repo`) in the same read transaction, each repo once, in PR key order; a Plan of another machine, or one with no PR yet, shows none. The record itself carries no repo (§2.2; a `repo` in a record written before R34 was dropped by migration 0007's import, contract §4.5). In `records`, a `plan` record's `plan` is `{id, machine, jira}`, and the entry has `repos`, the list just described.

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
| `<query>` | search string to match against topics and short forms of curated records |

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

`supersedes_invalid` has `details: {record, kind, allowed}`: the id named, its kind, and the kinds the writing record may supersede (`["decision", "note"]` for `decide`, `["note"]` for `note`). Nothing is written. A record superseded by a decision or note leaves force and every digest, stays readable, and `context show` labels it superseded, naming its successor (§3.3).

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
| no object of the cascade is live: no Session `working`, `waiting` or `blocked`, no Job `queued` or `running`, no open Escalation, no pending Plan review, and no PR of a Plan of the Context that a PR of a Plan outside it names as base PR or need (a Context's Plans archive together; contract §2.2 "Archived"); `details.blockers` lists each as `{kind, id, reason}` | `archive_blocked` | 3 |
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

Effects: **rows**, one transaction: `archived_at = NULL` on the Context and every archived Plan and Session of the cascade; one **Event** per object changed (`context-archived`, `plan-archived`, `session-archived`, each with `{archived: false}`). No tmux: no pane is touched, and `session open` resumes a Lead later (§10.4). No archive rule applies.

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
- **None:** **rows**, one transaction: `sessions` (new UUID v4; `role = manager`; `context`; `plan` null; `name` = `<context>-manager`; `cwd` = the realpath of the caller's cwd; `tier` = config `manager_tier`, default `O5`; `forked_from` null; `by_user` = 1 when `--by-user`, else 0; `stopped`); **Event** `session-registered` `{role, name, forked_from, tier}`. Then **tmux**, as `plan new` starts a Lead (§4.1 step 4): `new-window -d -t =cpd: -n ctx-<context> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -P -F '#{window_id} #{pane_id}' -- cpd session run <session>`, `@cpd_session` on the pane (no `CPD_PLAN`, no plannotator variable; `session run` also removes every `PLANNOTATOR_*` variable it inherited from the tmux server or the caller before it starts pi, whatever they hold); one transaction: `sessions.tmux_pane`, **Event** `session-moved` `{pane, window, place: "parked"}`. When `by_user` is 1, no first message is written; when it is 0, its first message is written to `run/<context>/manager-message.md` (§1.4): `You are the Manager of Context <c> "<title>". Follow the cpd-manager skill.`, then the Context as `plan new` gives it (§4.1 step 3; contract §4.6 "Consumers", the same function): the digest and at most 20 uncurated records raw.
- **`stopped` with no pane:** the tmux step alone, in the stored `cwd`: `session run` resumes pi on its session file (`pi --session <file>`, §6.5), with no first message.
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
                     "lead_branch": "cpd/pay-4521/api", "pull_request": {"number": 212, "url": "https://github.com/acme/api/pull/212", "state": "OPEN"},
                     "jobs": [{"key": "retry-core", "status": "merged", "title": "Retry policy", "touches": ["src/retry.rs"]},
                              {"key": "retry-cli", "status": "waiting", "title": "Retry flag", "touches": ["src/cli.rs", "docs/commands.md"], "waiting_on": ["retry-core"]}]},
                    {"key": "old", "repo": "/home/clayton/projects/api", "base": "main", "status": "cancelled", "pull_request": null,
                     "waiting_on": [],
                     "jobs": []}]}]}
```

With `--markdown` the output is the same data as markdown: `# Pending work in <context>`, then per Plan `## <id> (<phase>): <title>`, per PR `### PR <key> (<repo>, base <base>): <status>` with its pull request (`#<n> <state> <url>`) or `no pull request`, and when waiting, `waiting on <keys>` (§hs-d521), then one line per Job `- <key> <status>: <title> (<touches, comma-separated>)` or `- <key> waiting on <keys>: <title> (…)`. A Context with no active Plan prints `No pending work.`

Idempotent: yes; it only reads.

### 3.13 `context distill`

```
cpd context distill [--context <c>] [--batch <n>]
```

Curates one batch of the Context's records (contract §4.6 "The distiller curates" and "The distill run", hs-d549, hs-d551, replacing the digest-writing run of hs-d194 D1–D5; GXD1-GXD2 hs-d563: per-line acceptance, rejected lines stay uncurated): at most **N uncurated records** in force (FX4: N is `--batch` from 1 to 60, default 60, at most 49152 bytes of their text in batch.md, hs-d549, hs-d551) get a topic and a short form (for machines), and the run may propose retirements and flag documentation candidates, written by one-shot pi and **checked structurally by cpd**, with each line validated independently so invalid lines do not fail the whole output (GXD1). The curations are read by `context show --topic` for discovery and retrieval. Engine E9 starts it detached when the Context has uncurated records in force and the newest of them is 10 minutes old, or 20 or more exist (contract §3.7); one batch per run, so a backlog (751 records for `building-cpd`) is curated by one run per tick. When a run for a state (the highest uncurated seq and count) fails (distill_invalid only when no line passes, distill_failed after the whole ladder, or a crash), E9 starts the next run for the same state with the next smaller batch (60 → 30 → 15 → 5 → 1, FX4) and records the batch size with the state; after batch 1 fails, it stops until the state changes (a success curates at least one record, GXD1, or a new record) or the engine restarts (contract §3.7 E9). Anyone may run it on demand, and it then runs in the caller's process and waits for pi. The Context is `--context`, else the selected Plan's (§1.3). Its §2 row is added by the Job that implements it, as §10.9's was, so that `every_documented_command_exists` holds on every commit.

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
- **Skipped when not curable** (R4 of hs-d592): when the Context is archived, deleted, or under `context_curation_min_records` (§8.2), the command exits 0 before taking the lock, writes nothing, creates no Event, and its JSON data is `{"context": "<c>", "skipped": "archived"}` for an archived Context, or `{"context": "<c>", "skipped": "young", "records": <n>, "floor": <N>}` for one under the floor (archived is checked first). This makes engine E9's runs skip non-curan Contexts without process, lock or Event (contract §3.7 R5, R8).
- **Otherwise**: the files `prompt.md`, `batch.md` (at most N uncurated records in force, oldest first, in full, where N is the batch size), `known.md` (the topics in use, then `[<id>] <topic> | <short>` for every curated record in force) and `docs.md` (for each batch record, the `<repo>:<path>` hits of `git -C <repo> grep -l -w <id> -- docs '*.md'` on each repo of the Context's Plans' PRs, run by cpd), then the distiller, `pi --mode json ...` at `context_distill_tier` (default `H0`; hs-d560, contract §4.6 step 4: a distill run has no ladder, one step only; contract §6.5 "Distiller", §8.2), JSON mode, read by cpd, killed after `timeouts.distill_minutes`. GXD3: when pi's stop reason is `length`, cpd keeps the text up to the last complete line (drops incomplete trailing line if any) and notes `cut: true`. **GXD1-GXD2 Per-line acceptance**: every line is validated independently; lines that pass stay curated, lines that fail are rejected (their record stays uncurated); the output lists all attempted lines with a `rejected` array (GXD2) of failures. One transaction calls `propose` with only the passing lines; the run succeeds when at least one line passes.
- **The checks** (contract §4.6 step 5, GXD1-GXD2 hs-d563): each line is validated independently; failures are collected (GXD2: listed in `rejected` array), not fatal to the whole output. For each line: valid UTF-8 JSON of the output shape; `id` in the batch (GXD1: missing records stay uncurated; id not in batch is ignored; id appearing twice means both lines rejected); field bounds (`topic` 1 to 40 characters matching `[a-z0-9-]{1,40}`, `short` 1 to 120 bytes with GXD4 100-byte rule hs-d563, `reason` one of three); `duplicate` target is a record in force not the record itself with seq < record's seq; `superseded` target in force not the record itself with seq > record's seq; `documented` target is one of that record's paths in `docs.md`. GXD4: prompt examples have short of 90–100 bytes, never say only `correction to <id>`. No repair turn, no `## Contradictions`, no cite-every-decision: removed (hs-d549, hs-d551).
- **Valid run** (GXD1: at least one line passes): one transaction calls `propose`: for each passing line, the curation and retirement proposal; for doc candidates the `doc_candidate` `pending` with hits as `doc_paths`; then appends the **Event** `context-curated` `{context, curated, rejected, retire, docs}` (contract §5.2; GXD2: `rejected` the count of rejected lines, 0 when all passed). The output carries `rejected: [...]` and when any line was rejected or output was cut, raw text kept as `<upto_seq>.jsonl.invalid`. The proposals take effect only when the User decides them (Inbox items `curate-retire` and `curate-docs`, §3.15–§3.17). **Invalid run** (no line passes or output could not be parsed): the last output is kept as `run/context-digests/<context>/<upto_seq>.jsonl.invalid` (one per Context, replacing the previous; a valid run removes it) and the run fails (hs-d560: no ladder, no `distill-tier` item).
- **Run log**: every run that started pi appends a line to `run/context-digests/<context>/runs.jsonl` (`started_at`, `seconds`, `model`, `tier` (the Tier of that pi run, `context_distill_tier`, hs-d560), `tokens_in`, `tokens_out`, `cost`, `context`, `upto_seq`, `result`, `kind`, always `distill`; contract §4.6 "The run log"; GXD2: when any line was rejected or output was cut, `<upto_seq>.jsonl.invalid` is kept), also when it failed.
- **tmux:** none. **git:** read-only (`git grep`).

Failures, after the lock is taken. Each appends **one** `error` Event `{source: "context distill", code, message, details}` with no `context`, `plan` or `session` set (`details.context` names it; so no Lead gets a turn, contract §5.3), changes no curation, and returns the same code:

| Failure | Code | Exit | `details` |
|---|---|---|---|
| pi could not start, exited non-zero (an error, or an answer cut at the output limit), ran longer than `timeouts.distill_minutes`, or its JSON stream had no final assistant text (GXD3: length with no complete line fails as distill_failed) | `distill_failed` | 1 | `context`, `upto_seq`, `reason` (`spawn`, `exit`, `timeout` or `output`), `exit` (when it exited), `tail` (pi's last 60 stderr lines), `log` |
| the output fails any check above, after the last Tier of the ladder (GXD1: no line passes; GXD2: rejected lines listed in output; GXD3: output cut at length with no complete line) | `distill_invalid` | 1 | `context`, `upto_seq`, `errors` (every failure, each `{id?, line?, what}`; GXD2: rejected lines, each with id, line number from 1, and reason), `invalid` (the kept `.jsonl.invalid` file), `log` |

`log` is `run/context-digests/<context>/distill.log`, where the run writes its stderr (and, started by E9, its stdout too).

```json
{"ok": true, "context": "building-cpd", "distilled": true, "steps": ["H0"],
 "curated": 59, "rejected": 1, "retire": 4, "docs": 2, "remaining": 692,
 "run": {"seconds": 94.2, "model": "anthropic/claude-sonnet-4-5:medium", "tokens_in": 61234, "tokens_out": 7410, "cost": 0.43}}
```

`curated`, `rejected` (GXD2) and `docs` are the counts of the `context-curated` Event (contract §5.2); `retire` is the count of retirements proposed; `remaining` the records in force still uncurated. `steps` is present with `run` (hs-d538): the Tiers tried in order, e.g. `["H0", "H1"]`; `run` is that of the **last** step. `run` is present only when pi ran (`"distilled": true`); its fields are the run's `runs.jsonl` line without `started_at`, `context`, `upto_seq`, `result` and `kind`, and `tokens_in`, `tokens_out` and `cost` may be `null`. On a failure the same figures are in the log line only, not in the Event.

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

Effects: **rows and Events**, one transaction with the checks and the UPDATE on `contexts.owner`. One `context-owner` Event `{owner}` appended for each change; none for `already`. Nothing else: the Context's existing PRs keep their `name`, and from now on resolve `Repo(<owner>, <name>)` (or `Owner(<owner>)` while their `name` is null, contract §8.3). Sessions already started keep the policy text they were given for the old owner until they restart (contract §8.3 'Delivery to agents').

```json
{"ok": true, "context": {"id": "building-cpd", "owner": "clayton-duarte"}, "already": false}
```

Idempotent: yes; the same owner again returns `"already": true` and writes nothing.

### 3.15 `context delete`

```
cpd context delete <id> [--yes]
```

Deletes a Context: its Plans' rows, its records, its Manager, and its run files, worktrees and local branches (hs-d527). The `contexts` row, and the row of each of its Plans, stay as **tombstones** (`deleted_at` set, once only), and so do the Leads' Session rows, which the tombstoned Plans reference: all hidden from every list, view and lookup (contract §2.2 "Deleted", which has the design). Deletion is permanent and cannot be undone. Without `--yes` the command prints what would be deleted (a preview); with `--yes` it deletes.

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
   - Every Inbox item of the Context, then every Event of the Context (those of its Plans, Plans deleted earlier included, and those that have no Plan); Inbox items go first because they point to Events
   - For each Plan: its Escalations, Attempts, Dependencies, Jobs; its PR needs, PRs, Plan reviews; every Session of it but the Lead
   - Every `record_retirements` and `record_curations` row of the Context (hs-d549; they point to the records, so they go first), then every `context_records` row of the Context (a Plan delete keeps them; `context_records_immutable` still refuses every update)
   - The Manager Session
   - The Leads' Session rows stay (each tombstoned `plans.lead_session` references one), hidden with their Plan.
   Every `DELETE` writes a `history` row with the whole old row (§4.4); the tombstones are `update` rows. They stay, and the tamper check stays clean (contract §2.2). Last, in the same transaction, **one `context-deleted` Event** is appended, with the counts; it is never deleted by this delete.
2. **Session files** (after commit): every Session's file, the Leads' included; files that do not exist are skipped.
3. **Run files** (after commit): `$CPD_HOME/run/<plan>/` of each Plan and `$CPD_HOME/run/context-digests/<context>/`, with the summaries, renders, reviews, messages, specs, logs and distill artifacts under them.
4. **Worktrees and local branches** (after commit): `rm -rf` every Attempt worktree and PR Lead worktree, each with its `target/` (this, with the run files, is where the disk comes from: the database file does not shrink); then `git branch -D cpd/<plan>/<key>` and `git branch -D cpd/<plan>--<attempt>` from their repos (not on the remote), then the directories of step 3. The paths come from the live rows when the transaction commits, and on a rerun from their `before` copies in `history`.

**Rerun.** The rows go in one transaction and steps 2–4 after it, so a crash leaves a tombstoned Context with files on disk. A rerun with `--yes` on a deleted Context writes **nothing** to the store (no row, no Event), finishes whatever file step is left (a missing file, directory or branch is not an error) and returns `"already": true`. A preview of a deleted Context returns `"already": true` and what is left on disk, not the counts of the first preview, which only the Event keeps.

Output without `--yes` (preview) or with `--yes` after a delete:

```json
{"ok": true, "preview": true, "already": false, "plans": [{"id": "cpd2", "jobs": 47, "attempts": 121}], "jobs": 47, "attempts": 121, "sessions": 3, "worktrees": 11, "disk_freed_mb": 342}
```

`sessions` counts every Session of the Context, the Leads included (their rows stay, their files go). With `--yes` the first time: `"preview": false`, `"already": false`. Rerun with `--yes`: `"preview": false`, `"already": true`, and `worktrees` and `disk_freed_mb` of what this run removed (0 when nothing was left). Preview of a deleted Context: `{"ok": true, "preview": true, "already": true, "left": {"worktrees": 0, "branches": 0, "files": 0}}`.

Idempotent: yes; a Context already deleted returns `"already": true` and writes nothing to the store.

**Tests that assume otherwise change with this design** (hs-d527; for delete-core to have approved): `context_delete.rs` expects no Session with `context = <id>`, but the Leads' rows stay (tombstoned Plans reference them), and `plan_rows` as under `plan delete` counts the Leads and the Plans' `plan-deleted` Events; the Manager and every other Session are what is gone.

### 3.16 `context curation` (internal)

```
cpd context curation [--context <c>] [--limit <n>]
```

Lists what the distill step proposed and the User has not yet decided (contract §4.6 "Retirement"; hs-d549, hs-d551): the pending retirements and the pending doc candidates of the Context. Called by the cockpit when the User clicks a `curate-retire` or `curate-docs` Inbox item (§13.2), to fill the checklist dialog (hs-d568, hs-d569). Internal (§1.1): hidden in clap, as `plan decide` is. An archived Context or one under the floor (`context_curation_min_records`, §8.2) shows no Inbox item, its pending proposals stay pending, and the command still works when run by hand (R6 of hs-d592).

| Flag | Meaning |
|---|---|
| `--context <c>` | the Context (§1.3) |
| `--limit <n>` | at most n entries per list (hs-d569 CD4), where n >= 1 (default: no limit; all entries) |

| Precondition | Code | Exit |
|---|---|---|
| `CPD_SESSION` and `CPD_ATTEMPT` are not set in the environment: only the User decides (hs-d551). The cockpit's views and popups carry neither: cpd's tmux client removes `CPD_SESSION` and `CPD_ATTEMPT` along with `TMUX` and `TMUX_PANE` (contract §6.1 "The one place, in Rust"), so even a tmux server started from inside an agent never passes them on, and only the panes of a Session (`session run`, contract §6.5) get `CPD_SESSION`, through their `Spawn` | `user_only` | 3 |
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

Decides pending retirements (contract §4.6 "Retirement"; hs-d549, hs-d551). Called by the cockpit when the User approves the checklist dialog of a `curate-retire` item (§13.2): `--approve` the ticked rows, `--reject` the unticked. Internal (§1.1): hidden in clap. Only the User decides: no agent runs it. An archived Context or one under the floor shows no Inbox item, its pending proposals stay pending, and the command still works when run by hand (R6 of hs-d592).

| Flag | Meaning |
|---|---|
| `--context <c>` | the Context (§1.3) |
| `--approve <id,...>` | record ids (§2.1), comma-separated, whose pending retirement becomes `approved`: the record leaves force |
| `--reject <id,...>` | record ids whose pending retirement becomes `rejected`: the record stays in force and is never proposed again |

At least one of the two is required (`usage`).

| Precondition | Code | Exit |
|---|---|---|
| `CPD_SESSION` and `CPD_ATTEMPT` are not set in the environment (as §3.15) | `user_only` | 3 |
| a Context was selected, and exists | `context_required` (2), `context_not_found` (4) | |
| no id is in both lists | `args_invalid` | 2 |
| every id has a `pending` retirement in the Context; `details: {record}` names the first that has none (none at all, already decided) | `curation_not_pending` | 3 |

Effects: **one transaction**, all or nothing (a refusal writes nothing). **Rows:** each named `record_retirements` row gets its `status`, `decided_at` and `decided_by` `user`. **Event** (one): `records-retired` `{context, approved: [id], rejected: [id]}` (contract §5.2). A retirement writes no Context record. The Inbox item `curate-retire` follows at the engine's next E8 pass (contract §3.7): its count drops, and it closes when none is pending.

```json
{"ok": true, "context": "building-cpd", "approved": ["hs-d12"], "rejected": ["hs-d30"]}
```

Idempotent: no. A rerun finds the rows decided and is refused `curation_not_pending`.

### 3.18 `context docs` (internal)

```
cpd context docs [--context <c>] [--send <id,...>] [--dismiss <id,...>]
```

Decides pending doc candidates (contract §4.6 "Retirement"; hs-d549, hs-d551). Called by the cockpit when the User approves the checklist dialog of a `curate-docs` item (§13.2): `--send` the ticked rows, `--dismiss` the unticked. Internal (§1.1): hidden in clap. Only the User decides: no agent runs it. An archived Context or one under the floor shows no Inbox item, its pending proposals stay pending, and the command still works when run by hand (R6 of hs-d592). A dismissed candidate is never proposed again (R9 of hs-d592).

| Flag | Meaning |
|---|---|
| `--context <c>` | the Context (§1.3) |
| `--send <id,...>` | record ids whose `doc_candidate` becomes `sent`; they go to the Context's Manager in **one** board post, below |
| `--dismiss <id,...>` | record ids whose `doc_candidate` becomes `dismissed`; a dismissed candidate is never proposed again |

At least one of the two is required (`usage`).

| Precondition | Code | Exit |
|---|---|---|
| `CPD_SESSION` and `CPD_ATTEMPT` are not set in the environment (as §3.15) | `user_only` | 3 |
| a Context was selected, and exists | `context_required` (2), `context_not_found` (4) | |
| no id is in both lists | `args_invalid` | 2 |
| every id has a `pending` doc candidate in the Context; `details: {record}` | `curation_not_pending` | 3 |
| with `--send`: the Context has a live Manager Session (role `manager`, not archived) | `manager_missing` | 3 |

Effects: **one transaction**, all or nothing. **Rows:** each named `record_curations` row gets `doc_candidate` `sent` or `dismissed`. With `--send`, **one board post** (`board post` mechanics, §10a.1, `core/src/board.rs`) to the Context's Manager Session, `from_session` null (the User's side), listing each sent record's id, short form and paths and asking the Manager to plan a docs PR for them (it picks the post up at its next turn start, as every post, and no turn starts, hs-d514). The text is cut at the board's limit of 2000 characters: the lines of the records that do not fit are named by id alone, so the post is always written. **Event** (one): `docs-decided` `{context, sent: [id], dismissed: [id]}` (contract §5.2); the board post appends none. With no Manager, `manager_missing` is refused before anything is written.

```json
{"ok": true, "context": "building-cpd", "sent": ["hs-d300"], "dismissed": [], "manager": "a41b…"}
```

Idempotent: no. A rerun finds the candidates decided and is refused `curation_not_pending`.

## 4. `plan`

A Plan (§2.2) and its phases (§3.1). Replaces cpd 1's `workflow` group (D3) and `plan` group.

### 4.1 `plan new`

```
cpd plan new <id> --context <c> --title <text> [--jira <key>] [--name <name>] [--brief-file <file>] [--by-user]
```

A Plan has no repo (R34, A12): its PRs are added later with `pr add` (§9.4), and its Lead starts in the caller's cwd, the directory `plan new` runs from (usually `~`; §6.4). `--repo` and `--base` are gone: either is `usage` (exit 2).

| Flag | Meaning |
|---|---|
| `<id>` | Plan id (§2.1) |
| `--context <c>` | required: the Plan's Context (D2) |
| `--title <text>` | required; 1–200 characters, no control characters, else `usage` (§1.5) |
| `--jira <key>` | Jira task key, stored only |
| `--name <name>` | the Lead's pi display name; default `<id>` |
| `--brief-file <file>` | the brief for the Lead (B3-A4): written as a Context record of kind `brief` and put into the Lead's first message in full |
| `--by-user` | (hs-d542 A2) when registering a new Lead, stores the flag on the Session and writes no first message (§1.4); `plan new` and `context manager` use it (§3.10); omit it and the Lead starts with its first message, as today |

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| the Context has an owner (contract §8.3; null only for a Context made before the owner existed); the message names `cpd context owner <login> --context <c>` | `owner_unset` | 3 |
| no Plan with this id, unless every flag matches it (then the result is `already`) | `plan_exists` | 3 |
| the Context is not archived; `details.next` = `cpd context unarchive <c>` | `archived` | 3 |

Effects, in this order:
1. **Rows**, one transaction: `sessions` (new UUID v4; `role = lead`; `context`; `plan = <id>`; `name`; `cwd` = the realpath of the caller's cwd; `tier = lead_tier`; `forked_from` null; `by_user` = 1 when `--by-user`, else 0; `state = stopped`: the Lead is always fresh, never a fork, B3-A3); `plans` (`phase = grounding`, `lead_session` = the new Session); no `prs` row. **Events** `plan-created` `{title, jira, lead_session, forked_from: null, cwd}` and `session-registered` `{role, name, forked_from, tier}`.
2. **Context records:** the record write of §3 (one transaction) with kind `plan` (letter `p`), `text` = the title, `plan` = `{id, machine, jira}` (`jira` omitted when null); **Event** `context-record`. With `--brief-file`, a second record of kind `brief` (letter `b`), `text` = the file's text, `plan` = `{id, machine}`; **Event** `context-record`.
3. **First message**, when `by_user` is 0, written to `run/<id>/lead-message.md` (contract §4.6 "Consumers", hs-d655): a header (`You are the Lead of Plan <id> "<title>" in Context <c>. Phase grounding: follow the cpd-lead skill.`), the full brief (the file's text) if given, then the pointer line:
   ```
   Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>
   ```
   The Lead gets on-demand access to the Context through these commands. The Lead is always fresh (B3-A3): the grounded Lead and its forks are retired (contract §3.1), so there is no fork case. When `by_user` is 1, no file is written (hs-d542 A2).
4. **tmux:** start the server if it is not running (§6.1) and the cockpit session `cpd` if it is missing; then `new-window -d -t =cpd: -n park-<id> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -e CPD_PLAN=<id> -P -F '#{window_id} #{pane_id}' -- cpd session run <session>`, and `set -p -t <pane> @cpd_session <session>`. One transaction: `sessions.tmux_pane`; **Event** `session-moved` `{pane, window, place: "parked"}`. If any step after `new-window` fails (`set-option -p @cpd_session`, the row or the Event write), the new window is killed (`kill-window`) before the error is returned (teardown G11).
5. In the pane, `session run` (§10.8) starts a fresh Lead; when `by_user` is 1, it waits for the User's first prompt before starting pi (contract §10.8 step 3, hs-d542 A2).

When the cockpit session is missing, plan new creates it with the new Lead's park window as its first window; cpd tui builds main later.

```json
{"ok": true, "plan": {"id": "pay-4521", "context": "payments", "title": "Retry failed payouts", "jira": null, "phase": "grounding", "lead_session": "8d0e…", "prs": []},
 "lead": {"session": "8d0e…", "name": "pay-4521", "forked_from": null, "cwd": "/home/clayton", "pane": "%41", "window": "@17"},
 "record": "hs-p4"}
```

Idempotent: yes. With the same flags it returns `"already": true` and redoes any step that did not happen: the `plan` record when the Context has none for this Plan, the `brief` record when `--brief-file` is given and the Context has none for this Plan, and step 4 when the Lead is `stopped` with no pane (in the stored `cwd`). The caller's cwd is not a flag: a rerun from another directory is still `already` and keeps the stored `cwd`. Different flags: `plan_exists`. The `--by-user` flag is part of 'the same flags': a rerun with or without it when the Plan exists is `plan_exists` (hs-d542 A1).

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

Output: every field of the Plan (§2.2), `outline` included (hs-d502: the outline object of §4.13, parsed from `plans.outline`, or `null`); `prs` (R34, A14), every PR of the Plan in key order, `cancelled` ones included, each with its fields (§2.2 PR: `key`, `repo`, `base`, `base_pr` (the base PR as `<plan>/<key>`, derived from `base`, or null: hs-d324), `needs` (PR references: a bare key of this Plan, `<plan>/<key>` of another), `status`, `lead_branch`, `lead_worktree`, `pr`, `pull_request` (the row, as in §4.2, `mergeable` and `merge_state` included, or null), `feedback_open` (integer: the number of open feedback items in the row's stored list, contract §2.2 "Feedback item"; null when the list is NULL, never read; pro-d68), `all_merged_at`, `start_failed_at`, `created_at`, `started_at`, `merged_at`) and `jobs`, the keys of its Jobs in `seq` order; `jobs` with each Job's fields (Spec included, `pr`, `review` and `urgent` among them), `needs`, and its Attempts (`id`, `number`, `tier`, `origin`, `status`, `outcome`, `cause`, `worker_session`, `started_at`, `ended_at`); `escalations` that are `open`; the latest Plan review; `counts`; `tamper` (the tamper report of §4.3: `{kind: "schema" | "fingerprint" | "history", …}`); `events_cursor`.

```json
{"ok": true,
 "plan": {"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust", "jira": null, "phase": "dispatch", "lead_session": "596d007f-…", "created_at": "…", "locked_at": "…", "done_at": null, "archived_at": null, "outline": null},
 "prs": [{"key": "cpd", "repo": "/home/clayton/projects/cpd", "base": "main", "base_pr": null, "needs": [], "status": "dispatch", "lead_branch": "cpd/cpd2/cpd", "lead_worktree": "/home/clayton/projects/cpd.cpd2.cpd",
          "pr": null, "pull_request": null, "all_merged_at": null, "start_failed_at": null,
          "created_at": "…", "started_at": "…", "merged_at": null, "jobs": ["contract-core", "contract-commands"]},
         {"key": "web", "repo": "/home/clayton/projects/web", "base": "main", "base_pr": null, "needs": ["cpd"], "status": "waiting", "lead_branch": "cpd/cpd2/web", "lead_worktree": null,
          "pr": null, "pull_request": null, "all_merged_at": null, "start_failed_at": null,
          "created_at": "…", "started_at": null, "merged_at": null, "jobs": ["web-client"]}],
 "jobs": [{"key": "contract-commands", "pr": "cpd", "title": "cpd 2 contract: every command", "status": "running", "tier": "S3", "appended": false, "review": false, "needs": ["contract-core"], "fingerprint": "9f…", "retry_tier": null,
           "goal": "…", "context": "…", "touches": ["docs/commands.md"], "acceptance": ["…"], "out_of_scope": ["…"],
           "attempts": [{"id": "contract-commands-a1", "number": 1, "tier": "O3", "origin": "ladder", "status": "running", "outcome": null, "cause": null, "worker_session": "c1a2…", "started_at": "…", "ended_at": null}]}],
 "escalations": [{"id": "contract-commands-a1-q1", "attempt": "contract-commands-a1", "question": "…", "asked_at": "…"}],
 "review": {"id": "r2", "status": "approved", "fingerprint": "9f…", "current": true},
 "counts": {"draft": 0, "queued": 0, "running": 1, "merged": 1, "failed": 0, "cancelled": 0},
 "tamper": [], "events_cursor": 48213}
```

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

Effects: writes `run/<plan>/plan.md` and `run/<plan>/summary.md` (§1.4), nothing else. Content of `plan.md`, kept from cpd 1 §11 `plan render` and grouped by PR (R34): title, Context and phase; a table of the PRs that are not `merged`, with key, repo, base and needs, each row muted and marked `cancelled` when the PR is `cancelled` (hs-d518), with its `status` and `cancel_reason` or `superseded by <ref>` when the Event carries `superseded_by`; and, when there are two or more, a Mermaid `graph LR` of their needs (cancelled PRs not in the graph, hs-d518); then the section `## Overlaps` (below); then one section per PR that is not `cancelled` (hs-d518), in the order of the needs graph (ties by key), headed `PR <key>`, with its repo, base and needs, a table of its Jobs with key, title, Tier, status, needs and touches, each row muted and marked `cancelled` when the Job is `cancelled` (hs-d518), with its `status` and `cancel_reason` (hs-d518), a Mermaid `graph TD` of its Jobs' Dependencies (cancelled Jobs not in the graph), and one subsection per Job that is not `cancelled` (hs-d518) with its Spec. The Tier column and each Job section's `Tier:` line show the Tier the Job is displayed at (contract §3.4a): the active Attempt's Tier, or `retry_tier` while queued at it after `job retry`, or the latest Attempt's Tier, or the stored `tier`. The `open` Job placeholder keeps `open_tier`. **Waiting display** (hs-d521): a Job or PR table row whose status is `waiting` (display only, hs-d523) shows the blockers as `waiting on <keys>` in its status column or as a separate line (hs-d521); in the Mermaid graph (hs-d518), a cancelled PR is omitted, and in the render's graph of needs a cancelled PR appears neither as a node nor in edges; in the per-PR Job graph, cancelled Jobs do not appear. **The open Job's placeholder** (hs-d497): a PR that has not started has no `open` Job yet (engine E4 adds it at the start), so the render shows one for each such PR, as the last row of its Jobs table and as the last Job section: key `open-<pr>`, title `Open the pull request: <PR title>`, Tier `open_tier` (contract §8.2), status `—` (not a Job yet), needs `every other Job of the PR`, and, in place of the Spec, `Added by cpd when the PR starts: merges the base only on a conflict, runs the Gate, pushes and opens the pull request ready.` It is not a row, is in no fingerprint and no Mermaid graph, and is shown in every phase until the PR starts (once the PR has started the real `open` Job is shown as any Job, with its derived needs, contract §3.2). A Job's section is under its PR only. In a phase after `plan`, the Jobs that are not `draft` are Locked context: the render shows them as such, and marks every `draft` Job **under review** except a `draft` `open` Job, which cpd added and the Lead does not review: it is shown as context, never `**under review**` (hs-d573) (in its table row and its section), because a review in these phases covers exactly the drafts (§4.6; User, hs-d173, hs-d174, hs-d179). In phase `plan` every Job is a draft and nothing is marked. A **stub draft** (no Tier or no Spec, §5.1) is shown as it is, visibly incomplete: `Tier: —` in its table row and section when it has no Tier, and `Spec: not written yet` in place of its Spec subsection's goal, context, touches, acceptance and out of scope when it has none. GitHub-flavoured Markdown only: tables, code and Mermaid, no footnotes, no raw HTML (t4). It also writes the **Plan summary**, `run/<plan>/summary.md` (below, hs-d502). Output: the path, `summary` (the path of the summary), the plan fingerprint (§2.3: Jobs, PRs and the outline when set), the number of `draft` Jobs (`draft_jobs`, not counting a `draft` `open` Job: it is not under review, hs-d573), the number of PRs that are not `cancelled` and the overlaps.

**Kinds** (hs-d340, hs-d341). A Job's `kind` is a column of its row in the PR's table, and a line of its section when it is not `work`; a `test` Job's section lists its `tests` and `test_files`. The PR's `open` Job, once the PR has started, is a Job of its PR like any other, with its derived needs (contract §3.2); before the start it does not exist, so the review of a Plan shows the template's Jobs and the `open` Job as `plan render`'s placeholder (§4.5, hs-d497), the one every PR will get.

**Overlaps** (B4-A3, hs-d78, hs-d172, hs-n201): the warning that another Plan is working on the same files. It is made here and so in the Plan review (§4.6), and nowhere else: `job add` does not warn. It looks at **open work** only, on both sides: the Jobs that are not `cancelled` (`draft`, `queued`, `running`, `failed`, or `merged` on its Lead Branch) of PRs that are not `merged` and not `cancelled`. A Job of a `merged` PR is on its base already and cannot clash with anyone's work any more, so the Jobs of this Plan's `merged` PRs are not checked. For each PR of this Plan that is not `merged` and not `cancelled`, every **other** active Plan (any Context; its phase is not `done` and it is not archived) is looked at when it has a PR in the same repo (the same `prs.repo` realpath as stored) that is not `merged` and not `cancelled`. A Job of that PR overlaps a Job of this PR when a path in the `touches` of the one overlaps a path in the `touches` of the other. Two paths overlap when they are equal, or when one is a directory (ends with `/`) that contains the other. Nothing else is compared (no globs, no normalisation). A Plan's own Jobs are never listed. The warning is only that: nothing is refused and no state changes.

The overlaps are grouped: **one overlap per (this Job, other Plan, other PR)**. It lists every Job of that other PR that overlaps this Job, with its status, and the paths of this Job that overlap one of them, each path once, in the order of the Job's `touches`. A broad path (`core/src/`) or a shared file is one entry of a row, not a row of its own for every pair of paths and Jobs (hs-n201: on a busy Context that made thousands of rows, and nobody could read them in a Plan review).

`plan.md` has the section `## Overlaps` after the PRs table (and its graph), before the PR sections: a table with one row per overlap, ordered by this Job's key, then the other Plan, then the PR; or, when there is none, the line `No other active Plan touches these files.` A Plan with no PR yet has no PRs table and no section.

```
## Overlaps

| Job | Other Plan | PR | Its Jobs (status) | Paths |
|---|---|---|---|---|
| `x` | `pay-4522` | `api` | `y` (queued), `z` (draft) | `core/src/x.rs`, `core/src/lib.rs` |
```

The columns are this Plan's Job, the other Plan, its PR, the Jobs of that PR that overlap this Job with their status (in the order they were created), and the paths of this Job that overlap them. `Paths` shows at most five paths and then `+N` for the N more, as in `` `a`, `b`, `c`, `d`, `e`, +2 ``. In the JSON output, `overlaps` lists the same rows, in the same order, as `{job, plan, pr, other_jobs: [{key, status}], paths}`: `other_jobs` are the Jobs of the other PR that overlap `job`, and `paths` are all the paths of `job` that overlap them, deduplicated (only the table is capped); `[]` when there is none.

```json
{"ok": true, "path": "/home/clayton/.cpd/run/pay-4521/plan.md", "summary": "/home/clayton/.cpd/run/pay-4521/summary.md", "fingerprint": "9f86d081…", "draft_jobs": 6, "prs": 2,
 "overlaps": [{"job": "x", "plan": "pay-4522", "pr": "api", "other_jobs": [{"key": "y", "status": "queued"}, {"key": "z", "status": "draft"}], "paths": ["core/src/x.rs", "core/src/lib.rs"]}]}
```

Idempotent: yes (the same PRs and Jobs, the same outline and the same other Plans, render the same two files). Change: cpd 1's `--out` is dropped; the file has one place.

**The Plan summary** (hs-d502, D4; User, 2026-10-20, hs-d502 supersedes hs-d468). A short text cpd generates from the Plan and its outline (§4.13), so the User approves a Plan from a screenful in the Lead's chat instead of reading the render (§15.5). `plan render` writes it to `run/<plan>/summary.md` and `plan review` (§4.6) writes the same text, with the review's heading, to `run/<plan>/summary-<review>.md`. It is derived and holds no information the Plan lacks: it carries no Spec, no Mermaid and no row for a Locked Job. GitHub-flavoured Markdown only (§4.5 above). Deterministic: the same Plan, outline, overlaps, review and path give the same bytes.

**Vocabulary.** A Job **under review** is a `draft` Job that has a Tier and a Spec, a `draft` `open` Job excepted (cpd's, shown as context, hs-d573); a **stub draft** is a `draft` Job without one of them (§5.1). A Job is **locked** when it is neither `draft` nor `cancelled`. A PR **counts** when it is not `cancelled`. Keys sort bytewise.

**Layout.** The file is a sequence of **blocks**, in the order below, separated by exactly one blank line, and ends with one newline. A block that has nothing to say is left out, except the first, the Goal, the Jobs block and the last. A block is one or more consecutive lines.

| # | Block | Lines |
|---|---|---|
| 1 | heading | `# <review title>`, then `Plan <plan id>: <Plan title> · review <review id>`. In `summary.md` (no review): `# Plan: <Plan title>`, then `Plan <plan id> · phase <phase>` |
| 2 | goal | `Goal: <goal>` from the outline; `Goal: —` when the Plan has no outline |
| 3 | PRs | `## PRs`, then for each PR that counts and has a Job under review, in key order, `- <key> (<repo name>, base <base>): <PR title>` and under it one line `  Done when: <line>` (two leading spaces) per line of the outline's `done_when[<key>]`, in order, none when the outline is null or has no entry for it. `<repo name>` is the PR's `name` (§2.2 PR), else the last path segment of its `repo`; `<base>` the PR's `base`; `<PR title>` the PR's `title`, else `—` |
| 4 | decisions | `## Decisions`, then `D<n> <question> → <answer>` per outline decision, `n` from 1 in the outline's order |
| 5 | assumptions | `## Assumptions`, then `A<n> <text>` per outline assumption, `n` from 1 |
| 6 | Jobs | `## Jobs (<n> under review, <m> locked)`, `n` the number of Jobs under review and `m` the number of locked Jobs, then a table with the columns `Job`, `Title`, `Tier`, `Needs`, one row per Job under review in PR key order, then Job key order: its key, its title, its Tier, and its `needs` joined by `, ` (`—` when none). The table has its header and separator rows even with no Job under review. No locked Job has a row |
| 7 | order | `Order: <items>`, the items joined by `, `: one `<need> → <key>` for every need of a PR that counts, whose need is a PR that counts too, in key order of the PR and then of the need; then, as the last item, the PRs that count and neither need nor are needed by any other, joined by ` · ` (no such item when there is none). Then the line `Stacked on: <entries>`, one `<key> on <base PR>` per PR that counts and has a base PR (`base_pr` of `plan show`, §4.3: `<plan>/<key>`), in key order, joined by `, `; `none` when none is stacked. Both lines are left out when no PR counts |
| 8 | overlaps | `Overlaps: none` when §4.5's overlaps are none; else `Overlaps: <n>`, `n` the number of overlaps (the rows of §4.5's table), then one row per overlap, in §4.5's order and at most 5: `- <path>: <other plan>/<other pr>/<other job> (<status>)`, the first of the overlap's paths and the first of its other Jobs with its status, followed by ` +<k>` when the overlap has more (`k` = the other paths plus the other Jobs); when `n` is over 5, a last row `- +<n-5> more overlaps` |
| 9 | decided | `Decided: <ids>`, the outline's `decided` record ids joined by `, `; left out when there are none |
| 10 | later | `Later: <items>`, the work this review does not cover, joined by `, `: the keys of the stub drafts in key order, then `PR <key>` for each PR that counts and has no Job under review, in key order; left out when there are none |
| 11 | risks | `## Risks`, then `R<n> <text>` per outline risk, `n` from 1 |
| 12 | out of scope | `## Out of scope`, then `- <text>` per outline entry |
| 13 | last line | for a review: `Approve \| Change \| Open full plan (<path>)` (the pipes literal), `<path>` the absolute path of `plan-<review>.md`; for `summary.md`: `Full plan (<path>)`, the absolute path of `plan.md` |

Blocks 4, 5, 9, 11 and 12 come from the outline and are left out when it is null or the list is empty. The sizes are bounded by the outline's own limits (§4.13: at most 3 risks, 5 out-of-scope entries, 3 done-when lines per PR) and by the overlap rows above; the decisions, assumptions and Jobs are as many as the Plan has. A `done_when` key of a PR that was cancelled after `plan outline` is not shown (the block lists PRs that count).

```
# Plan review in chat
Plan plan-ux: Plan UX · review r2

Goal: Show a Plan review as a short summary in the Lead's chat, approved in one dialog

## PRs
- cpd (cpd, base main): Summary, review in chat and outline
  Done when: plan review writes summary-<review>.md from the outline
  Done when: the adapter shows it in chat with Approve, Change and Open full plan

## Decisions
D1 Where is the review shown? → In the Lead's chat; plannotator only on request

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
- Changing plannotator

Approve | Change | Open full plan (/home/clayton/.cpd/run/plan-ux/plan-r2.md)
```

### 4.6 `plan review`

```
cpd plan review [--plan <id>] --title <text>
```

`--title` is **required** (hs-d291): it names what the drafts under review do, so the User sees it as the heading of the review, in the summary in the Lead's chat and in the full render. It has the rule of a PR's title: 1–72 characters, one line, no control characters; a bad or missing one is `usage` (exit 2). It is stored as `plan_reviews.title` (contract §2.2).

Requests the User's approval of the draft Jobs (§2.2 Plan review). In phase `plan` the review covers the whole plan, as before; in every phase after `plan` it covers **exactly the draft Jobs** the Lead added: its fingerprint is over those drafts only (§2.3; a `draft` `open` Job is not one of them, so engine E4 adding one never voids a pending review, hs-d573), and the rendered plan marks them under review and shows the Locked Jobs as context (§4.5). A Lead that planned several batches adds the later ones as drafts (`job add --draft`, §5.1) and asks for one review per batch. The Lead runs it through its tool `cpd_plan_review` or directly; either way the pi adapter shows the summary in the Lead's chat, with the approval dialog, when the `plan-review` Event arrives (§15.5), so the review opens from any caller, in whatever phase the Plan is. The User approves, asks for a change, or opens the full render in plannotator (hs-d502).

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan`, or any phase after it (not `grounding`) | `illegal_phase` | 3 |
| at least one Job is `draft` (in a phase after `plan` too: the drafts are what is reviewed; a `draft` `open` Job does not count, it is not under review, hs-d573) | `no_draft_jobs` | 3 |
| every `draft` Job has its Tier and its Spec (hs-d241, R1: no stub draft is approvable); `details.jobs` lists `[{key, missing}]`, `missing` being `["tier"]`, `["spec"]` or both | `draft_incomplete` | 3 |
| every `impl` Job that is a `draft` needs at least one `test` Job of its PR (hs-d341; `details.jobs` lists the `impl` Jobs) | `test_job_required` | 3 |
| every PR of shape `tdd` that has not opened has at least one `test` and one `impl` Job that is not `cancelled` (hs-d497, contract §2.2 "Job kinds"; `details` `{pr, missing}`) | `shape_incomplete` | 3 |
| the Plan has an outline (§4.13; hs-d502, D3); `details.next` = `cpd plan outline --file <outline.json>` | `outline_missing` | 3 |
| every PR that has a `draft` Job (under review, since the previous row passed) has an entry in the outline's `done_when`; `details.prs` lists the PR keys without one, in key order | `outline_incomplete` | 3 |

Effects:
- **Files:** renders the plan to `run/<plan>/plan.md` and `run/<plan>/summary.md` (§4.5) and copies them to `run/<plan>/plan-<review>.md` and `run/<plan>/summary-<review>.md`, so a later render cannot change what the User reviews. The summary copy is the Plan summary of §4.5 with the review's heading (block 1) and its last line (block 13), and no other difference; it is what the User reviews (§15.5), and the full copy is shown only when the User asks for it. The copy opens with `# <review title>`, then one line `Plan <plan id>: <Plan title> · review <review id>`, then the rendered content (hs-d291); `plan.md`, made without a review (`plan render`, §4.5), keeps `# Plan: <Plan title>`. The reviewed file carries the `## Overlaps` section of §4.5, as it is at that moment, so the User sees the warning in the review: one row per open Job of this Plan and per other PR that overlaps it, short enough to read (hs-n201); it is a warning only and the review is requested all the same.
- **Closing other reviews:** every other pending review of the Plan (always older) closes `superseded` in the same transaction that inserts the new review request (the new `pending` Event comes after their `superseded` Events). Their Events say why: "Plan review r<k> of Plan P is superseded by r<k+1>." When the latest review is already pending for the current fingerprint (the `already: true` path) nothing is written, nothing closes.
- The refusals `outline_missing` and `outline_incomplete` come **before anything is written** (D3): no render, no file, no row, no Event.
- **Rows**, one transaction: `plan_reviews` (`id = r<k>`, `k` = 1 + the highest, `fingerprint` = the plan fingerprint (§2.3: it includes the outline), `title` = `--title`, `status = pending`). **Event** `plan-review` `{review, fingerprint, status: "pending"}`.

```json
{"ok": true, "review": {"id": "r3", "fingerprint": "9f86d081…", "status": "pending", "requested_at": "…"}, "path": "/home/clayton/.cpd/run/pay-4521/plan-r3.md", "summary": "/home/clayton/.cpd/run/pay-4521/summary-r3.md",
 "next": "The User reviews the plan summary in your chat and decides there. Stop and wait for the decision message."}
```

**Stubs** (hs-d241, R1). A draft without its Tier or its Spec (a stub, §5.1) cannot be approved, so the review is **not requested**: it is refused `draft_incomplete` (exit 3) with `details.jobs` = `[{"key": "api-1", "missing": ["spec"]}, …]`, in key order, `missing` holding `"tier"` and/or `"spec"`. The same refusal applies through the tool `cpd_plan_review`. Nothing is rendered or written. The Lead completes each stub with `job edit --tier --spec` (§5.2) or cancels it, then asks again. The plan fingerprint covers a stub as it is (contract §2.3), so completing one changes it.

Idempotent: yes. When the latest review is `pending` for the current fingerprint, it returns that review with `"already": true` (its `path` and `summary` too) and writes nothing; the two refusals above are still checked first, so a Plan with no outline is refused even when a review made before the outline existed is pending. When the Jobs or PRs changed since, it requests a new review and closes the older one `superseded`; the older one stays in the database, closed.

### 4.7 `plan decide` (internal)

```
cpd plan decide <review> (--approve | --reject) [--feedback-file <file>] [--plan <id>]
```

Records the User's decision. Called by the pi adapter with the result of the chat dialog or of plannotator (§15.5); replaces cpd 1's `plan decision` (cpd 1 §11), whose `--pending` step is now `plan review`. The cpd-lead skill forbids the Lead to run it. Internal (§1.1): hidden in clap, run only by the pi adapter.

| Precondition | Code | Exit |
|---|---|---|
| the review is `pending` (decided the same way: `already`); a review with status `approved`, `rejected`, or `superseded` is refused `review_decided` | `review_decided` | 3 |

Effects: **rows**, one transaction: `plan_reviews.status`, `feedback` (the file's text, or null), `decided_at`. **Event** `plan-review` `{review, fingerprint, status, feedback}`.

In a phase after `plan`, an **approval** also queues the drafts the review covered (§2.2, §3.2), in the same transaction:
- **Rows:** every `draft` Job of the Plan becomes `queued` and gets its Locked-Job fingerprint (§2.3), as an appended Job has at `job add`; `appended` stays 1. A `draft` `open` Job is one of them: it is queued here with the others, so a PR's `open` Job is `queued` exactly when its other Jobs are (contract §3.7 E4, hs-d573), and the `jobs` of the Event lists it too. The engine schedules each as soon as it is Ready (§3.2). **A draft of an `open` PR** that becomes `queued` here is a `work` Job and reaches the pull request by its own merge (§5.1; contract §3.8 "Updating"); no `update` Job is queued.
- **Event:** the same `plan-review` Event, whose `data` gains `jobs: [key]`, the Jobs it queued, in key order. No new Event kind. `plan-phase` is not appended: the phase does not change.
- Only a **current** approval queues (*decided*: a stale one would queue drafts the User never saw): when the drafts changed since the review (`current` false), the decision is recorded and nothing is queued, `jobs` is absent and `next` asks for a new `cpd plan review`.
- A **rejection** queues nothing: the drafts stay drafts, editable, as in phase `plan`; the Lead revises them (`job edit`, `job add --draft`, `job cancel`) and asks again.
- In phase `plan`, `plan decide` writes only what it did above, and `plan lock` (§4.8) works as before.

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
| every PR of shape `tdd` has a `test` and an `impl` Job that is not `cancelled`, as `plan review` requires (§4.6; `details` as there) | `shape_incomplete` | 3 |
| the latest Plan review is `approved` for the current plan fingerprint (§2.3: Jobs and PRs) | `plan_not_approved` | 3 |
| at least one PR is not `cancelled`, and each such PR has at least one `draft` Job; `details.prs` lists the empty PRs (none at all: `details.next` = `cpd pr add`) | `pr_empty` | 3 |
| the Dependency graph of the draft Jobs is acyclic, and so is the graph of the PRs' needs and base PRs (`details.cycle`, qualified references) | `dependency_cycle` | 3 |
| for each PR with no needs and no base PR: its workflow worktree path does not exist, except a leftover of an interrupted lock (a worktree of the PR's repo on its Lead Branch), which is removed first; `details.pr` names the PR | `worktree_exists` | 3 |
| for each PR with no needs and no base PR: its Lead Branch does not exist in its repo, except that leftover's branch, and neither does `cpd/<plan>` when the Lead Branch is `cpd/<plan>/<key>` (§2.1); `details.pr` | `branch_exists` | 3 |
| for each PR with no needs and no base PR: `origin/<lead branch>` does not exist, or it does not conflict with `origin/<base>` (`git merge-tree`, §7.2, hs-d490; `details.files` the conflicted paths); a repo with no `origin` has no remote check; `details.pr` | `branch_exists` | 3 |
| for each PR with no needs and no base PR: `pre-start` passed in its new workflow worktree (§7.3); `details.pr`, `details.tail`, `details.log` | `setup_failed` | 1 |

Effects (§3.1, §3.8, §7.2; R34, A8): every PR's `draft` Jobs are approved at once, and the PRs with no needs and no base PR **start**, all or nothing; the PRs with needs, and the **stacked** PRs (contract §2.2 "The base PR"; a base PR of the same Plan included: the lock orders no worktrees), stay `waiting` until engine E7 (or `pr start`, §9.7) starts them once their needs are `merged` and their base PR is `open`.
- **git**, for each PR with no needs and no base PR, in its repo (§7.2, R35 per PR branch, A5): `fetch origin <base>`; `ls-remote --exit-code --heads origin <lead branch>`. When it is on the remote and does not conflict with `origin/<base>` (`merge-tree --write-tree --name-only` exits 0; a base that merely moved ahead is no conflict, hs-d490): reuse it, `fetch origin <lead branch>` and `worktree add --no-track -b <lead branch> <lead worktree> origin/<lead branch>`. When it is absent: `worktree add --no-track -b <lead branch> <lead worktree> origin/<base>`. A repo with no `origin` (A3): no remote check and no push, `worktree add --no-track` from `<base>`. `--no-track`: a Lead Branch has no upstream, so nothing writes `.git/config` (hs-d243, contract §7.2). Then `pre-start` in the worktree, output in `run/<plan>/logs/lead-<key>.pre-start.log`. Every started PR's worktree is created and its `pre-start` run before anything is pushed; if one fails, every worktree and branch the lock created is removed, nothing is pushed, nothing in the database changes, and the lock fails with that PR's refusal. When all have passed, each new Lead Branch is pushed, `push origin <lead branch>` from its Lead worktree (no `-u`, hs-d243). A failed push does not fail the lock: one `error` Event with code `push_failed` and `details.pr` is appended per failed PR, and the next merge or `pr open` pushes again.
- **Rows**, one transaction, with a compare-and-swap on `phase = 'plan'` and on the review read: `plans.phase = dispatch`, `locked_at`; every `draft` Job of every PR `queued` with its Locked-Job fingerprint; for each started PR, with a compare-and-swap on `status = 'waiting'`: `status = dispatch`, `lead_worktree`, `started_at`. **Events** `plan-phase` `{from: "plan", to: "dispatch", jobs: [key], prs: [key]}` (`prs`: the PRs started) and one `pr-started` `{pr, repo, base, lead_branch, lead_worktree, base_sha, reused, pushed, by: "lock"}` per started PR. If the transaction fails, the worktrees and branches are removed.
- **Open Jobs:** the lock adds none. Engine E4 adds each started PR's `open` Job in the first tick after the lock (contract §3.7, hs-d340).
- **tmux:** `watch --ensure` (§12): starts `cpd watch` in `cpd-work:watch` when `locks/watch.lock` is free.

`prs` lists every PR that is not `cancelled`, in key order: a started PR with its Lead worktree, `base_sha`, `reused` and `pushed` (R35); a `waiting` one with its `needs`, and null for the rest.

```json
{"ok": true, "plan": "pay-4521", "phase": "dispatch", "jobs": ["api-schema", "worker", "ui"],
 "prs": [{"key": "api", "status": "dispatch", "needs": [], "lead_branch": "cpd/pay-4521/api", "lead_worktree": "/home/clayton/projects/pay.pay-4521.api", "base_sha": "4b825dc…", "reused": false, "pushed": true},
         {"key": "ui", "status": "waiting", "needs": ["api"], "lead_branch": "cpd/pay-4521/ui", "lead_worktree": null, "base_sha": null, "reused": null, "pushed": null}],
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
| no object of the cascade is live: no Session `working`, `waiting` or `blocked`, no Job `queued` or `running`, no open Escalation, no pending Plan review, and no PR of the Plan, not `merged` or `cancelled`, that a PR of another Plan (itself not `merged` or `cancelled`) names as base PR or need (`kind` `pr`, `id` `<plan>/<key>` of the PR named; contract §2.2 "Archived", hs-d324); `details.blockers` lists each as `{kind, id, reason}`. The phase does not matter: a Plan in `deploy` is archivable, and the engine keeps polling its PR | `archive_blocked` | 3 |
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

Abandons a Plan (E1, hs-d203): cancels every Job that is not merged, closes the Plan's pull requests that are not merged, closes its Lead and its forks, and archives it. cpd never closes an adopted pull request (§9.4) and never deletes its branch: step 5 leaves it open and the output lists it under `left_open`. Branches and worktrees of cancelled PRs are removed by `pr poll` (the same clean-up as merged PRs, §9.2, hs-d517). `--reason` is kept in the Events' `reason`; default `plan cancelled`. The Plan may be in any phase, `done` included (nothing is left to cancel; it is then archived).

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
3. **Jobs**, as `job cancel` (§5.3) for every Job not `merged` and not `cancelled`, with the reason, **but no Job is refused `shape_incomplete`**, in particular not the last `test` or `impl` Job of a `tdd` PR (cancelling a Plan is never refused by the shape check, hs-d573; the skip is an internal parameter of the path, never a flag, option or environment variable): `draft`, `queued` and `failed` become `cancelled` with **Event** `job-cancelled`; a `running` Job gets `job-cancel-requested`, and engine E2 or the driver ends its Attempt `cancelled` at the next safe point (contract §3.4). The command then waits, polling, up to 60 s until no Attempt of the Plan is not `ended`. Dependents cannot start, as for `job cancel`; they are cancelled themselves by this step.
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
| the file is a JSON object with no unknown field, and every field has the shape and limits above. Checked in this order, the first failure being the result: not JSON, or not an object (`details.field` `$`); an unknown field (its name, the first in byte order); then `goal`, `decisions`, `assumptions`, `risks`, `done_when`, `out_of_scope`, `decided`, each element in order; `details.field` is the path of the failure, as `goal`, `decisions[1].answer`, `risks[3]`, `done_when.api[0]`, `decided[0]` | `outline_invalid` | 2 |
| the Plan exists | `plan_not_found` | 4 |
| the Plan is not archived (§1.3) | `archived` | 3 |
| every key of `done_when` is a PR of the Plan that is not `cancelled`; `details.field` = `done_when.<key>`, `details.key` = the key | `outline_invalid` | 2 |
| every id of `decided` is a record of the Plan's Context; `details.field` = `decided[<i>]`, `details.record` = the id | `outline_invalid` | 2 |

Effects:
- The same canonical JSON as stored: `"already": true` and **nothing written**, no row, no Event.
- Otherwise **rows**, one transaction: `plans.outline` = the canonical JSON. **Event** `plan-outlined` `{replaced}` (contract §5.2), `replaced` true when the Plan had an outline. Nothing else is written: no file, no tmux. The plan fingerprint changes (§2.3: it includes the outline), so a Plan review made before this write is no longer current, and the Lead asks for a new one (§4.6).
- The outline is checked when it is written only: a PR cancelled afterwards leaves its `done_when` entry in it, which the summary does not show (§4.5) and `plan review` does not need (§4.6).

```json
{"ok": true, "plan": "pay-4521", "replaced": false,
 "outline": {"assumptions": ["The outline is written by the Lead"], "decided": ["hs-d502"],
             "decisions": [{"answer": "In the Lead's chat", "question": "Where is the review shown?"}],
             "done_when": {"api": ["plan review writes summary-<review>.md"]},
             "goal": "Show a Plan review as a short summary in chat", "out_of_scope": ["Changing plannotator"], "risks": []}}
```

`outline` is the stored object. Idempotent: yes. The same file twice changes nothing and writes one Event only the first time (`"already": true` the second time, `replaced` absent).

### 4.14 `plan delete`

```
cpd plan delete <id> [--yes]
```

Deletes a Plan: its Jobs, Attempts and the other rows below, its Sessions but the Lead, and its run files, worktrees and local branches (hs-d527). The `plans` row stays as a **tombstone** (`deleted_at` set, once only), and so does the Lead's Session row, which the tombstone references (`plans.lead_session`): both hidden from every list, view and lookup (contract §2.2 "Deleted", which has the design). The Context's records are kept. Deletion is permanent and cannot be undone. Without `--yes` the command prints what would be deleted (a preview); with `--yes` it deletes.

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
   - Every PR need, PR and Plan review of the Plan
   - Every Session of the Plan but the Lead (Workers and forks); the Lead's row stays, with its `plan` unchanged, hidden with the Plan
   - The Context's records stay (hs-d527)
   Every `DELETE` writes a `history` row with the whole old row (§4.4); the tombstone is an `update` row. They stay, and the tamper check stays clean (contract §2.2). Last, in the same transaction, **one `plan-deleted` Event** is appended, with the counts; it is never deleted by this delete. The Plan's `pull_requests` rows are no Plan's: the poll deletes them once no PR points to them (§7.6 step 5).
2. **Session files** (after commit): every Session's file, the Lead's included; files that do not exist are skipped.
3. **Run files** (after commit): everything under `$CPD_HOME/run/<plan>/`: summaries, renders, reviews, messages, specs, logs and distill artifacts.
4. **Worktrees and local branches** (after commit): `rm -rf` every Attempt worktree and PR Lead worktree, each with its `target/` (this, with the run files, is where the disk comes from: the database file does not shrink); then `git branch -D cpd/<plan>/<key>` and `git branch -D cpd/<plan>--<attempt>` from their repos (not on the remote), then the directory of step 3. The paths come from the live rows when the transaction commits, and on a rerun from their `before` copies in `history`.

**Rerun.** The rows go in one transaction and steps 2–4 after it, so a crash leaves a tombstoned Plan with files on disk. A rerun with `--yes` on a deleted Plan writes **nothing** to the store (no row, no Event), finishes whatever file step is left (a missing file, directory or branch is not an error) and returns `"already": true`. A preview of a deleted Plan returns `"already": true` and what is left on disk, not the counts of the first preview, which only the Event keeps.

Output without `--yes` (preview) or with `--yes` after a delete:

```json
{"ok": true, "preview": true, "already": false, "jobs": 47, "attempts": 121, "sessions": 3, "worktrees": 11, "disk_freed_mb": 342}
```

`sessions` counts every Session of the Plan, the Lead included (its row stays, its file goes). With `--yes` the first time: `"preview": false`, `"already": false`. Rerun with `--yes`: `"preview": false`, `"already": true`, and `worktrees` and `disk_freed_mb` of what this run removed (0 when nothing was left). Preview of a deleted Plan: `{"ok": true, "preview": true, "already": true, "left": {"worktrees": 0, "branches": 0, "files": 0}}`.

Idempotent: yes; a Plan already deleted returns `"already": true` and writes nothing to the store.

**Tests that assume otherwise change with this design** (hs-d527; for delete-core to have approved): `plan_rows` of `cli/tests/flow/plan_delete.rs` counts Sessions with `plan = <id>` and Events with `plan = <id>`, but the Lead's row stays with its `plan`, and the `plan-deleted` Event names the Plan; the Jobs, Attempts, PRs, the other Sessions and the other Events are what is gone.

## 5. `job`

Jobs, Specs and Dependencies (§2.2, §3.2). Kept from cpd 1 §11 "Jobs" except where stated.

**Spec file** (`--spec`), JSON, every field required, unknown fields refused (`spec_invalid`, exit 2, with the JSON path in `details`):

```json
{"goal": "…", "context": "…", "touches": ["docs/commands.md"], "acceptance": ["…"], "out_of_scope": ["…"]}
```

`touches` and `acceptance` have at least one non-empty string; `out_of_scope` may be empty. Change: cpd 1's `outOfScope` is now `out_of_scope`, the column's name.

**A `test` Job's Spec file** (kind `test`, hs-d341) has two more fields: `tests`, the named tests (one or more distinct one-line strings), and `test_files`, the files that hold the red tests (one or more repo-relative paths or, ending in `/`, directory prefixes; no `..`, no leading `/`, the rule of the `protected` list, contract §7.5):

```json
{"goal": "…", "context": "…", "touches": ["cli/tests/it/pr_open_job.rs", "core/src/pr/"], "acceptance": ["…"], "out_of_scope": ["…"],
 "tests": ["pr_open_job::merges_the_base", "pr_open_job::opens_the_pull_request"], "test_files": ["cli/tests/it/pr_open_job.rs"]}
```

Both are required for a `test` Job that is not a stub, and refused for every other kind (`spec_invalid`, exit 2, with the JSON path in `details`). An `impl` Job names no tests: its tests are the union of its test Jobs' (contract §2.2 "Job kinds").

**Tier flag** (`--tier`): `^(O|S|H)([0-5])$` or exactly `GX`. `G0`–`G5` are `tier_invalid` (exit 2, `details.tier`), before any catalog check, with the message `invalid Tier "G0": G has no effort, write GX (hs-d562).` A Tier from the pattern with an effort the letter's model supports (§9) is valid; otherwise `tier_invalid` (exit 2), with `details.efforts` listing the valid ones. **For a Job's Tier, the rule of pro-d53 applies next** (the User's decision, 2026-10-26; contract §9 "Tier rules"): a Worker uses only H0–H4 and S0–S3, so the Tier must be `H0`–`H4` or `S0`–`S3`, else `tier_not_allowed` (exit 2, `details` `{tier, allowed: "H0-H4, S0-S3", rule: "pro-d53"}`), message `Tier <T> is not allowed for a Worker: Workers use only the Tiers H0-H4, S0-S3, to keep cost down (pro-d53, hs-d554)`. `job add`, `job edit`, `job retry` and `job dispatch` apply it, after `tier_invalid`; a stub draft's Tier is checked too. A store that already holds a Job on a refused Tier is not rewritten: §5.5 and contract §9 ("Stored Jobs and Plans") say what happens to it.

### 5.1 `job add`

```
cpd job add <key> --title <text> --tier <T> --spec <file> [--pr <key>] [--kind <docs|test|impl>] [--needs <k1,k2> | --review] [--draft] [--urgent] [--plan <id>]
cpd job add <key> --draft --title <text> [--tier <T>] [--spec <file>] [--pr <key>] [--kind <docs|test|impl>] [--needs <k1,k2>] [--plan <id>]
```

`--tier` and `--spec` are required, except with `--draft`: a **stub draft** (hs-d241, R1) needs only `--title`; either or both of `--tier` and `--spec` may be given too. A Job that is not a draft, and a `--review` Job, still needs both (`usage`, exit 2). The Lead registers planned work with a stub the moment it is planned (hs-d239) and completes it later with `cpd job edit <key> --tier <T> --spec <file>` (§5.2). A follow-up PR is added the same way: `pr add <key> --repo … --needs …` (§9.4) plus stub drafts in it.

| Flag | Meaning |
|---|---|
| `<key>` | Job key (§2.1). Change: positional, like every other `job` command (cpd 1 had `--key`) |
| `--pr <key>` | the Job's PR (R34, A14), selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one |
| `--needs <k1,k2>` | Dependencies, comma-separated, no duplicates (`usage`); Jobs of the same PR only (A7) |
| `--draft` | add the Job as a `draft` in a phase after `plan` too (User, hs-d173, hs-d174, hs-d179: every planned batch is visible in the Plan view before it is approved): `status = draft`, `appended = 1`, no fingerprint; the engine never runs it, and it becomes `queued` only at the approval of a Plan review of the drafts (§4.7). In phase `plan` every Job is a draft, so `--draft` changes nothing there. Its needs may be drafts. Without `--tier` and `--spec` it is a stub (above); `--draft` is the only way to add a Job without them. A Job that only carries out a recorded User decision, with no open design choice, is added without `--draft` (User, hs-d301, amending hs-d173, hs-d174, hs-d179); drafts are for design the User has not decided, and when unsure it is a draft |
| `--urgent` | add the Job with `urgent = 1` (contract §2.2): engine E3 starts it before every non-urgent Ready Job and may run it as one Attempt beyond `concurrency` (§3.7). Use it **only when the User explicitly asks**, usually for something that blocks all work or an incident. Not a Locked field and not in any fingerprint (§2.3); works with `--draft`, `--review` and in every phase `job add` works in. To mark an existing Job, see §5.8 |
| `--review` | a **review Job** (B2, contract §2.2): its Worker is a fork of the Plan's Lead, it commits nothing and writes `run/<plan>/reviews/<key>.md`. Its Dependencies are **set** to every Job of its PR that exists at that moment and is not `cancelled`, and is not an `open` or `update` Job, so `--needs` with `--review` is `usage` (exit 2). Jobs added later do not become its Dependencies. The Spec file is as for any Job; its `touches` is informational for a review Job (*decided*: the Lead lists `run/<plan>/reviews/<key>.md`). An audit with no finding (a PR holding only this merged review Job) ends with `cpd pr cancel <key>` instead of opening, because a PR with no change cannot open (pro-d77, contract §3.8 "Ending a `dispatch` PR that holds only review Jobs") |
| `--kind <docs\|test\|impl>` | the Job's kind (hs-d341, contract §2.2 "Job kinds"); default `work`. `open` and `update` are cpd's: `kind_reserved`. The PR's stored shape (§9.4) decides which kinds it may have (`shape_violation`, below). A `test` Job's Spec file has `tests` and `test_files` (§5), an `impl` Job needs a `test` Job of its PR, and both need the repo's `[test-commands]` (below). Not with `--review`, which is a `work` Job (`usage`) |

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan`, `dispatch`, `deploy` or `done` (`done` only with `--draft`, and then only into a PR that is not `merged` or `cancelled`: none exists until `pr add` has reopened the Plan); `grounding`: `illegal_phase` | 3 |
| the PR is not `merged` or `cancelled` (§3.2) | `pr_ended` | 3 |
| no Job with this key, unless its Locked fields (`pr` included) and needs are identical (then `already`) | `job_key_exists` | 3 |
| every `needs` key is a Job of the Plan | `needs_unknown` | 3 |
| every `needs` Job is in the same PR; `details.jobs` lists those that are not | `needs_other_pr` | 3 |
| no `needs` Job is `cancelled` or `failed`; with `--review`, no Job of the PR is `failed` (retry or cancel it first; `details.jobs` lists them), because a Dependency on a `failed` Job is refused | `needs_unavailable` | 3 |
| without `--draft`, in a phase after `plan`: no `needs` Job is a `draft`, and with `--review` no Job of the PR is a `draft` (`details.jobs` lists them; its message says to add this Job with `--draft`, or to approve the drafts first), because a `queued` Job could start before the draft it needs exists | `needs_unavailable` | 3 |
| `--kind open` or `update`; `details.kind` | `kind_reserved` | 3 |
| no `needs` Job is an `open` or `update` Job (nothing waits for them: contract §2.2 "Job kinds"); `details.jobs` | `needs_unavailable` | 3 |
| `--kind test` or `impl`: the PR's repo policy has `[test-commands]` (contract §7.7; none when there is no repo file or the file lacks the table) | `tests_unconfigured` | 3 |
| every `touches` entry of the Spec is checked against the tree the Job will start from (the PR's Lead Branch head once the PR started, else its base, contract §2.1): a literal path must exist in that tree, or its parent directory must (a new file); a glob must match a file, or its fixed prefix directory must exist; cpd's own `open` and `update` Jobs are not checked (contract §3.7 E4, hs-d512); `details: {missing: [entries], tree: <ref>}` | `touches_missing` | 3 |
| without `--draft`, an `impl` Job needs at least one `test` Job of its PR; `details.jobs` | `test_job_required` | 3 |
| the PR's stored shape allows the kind, while the PR is `waiting` or `dispatch` (hs-d497, contract §2.2 "Job kinds"): `tdd` allows `docs`, `test`, `impl`; `docs` allows `docs`; `config` allows `work`; `--review` (kind `work`) is allowed in every shape; `free` allows every kind; `details` `{pr, shape, kind, allowed}`. **A PR whose status is `open` allows only `work`, review Jobs included** (pro-d68, contract §2.2 "Job kinds"): `docs`, `test` and `impl` are refused whatever the shape, `details` `{plan, pr, status: "open", shape, kind, allowed: ["work"]}` | `shape_violation` | 3 |
| the Spec file has `tests` and `test_files` exactly when the kind is `test` and the Job is not a stub (§5); `--review` with a kind other than `work` | `spec_invalid`; `usage` | 2 |
| every string of the Spec (each field, each `touches` entry, each `acceptance` item, and their `test` and `test_files` for a test Job) names only repo-relative paths, not the primary checkout of any PR's repo (contract §8.3); a match is a substring ending at the end of the string or before a non-letter, non-digit, `.`, `_` or `-` character; `details: {path, primary, pr}` names the JSON path, the root realpath (or `~`/`$HOME` prefix), and the PR | `spec_invalid` | 2 |
| `--tier` is a Tier a Worker may have: `H0`–`H4` or `S0`–`S3` (§5, pro-d53, hs-d554; an `O` Tier, any `S` outside `S0`–`S3`, or effort 5 is refused; a Tier that is not valid at all: `tier_invalid`) | `tier_not_allowed` | 2 |

### A Spec names no primary checkout

A Spec must name only repo-relative paths. A Worker works in its own worktree (contract §7.2), not the repo's primary checkout (contract §15.16: the adapter refuses any write into a primary checkout). When reading the latest main, the Worker runs `git fetch origin` and then `git show origin/main:<path>`. cpd checks every Spec string (`goal`, `context`, each `touches` entry, each `acceptance` item, and for a `test` Job its `tests` and `test_files`), searching for the root realpath of each PR's repo (or `~`, `$HOME` or `${HOME}` as a prefix of it), as a substring ending at the end of the string or before a character that is not a letter, a digit, `.`, `_` or `-`: for example `<root>/docs/x.md`, `<root>` followed by a space, a quote, a backtick or a comma match; `<root>-old` and `<root>.wt/x` do not. The first match in field order (goal, context, touches, acceptance, out_of_scope, tests, test_files) and PR order is reported as `spec_invalid`, with `details: {path, primary, pr}` the JSON path, the root, and the PR key. Nothing is written when the check refuses.

**A stub draft** stores `tier` and the Spec fields `goal`, `context`, `touches`, `acceptance` and `out_of_scope` as null for what was not given (a given `--spec` sets all five; `--tier` sets the Tier), and its `job-added` Event has `tier: null` when it has none. A draft that lacks its Spec or its Tier is **incomplete**: it is never queued or run (contract §3.2), a Plan review of it is refused `draft_incomplete` (§4.6) and so is `plan lock` (§4.8). `needs` and `pr` work as for any draft.

Effects: **rows**, one transaction: `jobs` (`pr` = the PR's key; `kind` (`work` by default); `tests` and `test_files` from the Spec file of a `test` Job; `review` = 1 with `--review`; `urgent` = 1 with `--urgent` (else 0); `status = draft`, `appended = 0`, no fingerprint, in phase `plan`; `status = draft`, `appended = 1`, no fingerprint, with `--draft` in any later phase; `status = queued`, `appended = 1`, fingerprint stored, in a later phase without `--draft`); one `dependencies` row per need (with `--review`, per Job of the PR, as the Dependencies are set in the same transaction). **Event** `job-added` `{pr, kind, status, appended, tier, needs, review, urgent}`. A Job appended to a PR keeps the PR's `open` Job waiting until it is `merged` or `cancelled`: that Job's Ready rule counts every Job of the PR (contract §3.2). (Before hs-d340 the add also re-armed `all-merged`; that step and Event are retired, contract §3.7 E4.) No cycle is possible: nothing needs a new Job yet. An appended Job of a `waiting` PR is not Ready until its PR starts (§3.2); an appended draft is not Ready at all until its review is approved; one of an `open` PR (a `work` Job, above) reaches its pull request by its own merge (contract §3.8 "Updating"): **no `update` Job is queued**, no PR Gate runs. `pr push` (§9.9) is the repair after a `push_failed`, and `pr rebase` (§9.3) the repair for the rest. A review Job (`--review`) holds the PR's `open` Job through a stub fix draft the Lead registers with it (`job add <key> --title <t> --draft`, a title only; contract §2.2 "Review Job"): the draft keeps the `open` Job from being Ready until the Lead completes or cancels it.

```json
{"ok": true, "job": {"key": "api-schema", "pr": "api", "kind": "work", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false, "urgent": false}}
```

Idempotent: yes, as stated (`--draft` and `--urgent` are not Locked fields: the call that found an identical Job returns `already` whatever its status). `--review` is repeatable like any add: an identical call (same Locked fields, `review` included, same Dependencies) is `already`; a call after another Job was added to the PR differs in its Dependencies and is `job_key_exists`. `job edit` has no `--review` and refuses `--needs` and `--no-needs` on a review Job with `usage` (*decided*: its Dependencies are fixed by the add; to change them, cancel it and add a new one).

### 5.2 `job edit`

```
cpd job edit <key> [--title <text>] [--tier <T>] [--spec <file>] [--pr <key>] [--kind <work|docs|test|impl>] [--needs <k1,k2> | --no-needs] [--plan <id>]
```

**Completing a stub draft** (hs-d241, R1): `cpd job edit <key> --tier <T> --spec <file>` sets the Tier and the Spec of a draft that lacks them (a draft is editable, so this is an ordinary edit; `fields` lists `tier` and `spec`). Once the draft has both, it is complete and a Plan review may approve it (§4.6). The edit changes the plan fingerprint (contract §2.3), which covers a stub as it is.

**`--kind`** (hs-d341) changes a draft's kind, never to `open` or `update` (`kind_reserved`); a Job that is not a draft is Locked. The Spec's `tests` and `test_files` must fit the new kind (`spec_invalid`: a `test` Job that is not a stub has both, every other kind neither), and the rules of §5.1 apply as there (`tests_unconfigured`; `test_job_required` where an `impl` Job stops being a draft, at the review or the lock; `shape_violation` when the PR's shape does not allow the new kind). `fields` lists `kind`.

A draft is editable in **every phase** (User, hs-d173, hs-d174, hs-d179), including one added after the lock, whose review may be pending: the edit changes the fingerprint of the drafts, so the pending review no longer matters (§4.6). `--needs` may name drafts. `--no-needs` removes every Dependency (cpd 1 had no way to). `--pr <key>` moves the draft Job to another PR (R34, A14); it is a field flag, so §1.3's default does not apply: without it the Job keeps its PR. Dependencies stay inside one PR (A7): a Job that changes PR keeps none of its old needs, so give `--needs` with Jobs of the new PR, or `--no-needs`, and no other Job may need it.

| Precondition | Code | Exit |
|---|---|---|
| at least one field flag | `nothing_to_edit` | 2 |
| with `--tier`: the Tier is one a Worker may have, `H0`–`H4` or `S0`–`S3` (§5, pro-d53, hs-d554). A draft stored with a refused Tier before pro-d53 is now allowed; a draft stored with a refused Tier before hs-d554 is repaired by this edit; an edit that does not give `--tier` is not blocked by the Tier it holds | `tier_not_allowed` | 2 |
| the Job is `draft`, in whatever phase; a Job that is not `draft` is Locked; an `open` or `update` Job is cpd's and always refused (its Spec is cpd's), also while the `open` Job is a `draft` (hs-d573) | `job_locked` | 3 |
| `--pr` names a PR of the Plan | `pr_not_found` | 4 |
| that PR is not `cancelled` | `pr_ended` | 3 |
| the needs exist and are usable | `needs_unknown`, `needs_unavailable` | 3 |
| every Dependency after the edit links two Jobs of the same PR: the needs are in the Job's PR (the new one with `--pr`), and with `--pr` the Job keeps no old needs and no Job needs it; `details.jobs` lists the Jobs on the other side (§4.4 `dependencies_same_pr`, `jobs_pr_dependencies`) | `needs_other_pr` | 3 |
| the new needs make no cycle | `dependency_cycle` | 3 |
| with `--spec`: every `touches` entry is checked as in `job add` (§5.1), against the tree the Job will start from (contract §2.1, hs-d512); `details: {missing: [entries], tree: <ref>}` | `touches_missing` | 3 |
| with `--spec`: the Spec names only repo-relative paths, not the primary checkout of any PR's repo (§5.1 "A Spec names no primary checkout"); `details: {path, primary, pr}` | `spec_invalid` | 2 |

Effects: **rows**, one transaction: the changed `jobs` columns; `dependencies` rows deleted and inserted to match (§4.4 allows deletion only for `draft`); with `--pr`, in the order §4.4 states: delete the old Dependencies, update `pr`, insert the new ones. **Event** `job-edited` `{fields: [name]}`, listing only the fields whose value changed (`pr` among them).

```json
{"ok": true, "job": {"key": "api-schema", "status": "draft"}, "fields": ["tier", "needs"]}
```

Idempotent: yes; when no value changes it writes nothing and returns `"already": true`.

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
- `running`: one transaction, `cancel_reason` and `cancel_requested_at` only; **Event** `job-cancel-requested` `{reason}`. Engine E2 sets the Attempt's `stop_request = cancelled`, and the driver or E2 ends it `cancelled` at its next safe point (§3.4), which appends `job-cancelled`. No tmux action by this command.
- **Dependents (hs-d647)**: in the same transaction, every Job of the Plan that needs the cancelled Job, directly or transitively through any status, becomes `failed` (when `queued`: `failed_cause` is `its need <cancelled-key> was cancelled`, one `job-failed` Event) or `cancelled` (when a `draft`: `cancel_reason` the same string, one `job-cancelled` Event); one already `failed`, `merged` or `cancelled` is untouched. The result's `failed` and `cancelled` arrays name the Jobs this changed, and are absent when empty. In the `running` case the dependents are failed in the transaction that makes the Job `cancelled` (the engine's, 3.4), not by this command.

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
| `--tier` is a Tier a Worker may have, `H0`–`H4` or `S0`–`S3` (§5, pro-d53, hs-d554): **`job retry --tier` refuses a Tier outside the Worker range**, so a Job that failed on `S3` cannot be retried higher; its remedy is the same Tier or a new Job with a better Spec. Checked first, so a refused Tier changes nothing | `tier_not_allowed` | 2 |
| the Job is `failed` (or `queued` with this `retry_tier` and no Attempt since: `already`) | `job_not_failed` | 3 |
| no `needs` Job is `cancelled` (`needs_unavailable`, exit 3, `details.needs` lists them): a Job held by a cancelled need can never start, so cancel it and add a replacement whose needs do not name the cancelled Job. A `failed` need is allowed: retrying recovers it | `needs_unavailable` | 3 |

Effects: **rows**, one transaction: `status = queued`, `retry_tier = <T>`, `failed_cause = null` (the Job's `tier` field is left unchanged). **Event** `job-retried` `{tier}`. The next Attempt has `origin = retry` and this Tier, and the ladder restarts from it with a fresh cap (§9; Attempts whose driver never started are never counted). The Plan view, `plan render` and `context pending` show the Tier the Job is displayed at (contract §3.4a), which is `retry_tier` while the Job is queued at it; `plan show` and `job show` print both `tier` and `retry_tier` as raw field values.

```json
{"ok": true, "job": "worker", "status": "queued", "tier": "S4"}
```

Idempotent: yes, as stated.

### 5.5 `job dispatch`

```
cpd job dispatch <key> [--plan <id>]
```

Starts one Ready Job now: the same step as engine E3 (cpd 1 §11, kept).

| Precondition | Code | Exit |
|---|---|---|
| the Job is Ready (§3.2) | `job_not_ready` | 3 |
| its fingerprint matches | `locked_job_tampered` | 3 |
| the driver lock is free (§3.4 step 1) | `cas_lost` | 3 |
| a linear Attempt of the Job's PR is live (pro-d52, contract §3.7 E3 item 1) | `cas_lost` | 3 |
| fewer than `concurrency` Attempts are not `ended` (an urgent Job: fewer than `concurrency + 1`, contract §3.7) | `no_slot` | 3 |
| the Job's Tier for the Attempt is one a Worker may have (§5, pro-d53, hs-d554): a Job stored on a Tier outside the range before the rule is not started here, as engine E3 does not start it and raises one `error` Event per such Job (contract §3.7 E3) | `tier_not_allowed` | 2 |

Effects (§3.4 "The driver lock"): take `locks/attempt-<plan>--<attempt>.lock`; **rows**, one transaction: `attempts` (`starting`, Tier from §9, `origin`), Job `running`; **Event** `attempt-started` `{number, tier, origin}`; then spawn `cpd attempt run <attempt> --plan <plan>` with `setsid`, fd 3 = the lock, stderr to `run/<plan>/logs/attempt-<attempt>.log`, and record `driver_pid` (a data write, no Event). If the spawn fails, the command ends the Attempt `failed` / `crashed` with `driver_pid` still null (`attempt-retry` at the same Tier, not counted against `retry_cap`, §3.4) and returns `spawn_failed` (exit 1).

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
{"ok": true, "jobs": [{"key": "api-schema", "pr": "api", "title": "…", "status": "merged", "tier": "S2", "appended": false, "kind": "work", "review": false, "urgent": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee…"}]}
```

### 5.7 `job show`

```
cpd job show <key> [--plan <id>]
```

Effects: none. Output: every field of the Job (§2.2; `review: true` for a review Job, B2; `urgent`, a boolean; `kind`, and `tests`, `test_files` and `test_approved` where the kind has them; the `needs` of an `open` or `update` Job is the derived list of contract §3.2), `needs`, `needed_by`, and every Attempt with all its fields (`check_tail`, `metrics`) and its Escalations (cpd 1 §11, kept).

```json
{"ok": true, "job": {"key": "worker", "pr": "api", "status": "failed", "failed_cause": "timed_out", "tier": "S2", "review": false, "urgent": false, "fingerprint": "9f…", "retry_tier": null, "…": "…"}, "needs": ["api-schema"], "needed_by": ["ui"],
 "attempts": [{"id": "worker-a1", "status": "ended", "outcome": "failed", "cause": "timed_out", "check_tail": null, "escalations": []}]}
```

`tamper` is the tamper report of §4.3, as in `plan show` (empty when the Job is intact). Both are read-only.

### 5.8 `job urgent`

```
cpd job urgent <key> [--plan <id>]
```

Marks an existing Job **urgent** (contract §2.2): engine E3 starts urgent Ready Jobs first, then by `jobs.seq`, across Plans, and may start one urgent Job beyond `concurrency` (§3.7). A Lead or Manager runs it **only when the User explicitly asks**, usually for something that blocks all work or an incident. `urgent` is not a Locked field and in no fingerprint (§2.3), so it is allowed on a Locked Job: a `queued` one.

| Precondition | Code | Exit |
|---|---|---|
| the Job exists in the Plan | `job_not_found` | 4 |
| the Job is `queued` or `draft`; a `running`, `merged`, `failed` or `cancelled` Job is refused (*decided*: the flag only matters to scheduling, and E3 only starts `queued` Jobs; `job retry` makes a `failed` one `queued`, then mark it) | `illegal_transition` | 3 |

Effects: **rows**, one transaction: `jobs.urgent = 1`. **Event** `job-edited` `{fields: ["urgent"]}` (no new Event kind). Nothing else changes: status, fingerprint, `appended` and Dependencies stay as they are. There is no command to clear the flag.

```json
{"ok": true, "job": {"key": "hotfix", "status": "queued", "urgent": true}, "fields": ["urgent"]}
```

Idempotent: yes; a Job that is already urgent writes nothing and returns `"already": true` with `"fields": []`.

### 5.9 `job approve-tests`

```
cpd job approve-tests <job> --path <p> [--path <p> …] --reason <text> [--plan <id>]
```

The Lead's approval for an `impl` Job to change a red test (hs-d341; contract §2.2 "Job kinds", §7.5). The files of the `test` Jobs an `impl` Job needs are guarded: its submit is refused `tests_tampered` (§6.1) when it changes one. A Worker that needs a red test changed asks the Lead (`cpd ask`); the Lead decides and, to allow it, runs this. **The Lead only**, through `CPD_ATTEMPT` (hs-d497): the command is refused `not_a_lead` when `CPD_ATTEMPT` is set, which cpd sets in every Worker's pane (§6.5). That is all the check there is: the command is **unauthenticated, like `cpd answer`** (a Worker that unsets the variable gets through), and the approval is visible afterwards in the `job-edited` Event (`fields: ["test_approved"]`, with the `--reason`).

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

Effects: **rows**, one transaction: `jobs.test_approved` becomes the union of its old value and the paths. **Event** `job-edited` `{fields: ["test_approved"]}`, its `text` naming the paths and the reason. It takes effect at the Job's next check, a running Attempt's included (contract §7.7). `test_approved` is not a Locked field and is in no fingerprint (§2.3).

```json
{"ok": true, "job": {"key": "api-impl", "status": "running", "kind": "impl"}, "approved": ["cli/tests/it/pr_open_job.rs"], "fields": ["test_approved"]}
```

Idempotent: yes; paths already approved write nothing, and a call with none new returns `"already": true`.

## 6. `attempt`

### 6.1 `attempt submit`

```
cpd attempt submit <attempt> [--plan <id>]
```

Run by the Worker, from its worktree. Preconditions are §7.3's list, in that order, after the Attempt's own state:

| Precondition | Code | Exit |
|---|---|---|
| the Attempt is `running` (already `merging` or later with `submitted_sha` = the worktree's HEAD: `already`) | `attempt_not_running` | 3 |
| the caller's cwd is inside the Attempt worktree, on the Attempt branch; for a linear Attempt, the worktree is the workflow worktree and the branch is the Lead Branch | `wrong_worktree` | 3 |
| clean tree, untracked files included | `dirty_tree` | 3 |
| at least one commit ahead of `base_sha`; **for a review Job (B2) instead**: no commit ahead of `base_sha`; **for an `open` or `update` Job none is required** (the base may not have moved, so there may be nothing to commit, hs-d340) | `no_commits`; `review_commits` | 3 |
| (review Job only) `run/<plan>/reviews/<job>.md` exists and is not empty (contract §7.3, precondition 3) | `review_missing` | 3 |
| the tamper check passes (§7.5; a protected path whose Attempt version equals the Lead tip's or `origin/<base>`'s, as a base merge brings it, is allowed); `details.paths` lists the protected paths touched that are not allowed | `gate_tampered` | 3 |
| (kind `impl`) it changes no path guarded by the `test_files` of a `test` Job it needs, unless `job approve-tests` (§5.9) approved it; `details.paths`, `details.jobs` (contract §7.5, §7.7) | `tests_tampered` | 3 |
| (kind `test`) every `test_files` entry matches a path the Job changed (`git diff --name-only --no-renames <lead branch>...HEAD`); `details.paths` lists the entries that match none | `test_files_missing` | 3 |
| (kinds `test`, `impl`) the PR's repo policy has `[test-commands]` (none when there is no repo file or the file lacks the table) | `tests_unconfigured` | 3 |
| (kinds `test`, `impl`) the repo's `build` exits zero; `details` `{job, tests, tail, log}` | `tests_build_failed` | 3 |
| (kinds `test`, `impl`; the repo's `[test-commands]` has `list`) the `list` command exits zero for every named test, run once per name with the variable `test`; `details` `{job, tests, tail, log}`, `tests` the names that match nothing (hs-d497) | `tests_not_found` | 3 |
| (kind `test`) the repo's `run` of the named tests exits non-zero: they fail, for a reason other than a build error | `tests_not_red` | 3 |
| (kind `impl`) the repo's `run` of the derived tests exits zero | `tests_failed` | 3 |
| (kinds `test`, `impl`) the commands ran (not killed, not `hook_invalid`) | `tests_error` | 1 |
| (kind `open`) the PR has a stored title; the newest pull request of the Lead Branch is not closed without a merge (`details.number`, `details.url`) | `pr_title_required`; `pr_closed` | 3 |
| (kind `update`) the PR is `open` | `illegal_transition` | 3 |
| (kinds `open`, `update`) the PR's policy resolves (contract §8.3): its Context has an owner | `owner_unset` | 3 |
| its owner has a policy file; its policy files are valid | `owner_unconfirmed`, `policy_invalid` | 1 |
| (kinds `open`, `update`) the Lead Branch's tip is an ancestor of HEAD; `details.next` is `git -C <worktree> merge --no-edit <lead branch>` | `lead_branch_moved` | 3 |
| (kinds `open`, `update`; a stacked PR) the base PR's Lead Branch is on the remote; `details` `{pr, base, base_pr}` | `base_not_found` | 4 |
| (kinds `open`, `update`) HEAD does not conflict with `origin/<base>`, freshly fetched (`git merge-tree --write-tree --name-only origin/<base> HEAD`; hs-d490: the base is merged only on a conflict, and a base that merely moved ahead is no refusal); `details` `{pr, base, base_sha, sha, files, next}`, `next` = `["git -C <worktree> merge --no-edit origin/<base>", "cpd attempt submit <attempt>"]` | `base_conflict` | 3 |
| (kinds `open`, `update`; `gate.pr` is `local`) the Gate's `pre-merge` passes on HEAD in the Attempt worktree (the workflow worktree for linear Attempts); `details.tail`, `details.log` (`run/<plan>/logs/pr-<key>.pre-merge.log`) | `gate_failed` | 3 |
| (the same) the Attempt worktree's `pre-start` passes and the Gate ran (not killed, not `hook_invalid`) | `gate_error` | 1 |

Effects:
- **One check span** (contract §7.7, hs-d679): every check below, in order, runs inside ONE span, `what` `checks`: the tamper check, the red-test guard, and the kind check; for an `open` or `update` Job the base check and the Gate also run at the merge (contract §7.4, hs-d679). The preconditions of §7.3 run before it; a review Job has no checks and no span.
- **Hooks:** `pre-commit` only, in the worktree (§7.3); output to `run/<plan>/logs/<attempt>.pre-commit.log`. With no `pre-commit` hook nothing runs, no log is written and `check_tail` stays null; `pre-merge` only runs at the merge (§7.3, hs-d679).
- **Kind checks** (contract §7.7, hs-d340, hs-d341), for the kinds that have one: a `test` or `impl` Job's `[test-commands]` commands (`build`, then `run`; output in `run/<plan>/logs/<attempt>.tests-build.log` and `<attempt>.tests-run.log`; with `list`, once per name between them, `<attempt>.tests-list.log`), or an `open` or `update` Job's lookup and base check (`git fetch origin <base>`; `gh pr list` for `open`; `run/<plan>/logs/pr-<key>.pre-merge.log`). The span holds `locks/check-<plan>--<attempt>.lock`; it appends `attempt-status` with `check` `start` (before the first check) and `end` (contract §5.2; on a pass in the transaction of the Rows below, on a refusal before the error returns), so the Attempt's timeout and the stall kill do not count it, the tamper check included. A refusal changes nothing else: the Worker fixes, commits and submits again, and the checks run again.
- **Rows**, one transaction with a compare-and-swap on `status = 'running'` and on the HEAD checked: Attempt `merging`, `submitted_sha`, `submitted_at`, `check_tail`; the Job stays `running`; the span's `end` Event is appended in the same transaction. **Event** `attempt-status` `{from: "running", to: "merging"}`.
- The driver sees `merging` at A5 and starts the merge (A8, §7.4). For a review Job the merge changes nothing: the Attempt and the Job end `merged` with the Lead Branch unchanged and `merged_sha` its tip (contract §3.4, §7.4). An `open` or `update` Job's merge does not rebase, pushes the Lead Branch and, for an `open` Job, opens the pull request and moves the PR in step 10's transaction (contract §7.4, §7.7): `pr-opened` and `plan-phase` follow its `job-merged`.

```json
{"ok": true, "attempt": "api-schema-a1", "submitted": true, "sha": "a1b2c3d…", "next": "Stop now. Do not make more changes."}
```

Idempotent: yes, as stated; a HEAD that moved after a successful submit is refused `attempt_not_running`.

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

`show` returns every field of the Attempt (§2.2), its Escalations, and the paths it uses: `spec_path`, `check` (`run/<plan>/logs/<attempt>.pre-commit.log`, null when no quick check ran), `log` (`run/<plan>/logs/attempt-<attempt>.log`), and the Worker's session file. `list` returns `id`, `job`, `number`, `tier`, `origin`, `status`, `outcome`, `cause`, `started_at`, `ended_at`; `--active` keeps those not `ended`.

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

Effects: everything of §3.4 A1–A9, §7.2, §7.3, §7.4: the worktree and `pre-start`; the rendered Worker Spec (§16.1); the Worker's window `cpd-work:<plan>--<attempt>` (`new-window` with the Worker's argv of §6.5, `@cpd_session`); the Session rows (`session-registered`, `session-moved` `{place: "work"}`); the nudge (`attempt-nudge`); the stall kill (a Worker's session file idle for `timeouts.stall_seconds` inside one tool call: pi's descendant processes are killed, pi is not, the Attempt goes on; one driver-log line, no Event; contract §3.4 A4); the merge, with no lock (hs-d243: rule S and rule P, contract §7.4); the end (`metrics`, `kill-window`, worktree removal on `merged`). Each status change is one transaction with the Event of §3.4.

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
3. Answered: one transaction setting the Worker Session `working` (`session-state`); prints the answer. Timeout: one transaction, `status = expired`, `expired_at`, **Event** `escalation-expired` `{escalation, reason: "timeout"}`, Session `working`; prints `ask_timeout`. If the process dies, E1 expires it (`reason: "asker_dead"`).

```json
{"ok": true, "escalation": "api-schema-a1-q1", "answer": "Use the v2 field names."}
```

Idempotent: no. Each call is a new Escalation; ids are never reused (§3.5).

### 8.2 `answer`

```
cpd answer <escalation> <answer> [--plan <id>]
```

Run by the Lead.

| Precondition | Code | Exit |
|---|---|---|
| the Escalation is `open` | `escalation_answered`, `escalation_expired` | 3 |

Effects: **rows**, one transaction: `status = answered`, `answer`, `answered_at`. **Event** `escalation-answered` `{escalation}`. The waiting `ask` sees it at its next poll.

```json
{"ok": true, "escalation": "api-schema-a1-q1", "attempt": "api-schema-a1", "answered_at": "…"}
```

Idempotent: no; §3.5 refuses `answer` on an Escalation that is not `open`, so a second answer is `escalation_answered`.

## 9. `pr`

A Plan's PRs (R34, §2.2 PR, §3.8) and their pull requests, through `gh` (§7.6). `gh` failures are `gh_failed` (exit 1) with its stderr tail. Every command here works on **one PR**: `pr open`, `pr rebase`, `pr push`, `pr view` and `pr reply` select it by `--pr <key>` (§1.3: omitted, the Plan's one PR that is not `cancelled`); `pr add`, `pr edit`, `pr cancel` and `pr start` (§9.4–§9.7) take its key as their argument; `pr poll` has no `--pr`: it polls every open pull request of the User. `<repo>`, `<base>`, `<lead branch>` and `<lead worktree>` are the selected PR's (§7); `gh` runs with cwd = its Lead worktree, so it addresses the PR's repo.

### 9.1 `pr open`

**The repair** (hs-d340). A PR is opened by its `open` Job (contract §7.7), not by the Lead: engine E4 adds the Job to every started PR, its Worker merges the base, and its submit runs the Gate, pushes, and opens the pull request in its merge. `pr open` stays for what that cannot cover: a PR whose `open` Job is `failed` (the Lead decides between `job retry <job> --tier <T>` (§5.4) and this command), an engine that is down, or a PR whose `open` Job the Lead cancelled (`job cancel` stands, §5.3: E4 adds no new one, so this is how the PR is opened later; to give it up, `pr cancel`, §9.6). It is refused `open_job_live` while the PR has an `open` Job that is `draft`, `queued` or `running` (hs-d573), and its `jobs_not_done` counts every Job of the PR except its `open` and `update` Jobs. Nothing else here changes: the steps and refusals below are the ones the Job's submit and merge share (contract §7.6 steps 0, 1, 3, 4).

`pr open` is the **Mechanical checks** step of the Deploy stage (contract §3.9): it runs the Gate, then opens the pull request. Review follows the opening, not the Gate. **The Gate runs only when the PR's `gate.merge` is `local` (for `pr open` and `pr push`) or `gate.pr` is `local` (for `pr rebase`)** (contract §8.3, hs-d292; the PR's policy key, `Repo(<owner>, <name>)`): with `ci`, cpd's default, `pr open` runs no Gate, the repo's CI is the check, and the two Gate rows below do not apply.

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
| the PR has a stored title (hs-d285): a PR that existed before migration `pr-title` has none, and there is no fallback to the Plan's title. The message says to run `cpd pr edit <key> --title "<text>"`. Checked before the Gate | `pr_title_required` | 3 |
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

**A stacked PR** (hs-d324, hs-d325, hs-d301, contract §3.8 "Stacked PRs" step 7): when its base PR is `merged`, the command first **unstacks** it, **before the lookup and before every precondition below, `base_not_found` included** (its `base` becomes the base PR's own `base`, normally `main`, walking a chain of merged base PRs to its end; in its own transaction, **Event** `pr-base-changed` with `by` `pr open`, one per step, §5.2; the unstack is kept even when a later check refuses) and goes on with the new base, because GitHub deleted the merged base PR's branch: that branch is never looked up. The poll already unstacks such a PR when it records the base PR's merge (`by` `poll`, contract §7.6 step 6), so this is the case where no poll did yet. Otherwise `<base>` is the base PR's Lead Branch, so the conflict check, the Gate (its `target` the base PR's Lead Branch) and `gh pr create --base <base>` all run on top of it, and GitHub will not land the stacked pull request before the base PR. The Lead Branch contains the base PR's work, because it was started from the base PR's head (§9.7); a base PR that has moved on since is merged in only when it conflicts (`base_conflict`, `next`; hs-d490). Opened ready, never a draft, like every PR.

**Ready, never a draft** (hs-d329): the pull request is opened only after the conflict check above and, with `gate.pr` is `local`, the Gate; `gh pr create` never has `--draft` and no cpd command makes a pull request a draft or ready (contract §7.6 "Ready, never a draft"). Under `ci` the conflict check still applies and the pull request opens ready (the Lead's reading of hs-d329: `ci` skips only the Gate).

**An adopted PR** (`head_ref` set, §9.4): the pull request is found by `gh pr list --head <head_ref> --state all`, the Gate runs as above, and rule P pushes to `refs/heads/<head_ref>`, plain, never forced; `gh pr create` is never run, the pull request that exists is adopted (its own title kept). `push_rejected` (§9.9) names `cpd pr rebase` (the catch-up, §9.3) as the next step. Steps 0 and 3 and the merged check name `<head_ref>`, not the Lead Branch.

Effects:
- **Title** (hs-d285): the pull request is titled with the PR's stored title, passed through the one seam in `core/src/pr/` that turns it into the GitHub title (unchanged today; hs-d277 may change it later). The PR body's `# <title>` line uses the same title. The title no longer comes from the Plan's title, and a Plan with several PRs no longer gets ` (<key>)` appended.
- **No lock** (hs-d243, contract §7): merges into the PR go on during the command; no R first (an unrecorded merge leaves its Job `running`, which `jobs_not_done` refuses).
- **Adopting a merged pull request** (before the Gate; contract §7.6 `pr open` step 0): `gh pr list --head <lead branch> --state all --json number,url,state`, the newest decides. When it is `MERGED`, the Gate and the push are skipped (hs-d267 forbids skipping the Gate only when *opening* a pull request where `gate.pr` is `local`, hs-d292): one transaction records the pull request (its `pull_requests` row, `state` `merged`; the PR's `pr_number`, `pr_url`, `pull_request`) and moves the PR `dispatch → open → merged` exactly as `pr poll` does when it sees a merge: `pr-merged` (no `pr-opened`); `plan-phase` `dispatch → deploy` when every PR that is not `cancelled` is then `open` or `merged`, and `plan-done` (`deploy → done`) when every one is `merged`. Before the `plan-done` Event, every pending review of the Plan is closed `superseded`, reason "Plan review r<k> of Plan P is superseded: the Plan is done." The next `pr poll` cleans up the merged PR's worktrees. The output carries `"adopted": "merged"`. When it is `CLOSED` without a merge the command is refused `pr_closed` (reopen it on GitHub, or move the work to a new branch); when it is `OPEN`, it is adopted after the Gate and the push, as below.
- **Conflict check** (contract §7.6 step 1, hs-d490): `git -C <repo> fetch origin <base>` (no `origin`: skipped, `refs/heads/<base>` used), `B = rev-parse origin/<base>` read once, `G = rev-parse refs/heads/<lead branch>` read once, then `git merge-tree --write-tree --name-only B G`; exit 1 is `base_conflict` with nothing run, pushed or written, any other failure `git_failed`. No ancestry test: a base that moved ahead is not a refusal and starts no Gate. It also applies to the adoption of an open pull request, not to a merged one. B is not fetched again after the Gate.
- **Hooks:** with `gate.pr` is `local` (contract §8.3, hs-d292), the Gate runs in the Lead worktree at G: `pre-start` (output `run/<plan>/logs/pr-<key>.pre-start.log`), then `pre-merge` from the PR's policy (output `run/<plan>/logs/pr-<key>.pre-merge.log`), with the template variable `branch` = the Lead Branch. With `ci`, none. On `gate_failed` or `gate_error` nothing is pushed, no row is written and the PR stays `dispatch`: the Lead appends fix Jobs to it (`job add --pr <key>`) and runs `pr open --pr <key>` again once they are Merged (hs-d679, hs-d680).
- **Re-read:** after the Gate the PR is read again; when it is no longer `dispatch` (a concurrent `pr open` opened it), the first precondition answers (`already`) and nothing is pushed.
- **git:** rule P (contract §7.4): `git -C <lead worktree> push --porcelain origin G:refs/heads/<lead branch>` (no `-u`). The Lead Branch is usually on the remote already: the PR's start pushed it and every merge pushed it (§7.2, §7.4, R35), so this push is normally a no-op, and it pushes again what an earlier `push_failed` left behind. A rejection when the remote tip already contains G (a merge during the Gate pushed a descendant) is a success with `pushed` false; any other git failure is `git_failed`.
- **gh:** the lookup above (`--state all`); when it found an open pull request (an earlier run died before its transaction) that one is adopted; else `gh pr create --base <base> --head <lead branch> --title <title> --body <summary>` (never `--draft`). When `gh pr create` fails, the `gh pr list` lookup runs again and a pull request it finds (a concurrent `pr open` created it) is adopted; otherwise the failure stands. `<title>` is the PR's stored title (Title, above); `<summary>` is the table of the PR's own Jobs and its Merged commits (cpd 1 §11, kept).
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
| a `gh` call fails, times out (15 s each), prints an empty or non-JSON body, answers the search with `errors`, or answers the lookup with no `data`; `details` is `{argv, exit, stderr, call, transient, reason}`: `call` is `search` or `lookup`, `transient` is true for an HTTP 5xx (`gh`'s `HTTP 5xx` in its message), a 15 s timeout, or a cut response (`gh` exited non-zero with `unexpected end of JSON input` in its stderr and no complete stdout, hs-d354), false for everything else, and `reason` is `HTTP <status>`, `timeout`, `unexpected end of JSON input` or null (contract §7.6 step 4, hs-d354) | `gh_failed` | 1 |

There is no phase precondition: a Plan in any phase gets the poll.

**A transient `gh_failed`** (`details.transient` true) is still exit 1 and writes nothing: the command never retries, waits or keeps state, so a Lead's `pr poll` by hand fails at once. The tolerance is engine E5's (contract §3.7): it logs a `watch.log` line, polls again at the next due time, and raises an `error` Event only after `watch.pr_poll_transient_minutes` (default 10) of the same call failing (the cut response counts, with `reason` `unexpected end of JSON input`), keeping the first failure's time in `run/pr-poll-failing.json`. Every other `gh_failed` is an `error` Event at once.

Effects (contract §7.6 gives the query, the mapping and the caveats):
- **Lock:** `flock` on `locks/pr-poll.lock`, held for the whole poll, so two polls never overlap.
- **gh:** one `gh api graphql` call with two searches (`is:pr is:open author:@me archived:false sort:created-asc` and `review-requested:@me is:pr is:open archived:false sort:created-asc`, the oldest 100 each; every pull request is read with its `baseRefName`, for the base follow below), then, for the pull requests where at least one holds (a) the PR is owned and neither search listed it, (b) `authored` is 1 and the `author:@me` search did not list it, or (c) `review_requested` is 1 and the `review-requested:@me` search did not list it, one direct read call with one alias per pull request (hs-d565, contract §7.6 step 3). No candidate, no call. **The feedback read** (pro-d68, contract §7.6 step 3a): every pull request also reads `updatedAt` (still 1 point); when a PR a Plan owns (`prs.status` `open`) has an `updatedAt` that differs from its row's `updated_at`, or the row's is NULL, ONE more `gh api graphql` call reads the review threads, issue comments and reviews of all such pull requests, with `viewer { login }`, and the poll stores the items (contract §2.2 "Feedback item") in `feedback` with `updated_at`. More than 100 threads, comments, reviews or comments in a thread is `gh_failed` (`details` name the PR and the number; nothing is written); a pull request no Plan owns is never read; an `updatedAt` that did not move costs no call. Stdout is parsed whatever the exit code (gh exits 1 when an alias is `NOT_FOUND`). The direct read adds `viewerDidAuthor` and `viewerLatestReviewRequest { id }` (both required); answers tell what changed. **The checks read** (pro-d75, contract §7.6 step 3b): for each pull request where (a) the PR is owned, (b) the PR's `pr.feedback.checks` is `any`, and (c) the pull request's checks rollup is `failed`, ONE more `gh api graphql` call reads the checks: CheckRun and StatusContext nodes, applying the per-check mapping (FAILURE, TIMED_OUT, ACTION_REQUIRED, STARTUP_FAILURE, ERROR are failing; CANCELLED is not; pending checks never fail), and stores the result in the PR's `checks` field (contract §7.6 step 3b, §2.2). More than 100 contexts is `gh_failed` (details name the PR and the number; nothing is written); a pull request that is not owned, or whose `pr.feedback.checks` is `none`, or whose rollup is not `failed`, makes no such call.
- **Rows**, one transaction for every change, none when nothing changed: `pull_requests` rows inserted or updated (`changed_at` = now) when a value changed: `authored` and `review_requested` set by searches that list the PR (hs-d565); the direct read updates them per what it found (§9.2, contract §7.6 step 4). The poll does not delete rows (hs-d565): a row stays while it is in either search or a PR points to it; when it is in neither and no PR points to it, both flags 0 and no owner stays. The `authored` rows are what the Deploy column shows (§13.2, §17); the Inbox kinds read them all (§21).
- **Events**, in that transaction, for the Plan whose PR points to the pull request only: **Event** `pr-feedback` (data of contract §5.2) when the checks became `failed` (per-check mapping of step 3b: FAILURE, TIMED_OUT, ACTION_REQUIRED, STARTUP_FAILURE, ERROR; not CANCELLED or pending checks) and the PR's `pr.feedback.checks` is `any` (contract §8.3, hs-d292; with `none` failing checks raise nothing; a PR whose repo has no policy file is polled with the owner and default policy, contract §8.3 "Keys and resolution"), the review `changes_requested`, the state `closed` (from anything else, once per transition) or **`comments`** (pro-d68: the row's new `feedback` list holds an item whose `last` is the `last` of no item of the old list, a NULL old list counting as empty; items that close tell nothing; the Event's `text` states the number of new and of open comments and says `cpd pr view`; `pr.feedback.checks` does not apply to it; contract §7.6 step 6); a conflict is no longer a reason (hs-d340: it queues an `update` Job, below). A pull request that became `merged`: the PR `status = merged`, `merged_at`, **Event** `pr-merged` `{pr, number, url, merged_at}`; when every PR that is not `cancelled` is then `merged`, the same transaction closes every pending review of the Plan `superseded` (reason "Plan review r<k> of Plan P is superseded: the Plan is done.") and appends `plan-done` `{prs: [{key, number, url, merged_at}]}`, once each time the Plan reaches `done`: again when a reopened Plan's last PR merges (§3.1, D2). The plan phase transitions to `done` after the reviews are closed. Pull requests no Plan owns get no Event, nor does one only in the review-request search: `pr-feedback` is unchanged, still only for the author's Plans.
- **The base follow** (hs-d325, hs-d326; contract §3.8 "Stacked PRs" step 8, §7.6 step 6): for a PR whose status is `open` and whose pull request a search lists `OPEN`, when `baseRefName` differs from the PR's `base`, the same transaction sets the PR's `base` to it and appends **Event** `pr-base-changed` `{pr, from, to, from_pr, to_pr, by: "poll", number, url}`. This is what happens when a base PR merges: GitHub deletes its branch and repoints the stacked pull request to `main` (or to the base PR's own base); the poll learns it and **cpd follows GitHub, it never retargets** (no command edits a pull request's base). The PR is then unstacked, or stacked on the next PR when the new base is that PR's Lead Branch (contract §2.2 "The base PR"). **No Gate, no draft, no `pr-feedback`, no status change**: the pull request's tree is unchanged, so a ready PR stays ready (hs-d326); a conflict with the new base queues an `update` Job (below). A base PR closed unmerged is not repointed by GitHub: nothing changes for the PR above it (contract §3.8 step 9). A poll that finds no base changed writes nothing for this.
- **Unstacking the PRs stacked on a merged PR** (hs-d301, contract §7.6 step 6): in the transaction that records a PR's merge, every `waiting` or `dispatch` PR whose `base` is the merged PR's Lead Branch (any Plan) gets the merged PR's own `base` and one `pr-base-changed` (`by` `poll`, `number` and `url` null) for its own Plan. No `pr-feedback`, no Gate, no start.
- **Starting the PRs that waited for it** (A9): `pr poll` does not start a PR itself. Engine E7, which runs after E5 in the same tick (§3.7), starts every `waiting` PR whose needs are now all `merged`  and whose base PR, if any, is `open`, from a fresh `fetch origin <base>` (a stacked PR from its base PR's head, §9.7); when `pr poll` was run by hand, the next tick does, or the Lead runs `pr start <key>` (§9.7).
- **A pull request closed without a merge** (A11, hs-d329): `pr-feedback` with `reasons: ["closed"]`, once; the poll never ends the PR: it stays `open` in cpd, every PR that needs it stays `waiting`, and the Lead decides. A Lead may have closed it itself, to reopen it on GitHub once it is ready again (contract §7.6 "Ready, never a draft"): reopened, the same pull request is in the search again and the poll writes its row `state` `open`, with no Event for the reopening; the PR was `open` throughout. To give it up the Lead runs `pr cancel <key>` (§9.6).
- **A conflict** (hs-d340, hs-d497; contract §3.7 E5, §7.6 step 6): in the same transaction, for the PR that is `open` and points to the pull request, when `mergeable` becomes `conflicting` (from anything else, once per transition; a moved base with no conflict, `merge_state` `behind` included, queues nothing), the row's `state` is `open`, the Plan is not archived, the PR's policy resolves and the PR has no `update` Job that is `queued` or `running`: one `update` Job is inserted, with the Event `job-added` `{kind: "update", by: "poll", reason}`, `reason` `conflict`. No `pr-feedback`, no Gate here, nothing becomes a draft (hs-d329). A pull request that is a draft anyway is only stored (`draft`, `merge_state` `draft`): no Event.
- **After a PR is merged or cancelled** (§7.2, hs-d138, hs-d142, hs-d517): for each PR that became `merged` or `cancelled` in this poll, kill that PR's Attempt windows; then, per Attempt of its Jobs whose worktree exists: `git worktree remove --force`, `git branch -D`; then its Lead worktree, first brought up to date by rule S (contract §7.4): kept when dirty (`git --no-optional-locks status --porcelain` not empty, untracked files included) or unpushed (`git rev-list --no-merges HEAD --not --remotes=origin` not empty, **unless** `HEAD` is the pull request's head commit or an ancestor of it: `headRefOid`, read with `gh pr view <n> --repo <owner/repo> --json headRefOid` only then, so a squash-merged PR whose branch GitHub deleted is cleaned up; contract §7.2 "The recorded head and the unpushed rule" — unpushed means non-merge commits of HEAD outside origin/* and the recorded head, so a lone rule-S merge of the base does not keep a merged PR's Lead worktree), else `git worktree remove` and `git -C <repo> branch -D <lead branch>`. No lock (hs-d243, contract §7.2): the removals of a PR wait until the PR is idle, i.e. every Attempt of its Jobs is `ended` and has its driver lock free (probed without creating the file); until then the poll skips that PR's removals with no Event and a later poll cleans it up, because the poll never waits and a merge or an A9 must not lose its worktree (hs-n124). Best effort per item: a failure does not stop the others. Remote branches are never touched. A kept or failed Lead worktree appends one **Event** `error` `{source: "pr poll", code: "cleanup_failed", details: {plan, pr, path, reason}}` (`reason`: `dirty`, `unpushed` or git's stderr tail), only from the poll that saw the merge or cancel. Every later poll retries the merged or cancelled PRs that still have a path on disk, and those retries append no Event. It is per PR, not at `done`; PRs closed unmerged and abandoned Plans are not cleaned up here (G5). A cancelled PR's paths come from contract §7.1's naming rule and lead_worktree is null, so only paths that exist and are worktrees of the repo on the PR's branches are touched.

Output: `polled`, the number of distinct pull requests in either search; `changed`, one entry for each pull request whose row was inserted or updated; `base_changed`, the `{plan, pr, from, to}` of each PR whose `base` followed GitHub (empty when none); `feedback`, `merged` and `done`, the `{plan, pr}` pairs that got `pr-feedback` or `pr-merged`, and the Plans that became `done` (hs-d565); `update_jobs`, the `{plan, pr, job, reason}` of each `update` Job the poll queued (hs-d340); `rate_limit`, the search's `cost`, `remaining` and `reset_at` (E5 reads it, §3.7); and, only with a Plan, `prs`, one entry for each of its PRs in key order that is not `cancelled`, with its `status` and its `pull_request` row (or null).

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
| `git merge-tree` reports no conflict, and the rebase has no unexpected one; else the rebase is left **in progress** in the Lead worktree (nothing pushed, nothing else changed); `details.files` the conflicted paths, `details.worktree` its path, `details.next` the `--continue` command, `details.abort` the `--abort` command | `rebase_conflict` | 3 |
| no rebase is already in progress in the Lead worktree (a stop of an earlier run); `details.next`, `details.abort` as above | `rebase_in_progress` | 3 |
| the Lead worktree has no tracked local changes, which `git rebase` would refuse (checked only when a conflict needs the rebase there); `details.files` lists them | `lead_worktree_dirty` | 3 |
| guard U: an Attempt of the PR whose compare-and-swap is in the snapshot and whose driver lives ends within 60 s; `details.attempt` names it; rerun | `cas_lost` | 3 |
| the Lead Branch's compare-and-swap succeeds within 3 tries (merges kept moving it) | `cas_lost` | 3 |
| the PR's policy resolves (contract §8.3 "Validation", `details` as there): its Context has an owner | `owner_unset` | 3 |
| its owner has a policy file | `owner_unconfirmed` | 1 |
| its policy files are valid | `policy_invalid` | 1 |
| the `pre-start` passes before the Gate runs; `details.tail` is its last 60 lines, `details.log` the full log `run/<plan>/logs/pr-<key>.pre-start.log` | `gate_error` | 1 |
| the Gate passes on the rebased Lead Branch (§7.3, contract §7.4, hs-d679); `details.tail` is its last 60 lines, `details.log` the full log | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |
| the caller is not an agent, OR the incident is off, OR the resolved PR's Plan is not held (hs-d643): `CPD_SESSION` set and non-empty names an agent; a held Plan is in the incident; `details` is `{pr, plan, incident}` | `incident_hold` | 3 |

The Lead worktree's local changes refuse nothing (hs-d243; `dirty_target` is no longer a refusal of this command).

Effects: no lock (hs-d243); merges into the PR go on during the command. §7.6 steps 1–9: `fetch origin <base>`; the snapshot `S`; `merge-tree`; a temporary rebase worktree detached at `S` for the rebase, giving `R'`; guard U (R for an Attempt of the PR with a dead driver whose compare-and-swap is in `S`; a wait of at most 60 s for one with a live driver); the lease `X0`, the tracking ref `refs/remotes/origin/<lead branch>` read then (what cpd last saw of the remote branch, its own pushes and those of the merges guard U waited for included, so a push cpd has not seen refuses instead of being overwritten; Escalation nl-pr-a1-q1); `update-ref -m "cpd: pr rebase <plan>/<key>" refs/heads/<lead branch> R' S`, back to the snapshot when the ref moved; rule S on the Lead worktree (a stop appends `error` `lead_worktree_stale` and does not fail the command); with `gate.pr` is `local` (contract §8.3, hs-d292) the Gate in the temporary rebase worktree, `pre-start` then `pre-merge` from the PR's policy (hs-d679), outputs to `run/<plan>/logs/pr-<key>.pre-start.log` and `pr-<key>.pre-merge.log`, then the temporary worktree removed; the PR re-read; `push --porcelain --force-with-lease=refs/heads/<lead branch>:<X0> origin R':refs/heads/<lead branch>`, then, when the Lead Branch moved past R' during the Gate, rule P with its tip (a failure of either push is `git_failed`, except a rejection of the first when the remote tip already contains R', which is a success: two `pr rebase` of the PR at once push the same R', and the second can lose the race inside the remote; Escalation nl-pr-a1-q2). On `gate_failed` or `gate_error` nothing is pushed; the rebased Lead Branch stays (the compare-and-swap moved it before the Gate), and the Lead appends fix Jobs to the PR, then runs `pr rebase --pr <key>` again once they are Merged. **Rows**: none; the Ledger does not record the Lead Branch's SHA, so there is no Event. On an unexpected rebase conflict: `git rebase --abort` in the temporary worktree, which is removed, then the conflict stop below.

```json
{"ok": true, "plan": "pay-4521", "pr": "api", "rebased": true, "old": "4b825dc…", "new": "77aa01e…", "pushed": true}
```

**An adopted PR** (`head_ref` set, §9.4) takes the **catch-up**: the command never rebases onto the base. It fetches `origin <head_ref>` and rebases the Lead Branch's commits that `origin/<head_ref>` lacks onto `origin/<head_ref>` (the machinery above: gate worktree, guard U, compare-and-swap of the Lead Branch, rule S, the Gate when `gate.pr` is `local`, `--continue` and `--abort`), then pushes with rule P: `git push --porcelain origin <sha>:refs/heads/<head_ref>`, no lease, no force; history stays linear and is never deleted. It is allowed in `open`, and in `dispatch` once every Job of the PR but its `open` and `update` Jobs is `merged` or `cancelled` (`jobs_not_done` otherwise, as `pr open`). When `origin/<head_ref>` is already in the Lead Branch the outcome is `already`. The Lead runs it when an `open` or `update` Job ends `push_failed`, or when `pr open` or `pr push` is refused `push_rejected`; cpd never runs it by itself. A conflict of an adopted PR with its base stays the `update` Job's: its Worker merges the base (a merge commit) and cpd pushes plain.

Idempotent: yes; a base that has not moved gives `"rebased": false`, and only the Gate (with `gate.pr` `local`) and the push run. A run that stops at a conflict is repeatable only through `--continue`/`--abort`: a second plain run answers `rebase_in_progress`.

**The conflict stop** (contract §7.6). On a conflict nothing is aborted: the Lead worktree holds the rebase, stopped at the first conflicting commit, and the Lead Branch ref is unchanged until the rebase finishes. The error:

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
| `GIT_EDITOR=true git rebase --continue` does not stop at the next conflicting commit; if it does, `details.files` are that commit's, and the Lead repeats | `rebase_conflict` | 3 |
| `git rebase --continue` fails otherwise; `details.stderr`; the rebase stays in progress | `git_failed` | 1 |
| `gate_error` and `gate_failed` as the rows above; nothing is pushed, the rebased Lead Branch stays | `gate_error` / `gate_failed` | 1 / 3 |
| the push, as above: `--force-with-lease` against `X0`, `X0` read after guard U | `git_failed` | 1 |

Effects: no lock; guard U and the lease as `pr rebase` (the snapshot `S` is the tip the rebase started from, `rebase-merge/orig-head`); `GIT_EDITOR=true git -C <lead worktree> rebase --continue` (no editor; messages kept); `R'` the Lead worktree's `HEAD`; no rule S (that worktree was the one rebased); the Gate in the Lead worktree at `R'` and the push exactly as `pr rebase` (contract §7.4 R2, hs-d679), the Gate only when `gate.pr` is `local`. No `fetch`: the rebase ends on the `origin/<base>` it started from. **Rows**: none; no Event. Output as `pr rebase`, `"rebased": true`, `old` = `S`, `new` = `R'`. Idempotent: no; a second run is `no_rebase_in_progress`.

### `pr rebase --abort`

`git -C <lead worktree> rebase --abort`: the Lead Branch and the Lead worktree as before the stop. Refuses `no_rebase_in_progress` (exit 3) when none is in progress. It asks no PR status, runs no Gate and no `fetch`, pushes nothing, appends no Event; then rule S on the Lead worktree (a stop appends `error` `lead_worktree_stale`, never fails it). Output `{"ok": true, "plan": "pay-4521", "pr": "api", "aborted": true, "tip": "4b825dc…"}`. Idempotent: no; a second run is `no_rebase_in_progress`.

### 9.4 `pr add`

```
cpd pr add <key> --repo <path> --title <text> [--adopt <number> [--confirm-author <login>]] [--shape <tdd|docs|config>] [--base <branch> | --base <plan>/<key>] [--needs <r1,r2> | --no-needs] [--plan <id>]
```

Adds a PR to the Plan, `waiting` (R34, A13, §3.8). In phases `grounding` and `plan` it is part of the plan fingerprint (§2.3), so the Plan needs a review that includes it; in `dispatch`, `deploy` and `done` it is **appended**, like `job add`, and engine E7 starts it once its needs are `merged` (at once when it has none—stated with `--no-needs`—unless it is appended after the lock, when one flag is required, pro-d46). In `deploy` and `done` it also **moves the Plan back to `dispatch`** (hs-d87: a merged PR can hold a bug or a regression, so a Plan takes follow-up work at any time; no Plan review of the PR itself, D4; the Jobs added to it as drafts are reviewed, §4.6). A follow-up PR is registered at once, even when it cannot run yet (hs-d239, hs-d241): `pr add <key> --repo … --needs …`, then one stub draft per planned Job (`job add --draft --title …`, §5.1). When the Plan has a `merged` PR on the legacy Lead Branch `cpd/<plan>`, it first **moves that branch** to `cpd/<plan>/<that PR's key>` (below).
Adds a PR to the Plan, `waiting` (R34, A13, §3.8). In phases `grounding` and `plan` it is part of the plan fingerprint (§2.3), so the Plan needs a review that includes it; in `dispatch`, `deploy` and `done` it is **appended**, like `job add`, and engine E7 starts it once its needs are `merged` (at once when it has none, §3.7). In `deploy` and `done` it also **moves the Plan back to `dispatch`** (hs-d87: a merged PR can hold a bug or a regression, so a Plan takes follow-up work at any time; no Plan review of the PR itself, D4; the Jobs added to it as drafts are reviewed, §4.6). A follow-up PR is registered at once, even when it cannot run yet (hs-d239, hs-d241): `pr add <key> --repo … --needs …`, then one stub draft per planned Job (`job add --draft --title …`, §5.1). With `--adopt` it adopts an existing pull request whose head branch is not `cpd/<plan>/<key>`: cpd works it like a Plan (Jobs that address review comments and failing CI checks) and pushes to the pull request's own head branch, never a second pull request (pro-n8). When the Plan has a `merged` PR on the legacy Lead Branch `cpd/<plan>`, it first **moves that branch** to `cpd/<plan>/<that PR's key>` (below).

| Flag | Meaning |
|---|---|
| `<key>` | PR key (§2.1), chosen by the Lead, e.g. `schema`, `web` |
| `--repo <path>` | required: the PR's repo; stored as the realpath of its main worktree. Several PRs of one Plan may share a repo. Its `origin` must be a GitHub repo of the Context's owner (below) |
| `--title <text>` | required (hs-d285): the PR's own title, which names its change and becomes the pull request's title and then the squash commit's subject on `main`. 1–72 characters, one line, no control characters, else `usage` (§1.5). `pr add` is the only place a PR is created |
| `--base <branch>` | the branch the pull request targets; default the repo's `origin` HEAD branch, else `main` (cpd 1 §11 `workflow init`, kept) |
| `--base <plan>/<key>` | **stack on a PR** (hs-d324, hs-d325): the PR `<key>` of Plan `<plan>` (this Plan's own included) becomes the **base PR**, and the stored `base` is its Lead Branch, so the PR starts from that branch's head and its pull request targets it (contract §3.8 "Stacked PRs"). The value is a PR when it has the form `<plan>/<key>` and a Plan `<plan>` exists, else a branch (contract §2.2 "A PR reference") |
| `--needs <r1,r2>` | PR references (contract §2.2): keys of other PRs of the Plan (`<plan>/<key>` of the Plan itself is read as the bare key; another Plan's is refused `pr_other_plan`, hs-d522), that must be `merged` before this one starts, comma-separated, no duplicates (`usage`). Default: in `grounding` and `plan` none; in `dispatch` none: one flag is required (`needs_required`, pro-d46); in `deploy` or `done` every `merged` PR of the Plan |
| `--no-needs` | no needs, the way a PR with no needs is stated in `dispatch` when the default does not apply; exclusive with `--needs` (`usage`); in `grounding`, `plan`, `deploy` or `done` it changes the default to explicitly none |
| `--title <text>` | required (hs-d285): the PR's own title, which names its change and becomes the pull request's title and then the squash commit's subject on `main`. 1–72 characters, one line, no control characters, else `usage` (§1.5). `pr add` is the only place a PR is created. Under `--adopt` it is a local label (it names the stub Jobs): cpd never sends it to GitHub, the pull request keeps its own title |
| `--adopt <number>` | adopt the existing pull request `<number>` of `--repo`'s `origin` (below). The PR's base is the pull request's `baseRefName` (it must exist as `origin/<base>`: `base_not_found`) and its head is `head_ref`, stored in `prs.head_ref` |
| `--confirm-author <login>` | the pull request's author, when it is not the User (`adopt_foreign`); accepted and does nothing when the author is the User. Needs `--adopt` |
| `--base <branch>` | exclusive with `--adopt` (`usage`, exit 2: the base is the pull request's). The branch the pull request targets; default the repo's `origin` HEAD branch, else `main` (cpd 1 §11 `workflow init`, kept) |
| `--base <plan>/<key>` | exclusive with `--adopt`. **Stack on a PR** (hs-d324, hs-d325): the PR `<key>` of Plan `<plan>` (this Plan's own included) becomes the **base PR**, and the stored `base` is its Lead Branch, so the PR starts from that branch's head and its pull request targets it (contract §3.8 "Stacked PRs"). The value is a PR when it has the form `<plan>/<key>` and a Plan `<plan>` exists, else a branch (contract §2.2 "A PR reference") |
| `--needs <r1,r2>` | PR references (contract §2.2): keys of other PRs of the Plan (`<plan>/<key>` of the Plan itself is read as the bare key; another Plan's is refused `pr_other_plan`, hs-d522), that must be `merged` before this one starts, comma-separated, no duplicates (`usage`). Default: in phase `deploy` or `done` (the reopen, hs-d118), **every `merged` PR of the Plan**; in other phases none |
| `--no-needs` | no needs, also in phase `deploy` or `done`; exclusive with `--needs` (`usage`) |
| `--shape <tdd\|docs\|config>` | the template of stub drafts the PR starts with (hs-d341, contract §2.2 "Job kinds"), default `tdd`: `<key>-docs` (kind `docs`), `<key>-tests` (`test`, needs `<key>-docs`), `<key>-impl` (`impl`, needs `<key>-tests`). **`docs`**: only `<key>-docs`; **`config`**: only `<key>-config`, kind `work`: a docs-only or config-only PR says so here and has no test Job. **Stored in `prs.shape` and enforced** (hs-d497; contract §2.2 "Job kinds"): the kinds the PR may have (`shape_violation` at `job add`, `job edit --kind`) and the ones a `tdd` PR must keep (`shape_incomplete` at `plan review`, `plan lock` and `job cancel` typed by the Lead); never changed afterwards. `pr cancel` of the whole PR is the way out and cancels the `test` and `impl` Jobs itself (§9.6, hs-d573) `pr_exists`'s comparison includes it |

| Precondition | Code | Exit |
|---|---|---|
| the Plan is not archived | `archived` | 3 |
| `--repo` is inside a git repository (`git -C <path> rev-parse --git-common-dir`) | `not_a_git_repo` | 3 |
| the Plan's Context has an owner (contract §8.3); the message names `cpd context owner <login> --context <c>` | `owner_unset` | 3 |
| the repo is keyed from its `origin` (below); `details.url` (null when there is no `origin`) | `repo_unkeyed` | 1 |
| the repo's owner is the Context's; `details.owner` (the Context's), `details.repo_owner` | `owner_mismatch` | 3 |
| the owner has a policy file, `$CPD_HOME/policy/<owner>/owner.toml`; `details.owner` | `owner_unconfirmed` | 1 |
| the owner's and the repo's policy files are valid; `details.file`, `details.key` | `policy_invalid` | 1 |
| `--base` is a branch: it exists, `refs/remotes/origin/<base>`, or `refs/heads/<base>` when the repo has no `origin` | `base_not_found` | 4 |
| `--base <plan>/<key>`: the PR exists in that Plan; `details` is `{plan, pr}` | `base_pr_unknown` | 3 |
| the base PR and every need are PRs of the Plan, not of another Plan; `details` is `{pr, reference, plan}` (the PR being added, the reference as given, the other Plan) | `pr_other_plan` | 3 |
| the base PR is not `cancelled` and not `merged` (name its base branch instead); a need is not `cancelled`; `details.pr` | `base_pr_unavailable` (base PR), `needs_unavailable` (need) | 3 |
| the base PR is in the same repo as this PR (the same realpath); `details` is `{pr, repo, base_repo}` | `base_pr_other_repo` | 3 |
| a PR of an archived Plan is referenced only as a `merged` need; `details` is `{plan, next}`, `next` the `cpd plan unarchive` command | `archived` | 3 |
| the base PR is not also in `--needs` (a need is merged before this PR starts, a base PR is the implied need met when it is `open`) | `usage` | 2 |
| no PR of the Plan has this key, `cancelled` ones included, unless its repo, base, title, shape and needs equal the resolved flags, the default needs included (then `already`) | `pr_exists` | 3 |
| in phase `dispatch`, one of `--needs` or `--no-needs` is required; an existing PR key is compared first (`already` when the flags match, else `pr_exists`), so a rerun is never refused (idempotence, pro-d46) | `needs_required` | 3 |
| (`--adopt`) the pull request's state is `OPEN`; `details` is `{number, url, state}` | `adopt_not_open` | 3 |
| (`--adopt`) it is not from a fork (`isCrossRepository` false); `details` is `{number, url}` | `adopt_fork` | 3 |
| (`--adopt`) its head branch does not start with `cpd/`; `details` is `{number, head_ref}` | `adopt_cpd_branch` | 3 |
| (`--adopt`) no PR of any Plan with the same repo and head_ref is `waiting`, `dispatch` or `open`; `details` is `{head_ref, plan, pr}` | `adopt_taken` | 3 |
| (`--adopt`) its author is the User, or `--confirm-author` equals the author's login (case-insensitively); `details` is `{number, url, author, user, confirm: "--confirm-author <author>"}`; the message tells the Lead to ask the User first (pro-n4) | `adopt_foreign` | 3 |
| no PR of the Plan has this key, `cancelled` ones included, unless its repo, base, title, shape, head_ref and needs equal the resolved flags, the default needs included (then `already`) | `pr_exists` | 3 |
| every need is a PR of the Plan; `details.needs` lists the unknown keys | `needs_unknown` | 3 |
| no need is `cancelled`; `details.needs` lists them | `needs_unavailable` | 3 |
| the graph of PR needs and base PRs stays acyclic across every Plan (a PR stacked on `B` cannot be needed, directly or not, by `B`: it would wait for `B` to start while `B` waits for it to merge); `details.cycle` lists the references | `dependency_cycle` | 3 |
| `refs/heads/cpd/<plan>/<key>` does not exist in the repo, and neither does `refs/heads/cpd/<plan>`, except the legacy branch of a PR of the Plan that is `merged` (moved below); `details.branch` names it | `branch_exists` | 3 |
| (`--shape tdd`) the PR's repo policy has `[test-commands]` (contract §7.7; none when there is no repo file or the file lacks the table); the message names `--shape docs`, `--shape config` and the table | `tests_unconfigured` | 3 |
| none of the Job keys the shape lays out exists in the Plan; `details.jobs` | `job_key_exists` | 3 |
| the legacy move's `git push origin --delete cpd/<plan>` succeeds; `details`: `branch`, `repo`, `stderr` | `push_failed` | 1 |

The five `adopt_*` rows are checked in the order above, after every other check of this command, with nothing written. `--confirm-author` without `--adopt` is `usage`.

**What `--adopt` reads** (A2), with the working directory the repo, so `gh` resolves the pull request through its `origin`: `gh pr view <number> --json number,url,state,isCrossRepository,headRefName,baseRefName,author` and `gh api user --jq .login`. The first answers `{"number":7,"url":"https://github.com/acme/pay/pull/7","state":"OPEN","isCrossRepository":false,"headRefName":"feat/x","baseRefName":"main","author":{"login":"alice"}}`. The author is the User when `author.login` equals the login of `gh api user`, compared case-insensitively (pro-n4: the User's own pull request is adopted without asking; another author's needs a warning and the User's confirmation first). A `gh` failure is `gh_failed`. The pull request's number is not stored: `pr open` finds it by head (`gh pr list --head <head_ref> --state all`, §9.1).

Any phase is allowed: `grounding`, `plan`, `dispatch`, `deploy` and `done`, so no `illegal_phase`. A new PR has no dependents, so it creates no cycle of its own; `dependency_cycle` is possible only through `pr edit` (§9.5).

**The template** (hs-d341). The shape's Jobs are **stub drafts** (§5.1: a title, no Tier and no Spec): `jobs` rows with their `kind`, the titles `Docs: <PR title>`, `Red tests: <PR title>`, `Implement: <PR title>` and `Config: <PR title>`, `status = draft`, `appended` 0 before the lock and 1 after it (as `job add --draft` writes them), no fingerprint, and for `tdd` the `dependencies` rows `<key>-tests → <key>-docs` and `<key>-impl → <key>-tests`. The Lead completes each (`job edit --tier --spec`, §5.2) or cancels the ones it does not need (never, one by one, the `test` and `impl` Jobs of a `tdd` PR: `job cancel` is refused `shape_incomplete`; `pr cancel` of the whole PR cancels them itself, §9.6, hs-d573), and adds the PR's other Jobs with `job add --pr`. The engine runs nothing of them until the batch is approved (§4.6, §4.8). The PR's `open` Job is a **stub draft** laid out by the template, Tier `open_tier` (contract §8.2), no Spec or Dependencies (its Spec is fixed by cpd); the Lead may cancel it. Engine E4 (§3.7) is a **fallback**: when the PR becomes `dispatch`, if it has no `open` Job at all, E4 adds a fresh one `queued`; a `draft`, `queued`, `running`, `failed` or `cancelled` one it leaves alone, and it still promotes a `draft` opener to `queued` when no other draft of its PR remains (contract §3.7, hs-d573).

**The repo's key** (contract §8.3, hs-d288). `pr add` parses the repo's `origin` once, at the boundary: `git -C <repo> remote get-url origin`, which must be `https://github.com/<owner>/<name>` or `git@github.com:<owner>/<name>`, each with or without `.git`, `<owner>` and `<name>` valid by contract §2.1 (stored lowercased). Nothing else is accepted: no `origin`, another host, or an SSH host alias (`git@github-work:<work-owner>/x.git`) is `repo_unkeyed`; cpd never asks the network to resolve one. `<owner>` must be the Context's (`owner_mismatch`): one GitHub owner per Context, and a Plan may span several repos of it. The PR stores `<name>` only; its owner is the Context's.

**The legacy move** (contract §3.8, D3, hs-d118). Every Plan locked before R34 stored `prs.lead_branch = 'cpd/<plan>'`, and git cannot hold both `cpd/<plan>` and `cpd/<plan>/<key>`. When a PR of the Plan has `lead_branch = cpd/<plan>` and status `merged`, `pr add` does, before the insert and in this order, with `<k>` that PR's key:
1. `git -C <repo> branch -m cpd/<plan> cpd/<plan>/<k>`, when `refs/heads/cpd/<plan>` exists (a worktree that has it checked out follows the rename; unverified on git 2.43);
2. `git -C <repo> push origin --delete cpd/<plan>`, when the repo has `origin` and `git ls-remote --heads origin cpd/<plan>` lists it; a failure is `push_failed` and the database is unchanged;
3. in the transaction of the insert below, `UPDATE prs SET lead_branch = 'cpd/<plan>/<k>'` for that PR.

A legacy branch of a PR that is not `merged` still refuses `branch_exists`.

Effects: **rows**, one transaction: in `deploy` or `done`, `plans` (`phase = dispatch`, `done_at = null`); `prs` (`status = waiting`, `repo`, `name`, `base` (the base PR's `lead_branch` when stacked), `title`, `shape`, `lead_branch = cpd/<plan>/<key>` (with `--adopt`, the local branch of the Lead worktree; and `head_ref`, the pull request's head branch, set here only: the trigger `prs_head_ref_fixed` refuses any later change with `pr_locked`, and the partial unique index `prs_head_ref_live` allows one live PR per repo and head), `created_txn`, `created_at`; `lead_worktree` null); one `pr_needs` row per need; the legacy PR's `lead_branch` update (step 3); and the shape's stub drafts (below). **Events**, in this order: in `deploy` or `done`, `plan-phase` `{from: "deploy"|"done", to: "dispatch"}`; then `pr-added` `{pr, repo, base, base_pr, title, needs, appended, head_ref, author}` (`head_ref` and `author` null for a cpd PR), `appended` true in phases `dispatch`, `deploy` and `done`; then one `job-added` (`kind`, `status` `draft`) per stub draft. Engine E7 then starts the PR like any appended PR (at once when it has no needs). No tmux and no other git write: the Lead Branch and Lead worktree are created when the PR starts (§3.8, §7.2).

```json
{"ok": true, "pr": {"key": "web", "repo": "/home/clayton/projects/web", "name": "web", "base": "main", "base_pr": null, "title": "Add the web client", "needs": ["api"], "status": "waiting", "lead_branch": "cpd/pay-4521/web", "head_ref": null, "appended": false, "jobs": ["web-docs", "web-tests", "web-impl"]}}
```

An adopted pull request (`--adopt 3748`): the same object with `"head_ref": "feat/EEC-3238/review-submit-polish"`, `"base": "main"`, `"lead_branch": "cpd/pay-4521/card"`; `plan show` (§4) carries `head_ref` in its PR objects too.

A PR stacked on a PR of its Plan, needing another (`--base pay-1/api --needs schema`): `"base": "cpd/pay-1/api", "base_pr": "pay-1/api", "needs": ["schema"]`.

Idempotent: yes, as stated; the same flags again, `--adopt` included, are `already` (the comparison includes head_ref); every step of the legacy move is too (A3): a rerun after a crash between the local rename and the transaction finds `cpd/<plan>/<k>` and no `cpd/<plan>`, skips what is done and completes. The comparison uses the resolved repo and base (a base PR by its Lead Branch) and needs (as references), so a rerun without `--base` matches a PR whose base was defaulted to the same branch.

### 9.5 `pr edit`

```
cpd pr edit <key> [--repo <path>] [--base <branch> | --base <plan>/<key>] [--needs <r1,r2> | --no-needs] [--title <text>] [--plan <id>]
```

Changes a `waiting` PR before the lock (A13), and, with `--title`, a PR's title in any phase (below). `--needs` replaces the whole list; `--no-needs` removes every need. `--base` and `--needs` take what `pr add` takes (§9.4): a branch or, to stack, a base PR `<plan>/<key>` of the Plan, and PRs of the Plan; `--base <branch>` on a stacked PR unstacks it. A PR that another PR is stacked on keeps its repo: `--repo` is refused `pr_in_use` (`details.prs`), because the stacking is derived from the repo and the base and a change would silently unstack the PR above it (hs-d324). The PR's Jobs stay with it. A changed `--repo` keeps the stored base unless `--base` is given, and that base must exist in the new repo; the new repo's `origin` is parsed as `pr add` parses it (§9.4 "The repo's key") and replaces `name`. After the lock, the link (`--needs` and `--base`) cannot be changed, so it is stated at `pr add` with a required flag (pro-d46, §2.2).
Changes a `waiting` PR before the lock (A13), and, with `--title`, a PR's title in any phase (below). `--needs` replaces the whole list; `--no-needs` removes every need. `--base` and `--needs` take what `pr add` takes (§9.4): a branch or, to stack, a base PR `<plan>/<key>` of the Plan, and PRs of the Plan; `--base <branch>` on a stacked PR unstacks it. An **adopted** PR (`head_ref` set, §9.4) takes its repo and its base from the pull request: `--repo` or `--base` is refused `pr_adopted` (`details` is `{pr, flag}`), checked first. A PR that another PR is stacked on keeps its repo: `--repo` is refused `pr_in_use` (`details.prs`), because the stacking is derived from the repo and the base and a change would silently unstack the PR above it (hs-d324). The PR's Jobs stay with it. A changed `--repo` keeps the stored base unless `--base` is given, and that base must exist in the new repo; the new repo's `origin` is parsed as `pr add` parses it (§9.4 "The repo's key") and replaces `name`.

Changes a `waiting` PR before the lock (A13), and, with `--title`, a PR's title in any phase (below). `--needs` replaces the whole list; `--no-needs` removes every need. The PR's Jobs stay with it. A changed `--repo` keeps the stored base unless `--base` is given, and that base must exist in the new repo; the new repo's `origin` is parsed as `pr add` parses it (§9.4 "The repo's key") and replaces `name`.

`--title <text>` (hs-d285) works in **every phase and every PR status but `merged` and `cancelled`** (a PR that is over; `cancelled` is the PR status that stands for a closed PR, §3.8): 1–72 characters, one line, no control characters, else `usage` (§1.5). The title is **not a Locked field**: it is outside the plan fingerprint (§2.3) and `prs_locked`'s columns (§4.4), so editing it never invalidates a review or a lock, and the phase and status rules below (`pr_locked`, `illegal_transition`) apply to the other flags only. A `merged` or `cancelled` PR's title is history: `pr_ended`. When the PR has an **open** pull request, the command also retitles it on GitHub, through the seam of §9.1, with `gh api -X PATCH repos/<owner>/<repo>/pulls/<n> -f title=<text>`. It must **not** use `gh pr edit`: on 2026-10-19 that fails on this repo with "Projects (classic) is being deprecated … (repository.pullRequest.projectCards)", while the `gh api` PATCH works (both verified on #99). A failing GitHub update is `gh_failed` and leaves the stored title unchanged.

| Precondition | Code | Exit |
|---|---|---|
| at least one field flag | `nothing_to_edit` | 2 |
| the PR is not adopted when `--repo` or `--base` is given; `details` is `{pr, flag}` | `pr_adopted` | 3 |
| with `--title`, the PR is not `merged` or `cancelled` | `pr_ended` | 3 |
| for the flags other than `--title`: the phase is `grounding` or `plan`; after the lock a PR is Locked (§4.4 `prs_locked`) | `pr_locked` | 3 |
| for the flags other than `--title`: the PR is `waiting` (a `cancelled` one is terminal) | `illegal_transition` | 3 |
| `--repo` is inside a git repository | `not_a_git_repo` | 3 |
| with `--repo`: the new repo is keyed from its `origin`, as for `pr add` (§9.4) | `repo_unkeyed` | 1 |
| with `--repo`: the new repo's owner is the Context's, as for `pr add` | `owner_mismatch` | 3 |
| the base exists in the PR's repo (the new one with `--repo`), as for `pr add` | `base_not_found` | 4 |
| a base PR, as for `pr add`: `base_pr_unknown`, `pr_other_plan`, `base_pr_unavailable`, `base_pr_other_repo` (against the new repo with `--repo`), `archived` (§9.4) | as there | 3 |
| every need is a PR (of the Plan, or of another by its reference), other than this one | `needs_unknown` | 3 |
| no need is `cancelled` | `needs_unavailable` | 3 |
| the graph of PR needs and base PRs stays acyclic across every Plan; `details.cycle` lists the references | `dependency_cycle` | 3 |
| with `--repo`: no PR of the Plan is stacked on this PR (it has no dependents by base); `details.prs` lists them | `pr_in_use` | 3 |
| with `--repo`: neither `refs/heads/cpd/<plan>/<key>` nor `refs/heads/cpd/<plan>` exists in the new repo | `branch_exists` | 3 |
| with `--title` and an open pull request: the `gh api -X PATCH` succeeds; `details` is `{argv, exit, stderr}` | `gh_failed` | 1 |

Effects: **rows**, one transaction: the changed `prs` columns (`name` with `repo`; `base`, a base PR's Lead Branch when stacked; `title`); `pr_needs` rows deleted and inserted to match (§4.4 allows deletion only in `grounding` and `plan`). **Event** `pr-edited` `{pr, fields: [name]}`, listing only the fields whose value changed (`repo`, `base`, `needs`, `title`). The plan fingerprint changes with `repo`, `base` and `needs`, never with `title` (§2.3). With `--title` and an open pull request, the GitHub update runs before the transaction, and a failure writes nothing.

```json
{"ok": true, "pr": {"key": "web", "status": "waiting"}, "fields": ["base", "needs"]}
```

Idempotent: yes; when no value changes it writes nothing and returns `"already": true`.

### 9.6 `pr cancel`

```
cpd pr cancel <key> [--superseded-by <plan>/<key>] [--plan <id>]
```

`--superseded-by <ref>`: the PR is `dispatch` and its work was carried by another PR, `<ref>` (`<plan>/<key>`, or a bare `<key>` of the Plan); its `merged` Jobs no longer hold it (hs-d301, contract §3.8 "Ending a `dispatch` PR whose work was superseded").

Three uses (contract §3.8), and a fourth with `--superseded-by`. Before the lock it removes a `waiting` PR from the Plan: `waiting → cancelled` (A13). After the lock it ends a started PR with nothing left to do, `dispatch → cancelled`, so a PR whose Jobs were all cancelled, or that never had one, cannot keep the Plan out of `done` (hs-n271); and a `dispatch` PR whose merged Jobs are all review Jobs (pro-d77, contract §3.8 "Ending a `dispatch` PR that holds only review Jobs"), which has no work to carry and loses nothing when cancelled; and an **`open`** PR whose pull request was closed on GitHub without a merge, `open → cancelled` (hs-d329). The PR keeps its key either way, so `pr add` cannot reuse it. Locked Jobs are never edited or deleted: only the PR's status moves. The PR's `draft`, `queued` and `failed` Jobs, of every kind, go with it (below, hs-d573), so they need not be cancelled first; for a `waiting` PR the needs on it still have to go (`pr edit`). `pr cancel` is never refused by the shape check: the last `test` and `impl` Jobs of a `tdd` PR are cancelled with it, which is how a `tdd` PR left in draft is removed (hs-n421, hs-d573); and after it no Job of the PR is `queued` or `running`, so `plan archive` is not blocked by it. The status decides the path; a `cancelled` PR is `already` in any phase; a `merged` PR is refused.

| Precondition | Code | Exit |
|---|---|---|
| the PR is `open`, `dispatch`, `waiting` or `cancelled` (`cancelled`: `already`, in any phase; `merged`: `illegal_transition`); a `dispatch` PR only after the lock; an `open` PR is in phase `dispatch` or `deploy` | `illegal_transition` | 3 |
| `--superseded-by` on a PR that is not `dispatch` (`cancelled`: `already`), or naming a superseding PR that is `cancelled` | `illegal_transition` | 3 |
| `--superseded-by` names a PR that is not in any Plan (`details.plan`, `details.pr`) | `pr_not_found` | 4 |
| `--superseded-by` names a PR that is not of a Plan of this Context | `pr_not_found` | 4 |
| `--superseded-by` names the PR itself | `usage` | 2 |
| **`waiting`, `dispatch`**: no Job of the PR is `running`, and none is `merged` unless `--superseded-by` is given (with it `merged` Jobs are work another PR carried and do not hold it), or the PR's merged Jobs are all review Jobs, which commit nothing and do not hold it (pro-d77); a `draft`, `queued` or `failed` Job, of any kind, never holds it: this command cancels it (below, hs-d573). And no PR that is not `cancelled`, of any Plan, needs it or is stacked on it (contract §3.8 "Stacked PRs"); `details.jobs` lists the holding Jobs and `details.prs` the PRs, as references (`<plan>/<key>` for another Plan's) | `pr_in_use` | 3 |
| **`open`**: the PR's `pull_requests` row is `state = closed`, as the last poll stored it (no `gh` call: run `pr poll` first); `details` is `{pr, number, url, state}` | `pr_not_closed` | 3 |
| **`open`**: no PR of the Plan that is not `cancelled` needs it or is stacked on it (its Jobs are not checked: a `draft`, `queued` or `failed` Job never holds it, and it is cancelled by this command, below; hs-d573); `details.prs` lists the PRs as references | `pr_in_use` | 3 |
| **`open`**: the PR is still `open` at the transaction | `cas_lost` | 3 |

Every check runs before the first write: a refused `pr cancel` writes nothing (hs-d573).

Effects, **`waiting`** and **`dispatch`**: **rows**, one transaction: the PR `status = cancelled`, and every Job of the PR whose status is `draft`, `queued` or `failed`, of every kind (`docs`, `test`, `impl`, `work`, review, `open`, `update`), cancelled in that transaction with `status = cancelled`, the constant reason `pr cancelled` (also with `--superseded-by`), `failed_cause` cleared and `cancel_requested_at` set (one `job-cancelled` each; a Job already `cancelled` is untouched and gets no Event; one that is `running` is a live Job and refuses `pr_in_use`; contract §3.8, hs-d573); a `dispatch` PR also loses its `lead_worktree` (the workflow worktree) and `started_at` (the table allows neither on a `cancelled` PR). **Worktrees are removed at the next PR poll** (the same clean-up as merged PRs, §9.2 "After a PR is merged or cancelled", hs-d517: idle check, dirty or unpushed kept with one `cleanup_failed` Event from the first poll that sees it, later polls retry silently). **Event** `pr-cancelled` `{pr}`, and `superseded_by` (the superseding PR as a reference: the bare key in the Plan itself, else `<plan>/<key>`) with `--superseded-by`. Before the lock the plan fingerprint changes (§2.3: it covers the PRs that are not `cancelled`). After the lock, when no PR of the Plan is left `waiting` or `dispatch` and one is not `cancelled`, the same transaction moves the Plan as `pr open` and the poll do: `dispatch → deploy` with `plan-phase`, and `deploy → done` with `plan-done` when every PR left is `merged` (§3.1); the output then has `phase`. A Plan whose PRs are all `cancelled` stays in `dispatch`.

Effects, **`open`** (contract §3.8): **rows**, one transaction: the PR `status = cancelled` (and its `draft`, `queued` and `failed` Jobs, cancelled as above, in the same transaction; hs-d573), with `lead_worktree` (the workflow worktree), `started_at`, `pr_number`, `pr_url`, `pr_opened_at` and `pull_request` set to null (the table's checks tie them to the status; the old values stay in the history rows). **Events** `pr-cancelled` `{pr}`, its `text` naming the pull request, the workflow worktree and the Lead Branch, which cpd leaves for the Lead to remove by hand; then, when every PR that is not `cancelled` is now `merged` and at least one is, the Plan `deploy → done` with `done_at` and `plan-done` (§3.1); otherwise the phase stays. The plan fingerprint does not change after the lock (§2.3). The output also carries `phase` and `released`, `{number, url, lead_worktree, lead_branch}` as they were.

```json
{"ok": true, "pr": {"key": "web", "status": "cancelled"}, "phase": "done"}
```

`phase` is present only when the Plan moved.

An `open` PR given up, `phase` after the command:

```json
{"ok": true, "pr": {"key": "api", "status": "cancelled"}, "phase": "deploy", "released": {"number": 116, "url": "https://github.com/acme/pay/pull/116", "lead_worktree": "/home/me/projects/pay.pay-4521.api", "lead_branch": "cpd/pay-4521/api"}}
```

Idempotent: yes, as stated.

### 9.7 `pr start`

```
cpd pr start <key> [--plan <id>]
```

Starts a `waiting` PR whose needs are all `merged` and whose base PR, if it has one, is `open` (A9, §3.8, hs-d324): the Lead's retry after engine E7 failed to start it (the `error` Event with code `pr_start_failed`), once the cause is fixed. It does what E7 does.

**An adopted PR** (`head_ref` set, §9.4; the same for E7 and `plan lock`'s start): the Lead worktree path is as always; `git fetch origin <head_ref>` runs (a failure is `pr_start_failed` with `step` `remote`) and the local branch `cpd/<plan>/<key>` is cut from `origin/<head_ref>`. There is no conflict check against the base (a conflict with the base is the `update` Job's), nothing is pushed at the start (no `cpd/...` branch ever appears on `origin`), and the User's own local branch of the head's name is never read or touched.

| Precondition | Code | Exit |
|---|---|---|
| the phase is `dispatch` | `illegal_phase` | 3 |
| the PR is `waiting` (`dispatch`, `open` or `merged`: `already`; `cancelled`) | `illegal_transition` | 3 |
| every need is `merged` (`details.needs` lists those that are not, as keys), and the base PR, if the PR is stacked, is `open` (`details.base_pr` is its reference when it has not opened); a `merged` base PR is not a refusal: the PR is unstacked first (below) | `pr_not_ready` | 3 |
| its workflow worktree path does not exist, except a leftover of an interrupted start (a worktree of the PR's repo on its Lead Branch), which is removed first | `worktree_exists` | 3 |
| its Lead Branch does not exist in its repo, except that leftover's branch, and neither does `cpd/<plan>` when the Lead Branch is `cpd/<plan>/<key>` (§2.1) | `branch_exists` | 3 |
| `origin/<lead branch>` does not exist, or it does not conflict with `origin/<base>` (§7.2, hs-d490) | `branch_exists` | 3 |
| the worktree, the remote check and `pre-start` succeeded; `details` is `{pr, step, tail?, log?}`, as in `pr_start_failed` | `setup_failed` | 1 |

Effects (§3.8 "The start", §7.2):
- **Unstack** (contract §3.8 "Stacked PRs" step 7, hs-d301): when the PR's base PR is `merged`, a first transaction sets the PR's `base` to the base PR's own `base` (a chain of merged base PRs is walked to its end, one `pr-base-changed` per step) and appends `pr-base-changed` with `by` `pr start` (E7 does the same with `by` `engine`; the poll with `by` `poll` when it recorded the merge), before the start's checks, then the start goes on with the new base (the start point is `origin/<new base>`, never the merged PR's deleted branch). This is also why `pr_not_ready` is never about a `merged` base PR.
- **git**, in the PR's repo: `fetch origin <base>` (fresh, so the PR contains what its needs merged; for a **stacked** PR, `<base>` is its base PR's Lead Branch, fetched only when it is on the remote, and the Lead Branch is created from the base PR's Lead Branch **head**, the local branch, not from `origin/<default branch>`, contract §7.2), the remote check and the reuse or creation of its workflow worktree exactly as `plan lock` (§4.8) does for one PR, then `pre-start` in the new workflow worktree, output in `run/<plan>/logs/lead-<key>.pre-start.log`; on failure the worktree and branch are removed, nothing in the database changes (`start_failed_at` keeps its value) and the result is `setup_failed`. After `pre-start` passed, a new Lead Branch is pushed (`push origin <lead branch>`, no `-u`; the worktree is added with `--no-track`, hs-d243); a failed push does not fail the start: one `error` Event with code `push_failed` and `details.pr`.
- **Rows**, one transaction, with a compare-and-swap on `status = 'waiting'`: `status = dispatch`, `lead_worktree` (the workflow worktree), `started_at`, `start_failed_at = null`. **Event** `pr-started` `{pr, repo, base, lead_branch, lead_worktree, base_sha, reused, pushed, by: "pr start"}`. The PR's `queued` Jobs are now Ready (§3.2) and engine E3 schedules them.

```json
{"ok": true, "pr": {"key": "ui", "status": "dispatch", "lead_branch": "cpd/pay-4521/ui", "lead_worktree": "/home/clayton/projects/pay.pay-4521.ui", "base_sha": "9c1e7a2…", "reused": false, "pushed": true}}
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
| a `gh` call fails (§1.3) | `gh_failed` | 1 |

Effects: **gh**, in the PR's Lead worktree: `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url,baseRefName` and `gh api repos/{owner}/{repo}/pulls/<n>/comments`. No rows, no Event.

Output: `plan`; `pr`, the PR's `key`, `number`, `url`, `title`, `base` (the pull request's base branch on GitHub, `baseRefName`), `base_pr` (the base PR as `<plan>/<key>`, derived from the PR's stored `base`, or null: it differs from `base` only until the poll has followed a repoint, contract §3.8 "Stacked PRs" step 8), `state` (`open`, `closed` or `merged`), `review_decision` and `checks` (`none`, `passed`, `pending` or `failed`; the per-check mapping of contract §7.6 step 3b: FAILURE, TIMED_OUT, ACTION_REQUIRED, STARTUP_FAILURE, ERROR are failing; CANCELLED is not; pending checks never fail); `checks`, one entry for each check, `{name, conclusion, url?}` (includes CANCELLED and pending checks with their conclusions unmodified, mapping applies to the summary only); `reviews`, `{id, author, state, body}`; `comments`, `{id, author, body}`; `inline_comments`, the pull request's review comments, `{id, author, path, body}`; and `feedback` (pro-d68), the row's stored list of open feedback items (contract §2.2 "Feedback item"; null when the poll never read it), which is what `pr reply --to` names. `reviews`, `comments` and `inline_comments` are as before and list everything, a bot's included.

```json
{"ok": true, "plan": "pay-4521",
 "pr": {"key": "ui", "number": 813, "url": "https://github.com/acme/pay/pull/813", "title": "Retry failed payouts (ui)", "base": "main", "base_pr": null, "state": "open", "review_decision": "changes_requested", "checks": "failed"},
 "checks": [{"name": "ci/test", "conclusion": "FAILURE", "url": "https://github.com/acme/pay/actions/runs/1"}, {"name": "ci/lint", "conclusion": "SUCCESS"}],
 "reviews": [{"id": "PRR_kwDO…", "author": "reviewer", "state": "CHANGES_REQUESTED", "body": "Please add a test."}],
 "comments": [{"id": "IC_kwDO…", "author": "reviewer", "body": "Thanks for the fix."}],
 "inline_comments": [{"id": "1893", "author": "reviewer", "path": "src/payout.rs", "body": "This retry never backs off."}]}
```

Idempotent: yes (a read).

### 9.8a `pr reply`

```
cpd pr reply --pr <key> --to <item-id> (--body <text> | --body-file <path>) [--plan <id>]
```

Answers one feedback item (contract §2.2 "Feedback item"; pro-d68, Plan fb-path). **Callers:** Lead; not cockpit, not internal (§2). The Lead fixes each open item or decides not to, and answers it either way, saying what it did or why not. It **never resolves a thread** (no flag, no `resolveReviewThread`): the reviewer does.

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
| the push is rejected and the remote tip does not contain G (the remote moved: a `pr rebase` pushed, or someone else); `details.stderr` is git's stderr; the Lead runs `pr rebase` | `push_rejected` | 3 |
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

Merges a pull request on GitHub: the Deploy column's merge button (§13.2; hs-d272, hs-d273, hs-d274). **Callers:** the User, through the cockpit (`cockpit` true); none of the agent roles, and no guard against them calling it (hs-d248). It takes a pull request, not a Plan's PR key: it needs no Plan or Context, and `<owner>/<repo>#<n>` is the form the Deploy column shows. Per-owner merge rules (repo-policy, hs-d263) are later.

Steps:

1. Read the pull request fresh from GitHub: `gh pr view <n> --repo <owner/repo> --json state,mergeStateStatus,headRefOid,title,mergedAt`. The stored `pull_requests.merge_state` is not trusted: it can lag.
2. Refuse, failing loud, unless `state` is `OPEN` and `mergeStateStatus` is `CLEAN`.
3. `gh pr merge <n> --repo <owner/repo> --squash --delete-branch --match-head-commit <headRefOid>`. `--match-head-commit` makes GitHub refuse when the branch moved after step 1's check. The method is always squash, and the branch is deleted on GitHub after the merge. cpd's own clean-up never touches remote branches (§9.2 "After a PR is merged"), and nothing in cpd runs `git fetch --prune`, so the local clean-up is unaffected.

**Effects** (hs-d550 B1): after `gh pr merge` succeeds, one store transaction: read the pull request back once with `gh pr view <n> --repo <owner/repo> --json state,mergeStateStatus,headRefOid,title,mergedAt` (the precheck, hs-d550 B1: `gh::view_for_merge`). When it reads `state` `MERGED` the store transaction writes: the `pull_requests` row of <owner>/<repo>#<n>, when one exists, gets `state` `merged` and `merged_at` GitHub's `mergedAt` (else the transaction's time, changed_at now); when a `prs` row points to it, exactly the poll's merge (contract §7.6 pr poll step 6: `prs` `merged` with `merged_at`, `pr-merged`, Plan `done` with `plan-done` when it was the last PR, the unstack of PRs stacked on it). The open `pr-ready` item of subject `pr:<owner>/<repo>#<n>` is closed (closed_at, `inbox-closed`, as E8's reconcile closes one). The command output's `stored` is true. When it reads anything else, or the read fails, nothing is written and `stored` is false. The command succeeds either way. No stored row: nothing is written and the merge still succeeds. (hs-d550 B2: the precheck reading MERGED writes the merge as (A) does, with GitHub's `mergedAt`.) A row already stored merged writes nothing (idempotent). (hs-d550 B1, C: a failing `cpd pr poll` after the merge no longer matters, the merge is already stored; `pr merge` now opens the store.) GitHub's search listed the PR after its merge (hs-n415: evidence of why B1 changed the contract).

| Precondition | Code | Exit |
|---|---|---|
| the argument is `<owner>/<repo>#<n>`, `n` a positive integer | `usage` | 2 |
| the re-check finds the pull request not `OPEN` or its `mergeStateStatus` not `CLEAN`; `details` is `{repo, number, state, merge_state}` (GitHub's words); when the precheck reads `MERGED` (hs-d526), it writes the merge transaction and is still refused | `merge_not_clean` | 3 |
| each `gh` call succeeds (also when `--match-head-commit` makes GitHub refuse); `details` is `{argv, exit, stderr}` | `gh_failed` | 1 |

Output:

```json
{"ok": true, "repo": "<owner/repo>", "number": 31, "method": "squash", "branch_deleted": true, "stored": true}
```

**Rows** (hs-d550 B1): one store transaction, recorded immediately after `gh pr merge` succeeds (when the precheck reads `MERGED`, hs-d550 B2), never delayed to the next poll. The `pull_requests` row of <owner>/<repo>#<n>: when it exists, `state` becomes `merged` and `merged_at` is GitHub's `mergedAt` from the precheck's `gh pr view`, or the transaction's time when GitHub was not read or did not return it (changed_at now); when a `prs` row points to it, the PR `dispatch → open → merged` or `waiting → merged` with `merged_at`, Events `pr-merged` and, when every PR that is not `cancelled` is then `merged`, the Plan `deploy → done` with `plan-done` (contract §7.6 poll step 6). The open `pr-ready` Inbox item of subject `pr:<owner>/<repo>#<n>` is closed in the same transaction (closed_at, `inbox-closed`, hs-d550 B1, hs-n404). When no row exists, nothing is written and the merge still succeeds. No Event otherwise.

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

**The chat slot.** **Amends contract.md §6.2** (the Lead's answer to Escalation `contract-commands-a1-q1`): the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat`. A pane with `@cpd_view = chat` is the **placeholder**: it runs `cpd tui view chat`, which reads `No Lead here: pick a Plan in the session tree`, and it holds the slot while no Lead is there. **Amends §6.2's list of `@cpd_view` values** with `chat`. Why: the slot must hold a pane before any Lead exists (the first `cpd tui` on a machine) and after its Lead is parked (`swap-pane` needs a pane to swap in). At most one placeholder exists, and only ever in the slot: `session park` creates it there (§10.5), and `session open` kills it with its window when a Lead takes the slot (§10.4).

### 10.1 `session list`

```
cpd session list [--context <id>] [--plan <id>] [--role <role>] [--state <state>] [--archived]
```

Filters combine with AND; `--role` and `--state` may repeat. Without filters: every Session except archived ones (`archived_at` is null). With `--archived`: archived Sessions are listed too, live and archived together (hs-d516); the flag is given once (clap refuses it repeated). Here `--context` and `--plan` are filters only: neither falls back to `CPD_PLAN`.

Effects: none; one read transaction.

```json
{"ok": true, "sessions": [
  {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "state": "waiting", "state_label": "φ: Worker question", "state_at": "…", "archived_at": null, "asleep_at": null,
   "place": "chat", "tmux_pane": "%12", "file": "/home/clayton/.pi/agent/sessions/…_596d007f-….jsonl", "forked_from": null, "plan_lead": true, "tier": "O3", "cwd": "/home/clayton/projects/cpd", "title": "Dynamic session titles contract", "titled_at": "…"}],
 "events_cursor": 48213}
```

`role` is `lead`, `worker` or `manager` (B3-A1; a Manager has `plan` null, `attempt` null and `plan_lead` false; `--role manager` lists them). `plan_lead` is true for the Session that is its Plan's `lead_session`. `title` and `titled_at` are the dynamic title (contract §2.2, hs-d257) and when it was written, both null until the first `session retitle`. `archived_at` and `asleep_at` are null unless the Session is archived or asleep. Read-only.

### 10.2 `session show`

```
cpd session show [<id>]
```

`<id>` defaults to the selected Session (`CPD_SESSION`); it accepts the short forms of §1.3. Output: the fields of `session list` plus `created_at`, `registered_event` (the id of its `session-registered` Event, the pi adapter's first cursor, §15.2), `model` and `thinking` (the row's `tier` as pi is told it, below: hs-d506), `loop_guard_turns` (the config value, for every Session) and, for a Plan's Lead, `open_escalations` (ids). Effects: none.

`model` is from the policy's `[model]` table for the key of the row's `tier` (contract §8.3, §9) (`<provider>/<model-id>`), and `thinking` is the level of the tier's digit (`off`, `low`, `medium`, `high`, `xhigh`, `max`, contract §9): the two halves of the `--model <model>:<thinking>` argument `session run` passes (§10.8), computed by the one function both use. The models are read at each call, so a policy edit shows at once.

```json
{"ok": true, "session": {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "state": "idle", "state_label": null, "place": "chat", "tmux_pane": "%12", "file": "…", "forked_from": null, "plan_lead": true, "tier": "O3", "model": "github-copilot/claude-opus-5.5", "thinking": "high", "loop_guard_turns": 20, "cwd": "/home/clayton/projects/cpd", "title": null, "titled_at": null, "created_at": "…", "registered_event": 40117},
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
| `<source>` | the Session to fork (the short forms of §1.3 are accepted); a Lead. This command is for **tangents the User starts** (a bug, an idea), B4; a review is a review Job (§5.1), not a fork by hand |
| `--plan <id>` | the new Session's Plan; default the source's Plan. The fork is a **side session** of the Plan: the Plan's `lead_session` does not change, so Plan Events still go to the Plan's Lead only (§15.2). It is drawn in the tree **directly under its Context**, after the Context's Plans, never under the Plan or under `<source>` (B1, §13.2) |
| `--name <name>` | default `<source name> (fork)`, cut to 80 characters |
| `--message-file <file>` | its first message, copied to `run/<plan>/messages/<session>.md`; without it the fork starts with no message |

| Precondition | Code | Exit |
|---|---|---|
| the source is a `lead` Session | `not_a_lead` | 3 |
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
| the cockpit's `main` window and its chat slot exist; `details.next` = `cpd tui` | `no_cockpit` | 3 |

Effects:
- In the chat slot already: nothing; `"already": true`.
- **tmux:** when the Session has no live pane (`stopped`, or its pane is gone): first `kill-pane` on a dead pane of the Lead, if one is still there, so it never adds a second window; then `new-window -d -t =cpd: -n park-<label> … -- cpd session run <id>` first, which resumes it (`pi --session <file>`, §6.5), and `tmux_pane` is recorded; the pane's `@cpd_title` is set by the precedence of contract §6.1 (the dynamic `title`, else the Plan's title or the name). Then `swap-pane -d -s <its pane> -t <chat slot pane>`, and `rename-window -t <window> park-<displaced label>` for the window that now holds the displaced Lead. When the displaced pane is the placeholder or a history pane (`@cpd_view` `chat` or `history`, §10.12), `kill-pane` on it instead, which closes the now-empty park window with it, so placeholders and history panes never pile up in park windows.
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

Before it starts pi (steps 1–3) every pi a `session run` starts, a Lead's and a Manager's, gets `PLANNOTATOR_PORT=<the range>` in its environment (§6.5); there is no port to assign.

**Reload on install** (hs-d33, hs-d38, hs-d49, hs-d151; §6.7 "Reload on install"; incident hs-n141, cause hs-n148). The wrapper detects a new install by the identity of its executable (§3.4 of the contract), checked at every iteration of its loop **before any store access**. On a new cpd it re-execs **at once**, with no idle wait, whatever the Lead's place or state, and **keeps pi running**: pi is its child and survives the `exec`. It `exec`s the same path with the same argv plus the internal argument `--adopt <pid>` (**internal**: written only by the wrapper's own `exec`, not an interface of the User or an agent); the new `session run` skips steps 1–3, starts no pi, `waitpid`s that pid as its own child and continues the same loop on it (watchdog, archive on quit, `Press Enter to resume`). The adapter reloads itself at idle (hs-a5, §15.6); a cpd install does not restart pi. Before the `exec` it runs `<path> --version` and re-execs only when that succeeds and `store` is `null` or `store.opens` is `true` (§1.2); otherwise it does not `exec`, logs once per installed file (identity) `reload skipped: <path> cannot open the store: <store.refusal.message>` on the stderr of its pane and keeps running, pi untouched; a later install is checked again (contract §6.7). **A store error never ends a wrapper while its pi runs:** a failed read with a replaced cpd leads to the re-exec above; any other failed read is logged once on stderr and retried at the next poll, pi untouched. Why: wrappers that waited for the Lead's idle read the store after a newer cpd had migrated it, failed `store_too_new` and exited, and six Leads were ended instead of reloaded (hs-n141, hs-n148). If pi exits on its own after an install, the wrapper `exec`s the new binary (no `--adopt`) before any store write, and the existing paths run there.

**Transient store errors** (contract §6.7 "Reload on install"). The store accesses before pi starts (open, the Session's row, `sessions.file`, the port, a fork's source) and after it exits (`session state stopped`, the name for the resume prompt) are tried again while they fail with `store_busy`, `lock_timeout` (the store's setup lock), or SQLite's busy or locked as `store_failed`: 0.5 s, doubling to 5 s between tries; the pane says once `Waiting for the store (<what>): <code>: <message> Trying again.` A wrapper never ends for it. Any other error (`session_not_found`, `not_a_lead`, `wrong_pane`, a malformed row) ends it as in the table below.

**Restart for a new pi** (hs-d49). A change of pi (the identity of `<pi agent dir>/install/current-version`, else of the resolved `pi` executable) is the only reload that restarts pi, and it waits for the Lead's next idle (state `idle`, last turn finished, session file quiet `fork_quiet_seconds`): the wrapper sends pi `SIGTERM`, waits for it to exit (`SIGKILL` `timeouts.pi_stop_seconds` (default 10) after `SIGTERM`) and `exec`s the same path with the same argv (no `--adopt`); the new `session run` takes step 1. A parked Lead restarts at once. The Lead in the chat slot waits until it is idle **and** parked, or idle for 10 minutes, so unsent typing is not lost. A `waiting` or `blocked` Lead is never idle. See contract §6.7 "Reload on install".

**Adapter watchdog** (hs-d141; contract §6.7 "Adapter watchdog"). While pi is alive, the wrapper checks the Session's state at every poll (2 s). When the state has been `stopped` for more than **60 s** (a constant of the wrapper, not a config key; measured from the `session-state` Event that set it, so a state the wrapper itself set after a pi exit does not count while pi is not running), it restarts pi on the session file: `SIGTERM`, wait (`SIGKILL` after `timeouts.pi_stop_seconds`, as for a reload), then step 1. In one transaction before the restart it appends one `error` Event `{source: "session run", code: "adapter_missing", message, details: {session, state_since, seconds}}` with `session` = this Session (and its `plan`, `context`), which the new adapter delivers once it follows (§15.3). It applies to every Lead and Manager, whatever the place and whatever the idle rules of the reload; a `stopped` that ends (`idle`) before 60 s is the adapter's own reload and is never touched. After the restart the 60 s start again; a broken install loops once per 60 s with one `error` Event per restart (no cap: each is a visible fact, and a repaired install ends it).

The first message is the file `plan new` or `session fork` wrote (§1.4); for a Manager, `run/<context>/manager-message.md` (§3.10); none when resumed. Before it starts pi it writes the `git` wrapper `$CPD_HOME/bin/git` if it is missing or different (idempotent) and puts `$CPD_HOME/bin` first on pi's `PATH` (contract §6.5, hs-d310). It is passed as `@<path>` (its absolute path). While pi runs, it watches for the session file (§6.5) and records `sessions.file` once it appears (a data write, no Event). When pi exits it runs `session state stopped` if the adapter did not, prints `pi exited. Press Enter to resume <name> here.` and resumes on Enter, except after a quit by the User (below).

**Auto-archive on the User's quit** (hs-d91, hs-d122). pi exits with status 0 on the User's `/quit` and Ctrl+D (pi 1.2.0, `interactive-mode.js`: `shutdown()` ends in `process.exit(0)`; verified by reading it). The wrapper tells that quit from its own: when pi exits with status 0 and the wrapper did not cause the exit (it sent no reload `SIGTERM`), the User quit. Then, in this order:
1. If the Lead is in the chat slot, `session park` it first (§10.5), so the placeholder takes the slot.
2. Archive: a side fork runs `session archive <id>` (§10.10); a Plan's own Lead runs `plan archive --plan <p>` (§4.10), the cascade, forks included. The archive **rows are written before** any pane is killed, and the wrapper's own pane is killed **last**.
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
- No live pane (`tmux_pane` null, or the pane is gone): nothing; `"already": true`.
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
2. **tmux:** a live pane is closed as `session close` does (§10.9).
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
| it has a session file (`sessions.file` set and readable) | `file_unreadable` | 2 |
| the cockpit's `main` window and its chat slot exist; `details.next` = `cpd tui` | `no_cockpit` | 3 |

Effects, in this order:
1. **Park:** if a Lead holds the chat slot, park it as `session park` does (§10.5): a new `park-<label>` window with a placeholder, `swap-pane`, **Event** `session-moved` `{place: "parked"}` for that Lead.
2. **tmux:** `respawn-pane -k -t <the slot's pane> -- cpd tui view history <id>` (§13.2), then `set -p -t <pane> @cpd_view history` and `set -p -t <pane> @cpd_history <id>`, and `@cpd_session` unset. A placeholder or an earlier history pane is the slot's pane, so it is respawned in place.
3. **Rows:** none besides step 1's Event.

```json
{"ok": true, "session": "8d0e…", "pane": "%41", "parked": "596d007f-…"}
```

`parked` is null when no Lead held the slot. Idempotent: yes; rerunning for the same id respawns the slot's one pane, so exactly one history pane exists and no Lead is parked.

### 10.13 `session send`

```
cpd session send <id> (<text> | --file <f>) --unblocks <reason>
```

Sends one message to a Lead or a Manager Session (hs-d92, hs-d133, hs-d514). It appends an Event; the target's adapter delivers it (§15.3). The message always wakes: when the target is idle it starts a turn at once; a working, waiting or blocked target gets it after its running turn, never interrupted. It carries a required statement of what it unblocks (hs-d507, hs-d514). The command is not the User's only way to write one: the engine writes the same Event, from the User's side (`from` null), to a Context's Manager for each of its own step errors (contract §3.7, hs-d314) and for each stuck Job (contract §3.7 "A stuck Job", hs-d318).

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

A `stopped` Lead or Manager is accepted: this call starts it (hs-d725) as a parked `cpd session run <id>` window (hs-d726, A1), and it gets the message when its adapter starts; the chat slot never moves. A live pane of the Session means it is starting, and nothing is started.

Effects:
- **Rows**, one transaction: **Event** `session-message` with `session` = the target, `context` and `plan` = the target's, `data` = `{from, text, unblocks}`; `from` is `CPD_SESSION`, else `PI_SESSION_ID`, else null (the User).
- **tmux:** when the target is `stopped` with no live pane, the mechanism of §10.4 (starting a parked window with a pane, §6.4) applies: first `kill-pane` on a dead pane of the Session, if one is still there; then `new-window -d -t =cpd: -n park-<label> | ctx-<context> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -P -F '#{window_id} #{pane_id}' -- cpd session run <id>`, the pane title and `@cpd_session` set, `tmux_pane` recorded and **Event** `session-moved` `{place: "parked"}` appended, the chat slot never moving. With a live pane, nothing is started. Otherwise no tmux.

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
| `locks/title-<session>.lock` is free (step 1; held: `details.reason` is `running`, nothing written, and the other run is the one that counts) | `title_failed` | 1 |

**Steps.**
1. Take `locks/title-<session>.lock` without waiting and hold it for the run's whole life, so a Session never has two runs at once; the kernel frees it when the run dies, so a crashed run leaves no stale lock (contract §1.4).
2. **The conversation text.** Read the Session's pi session file (`sessions.file`, JSONL) and take **the last 2 turns** (hs-d543): from the **second-to-last** message of role `user` to the end of the file (the whole file when it holds fewer than two `user` messages), the text of the messages of role `user` or `assistant` in it (tool calls, tool results and other entries are not messages here), in order; if their text exceeds **8 000 characters**, cut it so the **newest** characters are kept. With no such message (no `file` yet, or none in it), **skip**: print the JSON below with `"skipped": "no_messages"`, run no pi, append no Event, write nothing and set nothing. A skip is not an error.
3. **Run pi** once, in `run/titles/` (created when missing): `pi --print --no-session --no-extensions --model <model>:off -- "<prompt>"` where `<model>` is from the policy's `[model]` table's `G` key (contract §8.3, §9) and `:off` is pi's level for no thinking: the title writer needs only the model. The prompt asks for a reminder of **what was last happening** in the conversation, answered as **one line `TITLE: <what was last happening, at most N characters>`** (N of the usage above) and nothing else. pi's environment is `PI_SKIP_VERSION_CHECK=1` only, no `CPD_*` (contract §6.5, "Title writer", hs-d562). Its stderr is kept (its last lines are the `tail` below).
4. **Parse and check the answer** (hs-d356). The title is taken **only from an explicit, parseable answer**: **the only line that starts with `TITLE:`** (leading spaces allowed), the text after the marker, trimmed and unquoted. Lines without the marker are ignored, so a preamble before the marked line does no harm. The answer is **malformed**, never a title, when: no line has the marker, or two lines do; the marked text is empty; it is over **N characters** (N = cols − 8, at most 200; it is **never cut**: an over-long answer is not a title; there is no word cap); it reads as sentences (a `. ` inside it, or it ends in `.`, `…` or `:`); or it is not a Title (`core/src/args.rs`: 1–200 characters, no control characters). An answer that is empty or only whitespace is `empty`, not `malformed`.
5. **Write**, in one transaction: `sessions.title` and `sessions.titled_at` (now); **Event** `session-titled` `{title}` (context, plan if any, session). Then **tmux**: sets `@cpd_title` to the title on the Session's live pane, when it has one (a tmux failure does not undo the write: it is reported on stderr, and the header is made right by the next start or move of the pane, which apply contract §6.1's precedence).

pi failing to start or exiting non-zero, or an answer that is empty or does not pass step 4, is a refusal `title_failed` (exit 1), with `details: {session, reason: "running" | "spawn" | "exit" | "empty" | "malformed" | "invalid", exit?, tail?}` (`tail`: the last lines of pi's stderr), and **writes nothing**: `titled_at` stays, so the Session stays due and the engine retries at the next tick, logs the failure, and reports it (an `error` Event) only once that Session's titling has failed for 10 minutes in a row; a success clears it (contract §3.7 E10, hs-d356). It appends no `error` Event itself: the engine's `report` does.

```json
{"ok": true, "session": "596d007f-…", "title": "Dynamic session titles contract", "titled_at": "2026-10-20T09:12:41Z", "event": 4821, "pane": "%12"}
```

`pane` is the pane whose header was set, null when the Session has no live pane. A skip prints:

```json
{"ok": true, "session": "596d007f-…", "title": null, "skipped": "no_messages"}
```

(`"already"` is not used here: it means the same value was already stored, and a retitle never compares.) Idempotent: yes: rerunning is safe and writes a new title each time (it never answers `"already"`).

### 10.15 `session first-message` (internal)

```
cpd session first-message [--session <id>]
```

**Internal:** the pi adapter runs it at turn start for a by-user Lead or Manager Session whose `cpd-first-message` custom message has not yet been delivered (hs-d542 A3). Renders the first message at call time with the same functions `plan new` and `context manager` use (§4.1 step 3, §3.10), writes it to the session file's destination (`run/<plan>/lead-message.md` or `run/<context>/manager-message.md`), and prints JSON for the adapter to add the message to the run (§15).

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
cpd session retier [--all | <id>]
```

Moves live Leads and Managers to the Tier the config names now (hs-d506: the User sets `lead_tier` and `manager_tier` and every running Session must follow, not only those registered later). **A Session's `tier` is not fixed at registration** (contract §2.2 Session): this command is the one that changes it. Workers are not touched: a Worker's Tier is its Job's (contract §9).

| Argument | Meaning |
|---|---|
| `<id>` | one Session (the short forms of §1.3) |
| `--all` | every Lead and Manager that is not archived |

Exactly one of the two: neither, or both, is `usage` (exit 2).

**The target.** A Session of role `lead` (a Plan's Lead and a side fork alike) moves to the config's `lead_tier`; one of role `manager` to `manager_tier` (contract §8.2).

| Precondition | Code | Exit |
|---|---|---|
| the Session exists (`<id>`) | `session_not_found` | 4 |
| it is a `lead` or a `manager`; a Worker is refused, its Tier is its Job's | `not_a_lead` | 3 |
| it is not archived; `details.next` = `cpd session unarchive <id>` | `archived` | 3 |

`--all` refuses nothing for a Session it passes over: archived Sessions and Workers are not part of it and are not listed.

**Effects**, one transaction for the whole command:
- **Rows:** for each selected Session whose `tier` differs from its target, `sessions.tier` = the target (the store's history trigger records the old row).
- **Event** `session-retiered` `{from, to}` for each such Session, one each, addressed to it (`session` set, with its `context` and `plan` when it has one), appended in the same transaction. `from` and `to` are Tier strings (`O3`, `O2`). `text`: `Session <id> moved from O3 to O2.`
- A Session already on its target is **unchanged**: no row write, no Event. When no Session changes, nothing is written at all.
- **No tmux call and no pi restart.** A running pi switches itself (below); a Session with no pi keeps the new `tier` for its next start (§10.8).

**The adapter's switch** (contract §5.3, §6.5; commands §15.3 `session-retiered`). The Event reaches the adapter of the Session it names. **At the Session's next idle** (right away when it already is) the adapter reads `cpd session show` for the Session's `model` and `thinking` (§10.2), finds the model in pi's registry (`ctx.modelRegistry.find(<provider>, <model-id>)`, the provider being the part of `model` before its first `/`) and calls `pi.setModel(<that model>)` and `pi.setThinkingLevel(<thinking>)`, once per change and never while the Session works, waits or is blocked: no turn is interrupted and nothing is restarted, and the conversation stays whole (docs: pi 1.3.0 `extensions.md`, `ExtensionAPI.setModel` "for the current session without changing the configured default"). Several Events before one idle make one switch, to what the row says at that idle.

```json
{"ok": true, "retiered": [
  {"session": "596d007f-…", "role": "lead", "from": "O3", "to": "O2", "event": 48214},
  {"session": "9a41c0de-…", "role": "manager", "from": "O5", "to": "O4", "event": 48215}],
 "unchanged": ["0c77e1b2-…"]}
```

`unchanged` lists the ids of the selected Sessions already on their target. `"already": true` is added (and `retiered` is `[]`) when nothing changed. Per Session: `session` (id), `role`, `from`, `to` and `event` (the id of its Event).

Idempotent: yes; run again, nothing changes (`"already": true`).

### 10.18 `session report` (internal)

```
cpd session report --what <text> --code <code> --message <text> [--details <json>] [--session <id>]
```

Reports an adapter error to the Manager (hs-d573, M4): the adapter of a Lead runs this when it hits an error (not to its chat, M3), once per distinct (`what`, `code`), keyed by `host.report`'s memory in pi/lib/host.ts.

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

Effects: **Rows**, one transaction: `events` (one `error` Event, source `adapter`; context = the Session's Context; **plan null and session null** so no Lead's follower, which follows its Plan's Events and those addressed to it, ever receives it back; data `{source: "adapter", code, message, details}` with `details` = the given object plus `{what, session, plan}`; text `cpd adapter: <what>: <code>: <message>`). Not idempotent by design: every call appends an Event.

Output:

```json
{"ok": true, "event": "4823"}
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
- **Event** `session-resumed` (session = the target, context and plan = the target's), `data` `{}`, `text` `Loop guard of Session <id> resumed.`. The adapter clears the trip (L5): empties the window, closes the blocking reason, appends the pi session entry `{tripped: false}`, and delivers ONE message with a turn (the Event's text, `followUp`), so one turn starts.
- **tmux:** none.

```json
{"ok": true, "event": 4816, "session": "a41b…"}
```

`event` is the id of the `session-resumed` Event. Idempotent: no; each call is one Event. A second call finds the Session no longer blocked once the adapter has acted.

## 10a. `board` (hs-d514)

The message board: a SQLite table whose posts name who they are for (to), read by `cpd board list` and picked up at an agent's turn start by `cpd board take`. Posts are written by `cpd board post`, and never start a turn (hs-d514). Important facts stay Context records.

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

| Precondition | Code | Exit |
|---|---|---|
| the selected Plan, Context or Session exists | `plan_not_found`, `context_not_found`, `session_not_found` | 4 |
| `--since` is not above the highest Event id (a cursor from another database) | `cursor_ahead` | 3 |

**A migration in progress is waited out** (hs-n278; contract §4.2, §1.4): when it opens the store, or reopens it, while another cpd is migrating it, it waits, up to `MIGRATION_WAIT_CEILING` (10 minutes), and prints one line on stderr (`waiting for cpd pid <pid> to migrate the store <from> -> <to> (<command>)`), nothing on stdout; it does not fail `lock_timeout` for it, so an install never ends a follower.

**Any other store error ends it** (hs-d140; contract §5.4): `store_too_new` after an upgrade (the verdict of contract §4.5, which the follower recomputes when `PRAGMA schema_version` changes), `migration_changed`, `migration_order`, `store_busy`, `store_failed`, … is printed as the last line, as the failure envelope, and the process exits with that code's exit (§1.2); it never stays alive on a store it cannot read. The pi adapter restarts a follower that ends (§15.2).

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

**Without `--ensure`:** runs the `@cpd_workers` step (below), then E1–E5 and E7–E14 (E6 is removed; E4 is the `open` Jobs since hs-d340, no longer the completion step; E7, the PR start, since R34; E8, the Inbox, §21; E9, the Context curation, since hs-d194 (hs-d549, hs-d551); E10, the Session titles, since hs-d257; E11, the team review read, §21.5; E12, the Worker archive, since hs-d516; E13, the Sleep, since hs-d515; E14, archive asleep Sessions, hs-d630) every `tick_seconds` (a decimal, minimum 0.1; default 5) in the foreground, in `cpd-work:watch`, logging to stderr and appending the same lines to `run/watch.log`.

E5 (the PR poll) runs at its due time, `pr_poll_seconds` after the previous poll ended (5 s by default), not at the next tick: the engine sleeps until the earlier of its next tick and E5's due time (contract §3.7). The pi stop grace is `timeouts.pi_stop_seconds` (default 10; contract §8.2).

| Precondition | Code | Exit |
|---|---|---|
| `locks/watch.lock` is free | `watch_running` | 3 |

Effects: **Events** `watch-started` and `watch-stopped` `{pid}`, one `session-message` to each Manager that is not archived when this engine follows an earlier one (how long none ran, contract §3.7), and every row and Event of E1–E5 and E7–E14 (E3 starts nothing while a tmux restart is pending, and the engine starts that restart once nothing is in the way, §3.7) (§3.7; a PR that E7 fails to start gets its own `error` with code `pr_start_failed` and is not tried again, §9.7; a distill run E9 starts appends its own `error` when it fails, §3.12; a retitle run E10 starts (§10.14) appends its own `session-titled` when it succeeds, and when it fails E10 reports one `error` with code `title_failed` for the Session). An unexpected failure in a step appends `error` `{source: "watch", code, message, details}` for the Plan it concerns and the loop continues; the same failure for the same object is appended once until it clears. It waits out a migration in progress when it opens the store (hs-n278; contract §3.7, §4.2): it does not fail `lock_timeout`, and logs the one waiting line to `run/watch.log`. It runs until SIGTERM or SIGINT, then prints `{"ok": true, "stopped": true}` and exits 0.
**The `@cpd_workers` step** (contract §6.1; User, hs-d149, hs-d198): on each tick the engine reads, in one read, `N` (the Attempts not `ended`), `M` (config `concurrency`), `W` (**waiting**: the Ready Jobs, §3.2, the ones E3 would start if a slot were free) and `Q` (**queued**: every Job with status `queued`, waiting ones included), and builds the StatusBar's texts: the long form `workers N/M · W waiting · Q queued` and the short form `N/M·W·Q`; The status line measures their display widths using tmux's #{w:…} format modifier, so no width is computed in the engine. The workers' pill with its two caps `` and `` (the text's width plus 2; hs-d479, contract §6.1); the texts hold neither caps nor background nor any style: no `#[fg=…]`, no `dim`, because the pill has one text colour, 0 (hs-d534, contract §6.1.1). It runs `tmux -L <socket> set -g @cpd_workers '<long>' ';' set -g @cpd_workers_short '<short>'` **only when a text changed** since the last tick it set (the first tick always sets them, and so does the first tick after a tmux failure or a missing server: a restarted server starts without them). No row, no Event; a tmux failure of the step is no `error` Event while a tmux restart is pending and has not failed (§13.3: the restart takes the server away under the step), it is tried again at the next tick. The indicator is therefore up to one tick (`watch.tick_seconds`) late, and stale while the engine is stopped; this deviates on purpose from §17's "no tmux option carries state".

Idempotent: a second one is refused.

**E3: Linear vs parallel Attempts** (pro-d52, hs-d680, hs-d682, hs-d683; contract §3.7 E3, §7.2, §7.4, A1). When E3 schedules a Ready Job of a PR each pass, it derives which Attempt runs as the linear one (the PR's only live Attempt for its whole life, reusing the PR's workflow worktree on its Lead Branch) and which as parallel siblings (each with its own worktree): by the rule of contract §3.7 E3, (1) if the PR has a live linear Attempt, no Ready Job starts; (2) if the PR has only live parallel Attempts, every Ready Job is parallel; (3) if no Attempt is live, one Ready Job is linear and the rest are parallel. E3 passes this allocation to the driver starter, which takes the driver lock and enforces it atomically before starting, ensuring no two Attempts can share the workflow worktree.

**With `--ensure`:** starts the engine when it is not running, and returns at once. It fails and creates nothing when `$CPD_HOME` does not exist (hs-d519 3c/5, docs/research/hygiene.md, bug 3c/5).

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

**With `--check`** (hs-d463; contract §3.7 "The install's engine order"): verifies, and **writes nothing**, what `--ensure` and the engine's start depend on: no row, no Event, no migration, no window, no pane, no lock taken, no tmux option set, no server started, no directory made. In this order, failing at the first, each with its own error and exit code (§1.2): (a) the config loads (`config_missing`, `config_invalid`); (b) the store opens, from the **read-only verdict** of `cpd --version` (§1.2; `store::verdict`, contract §4.5) on the database opened read-only: it reads `user_version`, `schema_migrations` and this binary's list, and **never migrates or writes**; the database file, its WAL and `locks/store.lock` are not written, created or locked (no `Store::open`, which migrates). No database is no refusal; a verdict that opens, with every pending migration known to this binary, passes; a refusal is its own `store_too_new`, `store_failed`, `migration_changed` or `migration_order`. The migration happens only in the new engine's own start, after the old engine is retired, so a failed check leaves the old engine working against an unmigrated store (hs-d463); (c) `tmux -V` parses and is at least 3.6 (`tmux_failed`); (d) the tmux listing: with the server of `<socket>` running, `list-panes -a -F` with the format of the liveness call (§6.3), as a UTF-8 client (§6.1), and every line parses (`tmux_failed`); with no server it is skipped; (e) `$CPD_HOME/locks` and `run` exist or can be made (their nearest existing ancestor is a writable directory) and `locks/watch.lock`, only if it exists, opens read-write, with no `O_CREAT` and no `flock`, so nothing is created or locked (`store_failed`); a missing file passes, `--ensure` creates it. Idempotent; no bound of its own.

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
- it takes `locks/watch-ensure.lock` first, as every `--ensure` does, and waits for it without a limit, and holds it until it is done, so only one runs at a time; after a failed start it waits 2 s under it, so a failing engine is tried once every few seconds;
- a dead engine pane is respawned in its window (`respawn-pane -e CPD_HOME=…`), the other dead `watch` windows are killed;
- it sets the server options and hooks again on a running server (§6.1), as `cpd tui` does: **`--ensure` does it too**, so every start of the engine leaves the hooks in place;
- it appends one line `supervision: …` to `run/watch.log` with the outcome. Its own output is dropped by the hook.

The engine it starts says how long it was down to each Manager (contract §3.7).

**`--restart-tmux` (internal; hs-d294).** Started by the engine, detached, when a restart of the tmux server is pending and nothing is in the way (contract §3.7, §6.1; §13.3); it is not an interface of the User or an agent. It restarts the server onto the installed tmux as §13.3 describes and exits 0, also when it did nothing (the server needs no restart, it is not safe now, another run holds `locks/tmux-restart.lock`). It prints one JSON value, `{"ok": true, "outcome": "…"}`, and logs to stderr, which the engine appends to `run/tmux-restart.log`. A failed restart exits with its failure and keeps the reason in `run/tmux-restart.pending`.

## 13. `tui`

### 13.1 `tui`

```
cpd tui [--attach] [--respawn]
```

Builds the cockpit (§6.1, §6.2) unless it exists, and starts the engine.

Effects, each only when missing:
- **tmux:** the server with §6.1's options; the session `cpd` with window `main` (`new-session -d -s cpd -n main -x 215 -y 62`), split into the tree (left, 25 %), the plan view (centre top) above the chat slot (centre bottom), and the widgets (right, 25 %), with the centre column 50 %; the plan view is 25 % of its height over the chat slot's 75 % (reset-layout §13.2); each pane runs `cpd tui view <tree|plan|widgets>` with `@cpd_view` set, with no StatusBar pane (the StatusBar is the session's status line, below); the chat slot holds the placeholder (§10) or, when one exists, the Plan Lead whose latest `session-moved` Event with `place: "chat"` is the newest, moved in as `session open` does. "Plan Lead" here means any Lead with a Plan, as for `session open` (side forks included, a Context's grounded Lead never), and one that `session open` refuses stays where it is while the placeholder keeps the slot (Escalation `tui-cockpit-a1-q3`). The session `cpd-work`.
- **Layout:** right after building a new `main`, `tui` runs reset-layout once (§13.2). On an existing `main` it adds every pane of the layout that is missing (the `tree`, `plan`, `widgets` or `chat` placeholder, each running its view with `@cpd_view` set; the User killed it, or a build stopped half-way) where it goes in the layout above, and then runs reset-layout, so the added panes are taken from the layout and the other panes get their sizes back (amendment 15, §19; tui-W5). A chat slot that holds a Lead is not missing, so no placeholder is added beside it; a placeholder added opens the last chat Lead as a new `main` does. On an existing `main` that still has a `statusbar` pane (a cockpit built before the StatusBar moved to the status line) it kills that pane once (`kill-pane`) and then runs reset-layout, so the other panes take the freed row and get their sizes back (amendment 15, §19). The same for a `worker` pane left by a cockpit built before the Worker pane was removed (pro-d63): `cpd tui` kills it once, then runs reset-layout, and listing panes skips it quietly, as for the StatusBar.
- **tmux options:** every option, the border format, the root `MouseDown1Status` binding and the conditional `pane-died` hook of §6.1. `host` sets them at every server start, so a server started by `plan new` or `watch --ensure` has them too; `cpd tui` sets them again on a running server (rerunning is safe). It also restores tmux's default `MouseDown1Pane` binding (`select-pane -t = ; send-keys -M`), which replaces cpd's earlier override.
- **The status line:** `status on` (hs-d567; hs-d561 had `2`), `status-style default` and `status-format[0]` (the `─` line, the StatusBar, §13.2), set with `-t <cockpit session>` once the session exists, never `-g`, so `cpd-work` has no status row (contract §6.1).
- **`--respawn`:** `respawn-pane -k` on every live pane of `main` that has `@cpd_view` (the views and the chat placeholder), so each runs the installed binary again; a Lead or engine pane is never touched. `scripts/install-cpd.sh` runs `cpd tui --respawn` when a cockpit exists. (Why: `cpd tui` respawns only dead panes, so a reinstall would never reach a running view.) Bare `cpd` does it too when it finds a view on an older binary (§13.3). While a preview is on (`@cpd_preview` is set on the server, §13.2), the views keep the preview build, so `--respawn` prints one line on stderr: `a preview is on (<value>); its views keep the preview build until 'preview-tui.sh off'` (hs-n264). Nothing is printed when it is unset, and the value's JSON is unchanged.
- `watch --ensure` (§12).
- **tmux version** (hs-d294): when the server runs an older tmux than the installed one, marks a restart pending (`run/tmux-restart.pending`, §13.3) and says so on stderr; it never restarts the server itself, since it may run inside it. The engine does it once nothing is in the way.
- No rows of its own; moving a Lead appends `session-moved`.

| Precondition | Code | Exit |
|---|---|---|
| with `--attach`: stdin and stdout are a terminal | `not_a_terminal` | 3 |

```json
{"ok": true, "built": true, "socket": "cpd", "session": "cpd", "attach": ["tmux", "-L", "cpd", "attach", "-t", "cpd"], "watch": {"started": true, "pane": "%3"}}
```

With `--respawn` the value also has `"respawned": ["%1", "%2", …]`, the panes restarted. With `--attach` it then replaces itself with `tmux -L cpd attach -t cpd` and prints nothing (§1.2); bare `cpd` is the same attach with the follow-ups of §13.3. Idempotent: yes; an existing cockpit gives `"built": false`. A pane of the layout that has died is respawned (`respawn-pane`, amendment 9, §19), never duplicated, and one that is gone is added again, so a second run adds nothing; a view pane that dies while the cockpit runs is respawned at once by the `pane-died` hook (§6.1).

### 13.2 `tui view` (internal)

```
cpd tui view <tree|plan|widgets|chat|legend>
cpd tui view logs [--event <id>]
cpd tui view merge <owner>/<repo>#<n> --client <name>
cpd tui view history <id>
cpd tui view dialog <confirm|input|text> …
```

The ratatui programs in the cockpit's panes (§6.2). They own their terminal (§1.2), make no writes of their own (two exceptions: the `plan` view's scroll offsets and the `tree` view's folds, below), and change state only by running `cpd` commands (`session open`, `plan new`, `session close`, `session park`, …) as subprocesses. What they read is §17.

A view in a pane never exits on a key: the `pane-died` hook (§6.1) starts a view again whenever its program exits, so the widgets view has no `q`. A view that fails (at start or later) prints its error, waits 3 s and then exits, so the hook does not start it again in a tight loop. Without a terminal the error is returned at once.

**Cockpit parity** (User, 2026-10-15). The cockpit matches the running cpd 1 cockpit exactly: layout, elements, text, colours, keys, mouse and behaviour. The reference was cpd 1's cockpit spike, `tui/` on branch `main` before the switch-over (`tui/src/*.rs`, `tui/README.md`; `tui/` now holds the Rust cockpit crate `cpd-tui`); the checklist of every difference is [cockpit-parity.md](cockpit-parity.md). Where this section states a deviation, it is deliberate and has its reason.

| View | What it is | Keys and mouse |
|---|---|---|
| `tree` | the **Context column**: Contexts, their Plans' Lead Sessions, and counters | ↑↓ and j/k move the cursor, Enter opens the Lead in the chat slot (on an Inbox row it jumps to the item's cause), `n` a new Plan, `x` closes a Lead's pane, `s` and `d` snooze and dismiss the selected Inbox item, `?` the legend; the popups of `n` and `x` are the `dialog` view's; clicks and the right-click menu as below. The tree's main loop may write a frame trace to its log (§17 "Frame trace") when `CPD_TUI_TRACE` is set. |
| `plan` | the **Plan view**: one box per PR of the chat slot's Plan, each holding the flowchart of its Jobs; with a Manager in the chat slot, it shows **Pulse** (one row per Plan of the Context) | ←→↑↓ and h/j/k/l select, Enter opens a Job's Worker, Esc clears the selection; the wheel scrolls (3 rows, 4 columns); clicks as below; no `q`. The view remembers each Plan's scroll offset and restores it when a Lead of that Plan comes back to the chat slot (a Plan Lead and its side forks share it), clamped to the chart, which may have changed size; the selection is not kept (reset). The offsets also survive a restart of the view: they are kept in `run/tui/plan-scroll.json` (below; User, hs-d197, hs-d254). With Pulse, a left click on a row opens that Plan's Lead and the wheel scrolls the rows (3 rows a notch). The plan view's main loop may write a frame trace to its log (§17 "Frame trace") when `CPD_TUI_TRACE` is set. |
| `widgets` | the **Deploy column**: the machine (hostname, CPU % and memory %) and the cockpit's PRs | no keys; a left click on an owned PR row brings its Lead into the chat slot (below), and a left click on a merge icon opens the `merge` popup (below); a right click on a PR row opens its menu, **Merge** (only with the merge icon), **Copy Link**, **Go to Lead** (only on an owned PR) (below; hs-d296); the wheel over the PRs section scrolls it by one pull request per notch (below); mouse capture on |
| `history` | an archived Session's history, read-only: the Session's pi file drawn with the session renderer the history view owns (`tui/src/history/render.rs`); its top line is `<name> (archived, read-only)`; **its content area is one row shorter than the pane, so the pane's last row is empty** (User, hs-d567; contract §6.1 "The blank row", Q2). Started by `session history` (§10.12) in the chat slot | `q` or Esc quits to the placeholder text; ↑/k, ↓/j, PageUp, PageDown and the wheel (3 rows) scroll; End or `G` follows the end again; no other keys |
| `chat` | the chat slot's placeholder (§10) | none |
| `legend` | the key legend, in a popup | ↑↓, j/k, PgUp/PgDn and the wheel scroll it when it is taller than its popup; any other key closes it |
| `logs` | the Event log, in a popup; `--event <id>` opens it at that Event | ↑↓, j/k, PgUp/PgDn and the wheel scroll; End or `G` goes to the bottom, where it follows again; `q` or Esc closes it |
| `merge` | the merge confirmation, in a popup, for the pull request `<owner>/<repo>#<n>` (below; hs-d273) | Enter or `y`, or a click on `[Merge]`, starts the merge in the background and closes the popup at once (hs-d473); Esc, `n`, `q` or a click on `[Cancel]` closes it with nothing done |
| `dialog` | the **dialog kit's** popup (below; hs-d295, hs-d297), of one kind: `confirm` (a question, two buttons), `input` (a labelled text box, contract §6.1.1), `text` (scrollable read-only text), `checklist` (tickable rows, hs-d549) or `select` (radio list, a cursor row picked with Enter or [OK]); its colours are contract §6.1.1's (title pill, body 7, the first button the blue block, the others 15, an input's refusal yellow, its border focused blue 12 and unfocused white 15, its cursor the blue block) | Enter or `y`, or a click on the first button, confirms; Esc, `n`, `q` or a click on the second cancels; `input`: Enter accepts, Esc cancels; `select`: ↑↓ and j/k move the cursor, Enter or a click on [OK] confirms, Esc or [Cancel] cancels; `checklist` and `select`: the cursor row is styled like checklist's, PgUp/PgDn and the wheel scroll; mouse capture on |

**`plan`: the scroll offsets file** (User, hs-d254). The `plan` view keeps each Plan's scroll offset in `$CPD_HOME/run/tui/plan-scroll.json` (contract §1.4), a JSON object `{"<plan id>": [x, y]}`, so that the offsets survive a restart of the view (every install restarts the views, `cpd tui --respawn`). This is a deliberate exception to "the views make no writes of their own" (the `tree` view's folds file is the other, below): the file is the view's own state, not Ledger state, as the Worker pane is the view's own layout (§17 "Actions"). Nothing else reads it.
- **Read** once, at the view's start. A missing file is an empty map. A file that cannot be read or is not that object (invalid JSON, an id that is not a Plan id, an offset that is not two non-negative integers) is not an error: the view starts with an empty map, logs `scroll positions reset: <reason>` (no note, §17 "No notes"), and replaces the file at its next write. The view never exits because of this file.
- **Written** atomically (a temporary file in the same directory, then a rename; the directory and its parents are created) whenever an offset has changed, at most once per second, and again as soon as the chat slot's Plan changes. A failed write is logged (§17 "No notes") and retried at the next write; it never ends the view.
- **Not cleaned up**: archived or unknown Plans stay in the file, harmlessly. Offsets are stored as left, not clamped: clamping stays at draw.
- The selection is never stored (reset at every restart and switch, above).

**Design rule** (User, Plan tui-visuals, 2026-10-17; hs-d124; refined by the powerline look, hs-d266 and hs-d322, 2026-10-19). Top-level layout separators (the tmux pane borders) are colour 7 when unfocused (interactive: they can be dragged) and blue when focused (hs-d531, hs-d534; contract §6.1.1); separators inside a slot are colour 8. The pane headers are tmux's (contract §6.1), **with a rounded cap on each side** and, by the title rule (User, hs-d475; hs-d478): the text in **Title Case**, **no padding** inside the pill, **a read-only title pill (colour 7 inverted: text in the terminal's background colour, caps colour 7), blue on the focused pane (colour 4 inverted: text in the terminal's background colour, caps colour 4)** (hs-d534, replacing hs-d530's 8 and 7; contract §6.1.1), and the pill starting **three columns from the pane's edge**, like the in-pane titles. `` (U+E0B6, nf-ple-left_half_circle_thick, left) and `` (U+E0B4, nf-ple-right_half_circle_thick, right) are one cell each on the normal background: `━━━Context━━━━━━━━━━━━`. The `━` before the pill is part of the border line and takes the line's colour (7 unfocused, blue focused), not the pill's. This is the powerline style of the User's zsh theme (agnoster) and needs the MesloLGS Nerd Font that dotfiles installs on every machine (`font-meslo-lg-nerd-font`, `~/dotfiles/home/.chezmoidata/packages.yaml`). A section title inside a slot is §17's "Section titles" rule (hs-d475, hs-d478, hs-d480), e.g. ` ──PRs───────────3 ` (` ╭─Inbox───…───35 ` for a collapsible widget's title): the title in Title Case as a read-only title pill (colour 7 inverted: text in the terminal's background colour, caps colour 7, `palette::pill_static()`, hs-d534) with no padding spaces, a muted line (colour 8) to the right edge, an optional count at its end as a pill too, the same colours as the title, inside the slot's one column of padding on each side (hs-d476), its left cap three columns from the edge (hs-d478). A slot's first section has no title, so no two titles stack: the pane header is the slot's title. **Pills** have the same rounded caps (the Plan view's Job pills, the legend's sample pill, the StatusBar's items): a pill is its block with a cap on each side, in the pill's background colour as their foreground (white for an interactive pill, grey for a read-only title, blue for the focused pane's title, the meaning's colour for a coloured one; hs-d534, contract §6.1.1), so the cockpit has one look, rounded.

**The chat slot's header** (hs-d257, contract §6.1). The border header of a Lead's or a Manager's pane is tmux's, from the pane option `@cpd_title`, not a view's: it shows the Session's **dynamic title** (`sessions.title`, written by `session retitle`, §10.14, started by engine E10 after the Session's turns end, at most every 10 minutes) when it has one, else what it showed before: the Plan's title for a Plan's Lead, the Session's name for a side fork or a Manager. It changes when a new title is written, not on a timer; a pane that gets no title yet shows the fallback. The text is title-cased by `Tmux::set_pane_title` (the first letter of each space-separated word upper-cased, the rest unchanged; hs-d475), which writes `@cpd_title`; the store's titles are unchanged. **The tree keeps showing ids** (hs-d101, "Labels" below): a title is never drawn in it.

**The dialog kit** (User, hs-d295, hs-d297; `tui/src/dialog/`). Every cockpit confirmation and input is a **tmux popup** running `cpd tui view dialog <confirm|input|text|checklist|select> …`, built on this one kit; the merge confirmation (`merge`, below; hs-d272, hs-d273) is its model. The kit has five kinds: **confirm** (a question with two pill buttons, for example `[Archive] [Cancel]`), **input** (a labelled text box, contract §6.1.1), **text** (scrollable read-only text), **checklist** (a scrollable list of rows the User ticks, with the pill buttons `[Approve] [Later]`; hs-d549, hs-d551) and **select** (a scrollable radio list, the cursor row picked with Enter or [OK], with the pill buttons `[OK] [Cancel]`; hs-d-tui-docs), below.
- **The caller keeps its actions.** It opens the popup with `display-popup -E`, which returns when the popup closes, then reads the **result file** the view wrote at the path the caller named with `--out <path>` (a temporary file the caller makes and removes): *confirmed* or *cancelled*, and for `input` the typed value. The view runs no command and holds no caller logic; the caller runs its `cpd` commands (§17). Decision hs-d621: the merge popup is the dialog kit's confirm (§13.2 below).
- **Style.** A dialog is one rounded frame and the popup is exactly that frame (hs-d578, amending hs-d569 CD1, hs-d475 to hs-d478 and hs-d298's section-title form): row 0 is `╭─`, the title as a pill, `─` up to `╮`; rows 1 to H-2 are `│`, the inside, `│`; row H-1 is `╰`, `─` × (W-2), `╯`. The title appears there and nowhere else (no tmux title, no header rows). A title wider than `W - 6` cells is cut to `W - 6`, and the top row keeps at least one `─` before `╮`; with no room for a pill (`W - 6 < 1`) none is drawn. The frame's corners, walls and lines (the `─` fill of the top row included) are colour 8 (muted); the title pill is the read-only pill (text in the terminal's background colour, in colour 7, caps colour 7 on the terminal background, `palette::pill_static()`), as every title pill. The view draws the frame itself and tmux draws none (`-B`, below), so a dialog cannot be dragged by its border any more. Buttons and keys are the `merge` popup's: a click on a button, Enter or `y` confirms; Esc, `n` or `q` cancels. In `input`, Enter accepts and Esc cancels, and `y`, `n` and `q` are typed text. Mouse capture is on. **Scrolling** is ↑↓, j/k, PgUp/PgDn and the **mouse wheel**, and End or `G` goes to the bottom (in `input`, the keys that are text stay text). A scrolling popup draws the scroll cues right-aligned (§17, "Scroll cues").
- **Size.** The popup is opened with `display-popup -E -B -w W -h H <command>` (`popup_args` in `tui/src/dialog/mod.rs`): `-B` means tmux draws no border, and there is no `-T`, so the program gets a pane of the full `W` × `H` (tmux 3.8: `stty size` 6 42 for `-w 42 -h 6`). The inside is 40 to 76 columns (`popup_size`: `(natural + 2).max(title chars + 6).clamp(40, 76)`); the popup is the inside plus 2 wide (the walls), capped at the newest client's width minus 2 (at least 1; `newest_client`, as the `merge` and `logs` popups do), and `body + 5` rows tall (the top and bottom borders and the three template rows below; it was `body + 5 + 2` with tmux's border), capped at the client's height (vertical margin 0). The `confirm` natural width is unchanged (`max(widest line, accept label + 6 + 8)`). The **merge popup** is fixed size 60 columns × 8 rows (hs-d621).
- **The frame and its rows** (hs-d578, template hs-n437; the Box rule, contract §6.1.1 "Boxes"). A popup of `H` rows with a body of `b` rows has `H = b + 5`: row 0 the top border, row 1 blank, rows 2 to `H-4` the body (`H - 5` rows), row `H-3` blank, row `H-2` the buttons, row `H-1` the bottom border. The two blank rows are spacing between elements, not padding. **Margin and padding:** the inside is `W - 2` cells wide and its first and last cell are the padding: content starts at popup column 2 (wall at 0, padding at 1) and ends at column `W - 3` (padding at `W - 2`, wall at `W - 1`), so the content width is `W - 4`; no padding row above or below. The popup has a margin of one column on each side (at most `client width - 2` wide, at least 1, so a column of the cockpit stays on each side) and 0 vertically. **The body:** `confirm` and `text`: each row is the padding cell, the row's text (wrapped or cut at `W - 4`), blank to the padding cell. `checklist`: entries as below, at width `W - 4`. `input`: the input box (the dialog's body rect, contract §6.1.1) is the box_widget, drawn from popup column 2 to column `W - 3` (the dialog's padding cells form the input's margin; `tw = (W - 2) - 6`, starting on popup row 2), with refusal rows directly under it (up to 2, the note colour, a leading space, cut at `W - 4`), the rest blank. **The buttons row** (row `H-2`): buttons are pills, the cap U+E0B6, the label, the cap U+E0B4, so `[Archive]` is nine cells. The accepting button (`confirm` the caller's label, `input` `OK`, `checklist` `Approve`; `text` `Close`, which is alone) is flush left: its left cap is at popup column 2. The other button (`Cancel`, `Later`) is flush right: its right cap is at column `W - 3`. Nothing is drawn between them. Colours (hs-d534: interactive pill 15, selected 12): the accepting button is colour 12 inverted (text in the terminal's background colour), caps colour 12; the other is colour 15 inverted (text in the terminal's background colour), caps colour 15. The whole pill, caps included, is the hit target. **The scroll position** of a scrolling body (`1-5/20`) stays on the buttons row, colour 8, right-aligned inside the padding (its last character at popup column `W - 3`), and only when at least one blank cell lies between it and the pill on its left and, if there is a right pill, between it and that pill; else omitted.
- **Drawn.** In every drawing `[` and `]` stand for the pill's caps, U+E0B6 and U+E0B4, one cell each (so a pill is as wide as its drawing), and `█` is the cursor cell. Each drawing is a whole popup, 42 columns wide, exactly as the User sees it.

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
- **`checklist`** (hs-d549, hs-d551; hs-d568, hs-d569): `cpd tui view dialog checklist --title <t> --items <json file> --out <file>`. `--items` is a JSON file, an array of `{"id": <record id>, "text": <full text>, "detail": <text>}` (hs-d569 CD2; `text` the record's full text; `detail`: `<reason> <target>` for a retirement, the doc paths joined by `, ` for a doc candidate; the caller builds it from `cpd context curation`, §3.15); a file that is not that shape is refused at the view's start, with the envelope of §1.2. The view draws a scrollable list of entries, one per array element, each of which is a header line `[ ] <id> — <detail>` (`[x]` when ticked) followed by the record's full `text`, wrapped to the body's width and indented by four columns; one empty row separates two entries (hs-d569 CD2). The cursor is an ENTRY: ↑↓ and j/k move one entry at a time, **Space toggles the cursor entry, a click on any line of an entry toggles that entry**, `a` toggles all entries (all ticked when any unticked, else all unticked), PgUp/PgDn and the wheel scroll the entries, and after the cursor moves the window shows at least the cursor entry's header line. The buttons are `[Approve] [Later]` (hs-d569 CD3: `[Later]` decides nothing; Esc, `q` or a click on `[Later]` close with `approved: false`, every entry stays pending). Enter or a click on `[Approve]` approves (ticked entries decided). The **result file** is `{"approved": bool, "ticked": [id], "unticked": [id]}`, the ids in entry order (on `[Later]` `approved` is false and the two lists are what was on screen; the caller ignores them). Colours are contract §6.1.1's, title pill 7, frame 8, body 7, the first button the blue pill 12, the other the white pill 15. Mouse capture is on. The view runs no command: the caller does (the Inbox click table below).
- **`select`** (new): `cpd tui view dialog select --title <t> --prompt <p> --option <o> [--option <o>…] --out <path>`. `--option` is repeated, in the order shown; at least one; each one non-empty single line without control characters, distinct, at most 70 characters; otherwise `args_invalid` like an unreadable checklist file. The view draws a scrollable list of rows, one per option, `(x) <option>` for the cursor row (the choice) and `( ) <option>` for the others, styled as checklist's cursor entry. Keys: ↑↓ and j/k move the cursor one option and stop at both ends (no wrap); Enter or `y` confirms; Esc, `n` or `q` cancels. Mouse: a click on an option row moves the cursor there and does not close; a click on [OK] confirms, on [Cancel] cancels; the wheel and PgUp/PgDn scroll the body when it is taller than the popup, and a cursor move scrolls so that the cursor row stays visible. The **result file** is `{"confirmed": true, "value": "<the option>"}` or `{"confirmed": false}`. Popup size: natural width = the widest of the prompt and `(x) ` + each option; body rows = the prompt's rows + one per option. The buttons are `[OK]` (first, left) and `[Cancel]` (second, right). Example, title `New Context`, prompt `Owner:`, options `clayton-duarte` and `other`, a popup of 42 × 8:
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

`merge` (title `merge`, three body rows with `<owner>/<repo>#<n>` in bold, a blank row, `method: squash`, 60 × 8; hs-d621):
```
╭─[merge]─────────────────────────────────────────────────╮
│                                                          │
│ clayton-duarte/cpd#321                                   │
│ Moon loader v2: 28 moon_alt phases at 50 ms in pi and…   │
│ method: squash                                           │
│                                                          │
│ [Merge]                                         [Cancel] │
╰─────────────────────────────────────────────────╯
```

- **The input box** (contract §6.1.1). The prompt is the box's label (trailing whitespace, then one trailing `:`, then trailing whitespace trimmed again; `Title: ` gives `Title`). There is no prompt row: the prompt moved into the box's top border. The box is always focused (border 12). The box starts on popup row 2 and is `W - 2` cells wide, with the refusal rows directly under it (up to 2, colour 8, a leading space, cut at `W - 4`; the frame and rows are "The frame and its rows" above), and its label pill follows the border's colour (12 here, focused). When the text needs more rows than available, the box shows consecutive rows from `start = max(0, cursor row + 1 - avail)` where `avail = max(H - 5 - 2 - REFUSAL_ROWS, 1)` and cursor row is `cursor / tw` (`tw = (W - 2) - 6`, `W` the popup's width; contract §6.1.1), the body being `H - 5` rows. The popup is sized so the box always fits the validator's longest accepted value (Plan id 24, title 200, name 80): `input_popup_size(client, title, prompt, initial, validator)` in `tui/src/dialog/mod.rs` computes the inside width and the height `body + 5` and caps both by the client (width minus 2, at least 1; height; no more `adds 2`; contract §6.1.1 has the worked sizes).
- **Validation** (`input`). The caller passes the check by name, from a closed set: `plan-id` (the Plan id of the `n` key, contract §2.1), `title` (`Title`, 1–200 characters) and `name` (`Name`, 1–80 characters). A refused value shows its reason **inside the popup**, which stays open, and the User keeps typing; only an accepted value is written to the result file.

**`tree`.**
- **Top row and side margins.** The view keeps one blank row at its top and one blank column on each side; its content starts one row lower, inside the inner area (the pane minus one column left and right), so the tree never touches the pane's edges. Everything the view draws sits inside it: the cursor's blue row (contract §6.1.1), the message row at the bottom (a command's refusal or result, never an input) and the right-click menu's clamping.
- **Rows** (User, hs-d337, hs-d339; replaces hs-d309's `┌` / `└` shapes). There is **no indent** and **no in-pane `Context` title** (tmux's pane title is enough). Each Context is a collapsible block (§17, "Collapsible blocks") drawn by the one shared widget (`tui/src/fold.rs`). **Always:** the title row is `╭` + the Context's glyph + its name (bold), for example `╭󰽢 building-cpd`, with **no Plan count** (main's count of a folded Context stood for the Plans folding hid; a fold hides no live Plan, hs-d344), and each content row, one per live Lead Session, is `├─` + the row's glyph + its text, straight against the branch, for example `├─󰽢 cross-repo       3⋅1⋅0⋅0⋅92`, with the counters right-aligned. **The toggle shows and hides only the archived rows** (hs-d344): live Plans and Sessions always show, folded or expanded. **Folded:** the title row, the live rows, then the closing row `╰󰬦 expand`. **Expanded:** the title row, the live rows, the archived rows (below, colour 8), then the closing row `╰󰬬 collapse`. A Context with **nothing to fold** (no archived rows; hs-d477) has **no closing row** and no toggle: its **last row's branch is `╰─`** instead of `├─`, for example

```
╭󰽤 homeserver
├─󰽤 b2-budget …
╰─󰽧 speedtest …
```

The Context block's rows (title, `├─` rows, closing row) end at column `W - 4`: the counters are right-aligned to it and the name is cut with `…` before them (the rail rule, contract §6.1.1 "The rail fold"; the branch rows' glyph placement is unchanged). A Context with archived rows keeps its closing row, `╰󰬦 expand` / `╰󰬬 collapse`, and its live rows keep `├─`. A LIVE Context with **no row at all** under it ends with the clickable row ` ╰󰬪 new` — the muted `╰` and glyph and the word `new` in colour 15 (clickable text) — and a left click on that row opens the New Plan input popup for that Context (the same action as `n` and the context menu's New Plan...). An ARCHIVED Context with **no row at all** keeps the plain muted terminator ` ╰󰬪` (U+F0B2A, colour 8, no word, no toggle; hs-d574, replacing hs-d459 and the plain closing `╰─` line). Each Context block is a collapsible widget and so has **one empty row above and one below** (hs-d476; the view's blank top row is the first block's, and the body has no vertical spacing). The closing row is drawn once per Context, never twice, by the shared helper (§17, "Collapsible blocks"; hs-d349), its lines colour 8 and its toggle (arrow and word) colour 7 (contract §6.1.1, hs-d534). The content rows are the Plans' Lead Sessions and every Session forked by `session fork` as siblings under the Context (B1, contract §6.8: a fork is a sibling under the Context, never nested under a Plan or under the Session it came from, whatever its `plan` or `forked_from`), ordered together at both levels by dot-urgency tier (red, yellow, magenta, cyan, shiro, then archived last), within a tier by creation (`created_at`, newest first; hs-d686, pro-d48); forks show no Job counters (the counters belong to the Plan Lead's row); then, for an expanded Context, **its archived Plans and Sessions, after the live ones, colour 8** (contract §6.1.1), with the same `├─` rows (hs-d339). There is **no archived `󰇘` row and no separate archived toggle**: the closing toggle is the archived rows' (hs-d344). Row: `├─`, glyph, one space, name (cut with `…` before the counters), and the four Job counters `planned⋅active⋅failed⋅done` right-aligned, and **only the nonzero roles are drawn** (a zero role draws no number and no separator; a Plan with every role zero, for example all its Jobs cancelled, shows no counters; cancelled Jobs are not counted; no counters without a Plan; hs-d636). **planned** is `draft + queued` (all not-started work: a Plan with 3 drafts and 7 queued shows 10, not 3), colour 13 (magenta, `Meaning::Planning`) while it has any Job in `draft`—a draft appended after the lock included (hs-d173, hs-d179)—else 15 (the queued colour, `Meaning::NotStarted`). **active** is `running + merging`, colour 14 (cyan, `Meaning::InProgress`), or 11 (yellow, `Meaning::Review`) when any of it is landing (**landing** counts the Jobs whose active Attempt is checking or merging, contract §6.1.1, hs-d604). **failed** is 9 (red, `Meaning::Failed`, always). **done** is `merged`, 10 (green, `Meaning::Done`). hs-d636 replaces hs-d623's draft-takes-the-first-slot rule and the five-counter row; hs-d580 keeps the role colours and amends audit Q3, hs-d303 for the counters. On an ASLEEP Lead's row every counter is colour 8 whatever the count, its dot and name staying 7; separators `⋅` colour 8. **A row has no icon** (hs-d337, which replaced hs-d266's tree icons): the corner or branch, the dot glyph, the name. **The selected row** (User, hs-d351, 2026-10-20) is the Plan or Session row whose Session is in the chat slot, or a Context row whose Manager is in the chat slot: its **name** (the text after the dot glyph, up to the counters) is drawn in blue (`palette` blue, §17, "Blue"), and blue **beats the muted colour of archived and asleep rows**; the rest of the row (the branch, the glyph, the counters, the PR status) keeps its colours, and the name is not bold for being selected. There is no marker in column 0. The glyph is one of `` U+E3E3 (moon_alt_new: the Session needs you, coloured by case), `` U+E3E3, U+E3C8..U+E3E2 (the 28 moon_alt phases, section 17; the Lead is working: a loader of 28 frames, one every 50 ms, on one shared clock, so every spinner turns together; red 9 while its Plan has a failed Job (hs-d581), else the bright pair of its Plan's phase colour: grounding grey 15, plan magenta 13, dispatch cyan 14, deploy yellow 11, done green 10 (hs-d622, hs-d624), §6.1.1), `` U+E3D5 (moon_alt_full: idle, nothing needed) or `` U+E3D5 (pi stopped, colour 8). A Session row's glyph looks at the Session's own state first (a working Session shows the spinner), and its `󰽢` comes only from its open, visible Inbox items (§17); a Context row shows the glyph of its Manager Session (hs-d215): a `󰽢` only from the Manager's own Inbox items (hs-d217), `󰽤` (colour 8) when the Context has none. Archived rows are colour 8 on every mark; asleep rows colour 7 except their Job counters, 8, and neither animates. The cockpit needs a Nerd Font for these symbols (https://www.nerdfonts.com/cheat-sheet). The rule, the colours and the precedences are §17's (The tree's dot rule) (hs-d622, hs-d624).
- **Cursor.** Keyboard only: a click never moves or shows it. It moves between Sessions only (Context rows are not selectable) and, after the last Session row, on into the Inbox section's item rows (below; the section title and the closing toggle row are skipped by the cursor but their click works, and the cursor goes back up from the first item row to the last Session), is drawn as the blue block (colour 4 inverted: text in the terminal's background colour, contract §6.1.1) across the full row only while the pane has keyboard focus (`focus-events`, §6.1) and after an arrow key; Enter, `n` and `x` act on it only while it shows.
- **Inbox section** (hs-d165, hs-d167, hs-d168, hs-d298, hs-d337, hs-d339, hs-d345 to hs-d349; contract §2.2 "Inbox item", commands §21). The bottom of the tree column shows the Inbox, the **single source of "needs you"**: the open, visible items (not snoozed, not dismissed) of **every** Context, in the order of `cpd inbox list` (red, yellow, magenta, blue, cyan, white; oldest first; `engine-down` first, when no live Manager exists). Its section title is §17's (hs-d475, hs-d478, hs-d480), the collapsible form (the helper's `collapsible_title`), a collapsible section widget with **one empty row above and one below** (hs-d476; the title and the rows follow with no vertical spacing): ` ╭─Inbox────…────35 ` (`35` the count of visible open items, **a pill too** (hs-d480); the title in Title Case, **a read-only title pill** (colour 7 inverted: text in the terminal's background colour, caps colour 7, contract §6.1.1), the count pill the same; the rest of the row a muted `─` line (colour 8) to the column before the count pill, which ends one column before the right edge; the row starts at column 1 and ends one column before the right edge, inside the panel's one column of padding on each side; the title's left cap is at column 3, hs-d478). Then **up to 3 item rows**, each the muted `│` (colour 8), one space (`│ `), **the item's tier glyph in its tier's colour, bright (the row is clickable; §6.1.1)** (hs-d483, replacing hs-d345's icon by kind: the state dots, the spinner glyphs, stay reserved for the Context tree and no Inbox row draws one), a space, then the text and age, the label cut with `…` so that it never reaches the age, the age right-aligned and ending at column `W - 4` (`42s`, `5m`, `3h`, `2d`: the largest whole unit of `age_seconds`; the rail rule, contract §6.1.1 "The rail fold"), for example `│ \u{F069A} cc-api-orc-apollo#2309 merg… 5h` and `│ \u{F0362} cpd#129 merge conflict       18m` (the escape stands for the glyph, written here as its code point; the first glyph is yellow, the second red); then the closing row, `╰󰬦 expand` when folded or `╰󰬬 collapse` when expanded. With 3 items or fewer there is nothing to fold and the section closes with the terminator ` ╰󰬪` (U+F0B2A, colour 8, no word, hs-d574). When more than 3 items exist, the list is a collapsible block (§17, "Collapsible blocks"; hs-d337): folded, `╰󰬦 expand` follows the 3 rows; a click on it expands the list and the row becomes `╰󰬬 collapse`, which folds it again. Only that row toggles. **The tier glyphs** (hs-d483; User, 2026-10-20; decided work, hs-d301). The tiers are named by their colours and rise in urgency white < cyan < blue < magenta < yellow < red (hs-d570 added the notice tier between white and magenta as cyan; pro-d16 names it cyan: blue means focus and selection only, amended by pro-d58 for the blue Inbox tier for external support, `review-requested`, between cyan and magenta). The glyph comes from the item's **urgency** (its tier), never from its kind, and is drawn in the urgency's existing colour; the six are named once, in `tui/src/palette.rs` beside the urgency colours:

| Tier (colour) | Glyph: code point and escape | Meaning |
|---|---|---|
| white | U+F0366 (`\u{F0366}`) | an unread reply |
| cyan | U+F06F1 (`\u{F06F1}`) (hs-d570, pro-d16) | a cpd notice: curation, nothing blocks (hs-d570) |
| blue | U+F0368 (`\u{F0368}`) (pro-d58) | external support: a review asked of you (review-requested Inbox kind) |
| magenta | U+F173A (`\u{F173A}`) (hs-d570: back from U+F06F1) | a decision before the work runs (a Plan review, a question while the Plan is still planning) |
| yellow | U+F069A (`\u{F069A}`) | pull-request work (your PR ready to merge, a team PR needing your review) |
| red | U+F0362 (`\u{F0362}`) | a blocker only the User can clear (a dialog while the Plan runs, `engine-down`, `engine-error`) |

The lines (`╭`, `│`, `╰`) are colour 8 and the toggle's arrows and words colour 7. The terminator `╰󰬪` is colour 8 whole (U+F0B2A; hs-d574). The pill and the count pill are read-only title pills (colour 7 inverted: text in the terminal's background colour, hs-d475, hs-d480, hs-d534), and the items are not (their own tier glyphs and colours; the fold only adds lines; the label is 15 and the age 8, contract §6.1.1). **The tree and the section share the column** (hs-d307, replacing the half-column cap of hs-n265). Let the *column* be the rows between the blank top row and the footer, the *tree's rows* the visible rows of the tree above, and the *section's rows* what the section wants: the empty row above its title, the title, the items, the closing row and the empty row below, three items when folded, all of them when expanded. While together they fit, each has its rows, the leftover rows stay empty between them, and the section stays pinned to the bottom. Otherwise the tree is guaranteed **two thirds** of the column and the section **one third**, rounded down (the tree's share is the rest); a section that wants less than its share gives the rest to the other (a short Inbox leaves the tree everything else, a short tree leaves the Inbox everything else); the folded section is never squeezed below its own rows (the empty row, the title, three items, the closing row, the empty row) except to leave the tree one row in a column that small. Folded, the section shows 3 items and `╰󰬦 expand`; expanded, it grows into the rows the rule gives it and then **scrolls inside them**. The tree scrolls inside its rows the same way: the wheel over it, 3 rows a notch, and the keyboard cursor (a Session row moving out of view scrolls it back; the wheel does not move the cursor), with the same cue rows (colour 8) of §17's "Scroll cues" (hs-d348), **right-aligned**, the last character of a cue at column `W - 4` (the rail rule, contract §6.1.1 "The rail fold"): `▲ <n> more` as its first row when rows are hidden above and `▼ <n> more` as its last when hidden below, `<n>` counting rows; a cue replaces a row and is no hit (one row of tree has no room for a cue and shows its row). The item rows **scroll inside their rows** with the mouse wheel, 3 rows a notch (as the Plan view), over the section only, with the cue rows (colour 8) of §17's "Scroll cues" (one helper, `tui/src/dialog/scroll.rs`, not a copy), **right-aligned**, ending at column `W - 4` (the rail rule): `▲ <n> more` as the first row of the list when items are hidden above and `▼ <n> more` as the last when hidden below, `<n>` counting items; a cue replaces an item row, and a cue row is no hit. The offset is clamped to the list, is view state only (starts at 0, a refresh keeps it, nothing is written), and the keyboard cursor on an item scrolls the list to keep it visible. The closing toggle row stays in view. Expanded or not is view state only: no command, no write. **The empty Inbox** (hs-d487; User, 2026-10-20; decided work, hs-d301): with no open, visible item the section is exactly these rows, between its empty row above and its empty row below (hs-d476): (1) its title row with the count pill at 0: ` ╭─Inbox────…────0 `; (2) then **one closing row**: ` ╰\u{F0B2A} up to date, drink some water` (`╰`, U+F0B2A, a space, the words; the escape stands for the glyph, written here as its code point). The whole row is colour 8 (contract §6.1.1), like the other fold lines. There are **no content rows and no toggle**: the closing row is not clickable (a click on it does nothing). The section is pinned to the bottom of the column and the tree scrolls above it.
- **Clicks on an Inbox item** jump to its cause (the cockpit never decides; each jump runs a command or is a link):

  | Kind | A left click on the row |
  |---|---|
  | `dialog`, `unread` | `session open <the item's Session>`, then `select-pane` on the chat slot: as a click on that Session's tree row (an archived Session: `session history`, as there) |
  | `plan-review` | `session open <the Lead of the item's Plan>` (`plans.lead_session`), then `select-pane` on the chat slot, then copies the item's `url` (the review page) as below when it is present, and says nothing; with no `url` (no open `plannotator_reviews` row of kind `plan` for the Lead, contract §2.2, hs-d502) the Lead opens and nothing else happens. If the Lead cannot be opened, nothing is copied and the failure is a `text` dialog (§17 "No notes") |
  | `loop-guard` | `cpd session resume <the item's Session>` in the background with no dialog (hs-d552, hs-d571: resumes a Lead or Manager blocked by the loop guard; L9); a refusal shows the failure dialog |
  | `pr-failing`, `pr-changes` (history rows only: hs-d470 dropped the kind, E8 opens none; no check or review state opens an Inbox item, hs-d470), and `review-requested` | copies the item's `url` to the clipboard as the tree's Copy id does (`tmux set-buffer -w`, OSC 52), and says nothing (the OSC 8 link stays for the terminal's modifier-click) |
  | `pr-ready` (a pull request a Plan owns or not) | the merge popup of its pull request, the one of the Deploy column's merge icon (`cpd tui view merge <owner>/<repo>#<n> --client <name>`, from the item's subject `pr:<owner>/<repo>#<n>`; a subject of another shape opens nothing); while that pull request's merge is running (the merge marker, below, hs-d495) its glyph cell shows the spinner instead and a click does nothing |
  | `engine-down` (shown only when no live Manager exists, below) | `cpd watch --ensure` (§12; idempotent), and says nothing (the result goes to the log only, never stderr; a failure is a `text` dialog); no popup (the StatusBar's `󰧭` button still opens the logs) |
  | `engine-error` (history rows only: hs-d314 dropped the kind, E8 opens none; the engine's errors are recorded in Events, counted in the Manager's turn-start digest, hs-d514, contract §3.7) | nothing: no jump, no popup |
  | `job-stuck` (history rows only: hs-d318 dropped the kind, E8 opens none; a stuck Job goes to the Context's Manager as a message, contract §3.7 "A stuck Job") | nothing: no jump, no popup |
  | `curate-retire` (hs-d549, hs-d551; hs-d568, hs-d569 CD4; hs-d592) | Exists only for a curable Context (R6 of hs-d592). Runs `cpd context curation --context <the item's Context> --limit=10` (§3.15; hs-d569 CD4: cockpit asks for 10) and opens the `checklist` dialog (§13.2) with title `Retire records of <context>` (with suffix ` (<shown> of <total>)` when total > 10, hs-d569 CD4) and one entry per row of `retire` (hs-d569 CD2: full `text`, header line `[ ] <id> — <reason> <target>`); on **Approve** runs `cpd context retire --context <context> --approve <ticked ids> --reject <unticked ids>` (§3.16), deciding only the shown entries; on **[Later]** (hs-d569 CD3: the button says it) or Esc/q runs nothing, every entry stays pending for the next opening. With nothing pending the dialog is not opened and nothing runs. A refusal of either command is a `text` dialog (§17 "No notes"). The click decides nothing by itself; dismissing the item (`d`) decides nothing either |
  | `curate-docs` (hs-d549, hs-d551; hs-d568, hs-d569 CD4; hs-d592) | Exists only for a curable Context (R6 of hs-d592). As `curate-retire`, with the entries of `docs` (hs-d569 CD2: full `text`, header line `[ ] <id> — <paths>`; `detail` the paths joined by `, `), title `Documentation candidates of <context>` (with suffix ` (<shown> of <total>)` when total > 10, hs-d569 CD4), and on **Approve** `cpd context docs --context <context> --send <ticked ids> --dismiss <unticked ids>` (§3.17: the ticked go to the Manager as one board post, the unticked are dismissed for good), deciding only the shown entries; on **[Later]** (hs-d569 CD3) or Esc/q nothing runs |
  | `question` (history rows only: hs-d306 (1) dropped the kind, E8 opens none) | `session open` of the Context's Manager Session (`cpd context manager --context <c>`, then `session open <manager>`, as a click on a Context row) |

  These three kinds (`plan-review`, `pr-ready`, `review-requested`) are also an **OSC 8 link** to the item's `url`, which stays for the terminal's modifier-click (hs-n265: a plain click always acts, the link is the way to open the page). The link is written with `tui/src/link.rs` over the cells already drawn (Links, below), so the terminal opens it with its modifier-click. For `plan-review` the page is plannotator's, `http://<review host>:<port>/`, the review host being `cpd_core::host::addr::review_host` (`PLANNOTATOR_URL_HOST` from cpd's environment, else the primary IPv4, else `localhost`; hs-d264), only when the item has a `url`: the Plan's Lead Session has an open `plannotator_reviews` row of kind `plan` (contract §2.2, hs-d502), and the port is the `port` column of that row (contract §6.5); for the other kinds the pull request's GitHub page. An item with no `url` (the `port` column missing or not a number for `pr-*`, or no open `plannotator_reviews` row of kind `plan` for `plan-review`) is drawn with no link, and a plain click acts as above (§13.2, hs-d502) |

  Whether the terminal opens an OSC 8 link on a row of this view through tmux is **unverified**; the cockpit Job tests it by hand with the User, as hs-d187, hs-d191 did for the PR links.
- **A stopped engine restarts itself** (hs-d328, hs-d363, replacing hs-d318's restart by the tree; contract §3.7 "Who restarts a stopped engine"). **The supervisor restarts it** (contract §6.6: the `pane-died` and `window-unlinked` hooks, a failed start retried and logged to `run/watch.log`); **the tree runs no `cpd watch --ensure` by itself**, because the supervisor covers every case a tree running in the same tmux server could see, and the tree's own restart raced it (it found the engine already running and left `engine was down: … found it running` on screen). A snapshot (§17) that reads the engine running, or a restart that `found it running`, shows nothing. The tree only **speaks**, and only when the engine **stays down past the supervisor's window**: when `locks/watch.lock` has been free on **every** snapshot for **60 s** (view state: the time of the first free read, forgotten at the first held one), the view sends, once per downtime, to **the one Manager whose Session state changed most recently** (hs-d514: the live Manager that was last active; a Session with role `manager`, not archived; `session send <manager> --wake`), `The cpd engine has been down for 60 s and its supervisor has not restarted it; see run/watch.log, start it with cpd watch --ensure.`: the view has no `CPD_SESSION`, so `from` is null (the User's side), and `--wake` wakes the Manager (hs-d514); with no live Manager nothing is sent. It shows no note (hs-d469, replacing hs-d364 (2)'s "short note"): the message to the Manager is the whole of it, and the view logs it. **The `engine-down` item shows in the Inbox section only when no live Manager exists** (with one, the message handles it: no red for what an agent handles, hs-d303); its click, the User's own explicit action (`Action::EnsureWatch`), is the one of the table above, and its reply goes to the log only, never to stderr (§17: a view never writes to its terminal outside its frame).
- **Clicks.** A click on a live Session opens it in the chat slot and focuses the chat (`session open`, then `select-pane`); a click on an **archived Session** row (a Plan row's Lead or a fork) runs `session history <id>` (§10.12), then focuses the chat; a Context is a collapsible block (§17, "Collapsible blocks"; hs-d337) whose title row is the Context row: a click on the closing toggle row (`╰󰬦 expand` / `╰󰬬 collapse`) shows or hides its archived rows, and a click on the title row, elsewhere on a live Context row, runs `cpd context manager --context <c> --by-user` (hs-d542 A1), then `session open <manager>`, then `select-pane` on the chat slot (B3-A6). For a by-user Session the Manager starts with no first message and waits for the User's first prompt to deliver it (hs-d540, hs-d542 A2, §15.7a). Clicks are ignored while a dialog popup is open.
- **Folds** (User, hs-d260). The folds survive a restart of the view: which Contexts have their archived rows hidden and which archived Contexts are expanded (the toggle shows and hides only archived rows, hs-d344; live Contexts start expanded and archived ones folded, hs-d339; the cursor and the scroll position are not kept). They are kept in `$CPD_HOME/run/tui/tree-folds.json` (contract §1.4), a JSON object `{"collapsed": ["<context id>", …], "expanded": ["<context id>", …], "archived": true}`: a Context in `collapsed` has its archived rows hidden (its closing row reads `╰󰬦 expand`) and one in `expanded` has them shown (`╰󰬬 collapse`); the `archived` key is the archive row's fold (absent or false = folded, true = expanded), starts folded; a live Context's own rows always show, and a Context with no archived rows has no toggle. It is under the same exception as the Plan view's offsets file (above): the view's own state, not Ledger state, written and failing as that file is. **Read** once, at the view's start: a Context in `collapsed` has its archived rows hidden and one in `expanded` has them shown, both counting as known; the `archived` key toggles the archive row; ids that match no Context are harmless, and a missing key is treated as false. A missing file is today's defaults. A file that cannot be read or is not that object is not an error: the view starts with the defaults, logs `fold state reset: <reason>` (no note, §17 "No notes"), and replaces the file at its next write. **Written** atomically (temporary file, rename, parents created) whenever the folds have changed, at most once per second; only Contexts that exist and differ from their start are written, so unknown ids are dropped at the next write. A failed write is logged (`fold state not saved: …`) and retried; it never ends the view.
- **`n`** asks for the new Plan's id in an **`input`** popup (the dialog kit above; validation `plan-id`; its question and texts are the former inline prompt's, `Fork <label> into a new Plan, name:`; Enter starts, Esc cancels, an empty or invalid id is refused inside the popup) and runs `plan new` (§4.1) in the selected Session's Context: the Plan forks the Context's grounded Lead (R15), the one deviation from cpd 1's `n`, which forked the selected session. On a Context row it says `pick the session to fork`.
- **`x`** closes the selected Session's pane after a **`confirm`** popup (the dialog kit above) asking `Close <label>? pi stops; the session file stays`, preceded by `<label> is working.` when its state is `working`, with `[Close]` and `[Cancel]`; only Enter, `y` or a click on `[Close]` closes. It runs `cpd session close <id>` (§10.9). The Session in the chat slot is refused with `switch to another session first`, also when it moved there while the popup was open. On a Context row it does nothing. `<label>` is the Session's name.
- **`s` and `d`** act on the Inbox item under the cursor (they do nothing on a Session row): `d` runs `inbox dismiss <id>` at once; `s` opens the snooze choices, the menu's three `Snooze …` entries (↑↓ and Enter pick one, Esc closes), then runs `inbox snooze <id> <15m|1h|tomorrow>`. Enter on an item row acts as a plain click does (above). A refusal (`item_closed`: the item closed meanwhile) is a `text` dialog with the failure (§17 "No notes"). The item leaves the section when the Inbox-* Event reaches the view (§17).
- **`?`** opens the legend popup. The view has no other key that opens the menu or acts on it; what a key used to do is still a command.
- **Labels** (contract §2.2: ids are unique, immutable and need not be readable; the title is the visible label). A Context row shows the Context's **id**, a Plan Lead row the Plan's **id**, and a fork row (a side Session) `<plan id>·<first 4 characters of its Session id>` (for example `cross-repo·7f3a`); a fork with no Plan keeps the fallback, the first 8 characters of its Session id. Archived rows follow the same rule. Titles and names no longer show in the tree (User, hs-d101; this supersedes the titles rule, hs-d63).
- **Archive folder row** (hs-d698, hs-d707). When at least one Context is archived, a single clickable row appears after the last live Context, one row, `󰬦 archive` folded (showing no archived Contexts) or `󰬬 archive` expanded (showing them). The row is colour 8 on every mark (branch, glyph, text), a hit target that toggles the archive row's fold (same as the two closing toggle buttons, but the whole row is the target); its fold is persisted in `tree-folds.json` under the `archived` key. Expanded, the archive row shows every archived Context and Plan, and every archived Manager Session, as muted blocks with the same structure as live ones (each Context's archived Plans and Sessions inside the collapsible block that usually holds the live rows, below them, and then the Context's archived Manager rows); they are muted (colour 8 on every mark, hs-d534), they have no indent, they render exactly as today but inside the archive row with no separate rows around them, and a click on an archived Session row runs `session history <id>` (above). The archive row is folded by default and appears only when it has archived Contexts to show; old files without the `archived` key still load. Its fold is view state only: no command, no write. Every row of an archived object carries `archived_at` from the read model (§17).
- **Right-click menu** (**every item is a `cpd` command**: the tree only runs commands). A right click on an Inbox item row opens the menu with its four entries (table below); a right click elsewhere opens the menu of its row. A right click opens a menu, drawn by the view (ratatui) over the tree at the click and clamped inside the pane. ↑↓ and Enter pick an item, Esc closes it; a click picks an item; a click outside closes it. There is **no key** that opens the menu: `n`, `x` and `?` stay as above. The tree has **no inline prompt row** (hs-d295, hs-d297): the `…` items (**New Context…**, **New Plan…**, **New session…**, **Rename…**, **Fork…**) type their text in an **`input`** popup of the dialog kit, with the questions, starting values and texts of the former prompt kept verbatim (`Title: ` for a Context's or Plan's title, `Name: ` for a Session's name); **Rename…** starts with the current text; validation is `title` for Contexts and Plans, `name` for Sessions. **Close** asks in a **`confirm`** popup first, as `x` does. **Archive** asks in a **`confirm`** popup, `Archive <label>?` with `[Archive]` and `[Cancel]` (hs-d297; this supersedes hs-d90's "Archive runs at once, with no `y/N`"; the rest of hs-d90 stands). The commands each item runs, and the opening of the new Lead below, are unchanged. The items by row:

  | Row | Items | Command each runs |
  |---|---|---|
  | Inbox item | Snooze 15 min, Snooze 1 h, Snooze until tomorrow 09:00, Dismiss | `inbox snooze <id> 15m`, `inbox snooze <id> 1h`, `inbox snooze <id> tomorrow`, `inbox dismiss <id>` (§21.3, §21.4) |
  | Context | New Plan…, Rename…, Copy id, Archive, Delete | `plan new` (§4.1), `context rename` (§3.7), `context archive` (§3.8), `context delete` (§3.14) |
  | Plan | New session…, Rename…, Copy id, Archive, Delete | `session fork <the Plan's Lead>` (§10.3), `plan rename` (§4.9), `plan archive` (§4.10), `plan delete` (§4.14) |
  | Fork (side Session) | Open, Rename…, Fork…, Copy id, Close, Archive | `session open` (§10.4), `session rename` (§10.7, `--session <id>`), `session fork` (§10.3), `session close` (§10.9), `session archive` (§10.10) |
  | empty tree area | New Context… | `context new` (§3.1) |
  | any archived row | Rename…, Copy id, Unarchive, and Delete on a Plan or Context row (hs-d527; also on a live Plan row whose Lead is asleep, and a live Context row whose Plans are all asleep or archived, which then have Delete last) | `context rename` (§3.7), `plan rename` (§4.9) or `session rename` (§10.7), `Copy id` (no cpd command; below), and `context unarchive` (§3.9), `plan unarchive` (§4.11) or `session unarchive` (§10.11); the rename commands accept archived objects (F4) |

  **Copy id** (hs-d102) is offered on every row that has an id: Context, Plan, fork and archived rows. It copies the row's shown id (see Labels) to the User's clipboard with `tmux -L <socket> set-buffer -w -- <id>`; tmux sends it to the outer terminal by OSC 52, which works over ssh when the terminal allows it (tmux's default `terminal-features` give `xterm*` the `clipboard` feature). Whether it reaches the User's Ghostty on the Mac is **unverified** until the User tries it. It says nothing. It is the one item that runs no `cpd` command: it changes no Ledger state.

  **Opening the new Lead** (hs-d103). After **New Plan…**, **Fork…** or **New session…** succeeds, the tree runs `session open <the new Session id>` (from the command's JSON: `plan new`'s `lead.session`, `session fork`'s Session id), then focuses the chat slot, as Open does. The commands themselves never move the chat slot.

  **Archive of the chat slot's Lead** (F3, hs-d90). Archive on a Session row (a Plan row's Lead, or a fork) whose Lead holds the chat slot, or a Context row whose Manager holds the chat slot, cannot go straight to the archive command, which refuses with `in_chat_slot` (§10.10, unchanged). The tree first frees the slot, then archives: it opens the next live Session in tree order, excluding every Session the archive cascade archives (`session open`, then focuses the chat); if none follows, the previous one; if there is none at all, it runs `session park`, so the placeholder holds the slot. Then it runs the archive command. Only the tree does this two-step; the CLI keeps its refusal.

  A refusal (`archive_blocked`, `in_chat_slot`, `archived`, …) is a `text` dialog with the failure (§17 "No notes").
- **New Plan… and New Context…** ask a title in an `input` popup (validation `title`; New Context… also the owner, below); the tree derives the id. The id is a lowercase ASCII slug of the title: each run of other characters becomes `-`, leading non-letters are dropped, and it is cut to fit the id pattern (Plan `^[a-z][a-z0-9-]{1,23}$`, Context: contract §2.1); `-2`, `-3`, … are appended while the id is taken in the tree's snapshot. New Plan… runs `plan new <id> --context=<c> --title=<title> --by-user` (hs-d542 A1): no repo, a Plan has none (R34, A12; its PRs are added with `pr add`). For a by-user Session, pi starts with no first message and waits for the User's first prompt to deliver it (hs-d540, hs-d542 A2, §15.7a). New Context… also needs the owner (§3.1): after the title, a `select` popup (title `New Context`, prompt `Owner:`, rows the policy files' stems sorted) asks the user to choose from the owner policy files in `/policy/` (the owner directories) (`<owner>.toml`, the file stem is the login; contract §8.3), with the first row selected at start; the rows are in the order the tree derives the owner list (sorted), and the first is always selected at start. Enter or [OK] confirms the row under the cursor and runs `context new <id> --title=<title> --owner=<chosen>`; Esc or [Cancel] cancels and nothing runs. With no policy file the refusal is a `text` dialog `No owner policy: add $CPD_HOME/policy/<owner>/owner.toml`, running nothing. A title no id can be made of is still refused right after the title, before the owner is asked, in a `text` dialog.
- **Right button.** The view relies on tmux 3.4's default root `MouseDown3Pane` binding: believed to pass the click to a pane whose app captures the mouse (`mouse_any_flag`), and to show tmux's own menu otherwise (**unverified**). cpd adds no binding for it (§6.1, §13.1 unchanged).

**`plan`.**
- **No top row, no tabs** (User, hs-d102). The PR boxes start at the top of the pane, inside the margin below; the view draws no Context id, no tab and no separator line. Switching Leads is the tree's (Enter or a click). With no Lead in the chat slot, the message row reads ` no Lead session in the centre` (colour 8). With a Manager in the chat slot, the pane shows Pulse (one row per Plan of the Context), not the Plan view. With the placeholder or a history pane in the slot, the message shows.
- **Chart:** the Plan's Jobs in a flowchart of boxes (reference `flow.rs`, ported), laid out when a snapshot arrives or the pane width changes, and at no other time; at a frame change only spinner glyph cells change, never the layout. Each PR box is a `BoxWidget<Full>` (the box widget, `tui/src/box_widget.rs`), a full box with rounded corners (`Borders::ROUNDED`, hs-d474), whose body is the flow chart of that PR's Jobs; the box's title row is drawn by the widget and reads ` ╭─ <key> · <repo name> 󱦰 <base> · <pull request> ` (one `─` after the corner, then the key, B5), and its body contains the chart; the chart has no padding of its own (the widget's one column of padding is the only one). Inside a blank margin of 1 column and 1 row (`CHART_MARGIN`; the Box rule, contract §6.1.1 "Boxes": one blank column each side, one blank row above the first box and below the last, the canvas) that belongs to the chart's canvas, not to the scroll window (which is the whole body, so scrolled content reaches the pane's edges; at scroll (0, 0) nothing touches them; this is the one place where the pane clips a box); the layout test asserts the margin of the container at scroll (0, 0) only and the walls of the boxes the pane shows whole. A PR with no Jobs shows an empty box (` no Jobs `, colour 8). Cancelled Jobs shown as a colour 8 pill (hs-d518, hs-d534). Pulse's `MARGIN` (2 columns, the **Row layout** and **Buckets** bullets) is unchanged, as are the chart's message row's: Pulse rows and that row are not boxes. Each Job is a pill: `` (U+E0B6), the compact body `<icon> <key>[│#<n>]│<tier>` (hs-d474: no padding inside the caps and none before the `│`; pro-d62: `#<n>` is the Job's Attempt count n, shown only from the second Attempt on, between the key and the tier, so the tier is always the last cell of the pill; e.g. `󰗠 titles-docs-2│S3` and, from the second Attempt, `󰗠 titles-docs-2│#2│S3`; the label is `#<n>`, while Attempt ids in commands, logs and the store stay `aN`; the keys of a column are padded so the right edges align). The `<tier>` is the Tier a Job is shown at, derived by contract §3.4a: the Tier of its active Attempt, or its `retry_tier` while queued at that Tier after a `job retry`, or the Tier of its latest Attempt, or its stored `tier`. After a `job retry --tier <T>`, a Job shows `<T>` until an Attempt starts at that Tier. The `│` (U+2502) in the pill's style (User, Plan tui-visuals), then `` (U+E0B4). The two caps are one cell each, so the pill is two cells wider than its text, and the layout's widths follow from the text; they are drawn as a **coloured pill** (hs-d534, contract §6.1.1: the status colour's base colour as the background (a pill that is not clickable takes the base index, contract §6.1.1 rule (3); pro-d63), text in the terminal's background colour (inverted), the icon included, caps the same colour as `fg`: draft 5, queued 7, running 6, landing 3, merged 2, failed 1, cancelled 8; from `palette::pill(…)`) on the normal background, so the pill has rounded ends (hs-d266; mockup: `✓ header-reverse│S2`, the `✓` standing for the status icon). A `running` Job's icon spins `` (the 28 moon_alt phases, one frame every 50 ms on the shared clock, as listed in §17 section 17 "The tree's dot rule"); every other status, landing `󱑤` included, stays static; the `queued` icon stays `󰝦` U+F0766 (nf-md-circle_outline, hs-d235; the Lead's assumption in hs-d579: it is not a dot of the tree). A `cancelled` Job's pill is colour 8; colours and icons are unchanged (User, hs-d567). A pill's colour shows the status again (hs-d534, which replaces hs-d530's single interactive colour; the icon shows it too) and the Job pills take the base colours and the tree's counters the bright pairs (they sit in clickable rows), the tree's failed counter staying always red (hs-d580, §13.2). A `failed` Job's pill is **red in every case** (hs-d586): with the static glyph U+F0159 while the Plan's Lead is not working (`sessions.state ≠ 'working'` of `plans.lead_session`, contract §3.6), and the moon loader (the 28 frames of §17 section 17, one frame per 50 ms on the shared clock `palette::frame`) while it works; every failed pill of the Plan turns together. A `cancelled` Job's pill is colour 8 with the static U+F0159. A Job pill is read-only (pro-d63): a click on it and every key do nothing, no pill is selected and no pane opens. **Job connector lines** (R1, R5): a Job's connector line leaves from the right cap of its own pill (the cell right after the pill's last cell, where the left arm of a `─` or `┤` points) and enters the Job it feeds at the cell just left of that pill's left cap (where the right arm of a `─` or `├` points), with an arm pointing toward the cap. Pill widths do not change: a `cancelled` Job's pill is as wide as itself and never widens a live pill; a live pill is as wide as the widest live pill of its column. A card of several stacked Jobs with the same needs is held by a bracket on each side: the left bracket (one cell left of the pills' left caps) and the right bracket (one cell right of the pills' right caps) touch every pill of the card. **Every connector cell touches what it joins** (R5): in the drawn view, scrolled or not, every connector cell's arms lead to another connector cell, to a pill's cap (Job lines) or to a `┬` / `┴` on a box border; no line ends in a blank cell, in the title's text or on a plain `─`. The glyphs and their arms: `─` left,right; `│` up,down; `╭` down,right; `╮` down,left; `╰` up,right; `╯` up,left; `├` up,down,right; `┤` up,down,left; `┬` left,right,down; `┴` left,right,up; `┼` all four. Messages are one row at the bottom of the pane: `no Plan yet`, `loading… (first fetch)`, or the note.
- **PR boxes** (R34, A15; contract §2.5). The chart is split into **one box per PR** that is not `cancelled`, each a bordered block (`Borders::ALL`, colour 8, with rounded corners `╭ ╮ ╰ ╯` instead of `┌ ┐ └ ┘`, hs-d474; every other line and junction, `─`, `│`, the `┬` and `┴` where the needs links join a box, stays) holding the cards and Dependency connectors of that PR's Jobs, laid out inside it as the single chart was. A PR with a base PR or a need of another Plan carries one more line under its title, `on <plan>/<key>` (the base PR, a reference printed bare for this Plan's own) and `needs <plan>/<key>` for each need of another Plan, colour 8; a same-Plan base PR is also an arrow, dashed, and orders the boxes as a need does; a reference to another Plan has no box and no arrow (contract §2.5, hs-d324). The box's title, on its top border, reads ` <key> · <repo name> 󱦰 <base> · <pull request> `: the PR key in bold (colour 7, the title's read-only text; contract §6.1.1); the repo's directory name (the last component of `prs.repo`); its base; then `#<number> <state>` once the PR has a pull request, where `<state>` is `open`, `merged`, or `closed` while the pull request is closed without a merge (`pull_request.state` is `closed`, A11), in the palette's PR state colours (contract §6.1.1, hs-d534; the box title is a hit target, so they are the bright pairs: `open` is 15, and 11 only when the PR is ready to merge, `merged` 10; reference `pr_state`; `closed` is cyan (`Meaning::Waiting`'s colour) while the Lead handles it, and **red only when the Plan's Lead has an open, visible `dialog` item**, it asks the User about it; a closed pull request no Plan owns is red, but no box draws one; audit Q4, hs-d303); `<state>` reads, instead of `open`, when the pull request is `open` and its `mergeable` is `conflicting` (`unknown` is not a conflict; `merged` and `closed` never read either word), **the conflict rule** (hs-d532, written once here; §13.2's PRs column and the legend refer to it): `updating` while the PR has an update Job (`jobs.kind = 'update'`, key `up-<pr>`, hs-d490, hs-d340) that is not ended (`status` `queued` or `running`), in cyan, whether that Job is `running` or `queued` (both are the Lead's agent activity, hs-d534: the two colours of hs-d532 are one); `conflict`, in cyan too (not red: every box is a pull request a Plan owns, so its Lead fixes the conflict, hs-d302, hs-d303), only when no such update Job exists. For example ` api · api-repo 󱦰 main · #12 updating 󱆭 ` and ` api · api-repo 󱦰 main · #12 conflict 󱆭 ` (the state word cyan). The update Job comes from `tui/src/read/flow.rs`'s `Pull`; the colours only from `tui/src/palette.rs`. Else the PR's status (`waiting`, `dispatch`) colour 8. **While the Plan's Lead works** (`sessions.state = 'working'` of `plans.lead_session`, contract §3.6; hs-d335) and the PR has a pull request, the Lead's working dot follows the state word, a space between: `#<number> <state> <dot>`, the dot being the tree's spinner (``, the 28 frames of §17 section 17, one frame per 50 ms on the shared clock, `palette::frame`; the state keeps its colour, the dot is the bright pair of the Plan's phase colour (hs-d622, hs-d624)); an idle, waiting, blocked or stopped Lead shows none, and the title is then as above. Once the PR has a pull request, the title ends with the `Link` widget (Links, below; pro-d60), which draws `#<number> <state>` (and the dot, which is not part of the link) as underlined text, then one space, then the cue `󱆭`: the whole `#<number> <state> <dot> 󱆭` appears as the underlined state and cue, the whole output clickable, and a plain left click opens `pull_requests.url` in the default browser (pro-d60). The PR key, the repo, the base and the spaces around the link are not linked; a `cancelled` PR (hs-d518) shows `cancelled`, and a PR that is `waiting` (display status only, hs-d523) is drawn colour 8 (User, hs-d567). The boxes are laid out **top to bottom** in the order of the needs graph, one row per level: a box sits below every box it needs; boxes of the same level sit side by side in that row, by key (hs-d115). A merged PR's level is 0 with no needs, else one more than the highest level among its needs. A PR that is not merged (any status but `merged`) is at the level max(1 + the highest level among its needs, M + 1), where M is the highest level of any merged PR (no floor when none is merged): **every box that is not merged is drawn below every merged box**, so the work in progress sits together, close below the finished work, instead of sharing a wide row with it (User, 2026-10-18). Each need is a plain box-drawing line from the needed box's bottom border to the needing box's top border: a `┬` on the bottom border, a line with `╭╮` corners, and a `┴` reaching the top border ON the needing box's top border; there is no arrowhead, the direction being implicit (the needed box sits above; hs-d638), and the ┬ exit is unchanged. The landing `┴` is on the box's middle column, or when the title covers that column, on the first `─` after the title. Inside a box the Jobs still flow left to right, and a box is as wide as the larger of its own chart and its title plus 4 cells: `╭`, `─`, the title with its own leading and trailing space, at least one `─`, `╮`. The view also re-reads its snapshot when a `pull_requests` row changed (see Rates), with the Event-driven coalescing as before: a conflict clears with no Event (`mergeable` changes silently; only becoming `conflicting` appends `pr-feedback`), so without that a box would keep `conflict`; a change of the update Job's status (`job-started`, `job-merged`, `job-failed`, `job-cancelled`, `job-added`; Events with `plan = p`) re-reads it too, which moves `updating` to `conflict` or back. It reads on no timer.

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
  - **Bars.** A bucket with activity: a bar `▁▂▃▄▅▆▇█` of level ceil(count * 8 / max) (1..8), where max is the largest bucket count over ALL rows (one scale, so rows compare; counts and scale as before hs-d327), in the colour of the bucket's kind (Kinds above): red, magenta, cyan or green, each the bright pair (`Color::LightRed`, `LightMagenta`, `LightCyan`, `LightGreen`). A bucket with no activity: a `▁` in colour 8, so the line never breaks. A row with fewer buckets than the line's n is blank on the left. No `󰗠`, and no distinction for a running Attempt.
  - **Age.** Minutes since the Plan's latest activity Event (any time, not only the window); `0m` while one of its Attempts is not ended (status starting, running or merging). The age text follows the tree's rule below (with minutes converted to seconds). Colour 8. Blank when the Plan has no activity Event at all.
  - **Quiet.** A Plan is quiet when all four hold: it has queued or running Jobs (drafts do not count); no Attempt of it is not ended; its latest activity is 30 min old or more (no activity at all also counts); its Lead's dot is not a needs-you dot (`Dot::needs_you()` false). The age cell then reads `quiet <age>` (e.g. `quiet 41m`) in colour 8, like the age (hs-d534: it was the Review colour). The 30 minutes are a constant, not a config key.
  - **Clicks and wheel.** A left click on a row runs the Plan view's `Actions::open_lead(<the Plan's lead_session>)` (session open, then focus the chat slot); a failure is a `text` dialog (§17 "No notes"). The wheel scrolls the rows (3 rows a notch, clamped). No keys, no selection.
  - **Re-read.** A new Pulse snapshot on any Event of the Manager's Context (`events.context`) and on `session-moved`, coalesced as other views; also every 60 s (the window moves and ages tick; a running Attempt is not an Event), and on pane resize (the bucket count changes). Between reads the rows do not change: a row has no spinner.
  - **Escalation bar.** Cyan, agent activity (hs-d534 replaces hs-d352's blue: blue is focus and selection only, contract §6.1.1).
  - **Empty.** A Manager whose Context has no row shows ` no Plans yet` (colour 8) in the message row.

    Example, a 60-column pane, id_width 10, so n = 60 - 4 - 23 = 33 buckets (colours by kind, as above; a `▁` in colour 8 is an empty bucket):

    ```
      cross-repo  █▃▁▁▄▁▁▁▁▁▁▂▁▂▁▁▄▁▁▃▁▁▁▂▁▁▁▁▅▁▁▂▇         4m
      manager     ▁▁▁▁▁▃▁▁▁▁▅▁█▄▁▁▃▁▇▅▃▁▂▁▁▁▁▁▁▁▁▁▁         3m
      quality     ▁▄▃▁▁▁▁▁▁▁▄▄▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁  quiet 41m
    ```

**`widgets`** (reference `dispatch`), top to bottom: one blank row at the top, the first section without a header, and the section title (§17, "Section titles"), with one empty row above and one below, in place of the shared separators and the bold section titles, as `Layout::vertical([Length(1), Length(2), Length(1), header, Length(1), Min(1)])`: blank row, machine, then the section title (` ──PRs──…──<n> `, the plain form, Title Case, a read-only title pill (colour 7 inverted: text in the terminal's background colour), the count a pill too, one column of padding each side: hs-d475, hs-d476, hs-d478, hs-d480) with one blank row above it and one below it, then the PRs, which take the rest of the column. There is no git and no Jira section (hs-d257). The title of the pane is ` Deploy ` (contract §6.1).
1. the machine (no header): its hostname in bold (` <hostname>`; the row is colour 7), then ` <cpu icon> N%  <gpu icon> A% B%  <mem icon> M%` (hs-d257, hs-d333), for example `  23%   61% 0%   41%` under ` homeserver`, with the CPU icon `nf-oct-cpu` (U+F4BC), the GPU icon (U+E266) and the memory icon (U+EFC5). The GPU part has one utilization % per GPU, in the order the tool lists them, one space between a GPU's numbers and two spaces between the parts; a GPU that reports no number is `–%` in its place. When no GPU is found (neither tool is available, or its output has no GPU) there is no GPU part at all and the row is `  N%   M%`; a missing tool is an empty list, never an error. All numbers are integers, drawn in colour 7 (read-only text, contract §6.1.1; no thresholds); the load average is not shown. CPU % is the share of non-idle time across all CPUs over the last 2 seconds, so the view keeps its previous CPU sample, and shows `–%` until a second sample exists; a number that cannot be read is `–%` too, which is no error of the view (as the hostname, tui-W10). Memory % is used / total. GPU % is the GPU's utilization. All are re-read every 2 seconds. On Linux the CPU comes from two reads of the `cpu` line of `/proc/stat` (idle = idle + iowait) and the memory from `/proc/meminfo` (`MemTotal - MemAvailable`, over `MemTotal`). On macOS the CPU is the sum of `ps -A -o %cpu=` divided by `sysctl -n hw.ncpu`, capped at 100 (a point-in-time figure, so it needs no earlier sample), and the memory is `vm_stat`'s pages active + wired down + occupied by compressor, times the page size `vm_stat` prints, over `sysctl -n hw.memsize`. The GPUs come, on Linux, from `nvidia-smi --query-gpu=index,utilization.gpu --format=csv,noheader,nounits -lms 2000`, one long-running process for the view's life (one integer per line, one GPU per line; a line that is no integer is `–`), and on macOS from `ioreg -r -d 1 -w 0 -c IOAccelerator` (each accelerator's `"PerformanceStatistics"` dictionary has `"Device Utilization %"=N`; an accelerator without that key is `–`). The macOS parse is unverified on hardware: it is tested on a captured sample only;
2. title `PRs`, with the **count** of the listed pull requests right, as a pill (hs-d298, hs-d475, hs-d478, hs-d480: ` ──PRs───────3 `), then **one flat list of every `open` row of `pull_requests` with `authored` 1** (hs-d96, D11; contract §2.2 "Pull request"; review requests are Inbox items, §21, not the User's pull requests, so rows with `authored` 0 are not listed), oldest `created_at` first, two lines each: the first ` <owner>/<repo>#<n>` in bold (`repo` and `number`; 15 on a row a Plan owns, 7 on one none owns), the second `  <open|draft> <checks icon> <review>  <title>`: `draft` when the row's `draft` is set, else `open` (the word's colour, contract §6.1.1, hs-d534: `draft` magenta; `open` yellow only when the PR is ready to merge, `merge_state` `clean`, else colourless text, 15 on a row a Plan owns and 7 on one none owns; owned rows are clickable and draw the bright pairs); the checks icon in the palette's `checks` colours (contract §6.1.1, hs-d534: passed green, pending cyan, none 8, `failed` cyan on a pull request a Plan owns and red when no Plan does, hs-d302; owned rows are clickable and draw the bright pairs; reference `checks`): `󰗠` passed (U+F05E0 md-check_circle), `󰅙` failed (U+F0159 md-close_circle), `󰮔` pending (U+F0B94 md-eye_circle), `󰍴` none (U+F0374 md-minus); the review `approved`, `changes requested` or `no review`; **a space and the Lead's working dot right after `open`/`draft`** (hs-d335) when the pull request's owner's Lead is working (`sessions.state = 'working'`; the tree's spinner of §17 section 17, one frame per 50 ms on the shared clock, `palette::frame`, the bright pair of the Plan's phase colour (hs-d622, hs-d624)), for example `  open 󰽡 󰗠 approved  <title>`;a pull request no Plan owns, or whose Lead is not working, shows none and the line is as before; then, when `pull_requests.mergeable = 'conflicting'`, for a pull request a Plan owns the word of §17's conflict rule (hs-d532, PR boxes; fed by `tui/src/read/widgets.rs`'s `OpenPr`): ` updating` while its update Job is `running` or `queued` (cyan), else ` conflict` in cyan (`palette::fg(Meaning::Waiting)`; its Lead fixes it, hs-d302, hs-d303), for example `  open 󰗠 approved updating  <title>` and `  open 󰗠 approved conflict  <title>`; for one no Plan owns (no update Job) ` conflict` in red (`palette::fg(Meaning::Failed)`, base 1 on a read-only row; hs-d219, B3), as `  open 󰗠 approved conflict  <title>` (`'mergeable'` and `'unknown'` show nothing); the title colour 8 and cut first when the column is narrow. The pull requests of the PRs of the Plan whose Lead is in the chat slot (`prs.pull_request`) are drawn as the blue block (colour 4 inverted: text in the terminal's background colour, contract §6.1.1) and bold on both lines, every mark inside it with inverted colour (rule (3): coloured text never sits on a coloured block; this replaces hs-d219's flag that kept its colour under the reversed-bold style). Plans without a PR are not listed. With no open row: ` no open PRs` (colour 8);

Each PR row has two areas, and a **merge icon** (below). The **`󱆭`** sits at the right end of the first line and links to `pull_requests.url` (Links, below). The **row** is the rest of both lines. The PR's **owner** is the Plan of the `prs` row whose `pull_request` is this pull request (the most recently created Plan when several exist); its Lead is `plans.lead_session`. On an owned PR, a left click anywhere on the two lines except the `󱆭` cell runs `cpd session open <lead>`, then focuses the chat slot, as the Plan view's open-Lead action does. A PR no Plan owns (for example one of the User's work repos) is read-only on both lines (the base colours of §6.1.1: its failing checks and its conflict are red 1), and a click on it does nothing. A left click on the `󱆭` opens the pull request's URL in the default browser through the `Link` widget (Links, below).

**The merge icon** (User, hs-d272, hs-d273, hs-d274). `󰘭` **md-source_merge U+F062D** (Material Design Nerd Font, hs-d235) sits in the **last column of the second line**, under the `󱆭`. It is shown only when the pull request's stored `pull_requests.merge_state` is `'clean'` (GitHub's `mergeStateStatus` `CLEAN`: every requirement met; contract §7.6 stores GitHub's value lowercased, one of `behind`, `blocked`, `clean`, `dirty`, `draft`, `has_hooks`, `unknown`, `unstable`); with any other value the cell is blank. It shows on every row that qualifies, owned or not. A left click on the icon does not open the Lead: it opens the **`merge` popup**, the dialog kit's confirm (§13.2, hs-d621), a tmux popup `display-popup -E -c <name> -B -w 60 -h 8 -- cpd tui view merge <owner>/<repo>#<n> --client <name>`, opened as the legend and `logs` popups are (`tui/src/logs/mod.rs` `open`, `popup_args`; popups are the cockpit's pattern for dialogs, hs-d273). The popup shows the pull request (`owner/repo#n` and its title) and the method (`squash`), with `[Merge]` and `[Cancel]`. Enter or `y`, or a click on `[Merge]`, **closes the popup at once and merges in the background** (User, hs-d473; every cockpit action is a cpd command, hs-d61): the popup runs `tmux run-shell -b 'cpd tui action merge <owner>/<repo>#<n> --client <name>'` and exits, so neither the cockpit nor the User waits for `gh`, and the run, owned by the tmux server, outlives the popup. `<name>` is the client the popup opened on: the opener names it, as above, and the view passes it on (the same rule as `cpd tui action` below); the opener, the icon's click, the menu's **Merge** and the Inbox `pr-ready` item alike, so all three behave the same. The command line must reach `cpd` unchanged: the popup quotes it for tmux's shell and tmux's format expansion (the `#` of the reference). Esc, `n`, `q` or a click on `[Cancel]` closes it with nothing done, and the popup's look before the confirm is unchanged. The popup shows nothing after the confirm: no `merging…`, no result, no error. The `merge` view has no pane of its own and writes nothing itself; the action's failures are below (the `merge` action, §13.2). When `session open` refuses or fails, the click opens a `text` dialog with the failure (§17 "No notes"); the column has no note row.

**A merge in progress** (User, hs-d495, hs-d550 B4–B5; the marker design is the Lead's, the User may correct it). From the confirm until the background request ends (`cpd tui action merge`, below), **every place that shows that pull request's merge button shows the cockpit's spinner (`palette::SPINNER`, framed by the shared clock as for a running Job) instead, and it cannot be clicked**: this merge icon, the right-click menu's **Merge** entry (below), and the Inbox's `pr-ready` item (§21: its glyph cell shows the spinner, a click does nothing). On success the merge's `cpd pr poll` clears the pull request from the lists; on failure the indicator goes and the failure dialog opens (hs-d469 `dialog::show_error`). **One marker is the source every view reads:** `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json` (contract §1.4), one file per pull request. The views (the Deploy column, the tree's Inbox, the PR menu) read the markers on each snapshot, and a restarted view reads the same files, so all views and restarts agree. **B5** A pull request is merging (spinner shown) when its marker's pid is alive, or when its marker has `"merged": true` and its pull request's `pull_requests` row is `open`; a marker with `merged: true` whose row is not open, and a marker without it whose pid is dead, are ignored.

**The PR row's right-click menu** (User, hs-d296). A right click on a PR row (either line, owned or not) opens a menu with these entries, in this order:

| Entry | Shown | Runs |
|---|---|---|
| **Merge** | only when the row shows the merge icon (`merge_state` `'clean'`, above) | opens the same `merge` popup as a click on the icon, which on its confirm merges in the background (above; hs-d473); while that pull request's merge is running (above, hs-d495) the entry is shown with the spinner and cannot be selected |
| **Copy Link** | always | no `cpd` command: copies the pull request's URL (`pull_requests.url`) to the clipboard with `tmux -L <socket> set-buffer -w -- <url>` (OSC 52), the way the tree's Copy id copies (Copy id, above); changes no Ledger state |
| **Go to Lead** | only on an owned PR; absent for a PR no Plan owns | `cpd session open <lead>` (§10.4), then focuses the chat slot, as a left click on the row does |

The menu is drawn as a rounded box (the Box rule, contract §6.1.1 "Boxes", by `tui/src/menu.rs` using box_widget), at the click, clamped inside the pane less one blank column on each side, as the tree's menu is (the vertical clamp unchanged; the click that picks or closes uses the same rect); ↑↓ and Enter pick, Esc closes; a click picks an item; a click outside closes it. The generic menu drawing is shared between the tree's right-click menu and the Deploy column's menu (`tui/src/menu.rs`, shared with `tui/src/tree/menu.rs`, hs-d9: reused, not copied). There is no key that opens it.

**The Inbox's right-click menu** (Deploy column). Claim is the first entry on `review-requested` rows only, and runs `inbox claim <id>` (§21.7, review-requested items only); the other entries are the Snooze and Dismiss entries.

**Scrolling the PRs section** (User, hs-n259). The section is as tall as the pane leaves it, and a pull request is drawn whole (two lines) or not at all. A mouse wheel event over the section scrolls it by one pull request (two lines) per notch, down or up; wheel events elsewhere in the column do nothing. The offset counts pull requests (the first one drawn) and is clamped between 0 and the largest offset at which the last pull request still reaches the section's bottom, so scrolling stops there. When pull requests are hidden above, the section's first row is `▲ <n> more` (colour 8, **right-aligned**, §17's "Scroll cues"); when some are hidden below, its last row is `▼ <n> more` (colour 8, right-aligned); `<n>` counts pull requests, not lines. A cue row replaces one row of the list, so a pull request that does not fit beside the cues is hidden and counted. With every pull request visible there is no cue. A left click works on the rows as drawn: the hit areas are those of the pull requests on screen, and a cue row is no hit. The offset lives in the view's memory only: it starts at 0, a restart of the view resets it, a refresh keeps it (clamped when the list shrinks), and nothing is written to disk.

The column has no engine line, no slots, no Escalations, no Events and no footer hint. The Event log is the `logs` view (R36, below).

**Links** (User, pro-d60; hs-d187, hs-d191). The cockpit links a page through the `Link` widget (pro-d60, 2026-10-26: cpd's cockpit links are one reusable ratatui widget, never OSC 8 terminal escape sequences). The Deploy column's PR rows draw the cue `󱆭` (U+F11AD, `link::CUE`) only, a left click on it opening the pull request's URL in the default browser (macOS `open` only, through the `Runner` port); the Plan view's PR box title draws the PR's state `#<number> <state>` (and the Lead's working dot when the Lead works on it) as underlined text, then one space, then the cue, the whole output clickable. A click on any other cell does what it did before. The click rule: a plain left click (any modifiers) on a link's cell calls `link::open_url(&*ports.runner, url)`, which opens the url in the default browser. The widget API: `Link` takes a url and optional text; `Link::new(url)` draws the cue only (one cell); `Link::text(line)` sets the text (drawn with `Modifier::UNDERLINED` added to each span's style, its colours kept) and outputs `<text> 󱆭` (the space unlined, the cue in `palette::clickable_text()`, not underlined); `Link::width()` returns the output's width in cells (1, or text's width + 2). `Links` holds the (Rect, url) of every link drawn on the frame; `Links::draw(link, area, buf)` renders into `area` and records the rect it drew with the url; `Links::at(pos)` returns the url at that cell or None. The widget never holds an escape character; output is ordinary cells. `open_url` refuses with an `Err` a url that does not start with `http://` or `https://` or holds a byte outside 0x21..=0x7e; otherwise it runs `open` with the url as its one argument through the `Runner`, and reports a non-zero exit or a runner failure as an `Err` whose message names `open`, the url and what `open` said on stderr, using the `ErrorCode` of `tree/actions.rs`. The Inbox's OSC 8 labels (the lines about `plan-review`, `pr-ready`, `review-requested`) stay as they are; they move to the widget in a later PR `inbox-link` of plan pr-links (hs-d187, hs-d191).

**The StatusBar** (R36, User decision; moved to tmux's status line by the User, Plan tui-visuals, hs-d149, hs-d179) is **not a view and has no pane**: it is the one row of tmux's status line on the cockpit session (contract §6.1), as **one dark grey (colour 8) `─` line across the full width** at the bottom of the client, drawn by tmux from `status-format[0]` (one status row; hs-d567: the chat slot's last row, the one above the line, is empty by its occupant, so the chat's content never touches the line; contract §6.1 "The blank row" has the rules Q1-Q4, the evidence and the 20-row mock-up). There is no `statusbar` view, no `@cpd_view statusbar` and no process behind it; it takes no keyboard focus (a click on it is not a click on a pane) and draws no border title. **Its left side holds action buttons only; its right side holds read-only information.**
- **Left, in order** (hs-d479): the line starts with one `─`, then three buttons, `󰨝` (U+F0A1D md-view_dashboard_outline, reset layout), `󰨁` (U+F0A01 md-map_legend, legend) and `󰧭` (U+F09ED md-text_box_outline, logs), Material Design (hs-d235), **joined by a single `─`**; the leading `─` and the joins are **outside the click ranges** (a range also owns the cell after it, so the one-`─` gap does what a space did). Each is a **pill** (hs-d266): the left cap `` (U+E0B6), one glyph on a white block with no padding (interactive, hs-d534: `#[fg=colour15]#[reverse]`, `#[noreverse]#[default]`, no `reverse`), the right cap `` (U+E0B4), the caps in `fg=colour15` on the normal background (contract §6.1.1 has the exact format); a button is 3 cells wide, the left group 12 cells. Each is its own `#[range=user|…]` (`reset`, `legend`, `logs`) wrapping the whole pill, caps included, and a click runs `cpd tui action <range> --client <name>` (below). Tooltips are not built (the User dropped them, hs-d179). Mockup: `─󰨝─󰨁─󰧭───…───inbox 8─workers 4/6 · 0 waiting · 6 queued─`.
- **Preview marker** (hs-n264, hs-d479), right after the buttons: while the tmux server option `@cpd_preview` is set (`scripts/preview-tui.sh on` sets it to `<worktree basename> <short sha>`, `off` unsets it), a single `─` join and then a **non-interactive pill** (yellow, hs-d534: colour 3 inverted: text in the terminal's background colour, caps colour 3; it was grey from hs-d530 to hs-d534) with no padding: the left cap ``, `preview: <value>`, the right cap ``, so a preview never passes for the installed cpd; nothing when it is unset. The group is then `─󰨝─󰨁─󰧭─preview: <value>───…`, and the fill is shorter by the pill's width (`#{n:@cpd_preview}` + 12).
- **The fill** (User, hs-d479): a `─` (U+2500) in **dark grey, colour 8** (no `reverse`) fills the **free width** between the left group and the right group, across the whole row, so the row has no blank gap; the line ends with one closing `─`. Its width is `#{client_width}` minus the left group (12 cells, plus the preview's) minus the right group (`#{w:@cpd_workers}` or `#{w:@cpd_workers_short}`, the form drawn, plus 1 for the closing `─`).
- **Right**, in order: the workers pill, a single `─` join, the version pill, then the closing `─`, **each item its own pill** with no padding (hs-d479; the joins and the closing `─` are outside the click ranges). There is no Inbox pill (hs-d541, the User's decision: the Inbox is the tree's section only). The workers and version pills use the tmux `pill!` shape: `#[fg=colour8]U+E0B6#[reverse]<text>#[noreverse]U+E0B4#[default]`, where the text is inverted (text in the terminal's background colour) and the caps are colour 8 (non-interactive; hs-d561 supersedes hs-d534's 7 for the workers and version pills, contract §6.1.1); the pill has one text colour: the text carries none. The workers text (User, hs-d198) is `workers N/M · W waiting · Q queued`, where `N` is the number of Attempts not `ended`, `M` is config `concurrency`, `W` is the number of Ready Jobs (a `queued` Job whose Dependencies are all `merged` and whose PR has started, §3.2) and `Q` is the number of `queued` Jobs, waiting ones included. The whole text is plain, inverted from the pill (text in the terminal's background colour; no green, yellow or dim, and no colour inside the pill: hs-d534 rule (3)). **The form is chosen per client**: the long form when the fill would still be at least 1 cell with it (`#{client_width}` at least the left group's 12 cells (plus the preview's) + `#{w:@cpd_workers}` + 1 for the closing `─` + 1 for the fill), else the short form `N/M·W·Q`, same plain text; the status line draws the pill of the form it chooses per client with `#{?#{e|>=:#{client_width},<12 + preview + #{w:@cpd_workers} + 2>},#{@cpd_workers},#{@cpd_workers_short}}`; the fill's width uses `#{w:@cpd_workers}` or `#{w:@cpd_workers_short}` the same way. Both texts are fed by the engine (§12), not computed by the status line: up to one tick late, and stale while the engine is stopped (the deviation in §17). The old failure note is gone: a failed click reports with `display-message -c <client>` (below).

**`cpd tui action merge <owner>/<repo>#<n> --client <name>`** (internal; hs-d473, hs-d495): what the `merge` popup's confirm starts, with `run-shell -b`, after the popup has closed. It runs `cpd pr merge <owner>/<repo>#<n>` (§9.10), then `cpd pr poll`, in the background. The merge marker (Lead's design, User may correct it; contract §1.4):
- **File:** `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json`, `{"pid": <the action's pid>, "at": <RFC 3339 time>, "merged": <true when the merge succeeded, absent otherwise>}`, written atomically (temporary file, then rename).
- **Writer and remover:** only this action (**B4**). It writes the marker first (the initial `{"pid", "at"}`), before `cpd pr merge`; on success rewrites it atomically with `"merged": true` after the merge and then runs `cpd pr poll`; then removes it when the pull request's `pull_requests` row is not `open` (merged, closed, or no row); removes it before opening the failure dialog on failure; and removes it on any error path, so no exit leaves it behind. **B5**: a marker with `merged: true` whose pull request's row is not open, and a marker without it whose pid is not alive, are ignored; a second `cpd tui action merge` for a pull request that is merging (`merged` field true or pid alive) does nothing and logs `merge already running`.
- **Readers:** the views, on each snapshot (above); a marker whose pid is not alive is stale and ignored.

**`cpd tui action <reset|legend|logs|host|incident> --client <name>`** (internal) is what a click on a StatusBar button runs (the `MouseDown1Status` binding, contract §6.1); **`cpd tui action merge <owner>/<repo>#<n> --client <name>`** is what the `merge` popup's confirm starts (hs-d473, below). Not a view, it owns no terminal; it runs the action **on the client named** (`#{client_name}` of the clicking client), so a popup opens on that client only, whatever other clients are attached.
- `reset` runs reset-layout (below); `legend` opens the legend popup, `logs` the logs popup (each `display-popup -c <name> …` sized to that client), `host` opens the host Manager (as the host action, §9.12, the same park/swap as the incident action below), and `incident` opens the incident's Lead (the same park/swap as the host action, no message sent; a failure is reported on the client).
- **It prints nothing, on stdout or stderr**: tmux's `run-shell -b` puts the active pane into view mode when its command fails (research `statusbar-no-row.md` 2c), and shows any output of it in view mode (tmux(1), `run-shell`). On a failure (an unknown action, a client that has gone, a tmux error) it shows the message with `display-message -c <name> '<message>'` and exits 0; the binding also appends `2>/dev/null`.

**`legend`** is what every colour and icon of the cockpit means, from the palette (contract §6.1.1, hs-d534: each swatch and sample is drawn by the same palette call as the element it shows, so the legend shows the colours the views show; its title is a read-only title pill, its section titles and words colour 7, its column header and scroll position colour 8). It opens in a tmux popup (`display-popup -E -T ' legend ' -w <w> -h <h>`, sized to the legend and at most the client's size) from the tree's `?` key or the StatusBar's legend button (`cpd tui action legend`), because the columns are narrower than the legend; its title is drawn in the pill section-title style, the plain form (§17, the dialog kit's, hs-d297, hs-d475, hs-d478): ` ──Legend: …───`; when it is taller than its popup, ↑↓, j/k, PgUp/PgDn and the **mouse wheel** scroll it (mouse capture on; the kit's scrolling); any other key closes it. It has no `@cpd_view` pane of its own. The Job states it lists include **landing** (the sample is the Job pill's, the base yellow 3, drawn by the same palette call; the counter beside it is the bright 11; a Job is landing from its first check to the end of its merge, contract §6.1.1). Its section titles are `Jobs: Plan Pills And Context Counters`, `Session Dots`, `Inbox` and `Pulse` (Title Case, hs-d475; the popup's own tmux title ` legend ` is tmux's and unchanged); the Job statuses it lists are those the chart draws, so `cancelled` is not among them, The Job statuses it lists are those the chart draws, so `cancelled` is not among them, and `running` is listed with the spinner frames `` (the 28 frames of §17 section 17). `Session Dots` lists each glyph (`` U+E3E3, `` U+E3E3, U+E3C8..U+E3E2 in each of the five phase colours (grounding grey 15, plan magenta 13, dispatch cyan 14, deploy yellow 11, done green 10) labelled with the phase (hs-d622, hs-d624), then `` U+E3E3, U+E3C8..U+E3E2 in red 9 (the Lead is working on a Plan with a failed Job (one frame every 50 ms), hs-d581), `󱡓` U+F1853, `` U+E3D5, `` U+E3D5) and each `` colour (red, yellow, magenta, cyan, white) with its meaning, as §17's dot rule gives them (the same colours mark the Inbox's items; the Inbox's items draw their own tier glyphs, below; the legend says so in one line), then the precedence line `red > yellow > magenta > cyan > white >  > 󱡓 >  > `, drawn with the real glyphs and colours. `Inbox` (hs-d483; between `Session Dots` and `Pulse`) lists the six tiers, one line each, in the order red, yellow, magenta, blue, cyan, white: the tier's glyph in its colour, then its meaning: `\u{F0362}` (U+F0362, red) `a blocker only you can clear`, `\u{F069A}` (U+F069A, yellow) `pull-request work`, `\u{F173A}` (U+F173A, magenta) `a decision before the work runs` (hs-d546, hs-d547), `\u{F0368}` (U+F0368, blue, pro-d58) `external support: a review asked of you`, `\u{F06F1}` (U+F06F1, cyan) `a cpd notice: curation, nothing blocks`, `\u{F0366}` (U+F0366, white) `an unread reply`; the Inbox rows draw no state dot. Its lines say what needs the User and what only waits for an agent (hs-d303): `red: a dialog in dispatch or deploy, a stuck Job, an engine error, loop guard, your own failing or changed pull request` (an Escalation is no Inbox item, so it is never red; the phase is `deploy`, not `pr`; a pull request a cpd Plan owns is not red, hs-d302); `yellow: ready to merge`; `magenta: a Plan review decision, a dialog question while planning` (hs-d570 changed meaning); `blue: review requested` (pro-d58); `cyan: a cpd notice: curation (hs-d570, pro-d16)`; `waiting: an agent waits for another agent (a Worker for its Lead), not for you` (the `waiting` dot, between `working` and `idle`); the `white` line is unchanged. The `Meanings → colour` table lists `waiting on an agent` in `Meaning::Waiting`'s colour, cyan (`Color::Cyan`: not blue, which means focus only, §17 "Blue"; neither the red of a failure nor the yellow of attention; the User's decision of 2026-10-20, which resolves hs-d303 against hs-d352, and cyan is agent activity in hs-d534). Its Jobs list shows one failed pill row: red with the static U+F0159, and the red loader sample (the same pill with the moon frames, 'its Lead is working on it', one frame every 50 ms, hs-d586); no stuck row, as E8 opens no `job-stuck` item (hs-d318). Its counters row lists the four roles in the tree's order `planned⋅active⋅failed⋅done`, with a sample of one of each (planned 1 magenta 13, active 1 cyan 14, failed 1 red 9, done 1 green 10); the tree draws only nonzero roles, and a failed counter is always red (hs-d580, hs-d636). Its PR states list is `open`, `merged`, `closed` (waiting: `its Lead handles it`), `closed` (red: `its Lead asks you`) `updating` (cyan, both: `its update Job is fixing the conflict`, `its update Job is queued`; the rule of §17 PR boxes, hs-d532) and `conflict` (red: the one of a pull request no Plan owns; an owned one with no update Job is drawn cyan, as the plan view's box titles do). Its sample pill has the same form, `` + `<icon> key│S1` + ``, drawn as the Job pill is (hs-d534, pro-d63: the status colour's base colour, text in the terminal's background colour (inverted), caps the same; one sample per status); the legend's titles are read-only title pills (colour 7 inverted: text in the terminal's background colour). Pulse section (hs-d327): `▂▄▆` = `activity per 5 min, first colour wins` (a slot takes the first colour below); then one `▄` per kind in the precedence, red `an error, or a Job failed`, magenta `a Plan or ask Event`, cyan `a PR opened`, cyan `an escalation`, green `a Job or a PR merged`, cyan `any other activity` (the colours are the bars', §13.2 Pulse); a `▁` in colour 8 = `no activity`; `quiet 41m` (colour 8) = queued or running work with no activity for 30 min. Three entries a row (the popup is at most 80 % of the client tall).

**`merge`** is the merge confirmation of the Deploy column's merge icon (hs-d273); it opens in a popup, and has no `@cpd_view` pane of its own. Its content and keys are the merge icon's paragraph above. On the confirm it starts `cpd tui action merge <owner>/<repo>#<n> --client <name>` with tmux `run-shell -b` and exits, so its popup closes (hs-d473); it runs no `cpd` command of its own. **The `merge` action** (hs-d473, hs-d312) is the background run, in its own process, owned by the tmux server, with none of the popup's. It prints nothing, on stdout or stderr, and exits 0 whatever happens, as every `cpd tui action` does (`run-shell -b` puts the active pane into view mode on a failure or any output, above):
- It runs `cpd pr merge <owner>/<repo>#<n>` (§9.10), which records the merge in the store immediately (hs-d526), then `cpd pr poll` (§9.2) to refresh (the poll's result window is unchanged: no second merge by the poll itself, just refresh of the stored merge). A failed merge runs no poll. **Nothing shows on success**: the Deploy column shows the merge after the poll, which is already stored.
- **A failed merge** is the cockpit's one failure rule (§17 "No notes (a)"): a `text` dialog of the dialog kit (§13.2), the same one every failure opens, never a second dialog, opens on that client (`<name>`) with `merge <owner>/<repo>#<n>: <code>: <message>`, the `<code>` and `<message>` those of the failed command's error (for example `merge_not_clean`), and a line with the same text goes to the merge's log, `$CPD_HOME/run/tui/merge.log` (hs-d364: the log of the `merge` view that started the action).
- **A poll that fails after a merge** is **a log line only**, in the same file, `pr poll after merge <owner>/<repo>#<n>: <code>: <message>`, and no dialog: the pull request did merge, and the next periodic poll refreshes the column (hs-d473's first reading, a dialog for it, is replaced by §17 "No notes").
- **When the dialog cannot open** (the client has gone, tmux fails), the error does not vanish: the log line is written **first**, before the dialog is tried, so it is always there; then the dialog is tried; if that fails, the action shows the message with `display-message -c <name> '<message>'`, as every `tui action` does for its own failures, and if that fails too the log line is what is left. Why the log first: it needs no client and no tmux, so it is the one place that cannot fail for the reasons the other two can.
- **A second confirm while a merge runs** (the icon shows while the stored `merge_state` is still `'clean'`, until the poll after the merge): the second action runs `cpd pr merge`, which reads GitHub fresh (§9.10 step 1). Either the first merge is done, and it is refused `merge_not_clean` (`details.state` `MERGED`), or `gh` is still merging and GitHub accepts one of the two and refuses the other. One pull request is merged once; the refused run's error opens the failure dialog. Acceptable, not harmful: the User asked twice, and the dialog says why the second did nothing.

**`logs`** (R36) is the **Event log**: every Event (§5), one line each: its local time of day (`HH:MM:SS`, colour 8, bare with no zone per §1.3 pro-d42), then its `text`, or its kind when the `text` is empty, an `error` Event in the failed colour (red 1; the dialog is read-only), except one **addressed to a Lead**, in `Meaning::Waiting`'s colour (cyan 6): the Lead handles it (audit Q5, hs-d303). An `error` is addressed to a Lead when it names a Plan (`plan` set) and its `data.source` is not the engine's own `watch`, or its `data.code` is `pr_start_failed` or `push_failed` (the engine reports those for a Lead, with source `watch`); the engine's own step errors (`source` `watch`, contract §5.2) and any `error` that names no Plan stay red. It opens in a tmux popup (`display-popup -E -T ' logs '`, at most 80 % of the client's width and height) from the StatusBar's `󰧭` button (`cpd tui action logs`, on the clicking client; no Inbox click opens it: `engine-down` starts the engine and the engine's errors go to the Manager, hs-d314), and has no `@cpd_view` pane of its own. With `--event <id>` (an Event id of §5) the view opens **at that Event**: its line is selected (the blue block, colour 4 inverted: text in the terminal's background colour, contract §6.1.1) and scrolled into view, and it does not follow until the User scrolls to the bottom; an id outside the latest 500 Events is read on its own too. Without it, it opens at the bottom. The newest Event is last; the view follows live while it shows the bottom, and stops following when the User scrolls up (it follows again at the bottom). Its title is drawn in the section-title style, the plain form (§17; hs-d297, hs-d475, hs-d478): ` ──Logs───…`. ↑↓, j/k, PgUp/PgDn and the **mouse wheel** scroll, and End or `G` goes to the bottom, where it follows again (mouse capture on; the kit's scrolling, hs-d297), with the live follow and `--event` behaviour above unchanged; `q` or Esc closes it. **Margin** (the Box rule, contract §6.1.1 "Boxes"; the popup's contents are a widget in the space inside tmux's border): every row has 1 blank column on each side, so the text starts at column 1 of the popup's inside and ends at column `W - 2`.

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
2. **Cockpit:** what `cpd tui` does (§13.1): build it when missing, respawn a dead pane of its layout, remove an older cpd's StatusBar pane or `worker` pane (pro-d63), and set the server options and the status line. One implementation, shared with `cpd tui`: bare `cpd` calls it, never a copy.
3. **Views on an old binary:** when the panes of the views run an older cpd than the installed one, `cpd tui --respawn` (§13.1).
4. **tmux version:** when the cockpit's tmux server runs an older tmux than the installed client (`tmux -L <socket> display -p '#{version}'` against `tmux -V`; hs-d166), restart the server onto the installed tmux **when it is safe**, reopen every Lead and Manager Session that had a pane (same pi session file, same place: the chat slot, its parked window, a Context's window), then attach. When it is not safe, **hold, drain, restart** (User, hs-d294):
   - **Safe** means: no Attempt `starting`, `running` or `merging`, and no Lead or Manager Session mid-turn (state `working`; a `blocked` Session has a dialog open, so it is also not safe). Restarting stops every pi in the server; Leads and Managers resume from their session files.
   - **Safe now:** bare `cpd` restarts the server itself, in its own process (it runs in a terminal outside the server): it records every Lead and Manager Session with a live pane and its place (`chat`, or parked: its `park-<label>` window, or a Manager's `ctx-<context>` window); `kill-server`; `cpd tui` (the cockpit on the installed tmux, which also runs `watch --ensure`; the last chat Lead is back in the chat slot); then each Session in the same place: the one of the chat slot as `session open` does, the others in a new window running `cpd session run <id>`, which resumes the Session's pi file, each with `session-moved`. It then attaches with the installed tmux and says in one line how many Sessions it reopened.
   - **Not safe:** it **marks a restart pending** (the file `$CPD_HOME/run/tmux-restart.pending`, JSON `{"at", "server", "installed", "failed"}`; no store change), attaches at once with the **server's own tmux binary** (its path from the server pid: on Linux `/proc/<pid>/exe`, on macOS read with `libproc`), because a newer client cannot attach to an older server (`open terminal failed: not a terminal`, hs-n224), and prints one line saying so and that the restart happens once nothing is in the way. `cpd tui` does the same marking when it finds the server older (it never restarts the server itself, since it may run inside it), and `tui --attach` attaches with the server's own binary then as well.
   - **While a restart is pending, the engine starts no new Attempt** (E3's hold, contract §3.7); running Attempts finish normally. It is a planned hold, not an error: `cpd doctor` shows it as the `tmux_restart` check ("tmux restart pending", §14.1). Once none runs (no Attempt `starting`, `running` or `merging`) and no Lead or Manager Session is `working` or `blocked`, the engine starts the restart **detached** (`cpd watch --restart-tmux`, internal; `setsid`, its log `run/tmux-restart.log`), because the restart kills the server the engine itself runs in. The detached process takes `locks/tmux-restart.lock`, checks again that it is safe, does what "safe now" does, clears the pending file and logs the result. Then the engine, started again by `cpd tui`, schedules as usual. The User reconnects with bare `cpd`.
   - **A failed restart** keeps the pending file with `failed` (`at`, `code`, `reason`) and the engine that runs again appends one `error` Event (`source: "watch"`, `code` of the failure, `details: {at, server, installed, log}`); a failed restart holds nothing. The next bare `cpd` finding the server still older tries again (a failed file is marked afresh); finding it as new as the installed tmux, it says the failure in one line and removes the file. Never from inside the cockpit's own server.

Rerunning is safe: a restart already pending keeps its time, and a server that is not older is left alone. Internal command: `cpd watch --restart-tmux` (not an interface of the User or an agent; it conflicts with `--ensure`).

`scripts/restart-tmux.sh` (PR `tmux`) is gone: its checks (the refusal while an Attempt runs, the attach hint with the server's binary) are the binary's own.

**Assumptions, confirmed by the User in the Plan review:**

- **A1** Without a terminal on stdin and stdout, bare `cpd` keeps today's behaviour: the JSON usage error `usage`, exit 2. It does no follow-up.
- **A2** Inside the cockpit's own server (`$TMUX` names its socket), bare `cpd` does not nest an attach: it does the follow-ups that are safe from inside, never the tmux restart nor the marking of one (it would kill the pane it runs in), and prints one line saying how to reach the cockpit.
- **A3** Each follow-up's failure is printed in one line on stderr, and the attach still happens when the cockpit exists. When the cockpit does not exist and could not be built, bare `cpd` prints the failure value and exits with its code.
- **A4** Nothing new in the store: no table, column or Event kind.

## 14. `doctor`, `config`, `tiers`, `models`

### 14.1 `doctor`

```
cpd doctor
```

Checks the machine (cpd 1 §11 `doctor`, kept in shape). Each check has `name`, `ok`, `severity` (`error` or `warn`), and when relevant `found`, `expected` and `message`. Any failing `error` check makes the result `doctor_failed` (exit 1) with every check in `details.checks`; `warn` checks never fail it.

**Minimum versions.** Each minimum is the version verified on this machine (the homeserver, 2026-10-14), because the contract's tmux, git, gh and pi behaviour was verified on exactly these (contract.md, evidence labels). Older versions may work; that is unverified, so doctor refuses them.

| Tool | Command run | Output found on 2026-10-14 | Minimum | Parsed as | What cpd needs from it |
|---|---|---|---|---|---|
| git | `git --version` | `git version 2.43.0` | `2.43.0` | the first `x.y.z` | `worktree add`, `update-ref` with an old value, `merge-base --is-ancestor`, `merge-tree --write-tree --name-only` (git 2.38 or later, docs), `read-tree -m -u` (§7) |
| tmux | `tmux -V` | `tmux 3.4` | `3.6` | `x.y`, ignoring a letter suffix (`3.4a`); `next-…` builds pass | `new-window`/`split-window -e`, argv commands after `--`, `swap-pane`, `pane-border-lines`, `extended-keys`, `#{pane_dead_status}` (§6); **3.6 because** older tmux can lose a pane's `SIGCHLD`, so `pane-died` never respawns a cockpit view (contract §6.1; fixed in tmux 3.6, tmux/tmux#4559) |
| pi | `pi --version` | `1.2.0` | `>= 1.2.0 < 2.0.0` | `x.y.z` | `--fork`, `--session-id`, `-n`, `--exclude-tools`, `--no-skills`, `--approve`; the catalog of §9 (cpd 1 §11: pi updates itself, so a range) |
| gh | `gh --version` | `gh version 2.45.0 (2025-07-18 Ubuntu 2.45.0-1ubuntu0.3)` | `2.45.0` | the first `x.y.z` | `pr create`, `pr list --json`, `pr view --json …statusCheckRollup…`, `api` (§7.6) |
| pi's Node | `<pi node> --version`, the binary §9 chooses | `v22.23.2` (`/usr/local/bin/node`; this machine has no `pi-node` directory) | none: it must run | | runs the catalog module of §9 |

**Deadline.** Each probe `doctor` runs (the five commands of the table, `<pi node> --version`, the catalog module of §9 and `gh auth status`) has a deadline of 10 s. A probe that runs past it is a failed check whose `message` names the tool, its argv and the deadline (contract §1.2 `tool_timeout`); `doctor` goes on with the next check and still reports every check.

Checks, in order:

| Name | Severity | Passes when |
|---|---|---|
| `config` | error | `config.toml` exists and is valid (§8) |
| `store` | error | the store verdict (§1.2 `--version`, contract §4.5) says the database opens, and it migrates and passes the schema check (§4.3); otherwise `found` is the verdict's refusal (`code`, `message`). Unknown additive migrations (`store.unknown`) are not a failure and have no check of their own: `--version` reports them (hs-d178 D1) |
| `tamper` | error | the tamper report (§4.3) is empty; `found` lists it |
| `git`, `tmux`, `pi`, `gh` | error | found on `PATH` at or above the minimum above |
| `gh_auth` | warn | `gh auth status` exits 0 (needed only by `pr` commands; verified on 2026-10-14: logged in through `GH_TOKEN`) |
| `pi_node` | error | pi's Node runs |
| `tiers` | error | each model in the policy's `[model]` table is in pi's catalog (contract §8.3, §9), and `lead_tier`, `manager_tier`, `context_distill_tier` and `open_tier` are valid Tiers that obey the Tier rules (§9, hs-d562, pro-d53, hs-d554 and hs-d506: `context_distill_tier` `GX` only (hs-d562: the auxiliaries' Tier); `open_tier` a Tier a Worker may have (`H0`–`H4` or `S0`–`S3`); effort 0–4) |
| `holographic` | error | when `context_engine_enabled = true`, resolve the bridge script and the Python interpreter from the config (or their defaults, §8.2), run `{"cmd":"ping"}` under `context_engine_timeout_ms`, and report the engine version; when `context_engine_enabled = false`, passes with `"disabled": true` (hs-d677); no index read or build |
| `pi_adapter` | error | the `pi_adapter` directory has a `package.json` |
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

Without `--migrate`, prints the effective configuration: defaults merged with `config.toml` (§8), each value with its source. Refusals: `config_missing`, `config_invalid` (exit 1, `details.key` = the key path), among them the Tier rules of hs-d562, pro-d53, hs-d554 and hs-d506 (contract §8.2, §9 "Tier rules"): `lead_tier` or `manager_tier` with effort 5 or letter G, `context_distill_tier` must be GX (hs-d562), `open_tier` outside the Worker range (§9). Defaults: `lead_tier` `O2`, `manager_tier` `O4`, `context_distill_tier` `GX` (hs-d562: the Tier every distill run uses, one step only, no ladder, hs-d560), `context_curation_min_records` `25` (hs-d592: a Context is curable when it holds at least this many records; an integer >= 1), `loop_guard_turns` `20` (hs-d552, hs-d571: an integer >= 1; the window size for the loop guard's turn counter). It reads only the config files and never opens the store, so it never waits on the setup lock (hs-n278); `cpd tiers` and every command that opens the store wait out a migration in progress (contract §4.2). No effects.

With `--migrate`, reads `$CPD_HOME/config.toml`, applies the ordered, additive migrations (contract §8.1 "Config migrations", hs-d722; the two first migrations are `distill-tier-gx` and `drop-test-concurrency`, hs-d722 and hs-d679), validates the result (the same checks as plain `config` above), and writes the file only when it changed and only when the result is valid. The file is untouched on any refusal. Refusals: `config_missing` (exit 1) when the file is missing; `config_invalid` (exit 1, `details.key`) when the migrated file is still invalid after the migrations; any other code the migrations may return. It prints `{"ok":true,"path":"<path>","migrated":["<name>",...]}` with an empty array when nothing changed. Used only by `cpd install` before replacing the binary (contract §3.7 "The config migration and validation", commands.md §14.4 step 3).

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

It opens the store (the catalog cache), so it waits out a migration in progress (hs-n278; contract §4.2): one line on stderr, the one JSON value at the end, and `lock_timeout` only past `MIGRATION_WAIT_CEILING`.

Effects: when `install/current-version` differs from the cached `pi_version`, one transaction inserting `tier_catalog` rows (a cache: no Event). No tmux, no git.

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
3. **The verdict of the built binary.** `<target>/release/cpd --version` must succeed, carry a `store` field, and not say `store.opens` false; else refused with the refusal's message and the fix of its code. Nothing migrates.
4. **The config migration and validation.** `<target>/release/cpd config --migrate` reads `$CPD_HOME/config.toml`, applies the migrations (contract §8.1 "Config migrations", hs-d722; the two first are `distill-tier-gx` and `drop-test-concurrency`, hs-d722 and hs-d679), validates the result, and writes the file only when it changed and only when the result is valid. The file is untouched on any refusal. Prints `{"ok":true,"path":"<path>","migrated":["<name>",...]}` and each applied migration as a stderr line `cpd install: config.toml migrated: <name>`. Refusals: `config_missing` (the file is absent), `config_invalid` (the migrated file is still invalid, `details.key` names it). The migration happens before the binary is replaced, so that if it fails, the config is not mutated.
5. **The binary.** Copied to `~/.local/bin/.cpd.tmp.<pid>`, `chmod 755`, `rename`d over `~/.local/bin/cpd` (atomic, also while it runs).
6. **The adapter swap.** `pi/` minus `test/` is built as `pi.tmp.<pid>`, renamed to `pi.<id>`, linked as `pi.link.<pid>` (a relative symlink) and `rename`d over `pi` in one `rename(2)`; a real `pi/` directory is first renamed to `pi.legacy` (the one non-atomic moment, restored if the swap fails); `pi/package.json` must exist; then `pi.<id>` and `pi.legacy` that are neither current nor displaced and are older than 60 minutes, and `pi.tmp.<pid>` and `pi.link.<pid>` of dead pids, are removed (a failure only warns). Prints `installed <bin> and <link>` on stderr.
7. **The follow-ups, on the new binary:** `cpd config`; `cpd watch --check` (§12); `cpd tui --respawn` (§13.1) when the cockpit has its `main` window; when the work session has its `watch` window, `cpd watch --mark-restart` (§12), `tmux kill-window -t =<work>:watch`, the wait for `locks/watch.lock` to be free (0.25 s steps, `CPD_INSTALL_LOCK_WAIT_SECS`, default 60), `cpd watch --ensure`, and the check that the lock is held. A failure of `config` or `watch --check` leaves the engine and the views alone and skips the rest; any other failure prints and goes on. Each failure is one line `cpd install follow-up failed: <text>` on stderr (texts: contract §3.7).
8. **The tag.** `scripts/tag-version.sh` (docs/versioning.md): a failure warns with `tag-version:` and does not change the exit.

| Precondition | Code | Exit |
|---|---|---|
| `--root` is given | `usage` | 2 |

Every refusal (steps 1 to 4, nothing replaced), failure (steps 5 and 6) and failed follow-up (step 7) is a message on stderr as above and **exit 1**; the exit is 1 at the end when any follow-up line was printed.

Output: the install's progress and every message above go to stderr as human text, as the script's did (a Lead's commit hooks and dotfiles read the exit code and the text, not JSON); the one JSON value on stdout is `{"ok": true, "binary": "<path>", "adapter": "<path>", "version": "<cpd --version of the built binary>"}` on success, and the error value (§1.2) with exit 1 otherwise (the messages are the script's own; every refusal, failure and failed follow-up is `install_failed`, exit 1). Effects: writes only `~/.local/bin`, `~/.local/share/cpd` and what the follow-ups write (`run/install-restart.json`, the engine's own files); no row, no Event. Idempotent: yes, safe to rerun, also by two installs at once (contract §3.7 "The adapter swap"). The command-tree test `cli/tests/tooling/interface.rs` lists it (callers none, cockpit no, internal no); `docs/interface.md` is regenerated from the tree.

### 14.5 `models`

```
cpd models refresh
cpd models show [--provider <provider>]
```

The model lists of the providers pi can use (contract §3.7 E15, Context decisions pro-d13, pro-d17, pro-d20). **cpd has no provider list and names no key**: the providers come from pi, through the helper `pi/lib/models-list.ts`, and `--provider` only names one of the files already cached. Callers: Manager, Lead, or the User at a shell (§2). Neither command opens the store, writes a row or appends an Event.

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

**`models show`** prints the cached lists and runs nothing (no helper, no network, no lock). `--provider <provider>` prints only that provider's file. A provider with no file, and a home with no file at all, is an empty `providers` array, not an error. A file it cannot parse is skipped, and named in `warnings` (`{provider, file, code: "models_unparseable"}`); the command still succeeds.

| Precondition | Code | Exit |
|---|---|---|
| `--provider` is a file-name-safe token (`^[A-Za-z0-9._-]+$`) | `usage` | 2 |

```json
{"ok": true, "providers": [
  {"provider": "bifrost", "polled_at": "2026-11-02T09:00:00Z", "age_seconds": 3600, "pi_version": "1.3.0", "auth": "custom", "count": 41, "ids": ["…"], "changes": {"at": "2026-11-02T09:00:00Z", "appeared": ["acme/new-model"], "went_away": []}}],
 "warnings": []}
```

`ids` is the file's sorted list, `count` its length, `auth` the file's `source.auth`, `changes` the file's last `changes` (`null` when it has none), `age_seconds` the time since `polled_at`, at the call. The providers are in name order. Idempotent: yes, read-only.

## 15. The pi adapter's protocol

The pi adapter is the TypeScript package `pi/` (decisions: "the pi adapter stays TypeScript"), loaded in every pi session cpd starts (§6.5). It is the only pi-specific code: it turns Events into messages (§5.3), reports agent states (§3.6) and runs the Plan review in the Lead's chat, with plannotator for the full render on request (§15.5). It calls only `cpd` commands; it never reads the database and never types into a pane (cpd 1 §15, kept).

### 15.1 Package and activation

- **Package:** `pi/package.json` with `"keywords": ["pi-package"]` and `"pi": {"extensions": ["./extension.ts"], "skills": ["./skills"]}`; host packages are `peerDependencies: "*"` (cpd 1 §15, kept). The skill `cpd-lead` is the cpd 2 skill.
- **Location:** `pi/`, installed to `~/.local/share/cpd/pi` by `cpd install` (§14.4). (History: before the switch-over it lived beside cpd 1's `pi/`, in a second directory.)
- **Activation:** at pi's `session_start`, only when `ctx.mode` is `tui` and `CPD_SESSION` is set. Change from cpd 1: cpd 1 activated in any herdr or tmux pane (`HERDR_ENV`, `TMUX_PANE`); now cpd marks its own panes with `CPD_SESSION` (§6.5), so a pi the User starts by hand is left alone.
- **The empty last row** (User, hs-d567; contract §6.1 "The blank row", Q2-Q3): in its `session_start` handler (`pi/lib/adapter.ts`), the adapter calls `ctx.ui.setStatus("cpd-pad", "")` (`ctx.mode` `tui`; it knows nothing of the chat slot, so a parked pane has the extra empty row too). pi's footer draws one more line, empty, when its extension statuses are not empty, so pi's last row is empty; a `/reload` runs `session_start` again and sets it again, and a resize needs nothing (verified in real pi, contract §6.1 evidence). The key `cpd-pad` is ours; another extension's status text would share the line.
- **Identity:** `CPD_SESSION` must equal `ctx.sessionManager.getSessionId()`. If not, the adapter delivers one error message (below) and stays inactive: it would otherwise report another Session's state.
- **Calls:** one-shot commands through `pi.exec("cpd", args)`, whose stdout is parsed as the JSON envelope at the boundary (a non-JSON stdout, or `ok: false`, is an error with the code). Each call passes a timeout of 30 s and the abort signal of the adapter's run (aborted at `session_shutdown`); a call that times out or is aborted is reported like a failed call (the error delivery below, code `tool_timeout` for a timeout). The follower is a child process (`node:child_process` `spawn`, argv, stdin `/dev/null`) read line by line.
- **Errors** (hs-d552; the adapter-only codes `protocol_mismatch` and `mcp_register_failed` are delivered by this rule, contract §1.2) for a **Manager** or **Worker**: delivered to the Session as `pi.sendMessage({customType: "cpd", content: "cpd adapter: <what>: <code>: <message>", display: true, details: {kind: "error", code, …}}, {triggerTurn: false, deliverAs: "followUp"})`: no turn started, shown as a follow-up. An error identical to the last one shown in this Session (same `what` and `code`) is not shown again until a different error has been shown; one memory per Session, cleared at reload. For a **Lead** (hs-d573 cause A): one line appended to the adapter log `$CPD_HOME/run/adapter/<session>.log` (15.6 'Log'): `<time> error: <what>: <code>: <message>`, and reported to the Manager with `cpd session report` (§10.18), once per (`what`, `code`) by memory (`host.report`): the same error again is neither logged again nor reported again until a different one has been. If the report itself fails, that is one log line (`<time> report failed: <code>: <message>`), never delivered and never reported again (no recursion); the next different error tries again. The adapter never throws into pi.

### 15.2 Start-up and the cursor

At `session_start`, in this order:
0. **Protocol check** (hs-a4): `cpd --version`, whose `protocol` (§1.2) is compared with the number the adapter was built for (5 today). Equal: nothing is said. Different: one message to the Session, `cpd adapter: protocol mismatch: protocol_mismatch: cpd speaks protocol N, this adapter M; reload pi (/reload)`, with `details: {kind: "error", code: "protocol_mismatch", cpd: N, adapter: M}`, delivered like the other errors (§15.1), once per installed cpd binary (the identity of the file `cpd` resolves to), not once per follower restart. The adapter keeps following Events either way; it never types into a pane. A `--version` that fails or lacks `protocol` is an ordinary error with its code, once per error code.
1. `cpd session show <id>`: role, Plan, Context, `plan_lead`, `registered_event`, name. A failure is delivered as an error message once per error code, and the call is retried with the follower's back-off (1 s, doubling to at most 30 s) until it succeeds; the adapter does nothing else until then, and `session_shutdown` ends the retries. A refusal that cannot pass (`usage`, `session_required`, `invalid_session_id`, `session_ambiguous`, `session_not_found`, `plan_not_found`, `context_not_found`, `cursor_ahead`) is delivered once and not retried: the adapter stays inactive.
2. `cpd session state idle --session <id>` (`stopped → idle`, §3.6); then `working` if `ctx.isIdle()` is false.
3. **Name:** if pi's session name differs from the Session's `name`, `pi.setSessionName(name)`: a `session-renamed` missed while pi was down still applies.
4. **Cursor** (§5.3 "No loss, no duplicates"): the greater of `registered_event` and the `details.id` of the last `cpd` custom message in the session's entries whose `details.delivered_to` is this Session's id. The adapter puts `delivered_to` into every message it delivers (amendment 5, §19). Why: a fork's file starts with its source's messages, whose Event ids belong to the source's stream; counting them could skip Events addressed to the fork.
5. **Blocking reasons** (Plan Lead only; a Manager has none): one per `open` Escalation of its Plan (`cpd plan show`), and one for the Plan's **latest** Plan review when it is pending and this session opened it (its `cpd-plan-review` entries of either surface, §15.5; a pending chat entry also reopens its dialog, §15.5 "Recovery"): the latest is the one `cpd plan show` reports (`review.id`), and an undecided older review never opens a reason (hs-d224).
6. **Follow:** spawn `cpd events follow --session <id> --since <cursor>`.

The cursor in memory advances after each line is handled, delivered or not. Only delivered messages persist it, so after a restart the Events handled without a message (`escalation-answered`, `escalation-expired`, `session-renamed`, `plan-review`) are seen again; every handler below is idempotent for that reason.

**The follower ends** (EOF, exit, or a failure line): the adapter restarts it with `--since <in-memory cursor>` after 1 s, then backing off to at most 30 s. A failure line or a non-zero exit is delivered as an error message once per error code, until a line is read again. A failure line whose code is one of the refusals of step 1 is permanent: it is delivered once and the follower is **not** restarted (every other code, for example `store_busy` or `store_too_new`, is retried without end). A well-formed Event (§5.2 fields) of a kind the adapter does not know is skipped silently: no message, and the cursor moves past it (an adapter that does not know a kind cannot need it, so an install that adds a kind does not flood older adapters). A malformed line (not JSON, or §5.2 fields missing or of the wrong type) is delivered as an error message and skipped, and the cursor moves past it, so one bad line cannot stop delivery. Before it spawns the follower again, the adapter repeats the protocol check of step 0; the message is sent only when the installed binary is a different one from the last that mismatched.

**store_too_new when the installed cpd is newer** (hs-d7): at each follower run start, the adapter records the identity of the file `cpd` resolves to (the installed cpd identity, from `installedIdentity()` in pi/lib/cpd.ts). A failure line whose code is `store_too_new` is **not delivered** if the installed cpd identity differs from the identity recorded at this follower's start: the follower was running the old cpd binary when it was started, and a newer cpd is now installed. In that case, the follower is restarted as usual (which runs the new cpd from PATH), and the restart delay resets to 1 s. When the installed cpd identity is the same as the one recorded at the follower's start (no install has happened since), `store_too_new` is delivered as today: once per error code, until a line is read again (the code is retried without end, as per the second sentence).

At `session_shutdown` the follower is killed.

### 15.3 What each Event does

Events arrive in id order; the follower already applied §11's `--session` rule, so a Lead receives its Plan's Events and a Worker or side fork only those addressed to it. Delivery is `pi.sendMessage({customType: "cpd", content: event.text, display: true, details: {...event, delivered_to: <id>}}, {triggerTurn, deliverAs})` (§5.3), where `deliverAs` is `"steer"` for the **urgent kinds** and `"followUp"` for every other message (below).

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
| `mcp-attached` (`session` = itself) | **the registration** (pro-d8, contract §5.3): no message, no turn. The adapter calls `pi.registerMcpServer(data.name, data.server)` (`attachServer`, contract §6.11). An Event for another Session registers nothing. The same Event handled again (a replay after a restart: the cursor counts delivered messages only, §15.2) registers the same name again, which pi replaces, so an attach is best effort across a restart and an agent that finds a server missing runs `cpd mcp attach` again. A registration pi refuses (it throws) is one error (§15.1, delivered as every adapter error is, once per `what` and `code`; code `mcp_register_failed`, `details: {kind: "error", code, name, message}` with pi's message). With no such Event the adapter registers no server. For every role. Adding the kind raises `protocol` | same |
| `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived` | ignored: no message, no turn | ignored |
| `session-titled` | ignored: **no message, no turn** (hs-d257): the pane header is set by `session retitle` itself, through tmux (§10.14), so the adapter has nothing to do; it only **learns the kind** (the list of kinds in `pi/lib/event.ts`, with delivery `none`), because an unknown kind costs an error turn (hs-n122). Adding the kind raises `protocol` (contract §5.2, "The protocol number") | ignored |
| every other kind | ignored | ignored |

**Steered and follow-up** (User, hs-d276; evidence hs-n241). Two kinds are urgent and are delivered with `deliverAs: "steer"`: **`pr-feedback`, whatever its `reason`**, and a Worker's **`escalation`**. pi injects a steering message after the current tool call instead of after the run (pi 1.3.0 `dist/core/agent-session.d.ts` `sendCustomMessage(…, {deliverAs: "steer" | "followUp" | "nextTurn"})`; `pi.sendMessage` takes the same option, `dist/core/extensions/types.d.ts` `ExtensionAPI.sendMessage`). An idle Lead is unchanged: `triggerTurn` starts a turn. **Every other message the adapter delivers is a `followUp`**: the Event kinds above, the adapter's errors (§15.1), `attempt-nudge` and the `cpd-plan-decision` message. The same rule holds for a Manager, if it is ever sent one of the two urgent kinds (today it gets neither, §11). Why: every Lead message used to be a follow-up, and pi's default `followUpMode` `"one-at-a-time"` delivers one follow-up per finished run, oldest first, so a busy Lead got PR #80's conflict notice 19 minutes late and a `job-merged` 56 minutes late (hs-n241).

**Other notices** stay follow-ups, delivered as the User's pi settings say: `followUpMode` (and `steeringMode`, for steered ones) `"all"` delivers every queued message together, pi's default `"one-at-a-time"` one per finished run. cpd does not set them (the User set both to `all` in the global pi settings on 2026-10-19; contract §6.5 "Queue modes are the User's").

**The Manager** (B3-A5) is a Session of role `manager`: it has no Plan, so §15.2 step 5 (blocking reasons) and §15.5 (the Plan review, the tool `cpd_plan_review`, the command `/plan-review`) do not apply to it and nothing is opened or registered for them. Its follower follows only Events addressed to it (§11), so of this table only `session-message` and `session-renamed` reach it: a Plan's Events never wake it. It learns of them through the digest at turn start (§15.7), which only a Manager gets.

This is §5.3's table; the `plan-review` row is the Plan review protocol §5.3 leaves to this file (amendment 6, §19). A blocking reason opened twice or closed when not open changes nothing (cpd 1 `blocked.ts`, kept), which makes replays harmless.

### 15.4 Agent states: `session state`

The adapter keeps three facts and reports the state they give (the model of `tmux-agent-state.ts`, §3.6):

| Fact | Set by |
|---|---|
| `active` | true at pi's `agent_start`; false at `agent_settled` when `ctx.isIdle()` is true |
| its own reasons | §15.3 (`escalation:<id>`, kind **agent**), §15.5 (`plan-review:<review>`, the Plan's latest review only, kind **human**), and L4 of hs-d552, hs-d571 (`loop-guard`, kind **human**, when the loop guard trips), each with a label |
| blocking dialogs | pi's `ui_prompt_start` and `ui_prompt_end` (pi 1.2.0 `ExtensionAPI`, emitted around every blocking extension UI prompt: `select`, `confirm`, `input`, `editor`, `custom`; depth-counted, so nested prompts give one pair) open and close **one** reason of kind **human** labelled `dialog`. **Exceptions:** the review chat's own `select` and `editor` calls (from `pi/lib/review-chat.ts` `runDialog`) open no `dialog` reason; they are represented by `plan-review:<review>` (the Plan's latest review, kind **human**) instead. This is implemented through a counter of the adapter's own prompts: each `runDialog` call increments the counter before opening the dialog and decrements it in a finally block; a `ui_prompt_start` or `ui_prompt_end` that arrives while the counter is above 0 opens no reason. Every other prompt (ask_user_question, any other extension's dialog) still opens `dialog` as today. A **Worker's** dialogs are not reported (hs-d303, audit Q6): a Worker never talks to the User, it asks its Lead with `cpd ask` |

Every reason has a **kind** (hs-d303, audit J2): **human** (the User has to act: `dialog`, `φ: plan review`, `φ: loop guard`) or **agent** (another agent handles it: `φ: Worker question`). Reported state: `blocked` with the label of the most recently opened **human** reason while any is open; else `waiting` with the label of the most recently opened **agent** reason while any is open; else `working` while `active`; else `idle`. So a Worker question opened during a dialog does not replace the dialog's label, and closing the dialog with the question still open reports `waiting`. On every change it runs `cpd session state <state> [--label <label>] --session <id>`. Calls are serialised: at most one in flight, and only the latest wanted state is sent next, so states never arrive out of order. A refused call (`illegal_transition`, for example after the engine marked the Session `stopped`) is reported as an error once and the next change is tried normally. At `session_shutdown`: `session state stopped`, waited for at most 2 s; `session run` reports it as well if pi exits without it (§10.8).

**Reply runs** (hs-d278; contract §2.2 `unread`, §3.6). The adapter also records whether the current run is a **reply run**, and reports it when the run's end moves the Session to `idle`: `cpd session state idle --reply --session <id>`; any other call omits `--reply`. A run is a reply run when it started at pi's `before_agent_start` with `event.messageType` `"user"` (a prompt the User submitted, or `pi.sendUserMessage()`), or with `"custom"` and the adapter's own message for a `session-message` Event whose `data.from` is null (`cpd session send` with no sender: "Message from the User"), or when pi's `input` event with `source: "interactive"` arrived during it (the User typed, also as a steer or follow-up). A run started by an engine Event, or by another Session's message, is not a reply run unless the User also typed during it. The flag is cleared when the run starts and read at the `idle` report; an older adapter sends no `--reply`, so its Sessions are never unread until it reloads (protocol 4, §1.2). by `ask` itself and back to `working` when it returns (§8.1); its adapter is mid-turn meanwhile and reports nothing.

**Names.** On pi's `session_info_changed`, when pi's name differs from the last name the adapter knows, it runs `cpd session rename <name> --session <id>`, so a `/name` typed in pi reaches the TUI. The `session-renamed` it causes comes back and changes nothing.

### 15.5 The Plan review in chat

Decision hs-d502 (User, 2026-10-20; supersedes hs-d468). Replaces cpd 1 §15's `cpd_plan_review` flow, which sent every review to plannotator. A Plan review now shows the User a **short summary** in the Lead's chat and asks for the decision in one **dialog**: `Approve`, `Change` or `Open full plan`. plannotator opens only on `Open full plan`, with the full render (§4.6 `plan-<review>.md`); the plannotator event API is the one cpd 1 uses (t4), parsed at the boundary because it is not a stable contract. `plan decide` by the adapter stays the only approval record (§4.7); nothing else approves.

**What pi gives** (docs, pi 1.3.0; verified in `docs/extensions.md` and `dist/core/extensions/types.d.ts`, and in `dist/modes/interactive/interactive-mode.js` for the behaviour of `select`):
- `ctx.ui.select(title, options, opts?)` returns the chosen option string, or `undefined` when the User presses Esc **or** when `opts.signal` (an `AbortSignal`, `ExtensionUIDialogOptions`) aborts; an already aborted signal returns `undefined` at once. So a caller tells its own abort from Esc by its own controller's `signal.aborted`.
- `ctx.ui.editor(title, prefill?)` returns the text, or `undefined` when cancelled. **It has no `opts`, so no signal**: an open editor cannot be closed by the adapter.
- `ctx.ui.notify(message, type?)` shows a notice in pi's chat area and adds nothing to the model's context. `pi.sendMessage(message, options)` adds a message to the session, drawn by the renderer of its `customType` (§15.9). `pi.registerCommand(name, {description, handler(args, ctx)})` registers a `/name` command.
- A dialog is not tied to a pi event handler: the adapter already calls `host.ctx.ui.notify(…)` from its follower's event handling (`handle.ts`, `startup.ts`) with the `ExtensionContext` it kept at `session_start`. `host.ctx` is set at `session_start` and cleared at shutdown; a reload makes a new one. It exists only in the TUI (`start` returns before it for any other mode, `startup.ts`), which is why "no `host.ctx`" below means "no UI". Dialogs share pi's editor slot (the User cannot type to the Lead while one is open), and pi emits `ui_prompt_start` and `ui_prompt_end` around each, which the adapter already reads for its state `dialog` (§15.4). **The review chat's own dialogs** (the `select` and `editor` that `runDialog` in `pi/lib/review-chat.ts` opens) **open no `dialog` reason**; they are already represented by `plan-review:<review>`. Every other dialog still opens `dialog` as today. The adapter implements this through a counter: each `runDialog` call increments the counter before the dialog opens and decrements it in a finally block; when `ui_prompt_start` or `ui_prompt_end` arrives while the counter is above 0, no reason is opened.
- Unverified: how a dialog opened by an adapter while the Lead's turn runs is drawn is pi's (the dialog takes the editor's place); the Job that implements this checks it with a real pi.

**Requesting.** The Lead runs `cpd plan review`, directly or through the tool `cpd_plan_review` (one required parameter, `title`, a string: the review's title, hs-d291; registered for Plan Leads only). The tool runs `cpd plan review --plan <CPD_PLAN> --title <title>` and returns its `next` text **at once**, without waiting for the User (a missing or empty `title` is the tool's error, from cpd's `usage`); when the result is `"already": true` and this session has no pending entry for that review, the tool opens it itself (below), so a review that failed to open can be retried. The tool's name, parameter and result are unchanged, and so is §6.5's `--exclude-tools` list for Workers.

**Opening**, on a `plan-review` Event with `status: pending` for its Plan, **whatever the Plan's phase** (a review of the drafts of a running Plan, §4.6, opens exactly like one in phase `plan`; the adapter does not read the phase and its list of Event kinds does not change), or from the tool:
1. Skip it when the session already has a `cpd-plan-review` entry for this review, or when it is not the Plan's latest review (`cpd plan show`, `review.id`): a replayed or superseded request.
2. **Without `host.ctx`** (no UI), or when `run/<plan>/summary-<review>.md` does not exist (a review made before cpd wrote it), the adapter opens the review in plannotator as before: "Open full plan" below, with no dialog. Otherwise:
3. Read `run/<plan>/summary-<review>.md` (§4.6). A failed read is an error message with the cause (turn); nothing is written and the review stays `pending`.
4. Show it: `pi.sendMessage({customType: "cpd-plan-summary", content: <the file's text>, display: true, details: {plan, review}}, {triggerTurn: false})`. The message is drawn **expanded**, always open (§15.9), and stays in the session, so `/reload` and a resume show it again.
5. `pi.appendEntry("cpd-plan-review", {plan, review, surface: "chat", status: "pending"})`; open the blocking reason `plan-review:<review>` labelled `φ: plan review`; close the reasons of this session's older reviews ("Latest only"). An entry **without `surface`** is a plannotator one, so a session recorded before this change keeps working; `plannotator_id` is required only on an entry with `surface: "plannotator"`.
6. Open the **dialog**, below. It runs **detached**: the handler that opens it (the follower's, the tool's or the command's) does not wait for it, so Events keep being read and handled, the cursor keeps moving, and the tool returns at once.

**The dialog**, with `host.ctx.ui` (one at a time per session; opening a newer review aborts the older review's dialog, below, whose entry stays pending as every superseded entry does):
- `ctx.ui.select("Plan review <review>: <review title>", ["Approve", "Change", "Open full plan"], {signal})`, `signal` of an `AbortController` the adapter keeps for this review.
- **Approve** → "Deciding" below with `cpd plan decide <review> --approve --plan <plan>`.
- **Change** → `ctx.ui.editor("What should change?")`. A note that is empty after trimming, or a cancelled editor, **returns to the select** (nothing is recorded). Otherwise the note is written to `$CPD_HOME/run/<plan>/feedback-<review>.md` (overwritten on a retry; no temporary directory is left behind) and "Deciding" runs `cpd plan decide <review> --reject --feedback-file <f> --plan <plan>`.
- **Open full plan** → the plannotator flow: `pi.events.emit("plannotator:request", {requestId, action: "plan-review", payload: {planContent, planFilePath}, respond})` with `run/<plan>/plan-<review>.md`, waiting at most 15 s for `respond`; a `handled` response with `status: pending` gives plannotator's own review id, and the adapter appends the entry again as `{plan, review, surface: "plannotator", plannotator_id, status: "pending"}` (the reason stays open). From here the decision comes from plannotator exactly as before ("Deciding"). plannotator's pntrack already records that page in `plannotator_reviews` (kind `plan`, contract §4.4), which is how the Inbox knows a page is open; the adapter adds nothing. If plannotator does not answer, answers `unavailable` or `error` (for example `Port 19432 in use`), the adapter delivers an error message with plannotator's error (turn), writes nothing, and **returns to the select**. Without `host.ctx` the same flow runs with no dialog to return to, and the review stays `pending`.
- **Esc** (`undefined`, and the controller's signal not aborted): nothing is recorded, the review stays `pending` and `plan-review:<review>` stays open. The adapter says so once with `host.ctx.ui.notify("Plan review <review> is still waiting: /plan-review reopens it")` (a notice, not a message to the model).

**`/plan-review`**, a pi command the adapter registers once at load (`pi.registerCommand("plan-review", {description, handler})`; for a Plan Lead only, as the tool is): it reopens the dialog of this session's **pending chat review**, the Plan's latest review when the last `cpd-plan-review` entry for it has `surface: "chat"` and `status: "pending"` and `cpd plan show` reports it `pending`; it sends no new summary. With none, `ctx.ui.notify("cpd: no plan review is waiting in chat")` and nothing else. With a dialog already open for it, nothing.

If the summary was shown and the dialog was left with Esc, only `/plan-review` (or a restart, "Recovery") brings the dialog back; the summary message stays in the chat.

**Latest only** (hs-d224, bug hs-n158). The reason `plan-review:<review>` follows only the Plan's **latest** review, because nobody decides a superseded review and entry `approved`, `rejected` or `superseded` are closed (`plan lock` and the Plan inbox read only the latest, contract §3.1, §4.6): it exists while the review is `pending` **and** is the Plan's latest review. Review ids are `r<k>`, compared by `k`.
- On a `plan-review` Event with `status: pending`, every reason of a review with a lower `k` closes, before the new review is opened, **and the dialog of such a review is closed** (its `AbortController` aborts; a closed dialog records nothing).
- On a `plan-review` Event with `status: superseded`, the reason closes and, if open, **the dialog is closed** the same way.
- At start-up (§15.2 step 5) only the review `cpd plan show` reports (`review.id`) can open a reason, when this session has a pending entry for it; the entries of older reviews open nothing. When `plan show` fails (an error message, §15.2), the highest `k` among this session's entries stands for it.
- The entries of superseded reviews stay in the entry list and plannotator's pages of them may stay open; only the blocking reason (and the dialog) closes. A reason closed twice, or one that is not open, changes nothing, so replays are harmless.

**Deciding**, for a pending entry, on the dialog's `Approve` or `Change` or on `plannotator:review-result` `{reviewId, approved, feedback?}`:
1. Write the feedback, if any, to `$CPD_HOME/run/<plan>/feedback-<review>.md` (the dialog's note, as above, or plannotator's; overwritten on a retry; no temporary directory is left behind); run `cpd plan decide <review> --approve | --reject [--feedback-file <f>] --plan <plan>`. `review_decided` means another caller decided it: the adapter reads the decision with `cpd plan show` and continues with it. A failed `plan decide` is an error message (turn); the entry stays pending and, for the dialog, the select opens again.
2. Append the entry with `status` `approved` or `rejected` (the entry keeps its `surface`); close `plan-review:<review>`.
3. Tell the Lead with `pi.sendMessage({customType: "cpd-plan-decision", …}, {triggerTurn: true, deliverAs: "followUp"})` (cpd 1 `decisionText`, kept; **the text is unchanged**): `Plan review r3 was APPROVED.` plus the feedback, then `Next: cpd plan lock` in phase `plan`, or in a later phase that the drafts are queued and no lock is needed (the Event's `data.jobs` lists them), or for a rejection `Revise the plan (job edit, job add, job cancel), then call cpd_plan_review again.`; when `current` is false, it says the Jobs changed since that review and asks for a new one. The custom type differs from `cpd`, so this message never moves the cursor.

**Deciding elsewhere**, on a `plan-review` Event with `status` `approved` or `rejected` for a pending entry of this session: any caller other than this adapter decided the review (`cpd plan decide` from the CLI, the User or a script), and the database already holds the decision, so the adapter does **not** run `plan decide`. Otherwise the Lead would stay `blocked` and never be told. It does what "Deciding" does after its own `plan decide`:
1. Read `cpd plan show` for `current` (and for the feedback when the Event's `data.feedback` is not a string).
2. Append the entry with the status; close `plan-review:<review>`; **abort the review's open dialog** (`AbortController.abort()`, which pi honours for `select`, "What pi gives"): the select closes and records nothing. An **editor** open in `Change` cannot be aborted (it has no signal): it stays until the User closes it, and its result is **ignored** because the entry is no longer pending (no `plan decide`, no new select).
3. Tell the Lead with the same `cpd-plan-decision` message (`triggerTurn`, followUp), with the Event's `data.feedback`, plus a last line saying the review was decided outside this adapter and, **for an entry with `surface: "plannotator"` only**, that plannotator's page stays open.
4. plannotator's page cannot be closed: its event API (`plannotator-events.ts`, 0.28.17) has no request for it (`plan-mode`, `plan-review`, `review-status`, `code-review`, `annotate`, `annotate-last`, `archive`); `closeKeepingDraft` in `server/serverPlan.ts` is only on the in-process server object, which the adapter cannot reach. The message says so when the entry is a plannotator one.

A failure of `plan show` is an error message and leaves the entry pending. An Event for a review with no pending entry (replayed, another Plan's, already decided) changes nothing. A later `plannotator:review-result` (or recovery's `completed`), or a late answer of the dialog, for an entry decided this way is ignored, because the entry is no longer pending; one that arrives while this path runs is ignored too, so the Lead is told once.

**Recovery** at `session_start` (t4 F5, kept for plannotator): for each pending entry,
- `surface: "plannotator"` (or no `surface`): `plannotator:request` `review-status` every second until it is not pending: `completed` is handled as a result; `missing` (plannotator restarted) appends the entry as `lost`, closes the reason, and tells the Lead to call `cpd_plan_review` again, which reopens it (above).
- `surface: "chat"`, the last entry of the Plan's **latest** review when `cpd plan show` reports that review `pending` (§15.2 step 5 has opened its reason): the dialog **reopens**, and the summary is **not** sent again (it is in the session). A review `plan show` reports `approved`, `rejected`, or `superseded` is not reopened; its `plan-review` Event, replayed from the cursor, is "Deciding elsewhere" (for superseded: treated as decided, so the reason closes and no dialog opens, like approved or rejected from the Event handler).

**Several Leads at once** (follow-up "Plan reviews collide across Leads"; hs-d35, hs-d40, hs-d50, decision User 2026-10-23). plannotator listens on one port by default, so every pi gets the configured range and plannotator picks the first free port of it per review (contract §6.5); config `plannotator_ports`, default `19432-19456` (contract §8.2): up to 25 reviews open at once. A full range is plannotator's own refusal in the chat. Links name the review host and the review's port (contract §6.5). History: 19432 was the old single port (cpd 1; seen on 2026-10-14 before the change). The old failure `Port 19432 in use after 5 retries` is replaced by plannotator's refusal when all ports are in use.

Rejected alternatives: one shared review server (cpd would have to run plannotator's server outside pi, which plannotator does not offer to extensions), and a queue (a review waiting for another Plan's User decision could wait for hours). Not verified: that two plannotator servers in one range do not disturb each other's sessions.

### 15.6 Reload after an install

Decision hs-d141 (research: `docs/research/pi-reload.md`), superseding its first answer; contract §5.3 "Reload after an install" and §6.7. After `cpd install` (§14.4) the adapter reloads its own pi (`ctx.reload()`: extensions, skills, prompts, themes, context files) without the User typing and without typing into a pane. Nothing reaches the model.

**Detection.** At `session_start` the adapter records the identity `(dev, ino, mtimeMs)` of its own directory (`dirname` of `pi/lib/`, from `import.meta.url`; contract §8 `pi_adapter`) with `statSync`. An `unref`'d timer polls it every 5 seconds (a constant of the adapter, not a config key) and is cleared at `session_shutdown`. The install replaces the directory as a whole (`rm -rf`, `mv`), so a new package has a new identity. `ENOENT` is the install in progress, not a change: the poll waits. A different identity of an existing directory is an install. The cpd binary is not watched (it changes first, before the directory is complete). The adapter runs only in `tui` mode (§15.1), so no other mode polls.

**The command.** The adapter registers `cpd-reload-after-install`. pi runs an extension's command with a full `ExtensionCommandContext` only through `pi.sendUserMessage("/cpd-reload-after-install", {expandPromptTemplates: true})` (pi 1.2.0 `agent-session.js` `prompt()`); without the flag the text goes to the model, and pi's own `/reload` cannot be reached this way. The handler checks the guard, then `await ctx.reload()`; nothing after it uses the old `ctx`. The new instance records the new identity at its `session_start`, which is why a successful reload is not repeated.

**The guard**, in this order, checked by the adapter before it triggers the command and again in the handler (the handler is authoritative; only it has `ctx`, and a User typing the command by hand is harmless):
1. `ctx.isIdle()` (mid-turn pi's `ctx.reload()` silently does nothing yet resolves, so it cannot be left to pi);
2. `ctx.hasPendingMessages()` is false;
3. the adapter's own state (§15.4) is `idle`: not `working`, `waiting` or `blocked`, so no Worker question, Plan review or `herdr:blocked` signal is open (pi's `isIdle()` does not see dialogs, and a reload closes an open dialog and loses its answer);
4. `ctx.ui.getEditorText() === ""` (no unsent typing).

**Retries.** A refused reload (the guard fails at the trigger or in the handler) does nothing and is **not** handled: the recorded identity stays the old one, so the next tick checks the guard again and triggers again; there is no limit and no backoff beyond the poll interval. At most one trigger is in flight. **At settle:** a Lead that gets a message every few seconds is idle for gaps shorter than the 5 s interval, so no tick may ever see it idle (`docs/research/pi-reload.md` "Why a busy Lead did not reload"). So the adapter also decides at pi's `agent_settled` (pi has no automatic work left), after its own state is updated: for a package that **already passed pre-validation** and is still the directory's identity it runs the same guard and, when it passes, the same trigger. It adds no guard, does not shorten the stability or validation steps (a directory not yet validated waits for its ticks), and keeps one trigger in flight.

**Logged.** Nothing is delivered to the Session and no `cpd` command is run for it. On a normal reload after an install, the wrapper prints nothing on re-exec and the adapter notifies nothing to the pane; the status line's version pill (#210, @cpd_version) is the only notice (hs-d533). When a reload fails, the adapter notifies with `ctx.ui.notify` (level `error`, `cpd: adapter update not loaded: <reason>`, `cpd: cannot write the reload log: …`, or the long refusal `cpd: a new adapter is installed; reload when free (/reload)`), the wrapper logs `reload skipped: …`, and pi's "Reload failed: …" is pi's own message. `ctx.ui.notify` is not written to the session file, so the notice is not evidence afterwards: the log below is.

**Log** (hs-d141, finding of `docs/research/pi-reload.md` "Why a busy Lead did not reload"). Every reload decision is one line appended to `$CPD_HOME/run/adapter/<session id>.log` (`CPD_HOME` default `~/.cpd`, `CPD_SESSION`; created with its directory at the first line; one file per Session, never rotated, small because a waiting or refusing line is not repeated while it is the last one). A line is `<ISO-8601 UTC time> <text>`; the texts, in the order they occur:
- `start: <reason>`: the adapter started (`session_start`; `reload` after a reload, `startup` or `new`, `resume`, `fork` otherwise);
- `error: <what>: <code>: <message>` (hs-d573): adapter error logged for a Lead Session, once per (`what`, `code`), keyed by `host.report`'s memory; the next different error is logged; if the error is a second occurrence, nothing is logged (G1's memory prevents it);
- `report failed: <code>: <message>` (hs-d573): the `cpd session report` command to the Manager failed with that code, never delivered and never reported again; the next different error tries again;
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
3. **pi itself loads the new package** (hs-d255 (a); the bug it replaces: a plain `node` + `jiti` load could not resolve `@earendil-works/pi-tui`, which pi provides to extensions at runtime, so every validation after #61 failed `Cannot find module`), in a child process, never in the live pi: `PI_OFFLINE=1 pi --no-extensions --no-session -e <dir> --mode rpc` with stdin `/dev/null`. `pi` is the one on `PATH`, the binary cpd starts the Session with (same version). Its `PI_CODING_AGENT_DIR` is a scratch empty directory under the OS temp directory (no User settings, sessions or credentials; with no model it needs no network), also its working directory, and removed afterwards; the User's sessions and settings are never read or written. Exit 0: the extension loads. Exit 1: pi's stderr says why (`Failed to load extension "<path>": …`, also `Extension does not export a valid factory function`). The child has a timeout of **60 s**, which tolerates a loaded machine; its process group is killed at the timeout, and **a timeout is a refusal** (`load timed out after 60000 ms`) retried at a later poll like any other, not a failure of the adapter. **Why a child:** pi's load runs the new module's top-level code and its factory; in the live pi that would be a second load of a package that may throw or hang. The factory runs in the child too, but the adapter activates only in `tui` mode (§15.1), so in `--mode rpc` it starts no follower and runs no `cpd` command. **Why pi itself:** it is the loader that will load the package, with its own aliases and virtual modules, so it cannot pass what pi rejects or fail what pi accepts (verified live: `docs/research/pi-reload.md` "Pre-validation with pi itself").

When a step fails: **no reload**; the recorded identity stays, so the check runs again at a later poll (no limit, the poll interval); and **one visible error**: `ctx.ui.notify("cpd: adapter update not loaded: <reason>", "error")` (`<reason>` is the first line of pi's stderr, `load timed out after <n> ms`, or `entry files missing`), shown once per distinct reason until a poll passes (a poll that passes re-arms it). Step 1 failing is not an error: it only waits. No `cpd` command is run and nothing reaches the model; the notice is the adapter's own, not an Event. Only when all three steps pass does the guard (above) run and the command trigger. A reload that still fails after a passing pre-validation (a load that differs from the child's) is caught by the wrapper's watchdog (§10.8 "Adapter watchdog").

**Relation to the wrapper's restart (§10.8; contract §6.7 "Reload on install").** The in-process reload is first. The wrapper's `SIGTERM` + `exec` stays as the fallback for what it cannot do: a pi without interactive UI, a reload refused for a long time (the wrapper's own idle rules: parked, or idle for 10 minutes), and an adapter that fails to load (the reload left pi without it; pre-validation above makes that rare, and the wrapper's watchdog, §10.8, recovers it). Both implement hs-d49 with the same guard in spirit (never with unsent typing, never `blocked`); when the adapter reloads first, pi keeps its process and the wrapper's executable check still sees the new `cpd` binary and may restart pi at its own safe point, which is harmless (the new `session run` resumes the session file).

### 15.7 Context updates at turn start

Batch B2 (hs-d79, hs-d132; narrowed by hs-d209, D1; redesigned by hs-d514). A Manager sees what other Sessions wrote to its Context, and what its Plans did, without polling. A Lead gets one short update with new records and pending board posts, not Jobs or other Events.

**Leads: short update with no turn start** (hs-d514, hs-d507). At pi's `before_agent_start`, for a Session of role `lead` (a Plan Lead, a side fork), one `cpd-context` message when anything is new since its cursor, holding only: (1) new decision records in force by other Sessions (`by.session` ≠ this Session), each as `- <id>: <first sentence>` (the rest: `cpd context show --record <id>`); (2) the pending board posts to it (from `cpd board take`), each in full with its sender; (3) the PRs of its Plan opened since, each as `PR <key> opened: <url>`. Nothing about Jobs merged or other Events. No turn is started. The cursor is the highest Event id of a new `context-record`, a new `board_posts`, or a new `pr-opened` Event of the Plan.

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

**Failure.** A failed read adds no message and leaves the cursor. It is reported once as an error message (§15.1, no turn), and again only after a read has succeeded.

### 15.7a First message of a by-user Session

Decision hs-d540 (User): when the User creates a Lead or Manager from the tree, pi starts with no first message and runs no turn; the role's first message is delivered with the User's first prompt, in the same turn, computed when that prompt is sent. Decision hs-d542 (User): (1) the tree passes `--by-user` to `plan new` and `context manager`; without it a command keeps today's behaviour. (2) Until the User's first prompt a by-user Session runs no turn at all: every message or Event that would start one is queued (pi's `sendMessage` `deliverAs` 'nextTurn', `triggerTurn` false) and goes with that prompt's run, with the first message. (3) The first message is hidden in the chat (`display` false); the User sees only their own prompt.

**Handler** (contract §10.8, hs-d542 A3). The pi adapter, for a by-user Session whose session file holds no `cpd-first-message` custom message delivered to it (`details.delivered_to` = this Session, the fork-safe rule of §15.2 step 4/§15.7), adds at the first run's `before_agent_start` one custom message `{customType: 'cpd-first-message', content: text, display: false, details: {event, delivered_to}}`; pi places handler messages after the User's prompt and the queued nextTurn messages (pi 1.3.0 `agent-session.js` `_prepareRun`). Until it is delivered, the adapter starts no turn: every `sendMessage` that would use `triggerTurn` true uses `deliverAs` 'nextTurn' instead. A failed first-message read is reported once as an error message (as §15.1 'Failure') and retried at the next run; no message is added.

**Reading.** The adapter calls `cpd session first-message --session <id>` (internal, §10.15), which renders the first message and returns the text, the file it was written to, and the highest Event id at rendering. The adapter reads the text and delivers it as a `cpd-first-message` custom message, with `details.event` = the returned Event id and `details.delivered_to` = this Session, so the read model and later runs can recognize it.

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

It is added **at most once per run**, and once it has been added the reminder is not due again until a later run ends with a message that fails the check. It is never delivered to a Session that is not a `lead`, and never starts a turn (`triggerTurn: false`, as §15.1's messages).

**Not done** (R3, *decided*): `pr open` does not ask for a checked statement; the reminder is the whole enforcement of this heuristic. The skills carry the rule (R4).

### 15.9 Rendering cpd's messages in a pi pane

Decision hs-d286 (User, 2026-10-19; it restyles hs-d221). **Display only:** every message below still reaches the model with its full `content` and `details`; only what the chat window draws changes. Nothing here changes a delivery (§15.1, §15.3), a cursor or an Event.

**Which messages.** Every `customType` the adapter sends with `display: true`: `cpd` (Events §15.3, Session messages, and the adapter's errors §15.1), `cpd-context` (§15.7), `cpd-plan-decision` and `cpd-plan-summary` (§15.5). A message with `display: false` (`cpd-reminder`, §15.8) is never drawn, so it has no renderer. A `customType` the adapter starts to display later is rendered by the same rule, its summary being the first line of its `content` (as an Event's, below). The adapter registers one pi message renderer per type, once at load (`pi.registerMessageRenderer`), from one module, `pi/lib/render.ts`; the renderer reads `options.expanded` and `options.outputPad` from pi.

**The look is pi-compact-tools 0.16.6's tool row, copied.** The adapter does not import or vendor that extension (it exposes no API and draws tool rows only); it draws the same rows, from the same glyphs, theme tokens, bold and dim, laid out as the extension's source lays them out (`extensions/compact-tools.ts`, `compact-tools-layout.ts`, `compact-tools-claude.ts`, `compact-tools-codex-rows.ts`, `compact-tools-palette.ts`, `compact-tools-core.ts`). The message keeps the `customMessageBg` background of hs-d221 behind the rows (the extension's rows sit on pi's tool background, which a message has no part in). **Colours** (hs-d534): the theme tokens named in this section (`dim`, `muted`, `toolOutput`, `customMessageLabel`, `customMessageBg`, …) are mapped to the palette's indices 0-15 by the Dango pi theme (contract §6.1.1: cpd's blocks magenta, lines and rails 8, toggles 7, content 7, tool pills by status in base colours, the Thinking pill cyan, a message's background 0); that theme and its pinning are Plan `pi-theme`'s, and until it lands the tokens keep the user's own theme.

**Which style: the extension's, read once.** The extension has four styles: `compact`, `claude`, `codex`, `off` (`DISPLAY_STYLES`, `compact-tools-types.ts`). Its **default is `compact`** (`DEFAULT_CONFIG.style`, `compact-tools-config.ts`), and that is what applies when no config exists, which is the case on hs today. The adapter reads the style **once, at `session_start`**, as the extension's `loadConfig` does, and keeps it for the run (a later `/compact-tools` switch shows in the adapter's rows after a `/reload`):

1. `compact-tools.json` in pi's agent directory (`getAgentDir()`, `~/.pi/agent` unless pi is told otherwise);
2. then, only when the project is trusted (`ctx.isProjectTrusted()`), `<cwd>/.pi/compact-tools.json`, which overrides the first.

Only the `style` key matters; the other keys are the extension's own. A file that is missing is skipped. A file that does not parse, or a `style` that is not one of the four, is handled **exactly as the extension handles it**: one line `[compact-tools] <path>: <message>` on stderr, and the value from the file before it (at first the default `compact`) is kept. The adapter adds no fallback of its own. With `off` the adapter registers **no** renderer and pi draws its own default box with the full `content` (the User turned the extension's rows off).

**The pill and the first sentence** (hs-d537, amended by the User 2026-10-22): the pill holds **only** `φ`, the cpd mark rendered bold (only the glyph is bold, not the colon or text that follows it). After it come one space and the summary's **first sentence**, **outside** the pill, text 7, as a tool row's title follows its pill: the summary up to the first `.`, `:` or line end, cut at **60 characters** with `…` (59 characters and `…`) only when that sentence is longer. A long sentence wraps onto continuation rows aligned like a wrapped tool title (`renderToolCall`). Example: ` ╭─[φ] Worker of Job chatfix-impl-misc (Attempt chatfix-impl-misc-a…` then ` ╰󰬦 expand`. The pill is magenta: `ESC[35m` U+E0B6 `ESC[7m` TEXT `ESC[27m` `ESC[35m` U+E0B4 `ESC[39m` (contract §6.1.1, hs-d545). The full summary is the title row's text where it is wider than that, below.

**The summary per kind** (what follows `cpd` on the collapsed row). One line, every newline and run of whitespace collapsed to one space and the ends trimmed:

| Message | Summary |
|---|---|
| `cpd`, a Session message (Event `session-message`) | `Message from <sender>`: the sender Session's name, or its id when it has no name; `Message from the User` when `data.from` is null |
| `cpd`, any other Event | the Event's `text`: its first line, cut to **160 characters** (159 and `…`) as the extension cuts a custom tool's summary (`CUSTOM_SUMMARY_MAX_LENGTH`); a row that is still wider than the content width `W - 6` wraps at it (ending at column `W - 4`, contract §6.1.1 "The rail fold"), as the extension's does, it is not cut again |
| `cpd`, an adapter error (`details.kind` is `error`) | `adapter: <what>`: the `<what>` of `cpd adapter: <what>: <code>: <message>` (§15.1) |
| `cpd-context` | its counts line, `cpd-context: 6 records · 17 Plan Events · 10 Plans pending` (§15.7) |
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

**The expand action** is the extension's: **a left click** on the ` ╰󰬦 expand · HH:MM` / ` ╰󰬬 collapse · HH:MM` line toggles that one message, and **Ctrl+O** (pi's `app.tools.expand`) expands or collapses every message and tool row at once. Pi hands the renderer `options.expanded` for Ctrl+O; a click is the renderer's own: it wraps its rows in pi-tui's `MouseRegion` that flips the message's own state, as pi does for a tool row (`createResultRegion`) and the extension does for its group row. The state is per rendered message and follows Ctrl+O both ways (the extension's group rule, `compact-tools-grouping.ts`); it is not persisted, so a reloaded session shows every message collapsed.

**No summary** (`cpd-context` whose `details` lack the counts, §15.7; and always `cpd-plan-summary`, hs-d502: the User reads it in the chat to decide, so it is never folded and Ctrl+O changes nothing for it; its content is Markdown, drawn as plain text like every message). There is nothing short to say, so the row is always open: the title `╭⦁ cpd-context` (for `cpd-plan-summary`, its summary above) and the full `content` as in "Expanded", ending with the terminator ` ╰󰬪` (contract §6.9 "The terminator": colour 8, no text, no toggle), never a bare `│`.

### 15.10 Policy text

At `session_start` the adapter runs `cpd policy show` with no flag (§22.1) and, at each run, adds one system prompt section, `cpd-policy`, with the `text` of every domain whose `audience` holds the output's `role`, labelled by domain and `source`. contract §8.3 "Delivery to agents" is the rule, including a failed `policy show` (an error message, no policy text).

### 15.11 A Manager or a Lead never blocks

Decision (User, 2026-10-19). Neither a Manager nor a Lead blocks on a wait: no sleep, polling loop or long wait in its own turn. Anything that takes time runs in the background and reports back as a message, or is the job of the Lead or Worker that owns it. The adapter enforces it, so it is not left to the skill's prose.

This rule is extended to a Lead (User, 2026-10-25, pro-d39). Evidence: during incident gatemac (pro-d29) the Lead of Plan tests-mock ran `for i in 1 2; do git fetch ...; sleep 280; done` in its own turn; the 280 s duration trips the 30 s bound and is refused. A Lead's wait holds the User's conversation as a Manager's does. A Worker keeps its own rule (§15.12), unchanged.

**When.** At pi's `tool_call`, for a `bash` call (`event.toolName` is `bash`, `event.input.command` and `event.input.timeout`), of a Session of role `manager` or `lead`. The handler is registered once at load (`pi.on` has no unsubscribe) and reads the role of the active run at each call; for a Worker, or before `session_start` has read the role, it returns nothing. A call that matches nothing runs as before.

**The bound** is **30 seconds**, one constant of the adapter (`pi/lib/noblock.ts`); there is no config key. The bound and the patterns are the same for both Manager and Lead.

**What is refused.** The call is refused when it waits longer than the bound, that is, when the command (`pi/lib/noblock.ts`, `blockingReason`) contains one of:

- `sleep N` with N > 30 s: any unit (`s`, `m`, `h`, `d`), decimals allowed (`sleep 30` passes; `sleep 31` and `sleep 1m` are refused); several arguments add up (`sleep 20 20`), and `sleep infinity` is refused;
- `timeout N …` with N > 30 s (same units; `timeout 20 cmd` passes, `timeout 60 cmd` is refused);
- `watch`;
- `tail -f` or `tail -F` (also `--follow`);
- `cpd events follow`;
- `gh run watch`;
- `gh pr checks --watch`;
- a `while` or `until` loop whose body sleeps (any `sleep`, whatever its duration);
- the bash tool's own `timeout` input above 30 (seconds).

A pattern counts where a command runs: a command word of a simple command (after `;`, `&`, `|`, a newline, a parenthesis, `do`, `then`, and a leading `VAR=value`, `time`, `nohup`, `exec`, `command`, `sudo` or `env`), not an argument or quoted text (`grep watch file` passes). A duration that is not a literal (`sleep $n`) is not judged.

A `for` loop is not a loop of this rule; the `sleep` in its body is judged by its own duration like any `sleep`, so `for i in 1 2; do git fetch origin; sleep 280; done` is refused (`sleep 280`) and `for i in 1 2; do sleep 5; done` passes.

**The refusal** is `{block: true, reason}` (pi's `ToolCallEventResult`), with the reason on one line naming the rule by role:

```
reasonText(matched, role) {
  if (role === "manager") return `cpd: a Manager never blocks: a wait over 30 s runs in the background or is the owning Lead's or Worker's job; refused: ${matched}`;
  if (role === "lead") return `cpd: a Lead never blocks: a wait over 30 s runs in the background or is left to the engine's Events; refused: ${matched}`;
}
```

`<what matched>` is the matched command (`sleep 1m`, `watch`, `tail -f`, `cpd events follow`, `gh run watch`, `gh pr checks --watch`, `timeout 60`, `while … sleep`) or, for the tool's input, `bash timeout 120`.

**Not affected:** a Worker, whatever it runs; every other tool; every `cpd` command's own timeout. A Lead's command that the User explicitly asked for and that waits over the bound runs in the background, as section 15.20 says.

### 15.12 A Worker runs targeted tests

Decision (User, 2026-10-19). A Worker runs only the tests its change touches; the full suite is the Gate's, run by the PR's `open` Job (contract §7.7; `pr open` is its repair). Several full suites and Gates at once loaded a host to 205 on 16 cores, and every Gate failed on timing. The Spec's acceptance names the targeted tests (the area binaries, `--test <area>` of docs/implementing.md's area table, and the modules the Job changes, plus its new tests), never a green full suite, and the adapter enforces the rule, as §15.11 does for the Manager.

**When.** At pi's `tool_call`, for a `bash` call (`event.toolName` is `bash`, `event.input.command`), of a Session of role `worker` only. It is the same handler as §15.11's, which consults each guard by role (`pi/lib/testguard.ts`, `unfilteredTestRun`); for a Lead, a Manager, or before `session_start` has read the role, this guard returns nothing.

**What is refused.** The call is refused when a command of it runs a full unfiltered test suite, that is:

- `cargo test` or `cargo nextest run`, also through `scripts/cargo.sh`, with or without `timeout N` in front, when it has `--workspace`, or has none of `-p` / `--package`, `--test`, `--lib`, `--bin`, `-E` / `--filterset`, or a positional test-name filter (also after `--`);
- `npm test` or `npm run test` with no file argument (a word after `--`, or after the script name, that is not an option);
- `npm run verify` (the Gate), whatever its arguments.

**Allowed**, for example: `scripts/cargo.sh nextest run -p cpd --test flow plan_overlap::`, `scripts/cargo.sh build -p cpd --test flow`, `cargo test -p cpd-core spec::`, `node --test pi/test/x.test.ts`, `npm test -- pi/test/x.test.ts`, `npm run typecheck`, `cargo build`.

A pattern counts where a command runs, as in §15.11 (a command word of a simple command, not an argument or quoted text: `grep "cargo test" f` passes).

**The refusal** is `{block: true, reason}`, with the reason on one line naming the rule:

```
cpd: a Worker runs only the tests its change touches; the full suite is the Gate's, run by the PR's open Job; refused: <what matched>
```

`<what matched>` is the command word and its subcommand, with `--workspace` when it was given (`cargo.sh test --workspace`, `cargo nextest run`, `npm test`, `npm run verify`).

**Not affected:** a Lead or a Manager, whatever it runs; every other tool; the Gate itself.

### 15.13 Questions stay short

Decision (User, 2026-10-20, hs-d502, which supersedes hs-d468's rule that a Plan with drafts never uses `ask_user_question`; hs-d503 keeps the 1500-character limit; hs-d548 adds guards that questions cite only what the User has seen). `ask_user_question` is for a few short choices, and a Plan's open choices go to the User through it, as rounds with a recommended option, **in every phase and whatever the Plan's Jobs**; the Plan review is a summary and a dialog in the chat (§15.5), and plannotator is for a long document. The adapter enforces what stays of the rule with guards, as §15.11 and §15.12 do for their rules. **The guard G1** (a refusal of every `ask_user_question` of a Lead whose Plan has a `draft` Job, with the reason `cpd: a Plan with draft Jobs is presented for review with cpd_plan_review (plannotator), not ask_user_question; refused: <n> draft Jobs`) **is deleted**: its code, its message and its tests; the adapter no longer reads `cpd plan show` for it. The guards G2 and G4 stay; G3 is deleted.

**When.** At pi's `tool_call`, for an `ask_user_question` call (`event.toolName`; the input is `{questions: [{question, header, options: [{label, description, preview?}], multiSelect?}]}`), in the same handler as §15.11 and §15.12, with the guard in its own module.

**G2, a long question.** For any Session (Lead, Manager, side fork), the call is refused when its text is over **1500 characters**. The text counted is the sum of every question's `question` and `header` and every option's `label`, `description` and `preview`. The limit is one constant, with no config key. The reason, on one line:

```
cpd: ask_user_question is for a few short choices; put anything larger in plannotator; refused: <n> characters (limit 1500)
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

**The ask pill.** The `[ask_user_question]` pill of the tool row (contract §6.1.1, §6.9) is magenta: `ESC[35m` U+E0B6 `ESC[7m` TEXT `ESC[27m` `ESC[35m` U+E0B4 `ESC[39m` exactly while the tool is executing and has no result, the window in which its closing row is ` ╰󰬦 choose` (the question is open and the User must decide). Reason: a question is a decision before the work runs, and that is magenta. Before the execution (arguments streaming) the pill is the running cyan 6 like any tool; after the result it is 2 (success) or 1 (failure). Every other tool's pill is unchanged.

**The question widget's header.** The widget is cpd's own copy of the package `@juicesharp/rpiv-ask-user-question` (`pi/ask-user-question/`, contract §6.10; only Leads and Managers have the tool). In single-question mode it draws the question's `header` as `theme.bg("selectedBg", " <header> ")` (a padded block, blue in our theme). The adapter turns that call into a read-only title pill, as it does for the jump hint (contract §6.1.1 "The jump hint"): caps U+E0B6 and U+E0B4, no padding spaces, `[Assumptions]` as the bytes `ESC[37m` U+E0B6 `ESC[7m` `Assumptions` `ESC[27m` `ESC[37m` U+E0B4 `ESC[39m` (colour 7 inverted: text in the terminal's background colour, caps 7: grey, as every in-pane title, §17 "Section titles"; never clickable). Assumption the User may correct: grey, not magenta; the colour is one constant. Other `selectedBg` uses (the selectors) are untouched.

**The question widget's tab bar** (User, 2026-10-22, Plan `pi-theme`; the colours are the User's, the Submit rule and the cut are the Lead's assumptions). With two or more questions the widget (cpd's copy of the package, contract §6.10; the tab bar rules below do not change) shows a tab bar. Ours is one row: one leading margin column, then pills joined by a single `─` in colour 8, no spaces: `[D4 plain pi]─[D5 key source]─[D6 machine allow]─[D7 cut]─[Submit]`. Pill, colour n (§6.1.1, title rule hs-d475): `ESC[`<code for colour n>`m` U+E0B6 `ESC[7m` `<label>` `ESC[27m` `ESC[`<code for colour n>`m` U+E0B4 `ESC[39m` (inverted), no padding spaces. Colour code: `ESC[3nm` for n in 0-7, `ESC[9+(n-8)m` for n in 8-15. Separator: colour 8 as indexed SGR `ESC[90m` `─` `ESC[39m`. Colour by state, in priority order: current question blue 4 (selection wins, even if answered); answered green 2; unanswered 7 (grey). The last pill `Submit`: blue 4 when current, else green 2 when every question is answered, else 7. No arrows `←`/`→`, no `■`/`□`/`✓` marks, no terminator after `[Submit]`. A line wider than the pane is cut at the pane width, never wrapped; the empty second row of the tab bar stays. Pills are not clickable (keyboard-driven widget). The line is produced from the widget's own props (`tabs`: label, answered, active; `submit`: active, allAnswered) and the width by a render hook recognising the `TabBar` component by class name and props shape, installed idempotently and removed at `session_shutdown`; the hook does not edit the widget's files and no text pattern is matched. The single-question header pill is unchanged.

### 15.14 Mentions of Leads and Managers (hs-d544)

User decision hs-d544 (2026-10-22, owner Plan `pi-theme`). Display only. In the pane of a **Lead or a Manager** (the adapter knows its Session's role; never a Worker's), a mention of another Lead or Manager in the **assistant's answer text** is drawn magenta: colour 5, text and not a block, `ESC[35m`…`ESC[39m` (contract §6.1.1).

- **What is scanned:** the assistant's answer text (the text parts of its message, not thinking and not tool rows), **outside code**: inline code spans and fenced code blocks are left alone (decision of this Plan: a quoted command is not a mention).
- **A mention** is a whole token (bounded by non-word characters, `[A-Za-z0-9_-]` counting as word characters, so `cpd2-x` does not match `cpd2`): a Session UUID or its 8-character prefix, a Plan id, or a Context id, of a Lead or Manager that exists, live or archived. Plan ids that are plain words (`distill`, `yield`, `delete`) light up in ordinary prose too; the User accepted that.
- **The set of ids** comes from the commands the adapter runs: `cpd session list --archived --role lead --role manager` (§10.1: `id`, so its 8-character prefix, and `plan`), `cpd plan list` (§4.2: `id`, archived ones included) and `cpd context list` (§3.2: `id`, archived included). A Plan or Context id counts only when a Lead (or Manager) Session exists for it.
- **Refresh:** at `session_start` (§10.8) and on Events of these kinds: `session-registered`, `plan-created`, `context-created`, `plan-deleted`, `context-deleted`, `session-renamed`, `plan-renamed`, `context-renamed`, `session-archived`, `plan-archived`, `context-archived` (hs-n423); every other Event refreshes nothing. A failed command keeps the previous set and shows the adapter's error as §15.1 does, once per error code, until a different error is shown. A Lead's follower only gets its Plan's Events (§11), so ids created elsewhere show after the next such Event or restart.
- **Drawing:** pi renders the assistant's markdown itself, so the adapter paints on the rendered lines: the assistant render override of `pi/compact-tools/extensions/compact-tools-silent.ts` (`RenderOverride`, `assistant`) wraps each rendered line's matching tokens in `ESC[35m`…`ESC[39m`, outside code. Idempotent: the same text gives the same lines.

### 15.15 The loop guard (hs-d552, hs-d571)

**L3 Counting** (the pi adapter; for a Lead and a Manager only). A *turn* is a pi agent run. It is **the User's** only when pi's `before_agent_start` event has `messageType: "user"` (a prompt the User typed). Every run a cpd message starts (`messageType: "custom"`: Events, a `session-message` with `from` null, `attempt-nudge`, a Plan review decision) is a **self-started turn**. The adapter keeps the start times of the self-started turns of the last 600 000 ms (a sliding window) from one clock, `options.now` (default `Date.now`).

**L4 Trip** (the adapter; trigger on the pi adapter, L4 of hs-d552). When a self-started turn starts and the window then holds MORE than `loop_guard_turns` turns (with the default 20: the 20th does not trip, the 21st does), the breaker trips. That 21st turn runs, pi has started it; from then on: (a) every message the adapter delivers with `triggerTurn: true` is sent with `{triggerTurn: false, deliverAs: "nextTurn"}` instead, through the one gate `gated` of `pi/lib/delivery.ts` that §15.7a's first-message hold already uses, so deliveries keep queueing, keep their order and keep moving the cursor but start nothing; (b) the adapter opens the blocking reason `loop-guard`, kind **human**, label `φ: loop guard` (§15.4); (c) it appends the pi session entry `cpd-loop-guard` `{tripped: true, at, turns}`.

**L5 The User's prompt** (`before_agent_start` with `messageType: "user"`) always runs. It empties the window; and when the breaker is tripped it also clears the trip: the reason closes, the entry `{tripped: false}` is appended, and deliveries trigger turns again. Nothing else resumes it: no timeout.

**L6 Resume from the Inbox**. `cpd session resume <id>` (§10.17) appends the Event `session-resumed`. On it, for its own Session while tripped, the adapter empties the window, clears the trip as L5, and delivers ONE message with a turn (the Event's text), so one turn starts and sees every delivery queued meanwhile. While not tripped it does nothing: no message, no turn. The turn it starts is a self-started turn (the window holds 1).

**L7 Reload** (L7 of hs-d552). The trip survives a pi reload. At `session_start`, before the follower starts, the adapter reads its pi session's last `cpd-loop-guard` entry; `tripped: true` re-opens the reason and the gate at once. The window is not persisted: a reload empties it.

**Sends that use the gate** (every send of a message with `triggerTurn: true`): `session-message` Events, `attempt-nudge`, the adapter's errors, the Plan review decision message. All of these go through `gated` when the breaker is tripped, so they queue without starting turns.

### 15.16 A primary checkout stays put (hs-n436, hs-d606, hs-d608)

Decision (User, 2026-10-22, hs-n436; it applies hs-d573, fix the cause, and hs-d248, a guard only for behaviour seen in practice, which this is; hs-d606 and hs-d608 widen it to every tool). No agent changes the files, the index or HEAD of a repo's primary checkout, with any tool: every repo of the machine, every role (Lead, Manager, Worker, side forks), while a run is active. On 2026-10-21 Leads ran `cd ~/projects/cpd && git checkout -q --detach origin/main` to read the latest main; HEAD stayed detached, dotfiles-sync installs cpd only from a checkout on `main`, and hs took no new cpd for 14 hours. On 2026-10-24 two Workers (Plans anim-perf and sync) changed `docs/contract.md` and `docs/commands.md` of cpd's primary checkout with the `edit` tool by absolute path (their Specs named it); the checkout went dirty and dotfiles-sync, which fast-forwards only a clean `main`, stopped installing cpd. A change reaches a primary checkout only through a merged PR and the sync that fast-forwards it. The adapter enforces the rule, as §15.11 and §15.12 do for theirs, so it is not left to the skills' prose.

**A primary checkout** is the main working tree of a git repository: the directory that holds a `.git` *directory* (the directory of a Plan's repo, `prs.repo`). A linked worktree (a Lead worktree, an Attempt worktree, a throwaway one made with `git worktree add`) holds a `.git` *file* and is not one. A directory is *in* a primary checkout when the nearest `.git` entry at or above it is a directory: a subdirectory of a primary checkout is in it; a subdirectory of a linked worktree is not, even when that worktree sits inside a primary checkout; a directory with no `.git` at or above it, or one that does not exist, is in none. For a write target, a path that does not exist is in the primary checkout of its nearest existing ancestor (a write creates the missing directories), and a path below a checkout's `.git` is in it. Only the file system is read (`lstat`), never `git` or `cpd`.

**When.** At pi's `tool_call`, of a Session of **every role** (Lead, Manager, Worker, side forks) while a run is active (`session_start` has read the role), in the one handler of §15.11 and §15.12 and before their per-role guards. The code is `pi/lib/mainguard.ts`: `primaryCheckoutMove(command: string, cwd: string, home: string): string | undefined`, `primaryCheckoutWrite(toolName: string, input: unknown, cwd: string, home: string): string | undefined` (each returns the refused call as the reason shows it, or `undefined`; the adapter passes the context's `cwd` and `os.homedir()`) and `reasonText(matched: string): string`. Its tokenizer is the one §15.11 and §15.12 share (`pi/lib/shell.ts`), not a third copy. The handler (`pi/lib/adapter.ts`), while `host.run` is set: `edit` and `write` call `primaryCheckoutWrite`; `bash` calls `primaryCheckoutMove`; any other tool is not judged by this guard, and a call that matches nothing returns nothing.

**What is refused.** A call is refused for three kinds: a git subcommand, a bash write form, and an `edit` or `write` call.

**Git.** A git command whose subcommand is one of **seventeen** — `checkout`, `switch`, `reset`, `rebase`, `merge`, `pull`, `stash`, `restore`, `clean`, `commit`, `cherry-pick`, `revert`, `am`, `apply`, `rm`, `mv` and `add` (they change files, the index or HEAD) — with any arguments (`clean -n` and `apply --check` included), run in a directory that is in a primary checkout. The subcommand is the word as written: `merge-base` and `merge-tree` are other subcommands and pass. The matched text is `git <subcommand> in <root>`. Everything else passes, among it `fetch`, `log`, `show`, `diff`, `status`, `grep`, `rev-parse`, `ls-files`, `branch`, `tag`, `config`, `remote`, `update-ref`, `merge-base`, `merge-tree`, `worktree add` and `worktree remove`: a ref move (`update-ref`, `branch -f`) does not dirty a diff and stays not judged.

**The `edit` and `write` tools.** For the pi tools `edit` and `write` the adapter calls `primaryCheckoutWrite(toolName: string, input: unknown, cwd: string, home: string): string | undefined`. It returns `undefined` for any other tool, for an `input` without a non-empty string `path` (no other field is read), and for a path that is not in a primary checkout; otherwise the matched text `<tool> <path> in <root>`, for example `edit <abs path> in <root>`. The path is resolved as pi resolves tool paths: one leading `@` dropped; `~` and `~/…` from `home`; an absolute path as it is; any other relative to `cwd`; then normalised lexically (`.` and `..`; never `realpath`, so a symlink into a primary checkout is not followed: not judged). In the matched text `<path>` is that absolute path and `<root>` the root of the primary checkout.

**Bash writes.** `primaryCheckoutMove(command, cwd, home)` keeps its name and signature and now also returns the matched text `<form> <path> in <root>` of a write form. The command is read as today (the simple commands of the shared tokenizer, the prefixes, the `cd` tracking, the current directory); each simple command is judged for its git form and its write forms in one walk, and the first match in order is returned. The write forms, and the words each judges:

- **Redirection.** A word made of optional digits, then `>`, `>>` or `>|`, then optionally the target (`>f`, `>>f`, `2>f`, `>|f`); an operator word alone takes the next word of the same simple command. `&>f` is read as `>f`, because the tokenizer ends a command at `&`. A redirect with no target word in its simple command (`2>&1`: the `&` ends the command) is not judged. `<form>` is the operator without its digits (`>`, `>>`, `>|`). Reads (`<`, `<<`, `<<<`) are not writes.
- **Command words** (after the prefixes; the word or a path ending in `/<word>`; `<form>` is the word without its directory): `tee`, `rm`, `touch`, `truncate` and `mv` judge every operand (a word not starting with `-`); `cp`, `install` and `ln` judge the last operand and the value of `-t <dir>` or `--target-directory=<dir>` (their other operands are sources that are only read, and pass); `sed` and `perl` with an in-place flag (a word that is `-i`, starts with `-i`, is `--in-place`, starts with `--in-place=`, or is a short-option cluster ending in `i` such as `-ni` or `-pi`) judge every operand; `dd` judges the value of an `of=<path>` operand; `patch` is judged by the directory it runs in (the current directory, or the value of `-d <dir>` or `--directory=<dir>` resolved from it), its operands are not read.
- **Resolving a target word.** As a directory word of this section (`~`, `~/…`, `$HOME`, `${HOME}`, relative to the current directory, lexical), extended: a word with `*`, `?`, `[` or `{` is judged by its literal leading directory, the segments before the first segment that holds one of them (`<root>/docs/*.md` is judged as `<root>/docs`); a word that holds `$` or a backtick, a `~user` and `-` cannot be resolved and is not judged; a relative word is not judged when the current directory is unknown. A resolved target is in a primary checkout by the definition above. Only the first matching target of a command is reported; `<path>` is the resolved absolute path (for a glob its literal directory, for `patch` the directory).
- **Everything that only reads passes:** `cat`, `grep`, `ls`, `head`, `sed -n`, `cp` from a checkout, `git show`, `echo rm <path>`, `grep 'rm <path>' f`.

**Where a command runs.** The call is read as its simple commands (the tokenizer of §15.11: split at `;`, `&`, `|`, a newline, parentheses and backticks, quotes taken off; a command is where a command word stands, so `echo git checkout` and `grep "git reset" f` pass). The command word is `git`, or a path ending in `/git`, after the prefixes of §15.11 (`FOO=1`, `time`, `nohup`, `exec`, `command`, `sudo`, `env`, `do`, `then`, and the like). The directory starts as `cwd`. A `cd <dir>` (the first word that does not start with `-`; none means `home`) sets it for every later command of the call, in whatever order the separators put them, a subshell's `cd` included: the guard errs towards refusing. `git`'s global options before the subcommand are skipped as the `git` wrapper of contract §6.5 skips them (`-c <k=v>`, `--git-dir <d>`, `--work-tree <d>`, `--namespace <n>`, `--super-prefix <p>`, `--config-env <k=v>`, their `--opt=value` forms, and flags such as `--no-pager`, `-p` and `--bare`), except `-C <dir>`, which changes the directory for that command only, each `-C` relative to the one before it. A directory word is resolved as the shell would: `~`, `~/…`, `$HOME` and `${HOME}` (alone or followed by `/…`) from `home`, a relative word from the current directory, then normalised lexically (`.` and `..`, no `realpath`). A word that still holds `$`, a backtick, `*`, `?`, `[` or `{`, a `~user`, and `cd -` cannot be resolved: that command, and every later one until a `cd` that resolves, is **not judged** (the throwaway form `cd $(mktemp -d)` must pass).

**Not judged**, because the guard stops mistakes and not a determined bypass: `sh -c '…'`, `xargs`, scripts and interpreters (python, node, make, cargo, npm: a build writing its ignored output dir included), editors, `rsync`, `tar`, `unzip`, `curl -o`, `wget -O`, `mkdir`, `rmdir`, aliases and functions, `sed` without an in-place flag, symlinks, a `$VAR` or command-substitution path, `GIT_DIR`, `--git-dir`, `--work-tree` (they do not change the directory the guard judges), and `patch` operands.

**The refusal** is `{block: true, reason}` for the three kinds, with `reasonText(matched)` giving this one line (the four read alternatives stay; the way to change files is new, before `refused:`):

```
cpd: no agent moves HEAD or changes the files of a repo's primary checkout; fetching is allowed, then read origin/main with 'git show origin/main:<path>', 'git log origin/main' or 'git diff origin/main', or in a throwaway 'git worktree add $(mktemp -d) origin/main' that you remove afterwards; to change files, work on a branch in a worktree ('git worktree add -b <branch> <dir> origin/main', or the worktree of your Job) and let a PR carry the change; refused: <matched>
```

`<matched>` is `git <subcommand> in <root>` for a git command (the refused subcommand and the root of the primary checkout, the directory that holds its `.git` directory, an absolute path as resolved above, never a `realpath`), `<form> <path> in <root>` for a bash write form, and `<tool> <path> in <root>` for an `edit` or `write` call.

**Examples.** The Session's `cwd` and `home` are `/home/clayton` and the example repo's root is `~/projects/cpd` (`<root>`). Refused: `edit` and `write` of `<root>/docs/x.md` by absolute path, by `~/…`, by a `@`-prefixed path and by a relative path from a subdirectory; `write` of `<root>/new/dir/f.md` (the directories do not exist); `echo x > <root>/f`; `cat <<EOF >> <root>/NOTES.md`; `echo x | tee <root>/f`; `sed -i s/a/b/ <root>/f`; `rm -rf <root>/src`; `rm <root>/docs/*.md`; `cp /tmp/a <root>/b`; `mv <root>/a /tmp/b`; `touch <root>/f`; `cd <root> && patch -p1 < /tmp/x.diff`; `git -C <root> commit -m x`; `cd <root> && git add .`. Passes: the same tools and commands in a linked worktree (also one inside the primary checkout) and outside any repository; `cat <root>/f`; `cp <root>/a /tmp/b`; `cmd 2>&1`; `cmd > /dev/null`; `git -C <root> fetch origin`; `git -C <root> show origin/main:README.md`; `echo x > $OUT`; `rm $DIR/f`. Today's git-only refused forms stay refused: `cd ~/projects/cpd && git checkout -q --detach origin/main`, `git -C ~/projects/cpd switch -c x`, `cd ~/projects/cpd/src; git reset --hard origin/main`, `git -C ~/projects/cpd rebase origin/main`, `git -C ~/projects/cpd merge origin/main`, `git -C ~/projects/cpd pull`, `git -C ~/projects/cpd stash`; today's git-only passes stay: `git -C ~/projects/cpd log origin/main`, `git -C ~/projects/cpd show origin/main:README.md`, `git -C ~/projects/cpd diff origin/main`, `git -C ~/projects/cpd status`, `git -C ~/projects/cpd grep -n x origin/main`, `git -C ~/projects/cpd worktree add $(mktemp -d) origin/main`, `git -C ~/projects/cpd merge-base HEAD origin/main`; each command of the refused list run in a linked worktree (`cd ~/projects/cpd.pay.api && git merge origin/main`); every command outside a repository.

**The skills.** Where the cpd-lead and cpd-manager skills tell an agent it may read a repo, they name the allowed forms only: `git fetch origin`, then `git show origin/main:<path>`, `git log origin/main`, `git diff origin/main`, or a throwaway `git worktree add $(mktemp -d) origin/main` removed afterwards; and they say that HEAD is never moved in a primary checkout. No skill file (a `SKILL.md` or a file of its `references/`) contains `git checkout`, `git switch`, `git reset`, `git pull`, `git stash` or `--detach`. The tests of this section read the skill files for both.

**The `git` wrapper stays.** `$CPD_HOME/bin/git` (contract §6.5, hs-d310) still refuses `git stash` in every directory of every Session, for its own reason (`refs/stash` is shared by every worktree). In a primary checkout this guard answers first, with the reason above.

### 15.17 Secrets in the pi adapter

Decision (User, 2026-10-22, accepted assumptions A1–A5). Agents never call 1Password's `op` directly; the pi adapter refuses it and offers `cpd secret` commands instead (contract §8.4, commands §24). cpd holds no `op` token and never signs in; the 1Password app's per-use prompt is the approval. The adapter enforces the raw-`op` guard as §15.11 and §15.12 do for theirs, and provides the `cpd_secret_approve` tool for a Lead or Manager to request a secret outside the `cpd` and `homeserver` vaults.

#### The raw-op guard

**When.** At pi's `tool_call`, for a `bash` call, of a Session of **every role** (Lead, Manager, Worker, side forks) while a run is active, in the one handler of §15.11 and §15.12 and before their per-role guards. The code is `pi/lib/secretguard.ts`: `secretBypass(command: string): string | undefined` (the refused command as the reason shows it, or `undefined`; the adapter passes the command text from pi) and `reasonText(matched: string): string`. Its tokenizer is the one §15.11 and §15.12 share (`pi/lib/shell.ts`), not a third copy. For any other tool, and for a call that matches nothing, the handler returns nothing.

**What is refused.** A simple command (the tokenizer of §15.11: split at `;`, `&`, `|`, newline, parentheses and backticks; quotes taken off; a command is where a command word stands, so `echo op read` and `grep "op read" f` pass) whose command word, after the prefixes of §15.11 (`FOO=1`, `time`, `nohup`, `exec`, `command`, `sudo`, `env`, `do`, `then`, and the like), is `op` or a path ending `/op`, with any arguments: the matched text is `op` followed by its first argument when it has one (`op read`, `op item`, `op`); and the command `cpd secret approve` (the matched text `cpd secret approve`): approvals are the User's, written only by the adapter after its dialog. Every other `cpd secret ...` command passes.

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

Decision (User, 2026-10-26 11:53 EDT, relayed by the Lead of Plan preco-home-buttons): a Lead runs a command the User explicitly asks it to run. Confirmed and scoped by decision pro-d57 (User, 2026-10-26 12:09 EDT, accepted assumptions A1–A4): A1 the adapter's guards stay for a Lead; A2 only an explicit request counts, never an inferred one; A3 the rule covers Leads only, a Manager's rules are unchanged; A4 the Lead skill's standing bans stay (tmux, wt, git push, product code and commits, and what belongs to cpd itself: `cpd plan decide`, `cpd watch`).

**The rule.** A Lead runs a command the User explicitly asks it to run, and does not refuse it because of its role (decision pro-d56, confirmed and scoped by pro-d57, User, 2026-10-26 11:53 and 12:09 EDT). The User asked in words, in the Lead's conversation, for the Lead to run a command, and the Lead runs it.

**Explicit only.** The User asked, in words, in the Lead's conversation; a Lead never infers a request from the context, from a Worker's Escalation or from a Manager's message that does not quote the User.

**Leads only.** A Manager's and a Worker's rules are unchanged. A Manager asks a Lead; a Worker runs its own turn; neither is given this latitude.

**What stays.** Every adapter guard applies to the request: `pi/lib/noblock.ts` (§15.11, a bash call that waits over 30 s), `pi/lib/testguard.ts` (§15.12, a Worker's full test suite), `pi/lib/askguard.ts` (§15.13, a question's length and labels), `pi/lib/loop-guard.ts` (§15.15, a loop that blocks), `pi/lib/mainguard.ts` (§15.16, a primary checkout's files), `pi/lib/secretguard.ts` (§15.17, raw `op` and `cpd secret approve`; `op` is answered with `cpd secret`), `pi/lib/hostapprove.ts` (§15.18, `cpd host approve`), and `pi/lib/pnguard.ts` (§15 'Every plannotator session is tracked', a plannotator call without a reachable link). So do the Lead skill's standing bans: tmux, wt, git push and force-push, product code and commits, and what belongs to cpd itself (`cpd plan decide`, `cpd watch` except `--ensure`; those last two are skill bans, not adapter guards). When one stops a request, the Lead tells the User what was refused and what to do instead.

**A command that does not exit within 30 seconds** (a dev server, a watcher, a long build) runs in the background: its own process group, its output in the log `/tmp/cpd-<plan>/<name>.log`, its process group id in `/tmp/cpd-<plan>/<name>.pid` (`<plan>` the Plan's id, `<name>` a short name), because the adapter refuses a bash call that waits over the bound (§15.11) and a foreground one would hold the User's conversation. The Lead then tells the User, in one message, the URL or port, the log path and how to stop it.

**Nothing is left orphaned:** the Lead stops what it started (the process group, so the children end too) when the User says they are done, when the User asks it to stop, and at `plan-done` if it still runs.

**Where it runs.** Where the User says; for a Plan's branch, in the PR's Lead worktree (`lead_worktree` of `cpd plan show`).

**Enforcement.** No guard is added: the adapter already lets a background start through (pi's bash tool starts each command in its own process group and returns when the shell exits), and the rule is prose in the cpd-lead skill, pinned by tests. State the exact shell commands NOT here: they belong to the cpd-lead skill, which a later Job of this PR writes, and this section must not repeat them (DRY).

## 16. The Worker contract

### 16.1 The Worker Spec

Rendered by the driver at A3 to `run/<plan>/specs/<attempt>.md` from a template compiled into the binary, so nothing in a repo or worktree can change what Workers are told. Content, kept from cpd 1 `templates/worker.md` (cpd 1 §16) with the changes marked:

| Section | Content |
|---|---|
| Title | `# Worker Spec: <job>, Attempt <n>` |
| Who you are | Attempt `<n>` of Job `<job>` (`<title>`) in Plan `<plan>` (was "Workflow"); attempt id, Tier, worktree path |
| Rules | work only in this worktree and touch only the listed files; commit your work; before submitting, run the quick check yourself: the commands of `pre-commit` from the PR's policy (`[hooks]`, contract §7.3), listed verbatim, or, when the policy has no `pre-commit` hook, the line `This repo defines no quick check.` (change: cpd 1 said `./verify.sh`; R20: no fallback to `pre-merge`); never edit the protected paths, listed: `.cpd/` and each entry of the PR's `protected.paths` (§7.5; change: cpd 1 hard-coded `verify.sh`); never ask the User anything; tear down everything it started before it submits (private tmux servers, background processes, temporary homes, a `cpd watch` it started; hs-d34, hs-d39); if continuing would require guessing, `cpd ask <attempt> "<question>"`, which blocks until the Lead answers (change: no `--wf`, the pane has `CPD_PLAN`); one blocker at a time; commit a skeleton within 10 minutes, then at least every 20 minutes; run only commands that exit: `timeout` around every test and measurement, no synthetic benchmarks, never `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run` or any other command that does not exit (hs-n97, hs-n107); run only the tests the change touches, never the full suite (the Gate's); do not stop until you have submitted |
| Job | Goal, Context, Files you may touch (`touches`), Acceptance, Out of scope (or "Nothing is listed. Still do only what the Goal and Acceptance require.") |
| Previous Attempts | only when the Job has ended Attempts: for each, number, Tier, outcome and cause, and what explains it: the conflicting files (`merge_conflict`), the failed Gate's hook and log (`gate_failed`, `gate_error`), or one sentence for every other cause (change: cpd 1 showed only feedback) |
| Answers already given | new: every `answered` Escalation of earlier Attempts, question and answer, so a Worker does not ask again |
| Finish | `cpd attempt submit <attempt>`; on success stop at once; on a refusal, what each code means: `wrong_worktree` (run it from the worktree), `dirty_tree`, `no_commits`, `gate_tampered` (revert the protected paths), `check_failed` (fix, commit, submit again; the tail is in `details`), `check_error` (the check could not run: ask the Lead), `attempt_not_running` (the Attempt was stopped: stop) |

**Gate causes in Previous Attempts.** For an Attempt that ended `failed` / `gate_failed` or `gate_error`, the entry names the failed Gate's hook (`pre-commit` or `pre-merge`) and log, read from the Attempt's end Event `detail`. When the detail has the hook and log:

- **`gate_failed`, hook and log known:** "The Gate (`<hook>`) failed at its merge. Its whole output is in this log; read it and fix what it reports:"
- **`gate_failed`, neither known:** "The Gate failed at its merge."
- **`gate_error`, hook and log known:** "The Gate (`<hook>`) could not be judged at its merge: it was killed, timed out or could not run, which is a problem of the Gate, not a failing check of your work. Its output so far is in this log, if the hook started:"
- **`gate_error`, neither known:** "The Gate could not be judged at its merge: it was killed, timed out or could not run, which is a problem of the Gate, not a failing check of your work."

When the log is known it follows as one indented list item holding the absolute path in backticks, like the files of `merge_conflict`.

**A review Job's Spec** (B2) differs from the table in these rows: *Who you are* says it is a review Job and that it is a fork of the Lead; *Rules* replace "commit your work" and the quick check with **commit nothing**, and tell it to read the diff of the PR's Lead Branch against the PR's base (`<lead branch>` and `<base>` are given) and the Plan's Specs, to write its findings to `$CPD_HOME/run/<plan>/reviews/<job>.md` (absolute path given, non-empty), and not to edit the repo; *Finish* adds `review_commits` (you committed: undo it and submit again) and `review_missing` (write the findings file) to the refusal codes and drops `no_commits` and `check_failed`. The findings are Markdown; the Lead reads them and turns them into fix Jobs before `pr open`.

**The kinds that have checks** (hs-d340, hs-d341; contract §2.2 "Job kinds", §7.7) differ from the table in these rows; a `docs` Job and a `work` Job have no row of their own.
- **`test`**: *Who you are* says it writes red tests for the Job's named tests and that another Worker implements them later. *Rules* add: write the named tests, in the files of `test_files`, from the docs alone; add stubs (empty or panicking implementations) so that the branch builds; do not implement; the submit builds the branch and runs the tests, which must fail. *Job* gets a subsection **Tests** that lists `tests` and `test_files`. *Finish* adds the refusals `test_files_missing`, `tests_unconfigured` (the repo's policy has no `[test-commands]`: ask the Lead), `tests_build_failed` (the stubs do not make it build), `tests_not_found` (a named test does not exist: fix the name or write the test), `tests_not_red` (a named test passes: make it fail for the right reason, for example against the stub) and `tests_error` (the check could not run: ask the Lead).
- **`impl`**: *Who you are* names the `test` Jobs it builds on. *Rules* add: make the listed tests pass (the union of those Jobs' `tests`, derived); never change a guarded file (the `test_files` of those Jobs, listed; the paths the Lead approved are listed apart, read when the Spec is rendered); a test that is wrong is a question (`cpd ask`), not an edit; the submit builds the branch and runs the tests, which must pass. *Job* gets the same **Tests** subsection: the derived tests and the guarded files. *Finish* adds `tests_unconfigured`, `tests_build_failed`, `tests_not_found` (ask the Lead), `tests_failed`, `tests_tampered` (revert the guarded paths, or ask) and `tests_error`.
- **`open`** and **`update`**: *Who you are* says it brings the PR's Lead Branch up to date with `<base>`, and that cpd runs the Gate, pushes and opens the pull request itself. *Rules* replace "commit your work" with: `git fetch origin <base>`, then, **only if the Lead Branch conflicts with it** (`git merge-tree --write-tree --name-only origin/<base> HEAD` exits 1; a base that merely moved is no reason, hs-d490), `git merge --no-edit origin/<base>` in the worktree; resolve every conflict keeping both sides' intent; commit the merge and the code it needs (recomputed hashes, tests adapted to the base's new rules, a renumbered migration); run the quick check; submit, also when there was nothing to merge (an `update` Job is queued by a conflict only, so it merges the base). It does not push, does not run `gh`, does not run the Gate and does not touch the pull request. *Job* is cpd's fixed text for the kind. *Finish* adds `base_conflict` (the head conflicts with the base: merge the base, resolve, commit), `lead_branch_moved` (merge the Lead Branch), `pr_title_required` and `pr_closed` (`open` only: ask the Lead), `illegal_transition` (`update` only: the PR is not open, ask the Lead), `base_not_found`, `gate_failed` (the tail is in `details`: fix it, commit, submit again), `gate_error` (ask the Lead) and the policy codes (`owner_unset`, `owner_unconfirmed`, `policy_invalid`: ask the Lead), and drops `no_commits`. The *Rules* row also says the Worker does not push, run `gh`, run the Gate or touch the pull request. The rows of the Spec for a kind are rendered from the Job's kind and its stored facts (`tests`, `test_files`, the guard of an `impl` Job, `test_approved`), as the Spec is rendered (A3); a `work` or `docs` Job's Spec is unchanged except that *Rules* name the PR's `open` Job, not `pr open`, as the place of the full suite.

The Worker is started as §6.5 says, with the first message `Read <spec path> and follow it exactly. Your attempt id is <attempt>.` and `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT` in its environment, and `PATH` with `$CPD_HOME/bin` first, where the `git` wrapper refuses `git stash` (contract §6.5, hs-d310).

**Nudge** (A5, once): an `attempt-nudge` Event to the Worker's Session with `data.turns` = the Worker's assistant-turn count when nudged (a resumed driver reads its nudge baseline from it) and `data.prompt` = `You ended your turn without running cpd attempt submit <attempt>. Finish the Spec and submit, or run cpd ask <attempt> "<question>" if you are blocked.` (cpd 1 §12 step 7, kept), delivered with a turn (§15.3).

**Submit preconditions:** §6.1, in §7.3's order.

### 16.2 The review files and the brief

Removed by R19 (no Job-level Reviewer).

### 16.3 The verdict

Removed by R19 (no Job-level Reviewer).

### 16.4 Worker latitude

A Worker may change a file outside its `touches` when a test named in its acceptance cannot pass without it (hs-d512, contract §16 "The Worker contract"). The Worker lists each such file and why in its commit message, instead of escalating with `cpd ask`. Examples: a migration that only this Job should create, a helper script the test uses, a stub of a new module. The Lead reads the commit message at the Worker's submit and approves or escalates; the test must pass without editing the guide.

## 17. What the TUI reads

Outline §8 and D4: the views take one **snapshot** with a read command, then **follow Events**; there is no timer polling of the CLI, no cache directory and no tmux option carrying state, **with one deliberate exception**: `@cpd_workers`, the StatusBar's workers text, which the engine sets, with the workers' short form and the width (below; contract §6.1). A view reads the database **in process**, through the read model (`tui/src/read/`): the same queries as the snapshot commands named below, one read transaction per snapshot, no subprocess and no `gh`. (An earlier draft had each view run `cpd` as a subprocess; the implementation does not, and one source of truth is the store.) Reads take no lock and never wait for writers (§4.2); cpd 1's `workflow show` took 58 s under load (research.md), which is why the spike needed a cache.

**Redraw rule, pacing and reads** (hs-d341). The measured causes of stutter in docs/research/anim-perf-plan.md and anim-perf-tree.md lead to four rules, the same everywhere the tree and plan views loop:

- **R1: Redraw rule.** The tree has one function that names every spinner its drawing code emits: `View::animated()` counts working and fixing Leads and merging Inbox items. The plan view's spinners are a running Job's pill and a PR box title's Lead dot (`Pull::lead_working`), and one function names them all with one line per spinner, so a spinner cannot be drawn without making the view redraw. While a spinner is on screen, the view draws once per 50 ms frame. With no spinner on screen, the view draws only when something changed (an Event, a size change, a clock tick, or input), never on a bare timer.
- **R3: Clock-aligned pacing.** While a spinner is on screen, the loop wakes at each 50 ms boundary of the wall clock (`wait = 50 ms - (unix ms % 50)`, between 1 and 50 ms) and draws once per frame. With no spinner on screen, the loop does not wake for the clock and blocks on its channel for a signal. A draw that takes longer than a frame is followed by the next frame's draw, never by a burst of catch-up draws: at most one draw per frame.
- **R4: Reads never block the draw.** No read (the snapshot, the Event follower, the open pull request read, the engine lock probe, the merge marker read) runs on the thread that draws. All views, including the Dispatch column, run their reads on a reader thread; the loop takes a finished result without waiting. At most one read runs at a time; a request made while one runs waits, and only the newest waiting request is kept. A read that fails keeps the last snapshot on screen and the view shows its error. The first snapshot is taken before the terminal is set up. The end of an action, a popup, or a Worker-pane call wakes the loop like a read result.

**Titles** (hs-d257). No view follows `session-titled`: the one place a Session's dynamic title is shown is the chat slot's pane header, which tmux draws from `@cpd_title` (§13.2, contract §6.1), set by `session retitle` itself. A view that wants the title reads `sessions.title` from `session list` or `session show` (§10.1, §10.2). `session-titled` is in the Event log (`logs`) like every Event, and in no view's follow list.

**Gap-free start.** Every snapshot command returns `events_cursor`, the highest Event id in the same read transaction. The view then runs `cpd events follow --since <events_cursor>`, so no Event between the snapshot and the follow is lost or seen twice.

| View (`@cpd_view`) | Snapshot (the read of the command named) | Events it acts on | What it updates |
|---|---|---|---|
| `tree` (Context column) | `cpd context list`, `cpd session list` | `context-created`, `context-grounded`, `plan-created`, `plan-phase`, `plan-done`, `session-registered`, `session-state`, `session-moved`, `session-renamed`, `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived`, `inbox-opened`, `inbox-closed`, `inbox-snoozed`, `inbox-dismissed`, `inbox-claimed` (and the `context-record`, Job, Attempt, PR, Escalation and Plan review kinds of `tui/src/tree/kinds.rs`: every Event that moves a row the snapshot reads) | Contexts, their Plans' Lead Sessions, then the Sessions forked by `session fork` as siblings under their Context (B1), each with its label (an id, §13.2 "Labels"; the dynamic `title` of a Session is never drawn in the tree, hs-d257), state, place, `archived_at` (archived objects are in the read model), Job counters (Plan Leads only) and **dot** (rule below). Inside the archive row, after the archived Plans and forks: every archived Manager Session as a muted row (colour 8, fg 8), named `<context>-manager`, with its own dot (rule below) drawn in colour 8, inside each Context's block within the archive row. The Context row's own dot comes only from its live Manager. The **Inbox section** (§13.2) shows the open, visible items (snoozed and dismissed ones are hidden; `engine-down` included, when no live Manager exists), in its order, with each item's `kind`, `urgency`, `label`, `session`, `plan`, `event`, `url` and `opened_at`; and each Session's state and place for the glyphs the Inbox does not give (the spinner, `󰽤`). The Inbox, not the Sessions' state, says who needs the User, so the snapshot no longer reads the Plan's pending review or its pull requests' checks for the dots. Two changes of the Inbox have no Event: a snooze ends by the clock, so the tree also re-reads when the earliest snooze of an open item ends (`snoozed_until`, read with each snapshot), and the engine stops or starts (`engine-down`, never stored), so with each poll of the Events it looks at the engine's lock (§1.4) and re-reads when that changed. It re-reads on no timer. It redraws once per 50 ms frame only while a spinner is on screen (§17 redraw rule), and every second while the Inbox section shows items (an item's age, from `opened_at`). Also read, and written: `run/tui/tree-folds.json`, the Contexts' folds and the archive row's fold, read once at the view's start and rewritten when a fold changes (§13.2, hs-d260), the view's own state |
| `plan` (centre top) | `cpd plan show --plan <p>` for the Plan of the Lead in the chat slot (its `prs`, each PR's key, repo, base, needs, status, `pr`, `pull_request` (its `state`, for a closed pull request) and `jobs`, for the PR boxes, §13.2); when a Manager is in the chat slot, the Context's Plans read through `read::tree()` with each Plan's Lead SessionNode's dot, Job counts, done_at and archived_at; the activity Events of the window, each with its kind of activity for the bar's colour (§13.2 Kinds), and each Plan's latest activity Event; the Attempts not ended or ended inside the window | every Event with `plan = p` (the `pr-*` kinds and `pr-feedback` included: they change a box's title or add a box); `session-moved` with `place: "chat"` (the slot changed: switch Plan and take a new snapshot); when a Manager is in the slot, every Event of the Manager's Context and `session-moved` | one box per PR that is not `cancelled`, top to bottom by needs and linked by them, the merged ones first and the others below them, each with the flowchart of its Jobs, Attempts and Tiers (R34, A15); with a Manager, Pulse (one row per Plan of the Context). Also read: the Plan's latest Plan review (its status and number)  and each pull request's `url`, for the `󱆭` links (§13.2). Also read: each pull request's `mergeable` and whether its PR has an update Job not ended (`jobs.kind = 'update'`, `queued` or `running`) with its status, for the `conflict`/`updating` title (§13.2, hs-d532), and the Plan's Lead Session's `state`, for the working dot beside the state word (§13.2; hs-d335; the re-read at least every 2 s picks up a change of `state`; see Rates for the redraw rule). A conflict clears with no Event (only becoming `conflicting` appends `pr-feedback`), so the view also re-reads its snapshot when a `pull_requests` row changed (see Rates), with the same coalescing, and the Plan view on no timer (Pulse: see Rates). Not drawn, so not read: open Escalations. Also read, and written: `run/tui/plan-scroll.json`, the Plans' scroll offsets, read once at the view's start and rewritten when an offset changes or the Plan changes (§13.2, hs-d254), the view's own state. |
| `widgets` (Deploy column) | the `open` rows of `pull_requests` with `authored` 1 (contract §4.4), with their `mergeable` and, for an owned one, its PR's update Job not ended and its status (the ` conflict`/` updating` flag, §13.2, hs-d532) and `merge_state` (the merge icon, §13.2) and the PRs of the chat slot's Plan (`prs.pull_request`; the Lead Session's place is `session-moved` `chat`); each open pull request's `url` and its owner's Lead (`prs.pull_request` → `plans.lead_session`) with that Lead's `state` (the row's working dot is shown while it is `working`, §13.2; hs-d335); and the host's CPU %, one utilization % per GPU and memory % (`/proc/stat`, `nvidia-smi --query-gpu=index,utilization.gpu --format=csv,noheader,nounits -lms 2000`, one long-running process for the view's life and `/proc/meminfo`; on macOS `ps -A -o %cpu=`, `sysctl -n hw.ncpu`, `ioreg -r -d 1 -w 0 -c IOAccelerator`, `vm_stat` and `sysctl -n hw.memsize`), sampled every 2 s with no Event, the CPU over the time since the last sample (it needs no read of the store) | `pr-opened`, `plan-created`, `session-registered`, `session-moved`, `job-added`, `job-started`, `job-merged`, `job-failed`, `job-cancelled` (of any kind: the update Job's status, hs-d532; the view's filter must cover them, and the fix is in scope if it does not), and a change of a `pull_requests` row (see Rates): pull requests no Plan owns append no Event (hs-d96), so the poll's changes reach the column by that, not by a timer | the machine's name, CPU %, GPU % and memory %, and the open pull requests with their links and owners. While a shown pull request's Lead works the view redraws every 50 ms for the spinner, from data already read, without a read (the re-read stays at least every 2 s, which picks up the Lead's `state`). No engine, slots, Escalations or Events |
| the StatusBar (tmux's status line; no view, no pane) | the slots, config `concurrency` and the queue: the Attempts not ended, `concurrency`, the Ready Jobs and the `queued` Jobs, read **by the engine** (§12), not by a view | the engine's tick (`watch.tick_seconds`): it runs `tmux set -g @cpd_workers '<text>'` (and `@cpd_workers_short`, `#{w:@cpd_workers}`, `#{w:@cpd_workers_short}`, `@cpd_version`) only when a value changed | `workers N/M · W waiting · Q queued`, or `N/M·W·Q` on a narrow client (User, hs-d198). **A deliberate deviation** from "no tmux option carries state" (User, hs-d149): the options hold display strings and a width, up to one tick late and stale while the engine is stopped; nothing reads them back |
| `logs` (popup of the StatusBar's `󰧭` button, via `cpd tui action logs`) | the latest 500 Events (the `events list` default), oldest first | every Event: it follows live while the view shows the bottom (R36) | the Event log |
| `history` (chat slot, an archived Session) | the Session's file, named by `cpd session history <id>` (its `sessions.file`) | none: the view is static and re-reads nothing | the Session's pi file itself, read-only, once |
| `chat` (placeholder) | none | none | the static text `No Lead here: pick a Plan in the session tree` |

**A view never writes to its terminal outside its frame** (hs-d363). The `tree` printed `engine was down: …` with `eprintln!` into its own terminal and broke the drawn frame. A view's lines go to its log file `$CPD_HOME/run/tui/<view>.log`, appended, one timestamped line each; what the User must see or confirm is a dialog, below. No `eprintln!`, `eprint!`, `print!` or `println!` appears in the view modules (`tui/src/`, except the code that runs before or after the alternate screen, such as `cockpit.rs`, `cpd tui`'s own CLI output, and the view runner's restart message printed after it leaves the alternate screen); a test in the style of `palette::tests::no_colour_outside_the_palette` pins it.

**No notes** (User, hs-d469, 2026-10-20; decided work, hs-d301; replaces hs-d364 (2)'s "short note" and en-impl2's in-frame notes). No view shows a transient note line: not `already in focus`, `copied <url>: …`, `engine down 60 s+: Managers told`, `fold state reset: …`, `scroll positions reset: …`, `no review page yet`, nor the result of an action. It applies to every view: the tree and its Inbox, the Plan view, the Deploy column, and the Worker and history views.
- **What the User must see or confirm is a dialog**: the dialog kit's popup, `cpd tui view dialog <confirm|input|text|checklist|select> …` (§13.2 "The dialog kit").
- **Otherwise nothing shows**, and the details go to the view's log `$CPD_HOME/run/tui/<view>.log` (hs-d364).

**Frame trace** (hs-d588; opt-in). The `tree` and `plan` views write a frame trace to their log file `$CPD_HOME/run/tui/<view>.log` when the environment variable `CPD_TUI_TRACE` is exactly `1` (read once at the view's start; unset, empty, `0`, `true`, anything else: off, and an off trace costs nothing, writes no line and touches no file). Each draw of the view's main loop appends ONE line after the draw, its own cost not counted in any field. The line is exactly `trace gap_us=<n> read_us=<n> layout_us=<n> draw_us=<n> frame=<n>`, every `<n>` a non-negative integer, all fields required:
  - `gap_us`: microseconds from the START of the previous draw to the START of this one; 0 on the view's first draw.
  - `read_us`: microseconds this view's loop thread spent inside reads (store snapshots, the Event follower, the open pull request read, the engine lock probe, the merge marker read) since the PREVIOUS draw ended; then the sum starts again at 0.
  - `layout_us`: microseconds spent laying out the content the draw shows, apart from drawing it, since the previous draw ended, summed like `read_us`: the plan view's `boxes::layout` (at a snapshot and when the width changes, per R2); 0 in the tree view, whose rows are built by the draw itself.
  - `draw_us`: microseconds the draw itself took (`terminal.draw`, which builds the tree's rows and renders both views).
  - `frame`: the spinner frame the draw used (`palette::frame`, 0..7).
The line is prefixed with an RFC 3339 time by the existing view log mechanism (commands.md §17 "No notes"). Why it exists: while a spinner is on screen the loaders must advance every 50 ms (hs-d616), so `gap_us` near 50000 is healthy only while an animation is on screen, since a view with none now idles between signals; a gap that is longer, with the cause in `read_us`, `layout_us` or `draw_us`, is a stutter.
- **(a) A failure is something the User must see** (Lead's decision, which the User may correct): a click or key whose command fails (for example `session open` refused) opens a `text` dialog with `<what failed>: <code>: <message>`, and a log line too. A success says nothing. Clicking an Inbox item already in focus does nothing and says nothing.
- **(b) A view that cannot read its snapshot** (the store unreadable) is a lasting state of the view, not a transient note: it keeps its error line. So do the lasting texts of a view with nothing to show (`no Plans yet`, `no Lead session in the centre`).
- **(c) `palette::note()` and `Ui::Note` stay only while something else uses them** (the chat's tool lines use `Ui::Tool`); otherwise they are deleted.

**Rates.**
- A view learns of a commit from a **file watcher on the store's directory**: it signals on a data write, a create or a remove of `cpd.db` or `cpd.db-wal` and nothing else; `cpd.db-shm`, `config.toml`, `locks/` and `run/` do not signal, nor do access events, so a view's own reads cause no signal. A **safety read** starts when 2 s have passed since the last read started, for the facts the store does not announce: the engine's lock, the end of a snooze, the merge marker.
- A watcher that cannot start is shown in the view's error row as `no commit watcher: <reason>; polling every <events.poll_ms> ms` and the view polls at `events.poll_ms`, never silently; `events.poll_ms` (default 200 ms) is the fallback period of the safety read and the period of `cpd events follow`.
- A **read** is the follow poll (the Events since the cursor and the `pull_requests` mark) and, when it found a change, the snapshot, in one job on the reader thread, at most one per view per 50 ms frame: a commit that arrives while a read is out, or within the frame, is one read at the frame's end. Nothing reads when nothing committed except the safety read.
- A read result is drawn on arrival; only animation frames stay aligned to the 50 ms boundaries. A view with no animation, no age text on screen and no commit wakes only for a signal and the safety read.
- On an Event of a kind it shows, a view re-runs its snapshot (it never applies an Event's `data` itself), coalesced to at most one snapshot per view per 50 ms frame, whatever the Event rate.
- **Pulse** re-reads every 60 s (the window moves and ages tick; a running Attempt is not an Event), and on pane resize (the bucket count changes), coalesced as other views.
- The **Dispatch column** follows `session-state` Events (a Lead's dot is a fact it draws), and its `Follow` reads on the reader thread like the others. Each view follows Events with no selection and filters by kind itself, through `cpd events follow` or the same poll in process (§5.4): one `PRAGMA data_version` read per `events.poll_ms`, which costs no I/O while nothing changes (§5.4). The follow also reads `count(*)` and `max(changed_at)` of `pull_requests` with each poll of the Events to detect row changes (the poll sets `changed_at` with every change, contract §4.4), and says the rows moved when they differ from the last poll (the first poll says so).
- A view draws only when its model, its error, its size (a resize is an input event), its input or a clock changed, never on a bare tick. While a spinner is on screen the view draws once per 50 ms frame; with no spinner it draws only on change (§17 redraw rule). The spinners: a working dot in the tree, including the red loader (hs-d581), a turning failed pill in the plan view (hs-d586), or in Pulse, a running Job's pill in the plan view; the tree redraws every second while its Inbox section shows an item's age, and the Dispatch column when the CPU, GPU or memory % it shows changed (sampled every 2 s; on Linux the GPUs come from one long-running `nvidia-smi -lms 2000`, so no tool is spawned per sample).
- Clocks such as "running 4 m" are redrawn every second from timestamps already read, without a read. The spinner (`` on the tree's dots, on a `running` Job's pill, on a turning failed pill with the moon loader (hs-d586), beside the state of a PR whose Lead works on the Deploy row and in the Plan view's box title) is redrawn every 50 ms, while one is on screen, from data already read, without a read.
- If a follower ends, the view takes a new snapshot and follows again from its `events_cursor`.

| View | Clocks needed besides a signal | Safety read covers |
|---|---|---|
| tree | the next whole second while the Inbox shows items (their age text); the next 50 ms frame while a spinner is on screen (a working or fixing Lead, a merging Inbox item) | the engine's lock (`engine-down`), the end of a snooze, the merge marker |
| plan | the next 50 ms frame while a spinner is on screen (a running Job's pill, a failed one's moon loader while the Lead works, a PR box title's Lead dot); Pulse's 60 s refresh when Pulse shows; no per-second clock (Pulse's ages are in minutes) | (covered by the 50 ms and 60 s frame rates) |
| widgets | the next 50 ms frame while a Lead works or a clean PR's merge runs; the 2 s sample of CPU, GPU and memory | the merge marker |

A view with no clocks on screen wakes only for a signal and the safety read.

**Blue** (User, hs-d351, hs-d352, 2026-10-20; decided work, hs-d301; amended pro-d58). **Blue means focus and selection**, always colour 4, named once in `tui/src/palette.rs`; the whole Dango palette is contract §6.1.1 (hs-d534). It has these uses and nothing else is blue: the focused pane's border and title (`pane-active-border-style fg=blue`, §6.1); **the selected row** (hs-d351), the Context tree's selected Context or Plan name, which draws in blue while nothing else of the row changes, beats the muted colour of archived and asleep rows, and has no chat-slot marker (§13.2); and every cursor and selection, which is a **blue block** (colour 4 inverted: text in the terminal's background colour) in place of reverse video (hs-d534): the tree's and the Inbox's cursor, the selected Job pill, the active PR row, the selected entry of a menu, the default dialog button, the dialog input's cursor, the Logs dialog's opened-at line, tmux's menus and copy mode. **Pulse's escalation bar is no longer blue**: hs-d352 made an escalation a focus change and drew it blue; hs-d534 replaces that, an escalation is agent activity and its bar is cyan (§13.2 Pulse). Later uses of focus and selection take the same blue. **The Inbox has a new blue tier** (pro-d58): external support for `review-requested` items, palette 4 (base blue) and 12 (bright blue), glyph U+F0368, fills no Session dot (§17 "Which items fill which row", contract §4.4), between cyan and magenta in order. **This amended the earlier rule**: hs-d570 drew the notice tier blue, pro-d16 renamed it cyan and said "blue means focus and selection only"; pro-d58 adds the blue tier for external support, keeping focus use of blue in the interface.

**Collapsible blocks** (User, hs-d337, hs-d339, hs-d344, hs-d349, 2026-10-20; Plan pr-links; replaces hs-d309's `┌` / `└` shapes). Every block that folds has one look, drawn by **one shared widget** (`tui/src/fold.rs`), not one copy per view:
- **Rows:** the title row is `╭` + the row's glyph + the name, for example `╭󰽢 building-cpd`; each content row is `├─` + its glyph + its text, straight against the branch with **no indent**, for example `├─󰽢 cross-repo       3⋅1⋅0⋅0⋅92`, the counters right-aligned. (Where content has no glyph of its own, the content row is `│ ` and the row; the Inbox's rows put their tier glyph there (hs-d483): `│ \u{F0362} cpd#129 merge conflict       18m`, the escape standing for the glyph; pi's are the same, `│ `.) **Right edge:** every row follows the rail rule (contract §6.1.1 "The rail fold"): column 0 blank, rail at column 1, content from column 3 to column `W - 4`, columns `W - 3` to `W - 1` blank; text wrapped or cut with `…` and right-aligned elements (the counters, the Inbox's age) end at column `W - 4`. Any row the widget wraps keeps its own glyphs and colours (hs-d345); the fold only adds lines (colour 8).
- **Closing row** (hs-d349; replaces the word-only `╰─ expand` / `╰─ collapse`): **folded**, `╰󰬦 expand` (U+F0B26 md-chevron_down_circle); **expanded**, `╰󰬬 collapse` (U+F0B2C md-chevron_up_circle); **one** closing row per block (a pi tool row has five forms, `expand`, `collapse`, loader, terminator and `choose` for `ask_user_question`: contract §6.9 "The five closing rows"). A block that cannot fold and has no row to close it (a Context with no rows at all; the Inbox with 3 items or fewer) ends with the terminator ` ╰󰬪` (U+F0B2A md-chevron_right_circle, colour 8, no word, no toggle, hs-d574, replacing hs-d477's and hs-d459's plain `╰─`); a **LIVE** Context with no rows instead ends with ` ╰󰬪 new` — the muted `╰` and glyph and the word `new` in colour 15 (clickable text) — and a left click on that row opens the New Plan input popup for that Context (the same action as `n` and the context menu's New Plan...). An **ARCHIVED** Context with no rows keeps the plain muted terminator, and the Inbox's closing rows are unchanged (the terminator ` ╰󰬪` and `╰󰬪 up to date, drink some water`). `plan new` is refused in an archived Context (`archived`), which is why the row is absent there. A tree Context block with **nothing to fold** (no archived rows, hs-d477) has no closing row: its last row's branch is `╰─` instead of `├─`, for example `╭󰽤 homeserver` / `├─󰽤 b2-budget …` / `╰─󰽧 speedtest …`. The `╰─` of a last row's branch (hs-d477) is a content row's branch, not a closing row, and is unchanged.
- **What the toggle hides** (hs-d344): in the tree's Context blocks **only the archived rows**. Live Plans and Sessions always show. Folded: the live rows, then `╰󰬦 expand`. Expanded: the live rows, then the archived rows in colour 8 (hs-d339, hs-d534), then `╰󰬬 collapse`. A Context with no archived rows has no closing row: its last row is `╰─` (hs-d477). In the Inbox's block the toggle hides the rows past the first 3 (§13.2).
- **Muted vs not** (contract §6.1.1, hs-d534): the lines (`╭`, `├─`, `│`, `╰`) are muted, colour 8 (`palette::fg(Meaning::Nothing)`, or pi's rail colour in the Dango theme); the arrows and the words `expand` / `collapse` are the toggle, colour 7; the content is not muted.
- **Spacing** (hs-d476; a box's margin and padding are the Box rule, contract §6.1.1 "Boxes", hs-d578; `tui/src/layout_tests/` pins the spacing): a collapsible section widget (title row, content rows, closing row; `tui/src/fold.rs`) has a margin of **one empty row above and one below**; its body has no vertical spacing.
- **Toggling:** only the closing row toggles; the title row keeps its own action (opening the Manager or the Session, for example).
- **Where it applies:** the tree's Contexts, with their archived Plans and Sessions inside the block (hs-d339), the Inbox's fold (§13.2), cpd's messages in pi (§15.9; `╭⦁ title`; pi's own rendering is not changed by hs-d349 and keeps hs-d339's closing row until a Job says otherwise), and any popup or Plan view panel that folds (none does yet). **Manager's assumption** (the User may restrict it): the arrow toggle is the same for **every** fold block, the tree's Context blocks included, from the one shared helper. It replaces the Inbox's `+<n> more` / `− less`, the archived `󰇘` row toggle and hs-d309's `┌` / `└` shapes.

**Section titles** (User, hs-d298, hs-d308, hs-d337, hs-d346, hs-d349, hs-d475 to hs-d478, hs-d480; **section look v4**). Every section title inside a pane (the tree's Inbox, the Deploy column, the Plan view's in-pane titles if any, logs, legend) is drawn by **one helper**, `tui/src/section.rs`, as a **pill** on a line, in **one of two forms** (hs-d478) that put the pill's left cap in the same place. One rule (hs-d475, hs-d480), the User's example `Section Or Panel Title`:
- the title is its text in **Title Case, every word capitalised**, with **no padding spaces** inside the pill, **a block between the rounded caps** U+E0B6 `` (left) and U+E0B4 `` (right), and **never clickable**;
- **in-pane titles are always a read-only title pill** (hs-d534, which replaces hs-d530's 8 and 7, hs-d349's unmuted pill and the reversed muted block): colour 7 inverted: text in the terminal's background colour, caps in colour 7 as foreground, from `palette::pill_static()`;
- **every title's left cap sits three columns from its container's edge** (hs-d478): the one column of padding, two cells of lead (`╭─` or `──`), then the cap U+E0B6;
- **a title that heads a collapsible widget** (the Inbox's) is the **collapsible form**, drawn by the helper's `collapsible_title`: `╭─`, then the pill, then a muted `─` line to the right edge, ending with the optional **count as a pill** (hs-d480, replacing hs-d475's plain number; hs-d298: the Inbox's and the Deploy column's PRs title carry one), the same colours as the title (colour 7 inverted: text in the terminal's background colour, caps 7), with no padding spaces, and then the padding: ` ╭─Inbox────…────35 `;
- **a title that is not collapsible** (logs, legend, the Deploy column's sections such as PRs) is the **plain form**, drawn by the helper's `title`: the padding, a muted `──`, then the pill, so its cap lines up with the collapsible form's: ` ──Logs────…────`, ` ──PRs───────3 `, ` ──Section Or Panel Title───`. **Manager's assumption** (hs-d478: the User may correct it): the `──` lead for the non-collapsible titles; the User's rule is only that the caps line up;
- **content rows** are `│ ` and then the row, with its own icon and colours (the Inbox's, hs-d345); the **closing row** is the fold's (`╰󰬦 expand` or `╰󰬬 collapse`, above);
- the lines are muted (colour 8), the toggle's arrow and words are colour 7, the pill is a read-only title pill (colour 7 inverted: text in the terminal's background colour);
- **spacing** (hs-d476, replacing hs-d448's; a box's margin and padding are the Box rule, contract §6.1.1 "Boxes", hs-d578): every **panel** has **one column of padding on the left and on the right**, and its titles sit inside it, so a title row **starts at column 1 and ends one column before the right edge**; a **collapsible section widget** (title row, content rows, closing row; `tui/src/fold.rs`) has a margin of **one empty row above and one below**; a **section title outside a collapsible widget** has one empty row above and one below; a section's **body has no vertical spacing**;
- **Manager's assumption** (the User may restrict it): the same title pill for **every** in-pane section title, not only the Inbox's. It replaces hs-d337's `┯━━ Inbox ━━ 32 ━━` and the heavy `━` titles.
- Not covered: the tmux pane titles (`───Context…`; Context, Plan, Deploy) and the popups' tmux titles (` legend `, ` logs `) are Plan tui-visuals', unchanged here; the dialog kit's title is the frame's pill (§13.2, hs-d578), not a section title.

**Scroll cues** (User, hs-d348). `▲ <n> more` and `▼ <n> more` are cpd's one scroll indicator, **right-aligned** at the end of their row everywhere a region scrolls (the tree, the Inbox, the Deploy column, the Plan view, popups), so a cue never cuts a line drawn on the left. They are drawn by **one shared helper** in the scroll kit, `tui/src/dialog/scroll.rs` (the views' one scroll module), not by `tui/src/tree/inbox.rs`, `tui/src/widgets/column.rs` or the tree's list, which used to draw a left-aligned ` ▲ <n> more` of their own.

**The age text rule** (hs-d341). One rule for all surfaces: `pub fn age_text(seconds: i64) -> String` in `tui/src/read/age.rs`. Under 60 s `Ns`, under 3600 s (1 hour) `Nm`, under 86400 s (1 day) `Nh`, else `Nd`; a negative age is `0s`. The tree's Inbox shows each item's age from `opened_at`; Pulse shows each Plan's age from its latest activity Event, calling `age_text(minutes * 60)` to convert.

**The PR state word rule** (hs-d341). One rule for all surfaces: `pub struct PrLook { pub state: (&'static str, Meaning), pub conflict: Option<(&'static str, Meaning)> }` and `pub fn pr_look(facts: &PrFacts) -> PrLook` in `tui/src/read/pr_look.rs`. The `state` word is: `draft` in `Meaning::Planning` when the PR is open and a draft; `open` in `palette::pr_open(clean)` when open and not a draft, yellow only when ready to merge (`merge_state = 'clean'`), else colourless; `closed` in `palette::PR_CLOSED_ASKED` (red) when closed and the Lead asked to close it; else the word and `palette::pr_state(state)` (`merged` green, or `closed` cyan if still being handled). The `conflict` word is `Some` when the PR is open and `conflicting`: `updating` in cyan when an update Job runs or is queued, else `conflict` in cyan (the Lead fixes it); `None` otherwise. The plan view's PR box title shows `conflict` when present, else `state` and now also `draft`; the Dispatch row shows `state` and appends `conflict` after review words when present.

**The tree's dot rule.** One loader for pi and the cockpit (hs-d616, a trial; replaces hs-d579 and the eight nf-md moons at 100 ms): the tree's working dots and a running Job's pill use the 28 moon_alt phases described in the table, one frame every 50 ms, on one shared clock, so every spinner turns together. History: eight nf-md moons at 100 ms (hs-d579), replaced by the 28 moon_alt phases at 50 ms (hs-d616, a trial: the User keeps or reverts it after reading docs/research/moon-loader-cost.md). The static dots follow: needs-you is moon_alt_new (U+E3E3, frame 0), idle and stopped are moon_alt_full (U+E3D5, frame 14), waiting is U+F1853 (unchanged). The filled dot means the Session needs you and the hollow dot means nothing is needed; in moon_alt the new moon U+E3E3 is the filled disc and the full moon U+E3D5 the hollow ring (reverse of the earlier nf-md family), hs-d620; a non-turning loader glyph (an archived working or fixing dot, a running Job's still icon) is the hollow U+E3D5. The filled needs-you glyph comes **only from open, visible Inbox items** and shows on the tree's rows only; the Inbox's own rows draw their tier glyphs instead (hs-d483) (hs-d165, hs-d167: the Inbox is the single source of "needs you"; it replaces the per-case rules the tree derived from Session states, Plan phases, Plan reviews and pull request checks, and cpd 1's `@asking`, `@settled_at` and `@seen_at` pane options). The other glyphs are derived from the read model with no write. Nothing is written by the tree.

**The Lead dot rule** (hs-d581, hs-d622, hs-d624). The plan view's PR box title and the Dispatch row each show the Plan's Lead's working state, its Plan's phase and failed Job state as one dot: `Dot::working(failed: bool, phase: Option<Phase>) -> Dot` returns `Dot::Working(Some(phase))` (the 28-frame spinner on the shared clock in the Plan's phase colour: grounding grey 15, plan magenta 13, dispatch cyan 14, deploy yellow 11, done green 10; working is agent activity, contract §6.1.1) when `failed` is false and the Lead has a Plan, `Dot::Working(None)` (cyan, the 28-frame spinner on the shared clock) when `failed` is false and no Plan, and `Dot::Fixing` (the red loader, the 28-frame spinner in red while working) when true. `Dot::of(state: State, item: Option<Urgency>, failed: bool, phase: Option<Phase>) -> Dot` returns the appropriate Dot for a Session: when `state = 'working'` it calls `Dot::working(failed, phase)`, else the Dot from the item's urgency or the static glyph for the state. The tree computes and shows this same rule for every Lead's dot; the plan box and the Dispatch row use the same derived `failed` boolean from the Plan's Job counts: whether the Plan has a Job with status `failed`.

| Glyph | Meaning | Style |
|---|---|---|
| `` U+E3E3 (moon_alt_new) | the row has an open, visible Inbox item (below) | the colour of its most urgent item, bright (the row is clickable; contract §6.1.1): red 9, yellow 11, magenta 13, cyan 14, white 15 |
| `` U+E3E3, U+E3C8, U+E3C9, U+E3CA, U+E3CB, U+E3CC, U+E3CD, U+E3CE, U+E3CF, U+E3D0, U+E3D1, U+E3D2, U+E3D3, U+E3D4, U+E3D5, U+E3D6, U+E3D7, U+E3D8, U+E3D9, U+E3DA, U+E3DB, U+E3DC, U+E3DD, U+E3DE, U+E3DF, U+E3E0, U+E3E1, U+E3E2 (moon_alt_new, moon_alt_waxing_crescent_1..6, moon_alt_first_quarter, moon_alt_waxing_gibbous_1..6, moon_alt_full, moon_alt_waning_gibbous_1..6, moon_alt_third_quarter, moon_alt_waning_crescent_1..6; the 28 moon_alt phases as described in section 17) | the Lead is working (`state = working`); 28 frames, one every 50 ms, looping, on one shared clock (`frame = (unix ms / 50) % 28`), so every spinner turns together; red 9 while its Plan has a failed Job (hs-d581), else the bright pair of its Plan's phase colour: grounding grey 15, plan magenta 13, dispatch cyan 14, deploy yellow 11, done green 10 (hs-d622, hs-d624) | the bright pair of the Plan's phase colour in the tree (working is agent activity, contract §6.1.1, hs-d534; hs-d622, hs-d624); red 9 only when the Plan has a failed Job (hs-d581) |
| `󱡓` U+F1853 (nf-md-circle_opacity, swapped by hs-d582) | the Session waits for another agent, not for the User (`state = waiting`: a Worker for its Lead's answer, a Lead for a Worker's question, hs-d303); not a filled dot, so never one of the four colours | `Meaning::Waiting`, cyan (`Color::Cyan`; not blue, which means focus only, §17 "Blue"; the User's decision of 2026-10-20, resolving hs-d303 against hs-d352; bright 14 in the tree, contract §6.1.1) |
| `` U+E3D5 (moon_alt_full) | idle, nothing needed | colour 15 (clickable colourless text, contract §6.1.1) |
| `` U+E3D5 | pi stopped (`state = stopped`) | colour 8 (it was dim) |

| `󰽢` colour | Which item (contract §2.2 gives each kind its urgency) |
|---|---|
| red | the most urgent item is red: `dialog` while its Plan is in `dispatch` or `deploy`, `job-stuck` |
| yellow | yellow: `pr-ready`, `review-requested` |
| magenta | magenta: `dialog` in `grounding`, `plan`, `done` or with no Plan, `plan-review` |
| cyan | cyan: `curate-retire`, `curate-docs` (cpd notices, hs-d549, hs-d551; pro-d16) |
| white | white: `unread` (contract §2.2 gives its rule: a finished turn that answered the User, hs-d278; this table does not define it) |

**Which items fill which row.** A **Session** row's `󰽢` is the most urgent open, visible item **linked to that Session** (`inbox_items.session`): `dialog` and `unread` for any Lead, fork or Manager; and, for a **Plan's own Lead** (`plan_lead`, never a fork), also the items of its Plan: `plan-review`, `job-stuck` and the `pr-ready` items of the Plan's pull requests (an item whose `plan` is the Plan). A **Context** row's `󰽢` comes **only from its Manager Session's own items** (`dialog` and `unread` linked to the Manager; hs-d215, hs-d217): it is not the colour of the Context's most urgent Session, and the Context's other items (`job-stuck`, `plan-review`, `question`, `pr-*`, and the items of its Leads and forks) **do not colour the Context row**; they show only in the Inbox section (§13.2). `engine-down` is linked to no Session, so it fills no dot. A hidden (snoozed or dismissed) item fills no dot, and when it closes (its cause ends) the dot falls back to the glyph rules below at once. **Own state first** (hs-d183): a `working` Session shows the spinner whatever its items; a Plan's own Lead that is working while its Plan has a failed Job shows the loader in red (hs-d581), and only in that case its own open visible red Inbox item keeps the static red `󰽢` (precedence); a working Lead with no failed Job shows the cyan spinner; any other state shows its most urgent item's `󰽢`, else `󰽡` in the waiting colour (`waiting` with no item) or `󰽤` (idle or blocked with no item) or `󰽤` in colour 8 (stopped). **A Context row** (hs-d215) is its Manager Session's dot by the same rule, the Session with `role = 'manager'` in that Context, not archived (one per Context, created the first time the Context row is opened, hs-d154); a Context with no Manager yet, or only an archived one, shows `󰽤` (stopped, colour 8). It is no longer the most urgent Session of the Context. **Archived rows** keep their glyph, drawn in colour 8 (`Meaning::Nothing`; asleep rows in 7, contract §6.1.1), never animated. **Archived Manager rows** appear inside their Context's collapsible archived section, after the archived Plans and forks, each muted (colour 8), named `<context>-manager`, with its own dot (rule below, hs-d630, hs-d633).

**Row ordering (hs-d686, pro-d48).** At both levels of the tree (Contexts, and within each Context live Plans and forks), pro-d48 replaces hs-d686's last-activity second key with creation, newest first. Rows are ordered first by dot-urgency tier, then by creation (`created_at` descending, newest first).

- **Tier:** red (anko) 0 > yellow 1 > magenta 2 > cyan 3 > shiro 4. Shiro is the white Inbox dot (hs-d483) and every row with no Inbox item: a working row (the spinner), an idle row, a waiting row, a stopped row, and the red loader. Archived rows are last, whatever their dot.
- **Within a tier:** the row's creation key, newest first. For a Plan row it is `plans.created_at`, for a fork row `sessions.created_at` (the Session's own creation, assumption A1 of the Decision), and for a Context row `contexts.created_at`. Events never reorder: a newer Event never moves a row. A tie (same tier and same created_at) keeps the read model's creation order (assumption A2, stable sort): Contexts by id; Plans before forks, each as the read model lists them (oldest first). So a hand-built tree whose rows all share one created_at keeps the order it was built in.
- **Level 1, Contexts:** a Context row's tier is its live (not archived) Manager's dot; `shiro`/white with no live Manager. Contexts are ordered by their tier and creation, newest first.
- **Level 2, within a Context:** its live Plans and forks, one list (a fork can outrank a Plan, since they share the same ordering rule). A Plan row's tier is its Lead Session's dot; a fork row's tier is the fork Session's dot. Both are ordered together by tier and creation, newest first, live rows above archived ones; archived Plans, then stray forks, then archived Managers, are grouped in that order (hs-d339), muted (colour 8), each group in creation order. A Manager never has a live row (only archived Managers within the Context block).

**Precedence.** A row shows its state's spinner when working, else the most urgent item's `󰽢` when it has one, else its state's glyph; the order of the dots is `red > yellow > magenta > cyan > white > red loader > working > waiting > idle > stopped`. Archived rows take no Inbox item.

**Actions.** The views change state only through commands: selecting a Lead runs `cpd session open <id>`; a click on a Context row runs `cpd context manager --context <c>` first (§13.2); the new-Plan key runs `cpd plan new …` after its `input` popup; the close key runs `cpd session close <id>` after its `confirm` popup; each item of the tree's right-click menu runs the command §13.2 names, after its dialog popup where it has one (the tree opens `cpd tui view dialog …` with `display-popup -E`, reads the result file, then runs the command; hs-d295, hs-d297); a click on an owned PR row of the Deploy column runs `cpd session open <lead>`, then `select-pane` on the chat slot; a click on its merge icon opens the `merge` popup (`cpd tui view merge <owner>/<repo>#<n> --client <name>`), whose confirm closes it and starts `cpd tui action merge <owner>/<repo>#<n> --client <name>` with `run-shell -b`, which runs `cpd pr merge <owner>/<repo>#<n>` (§9.10) and, once it succeeds, `cpd pr poll` (§9.2; hs-d312, hs-d473) in the background, and opens the failure dialog (§17 "No notes") when the merge fails (§13.2); a right click on a PR row opens its menu (hs-d296): **Merge** (only with the merge icon) opens that same `merge` popup, **Copy Link** runs `tmux set-buffer -w -- <url>` (no `cpd` command), **Go to Lead** (only on an owned PR) runs `cpd session open <lead>`, then `select-pane` on the chat slot (§13.2); parking runs `cpd session park`; the dialog popups, like the legend, logs and `merge` popups, are the views' own layout, not Ledger state: they hold no state and write only their result file. The `plan` view also writes its own scroll offsets to `run/tui/plan-scroll.json`, and the `tree` view its folds to `run/tui/tree-folds.json` (§13.2), the views' own state, not Ledger state. The StatusBar's clicks run `cpd tui action` (§13.2): `󰨝` click click resets the layout (`resize-pane`, the view's own layout, not Ledger state), its legend button opens the legend popup and its `󰧭` button the logs popup (§13.2). A click on an Inbox item runs `cpd session open <session>` (then `select-pane`, as a Session row; for a `plan-review` item, the Plan's Lead), opens the logs popup at the item's Event, or copies the item's `url` (`review-requested`, a `pr-*` no Plan owns); the OSC 8 link stays for the terminal's modifier-click. The tree and the Inbox list each scroll with the wheel inside the rows the column gives them, two thirds and one third (§13.2, hs-d307); `s` and `d` and the right-click menu's entries run `cpd inbox snooze` and `cpd inbox dismiss` (§21).

Change from the spike and cpd 1's cockpit: the tree no longer discovers Sessions from tmux pane options (`@session_id`, `@agent_state`) or `leads/<id>/workflow`, and the plan view no longer reads `/tmp/cpd-tui/cache/`; both read the database through commands.

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
| `attempt submit` runs the Gate (`wt hook pre-merge`, exit 64) | runs the quick check, `pre-commit` only (none when the repo has no `pre-commit`); refusals `check_failed`, `check_error` | §7.3, R20 |
| the Gate runs at every merge, on the rebased Attempt and again on the Lead | the Gate runs only at `pr open` and `pr rebase`, before the push; refusals `gate_failed`, `gate_error` | §7.4, §7.6, R19 |
| a Reviewer per Attempt; `review decide`; Job status `in_review`; Attempt statuses `reviewing`, `awaiting_lead`; outcome `approved` | removed: a submitted Attempt goes straight to `merging`; the Job stays `running`; outcome `merged`; the review is the PR's own, on GitHub | §3.2, §3.4, R19 |
| `watch --wf` per Workflow | `watch`, `watch --ensure`, one per machine | D5 |
| Lead queue files, `herdr:blocked` emitted to herdr | Events; `session state` | D4 |
| — | new: `context …`, `session list/show/fork/open/park/state/run`, `events list/follow`, `tui`, `config` | the model and the tmux host |
| `doctor` checks node, `bd`, `wt`, `herdr` | checks git, tmux, pi, gh with minimum versions | §1.1 |

## 19. Amendments to contract.md

Applied to contract.md by Job contract-amend.

This file specifies the following, which contract.md does not state or states otherwise. contract.md is not edited by this Job; the Lead appends a follow-up Job that applies this list. Each item names the contract.md section it changes.

Rows 22–27 were added by Job `docs` of Plan `tree-menu` (renames, archive and the tree's right-click menu); they are applied to contract.md itself. Rows 28–31 were added by Job `docs-followups` of Plan `tree-menu` (PR `followups`); rows 28 and 29 are applied to contract.md itself. Rows 12–21 were added by Job `parity-contract` (cockpit parity, User, 2026-10-15, and R36); the checklist behind them is [cockpit-parity.md](cockpit-parity.md). Rows 12–14 and 16 are also applied to contract.md itself; rows 15 and 17–21 state what contract.md leaves to this file.

| # | contract.md section | Amendment | Where here |
|---|---|---|---|
| 1 | §6.2, chat slot rule | the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat` (Escalation `contract-commands-a1-q1`, answered by the Lead) | §10 |
| 2 | §6.2, `@cpd_view` values | new value `chat`: the placeholder pane running `cpd tui view chat`, text `No Lead here: pick a Plan in the session tree`; at most one, only ever in the slot | §10, §13.2, §17 |
| 3 | §6.4, `session park` | park creates a new park window holding a placeholder, then `swap-pane`s the Lead and the placeholder; `session open` kills the placeholder pane with its window when a Lead takes the slot | §10.4, §10.5 |
| 4 | §1.4, `$CPD_HOME` layout | three more files: `run/<plan>/lead-message.md`, `run/<plan>/plan-<review>.md`, `run/<plan>/messages/<session>.md` | §1.4 |
| 5 | §5.3, the cursor | the adapter adds `delivered_to` to every message's `details`, and the cursor counts only messages delivered to this Session, never at or below its `session-registered` Event | §15.2 |
| 6 | §5.3, delivery table | `plan-review` with `status: pending` opens plannotator in the Plan Lead's adapter (no message); listed today under "every other kind: ignored" | §15.3, §15.5 |
| 7 | §5.3 and §5.4, `--session` selection | "a Lead of Plan P" is the Session that is `plans.lead_session`; any other Session, side forks included, follows only Events addressed to itself | §10.3, §11 |
| 8 | §6.5, environment | Lead and Manager panes get `PLANNOTATOR_PORT` as the configured range, `PLANNOTATOR_REMOTE=1` and `PLANNOTATOR_URL_HOST` set by cpd (decision User 2026-10-23, contract §6.5) | §15.5 |
| 9 | §6.3, tmux commands | `respawn-pane`, for a dead pane of the cockpit's layout | §13.1 |
| 10 | §1.2, output | `session run`, `tui view` and `tui --attach` own their terminal instead of printing one JSON value (the outline's "builds and attaches the cockpit") | §1.2 |
| 11 | §1.2 table of codes | the error codes this file introduces: `context_required`, `session_required`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id`, `context_exists`, `question_answered`, `not_a_git_repo`, `base_not_found`, `plan_exists`, `illegal_phase`, `no_draft_jobs`, `review_decided`, `dependency_cycle`, `worktree_exists`, `watch_start_failed`, `spec_invalid`, `tier_invalid`, `job_key_exists`, `needs_unknown`, `needs_unavailable`, `nothing_to_edit`, `job_merged`, `job_not_failed`, `job_not_ready`, `spawn_failed`, `attempt_not_running`, `session_malformed`, `driver_lock_missing`, `jobs_not_done`, `rebase_conflict`, `not_a_lead`, `grounded_lead`, `no_cockpit`, `wrong_pane`, `cursor_ahead`, `not_a_terminal`, `doctor_failed`, `pi_not_found`, `catalog_failed`, `file_unreadable`, `invalid_name`; most are cpd 1's (cpd 1 §11 and its `src/`) | §1.3 and each command |
| 12 | §6.1, global options | the options `focus-events on`, `pane-active-border-style fg=blue` and `pane-border-format` (titles, Title Case, unpadded, a grey pill with black text and a blue one with black text when focused, hs-d475, hs-d534 (replacing hs-d530's): `Context`, `Plan`, `Deploy`, `Worker · <job> (Read-Only)`; none for Leads, the chat placeholder and the StatusBar), the root `MouseDown1Pane` binding and the `pane-died` hook that respawns only a pane with `@cpd_view`: all set at every server start, not only by `cpd tui` | §13.1 |
| 13 | §6.2, §6.3 | a Worker view pane carries `@cpd_view worker`, `@cpd_job` and `@cpd_attempt`, opens with `split-window -h -d -l 50%` and retargets with `respawn-pane -k`; tmux commands `respawn-pane -k`, `set-hook` and `display-popup` | §13.2 |
| 14 | §6.4, §3.6, §5.2, §1.2 | `session close <id>`: kills a Lead's pane, the Session is `stopped` with `session-moved {place: "none"}`, the Lead in the chat slot refused (`in_chat_slot`, exit 3) | §10.9 |
| 15 | §6.3, reset-layout | `cpd tui --respawn` restarts every live view; a StatusBar added to an existing `main` is followed by reset-layout; `cpd tui` applies §6.1's options to a running server | §13.1 |
| 16 | §2.2 Plan, §4.5, §7.6 | `pr poll` stores the PR's title, review decision and checks summary in `plans.pr_title`, `pr_review_decision` and `pr_checks`, added by migration 2 | §9.2 |
| 17 | §6.2 (the views) | the Dispatch column is the machine's hostname and load, the PRs of the Plans in the cockpit, git (future) and Jira (future); no engine, slots, Escalations or Events | §13.2, §17 |
| 18 | §6.2 (the views) | the plan view's top row is ` <context id> ▸ ` and the tabs only; one tab per Lead Session of the Context with a pane; the tree's keys are ↑↓/jk, Enter, `n`, `x`, `?` and its cursor, clicks and prompts are the reference's | §13.2 |
| 19 | §6.2 (the views) | what each view reads, in process; the tree's dot (needs you) rule from Events; the plan row no longer reads Escalations, the Plan review or the PR | §17 |
| 20 | §6.2 (the views) | the legend opens from the tree's `?` and the StatusBar button, not from the Dispatch column; its section titles say Context, and it lists no `cancelled` | §13.2 |
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
| quick check falls back to `pre-merge` | no fallback: no `pre-commit` table means no quick check, and it passes |

**contract.md:** the status and sources lines; §1.1, §1.2, §1.4, §2.2 (Job, Attempt, Session), §2.3, §2.4, §3.2, §3.4, §3.6, §3.7 (E4), §4.4, §5.1, §5.2, §5.3, §6.2, §6.3, §6.4, §6.5, §7.3, §7.4, §7.6, §8.2, §9, §10 (R13, R17 superseded for cpd 2, R19, R20).

**commands.md:** the introduction and contents; §1.5, §2, §4.2, §4.3, §4.5, §5 (the Tier flag), §5.1, §5.2, §5.3, §5.6, §5.7, §6.1, §6.2, §6.3, §6.4, §7 and §7.1 (emptied), §9.1, §9.3, §11, §13.2, §14.1, §14.2, §15.3, §16 (now "The Worker contract"), §16.1, §16.2 and §16.3 (emptied), §17, §18, §19 (amendment 11), §20 (new).

**CONTEXT.md:** Dispatch, Ledger, Session, Reviewer (removed), Gate, Quick check, Merged.

## 21. `inbox`

Contract §2.2 "Inbox item", §3.7 E8, §4.4, §5.2. The Inbox is the single source of "needs you": one stored item per thing that needs the User, opened and closed by the engine (E8) from its cause, never by a command. The commands read the items and hide them. Area `core/src/inbox/`. This section describes the core kinds (`dialog`, `plan-review`, `loop-guard`, `unread`, and `engine-down`, which is not stored; `engine-error` is no longer opened, hs-d314, and nor is `job-stuck`, hs-d318: a stuck Job is a message to the Context's Manager, contract §3.7 "A stuck Job") and the PR poll's (`pr-ready` and `pr-failing`, `pr-changes`, which are no longer opened, hs-d470) and the team review poll's (`review-requested`, §21.5; contract §2.2 gives their conditions, urgency and labels, and their `url` is the pull request's page).

An item as printed:

```json
{"id": 12, "kind": "job-stuck", "urgency": "red", "label": "inbox/core-docs failed", "subject": "job:inbox/core-docs", "context": "building-cpd", "plan": "inbox", "job": "core-docs", "session": null, "record": null, "event": 48311, "url": null, "opened_at": "2026-10-14T21:40:02.114Z", "closed_at": null, "snoozed_until": null, "dismissed_at": null, "claimed_at": null, "age_seconds": 1260}
```

`age_seconds` is `closed_at` (or now, for an open item) minus `opened_at`. `dismissed_at` and `claimed_at` are null except for hidden items; `dismissed_at` is set when `inbox dismiss` hides an item for the rest of its life; `claimed_at` is set when `inbox claim` hides a `review-requested` item. The item `engine-down` has `id: null`, `subject: "engine"` and no links.

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
| `--all` and `--closed` are not given together, and `--limit` comes only with `--closed` | `usage` | 2 |
| `--context` names a Context that exists | `context_not_found` | 4 |

Effects: none; it takes no lock (§4.2). Open visible items come in the order of §2.2 (red, yellow, magenta, blue, cyan, white; oldest `opened_at` first), with `engine-down` first when `locks/watch.lock` is free **and no live Manager Session exists** (role `manager`, not archived; hs-d318, hs-d363: with one, the tree tells it when the engine stays down 60 s, §13.2). A snoozed item whose `snoozed_until` has passed is visible again. `hidden` counts the open items now hidden by snooze, dismissal or claim, whether or not `--all` lists them; when several apply, the item's `hidden` is `dismissed`, then `claimed`, then `snoozed`. `events_cursor` is the highest Event id, so a follower can start from it.

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

Effects: none. Output `{"ok": true, "item": {…}}`, the item as above. Idempotent: read-only.

### 21.3 `inbox snooze`

```
cpd inbox snooze <id> <15m | 1h | tomorrow>
```

`15m` and `1h` are that long from now; `tomorrow` is the next 09:00 in local time (the offset read with `date +%z`, which GNU and BSD `date` both print). Any other value is `usage` (exit 2).

| Precondition | Code | Exit |
|---|---|---|
| the item exists | `item_not_found` | 4 |
| the item is open | `item_closed` | 3 |

Effects, one transaction: **rows** `inbox_items.snoozed_until`; **Event** `inbox-snoozed` `{item, until}`. The item returns at its rank when the time passes; its `opened_at` is kept.

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

Effects, one transaction: **rows** `inbox_items.dismissed_at`; **Event** `inbox-dismissed` `{item}`. For a `review-requested` item, the dismissal holds for the pull request forever (C1, contract §2.2 "The team review rule"): an open or closed `review-requested` item with `dismissed_at` or `claimed_at` set suppresses new items for that pull request. For any other kind, the item stays hidden for the rest of its life; because an item lives only while its cause holds, a new cause is a new item and shows again.

```json
{"ok": true, "item": {"id": 12, "dismissed_at": "2026-10-14T21:41:02.114Z", "…": "…"}}
```

Idempotent: yes. An item already dismissed returns `"already": true` and writes nothing, with no Event.

### 21.5 The team review cache (engine E11)

Not a command. The team review read (Context decision "Team review Inbox", User, 2026-10-19; contract §2.2 "The team review rule", §3.7 E11): engine step E11 runs the Slack helper (§21.6) every `watch.team_review_seconds` (contract §8.2, default **1800**) after the end of the previous run, when `inbox.team_channel` is set, and caches what it read in `run/team-review.json`. **It writes no row and appends no Event of its own** (a failure is reported, below): the file is a derived cache (contract §1.4), like a Context digest.

- **Detached.** The helper runs on its own thread with a deadline of 120 s (§1.2 `tool_timeout`, `details.tool` `node`), and a later tick collects its answer: E11 never holds the tick. `locate_pi` (contract §9) gives the entry and `PI_CODING_AGENT_DIR`; `pi` that is not found is `pi_not_found`. The helper's argv is the one of §21.6, cwd `$CPD_HOME`, under pi's Node.
- **The file**, written to `run/team-review.json.tmp` and renamed, so a reader never sees half of it: `{"version": 1, "polled_at": <time>, "channel": "C02JU6C2Z1D", "read": 74, "posts": [ … ]}`, `posts` as the helper printed them (§21.6: `repo`, `number`, `ts`, `ts_raw`, `author`, `primary`, `reactions`, `claim`, `eyes_me`, `thread_tags_me`, newest `ts` first). An answer with no `posts` or no `read` is refused (`slack_helper_failed`) and the file stays.
- **Failures** leave `run/team-review.json` **as it was**: the last good copy stays (hermes `fetch_slack`, `refresh.py:59-93`). Each is E11's one `error` Event per code (contract §3.7; `source` `watch`, the code and `details` the helper's, so `slack_auth` says `pi mcp login slack`); the Manager receives the Event (hs-d314), and no Inbox item opens.
- **What reads it.** The Inbox rule, in E8 (contract §2.2 "The team review rule", hs-d320): the status of each primary post's pull request from one memoised `gh api graphql` call, with the Slack overrides from `claim`, `eyes_me` and `thread_tags_me` (hermes `classify` 264-341, `refresh.build` 435-466); only `NEEDS_REVIEW` opens an item. It is not E11's: E11 calls no GitHub. A missing file is "unknown": the open items stay.

### 21.6 The helper `slack-channel.ts`

`pi/lib/slack-channel.ts` in cpd's `pi/` package, beside the adapter (contract §8 `pi_adapter`). Not a cpd command: engine E11 (§21.5) runs it, detached. It calls the Slack MCP server's tools **through pi's own MCP client and token store**, so cpd has no HTTP client (hs-d107) and never reads, copies or logs the credential (as hermes' `slack_fetch.py:21-28` did through Hermes' own storage). Evidence and the rejected alternatives: `docs/research/team-review.md`.

```
node <adapter>/lib/slack-channel.ts --coding-agent <…/@earendil-works/pi-coding-agent/dist/index.js> --channel <C…> --me <U…>
```

Environment: `PI_CODING_AGENT_DIR` (the agent directory of the pi release, contract §9). cwd is `$CPD_HOME`; the helper hands it to the connection, which sends it to the server as its root (`extensions/mcp/runtime.js:149`). Constants in the helper, each with its source: tool `slack_read_channel`, server entry `slack` of `<agentDir>/mcp.json` (the global file only; a project file is never read), `limit` 200 (`refresh.py:56`), window 7 days (`refresh.py:626-641`), 45 s for the channel read (`slack_fetch.py:183`), 30 s for each thread or reaction read (`slack_fetch.py:211, 324`), at most 8 of those at once (`refresh.py:353`).

**Entry.** The helper answers only when it is the script `node` was started with, and it decides by real path: the installed adapter path is a symlink (`<share>/pi` to `pi.<id>`, docs/implementing.md "The adapter swap"), and Node resolves symlinks in a module's `import.meta.url` but not in `process.argv[1]`, so comparing the two as they are never matches there and the script would exit 0 with no output. Reached through a symlink to the script or to a directory that holds it, the helper runs as when reached by its real path. A module that only imports the helper does not run it.

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

Writes nothing: no file, no Slack message, no reaction (hermes' `post_message`, `add_reaction` and `mark_merged` are not ported). The `:eyes:` reaction is not placed (C7, §21.7: it awaits a writable Slack integration). The credential is read and refreshed inside pi's modules; the helper holds no token.

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

Effects, one transaction: **rows** `inbox_items.claimed_at` = now; **Event** `inbox-claimed` `{item}` (C4); one `session-message` Event to the Context's Manager (C4), worded to ask the Manager to start a review of the pull request, containing the URL and the Slack post's channel and `ts_raw`. Output: `{"ok": true, "item": {…, "claimed_at": "…"}, "manager": "<session id>"}` (C4).

Idempotent: yes. An item already claimed returns `"already": true` and writes nothing, with no Event.

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
| every file the key reads is valid TOML with known domains, keys, types and values, and a valid file name; `details.file`, `details.key` | `policy_invalid` | 1 |

Effects: none. One read transaction for the key's rows, then the compiled default and the key's files, read in order (`policy/host.toml` when it exists, `policy/<owner>/owner.toml`, `policy/<owner>/<repo>.toml` for a `Repo` key when it exists: the file is optional), each parsed once. No subprocess. `check_file_names`: the file `host.toml` is the machine scope; a directory whose name is a valid owner login must contain only `owner.toml` and files with valid repo names ending in `.toml`, else `policy_invalid` (§8.3).

Output: `key`, `{owner, repo}` (`repo` null for an `Owner` key); `role`, the Session's role (`manager`, `lead` or `worker`) with no flag, else null; `files`, the realpath of each file read in order (machine, owner, repo), only the files that exist (a repo without a file has no repo scope and prints the owner's policy); and `policy`, one entry per domain of contract §8.3, in its table's order, each with `source`, `audience` (the roles its `text` reaches), `keys` (its typed keys, each with its `value` and `source`; `{}` for a domain that has none) and `text`. The three repo-only domains (contract §8.3) print like the others: `audience` `[]`, `text` `""`, `source` `default` or `repo:<path>`; the `keys` of `hooks` are `pre-start`, `pre-commit` and `pre-merge`, each `value` a list of blocks, each block a list of `{"name": <string or null>, "command": <string>}` (`[]` when the hook is not set); of `test-commands`, `build`, `run` and `list` (a string, or null when not set); of `protected`, `paths` (a list of strings, `[]` by default). A domain's `source` is `default`, `machine:<path>`, `owner:<path>` or `repo:<path>`; a typed key left out of a set domain has its domain's source followed by ` (key default)`, and its value is cpd's default (contract §8.3 "Precedence and replace"). The adapter keeps the domains whose `audience` holds `role` and whose `text` is not empty.

```json
{"ok": true, "key": {"owner": "clayton-duarte", "repo": "cpd"}, "role": "worker",
 "files": ["/home/clayton/.cpd/policy/host.toml", "/home/clayton/.cpd/policy/clayton-duarte/owner.toml"],
 "policy": {
   "gate": {"source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml", "audience": ["lead", "worker"],
           "keys": {"merge": {"value": "local", "source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml"},
                    "pr": {"value": "local", "source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml"}},
           "text": "These repos run no GitHub Actions: …"},
   "pr": {"source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml", "audience": ["lead", "worker"],
          "keys": {"feedback.checks": {"value": "none", "source": "owner:/home/clayton/.cpd/policy/clayton-duarte/owner.toml"}},
          "text": ""},
   "deploy": {"source": "default", "audience": ["lead"], "keys": {}, "text": ""},
   "code": {"source": "default", "audience": ["lead", "worker"], "keys": {}, "text": ""},
   "tests": {"source": "default", "audience": ["lead", "worker"], "keys": {}, "text": ""},
   "monitor": {"source": "default", "audience": ["lead", "worker"], "keys": {}, "text": ""},
   "instructions": {"source": "default", "audience": ["manager", "lead", "worker"], "keys": {}, "text": ""},
   "hooks": {"source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml", "audience": [],
             "keys": {"pre-start": {"value": [], "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"},
                      "pre-commit": {"value": [[{"name": null, "command": "npm run typecheck || exit 64"}]], "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"},
                      "pre-merge": {"value": [], "source": "repo:/home/clayton/.cpd/policy/clayton-duarte/cpd.toml"}}, "text": ""},
   "test-commands": {"source": "default", "audience": [],
                     "keys": {"build": {"value": null, "source": "default"}, "run": {"value": null, "source": "default"}, "list": {"value": null, "source": "default"}}, "text": ""},
   "protected": {"source": "default", "audience": [], "keys": {"paths": {"value": [], "source": "default"}}, "text": ""}}}
```

**The machine object.** Every form of the command (`--pr`, `--context`, `--owner`, `--owner --repo` and no flag) also carries a top-level `machine` object, because every form reads the machine file (`$CPD_HOME/policy/host.toml`, contract §8.3; it is also a full policy scope, read as the "Effects" above say, and the `mcp` table is the only part this object reports): `{"name": "<machine>", "mcp": {"source", "servers"}}`. `name` is the config key `machine`; `source` is `default` or `machine:<realpath>` (contract §8.3 "MCP servers of the machine scope"); `servers` maps each server name to its entry with the keys as written in the file (a key left out is absent; an entry carries `headers`, a table of header name to literal value, when the file sets it). An invalid machine file is `policy_invalid`, with the same exit and `details` (`file`, `key`) as an invalid owner file, and fails every form. `markdown` gains a section `## Machine <name>` listing each server's name, `url` and `exposure`, or `No MCP servers.`. Example, on machine `pro`, `machine` only:

```json
{"machine": {"name": "pro", "mcp": {
  "source": "machine:/home/clayton/.cpd/policy/host.toml",
  "servers": {"slack": {
    "url": "https://mcp.slack.com/mcp",
    "description": "Slack: read channels and threads, post messages, and manage reactions.",
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

Idempotent: yes; it only reads. It is also the check to run after editing a policy file.

## 23. `secret`

Wraps 1Password's CLI `op` to manage secrets in Secure Notes. cpd enforces who may do what, in which vault, and when. Agents never call `op` (the pi adapter refuses it). Agents run `get` and `list` only; the Lead runs all commands. Workers on a permitted Attempt run `get` under an allowance. A Lead or Manager needs an approval for most vaults. cpd holds no token and never signs in; the 1Password app's per-use prompt (where it runs) is the approval. Values never appear in argv, output, Events, logs, files or the store. Area `core/src/secret/`.

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
| `CPD_SESSION` is set, valid, and a Manager Session (role `manager`, not archived) | `manager_only` | 3 |
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
| `CPD_SESSION` is set, valid, and a Manager Session (role `manager`, not archived) | `manager_only` | 3 |

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

Run by `scripts/gate-run.sh` after its command, with `TMPDIR` the run's directory. It lists what the run left: the processes that have `GATE_RUN=<id>` in their environment (`Spawn::with_env`), the tmux sockets named with the id (`/tmp/tmux-<uid>` and `$TMUX_TMPDIR/tmux-<uid>`), the paths under `$TMPDIR`, and the lines of `$TMPDIR/tripwire.log`. A clean run prints nothing, removes the empty directory and exits with `--status`. Otherwise it prints `gate-run: FAIL: …` on stderr, `SIGKILL`s only the listed pids, removes only the listed sockets and the run's directory, and exits with `--status` when that is not 0, else 64. Another run's id never matches. Like `slot`, it prints no JSON of its own; a usage error is the envelope on stderr. Idempotent: yes.

## 23. `prune`

```
cpd prune [--apply] [--drop <lead-branch>]... <repo>...
```

The successor of `scripts/prune-worktrees.sh` (hs-d138, hs-d143, hs-d144): the clean-up of leftover worktrees whose work reached `origin/main`, which the Manager runs by hand after a Plan. The script shrinks to `exec cpd prune "$@"`. A User's command at a shell: no callers, not cockpit, not internal.

Flags and arguments:
- `--apply`: do the removals. Without it nothing changes (apart from `git fetch origin`) and the command prints what it would do.
- `--drop <lead-branch>`: repeatable. A worktree whose Lead Branch is one of the values is removed whether or not it is merged (rule `remove dropped`), unless it is protected.
- `<repo>...`: one or more repos, at least one. A missing repo argument or an unknown flag is a usage error, exit 2.

**Protected set.** Before anything else the command reads the store (no `cpd` child, no `jq`): for every Plan that is not done, the `lead_branch` of each PR whose status is not merged, and `cpd/<plan>--<attempt id>` for each Attempt of a Job of such a PR. If the store cannot be read the command exits 2 before touching anything. `git` and `gh` (authenticated) go through the `Runner`.

**Rules.** Per worktree (the main one is skipped), the first rule that holds wins:

```
keep   active       the branch is protected (also with --drop)
keep   dirty        `git status --porcelain` is not empty (untracked files included)
remove dropped      the Lead Branch L is one of the --drop values (merged or not)
remove              HEAD is an ancestor of origin/main
remove              a merged PR P has a head branch equal to L or starting with `L/`, or,
                    when L is `cpd/<plan>/<key>` (a legacy Lead Branch renamed locally),
                    equal to `cpd/<plan>`; HEAD is an ancestor of P's head commit, and P
                    reached main (its base is main, or the base is the head of a merged PR
                    that reached main, at most 5 levels)
keep   not merged   otherwise
```

L is the branch, or for a branch containing `--` the part before the first `--`. A detached HEAD has no L. Removal deletes the worktree and its local branch, never a remote branch.

Effects per repo: `git fetch origin`, `gh pr list --state merged --limit 500` of the repo's `origin` slug, `git worktree list --porcelain`; with `--apply`, for each removal `git worktree remove --force`, `git branch -D` (only when no other worktree has the branch) and finally `git worktree prune`.

Output: one line per worktree, `keep <path> (<branch|detached>): <reason>`, `remove <path> (<branch|detached>)` or `remove <path> (<branch>): dropped`, `failed <path>: <error>`, then `<repo>: <n> remove, <m> keep`.

Exit codes: 0 unless a command failed; a failure is printed and the run goes on, exit 1 at the end. 2: usage, or the protected set could not be read.

Tests: in-process on `Fakes` (`FakeRunner` git and gh routes, a temp store), the names of the old `cli/tests/tooling/prune_script.rs` kept: `a_failed_command_is_reported_and_exits_non_zero`, `keeps_what_cpd_still_runs`, `matches_a_legacy_lead_branch_renamed_locally`, `drop_removes_clean_unmerged_worktrees_of_the_named_lead_branch`, and `a_failing_cpd_exits_2_and_removes_nothing` (now: an unreadable store). The command is listed in `cli/tests/tooling/interface.rs`.
## 23. `tag` and `version`

The release plumbing of docs/versioning.md, in Rust over `Ports` (`Runner` for git, std `fs` for files). `scripts/tag-version.sh` and `scripts/sync-version.sh` are thin: each is `exec cpd tag` / `exec cpd version sync "$@"` and keeps only what must exist before cpd does. Their old exec tests are replaced by in-process tests on `Fakes` (a `FakeRunner` git route; a temp dir for the files), and `scripts/check-portable.sh` still guards the two scripts.

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

The logic behind `scripts/test-changed` and `scripts/tests.sh`, which become thin (`exec cpd test …`). The selection and the name classification are pure over `git diff` and `cargo metadata` output, read through the `Runner`; the commands start `cargo` through `scripts/cargo.sh` (the bootstrap that builds cpd). Callers: Manager, Lead, Worker; not cockpit, not internal. The command tree is pinned by `cli/tests/tooling/interface.rs`, and [docs/interface.md](interface.md) is regenerated.

### 23.1 `test changed`

```
cpd test changed [--dry-run] [<base>]
```

Runs the Rust tests the changed files can reach: exactly the selection of docs/implementing.md "The scoped loop" (changed files from the merge base of `<base>`, default `origin/main`, to the working tree and untracked files; crate and reverse dependencies as `-p`; `cli/tests/<area>/<module>.rs` as `--test <area> -E 'test(/^<module>::/)'`; root-level inputs as `--workspace`; docs, `pi/` and other scripts select nothing). `--dry-run` prints the one command on stdout and runs nothing, exit 0. Nothing selected: `test-changed: nothing to run` on stderr, exit 0. Otherwise the exit status is nextest's. `scripts/test-changed [--dry-run] [<base>]` stays as `exec cpd test changed "$@"`.

### 23.2 `test run`

```
cpd test run <name>...
```

Runs the named tests and exits non-zero when any failed or did not run. A name is a pi name, `pi/test/<file>.test.ts::<title>` (the file's path, `::`, the exact `node:test` title; run with `node --test --test-name-pattern=<title> <file>`, and refused when the title is in no `test(` or `it(` call of the file), or a Rust name as nextest filters it (run with `nextest run --workspace --no-fail-fast`). Both run filtered, so neither takes a test slot. Runs from the repo root, whatever the cwd. No name is a `usage` error.

### 23.3 `test list` and `test build`

```
cpd test list <name>
cpd test build
```

`list` exits 0 only when that one test exists: a pi name when its title is in a `test(` or `it(` call of its file (nothing is started), a Rust name when `nextest list --workspace --message-format oneline <name>` prints a test. `build` runs `nextest run --workspace --no-run`, then `npm run typecheck`, and stops at the first failure; its status is that of the step that failed. Both run from the repo root, whatever the cwd.

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
| the machine file, `$CPD_HOME/policy/host.toml`, is valid TOML and its `[mcp]` table has known keys and types (contract §8.3 "MCP servers of the machine scope"; the file's other domains are the policy scope's, and an invalid one is `policy_invalid` too); `details.file`, `details.key`; nothing is appended | `policy_invalid` | 1 |
| the machine file's `[mcp]` lists `<name>`; `details` `{name, machine, available}` (`machine` is the config key `machine`), `available` the sorted server names (an empty array when none); the message names the machine and lists what is available, or says none | `mcp_unknown` | 4 |

**Effects**, one transaction:
- **Event** `mcp-attached` (contract §5.2) addressed to the caller's Session (`session` set, with its `context` and `plan` when it has one), `data` `{name, server}`: `server` is the entry exactly as the machine policy gives it (pi's key names; a key left out is absent). `text`: `Server <name> attached to Session <id>.`
- Nothing else: no row, no file, no tmux call.

**The adapter** of that Session registers the server with pi (§15.3 `mcp-attached`, contract §5.3). The attach is for that Session only: never for another Session and never for a later one, never saved and never done by default (pro-d2). There is no list and no detach command (pro-d8): `cpd policy show` lists what the machine allows, and the Session's end detaches. The server's `direct` tools appear in that Session as `mcp__<name>__<tool>` on a later model call of the same turn, not the call right after the attach (the adapter registers the server when it delivers the Event, then pi connects), and `tool_search` never finds the `direct` tools of a `hidden` server.

```json
{"ok": true, "attached": {"name": "slack", "session": "596d007f-…", "event": 48301}}
```

Idempotent in effect: running it again appends another Event, and the adapter registers the same name again, which pi accepts as a replacement.

#### Signing in

A server that uses OAuth needs the User to sign in once on the machine (pro-d10): in that Session's pi with `/mcp login <name>`, or from a shell with `pi mcp login <name>`. The shell form works while the user-level `~/.pi/agent/mcp.json` has an entry with the same name and URL, because pi keys the credential by server name and URL. Tokens live in pi's own `mcp-auth.json`; cpd never reads or writes one (hs-d107, pro-d4). A running Session uses new credentials on its next turn (pi's rule), and an attached server that needs a sign-in shows pi's own "requires sign-in" report.
