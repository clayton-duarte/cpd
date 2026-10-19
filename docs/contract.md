# cpd 2: contract (core)

Status: **normative for every cpd 2 implementation Job.** A Plan is one deliverable over one or more PRs (R34). Vocabulary: [CONTEXT.md](../CONTEXT.md). Command-by-command reference (synopses, flags, every refusal, JSON examples, the pi adapter protocol, the Worker contract, what the TUI reads): `docs/commands.md`, written by Job `contract-commands` after this document. Where the two disagree, this document wins and `commands.md` is fixed.

Sources: the User's decisions R1–R14 and R19–R20 (applied by Job `contract-gate-pr`, §10), R34 (a Plan is one deliverable over one or more PRs, each with its own repo, base, Lead Branch, Lead worktree, Gate and GitHub pull request; Job `r34-contract`, §10), R35 (§10) and the approved outline with D1–D5 (`~/.cpd/plans/cpd-rust/`), ADR-0009 to ADR-0012 (`docs/adr/`; ADR-0012 is superseded by hs-d23 and hs-a1, Plan manager B1, §4.6), the cockpit vision and research (`~/.cpd/plans/cpd-ui/`). cpd 1 (`docs/archive/cpd1/cli-contract.md`, cited below as "cpd 1 §n") is reference only: where its behaviour is kept, the section is cited; where it changes, the change and its reason are stated.

Evidence labels, as in the research notes: **verified** means run on the homeserver on 2026-10-14 (tmux 3.4, git 2.43.0, gh 2.45.0, pi 1.2.0, worktrunk 0.80.0); **docs** means read in the named tool's documentation; **unverified** means a design assumption the implementation Job must check.

## Contents

1. [Principles](#1-principles)
2. [Model and identifiers](#2-model-and-identifiers)
3. [State machines](#3-state-machines)
4. [Storage](#4-storage)
5. [Events](#5-events)
6. [The tmux host](#6-the-tmux-host)
7. [git and the Gate](#7-git-and-the-gate)
8. [Configuration](#8-configuration)
9. [Tiers](#9-tiers)
10. [Traceability](#10-traceability)

## 1. Principles

### 1.1 What cpd 2 is

- **CLI first.** Every capability is a command. Agents, the pi adapter, scripts and the TUI all use the same commands; the TUI (`cpd tui`) is the human view onto them, never a second way to change state.
- **One binary, written in Rust.** The Cargo workspace at the root of the repo has three crates: `core/` (`cpd-core`, the library), `cli/` (`cpd`, the binary) and `tui/` (`cpd-tui`, the cockpit) (R6, switch-over; `implementing.md`). The binary is `cpd`, and this contract writes every command as `cpd …`. Until the switch-over the binary was installed as `cpd2` beside the TypeScript cpd (R12); the switch-over replaced cpd 1 and renamed the binary everywhere, including in rendered Specs and the pi adapter.
- **System dependencies:** `git`, `tmux`, `pi` and, for pull requests only, `gh`. No `bd`, no `wt` (worktrunk), no `herdr`, and no Node.js of cpd's own. pi brings its own Node runtime; cpd uses it only to read pi's model catalog (§9).
- **State in SQLite inside the binary** (R1): one database per machine, `$CPD_HOME/cpd.db`, in WAL mode, with SQLite bundled into the binary (no system SQLite). Reads never wait for writers. Every write is recorded in the append-only `history` table (§4).
- **Context records in the database.** A Context's records (decisions, notes, questions and answers, Plans) live only in the database tables `contexts` and `context_records`; Contexts are machine-local (hs-d23, hs-a1; Plan manager B1, §4.6). ADR-0012's git store is superseded.
- **tmux is the only host.** Leads run in the cockpit; Workers and the engine run in a hidden session of the same tmux server (§6).
- **The repo owns its setup and its Gate,** in `.config/wt.toml`, read by cpd itself so that repos set up for worktrunk work unchanged (R4, R5, §7).
- **Events are the single channel** to Leads, the TUI and agent states (D4, §5). There are no queue directories, no cache directories and no tmux user options carrying state.
- **One engine per machine** (D5): one `cpd watch` schedules every Plan under one concurrency limit.
- **A Plan is one deliverable over one or more PRs** (R34): each PR is the Plan's part in one repo, with its own base, Lead Branch, Lead worktree, Gate and GitHub pull request; one repo may hold several PRs of one Plan. A Plan of one PR behaves exactly as a Plan did before R34.
- **Kept from cpd 1:** the vocabulary (Job, Spec, Dependency, Attempt, Escalation, Tier, Gate, Lead Branch, Merged, Locked); the roles User, Manager (Plan manager B3, §3.1), Lead and Worker (cpd 1's Reviewer is removed by R19, §10); Workers write all the code; the Lead never writes code.

### 1.2 Output: one JSON value (D1)

Every command prints exactly one JSON value on stdout, always, except the three that own their terminal (below); there is no `--json` flag and no human format.

- Success: `{"ok": true, …}`, with the command's fields at the top level.
- Failure: `{"ok": false, "error": {"code": "<snake_case>", "message": "<one sentence>", "details": {…}}}`.
- Progress and debug text goes to stderr only.
- **The one streaming command** is `cpd events follow`: it prints one complete JSON value per line, each line an Event (§5.4), as the approved outline specifies. If it fails, its last line is a failure value as above.
- **Commands that own their terminal** print no JSON value on success: `cpd session run` and `cpd tui view` (internal) run pi or draw a ratatui view in their pane, and `cpd tui --attach` replaces itself with `tmux attach` (the outline's "builds and attaches the cockpit"). If one fails before taking the terminal, it prints the failure value and exits with its code. `cpd tui` without `--attach` prints one JSON value like every command.

Exit codes (kept from cpd 1 §2):

| Exit | Meaning | Examples of codes |
|---|---|---|
| `0` | ok | |
| `1` | failure: an external tool or something unexpected | `git_failed`, `tmux_failed`, `store_tampered`, `store_failed` (the database cannot be opened, is not cpd's, or a statement fails through a bug; Escalation `rs-store-a1-q1`; SQLite failures only), `internal` (a bug in cpd itself, for example a malformed Event built by a command; also a failed write to stdout other than EPIPE, because no I/O error code exists), `config_invalid` |
| `2` | usage error | `usage`, `plan_required`, `invalid_plan_id` |
| `3` | refused by an invariant: Locked, an illegal transition, a precondition | `job_locked`, `illegal_transition`, `dirty_tree`, `cas_lost` |
| `4` | not found | `plan_not_found`, `job_not_found`, `session_not_found` |
| `5` | timeout | `store_busy`, `lock_timeout` (`pr poll`'s `locks/pr-poll.lock`, §7.6; no git operation waits for a cpd lock since hs-d243, §7), `tool_timeout`, `ask_timeout`, `cancel_timeout` |

**`tool_timeout`** (exit 5): a subprocess cpd started ran past its deadline (the deadlines are listed in §7 for `git`, §6.3 for `tmux`, §9 for `node`, §3.7 for the engine's `cpd pr poll` child, `commands.md` §14.1 for `doctor`). `details`: `tool` (`git`, `tmux`, `node`, `cpd`, …), `argv` (the full argument vector) and `deadline_seconds`. Every subprocess cpd starts has a deadline (except tmux's `display-popup`, §6.3), a constant in the code with its reason, not a config key; `gh` keeps its own timeouts (§7.6) and hooks theirs (§7.3). At the deadline cpd kills the process **group** (`SIGTERM`, then `SIGKILL` after 5 s to those still alive) and the command fails with `tool_timeout`; it replaces `git_failed`, `tmux_failed` and the like for that call.

`commands.md` introduces more codes, most of them cpd 1's (cpd 1 §11 and its `src/`); each is specified at the command that returns it, under the same exit meanings:

| Exit | Codes |
|---|---|
| `1` | `gate_error`, `migration_changed` and `migration_order` (§4.5; hs-d178), `watch_start_failed`, `spawn_failed`, `session_malformed`, `driver_lock_missing`, `doctor_failed`, `pi_not_found`, `catalog_failed`, `cleanup_failed` (only as an `error` Event's code, source `pr poll`, never a command's failure; §7.2), `distill_invalid` and `distill_failed` (`context distill`'s failure and, the same, its `error` Event's code; §4.6) |
| `2` | `context_required`, `session_required`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id`, `spec_invalid`, `tier_invalid`, `nothing_to_edit`, `file_unreadable`, `invalid_name`, `pr_required`, `invalid_pr_id` (R34) |
| `3` | `distill_running` (§4.6), `push_rejected` (hs-d139, §7.6), `session_ambiguous` (hs-a3, commands §1.3), `pr_exists`, `pr_in_use`, `pr_empty`, `pr_locked`, `pr_not_ready`, `pr_ended`, `pr_closed` (§7.6), `needs_other_pr`, `branch_exists` (R34, §3.8), `context_exists`, `question_answered`, `not_a_git_repo`, `plan_exists`, `illegal_phase`, `no_draft_jobs`, `review_decided`, `dependency_cycle`, `worktree_exists`, `job_key_exists`, `draft_incomplete` (hs-d241, commands §4.6, §4.8), `needs_unknown`, `needs_unavailable`, `review_commits`, `review_missing`, `job_merged`, `job_not_failed`, `job_not_ready`, `attempt_not_running`, `no_port`, `jobs_not_done`, `gate_failed`, `rebase_conflict`, `not_a_lead`, `no_cockpit`, `wrong_pane`, `in_chat_slot`, `cursor_ahead`, `not_a_terminal` |
| `4` | `base_not_found`, `pr_not_found` (R34) |

**Adapter-only codes.** `protocol_mismatch` is never a command's failure and never an `error` Event's code: it appears only in the pi adapter's message to its Session (`cpd adapter: …`, commands.md §15.2), when `cpd --version`'s `protocol` is not the number the adapter was built for (§5.2 "The protocol number"). It has no exit code.

### 1.3 Conventions every command follows

- **Selecting a Plan.** `--plan <id>`, else the environment variable `CPD_PLAN`, else usage error `plan_required` (exit 2). cpd sets `CPD_PLAN` in every pane it starts for a Plan (§6.5). This replaces cpd 1's `--wf` / `CPD_WF`.
- **Selecting a PR** (R34). `--pr <key>`. It may be omitted when the Plan has exactly one PR that is not `cancelled`, which is then selected; with two or more it is the usage error `pr_required` (exit 2), and with none `pr_required` too, whose message names `cpd pr add`. A key that is not one of the Plan's PRs is `pr_not_found` (exit 4). Used by `job add`, `pr open`, `pr rebase` and `pr view` (§2.5); `pr poll` has no `--pr`: it polls every open pull request of the User (§7.6, hs-d96); `pr add`, `pr edit`, `pr cancel` and `pr start` take the key as their argument.
- **Validation at the boundary.** Every external input (arguments, `config.toml`, Spec files, `.config/wt.toml`, `.config/cpd.toml`, Context record files, `gh` and `git` output, pi session lines) is parsed into types once, at the edge. Unknown flags and unknown config keys are errors. Nothing falls back silently.
- **Subprocesses** run from argv arrays (never a shell string, except the repo's own hook commands, which are shell by definition, §7.3), with stdin from `/dev/null` unless stated, an explicit `cwd` and an explicit environment. The exit code is checked before any output is parsed.
- **Times** are RFC 3339 UTC strings with milliseconds, `2026-10-14T21:10:00.000Z`.
- **Idempotence.** A command that finds its effect already in place returns `ok` with `"already": true` instead of failing, unless `commands.md` says otherwise for that command.

### 1.4 `$CPD_HOME`

`$CPD_HOME` defaults to `~/.cpd`. cpd 1 uses the same directory; cpd 2 never reads or writes cpd 1's files (`config.json`, `leads/`, `slots/`, `workflows/`, `locks/merge-*.lock`), and none of its own names collide with them.

```
$CPD_HOME/
  config.toml                     machine config (§8)
  cpd.db, cpd.db-wal, cpd.db-shm  the database (§4)
  archive/contexts/<context>/     an old git store of a Context, moved here by migration 0007 (§4.5); nothing reads it
  locks/
    watch.lock                    held by the one `cpd watch` (§6.6)
    attempt-<plan>--<attempt>.lock  the driver lock: taken by the Attempt's starter, inherited by its driver, removed when the Attempt ends (§3.4)
    pr-poll.lock                  held by `pr poll` for the whole poll (§7.6, hs-d96)
    distill-<context>.lock        held by the Context's one distill run, `context distill`, for its whole life (§4.6)
    plannotator-ports.lock        held while a Plan's review port is assigned (§6.5)
    store.lock                    held by every open of `cpd.db` during its one-time setup only: the WAL switch, the migrations and the conversion of a legacy store (§4.1, §4.5); waited for at most `store_busy_ms`, then `lock_timeout` (§4.2)
  run/
    watch.log                     the engine's stderr
    <plan>/plan.md                the last `plan render`
    <plan>/lead-message.md        the Lead's first message, written by `plan new`
    <plan>/plannotator-port       the Plan's plannotator port, one line, assigned by `session run` (§6.5)
    <plan>/plan-<review>.md       the Plan review's content, written by `plan review`, opened in plannotator by the pi adapter
    <plan>/feedback-<review>.md   plannotator's feedback on a Plan review, written by the pi adapter and overwritten on a retry; passed to `plan decide --feedback-file`
    <plan>/messages/<session>.md  a forked Session's first message, copied by `session fork --message-file`
    <plan>/specs/<attempt>.md     rendered Worker Spec
    <plan>/reviews/<k>.md         a review Job's findings, written by its Worker (B2, §3.4); the directory R19 removed, reused for this
    <plan>/logs/attempt-<attempt>.log      the Attempt driver's stderr
    <plan>/logs/<attempt>.<hook>.log       full output of a hook run (§7.3)
    <plan>/logs/pr-<key>.pre-merge.log     full output of the last Gate run by `pr open`, `pr rebase` or `pr push` on PR <key> (§7.6)
    <plan>/logs/pr-<key>.pre-start.log     full output of the last `pre-start` in a gate worktree of PR <key>, run before that Gate (§7.6, hs-d243)
    <plan>/logs/lead-<key>.<hook>.log      full output of a hook run in PR <key>'s Lead worktree: `pre-start` at the PR's start (§7.2)
    context-digests/<context>/<upto_seq>.md      the Context digest (§4.6): derived, never a Context record; the latest is the highest <upto_seq>
    context-digests/<context>/<upto_seq>.md.tmp  the distiller's output before it is validated; renamed to <upto_seq>.md when valid, else removed
    context-digests/<context>/prompt.md          the distiller's prompt, written by each run (§4.6)
    context-digests/<context>/records.md         the records in force a run gives the distiller (§4.6)
    context-digests/<context>/distill.log        every distill run's stderr, appended; the engine's runs also their stdout (§3.7 E9)
```

Locks are `flock(2)` locks on these files: the kernel releases them when the holder dies, so there are no stale-lock takeovers (a change from cpd 1 §4, whose pid files needed them).

There is **no per-repo lock** (hs-d243): merges, `pr open`, `pr rebase`, `pr push` and the clean-up after a PR's merge take no lock shared by two PRs or two Plans; §7 says what replaces it. A file `locks/repo-<sha1>.lock` left by an earlier cpd is inert: nothing opens it, and nothing needs to delete it.

## 2. Model and identifiers

### 2.1 Identifiers

| Thing | Format | Unique within | Example |
|---|---|---|---|
| Context id | `^[a-z][a-z0-9-]{1,39}$` | this machine (Contexts are machine-local, §4.6) | `building-cpd` |
| Machine name | `^[a-z][a-z0-9]{0,7}$`, set in `config.toml` (§8) | the User's machines | `hs` |
| Context record id | `<machine>-<letter><n>`; letter `d` decision, `n` note, `q` question, `a` answer, `p` plan; `n` from 1, sequential per machine, Context and letter | the Context | `hs-d3` |
| Plan id | `^[a-z][a-z0-9-]{1,23}$` (cpd 1's Workflow id) | the machine; `pr add` and every PR start (§3.8) also refuse a PR whose Lead Branch `cpd/<plan>/<key>`, or the branch `cpd/<plan>` itself (git cannot hold `cpd/<plan>` and `cpd/<plan>/…` at once), already exists in the PR's repo (`branch_exists`), so cpd 1 Workflows and cpd 2 Plans on one repo never share a branch. The one exception is the legacy branch `cpd/<plan>` of a `merged` PR, which `pr add` first moves to `cpd/<plan>/<that PR's key>` (§3.8, hs-d118) | `pay-4521` |
| PR key (R34) | `^[a-z][a-z0-9-]{0,23}$`, chosen by the Lead (`pr add`); invalid: `invalid_pr_id` (exit 2) | the Plan (a `cancelled` PR keeps its key) | `schema`, `web` |
| Job key | `^[a-z][a-z0-9-]{0,31}$` | the Plan | `api-schema` |
| Attempt id | `<job>-a<n>`, `n` from 1 per Job | the Plan | `api-schema-a2` |
| Escalation id | `<attempt>-q<k>`, `k` from 1 per Attempt | the Plan | `api-schema-a2-q1` |
| Plan review id | `r<k>`, `k` from 1 per Plan | the Plan | `r3` |
| Session id | pi's session id. cpd chooses it for every session it starts (`pi --session-id`): a lowercase UUID v4. Accepted on input: `^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$` | the machine | `596d007f-6775-4134-b77a-b356cc523842` |
| Event id | positive integer, assigned by SQLite, strictly increasing in commit order, never reused | the machine | `48213` |
| Lead Branch | `cpd/<plan>/<key>`, one per PR (R34); a PR migrated from a Plan made before R34 keeps `cpd/<plan>` (§4.5) | the repo | `cpd/pay-4521/api` |
| Attempt branch | `cpd/<plan>--<attempt>`, in the repo of the Job's PR (unchanged by R34: flat, a sibling of `cpd/<plan>/` and never inside it; t3) | the repo | `cpd/pay-4521--api-schema-a2` |
| Lead worktree | `<repo-parent>/<repo-name>.<plan>.<key>`, one per PR, `<repo>` the PR's repo (R34); a migrated PR keeps `<repo-parent>/<repo-name>.<plan>` | the filesystem | `~/projects/pay.pay-4521.api` |
| Attempt worktree | `<repo-parent>/<repo-name>.<plan>--<attempt>`, `<repo>` the repo of the Job's PR | the filesystem | `~/projects/pay.pay-4521--api-schema-a2` |

Within a Plan, Jobs, Attempts and Escalations are addressed by their id plus the Plan; across Plans, by the pair. An invalid id is a usage error (`invalid_<thing>_id`, exit 2).

### 2.2 Objects

Types: `text`, `int`, `bool`, `time` (§1.3), `json` (a JSON value of the stated shape), `?` = nullable. The storage of each field is the DDL in §4.4.

#### Context

The knowledge several Plans share, optionally a Jira Epic. Not a session (ADR-0011).

| Field | Type | Meaning |
|---|---|---|
| `id` | text | Context id (§2.1) |
| `title` | text | the visible label; editable (`context rename`). The id is immutable and need not be readable |
| `jira` | text? | Jira Epic key, stored only |
| `created_at`, `created_on` | time, text | when and on which machine `context new` ran |
| `archived_at` | time? | null, or when the Context was archived (migration 3, §4.5); see "Archived" below |
| records | [Context record] | append-only, below |
| Manager | Session? | not a field: the Context's one live (not archived) Session of role `manager` (§3.1), if any |

#### Context record

One typed, append-only row of `context_records`, a Context's only store (§4.6). Written only by `cpd context …` commands and by `plan new` (the `plan` kind, and the `brief` kind with `--brief-file`), each in one database transaction.

| Field | Type | Meaning |
|---|---|---|
| `id` | text | `<machine>-<letter><n>` (§2.1) |
| `kind` | text | `decision`, `note`, `question`, `answer`, `plan` or `brief` |
| `at`, `machine` | time, text | when and where it was written |
| `by` | json `{session: text?, plan: text?}` | the writing Session (`CPD_SESSION`, else `PI_SESSION_ID`, else null) and its Plan |
| `text` | text | the decision, note, question or answer; for `plan`, the Plan's title; for `brief`, the Plan's brief (B3-A4) |
| `supersedes` | text? | `decision` only: the id of the decision it replaces (must exist) |
| `question` | text? | `answer` only, required: the id of the question it answers (must exist) |
| `plan` | json? | `plan` and `brief` only, required: `{id, machine, jira?}` (a `brief` has `{id, machine}`); references the Plan by id and machine. R34 (A12): no `repo`, because a Plan has one or more PRs and none when it is created; a record written before R34 may still carry `repo`, which migration 0007's import drops (§4.5) |

A decision is **active** unless a later decision supersedes it. A question is **open** until an answer names it. The rendering Leads read (`context show`) lists, in order: title and Jira key; Plans, each with the repos of its PRs that are not `cancelled`, read from this machine's database (a Plan of another machine shows none); briefs, each listed short (its id and first line) under its Plan, never in full (`context show --record <id>` prints one record in full); active decisions; superseded decisions, each naming its successor; open questions; answered questions with their answers; notes. Each item shows its id and date. Commands (detail in `commands.md`): `context new`, `list`, `show`, `decide`, `note`, `ask`, `answer`, `manager`, `distill`. The letter of a `brief` is `b`. The **Context digest** (§4.6, hs-d194) is not a record: it is a file derived from the records in force, rebuilt from them at any time.

#### Plan

One deliverable over one or more PRs (R34), optionally a Jira task. This was cpd 1's Workflow (D3). Before R34 a Plan was one repo, one Lead Branch and one PR; the per-repo fields moved to the PR (below, A6).

| Field | Type | Meaning |
|---|---|---|
| `id` | text | Plan id |
| `context` | text | its Context (required: every Plan belongs to one) |
| `title`, `jira` | text, text? | `title` is the visible label and editable (`plan rename`); the id is immutable |
| `phase` | text | `grounding`, `plan`, `dispatch`, `deploy`, `done` (§3.1) |
| `lead_session` | text | the Plan's Lead Session; its `cwd` is the directory `plan new` ran from (A12), not a repo |
| prs | [PR] | one or more, below; none until the Lead adds one (`pr add`, §3.8) |
| `created_at`, `locked_at`, `done_at` | time, time?, time? | |
| `archived_at` | time? | null, or when the Plan was archived (migration 3, §4.5); see "Archived" below |

#### PR

The part of a Plan in one repo (R34, A4): one repo, one base, one Lead Branch and Lead worktree, its Jobs, its Gate run and one GitHub pull request opened from its Lead Branch. No other term names it (hs-d36). One repo may hold several PRs of one Plan, for example a contract change in repo A, then repo B, then A's integration: three PRs `a-contract`, `b`, `a-integration`, where `b` needs `a-contract` and `a-integration` needs `b`.

| Field | Type | Meaning |
|---|---|---|
| `plan`, `key` | text | the PR key (§2.1), unique in its Plan |
| `repo` | text | realpath of the repo's main worktree |
| `base` | text | the branch the pull request targets; default the repo's `origin` HEAD branch, else `main`; `pr add --base` overrides (cpd 1 §11 `workflow init`) |
| `needs` | [text] | keys of other PRs of the same Plan that must be `merged` before this one starts (A7, A9); acyclic; none may be `cancelled` when the need is recorded. `pr add` defaults them, in phase `deploy` or `done`, to every `merged` PR of the Plan, unless `--needs` or `--no-needs` is given |
| `status` | text | `waiting`, `dispatch`, `open`, `merged`, `cancelled` (§3.8) |
| `lead_branch` | text | `cpd/<plan>/<key>` (§2.1); set by `pr add` |
| `lead_worktree` | text? | `<repo-parent>/<repo-name>.<plan>.<key>`; set when the PR starts (§3.8, §7.2), null while `waiting` or `cancelled` |
| `pr` | json? | `{number, url, opened_at}`, set by `pr open`; `opened_at` is when the PR became `open` |
| `pull_request` | integer? | the id of its Pull request row (below; hs-d96, migration 6, §4.5): set by `pr open`, non-null exactly when the status is `open` or `merged`. What the poll last read from the pull request (title, review decision, checks, state) is in that row, not here |
| `all_merged_at` | time? | when this PR's `all-merged` was last appended; `job add` clears it when it appends a Job to the PR (hs-d140, §3.7 E4), so `all-merged` is sent once per completion, not once per PR |
| `start_failed_at` | time? | set when the engine failed to start the PR (A9); while set, the engine does not try again and `pr start` retries; cleared when the PR starts |
| `created_at`, `started_at`, `merged_at` | time, time?, time? | `started_at` when it became `dispatch`, `merged_at` when `pr poll` saw it merged |

The PR object is the cpd record; "pull request" names the object on GitHub that its `pr` field points to.

#### Pull request

What the PR poller (hs-d65, hs-d72, hs-d96, hs-d97, hs-d98, hs-d107, §7.6) last read of one of the User's GitHub pull requests: one row per pull request, kept for the Dispatch column (commands.md §13.2). A PR (above) points to its row; the User's other pull requests have a row and no PR. Table `pull_requests`, migration 6; `mergeable` and `merge_state` come with migration 11, `authored` and `review_requested` with migration 0013 (§4.4, §4.5). A row is there because the User wrote the pull request, because it requests the User's review, or both.

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | the row's id; `prs.pull_request` refers to it |
| `repo` | text | GitHub's `owner/name` (`nameWithOwner`), not a path |
| `number` | integer | the pull request's number; `(repo, number)` is unique |
| `url` | text | its URL |
| `title` | text | its title |
| `draft` | bool | whether it is a draft |
| `state` | text | `open`, `closed` or `merged` |
| `review_decision` | text | `none`, `approved` or `changes_requested` (§7.6 maps gh's value) |
| `checks` | text | `none`, `passed`, `pending` or `failed` (§7.6 maps gh's value) |
| `mergeable` | text | `mergeable`, `conflicting` or `unknown`: GitHub's `PullRequest.mergeable` (`MERGEABLE`, `CONFLICTING`, `UNKNOWN`), lowercased (§7.6). Stored as each poll reads it, `unknown` included: GitHub computes it lazily after the base moves. `pr open` inserts `unknown` |
| `merge_state` | text | `behind`, `blocked`, `clean`, `dirty`, `draft`, `has_hooks`, `unknown` or `unstable`: GitHub's `mergeStateStatus`, lowercased (§7.6). Stored as each poll reads it. `pr open` inserts `unknown` |
| `authored` | bool | 1 when the User wrote it: the pull request is in the `author:@me` search (§7.6). Set by each poll |
| `review_requested` | bool | 1 when the User's review is requested: the pull request is in the `review-requested:@me` search (§7.6). Set by each poll; the Inbox kind `review-requested` (§2.2 Inbox item) reads it |
| `created_at` | time | when GitHub created it (GitHub's time, not cpd's) |
| `merged_at` | time? | when it was merged; set exactly when `state` is `merged` |
| `changed_at` | time | cpd's time of the last change of any other field (`mergeable`, `merge_state`, `authored` and `review_requested` included); a poll that finds nothing changed writes nothing (§7.6) |

A row is deleted when its pull request leaves both open searches and no PR points to it; a row a PR points to is never deleted (§4.4). The row appends no Event of its own (§5.1); `pr-feedback` and `pr-merged` come from the transitions it causes (§5.2).

#### Plan review

One request for the User's approval of the draft Jobs, through plannotator (cpd 1 §11 `plan decision`, §15).

In phase `plan` it covers every draft Job (all of them are drafts) and the PRs, and its approval is what `plan lock` needs. In every phase after `plan` it covers **exactly the draft Jobs** the Plan has at that moment: a Lead that plans several batches adds the later ones as drafts (`job add --draft`, §3.2), and the User approves each batch through a review of its own. Its approval queues those drafts (§3.2) and there is no `plan lock` in these phases; the Jobs already Locked are shown to the User as context, not for approval. Reason: every planned batch must be visible in the Plan view before it is approved, and approved only by the User (User, hs-d173, hs-d174, hs-d179).

| Field | Type | Meaning |
|---|---|---|
| `plan`, `id` | text | `r<k>` |
| `fingerprint` | text | the plan fingerprint (§2.3) the review was rendered from |
| `status` | text | `pending`, `approved`, `rejected` |
| `feedback` | text? | the User's feedback |
| `requested_at`, `decided_at` | time, time? | |

#### Job and Spec

A self-contained unit of work. Its **Spec** is the fields `goal`, `context`, `touches`, `acceptance` and `out_of_scope`; with `title`, `tier`, `pr`, `review` and the Job's Dependencies, they are the Job's **Locked fields**. A Job runs in its PR's repo: its Attempts' worktrees are cut from, and merged into, its PR's Lead Branch (§7).

| Field | Type | Meaning |
|---|---|---|
| `plan`, `key` | text | |
| `pr` | text | the key of the Job's PR (R34, A7): every Job belongs to exactly one PR; set by `job add --pr` (§1.3), changed only by `job edit --pr` while `draft`; a Locked field |
| `review` | bool | a **review Job** (B2, below): set by `job add --review`, never changed (`job edit` has no such flag); default 0 (every Job before B2 is 0); a Locked field |
| `seq` | int | machine-wide creation order (scheduling order, §3.2) |
| `urgent` | bool | the **urgent** flag (User, Context record "Urgent Jobs", hs-d238): set by `job add --urgent`, or by `job urgent` on a Job that is `queued` or `draft`; default 0; engine E3 starts urgent Ready Jobs first and may run one beyond `concurrency` (§3.7). **Not a Locked field** and not in any fingerprint (§2.3): it changes no Spec, so a Locked Job may become urgent. A Lead or Manager marks a Job urgent **only when the User explicitly asks**, usually for something that blocks all work or for an incident |
| `title` | text | |
| `tier` | text? | Tier (§9); null only while `draft` or `cancelled` (a stub draft) |
| `goal`, `context` | text? | Spec; null only while `draft` or `cancelled` |
| `touches` | json [text]?, ≥ 1 | Spec: files or globs the Job may touch; null only while `draft` or `cancelled` |
| `acceptance` | json [text]?, ≥ 1 | Spec: checks that must hold; null only while `draft` or `cancelled` |
| `out_of_scope` | json [text]? | Spec; null only while `draft` or `cancelled` |
| `status` | text | §3.2 |
| `appended` | bool | added after `plan lock`; a draft added in a later phase (`job add --draft`, §3.2) is appended too, from the add |
| `fingerprint` | text? | Locked-Job fingerprint (§2.3); null exactly while `draft` (and for a draft that was cancelled), in every phase |
| `retry_tier` | text? | the Tier `job retry` chose for the next Attempt |
| `cancel_reason`, `cancel_requested_at` | text?, time? | set by `job cancel` |
| `failed_cause` | text? | the cause that failed the Job (§3.4); set exactly while `failed` |
| `merged_sha` | text? | the Lead Branch commit after the merge; set exactly when `merged` |
| `created_at` | time | |

**Stub draft** (hs-d241, R1). A `draft` Job may lack its Tier, its Spec, or both: `job add <k> --draft --title <t>` needs neither `--tier` nor `--spec` (commands §5.1), so a Lead can register planned work the moment it is planned (hs-d239). The five Spec fields are null together (all given by `--spec`, or none) and the Tier is null or valid. A draft is editable, so `job edit <k> --tier <T> --spec <f>` completes it (commands §5.2). A draft that lacks its Spec or Tier is **incomplete**: it can be neither approved (`plan review` and `plan lock` are refused `draft_incomplete`, exit 3, `details.jobs` listing each key and what it lacks) nor queued or run (§3.2). The nulls hold only while `status` is `draft` or `cancelled` (§4.4 CHECK): a stub can be cancelled (`job cancel`) and keeps its nulls; a Job that is queued, running, merged or failed has every field.

**Review Job** (B2, hs-d70). A Job with `review = 1`: its Worker is a **fork of the Plan's Lead Session**, so it knows the intention, instead of a fresh session (§3.4, §6.5). It reads its PR's Lead Branch diff against the PR's base and the Plan's Specs, and writes its findings to `run/<plan>/reviews/<k>.md` (§1.4). It commits nothing: `attempt submit` without commits is allowed only for a review Job, which then ends `merged` with no change to the Lead Branch (§3.4, §7.4). Its Dependencies are set by `job add --review` to every Job of **its PR** that exists at that moment and is not `cancelled`; Jobs appended later (CI or PR-feedback fixes) do not become its Dependencies. The Lead turns the findings into fix Jobs before `pr open`; a final review Job is optional, and the Lead adds one only when it makes sense (B3); no local review is required before the Gate (§3.9). It belongs to a PR like every Job (A7). This is not cpd 1's Reviewer (removed by R19): it is an ordinary Job of the Plan, once, where the Lead wants it, and no Gate or verdict belongs to it.

*Decided where B2 is silent:* (1) "every Job of the Plan" is read as every Job **of its PR**, because Dependencies stay inside one PR (A7); a review of another PR is a review Job of that PR. (2) A review Job is its own Job, so a later review Job of the same PR depends on an earlier one.

#### Dependency

`(plan, job, needs)`: Job `job` may start only after Job `needs` is Merged. Both are in the same Plan **and the same PR** (R34, A7: `needs_other_pr`, exit 3); order across PRs is the PRs' `needs`. The graph is acyclic; `needs` may not be `cancelled` or `failed` when the Dependency is created (cpd 1 §11 `job add`). Dependencies are Locked with their Job.

#### Attempt

One execution of a Job by one fresh Worker session, in its own worktree.

| Field | Type | Meaning |
|---|---|---|
| `plan`, `id`, `job`, `number` | text, text, text, int | `id = <job>-a<number>` |
| `tier` | text | §9 |
| `origin` | text | `ladder` (created by the engine) or `retry` (after `job retry`) |
| `status` | text | §3.4 |
| `outcome`, `cause` | text?, text? | set together, exactly when `status = ended` (§3.4) |
| `branch`, `worktree` | text | §2.1 |
| `base_sha` | text? | Lead Branch HEAD the worktree was cut from |
| `driver_pid` | int? | pid of `cpd attempt run` |
| `worker_session` | text? | the Worker's Session |
| `spec_path` | text? | rendered Worker Spec |
| `submitted_sha`, `submitted_at` | text?, time? | set by `attempt submit` |
| `check_tail` | text? | last 60 lines of the quick check at submit (§7.3) |
| `merged_sha` | text? | set exactly when the outcome is `merged` |
| `nudged` | bool | the "you ended your turn without submitting" message was sent (§3.4) |
| `stop_request` | text? | `timed_out` or `cancelled`, set by the engine for the driver to act on |
| `metrics` | json? | cpd 1 §14, kept: `{turns, tokens_in, tokens_out, cache_read, cache_write, cost_usd, duration_s, model, thinking_level, stop_reason}` |
| `started_at`, `ended_at` | time, time? | |

#### Escalation

A question from a Worker to its Lead (`cpd ask`).

| Field | Type | Meaning |
|---|---|---|
| `plan`, `id`, `attempt`, `number` | | `id = <attempt>-q<number>` |
| `question` | text | |
| `status` | text | `open`, `answered`, `expired` (§3.5) |
| `answer` | text? | set exactly when `answered` |
| `asker_pid` | int | pid of the blocking `cpd ask` |
| `asked_at`, `answered_at`, `expired_at` | time, time?, time? | |

#### Session

A pi session cpd knows about: every Manager, Lead and Worker is one.

| Field | Type | Meaning |
|---|---|---|
| `id` | text | pi session id |
| `role` | text | `manager`, `lead`, `worker`. A Manager is the Context's higher-level Lead (hs-d83..d86, §3.1); at most one live (not archived) per Context |
| `context` | text | every Session belongs to a Context (vision: "a new session must belong to a Context") |
| `plan` | text? | null exactly for a Context's Manager |
| `attempt` | text? | set exactly for Workers |
| `name` | text | pi display name (`-n`); 1–80 characters, no control characters |
| `file` | text? | absolute path of the session JSONL, known once pi has created it |
| `forked_from` | text? | the Session it was forked from. A fork is never drawn under its source or a Plan: §6.8 (B1) |
| `cwd`, `tier` | text | a Manager's `tier` is the config key `manager_tier` (§8.2); its `cwd` is the directory `context manager` ran from |
| `tmux_pane` | text? | `%<n>` pane id on the cpd tmux server; null when it has no pane |
| `state` | text | `working`, `blocked`, `idle`, `stopped` (§3.6) |
| `state_label` | text? | why it is blocked (for example `cpd: Worker question`); set only while `blocked` |
| `state_at`, `created_at` | time | |
| `archived_at` | time? | null, or when the Session was archived (migration 3, §4.5); see "Archived" below |

#### Archived

Context, Plan and Session carry `archived_at`. Archiving hides an object from the cockpit's tree (it collapses into a `…` row); it deletes nothing and changes no phase, status or state. Ids are unique and immutable; the **title** (Context, Plan) or **name** (Session) is the visible label.

- **Refused while live** (`archive_blocked`, exit 3, `details.blockers` lists each): a Session in state `working` or `blocked`; a Job `queued` or `running`; an open Escalation; a pending Plan review. The Lead in the chat slot is refused with `in_chat_slot`. A Plan's own Lead (`plans.lead_session`) is archived only with its Plan. Phases do not matter: a Plan in `deploy` is archivable, and the engine keeps polling its PR.
- **Cascade, all or nothing:** a Context archives with its Plans, all their Sessions and its Manager; a Plan with its Lead and side forks. Every object is checked first (nothing is written if any blocks); then every live Lead pane is closed as `session close` does (§6.4: pi stops, the session files stay); then `archived_at` is set and the Events appended in one transaction, which re-checks the rule. Unarchiving clears `archived_at` with the same cascade and touches no pane; `session open` resumes a Lead later. Unarchiving a Plan or Session whose Context (or Plan) is archived is refused with `archived`.
- **Refused on an archived object** (`archived`, exit 3, `details.next` is the unarchive command): `session open` and `session fork` of an archived Session, `plan new` in an archived Context.

#### Event

An append-only record of something that happened (D4, §5).

| Field | Type | Meaning |
|---|---|---|
| `id` | int | §2.1 |
| `at` | time | |
| `kind` | text | §5.2 |
| `context`, `plan`, `job`, `attempt`, `session` | text? | what it is about; which are set depends on the kind |
| `text` | text | one short paragraph for the Lead, ending with the exact `cpd` command(s) it may want to run (cpd 1 §10, kept) |
| `data` | json object | kind-specific fields (§5.2) |

#### Inbox item

One thing that needs the User: the **single source of "needs you"** (hs-d165), shown by `cpd inbox` and the cockpit. The engine opens it when its condition starts and closes it when the condition ends (E8, §3.7); nobody else does. Its identity is (`kind`, `subject`, `urgency`): when the urgency changes (a Plan locks while a dialog is open), the item closes and a new one opens. A closed item is kept forever (hs-d168).

| Field | Type | Meaning |
|---|---|---|
| `id` | int | |
| `kind` | text | the table below |
| `urgency` | text | `red` (blocked work), `yellow` (review or merge due), `magenta` (a decision before the work runs), `white` (an unread turn) |
| `subject` | text | the identity key: `session:<uuid>`, `review:<plan>/<id>`, `job:<plan>/<key>`, `error:<step>/<scope>/<code>`, `record:<context>/<id>`, `pr:<owner>/<repo>#<n>` (review ids are per Plan and record ids repeat across Contexts, so both are qualified) |
| `context`, `plan`, `job`, `session`, `record` | text? | what it is about, when it has one |
| `event` | int? | the Event that caused it (the `session-state`, `job-failed` or `error` Event), for "jump to the cause" |
| `label` | text | the short text of the table below |
| `url` | text? | a page that shows it (plannotator's for a Plan review; the pull request's page for the `pr-*` and `review-requested` kinds) |
| `opened_at` | time | |
| `closed_at` | time? | null while open |
| `snoozed_until` | time? | hidden until then; the item returns at its rank, `opened_at` kept |
| `dismissed_at` | time? | hidden for the rest of its life; a new cause is a new item and shows again |

A hidden (snoozed or dismissed) item fills no dot and is not counted. Escalations from Workers go to their Lead, not to the User, so they are not items; nor are the errors addressed to a Lead (`pr_start_failed`, `push_failed`, driver errors).

| Kind | Condition (subject) | Urgency | Label (`<who>`: the tree's label of the Session: its Plan id, `<plan>·<suffix>` for a fork, the Context id for a Manager) |
|---|---|---|---|
| `dialog` | a Lead, side fork or Manager Session (never a Worker) is `blocked` with a label other than `cpd: Worker question` and `cpd: plan review` (the Session) | red when its Plan is in `dispatch` or `pr`; magenta in `grounding`, `plan`, `done` or with no Plan | `<who> asks you` |
| `plan-review` | the Plan's latest Plan review is `pending` (the review) | magenta | `<plan> plan review` |
| `unread` | a Lead, side fork or Manager Session is `idle`, not in the chat slot, and its latest `session-state` Event from `working` or `blocked` to `idle` **with `data.reply` true** is newer than its latest `session-moved` with `place: chat` (the Session); only a finished turn that answered a message the User wrote to that Session is a reply (hs-d278, refining hs-d242): a turn started by an engine Event or by another Session's message (the Manager, a Lead, a Worker) is not, and a later turn that is not a reply does not hide an unread reply; a start or reload (`stopped` → `idle`) is not a finished turn (hs-d242) | white | `<who> replied` |
| `job-stuck` | a Job is `failed` and its latest `job-failed` Event is at least `inbox.stuck_job_minutes` (§8.2) old; it clears on retry or cancel (the Job) | red | `<plan>/<job> failed` |
| `engine-error` | an `error` the engine itself reported (a failure of E1–E9, §3.7) has not cleared (step, scope and code) | red | `engine <step>: <code>` |
| `engine-down` | `locks/watch.lock` is free. **Not stored**: the engine cannot report its own absence, so `cpd inbox list` adds it, first, with `id: null` | red | `engine is down` |
| `question` | a Context record of kind `question` has no `answer` record (the record) | magenta | `<record id> open question` |
| `pr-failing` | an open pull request the User authored (`authored` 1) has `checks` `failed`, **or** `mergeable` `conflicting` (hs-d188 (3)) (`pr:<owner>/<repo>#<n>`) | red | `<repo>#<n> checks failed`; `<repo>#<n> merge conflict` when only the conflict holds when it opens |
| `pr-changes` | an open pull request the User authored has `review_decision` `changes_requested` (the pull request) | red | `<repo>#<n> changes requested` |
| `pr-ready` | an open, non-draft pull request the User authored has `checks` `passed`, `mergeable` `mergeable` and no `changes_requested` (the pull request) | yellow | `<repo>#<n> ready to merge` |
| `review-requested` | an open pull request requests the User's review (`review_requested` 1) (the pull request) | yellow | `review <repo>#<n>` |

The four pull request kinds read the `pull_requests` row the poll last wrote (§7.6): `url` is the row's `url`, the pull request's page, and `<repo>` is the row's `repo` without the owner. When a Plan's PR points to the row, the item also links that Plan and its Context. `mergeable` and `merge_state` are only read here; the PR poll's mergeable state defines them (§2.2 Pull request, §7.6). Labels are short, for a column of 20 % of 215 columns; the cockpit truncates with `…`.

**Order** (hs-d167): red, then yellow, magenta, white; within one urgency the oldest `opened_at` first. `engine-down` is always first.

**Snooze and dismiss.** `inbox snooze` hides an item until a time (15 minutes, 1 hour, or tomorrow 09:00 local time); `inbox dismiss` hides it for the rest of its life (commands.md §21).

### 2.3 Fingerprints

- **Locked-Job fingerprint:** `sha256` of the canonical JSON (keys sorted, no whitespace) of `{goal, tier, context, touches, acceptance, out_of_scope, needs}` with `needs` sorted. Stored at `plan lock`, at append, and, for a draft added after the lock, when its Plan review is approved (§3.2). Every engine step that reads a Job recomputes it; a mismatch is `locked_job_tampered` (exit 3) and the Job is not scheduled (cpd 1 §8, kept).
- **Plan fingerprint:** `sha256` of the canonical JSON object `{jobs, prs}` (R34): `jobs` is the array of the Locked fields of every `draft` Job, `pr` included, ordered by key (cpd 1 §11 `plan render`, kept), a stub draft with its missing fields as JSON `null`, as it is (title only, hs-d241), so completing it changes the fingerprint; `prs` is the array of `{key, repo, base, needs}` of every PR that is not `cancelled`, `needs` sorted, ordered by key, so adding, editing or cancelling a PR changes it (A13). In a phase after `plan` the Plan review covers the drafts only (§2.2), so the fingerprint is the same object with `prs` the empty array: the PRs added after the lock are appended and not reviewed, so adding one must not void a review of drafts. `plan lock` requires the latest Plan review to be `approved` for the current plan fingerprint. A Plan in phase `plan` when migration 4 runs (§4.5) therefore needs a new review before it can be locked.
- `urgent` is in **neither** fingerprint: it is not a Locked field (§2.2), so `job urgent` on a Locked Job leaves its fingerprint as stored and `jobs_locked` (§4.4) does not list it.
- The Locked-Job fingerprint does **not** include `pr`: adding it would invalidate every fingerprint stored before R34. A Locked Job's `pr` is guarded by the trigger `jobs_locked` instead (§4.4). The same holds for `review` (B2): it is not in the fingerprint, which would invalidate every fingerprint stored before it, and `jobs_locked` guards it.

### 2.4 Relations

```mermaid
erDiagram
  CONTEXT ||--o{ CONTEXT_RECORD : "has (append-only)"
  CONTEXT ||--o{ PLAN : "parent of"
  CONTEXT |o--o| SESSION : "Manager (one live)"
  PLAN ||--|| SESSION : "Lead"
  PLAN ||--o{ PLAN_REVIEW : "reviewed by"
  PLAN ||--o{ PR : "delivered by"
  PR ||--o{ PR_NEED : "needs"
  PR }o--o| PULL_REQUEST : "opened as"
  PR ||--o{ JOB : "contains"
  JOB ||--o{ DEPENDENCY : "needs"
  JOB ||--o{ ATTEMPT : "executed by"
  ATTEMPT ||--o| SESSION : "Worker"
  ATTEMPT ||--o{ ESCALATION : "asks"
  SESSION |o--o{ SESSION : "forked from"
  PLAN ||--o{ EVENT : "about"
  CONTEXT ||--o{ EVENT : "about"
```

- A Context has many Plans; a Plan has exactly one Context and one Lead Session.
- A new Plan's Lead is always a fresh Lead that starts from its brief and the Context's records (§3.1); there is no fork of D2 any more (hs-d92). A Context has at most one live Manager (§3.1).
- A Plan has one or more PRs once the Lead adds them (R34; none right after `plan new`, and `plan lock` requires one, §3.1). A PR belongs to exactly one Plan.
- A PR points to **0..1 Pull request** (§2.2): none while `waiting` or `dispatch`, exactly one when `open` or `merged` (hs-d96). A Pull request has 0..n PRs pointing to it: none for the User's other pull requests, which the poller still stores.
- A PR **needs** zero or more other PRs of the same Plan; the needs graph is acyclic (`dependency_cycle`, exit 3). A PR starts only when every PR it needs is `merged` (§3.8).
- A PR has one or more Jobs by `plan lock` (`pr_empty`, exit 3); a Job belongs to exactly one PR of its Plan (A7). Dependencies link Jobs of the same PR only; order across PRs is the PRs' needs.
- A Job has zero or more Attempts, at most one of them not `ended`.
- An Attempt has one Worker Session. An Escalation belongs to one Attempt.
- An Inbox item links to at most one Context, Plan, Job, Session, Context record and Event (§2.2): the ones its condition is about, all optional.
- Every Event about a Context or a Plan names the Context, and the Plan, Job, Attempt or Session it is about, when there is one. Events about neither, such as `watch-started` and `watch-stopped`, name no Context (§5.2).

### 2.5 PRs in commands and views (R34)

The command surface of PRs; `commands.md` specifies each command in full.

| Command | Phases | What it does |
|---|---|---|
| `plan new <id> --context <c> --title <t> …` | | no `--repo` and no `--base` any more (A12): the Plan has no repo; the Lead starts in the directory `plan new` runs from (usually `~`, §6.4) |
| `pr add <key> --repo <path> [--base <b>] [--needs <k1,k2>]` | `grounding`, `plan`, `dispatch`, `deploy`, `done` | adds a PR, `waiting` (§3.8). In `grounding` and `plan` it is part of the plan fingerprint (§2.3); in `dispatch`, `deploy` and `done` it is **appended**, like `job add`, and the engine starts it when its needs are merged (E7). In `deploy` and `done` it also moves the Plan back to `dispatch` (§3.1). Refused on an archived Plan (`archived`) |
| `pr edit <key> [--repo <path>] [--base <b>] [--needs <k1,k2>]` | `grounding`, `plan` | changes a `waiting` PR; `--needs` replaces the whole list. After the lock: `pr_locked` (exit 3) |
| `pr cancel <key>` | `grounding`, `plan` | `waiting → cancelled`; refused `pr_in_use` (exit 3) while a Job of the PR is not `cancelled` or a PR that is not `cancelled` needs it |
| `pr start <key>` | `dispatch` | starts a `waiting` PR whose needs are all `merged` (`pr_not_ready`, exit 3): the retry after a failed start (A9) |
| `job add --draft` | `plan`, `dispatch`, `deploy`, `done` | adds the Job as a `draft` in any phase (§3.2); the Plan review of a later phase approves the drafts (§2.2) |
| `job add --pr <key>` | as `job add` | the Job's PR, selected by the rule of §1.3; the PR must not be `merged` or `cancelled` (`pr_ended`, exit 3) |
| `pr open --pr <key>`, `pr rebase --pr <key>`, `pr view --pr <key>` | as §7.6 | one PR, selected by the rule of §1.3 |
| `pr poll [--plan <id>]` | any | the PR poller (hs-d96): one poll of every open pull request of the User, run by engine E5 or a Lead (§7.6); no `--pr` |
| `plan show` | any | lists the Plan's PRs, each with its fields (§2.2) and its Jobs |

**The plan view** (`PlanView`, the cockpit's centre top; A15): one box per PR that is not `cancelled`, showing its key, its repo's directory name, its base, and its pull request's number and state (or its status before it is open), with the chart of its Jobs and their Dependencies inside. The boxes are laid out left to right in the order of the needs graph (a PR to the right of every PR it needs) and linked by arrows for the needs. A Plan of one PR shows one box.

## 3. State machines

Every transition below is the only way its state changes. Anything else is refused with `illegal_transition` (exit 3) by the command, and also by a trigger in the database (§4.4), so a bug cannot write it either. "Engine" steps are those of `cpd watch` (§3.7, E1–E8) and of the Attempt driver `cpd attempt run` (§3.4, A1–A9). Each row names the Event its transaction appends (§5.2).

### 3.1 Plan phases

The phases are the cpd side of the CPD flow, whose three stages are Context, Plan and Deploy: §3.9 maps each step of the flow to a phase and its commands.

```mermaid
stateDiagram-v2
  [*] --> grounding: plan new
  grounding --> plan: plan phase plan
  plan --> dispatch: plan lock
  dispatch --> deploy: pr open of the last PR not yet open
  deploy --> done: the poll sees the last PR merged
  deploy --> dispatch: pr add
  done --> dispatch: pr add
```

Before migration 0008 the phase `deploy` was called `pr` (hs-d76); old Events and migration entries above 0008 may still say `pr`.

| From | To | Moved by | Preconditions | Event |
|---|---|---|---|---|
| (none) | `grounding` | `plan new --context X`, run from any directory: it has no `--repo` (A12) | Context exists; Plan id free | `plan-created`, `session-registered` |
| `grounding` | `plan` | `plan phase plan` (the Lead) | none | `plan-phase` |
| `plan` | `dispatch` | `plan lock` | latest Plan review `approved` for the current plan fingerprint (`plan_not_approved`); at least one `draft` Job; at least one PR that is not `cancelled`, and each such PR has at least one `draft` Job (`pr_empty`); acyclic Dependency graph and acyclic needs graph (`dependency_cycle`); for every PR with no needs, its Lead Branch absent from its repo, and on the remote either absent or reusable, i.e. `origin/<base>` is its ancestor (`branch_exists`, §7.2) | `plan-phase`, and `pr-started` for each PR started. All `draft` Jobs of every PR become `queued` with their fingerprints at once (A8). Every PR with no needs **starts** (§3.8): its Lead worktree is created, reusing or creating and pushing its Lead Branch on the remote (§7.2), and it becomes `dispatch`; every other PR stays `waiting`. `watch` is started if it is not running (§6.6) |
| `dispatch` | `deploy` | `pr open --pr <key>` (§7.6), in the transaction that makes that PR `open` | afterwards every PR that is not `cancelled` is `open` or `merged` (A10) | `pr-opened`, then `plan-phase` |
| `deploy` | `done` | `pr poll` (engine E5), in the transaction that makes a PR `merged` (it sees the pull request `merged`, §7.6; hs-d96, D7) | afterwards every PR that is not `cancelled` is `merged` | `pr-merged`, then `plan-done`, appended once each time the Plan reaches `done` |
| `deploy` or `done` | `dispatch` | `pr add` (§3.8, D1), in the transaction that appends the PR | the Plan is not archived (`archived`); the other preconditions of `pr add` (§3.8) | `plan-phase` `{from: "deploy"\|"done", to: "dispatch"}`, then `pr-added` (`appended: true`). `plans.phase = 'dispatch'` and `done_at = NULL` in the same transaction |

**PRs across the phases** (R34, A8–A10). `grounding` and `plan`: the Lead adds, edits and cancels PRs (`pr add`, `pr edit`, `pr cancel`), all `waiting`; they are part of the plan fingerprint (§2.3). `dispatch`: from the lock until every PR that is not `cancelled` is `open` (or already `merged`): the PRs start as their needs are merged (§3.8, E7), their Jobs run, and each is opened by its own `pr open`; the poll (E5) runs in this phase too and handles every `open` PR, so a prerequisite PR merged during `dispatch` starts its dependents. `deploy`: every PR is `open` or `merged`; Jobs appended to an `open` PR reach its pull request through `pr rebase --pr <key>`. `done`: every PR that is not `cancelled` is `merged`. `deploy` and `done` take appended PRs too (hs-d87): a merged PR can hold a bug or a regression, so `pr add` in `deploy` or `done` appends the PR `waiting` and moves the Plan back to `dispatch` (the table above); engine E7 then starts it like any PR appended in `dispatch`, at once when it has no needs. When its last PR is opened the Plan goes `dispatch → deploy`, and when the last is merged `deploy → done`, as before; each arrival at `done` appends `plan-done` again. A reopened Plan has no Plan review of its own (D4): its appended PRs and Jobs work as appended work does in `dispatch`; only draft Jobs (`job add --draft`, §3.2) are reviewed, by a review of the drafts (§2.2). Only deleting or editing committed (Locked) Jobs stays forbidden. The lock is all or nothing across PRs: if any started PR's worktree or `pre-start` fails, every worktree and branch the lock created is removed, nothing is pushed and nothing in the database changes (§7.2).

**Grounding.** The phase in which the Lead grounds itself on the Context: it reads its first message (its brief and the rendered Context records), the material the User gives, and interviews the User (cpd 1 requirements §2 "Context", kept). The phase is always entered; a Lead whose brief and records already say enough adds to the Context with `context decide`, `note` or `ask` if it learns something, or moves on to `plan` at once. That is how "skipped or appended to" (ADR-0011) is represented: one phase name, `grounding`, and no special case. `plan phase plan` writes the phase and `plan-phase` and nothing else.

**`plan new` always starts a fresh Lead** (hs-d92): a Session registered `forked_from` null, whose first message is the Plan header, the full brief and the Context: its latest digest and at most 20 records written after it, or every Context record when the Context has no digest yet (§4.6 "Consumers", `commands.md` §4.1). With `--brief-file` it also writes the brief as a Context record of kind `brief` (§2.2, B3-A4).

**The Context's Manager** (hs-d83..d86, hs-d92, hs-d133; Plan manager B3). The higher-level Lead of a Context: it works at the Epic level, holds the Context's records and Requirements, splits the User's requirements into Plans, starts their Leads and knows where each is. Its memory is the records (records first; the chat is disposable). It acts only when the User talks to it; at each turn start the adapter gives it a digest (§5.3, `commands.md` §15.7). Leads keep asking the User directly; the Manager never answers for the User.
- **One live Manager per Context**: a Session with `role = manager`, `plan` null, `tier` = `manager_tier` (§8.2, default `O5`), name `<context>-manager`, `cwd` = the caller's directory (usually `~`). A unique index allows one non-archived Manager per Context (§4.4).
- **`cpd context manager [--context <c>]`** registers it when the Context has none live (`session-registered`), then creates its parked window `ctx-<context>` running `cpd session run <id>` and appends `session-moved` in a second transaction after the commit, as `plan new` starts a Lead (§6.5). When the Manager is `stopped` with no pane, it starts it again on its session file (resume); otherwise it changes nothing. It never moves the chat slot (hs-d103) and is idempotent. An archived Context is refused with `archived`, an unknown one with `context_not_found`.
- **First message**: `You are the Manager of Context <c> "<title>". Follow the cpd-manager skill.` followed by the Context as `plan new` gives it (§4.6 "Consumers"): its latest digest and at most 20 records written after it, or `context show` rendered as markdown (every record) when it has no digest yet.
- It follows only Events addressed to it (§5.3), so it is never woken by Plan Events (hs-d86).

**The grounded Lead is retired** (hs-d92, hs-d129). Until Plan manager B3 a Context's grounded Lead was a parked fork that every new Plan forked (D2, ADR-0011, R15), created by `plan phase plan` with the Event `context-grounded`. Migration 0010 (§4.5) dropped `contexts.lead_session` and `contexts.grounded_seq` and archived every grounded Lead Session. The Manager replaces its role.

### 3.2 Job statuses

```mermaid
stateDiagram-v2
  [*] --> draft: job add (phase plan), job add --draft (any later phase)
  [*] --> queued: job add (phase after plan, no --draft)
  draft --> queued: plan lock, or an approved review of the drafts
  queued --> running: engine E3 starts an Attempt
  running --> merged: driver A8 merge
  running --> queued: Attempt failed, ladder has a next Tier
  running --> failed: no next Tier, or a cause outside the ladder
  failed --> queued: job retry --tier
  draft --> cancelled: job cancel
  queued --> cancelled: job cancel
  failed --> cancelled: job cancel
  running --> cancelled: job cancel, then the engine stops the Attempt
  merged --> [*]
  cancelled --> [*]
```

| From | To | Moved by | Event |
|---|---|---|---|
| (none) | `draft` | `job add --pr <key>` in phase `plan`, into a PR that is `waiting` | `job-added` |
| (none) | `draft` | `job add --draft --pr <key>` in a phase after `plan` (`done` included; there every PR is `merged` or `cancelled`, so the add is refused `pr_ended` until a `pr add` has reopened the Plan, §3.1), into a PR that is `waiting`, `dispatch` or `open` (`merged` or `cancelled`: `pr_ended`): an **appended** draft (`appended = 1`), no fingerprint; the engine never runs it | `job-added` |
| (none) | `queued` | `job add --pr <key>` in a phase after `plan` without `--draft`, into a PR that is `waiting`, `dispatch` or `open` (`merged` or `cancelled`: `pr_ended`): an **appended** Job, fingerprint stored at once (cpd 1 §11 `job add`, kept); it may not need a `draft` Job (`needs_unavailable`: it could start before its need exists) | `job-added` |
| `draft` | `draft` | `job edit`, in any phase (Locked fields may change only here) | `job-edited` |
| `draft` | `queued` | `plan lock`; or, in a phase after `plan`, `plan decide --approve` of a review of the drafts (§2.2): in one transaction every draft becomes `queued`, gets its Locked-Job fingerprint (§2.3) and keeps `appended = 1`, as an appended Job has at `job add` | `plan-phase` (one Event for the whole lock); `plan-review` with `jobs: [key]` (one Event for the whole batch) |
| `queued` | `running` | engine E3, or `job dispatch` by hand: creates the Attempt (§3.4) | `attempt-started` |
| `running` | `merged` | driver A8: the merge succeeded; or R (§7.4) when the driver died after A8's compare-and-swap | `job-merged` |
| `running` | `queued` | driver or engine: the Attempt ended `failed` with a ladder cause (§3.4), the ladder has a next Tier and the automatic Attempts are fewer than `1 + retry_cap` (§9); or the Attempt's driver never started (`driver_pid` null, §3.4): the Job goes back to `queued` at the **same** Tier and the Attempt is not counted | `attempt-retry` |
| `running` | `failed` | driver or engine: no next Tier, the cap is reached (never for an Attempt whose driver never started), or the cause is outside the ladder | `job-failed` |
| `failed` | `queued` | `job retry <job> --tier <T>` (the Lead's decision; sets `retry_tier`) | `job-retried` |
| `draft`, `queued` | (unchanged; `urgent` set) | `job urgent` (§2.2): a flag, not a status change; a `queued` Job may be Locked | `job-edited` with `fields: ["urgent"]` |
| `draft`, `queued`, `failed` | `cancelled` | `job cancel --reason` with no active Attempt (a `draft` in any phase) | `job-cancelled` |
| `running` | (unchanged; `cancel_requested_at` set) | `job cancel --reason` with an active Attempt | `job-cancel-requested` |
| `running` | `cancelled` | engine E2 or the driver honours the request at its next safe point (§3.4) | `job-cancelled` |

A Job stays `running` from the start of its Attempt until it is `merged`, `queued` again, `failed` or `cancelled`; the Attempt's status (§3.4) says whether it is still being worked on or is being merged. Change (R19): there is no Job status `in_review`, because there is no Job-level review.

**Ready.** A `queued` Job whose every Dependency is `merged`, whose PR is `dispatch` or `open` (R34: a Job of a `waiting` PR waits for its PR to start, §3.8), in a Plan whose phase is `dispatch` or `deploy`.

**Review Job** (B2). `job add --review` sets its Dependencies to every non-`cancelled` Job of its PR at that moment, so it is Ready only when all of them are `merged` (a `failed` Job of the PR makes the add refuse `needs_unavailable`: retry or cancel it first). Its Attempt is §3.4's "A review Job's Attempt"; it ends `merged` like any Job and its `merged_sha` is the Lead Branch tip.

**Drafts in a running Plan** (User, hs-d173, hs-d174, hs-d179). A `draft` Job exists in every phase, not only in `plan`: `job add --draft` adds one after the lock, so that the Plan view shows every planned batch before it is approved. A draft is drawn as the draft pill (`◌`, grey) and the engine never schedules it (a Ready Job is `queued`). Drafts stay editable and cancellable in every phase (`job edit`, `job cancel`), and Dependencies may point at drafts; a `queued` Job may not need one (`needs_unavailable`). A draft becomes `queued` only at the approval of a review (§2.2), or at `plan lock` in phase `plan`; a rejected review leaves the drafts as they are, editable.

**Stubs are never queued** (hs-d241, R1). A `draft` that lacks its Spec or its Tier (§2.2) is never moved to `queued`: no review of it can be approved and `plan lock` refuses it (`draft_incomplete`, exit 3), so no transition above applies to it, and the engine never schedules a draft in any case. It leaves `draft` only by being completed with `job edit` and then approved, or by `job cancel`. The `jobs` CHECK refuses `draft → queued` on a Job with a null Tier or Spec field (§4.4).

**Locked.** Every Job that is not `draft` is Locked: its Locked fields and Dependencies can never change. It can only be re-attempted (the ladder, `job retry`) or cancelled, and new Jobs can be appended (ADR-0003, cpd 1 requirements §2, kept). `job edit` on a Locked Job is refused with `job_locked` (exit 3); a database trigger refuses the same write (§4.4).

**Cancel.** Kept from cpd 1 §11 `job cancel`, with one change: a cancel request is honoured even after the Worker submitted, up to the compare-and-swap of the merge; after it, the Job is Merged and the request is reported as too late in `job-merged`'s data (`cancel_too_late: true`). (cpd 1 could not cancel an Attempt once it was approved for merge: Workflow lead-fork's `session-rename-2` and `review-off-lead` stayed in cpd 1's `in_review` and were merged by hand with the Gate bypassed; `~/.cpd/plans/cpd-ui/fork-note.md`.) Dependents of a `cancelled` or `failed` Job stay blocked.

`merged` and `cancelled` are terminal.

### 3.3 Escalations in the Job's life

While an Escalation of the active Attempt is `open`, the Attempt's timeout clock is paused (§3.4) and the Worker Session is `blocked` (§3.6).

### 3.4 Attempt statuses, outcomes and causes

Statuses: `starting` → `running` → `merging` → `ended`. Outcome and cause are set exactly when the status becomes `ended`. Outcomes: `merged`, `failed`, `cancelled`. Change (R19): the statuses `reviewing` and `awaiting_lead`, the outcome `rejected` and the cause `review_rejected` are gone, and the success outcome is named `merged` (it was `approved`), because nothing approves an Attempt any more: a submitted Attempt goes straight to the merge.

```mermaid
stateDiagram-v2
  [*] --> starting: E3
  starting --> running: A3 Worker started
  starting --> ended: setup_failed
  running --> merging: attempt submit
  running --> ended: timed_out, crashed, no_submit, cancelled
  merging --> ended: merged, merge_conflict, cas_lost, tampered, cancelled, crashed
  ended --> [*]
```

| From | To | Outcome / cause | Moved by | Event |
|---|---|---|---|---|
| (none) | `starting` | | E3 or `job dispatch`: takes the driver lock, inserts the Attempt with its Tier (§9), and starts the driver `cpd attempt run` detached, handing it the lock (below) | `attempt-started` |
| `starting` | `running` | | A3: Worker pane started | `attempt-status` |
| `starting` | `ended` | `failed` / `setup_failed` | A1–A2: worktree creation or `pre-start` failed (cpd 1 §12 step 2, kept: no ladder) | `job-failed` |
| `running` | `merging` | | `attempt submit` passes its preconditions (§7.3) | `attempt-status` |
| `running` | `ended` | `failed` / `timed_out` | A5 after E2 set `stop_request = timed_out` (cpd 1 §12 step 7, kept: no ladder) | `job-failed` |
| `running` | `ended` | `failed` / `crashed` | A5: the Worker pane is dead, or pi stopped with a last assistant `stopReason` other than `stop` | `attempt-retry` or `job-failed` |
| `running` | `ended` | `failed` / `no_submit` | A5: the Worker went idle without submitting, was nudged once (`attempt-nudge`), and went idle again without submitting (cpd 1 §12 step 7, kept) | `attempt-retry` or `job-failed` |
| `merging` | `ended` | `merged` / `merged` | A8: compare-and-swap succeeded | `job-merged` |
| `merging` | `ended` | `failed` / `merge_conflict` | A8: rebase conflict | `attempt-retry` or `job-failed` |
| `merging` | `ended` | `failed` / `cas_lost`, `tampered` | A8 (§7.4). The cause `dirty_target` is no longer produced (hs-d243: the merge has no Lead worktree check, §7.4 step 3); the schema keeps it for the Attempts that ended with it before; `tampered` also when the Attempt branch's tip is not `submitted_sha` (§7.4 step 0) | `job-failed` |
| any but `ended` | `ended` | `cancelled` / `cancel_requested` | the driver at its next safe point (before A3, before A8's rebase, before A8's compare-and-swap); or, when the driver is dead, E2 (any status but `merging`) or the merge recovery R (`merging`, commits not on the Lead Branch, §7.4) | `job-cancelled` |
| any but `ended` and `merging` | `ended` | `failed` / `crashed` | E2: the driver's lock is free (the driver died) | `attempt-retry` or `job-failed` |
| `merging` | `ended` | `merged` / `merged` | R (§7.4): the driver's lock is free and the Attempt branch's tip is already on the Lead Branch (the driver died after A8's compare-and-swap); `merged_sha` = that tip | `job-merged` |
| `merging` | `ended` | `failed` / `crashed` | R (§7.4): the driver's lock is free and the tip is not on the Lead Branch | `attempt-retry` or `job-failed` |

**A review Job's Attempt** (B2; every row of the tables above holds, except as stated here).
- **A3, the Worker.** A review Job's Worker is a fork of the Plan's Lead Session (`plans.lead_session`), not a fresh session. The driver first **waits until the Lead is idle**, by §6.7 step 1 (every 2 seconds, with no limit; the Attempt stays `starting`, so the Attempt timeout, which counts from `running`, does not run, and a cancel request is honoured at the next safe point), then starts the Worker with the argv of §6.5 "Worker (review Job)": pi `--fork <lead session file>` plus the Worker's exclusions (`--approve --no-skills --exclude-tools …`). Its Session row is a Worker's (`role = worker`, `attempt` set, hidden in `cpd-work`, never in the tree), with `forked_from` = the Lead's Session, and `session-registered` carries that. The Lead is never interrupted and never takes a turn for it.
- **The Spec.** The rendered Worker Spec (`commands.md` §16.1) tells it to read the diff of the PR's Lead Branch against the PR's base and the Plan's Specs, to write its findings to `$CPD_HOME/run/<plan>/reviews/<k>.md`, to commit nothing, and then to submit.
- **Submit.** `attempt submit` for a review Job: the `no_commits` precondition is replaced by two (`commands.md` §6.1): the Attempt branch has **no** commits ahead of `base_sha` (`review_commits`, exit 3), and `run/<plan>/reviews/<k>.md` exists and is not empty (`review_missing`, exit 3). The quick check does not run (nothing was changed to check; `check_tail` stays null). A non-review Job submitted without commits is refused `no_commits`, as before.
- **The merge, A8.** `merging` → `ended` as usual, but with no rebase, no fast-forward, no push and no worktree sync (§7.4 "A review Job's merge"): the Lead Branch is unchanged, the Attempt ends `merged` / `merged` and the Job `merged` with `merged_sha` = the Lead Branch tip at that moment.
- **Everything else is a Worker's:** the ladder (a `crashed` or `no_submit` review Attempt is followed by a new Attempt, a new fork of the Lead), the nudge, Escalations (`cpd ask`), cancel, timeout and A9.

**Ladder causes** are `crashed`, `no_submit` and `merge_conflict`: the next Attempt starts from the newer Lead Branch HEAD and its Spec carries the previous outcome and the conflict list. **An Attempt whose driver never started does not use up a try.** The marker is `attempts.driver_pid` (§4): the starter sets it only after a successful spawn, so an Attempt that ended `failed` / `crashed` with `driver_pid` null never had a driver. Such an Attempt is not counted among the automatic Attempts of `retry_cap` (§9) and does not raise the Tier: the Job goes back to `queued` (`attempt-retry` with `next_tier` = the same Tier; the next Attempt has `origin = ladder`). No schema change and no new cause. Every other cause fails the Job, and the Lead decides (`job retry`, `job cancel`). Change (R19): the merge runs no Gate, so `gate_failed` and `gate_error` are no longer Attempt causes; the Gate runs at `pr open` and `pr rebase` (§7.6), and a failure there is fixed by Jobs the Lead appends.

**The driver, `cpd attempt run`** (internal; started detached by E3 or `job dispatch` with `setsid`, stderr to `run/<plan>/logs/attempt-<attempt>.log`; holds the driver lock, below, for its whole life). Steps, adapted from cpd 1 §12:

| Step | What it does |
|---|---|
| A1 | Check that fd 3 is the driver lock (below); load the Attempt and Job; recompute the Locked-Job fingerprint (§2.3) |
| A2 | Create the worktree from the HEAD of the Lead Branch of the Job's PR, in that PR's repo, and run `pre-start` (§7.2, §7.3); record `base_sha` |
| A3 | Render the Worker Spec to `run/<plan>/specs/<attempt>.md` (content: `commands.md`); start the Worker in the hidden session (§6.5); register its Session. A review Job's Worker is a fork of the Lead (below) |
| A4 | Wait. Active time is counted from the Attempt's `running` transition (never from `starting`, so a review Job's wait for its Lead does not count) and is wall time minus time with an `open` Escalation. The driver logs one line (`run/<plan>/logs/attempt-<attempt>.log`) every 10 minutes it waits, with no Event. At each tick, while the Worker's pi is alive and `timeouts.stall_seconds` is not `0`, the driver also checks for a **stall** (hs-d195): the Worker's session file (§6.5) has not been modified for `stall_seconds` **and** its last entry is an assistant message whose last content is a tool call with no tool result after it (pi is inside that tool call). It then kills the processes pi started for the call: every descendant process of the Worker's pi process, never pi itself (`SIGTERM`, then `SIGKILL` after 10 s to those still alive). pi gets the tool's failure, writes it to the session and the Worker continues. The Attempt stays `running`, and its active-time clock is unchanged. One line per kill goes to the driver log (`run/<plan>/logs/attempt-<attempt>.log`): the pids, their command lines and the idle time. No Event, no cause, no row is written; a second stall in the same Attempt is handled the same way |
| A5 | Decide the end of `running`: submitted (the Attempt is `merging`) → A8; `stop_request` → end; pane dead or bad stop reason → `crashed`; Worker `idle` without a submit → append `attempt-nudge` once (its `data.turns` is the Worker's assistant-turn count when nudged, §5.2), then `no_submit` if idle again without a submit. A driver that resumes after a reload reads the nudge baseline (that `turns`) from the Attempt's `attempt-nudge` Event, not from its own memory. A Worker's idleness is read from its Session state and confirmed from its session file (the last assistant `stopReason`), never from the state alone (t1 F7, kept) |
| A6 | Removed by R19 (no Job-level Reviewer). |
| A7 | Removed by R19 (no Job-level Reviewer). |
| A8 | Merge (§7.4); a review Job's Attempt has no commits to merge (below) |
| A9 | End: record `metrics` (parsed from the Worker's session file, cpd 1 §14) in the transaction that ends the Attempt; kill the Attempt's tmux window (`kill-window` is tried 3 times, 1 s apart, before the `error` Event); remove the driver lock file `locks/attempt-<plan>--<attempt>.lock` (a missing file is fine; the merge recovery R does the same after it commits an Attempt's end, G13); on `merged`, remove the worktree and branch; otherwise keep them for debugging until their PR is merged, when the PR's clean-up removes them (§7.2) |

**The driver lock** `locks/attempt-<plan>--<attempt>.lock` is held without a gap from before the Attempt row exists until its driver exits, so a free lock always means a dead driver, and E2 never ends a healthy Attempt whose driver has not started running yet. Chosen fix: the starter takes the lock and hands it to the driver at the spawn. Rejected alternative: an E2 grace period for young `starting` Attempts, which rests on a timing guess and needs a new key.

1. The starter (E3 or `job dispatch`) opens the lock file and takes `flock(LOCK_EX | LOCK_NB)` **before** the transaction that inserts the `starting` Attempt. If the lock is held, another starter is creating the same Attempt: `cas_lost`.
2. It commits the insert, then spawns the driver with that descriptor inherited as fd 3 (not close-on-exec).
3. It closes its own copy, without `LOCK_UN`. A `flock` lock belongs to the open file description, which the driver now shares, so the lock stays held by the driver with no gap; an explicit unlock on any copy would release it for both (docs: flock(2)).
4. At A1 the driver checks that fd 3 is that file (same device and inode as the path), else it exits at once without writing, and E2 ends the Attempt `crashed`. A1's refusal ("already has a driver") stays for an Attempt whose status is not `starting`, `running` or `merging`.

**Reload on install** (hs-d33, hs-d38). Installing cpd replaces the binary (§3.7); a running driver reloads itself:

- **Detection.** At start the driver records the (device, inode, mtime) of its own executable path (`host::exe`, `std::env::current_exe()` read once at process start). At each safe point it stats that path again; a different identity, or a path that exists again after being replaced, means a new install. Works on Linux and macOS; no new command.
- **Safe points:** each A4 tick (status `running`) and the point before A8's rebase (status `merging`). Never inside another step.
- **Check before reload:** before the `exec` the driver runs `<path> --version` (the new binary, with its own `CPD_HOME`) and reloads only when that succeeds and `store` is `null` or `store.opens` is `true` (commands.md §1.2; §4.5 "The window"), so a binary that cannot open the store is never `exec`ed (hs-d151). Otherwise it does not `exec`: it logs once per installed file (identity) `reload skipped: <path> cannot open the store: <store.refusal.message>` (in its log) and keeps running its old code; a later install (another identity) is checked again.
- **Reload:** `exec` of the same path with the same argv. Before the `exec` the driver clears close-on-exec on fd 3 (A1 sets it, so the lock is not leaked to children), so the lock stays held with no gap; the new process takes its state from the store.
- **Resume.** The new driver's A1 checks fd 3 as above, then resumes: at A4 for `running` (the Worker's Session and pane from the store; the nudge-once state and the nudge baseline `turns` from the Attempt's `attempt-nudge` Event), at A8 for `merging`.
- A Worker's pi is never touched by a driver reload; it is replaced only between Attempts (a new Attempt starts the installed pi adapter).

If the insert fails (`no_slot`, `cas_lost`), the starter closes the descriptor and no Attempt exists. If the spawn fails, the starter ends the Attempt `failed` / `crashed` itself, as E2 would, with `driver_pid` still null (the marker of a driver that never started; §3.4 "Ladder causes"), and then closes the descriptor. E2 probes a lock by opening the file afresh and trying `flock(LOCK_EX | LOCK_NB)`. The lock file is removed when the Attempt ends: by the driver at A9, and by E2 when it ends a dead driver's Attempt. E2's probe never creates the file (it opens without `O_CREAT`): a missing file means no driver.

The concurrency slot is implicit: an Attempt holds one from `starting` until `ended`. E3 and `job dispatch` insert an Attempt only inside a write transaction that counts the Attempts not `ended` on the machine and refuses at `concurrency` (`no_slot`, exit 3); an urgent Job is refused only at `concurrency + 1` (§3.7 E3; `job dispatch` applies the same rule to an urgent Job). This replaces cpd 1's slot files (§9).

### 3.5 Escalation states

| From | To | Moved by | Event |
|---|---|---|---|
| (none) | `open` | `cpd ask <attempt> <question>` (the Worker; blocks, polling the row) | `escalation` |
| `open` | `answered` | `cpd answer <escalation> <answer>` (the Lead) | `escalation-answered` |
| `open` | `expired` | `cpd ask` itself at `ask_minutes` (exit 5 `ask_timeout`), or engine E1 when `asker_pid` is no longer alive | `escalation-expired` |

`answered` and `expired` are terminal; ids are never reused (cpd 1 §11 "Escalation", kept). `answer` on a non-`open` Escalation is refused: `escalation_answered` or `escalation_expired` (exit 3).

### 3.6 Session states

`working`, `blocked`, `idle`, `stopped`. A Session is created `stopped` (registered, pi not yet running).

| From | To | Moved by | Event |
|---|---|---|---|
| (none) | `stopped` | `plan new`, `context manager`, `session fork`, A3: the Session is registered before pi starts | `session-registered` |
| `stopped` | `idle` | `session state idle`, called by the pi adapter at pi's `session_start` | `session-state` |
| `idle`, `blocked` | `working` | `session state working`, at pi's `agent_start` (and from `blocked` when the last blocking reason closes while a turn runs) | `session-state` |
| `working`, `blocked` | `idle` | `session state idle`, at pi's `agent_settled` with pi idle (and from `blocked` when the last blocking reason closes while idle); **with `--reply` when the run that ended was a reply to the User** (hs-d278; commands.md §15.4), which the Event carries as `data.reply: true` | `session-state` |
| `working`, `idle` | `blocked` | `session state blocked --label <text>` by the adapter while any blocking reason is open (a dialog to the User, a pending Plan review, an open Escalation to this Lead); `cpd ask` for the asking Worker's own Session, which `cpd ask` sets back to `working` when it returns | `session-state` |
| any | `stopped` | `session state stopped` at pi's `session_shutdown`; engine E2 when the Session's pane is dead or gone; or `session close` | `session-state` |

The adapter counts blocking reasons and reports `blocked` while the count is above zero, the model of `tmux-agent-state.ts` (spike, verified), now written to the database instead of tmux options. `session state` writes nothing and appends nothing when the state and label are unchanged.

**A pane that is gone.** Engine E2 records a Session's pane as gone (Escalation `e2e-a1-q1`), and so does `session close` (commands.md §10.9), which kills the pane itself and writes the same rows. E2: when the pane in `tmux_pane` is dead or missing, one transaction sets `state = stopped` (with `session-state` if it was not `stopped`), `tmux_pane = NULL`, and appends `session-moved` `{pane: null, window: null, place: "none"}`. **Dead Lead panes are cleaned up** (G2): when E2 records a Lead's pane as gone it also kills that pane's window, unless the window is `<cockpit>:main` (a park or ctx window is only that pane's). A dead Lead pane in the chat slot (in `main`) is replaced by the placeholder instead: `respawn-pane -k` running `cpd tui view chat`, with `@cpd_view chat` set and `@cpd_session` unset. `stopped` alone does not mean the pane is gone: a Session gets its pane while still `stopped` (registered before pi starts), and `session run` keeps its pane after pi exits, so `session state stopped` leaves `tmux_pane` alone.

### 3.7 The engine, `cpd watch`

One per machine (D5). Every `tick_seconds`, E1–E9 in order:

| Step | What it does |
|---|---|
| E1 | **Escalations:** expire `open` Escalations whose `asker_pid` is dead |
| E2 | **Liveness:** for every Attempt not `ended` whose driver lock is free (the driver died; §3.4): in `merging`, run the merge recovery R (§7.4), which ends it `merged` when its commits are already on the Lead Branch, and otherwise `cancelled` if a cancel was requested, else `crashed`. R runs per orphaned Attempt with no repo lock (hs-d243): two runs of R, or R and a live driver, are kept apart by that Attempt's driver lock, which R takes, and by the compare-and-swap on `status = 'merging'` that ends it (§7.4); in any other status, end it `cancelled` if a cancel was requested, else `crashed`. For every Attempt with a live driver: active time over `attempt_minutes` → set `stop_request = timed_out`; Job `cancel_requested_at` set → set `stop_request = cancelled`. For every Session not `stopped` or with a pane: pane dead or missing (`tmux list-panes -a`) → `stopped` with no pane, and a Lead's dead pane's window is killed, or the chat slot's pane is replaced by the placeholder (§3.6) |
| E3 | **Scheduling:** for each Ready Job, **urgent Jobs first** (`jobs.urgent = 1`), then by `jobs.seq`, across all Plans, while a slot is free: a non-urgent Job needs fewer than `concurrency` Attempts not `ended`; an urgent Job may start **one** beyond that, needing fewer than `concurrency + 1`, so the Attempts not `ended` are at most `concurrency + 1` and the extra slot is never used by a non-urgent Job: verify the fingerprint, take the driver lock, create the Attempt, start its driver with the lock inherited (§3.4). **Back-off:** E3 does not start a Job whose latest `k >= 1` Attempts in a row ended with `driver_pid` null (the driver never started) until `min(2^k, 300)` seconds after the last one's `ended_at`; an Attempt whose driver started resets `k`. So a permanent spawn failure does not start an Attempt at every tick. The `error` Event rule is unchanged (once until the step clears, at most once per `watch.error_repeat_seconds`) |
| E4 | **Completion**, per PR (R34): for each PR in status `dispatch` with no Job `draft`, `queued` or `running`, at least one `merged`, and `all_merged_at` null: append `all-merged` (`data.pr` = its key) and set the PR's `all_merged_at`. A PR with a `draft` Job (an unapproved batch, §3.2) therefore never opens by itself: it waits until the batch is approved and merged, or cancelled (User, hs-d173, hs-d174, hs-d179). The Lead answers with `pr open --pr <key>`. **Again** (hs-d140): `job add` clears `all_merged_at` of the Job's PR in its own transaction, when it was set (the Job is `queued` or a `draft`, so the PR has a Job that is not `merged` or `cancelled` and E4 waits); when that PR's Jobs are all `merged` or `cancelled` once more, E4 appends `all-merged` again and sets `all_merged_at` again |
| E5 | **PR poll** (hs-d96, hs-d97): run `cpd pr poll` (no `--plan`) as a subprocess (a process boundary, cpd 1 §11 step 5), `pr_poll_seconds` after the **end** of the previous poll (never overlapping, never more often than one per tick), whether or not any Plan is in `deploy`. One poll covers every open pull request of the User (§7.6). When the output's `rate_limit.remaining` is below 100, E5 skips polls until `rate_limit.reset_at` and reports one `error` Event with code `gh_rate_limited` (`details`: `remaining`, `reset_at`); the error clears when polling resumes |
| E6 | *Removed by Plan manager B1 (hs-a1): Contexts are machine-local, so there is nothing to sync. The number stays.* |
| E7 | **PR start** (R34, A9): for each Plan in `dispatch`, each PR in status `waiting` whose needs are all `merged` and whose `start_failed_at` is null: start it (§3.8, §7.2). E7 runs after E5 in the same tick, so a PR starts in the tick in which E5's poll sees its last prerequisite merged; it also starts a PR appended in `dispatch` with no needs. A failed start sets `start_failed_at` and appends one `error` Event with code `pr_start_failed` for the Plan's Lead; E7 does not try that PR again, the Lead retries with `cpd pr start <key>` |
| E8 | **Inbox** (hs-d168): after E7, compute the conditions of the Inbox kinds (§2.2, the four pull request kinds included, from the `pull_requests` rows) from the store, the config (`inbox.stuck_job_minutes`), the clock and the engine's own active errors; compare them with the open `inbox_items`; and in **one transaction** insert the new items, set `closed_at` on the gone ones, and append `inbox-opened` and `inbox-closed` (§5.2). Nothing is written when nothing changed (idempotent). A failure of E8 is reported like any step's, and becomes an `engine-error` item once E8 works again. `engine-down` is not computed here: it is not stored |
| E9 | **Context digests** (hs-d194, D4): for each Context that is not archived, has at least one record in force (§4.6), and whose highest record `seq` is above its latest digest's `upto_seq` (or which has no digest), when its newest record's `at` is at least **10 minutes** old and `locks/distill-<context>.lock` is free (no run of that Context is alive): start `cpd context distill --context <c>` detached (`setsid`, stdin `/dev/null`, stdout and stderr appended to `run/context-digests/<c>/distill.log`), as E3 starts a driver, with no lock handed over (the run takes its own; one that loses the race exits `distill_running`, harmlessly). **At most one start per Context and highest `seq`** in the engine's life: the engine remembers, in memory, the highest `seq` it started a run for, and starts no other run of that Context until a newer record arrives or the engine restarts. So a run that fails (`distill_invalid`, `distill_failed`: the run appended its own `error` Event) or crashes (its lock is free and no digest reached that `seq`; its log says why) is not repeated at every tick. E9 itself appends nothing; its failure to spawn is an `error` with code `spawn_failed` under the step rule below |

**Deadline of the engine's own subprocesses.** E5's `cpd pr poll` child has a deadline of 120 s (above `pr poll`'s own 15 s per `gh` call, §7.6); past it the child's process group is killed (§1.2 `tool_timeout`) and the poll counts as failed like any step's failure (below). The `git` and `tmux` calls the steps make have the deadlines of §7 and §6.3.

`watch` holds `locks/watch.lock` (a second one exits with `watch_running`, exit 3), appends `watch-started` and `watch-stopped`, and never exits on its own: unlike cpd 1, it serves every Plan.

A step's failure appends an `error` Event once until it clears, and the same `error` (step, scope and code) at most once per `watch.error_repeat_seconds` even if it clears in between (R31, finding 4). `scripts/install-cpd.sh` restarts a running engine after installing (kills the `watch` window, then `cpd watch --ensure`), because an engine keeps running the binary it started with (finding 5).

**Which executable starts a driver** (hs-d151). The engine (E3) and `job dispatch` start each driver from the executable path the process recorded at its start (`host::exe`), never from `std::env::current_exe()` at spawn time: on Linux that reads `<path> (deleted)` once an install has replaced the file, and every new Attempt would fail with `cannot run`. **`scripts/install-cpd.sh` refuses**, before replacing anything (exit 1, nothing replaced), in two cases (hs-d151, hs-d178 D4), so that a build whose migrations are not all on `origin/main` is never installed:

- **The store check (the install guard, hs-d151).** `<build> --version` (with the caller's `CPD_HOME`) reports `store.opens` false. The script prints `cpd install refused: this build cannot open <CPD_HOME>/cpd.db: <store.refusal.message>` and a fix by `store.refusal.code` (§4.5): `store_too_new`, which includes a build that lacks a breaking migration the store has: `merge main into this branch, then install again`; `migration_changed`: `the branch's migration <name> differs from the one this store applied; restore it from origin/main or wait for the store to be repaired`; `migration_order`: `rename the branch's breaking migration to a newer name with scripts/new-migration.sh, then install again`; any other code: the message alone. A `--version` that fails, or that has no `store` field, is refused too.
- **The branch check (the preview guard, D4).** The script runs `git fetch --quiet origin main` (a failed fetch refuses: it cannot know) and compares the files of `core/src/store/migrations/` in the working tree it builds from with the same paths on `origin/main`. A file that is added, modified or untracked refuses with `cpd install refused: this build carries migration <names> that is not on origin/main, and a preview would apply it to <CPD_HOME>/cpd.db before the PR merges. Merge main into this branch, or wait until the PR merges, then install again.` A file of `origin/main` that the tree lacks is not this case: the store check (above) decides it. The check is git only, runs before the build replaces anything, and needs nothing from the new binary. This keeps hs-d151's rule "a preview of a Lead Branch must contain main" and enforces it: the store check refuses a build that lacks a breaking migration of the store, the branch check refuses a build that adds a migration `main` does not have. A build that lacks only additive migrations that the store applied installs (the window, §4.5).

**What an install reloads** (hs-d33, hs-d38): the engine and the views, by the script (as above and `cpd tui --respawn`, commands §13); each Lead's `session run` wrapper, by itself at once like a driver, re-execing the new cpd and adopting its running pi, so a cpd install never restarts a Lead's pi and never ends a Lead (hs-n141, hs-n148; §6.7 "Reload on install"); each Attempt driver, by itself between its steps (§3.4 "Reload on install"); a Lead's pi, when pi itself changes, by the same rule (a new pi from dotfiles' pin or a manual `pi update` restarts each Lead's pi, §6.7 "Reload on install"); a Worker's pi only between Attempts, so Workers get a new pi at their next Attempt.
### 3.8 PR statuses (R34)

```mermaid
stateDiagram-v2
  [*] --> waiting: pr add
  waiting --> dispatch: plan lock (no needs), engine E7 or pr start (needs merged)
  waiting --> cancelled: pr cancel (grounding, plan)
  dispatch --> open: pr open --pr
  open --> merged: the poll sees it merged
  merged --> [*]
  cancelled --> [*]
```

| From | To | Moved by | Preconditions | Event |
|---|---|---|---|---|
| (none) | `waiting` | `pr add <key> --repo <path> [--base <b>] [--needs <k,…>]`, phase `grounding`, `plan`, `dispatch`, `deploy` or `done`; the Plan is not archived (`archived`) | key valid (`invalid_pr_id`) and free in the Plan (`pr_exists`, unless every flag matches: `already`); `--repo` inside a git repository (`not_a_git_repo`); `<base>` exists as `origin/<base>`, or as a local branch when the repo has no `origin` (`base_not_found`); every need is a PR of the Plan (`needs_unknown`) that is not `cancelled` (`needs_unavailable`), and the needs graph stays acyclic (`dependency_cycle`); `refs/heads/cpd/<plan>/<key>` does not exist in the repo, and neither does `refs/heads/cpd/<plan>`, except the legacy branch of a `merged` PR, which is moved first (below); the legacy branch of a PR that is not `merged` is `branch_exists` (§2.1); the move's `git push origin --delete` fails: `push_failed` | in phase `deploy` or `done`, `plan-phase` (`deploy` or `done` → `dispatch`, §3.1), then `pr-added` |
| `waiting` | `waiting` | `pr edit <key>`, phase `grounding` or `plan` (after: `pr_locked`) | as `pr add` for the fields given | `pr-edited` |
| `waiting` | `cancelled` | `pr cancel <key>`, phase `grounding` or `plan` (after: `pr_locked`) | every Job of the PR is `cancelled`, and no PR that is not `cancelled` needs it (`pr_in_use`) | `pr-cancelled` |
| `waiting` | `dispatch` | **the PR's start**: `plan lock` for every PR with no needs; engine E7 for a PR whose needs are all `merged`; or `pr start <key>` by the Lead | Plan phase `dispatch` (or the lock's own transaction; `pr start` in another phase: `illegal_phase`); the PR `waiting` (`pr start` on a started PR: `already`; on a `cancelled` one: `illegal_transition`); every need `merged` (`pr_not_ready`); its Lead worktree created and `pre-start` passed, R35's rules applied to its Lead Branch (§7.2) | `pr-started` |
| `waiting` | `waiting`, `start_failed_at` set | engine E7, when the start fails (worktree, remote check or `pre-start`, §7.2) | | `error` with code `pr_start_failed` |
| `dispatch` | `open` | `pr open --pr <key>` (§7.6) | every Job of the PR `merged` or `cancelled`, at least one `merged` (`jobs_not_done`); the Gate passes | `pr-opened`; and `plan-phase` (`dispatch → deploy`) when every PR that is not `cancelled` is now `open` or `merged` (§3.1) |
| `dispatch` | `merged` | `pr open --pr <key>` (§7.6 step 0), when the newest pull request of the Lead Branch is already `merged` on GitHub: in one transaction `dispatch → open → merged`, no Gate | every Job of the PR `merged` or `cancelled`, at least one `merged` (`jobs_not_done`); the pull request `merged` (`closed`: `pr_closed`) | `pr-merged`, no `pr-opened`; `plan-phase` (`dispatch → deploy`) when every PR that is not `cancelled` is now `open` or `merged`, then `plan-done` (`deploy → done`) when every one is `merged` |
| `open` | `merged` | `pr poll` (E5, §7.6), in phase `dispatch` or `deploy` | the poll sees the PR's pull request `merged` (hs-d96, D7: the open search no longer lists it and the lookup answers `MERGED`) | `pr-merged`; and `plan-done` when every PR that is not `cancelled` is now `merged` (§3.1) |

`merged` and `cancelled` are terminal. A PR can be cancelled only before the lock (A13); a PR still `waiting` after the lock keeps the Plan in `dispatch` until it starts and opens.

**The legacy Lead Branch** (D3, hs-d118). Every Plan locked before R34 stored `prs.lead_branch = 'cpd/<plan>'`, and git cannot hold both `cpd/<plan>` and `cpd/<plan>/<key>` (a ref file and a directory clash, locally and on `origin`). When the Plan has a PR whose `lead_branch` is `cpd/<plan>` and whose status is `merged`, `pr add` (in any allowed phase) first moves it to `cpd/<plan>/<that PR's key>`, in this order:

1. locally, `git branch -m cpd/<plan> cpd/<plan>/<key>`, when `refs/heads/cpd/<plan>` exists (a worktree that has the branch checked out follows the rename; unverified on git 2.43, the code Job tests it);
2. on the remote, `git push origin --delete cpd/<plan>`, when the repo has `origin` and `origin` has the branch (`git ls-remote --heads origin cpd/<plan>`). If it fails, `pr add` is refused `push_failed` (exit 1; `details`: `branch`, `repo`, `stderr`) and the database is unchanged (A3);
3. in the same transaction as the new PR's insert, `UPDATE prs SET lead_branch = 'cpd/<plan>/<key>'` for that PR (the one change `prs_locked` allows, §4.4).

Each step is idempotent: a rerun after a crash between the local rename and the transaction finds `cpd/<plan>/<key>` and no `cpd/<plan>`, skips what is done and completes (A3). A legacy branch of a PR that is not `merged` still refuses `branch_exists`.

**The start** (A8, A9) creates the PR's Lead worktree from a fresh `fetch origin <base>`, so a PR that needs another PR in the same repo and on the same base starts from a base that already contains it (A1 → B → A2: A2 is cut after A1 merged). It applies R35 to the PR's Lead Branch (§7.2), runs `pre-start` from the PR's repo, then writes, in one transaction with a compare-and-swap on `status = 'waiting'`: `status = dispatch`, `lead_worktree`, `started_at`, `start_failed_at = null`, and `pr-started`. The PR's `queued` Jobs then become Ready (§3.2).

**A failed start** (A9). At `plan lock`, the lock fails as a whole (§3.1). From engine E7, the worktree and branch it created are removed, the PR stays `waiting`, and one transaction sets `start_failed_at` and appends one `error` Event (`code: pr_start_failed`, `details: {pr, step, tail?, log?}`) addressed to the Plan's Lead (§5.3). The Lead fixes the cause (or asks the User) and runs `cpd pr start <key>`, which starts the PR as E7 would; if it fails again, `pr start` returns `setup_failed` (exit 1) with the same details and changes nothing.

**A PR closed without a merge** (A11, hs-d96). The poll sees the pull request `closed` (it leaves the open search and the lookup answers `CLOSED`; the row then holds `closed` and the pull request is looked up once, not on every later poll, until a search lists it again): one `pr-feedback` with `reasons: ["closed"]` (§5.2); the PR stays `open` in cpd, every PR that needs it stays `waiting`, and the Plan cannot reach `done`. The Lead asks the User what to do. Reopening the pull request on GitHub puts it back in the open search, and the poll continues from the row; it sends no feedback for the reopening.

### 3.9 The CPD flow

**CPD** is Continuous Parallel Delivery (hs-d245). Its flow has three stages, Context, Plan and Deploy: the cockpit's three columns, read as past, present and future. The flow below names each stage's steps and maps them to the phases of §3.1 and to commands; it adds no state of its own.

| Stage | Step | cpd phase | What happens | Commands |
|---|---|---|---|---|
| Context | Grounding | `grounding` | read the Context's records and the User's material | `context show`, `context note` |
| Context | Clarifying | `grounding` | the interview: unknowns are minimised | `context ask`, `context decide` |
| Context | Specifying | `grounding` | Requirements, assumptions and records are written; ends with the move to `plan` | `context decide`, `context note`, `plan phase plan` |
| Plan | Dividing | `plan` | the work is split into PRs and Jobs | `pr add`, `job add` |
| Plan | Sequencing | `plan` | order: Dependencies and PR needs; parallel versus serial | `job add --needs`, `pr add --needs` |
| Plan | Approving | `plan` | the Plan review: the User reads and decides | `plan render`, `cpd_plan_review` (adapter), `plan lock` |
| Plan | Dispatching | `dispatch` | the engine runs the Jobs; Workers merge them into the Lead Branches | `watch` (engine), `job list` |
| Deploy | Mechanical checks | end of `dispatch` | all Jobs of a PR are merged; the Gate runs | `pr open` (the Gate, §7.6) |
| Deploy | PR | `dispatch` → `deploy` | the pull request is opened | `pr open` |
| Deploy | Reviewing and Monitoring | `deploy` | people, the User's review bot and CI review the open pull request; the engine polls it | `pr poll` (engine E5), `pr view`, Event `pr-feedback` |
| Deploy | Addressing | `deploy` → `dispatch` → `deploy` | feedback Jobs are appended and run (back to Dispatching), then pushed | `job add --pr`, `pr rebase` |

The Deploy stage runs from the end of `dispatch` (all Jobs merged, `pr open`) and through phase `deploy` until `done`.

**Who does the Context steps.** At the Epic level the Manager does Grounding, Clarifying and Specifying for the Context (§3.1, "The Context's Manager"); each Lead does them for its Plan, in phase `grounding`.

**No local review before the Gate** (hs-d171, supersedes hs-d77). A PR is reviewed after it opens, while CI runs, by people and the User's review bot (Reviewing and Monitoring). Nothing requires a review before the Gate. A final review Job (`job add --review`, §2.2) stays optional (hs-d70, B3).

## 4. Storage

### 4.1 The database

- One file per machine: `$CPD_HOME/cpd.db` (R1), created on first use with `PRAGMA journal_mode = WAL` (persistent) and `PRAGMA application_id = 1668310066` (`0x63706432`, "cpd2").
- Every connection sets `PRAGMA foreign_keys = ON`, `PRAGMA synchronous = FULL`, `PRAGMA busy_timeout = <store_busy_ms>`, and registers the SQL function `cpd_txn()` (§4.3) before it runs any statement. The function must not be flagged `SQLITE_DIRECTONLY`, because triggers call it.
- SQLite is bundled into the binary (rusqlite's `bundled` feature), at least 3.45, so `STRICT` tables and the JSON functions are always present.
- Nothing is ever deleted (§4.4). The database grows; there is no pruning in this contract.

### 4.2 Transactions and concurrency

- **Who writes:** any `cpd` process: commands, `watch`, Attempt drivers, `ask`. Nothing else can (§4.3).
- **Readers never block.** Reads run in deferred read transactions on the WAL snapshot and never wait for a writer. A reader keeps its transaction short (one query or one command's reads), so checkpoints are not starved; `events follow` opens a new read transaction for every poll (§5.4).
- **Writers** begin with `BEGIN IMMEDIATE`, so at most one write transaction is open on the machine at any moment. A writer that cannot get the write lock within `store_busy_ms` fails with `store_busy` (exit 5); `SQLITE_LOCKED` is reported as `store_busy` too.
- **The setup lock.** `Store::open` takes `locks/store.lock` (§1.4) for the one-time setup. It waits for it at most `store_busy_ms`, then fails `lock_timeout` (exit 5) with a message naming what it waited for (the lock file, `locks/store.lock`, and the time waited). It never waits without a limit: a holder that hangs does not hang every other `cpd` command.
- **One command, one write transaction** for its state change, its `history` rows and its Events: all three commit together or not at all.
- **No external work inside a write transaction.** git, the Gate, tmux, pi and `gh` run outside. A step reads, acts, then writes with a compare-and-swap on the state it read (`UPDATE … WHERE status = :expected`); zero rows changed is `cas_lost` (exit 3), and the step re-reads and decides again.
- **Ordering.** Because writers are serialised, Event ids and history ids increase in commit order: a reader that has seen id N never later sees a new id below N.

### 4.3 Writes only through cpd: `txns` and `cpd_txn()`

Every write transaction starts by inserting its row into `txns` with an explicit id, `(SELECT coalesce(max(id), 0) + 1 FROM txns)`, after setting the connection's `cpd_txn()` to return that id. Triggers on every table call `cpd_txn()`:
- a connection that is not a `cpd` process (for example the `sqlite3` shell) has no such function, so any write it attempts fails with "no such function: cpd_txn" (SQLite resolves trigger functions when the outer statement is prepared; verified by `cli/tests/store.rs`; a delete from an append-only table fails with its `append_only` error instead);
- a write whose `cpd_txn()` does not match its transaction's `txns` row is refused by the triggers on `txns`, `events` and `history`.

`txns.cwd` is the process's working directory; when it cannot be read (the directory was deleted under the process) it is the text `(unreadable)` and the write goes on (hs-d196).

The **schema check** (hs-d178 D2): at open, after the migrations of §4.5 have run, cpd compares the store's schema with the schema this binary's own migrations create. The trusted value is **pinned in the code and never recorded in the store**: `SCHEMA_HASH`, a constant of the store module, is the canonical schema hash (below) of a fresh store that applied exactly this binary's migrations. A migration's PR updates the constant in the same change. A mismatch means someone dropped or changed an object: write commands refuse with `store_tampered` (exit 1); read commands still work and report it in their `tamper` field. This plus the history replaces cpd 1's `bd history` actor check (§8).

**Why a canonical form.** Additive migrations may reach different stores in different orders (§4.5 "Order"), and `ALTER TABLE … ADD COLUMN` appends the new column's text to the table's `sql` in `sqlite_schema`, so two orders leave two texts for the same schema (verified with Python's `sqlite3` 3.45.1; the bundled SQLite 3.53 is pinned by `add_column_in_either_order_gives_one_canonical_table` in `core/src/store/schema.rs`). The hash is therefore computed over a form that depends on which migrations were applied and not on their order. It is defined exactly here, and `core/src/store/` implements and tests exactly this:

1. **Objects.** The rows of `sqlite_schema` whose `name` does not start with `sqlite_`, of any `type`, except the generated history triggers (item 5).
2. **Normalising a text** `N(text)`. Read the text left to right. A *quoted run* is `'…'`, `"…"`, `` `…` `` (the quote character doubled inside it is an escape) or `[…]`; it is copied unchanged. Outside quoted runs, every comment (`--` to the end of the line, and `/* … */`) is deleted, every maximal run of whitespace and deleted comments becomes one space, and the ends are trimmed.
3. **A table** is the entry `["table", name, columns, constraints, options]`. In the table's `sql`, the *body* is the text between the first `(` outside a quoted run and its matching `)` (parentheses outside quoted runs are counted), and the *options* are the text after that `)` (for example `STRICT`). The body is split at every comma that is outside quoted runs and not inside parentheses; each piece is an *item*. An item is a *constraint* when its first word (the characters up to the first whitespace or `(`), unquoted, equals `CONSTRAINT`, `PRIMARY`, `UNIQUE`, `CHECK` or `FOREIGN` ignoring ASCII case; every other item is a *column*, whose *key* is its first token with its quotes removed (a doubled quote character becomes one), ASCII-lowercased. Then `columns` is the `N` of every column item, **sorted by key** (bytewise); `constraints` is the `N` of every constraint item **in source order** (only `CREATE TABLE` writes them: `ADD COLUMN` inserts its column before the first table constraint, or before the closing `)`, so the constraints never move); `options` is `N` of the options; `name` is the `name` of the `sqlite_schema` row.
4. **Every other object** (index, view, trigger) is the entry `[type, name, sql]` with `sql` exactly as stored.
5. **Generated triggers** are not hashed as text, because they name the columns in column order. They are checked on their own, in every case, as a part of the schema check: for each row `(t, no_delete)` of `history_tables` (§4.4) the store has `t_hist_ins`, `t_hist_upd` and, when `no_delete` is 1, `t_no_delete`, and each equals, byte for byte, the SQL the history-trigger generator (§4.4 "Triggers", the one the runner's reconcile uses, §4.5) produces from the table's **current** columns and primary key. Those are the generated triggers: a trigger with any other name is an object of item 4, and a table with `no_delete` 0 may have its own hand-written `t_no_delete` (`dependencies`, `pr_needs`). A missing or different generated trigger is a mismatch.
6. **The hash** is the lowercase hex sha256 of the compact JSON text (no whitespace; `serde_json`'s default) of the array of all entries, sorted by `(type, name)` bytewise (the order of `SELECT … ORDER BY type, name`).

**What the check does, by the store's state after the migrations** (the open has applied every migration this binary knows, so the store has them all):
- **No applied migration is unknown to this binary** (the verdict's `unknown` is empty, §4.5): the hash of the store's objects (item 6) must equal `SCHEMA_HASH`, and item 5 must hold.
- **The store also has additive migrations this binary does not know** (inside the window, §4.5): a pinned hash of the whole schema cannot match, since the store has more objects and more columns. The check is then of **the canonical subset of the objects this binary's migrations created**. The reference *R* is the list of entries (items 1 to 4) of a fresh in-memory database that applied this binary's migrations in name order, built once per process and only when needed (a test asserts that the hash of *R* is `SCHEMA_HASH`). The store passes when **all** hold: (a) for every table entry of *R*, the store has a table of that name, with the same `constraints` and `options`, and whose `columns`, **restricted to the keys that *R*'s columns have**, are equal to *R*'s (a column of the store whose key *R* lacks belongs to an unknown migration and is ignored); (b) every other entry of *R* is in the store, equal; (c) every trigger of the store that is on a table of *R* (its `tbl_name`) and is neither generated (item 5) nor an entry of *R* is a mismatch, because an additive migration only creates triggers on tables it creates (§4.5 A1); (d) item 5 holds for every row of `history_tables`, tables of unknown migrations included. Tables, indexes and triggers of unknown migrations are **not checked**: this binary cannot know them. The binary that knows them checks them when it opens the store, and an install of `main` (§3.7) does.

The check never reads a hash from the store: `schema_migrations` records names, compat and checksums only (§4.5).

The **tamper report** (`plan show`'s `tamper` list, `doctor`): the schema check; every Locked Job whose recomputed fingerprint differs from the stored one; any `history` row whose `txn` has no `txns` row. The threat is an agent writing the Ledger outside cpd, as in cpd 1; a local user who drops the triggers, edits and recreates them exactly is out of scope (in cpd 1 the beads database could be edited the same way).

### 4.4 DDL

The schema after migration `kind-tables`, the first named migration (§4.5), and so after every migration that exists today. Migration `0001-initial` created these tables as they were at its version (`core/src/store/migrations/0001-initial.sql`); `0002-cockpit-parity` to `0004-cross-repo` (§4.5) brought `plans`, `jobs`, `events` and `context_records` to the shape below and created `prs` and `pr_needs` (R34); `0005-plan-reopen` changed the triggers `plans_phase` and `prs_locked` (hs-d118); `0006-pr-poller` created `pull_requests` and rebuilt `prs` (hs-d96); `0007-context-records` dropped `contexts.sync_failing` and made `context_records` the only store of Context records (§4.6); `0008-phase-deploy` renamed the Plan phase `pr` to `deploy`; `0009-session-message` added the Event kind `session-message`; `0010-manager` added the role `manager` and the record kind `brief` and dropped `contexts.lead_session` and `grounded_seq`; `0011-pr-mergeable` added `pull_requests.mergeable` and `merge_state` (hs-n152); `0012-inbox` created `inbox_items` (the Inbox, hs-d168) and added the four `inbox-*` Event kinds; `0013-pr-inbox` added `pull_requests.authored` and `review_requested` and the four pull request kinds to `inbox_items` (hs-d165); `kind-tables` created `schema_migrations`, `event_kinds`, `record_kinds` and `history_tables` and replaced the two kind checks by foreign keys (below, §4.5). Every later change is a named migration (§4.5) and amends this section in the same PR. Check constraints that need regular expressions are enforced in the binary at the boundary (§2.1), and the database checks shapes and lengths.

```sql
-- ---------- audit ----------
CREATE TABLE txns (
  id      INTEGER PRIMARY KEY,               -- explicit: max(id) + 1 under BEGIN IMMEDIATE
  at      TEXT    NOT NULL,
  command TEXT    NOT NULL,                  -- e.g. 'job add', 'watch', 'attempt run', 'migrate'
  pid     INTEGER NOT NULL,
  session TEXT,                              -- CPD_SESSION or PI_SESSION_ID of the caller, if any
  cwd     TEXT    NOT NULL
) STRICT;

CREATE TABLE history (
  id     INTEGER PRIMARY KEY AUTOINCREMENT,
  txn    INTEGER NOT NULL REFERENCES txns(id),
  tbl    TEXT    NOT NULL,
  pk     TEXT    NOT NULL CHECK (json_valid(pk) AND json_type(pk) = 'array'),
  op     TEXT    NOT NULL CHECK (op IN ('insert', 'update', 'delete')),
  before TEXT    CHECK (before IS NULL OR json_valid(before)),
  after  TEXT    CHECK (after  IS NULL OR json_valid(after)),
  CHECK ((op = 'insert') = (before IS NULL)),
  CHECK ((op = 'delete') = (after IS NULL))
) STRICT;
CREATE INDEX history_tbl_pk ON history(tbl, pk);
CREATE INDEX history_txn ON history(txn);

-- ---------- Contexts ----------
CREATE TABLE contexts (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 2 AND 40),
  title         TEXT NOT NULL CHECK (length(title) >= 1),
  jira          TEXT,
  created_at    TEXT NOT NULL,
  created_on    TEXT NOT NULL,
  archived_at   TEXT                          -- see migration 0010 (§4.5) for the columns it dropped
) STRICT;

CREATE TABLE context_records (               -- the only store of Context records (§4.6)
  seq          INTEGER PRIMARY KEY AUTOINCREMENT,   -- position in this machine's database
  context      TEXT NOT NULL REFERENCES contexts(id),
  id           TEXT NOT NULL,
  kind         TEXT NOT NULL REFERENCES record_kinds(name),
  at           TEXT NOT NULL,
  machine      TEXT NOT NULL,
  by_session   TEXT,
  by_plan      TEXT,
  text         TEXT NOT NULL CHECK (length(text) >= 1),
  supersedes   TEXT,
  question     TEXT,
  plan_id      TEXT,
  plan_machine TEXT,
  plan_jira    TEXT,                          -- R34: no plan_repo (A12)
  UNIQUE (context, id),
  CHECK (supersedes IS NULL OR kind = 'decision'),
  CHECK ((question IS NOT NULL) = (kind = 'answer')),
  CHECK ((plan_id IS NOT NULL) = (kind IN ('plan', 'brief'))),
  CHECK ((plan_id IS NULL) = (plan_machine IS NULL))
) STRICT;

-- ---------- Plans ----------
CREATE TABLE plans (
  id            TEXT PRIMARY KEY CHECK (length(id) BETWEEN 2 AND 24),
  context       TEXT NOT NULL REFERENCES contexts(id),
  title         TEXT NOT NULL CHECK (length(title) >= 1),
  jira          TEXT,
  phase         TEXT NOT NULL CHECK (phase IN ('grounding', 'plan', 'dispatch', 'deploy', 'done')),
  lead_session  TEXT NOT NULL REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
  created_at    TEXT NOT NULL,
  locked_at     TEXT,
  done_at       TEXT,
  archived_at   TEXT,                          -- migration 3 (archive)
  CHECK ((phase IN ('grounding', 'plan')) = (locked_at IS NULL)),
  CHECK ((phase = 'done') = (done_at IS NOT NULL))
) STRICT;

-- ---------- Pull requests (the PR poller, hs-d96; migration 6; mergeable and merge_state: migration 11; authored and review_requested: migration 0013) ----------
CREATE TABLE pull_requests (
  id              INTEGER PRIMARY KEY,
  repo            TEXT NOT NULL CHECK (repo LIKE '_%/_%'),      -- GitHub's owner/name
  number          INTEGER NOT NULL CHECK (number >= 1),
  url             TEXT NOT NULL,
  title           TEXT NOT NULL CHECK (length(title) >= 1),
  draft           INTEGER NOT NULL CHECK (draft IN (0, 1)),
  state           TEXT NOT NULL CHECK (state IN ('open', 'closed', 'merged')),
  review_decision TEXT NOT NULL CHECK (review_decision IN ('none', 'approved', 'changes_requested')),
  checks          TEXT NOT NULL CHECK (checks IN ('none', 'passed', 'pending', 'failed')),
  created_at      TEXT NOT NULL,                               -- GitHub's
  merged_at       TEXT,
  changed_at      TEXT NOT NULL,
  mergeable       TEXT NOT NULL DEFAULT 'unknown' CHECK (mergeable IN ('mergeable', 'conflicting', 'unknown')),
  merge_state     TEXT NOT NULL DEFAULT 'unknown' CHECK (merge_state IN ('behind', 'blocked', 'clean', 'dirty', 'draft', 'has_hooks', 'unknown', 'unstable')),
  authored        INTEGER NOT NULL DEFAULT 1 CHECK (authored IN (0, 1)),          -- migration 0013: 1 when the User wrote it
  review_requested INTEGER NOT NULL DEFAULT 0 CHECK (review_requested IN (0, 1)), -- migration 0013: 1 when the User's review is requested
  -- cpd's time of the last change
  UNIQUE (repo, number),
  CHECK ((state = 'merged') = (merged_at IS NOT NULL))
) STRICT;

-- ---------- PRs (R34) ----------
CREATE TABLE prs (
  plan               TEXT NOT NULL REFERENCES plans(id),
  key                TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 24),
  repo               TEXT NOT NULL CHECK (length(repo) >= 1),
  base               TEXT NOT NULL CHECK (length(base) >= 1),
  status             TEXT NOT NULL CHECK (status IN ('waiting', 'dispatch', 'open', 'merged', 'cancelled')),
  lead_branch        TEXT NOT NULL CHECK (lead_branch IN ('cpd/' || plan || '/' || key, 'cpd/' || plan)),  -- the second form only for PRs made by migration 4
  lead_worktree      TEXT,
  pr_number          INTEGER,
  pr_url             TEXT,
  pr_opened_at       TEXT,
  pull_request       INTEGER REFERENCES pull_requests(id),   -- migration 6 (hs-d96): set by pr open
  all_merged_at      TEXT,
  start_failed_at    TEXT,
  created_txn        INTEGER NOT NULL REFERENCES txns(id),
  created_at         TEXT NOT NULL,
  started_at         TEXT,
  merged_at          TEXT,
  PRIMARY KEY (plan, key),
  UNIQUE (plan, lead_branch),
  CHECK ((status IN ('waiting', 'cancelled')) = (lead_worktree IS NULL)),
  CHECK ((status IN ('waiting', 'cancelled')) = (started_at IS NULL)),
  CHECK ((status IN ('open', 'merged')) = (pr_number IS NOT NULL)),
  CHECK ((pr_number IS NULL) = (pr_url IS NULL) AND (pr_number IS NULL) = (pr_opened_at IS NULL)),
  CHECK ((status IN ('open', 'merged')) = (pull_request IS NOT NULL)),
  CHECK ((status = 'merged') = (merged_at IS NOT NULL)),
  CHECK (start_failed_at IS NULL OR status = 'waiting')
) STRICT;

-- A join table, not JSON text in prs: its foreign keys keep every need on an existing PR of the same Plan, as dependencies does for Jobs.
CREATE TABLE pr_needs (
  plan  TEXT NOT NULL,
  pr    TEXT NOT NULL,
  needs TEXT NOT NULL,
  PRIMARY KEY (plan, pr, needs),
  FOREIGN KEY (plan, pr)    REFERENCES prs(plan, key),
  FOREIGN KEY (plan, needs) REFERENCES prs(plan, key),
  CHECK (pr <> needs)
) STRICT;

CREATE TABLE plan_reviews (
  plan         TEXT NOT NULL REFERENCES plans(id),
  id           TEXT NOT NULL,
  number       INTEGER NOT NULL CHECK (number >= 1),
  fingerprint  TEXT NOT NULL CHECK (length(fingerprint) = 64),
  status       TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'rejected')),
  feedback     TEXT,
  requested_at TEXT NOT NULL,
  decided_at   TEXT,
  PRIMARY KEY (plan, id),
  UNIQUE (plan, number),
  CHECK (id = 'r' || number),
  CHECK ((status = 'pending') = (decided_at IS NULL))
) STRICT;

-- ---------- Jobs ----------
CREATE TABLE jobs (
  seq                 INTEGER PRIMARY KEY AUTOINCREMENT,
  plan                TEXT NOT NULL REFERENCES plans(id),
  pr                  TEXT NOT NULL,          -- R34: the Job's PR (A7)
  review              INTEGER NOT NULL DEFAULT 0 CHECK (review IN (0, 1)),  -- B2: a review Job (added by migration 4)
  key                 TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 32),
  title               TEXT NOT NULL CHECK (length(title) >= 1),
  tier                TEXT CHECK (tier IS NULL OR (length(tier) = 2 AND substr(tier, 1, 1) IN ('O', 'S', 'H') AND substr(tier, 2, 1) BETWEEN '0' AND '5')),  -- null only while draft (migration 0014)
  goal                TEXT CHECK (goal IS NULL OR length(goal) >= 1),
  context             TEXT,
  touches             TEXT CHECK (touches IS NULL OR (json_valid(touches) AND json_type(touches) = 'array' AND json_array_length(touches) >= 1)),
  acceptance          TEXT CHECK (acceptance IS NULL OR (json_valid(acceptance) AND json_type(acceptance) = 'array' AND json_array_length(acceptance) >= 1)),
  out_of_scope        TEXT CHECK (out_of_scope IS NULL OR (json_valid(out_of_scope) AND json_type(out_of_scope) = 'array')),
  status              TEXT NOT NULL CHECK (status IN ('draft', 'queued', 'running', 'merged', 'failed', 'cancelled')),
  appended            INTEGER NOT NULL CHECK (appended IN (0, 1)),
  fingerprint         TEXT CHECK (fingerprint IS NULL OR length(fingerprint) = 64),
  created_txn         INTEGER NOT NULL REFERENCES txns(id),
  retry_tier          TEXT,
  cancel_reason       TEXT,
  cancel_requested_at TEXT,
  failed_cause        TEXT,
  merged_sha          TEXT,
  created_at          TEXT NOT NULL,
  urgent              INTEGER NOT NULL DEFAULT 0 CHECK (urgent IN (0, 1)),  -- the urgent flag (§2.2); not in jobs_locked, not in a fingerprint; added by the migration 20261019T0451-job-urgent (an ADD COLUMN, so last among the columns)
  UNIQUE (plan, key),
  FOREIGN KEY (plan, pr) REFERENCES prs(plan, key),
  CHECK (status <> 'draft' OR fingerprint IS NULL),
  CHECK (status IN ('draft', 'cancelled') OR fingerprint IS NOT NULL),
  CHECK (status IN ('draft', 'cancelled') OR (tier IS NOT NULL AND goal IS NOT NULL AND context IS NOT NULL AND touches IS NOT NULL AND acceptance IS NOT NULL AND out_of_scope IS NOT NULL)),  -- stubs only as drafts, or cancelled as such (hs-d241)
  CHECK ((goal IS NULL) = (context IS NULL) AND (goal IS NULL) = (touches IS NULL) AND (goal IS NULL) = (acceptance IS NULL) AND (goal IS NULL) = (out_of_scope IS NULL)),  -- the Spec is all or nothing
  CHECK ((status = 'merged') = (merged_sha IS NOT NULL)),
  CHECK ((status = 'failed') = (failed_cause IS NOT NULL)),
  CHECK ((cancel_reason IS NULL) = (cancel_requested_at IS NULL)),
  CHECK (status <> 'cancelled' OR cancel_reason IS NOT NULL)
) STRICT;

CREATE TABLE dependencies (
  plan  TEXT NOT NULL,
  job   TEXT NOT NULL,
  needs TEXT NOT NULL,
  PRIMARY KEY (plan, job, needs),
  FOREIGN KEY (plan, job)   REFERENCES jobs(plan, key),
  FOREIGN KEY (plan, needs) REFERENCES jobs(plan, key),
  CHECK (job <> needs)
) STRICT;

-- ---------- Attempts ----------
CREATE TABLE attempts (
  plan             TEXT NOT NULL,
  id               TEXT NOT NULL,
  job              TEXT NOT NULL,
  number           INTEGER NOT NULL CHECK (number >= 1),
  tier             TEXT NOT NULL CHECK (length(tier) = 2),
  origin           TEXT NOT NULL CHECK (origin IN ('ladder', 'retry')),
  status           TEXT NOT NULL CHECK (status IN ('starting', 'running', 'merging', 'ended')),
  outcome          TEXT CHECK (outcome IN ('merged', 'failed', 'cancelled')),
  cause            TEXT CHECK (cause IN ('merged', 'cancel_requested', 'setup_failed', 'timed_out',
                                         'crashed', 'no_submit', 'merge_conflict',
                                         'cas_lost', 'dirty_target', 'tampered')),
  branch           TEXT NOT NULL CHECK (branch = 'cpd/' || plan || '--' || id),
  worktree         TEXT NOT NULL,
  base_sha         TEXT,
  driver_pid       INTEGER,
  worker_session   TEXT REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
  spec_path        TEXT,
  submitted_sha    TEXT,
  submitted_at     TEXT,
  check_tail       TEXT,
  merged_sha       TEXT,
  nudged           INTEGER NOT NULL DEFAULT 0 CHECK (nudged IN (0, 1)),
  stop_request     TEXT CHECK (stop_request IN ('timed_out', 'cancelled')),
  metrics          TEXT CHECK (metrics IS NULL OR json_valid(metrics)),
  started_at       TEXT NOT NULL,
  ended_at         TEXT,
  PRIMARY KEY (plan, id),
  UNIQUE (plan, job, number),
  FOREIGN KEY (plan, job) REFERENCES jobs(plan, key),
  CHECK (id = job || '-a' || number),
  CHECK ((status = 'ended') = (outcome IS NOT NULL)),
  CHECK ((outcome IS NULL) = (cause IS NULL)),
  CHECK ((status = 'ended') = (ended_at IS NOT NULL)),
  CHECK ((outcome = 'merged') = (cause = 'merged')),
  CHECK ((outcome = 'cancelled') = (cause = 'cancel_requested')),
  CHECK ((outcome = 'merged') = (merged_sha IS NOT NULL)),
  CHECK ((submitted_sha IS NULL) = (submitted_at IS NULL)),
  CHECK (status <> 'merging' OR submitted_sha IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX attempts_one_active ON attempts(plan, job) WHERE status <> 'ended';
CREATE INDEX attempts_active ON attempts(status) WHERE status <> 'ended';

-- ---------- Escalations ----------
CREATE TABLE escalations (
  plan        TEXT NOT NULL,
  id          TEXT NOT NULL,
  attempt     TEXT NOT NULL,
  number      INTEGER NOT NULL CHECK (number >= 1),
  question    TEXT NOT NULL CHECK (length(question) >= 1),
  status      TEXT NOT NULL CHECK (status IN ('open', 'answered', 'expired')),
  answer      TEXT,
  asker_pid   INTEGER NOT NULL,
  asked_at    TEXT NOT NULL,
  answered_at TEXT,
  expired_at  TEXT,
  PRIMARY KEY (plan, id),
  UNIQUE (plan, attempt, number),
  FOREIGN KEY (plan, attempt) REFERENCES attempts(plan, id),
  CHECK (id = attempt || '-q' || number),
  CHECK ((status = 'answered') = (answer IS NOT NULL)),
  CHECK ((status = 'answered') = (answered_at IS NOT NULL)),
  CHECK ((status = 'expired') = (expired_at IS NOT NULL))
) STRICT;

-- ---------- Sessions ----------
CREATE TABLE sessions (
  id          TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 128),
  role        TEXT NOT NULL CHECK (role IN ('manager', 'lead', 'worker')),
  context     TEXT NOT NULL REFERENCES contexts(id),
  plan        TEXT REFERENCES plans(id) DEFERRABLE INITIALLY DEFERRED,
  attempt     TEXT,
  name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  file        TEXT,
  forked_from TEXT REFERENCES sessions(id),
  cwd         TEXT NOT NULL,
  tier        TEXT NOT NULL CHECK (length(tier) = 2),
  tmux_pane   TEXT CHECK (tmux_pane IS NULL OR tmux_pane GLOB '%[0-9]*'),
  state       TEXT NOT NULL CHECK (state IN ('working', 'blocked', 'idle', 'stopped')),
  state_label TEXT,
  state_at    TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  archived_at TEXT,
  FOREIGN KEY (plan, attempt) REFERENCES attempts(plan, id) DEFERRABLE INITIALLY DEFERRED,
  CHECK ((role = 'worker') = (attempt IS NOT NULL)),
  CHECK (role = 'manager' OR role = 'lead' OR plan IS NOT NULL),
  CHECK (role <> 'manager' OR plan IS NULL),
  CHECK ((state = 'blocked') = (state_label IS NOT NULL))
) STRICT;
CREATE INDEX sessions_plan ON sessions(plan);
CREATE UNIQUE INDEX sessions_one_manager ON sessions(context) WHERE role = 'manager' AND archived_at IS NULL;   -- migration 0010

-- ---------- Events ----------
CREATE TABLE events (
  id      INTEGER PRIMARY KEY AUTOINCREMENT,
  txn     INTEGER NOT NULL REFERENCES txns(id),
  at      TEXT    NOT NULL,
  kind    TEXT    NOT NULL REFERENCES event_kinds(name),   -- §4.5: a kind exists when its row does
  context TEXT    REFERENCES contexts(id),
  plan    TEXT    REFERENCES plans(id),
  job     TEXT,
  attempt TEXT,
  session TEXT    REFERENCES sessions(id),
  text    TEXT    NOT NULL,
  data    TEXT    NOT NULL CHECK (json_valid(data) AND json_type(data) = 'object'),
  CHECK (plan IS NULL OR context IS NOT NULL)
) STRICT;
CREATE INDEX events_plan ON events(plan, id);
CREATE INDEX events_context ON events(context, id);
CREATE INDEX events_session ON events(session, id);

-- ---------- Inbox items (§2.2) ----------
CREATE TABLE inbox_items (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL CHECK (kind IN ('dialog', 'plan-review', 'unread', 'job-stuck', 'engine-error', 'question', 'pr-failing', 'pr-changes', 'pr-ready', 'review-requested')),  -- the last four: migration 0013
  urgency       TEXT NOT NULL CHECK (urgency IN ('red', 'yellow', 'magenta', 'white')),
  subject       TEXT NOT NULL CHECK (length(subject) >= 1),
  context       TEXT REFERENCES contexts(id),
  plan          TEXT REFERENCES plans(id),
  job           TEXT,
  session       TEXT REFERENCES sessions(id),
  record        TEXT,
  event         INTEGER REFERENCES events(id),
  label         TEXT NOT NULL CHECK (length(label) >= 1),
  url           TEXT,
  opened_at     TEXT NOT NULL,
  closed_at     TEXT,
  snoozed_until TEXT,
  dismissed_at  TEXT,
  CHECK (plan IS NULL OR context IS NOT NULL),
  CHECK (job IS NULL OR plan IS NOT NULL)
) STRICT;
CREATE UNIQUE INDEX inbox_items_open ON inbox_items(kind, subject, urgency) WHERE closed_at IS NULL;
CREATE INDEX inbox_items_closed ON inbox_items(closed_at);

-- ---------- Tier catalog cache (§9) ----------
CREATE TABLE tier_catalog (
  pi_version TEXT NOT NULL,
  letter     TEXT NOT NULL CHECK (letter IN ('O', 'S', 'H')),
  model      TEXT NOT NULL,
  efforts    TEXT NOT NULL CHECK (json_valid(efforts) AND json_type(efforts) = 'array'),
  read_at    TEXT NOT NULL,
  PRIMARY KEY (pi_version, letter, model)
) STRICT;
```

**The runner's table and the lookup tables** (created by `kind-tables`, §4.5; `schema_migrations` by the runner itself, in the same transaction, never by a `.sql` file):

```sql
CREATE TABLE schema_migrations (
  name      TEXT    PRIMARY KEY CHECK (length(name) BETWEEN 6 AND 54),
  compat    TEXT    NOT NULL CHECK (compat IN ('additive', 'breaking')),
  checksum  TEXT    NOT NULL CHECK (length(checksum) = 64),      -- sha256 (hex) of the migration's text
  seq       INTEGER NOT NULL UNIQUE CHECK (seq >= 1),           -- order of application on this store
  txn       INTEGER NOT NULL REFERENCES txns(id),               -- the migrating transaction
  converted INTEGER NOT NULL CHECK (converted IN (0, 1))        -- 1: a legacy migration recorded by the conversion (§4.5)
) STRICT;
CREATE TRIGGER schema_migrations_own BEFORE INSERT ON schema_migrations WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'schema_migrations.txn must be cpd_txn()'); END;
CREATE TRIGGER schema_migrations_no_update BEFORE UPDATE ON schema_migrations BEGIN SELECT RAISE(ABORT, 'append_only: schema_migrations'); END;
CREATE TRIGGER schema_migrations_no_delete BEFORE DELETE ON schema_migrations BEGIN SELECT RAISE(ABORT, 'append_only: schema_migrations'); END;

CREATE TABLE event_kinds    (name TEXT PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 40)) STRICT, WITHOUT ROWID;
CREATE TABLE record_kinds   (name TEXT PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 40)) STRICT, WITHOUT ROWID;
CREATE TABLE history_tables (name TEXT PRIMARY KEY, no_delete INTEGER NOT NULL CHECK (no_delete IN (0, 1))) STRICT, WITHOUT ROWID;

-- what kind-tables inserts (the kinds the two CHECKs listed at 0013-pr-inbox, and the tables of "Triggers" below):
INSERT INTO event_kinds (name) VALUES
  ('context-created'), ('context-record'), ('context-grounded'),
  ('plan-created'), ('plan-phase'), ('plan-review'), ('plan-done'),
  ('job-added'), ('job-edited'), ('job-merged'), ('job-failed'), ('job-retried'),
  ('job-cancel-requested'), ('job-cancelled'),
  ('attempt-started'), ('attempt-status'), ('attempt-retry'), ('attempt-nudge'),
  ('all-merged'), ('pr-feedback'),
  ('pr-added'), ('pr-edited'), ('pr-cancelled'), ('pr-started'), ('pr-opened'), ('pr-merged'),
  ('escalation'), ('escalation-answered'), ('escalation-expired'),
  ('session-registered'), ('session-state'), ('session-moved'), ('session-renamed'),
  ('context-renamed'), ('plan-renamed'),
  ('context-archived'), ('plan-archived'), ('session-archived'),
  ('session-message'),
  ('inbox-opened'), ('inbox-closed'), ('inbox-snoozed'), ('inbox-dismissed'),
  ('watch-started'), ('watch-stopped'), ('error');
INSERT INTO record_kinds (name) VALUES ('decision'), ('note'), ('question'), ('answer'), ('plan'), ('brief');
INSERT INTO history_tables (name, no_delete) VALUES
  ('contexts', 1), ('context_records', 1), ('plans', 1), ('pull_requests', 0), ('prs', 1), ('pr_needs', 0),
  ('plan_reviews', 1), ('jobs', 1), ('dependencies', 0), ('attempts', 1), ('escalations', 1),
  ('sessions', 1), ('inbox_items', 1), ('tier_catalog', 1), ('event_kinds', 1), ('record_kinds', 1), ('history_tables', 1);
```

Only lists that an older binary can ignore are lookup tables (hs-d178 D1): the Event kinds and the record kinds. State-machine columns (`plans.phase`, `jobs.status`, `prs.status`, `attempts.status`, `attempts.cause`, `sessions.role` and `state`, …) stay `CHECK`s, with their transition triggers: a new state changes what old code must handle, so changing one is a breaking migration. No other column becomes a lookup table without an amendment of this paragraph. The other `CHECK`s of `context_records` stay, since they name specific kinds. The three lookup tables are themselves in `history_tables` with `no_delete` 1: a row cannot be deleted, every change is recorded in `history`, and an insert from the `sqlite3` shell fails on `cpd_txn()` like any other table (§4.3).

**Triggers.** For each table `T` listed in `history_tables` (`contexts`, `context_records`, `plans`, `pull_requests`, `prs`, `pr_needs`, `plan_reviews`, `jobs`, `dependencies`, `attempts`, `escalations`, `sessions`, `inbox_items`, `tier_catalog` and the three lookup tables), the runner generates three triggers from `T`'s current column list (shown for `escalations`; the others differ only in table name, primary key and columns); `no_delete` 0 means no `_no_delete` (`dependencies`, `pr_needs` and `pull_requests` write their own rule, below). **A migration never creates a generated trigger** (it may drop them, which SQLite needs before `DROP COLUMN`); it inserts a row into `history_tables` for a table it creates, and after the last migration of a migrating transaction the runner **reconciles** (§4.5), so the triggers always list every column of the final table and do not depend on the order in which additive migrations added columns. The legacy migrations (§4.5) still create theirs, with the same generator. The generated names `T_hist_ins`, `T_hist_upd` and `T_no_delete` are reserved; the schema check verifies them against the generator (§4.3 item 5):

```sql
CREATE TRIGGER escalations_hist_ins AFTER INSERT ON escalations BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'escalations',
    json_array(NEW.plan, NEW.id), 'insert', NULL,
    json_object('plan', NEW.plan, 'id', NEW.id, 'attempt', NEW.attempt, 'number', NEW.number,
                'question', NEW.question, 'status', NEW.status, 'answer', NEW.answer,
                'asker_pid', NEW.asker_pid, 'asked_at', NEW.asked_at,
                'answered_at', NEW.answered_at, 'expired_at', NEW.expired_at));
END;
CREATE TRIGGER escalations_hist_upd AFTER UPDATE ON escalations BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'escalations',
    json_array(NEW.plan, NEW.id), 'update',
    json_object('plan', OLD.plan, 'id', OLD.id, 'attempt', OLD.attempt, 'number', OLD.number,
                'question', OLD.question, 'status', OLD.status, 'answer', OLD.answer,
                'asker_pid', OLD.asker_pid, 'asked_at', OLD.asked_at,
                'answered_at', OLD.answered_at, 'expired_at', OLD.expired_at),
    json_object('plan', NEW.plan, 'id', NEW.id, 'attempt', NEW.attempt, 'number', NEW.number,
                'question', NEW.question, 'status', NEW.status, 'answer', NEW.answer,
                'asker_pid', NEW.asker_pid, 'asked_at', NEW.asked_at,
                'answered_at', NEW.answered_at, 'expired_at', NEW.expired_at));
END;
CREATE TRIGGER escalations_no_delete BEFORE DELETE ON escalations BEGIN
  SELECT RAISE(ABORT, 'append_only: escalations');
END;
```

`Store::open` generates these triggers from the table definitions above (primary key: `id` for `contexts`, `plans` and `sessions`; `seq` for `context_records` and `jobs`; `id` for `pull_requests` and `inbox_items` too, the declared `PRIMARY KEY` for the others, `prs` and `pr_needs` included), and a test asserts that every column of every table appears in its `json_object` calls.

The exceptions to "never delete": a pull request row that no PR points to may be deleted (the poller deletes it when its pull request leaves the open search; hs-d96, D5), a Dependency of a `draft` Job may be deleted (that is how `job edit --needs` changes a draft), and a PR's need may be deleted while its Plan is in `grounding` or `plan` (that is how `pr edit --needs` changes it; R34). `pr_needs` gets `pr_needs_no_delete` and `pr_needs_hist_del`, written as below for `dependencies` with the condition `(SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')` and the error `pr_locked`:

`pull_requests` gets no `_no_delete`. It gets `pull_requests_hist_del`, a delete history trigger written like `dependencies_hist_del` below (`json_array(OLD.id)`, `json_object` of every column), and `pull_requests_owned`, which refuses the delete of a row a PR points to:

```sql
CREATE TRIGGER pull_requests_owned BEFORE DELETE ON pull_requests
WHEN EXISTS (SELECT 1 FROM prs WHERE pull_request = OLD.id)
BEGIN SELECT RAISE(ABORT, 'append_only: pull_requests owned by a PR'); END;
```

```sql
CREATE TRIGGER dependencies_no_delete BEFORE DELETE ON dependencies
WHEN (SELECT status FROM jobs WHERE plan = OLD.plan AND key = OLD.job) <> 'draft'
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
CREATE TRIGGER dependencies_hist_del AFTER DELETE ON dependencies BEGIN
  INSERT INTO history(txn, tbl, pk, op, before, after) VALUES (cpd_txn(), 'dependencies',
    json_array(OLD.plan, OLD.job, OLD.needs), 'delete',
    json_object('plan', OLD.plan, 'job', OLD.job, 'needs', OLD.needs), NULL);
END;
```

Locked, in the database:

```sql
CREATE TRIGGER jobs_locked BEFORE UPDATE OF title, tier, goal, context, touches, acceptance, out_of_scope, plan, key, pr, review ON jobs
WHEN OLD.status <> 'draft'
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;

-- R34: a PR's identity, repo and base are Locked once its Plan is locked; needs only at the PR's own insert (pr add in dispatch)
-- migration 5 (hs-d118): one exception, the legacy Lead Branch of a merged PR moves to `cpd/<plan>/<key>` (pr add, §3.8)
CREATE TRIGGER prs_locked BEFORE UPDATE OF plan, key, repo, base, lead_branch ON prs
WHEN (SELECT phase FROM plans WHERE id = OLD.plan) NOT IN ('grounding', 'plan')
 AND NOT (OLD.status = 'merged'
          AND NEW.plan IS OLD.plan AND NEW.key IS OLD.key AND NEW.repo IS OLD.repo AND NEW.base IS OLD.base
          AND OLD.lead_branch = 'cpd/' || OLD.plan
          AND NEW.lead_branch = 'cpd/' || OLD.plan || '/' || OLD.key)
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

CREATE TRIGGER pr_needs_locked BEFORE INSERT ON pr_needs
WHEN (SELECT phase FROM plans WHERE id = NEW.plan) NOT IN ('grounding', 'plan')
 AND (SELECT created_txn FROM prs WHERE plan = NEW.plan AND key = NEW.pr) <> cpd_txn()
BEGIN SELECT RAISE(ABORT, 'pr_locked'); END;

CREATE TRIGGER dependencies_locked BEFORE INSERT ON dependencies
WHEN (SELECT status FROM jobs WHERE plan = NEW.plan AND key = NEW.job) <> 'draft'
 AND (SELECT created_txn FROM jobs WHERE plan = NEW.plan AND key = NEW.job) <> cpd_txn()
BEGIN SELECT RAISE(ABORT, 'job_locked'); END;
```

Dependencies only inside one PR (R34, A7), in the database. `job edit` that changes both `--pr` and `--needs` deletes the old Dependencies, updates `pr`, then inserts the new ones, so neither trigger fires on a legal edit:

```sql
CREATE TRIGGER dependencies_same_pr BEFORE INSERT ON dependencies
WHEN (SELECT pr FROM jobs WHERE plan = NEW.plan AND key = NEW.job)
  IS NOT (SELECT pr FROM jobs WHERE plan = NEW.plan AND key = NEW.needs)
BEGIN SELECT RAISE(ABORT, 'needs_other_pr'); END;

CREATE TRIGGER jobs_pr_dependencies BEFORE UPDATE OF pr ON jobs
WHEN OLD.pr <> NEW.pr AND EXISTS (SELECT 1 FROM dependencies WHERE plan = NEW.plan AND (job = NEW.key OR needs = NEW.key))
BEGIN SELECT RAISE(ABORT, 'needs_other_pr'); END;
```

Legal transitions, in the database (the tables of §3 as data):

```sql
CREATE TRIGGER plans_phase BEFORE UPDATE OF phase ON plans
WHEN OLD.phase <> NEW.phase AND (OLD.phase || '>' || NEW.phase) NOT IN
  ('grounding>plan', 'plan>dispatch', 'dispatch>deploy', 'deploy>done', 'deploy>dispatch', 'done>dispatch')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER prs_status BEFORE UPDATE OF status ON prs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('waiting>dispatch', 'dispatch>open', 'open>merged', 'waiting>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER jobs_status BEFORE UPDATE OF status ON jobs
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('draft>queued', 'queued>running', 'running>merged',
   'running>queued', 'running>failed', 'failed>queued',
   'draft>cancelled', 'queued>cancelled', 'failed>cancelled', 'running>cancelled')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER attempts_status BEFORE UPDATE OF status ON attempts
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN
  ('starting>running', 'starting>ended', 'running>merging', 'running>ended', 'merging>ended')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER attempts_ended BEFORE UPDATE ON attempts
WHEN OLD.status = 'ended'
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;   -- metrics are written in the transaction that ends the Attempt

CREATE TRIGGER escalations_status BEFORE UPDATE OF status ON escalations
WHEN OLD.status <> NEW.status AND (OLD.status || '>' || NEW.status) NOT IN ('open>answered', 'open>expired')
BEGIN SELECT RAISE(ABORT, 'illegal_transition'); END;

CREATE TRIGGER context_records_immutable BEFORE UPDATE ON context_records
BEGIN SELECT RAISE(ABORT, 'append_only: context_records'); END;
```

Append-only audit tables and writes only through cpd:

```sql
CREATE TRIGGER txns_own BEFORE INSERT ON txns WHEN NEW.id IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'txns.id must be cpd_txn()'); END;
CREATE TRIGGER events_own BEFORE INSERT ON events WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'events.txn must be cpd_txn()'); END;
CREATE TRIGGER history_own BEFORE INSERT ON history WHEN NEW.txn IS NOT cpd_txn()
BEGIN SELECT RAISE(ABORT, 'history.txn must be cpd_txn()'); END;
CREATE TRIGGER txns_no_update    BEFORE UPDATE ON txns    BEGIN SELECT RAISE(ABORT, 'append_only: txns'); END;
CREATE TRIGGER txns_no_delete    BEFORE DELETE ON txns    BEGIN SELECT RAISE(ABORT, 'append_only: txns'); END;
CREATE TRIGGER history_no_update BEFORE UPDATE ON history BEGIN SELECT RAISE(ABORT, 'append_only: history'); END;
CREATE TRIGGER history_no_delete BEFORE DELETE ON history BEGIN SELECT RAISE(ABORT, 'append_only: history'); END;
CREATE TRIGGER events_no_update  BEFORE UPDATE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
CREATE TRIGGER events_no_delete  BEFORE DELETE ON events  BEGIN SELECT RAISE(ABORT, 'append_only: events'); END;
```

`events` are not copied into `history`: the table is itself append-only and carries its `txn`.

### 4.5 Schema versions and migrations

The store's schema changes by **named migrations** (hs-d178; design `docs/research/migrations.md`, as amended by the User's decisions D1–D4, which win where they differ). Everything below is normative for `core/src/store/`.

- **Names (N1).** A migration is a file `core/src/store/migrations/<name>.sql`, `<name>` = `<YYYYMMDDTHHMM>-<slug>`: the **UTC** minute its author created it (by rule; no `Z`), then a slug `[a-z][a-z0-9-]{0,39}`, for example `20261019T0905-plan-pins`. Names order bytewise, which is chronological. A build test refuses any other shape. The name is the migration's identity.
- **Legacy names (N2).** The migrations that exist as numbered files when the first named migration lands are the **legacy set**: `0001-initial`, `0002-cockpit-parity`, `0003-archive`, `0004-cross-repo`, `0005-plan-reopen`, `0006-pr-poller`, `0007-context-records`, `0008-phase-deploy`, `0009-session-message`, `0010-manager`, `0011-pr-mergeable`, `0012-inbox`, `0013-pr-inbox`, one name per numbered file, the number kept as the prefix (every numbered migration on `main` when the conversion integrated it joins the set with its number as the prefix, frozen like these; hs-d178 D3, hs-d211). `0` sorts before `2`, so the legacy set always comes first. It is a frozen constant of the store module (`LEGACY`); each entry keeps its Rust function. The files `0001.sql` … `0013.sql` are renamed with `git mv` to their legacy names, and a numbered file is never added again. The legacy per-version hashes (`SCHEMA_HASHES`) stay, as **legacy schema hashes**, used only by the conversion (C2 step 2): the sha256 (hex) of the JSON array of `[type, name, sql]` rows of `SELECT type, name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name`, as integer-era binaries computed it.
- **Declaration (N3).** The first line of every named migration is `-- cpd-migration: additive` or `-- cpd-migration: breaking after <n>`; `n` is the number of migrations (legacy included) that sort before it: the ones its author wrote it against. A missing or malformed header fails the build.
- **The list is the directory (N4).** A build step lists `migrations/*.sql`, sorts by name and generates the array of `(name, compat, sql)`: a Plan that adds a migration adds one file and edits no shared line (it also edits `SCHEMA_HASH`, §4.3). A migration that needs more than SQL has a `<name>.rs` beside its `.sql`, with `pub fn run(conn, home)`, which the build step wires in. `scripts/new-migration.sh <slug> additive|breaking` writes the file with the name from `date -u +%Y%m%dT%H%M` and the header, and for a breaking migration counts the files for `after <n>`.
- **Frozen once merged (N5).** A migration file on `main` is never edited, renamed or deleted. The runtime checksum (R3) catches an edit on any store that applied it; the Gate also fails when the diff against `origin/main` modifies, renames or deletes a file of `migrations/` (`git diff --diff-filter=MDR origin/main...HEAD -- core/src/store/migrations/`, a hook of `.config/wt.toml`, which only the Lead edits).
- **What the store records (R1–R4).** The table `schema_migrations` (§4.4) has one row for every applied migration: name, `compat`, `checksum` (sha256 of the file's text), `seq` (the order of application on this store) and `txn`. It records **no schema hash**: the trusted hash stays pinned in code (§4.3, D2). The runner creates it, not a migration, in the transaction that first records a migration. `PRAGMA user_version` is **1000** in every named store and never changes again: it means only "this store uses named migrations". Every integer-era binary has a `VERSION` far below 1000, so it refuses such a store with its existing `store_too_new`.
  - **R3. Checksum.** For every applied migration whose name the binary knows, the recorded `checksum` must equal the sha256 of the binary's embedded text, else the open refuses with `migration_changed` (exit 1; `details: {name, recorded, embedded}`).
  - **R4. The floor** is the `breaking` row with the highest `seq` (O3 makes it the breaking row with the latest name): the oldest schema allowed to open the store. A store with no breaking row has none.
- **Lookup tables (K1–K3).** `event_kinds` and `record_kinds` (§4.4) list the kinds; `events.kind` and `context_records.kind` reference them (hs-d178 D1: no other column). **A kind exists when its row does.** Adding one is an additive migration of one statement, `INSERT INTO event_kinds (name) VALUES ('<kind>');`: two such migrations commute and neither can drop the other's kind, because neither rewrites a list (§5.2; adding an Event kind also raises `PROTOCOL`). Old binaries read an Event kind as a string, so that is additive. A record kind is parsed into a closed type (`RecordKind`) by every binary: **a new value that old code parses into a closed type makes a migration `breaking`** (D1). No test can enforce that; the author declares it, and a review checks it.
- **History triggers are derived (K4).** A migration never *creates* `T_hist_ins`, `T_hist_upd` or `T_no_delete` (§4.4); it inserts a row into `history_tables`. After the last migration of a migrating transaction the runner **reconciles**: for each row of `history_tables` it generates the three triggers from the table's current columns and primary key (the generator of the legacy migrations, §4.4 "Triggers", unchanged), keeps a trigger whose `sqlite_schema.sql` is identical to the generated SQL and drops and recreates any that differs or is missing; a listed table that does not exist is `migration_failed`. The result depends only on the final columns, so it is the same in any order of application. The hand-written triggers (`dependencies_hist_del`, `pr_needs_hist_del`, `pull_requests_hist_del`, `pull_requests_owned`) stay owned by their migrations. The generator's output is part of the schema: changing it changes every store's triggers, so it ships only with a breaking migration. Dropping a table listed in `history_tables` is not supported until needed. A rebuild of a table (`CREATE <t>_new`, copy, `DROP`, `RENAME`) copies only the columns its author knew, so every rebuild is **breaking**.
- **Additive means compatible (A1).** A binary that does not know an *additive* migration can still read and write the store correctly at the SQL level. Every statement of an additive migration is one of: `CREATE TABLE`, with an insert into `history_tables` for it; `CREATE INDEX` that is not `UNIQUE` on an existing table (any index on a new table); `CREATE TRIGGER` on a table the same migration creates; `ALTER TABLE <existing> ADD COLUMN`, nullable or `NOT NULL` with a constant `DEFAULT` (a `CHECK` or `REFERENCES` on it is allowed, since a null passes both); `INSERT` into `event_kinds`, `record_kinds`, `history_tables`, or into a table the migration created. Everything else is **breaking**: a rebuild, `DROP`, `RENAME`, `UPDATE` or `DELETE` of existing rows, a new or changed trigger or unique index on an existing table, **and a trigger change that only relaxes** (the legacy `0005-plan-reopen`; D1). A `breaking` header on a migration that would pass is allowed (the label is conservative); an `additive` header on one that fails is not.
- **The checks (A2, A3; `core/tests/`, so `cargo test` and the Gate run them).** (1) *Additive test:* every migration declared `additive` is run on the name-ordered prefix before it, and an SQLite authorizer allows only the actions of A1 (`CREATE TABLE/INDEX/TRIGGER`, `ALTER TABLE`, `INSERT` into the tables A1 names, reads) and denies `UPDATE`, `DELETE`, every `DROP` and `PRAGMA`; and a catalog diff: every `sqlite_schema` row that existed before is unchanged after, except that a table's `sql` has only grown by `ADD COLUMN`; the old `pragma_table_xinfo` rows are an unchanged prefix of the new ones and each new column is nullable or has a non-null `dflt_value`; every new trigger is on a new table; every new index on an old table has `unique = 0`. (2) *Order test:* for every migration declared `breaking after n`, exactly `n` names sort before it in the binary's list. (3) *Orders test:* applying the additive migrations of the list in every permutation to the same prefix ends with the same canonical hash (§4.3) and with `SCHEMA_HASH`; and no code uses `SELECT *` or an `INSERT` without a column list (cpd names its columns, so column order is harmless; O2).
- **The migrating transaction.** At start-up every `cpd` process opens the database and computes the verdict (below). When migrations are pending (a fresh store, a conversion, or a newer binary), it runs `BEGIN IMMEDIATE`, computes the verdict again inside the transaction (another process may have migrated meanwhile), inserts one `txns` row with `command = 'migrate'`, applies the migrations, reconciles (K4), checks `PRAGMA foreign_key_check`, inserts one `schema_migrations` row for each migration it applied, runs the schema check (§4.3) and commits. A migration that fails rolls the whole transaction back and the command fails with `migration_failed` (exit 1). **The schema check of a migration** (hs-n121, S2): the check every open runs (§4.3: the generated triggers, then the pinned canonical hash, or the canonical subset inside the window) also runs inside the migrating transaction, after the last migration and before the commit. A mismatch rolls the whole transaction back, so `user_version` and every table stay as they were, and the command fails with `migration_failed` (exit 1; `details: {migration: "schema_check", expected, actual, object}`, `object` only for a single object). A migration that produces a schema this binary does not describe is never committed. Rerunning is safe: nothing was written. The check at the open follows, as at every open.
- **Order (O1–O3).** *O1:* a fresh store applies the legacy set, then every named migration in name order, in one `BEGIN IMMEDIATE` transaction, and records each. *O2:* on an existing store the runner applies the binary's unapplied migrations in name order after what the store has, so `seq` is the order of application. An unapplied migration may sort before an applied one (Plan A's lands on `main` after Plan B's, which a machine already applied): **allowed when it is additive**, because additive migrations commute (distinct objects, rows and columns; column order differs between stores and is harmless, by the Orders test). *O3: a breaking migration is a barrier.* At build, its `after n` must equal the number of names that sort before it (check 2), so a migration that lands on `main` after it with an earlier name fails and must be renamed to a newer timestamp (`git mv`, `scripts/new-migration.sh`): when two Plans have breaking migrations, whoever lands second re-checks the rebuild against the first (hs-d68). At run time the runner refuses with `migration_order` (exit 1, nothing written; `details: {name, applied_after, unknown}`) to apply a breaking migration when the store has an applied migration that sorts after it, or one this binary does not know: its rebuild was written without seeing something the store has.
- **The window and its one verdict (W1–W5).** `store::verdict(conn, list) -> Verdict` is computed from read-only state (`user_version`, the `schema_migrations` rows, the binary's list), the first row that matches deciding:

  | Store state | Verdict |
  |---|---|
  | no file, or `user_version = 0` | opens: fresh (O1) |
  | `1 ≤ user_version ≤ LEGACY.len()` | opens: convert (C1) |
  | `LEGACY.len() < user_version < 1000` | `store_too_new`: a numbered migration this binary lacks |
  | `user_version > 1000` | `store_too_new`: a later scheme |
  | `user_version = 1000`, no `schema_migrations` | `store_failed`: not a store cpd wrote |
  | an applied name this binary knows, with another checksum (R3) | `migration_changed` |
  | the floor (R4) is not in the binary's list | `store_too_new`; `unknown` lists every applied name the binary lacks |
  | a pending breaking migration breaks O3 | `migration_order` |
  | a pending additive migration sorts before the floor | `migration_order` (unreachable when check 2 holds; it fails loudly anyway) |
  | otherwise | opens; `pending` = the binary's unapplied names, applied in name order |

  A refusal carries `{user_version, floor, unknown}` for `store_too_new` (`floor` null when none). **The window:** a binary opens the store when it knows the store's floor. Knowing the floor implies knowing every migration that sorts before it (check 2 in the binary that contains the floor), so an older binary lacks only additive migrations named after it, and by A1 they cannot change what it reads or writes at the SQL level. It reads and writes normally and runs the schema check on the canonical subset (§4.3). If it has pending migrations of its own (a Lead Branch build), it applies them by O2/O3 and reconciles (K4), which covers tables of migrations it does not know, because `history_tables` is in the store. **Who uses the verdict:** nothing compares schema numbers on its own. `Store::open` refuses with the verdict's code, and otherwise migrates `pending` and runs the schema check (§4.3); it recomputes the verdict inside `BEGIN IMMEDIATE`, since another process may have migrated meanwhile (the order is today's: migrate, then check; the pinned hash makes that safe). A process keeps its connection for its life, so every write transaction re-reads `user_version` after `BEGIN IMMEDIATE`, before its `txns` row, and refuses with `store_too_new` when it is neither a legacy version, nor 0, nor 1000. `cpd --version` reports it (commands.md §1.2) and never creates or migrates. The install guard and the reload guard (§3.7, §3.4, §6.7) and `events follow` (§5.4) act on it; `doctor`'s `store` check reports its refusal (commands.md §14.1; there is no separate `store_window` check, D1: `--version`'s `store.unknown` is enough). **`store_too_new` keeps its code and exit 1** and means: this store has a breaking migration this cpd does not know, or a later scheme.
- **Conversion of the legacy store (C1–C6).** The first binary with named migrations opens a store with `1 ≤ user_version ≤ LEGACY.len()` and no `schema_migrations` table (on a User's machine: `hs`, `air` and `pro`, at their integer-era `user_version`, 13 once they run `main` with `0013`). Converting `~/.cpd` is a Lead step with the User's go after the PR merges; code and tests never touch the real `~/.cpd`. In **one** `BEGIN IMMEDIATE` transaction under `locks/store.lock`, with the pragmas `foreign_keys = OFF` and `legacy_alter_table = ON` set around it as for every migration (the legacy functions need them, migration 4 step 1):
  1. re-read `user_version` and check that `schema_migrations` still does not exist; if another process converted meanwhile, take the named path instead;
  2. check that the legacy schema hash of the store equals `SCHEMA_HASHES[user_version - 1]`; on a mismatch refuse with `store_tampered` and convert nothing (a tampered store must not become the base of a trusted chain);
  3. apply the legacy functions `user_version + 1 ..= LEGACY.len()`; right after `0007-context-records` the runner imports the old Context stores (its Rust step, below), and moves their folders after the commit;
  4. create `schema_migrations` and insert one row per legacy migration: its legacy name; `compat` `breaking` for `0001`, `0003` to `0013` and `additive` for `0002` (A1 applied to each: `0005` is breaking because its triggers only relax, D1; `0011` and `0013` because they rewrite the hand-written `pull_requests_hist_del`, `0012` and `0013` because they rebuild a table); the checksum of its `.sql` text; `seq` = its number; `txn` = the migrating `txns` row; `converted` 1. The original application times were never recorded, so none are invented;
  5. apply the named migrations in name order, the first being `kind-tables` (below), then reconcile (K4), run `PRAGMA foreign_key_check` (any row: `migration_failed`), set `user_version = 1000`, run the schema check (§4.3; a mismatch is `migration_failed`, above) and commit. The schema check runs again at the open, as at every open.

  Any failure rolls the whole transaction back and leaves the store at its old `user_version`, readable by the old binary; a rerun starts from step 1 and, once committed, step 1 never matches again, so converting is idempotent. Nothing is deleted. A **fresh** store applies the same legacy functions and `kind-tables` and records the legacy rows with `converted` 0. **An old binary meets a converted store** (`user_version 1000 > VERSION`): `Store::open` and `events follow` refuse with `store_too_new`, and its `--version` reports `store_schema 1000 > schema`, so the old install guard and the old reload guard (`store_schema` above `schema`) both refuse it: correct, because `kind-tables` is breaking. **A new binary meets an unconverted store:** it converts; its `--version` says `store_schema` 1000 (it opens), so integer-era drivers and wrappers still running reload into it, the same one-time disruption as `0004` and `0006`. A store at a numbered version that is not in the legacy set (above 13) is refused with `store_too_new`. **The tests:** version-1 to version-13 databases with rows in every table convert, keep every row and id, pass `foreign_key_check`, have thirteen `converted = 1` rows and end with the same canonical hash as a fresh store (the legacy tests of each version's hash stay, on the legacy hash); a legacy database with a changed trigger is refused and unchanged; converting twice is a no-op.

**The legacy record.** `0001-initial` created the tables as they were at version 1 (§4.4). The twelve bullets below are the record of `0002-cockpit-parity` (migration 2), `0003-archive` (3), `0004-cross-repo` (4), `0005-plan-reopen` (5), `0006-pr-poller` (6), `0007-context-records` (7), `0008-phase-deploy` (8), `0009-session-message` (9), `0010-manager` (10), `0011-pr-mergeable` (11), `0012-inbox` (12) and `0013-pr-inbox` (13); the legacy set is frozen (N2). Under the named scheme, what their texts say about `user_version`, the pragma pair around the transaction, the history triggers they drop and recreate, and `PRAGMA foreign_key_check` is done once for the whole migrating transaction by the runner (the conversion's steps 3 and 5, a fresh store's O1); "its schema hash equals that of a fresh database" in their tests means the legacy schema hash (N2).

- **Migration 2** (cockpit parity; a real `cpd.db` exists since migration 1 shipped, so the change is a migration and not an edit of §4.4): adds three nullable columns to `plans`, each with its own check, then drops and recreates `plans`' history triggers (the rule above):

  ```sql
  ALTER TABLE plans ADD COLUMN pr_title TEXT CHECK (pr_title IS NULL OR length(pr_title) >= 1);
  ALTER TABLE plans ADD COLUMN pr_review_decision TEXT CHECK (pr_review_decision IN ('none', 'approved', 'changes_requested'));
  ALTER TABLE plans ADD COLUMN pr_checks TEXT CHECK (pr_checks IN ('none', 'passed', 'pending', 'failed'));
  ```

  A Plan that has a PR when the migration runs keeps the three columns null until its next `pr poll`. `pr poll` writes the three together (§7.6); `ALTER TABLE` cannot add a check across columns, so the binary keeps them all null or all set (migration 4 moves them to `prs`, where a check holds it).
- **Migration 3** (archive and rename, the tree's right-click menu): adds a nullable `archived_at` to `contexts`, `plans` and `sessions`, rebuilds `events`, and drops and recreates the history triggers of the three tables (the rule above):

  ```sql
  ALTER TABLE contexts ADD COLUMN archived_at TEXT;
  ALTER TABLE plans    ADD COLUMN archived_at TEXT;
  ALTER TABLE sessions ADD COLUMN archived_at TEXT;
  ```

  `events`' `kind` check gains `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived` and `session-archived` (§5.2). SQLite cannot alter a check, so the migration **recreates the table**: it creates the new `events` with the longer check, copies every row (ids included, so the `AUTOINCREMENT` sequence and every Event cursor stay valid), drops the old table, renames the new one, and recreates `events`' indexes (`events_plan`, `events_context`, `events_session`) and its triggers (`events_own`, `events_no_update`, `events_no_delete`). Existing rows are unchanged. Every `archived_at` is null until the first archive. If another migration numbered 3 reaches `main` first, this one is renumbered when it is rebased.
- **Migration 4** (R34, A6: Plans across repos), applied after migration 3. It moves the per-repo columns from `plans` to the new table `prs`, adds `jobs.pr` and `jobs.review` (B2: `INTEGER NOT NULL DEFAULT 0 CHECK (review IN (0, 1))`, 0 for every existing Job), adds the six `pr-*` Event kinds and drops `context_records.plan_repo`. `ALTER TABLE` can do none of it except `CREATE TABLE`: SQLite refuses `DROP COLUMN` on a column named in a table `CHECK` or `UNIQUE` (`plans.repo` is not, but `lead_branch`, `lead_worktree` and `pr_number` are), cannot add a composite foreign key to `jobs`, and cannot change the `CHECK` on `events.kind` or `context_records` (docs: SQLite `ALTER TABLE`). So migration 4 rebuilds `plans`, `jobs`, `events` and `context_records` with SQLite's documented procedure (docs: "Making Other Kinds Of Table Schema Changes"):
  1. before its `BEGIN IMMEDIATE`, `PRAGMA foreign_keys = OFF` (the pragma is a no-op inside a transaction, docs) and `PRAGMA legacy_alter_table = ON`, so the renames below do not re-check the triggers of other tables that name a table while it is being replaced (the migration tests exercise the whole procedure);
  2. `CREATE TABLE prs` and `pr_needs` (§4.4) with their history triggers, and fill `prs` with **one PR per Plan**, one row per `plans` row:

     | `prs` column | from the Plan |
     |---|---|
     | `plan` | `id` |
     | `key` | the base name of `repo`, made a valid PR key by the binary: lower-cased, each character outside `[a-z0-9-]` replaced by `-`, prefixed `pr-` when it does not then start with a letter, cut to 24 characters |
     | `repo`, `base`, `lead_branch`, `lead_worktree` | the same columns, kept as stored (`lead_branch` stays `cpd/<plan>`, the worktree `<repo-name>.<plan>`) |
     | `status` | from `phase`: `grounding` and `plan` → `waiting`; `dispatch` → `dispatch`; `pr` → `open`; `done` → `merged` |
     | `pr_number`, `pr_url`, `pr_opened_at`, `pr_seen`, `pr_polled_at`, `pr_title`, `pr_review_decision`, `pr_checks`, `all_merged_at` | the same columns |
     | `start_failed_at` | null |
     | `created_txn` | the migration's own `txns` row |
     | `created_at`, `started_at`, `merged_at` | `created_at`, `locked_at`, `done_at` |

     No `pr_needs` rows: one PR has no needs;
  3. for each of `plans`, `jobs`, `events` and `context_records`: `CREATE TABLE <t>_new` with the §4.4 definition, `INSERT INTO <t>_new SELECT …` every row with its primary key kept (`jobs.seq`, `events.id`, `context_records.seq`), `DROP TABLE <t>`, `ALTER TABLE <t>_new RENAME TO <t>`. `jobs.pr` is filled with the key of its Plan's one PR and `jobs.review` with 0; `plans` loses the moved columns and keeps `archived_at` (migration 3); `context_records` loses `plan_repo`; `events` keeps migration 3's kinds and gains the six, and its `AUTOINCREMENT` sequence is kept at least the old one's, as migration 3 keeps it;
  4. recreate every index and trigger of the rebuilt tables (§4.4: `plans_phase`, `jobs_locked` (which now lists `review`), `jobs_status`, `jobs_pr_dependencies`, `events_own`, `events_no_update`, `events_no_delete`, `context_records_immutable`, the indexes `events_plan`, `events_context`, `events_session`), the new ones (`prs_status`, `prs_locked`, `pr_needs_locked`, `pr_needs_no_delete`, `dependencies_same_pr`), and the history triggers of `plans`, `jobs` and `context_records` (the rule above). `events_own` is created after the copy, because the copied rows carry their old `txn`;
  5. `PRAGMA foreign_key_check` must return no row, else the migration fails (`migration_failed`) and rolls back; set `user_version = 4`; commit; then `PRAGMA legacy_alter_table = OFF` and `PRAGMA foreign_keys = ON`.

  History: each migrated PR gets an `insert` history row in the migration's `txns` row (step 2); the copies of step 3 write none, because the rebuilt tables' history triggers exist only after them, and the moved values stay readable in the earlier history rows of `plans`. A Plan in phase `plan` keeps its Jobs `draft` and its review, but the new plan fingerprint (§2.3) no longer matches the review's, so it needs a new `plan review` before `plan lock`. The tests: a version-2 database with one Plan in each phase, Jobs, Attempts, Events and Context records migrates through 3 to 4, keeps every row and id, passes `PRAGMA foreign_key_check`, and its schema hash equals that of a fresh database migrated to 4; a version-3 database as migration 3 leaves it (archived rows, the archive Event kinds) migrates to 4 keeping every `archived_at`.
- **Migration 5** (hs-d87, A4: a Plan takes follow-up PRs after `pr` and `done`; no table rebuild), applied after migration 4. It drops and recreates two triggers and changes no table:
  - `plans_phase` gains `pr>dispatch` and `done>dispatch` (§4.4). The existing `CHECK ((phase = 'done') = (done_at IS NOT NULL))` already forces `done_at` to be cleared by the same update;
  - `prs_locked` gains one exception (§4.4): an update whose only `lead_branch` change is from `'cpd/' || OLD.plan` to `'cpd/' || OLD.plan || '/' || OLD.key`, on a PR with `OLD.status = 'merged'`. Every other column it guards is still refused.

  Every row is unchanged; `user_version = 5`. Plan pr-poller's migration, which landed second, is migration 6 (hs-n59, hs-d68).
- **Migration 6** (the PR poller, hs-d96, D5, D6), applied after migration 5. It creates `pull_requests`, back-fills it from the existing PRs, and rebuilds `prs` without `pr_seen`, `pr_polled_at`, `pr_title`, `pr_review_decision` and `pr_checks` and with `pull_request`. `DROP COLUMN` cannot do it: SQLite refuses it on a column named in a table `CHECK`, and `pr_title` is named in `prs`' `CHECK`. So it rebuilds `prs` as migration 4 rebuilt `plans`: `prs_new`, copy, `DROP`, `RENAME`, history triggers generated again. Steps:
  1. as migration 4's step 1: before its `BEGIN IMMEDIATE`, `PRAGMA foreign_keys = OFF` and `PRAGMA legacy_alter_table = ON` (`pr_needs` and `jobs` name `prs`);
  2. create `pull_requests` (exactly this):

     ```sql
     CREATE TABLE pull_requests (id INTEGER PRIMARY KEY, repo TEXT NOT NULL CHECK (repo LIKE '_%/_%'), number INTEGER NOT NULL CHECK (number >= 1), url TEXT NOT NULL, title TEXT NOT NULL CHECK (length(title) >= 1), draft INTEGER NOT NULL CHECK (draft IN (0, 1)), state TEXT NOT NULL CHECK (state IN ('open', 'closed', 'merged')), review_decision TEXT NOT NULL CHECK (review_decision IN ('none', 'approved', 'changes_requested')), checks TEXT NOT NULL CHECK (checks IN ('none', 'passed', 'pending', 'failed')), created_at TEXT NOT NULL, merged_at TEXT, changed_at TEXT NOT NULL, UNIQUE (repo, number), CHECK ((state = 'merged') = (merged_at IS NOT NULL))) STRICT;
     ```

     with its history triggers: generated `pull_requests_hist_ins` and `pull_requests_hist_upd`, a delete history trigger `pull_requests_hist_del` like `dependencies_hist_del`, and **no** `pull_requests_no_delete`; instead

     ```sql
     CREATE TRIGGER pull_requests_owned BEFORE DELETE ON pull_requests
     WHEN EXISTS (SELECT 1 FROM prs WHERE pull_request = OLD.id)
     BEGIN SELECT RAISE(ABORT, 'append_only: pull_requests owned by a PR'); END;
     ```

  3. **back-fill**, one row for each `prs` row with a `pr_number` (the triggers of step 2 exist, so each row gets an `insert` history row in the migration's `txns` row):

     | `pull_requests` column | from the `prs` row |
     |---|---|
     | `repo` | parsed from `pr_url`, `https://github.com/<owner>/<name>/pull/<n>`: `<owner>/<name>` |
     | `number`, `url` | `pr_number`, `pr_url` |
     | `title` | `coalesce(prs.pr_title, plans.title)` |
     | `draft` | 0 |
     | `state`, `merged_at` | `merged` with `merged_at = prs.merged_at` for a PR in status `merged`, else `open` with null |
     | `review_decision`, `checks` | `coalesce(pr_review_decision, 'none')`, `coalesce(pr_checks, 'none')` |
     | `created_at`, `changed_at` | `pr_opened_at`, both |

  4. rebuild `prs`: `CREATE TABLE prs_new` with the §4.4 definition (without the five columns and their `CHECK`; `pull_request INTEGER REFERENCES pull_requests(id)`; `CHECK ((status IN ('open', 'merged')) = (pull_request IS NOT NULL))`), `INSERT INTO prs_new SELECT …` every row with `pull_request` set to the id of the back-filled row of its `pr_url` (null without a `pr_number`), `DROP TABLE prs`, `ALTER TABLE prs_new RENAME TO prs`. `pr_needs` and `jobs` keep referencing `prs(plan, key)`;
  5. recreate every trigger of `prs` (`prs_status`, and `prs_locked` with migration 5's exception, as §4.4) and generate its history triggers again (the rule above; the copy of step 4 writes no history);
  6. `PRAGMA foreign_key_check` must return no row, else `migration_failed`; set `user_version = 6`; commit; then `PRAGMA legacy_alter_table = OFF` and `PRAGMA foreign_keys = ON`.

  The five dropped columns' values stay readable in `prs`' earlier history rows. The tests: a version-5 database with PRs in every status, open ones with `pr_title` set and null, migrates to 6 keeping every other column and migration 5's triggers on `prs`, gets one row per PR with a number, passes `PRAGMA foreign_key_check`, and its schema hash equals that of a fresh database migrated to 6; deleting a row a PR points to is refused, deleting any other is allowed and leaves a `delete` history row.
- **Migration 0007** (Plan manager B1, hs-d23, hs-a1, A2: Context records only in the database), applied after migration 6; migration 0008 follows it (it renames a Plan phase). It imports from `$CPD_HOME/contexts/<id>/` (`context.json` and `records/*.jsonl`, the git store's format: one JSON record per line with `"v": 1`; a `plan` record may carry the pre-R34 field `plan.repo`, which is accepted and dropped) every Context and every record the database does not hold yet, keyed by Context id and by `(context, id)`, with the same row values the earlier import (`import_all`) wrote, and drops the column `contexts.sync_failing` (the history triggers of `contexts` are generated again, the rule above). A folder that is not a valid store fails the migration, which rolls back entirely (`migration_failed`). After the transaction commits, the code that applied the migration moves each `$CPD_HOME/contexts/<id>/` to `$CPD_HOME/archive/contexts/<id>/`, creating the parents. Nothing is deleted. Rerunning is safe: a folder already moved is absent. An existing destination is never overwritten: the folder stays and the failure is reported loud.
- **Migration 0008** (Plan manager B1, hs-d76, hs-d131, A4: the Plan phase `pr` is called `deploy`), applied after migration 0007. It renames the phase value in every place that holds it, in one transaction: (1) `plans` is rebuilt as migration 4 rebuilt it (`plans_new` with the §4.4 definition, whose `CHECK` lists `'grounding', 'plan', 'dispatch', 'deploy', 'done'`; copy with every primary key and column kept and `phase = 'pr'` written `'deploy'`; `DROP`, `RENAME`, history triggers generated again), because `ALTER TABLE` cannot change a `CHECK`; (2) `plans_phase` is dropped and recreated with `dispatch>deploy`, `deploy>done` and `deploy>dispatch` in place of `dispatch>pr`, `pr>done` and `pr>dispatch`; no other `CHECK` or trigger lists phases; (3) the `from` and `to` of every `plan-phase` Event's `data` that read `"pr"` are rewritten to `"deploy"`: the append-only guards `events_no_update` and `events_no_delete` are dropped before the rewrite and recreated after it, inside the transaction. Event `text`, Event kind names, the `cpd pr …` command names, the PR statuses and `data.pr` are unchanged. The tests: a database with one Plan in each phase and `plan-phase` Events with `pr` in `from` and `to` migrates keeping every row, id and `Event` `text`, holds `deploy` where it held `pr` and no `pr` phase anywhere, passes `PRAGMA foreign_key_check`, and its schema hash equals that of a fresh database migrated to 0008; `deploy>done` is accepted and `pr>done` refused.
- **Migration 0009** (Plan manager B2, hs-d92 A4, hs-d133, B2-A3: `cpd session send`), applied after migration 0008. It adds the Event kind `session-message` (§5.2) to the `CHECK` on `events.kind`, keeping every existing kind. SQLite cannot alter a check, so the migration recreates `events` as migration 3 and 4 did: create the new table with the §4.4 definition, copy every row (ids included, so the `AUTOINCREMENT` sequence and every Event cursor stay valid), drop the old table, rename the new one, recreate its indexes (`events_plan`, `events_context`, `events_session`) and triggers (`events_own`, created after the copy, `events_no_update`, `events_no_delete`), in one transaction with the foreign-key handling of migration 4 steps 1 and 5. The adapter's `KINDS` learns the kind. The tests: a database at 0008 with Events of every earlier kind migrates keeping every row and id, passes `PRAGMA foreign_key_check`, accepts a `session-message` Event, and its schema hash equals that of a fresh database migrated to 0009.

- **Migration 0010** (Plan manager B3, hs-d92, hs-d129, hs-d133, B3-A3: the Manager role and the `brief` record; the grounded Lead is retired), applied after migration 0009, in one transaction with the foreign-key handling of migration 4 steps 1 and 5. SQLite cannot alter a check or drop a column that a check names, so each table is rebuilt as migration 4 rebuilt `plans` (new table with the §4.4 definition, copy with every primary key and column kept, `DROP`, `RENAME`, indexes and history triggers generated again):
  1. `sessions` is rebuilt: `role` accepts `manager`; the checks become those of §4.4 (`(role = 'worker') = (attempt IS NOT NULL)`; a Manager has no Plan); the unique index `sessions_one_manager` (one non-archived Manager per Context) is created;
  2. `context_records` is rebuilt: `kind` accepts `brief`, and the `plan_id` check requires the Plan reference for `plan` and `brief` alike (§4.4); every `seq` is kept, so the `AUTOINCREMENT` sequence and every cursor stay valid;
  3. `contexts` is rebuilt without `lead_session` and `grounded_seq` and without the check that tied them;
  4. **every grounded Lead Session is archived**: each Session with `role = 'lead'` and `plan IS NULL` and `archived_at IS NULL` gets `archived_at` set to the migration's time, with one `session-archived` Event each, named as `session archive` names it (§5.2). Its pane, if any, is not touched here (the grounded Lead was never typed into; a pane that is still alive is closed by engine E2 or by the User). The rows stay, with `plan` null and `role = 'lead'`, as history.
  The Event kind `context-grounded` stays in the `events` check for history and is no longer appended. Event kinds are unchanged, so `events` is not rebuilt. The error code `grounded_lead` is removed with its last use. The tests: a database at 0009 with a Context that has a grounded Lead (and Plans, records of every kind, Events of every kind) migrates keeping every row and id, passes `PRAGMA foreign_key_check`, archives the grounded Lead with one `session-archived` Event, has no `lead_session` or `grounded_seq` column, accepts a `brief` record and a `manager` Session, refuses a second live Manager for one Context and accepts one after the first is archived, and its schema hash equals that of a fresh database migrated to 0010.
- **Migration 11** (file 0011; the PR poll's mergeable state, hs-d188; applied after migration 0010; the next free number on its base: written as 7, renumbered 11 at rebase because Plan manager's 0007–0010 landed first, hs-d68): adds two columns to `pull_requests`, then drops and recreates `pull_requests_hist_ins`, `pull_requests_hist_upd` and `pull_requests_hist_del` so they list every column (the rule above):

  ```sql
  ALTER TABLE pull_requests ADD COLUMN mergeable TEXT NOT NULL DEFAULT 'unknown' CHECK (mergeable IN ('mergeable', 'conflicting', 'unknown'));
  ALTER TABLE pull_requests ADD COLUMN merge_state TEXT NOT NULL DEFAULT 'unknown' CHECK (merge_state IN ('behind', 'blocked', 'clean', 'dirty', 'draft', 'has_hooks', 'unknown', 'unstable'));
  ```

  Existing rows get `unknown` for both until their next poll. `pull_requests_hist_del` is rewritten with the new columns; `pull_requests_owned` is unchanged and `pull_requests` still gets no `_no_delete`. Set `user_version = 11`.
- **Migration 0012** (file `0012-inbox`; the Inbox, hs-d168; D4 of Plan inbox; applied after migration 0011, the PR poll's mergeable state): creates `inbox_items` with its partial unique index, `inbox_items_no_delete` and its history triggers (§4.4), and rebuilds `events` with the four `inbox-*` kinds in its `kind` check, as migrations 3 and 0009 did (copy with ids kept, drop, rename, recreate its indexes and triggers). Every other row is unchanged. Its `kind` check lists the core kinds only; the pull request kinds come with migration 0013.
- **Migration 0013** (file `0013-pr-inbox`; the PR poll's Inbox kinds, hs-d165, D1 and D4 of Plan inbox; applied after migration 0012; the next free number on its base, renumbered at rebase when another migration lands first, hs-d68, hs-d211): adds two columns to `pull_requests`, then drops and recreates `pull_requests_hist_ins`, `pull_requests_hist_upd` and `pull_requests_hist_del` so they list every column (the rule above), and rebuilds `inbox_items` with `pr-failing`, `pr-changes`, `pr-ready` and `review-requested` in its `kind` check (copy with ids kept, drop, rename, recreate its index, `inbox_items_no_delete` and history triggers; SQLite cannot alter a check):

  ```sql
  ALTER TABLE pull_requests ADD COLUMN authored INTEGER NOT NULL DEFAULT 1 CHECK (authored IN (0, 1));
  ALTER TABLE pull_requests ADD COLUMN review_requested INTEGER NOT NULL DEFAULT 0 CHECK (review_requested IN (0, 1));
  ```

  Every existing row came from the `author:@me` search, so it gets `authored` 1 and `review_requested` 0, which the defaults give. `pull_requests_owned` is unchanged and `pull_requests` still gets no `_no_delete`. Set `user_version = 13`.
- **Migration 0014** (file `0014-register`; stub drafts, hs-d241, R1; applied after migration 0013; the next free number on its base, renumbered at rebase when another migration lands first, hs-d68, hs-d211): makes `jobs.tier`, `goal`, `context`, `touches`, `acceptance` and `out_of_scope` nullable **only while `status` is `draft` or `cancelled`** (a stub is cancellable). SQLite cannot drop a `NOT NULL` or alter a check, so `jobs` is rebuilt as migration 4 rebuilt it (foreign-key handling of its steps 1 and 5; `jobs_new` with the §4.4 definition; copy with every primary key and column kept, `jobs.seq` included; `DROP`, `RENAME`; indexes, triggers (`jobs_locked`, `jobs_status`, `jobs_pr_dependencies`, and the others of §4.4) and history triggers generated again), in one transaction. The new definition adds the checks `status IN ('draft', 'cancelled') OR (tier IS NOT NULL AND goal IS NOT NULL AND context IS NOT NULL AND touches IS NOT NULL AND acceptance IS NOT NULL AND out_of_scope IS NOT NULL)`, so a Job that is queued, running, merged or failed has every field, and `(goal IS NULL) = (context IS NULL) = (touches IS NULL) = (acceptance IS NULL) = (out_of_scope IS NULL)`, so the Spec is all or nothing; each field's own check accepts null. Every existing row has every field, so no row changes; `user_version = 14`. The tests: a database at 0013 with Jobs of every status migrates keeping every row and `seq`, passes `PRAGMA foreign_key_check`, accepts a `draft` with only a title and refuses a `queued` Job with a null `tier` or `goal`, and its schema hash equals that of a fresh database migrated to 0014.
- **`kind-tables`**, the first named migration (`<timestamp>-kind-tables`, `-- cpd-migration: breaking after 14`: the fourteen legacy names sort before it; the timestamp is its author's, and a test asserts that no named migration sorts before it). It creates the lookup tables and `history_tables` of §4.4 and fills them with the inserts shown there, then rebuilds `events` and `context_records` **once more, and the last time either is rebuilt for a kind**: `CREATE TABLE <t>_new` with the §4.4 definition (`kind` a foreign key instead of the `CHECK`; every other column and check of `context_records` unchanged), `INSERT INTO <t>_new SELECT …` every row with its primary key (`events.id` and the `AUTOINCREMENT` sequence kept, as `0003`), `DROP`, `RENAME`, recreate the indexes (`events_plan`, `events_context`, `events_session`) and the hand-written triggers (`events_own` after the copy, `events_no_update`, `events_no_delete`, `context_records_immutable`). It creates no history trigger: the reconcile (K4) generates them for every table of `history_tables`, the three lookup tables included. The copies write no `history` rows. Tests: after the conversion `events` and `context_records` have every row and id; an `events` insert with a kind that is not in `event_kinds` fails on the foreign key; an additive migration that inserts a kind makes it insertable.
- No migration from cpd 1's beads Ledgers (R7): running Workflows finish on cpd 1.

### 4.6 The Context store

Since Plan manager B1 (hs-d23, hs-a1, hs-d128; migration 0007, §4.5) the database tables `contexts` and `context_records` (§4.4) are the only store of Context records. ADR-0012's git store, its index and its sync are superseded and gone.

- **Contexts are machine-local** (hs-a1): a Context and its records exist on the machine that wrote them. Nothing copies them to another machine, so there is no sync, no remote, no conflict and no lock file.
- **One record per row**, the fields of §2.2 "Context record". A record's id is `<machine>-<letter><n>` (§2.1); existing ids never change.
- **A write** (`context decide`, `note`, `ask`, `answer`, and `plan new`'s `plan` record) is one database transaction: validate (`supersedes` names an existing decision, `question` an existing question; else `record_not_found`, exit 3), insert the `context_records` row, append `context-record`. `context new` writes only its `contexts` row and appends `context-created`. Rendering is on demand (`context show`); no rendered file is stored, except the Context digest, a derived file (below).
- **Positions are machine-local**: `context_records.seq` belongs to this database.
- **Old stores.** A `$CPD_HOME/contexts/<id>/` folder of an earlier version is imported and archived by migration 0007 (§4.5, §1.4). Nothing reads `archive/contexts/` afterwards.
- **Record kinds are the rows of `record_kinds`** (§4.4, §4.5): a kind exists when its row does. Every binary parses a record's kind into a closed type, so a migration that inserts a record kind is declared `breaking` (§4.5: a new value that old code parses into a closed type; hs-d178 D1), unlike an Event kind.

**Records in force** (hs-d194, D2). Computed by cpd, deterministically, from the database alone: every record of kind `decision`, `note`, `question` (open or answered) or `answer` that no `decision` of the same Context names in its `supersedes`. `plan` and `brief` records are never in force (a Lead gets its own brief in full, §3.1; the others are listed short, §2.2). Computed **as of a `seq`**, it considers only the records whose `seq` is at most that one. A superseded record stays queryable (`context show --record <id>`) but is never in a digest. Lives in `core/src/context/inforce.rs`.

**The Context digest** (hs-d182, hs-d192, hs-d194; D1, D3). A compressed, derived rendering of a Context's records in force, written by a pi run (the distiller, below) so that first messages stay small (hs-n111). It is **never a Context record**: the records stay the source of truth, and a digest can always be rebuilt from them (hs-d84). It lives in `core/src/context/digest.rs`.
- **Files**: `run/context-digests/<context>/<upto_seq>.md` (§1.4), every path through `CpdHome`, never built by hand; `<upto_seq>` is the highest `seq` of the Context's records (of any kind) when the run read them, the last record the digest covers. The **latest digest** is the file with the highest `<upto_seq>`. A file is renamed into place only after it passed validation, so every `<upto_seq>.md` is valid; after the rename the run removes the Context's other `*.md`, and every `*.md.tmp`, so the directory holds one digest. No table, no migration.
- **Validation**, deterministic, in this order (`distill_invalid` lists every failure at once):
  1. UTF-8, and at most **32 768 bytes** (32 KB);
  2. **citations**: a citation is a square-bracketed group whose content is one or more record ids (§2.1) separated by commas and optional spaces, `[hs-d43]` or `[hs-d43, hs-n7]` (one statement may cite several); any other bracketed text (a markdown link) cites nothing. Every record in force as of `<upto_seq>` must be cited at least once (else it is **missing**), and every id cited must be such a record (else it is **stray**: a superseded record, a `plan` or `brief` record, or an id the Context does not have).
- **A digest that fails** is removed (its `.md.tmp`), the previous latest digest stays, the command fails with `distill_invalid` (exit 1) and appends **one** `error` Event: `source` `context distill`, `code` `distill_invalid`, `details: {context, upto_seq, bytes, limit: 32768, missing: [id], stray: [id], log}`. The Event sets no `context`, `plan` or `session` (the Context is in `details`): §5.3 delivers an Event with a Context and no Plan to every Lead of the Context, with a turn, and a digest concerns none of them. It is read in the cockpit's `logs` view and with `events list`.

**The distill run, `cpd context distill`** (D4, D5; `commands.md` §3.12). It lives in `core/src/context/distill.rs`. Started on demand, or detached by engine E9 (§3.7, debounced: the newest record at least 10 minutes old). Steps:
1. Take `locks/distill-<context>.lock` without waiting (held: `distill_running`, exit 3), so a Context never has two runs at once; the kernel frees it when the run dies, so a crashed run leaves no stale lock (§1.4).
2. One read transaction: the Context's highest `seq` (`upto_seq`) and its records in force as of it. Nothing to do when the latest digest's `upto_seq` is at least `upto_seq` (`"already": true`), or no record is in force (`"already": true`, no digest). When no record in force has a `seq` above the latest digest's `upto_seq` (only `plan` and `brief` records came since), the records in force are unchanged, so the run **carries the latest digest forward** without pi: it writes its text as `<upto_seq>.md.tmp` and continues at step 5.
3. Write `prompt.md`, the distiller's prompt, embedded in the binary from `core/src/context/distill_prompt.md` (versioned with the code that validates it; no config), and `records.md`, the records in force oldest first, one block each: `[<id>] <kind> (<date>)`, `answers <question id>` for an answer, then its full `text`. A superseding decision's `supersedes` is not shown, since the record it names is not in force.
4. Run the distiller (§6.5 "Distiller"), one-shot pi in print mode at Tier `context_distill_tier` (§8.2, default `S2`), in `run/context-digests/<context>/`, stdout to `<upto_seq>.md.tmp`, stderr to the log. The prompt tells it to cite every record of `records.md` in square brackets, to cite nothing else, and to stay under 32 KB. pi exiting non-zero (an error, or an answer cut at the output limit, docs: pi `cli-integration.md`), failing to start, or running longer than `timeouts.distill_minutes` (§8.2; the run then kills it) fails the run with `distill_failed` (exit 1) and one `error` Event like `distill_invalid`'s, `details: {context, upto_seq, reason: "exit" | "spawn" | "timeout", exit?, tail, log}`, `tail` the last 60 lines of pi's stderr; the `.md.tmp` is removed.
5. Validate (above); when valid, rename `<upto_seq>.md.tmp` to `<upto_seq>.md` and remove the older files of the directory. No database write and no Event on success.

**Consumers** (D6). One function (in `core/src/context/digest.rs`) renders "the Context" for a first message: with a digest, the latest digest's text, then the records written after its `upto_seq` (`seq` greater), oldest first, **at most 20**, rendered as `context show --since <upto_seq>` renders them (briefs short), then, when more exist, `and <K> more: cpd context show --context <c> --since <seq>` with `<seq>` the `seq` before the first record not listed; with no digest yet, `context show` as markdown (every record), the behaviour before the digest. `plan new` uses it for its Lead's first message (§3.1; it always starts a fresh Lead, the grounded Lead and its forks are retired) and `context manager` for its Manager's (§3.1). Turn-start Context updates stay raw records (hs-d147, `commands.md` §15.7). `context show --digest` prints the latest digest (`commands.md` §3.3).

**Nothing is lost** (hs-d192, D7). `docs/research/unplanned.md` lists the decided work no Plan holds. Each of its items, the MISSING items and the rows of "Owned by a record only", names its records (the ids in the item's parentheses, or its Records column). The Job that builds the distiller proves two things:
- a binary-driven test reads those ids from the file and builds a Context in which each is in force (machine `hs`); a fake pi's digest that cites all of them is accepted, and one that leaves out any of them is refused `distill_invalid` with that id in `details.missing`;
- on the User's `building-cpd` Context, `cpd context distill --context building-cpd` produces a valid digest, which therefore cites every one of those records that is still in force; one that is superseded since the audit is reported by the Job, not cited.

## 5. Events

### 5.1 Rules

- **Every state change appends at least one Event, in the same transaction** (§4.2). The tables of §3 name the kind for each transition. Rows written only to record data (metrics, a `pull_requests` row that changes no PR's state) append no Event of their own; they belong to the transition that wrote them.
- Events are the single channel (D4): to Leads (through the pi adapter), to the TUI, and for agent states. Nothing else carries messages: no queue directories (cpd 1 §10), no tmux user options for state (spike), no cache directories (TUI spike).
- `text` is written for the Lead and ends with the `cpd` command(s) it may run, for every kind delivered as a message; for silent kinds it is a one-line description.

### 5.2 Kinds

Common fields on every Event: `id`, `at`, `kind`, `context`, `plan`, `job`, `attempt`, `session`, `text`, `data`. The table lists which of `plan`/`job`/`attempt`/`session` are set and the fields of `data`.

**A kind exists when its row is in `event_kinds`** (§4.4). **Adding a kind is one additive migration**, `INSERT INTO event_kinds (name) VALUES ('<kind>');` (§4.5; D1), plus the row of the table below, the `Kind` enum of `core/src/events/kind.rs` and `pi/lib/event.ts`, and **`PROTOCOL` raised** (below). Old binaries read an Event's kind as a string, so the migration is additive; they do not know the kind's code and the adapter ignores a kind its table in §5.3 ignores. The table below lists the kinds that exist; the `kind-tables` migration's insert (§4.4) is the set at its creation, and each later kind has its own migration.

| Kind | Appended when | Set | `data` |
|---|---|---|---|
| `context-created` | `context new` | context | `{title, jira, created_on}` |
| `context-record` | a record is written (§4.6) | context | `{record, kind, machine, seq}` |
| `context-grounded` | **no longer appended** (retired by migration 0010, hs-d129; kept for history) | context, session | `{seq, forked_from}` |
| `plan-created` | `plan new` | plan | `{title, jira, lead_session, forked_from, cwd}` (`forked_from` is always null since Plan manager B3: `plan new` starts a fresh Lead, §3.1); R34: no `repo` or `base` (A12), `cwd` is the directory `plan new` ran from |
| `plan-phase` | `grounding → plan`, `plan → dispatch` (also moves every draft Job to `queued`), `dispatch → deploy` (when the last PR not yet open opens, A10), `deploy → dispatch` and `done → dispatch` (`pr add` appends a PR to a Plan that was past `dispatch`, D1; appended before `pr-added`) | plan | `{from, to}` (`from` is `deploy` or `done` and `to` is `dispatch` when `pr add` reopens the Plan); at lock also `jobs: [key]` and `prs: [key]` (the PRs it started); at `deploy` also `prs: [{key, number, url}]` (every PR not `cancelled`) |
| `plan-review` | a Plan review is requested or decided | plan | `{review, fingerprint, status, feedback?, jobs?}`; `jobs: [key]` only when an approval in a phase after `plan` queued the drafts (§3.2): the Jobs it queued |
| `plan-done` | `deploy → done`, when the last PR not `cancelled` is merged; appended each time the Plan reaches `done`, so again after a reopen (D2; cpd 1's `workflow-done`) | plan | `{prs: [{key, number, url, merged_at}]}` |
| `job-added` | `job add` | plan, job | `{pr, status, appended, tier, needs, review, urgent}`; `status` is `draft` for a draft added in a later phase too, with `appended` true |
| `job-edited` | `job edit`; `job urgent` | plan, job | `{fields: [name]}`; `job urgent` appends `{fields: ["urgent"]}` (no new Event kind) |
| `job-merged` | Job → `merged` | plan, job, attempt | `{sha, tier, cancel_too_late?: true}` |
| `job-failed` | Job → `failed` | plan, job, attempt? | `{cause, detail, tier, attempts}` |
| `job-retried` | `job retry` (`failed → queued`) | plan, job | `{tier}` |
| `job-cancel-requested` | `job cancel` with an active Attempt | plan, job, attempt | `{reason}` |
| `job-cancelled` | Job → `cancelled` | plan, job, attempt? | `{reason}` |
| `attempt-started` | Attempt inserted (`queued → running`) | plan, job, attempt | `{number, tier, origin}` |
| `attempt-status` | an Attempt status change not covered by another kind | plan, job, attempt | `{from, to}` |
| `attempt-retry` | Attempt ended with a ladder cause and the Job went back to `queued` | plan, job, attempt | `{outcome, cause, next_tier, feedback?}` |
| `attempt-nudge` | A5 (the Worker ended its turn without submitting) | plan, job, attempt, session | `{prompt, turns}` (`turns` = the Worker's assistant-turn count when nudged) |
| `all-merged` | E4, once per PR completion: again after `job add` appended a Job to a PR whose `all-merged` was sent (§3.7 E4) | plan | `{pr, merged: [key], cancelled: [key]}`; `pr` is the PR's key, the lists its Jobs |
| `pr-feedback` | the poll (§7.6) wrote a change to the pull request of a PR of this Plan (hs-d96, D7), when any of: its checks became `failed` (from anything else), its review became `changes_requested` (from anything else), its state became `closed` (from `open`), its `mergeable` became `conflicting` (from anything else: `mergeable`, `unknown` or no row before). Written in the transaction that writes the change; only the Plan whose PR points to the pull request gets it, and a pull request no Plan owns gets no Event | plan | `{pr, number, url, state, checks, review_decision, mergeable, merge_state, conflict, reasons}`; `pr` is the PR's key, `state`, `checks`, `review_decision`, `mergeable` and `merge_state` are the row's, `conflict` is true exactly when `conflict` is in `reasons`, `reasons` is a subset of `["checks_failed", "changes_requested", "closed", "conflict"]`, in that order. Once per transition, not every poll: `unknown` is no conflict, and a conflict seen again after `unknown` (`conflicting` → `unknown` → `conflicting`) is a new transition and wakes the Lead again, as a re-failed check does. `text`: `PR <key> (#<n>) of Plan <p> needs attention: <reasons>. Read the details with cpd pr view --plan <p> --pr <key>.`, the reasons comma-separated as `checks failed`, `changes requested`, `closed without a merge`, `merge conflict` |
| `pr-added` | `pr add` (R34) | plan | `{pr, repo, base, needs: [key], appended}`; `appended` is true in phases `dispatch`, `deploy` and `done` |
| `pr-edited` | `pr edit` | plan | `{pr, fields: [name]}` |
| `pr-cancelled` | `pr cancel` | plan | `{pr}` |
| `pr-started` | a PR's start: `plan lock`, engine E7 or `pr start` (§3.8) | plan | `{pr, repo, base, lead_branch, lead_worktree, base_sha, reused, pushed, by}`; `reused` and `pushed` as R35 (§7.2), `by` is `lock`, `engine` or `pr start` |
| `pr-opened` | `pr open` (`dispatch → open`) | plan | `{pr, number, url}` |
| `pr-merged` | `pr poll` (`open → merged`: the poll sees the pull request `merged`, D7) | plan | `{pr, number, url, merged_at}` |
| `escalation` | `ask` | plan, job, attempt | `{escalation, question}` |
| `escalation-answered` | `answer` | plan, job, attempt | `{escalation}` |
| `escalation-expired` | timeout or dead asker | plan, job, attempt | `{escalation, reason}` |
| `session-registered` | a Session row is created | context, plan?, session | `{role, name, forked_from, tier}` |
| `session-state` | a Session's state or label changes | context, plan?, session | `{from, to, label, reply?}`; `reply` is true only on a transition to `idle` that ended a reply run (§3.6, hs-d278), false or absent otherwise |
| `session-moved` | the Session's pane is created, moved into the chat slot, parked or gone (§6.4); gone is appended by E2 or by `session close`, with `pane` and `window` null (§3.6, §6.4) | context, plan?, session | `{pane, window, place}`, `place` is `chat`, `parked`, `work` or `none` |
| `session-renamed` | `session rename` (cpd 1's `rename`, User decision 7 in the vision) | context, plan?, session | `{name}` |
| `context-renamed` | `context rename` | context | `{title}` |
| `plan-renamed` | `plan rename` | context, plan | `{title}` |
| `context-archived` | `context archive` or `unarchive`, for the Context | context | `{archived: bool}` |
| `plan-archived` | `plan archive` or `unarchive`, or the cascade of the Context's, for each Plan | context, plan | `{archived: bool}` |
| `session-archived` | `session archive` or `unarchive`, or a cascade, for each Session | context, plan?, session | `{archived: bool}` |
| `session-message` | `session send` (commands.md §10.13): one message to a Lead or a Manager | context, plan?, session = the target | `{from, text, wake}`; `from` is the sender Session's id or null (the User); `wake` is a boolean, true only for `session send --wake` (hs-d209) |
| `inbox-opened` | E8 inserts an Inbox item (§3.7) | context?, plan?, job?, session? | `{item, kind, urgency, label}` |
| `inbox-closed` | E8 closes an Inbox item | context?, plan?, job?, session? | `{item, kind}` |
| `inbox-snoozed` | `inbox snooze` | context?, plan?, job?, session? | `{item, until}` |
| `inbox-dismissed` | `inbox dismiss` | context?, plan?, job?, session? | `{item}` |
| `watch-started`, `watch-stopped` | the engine starts or stops | (none) | `{pid}` |
| `error` | `watch` or a driver hit an unexpected failure; codes include `lead_worktree_stale`, `merge_check_failed`, `push_failed` (a failed push of a Lead Branch at a PR's start or at a merge, §7.2, §7.4; `details.pr` names the PR) `cleanup_failed` (`pr poll` kept a merged PR's Lead worktree, or a removal failed, §7.2; `details: {plan, pr, path, reason}`, `reason` `dirty`, `unpushed` or git's stderr tail) `pr_start_failed` (engine E7 could not start a PR, §3.8; `details: {pr, step, tail?, log?}`) and `adapter_missing` (the `session run` wrapper restarted a pi whose adapter reported `stopped` for more than 60 s, §6.7 "Adapter watchdog"; `details: {session, state_since, seconds}`) and `gh_rate_limited` (engine E5 skips polls while GitHub's rate limit is nearly used, §3.7; `details: {remaining, reset_at}`; no Plan, it concerns the User's account) and `distill_invalid`, `distill_failed` (source `context distill`: a Context digest failed validation, or its pi run failed, §4.6; `details.context` names the Context, and the Event sets no `context`, so no Lead gets a turn) | context?, plan?, … | `{source, code, message, details}` |

**The protocol number** (hs-a4). `cpd --version` reports `protocol`, an integer that starts at **1** and is raised by one whenever an Event kind, a field of an Event's `data` that the pi adapter reads, or the output of a command the adapter runs changes in a way the adapter depends on. **Today it is 4** (1 until `session-message`, 2 until the four `inbox-*` kinds, 3 until `session state idle --reply`, hs-d278: the adapter runs a command with a new flag). So **adding a kind, or a `data` field the adapter reads, raises `protocol`**; so does removing or renaming one. An addition the adapter ignores (a kind its table in §5.3 ignores, a `data` field it does not read) does not. The adapter holds the number it was built for and checks it at start-up and at every restart of its follower (commands.md §15.2); a mismatch is reported once per installed binary, with the code `protocol_mismatch` (§1.2).

### 5.3 Delivery by the pi adapter

The pi adapter (TypeScript in `pi/`, loaded in every pi session cpd starts, §6.5) follows the Events **for its Session**:
- for the Session that is Plan P's `lead_session` (Context C): Events with `plan = P`; Events with `plan IS NULL AND context = C`; Events with `session` = itself. A Plan has one Lead for all its PRs (R34): every PR Event (`pr-*`, `all-merged`, `pr-feedback`), and an `error` about a PR such as `pr_start_failed`, sets `plan = P` and goes to that Lead;
- for any other Session (a Worker, a side fork of a Lead, a Manager): Events with `session` = itself (for a Worker only `attempt-nudge` and `session-renamed` are ever addressed so). A Manager (`plan` null) therefore follows only Events addressed to it, `session-message` (`cpd session send`, hs-d86), and is never woken by the Events of its Plans. Its Plan Events reach it only as the turn-start digest, which the adapter builds, for a Manager only (hs-d209, D1), when the User talks to it: after the Context records, a section `Plans:` with one line per Event since the cursor of kinds `plan-created`, `plan-phase`, `pr-opened`, `pr-merged`, `job-failed`, `escalation` and `plan-done` of the Context (`- <at> <plan>: <event text>`), at most 20, then `and <K> more: cpd events list --context <c> --since <id>`; one cursor (Event ids) serves both sections, and nothing is added when nothing is new (B3-A5; `commands.md` §15.7). Turn-start updates show a `brief` record short (§2.2). **A Lead gets no turn-start update** (hs-d209, D1, D2): nothing from the Context is pushed into a Lead's turn; it reads the Context on demand with `cpd context show --since <seq>`. `cpd session send` starts no turn unless `--wake` (D3).

It delivers a message with `pi.sendMessage({customType: "cpd", content: text, display: true, details: event}, {triggerTurn, deliverAs: "followUp"})` (cpd 1 §15, kept). It never types into a pane.

| Kind | Lead | Worker |
|---|---|---|
| `job-merged` | message, **turn** | |
| `attempt-retry` | message, no turn | |
| `job-failed` | message, **turn** | |
| `escalation` | message, **turn**; opens a blocking reason (`cpd: Worker question`) | |
| `escalation-answered`, `escalation-expired` | no message; closes that blocking reason | |
| `all-merged` | message, **turn** | |
| `pr-feedback` | message, **turn** | |
| `pr-started` | message, no turn (R34: the PR's Jobs are now scheduled) | |
| `pr-merged` | message, no turn | |
| `plan-done` | message, **turn** | |
| `plan-review` with `status: pending` | opens plannotator in the Plan Lead's adapter; no message | |
| `error` | message, **turn**; this includes `pr_start_failed`, which the Lead answers with `cpd pr start <key>` (A9) | |
| `job-cancelled` | message, no turn (it may come from the engine, after a request) | |
| `session-renamed` (own session) | no message, no turn: `pi.setSessionName(name)` and a one-line notice (cpd 1 §10, kept) | same |
| `attempt-nudge` (own session) | | message, **turn** (replaces cpd 1's `herdr agent prompt`) |
| `session-message` (own session) | message (`Message from <sender>: <text>`), for every Lead and a Manager; **turn only when `data.wake`** (hs-d209, D3): then an idle Session starts a turn at once, a working or blocked one gets it after its running turn; without `wake` the message waits in the Session's context for its next turn and starts none | |
| `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived` | ignored: no message, no turn (they are for the TUI) | ignored |
| `inbox-opened`, `inbox-closed`, `inbox-snoozed`, `inbox-dismissed` | ignored: delivered to no Session, they are for the views that follow Events (the adapter only learns the kinds) | ignored |
| every other kind | ignored | ignored |

The turn column is cpd 1 §10's table, kept; the renamed kinds are `workflow-done` → `plan-done` and `rename` → `session-renamed`.

**No loss, no duplicates.** The adapter's cursor is the highest Event id it has delivered, which it reads back at `session_start` from its own session file (the `details.id` of the last `cpd` custom message whose `details.delivered_to` is this Session), else the `id` of the Session's `session-registered` Event, whichever is greater. The adapter adds `delivered_to` (the Session's id) to the `details` of every message it delivers, so the cursor counts only messages delivered to this Session and never one at or below its `session-registered` Event: a fork's session file starts with its source's messages, whose Event ids belong to the source's stream. It runs `cpd events follow --session <id> --since <cursor>` and restarts it with the latest cursor if it exits. The exact protocol (activation, errors, the Plan review through plannotator) is in `commands.md`.

**Reload after an install** (hs-d141, hs-d49). After an install the adapter reloads its own pi by itself (`ctx.reload()`), when its guard passes (idle, nothing pending, own state `idle` and not `blocked`, nothing typed in the editor); it detects the install by the identity of its own directory, runs its own command, and retries a refused reload at the next poll. Nothing reaches the model and nothing is typed into a pane. Before it triggers, it proves that the new package loads (the identity is stable on two polls, the entry files exist, a throwaway load in a child process succeeds); otherwise it does not reload, shows one error and retries at a later poll. Detection, pre-validation, command, guard and retries: commands.md §15.6.

### 5.4 `cpd events follow`

- **Selection:** `--plan <id>`, `--context <id>`, `--session <id>` (the rule of §5.3: the Plan's `lead_session` follows its Plan, its Context and itself; any other Session only Events addressed to itself) or none (every Event); `--kind <k>` may repeat.
- **Since:** `--since <id>` is exclusive: the first line printed is the first matching Event with an id greater than it. Without `--since`, it starts after the current highest id (only new Events). `--since 0` replays everything.
- **Order:** strictly increasing `id`, which is commit order (§4.2). Each line is one Event as JSON (the fields of §5.2, `data` as an object).
- **Mechanism: polling, no notification channel.** Every `events_poll_ms` the follower reads `PRAGMA data_version` together with `PRAGMA schema_version`; when `schema_version` has changed since the last read, it recomputes the store verdict (§4.5) and, when the store no longer opens, ends with the verdict's code (below); when `data_version` has changed since the last read, it runs `SELECT … FROM events WHERE id > :cursor AND <filter> ORDER BY id LIMIT 500` in a fresh read transaction, prints each row, flushes, and advances the cursor. It repeats the query at once while it returns a full batch. `data_version` costs no I/O when nothing has changed, so an idle follower is cheap (docs: SQLite `PRAGMA data_version`).
- **Back-pressure:** writes to stdout are blocking. A slow reader slows the follower, which reads the next batch only after the previous one is written; nothing is buffered beyond one batch and nothing is dropped, because the database is the buffer. A reader that falls behind or restarts resumes with `--since <last id it processed>`.
- **End:** it runs until killed or until stdout closes (EPIPE), and then exits 0. With `--until-idle` it exits 0 once it has printed everything committed when it started (for scripts and tests).
- **Store errors end it** (hs-d140): on any store error (`store_too_new` after an upgrade included, which now comes from the verdict of §4.5 and means a breaking migration this binary does not know, and `migration_changed`, `migration_order`, `store_busy`, `store_failed`, …) it prints the failure value (§1.2) as its last line and exits with that code's exit; it never stays alive on a database it cannot read. The pi adapter restarts a follower that ends (§5.3), so the restart meets the new binary or reports the error again.
- **Re-exec on a new install** (hs-d140; hs-d55): between polls it checks its own executable path with the `(device, inode, mtime)` check of `core/src/host/exe.rs`, as `session run` and `attempt run` do (§3.4, §6.7); when it changed, it re-execs itself with the same argv except `--since`, which is the id of the last Event it printed (when it has printed none: the cursor it started from, its `--since`, or the highest id at its start), so no follower outlives an install and none loses or repeats an Event. The check runs in every mode.
- `cpd events list` takes the same selection and prints one JSON value `{ok, events: [...], next: <id>}`, at most `--limit` (default 500) Events.

## 6. The tmux host

### 6.1 The server

- One tmux server per machine: `tmux -L <tmux.socket>`, default socket name **`cpd`**. cpd starts it when it is not running, with `-f /dev/null` (the User's own tmux config is not read, so behaviour is the same everywhere) and then sets the options below. Every tmux call cpd makes passes `-L <socket>` and runs with `TMUX` and `TMUX_PANE` removed from its environment.
- Global options cpd sets (from the spike's `tmux.conf`, verified in daily use with tmux 3.4):

| Option | Value | Why |
|---|---|---|
| `mouse` | `on` | clicks in the ratatui panes |
| `default-terminal` | `tmux-256color` | |
| `extended-keys` | `on` | pi's keys (verified by the User) |
| `terminal-features` | append `xterm*:extkeys,xterm-ghostty:extkeys,xterm-ghostty:hyperlinks` | `hyperlinks`: inside tmux, pi emits clickable OSC 8 links only when the client lists it (pi-tui `terminal-image.js`), and tmux forwards them only then |
| `escape-time` | `10` | |
| `status` | `off` | the cockpit draws its own bars |
| `pane-border-status` | `top` | |
| `pane-border-lines` | `heavy` | |
| `remain-on-exit` | `on` | a dead pane stays, so cpd can read `#{pane_dead_status}` |
| `focus-events` | `on` | the views are told when their pane gains or loses focus |
| `pane-active-border-style` | `fg=blue` | the focused pane's border |
| `pane-border-format` | the format below | the column titles |

**Every server start sets every option, key binding and hook of this section**, whichever command starts the server (`plan new`, `watch --ensure`, `tui`), so the cockpit looks and behaves the same whatever started it. Rerunning is safe (each is a plain `set -g`, `bind` or `set-hook -g`), so `cpd tui` also applies them to a server that is already running. Cockpit parity (User, 2026-10-15): the cockpit matches the running cpd 1 cockpit, whose `tui/scripts/tmux.conf` these are, renamed (`@cpd_view`, `@cpd_job`).

**The pane border format** (`pane-border-format`) titles a pane by its pane options, first match:

| Pane | Title |
|---|---|
| has `@cpd_session` (a Lead) | `#[reverse] #{@cpd_title} #[noreverse]` when `@cpd_title` is set (the Plan's title for a Plan's Lead, the Session's name for a side fork), else none |
| `@cpd_view = chat` (the placeholder), `@cpd_view = history` (an archived Session's history) or `@cpd_view = statusbar` | none: pi and the placeholder say what they are |
| `@cpd_view = tree` | ` Context ` |
| `@cpd_view = plan` | ` Plan ` |
| `@cpd_view = widgets` | ` Deploy ` |
| `@cpd_view = worker` | ` Worker · #{@cpd_job} (read-only) ` |
| any other | `#{pane_current_command}` |

Every title is drawn reversed: `#[reverse]` before it and `#[noreverse]` after it, inside the branch that has a title, so untitled panes get neither. It shows as a block in the border line's colour (blue on the focused pane, from `pane-active-border-style`; the default border colour elsewhere) with the text in the background colour, so the line reads as continuous, with a wider block where the title sits. Reason: User, Plan tui-visuals (hs-d105). Pane borders are the cockpit's top-level separators: white, the focused pane's blue (`pane-active-border-style`); separators inside a slot are grey (`commands.md` §13.2, design rule; hs-d124). The widgets title is ` Deploy ` (User, hs-d76, Plan tui-visuals).

**The click binding.** `bind -T root MouseDown1Pane if -F '#{==:#{@cpd_view},statusbar}' 'send-keys -M' 'select-pane -t = ; send-keys -M'`: a click on the StatusBar is passed to it and never selects it, so the chat keeps the keyboard focus; every other pane is selected, then gets the click.

**The `pane-died` hook.** `set-hook -g pane-died 'if -F "#{@cpd_view}" "respawn-pane"'`: only a pane that carries `@cpd_view` (a view of the cockpit) is respawned when its program dies. A Lead or engine pane stays dead, as `remain-on-exit` leaves it, because engine E2 must see its death (§3.6, §3.7). This differs from cpd 1's unconditional `set-hook -g pane-died "respawn-pane"`.

**Known tmux limitation (tmux < 3.6 built with libutempter, e.g. Ubuntu 24.04's 3.4).** Such a tmux can lose the `SIGCHLD` of a pane whose tty closes before the kernel signals it: on that EOF tmux calls `utempter_remove_record`, which sets `SIGCHLD` to `SIG_DFL` while it waits for its helper. The pane then shows dead, but tmux never reads its status, so `pane-died` does not fire and a dead view stays dead until the server's next `SIGCHLD` (any pane spawn, e.g. `cpd tui` respawning the dead panes of its layout). Fixed upstream in tmux 3.6 (commit fa5f3cef3d, tmux GitHub issue 4559).

### 6.2 Sessions, windows and panes

| tmux session | Window | Panes | What runs |
|---|---|---|---|
| `cpd` (the cockpit; `tmux.cockpit_session`) | `main` | the tree (left), the plan view (centre top, showing Pulse while a Manager is in the chat slot), the **chat slot** (centre bottom), the widgets (right); the **StatusBar**, one row across the full width at the bottom; optionally the read-only Worker view beside the chat | `cpd tui` views (the StatusBar runs `cpd tui view statusbar`); the chat slot holds one Lead Session's pane |
| `cpd` | `park-<label>` (one per Lead Session not in the chat slot) | one pane | a Lead (`cpd session run <id>`), cwd = the Session's `cwd`: for a Plan's Lead the directory `plan new` ran from, not a repo (R34, A12; §6.4) |
| `cpd` | `ctx-<context>` | one pane | a Context's Manager (`cpd session run <id>`), started by `cpd context manager` (§3.1) |
| `cpd-work` (hidden; `tmux.work_session`) | `watch` | one pane | `cpd watch` (D5) |
| `cpd-work` | `<plan>--<attempt>` (one per Attempt) | one pane, the Worker | `pi` |

- `<label>` is the Session's Plan id, or the first 8 characters of its id when it has no Plan. tmux targets are always pane ids (`%<n>`) or window ids (`@<n>`), never names; names are labels for people.
- **Pane options** identify panes, never state: `@cpd_session` (Session id) on every pane that runs a Session, `@cpd_title` on a Lead's pane (below), and `@cpd_view` (`tree`, `plan`, `widgets`, `statusbar`, `worker`, `chat`, `history`) on the TUI's panes. A Worker view pane (`@cpd_view worker`) also carries `@cpd_job` (the Job's key, shown in its border title) and `@cpd_attempt` (the Attempt id it shows). The Worker pane opens beside the chat slot with `split-window -h -d -l 50%` (no focus); a click on another Job retargets it with `respawn-pane -k` and new `@cpd_job` and `@cpd_attempt`; a click on the Job it shows closes it. Its size in reset-layout stays the reference's (`-x 30%`, commands.md §13.2). The chat slot is the pane of `cpd:main` that has `@cpd_session`, **or** `@cpd_view` `chat` or `history`. The value `chat` marks the **placeholder**: the pane running `cpd tui view chat`, text `No Lead here: pick a Plan in the session tree`, which holds the slot while no Lead is there. The value `history` marks a **history pane**: the pane running `cpd tui view history <id>`, an archived Session's file read-only, with `@cpd_history` set to that Session's id; it holds the slot like the placeholder and is never a Lead. At most one placeholder or history pane exists, and only ever in the slot. **`@cpd_title` is a deliberate exception** to "identify panes, never state": it is a label for people, the title shown in a Lead's pane header (the Plan's **title** for a Plan's Lead, the Session's **name** for a side fork). cpd keeps it current: it is set wherever cpd marks a Lead's pane with `@cpd_session` (`plan new`, `session fork` and the moves; not Worker panes), and `plan rename` and `session rename` set it again on the Session's pane when it has one. The border format (§6.1) shows it when set. Pane options move with the pane on `swap-pane`, which is why the chat slot is found by that rule and not by a fixed option.
- The User attaches with `tmux -L cpd attach -t cpd` (locally or `ssh -t <host> …`). Several clients may attach at once. The User never needs to look at `cpd-work`.

### 6.3 tmux commands cpd uses

| Purpose | Command (all with `-L <socket>`) |
|---|---|
| server and sessions | `has-session -t =<name>`; `new-session -d -s <name> -n <window> -x 215 -y 62 -c <dir> [command]` |
| the full-width StatusBar | `split-window -v -f -l 1` |
| reset-layout (§13.2) | `resize-pane -t <pane> -x\|-y <size>` |
| the StatusBar never takes focus (§6.1) | `bind -T root MouseDown1Pane if -F '#{==:#{@cpd_view},statusbar}' 'send-keys -M' 'select-pane -t = ; send-keys -M'` |
| options | `set -g <option> <value>`; `set -p -t <pane> @cpd_session <id>`; `set -p -t <pane> @cpd_view history` and `@cpd_history <id>` |
| a new parked window or Attempt window | `new-window -d -t =<session>: -n <name> -c <cwd> -e K=V … -P -F '#{window_id} #{pane_id}' -- <argv…>` (several arguments run without a shell, docs: tmux(1)) |
| bring a Lead into the chat slot | `swap-pane -d -s <parked pane> -t <chat slot pane>` (verified: both processes keep their pids, the slot keeps its size), then `rename-window -t <window id> park-<label>` for the window that now holds the displaced Lead |
| liveness | `list-panes -a -F '#{pane_id} #{window_id} #{session_name} #{pane_dead} #{pane_dead_status} #{pane_pid} #{@cpd_session}'` (plus `#{window_name}` and `#{@cpd_view}`); a pane whose `@cpd_view` this binary does not know is skipped and logged once, never an error (R31, finding 4). A failed `list-panes` means "the pane is gone" only when tmux says no server is running; any other tmux failure is retried once at the next poll before a pane is judged gone (engine-W8). On tmux >= 3.8 `#{pane_pid}` is empty once a pane's tty has closed (dead, or dying and not yet reaped); cpd identifies dead panes by `#{pane_dead}`, never by pid |
| a dead pane of the cockpit's layout | `respawn-pane` |
| retarget the Worker view to another Attempt; restart every live view (`cpd tui --respawn`, commands.md §13.1); put the history view in the chat slot's placeholder (`session history`, commands.md §10.12) | `respawn-pane -k -t <pane> -- <argv…>` |
| the Worker view beside the chat | `split-window -h -d -t <chat slot pane> -l 50% -P -F '#{pane_id}' -- <argv…>` |
| the conditional respawn of a dead view (§6.1) | `set-hook -g pane-died '<command>'` |
| the legend and `logs` popups (commands.md §13.2) | `display-popup -E -T <title> -w <width> -h <height> -- <argv…>`, at most the client's size |
| closing | `kill-pane -t <pane>`; `kill-window -t <window id>` |

**Deadline.** Every `tmux` call cpd runs has a deadline of 10 s (a local socket answers in milliseconds; a longer wait means a hung server), except `display-popup`, which runs until the User closes the popup. At the deadline the call fails with `tool_timeout` (§1.2), `details.tool` = `tmux`.

### 6.4 Where everything runs

| Who | Where | Started by |
|---|---|---|
| A Plan's Lead | the cockpit: a `park-<label>` window, or the chat slot of `cpd:main`; cwd = the directory `plan new` ran from, usually `~` (R34, A12): a Plan may span several repos, so its Lead starts in none of them | `plan new` (§3.1); `session open` resumes a `stopped` one |
| A Context's Manager | the cockpit: window `ctx-<context>` while not in the chat slot, else the chat slot; cwd = the directory `context manager` ran from, usually `~`. `context manager` never moves the chat slot (hs-d103); `session open` does | `context manager` (§3.1); `session open` resumes a `stopped` one |
| A Worker | `cpd-work`, window `<plan>--<attempt>`, its only pane, cwd = Attempt worktree | driver A3 |
| `cpd watch` | `cpd-work`, window `watch` | `plan lock`, `cpd tui` and `cpd watch --ensure`: each starts it when `locks/watch.lock` is free |
| Attempt drivers | detached processes (`setsid`), not in tmux; logs in `run/<plan>/logs/` | E3 |
| Distill runs (`context distill`) | detached processes (`setsid`) when the engine starts them, not in tmux; log `run/context-digests/<context>/distill.log`; on demand, in the caller's process | E9, or anyone on demand (§4.6) |

`session open <id>` brings a Lead into the chat slot; `session park <id>` moves the Lead in the chat slot back into a parked window: it creates a new `park-<label>` window holding a placeholder, then `swap-pane`s the Lead and the placeholder. When a Lead takes the slot, `session open` kills the placeholder or history pane it displaced, with its window. `session history <id>` parks a Lead in the slot first, then respawns the placeholder with the history view (commands.md §10.12). A Lead that quits pi with status 0 is archived by its own `session run` (commands.md §10.8). Each appends `session-moved` for every Session whose place changed.

`session close <id>` ends a Lead's pane: `kill-pane` (pi stops, and the Session's file stays, so `session open` resumes it later), then the Session is `stopped` with no pane, and `session-moved` `{pane: null, window: null, place: "none"}` is appended, the rows E2 writes for a pane that is gone (§3.6). It refuses the Session in the chat slot (commands.md §10.9).

### 6.5 How cpd starts pi

| Role | argv (after `pi`) | Environment added |
|---|---|---|
| Lead (fresh) | `--model <tiers[L]>:<level of lead_tier> [-e …] --session-id <id> -n <name> -- @<first message file>` | `CPD_HOME`, `CPD_SESSION=<id>`, `CPD_PLAN=<plan>`, `PLANNOTATOR_DATA_DIR=$CPD_HOME/run/<plan>/plannotator`, `PLANNOTATOR_UPDATE_CHECK=0`, `PLANNOTATOR_PORT=<the Plan's port>` (below), `PLANNOTATOR_REMOTE=1` and `PLANNOTATOR_URL_HOST=<the review host>` ("The review host" below), passed with `-e` so several Plan reviews can be open at once, `PI_SKIP_VERSION_CHECK=1` |
| Manager | as a fresh Lead (or, resumed, `--session <file>` and the `[-e …]` arguments, no first message). Model: `manager_tier` (§8.2). First message: `You are the Manager of Context <c> "<title>". Follow the cpd-manager skill.` then the Context as §4.6 "Consumers" renders it (its digest and the records after it, or every record) | `CPD_HOME`, `CPD_SESSION=<id>`, `PLANNOTATOR_UPDATE_CHECK=0`, `PI_SKIP_VERSION_CHECK=1`; no `CPD_PLAN` and no other `PLANNOTATOR_*`, so no port (it has no Plan). `session run` removes every `PLANNOTATOR_*` variable of its own environment (the tmux server's, the caller's) from pi's, so a Manager never inherits a Plan Lead's `PLANNOTATOR_DATA_DIR`. Name `<context>-manager` |
| Lead (resume) | `--session <file>` and the `[-e …]` arguments, no first message | same |
| A side fork with a Plan (`session fork --plan`) | `--fork <source file>` added to the Lead (fresh) row (§6.7) | a Lead's: it gets the Plan's port |
| Worker (review Job, B2) | `--fork <lead session file>` added to the Worker's argv below, with the same exclusions (`--approve --no-skills --exclude-tools …`), model, `--session-id`, `-n` and first message; started only after the §6.7 idle wait | same as a Worker |
| Worker | `--approve --no-skills --exclude-tools ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox [-e …] --model <model:level> --session-id <id> -n "<plan> <job> a<n>" -- "Read <spec path> and follow it exactly. Your attempt id is <attempt>."` | `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT`, `PI_SKIP_VERSION_CHECK=1` |
| Distiller (§4.6, D5) | `--print --no-session --no-extensions --no-skills --no-context-files --no-tools --model <tiers[L]>:<level of context_distill_tier> -- @prompt.md @records.md`; cwd `run/context-digests/<context>/`; stdout to `<upto_seq>.md.tmp`. No `-e`: it needs no adapter and no package. No tools and no context files, so its only input is the two files and it writes nothing itself (flags read in pi 1.3.0 `--help`; their effect on a model run is unverified) | `PI_SKIP_VERSION_CHECK=1` only: no `CPD_*` (it is no Session) |

A Lead's or a Manager's first message is passed as the absolute path of its file (`run/<plan>/lead-message.md`, `run/<plan>/messages/<session>.md` or, for a Manager, `run/<context>/manager-message.md`, §1.4) prefixed with `@`, never as text, because Linux limits one argument to 131072 bytes (MAX_ARG_STRLEN; hs-n111) and pi reads `@` files after `--` into the first user message (pi 1.3.0 `--help`: `pi [options] [--] [@files...] [messages...]`).

`PI_SKIP_VERSION_CHECK=1` is in every row (A1). It disables pi's request for the latest version and so its "Update Available" banner; dotfiles pins pi's version (hs-d134, hs-d144). cpd does not use `PI_OFFLINE`: it also stops model catalog refreshes.

- **cwd** (R34, A12): a Plan's Lead runs in the cwd of `plan new` (stored as its Session's `cwd`; a side fork by `session fork` takes its source's cwd); a Worker in its Attempt worktree, in the repo of its Job's PR. The project settings read below (`<cwd>/.pi/settings.json`) are therefore those of that directory.
- The excluded tools and `--approve` are kept from cpd 1 §12 step 5 and requirements §5.
- `<adapter>` is the `pi/` package installed beside the binary (§8 `pi_adapter`).
- **`[-e …]`, the extension arguments.** The candidates are `<adapter>` for every role, plus each `lead_pi_packages` entry for a Lead. cpd passes `-e <candidate>` only for a candidate that the pi settings do not already declare. It reads the `packages` of the global settings `<agent dir>/settings.json` (`PI_CODING_AGENT_DIR`, default `~/.pi/agent`) and of the project settings `<cwd>/.pi/settings.json`; an entry is a source string or an object with `source`. Candidates and entries are compared by package identity, the way pi identifies packages (docs: pi `packages.md`, "Understand scope and identity"):
  - an npm spec by its package name without the version (`npm:@plannotator/pi-extension@0.28.17` is `@plannotator/pi-extension`);
  - a git source by its repository URL without the ref;
  - a local path by its resolved absolute path, a relative path resolving from the directory of the settings file that declares it.

  Each skipped candidate is logged on stderr (for a driver, in its log), with the settings file that declares it. Why: passing an installed package again with `-e` loads a second copy, and pi refuses to start (`Tool "plannotator_mark_done" conflicts …`, the same for `plannotator_submit_plan`, `ask_user_question` and the flag `--plan`; verified 2026-10-13, `~/.cpd/plans/cpd-v1/jobs/lead-start.json`). The User's machines install the cpd Lead packages globally, so there both default `lead_pi_packages` are skipped. For `-e <adapter>`, the same conflict is inferred, not verified, when the globally declared adapter is a different path: `-e` of the identical path was seen not to conflict.
- **Tests and Gates** that start pi with explicit `-e` flags also pass `--no-extensions`, which disables discovered and configured extensions while explicit `-e` paths still load (docs: pi `cli.md`), so a machine's global install cannot break them. cpd's own launches of Leads and Workers do not pass it.
- The Session's `file` is found after start as `~/.pi/agent/sessions/*/*_<id>.jsonl` (the pattern `fork-lead.sh` uses, verified), honouring `PI_CODING_AGENT_SESSION_DIR` when set.

### 6.6 The engine's place (D5)

One `cpd watch` per machine, in `cpd-work:watch`. It is not tied to any Lead's pane (vision decision 6). Change from cpd 1: one engine instead of one per Workflow, so one concurrency limit covers every Plan and nothing has to be started per Plan.

**The Plan's plannotator port** (hs-d35, hs-d40, hs-d50). Config key `plannotator_ports` (§8.2) is an inclusive range. A Plan's port lives in `run/<plan>/plannotator-port` (one line, the port; no schema change). `session run` assigns it when it starts pi, under `flock` on `locks/plannotator-ports.lock`:

1. When the Plan's file holds a port inside the range, the Plan keeps it.
2. Else the Plan gets the lowest port of the range that no other Plan holds, and the file is written. A Plan **holds** its port unless its phase is `done` or it is archived (`archived_at` set; E1, hs-d203). An archived Plan's `plannotator-port` file stays, but its port is free: rule 1 keeps a Plan's port only while no other holder has it, else the Plan is assigned afresh by this rule. `plan archive` and `plan cancel` (commands.md §4.10, §4.12) therefore free the port, and `plan unarchive` takes none (the next `session run` assigns one).
3. Else none is free: `no_port` (exit 3; `details`: the range and the holders `[{plan, port}]`), the Lead does not start, and the pane names the Plans that hold the range and offers Enter to retry (§10.8 of `commands.md`).

**The review host** (User, 2026-10-19; hs-n228, hs-n229). Plan review links said `http://localhost:<port>` and worked from the Macs only through ssh `LocalForward`s that short-lived `ssh homeserver <cmd>` sessions grab and drop; direct LAN links work. So every Lead pane gets `PLANNOTATOR_REMOTE=1` (plannotator binds all interfaces and opens no browser; cpd no longer copies it from its own environment) and `PLANNOTATOR_URL_HOST=<the review host>` (plannotator's own setting: the host shown in the links it prints, display-only; the environment variable beats its config). One function in core decides the review host, and the cockpit's review link uses it too (`commands.md` §13.2):

1. `PLANNOTATOR_URL_HOST` in cpd's own environment, when set: an override (e.g. a Tailscale name), so there is one place to change it.
2. Else the machine's primary IPv4 address, detected when cpd starts the Lead's pi: the source address the OS would use toward an outside address (a UDP socket `connect`ed to `192.0.2.1:9`, then its local address; no packet is sent; standard library only, Linux and macOS).
3. Else `localhost`, with one stderr line, when detection fails or gives a loopback address.

No IP is written in any config file. `core/src/plan/lead.rs` keeps passing `PLANNOTATOR_PORT` from cpd's environment to a Lead pane; `session run` overrides it with the Plan's port when it starts pi (A7), and `cpd doctor` no longer checks it (`commands.md` §14.1).

### 6.7 `session fork`: fork when idle

`cpd session fork <source> [--plan <id>] [--name <n>] [--message-file <f>]` registers a new Session (`stopped`, `forked_from = source`), creates its parked window running `cpd session run <new id>`, appends `session-registered` and `session-moved`, and returns at once with the new id. The waiting happens in the new pane, not in the command. `cpd session run <id>` (internal) then:

1. **Waits until the source is idle:** its session file exists, **and** its Session state is `idle` or `stopped`, **and** the last `message` entry of its session file is an assistant message with `stopReason` `stop`, **and** the file has not been modified for `fork_quiet_seconds` (default 5). A source that is registered but has no session file yet (a Lead whose own `session run` has not started pi yet) is waited for, not refused. It checks every 2 seconds and shows `Waiting for the source Lead to finish its turn, then forking it as "<name>"…`. Rule from `fork-lead.sh` (verified 2026-10-14); the Session-state and file-exists conditions are new: the first only makes the check earlier, the second covers a source that has not started.
2. Runs `pi … --fork <source file> --session-id <id> -n <name> -- @<first message file>` (§6.5); without `--message-file` there is no first message and no `--`.
3. When pi exits: reports `stopped`, prints `pi exited. Press Enter to resume <name> here.`, and on Enter runs `pi --session <file>` again. If the session file already exists when `session run` starts, it resumes instead of forking (idempotent restart, as `fork-lead.sh`).

The fork never copies a half-done turn, and the source is never interrupted.

**Reload on install** (hs-d33, hs-d38, hs-d49, hs-d151; incident hs-n141, cause hs-n148). `session run` records the identity of its own executable as the driver does (§3.4 "Reload on install"). It also records the identity (device, inode, mtime) of pi's install when it starts pi: `<pi agent dir>/install/current-version` when that file exists (agent dir: `PI_CODING_AGENT_DIR`, else `~/.pi/agent`), else the resolved path of the `pi` executable. The two changes are handled differently:

- **A new cpd reloads the wrapper at once, and pi keeps running.** At every iteration of its loop, **before any store access**, the wrapper checks its executable path. A different identity means a new install, and the wrapper re-execs the new binary at once, with no idle wait, whatever the Lead's place or state (parked, in the chat slot, `working`, `blocked`). pi is the wrapper's child and a child survives its parent's `exec`, so pi is neither signalled nor restarted: the wrapper `exec`s the same path with the same argv plus the internal argument `--adopt <pid>` (commands.md §10.8), and the new `session run` **adopts** that pi: it `waitpid`s it as its own child and continues the same loop on it (the same watchdog, the same archive and `Press Enter to resume` paths when pi exits). It does not run steps 1–3 and starts no pi. The adapter reloads itself in pi at the Lead's idle (hs-a5, §5.3 "Reload after an install", commands.md §15.6); a cpd install never restarts a Lead's pi. Why: a wrapper that kept its old binary until the Lead's idle read the store (`record_file`, `reload_due`, the watchdog every 2 s) after a newer cpd had migrated it, failed `store_too_new`, and exited; tmux marked the pane dead, pi got `SIGHUP`, and six Leads, working and blocked ones among them, were ended instead of reloaded (hs-n141, hs-n148). An install reloads a Lead, never ends it.
- **A store error never ends a wrapper while its pi runs.** A failed store read with a replaced cpd leads to the reload above. Any other failed read is logged once on the stderr of the pane and retried at the next poll, with pi untouched.
- **A new pi restarts pi, at the Lead's idle** (hs-d49). A different pi identity is acted on only at a safe point, which is the Lead's next idle: Session state `idle`, the last turn finished (the last `message` entry is an assistant message with `stopReason` `stop`) and the session file quiet `fork_quiet_seconds`, the rule of step 1. A Lead that reaches it:
  - **parked:** restarts pi at once;
  - **in the chat slot:** waits until it is idle **and** parked, or idle for 10 minutes, so unsent typing in the pane is not lost;
  - **`blocked`** (a dialog, a pending Plan review): is never idle, so never restarts pi.

**Check before reload** (hs-d151): before it `exec`s, the wrapper runs `<path> --version` (the new binary, with its own `CPD_HOME`) and reloads only when that succeeds and `store` is `null` or `store.opens` is `true` (§3.4, §4.5; commands.md §1.2), so a binary that cannot open the store is never `exec`ed (hs-d151). Otherwise it does not `exec`: it logs once per installed file (identity) `reload skipped: <path> cannot open the store: <store.refusal.message>` (on the stderr of its pane) and keeps running its old code, pi untouched; a later install (another identity) is checked again. The same check applies to the `exec` after pi exits on its own.

Restart for a new pi: send pi `SIGTERM` (pi 1.2.0 runs `session_shutdown` on SIGTERM and SIGHUP; `dist/modes/interactive/interactive-mode.js`), wait for it to exit (if it has not exited 10 s after `SIGTERM`, send `SIGKILL`), then `exec` the same path with the same argv (no `--adopt`); the new `session run` resumes pi on the session file (step 3), so the new pi and the new adapter load and no turn is cut. If pi exits on its own after an install, the wrapper `exec`s the new binary before any store write (no pi to adopt). If pi exits on its own after a pi change, the resume on Enter already starts the new pi.

The adapter's own in-process reload comes first (§5.3 "Reload after an install", commands.md §15.6); the wrapper's pi restart for a new pi above is the **fallback** for what it cannot do: a pi without interactive UI, a reload refused for a long time, an adapter that fails to load.

**Adapter watchdog** (hs-d141). A pi that is alive with no adapter never reports again: after a failed load, or a reload in the window of an install, pi runs but the Session's state stays `stopped` (the adapter reports `stopped` at `session_shutdown` and `idle` when it starts again, commands.md §15.4). While pi runs, `session run` watches the Session's state; when it has been `stopped` for more than **60 s**, it restarts pi on its session file (`SIGTERM`, wait, `SIGKILL` after 10 s, then step 3's resume) and, before the restart, appends one `error` Event (`code: adapter_missing`, `details: {session, state_since, seconds}`, addressed as the Session's own Events, §5.3; §5.2 `error`). It ignores the idle rules of "Reload on install": a Lead with no adapter cannot be idle by the adapter's state and has nothing to lose. Details: commands.md §10.8.

### 6.8 Forks: why they happen and where they are drawn (B1, B4)

Forks happen for two reasons only, and neither nests:
- a **review Job** (B2): its Worker is a fork of the Plan's Lead, started by its driver after the §6.7 idle wait (§3.4). Workers are hidden in `cpd-work` and are not in the tree, so this fork is not drawn;
- `cpd session fork` (B4), kept for tangents the User starts (a bug, an idea): a Lead Session forked from a Lead.

Both are **siblings**: a forked Session is drawn in the tree **directly under its Context**, after the Context's Plans, a sibling of the Plans' Leads and of its source, and never under a Plan or under the Session it came from (a fork of a fork is drawn the same way). `forked_from` records the source and `plan` the Plan whose Events and `CPD_PLAN` it uses, but neither moves the row in the tree. (A Context's Manager and a Plan's own Lead are not `session fork` forks and keep their places, §3.1.)

*Decided where B1 is silent:* a forked Session's row in the tree is ordered by `created_at` after every Plan Lead row of its Context, and carries no Job counters (the Plan's are on the Plan Lead's row); `commands.md` §13.2 and §17 state it.

## 7. git and the Gate

cpd runs git itself (R4); there is no worktrunk. Every git call is `git -C <dir> …` with argv, `GIT_TERMINAL_PROMPT=0`, and stdin from `/dev/null`. Every git call also gets, in its environment, `GIT_CONFIG_COUNT=2`, `GIT_CONFIG_KEY_0=core.filesRefLockTimeout`, `GIT_CONFIG_VALUE_0=10000`, `GIT_CONFIG_KEY_1=core.packedRefsTimeout`, `GIT_CONFIG_VALUE_1=10000` (git-config(1)): a ref or `packed-refs` lock held by another git process is waited for up to 10 s instead of git's 100 ms (`filesRefLockTimeout`) and 1 s (`packedRefsTimeout`), because cpd has concurrent writers of shared refs such as `refs/remotes/origin/<base>` (hs-d243, G2). Hooks are spawned by cpd, not by git, and do not get it (§7.3).

Every `git fetch` cpd runs is retried while it fails with `bad object worktrees/<id>/HEAD` in its stderr, up to 20 rounds 100–500 ms apart; any other failure is `git_failed` at once. A `git worktree add` in the same repo writes a placeholder all-zero `HEAD` into `.git/worktrees/<id>/` before the real one, and a concurrent fetch's connectivity check, which walks every worktree's `HEAD`, dies on it (reproduced: 8 parallel `worktree add --no-track` with 8 parallel `fetch`, 10 rounds, 6 of 80 fetches failed, all so; hs-d243). It is a targeted retry of a transient git race, not a lock.

**Deadlines** (every git call has one; §1.2 `tool_timeout`): a local `git` call 60 s; `git` over the network (`fetch`, `push`, `ls-remote`) 300 s. This holds for the Gate's and the merge's calls, and for `pr open`, `pr rebase`, `pr add`, `pr start` and `plan lock`: a stalled remote therefore ends in `tool_timeout` instead of blocking the command. Hooks have their own timeouts (§7.3).

**Everything in this section is per PR** (R34). `<repo>`, `<base>`, `<lead branch>` and `<lead worktree>` are the `repo`, `base`, `lead_branch` and `lead_worktree` of one PR (§2.2): the PR being started (§7.2), the Job's PR for an Attempt, its merge and its tamper check (§7.2–§7.5), or the PR named by `--pr` for `pr open`, `pr rebase` and `pr view` (§7.6). Two PRs of one Plan in the same repo are two Lead Branches and two Lead worktrees in that repo.

**No lock between PRs** (hs-d243). Two PRs in one repo, of one Plan or of two, share nothing cpd locks: work that conflicts across PRs is a planning matter (the PRs' `needs`), not something cpd serialises. What they do share is git's own state, and git guards it: the object store; each ref by its own lock file, waited for as above; `packed-refs` likewise. The one piece of shared state git does not retry on, `.git/config` (`config.lock` fails at once), cpd never writes: Lead worktrees are created with `--no-track` and no push uses `-u` (§7.2, §7.6; G1). Inside one PR, the exclusion is replaced by state that already exists or is per invocation: the compare-and-swap on the Lead Branch ref (§7.4 step 8), the compare-and-swap on `attempts.status = 'merging'` and the driver lock (R, §7.4), the PR's status in the database, rule S for the Lead worktree (§7.4), rule P for pushes (§7.4) and a gate worktree per Gate run (§7.6).

### 7.1 Worktrees and branches

| What | Path | Branch | Created by |
|---|---|---|---|
| Lead worktree, one per PR | `<repo-parent>/<repo-name>.<plan>.<key>` | `cpd/<plan>/<key>` | the PR's start (§3.8): `plan lock` for a PR with no needs, engine E7 or `pr start` for the others |
| Attempt worktree | `<repo-parent>/<repo-name>.<plan>--<attempt>` | `cpd/<plan>--<attempt>` | driver A2, in the repo of the Job's PR |
| Gate worktree, one per Gate run (hs-d243) | `<repo-parent>/<repo-name>.<plan>.<key>.gate-<pid>`, `<pid>` the pid of the cpd process that runs the Gate | none: detached at the gated commit | `pr open`, `pr rebase`, `pr push` (§7.6); removed by the same run |

`<repo-parent>` and `<repo-name>` are those of the PR's repo, and `<key>` is the PR's key (A5). One rule names every PR; Attempt names are unchanged by R34. A PR made by migration 4 from a Plan of before R34 keeps the names it had, `<repo-parent>/<repo-name>.<plan>` and `cpd/<plan>` (§4.5); its gate worktrees are named by the rule above all the same. Lead Branch names never collide: within a Plan the key is unique, and `cpd/<plan>/<key>` and `cpd/<plan>--<attempt>` differ by their separator. A gate worktree's path never collides with a Lead or Attempt worktree's, since a Plan id and a PR key have no `.` (§2.1).

**The Lead worktree is a view** of its Lead Branch (hs-d243): no Gate runs in it, and cpd changes its files only at the PR's start (the checkout and `pre-start`) and by rule S. The Lead Branch moves by a ref compare-and-swap (a merge, R, `pr rebase`), and whoever moved it brings the Lead worktree up to date by rule S (§7.4), which ends in the same state whatever the order of the movers. Its local changes are the Lead's or the User's own: no cpd command refuses because of them, and rule S never overwrites them.

The repo's main checkout is never touched; Lead and Attempt worktrees are siblings of it (t3, kept). A PR's Lead worktree and local Lead Branch stay until the PR is merged; the PR's clean-up then removes them unless the worktree is dirty or has unpushed commits, which are kept with one `error` Event `cleanup_failed` (§7.2).

### 7.2 Creating worktrees

- **Lead worktree**, at the PR's start (§3.8: `plan lock`, engine E7 or `pr start`; R35 applied to the PR's Lead Branch, A5): first, a leftover of an interrupted start (a worktree at the PR's Lead worktree path on `<lead branch>`, with that branch) is removed; any other existing path is `worktree_exists`, any other local `<lead branch>`, or a local `cpd/<plan>` when the Lead Branch is `cpd/<plan>/<key>` (§2.1), is `branch_exists` (exit 3). Then `git -C <repo> fetch origin <base>` (always fresh, so a PR started after its prerequisite merged contains it, A9), then look for the Lead Branch on the remote: `git -C <repo> ls-remote --exit-code --heads origin <lead branch>` (exit 0: it exists; exit 2: it does not; any other result is `setup_failed`). Three cases:
  - **On the remote, and `origin/<base>` is its ancestor** (`git -C <repo> fetch origin <lead branch>`, then `git -C <repo> merge-base --is-ancestor origin/<base> origin/<lead branch>` exits 0): **reuse** it, `git -C <repo> worktree add --no-track -b <lead branch> <path> origin/<lead branch>`. This is how a PR started again after an interrupted start, or on another machine, continues from the pushed branch.
  - **On the remote, and `origin/<base>` is not its ancestor** (the same command exits 1): the start is refused `branch_exists` (exit 3) and nothing is changed; cpd never resets or force-pushes a branch it did not create.
  - **Absent from the remote:** create it from the base, `git -C <repo> worktree add --no-track -b <lead branch> <path> origin/<base>`, and, after `pre-start` has passed, push it at once: `git -C <lead worktree> push origin <lead branch>` (no `-u`). A failed push does not fail the start: cpd appends one `error` Event with code `push_failed` (with git's stderr and the PR's key), and the next merge (§7.4) or `pr open` (§7.6) pushes again.

  A repo with no `origin` remote (A3): no fetch, no remote check and no push; the worktree is `git -C <repo> worktree add --no-track -b <lead branch> <path> <base>`, as before R35.

  **No upstream** (hs-d243, G1): a Lead Branch has no upstream (`branch.<name>.remote`, `.merge`), because setting one writes `.git/config`, which every worktree of the repo shares and which git locks without retrying, so two PR starts at once failed and left a branch with no worktree. `--no-track` stops `worktree add` from setting one from a remote-tracking start point (`branch.autoSetupMerge`, git-branch(1)), and no push uses `-u`. Nothing in cpd reads an upstream.

  Then `pre-start` in the worktree (§7.3), read from the PR's repo (the new worktree's `.config/wt.toml`), its output in `run/<plan>/logs/lead-<key>.pre-start.log`. On failure: `git worktree remove --force <path>`, `git branch -D <lead branch>`, and the start fails with `setup_failed` (exit 1); nothing in the database changed. The remote is touched only after `pre-start` has passed; a start that fails after its push leaves the pushed branch, which the next start reuses.

  **At `plan lock`** the PRs with no needs start together and all or nothing: every one's worktree is created and its `pre-start` run first; only when all have passed are the new branches pushed and the lock's transaction written. If one fails, every worktree and branch the lock created is removed, nothing is pushed, and `plan lock` fails with `setup_failed`, `details.pr` naming the PR. **From engine E7 and `pr start`** one PR starts; its failure is handled as §3.8 says.
- **Attempt worktree** (A2): read `L = git rev-parse refs/heads/<lead branch>`; `git -C <lead worktree> worktree add -b cpd/<plan>--<attempt> <path> L`; record `base_sha = L`; run `pre-start` in it, blocking, so the Worker never starts before setup has finished (requirements §5, kept). On failure: remove the worktree and branch as above, outcome `failed` / `setup_failed`.
- **Removal** after a Merged Attempt (A9): `git worktree remove <path>` then `git branch -d cpd/<plan>--<attempt>` (`-d`, not `-D`: git refuses unless the branch is merged into HEAD of the Lead worktree, a native guard; t3 F9). Failed and cancelled Attempts keep theirs until their PR is merged.
- **Clean-up at a PR's merge** (hs-d138, hs-d142): when `pr poll` sees a PR merged (commands.md §9.2), that PR's Attempt worktrees and local Attempt branches still on disk are removed (`git worktree remove --force`, `git branch -D`), then its Lead worktree and local Lead Branch (`git worktree remove`, `git -C <repo> branch -D <lead branch>`; `-D`, because a squash merge is never an ancestor). It takes no lock (hs-d243); it checks the database and the processes instead, so a Gate, a merge, R or an A9 still at work never loses its worktree (hs-n124, a Lead worktree removed mid-Gate):
  - **The PR is idle.** While any Attempt of the PR's Jobs is not `ended`, or has its driver lock held (probed as E2 does, opening the file without `O_CREAT`, §3.4: a driver that just ended the Attempt may still be in A9, removing its worktree), the poll skips that PR, with no Event, and a later poll cleans it up, since it is still on disk. The poll never waits.
  - **Gate worktrees.** Each gate worktree of the PR (§7.1, §7.6) whose pid is not alive (`kill(pid, 0)` fails with `ESRCH`) is removed (`git worktree remove --force --force`); one whose pid is alive is left alone and retried by a later poll. pid reuse can keep a leftover until a later poll (accepted).
  - **The Lead worktree** is first brought up to date by rule S (§7.4), so a tree a merge left stale is not mistaken for local changes; then it is checked and removed as below. Its Gate never runs there (§7.6), and a `pr push` or `pr rebase` still running for the merged PR stops before any push or `gh` call, because it re-reads the PR's status after its Gate (§7.6).

  Each removal is best effort: one failure does not stop the others, and every later poll retries the merged PRs that still have a path on disk. A Lead worktree with uncommitted changes (`git --no-optional-locks status --porcelain` not empty, untracked files included; `--no-optional-locks` so the check takes no `index.lock`, git(1)) or with commits not on origin (`git rev-list HEAD --not --remotes=origin` not empty) is **kept**, with its Lead Branch, and the poll that saw the merge appends one `error` Event `cleanup_failed`; later polls skip it silently. A rule S that stops here appends no `lead_worktree_stale`: it leaves local changes, which this check keeps and reports as `cleanup_failed`. Remote branches are never touched. A PR closed without a merge and an abandoned Plan are not cleaned up here (G5).

### 7.3 Hooks from `.config/wt.toml`

cpd reads `<worktree>/.config/wt.toml` from the worktree it is about to run the hook in, at the moment it runs it, never from a cached copy. Behaviour matches worktrunk 0.80.0 so that repos set up for wt behave the same (docs: `wt hook --help`; the points marked verified were run with `wt` 0.80.0 on 2026-10-14).

**Tables cpd reads:**

| Hook | Used for | When |
|---|---|---|
| `pre-start` | setup | after a Lead or Attempt worktree is created (§7.2), and in a gate worktree before its Gate (§7.6); blocking |
| `pre-commit` | the **quick check** | at `attempt submit`, in the Attempt worktree. When the file has no `pre-commit` table, there is no quick check: it passes, and `pre-merge` does **not** run instead (R20; this replaces outline §6's fallback). This is cpd's own use: worktrunk runs `pre-commit` before the commits it makes, which cpd does not make |
| `pre-merge` | **the Gate** | at `pr open` and `pr push`, before they push, and at `pr rebase`, after the rebase and before it pushes: in a gate worktree of its own, detached at the gated commit, after that worktree's `pre-start` (§7.6, hs-d243). Never in the Lead worktree, and never per Job (R19) |

Every other table (`post-*`, `pre-switch`, `pre-remove`, aliases, `[list]` and so on) is ignored. A missing file or a missing table means no commands, and the hook passes (wt prints "No pre-merge hooks configured" and exits 0; verified).

**Forms and order** (same as worktrunk):

| TOML shape | Meaning | Order |
|---|---|---|
| `pre-merge = "cmd"` | one unnamed command | |
| `[pre-merge]` with `name = "cmd"` keys | named commands | **concurrent**: all start together and all are waited for, even after one fails (verified: a failing `fast` did not stop `slow`) |
| `[[pre-merge]]` blocks | a pipeline | blocks run in file order; keys within a block run concurrently; a failing block stops the pipeline (verified) |

The hook's result is the first failing command's exit code in file order, or 0. Output lines of concurrent commands are prefixed `<name> │ ` as wt does, and the whole output is written to `run/<plan>/logs/<attempt>.<hook>.log`; results carry its last 60 lines.

**How each command runs:**
- `sh -c <command>` (verified: wt runs `sh`), cwd = the worktree root.
- stdin: the hook's template variables as one JSON object (verified: wt passes the JSON context on stdin).
- Environment: cpd's own environment, minus `TMUX`, `TMUX_PANE`, every `PI_*` and every `CPD_*` variable, so a test suite run by the Gate cannot reach the real cockpit, pi session or cpd database. wt adds no variables of its own (verified), and neither does cpd.
- Templates: `{{ … }}` and `{% … %}` are rendered before running, with the variables `branch`, `worktree_path`, `worktree_name`, `commit`, `short_commit`, `repo`, `repo_path`, `primary_worktree_path`, `default_branch`, `remote`, `remote_url`, `cwd`, `hook_type`, `hook_name`, `args` (`[]`), plus `base` and `base_worktree_path` for `pre-start`, `target` and `target_worktree_path` for `pre-commit` (the Lead Branch and Lead worktree), and `target` and `target_worktree_path` for `pre-merge` (the PR's `base`, and the worktree that has it checked out; when none has, `target_worktree_path` is undefined, as in wt). Their values follow wt's table (docs). **In a gate worktree** (§7.6) `HEAD` is detached, so `branch` is not read from it: cpd passes the PR's Lead Branch as `branch`, for `pre-start` and `pre-merge` alike, so `{{ branch }}` means what it meant when the Gate ran in the Lead worktree; `worktree_path` and `worktree_name` are the gate worktree's, `commit` the gated commit, and `base` and `base_worktree_path` of its `pre-start` are those of the PR's start (hs-d243). worktrunk's own filters and functions (`sanitize`, `sanitize_db`, `sanitize_hash`, `hash`, `hash_port`, `codename`, `worktree_path_of_branch`) and `vars.*` are not provided: a command that uses one, or an undefined variable, fails before anything runs with `hook_invalid`. User hooks in `~/.config/worktrunk/config.toml` are not run: the repo owns its setup and Gate (R5), and a machine-local hook would make Gates differ between machines. There are no approvals (cpd behaves as `wt --yes`).
- Timeout: `setup_minutes` for `pre-start`, `gate_minutes` for `pre-commit` and `pre-merge`. At the limit cpd kills the command's process group; it also kills it when the command's `sh` exits (`killpg` with `SIGKILL`, `ESRCH` ignored), so a background child the hook left behind does not outlive it. A caller (cpd) that dies mid-hook has no portable teardown (macOS has no `PDEATHSIG`); there is no detector (hs-d34, hs-d39).

**What each result means:**

| Result | `pre-start` | quick check at submit | Gate at `pr open`, `pr rebase` and `pr push` (its `pre-start` included) |
|---|---|---|---|
| exit 0 (or no commands) | setup done | submit continues | the command continues to its push |
| exit non-zero | `setup_failed` | refused `check_failed` (exit 3) with the tail; the Worker fixes and submits again | refused `gate_failed` (exit 3) with the tail; nothing is pushed; the Lead appends fix Jobs. A gate worktree's `pre-start` that exits non-zero is refused `gate_error` (exit 1) instead, with its own tail and log (§7.6) |
| killed at the timeout, killed by a signal, `sh` not startable, or `hook_invalid` | `setup_failed` | refused `check_error` (exit 1) | refused `gate_error` (exit 1) with the tail; nothing is pushed; the Lead decides |

Change from cpd 1: exit code 64 is no longer special. cpd 1 needed `verify = "./verify.sh || exit 64"` to tell a Gate failure from worktrunk's own errors; cpd 2 runs the commands itself, so any non-zero exit is a failed check. Repos that keep `|| exit 64` still work.

**`attempt submit` preconditions**, in order, each refused with exit 3 and a code the Worker can act on (cpd 1 §11, kept except the check; R20 keeps exactly these at submit):
1. the caller's cwd is inside the Attempt worktree, which is on the Attempt branch (`wrong_worktree`);
2. clean tree, untracked files included (`dirty_tree`);
3. at least one commit ahead of `base_sha` (`no_commits`); **for a review Job (B2) instead: no commit ahead of `base_sha` (`review_commits`) and a non-empty `run/<plan>/reviews/<k>.md` (`review_missing`)**, and precondition 5 is skipped for it (§3.4);
4. the tamper check passes (`gate_tampered`, §7.5);
5. the quick check passes (`check_failed`, or `check_error` exit 1); with no `pre-commit` table it passes without running anything, and `check_tail` stays null.

On success the Attempt becomes `merging` with `submitted_sha` and `check_tail`, and the output tells the Worker to stop.

### 7.4 The merge (driver step A8)

The merge takes **no lock** (hs-d243). Two PRs' merges touch different refs and worktrees (§7). Two merges of one PR are kept apart by the compare-and-swap of step 8, their pushes by rule P and their worktree syncs by rule S (both below); a merge never waits for a Gate, which runs in a worktree of its own (§7.6). Before hs-d243 a per-repo lock `locks/repo-<sha1>.lock` was held from step 1 to step 10, and every holder ran R first.

0. **Branch check** (engine-W1). At the merge's start the driver compares the Attempt branch's tip (`git rev-parse refs/heads/<attempt branch>`) with the Attempt's `submitted_sha`; when they differ the Attempt ends `failed` / `tampered` (the existing cause) with the detail `the Attempt branch moved after submit (<submitted> -> <tip>)`. So that nothing can move the branch afterwards, the driver kills the Worker's window as soon as it sees the Attempt `merging`, before it waits for the per-repo lock.
1. **Cancel check.** If the Job has a cancel request: end `cancelled`.
2. **Snapshot** `old = git rev-parse refs/heads/<lead branch>`.
3. Removed by hs-d243 (it was the Lead worktree check, `dirty_target`, t3 F8): the merge never works in the Lead worktree (the rebase is in the Attempt worktree, t3 F6; the fast-forward is a ref compare-and-swap), and rule S at step 9 neither overwrites local changes nor fails the Job for them. Without the lock, the check would also take another merge's half-written sync for local changes and fail a Job for it. The step keeps its number, as step 6 does; the cause `dirty_target` is no longer produced (§3.4).
4. **Tamper check** of `old...<attempt branch>` (§7.5), else `tampered`.
5. **Rebase** in the Attempt worktree: `git -C <attempt worktree> rebase old`. On a conflict: record `git diff --name-only --diff-filter=U`, run `git rebase --abort`, and end `merge_conflict` with the file list in the feedback. The rebase never happens in the Lead worktree (t3 F6).
    **Skipped when `old` is already an ancestor of the Attempt's HEAD** (hs-d139, hs-n52, hs-n67): `git -C <attempt worktree> merge-base --is-ancestor old HEAD` exits 0, so the Attempt already contains the Lead tip and a rebase would add nothing; the merge goes straight to step 7 and the fast-forward of step 8 (`new` = HEAD). Why: a Job may carry a merge of the base into its branch; a rebase drops merge commits and replays the base's commits, which then conflict. Exit 1 (not an ancestor): rebase as above; any other result is `git_failed`. The tamper check of step 4 allows the protected files such a merge brings (§7.5).
6. Removed by R19 (no Job-level Gate): the merge runs no Gate; the Gate runs at `pr open` and `pr rebase` (§7.6). The step keeps its number so that steps 7–10 keep theirs.
7. **Cancel check** again (the last safe point).
8. **Fast-forward with a compare-and-swap:** `new = git -C <attempt worktree> rev-parse HEAD`; verify `git merge-base --is-ancestor old new`; then `git update-ref -m "cpd: merge <plan>/<attempt>" refs/heads/<lead branch> new old`. git applies it only if the ref still equals `old` (t3 F7). If it fails because the ref moved (another merge of the same PR, a `pr rebase`, or someone outside cpd moved it): back to step 2, at most 3 times, then `cas_lost`.
    **Push** (R35), right after the fast-forward: rule P (below) with `new`, a plain push, never forced. A repo with no `origin` remote does not push. A failed push does not fail the Job: the merge stands and the Job is Merged, cpd appends one `error` Event with code `push_failed` (with git's stderr), and the next merge or `pr open` pushes again.
9. **Sync the Lead worktree** by rule S (below). When rule S stops, the merge stands, because the ref has moved, and cpd appends `error` with code `lead_worktree_stale` (with git's stderr when git failed) and the command that fixes it. A rule S that gives up because `index.lock` stayed held appends nothing.
10. End the Attempt `merged` / `merged` with `merged_sha = new`, and the Job `merged`, in one transaction (`job-merged`).

**Rule S: the Lead worktree follows its Lead Branch, in any order** (hs-d243). Whoever moves a Lead Branch brings its Lead worktree up to date, after the move: merge step 9, R2 and `pr rebase` (§7.6); the PR's clean-up runs it too, before its dirty check (§7.2). It takes no lock: each run loops until the tree equals the `HEAD` of that moment, so whichever mover runs last finishes the job, and two runs at once take turns through git's own `index.lock`. Up to 20 rounds, each:

1. `H = git -C <lead worktree> rev-parse HEAD` (the Lead worktree has the Lead Branch checked out, so this is its tip);
2. `git -C <lead worktree> diff --quiet` exits 1 (the working tree differs from the index): local changes, **stop** with `lead_worktree_stale`;
3. `T = git -C <lead worktree> write-tree`, the tree the index and the working tree agree on;
4. `T` equals `git -C <lead worktree> rev-parse H^{tree}`: **done**;
5. `T` is not the tree of an entry of the Lead Branch's reflog (`git -C <lead worktree> log -g --format=%T refs/heads/<lead branch>`): staged local changes, **stop** with `lead_worktree_stale`. A missing or empty reflog counts as local changes;
6. `git -C <lead worktree> read-tree -m -u T H`. If it fails because `.git/worktrees/<id>/index.lock` exists (another rule S, or the Lead's own git command), sleep a random 100–500 ms; any other failure (for example an untracked file in the way) is a **stop** with `lead_worktree_stale` and git's stderr. Then the next round.

After 20 rounds with neither done nor stop, rule S gives up and appends nothing: the next mover finishes it. Why the reflog (step 5): a stale tree is a previous tip of the branch; a staged edit of the Lead's is not, and looks the same to `diff`, and rule S runs at every merge, so it must never take one for the other. `core.logAllRefUpdates` is "true by default in a repository that has a working directory" (git-config(1)); a repo with reflogs off has every stale tree reported, and nothing is lost. Who reports a stop: the mover whose rule S stopped, with one `error` `lead_worktree_stale` (step 9, R2, `pr rebase`); the clean-up reports it as `cleanup_failed` (§7.2). Rule S replaces step 9's `read-tree -m -u old new` and R2's "stale" repair of before hs-d243.

**Rule P: every push names its commit** (hs-d243). cpd pushes a Lead Branch as `git -C <lead worktree> push --porcelain origin <sha>:refs/heads/<lead branch>`, never with `-u` (§7.2), so it publishes the commit it means and not whatever the ref holds at that moment. `<sha>` is `new` at step 8, `tip` at R2 (below), the gated commit at `pr open` and `pr push`, and, with a lease, the rebased commit at `pr rebase` (§7.6). A plain push that git rejects as not a fast-forward is checked before it counts as a failure: `X` is the remote tip, `git -C <repo> ls-remote origin refs/heads/<lead branch>`; when `X` is in the local object store and `git -C <repo> merge-base --is-ancestor <sha> X` exits 0, the remote already contains `<sha>` (a later merge pushed a descendant), and the push is a **success** that pushed nothing. Otherwise the rejection stands: `push_failed` at step 8 and R2, `push_rejected` at `pr push`, `git_failed` at `pr open`.

**R: recovering an interrupted merge.** The ref moves at step 8, but the Ledger records the merge only at step 10. A driver that dies in between leaves an Attempt in `merging` whose commits are already on the Lead Branch; ending it `crashed` would make the ladder re-run a Merged Job. So an Attempt in `merging` whose driver lock is free is ended only by R. R works on **one Attempt** and takes no repo lock (hs-d243): it first takes that Attempt's free driver lock without waiting (held: a live driver, or another R, has it, and R does nothing), re-reads the Attempt (no longer `merging`: nothing to do), and ends it with the compare-and-swap on `status = 'merging'` (§4.2), so two runs of R, or R and a live driver, never both end it.

- **R1.** `tip = git -C <repo> rev-parse refs/heads/cpd/<plan>--<attempt>`, then `git -C <repo> merge-base --is-ancestor <tip> refs/heads/<lead branch>`.
- **R2. Exit 0: the merge happened.** Rule S on the Lead worktree (its stop is reported as step 9's is). Then run the step-10 transaction with `merged_sha = tip` and A9's `metrics` (`job-merged`, with `cancel_too_late: true` if the Job has a cancel request), then the rest of A9. R also pushes, as step 8's push does (rule P with `tip`), after it records the merge; a failure is `push_failed` and does not undo it.
- **R3. Exit 1: no merge.** End the Attempt `cancelled` / `cancel_requested` if the Job has a cancel request, else `failed` / `crashed` (a ladder cause, §3.4).
- **R4. Any other result** (git failed) proves nothing either way: R changes nothing and appends `error` with code `merge_check_failed` the first time it fails for that Attempt; it is run again at the next occasion below.

Who runs R:
- **E2**, for every Attempt in `merging` whose driver lock is free (§3.7), at every tick, each Attempt on its own.
- **`pr rebase`'s guard U** (§7.6), for an Attempt of its PR whose compare-and-swap is in its snapshot and whose driver is dead, so the rebase never rewrites a merge the Ledger has not recorded (R1 would then answer "not an ancestor").
- Nobody else (hs-d243). The merge needs no R first: step 3 is gone, and rule S finishes a dead driver's stale Lead worktree. `pr open` needs none: an unrecorded merge leaves its Job `running`, which `jobs_not_done` refuses. `pr push` needs none: pushing a commit the Ledger has or has not recorded changes nothing for R.
- **Nothing restarts a driver at A8.** A dead driver's Attempt is ended by R or E2, never resumed; an implementation that adds a restart at A8 must run R first, so a merge is never done twice.

A driver killed while its `git update-ref` child runs frees its driver lock before git finishes, so R can decide R3 a few milliseconds before the compare-and-swap lands (unverified, inferred: the driver's descriptors are close-on-exec, §3.4). This was so under the per-repo lock too, and hs-d243 does not change it.

**A review Job's merge** (B2). The driver runs step 1 (cancel check), step 2 (snapshot `old`) and step 7 (cancel check again); it skips steps 4–6 and 8–9 (no tamper check, rebase, fast-forward, push or worktree sync: the Attempt has no commits and the Lead Branch does not move), then runs step 10 with `merged_sha = old`. R ends a review Attempt found in `merging` with a free driver lock `merged` (its submit already proved the findings file and the absence of commits), with `merged_sha` the Lead Branch's tip at that moment, and pushes nothing. A9 removes its worktree and branch as for a merged Attempt (`git branch -d` succeeds: the branch has no commits of its own).

**No Gate at the merge** (R19). A merge is checked by the Attempt's quick check at submit (§7.3), the tamper check (step 4) and a clean rebase (step 5); the Gate runs on the whole Lead Branch, at `pr open` and `pr rebase` (§7.6), and a failure there is fixed by Jobs the Lead appends. cpd 1 ran the Gate on the rebased Attempt and again on the Lead (cpd 1 §12 step 11); both runs are dropped. The tamper check still guards the Gate's own files (§7.5), so an Attempt cannot weaken the Gate that later runs on its commits.

### 7.5 The tamper check

A diff, computed as `git diff --name-only --no-renames <base>...<branch>` (so renaming a protected file away still shows its old path; cpd 1 `gate.ts`, kept), may not touch:
- `.config/wt.toml` (the hooks) and `.config/cpd.toml` (this list);
- anything under `.cpd/`;
- each path in the repo's own protected list: `protected = ["verify.sh", "scripts/ci/"]` in `.config/cpd.toml`, read from the Lead Branch's tree (`git show <lead branch>:.config/cpd.toml`), never from the Attempt's. An entry ending in `/` is a directory prefix; any other entry is an exact path. The file is optional; unknown keys are `config_invalid`.

**Exception: a base merge** (hs-d139). A protected path in that diff is allowed when the Attempt's version of it equals the Lead tip's or `origin/<base>`'s: the same blob id (`git rev-parse <rev>:<path>`), or absent in both. So a Job that merges its base may bring the base's protected files. Any other change to a protected path stays `tampered` (`gate_tampered` at submit). The Lead tip is the Lead Branch's tip when the check runs; `origin/<base>` is the remote-tracking ref as last fetched.

The list lives in its own file, not in `.config/wt.toml`, because worktrunk warns about unknown tables in its file on every run (verified: "Project config has unknown field cpd (will be ignored)"). cpd 1 hard-coded `verify.sh`; now the repo names it. It runs at `attempt submit` (refusal `gate_tampered`) and again at merge (outcome `tampered`).

### 7.6 Pull requests and the PR rebase

Each command here works on **one PR** (R34, A14), selected by `--pr <key>` (§1.3), except `pr poll`, which has no `--pr`: it polls every open pull request of the User at once (hs-d96). Each PR has its own Gate run and its own pull request on its repo's GitHub. `gh` runs with cwd = the PR's Lead worktree, so it addresses the PR's repo; `pr poll`'s calls (`gh api graphql`) address no repo.

None of these commands takes a lock on the repo or waits for another PR (hs-d243, §7). Merges into the PR go on while they run; what keeps them apart is the PR's status in the database, the Lead Branch's compare-and-swap, rule P and rule S (§7.4), guard U (`pr rebase`) and the gate worktree below.

**The Gate on a snapshot** (hs-d243). `pr open`, `pr push` and `pr rebase` run the Gate on one commit `G`, read once (for `pr rebase`, the rebased commit), in a **gate worktree** `W` of their own (§7.1), never in the Lead worktree:
1. Every gate worktree of the PR whose pid is not alive (`kill(pid, 0)` fails with `ESRCH`), the leftover of a run that was killed, is removed: `git -C <repo> worktree remove --force --force <path>`. pid reuse can keep a leftover until a later sweep (accepted).
2. `git -C <repo> worktree add --detach --lock --reason "cpd gate <pid>" W G`, `<pid>` this process's. Detached: no branch and no `.git/config` write. `--lock` is "the equivalent of git worktree lock after git worktree add, but without a race condition" (git-worktree(1)), so `git worktree prune` and a plain `git worktree remove` leave it alone.
3. `pre-start` in W (§7.3, `setup_minutes`), its output in `run/<plan>/logs/pr-<key>.pre-start.log`; then **the Gate** (R19), `pre-merge` in W (`gate_minutes`), its whole output in `run/<plan>/logs/pr-<key>.pre-merge.log`. Both come from W's `.config/wt.toml`, which is G's: the hooks come from the tree under test, which the tamper check keeps every Attempt from changing (§7.5); this is what fixes v0's stale-hook incident. A `pre-merge` failure is refused `gate_failed` (exit 3), an error `gate_error` (exit 1), both with the last 60 lines in `details.tail` and the log's path in `details.log`; a `pre-start` that fails, in any way, is refused `gate_error` (exit 1) with its own tail and log, and `pre-merge` does not run. Nothing is pushed on a refusal.
4. `git -C <repo> worktree remove --force --force W`, whatever the result ("To remove a locked worktree, specify --force twice", git-worktree(1)).
5. The PR is read again. When it is no longer in the status the command needs (`dispatch` for `pr open`, `open` for `pr push` and `pr rebase`; another run, or the poll, changed it during the Gate), the command stops with its own refusal for that status (`pr open`: `already`; `pr push` and `pr rebase`: `illegal_transition`) and pushes nothing. So a `pr push` or `pr rebase` whose PR merged during its Gate never pushes or calls `gh` in a Lead worktree the clean-up removed (§7.2).
6. The push, by rule P (§7.4).

What follows: the Lead worktree's state does not affect the Gate, so its local changes refuse nothing (there is no `dirty_target` here any more: uncommitted edits there are neither gated nor pushed, which was the reason for the check); two Gates of one PR at once each have their own W; and a merge into the PR during a Gate does not wait, so the PR's head can be past G when the command returns, which the output's `remote` shows. Each Gate runs `pre-start` in a fresh worktree; its cost against a warm Lead worktree is unmeasured (`docs/research/no-repo-lock.md` §4, risk 1). Rejected: one reusable gate worktree per PR, which needs an exclusion between two Gates of one PR.

- **`pr open --pr <key>`**: the PR must be `dispatch` (`open` or `merged`: `already`; `waiting` or `cancelled`: `illegal_transition`), and every Job of the PR `merged` or `cancelled`, at least one `merged` (`jobs_not_done`). No R first: an unrecorded merge leaves its Job `running`, which `jobs_not_done` refuses (§7.4).
  0. **the lookup**, before the Gate: `gh pr list --head <lead branch> --state all --json number,url,state`, whose newest pull request decides. **`MERGED`**: the Lead Branch was merged on GitHub and cpd never recorded it (an earlier `pr open` that died, a merge by hand), so the PR adopts it with **no Gate and no push** (a pull request is never *opened* without the Gate, hs-d267; this one is already merged): one transaction writes the `pull_requests` row from one lookup (§7.6 `pr poll` step 3's query: `state` `merged`, `merged_at` GitHub's, else the transaction's time; the row is updated to what the lookup reads when the poll stored it first), then the PR `dispatch → open → merged` (`pr_number`, `pr_url`, `pr_opened_at` and `merged_at` the transaction's time, `pull_request`) and **exactly the poll's merge** (step 6): `pr-merged`; when every PR that is not `cancelled` is then `open` or `merged` the Plan `dispatch → deploy` with `plan-phase` (§3.1), and when every one is `merged` the Plan then `deploy → done` with `done_at` and `plan-done`. No `pr-opened`. The clean-up of the merged PR's worktrees is the poll's: the next poll removes what is on disk (§7.2). The output says `"adopted": "merged"`. **`CLOSED`** (not merged): refused `pr_closed` (exit 3) naming the pull request; the Lead decides, to reopen it on GitHub or to move the work to a new branch. **`OPEN`** or none: the steps below, which adopt the open one instead of creating it. A merged pull request is looked up after the Jobs are checked (`jobs_not_done`) and before the Gate;
  1. `G = git -C <repo> rev-parse refs/heads/<lead branch>`;
  2. **the Gate** on G, steps 1–5 above. On `gate_failed` or `gate_error` the PR stays `dispatch`, and the Lead appends fix Jobs to it (`job add --pr <key>`, queued at once, §3.2), then runs `pr open --pr <key>` again once they are Merged;
  3. rule P with G (§7.4), then `gh pr create --base <base> --head <lead branch> --title <title> --body <summary>` (cpd 1 §11, kept), unless step 0 found an open pull request of the Lead Branch (an earlier run died before its transaction), which is adopted. When `gh pr create` fails, the pull request is looked up again and adopted if it exists now (another `pr open` of the PR created it; that GitHub refuses a second one for the same head is unverified); otherwise the failure stands. `<title>` is the Plan's title when the Plan has one PR that is not `cancelled`, else `<plan title> (<key>)`; `<summary>` covers the PR's own Jobs and Merged commits;
  4. one transaction, with a compare-and-swap on the PR's `status = 'dispatch'` (a concurrent `pr open` that committed first: `cas_lost`): the `pull_requests` row of the pull request, from `gh pr create`'s URL (`repo`, `number`, `url`, the title given to `gh`, `draft` 0, `state` `open`, `review_decision` and `checks` `none`, `mergeable` and `merge_state` `unknown`, `created_at` and `changed_at` now): inserted, or, when the poll stored it first, kept as the poll wrote it (hs-d96, D6); the PR `open` with `pr_number`, `pr_url`, `pr_opened_at`, `pull_request` (the row's id), `pr-opened`; and, when every PR that is not `cancelled` is now `open` or `merged`, the Plan `dispatch → deploy` with `plan-phase` (§3.1).

  The output carries `sha` (G), `pushed` (false when the remote already had G, or a descendant of it, rule P) and `remote`, the remote branch's tip after the push: past G when merges that landed during the Gate were pushed (hs-d243).
- **`pr poll [--plan <id>]`** (hs-d96, hs-d97, hs-d98, hs-d107; evidence: `docs/research/gh-interface.md`, "Failure behaviour", "Facts", "Recommendation"). One poll of all of the User's open pull requests, any repo on github.com, whether or not any Plan is in `deploy`; no phase precondition. Run by engine E5 every `pr_poll_seconds` (§3.7) or by a Lead; under `flock` on `locks/pr-poll.lock`, waiting at most 60 s, else `lock_timeout` (exit 5).
  1. **The search** (D1): one `gh api graphql` call, its query text passed inline with `-f query=<text>` (`-f query=@file` sends the literal `@file`; no temporary file): `search(type: ISSUE, query: "is:pr is:open author:@me archived:false sort:created-asc", first: 100) { issueCount nodes { ... on PullRequest { repository { nameWithOwner } number url title isDraft state createdAt mergedAt reviewDecision mergeable mergeStateStatus commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } } } } rateLimit { cost remaining resetAt }`, and, in the same query, a **second search alias** for the review requests, `review-requested:@me is:pr is:open archived:false sort:created-asc` (first 100, the same fields; `author:@me` stays the first alias, `authored`'s). It costs 1 point per search (the first alone measured). Above 100 open pull requests per search only the oldest 100 are seen. An unknown search qualifier silently returns 0 pull requests, so a test pins the query string, which is a constant.
  2. **The mapping** (D2). `checks` from `statusCheckRollup.state`: `SUCCESS` is `passed`; `FAILURE` and `ERROR` are `failed`; `PENDING` and `EXPECTED` are `pending`; a null rollup or commit is `none` (`failed` can show while other checks still run: accepted). `review_decision` from `reviewDecision`: `APPROVED` is `approved`, `CHANGES_REQUESTED` is `changes_requested`, anything else or null is `none`. `state`: `OPEN` is `open`, `CLOSED` is `closed`, `MERGED` is `merged`. `mergeable` from `mergeable`: `MERGEABLE` is `mergeable`, `CONFLICTING` is `conflicting`, `UNKNOWN` is `unknown`. `merge_state` from `mergeStateStatus`: `BEHIND`, `BLOCKED`, `CLEAN`, `DIRTY`, `DRAFT`, `HAS_HOOKS`, `UNKNOWN`, `UNSTABLE` are the same words lowercased. Both are read in the same call (no preview header). A value outside these sets fails the poll (`gh_failed`), as the other mappings do.
  3. **The lookup** (D3) for the pull requests of `prs` rows in status `open` whose `pull_requests` row is `state` `open` and that neither search listed (they merged, were closed, or the index lags). A pull request already stored `closed` (A11) is right as it is: it is not looked up again until a search lists it: one more `gh api graphql` call with one alias per pull request, `p0: repository(owner: "<o>", name: "<n>") { pullRequest(number: <n>) { <the same fields> } }`. `gh` exits 1 when an alias is `NOT_FOUND` although `data` holds the others, so stdout is parsed whatever the exit code; a call with no `data` is `gh_failed`. An answer `OPEN` means "not decided yet" (the search index lags, or the User switched `gh` accounts): nothing changes.
  4. **Failures** (D4). Each call has its own 15 s timeout (`gh` has none; GitHub cuts at 10 s). An empty body, a non-JSON body, or a search response with `errors` is `gh_failed` (exit 1; `details`: `argv`, `exit`, `stderr`), never "no pull requests". A failed poll writes nothing.
  5. **The rows** (D5). One transaction writes every change; a value that did not change is not written, so a poll that finds nothing new opens no write transaction. A pull request in either search is inserted or updated in `pull_requests` (`changed_at` = now when a value changed, `mergeable`, `merge_state`, `authored` and `review_requested` included): `authored` is 1 exactly when it is in the `author:@me` search and `review_requested` is 1 exactly when it is in the `review-requested:@me` search, so a pull request that leaves one search has that flag cleared on the next poll. Both are stored as each poll reads them, `unknown` included (GitHub computes mergeability lazily after the base moves, as checks go `pending` when CI reruns). A row no PR points to is **deleted** when its pull request is in neither open search; a row is **kept** while it is in either search or a PR points to it, and a row a PR points to is never deleted (§4.4). The rows hold what was reported, so a rerun with no change on GitHub reports nothing and writes nothing.
  6. **The Events** (D7, §5.2). In the transaction that writes the change, for the Plan whose `prs` row points to the pull request only: `pr-feedback` when the checks became `failed`, the review `changes_requested`, the state `closed` or `mergeable` `conflicting` (from anything else, once per transition; `unknown` is no conflict). A pull request becoming `merged`: the PR `merged` with `merged_at` (`pr-merged`); when every PR that is not `cancelled` is then `merged`, the Plan's phase `done` with `done_at` (`plan-done`, once each time the Plan reaches `done`, so again when a reopened Plan's last PR merges), after which the Plan's remaining Attempt worktrees and branches are removed in every PR's repo. Engine E7 then starts the PRs whose last prerequisite this was (§3.7). Pull requests no Plan owns get no Event, and a pull request only in the review-request search gets none: `pr-feedback` is unchanged, still only for the author's Plans.
  7. **Output** (commands.md §9.2): what changed, the rate limit of the search (`cost`, `remaining`, `reset_at`, which E5 uses, §3.7), and with `--plan` (or `CPD_PLAN`) that Plan's PRs with their rows.
- **`pr view [--pr <key>]`** (hs-d98, D10), read-only: the PR must be `open` or `merged` (else `illegal_transition`, as `pr rebase`). It runs `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url` and `gh api repos/{owner}/{repo}/pulls/<n>/comments` (the detail the poll does not store) and prints the checks, reviews, comments and inline comments typed. It writes nothing and appends no Event. A Lead reads it when `pr-feedback` arrives (§5.2).
- **`pr rebase --pr <key>`** onto the PR's base; the PR must be `open` (else `illegal_transition`). Merges into the PR go on during it; it rewrites the Lead Branch only by a compare-and-swap, and guard U keeps it from rewriting a merge the Ledger has not recorded:
  1. `git -C <repo> fetch origin <base>` (the ref lock on `origin/<base>` is waited for, §7);
  2. **snapshot**: `S = git -C <repo> rev-parse refs/heads/<lead branch>`, and, in the same round once guard U (step 5) has passed, `X0 = git -C <repo> rev-parse --verify --quiet refs/remotes/origin/<lead branch>`, what cpd last saw of the remote branch (empty when it has never seen it: the lease then requires that the remote has no such branch). It is the lease of step 9: every push of the Lead Branch from this machine, rule P's included, updates that ref, so a push cpd has not seen (from another machine, or GitHub's "Update branch"), which the Lead Branch lacks, makes the push refuse instead of being overwritten (Escalation nl-pr-a1-q1; the remote tip read by `ls-remote` would overwrite it). It is read after guard U because the merges in S push after their compare-and-swap and before their step 10, and R pushes too: read before, their pushes would make the lease stale;
  3. `git -C <repo> merge-tree --write-tree --name-only origin/<base> S`: exit 1 means a conflict; return `rebase_conflict` (exit 3) with the file list and change nothing (t3 F10; the Lead decides, for example by appending a Job);
  4. a gate worktree W at S (§7.1, the Gate on a snapshot steps 1–2), then `git -C W rebase origin/<base>` and `R' = git -C W rev-parse HEAD`; on an unexpected conflict, `git -C W rebase --abort`, W is removed and the result is `rebase_conflict`;
  5. **guard U**: for each Attempt of the PR's Jobs in `merging` whose Attempt branch tip is an ancestor of S (`git -C <repo> merge-base --is-ancestor <tip> S` exits 0: its compare-and-swap happened, its step 10 has not committed): when its driver lock is free, run R for it (§7.4); when it is held, read the store every 200 ms until the Attempt is `ended`, at most 60 s, else W is removed and the result is `cas_lost` (exit 3, `details.attempt`; rerun). An Attempt whose compare-and-swap comes after S cannot be lost: its compare-and-swap fails against R' and it rebases onto R', or step 6 fails against it;
  6. `git -C <repo> update-ref -m "cpd: pr rebase <plan>/<key>" refs/heads/<lead branch> R' S`. When it fails because the ref moved (a merge landed): W is removed and the command goes back to step 2, at most 3 times in all, then `cas_lost` (exit 3). A second `pr rebase` of the PR at once is decided here too: the loser goes back to step 2 and finds the rebase done;
  7. rule S on the Lead worktree (§7.4); a stop appends `error` `lead_worktree_stale` and does not fail the command;
  8. **the Gate** on R' in W, the Gate on a snapshot steps 3–5. On `gate_failed` or `gate_error` nothing is pushed; the rebased Lead Branch stays (step 6 moved it), the Lead appends fix Jobs, which build on it, and runs `pr rebase` again once they are Merged;
  9. `git -C <lead worktree> push --porcelain --force-with-lease=refs/heads/<lead branch>:<X0> origin R':refs/heads/<lead branch>`. A rejection is checked as rule P checks one (§7.4): when the remote tip `X` (`ls-remote`) is in the local object store and R' is `X` or an ancestor of it, the push is a success that pushed nothing, because two `pr rebase` of the PR at once push the same R' and the second can lose the race inside the remote (`cannot lock ref … is at R' but expected X0`), or a merge onto R' pushed after it (Escalation nl-pr-a1-q2); any other rejection is `git_failed`, and nothing is overwritten either way. Then, when the Lead Branch's tip is no longer R' and descends from it (merges landed during the Gate), rule P with that tip, so they reach the pull request; its failure is `git_failed`, R' being pushed already (the Lead runs `pr push`).

  If the base has not moved, the rebase is a no-op (R' = S) and only the Gate and the push happen; this is also how Jobs Merged into an `open` PR reach its pull request when an earlier push failed (cpd 1 §11, kept). Active Attempts based on the old Lead SHA are unaffected; they rebase at their own merge (t3 F10). A merge's own push between step 6 and step 9 is rejected (the remote does not have R' yet) and appends a `push_failed` that step 9 makes moot.

  Rejected (hs-d243): R deciding by the reflog entry `cpd: merge <plan>/<attempt>` instead of ancestry, which would make guard U unnecessary but tie R's verdict to the reflog's existence and expiry.
- **`pr push [--pr <key>]`** (hs-d139, hs-n52): publishes the Lead Branch of an `open` PR without rebasing, a plain push, never forced; how Jobs Merged into an `open` PR reach its pull request when the branch carries a merge of the base (which `pr rebase` would drop) or when the remote has not moved. The PR must be `open` (else `illegal_transition`). No R first (§7.4):
  1. `G = git -C <repo> rev-parse refs/heads/<lead branch>`;
  2. **the Gate** on G, the Gate on a snapshot steps 1–5: `gate_failed` (exit 3), `gate_error` (exit 1), with `details.tail` and `details.log`. Nothing is pushed on a failure; a PR merged during the Gate is `illegal_transition`;
  3. rule P with G: `git -C <lead worktree> push --porcelain origin G:refs/heads/<lead branch>` (no `-u`, no force, no lease). A rejection whose remote tip does not contain G (the remote moved: a `pr rebase` pushed, or someone else) is `push_rejected` (exit 3) with git's stderr in `details.stderr`; the Lead then runs `pr rebase`. A remote that already contains G is a success that pushed nothing. Any other git failure is `git_failed`.

  No rows and no Event. A G the remote already has, or has a descendant of, still runs the Gate and pushes nothing (`pushed` false). The output carries `sha` (G), `pushed` and `remote`, the remote branch's tip after the push (hs-d243).

## 8. Configuration

### 8.1 File and format

The machine config is **`$CPD_HOME/config.toml`**, managed by the User's dotfiles (requirements §6, kept).

TOML, because:
- it allows comments, and every configured value that deviates from a default should say why next to it;
- repos already describe their setup and Gate in TOML (`.config/wt.toml`), so the User reads one format;
- Rust parses it into typed structs with unknown keys rejected (serde `deny_unknown_fields`).

cpd 1 used JSON (`config.json`), which has no comments; the two files coexisted in `$CPD_HOME` until the switch-over, which removed cpd 1 and its `config.json`.

Rules:
- The file is required, because `machine` has no default; a missing file is `config_missing` (exit 1).
- Every other key has the default below, built into the binary. The file holds only `version`, `machine` and real deviations.
- Unknown keys, wrong types or out-of-range values are `config_invalid` (exit 1), with the key path in `details`.
- `cpd config` prints the effective configuration (defaults merged with the file), with each value's source.

### 8.2 Keys

```toml
version = 1                  # required; the schema of this file
machine = "hs"               # required; names this machine in Context record ids (§2.1)

concurrency = 3              # Attempts not ended, machine-wide (§3.4)
retry_cap = 2                # automatic Attempts are capped at 1 + retry_cap (§9); Attempts whose driver never started do not count
lead_tier = "O3"
manager_tier = "O5"          # the Tier of a Context's Manager (§3.1, hs-d133)
context_distill_tier = "S2"  # the Tier of the distiller, the pi run that writes a Context digest (§4.6, hs-d194)
lead_pi_packages = ["npm:@plannotator/pi-extension@0.28.17", "npm:@juicesharp/rpiv-ask-user-question@2.13.8"]
pi_adapter = ""              # "" = <dir of the cpd binary>/../share/cpd/pi
plannotator_ports = "19432-19441"  # inclusive "<a>-<b>", a <= b, else config_invalid; one port per Plan's review (§6.5)

[tiers]                      # pi model per Tier letter, "<provider>/<model-id>"
O = "github-copilot/claude-opus-5.5"
S = "github-copilot/claude-sonnet-5.5"
H = "github-copilot/claude-haiku-4.5"

[timeouts]
attempt_minutes = 30         # active time per Attempt, from `running` (§3.4 A4)
stall_seconds = 1200         # a Worker idle in one tool call this long has that call's processes killed (§3.4 A4); 0 = off
ask_minutes = 60             # an Escalation expires after this
setup_minutes = 15           # pre-start
gate_minutes = 30            # pre-commit and pre-merge
store_busy_ms = 5000         # SQLite busy timeout (§4.2)
fork_quiet_seconds = 5       # fork-when-idle (§6.7)
distill_minutes = 15         # a distill run kills its pi after this (§4.6)

[watch]
tick_seconds = 5
pr_poll_seconds = 5             # one GitHub search of every open PR of the User (E5, §3.7, hs-d97); one point a poll, 720 an hour
error_repeat_seconds = 600   # the same `error` Event at most once per this (§3.7)

[inbox]
stuck_job_minutes = 10     # a failed Job becomes a `job-stuck` item after this (E8, §2.2)

[events]
poll_ms = 200                # events follow (§5.4)

[tmux]
socket = "cpd"               # tmux -L <socket>: the cockpit's server
cockpit_session = "cpd"
work_session = "cpd-work"    # the hidden session for Workers and watch
```

| Key | Type | Default | Constraint |
|---|---|---|---|
| `version` | int | required | `1` |
| `machine` | text | required | `^[a-z][a-z0-9]{0,7}$` |
| `concurrency` | int | `3` | ≥ 1 |
| `retry_cap` | int | `2` | ≥ 0 |
| `lead_tier` | Tier | `O3` | valid Tier (§9) |
| `manager_tier` | Tier | `O5` | valid Tier (§9); the Tier of a Context's Manager (§3.1, hs-d133) |
| `context_distill_tier` | Tier | `S2` | valid Tier (§9); the Tier of the distiller (§4.6, hs-d194) |
| `lead_pi_packages` | [text] | the two packages above | non-empty strings |
| `pi_adapter` | path | `""` (beside the binary) | must contain `package.json` |
| `tiers.O`, `tiers.S`, `tiers.H` | text | above | `^[^/\s]+/\S+$` |
| `timeouts.*` | int | above | ≥ 1, except `timeouts.stall_seconds` |
| `timeouts.stall_seconds` | int | `1200` | ≥ 0; `0` disables the stall kill (§3.4 A4). Why (hs-n107, hs-d195): a Worker's tool call that never exits (a command run without `timeout`) left pi waiting and its session file unwritten, and the Attempt ran to `attempt_minutes` for nothing; killing that call's processes gives pi the tool's failure and the Worker continues |
| `watch.*` | int | above | ≥ 1 |
| `inbox.stuck_job_minutes` | int | `10` | ≥ 1 |
| `events.poll_ms` | int | `200` | 20–10000 |
| `tmux.socket`, `tmux.cockpit_session`, `tmux.work_session` | text | above | `^[A-Za-z0-9_-]{1,32}$`; the two sessions differ |

Removed by R19: `default_review_tier` and `review_mode` (there is no Reviewer). Removed by hs-d243: `timeouts.lock_wait_seconds` (there is no per-repo lock to wait for, §7); a file that still sets it is `config_invalid`, as any unknown key. Removed from cpd 1 §5: `herdrSession` (herdr is dropped) and `askTimeoutMinutes`, `attemptTimeoutMinutes`, `tickSeconds`, `prPollSeconds`, which moved into tables. `CPD_HOME` stays an environment variable, not a key.

## 9. Tiers

Kept from cpd 1 §7, with one change in how pi's catalog is read.

- **Syntax** `^(O|S|H)([0-5])$`. Effort to pi level: `0` off, `1` low, `2` medium, `3` high, `4` xhigh, `5` max. pi argument: `--model <tiers[L]>:<level>`. `minimal` is never used.
- **Valid efforts per model** come from pi's own catalog, never from a table kept by hand (requirements §3; pi silently clamps unsupported levels, so cpd rejects invalid Tiers itself):
  - locate pi as cpd 1 does: realpath of `pi` on `PATH`, then `<agentDir>/install/current-version`, then `<agentDir>/install/releases/<version>/node_modules/@earendil-works/{pi-coding-agent,pi-ai}/dist/index.js`;
  - **change:** cpd 2 has no Node of its own, so it runs a small embedded ES module with **pi's Node**, the one pi's launcher uses: `${XDG_DATA_HOME:-~/.local/share}/pi-node/current/bin/node` when it exists, else `node` on `PATH` (verified in the launcher script of pi 1.2.0). The module calls `ModelRuntime.create({allowModelNetwork: false})`, `getModel(provider, id)` and pi-ai's `getSupportedThinkingLevels`, and prints JSON;
  - the `node` call has a deadline of 30 s (§1.2 `tool_timeout`, `details.tool` = `node`);
  - the result is cached in `tier_catalog` per pi version and refreshed when `current-version` changes; an unknown model is `tier_model_unknown`.
- **Ladder** `next(t)`: the same model at effort + 1 if valid; otherwise the next model in `H → S → O` at the lowest valid effort ≥ the current one; otherwise none. Examples: `H1→H2`, `H3→S3`, `S4→S5`, `S5→O5`, `O4→O5`, `O5→none`.
- **Attempt Tier:** Attempt 1 uses the Job's Tier; each automatic Attempt n uses `next` applied n − 1 times from the Job's Tier. Automatic Attempts are capped at `1 + retry_cap` and stop early when `next` is none. An Attempt whose driver never started (`driver_pid` null, §3.4) is not counted in the cap and does not advance the Tier: the next Attempt uses the same Tier. `job retry --tier <T>` sets an explicit Tier for the next Attempt (`origin = retry`), and the ladder continues from it, with a fresh cap.
- **Defaults:** Lead `O3` (`lead_tier`); Manager `O5` (`manager_tier`); the distiller `S2` (`context_distill_tier`, §4.6); the contract Jobs ran at `O3` (R13). There is no review Tier (R19).
- `cpd tiers` prints `{ok, pi_version, tiers: {O: {model, efforts: [..]}, S: …, H: …}}`.

## 10. Traceability

| Decision | Where it is implemented |
|---|---|
| R1 SQLite inside the binary; reads without a lock; own history for the tamper check | §1.1, §4.1–§4.4 (`history`, triggers, schema check, tamper report), §5.4 |
| R2 rewrite with a new contract; CLI first with JSON output | this document; §1.1, §1.2; changes from cpd 1 stated where they occur |
| R3 v1 stopped for good; its work is reference only | §1.4 (cpd 1's files untouched); nothing here depends on `cpd/v1` |
| R4 drop worktrunk; cpd uses git directly | §7.1, §7.2, §7.4, §7.6 |
| R5 the repo defines setup and Gate in `.config/wt.toml` | §7.3, §7.5 |
| R6 Cargo workspace: `cli/` next to `tui/` (since the switch-over, three crates: `core/`, `cli/`, `tui/`) | §1.1 |
| R7 running Workflows finish on TypeScript cpd; no Ledger migration | §1.4, §4.5 |
| R8 outline agreed, then the full contract written by a Worker | this document and `commands.md` (Job `contract-commands`) |
| R9 the Gate extends `npm run verify` with cargo | §7.3 (the Gate is whatever `pre-merge` runs; the repo's `pre-merge` stays `npm run verify`, which follow-up 1 extends) |
| R10 outline approved with D1–D5 | §1–§9; D1–D5 below |
| R11 one big Plan, one PR | §2.2 Plan, §3.1; generalised by R34: a Plan of one PR is the R11 case |
| R12 the binary was installed as `cpd2` during development (history: since the switch-over it is `cpd`) | §1.1; every command in this document is written `cpd` |
| R13 the full contract at Tier O3, review S4 | §9 (defaults); the Job that wrote this. Its review Tier no longer exists in cpd 2 (R19) |
| R35 the Lead Branch on the remote: `plan lock` reuses or creates and pushes the Lead Branch, every merge pushes it (hs-d30, hs-d42); since R34 per PR branch, at each PR's start (A5) | §3.1 (`plan → dispatch` row), §3.8 (the start), §7.2, §7.4 (push after step 8, R2), §5.2 `error` (`push_failed`); `commands.md` §4.8, §9.1 |
| R34 Plans across repos: a Plan has one or more PRs, each with its own repo, base, Lead Branch, Lead worktree, Gate and pull request (hs-d28, hs-d29, hs-d36, hs-a2, hs-d43, hs-d44, hs-d45) | A4 PR object: §2.1 (PR key), §2.2 PR; A5 naming: §2.1, §7.1; A6 storage: §4.4 (`prs`, `pr_needs`, `jobs.pr`), §4.5 migration 4; A7 Jobs in one PR, Dependencies inside it: §2.2 Job and Dependency, §2.4, §4.4 (`dependencies_same_pr`); A8 PR statuses and the lock: §3.1, §3.8; A9 the engine starts PRs: §3.7 E7, §3.8, §5.2 `pr-started`, §5.3; A10 phases and polling: §3.1, §3.7 E4–E5, §7.6; A11 a PR closed unmerged: §3.8, §7.6; A12 no `--repo`, the Lead's cwd: §2.2 Context record, §2.5, §3.1, §4.6, §6.2, §6.4, §6.5; A13 `pr add`, `pr edit`, `pr cancel`, `pr start`: §2.3, §2.5, §3.8; A14 `--pr`, `plan show`: §1.3, §2.5, §7.6; A15 the plan view: §2.5; A16 the cpd-lead skill: not in this document (`pi/skills/cpd-lead/`); `commands.md` (Job `r34-commands`) |
| The PR poller: one engine poll every 5 s of all the User's open PRs by one GraphQL search through `gh`, stored in `pull_requests`; `pr-feedback` only to the owning Lead; `pr view` for the detail (hs-d65, hs-d72, hs-d96, hs-d97, hs-d98, hs-d107; `docs/research/gh-interface.md`) | §1.3, §2.2 (Pull request, PR), §2.4, §2.5, §3.1, §3.7 E5, §3.8, §4.4 (`pull_requests`, `prs`), §4.5 migration 6, §5.1, §5.2 (`pr-feedback`, `error` `gh_rate_limited`), §7.6 (`pr poll`, `pr view`), §8.2 (`pr_poll_seconds`); `commands.md` §1.2, §1.3, §2, §4.2, §4.3, §9.2, §9.8, §13.2, §17 |
| B1–B4 forks are siblings under their Context; `job add --review` makes a review Job whose Worker is a fork of the Lead (hs-d70, User) | B1: §2.2 Session, §6.8; `commands.md` §13.2, §17. B2: §1.4, §2.2 Job (Review Job), §3.2, §3.4 (A review Job's Attempt), §4.4 (`jobs.review`), §4.5 (migration 4), §5.2 `job-added`, §6.5, §7.4 (A review Job's merge); `commands.md` §5.1, §5.7, §6.1, §16.1. B3: §2.2 Review Job; the cpd-lead skill is Job `r34-skill`'s. B4: §6.7, §6.8; `commands.md` §10.3 |
| R14 the Plan starts from `main` after lead-fork's PR | §3.1 (`plan lock` creates the Lead Branch from `origin/<base>`), §7.2 |
| R15 Contexts follow ADR-0011 and ADR-0012 (the grounded Lead is **superseded** by hs-d92, hs-d129: the Manager replaces it, §3.1, §4.5 migration 0010): the grounded Lead is a fork taken at the first Plan's `grounding → plan`, parked and never typed into; later forks get the newer records; the Context store was typed JSONL records in git, indexed by SQLite (superseded by hs-d23 and hs-a1: records in the database, §4.6) | §2.2 Context and Context record, §3.1, §4.6, §6.7 |
| R17 a Reviewer is a fork of the Lead | **superseded for cpd 2 by R19**: cpd 2 has no Reviewer, so nothing implements it |
| R19 no Job-level Gate or Reviewer: the Gate runs only at the Plan level, before the PR opens (`pr open`) and before `pr rebase` pushes; the review is the PR's own, on GitHub | §1.1, §1.2, §1.4, §2.2–§2.4, §3.2 (no `in_review`), §3.4 (no review statuses, outcomes or causes; A6–A7 removed; outcome `merged`), §3.6, §3.7, §4.4, §5.1–§5.3, §6.2–§6.5, §7.3, §7.4 (step 6 removed), §7.6, §8.2, §9; `commands.md` §20 lists every changed section |
| R20 at submit: the mechanical checks plus the quick check, the `pre-commit` hook; no `pre-commit` table means no quick check, with no fallback to `pre-merge` | §7.3 (hook table, `attempt submit` preconditions) |
| Cockpit parity (User, 2026-10-15): cpd 2's cockpit matches the running cpd 1 cockpit exactly: layout, elements, text, colours, keys, mouse and behaviour | §6.1 (options, border titles, click binding, conditional `pane-died` hook), §6.2 (Worker pane options), §6.3, §6.4 (`session close`); `commands.md` §10.9, §13, §17, §19 amendments 12–21; checklist `cockpit-parity.md` |
| R36 the StatusBar's left holds action buttons only, its right read-only information; the Event log is the `logs` view, opened by the `▤` button, not part of the Dispatch column | `commands.md` §13.2 (StatusBar, `logs`), §17, §19 amendment 21 |
| The Manager role (Plan manager B3: hs-d83..hs-d86, hs-d92, hs-d129, hs-d133, hs-d154; B3-A1..A5) | §2.2 Context, Context record (`brief`), Session (`manager`), Archived; §3.1 (The Context's Manager), §3.6; §4.4, §4.5 (migration 0010); §5.2, §5.3 (digest); §6.2, §6.4, §6.5; §8.2 (`manager_tier`) |
| Plumbing (hs-d139, hs-d140, hs-a3): the merge step skips its rebase when the Lead tip is already in the Attempt; `pr push`; `events follow` ends on store errors and re-execs after an install; `job add` re-arms `all-merged`; short Session ids | §1.2 (`push_rejected`, `session_ambiguous`), §2.2 PR, §3.7 E4, §5.2 `all-merged`, §5.4, §7.4 step 5, §7.6 `pr push`; `commands.md` §1.3, §5.1, §9.9, §10 (every command that takes a Session), §11.2; `docs/implementing.md` |
| Named migrations and an additive window (hs-a4, hs-d178 D1–D4): migrations are named files; the schema hash stays pinned in code over a canonical form; lookup tables for Event and record kinds; installs refuse a build whose migrations are not all on `origin/main`; `docs/research/migrations.md` is the design | §1.2 (`migration_changed`, `migration_order`), §3.4, §3.7, §6.7 (the guards), §4.3 (canonical hash, the window check), §4.4 (`schema_migrations`, the lookup tables), §4.5, §4.6, §5.2, §5.4; commands.md §1.2 (`--version`), §1.3, §11.2, §14.1 |
| The distilled Context (hs-d182, hs-d192, hs-d194, hs-n111; D1–D7 of PR `distill`, Plan cross-repo; the pilot of Epic B, hs-n95) | D1 the digest files: §1.4, §2.2 Context record, §4.6; D2 records in force: §4.6; D3 validation: §1.2 (`distill_invalid`), §4.6, §5.2 `error`; D4 the engine rebuild: §1.2 (`distill_running`), §3.7 E9, §6.4; D5 the distiller: §4.6, §6.5, §8.2 (`context_distill_tier`, `timeouts.distill_minutes`), §9; D6 consumers: §3.1, §4.6, §6.5; D7: §4.6 "Nothing is lost"; `commands.md` §3.3, §3.12, §4.1, §3.10; `docs/implementing.md` |
| No per-repo lock (hs-d243, User, 2026-10-19; design `docs/research/no-repo-lock.md`): no lock between PRs or Plans; inside one PR, ref and status compare-and-swaps, rule S, rule P, guard U and a gate worktree per Gate run | §1.2 (`lock_timeout`), §1.4 (`repo-<sha1>.lock` removed, `pr-<key>.pre-start.log`), §3.4 (`dirty_target` no longer produced), §3.7 E2, §7 (G1 no `.git/config` writes, G2 ref-lock timeouts), §7.1 (gate worktree, the Lead worktree a view), §7.2 (`--no-track`, the clean-up), §7.3 (`pre-merge` in a gate worktree, `branch`), §7.4 (step 3 removed, steps 8–9, rule S, rule P, R), §7.6 (the Gate on a snapshot, `pr open`, `pr rebase` with guard U, `pr push`), §8.2 (`lock_wait_seconds` removed); `commands.md` §4.8, §6.4, §9.1, §9.2, §9.3, §9.7, §9.9; `docs/implementing.md` (`sha1`) |
| D1 every command prints one JSON value; exit codes | §1.2 |
| D2 `plan new --context X` forks the Context's grounded Lead (**superseded** by hs-d92: `plan new` starts a fresh Lead, the grounded Lead is retired) | §2.2 Context, §3.1 (The grounded Lead is retired), §4.5 migration 0010 |
| D3 `workflow` becomes `plan` | §1.3 (`--plan`, `CPD_PLAN`), §2.2 Plan, CONTEXT.md |
| D4 Events are the single channel | §5, §3.6 (agent states as Events), §6.2 (no state in tmux options) |
| D5 one `cpd watch` per machine | §3.7, §6.4, §6.6 |
| ADR-0011 Contexts and Plans; a new Plan forks the Context's grounded Lead (**superseded** by hs-d92 for the fork; Contexts and Plans stand) | §2.2, §3.1 |
| ADR-0012 the Context store: typed records in a git repository, rendered for Leads (**superseded** by hs-d23 and hs-a1: records in the database, Contexts machine-local; Plan manager B1) | §2.2 Context record, §4.6 |
