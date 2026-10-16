# cpd 2: command reference

Status: **normative for every cpd 2 implementation Job**, under [contract.md](contract.md): where the two disagree, contract.md wins and this file is fixed. Vocabulary: [CONTEXT.md](../CONTEXT.md). This file uses the contract's names, states, tables and Event kinds exactly; "§n" without a prefix means contract.md §n, and "cpd 1 §n" means `docs/archive/cpd1/cli-contract.md` §n.

It specifies every command of outline §3 (synopsis, flags, preconditions with their error and exit codes, effects, JSON output, idempotence), the pi adapter's protocol, the Worker contract, what the TUI reads, and `doctor`.

Evidence labels as in contract.md: **verified** (run on the homeserver on 2026-10-14), **docs** (read in the named tool's documentation or source), **unverified** (a design assumption the implementation Job must check).

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
11. [`events`](#11-events)
12. [`watch`](#12-watch)
13. [`tui`](#13-tui)
14. [`doctor`, `config`, `tiers`](#14-doctor-config-tiers)
15. [The pi adapter's protocol](#15-the-pi-adapters-protocol)
16. [The Worker contract](#16-the-worker-contract)
17. [What the TUI reads](#17-what-the-tui-reads)
18. [Changes from cpd 1](#18-changes-from-cpd-1)
19. [Amendments to contract.md](#19-amendments-to-contractmd)
20. [Sections changed by R19 and R20](#20-sections-changed-by-r19-and-r20)

## 1. Conventions

### 1.1 How each command is written

Every command section has the same parts:

- **Synopsis**, then a table of every flag. `<x>` is required, `[x]` optional, `a | b` exclusive. A repeated flag says so.
- **Preconditions**, checked in the order listed; the first that fails is the result. Each row gives the error `code` and the exit code (§1.2). The common refusals of §1.3 apply to every command and are not repeated.
- **Effects**: **rows** written (table and columns), **Events** appended (kinds of §5.2), **tmux** and **git** actions, and files under `$CPD_HOME` (§1.4). "One transaction" means one write transaction with its `txns` row, its `history` rows and its Events (§4.2). External work (git, tmux, hooks, `gh`, pi) is never inside it.
- **Output**: one JSON example of success. Every failure is the envelope of §1.2, `{"ok": false, "error": {"code", "message", "details"}}`; `details` is listed where it carries something the caller acts on.
- **Idempotent**: what a rerun does. The default of §1.3 is `"already": true`; a command that differs says so.

Commands marked **internal** are started by cpd itself (in a tmux pane or detached) and are not meant for agents or the User. They are documented so their implementation Jobs have a contract.

### 1.2 Output and exit codes

Kept from §1.2 (D1). Every command prints exactly one JSON value on stdout. Three kinds of command own a terminal instead, and are the only exceptions (amendment 10, §19):

| Command | What it prints | Why |
|---|---|---|
| `cpd events follow` | one Event per line (§5.4) | the one streaming command (§1.2) |
| `cpd session run`, `cpd tui view` (internal) | they run pi or draw a ratatui view in their pane | they are the programs inside cockpit panes (§6.2), not commands an agent calls. If they fail before taking the terminal, they print the failure envelope and exit with its code |
| `cpd tui --attach` | nothing on success: it replaces itself with `tmux attach` | the approved outline says `tui` "builds and attaches the cockpit"; without `--attach`, `cpd tui` prints JSON like every command |

### 1.3 Selection and common refusals

- **Plan:** `--plan <id>`, else `CPD_PLAN`, else `plan_required` (exit 2) (§1.3).
- **Context:** `--context <id>`, else the Context of the selected Plan when a Plan is selected (by `--plan` or `CPD_PLAN`), else `context_required` (exit 2). New: a Lead's pane has `CPD_PLAN` set (§6.5), so it never needs to name its Context.
- **Session:** `--session <id>`, else `CPD_SESSION`, else `session_required` (exit 2). `CPD_SESSION` is set in every pane cpd starts (§6.5).
- **PR** (R34, §1.3): `--pr <key>`, within the selected Plan, used by `job add`, `pr open`, `pr poll` and `pr rebase`. It may be omitted when the Plan has exactly one PR that is not `cancelled`, which is then selected; with two or more, `pr_required` (exit 2); with none, `pr_required` too, its message naming `cpd pr add`. A key that is not one of the Plan's PRs is `pr_not_found` (exit 4). Exceptions, stated at each command: `pr poll` without `--pr` polls every `open` PR (§9.2), and `job edit --pr` changes the Job's PR, so omitting it changes nothing (§5.2). `pr add`, `pr edit`, `pr cancel` and `pr start` take the key as their positional argument.
- **Job, Attempt, Escalation, Plan review:** a positional id, within the selected Plan (§2.1).

Common refusals, which apply to every command and are not repeated in the sections below:

| Code | Exit | When |
|---|---|---|
| `usage` | 2 | unknown flag, missing argument, flag value of the wrong shape, exclusive flags together |
| `plan_required`, `context_required`, `session_required` | 2 | the command needs one and none was selected |
| `pr_required` | 2 | the command needs a PR, `--pr` is omitted, and the Plan does not have exactly one PR that is not `cancelled` (R34) |
| `invalid_context_id`, `invalid_plan_id`, `invalid_pr_id`, `invalid_job_id`, `invalid_attempt_id`, `invalid_escalation_id`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id` | 2 | an id does not match §2.1 |
| `config_missing`, `config_invalid` | 1 | §8.1 |
| `store_too_new`, `migration_failed` | 1 | §4.5 |
| `store_tampered` | 1 | a write command, when the schema check fails (§4.3); read commands report it in `tamper` instead |
| `store_busy` | 5 | no write lock within `store_busy_ms` (§4.2) |
| `context_not_found`, `plan_not_found`, `pr_not_found`, `job_not_found`, `attempt_not_found`, `escalation_not_found`, `review_not_found`, `session_not_found` | 4 | the selected or named object does not exist. (A Context record that names a missing record is `record_not_found`, exit 3, §4.6) |
| `cas_lost` | 3 | a compare-and-swap found the state changed since it was read (§4.2); rerun the command |
| `illegal_transition` | 3 | a transition not in §3, refused by the command or by a trigger |
| `locked_job_tampered` | 3 | a Locked Job's fingerprint does not match (§2.3) |
| `git_failed`, `tmux_failed`, `gh_failed` | 1 | the tool exited non-zero where the command did not expect it; `details` has `argv`, `exit` and the last 60 lines of stderr |

### 1.4 Files written by commands

The `$CPD_HOME` layout is §1.4. Commands in this file write four files the contract's layout does not list (amendment 4, §19); each is beside its peers and named so it cannot collide:

| File | Written by | Read by |
|---|---|---|
| `run/<plan>/lead-message.md` | `plan new` | the Plan's Lead `session run` (its first message) |
| `run/<plan>/plan-<review>.md` | `plan review` | the pi adapter, which opens it in plannotator (§15.5) |
| `run/<plan>/messages/<session>.md` | `session fork --message-file` copies the file here | that Session's `session run` |
| `run/<plan>/reviews/<job>.md` | a review Job's Worker (B2, contract §1.4) | the Lead (and the User) |

### 1.5 Text arguments

Free text (`--title`, `--reason`, Context record text, questions and answers) is one argument, 1 to 20 000 characters, with no NUL; else `usage`. Names (`--name`, `session rename`) are 1–80 characters with no control characters, else `invalid_name` (exit 2) (cpd 1 §11 `session rename`, kept). Files given by path (`--spec`, `--brief-file`, `--feedback-file`, `--message-file`) must be readable UTF-8, else `file_unreadable` (exit 2).

## 2. Command index

"Caller" is who runs it in normal use; anyone may run any command. "Writes" says whether it opens a write transaction.

| Command | Caller | Writes | Contract |
|---|---|---|---|
| `context new` | Lead, User | yes | §2.2, §4.6 |
| `context list`, `context show` | Lead, TUI | no | §2.2 |
| `context decide`, `note`, `ask`, `answer` | Lead | yes | §4.6 |
| `context sync` | engine E6, User | yes | §4.6 |
| `plan new` | User (TUI), Lead | yes | §3.1, §6.7 |
| `plan list`, `plan show` | Lead, TUI | no | |
| `plan phase plan` | Lead | yes | §3.1 |
| `plan render` | Lead | no | §2.3 |
| `plan review` | Lead (tool `cpd_plan_review`) | yes | §2.2 Plan review |
| `plan decide` | pi adapter | yes | §2.2 Plan review |
| `plan lock` | Lead | yes | §3.1, §7.2 |
| `job add`, `edit`, `cancel`, `retry` | Lead | yes | §3.2 |
| `job dispatch` | Lead | yes | §3.4 |
| `job list`, `job show` | Lead, TUI | no | |
| `attempt submit` | Worker | yes | §7.3 |
| `attempt read`, `show`, `list` | Lead, TUI | no | |
| `attempt run` (internal) | E3, `job dispatch` | yes | §3.4 |
| `ask` | Worker | yes | §3.5 |
| `answer` | Lead | yes | §3.5 |
| `pr open`, `pr rebase` | Lead | yes | §7.3, §7.6 |
| `pr add`, `edit`, `cancel` | Lead | yes | §2.3, §2.5, §3.8 |
| `pr poll` | engine E5 | yes | §7.6 |
| `session list`, `session show` | TUI, Lead | no | |
| `session fork`, `open`, `park`, `close`, `rename` | User (TUI), Lead | yes | §6.4, §6.7 |
| `session state` | pi adapter | yes | §3.6 |
| `session run` (internal) | cockpit panes | yes | §6.7 |
| `events list`, `events follow` | pi adapter, TUI, scripts | no | §5.4 |
| `watch` | `plan lock`, `tui`, User | yes | §3.7, §6.6 |
| `tui`, `tui view` (internal) | User | no | §6.2 |
| `doctor`, `config`, `tiers` | User | `tiers` refreshes its cache | §8, §9 |

`pr start` (R34, §9.7; Lead; it writes; §2.5, §3.8) is not in this table yet: the Job that implements it adds its row in the same commit, as for `session close` (§10.9), because `every_documented_command_exists` (`cli/tests/command_index.rs`) fails for a row whose command does not exist.

## 3. `context`

A Context and its records (§2.2, §4.6). Every record write follows §4.6 exactly: under `locks/context-<id>.lock`, validate, append one line to `contexts/<id>/records/<machine>.jsonl`, `git add`, `git commit`, then one transaction inserting the `context_records` row and appending `context-record` `{record, kind, machine, seq}`.

**git identity.** cpd commits in a Context store with `git -c user.name="cpd <machine>" -c user.email=cpd@<machine>.invalid`, so a commit never depends on the machine's git configuration. (New: cpd 1 had no Context store.)

**Record ids.** `<machine>-<letter><n>` with `n` = 1 + the highest `n` already in this Context's index for this machine and letter (§2.1). The Context lock makes it race-free on one machine; only this machine writes its letter sequence.

**Retries.** A record write is recognised as a retry, and returns the existing record with `"already": true`, when this machine's latest record in the Context has the same `kind`, `text`, `supersedes`, `question` and `by.session`. Any other write appends a new record; records are never edited.

### 3.1 `context new`

```
cpd context new <id> --title <text> [--jira <key>]
```

| Flag | Meaning |
|---|---|
| `<id>` | Context id (§2.1) |
| `--title <text>` | required |
| `--jira <key>` | Jira Epic key, stored only (`^[A-Z][A-Z0-9]+-[0-9]+$`, else `usage`) |

| Precondition | Code | Exit |
|---|---|---|
| no Context row with this id, unless it has the same title and Jira key | `context_exists` | 3 |
| `contexts/<id>/` does not exist (for example cloned from another machine but not imported); `details.next` is `cpd context sync --context <id>` | `context_exists` | 3 |

Effects:
- **git:** under the Context lock: `git init -q -b main contexts/<id>`; write `context.json` (§4.6), `.gitattributes` (`records/*.jsonl merge=union`), `.gitignore` (`*.tmp`) and an empty `records/<machine>.jsonl`; `git add -A`; `git commit -q -m "context <id>: <title>"`. No remote: the User adds `origin` (§4.6).
- **Rows**, one transaction: `contexts` (`created_on` = `machine`, `lead_session` and `grounded_seq` null). **Event** `context-created` `{title, jira, created_on}`.
- **tmux:** none.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null, "created_at": "2026-10-14T21:10:00.000Z", "created_on": "hs", "dir": "/home/clayton/.cpd/contexts/building-cpd"}}
```

Idempotent: yes. The same id, title and Jira key return `"already": true`. If the git commit happened but the row did not (a crash), the second rule refuses and `context sync --context <id>` imports it.

### 3.2 `context list`

```
cpd context list
```

No flags, no preconditions, no effects (one read transaction).

```json
{"ok": true, "contexts": [
  {"id": "building-cpd", "title": "Building cpd", "jira": null, "grounded": true, "lead_session": "6f1c…", "grounded_seq": 41,
   "records": 57, "open_questions": 2, "sync_failing": false,
   "plans": [{"id": "cpd2", "title": "cpd in Rust", "phase": "dispatch", "lead_session": "596d007f-…"}]}],
 "events_cursor": 48213}
```

`events_cursor` is the highest Event id visible in the same read transaction (§17). Idempotent: read-only.

### 3.3 `context show`

```
cpd context show [--context <id>] [--since <seq>]
```

| Flag | Meaning |
|---|---|
| `--since <seq>` | only records whose index position `seq` is greater (exclusive, like `events --since`); what a forked Lead's first message holds (§3.1) |

Effects: none. Output: the Context, its records in index order, and `markdown`, the rendering of §2.2 "Context record" (title and Jira key; Plans; active decisions; superseded decisions with their successor; open questions; answered questions with their answers; notes; each item with its id and date). With `--since`, `markdown` renders only those records, in the same order of sections.

**Plans** (R34, A12): each `plan` record is listed with the repos of its Plan's PRs that are not `cancelled`, read from this machine's database (`prs.repo`) in the same read transaction, each repo once, in PR key order; a Plan of another machine, or one with no PR yet, shows none. The record itself carries no repo (§2.2; a `repo` in a record written before R34 is ignored, §4.6). In `records`, a `plan` record's `plan` is `{id, machine, jira}`, and the entry has `repos`, the list just described.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null, "grounded": true, "grounded_seq": 41},
 "records": [{"seq": 42, "id": "hs-d7", "kind": "decision", "at": "2026-10-14T21:12:00.000Z", "machine": "hs",
              "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "One engine per machine.", "supersedes": "hs-d2"}],
 "markdown": "# Building cpd\n\n## Plans\n…"}
```

Idempotent: read-only.

### 3.4 `context decide`, `context note`, `context ask`

```
cpd context decide <text> [--supersedes <record>] [--context <id>]
cpd context note <text> [--context <id>]
cpd context ask <question> [--context <id>]
```

| Flag | Meaning |
|---|---|
| `<text>`, `<question>` | the record's `text` (§1.5) |
| `--supersedes <record>` | `decide` only: the id of the decision this one replaces |

| Precondition | Code | Exit |
|---|---|---|
| `--supersedes` names a record of this Context whose kind is `decision` | `record_not_found` | 3 |

Effects: the record write above, with kind `decision` (letter `d`), `note` (`n`) or `question` (`q`); `by` = `{session: CPD_SESSION or PI_SESSION_ID or null, plan: the selected Plan or null}`. **Event** `context-record`. No tmux.

```json
{"ok": true, "record": {"id": "hs-d8", "kind": "decision", "at": "2026-10-14T21:30:00.000Z", "machine": "hs", "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "…", "supersedes": "hs-d2"}, "seq": 58}
```

Idempotent: a retry is recognised (above); otherwise each call appends a record, by design (the store is append-only).

### 3.5 `context answer`

```
cpd context answer <question> <text> [--context <id>]
```

`<question>` is the record id of the question.

| Precondition | Code | Exit |
|---|---|---|
| `<question>` is a record of this Context of kind `question` | `record_not_found` | 3 |
| no answer in this machine's index names it yet | `question_answered` | 3 |

Effects: the record write with kind `answer` (letter `a`) and `question` = `<question>`; **Event** `context-record`.

```json
{"ok": true, "record": {"id": "hs-a2", "kind": "answer", "at": "…", "machine": "hs", "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "Yes, per machine.", "question": "hs-q3"}, "seq": 59}
```

Idempotent: a retry is recognised; a second, different answer is refused. Two machines may still answer the same question before they sync; `context show` then lists both answers.

### 3.6 `context sync`

```
cpd context sync [--context <id>]
```

Without `--context`, every directory under `contexts/` is synced, in name order. Run by engine E6 every `context_sync_seconds` for Contexts with a remote, and by the User.

| Precondition | Code | Exit |
|---|---|---|
| `contexts/<id>/` is a git repository with a valid `context.json` whose `id` is `<id>` | `context_store_invalid` | 1 |
| the merge did not conflict (§4.6 step 2); `details.files` lists the files | `context_sync_conflict` | 3 |
| fetch, merge and push succeeded within 3 rounds (§4.6 step 3) | `context_sync_failed` | 1 |

Effects (§4.6, under the Context lock):
- **git:** no `origin`: nothing. Else `git fetch origin`, `git merge --no-edit origin/main` (`git merge --abort` on a conflict), `git push origin main`, repeated at most 3 times.
- **Rows**, one transaction per Context: a Context first seen (no row): `contexts` from `context.json` and **Event** `context-created`; each record not yet indexed: `context_records` and **Event** `context-record`; `sync_failing = 0` on success. A failure from E6 sets `sync_failing = 1` and, only when it was 0, appends `error` `{source: "context sync", code, message, details}` for the Context (§4.6). A failure run by hand only reports it.

```json
{"ok": true, "contexts": [{"id": "building-cpd", "synced": true, "imported": 3, "pushed": true},
                          {"id": "home-lab", "synced": false, "reason": "no_remote", "imported": 0, "pushed": false}]}
```

Without `--context`, one failing Context makes the whole result a failure (exit of the first failure) whose `details.contexts` holds every Context's entry, failures included as `{"id", "error": {code, message}}`; the others are still synced. Idempotent: yes; a second run imports and pushes nothing.

## 4. `plan`

A Plan (§2.2) and its phases (§3.1). Replaces cpd 1's `workflow` group (D3) and `plan` group.

### 4.1 `plan new`

```
cpd plan new <id> --context <c> --title <text> [--jira <key>] [--name <name>] [--brief-file <file>]
```

A Plan has no repo (R34, A12): its PRs are added later with `pr add` (§9.4), and its Lead starts in the caller's cwd, the directory `plan new` runs from (usually `~`; §6.4). `--repo` and `--base` are gone: either is `usage` (exit 2).

| Flag | Meaning |
|---|---|
| `<id>` | Plan id (§2.1) |
| `--context <c>` | required: the Plan's Context (D2) |
| `--title <text>` | required |
| `--jira <key>` | Jira task key, stored only |
| `--name <name>` | the Lead's pi display name; default `<id>` |
| `--brief-file <file>` | the User's brief for the Lead: its text goes into the Lead's first message |

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| no Plan with this id, unless every flag matches it (then the result is `already`) | `plan_exists` | 3 |

Effects, in this order:
1. **Rows**, one transaction: `sessions` (new UUID v4; `role = lead`; `context`; `plan = <id>`; `name`; `cwd` = the realpath of the caller's cwd; `tier = lead_tier`; `forked_from` = the Context's `lead_session` or null; `state = stopped`); `plans` (`phase = grounding`, `lead_session` = the new Session); no `prs` row. **Events** `plan-created` `{title, jira, lead_session, forked_from, cwd}` and `session-registered` `{role, name, forked_from, tier}`.
2. **Context store:** the record write of §3 with kind `plan` (letter `p`), `text` = the title, `plan` = `{id, machine, jira}`; **Event** `context-record`.
3. **First message**, written to `run/<id>/lead-message.md`: a header (`You are the Lead of Plan <id> "<title>" in Context <c>. Phase grounding: follow the cpd-lead skill.`), the brief file's text if given, then `context show --context <c>` rendered as markdown, with `--since <grounded_seq>` when the Lead is a fork (§3.1).
4. **tmux:** start the server if it is not running (§6.1) and the cockpit session `cpd` if it is missing; then `new-window -d -t =cpd: -n park-<id> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -e CPD_PLAN=<id> -P -F '#{window_id} #{pane_id}' -- cpd session run <session>`, and `set -p -t <pane> @cpd_session <session>`. One transaction: `sessions.tmux_pane`; **Event** `session-moved` `{pane, window, place: "parked"}`.
5. In the pane, `session run` (§10.8) forks the grounded Lead when it is idle (§6.7), or starts a fresh Lead, with the first message.

When the cockpit session is missing, plan new creates it with the new Lead's park window as its first window; cpd tui builds main later.

```json
{"ok": true, "plan": {"id": "pay-4521", "context": "payments", "title": "Retry failed payouts", "jira": null, "phase": "grounding", "lead_session": "8d0e…", "prs": []},
 "lead": {"session": "8d0e…", "name": "pay-4521", "forked_from": "6f1c…", "cwd": "/home/clayton", "pane": "%41", "window": "@17"},
 "record": "hs-p4"}
```

Idempotent: yes. With the same flags it returns `"already": true` and redoes any step that did not happen: the `plan` record when the index has none for this Plan, and step 4 when the Lead is `stopped` with no pane (in the stored `cwd`). The caller's cwd is not a flag: a rerun from another directory is still `already` and keeps the stored `cwd`. Different flags: `plan_exists`.

### 4.2 `plan list`

```
cpd plan list [--context <id>] [--phase <phase>]
```

`--phase` is one of §3.1's phases. Effects: none. Each Plan lists its PRs (R34) in key order, `cancelled` ones included, with the fields the Dispatch column shows (§13.2, §17): `key`, `repo`, `status`, `pr` (`{number, url, opened_at}` or null), `pr_title`, `pr_review_decision`, `pr_checks`.

```json
{"ok": true, "plans": [{"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust", "phase": "dispatch", "lead_session": "596d007f-…",
  "counts": {"draft": 0, "queued": 2, "running": 1, "merged": 2, "failed": 0, "cancelled": 0},
  "prs": [{"key": "cpd", "repo": "/home/clayton/projects/cpd", "status": "dispatch", "pr": null, "pr_title": null, "pr_review_decision": null, "pr_checks": null}]}],
 "events_cursor": 48213}
```

Idempotent: read-only.

### 4.3 `plan show`

```
cpd plan show [--plan <id>]
```

Effects: none; one read transaction, so the result is one consistent snapshot. It is the TUI's snapshot of a Plan (§17).

Output: every field of the Plan (§2.2); `prs` (R34, A14), every PR of the Plan in key order, `cancelled` ones included, each with its fields (§2.2 PR: `key`, `repo`, `base`, `needs`, `status`, `lead_branch`, `lead_worktree`, `pr`, `pr_seen`, `pr_title`, `pr_review_decision`, `pr_checks`, `all_merged_at`, `start_failed_at`, `created_at`, `started_at`, `merged_at`) and `jobs`, the keys of its Jobs in `seq` order; `jobs` with each Job's fields (Spec included, `pr` and `review` among them), `needs`, and its Attempts (`id`, `number`, `tier`, `origin`, `status`, `outcome`, `cause`, `worker_session`, `started_at`, `ended_at`); `escalations` that are `open`; the latest Plan review; `counts`; `tamper` (the tamper report of §4.3: `{kind: "schema" | "fingerprint" | "history", …}`); `events_cursor`.

```json
{"ok": true,
 "plan": {"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust", "jira": null, "phase": "dispatch", "lead_session": "596d007f-…", "created_at": "…", "locked_at": "…", "done_at": null},
 "prs": [{"key": "cpd", "repo": "/home/clayton/projects/cpd", "base": "main", "needs": [], "status": "dispatch", "lead_branch": "cpd/cpd2/cpd", "lead_worktree": "/home/clayton/projects/cpd.cpd2.cpd",
          "pr": null, "pr_seen": {"checks": {}, "reviewIds": [], "commentIds": []}, "pr_title": null, "pr_review_decision": null, "pr_checks": null, "all_merged_at": null, "start_failed_at": null,
          "created_at": "…", "started_at": "…", "merged_at": null, "jobs": ["contract-core", "contract-commands"]},
         {"key": "web", "repo": "/home/clayton/projects/web", "base": "main", "needs": ["cpd"], "status": "waiting", "lead_branch": "cpd/cpd2/web", "lead_worktree": null,
          "pr": null, "pr_seen": {"checks": {}, "reviewIds": [], "commentIds": []}, "pr_title": null, "pr_review_decision": null, "pr_checks": null, "all_merged_at": null, "start_failed_at": null,
          "created_at": "…", "started_at": null, "merged_at": null, "jobs": ["web-client"]}],
 "jobs": [{"key": "contract-commands", "pr": "cpd", "title": "cpd 2 contract: every command", "status": "running", "tier": "O3", "appended": false, "needs": ["contract-core"],
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

Effects: exactly §3.1 "The Context's grounded Lead":
- **Rows**, one transaction: `plans.phase = plan`; the compare-and-swap on `contexts.lead_session` and `grounded_seq`; only if it changed a row, a `sessions` row for the grounded Lead (`role = lead`, `plan = null`, `forked_from` = this Plan's Lead, `name` = `<context> (grounded)`, `cwd` = this Lead's cwd, `tier = lead_tier`, `stopped`). **Events** `plan-phase` `{from: "grounding", to: "plan"}`, and with the new Session `session-registered` and `context-grounded` `{seq, forked_from}`.
- **tmux**, after the commit and only with the new Session: `new-window -d -t =cpd: -n ctx-<context> … -- cpd session run <grounded id>`, `@cpd_session` on its pane; one transaction: `tmux_pane` and `session-moved` `{place: "parked"}`. Its `session run` waits until this Lead is idle, then forks it with no first message (§6.7).

```json
{"ok": true, "plan": "pay-4521", "phase": "plan", "grounded_lead": {"session": "6f1c…", "pane": "%44", "window": "@19"}}
```

`grounded_lead` is null when the Context already had one. Idempotent: yes; in phase `plan` it returns `"already": true`, and creates the `ctx-<context>` window when the Context's grounded Lead was forked from this Plan's Lead and has no pane (a crash between the two transactions).

### 4.5 `plan render`

```
cpd plan render [--plan <id>]
```

Effects: writes `run/<plan>/plan.md` (§1.4), nothing else. Content, kept from cpd 1 §11 `plan render` and grouped by PR (R34): title, Context and phase; a table of the PRs that are not `cancelled`, with key, repo, base and needs, and, when there are two or more, a Mermaid `graph LR` of their needs; then one section per PR, in the order of the needs graph (ties by key), headed `PR <key>`, with its repo, base and needs, a table of its Jobs that are not `cancelled` with key, title, Tier, status, needs and touches, a Mermaid `graph TD` of its Jobs' Dependencies, and one subsection per Job with its Spec (goal, context, touches, acceptance, out of scope). A Job's section is under its PR only. GitHub-flavoured Markdown only: tables, code and Mermaid, no footnotes, no raw HTML (t4). Output: the path, the plan fingerprint (§2.3: Jobs and PRs), the number of `draft` Jobs and the number of PRs that are not `cancelled`.

```json
{"ok": true, "path": "/home/clayton/.cpd/run/pay-4521/plan.md", "fingerprint": "9f86d081…", "draft_jobs": 6, "prs": 2}
```

Idempotent: yes (the same PRs and Jobs render the same file). Change: cpd 1's `--out` is dropped; the file has one place.

### 4.6 `plan review`

```
cpd plan review [--plan <id>]
```

Requests the User's approval of the draft Jobs (§2.2 Plan review). The Lead runs it through its tool `cpd_plan_review` or directly; either way the pi adapter opens plannotator when the `plan-review` Event arrives (§15.5), so the review opens from any caller.

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan` | `illegal_phase` | 3 |
| at least one Job is `draft` | `no_draft_jobs` | 3 |

Effects:
- **Files:** renders the plan to `run/<plan>/plan.md` and copies it to `run/<plan>/plan-<review>.md`, so a later render cannot change what the User reviews.
- **Rows**, one transaction: `plan_reviews` (`id = r<k>`, `k` = 1 + the highest, `fingerprint` = the plan fingerprint, `status = pending`). **Event** `plan-review` `{review, fingerprint, status: "pending"}`.

```json
{"ok": true, "review": {"id": "r3", "fingerprint": "9f86d081…", "status": "pending", "requested_at": "…"}, "path": "/home/clayton/.cpd/run/pay-4521/plan-r3.md",
 "next": "The User reviews the plan in plannotator. Stop and wait for the decision message."}
```

Idempotent: yes. When the latest review is `pending` for the current fingerprint, it returns that review with `"already": true` and writes nothing. When the Jobs or PRs changed since, it requests a new review; the older one stays `pending` and no longer matters, because `plan lock` reads only the latest (§2.3).

### 4.7 `plan decide`

```
cpd plan decide <review> (--approve | --reject) [--feedback-file <file>] [--plan <id>]
```

Records the User's decision. Called by the pi adapter with plannotator's result (§15.5); replaces cpd 1's `plan decision` (cpd 1 §11), whose `--pending` step is now `plan review`. The cpd-lead skill forbids the Lead to run it.

| Precondition | Code | Exit |
|---|---|---|
| the review is `pending` (decided the same way: `already`) | `review_decided` | 3 |

Effects: **rows**, one transaction: `plan_reviews.status`, `feedback` (the file's text, or null), `decided_at`. **Event** `plan-review` `{review, fingerprint, status, feedback}`.

```json
{"ok": true, "review": {"id": "r3", "status": "approved", "fingerprint": "9f86d081…", "feedback": null, "decided_at": "…"}, "current": true, "next": "cpd plan lock"}
```

`current` says whether the review's fingerprint is still the plan fingerprint; `next` is `cpd plan lock` only when approved and current. Idempotent: yes, as stated.

### 4.8 `plan lock`

```
cpd plan lock [--plan <id>]
```

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan` (`dispatch`: `already`) | `illegal_transition` | 3 |
| at least one Job is `draft` | `no_draft_jobs` | 3 |
| the latest Plan review is `approved` for the current plan fingerprint (§2.3: Jobs and PRs) | `plan_not_approved` | 3 |
| at least one PR is not `cancelled`, and each such PR has at least one `draft` Job; `details.prs` lists the empty PRs (none at all: `details.next` = `cpd pr add`) | `pr_empty` | 3 |
| the Dependency graph of the draft Jobs is acyclic, and so is the needs graph of the PRs | `dependency_cycle` | 3 |
| for each PR with no needs: its Lead worktree path does not exist, except a leftover of an interrupted lock (a worktree of the PR's repo on its Lead Branch), which is removed first; `details.pr` names the PR | `worktree_exists` | 3 |
| for each PR with no needs: its Lead Branch does not exist in its repo, except that leftover's branch, and neither does `cpd/<plan>` when the Lead Branch is `cpd/<plan>/<key>` (§2.1); `details.pr` | `branch_exists` | 3 |
| for each PR with no needs: `origin/<lead branch>` does not exist, or `origin/<base>` is its ancestor (§7.2); a repo with no `origin` has no remote check; `details.pr` | `branch_exists` | 3 |
| for each PR with no needs: `pre-start` passed in its new Lead worktree (§7.3); `details.pr`, `details.tail`, `details.log` | `setup_failed` | 1 |

Effects (§3.1, §3.8, §7.2; R34, A8): every PR's `draft` Jobs are approved at once, and the PRs with no needs **start**, all or nothing; the PRs with needs stay `waiting` until engine E7 (or `pr start`, §9.7) starts them once their needs are `merged`.
- **git**, for each PR with no needs, in its repo (§7.2, R35 per PR branch, A5): `fetch origin <base>`; `ls-remote --exit-code --heads origin <lead branch>`. When it is on the remote and `origin/<base>` is its ancestor (`merge-base --is-ancestor`): reuse it, `fetch origin <lead branch>` and `worktree add -b <lead branch> <lead worktree> origin/<lead branch>`. When it is absent: `worktree add -b <lead branch> <lead worktree> origin/<base>`. A repo with no `origin` (A3): no remote check and no push, `worktree add` from `<base>`. Then `pre-start` in the worktree, output in `run/<plan>/logs/lead-<key>.pre-start.log`. Every started PR's worktree is created and its `pre-start` run before anything is pushed; if one fails, every worktree and branch the lock created is removed, nothing is pushed, nothing in the database changes, and the lock fails with that PR's refusal. When all have passed, each new Lead Branch is pushed, `push -u origin <lead branch>` from its Lead worktree. A failed push does not fail the lock: one `error` Event with code `push_failed` and `details.pr` is appended per failed PR, and the next merge or `pr open` pushes again.
- **Rows**, one transaction, with a compare-and-swap on `phase = 'plan'` and on the review read: `plans.phase = dispatch`, `locked_at`; every `draft` Job of every PR `queued` with its Locked-Job fingerprint; for each started PR, with a compare-and-swap on `status = 'waiting'`: `status = dispatch`, `lead_worktree`, `started_at`. **Events** `plan-phase` `{from: "plan", to: "dispatch", jobs: [key], prs: [key]}` (`prs`: the PRs started) and one `pr-started` `{pr, repo, base, lead_branch, lead_worktree, base_sha, reused, pushed, by: "lock"}` per started PR. If the transaction fails, the worktrees and branches are removed.
- **tmux:** `watch --ensure` (§12): starts `cpd watch` in `cpd-work:watch` when `locks/watch.lock` is free.

`prs` lists every PR that is not `cancelled`, in key order: a started PR with its Lead worktree, `base_sha`, `reused` and `pushed` (R35); a `waiting` one with its `needs`, and null for the rest.

```json
{"ok": true, "plan": "pay-4521", "phase": "dispatch", "jobs": ["api-schema", "worker", "ui"],
 "prs": [{"key": "api", "status": "dispatch", "needs": [], "lead_branch": "cpd/pay-4521/api", "lead_worktree": "/home/clayton/projects/pay.pay-4521.api", "base_sha": "4b825dc…", "reused": false, "pushed": true},
         {"key": "ui", "status": "waiting", "needs": ["api"], "lead_branch": "cpd/pay-4521/ui", "lead_worktree": null, "base_sha": null, "reused": null, "pushed": null}],
 "watch": {"started": true, "pane": "%3"}}
```

If the engine cannot be started, the Plan stays locked and the result is `watch_start_failed` (exit 1) with `details.next` = `cpd watch --ensure` (cpd 1's `watch_pane_failed`, renamed). Idempotent: yes; in phase `dispatch` it returns `"already": true` and still runs `watch --ensure`.

## 5. `job`

Jobs, Specs and Dependencies (§2.2, §3.2). Kept from cpd 1 §11 "Jobs" except where stated.

**Spec file** (`--spec`), JSON, every field required, unknown fields refused (`spec_invalid`, exit 2, with the JSON path in `details`):

```json
{"goal": "…", "context": "…", "touches": ["docs/commands.md"], "acceptance": ["…"], "out_of_scope": ["…"]}
```

`touches` and `acceptance` have at least one non-empty string; `out_of_scope` may be empty. Change: cpd 1's `outOfScope` is now `out_of_scope`, the column's name.

**Tier flag** (`--tier`): `^(O|S|H)([0-5])$` with an effort the letter's model supports (§9), else `tier_invalid` (exit 2), with `details.efforts` listing the valid ones.

### 5.1 `job add`

```
cpd job add <key> --title <text> --tier <T> --spec <file> [--pr <key>] [--needs <k1,k2> | --review] [--plan <id>]
```

| Flag | Meaning |
|---|---|
| `<key>` | Job key (§2.1). Change: positional, like every other `job` command (cpd 1 had `--key`) |
| `--pr <key>` | the Job's PR (R34, A14), selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one |
| `--needs <k1,k2>` | Dependencies, comma-separated, no duplicates (`usage`); Jobs of the same PR only (A7) |
| `--review` | a **review Job** (B2, contract §2.2): its Worker is a fork of the Plan's Lead, it commits nothing and writes `run/<plan>/reviews/<key>.md`. Its Dependencies are **set** to every Job of its PR that exists at that moment and is not `cancelled`, so `--needs` with `--review` is `usage` (exit 2). Jobs added later do not become its Dependencies. The Spec file is as for any Job; its `touches` is informational for a review Job (*decided*: the Lead lists `run/<plan>/reviews/<key>.md`) |

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan`, `dispatch` or `pr` | `illegal_phase` | 3 |
| the PR is not `merged` or `cancelled` (§3.2) | `pr_ended` | 3 |
| no Job with this key, unless its Locked fields (`pr` included) and needs are identical (then `already`) | `job_key_exists` | 3 |
| every `needs` key is a Job of the Plan | `needs_unknown` | 3 |
| every `needs` Job is in the same PR; `details.jobs` lists those that are not | `needs_other_pr` | 3 |
| no `needs` Job is `cancelled` or `failed`; with `--review`, no Job of the PR is `failed` (retry or cancel it first; `details.jobs` lists them), because a Dependency on a `failed` Job is refused | `needs_unavailable` | 3 |

Effects: **rows**, one transaction: `jobs` (`pr` = the PR's key; `review` = 1 with `--review`; `status = draft`, `appended = 0`, no fingerprint, in phase `plan`; `status = queued`, `appended = 1`, fingerprint stored, in `dispatch` or `pr`); one `dependencies` row per need (with `--review`, per Job of the PR, as the Dependencies are set in the same transaction). **Event** `job-added` `{pr, status, appended, tier, needs, review}`. No cycle is possible: nothing needs a new Job yet. An appended Job of a `waiting` PR is not Ready until its PR starts (§3.2); one of an `open` PR reaches its pull request through `pr rebase --pr <key>` (§9.3).

```json
{"ok": true, "job": {"key": "api-schema", "pr": "api", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false}}
```

Idempotent: yes, as stated. `--review` is repeatable like any add: an identical call (same Locked fields, `review` included, same Dependencies) is `already`; a call after another Job was added to the PR differs in its Dependencies and is `job_key_exists`. `job edit` has no `--review` and refuses `--needs` and `--no-needs` on a review Job with `usage` (*decided*: its Dependencies are fixed by the add; to change them, cancel it and add a new one).

### 5.2 `job edit`

```
cpd job edit <key> [--title <text>] [--tier <T>] [--spec <file>] [--pr <key>] [--needs <k1,k2> | --no-needs] [--plan <id>]
```

`--no-needs` removes every Dependency (cpd 1 had no way to). `--pr <key>` moves the draft Job to another PR (R34, A14); it is a field flag, so §1.3's default does not apply: without it the Job keeps its PR. Dependencies stay inside one PR (A7): a Job that changes PR keeps none of its old needs, so give `--needs` with Jobs of the new PR, or `--no-needs`, and no other Job may need it.

| Precondition | Code | Exit |
|---|---|---|
| at least one field flag | `nothing_to_edit` | 2 |
| the Job is `draft` | `job_locked` | 3 |
| `--pr` names a PR of the Plan | `pr_not_found` | 4 |
| that PR is not `cancelled` | `pr_ended` | 3 |
| the needs exist and are usable | `needs_unknown`, `needs_unavailable` | 3 |
| every Dependency after the edit links two Jobs of the same PR: the needs are in the Job's PR (the new one with `--pr`), and with `--pr` the Job keeps no old needs and no Job needs it; `details.jobs` lists the Jobs on the other side (§4.4 `dependencies_same_pr`, `jobs_pr_dependencies`) | `needs_other_pr` | 3 |
| the new needs make no cycle | `dependency_cycle` | 3 |

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

Effects (§3.2 "Cancel"):
- `draft`, `queued` or `failed`: one transaction, `status = cancelled`, `cancel_reason`, `cancel_requested_at`; **Event** `job-cancelled` `{reason}`.
- `running`: one transaction, `cancel_reason` and `cancel_requested_at` only; **Event** `job-cancel-requested` `{reason}`. Engine E2 sets the Attempt's `stop_request = cancelled`, and the driver or E2 ends it `cancelled` at its next safe point (§3.4), which appends `job-cancelled`. No tmux action by this command.

```json
{"ok": true, "job": "worker", "status": "running", "cancel_requested": true}
```

Idempotent: yes; a `cancelled` Job, or one with a request already recorded, returns `"already": true` (the first reason is kept). Dependents stay blocked (cpd 1 §11, kept).

### 5.4 `job retry`

```
cpd job retry <key> --tier <T> [--plan <id>]
```

| Precondition | Code | Exit |
|---|---|---|
| the Job is `failed` (or `queued` with this `retry_tier` and no Attempt since: `already`) | `job_not_failed` | 3 |

Effects: **rows**, one transaction: `status = queued`, `retry_tier = <T>`, `failed_cause = null`. **Event** `job-retried` `{tier}`. The next Attempt has `origin = retry` and this Tier, and the ladder restarts from it with a fresh cap (§9).

```json
{"ok": true, "job": "worker", "status": "queued", "tier": "O4"}
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
| fewer than `concurrency` Attempts are not `ended` | `no_slot` | 3 |

Effects (§3.4 "The driver lock"): take `locks/attempt-<plan>--<attempt>.lock`; **rows**, one transaction: `attempts` (`starting`, Tier from §9, `origin`), Job `running`; **Event** `attempt-started` `{number, tier, origin}`; then spawn `cpd attempt run <attempt> --plan <plan>` with `setsid`, fd 3 = the lock, stderr to `run/<plan>/logs/attempt-<attempt>.log`, and record `driver_pid` (a data write, no Event). If the spawn fails, the command ends the Attempt `failed` / `crashed` (`attempt-retry` or `job-failed`) and returns `spawn_failed` (exit 1).

```json
{"ok": true, "attempt": "api-schema-a1", "tier": "S2", "origin": "ladder", "pid": 81234}
```

Idempotent: no. A second call finds the Job `running` and is refused `job_not_ready`.

### 5.6 `job list`

```
cpd job list [--status <status>] [--plan <id>]
```

`--status` is one of §3.2's statuses and may repeat. Effects: none.

```json
{"ok": true, "jobs": [{"key": "api-schema", "pr": "api", "title": "…", "status": "merged", "tier": "S2", "appended": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee…"}]}
```

### 5.7 `job show`

```
cpd job show <key> [--plan <id>]
```

Effects: none. Output: every field of the Job (§2.2; `review: true` for a review Job, B2), `needs`, `needed_by`, and every Attempt with all its fields (`check_tail`, `metrics`) and its Escalations (cpd 1 §11, kept).

```json
{"ok": true, "job": {"key": "worker", "pr": "api", "status": "failed", "failed_cause": "timed_out", "tier": "S2", "…": "…"}, "needs": ["api-schema"], "needed_by": ["ui"],
 "attempts": [{"id": "worker-a1", "status": "ended", "outcome": "failed", "cause": "timed_out", "check_tail": null, "escalations": []}]}
```

Both are read-only.

## 6. `attempt`

### 6.1 `attempt submit`

```
cpd attempt submit <attempt> [--plan <id>]
```

Run by the Worker, from its worktree. Preconditions are §7.3's list, in that order, after the Attempt's own state:

| Precondition | Code | Exit |
|---|---|---|
| the Attempt is `running` (already `merging` or later with `submitted_sha` = the worktree's HEAD: `already`) | `attempt_not_running` | 3 |
| the caller's cwd is inside the Attempt worktree, on the Attempt branch | `wrong_worktree` | 3 |
| clean tree, untracked files included | `dirty_tree` | 3 |
| at least one commit ahead of `base_sha`; **for a review Job (B2) instead**: no commit ahead of `base_sha` | `no_commits`; `review_commits` | 3 |
| (review Job only) `run/<plan>/reviews/<job>.md` exists and is not empty (contract §7.3, precondition 3) | `review_missing` | 3 |
| the tamper check passes (§7.5); `details.paths` lists the protected paths touched | `gate_tampered` | 3 |
| the quick check passes (skipped for a review Job, B2: nothing was changed to check; no `pre-commit` table: it passes without running, R20); `details.tail` is its last 60 lines, `details.log` the full log | `check_failed` | 3 |
| the quick check ran (not killed, not `hook_invalid`) | `check_error` | 1 |

Effects:
- **Hooks:** `pre-commit` only, in the worktree (§7.3); output to `run/<plan>/logs/<attempt>.pre-commit.log`. With no `pre-commit` table nothing runs, no log is written and `check_tail` stays null; `pre-merge` never runs at submit (R20).
- **Rows**, one transaction with a compare-and-swap on `status = 'running'` and on the HEAD checked: Attempt `merging`, `submitted_sha`, `submitted_at`, `check_tail`; the Job stays `running`. **Event** `attempt-status` `{from: "running", to: "merging"}`.
- The driver sees `merging` at A5 and starts the merge (A8, §7.4). For a review Job the merge changes nothing: the Attempt and the Job end `merged` with the Lead Branch unchanged and `merged_sha` its tip (contract §3.4, §7.4).

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

The Attempt driver, steps A1–A9 of §3.4, started detached by E3 or `job dispatch` with the driver lock as fd 3. Not for agents.

| Precondition | Code | Exit |
|---|---|---|
| fd 3 is the driver lock file (same device and inode) | `driver_lock_missing`, printed to its log; nothing is written (§3.4 step 4) | 1 |
| the Attempt is `starting` | `illegal_transition` | 3 |

Effects: everything of §3.4 A1–A9, §7.2, §7.3, §7.4: the worktree and `pre-start`; the rendered Worker Spec (§16.1); the Worker's window `cpd-work:<plan>--<attempt>` (`new-window` with the Worker's argv of §6.5, `@cpd_session`); the Session rows (`session-registered`, `session-moved` `{place: "work"}`); the nudge (`attempt-nudge`); the merge under the per-repo lock; the end (`metrics`, `kill-window`, worktree removal on `merged`). Each status change is one transaction with the Event of §3.4.

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
1. **Rows**, one transaction: `escalations` (`<attempt>-q<k>`, `open`, `asker_pid` = its own pid); the Worker Session `blocked` with label `cpd: question to the Lead`. **Events** `escalation` `{escalation, question}` (the Lead's adapter delivers it with a turn) and `session-state`.
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

A Plan's PRs (R34, §2.2 PR, §3.8) and their pull requests, through `gh` (§7.6). `gh` failures are `gh_failed` (exit 1) with its stderr tail. Every command here works on **one PR**: `pr open` and `pr rebase` select it by `--pr <key>` (§1.3: omitted, the Plan's one PR that is not `cancelled`); `pr add`, `pr edit`, `pr cancel` and `pr start` (§9.4–§9.7) take its key as their argument; `pr poll` without `--pr` polls every `open` PR. `<repo>`, `<base>`, `<lead branch>` and `<lead worktree>` are the selected PR's (§7); `gh` runs with cwd = its Lead worktree, so it addresses the PR's repo.

### 9.1 `pr open`

```
cpd pr open [--pr <key>] [--plan <id>]
```

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR to open (R34, A14), selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one. The Lead runs it on each PR's `all-merged` (`data.pr`) |

| Precondition | Code | Exit |
|---|---|---|
| the PR is `dispatch` (`open` or `merged`: `already`; `waiting` or `cancelled`) | `illegal_transition` | 3 |
| every Job of the PR `merged` or `cancelled`, at least one `merged` | `jobs_not_done` | 3 |
| the per-repo lock within `lock_wait_seconds` | `lock_timeout` | 5 |
| the Lead worktree is clean | `dirty_target` | 3 |
| the Gate passes (§7.3, §7.6 `pr open` step 2); `details.tail` is its last 60 lines, `details.log` the full log | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |

Effects:
- **Lock:** the per-repo lock of the PR's repo (§7.4), then R for orphaned merges; held until the push has finished.
- **Hooks:** the Gate, `pre-merge` from `<lead worktree>/.config/wt.toml`, in the PR's Lead worktree on its Lead Branch, before anything is pushed (R19); output to `run/<plan>/logs/pr-<key>.pre-merge.log`. On `gate_failed` or `gate_error` nothing is pushed, no row is written and the PR stays `dispatch`: the Lead appends fix Jobs to it (`job add --pr <key>`) and runs `pr open --pr <key>` again once they are Merged.
- **git:** `git -C <lead worktree> push -u origin <lead branch>`. The Lead Branch is usually on the remote already: the PR's start pushed it and every merge pushed it (§7.2, §7.4, R35), so this push is normally a no-op that sets the upstream, and it pushes again what an earlier `push_failed` left behind.
- **gh:** `gh pr list --head <lead branch> --state open --json number,url`; when a pull request exists (an earlier run died before its transaction) it is adopted; else `gh pr create --base <base> --head <lead branch> --title <title> --body <summary>`. `<title>` is the Plan's title when the Plan has one PR that is not `cancelled`, else `<plan title> (<key>)`; `<summary>` is the table of the PR's own Jobs and its Merged commits (cpd 1 §11, kept).
- **Rows**, one transaction, with a compare-and-swap on the PR's `status = 'dispatch'`: the PR `status = open`, `pr_number`, `pr_url`, `pr_opened_at`. **Event** `pr-opened` `{pr, number, url}`. When every PR that is not `cancelled` is now `open` or `merged` (A10), the same transaction sets `plans.phase = pr` and appends `plan-phase` `{from: "dispatch", to: "pr", prs: [{key, number, url}]}` (§3.1); otherwise the Plan stays in `dispatch`.

`phase` is the Plan's phase after the command.

```json
{"ok": true, "plan": "pay-4521", "phase": "dispatch", "pr": {"key": "api", "status": "open", "pr": {"number": 812, "url": "https://github.com/acme/pay/pull/812", "opened_at": "…"}}}
```

Idempotent: yes, as stated.

### 9.2 `pr poll`

```
cpd pr poll [--pr <key>] [--plan <id>]
```

Run by engine E5 every `pr_poll_seconds` as a subprocess, without `--pr`; also by hand.

| Flag | Meaning |
|---|---|
| `--pr <key>` | poll only this PR. Without it: every `open` PR of the Plan, in key order (§1.3's default does not apply) |

| Precondition | Code | Exit |
|---|---|---|
| the phase is `dispatch` or `pr` (A10; `done`: `already`) | `illegal_phase` | 3 |
| with `--pr`: the PR is `open` (`merged`: `already`) | `illegal_transition` | 3 |

Without `--pr`, a Plan with no `open` PR polls nothing and returns `prs: []`.

Effects, for each PR polled, in the PR's Lead worktree:
- **gh:** `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision` and `gh api repos/{owner}/{repo}/pulls/<n>/comments`, compared with the PR's `pr_seen` (cpd 1 §13, kept).
- **Rows**, one transaction per PR: its `pr_seen`, `pr_polled_at`, and `pr_title`, `pr_review_decision` (`none`, `approved`, `changes_requested`) and `pr_checks` (`none`, `passed`, `pending`, `failed`), set together on every poll (contract §7.6). They are what the Dispatch column shows (§13.2, §17) and append no Event of their own. New failing checks, reviews or comments, or `state = CLOSED` without a merge: **Event** `pr-feedback` (data of §5.2, with `pr`). Merged: the PR `status = merged`, `merged_at`, **Event** `pr-merged` `{pr, number, url, merged_at}`; when every PR that is not `cancelled` is then `merged`, the same transaction sets `plans.phase = done`, `done_at` and appends `plan-done` `{prs: [{key, number, url, merged_at}]}`, once (§3.1).
- **Starting the PRs that waited for it** (A9): `pr poll` does not start a PR itself. Engine E7, which runs after E5 in the same tick (§3.7), starts every `waiting` PR whose needs are now all `merged`, from a fresh `fetch origin <base>`; when `pr poll` was run by hand, the next tick does, or the Lead runs `pr start <key>` (§9.7).
- **A pull request closed without a merge** (A11): `pr-feedback` with `state: "CLOSED"`; the PR stays `open` in cpd, every PR that needs it stays `waiting`, and the Lead asks the User.
- **After `done`:** the Plan's remaining Attempt worktrees and branches, kept for debugging (§3.4 A9), are removed in every PR's repo (`git worktree remove --force`, `git branch -D`), and any of its Attempt windows killed. The Lead worktrees and the Lead Branches stay.

`prs` has one entry per PR polled, in key order.

```json
{"ok": true, "plan": "pay-4521", "phase": "dispatch",
 "prs": [{"pr": "api", "number": 812, "state": "MERGED", "merged": true, "title": "Retry failed payouts (api)", "review_decision": "approved", "checks": "passed", "changes": {"checks": [], "reviews": [], "comments": []}},
         {"pr": "ui", "number": 813, "state": "OPEN", "merged": false, "title": "Retry failed payouts (ui)", "review_decision": "none", "checks": "failed", "changes": {"checks": [{"name": "ci/test", "conclusion": "FAILURE"}], "reviews": [], "comments": []}}]}
```

Idempotent: yes; what was reported is in each PR's `pr_seen`, so a rerun reports nothing new.

### 9.3 `pr rebase`

```
cpd pr rebase [--pr <key>] [--plan <id>]
```

Rebases the PR's Lead Branch onto its base and pushes (§7.6); also how Jobs Merged into an `open` PR reach its pull request.

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR to rebase, selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one |

| Precondition | Code | Exit |
|---|---|---|
| the PR is `open` | `illegal_transition` | 3 |
| the per-repo lock within `lock_wait_seconds` | `lock_timeout` | 5 |
| the Lead worktree is clean | `dirty_target` | 3 |
| `git merge-tree` reports no conflict; `details.files` lists them | `rebase_conflict` | 3 |
| the Gate passes on the rebased Lead Branch (§7.3, §7.6 `pr rebase` step 4); `details.tail` is its last 60 lines, `details.log` the full log | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |

Effects: under the per-repo lock of the PR's repo, after R (§7.4): §7.6 steps 1–5 (`fetch`, `merge-tree`, `rebase`, the Gate, `push --force-with-lease`). The Gate is `pre-merge` from `<lead worktree>/.config/wt.toml`, run in the PR's Lead worktree after the rebase and before the push (R19), output to `run/<plan>/logs/pr-<key>.pre-merge.log`. On `gate_failed` or `gate_error` nothing is pushed; the rebased Lead Branch stays in the Lead worktree, and the Lead appends fix Jobs to the PR, then runs `pr rebase --pr <key>` again once they are Merged. **Rows**: none; the Ledger does not record the Lead Branch's SHA, so there is no Event. On an unexpected rebase conflict: `git rebase --abort` and `rebase_conflict`.

```json
{"ok": true, "plan": "pay-4521", "pr": "api", "rebased": true, "old": "4b825dc…", "new": "77aa01e…", "pushed": true}
```

Idempotent: yes; a base that has not moved gives `"rebased": false`, and only the Gate and the push run.

### 9.4 `pr add`

```
cpd pr add <key> --repo <path> [--base <branch>] [--needs <k1,k2>] [--plan <id>]
```

Adds a PR to the Plan, `waiting` (R34, A13, §3.8). In phases `grounding` and `plan` it is part of the plan fingerprint (§2.3), so the Plan needs a review that includes it; in `dispatch` it is **appended**, like `job add`, and engine E7 starts it once its needs are `merged` (at once when it has none, §3.7).

| Flag | Meaning |
|---|---|
| `<key>` | PR key (§2.1), chosen by the Lead, e.g. `schema`, `web` |
| `--repo <path>` | required: the PR's repo; stored as the realpath of its main worktree. Several PRs of one Plan may share a repo |
| `--base <branch>` | the branch the pull request targets; default the repo's `origin` HEAD branch, else `main` (cpd 1 §11 `workflow init`, kept) |
| `--needs <k1,k2>` | keys of other PRs of the Plan that must be `merged` before this one starts, comma-separated, no duplicates (`usage`) |

| Precondition | Code | Exit |
|---|---|---|
| the phase is `grounding`, `plan` or `dispatch` | `illegal_phase` | 3 |
| `--repo` is inside a git repository (`git -C <path> rev-parse --git-common-dir`) | `not_a_git_repo` | 3 |
| `<base>` exists: `refs/remotes/origin/<base>`, or `refs/heads/<base>` when the repo has no `origin` | `base_not_found` | 4 |
| no PR of the Plan has this key, `cancelled` ones included, unless its repo, base and needs equal the resolved flags (then `already`) | `pr_exists` | 3 |
| every need is a PR of the Plan; `details.needs` lists the unknown keys | `needs_unknown` | 3 |
| no need is `cancelled`; `details.needs` lists them | `needs_unavailable` | 3 |
| neither `refs/heads/cpd/<plan>/<key>` nor `refs/heads/cpd/<plan>` exists in the repo (§2.1); `details.branch` names it | `branch_exists` | 3 |

No cycle is possible: nothing needs a new PR yet.

Effects: **rows**, one transaction: `prs` (`status = waiting`, `repo`, `base`, `lead_branch = cpd/<plan>/<key>`, `created_txn`, `created_at`; `lead_worktree` null); one `pr_needs` row per need. **Event** `pr-added` `{pr, repo, base, needs, appended}`, `appended` true in phase `dispatch`. No git write and no tmux: the Lead Branch and Lead worktree are created when the PR starts (§3.8, §7.2).

```json
{"ok": true, "pr": {"key": "web", "repo": "/home/clayton/projects/web", "base": "main", "needs": ["api"], "status": "waiting", "lead_branch": "cpd/pay-4521/web", "appended": false}}
```

Idempotent: yes, as stated; the comparison uses the resolved repo and base, so a rerun without `--base` matches a PR whose base was defaulted to the same branch.

### 9.5 `pr edit`

```
cpd pr edit <key> [--repo <path>] [--base <branch>] [--needs <k1,k2> | --no-needs] [--plan <id>]
```

Changes a `waiting` PR before the lock (A13). `--needs` replaces the whole list; `--no-needs` removes every need. The PR's Jobs stay with it. A changed `--repo` keeps the stored base unless `--base` is given, and that base must exist in the new repo.

| Precondition | Code | Exit |
|---|---|---|
| at least one field flag | `nothing_to_edit` | 2 |
| the phase is `grounding` or `plan`; after the lock a PR is Locked (§4.4 `prs_locked`) | `pr_locked` | 3 |
| the PR is `waiting` (a `cancelled` one is terminal) | `illegal_transition` | 3 |
| `--repo` is inside a git repository | `not_a_git_repo` | 3 |
| the base exists in the PR's repo (the new one with `--repo`), as for `pr add` | `base_not_found` | 4 |
| every need is a PR of the Plan, other than this one | `needs_unknown` | 3 |
| no need is `cancelled` | `needs_unavailable` | 3 |
| the needs graph stays acyclic; `details.cycle` lists it | `dependency_cycle` | 3 |
| with `--repo`: neither `refs/heads/cpd/<plan>/<key>` nor `refs/heads/cpd/<plan>` exists in the new repo | `branch_exists` | 3 |

Effects: **rows**, one transaction: the changed `prs` columns; `pr_needs` rows deleted and inserted to match (§4.4 allows deletion only in `grounding` and `plan`). **Event** `pr-edited` `{pr, fields: [name]}`, listing only the fields whose value changed (`repo`, `base`, `needs`). The plan fingerprint changes with them (§2.3).

```json
{"ok": true, "pr": {"key": "web", "status": "waiting"}, "fields": ["base", "needs"]}
```

Idempotent: yes; when no value changes it writes nothing and returns `"already": true`.

### 9.6 `pr cancel`

```
cpd pr cancel <key> [--plan <id>]
```

Removes a PR from the Plan before the lock: `waiting → cancelled` (A13, §3.8). It keeps its key, so `pr add` cannot reuse it. Cancel its Jobs first (`job cancel`), and edit away the needs on it (`pr edit`).

| Precondition | Code | Exit |
|---|---|---|
| the phase is `grounding` or `plan` | `pr_locked` | 3 |
| the PR is `waiting` (`cancelled`: `already`) | `illegal_transition` | 3 |
| every Job of the PR is `cancelled`, and no PR that is not `cancelled` needs it; `details.jobs` and `details.prs` list what holds it | `pr_in_use` | 3 |

Effects: **rows**, one transaction: the PR `status = cancelled`. **Event** `pr-cancelled` `{pr}`. The plan fingerprint changes (§2.3: it covers the PRs that are not `cancelled`).

```json
{"ok": true, "pr": {"key": "web", "status": "cancelled"}}
```

Idempotent: yes, as stated.

### 9.7 `pr start`

```
cpd pr start <key> [--plan <id>]
```

Starts a `waiting` PR whose needs are all `merged` (A9, §3.8): the Lead's retry after engine E7 failed to start it (the `error` Event with code `pr_start_failed`), once the cause is fixed. It does what E7 does.

| Precondition | Code | Exit |
|---|---|---|
| the phase is `dispatch` | `illegal_phase` | 3 |
| the PR is `waiting` (`dispatch`, `open` or `merged`: `already`; `cancelled`) | `illegal_transition` | 3 |
| every need is `merged`; `details.needs` lists those that are not | `pr_not_ready` | 3 |
| its Lead worktree path does not exist, except a leftover of an interrupted start (a worktree of the PR's repo on its Lead Branch), which is removed first | `worktree_exists` | 3 |
| its Lead Branch does not exist in its repo, except that leftover's branch, and neither does `cpd/<plan>` when the Lead Branch is `cpd/<plan>/<key>` (§2.1) | `branch_exists` | 3 |
| `origin/<lead branch>` does not exist, or `origin/<base>` is its ancestor (§7.2) | `branch_exists` | 3 |
| the worktree, the remote check and `pre-start` succeeded; `details` is `{pr, step, tail?, log?}`, as in `pr_start_failed` | `setup_failed` | 1 |

Effects (§3.8 "The start", §7.2):
- **git**, in the PR's repo: `fetch origin <base>` (fresh, so the PR contains what its needs merged), the remote check and the reuse or creation of its Lead Branch exactly as `plan lock` (§4.8) does for one PR, then `pre-start` in the new Lead worktree, output in `run/<plan>/logs/lead-<key>.pre-start.log`; on failure the worktree and branch are removed, nothing in the database changes (`start_failed_at` keeps its value) and the result is `setup_failed`. After `pre-start` passed, a new Lead Branch is pushed (`push -u origin <lead branch>`); a failed push does not fail the start: one `error` Event with code `push_failed` and `details.pr`.
- **Rows**, one transaction, with a compare-and-swap on `status = 'waiting'`: `status = dispatch`, `lead_worktree`, `started_at`, `start_failed_at = null`. **Event** `pr-started` `{pr, repo, base, lead_branch, lead_worktree, base_sha, reused, pushed, by: "pr start"}`. The PR's `queued` Jobs are now Ready (§3.2) and engine E3 schedules them.

```json
{"ok": true, "pr": {"key": "ui", "status": "dispatch", "lead_branch": "cpd/pay-4521/ui", "lead_worktree": "/home/clayton/projects/pay.pay-4521.ui", "base_sha": "9c1e7a2…", "reused": false, "pushed": true}}
```

Idempotent: yes, as stated; a PR that E7 started meanwhile returns `"already": true`.

## 10. `session`

Sessions (§2.2, §3.6) and their tmux panes (§6). Every tmux call is `tmux -L <tmux.socket> …` with `TMUX` and `TMUX_PANE` removed (§6.1); a failing call is `tmux_failed` (exit 1). A Session's **place** is the `place` of its latest `session-moved` Event (`chat`, `parked`, `work` or `none`), or `none` when it has none.

**The chat slot.** **Amends contract.md §6.2** (the Lead's answer to Escalation `contract-commands-a1-q1`): the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat`. A pane with `@cpd_view = chat` is the **placeholder**: it runs `cpd tui view chat`, which reads `No Lead here: pick a Plan in the session tree`, and it holds the slot while no Lead is there. **Amends §6.2's list of `@cpd_view` values** with `chat`. Why: the slot must hold a pane before any Lead exists (the first `cpd tui` on a machine) and after its Lead is parked (`swap-pane` needs a pane to swap in). At most one placeholder exists, and only ever in the slot: `session park` creates it there (§10.5), and `session open` kills it with its window when a Lead takes the slot (§10.4).

### 10.1 `session list`

```
cpd session list [--context <id>] [--plan <id>] [--role <role>] [--state <state>]
```

Filters combine with AND; `--role` and `--state` may repeat. Without filters: every Session. Here `--context` and `--plan` are filters only: neither falls back to `CPD_PLAN`.

Effects: none; one read transaction.

```json
{"ok": true, "sessions": [
  {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "state": "blocked", "state_label": "cpd: Worker question", "state_at": "…",
   "place": "chat", "tmux_pane": "%12", "file": "/home/clayton/.pi/agent/sessions/…_596d007f-….jsonl", "forked_from": "6f1c…", "plan_lead": true, "grounded_lead": false, "tier": "O3", "cwd": "/home/clayton/projects/cpd"}],
 "events_cursor": 48213}
```

`plan_lead` is true for the Session that is its Plan's `lead_session`; `grounded_lead` for a Context's `lead_session`. Read-only.

### 10.2 `session show`

```
cpd session show [<id>]
```

`<id>` defaults to the selected Session (`CPD_SESSION`). Output: the fields of `session list` plus `created_at`, `registered_event` (the id of its `session-registered` Event, the pi adapter's first cursor, §15.2) and, for a Plan's Lead, `open_escalations` (ids). Effects: none.

```json
{"ok": true, "session": {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "state": "idle", "state_label": null, "place": "chat", "tmux_pane": "%12", "file": "…", "forked_from": "6f1c…", "plan_lead": true, "grounded_lead": false, "tier": "O3", "cwd": "/home/clayton/projects/cpd", "created_at": "…", "registered_event": 40117},
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
| `<source>` | the Session to fork; a Lead. This command is for **tangents the User starts** (a bug, an idea), B4; a review is a review Job (§5.1), not a fork by hand |
| `--plan <id>` | the new Session's Plan; default the source's Plan. The fork is a **side session** of the Plan: the Plan's `lead_session` does not change, so Plan Events still go to the Plan's Lead only (§15.2). It is drawn in the tree **directly under its Context**, after the Context's Plans, never under the Plan or under `<source>` (B1, §13.2) |
| `--name <name>` | default `<source name> (fork)`, cut to 80 characters |
| `--message-file <file>` | its first message, copied to `run/<plan>/messages/<session>.md`; without it the fork starts with no message |

| Precondition | Code | Exit |
|---|---|---|
| the source is a `lead` Session | `not_a_lead` | 3 |
| a Plan is known: `--plan`, else the source's Plan (a grounded Lead has none and is forked only by `plan new`) | `plan_required` | 2 |
| the cockpit's tmux session (`tmux.cockpit_session`) exists, checked before anything is written; `details.next` = `cpd tui` | `no_cockpit` | 3 |

Effects:
- **Rows**, one transaction: `sessions` (new UUID v4, `role = lead`, the Plan's Context, `forked_from = <source>`, `cwd` = the source's, `tier = lead_tier`, `stopped`). **Event** `session-registered`.
- **tmux:** `new-window -d -t =cpd: -n park-<label> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<id> -e CPD_PLAN=<plan> -- cpd session run <id>`; `@cpd_session`. One transaction: `tmux_pane`, **Event** `session-moved` `{place: "parked"}`.
- If the cockpit disappears between that check and `new-window`, the command fails with `tmux_failed` after the Session is registered (Escalation `rs-session-a1-q2`).

```json
{"ok": true, "session": {"id": "a41b…", "name": "cpd rust lead (fork)", "plan": "cpd2", "forked_from": "596d007f-…", "pane": "%52", "window": "@23"}}
```

Idempotent: no; each call registers a new Session.

### 10.4 `session open`

```
cpd session open <id>
```

Brings a Lead into the chat slot (§6.4).

| Precondition | Code | Exit |
|---|---|---|
| the Session is a `lead` | `not_a_lead` | 3 |
| it has a Plan (a grounded Lead is never typed into, §3.1) | `grounded_lead` | 3 |
| the cockpit's `main` window and its chat slot exist; `details.next` = `cpd tui` | `no_cockpit` | 3 |

Effects:
- In the chat slot already: nothing; `"already": true`.
- **tmux:** when the Session has no live pane (`stopped`, or its pane is gone): `new-window -d -t =cpd: -n park-<label> … -- cpd session run <id>` first, which resumes it (`pi --session <file>`, §6.5), and `tmux_pane` is recorded. Then `swap-pane -d -s <its pane> -t <chat slot pane>`, and `rename-window -t <window> park-<displaced label>` for the window that now holds the displaced Lead. When the displaced pane is the placeholder, `kill-pane` on it instead, which closes the now-empty park window with it, so placeholders never pile up in park windows.
- **Rows**, one transaction: **Events** `session-moved` `{place: "chat"}` for this Session and `{place: "parked"}` for the displaced one.

```json
{"ok": true, "session": "8d0e…", "pane": "%41", "displaced": "596d007f-…"}
```

Idempotent: yes.

### 10.5 `session park`

```
cpd session park [<id>]
```

Moves the Lead in the chat slot back into a parked window (§6.4). `<id>` defaults to the Session in the chat slot.

| Precondition | Code | Exit |
|---|---|---|
| the cockpit exists | `no_cockpit` | 3 |
| the Session is a `lead` | `not_a_lead` | 3 |

Effects:
- Not in the chat slot: nothing; `"already": true`.
- **tmux** (**amends contract.md §6.4**, which says only that park moves the Lead into a parked window): `new-window -d -t =cpd: -n park-<label> -P -F '#{window_id} #{pane_id}' -- cpd tui view chat` (a new park window holding a placeholder), `set -p -t <placeholder> @cpd_view chat`, then `swap-pane -d -s <placeholder> -t <chat slot pane>`. The Lead keeps its pid and is now alone in `park-<label>`; the placeholder fills the slot.
- **Rows**, one transaction: **Event** `session-moved` `{place: "parked"}`.

```json
{"ok": true, "session": "8d0e…", "pane": "%41", "window": "@31"}
```

Idempotent: yes.

### 10.6 `session state`

```
cpd session state <working|blocked|idle|stopped> [--label <text>] [--session <id>]
```

How the pi adapter reports its agent state (§3.6, D4). `--label` is required with `blocked` and refused with any other state (`usage`); 1–80 characters.

| Precondition | Code | Exit |
|---|---|---|
| the transition is in §3.6's table | `illegal_transition` | 3 |

Effects: when the state and label are unchanged, nothing (`"already": true`, no Event, §3.6). Otherwise **rows**, one transaction: `state`, `state_label` (null unless `blocked`), `state_at`. **Event** `session-state` `{from, to, label}`. No tmux.

```json
{"ok": true, "session": "596d007f-…", "state": "blocked", "label": "cpd: Worker question", "from": "working"}
```

Idempotent: yes.

### 10.7 `session rename`

```
cpd session rename <name> [--session <id>]
```

cpd 1 §11 `session rename`, kept; the queue file becomes an Event.

Effects: unchanged name: `"already": true`, nothing written. Otherwise **rows**, one transaction: `sessions.name`. **Event** `session-renamed` `{name}`, addressed to the Session (`session` set), which its adapter applies with `pi.setSessionName` (§15.4). No tmux.

```json
{"ok": true, "session": "596d007f-…", "name": "cpd rust lead"}
```

Idempotent: yes.

### 10.8 `session run` (internal)

```
cpd session run <id>
```

The program in every Lead pane (§6.2): owns its terminal (§1.2). Steps of §6.7:
1. If the Session's file exists, resume: `pi --session <file>` with the Lead's `[-e …]` (§6.5).
2. Else, if `forked_from` is set, wait until the source is idle (§6.7 step 1, every 2 s, showing the waiting line), then `pi … --fork <source file> --session-id <id> -n <name> [-- <first message>]`.
3. Else start a fresh Lead: `pi --model … --session-id <id> -n <name> -- <first message>`.

The first message is the file `plan new` or `session fork` wrote (§1.4); none for a grounded Lead. While pi runs, it watches for the session file (§6.5) and records `sessions.file` once it appears (a data write, no Event). When pi exits it runs `session state stopped` if the adapter did not, prints `pi exited. Press Enter to resume <name> here.` and resumes on Enter.

| Precondition | Code | Exit |
|---|---|---|
| the Session is a `lead` | `not_a_lead` | 3 |
| its pane's `@cpd_session` is this Session | `wrong_pane` | 3 |

Idempotent: yes; a restart resumes (§6.7 step 3).

### 10.9 `session close`

```
cpd session close <id>
```

Ends a Lead's pane: pi stops and the Session's file stays, so `session open` resumes it later (§10.4). It is the tree's `x` key (§13.2). Added by cockpit parity (amendment 14, §19). The §2 index row is added by the Job that implements the command, in the same commit.

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

## 11. `events`

§5. Selection: `--plan <id>`, `--context <id>` or `--session <id>`, at most one (`usage`), or none for every Event; `--kind <kind>` may repeat (a kind not in §5.2 is `usage`). These flags are filters: they do not fall back to `CPD_PLAN` or `CPD_SESSION`.

`--session <id>` applies §5.3's rule, with "a Lead of Plan P" made precise (amendment 7, §19):
- for the Session that is its Plan's `lead_session`: Events with `plan` = its Plan; Events with `plan IS NULL AND context` = its Context; Events with `session` = itself;
- for any other Session (Workers, side forks, a grounded Lead): Events with `session` = itself.

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

§5.4, normative: one Event per line, in id order; without `--since`, only Events committed after it starts; polling `PRAGMA data_version` every `events.poll_ms`; ends on EPIPE or kill with exit 0, or with `--until-idle` once it has printed everything committed when it started.

| Precondition | Code | Exit |
|---|---|---|
| the selected Plan, Context or Session exists | `plan_not_found`, `context_not_found`, `session_not_found` | 4 |
| `--since` is not above the highest Event id (a cursor from another database) | `cursor_ahead` | 3 |

A failure after the first line (for example `store_too_new` after an upgrade) is printed as the last line, as the failure envelope, and the exit code is its exit code (§1.2). Each line:

```json
{"id": 48215, "at": "2026-10-14T21:41:02.114Z", "kind": "escalation", "context": "building-cpd", "plan": "cpd2", "job": "contract-commands", "attempt": "contract-commands-a1", "session": null, "text": "Worker contract-commands-a1 asks: … Answer: cpd answer contract-commands-a1-q1 \"<answer>\"", "data": {"escalation": "contract-commands-a1-q1", "question": "…"}}
```

Idempotent: read-only; the same `--since` replays the same lines.

## 12. `watch`

```
cpd watch [--ensure]
```

The engine, one per machine (§3.7, §6.6, D5).

**Without `--ensure`:** runs E1–E7 (E7, the PR start, since R34) every `tick_seconds` in the foreground, in `cpd-work:watch`, logging to stderr and appending the same lines to `run/watch.log`. At start it imports the Context stores (§4.6).

| Precondition | Code | Exit |
|---|---|---|
| `locks/watch.lock` is free | `watch_running` | 3 |

Effects: **Events** `watch-started` and `watch-stopped` `{pid}`, and every row and Event of E1–E7 (§3.7; a PR that E7 fails to start gets its own `error` with code `pr_start_failed` and is not tried again, §9.7). An unexpected failure in a step appends `error` `{source: "watch", code, message, details}` for the Plan it concerns and the loop continues; the same failure for the same object is appended once until it clears. It runs until SIGTERM or SIGINT, then prints `{"ok": true, "stopped": true}` and exits 0. Idempotent: a second one is refused.

**With `--ensure`:** starts the engine when it is not running, and returns at once.

| Precondition | Code | Exit |
|---|---|---|
| the window started and its process took the lock within 10 s | `watch_start_failed` | 1 |

Effects: probe `locks/watch.lock` (open and `flock(LOCK_EX | LOCK_NB)`, released at once). Free: start the server and `cpd-work` session if needed (§6.1), kill a dead `watch` window, then `new-window -d -t =cpd-work: -n watch -c $CPD_HOME -P -F '#{pane_id}' -- cpd watch`. No rows. The engine itself appends its stderr to `run/watch.log` (§1.4), so no shell is needed.

```json
{"ok": true, "started": true, "pane": "%3"}
```

Idempotent: yes; a running engine gives `"started": false` with `"already": true`.

## 13. `tui`

### 13.1 `tui`

```
cpd tui [--attach] [--respawn]
```

Builds the cockpit (§6.1, §6.2) unless it exists, and starts the engine.

Effects, each only when missing:
- **tmux:** the server with §6.1's options; the session `cpd` with window `main` (`new-session -d -s cpd -n main -x 215 -y 62`), split into the tree (left, 20 %), the plan view (centre top) above the chat slot (centre bottom), and the widgets (right, 20 %), each running `cpd tui view <tree|plan|widgets>` with `@cpd_view` set, and the StatusBar, one row across the full width at the bottom (`split-window -v -f -l 1`), running `cpd tui view statusbar` with `@cpd_view statusbar`; the chat slot holds the placeholder (§10) or, when one exists, the Plan Lead whose latest `session-moved` Event with `place: "chat"` is the newest, moved in as `session open` does. "Plan Lead" here means any Lead with a Plan, as for `session open` (side forks included, a Context's grounded Lead never), and one that `session open` refuses stays where it is while the placeholder keeps the slot (Escalation `tui-cockpit-a1-q3`). The session `cpd-work`.
- **Layout:** right after building a new `main`, `tui` runs reset-layout once (§13.2). On an existing `main` with no `statusbar` pane it adds one (same split) and then runs reset-layout, so the added row is taken from the layout and the other panes get their sizes back (amendment 15, §19).
- **tmux options:** every option, the border format, the root mouse binding and the conditional `pane-died` hook of §6.1. `host` sets them at every server start, so a server started by `plan new` or `watch --ensure` has them too; `cpd tui` sets them again on a running server (rerunning is safe).
- **`--respawn`:** `respawn-pane -k` on every live pane of `main` that has `@cpd_view` (the views, the chat placeholder and a Worker view), so each runs the installed binary again; a Lead or engine pane is never touched. `scripts/install-cpd.sh` runs `cpd tui --respawn` when a cockpit exists. (Why: `cpd tui` respawns only dead panes, so a reinstall would never reach a running view.)
- `watch --ensure` (§12).
- No rows of its own; moving a Lead appends `session-moved`.

| Precondition | Code | Exit |
|---|---|---|
| with `--attach`: stdin and stdout are a terminal | `not_a_terminal` | 3 |

```json
{"ok": true, "built": true, "socket": "cpd", "session": "cpd", "attach": ["tmux", "-L", "cpd", "attach", "-t", "cpd"], "watch": {"started": true, "pane": "%3"}}
```

With `--respawn` the value also has `"respawned": ["%1", "%2", …]`, the panes restarted. With `--attach` it then replaces itself with `tmux -L cpd attach -t cpd` and prints nothing (§1.2). Idempotent: yes; an existing cockpit gives `"built": false`. A pane of the layout that has died, the StatusBar included, is respawned (`respawn-pane`, amendment 9, §19), never duplicated; a view pane that dies while the cockpit runs is respawned at once by the `pane-died` hook (§6.1).

### 13.2 `tui view` (internal)

```
cpd tui view <tree|plan|widgets|statusbar|chat|legend|logs>
cpd tui view worker <attempt> --plan <id>
```

The ratatui programs in the cockpit's panes (§6.2). They own their terminal (§1.2), make no writes of their own, and change state only by running `cpd` commands (`session open`, `plan new`, `session close`, `session park`, …) as subprocesses. What they read is §17.

**Cockpit parity** (User, 2026-10-15). The cockpit matches the running cpd 1 cockpit exactly: layout, elements, text, colours, keys, mouse and behaviour. The reference was cpd 1's cockpit spike, `tui/` on branch `main` before the switch-over (`tui/src/*.rs`, `tui/README.md`; `tui/` now holds the Rust cockpit crate `cpd-tui`); the checklist of every difference is [cockpit-parity.md](cockpit-parity.md). Where this section states a deviation, it is deliberate and has its reason.

| View | What it is | Keys and mouse |
|---|---|---|
| `tree` | the **Context column**: Contexts, their Plans' Lead Sessions, and counters | ↑↓ and j/k move the cursor, Enter opens the Lead in the chat slot, `n` a new Plan, `x` closes a Lead's pane, `?` the legend; clicks as below |
| `plan` | the **Plan view**: one box per PR of the chat slot's Plan, each holding the flowchart of its Jobs, under a row of tabs | ←→↑↓ and h/j/k/l select, Enter opens a Job's Worker, Esc clears the selection; the wheel scrolls (3 rows, 4 columns); clicks as below; no `q` |
| `widgets` | the **Dispatch column**: the machine, the cockpit's PRs, git and Jira (future) | `q` quits; nothing else |
| `worker` | the read-only Worker view: the Attempt's Worker session file (vision decision 5), `you` and its text in cyan, `agent` in green, `→ tool` in yellow, `← last` dim | `q` or Esc quits; ↑/k, ↓/j, PageUp, PageDown and the wheel (3 rows) scroll; End or `G` follows the end again; no other keys |
| `statusbar` | one row across the bottom | no keys, no focus; clicks on its buttons |
| `chat` | the chat slot's placeholder (§10) | none |
| `legend` | the key legend, in a popup | any key closes it |
| `logs` | the Event log, in a popup | ↑↓, j/k, PgUp/PgDn scroll; `q` or Esc closes it |

**`tree`.**
- **Rows.** A Context row (a dot of the busiest phase inside, its name bold, the Plan count when collapsed) and under it one row per Lead Session: the Plans' Lead Sessions in `created_at` order, **then, after them, every Session forked by `session fork`** (B1, contract §6.8: a fork is a sibling under the Context, never nested under a Plan or under the Session it came from, whatever its `plan` or `forked_from`; *decided*: they are ordered by `created_at` and show no counters, the counters being the Plan Lead's). Row: guide, dot, name (cut with `…` before the counters), and the five Job counters `queued⋅running⋅merging⋅failed⋅merged` right-aligned (cancelled Jobs are not counted; no counters without a Plan). `⏵` marks the Session in the chat slot, in column 0, and its name is bold. The dot is `●` when the Session **needs you**, else `○`; the rule is §17's.
- **Cursor.** Keyboard only: a click never moves or shows it. It moves between Sessions only (Context rows are not selectable), is drawn inverted across the full row only while the pane has keyboard focus (`focus-events`, §6.1) and after an arrow key; Enter, `n` and `x` act on it only while it shows.
- **Clicks.** A click on a Session opens it in the chat slot and focuses the chat (`session open`, then `select-pane`); a click on a Context row collapses or expands it. Clicks are ignored while a prompt is open.
- **`n`** asks for a name in one inline row at the bottom (Enter starts, Esc cancels, an empty name is refused) and runs `plan new` (§4.1) in the selected Session's Context: the Plan forks the Context's grounded Lead (R15), the one deviation from cpd 1's `n`, which forked the selected session. On a Context row it says `pick the session to fork`.
- **`x`** closes the selected Session's pane after the inline prompt `Close <label>? pi stops; the session file stays (y/N)`, preceded by `<label> is working.` when its state is `working`; only `y` closes. It runs `cpd session close <id>` (§10.9). The Session in the chat slot is refused with `switch to another session first`, also when it moved there while the prompt was open. On a Context row it does nothing. `<label>` is the Session's name.
- **`?`** opens the legend popup. The view has no other key; what a key used to do is still a command.

**`plan`.**
- **Top row:** only ` <context id> ▸ ` (dim) and the tabs, over a separator line (`Borders::TOP`, colour 8). Nothing at the right: no PR, no workers count, no Plan title.
- **Tabs:** one per Lead Session of the Context that has a pane in the cockpit (a Plan's Lead and its side forks, B1; a grounded Lead never), labelled by the Session's name, else its Plan id, else the first 8 characters of its id, as ` <label> ` with one space between tabs and separators in colour 8. The active tab (the chat slot's) is reversed and bold; that is its only marker. With none: ` no Lead session in the centre` (dim). A click on a tab runs `session open <id>` and then `select-pane` on the chat slot.
- **Chart:** the Plan's Jobs as cards (reference `flow.rs`, ported), inside `Margin{2, 1}`; cancelled Jobs hidden. Messages are one row at the bottom of the window: `no Plan yet`, `loading… (first fetch)`, or the note.
- **PR boxes** (R34, A15; contract §2.5). The chart is split into **one box per PR** that is not `cancelled`, each a bordered block (`Borders::ALL`, colour 8) holding the cards and Dependency arrows of that PR's Jobs, laid out inside it as the single chart was. The box's title, on its top border, reads ` <key> · <repo name> → <base> · <pull request> `: the PR key in bold; the repo's directory name (the last component of `prs.repo`); its base; then `#<number> <state>` once the PR has a pull request, where `<state>` is `open`, `merged`, or `closed` while the pull request is closed without a merge (`pr_seen.closed`, A11), in the palette's PR state colours (reference `pr_state`), else the PR's status (`waiting`, `dispatch`) dim. The boxes are laid out **left to right** in the order of the needs graph: a box stands to the right of every box it needs (its column is one more than the highest column among its needs; boxes in the same column are stacked top to bottom by key), and each need is drawn as an arrow from the needed box's right border to the needing box's left border. A PR with no Jobs shows an empty box (` no Jobs `, dim). A Plan with one PR shows one box; a Plan with no PR yet shows the message `no PR yet: cpd pr add` (dim). Selection and Enter work on Job cards across boxes: ←→ move to the nearest card in the next column, crossing into the neighbouring box; the wheel and the scroll offsets apply to the whole row of boxes.

**`widgets`** (reference `dispatch`), top to bottom, with the shared separator (`Borders::TOP`, colour 8) between sections, as `Layout::vertical([Length(2), sep, Min(1), sep, Length(2), sep, Length(2)])`:
1. the machine: its hostname in bold (` <hostname>`) and ` load a b c`, the first three fields of `/proc/loadavg` (on macOS `sysctl -n vm.loadavg`), re-read every 2 seconds;
2. ` PRs` (bold), then one entry per Plan whose Lead Session has a pane in the cockpit (in the chat slot or parked). A Plan with a pull request takes one line ` <label>` (bold; the label is the Lead Session's name, as the tab's) and then one line per PR of the Plan that has a pull request (R34), in key order: `  #<number> <state> <checks> <review decision>  <title>`, the state and checks in the palette's colours (reference `pr_state`, `checks`), the decision `approved`, `changes requested` or `no review`, the title (`pr_title`) dim and cut first when the column is narrow. A Plan with no pull request takes one line, ` <label>` and `  no PR` (dim), listed after the Plans with pull requests and only while the section has the room, as the reference. With no Plan at all: ` no Plans in the cockpit` (dim);
3. ` git` (bold) and ` (future)` (dim);
4. ` Jira` (bold) and ` (future)` (dim).

The column has no engine line, no slots, no Escalations, no Events and no footer hint. The Event log is the `logs` view (R36, below).

**`statusbar`** (R36, User decision) is one row across the full width at the bottom of `cpd:main`; it has no keys, takes no keyboard focus (a click on it is passed on with `send-keys -M`, no `select-pane`, §6.3) and draws no border title. **Its left side holds action buttons only; its right side holds read-only information.** The middle expands.
- **Left, in order:** the reset button `⧉ ` (reversed, no leading space), the legend button ` ? legend ` (reversed), and the logs button ` ≡ logs ` (reversed). A click on `⧉ ` runs reset-layout (below); on ` ? legend ` opens the legend popup; on ` ≡ logs ` opens the logs popup.
- **Right, in order:** a failure note when the last click failed (cleared on the next click), then the workers indicator `workers N/M`, where `N` is the number of Attempts holding a slot and `M` is config `concurrency`. The indicator gets the full width, as cpd 1's; it shortens to `N/M` when the row is too narrow and is dropped when not even that fits.

**`legend`** is what every colour and icon of the cockpit means, from the palette. It opens in a tmux popup (`display-popup -E -T ' legend ' -w <w> -h <h>`, sized to the legend and at most the client's size) from the tree's `?` key or the StatusBar's legend button, because the columns are narrower than the legend; any key closes it. It has no `@cpd_view` pane of its own. Its section titles are `Jobs: Plan pills and Context counters` and `Context dots: … the Plan's phase`; the Job statuses it lists are those the chart draws, so `cancelled` is not among them.

**`logs`** (R36) is the **Event log**: every Event (§5), one line each: its time of day (`HH:MM:SS`, dim), then its `text`, or its kind when the `text` is empty, an `error` Event in the failed colour. It opens in a tmux popup (`display-popup -E -T ' logs '`, at most 80 % of the client's width and height) from the StatusBar's ` ≡ logs ` button, and has no `@cpd_view` pane of its own. The newest Event is last; the view follows live while it shows the bottom, and stops following when the User scrolls up (it follows again at the bottom). ↑↓, j/k and PgUp/PgDn scroll; `q` or Esc closes it.

**Reset-layout** is not a CLI command. It runs `resize-pane` on the panes of `<cockpit>:main` found by `@cpd_view`, in this order:

| # | `@cpd_view` | Size |
|---|---|---|
| 1 | `statusbar` | `-y 1` |
| 2 | `tree` | `-x 20%` |
| 3 | `widgets` | `-x 20%` |
| 4 | `plan` | `-y 20%` |
| 5 | `worker` | `-x 30%`, only when a Worker pane is open |

The chat slot is never resized; it takes what is left. The StatusBar's ⧉ click runs it in-process; `cpd tui` runs it once, right after building a new `main` and after adding a missing StatusBar to an existing one (§13.1).

Refusal before drawing: an unknown view or Attempt is `usage` or `attempt_not_found`. Idempotent: they only read.

## 14. `doctor`, `config`, `tiers`

### 14.1 `doctor`

```
cpd doctor
```

Checks the machine (cpd 1 §11 `doctor`, kept in shape). Each check has `name`, `ok`, `severity` (`error` or `warn`), and when relevant `found`, `expected` and `message`. Any failing `error` check makes the result `doctor_failed` (exit 1) with every check in `details.checks`; `warn` checks never fail it.

**Minimum versions.** Each minimum is the version verified on this machine (the homeserver, 2026-10-14), because the contract's tmux, git, gh and pi behaviour was verified on exactly these (contract.md, evidence labels). Older versions may work; that is unverified, so doctor refuses them.

| Tool | Command run | Output found on 2026-10-14 | Minimum | Parsed as | What cpd needs from it |
|---|---|---|---|---|---|
| git | `git --version` | `git version 2.43.0` | `2.43.0` | the first `x.y.z` | `worktree add`, `update-ref` with an old value, `merge-base --is-ancestor`, `merge-tree --write-tree --name-only` (git 2.38 or later, docs), `read-tree -m -u` (§7) |
| tmux | `tmux -V` | `tmux 3.4` | `3.4` | `x.y`, ignoring a letter suffix (`3.4a`); `next-…` builds pass | `new-window`/`split-window -e`, argv commands after `--`, `swap-pane`, `pane-border-lines`, `extended-keys`, `#{pane_dead_status}` (§6) |
| pi | `pi --version` | `1.2.0` | `>= 1.2.0 < 2.0.0` | `x.y.z` | `--fork`, `--session-id`, `-n`, `--exclude-tools`, `--no-skills`, `--approve`; the catalog of §9 (cpd 1 §11: pi updates itself, so a range) |
| gh | `gh --version` | `gh version 2.45.0 (2025-07-18 Ubuntu 2.45.0-1ubuntu0.3)` | `2.45.0` | the first `x.y.z` | `pr create`, `pr list --json`, `pr view --json …statusCheckRollup…`, `api` (§7.6) |
| pi's Node | `<pi node> --version`, the binary §9 chooses | `v22.23.2` (`/usr/local/bin/node`; this machine has no `pi-node` directory) | none: it must run | | runs the catalog module of §9 |

Checks, in order:

| Name | Severity | Passes when |
|---|---|---|
| `config` | error | `config.toml` exists and is valid (§8) |
| `store` | error | the database opens, migrates (§4.5) and passes the schema check (§4.3) |
| `tamper` | error | the tamper report (§4.3) is empty; `found` lists it |
| `git`, `tmux`, `pi`, `gh` | error | found on `PATH` at or above the minimum above |
| `gh_auth` | warn | `gh auth status` exits 0 (needed only by `pr` commands; verified on 2026-10-14: logged in through `GH_TOKEN`) |
| `pi_node` | error | pi's Node runs |
| `tiers` | error | each `tiers.*` model is in pi's catalog, and `lead_tier` is a valid Tier (§9) |
| `pi_adapter` | error | the `pi_adapter` directory has a `package.json` |
| `lead_packages` | warn | reports, for each `lead_pi_packages` entry and the adapter, whether a Lead gets it by `-e` or from which settings file (§6.5); warns when an entry is neither declared nor resolvable |
| `plannotator_port` | warn | when `PLANNOTATOR_REMOTE` is set, `PLANNOTATOR_PORT` is a range of at least two ports (§15.5); a single port lets only one Plan review be open on the machine |
| `contexts` | warn | every directory under `contexts/` is a valid store and none is `sync_failing` |
| `watch` | warn | the engine runs when any Plan is in `dispatch` or `pr` |

```json
{"ok": true, "checks": [
  {"name": "git", "ok": true, "severity": "error", "found": "2.43.0", "expected": ">= 2.43.0"},
  {"name": "tmux", "ok": true, "severity": "error", "found": "3.4", "expected": ">= 3.4"},
  {"name": "pi", "ok": true, "severity": "error", "found": "1.2.0", "expected": ">= 1.2.0 < 2.0.0"},
  {"name": "gh", "ok": true, "severity": "error", "found": "2.45.0", "expected": ">= 2.45.0"},
  {"name": "plannotator_port", "ok": false, "severity": "warn", "found": "19432", "expected": "a range, e.g. 19432-19439", "message": "only one Plan review can be open at a time"}]}
```

Change from cpd 1: no `node`, `bd`, `wt` or `herdr` checks (§1.1); `gh` now has a minimum. Idempotent: yes; it writes nothing except a migration on first open.

### 14.2 `config`

```
cpd config
```

Prints the effective configuration: defaults merged with `config.toml` (§8), each value with its source. Refusals: `config_missing`, `config_invalid` (exit 1, `details.key` = the key path). No effects.

```json
{"ok": true, "path": "/home/clayton/.cpd/config.toml",
 "config": {"version": 1, "machine": "hs", "concurrency": 3, "retry_cap": 2, "lead_tier": "O3", "tiers": {"O": "github-copilot/claude-opus-5.5", "S": "…", "H": "…"}, "timeouts": {"attempt_minutes": 30, "…": "…"}, "watch": {"tick_seconds": 5, "…": "…"}, "events": {"poll_ms": 200}, "tmux": {"socket": "cpd", "cockpit_session": "cpd", "work_session": "cpd-work"}},
 "sources": {"version": "file", "machine": "file", "concurrency": "default", "tiers.O": "default"}}
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

Effects: when `install/current-version` differs from the cached `pi_version`, one transaction inserting `tier_catalog` rows (a cache: no Event). No tmux, no git.

```json
{"ok": true, "pi_version": "1.2.0", "tiers": {"O": {"model": "github-copilot/claude-opus-5.5", "efforts": [0, 1, 2, 3, 4, 5]}, "S": {"model": "github-copilot/claude-sonnet-5.5", "efforts": [0, 1, 2, 3, 4, 5]}, "H": {"model": "github-copilot/claude-haiku-4.5", "efforts": [0, 1, 2, 3]}}}
```

The `efforts` above are an illustration, not verified values. Idempotent: yes.

## 15. The pi adapter's protocol

The pi adapter is the TypeScript package `pi/` (decisions: "the pi adapter stays TypeScript"), loaded in every pi session cpd starts (§6.5). It is the only pi-specific code: it turns Events into messages (§5.3), reports agent states (§3.6) and runs the Plan review through plannotator. It calls only `cpd` commands; it never reads the database and never types into a pane (cpd 1 §15, kept).

### 15.1 Package and activation

- **Package:** `pi/package.json` with `"keywords": ["pi-package"]` and `"pi": {"extensions": ["./extension.ts"], "skills": ["./skills"]}`; host packages are `peerDependencies: "*"` (cpd 1 §15, kept). The skill `cpd-lead` is rewritten for cpd 2 (follow-up Job 17).
- **Location:** `pi/`, installed to `~/.local/share/cpd/pi` by `scripts/install-cpd.sh`. (History: before the switch-over it lived beside cpd 1's `pi/`, in a second directory.)
- **Activation:** at pi's `session_start`, only when `ctx.mode` is `tui` and `CPD_SESSION` is set. Change from cpd 1: cpd 1 activated in any herdr or tmux pane (`HERDR_ENV`, `TMUX_PANE`); now cpd marks its own panes with `CPD_SESSION` (§6.5), so a pi the User starts by hand is left alone.
- **Identity:** `CPD_SESSION` must equal `ctx.sessionManager.getSessionId()`. If not, the adapter delivers one error message (below) and stays inactive: it would otherwise report another Session's state.
- **Calls:** one-shot commands through `pi.exec("cpd", args)`, whose stdout is parsed as the JSON envelope at the boundary (a non-JSON stdout, or `ok: false`, is an error with the code). The follower is a child process (`node:child_process` `spawn`, argv, stdin `/dev/null`) read line by line.
- **Errors** are delivered to the Session as `pi.sendMessage({customType: "cpd", content: "cpd adapter: <what>: <code>: <message>", display: true, details: {kind: "error", code, …}}, {triggerTurn: true, deliverAs: "followUp"})` (cpd 1's `errorDelivery`, kept). The adapter never throws into pi.

### 15.2 Start-up and the cursor

At `session_start`, in this order:
1. `cpd session show <id>`: role, Plan, Context, `plan_lead`, `registered_event`, name.
2. `cpd session state idle --session <id>` (`stopped → idle`, §3.6); then `working` if `ctx.isIdle()` is false.
3. **Name:** if pi's session name differs from the Session's `name`, `pi.setSessionName(name)`: a `session-renamed` missed while pi was down still applies.
4. **Cursor** (§5.3 "No loss, no duplicates"): the greater of `registered_event` and the `details.id` of the last `cpd` custom message in the session's entries whose `details.delivered_to` is this Session's id. The adapter puts `delivered_to` into every message it delivers (amendment 5, §19). Why: a fork's file starts with its source's messages, whose Event ids belong to the source's stream; counting them could skip Events addressed to the fork.
5. **Blocking reasons** (Plan Lead only): one per `open` Escalation of its Plan (`cpd plan show`), and one per pending Plan review this session opened (its `cpd-plan-review` entries, §15.5).
6. **Follow:** spawn `cpd events follow --session <id> --since <cursor>`.

The cursor in memory advances after each line is handled, delivered or not. Only delivered messages persist it, so after a restart the Events handled without a message (`escalation-answered`, `escalation-expired`, `session-renamed`, `plan-review`) are seen again; every handler below is idempotent for that reason.

**The follower ends** (EOF, exit, or a failure line): the adapter restarts it with `--since <in-memory cursor>` after 1 s, then backing off to at most 30 s. A failure line or a non-zero exit is delivered as an error message once per error code, until a line is read again. A line that is not a valid Event (§5.2 fields, a known kind) is delivered as an error message and skipped, and the cursor moves past it, so one bad line cannot stop delivery. At `session_shutdown` the follower is killed.

### 15.3 What each Event does

Events arrive in id order; the follower already applied §11's `--session` rule, so a Lead receives its Plan's Events and a Worker or side fork only those addressed to it. Delivery is `pi.sendMessage({customType: "cpd", content: event.text, display: true, details: {...event, delivered_to: <id>}}, {triggerTurn, deliverAs: "followUp"})` (§5.3).

| Kind | Plan Lead | Worker, side fork, grounded Lead |
|---|---|---|
| `job-merged`, `job-failed`, `all-merged`, `pr-feedback`, `plan-done`, `error` | message, **turn**. `all-merged` and `pr-feedback` carry the PR's key in `data.pr`; the Lead answers `all-merged` with `cpd pr open --pr <key>`, and an `error` with code `pr_start_failed` with `cpd pr start <key>` once the cause is fixed (R34, A9) | |
| `attempt-retry`, `job-cancelled` | message, no turn | |
| `pr-started`, `pr-merged` | message, no turn (R34: a started PR's Jobs are now scheduled; a merged PR may start the PRs that need it, §5.3) | |
| `escalation` | message, **turn**; opens the blocking reason `escalation:<id>` labelled `cpd: Worker question` | |
| `escalation-answered`, `escalation-expired` | closes `escalation:<id>`; no message | |
| `plan-review` with `status: pending` | opens it in plannotator (§15.5); no message | |
| `plan-review` with `status: approved` or `rejected`, for a pending entry of this session | a decision made outside plannotator (§15.5 "Deciding elsewhere"): closes `plan-review:<review>`; message `cpd-plan-decision`, **turn** (own custom type, not `cpd`). For any other review: ignored | |
| `session-renamed` (`session` = itself) | `pi.setSessionName(name)` and `ctx.ui.notify("cpd: session renamed to \"<name>\"")`; no message, no turn | same |
| `attempt-nudge` (`session` = itself) | | message with `data.prompt` as content, **turn** |
| every other kind | ignored | ignored |

This is §5.3's table; the `plan-review` row is the Plan review protocol §5.3 leaves to this file (amendment 6, §19). A blocking reason opened twice or closed when not open changes nothing (cpd 1 `blocked.ts`, kept), which makes replays harmless.

### 15.4 Agent states: `session state`

The adapter keeps three facts and reports the state they give (the model of `tmux-agent-state.ts`, §3.6):

| Fact | Set by |
|---|---|
| `active` | true at pi's `agent_start`; false at `agent_settled` when `ctx.isIdle()` is true |
| its own blocking reasons | §15.3 (`escalation:<id>`) and §15.5 (`plan-review:<review>`), each with a label |
| other extensions' blocking signals | `pi.events.on("herdr:blocked", {active, label})`, counted: one `active: false` closes one `active: true`. Dialog extensions such as `ask_user_question` emit these today (unverified for every Lead package; the adapter Job checks each) |

Reported state: `blocked` with the label of the most recently opened reason while any reason or signal is open; else `working` while `active`; else `idle`. On every change it runs `cpd session state <state> [--label <label>] --session <id>`. Calls are serialised: at most one in flight, and only the latest wanted state is sent next, so states never arrive out of order. A refused call (`illegal_transition`, for example after the engine marked the Session `stopped`) is reported as an error once and the next change is tried normally. At `session_shutdown`: `session state stopped`, waited for at most 2 s; `session run` reports it as well if pi exits without it (§10.8).

A Worker in `cpd ask` is set `blocked` by `ask` itself and back to `working` when it returns (§8.1); its adapter is mid-turn meanwhile and reports nothing.

**Names.** On pi's `session_info_changed`, when pi's name differs from the last name the adapter knows, it runs `cpd session rename <name> --session <id>`, so a `/name` typed in pi reaches the TUI. The `session-renamed` it causes comes back and changes nothing.

### 15.5 The Plan review through plannotator

Replaces cpd 1 §15's `cpd_plan_review` flow; the plannotator event API is the one cpd 1 uses (t4), parsed at the boundary because it is not a stable contract.

**Requesting.** The Lead runs `cpd plan review`, directly or through the tool `cpd_plan_review` (no parameters; registered for Plan Leads only). The tool runs `cpd plan review --plan <CPD_PLAN>` and returns its `next` text; when the result is `"already": true` and this session has no pending entry for that review, the tool opens it itself (below), so a review that failed to open can be retried. The tool's name is kept so §6.5's `--exclude-tools` list for Workers stays right.

**Opening**, on a `plan-review` Event with `status: pending` for its Plan, or from the tool:
1. Skip it when the session already has a `cpd-plan-review` entry for this review, or when it is not the Plan's latest review (`cpd plan show`, `review.id`): a replayed or superseded request.
2. Read `run/<plan>/plan-<review>.md` (§4.6).
3. `pi.events.emit("plannotator:request", {requestId, action: "plan-review", payload: {planContent, planFilePath}, respond})`, waiting at most 15 s for `respond`. A `handled` response with `status: pending` gives plannotator's own review id.
4. `pi.appendEntry("cpd-plan-review", {plan, review, plannotator_id, status: "pending"})`, so the mapping survives `/reload` and resume; open the blocking reason `plan-review:<review>` labelled `cpd: plan review`.

If plannotator does not answer, answers `unavailable` or `error` (for example `Port 19432 in use`), the adapter delivers an error message with plannotator's error (turn) and writes nothing; the review stays `pending` in the database.

**Deciding**, on `plannotator:review-result` `{reviewId, approved, feedback?}` for a pending entry:
1. Write the feedback, if any, to a temporary file; run `cpd plan decide <review> --approve | --reject [--feedback-file <f>] --plan <plan>`. `review_decided` means another caller decided it: the adapter reads the decision with `cpd plan show` and continues with it.
2. Append the entry with `status` `approved` or `rejected`; close `plan-review:<review>`.
3. Tell the Lead with `pi.sendMessage({customType: "cpd-plan-decision", …}, {triggerTurn: true, deliverAs: "followUp"})` (cpd 1 `decisionText`, kept): `Plan review r3 was APPROVED.` plus the feedback, then `Next: cpd plan lock`, or for a rejection `Revise the plan (job edit, job add, job cancel), then call cpd_plan_review again.`; when `current` is false, it says the Jobs changed since that review and asks for a new one. The custom type differs from `cpd`, so this message never moves the cursor.

**Deciding elsewhere**, on a `plan-review` Event with `status` `approved` or `rejected` for a pending entry of this session: any caller other than plannotator decided the review (`cpd plan decide` from the CLI, the User or a script), and the database already holds the decision, so the adapter does **not** run `plan decide`. Otherwise the Lead would stay `blocked` and never be told. It does what "Deciding" does after its own `plan decide`:
1. Read `cpd plan show` for `current` (and for the feedback when the Event's `data.feedback` is not a string).
2. Append the entry with the status; close `plan-review:<review>`.
3. Tell the Lead with the same `cpd-plan-decision` message (`triggerTurn`, followUp), with the Event's `data.feedback`, plus a last line saying the review was decided outside plannotator and that its page stays open.
4. plannotator's page cannot be closed: its event API (`plannotator-events.ts`, 0.28.17) has no request for it (`plan-mode`, `plan-review`, `review-status`, `code-review`, `annotate`, `annotate-last`, `archive`); `closeKeepingDraft` in `server/serverPlan.ts` is only on the in-process server object, which the adapter cannot reach. The message says so.

A failure of `plan show` is an error message and leaves the entry pending. An Event for a review with no pending entry (replayed, another Plan's, already decided) changes nothing. A later `plannotator:review-result` (or recovery's `completed`) for an entry decided this way is ignored, because the entry is no longer pending; one that arrives while this path runs is ignored too, so the Lead is told once.

**Recovery** at `session_start` (t4 F5, kept): for each pending entry, `plannotator:request` `review-status` every second until it is not pending: `completed` is handled as a result; `missing` (plannotator restarted) appends the entry as `lost`, closes the reason, and tells the Lead to call `cpd_plan_review` again, which reopens it (above).

**Several Leads at once** (follow-up "Plan reviews collide across Leads"). plannotator in remote mode listens on one port, `PLANNOTATOR_PORT` or 19432, and a second Lead's review fails with `Port 19432 in use after 5 retries` while the first is open (docs: plannotator 0.28.17 `server/network.ts`; seen on 2026-10-14). The same variable also accepts an inclusive range `<a>-<b>`, and each review server then takes the first free port in it (docs: same file, `parsePortSelection` and `listenOnPort`). So:
- the User's environment sets a range, for example `PLANNOTATOR_PORT=19432-19439`, and the Macs forward the range over ssh: up to 8 reviews open at once; plannotator prints each review's URL in its Lead's chat;
- cpd passes `PLANNOTATOR_PORT` and `PLANNOTATOR_REMOTE` from its own environment into every Lead pane with `-e`, when set (amendment 8, §19), so a pane gets them whatever environment the tmux server was started with;
- `doctor` warns about a single port (§14.1).

Rejected alternatives: one shared review server (cpd would have to run plannotator's server outside pi, which plannotator does not offer to extensions), and a queue (a review waiting for another Plan's User decision could wait for hours). Unverified: that two plannotator servers in one range do not disturb each other's sessions; the adapter Job tests it.

## 16. The Worker contract

### 16.1 The Worker Spec

Rendered by the driver at A3 to `run/<plan>/specs/<attempt>.md` from a template compiled into the binary, so nothing in a repo or worktree can change what Workers are told. Content, kept from cpd 1 `templates/worker.md` (cpd 1 §16) with the changes marked:

| Section | Content |
|---|---|
| Title | `# Worker Spec: <job>, Attempt <n>` |
| Who you are | Attempt `<n>` of Job `<job>` (`<title>`) in Plan `<plan>` (was "Workflow"); attempt id, Tier, worktree path |
| Rules | work only in this worktree and touch only the listed files; commit your work; before submitting, run the quick check yourself: the commands of `pre-commit` from the worktree's `.config/wt.toml`, listed verbatim, or, when the file has no `pre-commit` table, the line `This repo defines no quick check.` (change: cpd 1 said `./verify.sh`; R20: no fallback to `pre-merge`); never edit the protected paths, listed: `.config/wt.toml`, `.config/cpd.toml`, `.cpd/`, and each entry of the repo's protected list (§7.5; change: cpd 1 hard-coded `verify.sh`); never ask the User anything; if continuing would require guessing, `cpd ask <attempt> "<question>"`, which blocks until the Lead answers (change: no `--wf`, the pane has `CPD_PLAN`); one blocker at a time; do not stop until you have submitted |
| Job | Goal, Context, Files you may touch (`touches`), Acceptance, Out of scope (or "Nothing is listed. Still do only what the Goal and Acceptance require.") |
| Previous Attempts | only when the Job has ended Attempts: for each, number, Tier, outcome and cause, and what explains it: the conflicting files (`merge_conflict`), or one sentence for every other cause (change: cpd 1 showed only feedback) |
| Answers already given | new: every `answered` Escalation of earlier Attempts, question and answer, so a Worker does not ask again |
| Finish | `cpd attempt submit <attempt>`; on success stop at once; on a refusal, what each code means: `wrong_worktree` (run it from the worktree), `dirty_tree`, `no_commits`, `gate_tampered` (revert the protected paths), `check_failed` (fix, commit, submit again; the tail is in `details`), `check_error` (the check could not run: ask the Lead), `attempt_not_running` (the Attempt was stopped: stop) |

**A review Job's Spec** (B2) differs from the table in these rows: *Who you are* says it is a review Job and that it is a fork of the Lead; *Rules* replace "commit your work" and the quick check with **commit nothing**, and tell it to read the diff of the PR's Lead Branch against the PR's base (`<lead branch>` and `<base>` are given) and the Plan's Specs, to write its findings to `$CPD_HOME/run/<plan>/reviews/<job>.md` (absolute path given, non-empty), and not to edit the repo; *Finish* adds `review_commits` (you committed: undo it and submit again) and `review_missing` (write the findings file) to the refusal codes and drops `no_commits` and `check_failed`. The findings are Markdown; the Lead reads them and turns them into fix Jobs before `pr open`.

The Worker is started as §6.5 says, with the first message `Read <spec path> and follow it exactly. Your attempt id is <attempt>.` and `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT` in its environment.

**Nudge** (A5, once): an `attempt-nudge` Event to the Worker's Session with `data.prompt` = `You ended your turn without running cpd attempt submit <attempt>. Finish the Spec and submit, or run cpd ask <attempt> "<question>" if you are blocked.` (cpd 1 §12 step 7, kept), delivered with a turn (§15.3).

**Submit preconditions:** §6.1, in §7.3's order.

### 16.2 The review files and the brief

Removed by R19 (no Job-level Reviewer).

### 16.3 The verdict

Removed by R19 (no Job-level Reviewer).

## 17. What the TUI reads

Outline §8 and D4: the views take one **snapshot** with a read command, then **follow Events**; there is no timer polling of the CLI, no cache directory and no tmux option carrying state. A view reads the database **in process**, through the read model (`tui/src/read/`): the same queries as the snapshot commands named below, one read transaction per snapshot, no subprocess and no `gh`. (An earlier draft had each view run `cpd` as a subprocess; the implementation does not, and one source of truth is the store.) Reads take no lock and never wait for writers (§4.2); cpd 1's `workflow show` took 58 s under load (research.md), which is why the spike needed a cache.

**Gap-free start.** Every snapshot command returns `events_cursor`, the highest Event id in the same read transaction. The view then runs `cpd events follow --since <events_cursor>`, so no Event between the snapshot and the follow is lost or seen twice.

| View (`@cpd_view`) | Snapshot (the read of the command named) | Events it acts on | What it updates |
|---|---|---|---|
| `tree` (Context column) | `cpd context list`, `cpd session list` | `context-created`, `context-grounded`, `plan-created`, `plan-phase`, `plan-done`, `session-registered`, `session-state`, `session-moved`, `session-renamed` | Contexts, their Plans' Lead Sessions, then the Sessions forked by `session fork` as siblings under their Context (B1), each with its state, name, Job counters (Plan Leads only) and **needs-you** dot (rule below) |
| `plan` (centre top) | `cpd plan show --plan <p>` for the Plan of the Lead in the chat slot (its `prs`, each PR's key, repo, base, needs, status, `pr`, `pr_seen.closed` and `jobs`, for the PR boxes, §13.2), and the Context's Lead Sessions that have a pane (the tabs) | every Event with `plan = p` (the `pr-*` kinds and `pr-feedback` included: they change a box's title or add a box); `session-moved` with `place: "chat"` (the slot changed: switch Plan and take a new snapshot) | one box per PR that is not `cancelled`, left to right by needs and linked by them, each with the flowchart of its Jobs, Attempts and Tiers (R34, A15); and the tabs. Not drawn, so not read: open Escalations, the latest Plan review |
| `widgets` (Dispatch column) | `cpd plan list` for the Plans whose Lead Session has a pane in the cockpit (`session-moved` places `chat` or `parked`), with each one's PRs: number, state, checks, review decision and title (`pr`, `status`, `pr_title`, `pr_review_decision`, `pr_checks` of §4.2, §7.6, contract §4.4); and the host's load average (`/proc/loadavg`; on macOS `sysctl -n vm.loadavg`), re-read every 2 s with no Event | `plan-created`, `plan-phase`, `pr-opened`, `pr-feedback`, `pr-merged`, `plan-done` (and `session-moved`, for which Plans have a pane) | the machine's name and load, and the pull requests of each Plan. No engine, slots, Escalations or Events |
| `statusbar` (bottom row) | the slots and config `concurrency`: the Attempts not ended, and `concurrency` | any (re-read when the store's highest Event id moved, coalesced to 500 ms, at least every 2 s); no subprocess, no `gh` | `workers N/M` |
| `logs` (popup of the StatusBar's ` ≡ logs ` button) | the latest 500 Events (the `events list` default), oldest first | every Event: it follows live while the view shows the bottom (R36) | the Event log |
| `worker` (beside the chat) | `cpd attempt show <attempt>` for the Worker's session file | `session-state` for its Session; `attempt-status` for its Attempt | the session file itself, read-only (vision decision 5), and the state |
| `chat` (placeholder) | none | none | the static text `No Lead here: pick a Plan in the session tree` |

**Rates.**
- Each view follows Events with no selection and filters by kind itself, through `cpd events follow` or the same poll in process (§5.4): one `PRAGMA data_version` read per `events.poll_ms` (200 ms), which costs no I/O while nothing changes (§5.4).
- A view applies an Event's `data` directly when it carries what is shown (`session-state`, `session-renamed`, `session-moved`); otherwise it re-runs its snapshot, coalesced to at most one snapshot per view every 500 ms, whatever the Event rate.
- The `worker` view re-reads its session file when its size or modification time changes, checked every 500 ms; the file is not in the database.
- Clocks such as "running 4 m" are redrawn every second from timestamps already read, without a read.
- If a follower ends, the view takes a new snapshot and follows again from its `events_cursor`.

**The tree's ● rule (needs you).** A Lead Session's dot is `●` when its state is `blocked`, or when it is `idle`, not in the chat slot, and its latest `session-state` Event with `to: "idle"` is newer than its latest `session-moved` Event with `place: "chat"` (never in the slot counts as older than everything). It is derived from Events, with no write: it replaces cpd 1's `@asking`, `@settled_at` and `@seen_at` pane options, and shows the same thing, a Lead that waits on the User or finished a turn the User has not seen.

**Actions.** The views change state only through commands: selecting a Lead runs `cpd session open <id>`; the new-Plan key runs `cpd plan new …`; the close key runs `cpd session close <id>`; parking runs `cpd session park`; opening a Job's Worker opens a `cpd tui view worker` pane beside the chat (`split-window`, `@cpd_view worker`), which is the view's own layout, not Ledger state. The StatusBar's ⧉ click resets the layout (`resize-pane`, the view's own layout, not Ledger state), its legend button opens the legend popup and its ` ≡ logs ` button the logs popup (§13.2).

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

Rows 12–21 were added by Job `parity-contract` (cockpit parity, User, 2026-10-15, and R36); the checklist behind them is [cockpit-parity.md](cockpit-parity.md). Rows 12–14 and 16 are also applied to contract.md itself; rows 15 and 17–21 state what contract.md leaves to this file.

| # | contract.md section | Amendment | Where here |
|---|---|---|---|
| 1 | §6.2, chat slot rule | the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat` (Escalation `contract-commands-a1-q1`, answered by the Lead) | §10 |
| 2 | §6.2, `@cpd_view` values | new value `chat`: the placeholder pane running `cpd tui view chat`, text `No Lead here: pick a Plan in the session tree`; at most one, only ever in the slot | §10, §13.2, §17 |
| 3 | §6.4, `session park` | park creates a new park window holding a placeholder, then `swap-pane`s the Lead and the placeholder; `session open` kills the placeholder pane with its window when a Lead takes the slot | §10.4, §10.5 |
| 4 | §1.4, `$CPD_HOME` layout | three more files: `run/<plan>/lead-message.md`, `run/<plan>/plan-<review>.md`, `run/<plan>/messages/<session>.md` | §1.4 |
| 5 | §5.3, the cursor | the adapter adds `delivered_to` to every message's `details`, and the cursor counts only messages delivered to this Session, never at or below its `session-registered` Event | §15.2 |
| 6 | §5.3, delivery table | `plan-review` with `status: pending` opens plannotator in the Plan Lead's adapter (no message); listed today under "every other kind: ignored" | §15.3, §15.5 |
| 7 | §5.3 and §5.4, `--session` selection | "a Lead of Plan P" is the Session that is `plans.lead_session`; any other Session, side forks included, follows only Events addressed to itself | §10.3, §11 |
| 8 | §6.5, environment | Lead panes also get `PLANNOTATOR_PORT` and `PLANNOTATOR_REMOTE` from cpd's environment, when set (several Plan reviews at once) | §15.5 |
| 9 | §6.3, tmux commands | `respawn-pane`, for a dead pane of the cockpit's layout | §13.1 |
| 10 | §1.2, output | `session run`, `tui view` and `tui --attach` own their terminal instead of printing one JSON value (the outline's "builds and attaches the cockpit") | §1.2 |
| 11 | §1.2 table of codes | the error codes this file introduces: `context_required`, `session_required`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id`, `context_exists`, `context_store_invalid`, `question_answered`, `not_a_git_repo`, `base_not_found`, `plan_exists`, `illegal_phase`, `no_draft_jobs`, `review_decided`, `dependency_cycle`, `worktree_exists`, `watch_start_failed`, `spec_invalid`, `tier_invalid`, `job_key_exists`, `needs_unknown`, `needs_unavailable`, `nothing_to_edit`, `job_merged`, `job_not_failed`, `job_not_ready`, `spawn_failed`, `attempt_not_running`, `session_malformed`, `driver_lock_missing`, `jobs_not_done`, `rebase_conflict`, `not_a_lead`, `grounded_lead`, `no_cockpit`, `wrong_pane`, `cursor_ahead`, `not_a_terminal`, `doctor_failed`, `pi_not_found`, `catalog_failed`, `file_unreadable`, `invalid_name`; most are cpd 1's (cpd 1 §11 and its `src/`) | §1.3 and each command |
| 12 | §6.1, global options | the options `focus-events on`, `pane-active-border-style fg=blue` and `pane-border-format` (titles: ` Context `, ` Plan `, ` Dispatch `, ` Worker · <job> (read-only) `; none for Leads, the chat placeholder and the StatusBar), the root `MouseDown1Pane` binding and the `pane-died` hook that respawns only a pane with `@cpd_view`: all set at every server start, not only by `cpd tui` | §13.1 |
| 13 | §6.2, §6.3 | a Worker view pane carries `@cpd_view worker`, `@cpd_job` and `@cpd_attempt`, opens with `split-window -h -d -l 50%` and retargets with `respawn-pane -k`; tmux commands `respawn-pane -k`, `set-hook` and `display-popup` | §13.2 |
| 14 | §6.4, §3.6, §5.2, §1.2 | `session close <id>`: kills a Lead's pane, the Session is `stopped` with `session-moved {place: "none"}`, the Lead in the chat slot refused (`in_chat_slot`, exit 3) | §10.9 |
| 15 | §6.3, reset-layout | `cpd tui --respawn` restarts every live view; a StatusBar added to an existing `main` is followed by reset-layout; `cpd tui` applies §6.1's options to a running server | §13.1 |
| 16 | §2.2 Plan, §4.5, §7.6 | `pr poll` stores the PR's title, review decision and checks summary in `plans.pr_title`, `pr_review_decision` and `pr_checks`, added by migration 2 | §9.2 |
| 17 | §6.2 (the views) | the Dispatch column is the machine's hostname and load, the PRs of the Plans in the cockpit, git (future) and Jira (future); no engine, slots, Escalations or Events | §13.2, §17 |
| 18 | §6.2 (the views) | the plan view's top row is ` <context id> ▸ ` and the tabs only; one tab per Lead Session of the Context with a pane; the tree's keys are ↑↓/jk, Enter, `n`, `x`, `?` and its cursor, clicks and prompts are the reference's | §13.2 |
| 19 | §6.2 (the views) | what each view reads, in process; the tree's ● (needs you) rule from Events; the plan row no longer reads Escalations, the Plan review or the PR | §17 |
| 20 | §6.2 (the views) | the legend opens from the tree's `?` and the StatusBar button, not from the Dispatch column; its section titles say Context, and it lists no `cancelled` | §13.2 |
| 21 | §6.2 (the views), R36 | the StatusBar's left side holds action buttons only (`⧉ `, ` ? legend `, ` ≡ logs `) and its right side read-only information; the Event log is the `logs` view, a popup the ` ≡ logs ` button opens, not part of the Dispatch column | §13.2, §17 |

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
