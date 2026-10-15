# cpd 2: command reference

Status: **normative for every cpd 2 implementation Job**, under [contract.md](contract.md): where the two disagree, contract.md wins and this file is fixed. Vocabulary: [CONTEXT.md](CONTEXT.md). This file uses the contract's names, states, tables and Event kinds exactly; "§n" without a prefix means contract.md §n, and "cpd 1 §n" means `docs/cli-contract.md` §n.

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
| `cpd2 events follow` | one Event per line (§5.4) | the one streaming command (§1.2) |
| `cpd2 session run`, `cpd2 tui view` (internal) | they run pi or draw a ratatui view in their pane | they are the programs inside cockpit panes (§6.2), not commands an agent calls. If they fail before taking the terminal, they print the failure envelope and exit with its code |
| `cpd2 tui --attach` | nothing on success: it replaces itself with `tmux attach` | the approved outline says `tui` "builds and attaches the cockpit"; without `--attach`, `cpd2 tui` prints JSON like every command |

### 1.3 Selection and common refusals

- **Plan:** `--plan <id>`, else `CPD_PLAN`, else `plan_required` (exit 2) (§1.3).
- **Context:** `--context <id>`, else the Context of the selected Plan when a Plan is selected (by `--plan` or `CPD_PLAN`), else `context_required` (exit 2). New: a Lead's pane has `CPD_PLAN` set (§6.5), so it never needs to name its Context.
- **Session:** `--session <id>`, else `CPD_SESSION`, else `session_required` (exit 2). `CPD_SESSION` is set in every pane cpd starts (§6.5).
- **Job, Attempt, Escalation, Plan review:** a positional id, within the selected Plan (§2.1).

Common refusals, which apply to every command and are not repeated in the sections below:

| Code | Exit | When |
|---|---|---|
| `usage` | 2 | unknown flag, missing argument, flag value of the wrong shape, exclusive flags together |
| `plan_required`, `context_required`, `session_required` | 2 | the command needs one and none was selected |
| `invalid_context_id`, `invalid_plan_id`, `invalid_job_id`, `invalid_attempt_id`, `invalid_escalation_id`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id` | 2 | an id does not match §2.1 |
| `config_missing`, `config_invalid` | 1 | §8.1 |
| `store_too_new`, `migration_failed` | 1 | §4.5 |
| `store_tampered` | 1 | a write command, when the schema check fails (§4.3); read commands report it in `tamper` instead |
| `store_busy` | 5 | no write lock within `store_busy_ms` (§4.2) |
| `context_not_found`, `plan_not_found`, `job_not_found`, `attempt_not_found`, `escalation_not_found`, `review_not_found`, `session_not_found` | 4 | the selected or named object does not exist. (A Context record that names a missing record is `record_not_found`, exit 3, §4.6) |
| `cas_lost` | 3 | a compare-and-swap found the state changed since it was read (§4.2); rerun the command |
| `illegal_transition` | 3 | a transition not in §3, refused by the command or by a trigger |
| `locked_job_tampered` | 3 | a Locked Job's fingerprint does not match (§2.3) |
| `git_failed`, `tmux_failed`, `gh_failed` | 1 | the tool exited non-zero where the command did not expect it; `details` has `argv`, `exit` and the last 60 lines of stderr |

### 1.4 Files written by commands

The `$CPD_HOME` layout is §1.4. Commands in this file write three files the contract's layout does not list (amendment 4, §19); each is beside its peers and named so it cannot collide:

| File | Written by | Read by |
|---|---|---|
| `run/<plan>/lead-message.md` | `plan new` | the Plan's Lead `session run` (its first message) |
| `run/<plan>/plan-<review>.md` | `plan review` | the pi adapter, which opens it in plannotator (§15.5) |
| `run/<plan>/messages/<session>.md` | `session fork --message-file` copies the file here | that Session's `session run` |

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
| `pr poll` | engine E5 | yes | §7.6 |
| `session list`, `session show` | TUI, Lead | no | |
| `session fork`, `open`, `park`, `rename` | User (TUI), Lead | yes | §6.4, §6.7 |
| `session state` | pi adapter | yes | §3.6 |
| `session run` (internal) | cockpit panes | yes | §6.7 |
| `events list`, `events follow` | pi adapter, TUI, scripts | no | §5.4 |
| `watch` | `plan lock`, `tui`, User | yes | §3.7, §6.6 |
| `tui`, `tui view` (internal) | User | no | §6.2 |
| `doctor`, `config`, `tiers` | User | `tiers` refreshes its cache | §8, §9 |

## 3. `context`

A Context and its records (§2.2, §4.6). Every record write follows §4.6 exactly: under `locks/context-<id>.lock`, validate, append one line to `contexts/<id>/records/<machine>.jsonl`, `git add`, `git commit`, then one transaction inserting the `context_records` row and appending `context-record` `{record, kind, machine, seq}`.

**git identity.** cpd commits in a Context store with `git -c user.name="cpd2 <machine>" -c user.email=cpd2@<machine>.invalid`, so a commit never depends on the machine's git configuration. (New: cpd 1 had no Context store.)

**Record ids.** `<machine>-<letter><n>` with `n` = 1 + the highest `n` already in this Context's index for this machine and letter (§2.1). The Context lock makes it race-free on one machine; only this machine writes its letter sequence.

**Retries.** A record write is recognised as a retry, and returns the existing record with `"already": true`, when this machine's latest record in the Context has the same `kind`, `text`, `supersedes`, `question` and `by.session`. Any other write appends a new record; records are never edited.

### 3.1 `context new`

```
cpd2 context new <id> --title <text> [--jira <key>]
```

| Flag | Meaning |
|---|---|
| `<id>` | Context id (§2.1) |
| `--title <text>` | required |
| `--jira <key>` | Jira Epic key, stored only (`^[A-Z][A-Z0-9]+-[0-9]+$`, else `usage`) |

| Precondition | Code | Exit |
|---|---|---|
| no Context row with this id, unless it has the same title and Jira key | `context_exists` | 3 |
| `contexts/<id>/` does not exist (for example cloned from another machine but not imported); `details.next` is `cpd2 context sync --context <id>` | `context_exists` | 3 |

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
cpd2 context list
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
cpd2 context show [--context <id>] [--since <seq>]
```

| Flag | Meaning |
|---|---|
| `--since <seq>` | only records whose index position `seq` is greater (exclusive, like `events --since`); what a forked Lead's first message holds (§3.1) |

Effects: none. Output: the Context, its records in index order, and `markdown`, the rendering of §2.2 "Context record" (title and Jira key; Plans; active decisions; superseded decisions with their successor; open questions; answered questions with their answers; notes; each item with its id and date). With `--since`, `markdown` renders only those records, in the same order of sections.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null, "grounded": true, "grounded_seq": 41},
 "records": [{"seq": 42, "id": "hs-d7", "kind": "decision", "at": "2026-10-14T21:12:00.000Z", "machine": "hs",
              "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "One engine per machine.", "supersedes": "hs-d2"}],
 "markdown": "# Building cpd\n\n## Plans\n…"}
```

Idempotent: read-only.

### 3.4 `context decide`, `context note`, `context ask`

```
cpd2 context decide <text> [--supersedes <record>] [--context <id>]
cpd2 context note <text> [--context <id>]
cpd2 context ask <question> [--context <id>]
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
cpd2 context answer <question> <text> [--context <id>]
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
cpd2 context sync [--context <id>]
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
cpd2 plan new <id> --context <c> --repo <path> --title <text> [--base <branch>] [--jira <key>] [--name <name>] [--brief-file <file>]
```

| Flag | Meaning |
|---|---|
| `<id>` | Plan id (§2.1) |
| `--context <c>` | required: the Plan's Context (D2) |
| `--repo <path>` | the repo; stored as the realpath of its main worktree |
| `--title <text>` | required |
| `--base <branch>` | the branch the PR targets; default the repo's `origin` HEAD branch, else `main` (cpd 1 §11 `workflow init`, kept) |
| `--jira <key>` | Jira task key, stored only |
| `--name <name>` | the Lead's pi display name; default `<id>` |
| `--brief-file <file>` | the User's brief for the Lead: its text goes into the Lead's first message |

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| `--repo` is inside a git repository (`git -C <path> rev-parse --git-common-dir`) | `not_a_git_repo` | 3 |
| `<base>` exists: `refs/remotes/origin/<base>`, or `refs/heads/<base>` when the repo has no `origin` | `base_not_found` | 4 |
| no Plan with this id, unless every flag matches it (then the result is `already`) | `plan_exists` | 3 |
| `refs/heads/cpd/<id>` does not exist in the repo (§2.1) | `branch_exists` | 3 |

Effects, in this order:
1. **Rows**, one transaction: `sessions` (new UUID v4; `role = lead`; `context`; `plan = <id>`; `name`; `cwd` = the repo; `tier = lead_tier`; `forked_from` = the Context's `lead_session` or null; `state = stopped`); `plans` (`phase = grounding`, `lead_branch = cpd/<id>`, `lead_session` = the new Session). **Events** `plan-created` `{title, repo, base, jira, lead_session, forked_from}` and `session-registered` `{role, name, forked_from, tier}`.
2. **Context store:** the record write of §3 with kind `plan` (letter `p`), `text` = the title, `plan` = `{id, machine, repo, jira}`; **Event** `context-record`.
3. **First message**, written to `run/<id>/lead-message.md`: a header (`You are the Lead of Plan <id> "<title>" in Context <c>. Repo <repo>, base <base>. Phase grounding: follow the cpd-lead skill.`), the brief file's text if given, then `context show --context <c>` rendered as markdown, with `--since <grounded_seq>` when the Lead is a fork (§3.1).
4. **tmux:** start the server if it is not running (§6.1) and the cockpit session `cpd` if it is missing; then `new-window -d -t =cpd: -n park-<id> -c <repo> -e CPD_HOME=… -e CPD_SESSION=<session> -e CPD_PLAN=<id> -P -F '#{window_id} #{pane_id}' -- cpd2 session run <session>`, and `set -p -t <pane> @cpd_session <session>`. One transaction: `sessions.tmux_pane`; **Event** `session-moved` `{pane, window, place: "parked"}`.
5. In the pane, `session run` (§10.8) forks the grounded Lead when it is idle (§6.7), or starts a fresh Lead, with the first message.

```json
{"ok": true, "plan": {"id": "pay-4521", "context": "payments", "title": "Retry failed payouts", "jira": null, "repo": "/home/clayton/projects/pay", "base": "main", "lead_branch": "cpd/pay-4521", "phase": "grounding", "lead_session": "8d0e…"},
 "lead": {"session": "8d0e…", "name": "pay-4521", "forked_from": "6f1c…", "pane": "%41", "window": "@17"},
 "record": "hs-p4"}
```

Idempotent: yes. With the same flags it returns `"already": true` and redoes any step that did not happen: the `plan` record when the index has none for this Plan, and step 4 when the Lead is `stopped` with no pane. Different flags: `plan_exists`.

### 4.2 `plan list`

```
cpd2 plan list [--context <id>] [--phase <phase>]
```

`--phase` is one of §3.1's phases. Effects: none.

```json
{"ok": true, "plans": [{"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust", "phase": "dispatch", "repo": "/home/clayton/projects/cpd", "lead_session": "596d007f-…",
  "counts": {"draft": 0, "queued": 2, "running": 1, "merged": 2, "failed": 0, "cancelled": 0}, "pr": null}],
 "events_cursor": 48213}
```

Idempotent: read-only.

### 4.3 `plan show`

```
cpd2 plan show [--plan <id>]
```

Effects: none; one read transaction, so the result is one consistent snapshot. It is the TUI's snapshot of a Plan (§17).

Output: every field of the Plan (§2.2); `jobs` with each Job's fields (Spec included), `needs`, and its Attempts (`id`, `number`, `tier`, `origin`, `status`, `outcome`, `cause`, `worker_session`, `started_at`, `ended_at`); `escalations` that are `open`; the latest Plan review; `counts`; `tamper` (the tamper report of §4.3: `{kind: "schema" | "fingerprint" | "history", …}`); `events_cursor`.

```json
{"ok": true,
 "plan": {"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust", "jira": null, "repo": "/home/clayton/projects/cpd", "base": "main", "lead_branch": "cpd/cpd2", "lead_worktree": "/home/clayton/projects/cpd.cpd2", "phase": "dispatch", "lead_session": "596d007f-…", "pr": null, "all_merged_at": null, "created_at": "…", "locked_at": "…", "done_at": null},
 "jobs": [{"key": "contract-commands", "title": "cpd 2 contract: every command", "status": "running", "tier": "O3", "appended": false, "needs": ["contract-core"],
           "goal": "…", "context": "…", "touches": ["docs/v2/commands.md"], "acceptance": ["…"], "out_of_scope": ["…"],
           "attempts": [{"id": "contract-commands-a1", "number": 1, "tier": "O3", "origin": "ladder", "status": "running", "outcome": null, "cause": null, "worker_session": "c1a2…", "started_at": "…", "ended_at": null}]}],
 "escalations": [{"id": "contract-commands-a1-q1", "attempt": "contract-commands-a1", "question": "…", "asked_at": "…"}],
 "review": {"id": "r2", "status": "approved", "fingerprint": "9f…", "current": true},
 "counts": {"draft": 0, "queued": 0, "running": 1, "merged": 1, "failed": 0, "cancelled": 0},
 "tamper": [], "events_cursor": 48213}
```

Idempotent: read-only. `tamper` non-empty is not an error (cpd 1 §11 `workflow show`, kept).

### 4.4 `plan phase`

```
cpd2 plan phase plan [--plan <id>]
```

The only phase moved by hand (cpd 1 §11 `workflow phase`, kept). Any other target is `illegal_transition` (exit 3).

| Precondition | Code | Exit |
|---|---|---|
| the phase is `grounding` (already `plan`: `already`) | `illegal_transition` | 3 |

Effects: exactly §3.1 "The Context's grounded Lead":
- **Rows**, one transaction: `plans.phase = plan`; the compare-and-swap on `contexts.lead_session` and `grounded_seq`; only if it changed a row, a `sessions` row for the grounded Lead (`role = lead`, `plan = null`, `forked_from` = this Plan's Lead, `name` = `<context> (grounded)`, `cwd` = this Lead's cwd, `tier = lead_tier`, `stopped`). **Events** `plan-phase` `{from: "grounding", to: "plan"}`, and with the new Session `session-registered` and `context-grounded` `{seq, forked_from}`.
- **tmux**, after the commit and only with the new Session: `new-window -d -t =cpd: -n ctx-<context> … -- cpd2 session run <grounded id>`, `@cpd_session` on its pane; one transaction: `tmux_pane` and `session-moved` `{place: "parked"}`. Its `session run` waits until this Lead is idle, then forks it with no first message (§6.7).

```json
{"ok": true, "plan": "pay-4521", "phase": "plan", "grounded_lead": {"session": "6f1c…", "pane": "%44", "window": "@19"}}
```

`grounded_lead` is null when the Context already had one. Idempotent: yes; in phase `plan` it returns `"already": true`, and creates the `ctx-<context>` window when the Context's grounded Lead was forked from this Plan's Lead and has no pane (a crash between the two transactions).

### 4.5 `plan render`

```
cpd2 plan render [--plan <id>]
```

Effects: writes `run/<plan>/plan.md` (§1.4), nothing else. Content, kept from cpd 1 §11 `plan render`: title, Context and phase; a table of every Job not `cancelled` with key, title, Tier, status, needs and touches; a Mermaid `graph TD` of the Dependencies; one section per Job with its Spec (goal, context, touches, acceptance, out of scope). GitHub-flavoured Markdown only: tables, code and Mermaid, no footnotes, no raw HTML (t4). Output: the path, the plan fingerprint (§2.3) and the number of `draft` Jobs.

```json
{"ok": true, "path": "/home/clayton/.cpd/run/pay-4521/plan.md", "fingerprint": "9f86d081…", "draft_jobs": 6}
```

Idempotent: yes (the same Jobs render the same file). Change: cpd 1's `--out` is dropped; the file has one place.

### 4.6 `plan review`

```
cpd2 plan review [--plan <id>]
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

Idempotent: yes. When the latest review is `pending` for the current fingerprint, it returns that review with `"already": true` and writes nothing. When the Jobs changed since, it requests a new review; the older one stays `pending` and no longer matters, because `plan lock` reads only the latest (§2.3).

### 4.7 `plan decide`

```
cpd2 plan decide <review> (--approve | --reject) [--feedback-file <file>] [--plan <id>]
```

Records the User's decision. Called by the pi adapter with plannotator's result (§15.5); replaces cpd 1's `plan decision` (cpd 1 §11), whose `--pending` step is now `plan review`. The cpd-lead skill forbids the Lead to run it.

| Precondition | Code | Exit |
|---|---|---|
| the review is `pending` (decided the same way: `already`) | `review_decided` | 3 |

Effects: **rows**, one transaction: `plan_reviews.status`, `feedback` (the file's text, or null), `decided_at`. **Event** `plan-review` `{review, fingerprint, status, feedback}`.

```json
{"ok": true, "review": {"id": "r3", "status": "approved", "fingerprint": "9f86d081…", "feedback": null, "decided_at": "…"}, "current": true, "next": "cpd2 plan lock"}
```

`current` says whether the review's fingerprint is still the plan fingerprint; `next` is `cpd2 plan lock` only when approved and current. Idempotent: yes, as stated.

### 4.8 `plan lock`

```
cpd2 plan lock [--plan <id>]
```

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan` (`dispatch`: `already`) | `illegal_transition` | 3 |
| at least one Job is `draft` | `no_draft_jobs` | 3 |
| the latest Plan review is `approved` for the current plan fingerprint | `plan_not_approved` | 3 |
| the Dependency graph of the draft Jobs is acyclic | `dependency_cycle` | 3 |
| the Lead worktree path does not exist, except a leftover of an interrupted lock (a worktree of this repo on `cpd/<plan>`), which is removed first | `worktree_exists` | 3 |
| `refs/heads/cpd/<plan>` does not exist, except that leftover's branch | `branch_exists` | 3 |
| `pre-start` passed in the new worktree (§7.3) | `setup_failed` | 1 |

Effects:
- **git** (§7.2): `fetch origin <base>`; `worktree add -b cpd/<plan> <lead worktree> origin/<base>` (or `<base>`); `pre-start`. On any failure the worktree and branch are removed and nothing in the database changes.
- **Rows**, one transaction, with a compare-and-swap on `phase = 'plan'` and on the review read: `plans.phase = dispatch`, `lead_worktree`, `locked_at`; every `draft` Job `queued` with its Locked-Job fingerprint. **Event** `plan-phase` `{from: "plan", to: "dispatch", jobs: [key]}`. If the transaction fails, the worktree and branch are removed.
- **tmux:** `watch --ensure` (§12): starts `cpd2 watch` in `cpd-work:watch` when `locks/watch.lock` is free.

```json
{"ok": true, "plan": "pay-4521", "phase": "dispatch", "lead_branch": "cpd/pay-4521", "lead_worktree": "/home/clayton/projects/pay.pay-4521", "base_sha": "4b825dc…",
 "jobs": ["api-schema", "worker", "ui"], "watch": {"started": true, "pane": "%3"}}
```

If the engine cannot be started, the Plan stays locked and the result is `watch_start_failed` (exit 1) with `details.next` = `cpd2 watch --ensure` (cpd 1's `watch_pane_failed`, renamed). Idempotent: yes; in phase `dispatch` it returns `"already": true` and still runs `watch --ensure`.

## 5. `job`

Jobs, Specs and Dependencies (§2.2, §3.2). Kept from cpd 1 §11 "Jobs" except where stated.

**Spec file** (`--spec`), JSON, every field required, unknown fields refused (`spec_invalid`, exit 2, with the JSON path in `details`):

```json
{"goal": "…", "context": "…", "touches": ["docs/v2/commands.md"], "acceptance": ["…"], "out_of_scope": ["…"]}
```

`touches` and `acceptance` have at least one non-empty string; `out_of_scope` may be empty. Change: cpd 1's `outOfScope` is now `out_of_scope`, the column's name.

**Tier flag** (`--tier`): `^(O|S|H)([0-5])$` with an effort the letter's model supports (§9), else `tier_invalid` (exit 2), with `details.efforts` listing the valid ones.

### 5.1 `job add`

```
cpd2 job add <key> --title <text> --tier <T> --spec <file> [--needs <k1,k2>] [--plan <id>]
```

| Flag | Meaning |
|---|---|
| `<key>` | Job key (§2.1). Change: positional, like every other `job` command (cpd 1 had `--key`) |
| `--needs <k1,k2>` | Dependencies, comma-separated, no duplicates (`usage`) |

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan`, `dispatch` or `pr` | `illegal_phase` | 3 |
| no Job with this key, unless its Locked fields and needs are identical (then `already`) | `job_key_exists` | 3 |
| every `needs` key is a Job of the Plan | `needs_unknown` | 3 |
| no `needs` Job is `cancelled` or `failed` | `needs_unavailable` | 3 |

Effects: **rows**, one transaction: `jobs` (`status = draft`, `appended = 0`, no fingerprint, in phase `plan`; `status = queued`, `appended = 1`, fingerprint stored, in `dispatch` or `pr`); one `dependencies` row per need. **Event** `job-added` `{status, appended, tier, needs}`. No cycle is possible: nothing needs a new Job yet.

```json
{"ok": true, "job": {"key": "api-schema", "status": "draft", "appended": false, "tier": "S2", "needs": []}}
```

Idempotent: yes, as stated.

### 5.2 `job edit`

```
cpd2 job edit <key> [--title <text>] [--tier <T>] [--spec <file>] [--needs <k1,k2> | --no-needs] [--plan <id>]
```

`--no-needs` removes every Dependency (cpd 1 had no way to).

| Precondition | Code | Exit |
|---|---|---|
| at least one field flag | `nothing_to_edit` | 2 |
| the Job is `draft` | `job_locked` | 3 |
| the needs exist and are usable | `needs_unknown`, `needs_unavailable` | 3 |
| the new needs make no cycle | `dependency_cycle` | 3 |

Effects: **rows**, one transaction: the changed `jobs` columns; `dependencies` rows deleted and inserted to match (§4.4 allows deletion only for `draft`). **Event** `job-edited` `{fields: [name]}`, listing only the fields whose value changed.

```json
{"ok": true, "job": {"key": "api-schema", "status": "draft"}, "fields": ["tier", "needs"]}
```

Idempotent: yes; when no value changes it writes nothing and returns `"already": true`.

### 5.3 `job cancel`

```
cpd2 job cancel <key> --reason <text> [--plan <id>]
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
cpd2 job retry <key> --tier <T> [--plan <id>]
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
cpd2 job dispatch <key> [--plan <id>]
```

Starts one Ready Job now: the same step as engine E3 (cpd 1 §11, kept).

| Precondition | Code | Exit |
|---|---|---|
| the Job is Ready (§3.2) | `job_not_ready` | 3 |
| its fingerprint matches | `locked_job_tampered` | 3 |
| the driver lock is free (§3.4 step 1) | `cas_lost` | 3 |
| fewer than `concurrency` Attempts are not `ended` | `no_slot` | 3 |

Effects (§3.4 "The driver lock"): take `locks/attempt-<plan>--<attempt>.lock`; **rows**, one transaction: `attempts` (`starting`, Tier from §9, `origin`), Job `running`; **Event** `attempt-started` `{number, tier, origin}`; then spawn `cpd2 attempt run <attempt> --plan <plan>` with `setsid`, fd 3 = the lock, stderr to `run/<plan>/logs/attempt-<attempt>.log`, and record `driver_pid` (a data write, no Event). If the spawn fails, the command ends the Attempt `failed` / `crashed` (`attempt-retry` or `job-failed`) and returns `spawn_failed` (exit 1).

```json
{"ok": true, "attempt": "api-schema-a1", "tier": "S2", "origin": "ladder", "pid": 81234}
```

Idempotent: no. A second call finds the Job `running` and is refused `job_not_ready`.

### 5.6 `job list`

```
cpd2 job list [--status <status>] [--plan <id>]
```

`--status` is one of §3.2's statuses and may repeat. Effects: none.

```json
{"ok": true, "jobs": [{"key": "api-schema", "title": "…", "status": "merged", "tier": "S2", "appended": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee…"}]}
```

### 5.7 `job show`

```
cpd2 job show <key> [--plan <id>]
```

Effects: none. Output: every field of the Job (§2.2), `needs`, `needed_by`, and every Attempt with all its fields (`check_tail`, `metrics`) and its Escalations (cpd 1 §11, kept).

```json
{"ok": true, "job": {"key": "worker", "status": "failed", "failed_cause": "timed_out", "tier": "S2", "…": "…"}, "needs": ["api-schema"], "needed_by": ["ui"],
 "attempts": [{"id": "worker-a1", "status": "ended", "outcome": "failed", "cause": "timed_out", "check_tail": null, "escalations": []}]}
```

Both are read-only.

## 6. `attempt`

### 6.1 `attempt submit`

```
cpd2 attempt submit <attempt> [--plan <id>]
```

Run by the Worker, from its worktree. Preconditions are §7.3's list, in that order, after the Attempt's own state:

| Precondition | Code | Exit |
|---|---|---|
| the Attempt is `running` (already `merging` or later with `submitted_sha` = the worktree's HEAD: `already`) | `attempt_not_running` | 3 |
| the caller's cwd is inside the Attempt worktree, on the Attempt branch | `wrong_worktree` | 3 |
| clean tree, untracked files included | `dirty_tree` | 3 |
| at least one commit ahead of `base_sha` | `no_commits` | 3 |
| the tamper check passes (§7.5); `details.paths` lists the protected paths touched | `gate_tampered` | 3 |
| the quick check passes (no `pre-commit` table: it passes without running, R20); `details.tail` is its last 60 lines, `details.log` the full log | `check_failed` | 3 |
| the quick check ran (not killed, not `hook_invalid`) | `check_error` | 1 |

Effects:
- **Hooks:** `pre-commit` only, in the worktree (§7.3); output to `run/<plan>/logs/<attempt>.pre-commit.log`. With no `pre-commit` table nothing runs, no log is written and `check_tail` stays null; `pre-merge` never runs at submit (R20).
- **Rows**, one transaction with a compare-and-swap on `status = 'running'` and on the HEAD checked: Attempt `merging`, `submitted_sha`, `submitted_at`, `check_tail`; the Job stays `running`. **Event** `attempt-status` `{from: "running", to: "merging"}`.
- The driver sees `merging` at A5 and starts the merge (A8, §7.4).

```json
{"ok": true, "attempt": "api-schema-a1", "submitted": true, "sha": "a1b2c3d…", "next": "Stop now. Do not make more changes."}
```

Idempotent: yes, as stated; a HEAD that moved after a successful submit is refused `attempt_not_running`.

### 6.2 `attempt read`

```
cpd2 attempt read <attempt> [--lines <n>] [--plan <id>]
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
cpd2 attempt show <attempt> [--plan <id>]
cpd2 attempt list [--job <key>] [--active] [--plan <id>]
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
cpd2 attempt run <attempt> --plan <id>
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
cpd2 ask <attempt> <question> [--timeout-minutes <n>] [--plan <id>]
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
cpd2 answer <escalation> <answer> [--plan <id>]
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

Through `gh` (§7.6). `gh` failures are `gh_failed` (exit 1) with its stderr tail.

### 9.1 `pr open`

```
cpd2 pr open [--plan <id>]
```

| Precondition | Code | Exit |
|---|---|---|
| the phase is `dispatch` (`pr` or `done`: `already`) | `illegal_phase` | 3 |
| every Job `merged` or `cancelled`, at least one `merged` | `jobs_not_done` | 3 |
| the per-repo lock within `lock_wait_seconds` | `lock_timeout` | 5 |
| the Lead worktree is clean | `dirty_target` | 3 |
| the Gate passes (§7.3, §7.6 `pr open` step 2); `details.tail` is its last 60 lines, `details.log` the full log | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |

Effects:
- **Lock:** the per-repo lock (§7.4), then R for orphaned merges; held until the push has finished.
- **Hooks:** the Gate, `pre-merge` from `<lead worktree>/.config/wt.toml`, in the Lead worktree on the Lead Branch, before anything is pushed (R19); output to `run/<plan>/logs/pr.pre-merge.log`. On `gate_failed` or `gate_error` nothing is pushed, no row is written and the phase stays `dispatch`: the Lead appends fix Jobs (`job add`) and runs `pr open` again once they are Merged.
- **git:** `git -C <lead worktree> push -u origin cpd/<plan>`.
- **gh:** `gh pr list --head cpd/<plan> --state open --json number,url`; when a PR exists (an earlier run died before its transaction) it is adopted; else `gh pr create --base <base> --head cpd/<plan> --title <title> --body <summary>`, the summary being the Jobs table and the Merged commits (cpd 1 §11, kept).
- **Rows**, one transaction: `pr_number`, `pr_url`, `pr_opened_at`, `phase = pr`. **Event** `plan-phase` `{from: "dispatch", to: "pr", pr: {number, url}}`.

```json
{"ok": true, "plan": "pay-4521", "phase": "pr", "pr": {"number": 812, "url": "https://github.com/acme/pay/pull/812", "opened_at": "…"}}
```

Idempotent: yes, as stated.

### 9.2 `pr poll`

```
cpd2 pr poll [--plan <id>]
```

Run by engine E5 every `pr_poll_seconds` as a subprocess; also by hand.

| Precondition | Code | Exit |
|---|---|---|
| the phase is `pr` (`done`: `already`) | `illegal_phase` | 3 |

Effects:
- **gh:** `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments` and `gh api repos/{owner}/{repo}/pulls/<n>/comments`, compared with `pr_seen` (cpd 1 §13, kept).
- **Rows**, one transaction: `pr_seen`, `pr_polled_at`. New failing checks, reviews or comments, or `state = CLOSED` without a merge: **Event** `pr-feedback` (data of §5.2). Merged: `phase = done`, `done_at`, **Event** `plan-done` `{pr, merged_at}`.
- **After `done`:** the Plan's remaining Attempt worktrees and branches, kept for debugging (§3.4 A9), are removed (`git worktree remove --force`, `git branch -D`), and any of its Attempt windows killed. The Lead worktree and the Lead Branch stay.

```json
{"ok": true, "plan": "pay-4521", "state": "OPEN", "merged": false, "changes": {"checks": [{"name": "ci/test", "conclusion": "FAILURE"}], "reviews": [], "comments": []}}
```

Idempotent: yes; what was reported is in `pr_seen`, so a rerun reports nothing new.

### 9.3 `pr rebase`

```
cpd2 pr rebase [--plan <id>]
```

Rebases the Lead Branch onto the base and pushes (§7.6); also how Jobs Merged in phase `pr` reach the PR.

| Precondition | Code | Exit |
|---|---|---|
| the phase is `pr` | `illegal_phase` | 3 |
| the per-repo lock within `lock_wait_seconds` | `lock_timeout` | 5 |
| the Lead worktree is clean | `dirty_target` | 3 |
| `git merge-tree` reports no conflict; `details.files` lists them | `rebase_conflict` | 3 |
| the Gate passes on the rebased Lead Branch (§7.3, §7.6 `pr rebase` step 4); `details.tail` is its last 60 lines, `details.log` the full log | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |

Effects: under the per-repo lock, after R (§7.4): §7.6 steps 1–5 (`fetch`, `merge-tree`, `rebase`, the Gate, `push --force-with-lease`). The Gate is `pre-merge` from `<lead worktree>/.config/wt.toml`, run in the Lead worktree after the rebase and before the push (R19), output to `run/<plan>/logs/pr.pre-merge.log`. On `gate_failed` or `gate_error` nothing is pushed; the rebased Lead Branch stays in the Lead worktree, and the Lead appends fix Jobs, then runs `pr rebase` again once they are Merged. **Rows**: none; the Ledger does not record the Lead Branch's SHA, so there is no Event. On an unexpected rebase conflict: `git rebase --abort` and `rebase_conflict`.

```json
{"ok": true, "plan": "pay-4521", "rebased": true, "old": "4b825dc…", "new": "77aa01e…", "pushed": true}
```

Idempotent: yes; a base that has not moved gives `"rebased": false`, and only the Gate and the push run.

## 10. `session`

Sessions (§2.2, §3.6) and their tmux panes (§6). Every tmux call is `tmux -L <tmux.socket> …` with `TMUX` and `TMUX_PANE` removed (§6.1); a failing call is `tmux_failed` (exit 1). A Session's **place** is the `place` of its latest `session-moved` Event (`chat`, `parked`, `work` or `none`), or `none` when it has none.

**The chat slot.** **Amends contract.md §6.2** (the Lead's answer to Escalation `contract-commands-a1-q1`): the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat`. A pane with `@cpd_view = chat` is the **placeholder**: it runs `cpd2 tui view chat`, which reads `No Lead here: pick a Plan in the session tree`, and it holds the slot while no Lead is there. **Amends §6.2's list of `@cpd_view` values** with `chat`. Why: the slot must hold a pane before any Lead exists (the first `cpd2 tui` on a machine) and after its Lead is parked (`swap-pane` needs a pane to swap in). At most one placeholder exists, and only ever in the slot: `session park` creates it there (§10.5), and `session open` kills it with its window when a Lead takes the slot (§10.4).

### 10.1 `session list`

```
cpd2 session list [--context <id>] [--plan <id>] [--role <role>] [--state <state>]
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
cpd2 session show [<id>]
```

`<id>` defaults to the selected Session (`CPD_SESSION`). Output: the fields of `session list` plus `created_at`, `registered_event` (the id of its `session-registered` Event, the pi adapter's first cursor, §15.2) and, for a Plan's Lead, `open_escalations` (ids). Effects: none.

```json
{"ok": true, "session": {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "state": "idle", "state_label": null, "place": "chat", "tmux_pane": "%12", "file": "…", "forked_from": "6f1c…", "plan_lead": true, "grounded_lead": false, "tier": "O3", "cwd": "/home/clayton/projects/cpd", "created_at": "…", "registered_event": 40117},
 "open_escalations": ["contract-commands-a1-q1"]}
```

Read-only.

### 10.3 `session fork`

```
cpd2 session fork <source> [--plan <id>] [--name <name>] [--message-file <file>]
```

§6.7. Registers a new Lead Session forked from `<source>` and starts it in a parked window; the waiting for the source to be idle happens in that window, so the command returns at once.

| Flag | Meaning |
|---|---|
| `<source>` | the Session to fork; a Lead |
| `--plan <id>` | the new Session's Plan; default the source's Plan. The fork is a **side session** of the Plan: the Plan's `lead_session` does not change, so Plan Events still go to the Plan's Lead only (§15.2) |
| `--name <name>` | default `<source name> (fork)`, cut to 80 characters |
| `--message-file <file>` | its first message, copied to `run/<plan>/messages/<session>.md`; without it the fork starts with no message |

| Precondition | Code | Exit |
|---|---|---|
| the source is a `lead` Session | `not_a_lead` | 3 |
| a Plan is known: `--plan`, else the source's Plan (a grounded Lead has none and is forked only by `plan new`) | `plan_required` | 2 |
| the cockpit's tmux session (`tmux.cockpit_session`) exists, checked before anything is written; `details.next` = `cpd2 tui` | `no_cockpit` | 3 |

Effects:
- **Rows**, one transaction: `sessions` (new UUID v4, `role = lead`, the Plan's Context, `forked_from = <source>`, `cwd` = the source's, `tier = lead_tier`, `stopped`). **Event** `session-registered`.
- **tmux:** `new-window -d -t =cpd: -n park-<label> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<id> -e CPD_PLAN=<plan> -- cpd2 session run <id>`; `@cpd_session`. One transaction: `tmux_pane`, **Event** `session-moved` `{place: "parked"}`.
- If the cockpit disappears between that check and `new-window`, the command fails with `tmux_failed` after the Session is registered (Escalation `rs-session-a1-q2`).

```json
{"ok": true, "session": {"id": "a41b…", "name": "cpd rust lead (fork)", "plan": "cpd2", "forked_from": "596d007f-…", "pane": "%52", "window": "@23"}}
```

Idempotent: no; each call registers a new Session.

### 10.4 `session open`

```
cpd2 session open <id>
```

Brings a Lead into the chat slot (§6.4).

| Precondition | Code | Exit |
|---|---|---|
| the Session is a `lead` | `not_a_lead` | 3 |
| it has a Plan (a grounded Lead is never typed into, §3.1) | `grounded_lead` | 3 |
| the cockpit's `main` window and its chat slot exist; `details.next` = `cpd2 tui` | `no_cockpit` | 3 |

Effects:
- In the chat slot already: nothing; `"already": true`.
- **tmux:** when the Session has no live pane (`stopped`, or its pane is gone): `new-window -d -t =cpd: -n park-<label> … -- cpd2 session run <id>` first, which resumes it (`pi --session <file>`, §6.5), and `tmux_pane` is recorded. Then `swap-pane -d -s <its pane> -t <chat slot pane>`, and `rename-window -t <window> park-<displaced label>` for the window that now holds the displaced Lead. When the displaced pane is the placeholder, `kill-pane` on it instead, which closes the now-empty park window with it, so placeholders never pile up in park windows.
- **Rows**, one transaction: **Events** `session-moved` `{place: "chat"}` for this Session and `{place: "parked"}` for the displaced one.

```json
{"ok": true, "session": "8d0e…", "pane": "%41", "displaced": "596d007f-…"}
```

Idempotent: yes.

### 10.5 `session park`

```
cpd2 session park [<id>]
```

Moves the Lead in the chat slot back into a parked window (§6.4). `<id>` defaults to the Session in the chat slot.

| Precondition | Code | Exit |
|---|---|---|
| the cockpit exists | `no_cockpit` | 3 |
| the Session is a `lead` | `not_a_lead` | 3 |

Effects:
- Not in the chat slot: nothing; `"already": true`.
- **tmux** (**amends contract.md §6.4**, which says only that park moves the Lead into a parked window): `new-window -d -t =cpd: -n park-<label> -P -F '#{window_id} #{pane_id}' -- cpd2 tui view chat` (a new park window holding a placeholder), `set -p -t <placeholder> @cpd_view chat`, then `swap-pane -d -s <placeholder> -t <chat slot pane>`. The Lead keeps its pid and is now alone in `park-<label>`; the placeholder fills the slot.
- **Rows**, one transaction: **Event** `session-moved` `{place: "parked"}`.

```json
{"ok": true, "session": "8d0e…", "pane": "%41", "window": "@31"}
```

Idempotent: yes.

### 10.6 `session state`

```
cpd2 session state <working|blocked|idle|stopped> [--label <text>] [--session <id>]
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
cpd2 session rename <name> [--session <id>]
```

cpd 1 §11 `session rename`, kept; the queue file becomes an Event.

Effects: unchanged name: `"already": true`, nothing written. Otherwise **rows**, one transaction: `sessions.name`. **Event** `session-renamed` `{name}`, addressed to the Session (`session` set), which its adapter applies with `pi.setSessionName` (§15.4). No tmux.

```json
{"ok": true, "session": "596d007f-…", "name": "cpd rust lead"}
```

Idempotent: yes.

### 10.8 `session run` (internal)

```
cpd2 session run <id>
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

## 11. `events`

§5. Selection: `--plan <id>`, `--context <id>` or `--session <id>`, at most one (`usage`), or none for every Event; `--kind <kind>` may repeat (a kind not in §5.2 is `usage`). These flags are filters: they do not fall back to `CPD_PLAN` or `CPD_SESSION`.

`--session <id>` applies §5.3's rule, with "a Lead of Plan P" made precise (amendment 7, §19):
- for the Session that is its Plan's `lead_session`: Events with `plan` = its Plan; Events with `plan IS NULL AND context` = its Context; Events with `session` = itself;
- for any other Session (Workers, side forks, a grounded Lead): Events with `session` = itself.

### 11.1 `events list`

```
cpd2 events list [--plan <id> | --context <id> | --session <id>] [--kind <kind>]… [--since <id>] [--limit <n>]
```

`--since` is exclusive and defaults to 0; `--limit` defaults to 500 (1–5000). Effects: none.

```json
{"ok": true, "events": [{"id": 48214, "at": "…", "kind": "job-merged", "context": "building-cpd", "plan": "cpd2", "job": "contract-core", "attempt": "contract-core-a1", "session": null,
  "text": "Job contract-core merged at c0ffee1 (Tier O3). Next: cpd2 plan show", "data": {"sha": "c0ffee1…", "tier": "O3"}}], "next": 48214}
```

`next` is the id of the last Event returned, or `--since` when none; pass it as the next `--since`. Idempotent: read-only.

### 11.2 `events follow`

```
cpd2 events follow [--plan <id> | --context <id> | --session <id>] [--kind <kind>]… [--since <id>] [--until-idle]
```

§5.4, normative: one Event per line, in id order; without `--since`, only Events committed after it starts; polling `PRAGMA data_version` every `events.poll_ms`; ends on EPIPE or kill with exit 0, or with `--until-idle` once it has printed everything committed when it started.

| Precondition | Code | Exit |
|---|---|---|
| the selected Plan, Context or Session exists | `plan_not_found`, `context_not_found`, `session_not_found` | 4 |
| `--since` is not above the highest Event id (a cursor from another database) | `cursor_ahead` | 3 |

A failure after the first line (for example `store_too_new` after an upgrade) is printed as the last line, as the failure envelope, and the exit code is its exit code (§1.2). Each line:

```json
{"id": 48215, "at": "2026-10-14T21:41:02.114Z", "kind": "escalation", "context": "building-cpd", "plan": "cpd2", "job": "contract-commands", "attempt": "contract-commands-a1", "session": null, "text": "Worker contract-commands-a1 asks: … Answer: cpd2 answer contract-commands-a1-q1 \"<answer>\"", "data": {"escalation": "contract-commands-a1-q1", "question": "…"}}
```

Idempotent: read-only; the same `--since` replays the same lines.

## 12. `watch`

```
cpd2 watch [--ensure]
```

The engine, one per machine (§3.7, §6.6, D5).

**Without `--ensure`:** runs E1–E6 every `tick_seconds` in the foreground, in `cpd-work:watch`, logging to stderr and appending the same lines to `run/watch.log`. At start it imports the Context stores (§4.6).

| Precondition | Code | Exit |
|---|---|---|
| `locks/watch.lock` is free | `watch_running` | 3 |

Effects: **Events** `watch-started` and `watch-stopped` `{pid}`, and every row and Event of E1–E6 (§3.7). An unexpected failure in a step appends `error` `{source: "watch", code, message, details}` for the Plan it concerns and the loop continues; the same failure for the same object is appended once until it clears. It runs until SIGTERM or SIGINT, then prints `{"ok": true, "stopped": true}` and exits 0. Idempotent: a second one is refused.

**With `--ensure`:** starts the engine when it is not running, and returns at once.

| Precondition | Code | Exit |
|---|---|---|
| the window started and its process took the lock within 10 s | `watch_start_failed` | 1 |

Effects: probe `locks/watch.lock` (open and `flock(LOCK_EX | LOCK_NB)`, released at once). Free: start the server and `cpd-work` session if needed (§6.1), kill a dead `watch` window, then `new-window -d -t =cpd-work: -n watch -c $CPD_HOME -P -F '#{pane_id}' -- cpd2 watch`. No rows. The engine itself appends its stderr to `run/watch.log` (§1.4), so no shell is needed.

```json
{"ok": true, "started": true, "pane": "%3"}
```

Idempotent: yes; a running engine gives `"started": false` with `"already": true`.

## 13. `tui`

### 13.1 `tui`

```
cpd2 tui [--attach]
```

Builds the cockpit (§6.1, §6.2) unless it exists, and starts the engine.

Effects, each only when missing:
- **tmux:** the server with §6.1's options; the session `cpd` with window `main` (`new-session -d -s cpd -n main -x 215 -y 62`), split into the tree (left, 20 %), the plan view (centre top) above the chat slot (centre bottom), and the widgets (right, 20 %), each running `cpd2 tui view <tree|plan|widgets>` with `@cpd_view` set; the chat slot holds the placeholder (§10) or, when one exists, the Plan Lead whose latest `session-moved` Event with `place: "chat"` is the newest, moved in as `session open` does. The session `cpd-work`.
- `watch --ensure` (§12).
- No rows of its own; moving a Lead appends `session-moved`.

| Precondition | Code | Exit |
|---|---|---|
| with `--attach`: stdin and stdout are a terminal | `not_a_terminal` | 3 |

```json
{"ok": true, "built": true, "socket": "cpd", "session": "cpd", "attach": ["tmux", "-L", "cpd", "attach", "-t", "cpd"], "watch": {"started": true, "pane": "%3"}}
```

With `--attach` it then replaces itself with `tmux -L cpd attach -t cpd` and prints nothing (§1.2). Idempotent: yes; an existing cockpit gives `"built": false`. A pane of the layout that has died is respawned (`respawn-pane`, amendment 9, §19), never duplicated.

### 13.2 `tui view` (internal)

```
cpd2 tui view <tree|plan|widgets|chat> 
cpd2 tui view worker <attempt> --plan <id>
```

The ratatui programs in the cockpit's panes (§6.2). They own their terminal (§1.2), make no writes of their own, and change state only by running `cpd2` commands (`session open`, `plan new`, `session park`, …) as subprocesses. What they read is §17. The `worker` view renders the Attempt's Worker session file read-only (vision decision 5); `chat` is the chat slot's placeholder (§10). Refusal before drawing: an unknown view or Attempt is `usage` or `attempt_not_found`. Idempotent: they only read.

## 14. `doctor`, `config`, `tiers`

### 14.1 `doctor`

```
cpd2 doctor
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
cpd2 config
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
cpd2 tiers
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

The pi adapter is the TypeScript package `pi/` (decisions: "the pi adapter stays TypeScript"), loaded in every pi session cpd starts (§6.5). It is the only pi-specific code: it turns Events into messages (§5.3), reports agent states (§3.6) and runs the Plan review through plannotator. It calls only `cpd2` commands; it never reads the database and never types into a pane (cpd 1 §15, kept).

### 15.1 Package and activation

- **Package:** `pi/package.json` with `"keywords": ["pi-package"]` and `"pi": {"extensions": ["./extension.ts"], "skills": ["./skills"]}`; host packages are `peerDependencies: "*"` (cpd 1 §15, kept). The skill `cpd-lead` is rewritten for cpd 2 (follow-up Job 17).
- **Activation:** at pi's `session_start`, only when `ctx.mode` is `tui` and `CPD_SESSION` is set. Change from cpd 1: cpd 1 activated in any herdr or tmux pane (`HERDR_ENV`, `TMUX_PANE`); now cpd marks its own panes with `CPD_SESSION` (§6.5), so a pi the User starts by hand is left alone.
- **Identity:** `CPD_SESSION` must equal `ctx.sessionManager.getSessionId()`. If not, the adapter delivers one error message (below) and stays inactive: it would otherwise report another Session's state.
- **Calls:** one-shot commands through `pi.exec("cpd2", args)`, whose stdout is parsed as the JSON envelope at the boundary (a non-JSON stdout, or `ok: false`, is an error with the code). The follower is a child process (`node:child_process` `spawn`, argv, stdin `/dev/null`) read line by line.
- **Errors** are delivered to the Session as `pi.sendMessage({customType: "cpd", content: "cpd adapter: <what>: <code>: <message>", display: true, details: {kind: "error", code, …}}, {triggerTurn: true, deliverAs: "followUp"})` (cpd 1's `errorDelivery`, kept). The adapter never throws into pi.

### 15.2 Start-up and the cursor

At `session_start`, in this order:
1. `cpd2 session show <id>`: role, Plan, Context, `plan_lead`, `registered_event`, name.
2. `cpd2 session state idle --session <id>` (`stopped → idle`, §3.6); then `working` if `ctx.isIdle()` is false.
3. **Name:** if pi's session name differs from the Session's `name`, `pi.setSessionName(name)`: a `session-renamed` missed while pi was down still applies.
4. **Cursor** (§5.3 "No loss, no duplicates"): the greater of `registered_event` and the `details.id` of the last `cpd` custom message in the session's entries whose `details.delivered_to` is this Session's id. The adapter puts `delivered_to` into every message it delivers (amendment 5, §19). Why: a fork's file starts with its source's messages, whose Event ids belong to the source's stream; counting them could skip Events addressed to the fork.
5. **Blocking reasons** (Plan Lead only): one per `open` Escalation of its Plan (`cpd2 plan show`), and one per pending Plan review this session opened (its `cpd-plan-review` entries, §15.5).
6. **Follow:** spawn `cpd2 events follow --session <id> --since <cursor>`.

The cursor in memory advances after each line is handled, delivered or not. Only delivered messages persist it, so after a restart the Events handled without a message (`escalation-answered`, `escalation-expired`, `session-renamed`, `plan-review`) are seen again; every handler below is idempotent for that reason.

**The follower ends** (EOF, exit, or a failure line): the adapter restarts it with `--since <in-memory cursor>` after 1 s, then backing off to at most 30 s. A failure line or a non-zero exit is delivered as an error message once per error code, until a line is read again. A line that is not a valid Event (§5.2 fields, a known kind) is delivered as an error message and skipped, and the cursor moves past it, so one bad line cannot stop delivery. At `session_shutdown` the follower is killed.

### 15.3 What each Event does

Events arrive in id order; the follower already applied §11's `--session` rule, so a Lead receives its Plan's Events and a Worker or side fork only those addressed to it. Delivery is `pi.sendMessage({customType: "cpd", content: event.text, display: true, details: {...event, delivered_to: <id>}}, {triggerTurn, deliverAs: "followUp"})` (§5.3).

| Kind | Plan Lead | Worker, side fork, grounded Lead |
|---|---|---|
| `job-merged`, `job-failed`, `all-merged`, `pr-feedback`, `plan-done`, `error` | message, **turn** | |
| `attempt-retry`, `job-cancelled` | message, no turn | |
| `escalation` | message, **turn**; opens the blocking reason `escalation:<id>` labelled `cpd: Worker question` | |
| `escalation-answered`, `escalation-expired` | closes `escalation:<id>`; no message | |
| `plan-review` with `status: pending` | opens it in plannotator (§15.5); no message | |
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

Reported state: `blocked` with the label of the most recently opened reason while any reason or signal is open; else `working` while `active`; else `idle`. On every change it runs `cpd2 session state <state> [--label <label>] --session <id>`. Calls are serialised: at most one in flight, and only the latest wanted state is sent next, so states never arrive out of order. A refused call (`illegal_transition`, for example after the engine marked the Session `stopped`) is reported as an error once and the next change is tried normally. At `session_shutdown`: `session state stopped`, waited for at most 2 s; `session run` reports it as well if pi exits without it (§10.8).

A Worker in `cpd2 ask` is set `blocked` by `ask` itself and back to `working` when it returns (§8.1); its adapter is mid-turn meanwhile and reports nothing.

**Names.** On pi's `session_info_changed`, when pi's name differs from the last name the adapter knows, it runs `cpd2 session rename <name> --session <id>`, so a `/name` typed in pi reaches the TUI. The `session-renamed` it causes comes back and changes nothing.

### 15.5 The Plan review through plannotator

Replaces cpd 1 §15's `cpd_plan_review` flow; the plannotator event API is the one cpd 1 uses (t4), parsed at the boundary because it is not a stable contract.

**Requesting.** The Lead runs `cpd2 plan review`, directly or through the tool `cpd_plan_review` (no parameters; registered for Plan Leads only). The tool runs `cpd2 plan review --plan <CPD_PLAN>` and returns its `next` text; when the result is `"already": true` and this session has no pending entry for that review, the tool opens it itself (below), so a review that failed to open can be retried. The tool's name is kept so §6.5's `--exclude-tools` list for Workers stays right.

**Opening**, on a `plan-review` Event with `status: pending` for its Plan, or from the tool:
1. Skip it when the session already has a `cpd-plan-review` entry for this review, or when it is not the Plan's latest review (`cpd2 plan show`, `review.id`): a replayed or superseded request.
2. Read `run/<plan>/plan-<review>.md` (§4.6).
3. `pi.events.emit("plannotator:request", {requestId, action: "plan-review", payload: {planContent, planFilePath}, respond})`, waiting at most 15 s for `respond`. A `handled` response with `status: pending` gives plannotator's own review id.
4. `pi.appendEntry("cpd-plan-review", {plan, review, plannotator_id, status: "pending"})`, so the mapping survives `/reload` and resume; open the blocking reason `plan-review:<review>` labelled `cpd: plan review`.

If plannotator does not answer, answers `unavailable` or `error` (for example `Port 19432 in use`), the adapter delivers an error message with plannotator's error (turn) and writes nothing; the review stays `pending` in the database.

**Deciding**, on `plannotator:review-result` `{reviewId, approved, feedback?}` for a pending entry:
1. Write the feedback, if any, to a temporary file; run `cpd2 plan decide <review> --approve | --reject [--feedback-file <f>] --plan <plan>`. `review_decided` means another caller decided it: the adapter reads the decision with `cpd2 plan show` and continues with it.
2. Append the entry with `status` `approved` or `rejected`; close `plan-review:<review>`.
3. Tell the Lead with `pi.sendMessage({customType: "cpd-plan-decision", …}, {triggerTurn: true, deliverAs: "followUp"})` (cpd 1 `decisionText`, kept): `Plan review r3 was APPROVED.` plus the feedback, then `Next: cpd2 plan lock`, or for a rejection `Revise the plan (job edit, job add, job cancel), then call cpd_plan_review again.`; when `current` is false, it says the Jobs changed since that review and asks for a new one. The custom type differs from `cpd`, so this message never moves the cursor.

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
| Rules | work only in this worktree and touch only the listed files; commit your work; before submitting, run the quick check yourself: the commands of `pre-commit` from the worktree's `.config/wt.toml`, listed verbatim, or, when the file has no `pre-commit` table, the line `This repo defines no quick check.` (change: cpd 1 said `./verify.sh`; R20: no fallback to `pre-merge`); never edit the protected paths, listed: `.config/wt.toml`, `.config/cpd.toml`, `.cpd/`, and each entry of the repo's protected list (§7.5; change: cpd 1 hard-coded `verify.sh`); never ask the User anything; if continuing would require guessing, `cpd2 ask <attempt> "<question>"`, which blocks until the Lead answers (change: no `--wf`, the pane has `CPD_PLAN`); one blocker at a time; do not stop until you have submitted |
| Job | Goal, Context, Files you may touch (`touches`), Acceptance, Out of scope (or "Nothing is listed. Still do only what the Goal and Acceptance require.") |
| Previous Attempts | only when the Job has ended Attempts: for each, number, Tier, outcome and cause, and what explains it: the conflicting files (`merge_conflict`), or one sentence for every other cause (change: cpd 1 showed only feedback) |
| Answers already given | new: every `answered` Escalation of earlier Attempts, question and answer, so a Worker does not ask again |
| Finish | `cpd2 attempt submit <attempt>`; on success stop at once; on a refusal, what each code means: `wrong_worktree` (run it from the worktree), `dirty_tree`, `no_commits`, `gate_tampered` (revert the protected paths), `check_failed` (fix, commit, submit again; the tail is in `details`), `check_error` (the check could not run: ask the Lead), `attempt_not_running` (the Attempt was stopped: stop) |

The Worker is started as §6.5 says, with the first message `Read <spec path> and follow it exactly. Your attempt id is <attempt>.` and `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT` in its environment.

**Nudge** (A5, once): an `attempt-nudge` Event to the Worker's Session with `data.prompt` = `You ended your turn without running cpd2 attempt submit <attempt>. Finish the Spec and submit, or run cpd2 ask <attempt> "<question>" if you are blocked.` (cpd 1 §12 step 7, kept), delivered with a turn (§15.3).

**Submit preconditions:** §6.1, in §7.3's order.

### 16.2 The review files and the brief

Removed by R19 (no Job-level Reviewer).

### 16.3 The verdict

Removed by R19 (no Job-level Reviewer).

## 17. What the TUI reads

Outline §8 and D4: the views take one **snapshot** with a read command, then **follow Events**; there is no timer polling of the CLI, no cache directory and no tmux option carrying state. Each view runs `cpd2` as a subprocess, like any agent (§1.1). Reads take no lock and never wait for writers (§4.2); cpd 1's `workflow show` took 58 s under load (research.md), which is why the spike needed a cache.

**Gap-free start.** Every snapshot command returns `events_cursor`, the highest Event id in the same read transaction. The view then runs `cpd2 events follow --since <events_cursor>`, so no Event between the snapshot and the follow is lost or seen twice.

| View (`@cpd_view`) | Snapshot | Events it acts on | What it updates |
|---|---|---|---|
| `tree` (Context column) | `cpd2 context list`, `cpd2 session list` | `context-created`, `context-grounded`, `plan-created`, `plan-phase`, `plan-done`, `session-registered`, `session-state`, `session-moved`, `session-renamed` | Contexts, their Plans and Lead Sessions, each with its state, label and name |
| `plan` (centre top) | `cpd2 plan show --plan <p>` for the Plan of the Lead in the chat slot | every Event with `plan = p`; `session-moved` with `place: "chat"` (the slot changed: switch Plan and take a new snapshot) | the flowchart of Jobs, Attempts and Tiers, open Escalations, the latest Plan review |
| `widgets` (Dispatch column) | `cpd2 plan list` | `plan-phase`, `pr-feedback`, `plan-done`, `watch-started`, `watch-stopped`, `error` | PRs and their feedback, the engine's state |
| `worker` (beside the chat) | `cpd2 attempt show <attempt>` for the Worker's session file | `session-state` for its Session; `attempt-status` for its Attempt | the session file itself, read-only (vision decision 5), and the state |
| `chat` (placeholder) | none | none | the static text `No Lead here: pick a Plan in the session tree` |

**Rates.**
- Each view runs one `cpd2 events follow` with no selection and filters by kind itself: one `PRAGMA data_version` read per `events.poll_ms` (200 ms), which costs no I/O while nothing changes (§5.4).
- A view applies an Event's `data` directly when it carries what is shown (`session-state`, `session-renamed`, `session-moved`); otherwise it re-runs its snapshot, coalesced to at most one snapshot per view every 500 ms, whatever the Event rate.
- The `worker` view re-reads its session file when its size or modification time changes, checked every 500 ms; the file is not in the database.
- Clocks such as "running 4 m" are redrawn every second from timestamps already read, without a read.
- If a follower ends, the view takes a new snapshot and follows again from its `events_cursor`.

**Actions.** The views change state only through commands: selecting a Lead runs `cpd2 session open <id>`; the new-Plan key runs `cpd2 plan new …`; parking runs `cpd2 session park`; opening a Job's Worker opens a `cpd2 tui view worker` pane beside the chat (`split-window`, `@cpd_view worker`), which is the view's own layout, not Ledger state.

Change from the spike and cpd 1's cockpit: the tree no longer discovers Sessions from tmux pane options (`@session_id`, `@agent_state`) or `leads/<id>/workflow`, and the plan view no longer reads `/tmp/cpd-tui/cache/`; both read the database through commands.

## 18. Changes from cpd 1


| cpd 1 | cpd 2 | Why |
|---|---|---|
| `--wf`, `CPD_WF`, `workflow_required` | `--plan`, `CPD_PLAN`, `plan_required` | D3 (§1.3) |
| `workflow init` | `plan new` (also starts the Lead) | D2, D3 |
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

| # | contract.md section | Amendment | Where here |
|---|---|---|---|
| 1 | §6.2, chat slot rule | the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat` (Escalation `contract-commands-a1-q1`, answered by the Lead) | §10 |
| 2 | §6.2, `@cpd_view` values | new value `chat`: the placeholder pane running `cpd2 tui view chat`, text `No Lead here: pick a Plan in the session tree`; at most one, only ever in the slot | §10, §13.2, §17 |
| 3 | §6.4, `session park` | park creates a new park window holding a placeholder, then `swap-pane`s the Lead and the placeholder; `session open` kills the placeholder pane with its window when a Lead takes the slot | §10.4, §10.5 |
| 4 | §1.4, `$CPD_HOME` layout | three more files: `run/<plan>/lead-message.md`, `run/<plan>/plan-<review>.md`, `run/<plan>/messages/<session>.md` | §1.4 |
| 5 | §5.3, the cursor | the adapter adds `delivered_to` to every message's `details`, and the cursor counts only messages delivered to this Session, never at or below its `session-registered` Event | §15.2 |
| 6 | §5.3, delivery table | `plan-review` with `status: pending` opens plannotator in the Plan Lead's adapter (no message); listed today under "every other kind: ignored" | §15.3, §15.5 |
| 7 | §5.3 and §5.4, `--session` selection | "a Lead of Plan P" is the Session that is `plans.lead_session`; any other Session, side forks included, follows only Events addressed to itself | §10.3, §11 |
| 8 | §6.5, environment | Lead panes also get `PLANNOTATOR_PORT` and `PLANNOTATOR_REMOTE` from cpd's environment, when set (several Plan reviews at once) | §15.5 |
| 9 | §6.3, tmux commands | `respawn-pane`, for a dead pane of the cockpit's layout | §13.1 |
| 10 | §1.2, output | `session run`, `tui view` and `tui --attach` own their terminal instead of printing one JSON value (the outline's "builds and attaches the cockpit") | §1.2 |
| 11 | §1.2 table of codes | the error codes this file introduces: `context_required`, `session_required`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id`, `context_exists`, `context_store_invalid`, `question_answered`, `not_a_git_repo`, `base_not_found`, `plan_exists`, `illegal_phase`, `no_draft_jobs`, `review_decided`, `dependency_cycle`, `worktree_exists`, `watch_start_failed`, `spec_invalid`, `tier_invalid`, `job_key_exists`, `needs_unknown`, `needs_unavailable`, `nothing_to_edit`, `job_merged`, `job_not_failed`, `job_not_ready`, `spawn_failed`, `attempt_not_running`, `session_malformed`, `driver_lock_missing`, `jobs_not_done`, `rebase_conflict`, `not_a_lead`, `grounded_lead`, `no_cockpit`, `wrong_pane`, `cursor_ahead`, `not_a_terminal`, `doctor_failed`, `pi_not_found`, `catalog_failed`, `file_unreadable`, `invalid_name`; most are cpd 1's (cpd 1 §11 and its `src/`) | §1.3 and each command |

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
