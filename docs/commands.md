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
21. [`inbox`](#21-inbox)

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

Kept from §1.2 (D1). Every command prints exactly one JSON value on stdout. Three kinds of command own a terminal instead, and are the only exceptions (amendment 10, §19):

| Command | What it prints | Why |
|---|---|---|
| `cpd events follow` | one Event per line (§5.4) | the one streaming command (§1.2) |
| `cpd session run`, `cpd tui view` (internal) | they run pi or draw a ratatui view in their pane | they are the programs inside cockpit panes (§6.2), not commands an agent calls. If they fail before taking the terminal, they print the failure envelope and exit with its code |
| `cpd tui --attach` | nothing on success: it replaces itself with `tmux attach` | the approved outline says `tui` "builds and attaches the cockpit"; without `--attach`, `cpd tui` prints JSON like every command |

`cpd --version` (like `--help`) prints one JSON value too: `{"ok": true, "version": "0.0.0", "schema": 1000, "store_schema": 1000, "protocol": 4, "store": {"opens": true, "refusal": null, "unknown": [], "pending": []}}`. `store` is the **verdict** of contract §4.5 ("The window") on `$CPD_HOME`'s database opened read-only, computed by the one function every guard uses (`store::verdict`): `null` when the database file does not exist; otherwise `opens` (`true` when this binary can open the store, migrating or converting it if needed), `refusal` (`null`, or `{"code", "message"}` with `code` one of `store_too_new`, `store_failed`, `migration_changed`, `migration_order`, §1.3) when `opens` is `false`, `unknown` (the migration names the store has applied that this binary lacks, in name order: inside the window they are all additive) and `pending` (the names of this binary's migrations the store has not applied, in the order they would be applied; for an unconverted store the legacy names it lacks, then every named one). `unknown` and `pending` are `[]` for a store that is not named yet, and filled whenever the store is a named store, whether or not it opens. **`schema` and `store_schema` are a projection of the verdict, kept only for the integer-era binaries that still run (their reload and install guards compare the two: `store_schema` above `schema` refuses) until `hs`, `air` and `pro` are converted; a later change removes them (hs-d178 D1):** `schema` is always `1000`; `store_schema` is `null` when `store` is, `1000` when `store.opens` is `true`, `1001` when it is `false`. No new code reads them. `protocol` is an integer, **4** today (1 until Plan manager added `session-message`, 2 until Plan inbox added the four `inbox-*` kinds, 3 until `session state idle --reply`, hs-d278), raised whenever an Event kind, an Event `data` field the pi adapter reads, or the output of a command the adapter runs changes in a way the adapter depends on (contract §5.2 "The protocol number"); the adapter checks it (§15.2). `--version` never creates, migrates or writes the database, and runs neither the schema check nor the conversion. **The guards read `store`:** a driver or a Lead's wrapper reloads into a binary only when `store` is `null` or `store.opens` is `true` (contract §3.4, §6.7), and `scripts/install-cpd.sh` refuses a build whose `store.opens` is `false`, and a Lead Branch build that carries a migration file that is not on `origin/main` (contract §3.7).

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
- **PR** (R34, §1.3): `--pr <key>`, within the selected Plan, used by `job add`, `pr open`, `pr rebase`, `pr push` and `pr view`. It may be omitted when the Plan has exactly one PR that is not `cancelled`, which is then selected; with two or more, `pr_required` (exit 2); with none, `pr_required` too, its message naming `cpd pr add`. A key that is not one of the Plan's PRs is `pr_not_found` (exit 4). Exceptions, stated at each command: `pr poll` has no `--pr`, it polls every open pull request of the User (§9.2, hs-d96), and `job edit --pr` changes the Job's PR, so omitting it changes nothing (§5.2). `pr add`, `pr edit`, `pr cancel` and `pr start` take the key as their positional argument.
- **Job, Attempt, Escalation, Plan review:** a positional id, within the selected Plan (§2.1).

Common refusals, which apply to every command and are not repeated in the sections below:

| Code | Exit | When |
|---|---|---|
| `usage` | 2 | unknown flag, missing argument, flag value of the wrong shape, exclusive flags together |
| `plan_required`, `context_required`, `session_required` | 2 | the command needs one and none was selected |
| `pr_required` | 2 | the command needs a PR, `--pr` is omitted, and the Plan does not have exactly one PR that is not `cancelled` (R34) |
| `session_ambiguous` | 3 | a short Session id (§1.3) matches more than one Session; `details.candidates` lists their ids |
| `push_rejected` | 3 | `pr push` found the remote branch moved (§9.9); `details.stderr` is git's |
| `invalid_context_id`, `invalid_plan_id`, `invalid_pr_id`, `invalid_job_id`, `invalid_attempt_id`, `invalid_escalation_id`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id` | 2 | an id does not match §2.1 |
| `config_missing`, `config_invalid` | 1 | §8.1 |
| `store_too_new` | 1 | the store has a breaking migration this binary does not know, or a later scheme (contract §4.5 "The window"; it keeps its code and exit); `details: {user_version, floor, unknown}`: the store's `user_version`, its floor (the name of its newest breaking migration, or `null`) and every applied name this binary lacks. A store with a numbered migration this binary lacks (`user_version` above the legacy set and below 1000) is the same code, with `unknown` empty |
| `migration_failed` | 1 | a migration, or the conversion, failed and rolled back (§4.5); `details: {from, to, sqlite_message}` |
| `migration_changed` | 1 | an applied migration this binary knows has another checksum than the store recorded (§4.5 R3); `details: {name, recorded, embedded}` |
| `migration_order` | 1 | a pending migration cannot be applied in this order, nothing written (§4.5 O3): a breaking one while the store has an applied migration that sorts after it or that this binary does not know; `details: {name, applied_after, unknown}` (names) |
| `store_tampered` | 1 | a write command, when the schema check fails (§4.3); read commands report it in `tamper` instead |
| `store_busy` | 5 | no write lock within `store_busy_ms` (§4.2); `SQLITE_BUSY` and `SQLITE_LOCKED` both |
| `internal` | 1 | a bug in cpd itself (for example a malformed Event built by a command), or a failed write to stdout other than EPIPE (no I/O error code exists). `store_failed` is for SQLite failures only |
| `context_not_found`, `plan_not_found`, `pr_not_found`, `job_not_found`, `attempt_not_found`, `escalation_not_found`, `review_not_found`, `session_not_found` | 4 | the selected or named object does not exist. (A Context record that names a missing record is `record_not_found`, exit 3, §4.6) |
| `item_not_found` | 4 | `inbox show`, `snooze` or `dismiss` named an Inbox item that does not exist |
| `item_closed` | 3 | `inbox snooze` or `dismiss` named an Inbox item that is closed (§21) |
| `archive_blocked` | 3 | `context archive`, `plan archive` or `session archive` found a live object (contract §2.2 "Archived"); `details.blockers` lists each, `details.next` says what to do |
| `archived` | 3 | the object (or its Context or Plan) is archived and the command does not work on archived objects; `details.next` is the unarchive command |
| `cas_lost` | 3 | a compare-and-swap found the state changed since it was read (§4.2); rerun the command |
| `illegal_transition` | 3 | a transition not in §3, refused by the command or by a trigger |
| `locked_job_tampered` | 3 | a Locked Job's fingerprint does not match (§2.3) |
| `git_failed`, `tmux_failed`, `gh_failed` | 1 | the tool exited non-zero where the command did not expect it; `details` has `argv`, `exit` and the last 60 lines of stderr. `pr poll` also returns `gh_failed` for an empty or non-JSON body, a search response with `errors`, and a lookup with no `data` (contract §7.6, hs-d96) |
| `gh_rate_limited` | 1 | an `error` Event code of engine E5 (contract §3.7, §5.2; `details`: `remaining`, `reset_at`), reported while it skips polls because GitHub's rate limit is nearly used; never a command's refusal |
| `adapter_missing` | 1 | an `error` Event code of the Lead wrapper `session run` (contract §6.7 "Adapter watchdog", §10.8; `details`: `session`, `state_since`, `seconds`), appended when it restarts a pi whose adapter never came back; never a command's refusal |

### 1.4 Files written by commands

The `$CPD_HOME` layout is §1.4. Commands in this file write five files the contract's layout does not list (amendment 4, §19); each is beside its peers and named so it cannot collide:

| File | Written by | Read by |
|---|---|---|
| `run/<plan>/lead-message.md` | `plan new` | the Plan's Lead `session run` (its first message) |
| `run/<context>/manager-message.md` | `context manager` (B3-A2) | the Manager's `session run` (its first message) |
| `run/<plan>/plan-<review>.md` | `plan review` | the pi adapter, which opens it in plannotator (§15.5) |
| `run/<plan>/feedback-<review>.md` | the pi adapter: plannotator's feedback on a Plan review (§15.5), overwritten on a retry | `plan decide --feedback-file` |
| `run/<plan>/messages/<session>.md` | `session fork --message-file` copies the file here | that Session's `session run` |
| `run/<plan>/reviews/<job>.md` | a review Job's Worker (B2, contract §1.4) | the Lead (and the User) |

### 1.5 Text arguments

Free text (Job `--title`, `--reason`, Context record text, questions and answers) is one argument, 1 to 20 000 characters, with no NUL; else `usage`. Context and Plan titles (`--title` of `context new`, `context rename`, `plan new`, `plan rename`) are 1–200 characters with no control characters, else `usage` (exit 2). Names (`--name`, `session rename`) are 1–80 characters with no control characters, else `invalid_name` (exit 2) (cpd 1 §11 `session rename`, kept). Files given by path (`--spec`, `--brief-file`, `--feedback-file`, `--message-file`) must be readable UTF-8, else `file_unreadable` (exit 2).

## 2. Command index

The index is generated: [docs/interface.md](interface.md), rendered from the `tree` of `cpd --help` (§1.2) by the test `cli/tests/interface.rs`, which fails when the page and the binary differ, either way; `CPD_BLESS=1 scripts/cargo.sh test -p cpd --test interface` rewrites the page; never edit it by hand.

**Definitions:**
- **callers** = the agents meant to run the command, any of `Lead`, `Manager`, `Worker`.
- **cockpit** = an action of the User's cockpit runs it (the cockpit is the User's interface over the same commands, §17 Actions).
- **internal** = §1.1.

Anyone may run any command; a command with no callers that is neither cockpit nor internal is the User's, at a shell (for example `doctor`). The callers and the cockpit flag are kept in the binary (`cli/src/interface.rs`), next to the clap tree. The table's Writes column (each command's Effects says what it writes) and Contract column (each description ends with its §) are not carried over (hs-d251).

## 3. `context`

A Context and its records (§2.2, §4.6). The records live only in the database, and Contexts are machine-local (hs-a1). Every record write follows §4.6 exactly: one transaction that validates, inserts the `context_records` row and appends `context-record` `{record, kind, machine, seq}`. No lock file, no git, no file.

**Record ids.** `<machine>-<letter><n>` with `n` = 1 + the highest `n` already in this Context's records for this machine and letter (§2.1). The write transaction makes it race-free; only this machine writes its letter sequence.

**Retries.** A record write is recognised as a retry, and returns the existing record with `"already": true`, when this machine's latest record in the Context has the same `kind`, `text`, `supersedes`, `question` and `by.session`. Any other write appends a new record; records are never edited.

### 3.1 `context new`

```
cpd context new <id> --title <text> [--jira <key>]
```

| Flag | Meaning |
|---|---|
| `<id>` | Context id (§2.1) |
| `--title <text>` | required; 1–200 characters, no control characters, else `usage` (§1.5) |
| `--jira <key>` | Jira Epic key, stored only (`^[A-Z][A-Z0-9]+-[0-9]+$`, else `usage`) |

| Precondition | Code | Exit |
|---|---|---|
| no Context row with this id, unless it has the same title and Jira key | `context_exists` | 3 |

Effects:
- **Rows**, one transaction: `contexts` (`created_on` = `machine`). **Event** `context-created` `{title, jira, created_on}`. Nothing else is written: no directory, no git.
- **tmux:** none.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null, "created_at": "2026-10-14T21:10:00.000Z", "created_on": "hs"}}
```

Idempotent: yes. The same id, title and Jira key return `"already": true`.

### 3.2 `context list`

```
cpd context list
```

No flags, no preconditions, no effects (one read transaction).

```json
{"ok": true, "contexts": [
  {"id": "building-cpd", "title": "Building cpd", "jira": null, "archived_at": null,
   "records": 57, "open_questions": 2,
   "plans": [{"id": "cpd2", "title": "cpd in Rust", "phase": "dispatch", "lead_session": "596d007f-…", "archived_at": null}]}],
 "events_cursor": 48213}
```

`archived_at` (here and on each Plan) is null, or the time the object was archived (contract §2.2 "Archived"). Archived Contexts and Plans are listed too. `events_cursor` is the highest Event id visible in the same read transaction (§17). Idempotent: read-only.

### 3.3 `context show`

```
cpd context show [--context <id>] [--since <seq>] [--record <id>] [--digest]
```

| Flag | Meaning |
|---|---|
| `--since <seq>` | only records whose index position `seq` is greater (exclusive, like `events --since`); what a Manager's turn-start update holds (§15.7); a Lead passes the `seq` it last read |
| `--record <id>` | print that one record in full (B3-A4): `records` has only it, `markdown` renders only it with its whole `text`. Exclusive with `--since` (`usage`). A record not in the Context: `record_not_found` (exit 3, §4.6) |
| `--digest` | print the Context's latest digest (contract §4.6, hs-d194), below. Exclusive with `--since` and `--record` (`usage`) |

Effects: none. Output: the Context, its records in index order, and `markdown`, the rendering of §2.2 "Context record" (title and Jira key; Plans; active decisions; superseded decisions with their successor; open questions; answered questions with their answers; notes; each item with its id and date; briefs under their Plan, see below). With `--since`, `markdown` renders only those records, in the same order of sections.

**Briefs** (B3-A4): a record of kind `brief` is listed **short**, in `markdown` and in `records`: its id and the first line of its `text`, under its Plan (the Plan's section), never in full. In `records` its `text` is that first line and the entry has `"short": true`; its `plan` is `{id, machine}`. The full text is `cpd context show --record <id>`. `--record` is the only way to get a brief whole.

**Plans** (R34, A12): each `plan` record is listed with the repos of its Plan's PRs that are not `cancelled`, read from this machine's database (`prs.repo`) in the same read transaction, each repo once, in PR key order; a Plan of another machine, or one with no PR yet, shows none. The record itself carries no repo (§2.2; a `repo` in a record written before R34 was dropped by migration 0007's import, contract §4.5). In `records`, a `plan` record's `plan` is `{id, machine, jira}`, and the entry has `repos`, the list just described.

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null},
 "records": [{"seq": 42, "id": "hs-d7", "kind": "decision", "at": "2026-10-14T21:12:00.000Z", "machine": "hs",
              "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "One engine per machine.", "supersedes": "hs-d2"}],
 "markdown": "# Building cpd\n\n## Plans\n…"}
```

Idempotent: read-only.

**`--digest`** (hs-d194, D6). Reads the latest digest file of the Context (`run/context-digests/<context>/<upto_seq>.md`, the highest `upto_seq`; contract §1.4, §4.6) and no record. `digest` is `{upto_seq, file, bytes}` and `markdown` the file's text. A Context with no digest yet is not an error: `digest` and `markdown` are `null` (its records are read with `context show`); `cpd context distill` builds one (§3.12).

```json
{"ok": true, "context": {"id": "building-cpd", "title": "Building cpd", "jira": null},
 "digest": {"upto_seq": 1342, "file": "/home/clayton/.cpd/run/context-digests/building-cpd/1342.md", "bytes": 27311},
 "markdown": "# Building cpd: digest\n\n## Engine\nOne engine per machine [hs-d7] …"}
```

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

Effects: the record write above (one transaction), with kind `decision` (letter `d`), `note` (`n`) or `question` (`q`); `by` = `{session: CPD_SESSION or PI_SESSION_ID or null, plan: the selected Plan or null}`. **Event** `context-record`. No tmux.

```json
{"ok": true, "record": {"id": "hs-d8", "kind": "decision", "at": "2026-10-14T21:30:00.000Z", "machine": "hs", "by": {"session": "596d007f-…", "plan": "cpd2"}, "text": "…", "supersedes": "hs-d2"}, "seq": 58}
```

Idempotent: a retry is recognised (above); otherwise each call inserts a record, by design (records are never edited).

### 3.5 `context answer`

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

### 3.6 `context sync`

Removed by Plan manager B1 (hs-a1): Contexts are machine-local; records live in the database. The command is now unknown (`usage`, exit 2); engine E6 is removed with it (contract §3.7).

### 3.7 `context rename`

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

### 3.8 `context archive`

```
cpd context archive <id>
```

Archives the Context with its Plans, all their Sessions and its Manager (contract §2.2 "Archived").

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| no object of the cascade is live: no Session `working` or `blocked`, no Job `queued` or `running`, no open Escalation, no pending Plan review; `details.blockers` lists each as `{kind, id, reason}` | `archive_blocked` | 3 |
| no Lead of the cascade is in the chat slot; `details.next` = `cpd session park` | `in_chat_slot` | 3 |

Effects, in this order (all or nothing):
1. **Check** every object of the cascade. If any blocks, nothing is written and nothing is closed.
2. **tmux:** close every live Lead pane exactly as `session close` does (§10.9): `kill-pane`, `state = stopped`, `tmux_pane = NULL`, with its `session-state` and `session-moved` Events. pi stops; the session files stay.
3. **Rows**, one transaction that re-checks the rule first (`archive_blocked` if something became live meanwhile): `archived_at` = now on the Context, on each of its Plans and on each Session of the cascade, that is not archived yet. One **Event** per object changed: `context-archived` `{archived: true}`, `plan-archived`, `session-archived`.

```json
{"ok": true, "archived": [{"kind": "context", "id": "building-cpd"}, {"kind": "plan", "id": "cpd2"}, {"kind": "session", "id": "596d007f-…"}], "closed": ["%41"]}
```

`closed` lists the panes closed in step 2. Idempotent: yes; a Context that is archived already (and whose cascade has nothing to archive) returns `"already": true`, writes nothing and closes nothing.

### 3.9 `context unarchive`

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

### 3.10 `context manager`

```
cpd context manager [--context <c>]
```

Gets the Context's **Manager** (B3-A1, B3-A2; CONTEXT.md "Manager"): registers and starts it when the Context has none, starts it again when it is stopped. The Context is `--context`, else the selected Plan's (§1.3).

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| the Context is not archived; `details.next` = `cpd context unarchive <c>` | `archived` | 3 |
| the cockpit's tmux session exists when a pane must be started; `details.next` = `cpd tui` | `no_cockpit` | 3 |

Effects, by the state of the Context's live (not archived) Manager Session:
- **None:** **rows**, one transaction: `sessions` (new UUID v4; `role = manager`; `context`; `plan` null; `name` = `<context>-manager`; `cwd` = the realpath of the caller's cwd; `tier` = config `manager_tier`, default `O5`; `forked_from` null; `stopped`); **Event** `session-registered` `{role, name, forked_from, tier}`. Then **tmux**, as `plan new` starts a Lead (§4.1 step 4): `new-window -d -t =cpd: -n ctx-<context> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -P -F '#{window_id} #{pane_id}' -- cpd session run <session>`, `@cpd_session` on the pane (no `CPD_PLAN`, no plannotator variable; `session run` also removes every `PLANNOTATOR_*` variable it inherited from the tmux server or the caller before it starts pi, whatever they hold); one transaction: `sessions.tmux_pane`, **Event** `session-moved` `{pane, window, place: "parked"}`. Its first message, written to `run/<context>/manager-message.md` (§1.4): `You are the Manager of Context <c> "<title>". Follow the cpd-manager skill.`, then the Context as `plan new` gives it (§4.1 step 3; contract §4.6 "Consumers", the same function): its latest digest and at most 20 records after it, or `context show --context <c>` rendered as markdown (every record; briefs short) when it has no digest yet.
- **`stopped` with no pane:** the tmux step alone, in the stored `cwd`: `session run` resumes pi on its session file (`pi --session <file>`, §6.5), with no first message.
- **Otherwise** (it has a pane): nothing.

The command never moves the chat slot (hs-d103); `session open <manager>` does (§10.4).

```json
{"ok": true, "manager": {"session": "3c9a…", "name": "building-cpd-manager", "state": "stopped", "pane": "%47", "window": "@21"}, "started": true}
```

`started` is true when this call registered the Manager or started its pane. Idempotent: yes; a Manager with a pane returns `"started": false` and writes nothing.

### 3.11 `context pending`

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

Output: `plans`, every Plan of the Context that is not `done` and not archived, in creation order, each with `id`, `title`, `phase`, `lead_session` and `prs`: its PRs that are not `merged` and not `cancelled`, in key order, each with `key`, `repo`, `base`, `status`, `lead_branch`, `pull_request` (`{number, url, state}` as stored by `pr poll`, or null before the PR is opened) and `jobs`: the PR's Jobs that are not `cancelled` and not yet on the base, in `seq` order, with status `draft`, `queued`, `running`, `failed` or `merged`. Here `merged` means on the Lead Branch and not yet on the base (§2.2 Job). Each Job has `key`, `status`, `title` and `touches`.

```json
{"ok": true, "context": "building-cpd",
 "plans": [{"id": "pay-4521", "title": "Retry failed payments", "phase": "dispatch", "lead_session": "596d007f-…",
            "prs": [{"key": "api", "repo": "/home/clayton/projects/api", "base": "main", "status": "dispatch",
                     "lead_branch": "cpd/pay-4521/api", "pull_request": {"number": 212, "url": "https://github.com/acme/api/pull/212", "state": "OPEN"},
                     "jobs": [{"key": "retry-core", "status": "merged", "title": "Retry policy", "touches": ["src/retry.rs"]},
                              {"key": "retry-cli", "status": "queued", "title": "Retry flag", "touches": ["src/cli.rs", "docs/commands.md"]}]}]}]}
```

With `--markdown` the output is the same data as markdown: `# Pending work in <context>`, then per Plan `## <id> (<phase>): <title>`, per PR `### PR <key> (<repo>, base <base>): <status>` with its pull request (`#<n> <state> <url>`) or `no pull request`, then one line per Job `- <key> <status>: <title> (<touches, comma-separated>)`. A Context with no active Plan prints `No pending work.`

Idempotent: yes; it only reads.

### 3.12 `context distill`

```
cpd context distill [--context <c>]
```

Builds the Context's **digest** (contract §4.6, hs-d194 D1–D5; CONTEXT.md "Distilled Context"): a derived, compressed rendering of its records in force, written by one-shot pi and validated by cpd. Engine E9 starts it detached, debounced, when the Context has records newer than its digest (contract §3.7); anyone may run it on demand, and it then runs in the caller's process and waits for pi. The Context is `--context`, else the selected Plan's (§1.3). Its §2 row is added by the Job that implements it, as §10.9's was, so that `every_documented_command_exists` holds on every commit.

| Precondition | Code | Exit |
|---|---|---|
| a Context was selected (§1.3) | `context_required` | 2 |
| the Context exists | `context_not_found` | 4 |
| no other distill run of this Context is alive: `locks/distill-<context>.lock` is free (taken without waiting, held until the run ends) | `distill_running` | 3 |

An archived Context is not refused (the digest is a derived file); engine E9 never starts one for it.

Effects (contract §4.6 "The distill run", steps 1–5):
- **Nothing to do**, `"already": true` and nothing written: the latest digest's `upto_seq` is at least the Context's highest record `seq`, or no record is in force (then `digest` is null).
- **Carried forward**: only `plan` and `brief` records came after the latest digest, so the records in force are the same; its text is written as the new `<upto_seq>.md` without pi (`"distilled": false`).
- **Otherwise**: files `prompt.md` and `records.md`, then the distiller, `pi --print …` at Tier `context_distill_tier` (contract §6.5 "Distiller", §8.2), stdout to `<upto_seq>.md.tmp`, killed after `timeouts.distill_minutes`; then the validation, and on success the rename to `<upto_seq>.md` and the removal of the directory's older digests and temporary files. `"distilled": true`.
- **Rows and Events**: none on success; no database write ever, except the one `error` Event of a failure (below).
- **tmux:** none. **git:** none.

Failures, after the lock is taken. Each removes the `.md.tmp`, leaves the previous digest in place, appends **one** `error` Event `{source: "context distill", code, message, details}` with no `context`, `plan` or `session` set (`details.context` names it; so no Lead gets a turn, contract §5.3), and returns the same code:

| Failure | Code | Exit | `details` |
|---|---|---|---|
| pi could not start, exited non-zero (an error, or an answer cut at the output limit), or ran longer than `timeouts.distill_minutes` | `distill_failed` | 1 | `context`, `upto_seq`, `reason` (`spawn`, `exit` or `timeout`), `exit` (when it exited), `tail` (pi's last 60 stderr lines), `log` |
| the digest is not UTF-8, is over 32 768 bytes, leaves a record in force uncited (`missing`) or cites an id that is not one (`stray`: superseded, `plan`, `brief` or unknown) | `distill_invalid` | 1 | `context`, `upto_seq`, `bytes`, `limit` (32768), `missing` (ids), `stray` (ids), `log` |

`log` is `run/context-digests/<context>/distill.log`, where the run writes its stderr (and, started by E9, its stdout too).

```json
{"ok": true, "context": "building-cpd", "distilled": true,
 "digest": {"upto_seq": 1342, "file": "/home/clayton/.cpd/run/context-digests/building-cpd/1342.md", "bytes": 27311},
 "records_in_force": 231}
```

Idempotent: yes. With no record newer than the latest digest it returns `"already": true` and runs nothing; a rerun after a failure runs again (on demand; E9 does not retry the same `seq`, contract §3.7).

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
| `--title <text>` | required; 1–200 characters, no control characters, else `usage` (§1.5) |
| `--jira <key>` | Jira task key, stored only |
| `--name <name>` | the Lead's pi display name; default `<id>` |
| `--brief-file <file>` | the brief for the Lead (B3-A4): written as a Context record of kind `brief` and put into the Lead's first message in full |

| Precondition | Code | Exit |
|---|---|---|
| the Context exists | `context_not_found` | 4 |
| no Plan with this id, unless every flag matches it (then the result is `already`) | `plan_exists` | 3 |
| the Context is not archived; `details.next` = `cpd context unarchive <c>` | `archived` | 3 |

Effects, in this order:
1. **Rows**, one transaction: `sessions` (new UUID v4; `role = lead`; `context`; `plan = <id>`; `name`; `cwd` = the realpath of the caller's cwd; `tier = lead_tier`; `forked_from` null; `state = stopped`: the Lead is always fresh, never a fork, B3-A3); `plans` (`phase = grounding`, `lead_session` = the new Session); no `prs` row. **Events** `plan-created` `{title, jira, lead_session, forked_from: null, cwd}` and `session-registered` `{role, name, forked_from, tier}`.
2. **Context records:** the record write of §3 (one transaction) with kind `plan` (letter `p`), `text` = the title, `plan` = `{id, machine, jira}` (`jira` omitted when null); **Event** `context-record`. With `--brief-file`, a second record of kind `brief` (letter `b`), `text` = the file's text, `plan` = `{id, machine}`; **Event** `context-record`.
3. **First message**, written to `run/<id>/lead-message.md`: a header (`You are the Lead of Plan <id> "<title>" in Context <c>. Phase grounding: follow the cpd-lead skill.`), the full brief (the file's text) if given, then the Context (contract §4.6 "Consumers", hs-d194 D6):
   - **with a digest**: the latest digest's text (§3.3 `--digest`), then the records written after its `upto_seq`, oldest first, at most 20 (this Plan's own `plan` and `brief` records among them), rendered as `context show --since <upto_seq>` renders them (briefs short), then, when more exist, `and <K> more: cpd context show --context <c> --since <seq>`, with `<seq>` the `seq` before the first record not listed;
   - **with no digest yet**: `context show --context <c>` rendered as markdown: every Context record, other Plans' briefs short (§3.3).

   The Lead is always fresh (B3-A3): the grounded Lead and its forks are retired (contract §3.1), so there is no fork case.
4. **tmux:** start the server if it is not running (§6.1) and the cockpit session `cpd` if it is missing; then `new-window -d -t =cpd: -n park-<id> -c <cwd> -e CPD_HOME=… -e CPD_SESSION=<session> -e CPD_PLAN=<id> -P -F '#{window_id} #{pane_id}' -- cpd session run <session>`, and `set -p -t <pane> @cpd_session <session>`. One transaction: `sessions.tmux_pane`; **Event** `session-moved` `{pane, window, place: "parked"}`. If any step after `new-window` fails (`set-option -p @cpd_session`, the row or the Event write), the new window is killed (`kill-window`) before the error is returned (teardown G11).
5. In the pane, `session run` (§10.8) starts a fresh Lead with the first message.

When the cockpit session is missing, plan new creates it with the new Lead's park window as its first window; cpd tui builds main later.

```json
{"ok": true, "plan": {"id": "pay-4521", "context": "payments", "title": "Retry failed payouts", "jira": null, "phase": "grounding", "lead_session": "8d0e…", "prs": []},
 "lead": {"session": "8d0e…", "name": "pay-4521", "forked_from": null, "cwd": "/home/clayton", "pane": "%41", "window": "@17"},
 "record": "hs-p4"}
```

Idempotent: yes. With the same flags it returns `"already": true` and redoes any step that did not happen: the `plan` record when the Context has none for this Plan, the `brief` record when `--brief-file` is given and the Context has none for this Plan, and step 4 when the Lead is `stopped` with no pane (in the stored `cwd`). The caller's cwd is not a flag: a rerun from another directory is still `already` and keeps the stored `cwd`. Different flags: `plan_exists`.

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

Output: every field of the Plan (§2.2); `prs` (R34, A14), every PR of the Plan in key order, `cancelled` ones included, each with its fields (§2.2 PR: `key`, `repo`, `base`, `needs`, `status`, `lead_branch`, `lead_worktree`, `pr`, `pull_request` (the row, as in §4.2, `mergeable` and `merge_state` included, or null), `all_merged_at`, `start_failed_at`, `created_at`, `started_at`, `merged_at`) and `jobs`, the keys of its Jobs in `seq` order; `jobs` with each Job's fields (Spec included, `pr` and `review` among them), `needs`, and its Attempts (`id`, `number`, `tier`, `origin`, `status`, `outcome`, `cause`, `worker_session`, `started_at`, `ended_at`); `escalations` that are `open`; the latest Plan review; `counts`; `tamper` (the tamper report of §4.3: `{kind: "schema" | "fingerprint" | "history", …}`); `events_cursor`.

```json
{"ok": true,
 "plan": {"id": "cpd2", "context": "building-cpd", "title": "cpd in Rust", "jira": null, "phase": "dispatch", "lead_session": "596d007f-…", "created_at": "…", "locked_at": "…", "done_at": null, "archived_at": null},
 "prs": [{"key": "cpd", "repo": "/home/clayton/projects/cpd", "base": "main", "needs": [], "status": "dispatch", "lead_branch": "cpd/cpd2/cpd", "lead_worktree": "/home/clayton/projects/cpd.cpd2.cpd",
          "pr": null, "pull_request": null, "all_merged_at": null, "start_failed_at": null,
          "created_at": "…", "started_at": "…", "merged_at": null, "jobs": ["contract-core", "contract-commands"]},
         {"key": "web", "repo": "/home/clayton/projects/web", "base": "main", "needs": ["cpd"], "status": "waiting", "lead_branch": "cpd/cpd2/web", "lead_worktree": null,
          "pr": null, "pull_request": null, "all_merged_at": null, "start_failed_at": null,
          "created_at": "…", "started_at": null, "merged_at": null, "jobs": ["web-client"]}],
 "jobs": [{"key": "contract-commands", "pr": "cpd", "title": "cpd 2 contract: every command", "status": "running", "tier": "O3", "appended": false, "review": false, "needs": ["contract-core"], "fingerprint": "9f…", "retry_tier": null,
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

Effects: writes `run/<plan>/plan.md` (§1.4), nothing else. Content, kept from cpd 1 §11 `plan render` and grouped by PR (R34): title, Context and phase; a table of the PRs that are not `cancelled`, with key, repo, base and needs, and, when there are two or more, a Mermaid `graph LR` of their needs; then the section `## Overlaps` (below); then one section per PR, in the order of the needs graph (ties by key), headed `PR <key>`, with its repo, base and needs, a table of its Jobs that are not `cancelled` with key, title, Tier, status, needs and touches, a Mermaid `graph TD` of its Jobs' Dependencies, and one subsection per Job with its Spec (goal, context, touches, acceptance, out of scope). A Job's section is under its PR only. In a phase after `plan`, the Jobs that are not `draft` are Locked context: the render shows them as such, and marks every `draft` Job **under review** (in its table row and its section), because a review in these phases covers exactly the drafts (§4.6; User, hs-d173, hs-d174, hs-d179). In phase `plan` every Job is a draft and nothing is marked. A **stub draft** (no Tier or no Spec, §5.1) is shown as it is, visibly incomplete: `Tier: —` in its table row and section when it has no Tier, and `Spec: not written yet` in place of its Spec subsection's goal, context, touches, acceptance and out of scope when it has none. GitHub-flavoured Markdown only: tables, code and Mermaid, no footnotes, no raw HTML (t4). Output: the path, the plan fingerprint (§2.3: Jobs and PRs), the number of `draft` Jobs, the number of PRs that are not `cancelled` and the overlaps.

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
{"ok": true, "path": "/home/clayton/.cpd/run/pay-4521/plan.md", "fingerprint": "9f86d081…", "draft_jobs": 6, "prs": 2,
 "overlaps": [{"job": "x", "plan": "pay-4522", "pr": "api", "other_jobs": [{"key": "y", "status": "queued"}, {"key": "z", "status": "draft"}], "paths": ["core/src/x.rs", "core/src/lib.rs"]}]}
```

Idempotent: yes (the same PRs and Jobs, and the same other Plans, render the same file). Change: cpd 1's `--out` is dropped; the file has one place.

### 4.6 `plan review`

```
cpd plan review [--plan <id>]
```

Requests the User's approval of the draft Jobs (§2.2 Plan review). In phase `plan` the review covers the whole plan, as before; in every phase after `plan` it covers **exactly the draft Jobs**: its fingerprint is over the drafts only (§2.3), and the rendered plan marks them under review and shows the Locked Jobs as context (§4.5). A Lead that planned several batches adds the later ones as drafts (`job add --draft`, §5.1) and asks for one review per batch. The Lead runs it through its tool `cpd_plan_review` or directly; either way the pi adapter opens plannotator when the `plan-review` Event arrives (§15.5), so the review opens from any caller, in whatever phase the Plan is.

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan`, or any phase after it (not `grounding`) | `illegal_phase` | 3 |
| at least one Job is `draft` (in a phase after `plan` too: the drafts are what is reviewed) | `no_draft_jobs` | 3 |
| every `draft` Job has its Tier and its Spec (hs-d241, R1: no stub draft is approvable); `details.jobs` lists `[{key, missing}]`, `missing` being `["tier"]`, `["spec"]` or both | `draft_incomplete` | 3 |

Effects:
- **Files:** renders the plan to `run/<plan>/plan.md` and copies it to `run/<plan>/plan-<review>.md`, so a later render cannot change what the User reviews. The reviewed file carries the `## Overlaps` section of §4.5, as it is at that moment, so the User sees the warning in the review: one row per open Job of this Plan and per other PR that overlaps it, short enough to read (hs-n201); it is a warning only and the review is requested all the same.
- **Rows**, one transaction: `plan_reviews` (`id = r<k>`, `k` = 1 + the highest, `fingerprint` = the plan fingerprint, `status = pending`). **Event** `plan-review` `{review, fingerprint, status: "pending"}`.

```json
{"ok": true, "review": {"id": "r3", "fingerprint": "9f86d081…", "status": "pending", "requested_at": "…"}, "path": "/home/clayton/.cpd/run/pay-4521/plan-r3.md",
 "next": "The User reviews the plan in plannotator. Stop and wait for the decision message."}
```

**Stubs** (hs-d241, R1). A draft without its Tier or its Spec (a stub, §5.1) cannot be approved, so the review is **not requested**: it is refused `draft_incomplete` (exit 3) with `details.jobs` = `[{"key": "api-1", "missing": ["spec"]}, …]`, in key order, `missing` holding `"tier"` and/or `"spec"`. The same refusal applies through the tool `cpd_plan_review`. Nothing is rendered or written. The Lead completes each stub with `job edit --tier --spec` (§5.2) or cancels it, then asks again. The plan fingerprint covers a stub as it is (contract §2.3), so completing one changes it.

Idempotent: yes. When the latest review is `pending` for the current fingerprint, it returns that review with `"already": true` and writes nothing. When the Jobs or PRs changed since, it requests a new review; the older one stays `pending` and no longer matters, because `plan lock` reads only the latest (§2.3).

### 4.7 `plan decide` (internal)

```
cpd plan decide <review> (--approve | --reject) [--feedback-file <file>] [--plan <id>]
```

Records the User's decision. Called by the pi adapter with plannotator's result (§15.5); replaces cpd 1's `plan decision` (cpd 1 §11), whose `--pending` step is now `plan review`. The cpd-lead skill forbids the Lead to run it. Internal (§1.1): hidden in clap, run only by the pi adapter.

| Precondition | Code | Exit |
|---|---|---|
| the review is `pending` (decided the same way: `already`) | `review_decided` | 3 |

Effects: **rows**, one transaction: `plan_reviews.status`, `feedback` (the file's text, or null), `decided_at`. **Event** `plan-review` `{review, fingerprint, status, feedback}`.

In a phase after `plan`, an **approval** also queues the drafts the review covered (§2.2, §3.2), in the same transaction:
- **Rows:** every `draft` Job of the Plan becomes `queued` and gets its Locked-Job fingerprint (§2.3), as an appended Job has at `job add`; `appended` stays 1. The engine schedules each as soon as it is Ready (§3.2).
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

### 4.9 `plan rename`

```
cpd plan rename [--plan <id>] --title <text>
```

Changes the Plan's title, the label the tree shows (contract §2.2). `--title` is required, as `plan new`'s: 1–200 characters, no control characters, else `usage` (§1.5).

| Precondition | Code | Exit |
|---|---|---|
| the Plan exists | `plan_not_found` | 4 |

Effects: an unchanged title: `"already": true`, nothing written. Otherwise **rows**, one transaction: `plans.title`; **Event** `plan-renamed` `{title}`; **tmux**: sets `@cpd_title` on the Session's pane (the Plan's Lead) when it has one (§6.2 of contract.md). The Context store's `plan` record keeps the old title. Archived Plans can be renamed.

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
| no object of the cascade is live: no Session `working` or `blocked`, no Job `queued` or `running`, no open Escalation, no pending Plan review; `details.blockers` lists each as `{kind, id, reason}`. The phase does not matter: a Plan in `deploy` is archivable, and the engine keeps polling its PR | `archive_blocked` | 3 |
| neither the Lead nor a side fork is in the chat slot; `details.next` = `cpd session park` | `in_chat_slot` | 3 |

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

Abandons a Plan (E1, hs-d203): cancels every Job that is not merged, closes the Plan's pull requests that are not merged, closes its Lead and its forks, frees its plannotator port and archives it. Branches and worktrees are not removed by it; they go by the normal clean-up. `--reason` is kept in the Events' `reason`; default `plan cancelled`. The Plan may be in any phase, `done` included (nothing is left to cancel; it is then archived).

| Precondition or failure | Code | Exit |
|---|---|---|
| the Plan exists | `plan_not_found` | 4 |
| neither the Lead nor a side fork is in the chat slot; `details.next` = `cpd session park` | `in_chat_slot` | 3 |
| no Plan review is `pending` (the User decides it first); `details.blockers` as `plan archive` | `archive_blocked` | 3 |
| every running Attempt has ended within 60 s of its stop request (below); `details.attempts` lists those still running | `cancel_timeout` | 5 |
| each `gh pr close` succeeds, or its pull request is already closed or merged; `details` is `{argv, exit, stderr}` | `gh_failed` | 1 |

A Session `working` or `blocked` and a Job `running` do not block (unlike `plan archive`): the command stops them. The caller's own Lead is in that case.

Effects, in this order; each step is idempotent, so a rerun after any failure continues where it stopped:
1. **Check** the preconditions above that need no write.
2. **Jobs**, as `job cancel` (§5.3) for every Job not `merged` and not `cancelled`, with the reason: `draft`, `queued` and `failed` become `cancelled` with **Event** `job-cancelled`; a `running` Job gets `job-cancel-requested`, and engine E2 or the driver ends its Attempt `cancelled` at the next safe point (contract §3.4). The command then waits, polling, up to 60 s until no Attempt of the Plan is not `ended`. Dependents stay blocked, as for `job cancel`; they are cancelled themselves by this step.
3. **Escalations:** each `open` Escalation of the Plan becomes `expired`, **Event** `escalation-expired`.
4. **Pull requests:** for each PR of the Plan with status `open` whose pull request is not `merged`, `gh pr close <number> --repo <owner/repo>`. The PR rows keep their status (`open`; PR statuses never end this way, contract §3.8); the next `pr poll` reads the closure and appends no `pr-feedback` for an archived Plan. A `waiting` PR has no pull request: nothing. A `merged` PR is never touched.
5. **tmux:** close every live Lead pane as `session close` does (§10.9).
6. **Rows**, one transaction: `archived_at` = now on the Plan and its Sessions, with **Event** `plan-archived` `{archived: true}` and `session-archived` per object, as `plan archive` (§4.10). The Plan's port is free from here (contract §6.5).

No new Event kind and no schema change. The command's own Events are the existing ones named above.

```json
{"ok": true, "cancelled": ["api", "ui"], "closed_prs": [{"pr": "ui", "number": 813}], "closed": ["%41"], "archived": [{"kind": "plan", "id": "cpd2"}, {"kind": "session", "id": "596d007f-…"}]}
```

Idempotent: yes. An archived Plan with nothing left to cancel: `"already": true`, nothing written. A rerun after a failure repeats steps 2 to 6 on what is left; Jobs already `cancelled` and pull requests already closed are skipped.

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
cpd job add <key> --title <text> --tier <T> --spec <file> [--pr <key>] [--needs <k1,k2> | --review] [--draft] [--plan <id>]
cpd job add <key> --draft --title <text> [--tier <T>] [--spec <file>] [--pr <key>] [--needs <k1,k2>] [--plan <id>]
```

`--tier` and `--spec` are required, except with `--draft`: a **stub draft** (hs-d241, R1) needs only `--title`; either or both of `--tier` and `--spec` may be given too. A Job that is not a draft, and a `--review` Job, still needs both (`usage`, exit 2). The Lead registers planned work with a stub the moment it is planned (hs-d239) and completes it later with `cpd job edit <key> --tier <T> --spec <file>` (§5.2). A follow-up PR is added the same way: `pr add <key> --repo … --needs …` (§9.4) plus stub drafts in it.

| Flag | Meaning |
|---|---|
| `<key>` | Job key (§2.1). Change: positional, like every other `job` command (cpd 1 had `--key`) |
| `--pr <key>` | the Job's PR (R34, A14), selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one |
| `--needs <k1,k2>` | Dependencies, comma-separated, no duplicates (`usage`); Jobs of the same PR only (A7) |
| `--draft` | add the Job as a `draft` in a phase after `plan` too (User, hs-d173, hs-d174, hs-d179: every planned batch is visible in the Plan view before it is approved): `status = draft`, `appended = 1`, no fingerprint; the engine never runs it, and it becomes `queued` only at the approval of a Plan review of the drafts (§4.7). In phase `plan` every Job is a draft, so `--draft` changes nothing there. Its needs may be drafts. Without `--tier` and `--spec` it is a stub (above); `--draft` is the only way to add a Job without them |
| `--review` | a **review Job** (B2, contract §2.2): its Worker is a fork of the Plan's Lead, it commits nothing and writes `run/<plan>/reviews/<key>.md`. Its Dependencies are **set** to every Job of its PR that exists at that moment and is not `cancelled`, so `--needs` with `--review` is `usage` (exit 2). Jobs added later do not become its Dependencies. The Spec file is as for any Job; its `touches` is informational for a review Job (*decided*: the Lead lists `run/<plan>/reviews/<key>.md`) |

| Precondition | Code | Exit |
|---|---|---|
| the phase is `plan`, `dispatch`, `deploy` or `done` (`done` only with `--draft`, and then only into a PR that is not `merged` or `cancelled`: none exists until `pr add` has reopened the Plan); `grounding`: `illegal_phase` | 3 |
| the PR is not `merged` or `cancelled` (§3.2) | `pr_ended` | 3 |
| no Job with this key, unless its Locked fields (`pr` included) and needs are identical (then `already`) | `job_key_exists` | 3 |
| every `needs` key is a Job of the Plan | `needs_unknown` | 3 |
| every `needs` Job is in the same PR; `details.jobs` lists those that are not | `needs_other_pr` | 3 |
| no `needs` Job is `cancelled` or `failed`; with `--review`, no Job of the PR is `failed` (retry or cancel it first; `details.jobs` lists them), because a Dependency on a `failed` Job is refused | `needs_unavailable` | 3 |
| without `--draft`, in a phase after `plan`: no `needs` Job is a `draft`, and with `--review` no Job of the PR is a `draft` (`details.jobs` lists them; its message says to add this Job with `--draft`, or to approve the drafts first), because a `queued` Job could start before the draft it needs exists | `needs_unavailable` | 3 |

**A stub draft** stores `tier` and the Spec fields `goal`, `context`, `touches`, `acceptance` and `out_of_scope` as null for what was not given (a given `--spec` sets all five; `--tier` sets the Tier), and its `job-added` Event has `tier: null` when it has none. A draft that lacks its Spec or its Tier is **incomplete**: it is never queued or run (contract §3.2), a Plan review of it is refused `draft_incomplete` (§4.6) and so is `plan lock` (§4.8). `needs` and `pr` work as for any draft.

Effects: **rows**, one transaction: `jobs` (`pr` = the PR's key; `review` = 1 with `--review`; `status = draft`, `appended = 0`, no fingerprint, in phase `plan`; `status = draft`, `appended = 1`, no fingerprint, with `--draft` in any later phase; `status = queued`, `appended = 1`, fingerprint stored, in a later phase without `--draft`); one `dependencies` row per need (with `--review`, per Job of the PR, as the Dependencies are set in the same transaction). **Event** `job-added` `{pr, status, appended, tier, needs, review}`. **Re-arms `all-merged`** (hs-d140): the same transaction sets the PR's `all_merged_at` to null when it was set, so engine E4 appends `all-merged` for that PR again once its Jobs are all `merged` or `cancelled` (contract §3.7); a PR that never had it sent is unchanged. No cycle is possible: nothing needs a new Job yet. An appended Job of a `waiting` PR is not Ready until its PR starts (§3.2); an appended draft is not Ready at all until its review is approved; one of an `open` PR reaches its pull request through `pr rebase --pr <key>` (§9.3), or `pr push --pr <key>` (§9.9).

```json
{"ok": true, "job": {"key": "api-schema", "pr": "api", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false}}
```

Idempotent: yes, as stated (`--draft` is not a Locked field: the call that found an identical Job returns `already` whatever its status). `--review` is repeatable like any add: an identical call (same Locked fields, `review` included, same Dependencies) is `already`; a call after another Job was added to the PR differs in its Dependencies and is `job_key_exists`. `job edit` has no `--review` and refuses `--needs` and `--no-needs` on a review Job with `usage` (*decided*: its Dependencies are fixed by the add; to change them, cancel it and add a new one).

### 5.2 `job edit`

```
cpd job edit <key> [--title <text>] [--tier <T>] [--spec <file>] [--pr <key>] [--needs <k1,k2> | --no-needs] [--plan <id>]
```

**Completing a stub draft** (hs-d241, R1): `cpd job edit <key> --tier <T> --spec <file>` sets the Tier and the Spec of a draft that lacks them (a draft is editable, so this is an ordinary edit; `fields` lists `tier` and `spec`). Once the draft has both, it is complete and a Plan review may approve it (§4.6). The edit changes the plan fingerprint (contract §2.3), which covers a stub as it is.

A draft is editable in **every phase** (User, hs-d173, hs-d174, hs-d179), including one added after the lock, whose review may be pending: the edit changes the fingerprint of the drafts, so the pending review no longer matters (§4.6). `--needs` may name drafts. `--no-needs` removes every Dependency (cpd 1 had no way to). `--pr <key>` moves the draft Job to another PR (R34, A14); it is a field flag, so §1.3's default does not apply: without it the Job keeps its PR. Dependencies stay inside one PR (A7): a Job that changes PR keeps none of its old needs, so give `--needs` with Jobs of the new PR, or `--no-needs`, and no other Job may need it.

| Precondition | Code | Exit |
|---|---|---|
| at least one field flag | `nothing_to_edit` | 2 |
| the Job is `draft`, in whatever phase; a Job that is not `draft` is Locked | `job_locked` | 3 |
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
- `draft` (in any phase: a draft added after the lock is cancelled like one in phase `plan`), `queued` or `failed`: one transaction, `status = cancelled`, `cancel_reason`, `cancel_requested_at`; **Event** `job-cancelled` `{reason}`.
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

Effects: **rows**, one transaction: `status = queued`, `retry_tier = <T>`, `failed_cause = null`. **Event** `job-retried` `{tier}`. The next Attempt has `origin = retry` and this Tier, and the ladder restarts from it with a fresh cap (§9; Attempts whose driver never started are never counted).

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

Effects (§3.4 "The driver lock"): take `locks/attempt-<plan>--<attempt>.lock`; **rows**, one transaction: `attempts` (`starting`, Tier from §9, `origin`), Job `running`; **Event** `attempt-started` `{number, tier, origin}`; then spawn `cpd attempt run <attempt> --plan <plan>` with `setsid`, fd 3 = the lock, stderr to `run/<plan>/logs/attempt-<attempt>.log`, and record `driver_pid` (a data write, no Event). If the spawn fails, the command ends the Attempt `failed` / `crashed` with `driver_pid` still null (`attempt-retry` at the same Tier, not counted against `retry_cap`, §3.4) and returns `spawn_failed` (exit 1).

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
{"ok": true, "jobs": [{"key": "api-schema", "pr": "api", "title": "…", "status": "merged", "tier": "S2", "appended": false, "review": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee…"}]}
```

### 5.7 `job show`

```
cpd job show <key> [--plan <id>]
```

Effects: none. Output: every field of the Job (§2.2; `review: true` for a review Job, B2), `needs`, `needed_by`, and every Attempt with all its fields (`check_tail`, `metrics`) and its Escalations (cpd 1 §11, kept).

```json
{"ok": true, "job": {"key": "worker", "pr": "api", "status": "failed", "failed_cause": "timed_out", "tier": "S2", "review": false, "fingerprint": "9f…", "retry_tier": null, "…": "…"}, "needs": ["api-schema"], "needed_by": ["ui"],
 "attempts": [{"id": "worker-a1", "status": "ended", "outcome": "failed", "cause": "timed_out", "check_tail": null, "escalations": []}]}
```

`tamper` is the tamper report of §4.3, as in `plan show` (empty when the Job is intact). Both are read-only.

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
| the tamper check passes (§7.5; a protected path whose Attempt version equals the Lead tip's or `origin/<base>`'s, as a base merge brings it, is allowed); `details.paths` lists the protected paths touched that are not allowed | `gate_tampered` | 3 |
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

Effects: everything of §3.4 A1–A9, §7.2, §7.3, §7.4: the worktree and `pre-start`; the rendered Worker Spec (§16.1); the Worker's window `cpd-work:<plan>--<attempt>` (`new-window` with the Worker's argv of §6.5, `@cpd_session`); the Session rows (`session-registered`, `session-moved` `{place: "work"}`); the nudge (`attempt-nudge`); the stall kill (a Worker's session file idle for `timeouts.stall_seconds` inside one tool call: pi's descendant processes are killed, pi is not, the Attempt goes on; one driver-log line, no Event; contract §3.4 A4); the merge under the per-repo lock; the end (`metrics`, `kill-window`, worktree removal on `merged`). Each status change is one transaction with the Event of §3.4.

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

A Plan's PRs (R34, §2.2 PR, §3.8) and their pull requests, through `gh` (§7.6). `gh` failures are `gh_failed` (exit 1) with its stderr tail. Every command here works on **one PR**: `pr open`, `pr rebase`, `pr push` and `pr view` select it by `--pr <key>` (§1.3: omitted, the Plan's one PR that is not `cancelled`); `pr add`, `pr edit`, `pr cancel` and `pr start` (§9.4–§9.7) take its key as their argument; `pr poll` has no `--pr`: it polls every open pull request of the User. `<repo>`, `<base>`, `<lead branch>` and `<lead worktree>` are the selected PR's (§7); `gh` runs with cwd = its Lead worktree, so it addresses the PR's repo.

### 9.1 `pr open`

`pr open` is the **Mechanical checks** step of the Deploy stage (contract §3.9): it runs the Gate, then opens the pull request. Review follows the opening, not the Gate.

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
| the newest pull request of the Lead Branch is not closed without a merge (a merged one is adopted, below); `details.number`, `details.url` | `pr_closed` | 3 |
| the Lead worktree is clean | `dirty_target` | 3 |
| the Gate passes (§7.3, §7.6 `pr open` step 2); `details.tail` is its last 60 lines, `details.log` the full log | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |

Effects:
- **Lock:** the per-repo lock of the PR's repo (§7.4), then R for orphaned merges; held until the push has finished.
- **Adopting a merged pull request** (before the Gate; contract §7.6 `pr open` step 0): `gh pr list --head <lead branch> --state all --json number,url,state`, the newest decides. When it is `MERGED`, the Gate and the push are skipped (hs-d267 forbids skipping the Gate only when *opening* a pull request): one transaction records the pull request (its `pull_requests` row, `state` `merged`; the PR's `pr_number`, `pr_url`, `pull_request`) and moves the PR `dispatch → open → merged` exactly as `pr poll` does when it sees a merge: `pr-merged` (no `pr-opened`); `plan-phase` `dispatch → deploy` when every PR that is not `cancelled` is then `open` or `merged`, and `plan-done` (`deploy → done`) when every one is `merged`. The next `pr poll` cleans up the merged PR's worktrees. The output carries `"adopted": "merged"`. When it is `CLOSED` without a merge the command is refused `pr_closed` (reopen it on GitHub, or move the work to a new branch); when it is `OPEN`, it is adopted after the Gate and the push, as below.
- **Hooks:** the Gate, `pre-merge` from `<lead worktree>/.config/wt.toml`, in the PR's Lead worktree on its Lead Branch, before anything is pushed (R19); output to `run/<plan>/logs/pr-<key>.pre-merge.log`. On `gate_failed` or `gate_error` nothing is pushed, no row is written and the PR stays `dispatch`: the Lead appends fix Jobs to it (`job add --pr <key>`) and runs `pr open --pr <key>` again once they are Merged.
- **git:** `git -C <lead worktree> push -u origin <lead branch>`. The Lead Branch is usually on the remote already: the PR's start pushed it and every merge pushed it (§7.2, §7.4, R35), so this push is normally a no-op that sets the upstream, and it pushes again what an earlier `push_failed` left behind.
- **gh:** the lookup above; when it found an open pull request (an earlier run died before its transaction) that one is adopted; else `gh pr create --base <base> --head <lead branch> --title <title> --body <summary>`. `<title>` is the Plan's title when the Plan has one PR that is not `cancelled`, else `<plan title> (<key>)`; `<summary>` is the table of the PR's own Jobs and its Merged commits (cpd 1 §11, kept).
- **Rows**, one transaction, with a compare-and-swap on the PR's `status = 'dispatch'`: the `pull_requests` row of the pull request (inserted from `gh`'s URL, `repo`, `number`, `url`, the title given to `gh`, `draft` 0, `state` `open`, `review_decision` and `checks` `none`, `mergeable` and `merge_state` `unknown`; `created_at` from `gh`'s `createdAt`, read by a lookup of the pull request just created (`gh_failed` when `gh` cannot find it), written once: later polls never overwrite it; kept as it is when the poll stored it first; hs-d96, D6), then the PR `status = open`, `pr_number`, `pr_url`, `pr_opened_at`, `pull_request` (the row's id). **Event** `pr-opened` `{pr, number, url}`. When every PR that is not `cancelled` is now `open` or `merged` (A10), the same transaction sets `plans.phase = deploy` and appends `plan-phase` `{from: "dispatch", to: "deploy", prs: [{key, number, url}]}` (§3.1); otherwise the Plan stays in `dispatch`.

`phase` is the Plan's phase after the command.

```json
{"ok": true, "plan": "pay-4521", "phase": "dispatch", "pr": {"key": "api", "status": "open", "pr": {"number": 812, "url": "https://github.com/acme/pay/pull/812", "opened_at": "…"}}}
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
| a `gh` call fails, times out (15 s each), prints an empty or non-JSON body, answers the search with `errors`, or answers the lookup with no `data`; `details` is `{argv, exit, stderr}` | `gh_failed` | 1 |

There is no phase precondition: a Plan in any phase gets the poll.

Effects (contract §7.6 gives the query, the mapping and the caveats):
- **Lock:** `flock` on `locks/pr-poll.lock`, held for the whole poll, so two polls never overlap.
- **gh:** one `gh api graphql` call with two searches (`is:pr is:open author:@me archived:false sort:created-asc` and `review-requested:@me is:pr is:open archived:false sort:created-asc`, the oldest 100 each), then, for the pull requests of `prs` rows in status `open` whose `pull_requests` row is `state` `open` and that neither search listed, one lookup call with one alias per pull request (a pull request closed without a merge is looked up once: its row then holds `closed`, and it is not looked up again until a search lists it). Stdout of the lookup is parsed whatever the exit code (gh exits 1 when one alias is `NOT_FOUND`). A lookup answering `OPEN` changes nothing.
- **Rows**, one transaction for every change, none when nothing changed: `pull_requests` rows inserted or updated (`changed_at` = now) when a value changed (`mergeable`, `merge_state`, `authored` and `review_requested` included: `authored` is 1 when the pull request is in the `author:@me` search, `review_requested` when it is in the `review-requested:@me` search); a row is kept while it is in either search or a PR points to it, and deleted, when no PR points to it, once its pull request is in neither. The `authored` rows are what the Deploy column shows (§13.2, §17); the Inbox kinds read them all (§21).
- **Events**, in that transaction, for the Plan whose PR points to the pull request only: **Event** `pr-feedback` (data of contract §5.2) when the checks became `failed`, the review `changes_requested`, the state `closed` or `mergeable` `conflicting` (reason `conflict`; from anything else, once per transition). A pull request that became `merged`: the PR `status = merged`, `merged_at`, **Event** `pr-merged` `{pr, number, url, merged_at}`; when every PR that is not `cancelled` is then `merged`, the same transaction sets `plans.phase = done`, `done_at` and appends `plan-done` `{prs: [{key, number, url, merged_at}]}`, once each time the Plan reaches `done`: again when a reopened Plan's last PR merges (§3.1, D2). Pull requests no Plan owns get no Event, nor does one only in the review-request search: `pr-feedback` is unchanged, still only for the author's Plans.
- **Starting the PRs that waited for it** (A9): `pr poll` does not start a PR itself. Engine E7, which runs after E5 in the same tick (§3.7), starts every `waiting` PR whose needs are now all `merged`, from a fresh `fetch origin <base>`; when `pr poll` was run by hand, the next tick does, or the Lead runs `pr start <key>` (§9.7).
- **A pull request closed without a merge** (A11): `pr-feedback` with `reasons: ["closed"]`; the PR stays `open` in cpd, every PR that needs it stays `waiting`, and the Lead asks the User.
- **After a PR is merged** (§7.2; hs-d138, hs-d142): for each PR that became `merged` in this poll, kill that PR's Attempt windows; then, per Attempt of its Jobs whose worktree exists: `git worktree remove --force`, `git branch -D`; then its Lead worktree: kept when dirty (`git status --porcelain` not empty, untracked files included) or unpushed (`git rev-list HEAD --not --remotes=origin` not empty), else `git worktree remove` and `git -C <repo> branch -D <lead branch>`. The removals of a PR run under its repo's per-repo lock (contract §1.4), taken without waiting and held only while that PR's paths are removed; when it is held (a Gate, a merge or a push runs in that repo), the poll skips that PR with no Event and a later poll cleans it up, because the poll never waits and a Gate must not lose its worktree (hs-n124); a lock that cannot be opened counts as a failed removal. Killing the Attempt windows does not need the lock. Best effort per item: a failure does not stop the others. Remote branches are never touched. A kept or failed Lead worktree appends one **Event** `error` `{source: "pr poll", code: "cleanup_failed", details: {plan, pr, path, reason}}` (`reason`: `dirty`, `unpushed` or git's stderr tail), only from the poll that saw the merge. Every later poll retries the merged PRs that still have a path on disk, and those retries append no Event. It is per PR, not at `done`; PRs closed unmerged and abandoned Plans are not cleaned up here (G5).

Output: `polled`, the number of distinct pull requests in either search; `changed`, one entry for each pull request whose row was inserted or updated; `removed`, the number of rows deleted; `feedback`, `merged` and `done`, the `{plan, pr}` pairs that got `pr-feedback` or `pr-merged`, and the Plans that became `done`; `rate_limit`, the search's `cost`, `remaining` and `reset_at` (E5 reads it, §3.7); and, only with a Plan, `prs`, one entry for each of its PRs in key order that is not `cancelled`, with its `status` and its `pull_request` row (or null).

```json
{"ok": true, "polled": 3,
 "changed": [{"repo": "acme/pay", "number": 813, "state": "open", "draft": false, "checks": "failed", "review_decision": "none", "mergeable": "conflicting", "merge_state": "dirty"}],
 "removed": 1,
 "feedback": [{"plan": "pay-4521", "pr": "ui"}],
 "merged": [{"plan": "pay-4521", "pr": "api"}],
 "done": [],
 "rate_limit": {"cost": 1, "remaining": 4987, "reset_at": "2026-10-20T11:00:00Z"},
 "prs": [{"key": "api", "status": "merged", "pull_request": {"repo": "acme/pay", "number": 812, "url": "https://github.com/acme/pay/pull/812", "title": "Retry failed payouts (api)", "draft": false, "state": "merged", "review_decision": "approved", "checks": "passed", "mergeable": "unknown", "merge_state": "unknown", "created_at": "…", "merged_at": "…"}},
         {"key": "ui", "status": "open", "pull_request": {"repo": "acme/pay", "number": 813, "url": "https://github.com/acme/pay/pull/813", "title": "Retry failed payouts (ui)", "draft": false, "state": "open", "review_decision": "none", "checks": "failed", "mergeable": "conflicting", "merge_state": "dirty", "created_at": "…", "merged_at": null}}]}
```

Idempotent: yes; the rows hold what was reported, so a rerun with no change on GitHub reports nothing new (`changed`, `feedback`, `merged` and `removed` empty or 0) and writes nothing.

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
cpd pr add <key> --repo <path> [--base <branch>] [--needs <k1,k2> | --no-needs] [--plan <id>]
```

Adds a PR to the Plan, `waiting` (R34, A13, §3.8). In phases `grounding` and `plan` it is part of the plan fingerprint (§2.3), so the Plan needs a review that includes it; in `dispatch`, `deploy` and `done` it is **appended**, like `job add`, and engine E7 starts it once its needs are `merged` (at once when it has none, §3.7). In `deploy` and `done` it also **moves the Plan back to `dispatch`** (hs-d87: a merged PR can hold a bug or a regression, so a Plan takes follow-up work at any time; no Plan review of the PR itself, D4; the Jobs added to it as drafts are reviewed, §4.6). A follow-up PR is registered at once, even when it cannot run yet (hs-d239, hs-d241): `pr add <key> --repo … --needs …`, then one stub draft per planned Job (`job add --draft --title …`, §5.1). When the Plan has a `merged` PR on the legacy Lead Branch `cpd/<plan>`, it first **moves that branch** to `cpd/<plan>/<that PR's key>` (below).

| Flag | Meaning |
|---|---|
| `<key>` | PR key (§2.1), chosen by the Lead, e.g. `schema`, `web` |
| `--repo <path>` | required: the PR's repo; stored as the realpath of its main worktree. Several PRs of one Plan may share a repo |
| `--base <branch>` | the branch the pull request targets; default the repo's `origin` HEAD branch, else `main` (cpd 1 §11 `workflow init`, kept) |
| `--needs <k1,k2>` | keys of other PRs of the Plan that must be `merged` before this one starts, comma-separated, no duplicates (`usage`). Default: in phase `deploy` or `done` (the reopen, hs-d118), **every `merged` PR of the Plan**; in other phases none |
| `--no-needs` | no needs, also in phase `deploy` or `done`; exclusive with `--needs` (`usage`) |

| Precondition | Code | Exit |
|---|---|---|
| the Plan is not archived | `archived` | 3 |
| `--repo` is inside a git repository (`git -C <path> rev-parse --git-common-dir`) | `not_a_git_repo` | 3 |
| `<base>` exists: `refs/remotes/origin/<base>`, or `refs/heads/<base>` when the repo has no `origin` | `base_not_found` | 4 |
| no PR of the Plan has this key, `cancelled` ones included, unless its repo, base and needs equal the resolved flags, the default needs included (then `already`) | `pr_exists` | 3 |
| every need is a PR of the Plan; `details.needs` lists the unknown keys | `needs_unknown` | 3 |
| no need is `cancelled`; `details.needs` lists them | `needs_unavailable` | 3 |
| `refs/heads/cpd/<plan>/<key>` does not exist in the repo, and neither does `refs/heads/cpd/<plan>`, except the legacy branch of a PR of the Plan that is `merged` (moved below); `details.branch` names it | `branch_exists` | 3 |
| the legacy move's `git push origin --delete cpd/<plan>` succeeds; `details`: `branch`, `repo`, `stderr` | `push_failed` | 1 |

Any phase is allowed: `grounding`, `plan`, `dispatch`, `deploy` and `done`, so no `illegal_phase`. No cycle is possible: nothing needs a new PR yet.

**The legacy move** (contract §3.8, D3, hs-d118). Every Plan locked before R34 stored `prs.lead_branch = 'cpd/<plan>'`, and git cannot hold both `cpd/<plan>` and `cpd/<plan>/<key>`. When a PR of the Plan has `lead_branch = cpd/<plan>` and status `merged`, `pr add` does, before the insert and in this order, with `<k>` that PR's key:
1. `git -C <repo> branch -m cpd/<plan> cpd/<plan>/<k>`, when `refs/heads/cpd/<plan>` exists (a worktree that has it checked out follows the rename; unverified on git 2.43);
2. `git -C <repo> push origin --delete cpd/<plan>`, when the repo has `origin` and `git ls-remote --heads origin cpd/<plan>` lists it; a failure is `push_failed` and the database is unchanged;
3. in the transaction of the insert below, `UPDATE prs SET lead_branch = 'cpd/<plan>/<k>'` for that PR.

A legacy branch of a PR that is not `merged` still refuses `branch_exists`.

Effects: **rows**, one transaction: in `deploy` or `done`, `plans` (`phase = dispatch`, `done_at = null`); `prs` (`status = waiting`, `repo`, `base`, `lead_branch = cpd/<plan>/<key>`, `created_txn`, `created_at`; `lead_worktree` null); one `pr_needs` row per need; and the legacy PR's `lead_branch` update (step 3). **Events**, in this order: in `deploy` or `done`, `plan-phase` `{from: "deploy"|"done", to: "dispatch"}`; then `pr-added` `{pr, repo, base, needs, appended}`, `appended` true in phases `dispatch`, `deploy` and `done`. Engine E7 then starts the PR like any appended PR (at once when it has no needs). No tmux and no other git write: the Lead Branch and Lead worktree are created when the PR starts (§3.8, §7.2).

```json
{"ok": true, "pr": {"key": "web", "repo": "/home/clayton/projects/web", "base": "main", "needs": ["api"], "status": "waiting", "lead_branch": "cpd/pay-4521/web", "appended": false}}
```

Idempotent: yes, as stated; every step of the legacy move is too (A3): a rerun after a crash between the local rename and the transaction finds `cpd/<plan>/<k>` and no `cpd/<plan>`, skips what is done and completes. The comparison uses the resolved repo and base, so a rerun without `--base` matches a PR whose base was defaulted to the same branch.

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

Effects: **gh**, in the PR's Lead worktree: `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url` and `gh api repos/{owner}/{repo}/pulls/<n>/comments`. No rows, no Event.

Output: `plan`; `pr`, the PR's `key`, `number`, `url`, `title`, `state` (`open`, `closed` or `merged`), `review_decision` and `checks` (`none`, `passed`, `pending` or `failed`; the mappings of contract §7.6, applied to this call's fields); `checks`, one entry for each check, `{name, conclusion, url?}`; `reviews`, `{id, author, state, body}`; `comments`, `{id, author, body}`; `inline_comments`, the pull request's review comments, `{id, author, path, body}`.

```json
{"ok": true, "plan": "pay-4521",
 "pr": {"key": "ui", "number": 813, "url": "https://github.com/acme/pay/pull/813", "title": "Retry failed payouts (ui)", "state": "open", "review_decision": "changes_requested", "checks": "failed"},
 "checks": [{"name": "ci/test", "conclusion": "FAILURE", "url": "https://github.com/acme/pay/actions/runs/1"}, {"name": "ci/lint", "conclusion": "SUCCESS"}],
 "reviews": [{"id": "PRR_kwDO…", "author": "reviewer", "state": "CHANGES_REQUESTED", "body": "Please add a test."}],
 "comments": [{"id": "IC_kwDO…", "author": "reviewer", "body": "Thanks for the fix."}],
 "inline_comments": [{"id": "1893", "author": "reviewer", "path": "src/payout.rs", "body": "This retry never backs off."}]}
```

Idempotent: yes (a read).

### 9.9 `pr push`

```
cpd pr push [--pr <key>] [--plan <id>]
```

Publishes an `open` PR's Lead Branch with a plain push, after the Gate (hs-d139; contract §7.6). It does not rebase: use it when the branch carries a merge of the base, which `pr rebase` would drop (§9.3), or when the remote has not moved and the Jobs Merged since only need to reach the pull request.

| Flag | Meaning |
|---|---|
| `--pr <key>` | the PR to push, selected by §1.3's rule: required when the Plan has two or more PRs that are not `cancelled`, defaulted to the one when it has one |

| Precondition | Code | Exit |
|---|---|---|
| the PR is `open` | `illegal_transition` | 3 |
| the per-repo lock within `lock_wait_seconds` | `lock_timeout` | 5 |
| the Lead worktree is clean | `dirty_target` | 3 |
| the Gate passes (§7.3, as `pr open` step 2); `details.tail` is its last 60 lines, `details.log` the full log `run/<plan>/logs/pr-<key>.pre-merge.log` | `gate_failed` | 3 |
| the Gate ran (not killed, not `hook_invalid`); `details.tail`, `details.log` as above | `gate_error` | 1 |
| the remote branch is not an ancestor of the Lead Branch (the remote moved); `details.stderr` is git's stderr; the Lead runs `pr rebase` | `push_rejected` | 3 |

Effects: under the per-repo lock of the PR's repo, after R (§7.4): the clean check, the Gate, then `git -C <lead worktree> push --porcelain origin <lead branch>` (no `-u`, never `--force` or a lease). Any other git failure is `git_failed`. **Rows**: none; no Event.

```json
{"ok": true, "plan": "pay-4521", "pr": "api", "pushed": true, "sha": "77aa01e…"}
```

`sha` is the Lead Branch's tip. Idempotent: yes; a branch already at the remote's sha (git's porcelain status `=`, "up to date") still runs the Gate, pushes nothing and returns `"pushed": false`.

## 10. `session`

Sessions (§2.2, §3.6) and their tmux panes (§6). Every tmux call is `tmux -L <tmux.socket> …` with `TMUX` and `TMUX_PANE` removed (§6.1); a failing call is `tmux_failed` (exit 1). A Session's **place** is the `place` of its latest `session-moved` Event (`chat`, `parked`, `work` or `none`), or `none` when it has none.

**The chat slot.** **Amends contract.md §6.2** (the Lead's answer to Escalation `contract-commands-a1-q1`): the chat slot is the pane of `cpd:main` that has `@cpd_session` **or** `@cpd_view = chat`. A pane with `@cpd_view = chat` is the **placeholder**: it runs `cpd tui view chat`, which reads `No Lead here: pick a Plan in the session tree`, and it holds the slot while no Lead is there. **Amends §6.2's list of `@cpd_view` values** with `chat`. Why: the slot must hold a pane before any Lead exists (the first `cpd tui` on a machine) and after its Lead is parked (`swap-pane` needs a pane to swap in). At most one placeholder exists, and only ever in the slot: `session park` creates it there (§10.5), and `session open` kills it with its window when a Lead takes the slot (§10.4).

### 10.1 `session list`

```
cpd session list [--context <id>] [--plan <id>] [--role <role>] [--state <state>]
```

Filters combine with AND; `--role` and `--state` may repeat. Without filters: every Session, archived ones included (`archived_at` is null or a time). Here `--context` and `--plan` are filters only: neither falls back to `CPD_PLAN`.

Effects: none; one read transaction.

```json
{"ok": true, "sessions": [
  {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "state": "blocked", "state_label": "cpd: Worker question", "state_at": "…", "archived_at": null,
   "place": "chat", "tmux_pane": "%12", "file": "/home/clayton/.pi/agent/sessions/…_596d007f-….jsonl", "forked_from": null, "plan_lead": true, "tier": "O3", "cwd": "/home/clayton/projects/cpd"}],
 "events_cursor": 48213}
```

`role` is `lead`, `worker` or `manager` (B3-A1; a Manager has `plan` null, `attempt` null and `plan_lead` false; `--role manager` lists them). `plan_lead` is true for the Session that is its Plan's `lead_session`. Read-only.

### 10.2 `session show`

```
cpd session show [<id>]
```

`<id>` defaults to the selected Session (`CPD_SESSION`); it accepts the short forms of §1.3. Output: the fields of `session list` plus `created_at`, `registered_event` (the id of its `session-registered` Event, the pi adapter's first cursor, §15.2) and, for a Plan's Lead, `open_escalations` (ids). Effects: none.

```json
{"ok": true, "session": {"id": "596d007f-…", "role": "lead", "context": "building-cpd", "plan": "cpd2", "attempt": null, "name": "cpd rust lead", "state": "idle", "state_label": null, "place": "chat", "tmux_pane": "%12", "file": "…", "forked_from": null, "plan_lead": true, "tier": "O3", "cwd": "/home/clayton/projects/cpd", "created_at": "…", "registered_event": 40117},
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
- **tmux:** when the Session has no live pane (`stopped`, or its pane is gone): first `kill-pane` on a dead pane of the Lead, if one is still there, so it never adds a second window; then `new-window -d -t =cpd: -n park-<label> … -- cpd session run <id>` first, which resumes it (`pi --session <file>`, §6.5), and `tmux_pane` is recorded. Then `swap-pane -d -s <its pane> -t <chat slot pane>`, and `rename-window -t <window> park-<displaced label>` for the window that now holds the displaced Lead. When the displaced pane is the placeholder or a history pane (`@cpd_view` `chat` or `history`, §10.12), `kill-pane` on it instead, which closes the now-empty park window with it, so placeholders and history panes never pile up in park windows.
- If a step after the new window is created fails (`set-option -p @cpd_session`, the swap, the row or the `session-moved` write), that new window is killed (`kill-window`) before the error is returned (teardown G11).
- **Rows**, one transaction: **Events** `session-moved` `{place: "chat"}` for this Session and `{place: "parked"}` for the displaced one.

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
cpd session state <working|blocked|idle|stopped> [--label <text>] [--reply] [--session <id>]
```

How the pi adapter reports its agent state (§3.6, D4). `--label` is required with `blocked` and refused with any other state (`usage`); 1–80 characters. `--reply` (hs-d278) is allowed only with `idle`, else `usage`: it says the run that ended answered a message the User wrote to this Session (§15.4). Internal (§1.1): hidden in clap, run only by the pi adapter.

| Precondition | Code | Exit |
|---|---|---|
| the transition is in §3.6's table | `illegal_transition` | 3 |

Effects: when the state and label are unchanged, nothing (`"already": true`, no Event, §3.6). Otherwise **rows**, one transaction: `state`, `state_label` (null unless `blocked`), `state_at`. **Event** `session-state` `{from, to, label}`, plus `reply: true` when `--reply` was given (contract §5.2); `reply` is written only on a real change, so a call that changes nothing (`"already": true`) records no reply. No tmux.

```json
{"ok": true, "session": "596d007f-…", "state": "blocked", "label": "cpd: Worker question", "from": "working"}
```

Idempotent: yes.

### 10.7 `session rename`

```
cpd session rename <name> [--session <id>]
```

cpd 1 §11 `session rename`, kept; the queue file becomes an Event. `--session` accepts the short forms of §1.3.

Effects: unchanged name: `"already": true`, nothing written. Otherwise **rows**, one transaction: `sessions.name`. **Event** `session-renamed` `{name}`, addressed to the Session (`session` set), which its adapter applies with `pi.setSessionName` (§15.4). **tmux**: sets `@cpd_title` on the Session's pane when it has one; nothing else.

```json
{"ok": true, "session": "596d007f-…", "name": "cpd rust lead"}
```

Idempotent: yes.

### 10.8 `session run` (internal)

```
cpd session run <id> [--adopt <pid>]
```

The program in every Lead pane (§6.2): owns its terminal (§1.2). Steps of §6.7:
1. If the Session's file exists, resume: `pi --session <file>` with the Lead's `[-e …]` (§6.5).
2. Else, if `forked_from` is set, wait until the source is idle (§6.7 step 1, every 2 s, showing the waiting line), then `pi … --fork <source file> --session-id <id> -n <name> [-- @<first message file>]`.
3. Else start a fresh Lead: `pi --model … --session-id <id> -n <name> -- @<first message file>`.

Before it starts pi (steps 1–3) a Lead with a Plan gets its plannotator port (§6.5 "The Plan's plannotator port"): `PLANNOTATOR_PORT=<port>` replaces the value in the pane's environment. When the range is full the pane shows `no_port` and names the Plans that hold it, and Enter retries; pi is not started. A Manager gets no port.

**Reload on install** (hs-d33, hs-d38, hs-d49, hs-d151; §6.7 "Reload on install"; incident hs-n141, cause hs-n148). The wrapper detects a new install by the identity of its executable (§3.4 of the contract), checked at every iteration of its loop **before any store access**. On a new cpd it re-execs **at once**, with no idle wait, whatever the Lead's place or state, and **keeps pi running**: pi is its child and survives the `exec`. It `exec`s the same path with the same argv plus the internal argument `--adopt <pid>` (**internal**: written only by the wrapper's own `exec`, not an interface of the User or an agent); the new `session run` skips steps 1–3, starts no pi, `waitpid`s that pid as its own child and continues the same loop on it (watchdog, archive on quit, `Press Enter to resume`). The adapter reloads itself at idle (hs-a5, §15.6); a cpd install does not restart pi. Before the `exec` it runs `<path> --version` and re-execs only when that succeeds and `store` is `null` or `store.opens` is `true` (§1.2); otherwise it does not `exec`, logs once per installed file (identity) `reload skipped: <path> cannot open the store: <store.refusal.message>` on the stderr of its pane and keeps running, pi untouched; a later install is checked again (contract §6.7). **A store error never ends a wrapper while its pi runs:** a failed read with a replaced cpd leads to the re-exec above; any other failed read is logged once on stderr and retried at the next poll, pi untouched. Why: wrappers that waited for the Lead's idle read the store after a newer cpd had migrated it, failed `store_too_new` and exited, and six Leads were ended instead of reloaded (hs-n141, hs-n148). If pi exits on its own after an install, the wrapper `exec`s the new binary (no `--adopt`) before any store write, and the existing paths run there.

**Restart for a new pi** (hs-d49). A change of pi (the identity of `<pi agent dir>/install/current-version`, else of the resolved `pi` executable) is the only reload that restarts pi, and it waits for the Lead's next idle (state `idle`, last turn finished, session file quiet `fork_quiet_seconds`): the wrapper sends pi `SIGTERM`, waits for it to exit (`SIGKILL` 10 s after `SIGTERM`) and `exec`s the same path with the same argv (no `--adopt`); the new `session run` takes step 1. A parked Lead restarts at once. The Lead in the chat slot waits until it is idle **and** parked, or idle for 10 minutes, so unsent typing is not lost. A `blocked` Lead is never idle. See contract §6.7 "Reload on install".

**Adapter watchdog** (hs-d141; contract §6.7 "Adapter watchdog"). While pi is alive, the wrapper checks the Session's state at every poll (2 s). When the state has been `stopped` for more than **60 s** (a constant of the wrapper, not a config key; measured from the `session-state` Event that set it, so a state the wrapper itself set after a pi exit does not count while pi is not running), it restarts pi on the session file: `SIGTERM`, wait (`SIGKILL` after 10 s, as for a reload), then step 1. In one transaction before the restart it appends one `error` Event `{source: "session run", code: "adapter_missing", message, details: {session, state_since, seconds}}` with `session` = this Session (and its `plan`, `context`), which the new adapter delivers once it follows (§15.3). It applies to every Lead and Manager, whatever the place and whatever the idle rules of the reload; a `stopped` that ends (`idle`) before 60 s is the adapter's own reload and is never touched. After the restart the 60 s start again; a broken install loops once per 60 s with one `error` Event per restart (no cap: each is a visible fact, and a repaired install ends it).

The first message is the file `plan new` or `session fork` wrote (§1.4); for a Manager, `run/<context>/manager-message.md` (§3.10); none when resumed. It is passed as `@<path>` (its absolute path). While pi runs, it watches for the session file (§6.5) and records `sessions.file` once it appears (a data write, no Event). When pi exits it runs `session state stopped` if the adapter did not, prints `pi exited. Press Enter to resume <name> here.` and resumes on Enter, except after a quit by the User (below).

**Auto-archive on the User's quit** (hs-d91, hs-d122). pi exits with status 0 on the User's `/quit` and Ctrl+D (pi 1.2.0, `interactive-mode.js`: `shutdown()` ends in `process.exit(0)`; verified by reading it). The wrapper tells that quit from its own: when pi exits with status 0 and the wrapper did not cause the exit (it sent no reload `SIGTERM`), the User quit. Then, in this order:
1. If the Lead is in the chat slot, `session park` it first (§10.5), so the placeholder takes the slot.
2. Archive: a side fork runs `session archive <id>` (§10.10); a Plan's own Lead runs `plan archive --plan <p>` (§4.10), the cascade, forks included. The archive **rows are written before** any pane is killed, and the wrapper's own pane is killed **last**.
3. The wrapper's pane is closed with the archive (§10.10 step 2, §4.10).

If the archive is refused (`archive_blocked`, for example a Job running), nothing is archived: the pane prints the refusal's message and then `pi exited. Press Enter to resume <name> here.`, as before. A non-zero exit, a signal, or an exit the wrapper caused (a reload) keeps the behaviour above: no archive. So does a Manager's quit: a Manager is archived only with its Context (§3.8). The tests use the harness's fake pi, exiting 0 or 1.

| Precondition | Code | Exit |
|---|---|---|
| the Session is a `lead` or a `manager` | `not_a_lead` | 3 |
| its pane's `@cpd_session` is this Session | `wrong_pane` | 3 |
| the Plan's port is free in the range (Lead with a Plan only) | `no_port` | 3 |

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
| its state is not `working` or `blocked`; `details.blockers` lists it | `archive_blocked` | 3 |
| it is not in the chat slot; `details.next` = `cpd session park` | `in_chat_slot` | 3 |

Effects, in this order:
1. **Check** the rule above; nothing is written if it blocks.
2. **tmux:** a live pane is closed as `session close` does (§10.9).
3. **Rows**, one transaction that re-checks the rule: `sessions.archived_at` = now; **Event** `session-archived` `{archived: true}`.

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

Effects: **rows**, one transaction: `sessions.archived_at = NULL`; **Event** `session-archived` `{archived: false}`. No tmux: no pane is started; `session open` resumes the Lead (§10.4).

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
cpd session send <id> (<text> | --file <f>) [--wake]
```

Sends one message to a Lead or a Manager Session (hs-d92, hs-d133). It appends an Event; the target's adapter delivers it (§15.3). **By default it never starts a turn** (hs-d209, D3): the target holds the message in its context at its next turn.

| Argument | Meaning |
|---|---|
| `<id>` | the target Session (the short forms of §1.3 are accepted) |
| `<text>` | the message, a `Text` (1–20 000 characters) |
| `--file <f>` | the message read from a file, same limits |
| `--wake` | the message starts a turn when the target is idle (a working or blocked target gets it after its running turn, never interrupted). For a Manager waking a stalled Lead; use it sparingly. Default: off |

Exactly one of `<text>` and `--file` (else `usage`).

| Precondition | Code | Exit |
|---|---|---|
| the Session exists | `session_not_found` | 4 |
| it is a `lead` or a `manager`; a Worker is refused with "Session <id> is a worker, not a Lead or a Manager." | `not_a_lead` | 3 |
| it is not archived; `details.next` = `cpd session unarchive <id>` | `archived` | 3 |

A `stopped` Lead or Manager is accepted: it gets the message when its pi starts (its follower replays from its cursor, §15.2).

Effects:
- **Rows**, one transaction: **Event** `session-message` with `session` = the target, `context` and `plan` = the target's, `data` = `{from, text, wake}`; `wake` is true only with `--wake`. `from` is `CPD_SESSION`, else `PI_SESSION_ID`, else null (the User). The Event's `text` is the message text; the adapter composes the delivered content (§15.3).
- **tmux:** none. No pane is started or typed into.

```json
{"ok": true, "event": 4812, "session": "a41b…", "from": "596d007f-…", "wake": false}
```

`from` is null when neither variable is set. `wake` echoes the flag. Idempotent: no; each call is one message.

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

**Any store error ends it** (hs-d140; contract §5.4): `store_too_new` after an upgrade (the verdict of contract §4.5, which the follower recomputes when `PRAGMA schema_version` changes), `migration_changed`, `migration_order`, `store_busy`, `store_failed`, … is printed as the last line, as the failure envelope, and the process exits with that code's exit (§1.2); it never stays alive on a store it cannot read. The pi adapter restarts a follower that ends (§15.2).

**Re-exec** (hs-d140; contract §5.4): between polls it checks its executable path with `core/src/host/exe.rs`'s `(device, inode, mtime)` check, as `session run` and `attempt run` do (hs-d55); when it changed it re-execs itself with the same argv except `--since`, set to the id of the last Event it printed (none printed: the cursor it started from), so no follower outlives an install. Each line:

```json
{"id": 48215, "at": "2026-10-14T21:41:02.114Z", "kind": "escalation", "context": "building-cpd", "plan": "cpd2", "job": "contract-commands", "attempt": "contract-commands-a1", "session": null, "text": "Worker contract-commands-a1 asks: … Answer: cpd answer contract-commands-a1-q1 \"<answer>\"", "data": {"escalation": "contract-commands-a1-q1", "question": "…"}}
```

Idempotent: read-only; the same `--since` replays the same lines.

## 12. `watch`

```
cpd watch [--ensure]
```

The engine, one per machine (§3.7, §6.6, D5).

**Without `--ensure`:** runs E1–E5 and E7–E9 (E6 is removed; E7, the PR start, since R34; E8, the Inbox, §21; E9, the Context digests, since hs-d194) every `tick_seconds` in the foreground, in `cpd-work:watch`, logging to stderr and appending the same lines to `run/watch.log`.

| Precondition | Code | Exit |
|---|---|---|
| `locks/watch.lock` is free | `watch_running` | 3 |

Effects: **Events** `watch-started` and `watch-stopped` `{pid}`, and every row and Event of E1–E5 and E7–E9 (§3.7; a PR that E7 fails to start gets its own `error` with code `pr_start_failed` and is not tried again, §9.7; a distill run E9 starts appends its own `error` when it fails, §3.12). An unexpected failure in a step appends `error` `{source: "watch", code, message, details}` for the Plan it concerns and the loop continues; the same failure for the same object is appended once until it clears. It runs until SIGTERM or SIGINT, then prints `{"ok": true, "stopped": true}` and exits 0. Idempotent: a second one is refused.

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
cpd tui view <tree|plan|widgets|statusbar|chat|legend>
cpd tui view logs [--event <id>]
cpd tui view worker <attempt> --plan <id>
cpd tui view history <id>
```

The ratatui programs in the cockpit's panes (§6.2). They own their terminal (§1.2), make no writes of their own, and change state only by running `cpd` commands (`session open`, `plan new`, `session close`, `session park`, …) as subprocesses. What they read is §17.

**Cockpit parity** (User, 2026-10-15). The cockpit matches the running cpd 1 cockpit exactly: layout, elements, text, colours, keys, mouse and behaviour. The reference was cpd 1's cockpit spike, `tui/` on branch `main` before the switch-over (`tui/src/*.rs`, `tui/README.md`; `tui/` now holds the Rust cockpit crate `cpd-tui`); the checklist of every difference is [cockpit-parity.md](cockpit-parity.md). Where this section states a deviation, it is deliberate and has its reason.

| View | What it is | Keys and mouse |
|---|---|---|
| `tree` | the **Context column**: Contexts, their Plans' Lead Sessions, and counters | ↑↓ and j/k move the cursor, Enter opens the Lead in the chat slot (on an Inbox row it jumps to the item's cause), `n` a new Plan, `x` closes a Lead's pane, `s` and `d` snooze and dismiss the selected Inbox item, `?` the legend; clicks and the right-click menu as below |
| `plan` | the **Plan view**: one box per PR of the chat slot's Plan, each holding the flowchart of its Jobs; with a Manager in the chat slot, it shows **Pulse** (one row per Plan of the Context) | ←→↑↓ and h/j/k/l select, Enter opens a Job's Worker, Esc clears the selection; the wheel scrolls (3 rows, 4 columns); clicks as below; no `q`. The view remembers each Plan's scroll offset while it runs and restores it when a Lead of that Plan comes back to the chat slot (a Plan Lead and its side forks share it), clamped to the chart, which may have changed size; the selection is not kept (reset), and a restart of the view forgets every offset (User, hs-d197). With Pulse, a left click on a row opens that Plan's Lead and the wheel scrolls the rows (3 rows a notch) |
| `widgets` | the **Deploy column**: the machine, the cockpit's PRs, git and Jira (future) | `q` quits; a left click on an owned PR row brings its Lead into the chat slot (below); the wheel over the PRs section scrolls it by one pull request per notch (below); mouse capture on |
| `worker` | the read-only Worker view: the Attempt's Worker session file (vision decision 5), `you` and its text in cyan, `agent` in green, `→ tool` in yellow, `󰘌 last` (U+F060C md-subdirectory_arrow_left, the tool result's arrow) dim | `q` or Esc quits; ↑/k, ↓/j, PageUp, PageDown and the wheel (3 rows) scroll; End or `G` follows the end again; no other keys |
| `history` | an archived Session's history, read-only: the Session's pi file drawn with the Worker view's renderer (`tui/src/worker/render.rs`); its top line is `<name> (archived, read-only)`. Started by `session history` (§10.12) in the chat slot | `q` or Esc quits to the placeholder text; ↑/k, ↓/j, PageUp, PageDown and the wheel (3 rows) scroll; End or `G` follows the end again; no other keys |
| `statusbar` | one row across the bottom | no keys, no focus; clicks on its buttons |
| `chat` | the chat slot's placeholder (§10) | none |
| `legend` | the key legend, in a popup | any key closes it |
| `logs` | the Event log, in a popup; `--event <id>` opens it at that Event | ↑↓, j/k, PgUp/PgDn scroll; `q` or Esc closes it |

**Design rule** (User, Plan tui-visuals, 2026-10-17; hs-d124). Top-level layout separators (the tmux pane borders) are white; separators inside a slot are grey. A header is one line with its title inverted: the pane headers (tmux, reversed, contract §6.1) and the section headers inside a slot alike. Inside a slot the header is a light `─` line in grey (the palette's Nothing colour, colour 8) with the title as a grey inverted block, not bold: two `─` cells, then ` <title> ` reversed (grey background, text in the terminal's background colour), then `─` to the right edge, e.g. `──[ PRs ]──────────` (`[ … ]` is the reversed block). A slot's first section has no header, so no two titles stack: the pane header is the slot's title.

**`tree`.**
- **Top row and side margins.** The view keeps one blank row at its top and one blank column on each side; its content starts one row lower, inside the inner area (the pane minus one column left and right), so the tree never touches the pane's edges. Everything the view draws sits inside it: the cursor's inverted row, the footer and prompt row, and the right-click menu's clamping.
- **Rows.** A Context row (a dot, its name bold, the Plan count when collapsed) and under it one row per Lead Session: the Plans' Lead Sessions in `created_at` order, **then, after them, every Session forked by `session fork`** (B1, contract §6.8: a fork is a sibling under the Context, never nested under a Plan or under the Session it came from, whatever its `plan` or `forked_from`; *decided*: they are ordered by `created_at` and show no counters, the counters being the Plan Lead's). Row: guide, dot, name (cut with `…` before the counters), and the five Job counters `queued⋅running⋅merging⋅failed⋅merged` right-aligned (cancelled Jobs are not counted; no counters without a Plan). `󰐊` marks the Session in the chat slot, in column 0, and its name is bold. The dot is one of `󰪥` U+F0AA5 (the Session needs you, coloured by case; the last frame), `󰝦󰪞󰪟󰪠󰪡󰪢󰪣󰪤󰪥` U+F0766, U+F0A9E … U+F0AA5 (nf-md-circle_outline, then nf-md-circle_slice_1 … 8) (the Lead is working: a spinner of 9 frames, one every 100 ms, on one shared clock, so every spinner turns together; terminal default foreground), `󰝦` U+F0766 (idle, nothing needed; the first frame) or `󰝦` U+F0766 (pi stopped, dim). A Session row's dot looks at the Session's own state first (a working Session shows the spinner), and its `󰪥` comes only from its open, visible Inbox items (§17); a Context row shows the dot of its Manager Session (hs-d215): a `󰪥` only from the Manager's own Inbox items (hs-d217), `󰝦` (dim) when the Context has none. Archived rows keep their glyph in the palette's darker grey (`Meaning::Nothing`) and never animate. There are no phase colours on dots. The cockpit needs a Nerd Font for these symbols (https://www.nerdfonts.com/cheat-sheet). The rule, the colours and the precedences are §17's (The tree's dot rule).
- **Cursor.** Keyboard only: a click never moves or shows it. It moves between Sessions only (Context rows are not selectable) and, after the last Session row, on into the Inbox section's item rows (below; the header and the `+N more` / `− less` row are skipped by the cursor but their click works, and the cursor goes back up from the first item row to the last Session), is drawn inverted across the full row only while the pane has keyboard focus (`focus-events`, §6.1) and after an arrow key; Enter, `n` and `x` act on it only while it shows.
- **Inbox section** (hs-d165, hs-d167, hs-d168; contract §2.2 "Inbox item", commands §21). The bottom of the tree column shows the Inbox, the **single source of "needs you"**: the open, visible items (not snoozed, not dismissed) of **every** Context, in the order of `cpd inbox list` (red, yellow, magenta, white; oldest first; `engine-down` first). It has a header row ` Inbox <n> ` (`<n>` the count of visible open items) drawn like the column's other section headers (design rule above), then **up to 3 item rows**, each `󰪥 <label>` with the age right-aligned (`42s`, `5m`, `3h`, `2d`: the largest whole unit of `age_seconds`) and the `󰪥` in the item's urgency colour (red, yellow, magenta, white; the palette's `Meaning` colours, as the Session dots, §17); the label is cut with `…` before the age. When more than 3 items exist, a row `+<n> more` follows; a click on it expands the list inline (every item shown, the Session rows above scroll to keep the cursor visible) and the row becomes `− less`, which folds it again. Expanded or not is view state only: no command, no write. With no item the section shows its header (` Inbox 0 `) and no rows. The section is pinned to the bottom of the column and the tree scrolls above it.
- **Clicks on an Inbox item** jump to its cause (the cockpit never decides; each jump runs a command or is a link):

  | Kind | A left click on the row |
  |---|---|
  | `dialog`, `unread` | `session open <the item's Session>`, then `select-pane` on the chat slot: as a click on that Session's tree row (an archived Session: `session history`, as there) |
  | `job-stuck` | `session open <the Lead of the item's Plan>` (`plans.lead_session`), then `select-pane` on the chat slot |
  | `plan-review`, `pr-failing`, `pr-changes`, `pr-ready`, `review-requested` | none by the view: the row's text is an **OSC 8 link** to the item's `url`, written with `tui/src/link.rs` over the cells already drawn (Links, below), so the terminal opens it with its modifier-click. For `plan-review` the page is plannotator's, `http://localhost:<port>/`, the port being the one line of `run/<plan>/plannotator-port` (as the Plan view's pending-review row, above); for the other kinds the pull request's GitHub page. An item with no `url` (the port file missing or not a number) is drawn with no link and a click does nothing |
  | `engine-error`, `engine-down` | the logs dialog (`cpd tui view logs --event <the item's event>`, in the popup of the StatusBar's `▤` button) opened **at the item's `event`**: the Event's line is selected and scrolled into view and the view does not follow until the User reaches the bottom. `engine-down` has no stored item and so no `event`: it opens the logs at the bottom, as `▤` does |
  | `question` | `session open` of the Context's Manager Session (`cpd context manager --context <c>`, then `session open <manager>`, as a click on a Context row) |

  Whether the terminal opens an OSC 8 link on a row of this view through tmux is **unverified**; the cockpit Job tests it by hand with the User, as hs-d187, hs-d191 did for the PR links.
- **Clicks.** A click on a live Session opens it in the chat slot and focuses the chat (`session open`, then `select-pane`); a click on an **archived Session** row (a Plan row's Lead or a fork) runs `session history <id>` (§10.12), then focuses the chat; a click on a Context row's fold glyph (`󰅂`/`󰅀`: U+F0142 md-chevron_right folded, U+F0140 md-chevron_down open) folds or unfolds it, and a click elsewhere on a live Context row runs `cpd context manager --context <c>`, then `session open <manager>`, then `select-pane` on the chat slot (B3-A6); a click on a `󰇘` (U+F01D8 md-dots_horizontal) row toggles its archived rows (below). Clicks are ignored while a prompt is open.
- **`n`** asks for a name in one inline row at the bottom (Enter starts, Esc cancels, an empty name is refused) and runs `plan new` (§4.1) in the selected Session's Context: the Plan forks the Context's grounded Lead (R15), the one deviation from cpd 1's `n`, which forked the selected session. On a Context row it says `pick the session to fork`.
- **`x`** closes the selected Session's pane after the inline prompt `Close <label>? pi stops; the session file stays (y/N)`, preceded by `<label> is working.` when its state is `working`; only `y` closes. It runs `cpd session close <id>` (§10.9). The Session in the chat slot is refused with `switch to another session first`, also when it moved there while the prompt was open. On a Context row it does nothing. `<label>` is the Session's name.
- **`s` and `d`** act on the Inbox item under the cursor (they do nothing on a Session row): `d` runs `inbox dismiss <id>` at once; `s` opens the snooze choices, the menu's three `Snooze …` entries (↑↓ and Enter pick one, Esc closes), then runs `inbox snooze <id> <15m|1h|tomorrow>`. Enter on an item row jumps to its cause as a click does (above; for an OSC 8 kind with no click handler, Enter does nothing). A refusal (`item_closed`: the item closed meanwhile) is shown in the prompt row as the command's own message. The item leaves the section when the Inbox-* Event reaches the view (§17).
- **`?`** opens the legend popup. The view has no other key that opens the menu or acts on it; what a key used to do is still a command.
- **Labels** (contract §2.2: ids are unique, immutable and need not be readable; the title is the visible label). A Context row shows the Context's **id**, a Plan Lead row the Plan's **id**, and a fork row (a side Session) `<plan id>·<first 4 characters of its Session id>` (for example `cross-repo·7f3a`); a fork with no Plan keeps the fallback, the first 8 characters of its Session id. Archived rows follow the same rule. Titles and names no longer show in the tree (User, hs-d101; this supersedes the titles rule, hs-d63).
- **Archived rows** (hs-n38, hs-n39). Archived Contexts collapse into one `…` row at the end of the Context list; the archived Plans and Sessions of a Context collapse into one `…` row at the end of that Context's subtree. A click on the `…` row expands them and a click on it again collapses them (hs-d53); a click on a guide does nothing. Expanded, an archived Plan or fork is drawn **at the depth it would have if live**, under its own Context (or Plan), not one level deeper under the `…` row. It uses the normal tree guide characters, and the row, guides included, is coloured the palette's darker grey (`Meaning::Nothing`). There are no dotted lines and **no empty row** between the live rows and the archived ones. Expanded or not is view state only: no command, no write. Every row of an archived object carries `archived_at` from the read model (§17).
- **Right-click menu** (**every item is a `cpd` command**: the tree only runs commands). A right click on an Inbox item row opens the menu with its four entries (table below); a right click elsewhere opens the menu of its row. A right click opens a menu, drawn by the view (ratatui) over the tree at the click and clamped inside the pane. ↑↓ and Enter pick an item, Esc closes it; a click picks an item; a click outside closes it. There is **no key** that opens the menu: `n`, `x` and `?` stay as above. Text is typed in the inline prompt row of the `n` key (`tui/src/tree/prompt.rs`); **Rename…** starts with the current text; **Close** asks `y/N` first, as `x` does; **Archive** runs at once, with no `y/N` (hs-d90). The items by row:

  | Row | Items | Command each runs |
  |---|---|---|
  | Inbox item | Snooze 15 min, Snooze 1 h, Snooze until tomorrow 09:00, Dismiss | `inbox snooze <id> 15m`, `inbox snooze <id> 1h`, `inbox snooze <id> tomorrow`, `inbox dismiss <id>` (§21.3, §21.4) |
  | Context | New Plan…, Rename…, Copy id, Archive | `plan new` (§4.1), `context rename` (§3.7), `context archive` (§3.8) |
  | Plan | New session…, Rename…, Copy id, Archive | `session fork <the Plan's Lead>` (§10.3), `plan rename` (§4.9), `plan archive` (§4.10) |
  | Fork (side Session) | Open, Rename…, Fork…, Copy id, Close, Archive | `session open` (§10.4), `session rename` (§10.7, `--session <id>`), `session fork` (§10.3), `session close` (§10.9), `session archive` (§10.10) |
  | empty tree area | New Context… | `context new` (§3.1) |
  | any archived row | Rename…, Copy id, Unarchive | `context rename` (§3.7), `plan rename` (§4.9) or `session rename` (§10.7), `Copy id` (no cpd command; below), and `context unarchive` (§3.9), `plan unarchive` (§4.11) or `session unarchive` (§10.11); the rename commands accept archived objects (F4) |

  **Copy id** (hs-d102) is offered on every row that has an id: Context, Plan, fork and archived rows. It copies the row's shown id (see Labels) to the User's clipboard with `tmux -L <socket> set-buffer -w -- <id>`; tmux sends it to the outer terminal by OSC 52, which works over ssh when the terminal allows it (tmux's default `terminal-features` give `xterm*` the `clipboard` feature). Whether it reaches the User's Ghostty on the Mac is **unverified** until the User tries it. The prompt row then says `copied <id>`. It is the one item that runs no `cpd` command: it changes no Ledger state.

  **Opening the new Lead** (hs-d103). After **New Plan…**, **Fork…** or **New session…** succeeds, the tree runs `session open <the new Session id>` (from the command's JSON: `plan new`'s `lead.session`, `session fork`'s Session id), then focuses the chat slot, as Open does. The commands themselves never move the chat slot.

  **Archive of the chat slot's Lead** (F3, hs-d90). Archive on a Session row (a Plan row's Lead, or a fork) whose Lead holds the chat slot cannot go straight to `session archive`, which refuses with `in_chat_slot` (§10.10, unchanged). The tree first frees the slot, then archives: it opens the next live Session in tree order (`session open`, then focuses the chat); if none follows, the previous one; if there is none at all, it runs `session park`, so the placeholder holds the slot. Then it runs the archive command. Only the tree does this two-step; the CLI keeps its refusal.

  A refusal (`archive_blocked`, `in_chat_slot`, `archived`, …) is shown in the prompt row as the command's own message.
- **New Plan… and New Context…** ask only a title; the tree derives the id. The id is a lowercase ASCII slug of the title: each run of other characters becomes `-`, leading non-letters are dropped, and it is cut to fit the id pattern (Plan `^[a-z][a-z0-9-]{1,23}$`, Context: contract §2.1); `-2`, `-3`, … are appended while the id is taken in the tree's snapshot. New Plan… runs `plan new <id> --context=<c> --title=<title>`: no repo, a Plan has none (R34, A12; its PRs are added with `pr add`). New Context… runs `context new <id> --title=<title>`.
- **Right button.** The view relies on tmux 3.4's default root `MouseDown3Pane` binding: believed to pass the click to a pane whose app captures the mouse (`mouse_any_flag`), and to show tmux's own menu otherwise (**unverified**). cpd adds no binding for it (§6.1, §13.1 unchanged).

**`plan`.**
- **No top row, no tabs** (User, hs-d102). The PR boxes start at the top of the pane, inside the margin below; the view draws no Context id, no tab and no separator line. Switching Leads is the tree's (Enter or a click). With no Lead in the chat slot, the message row reads ` no Lead session in the centre` (dim). With a Manager in the chat slot, the pane shows Pulse (one row per Plan of the Context), not the Plan view. With the placeholder or a history pane in the slot, the message shows.
- **Pending Plan review** (User, hs-d187, hs-d191). While the Plan's latest Plan review is `pending`, the view draws one row above the boxes: ` Plan review r<n> pending 󰏌`, with `pending` in `palette::fg(Meaning::Review)`. The whole row text, from `Plan` to `󰏌` (not the leading blank), is one link to `http://<host>:<port>` (hs-d219, B2), where `<port>` is the one line of `run/<plan>/plannotator-port` (`CpdHome::plannotator_port(&plan)`) and `<host>` is the review host (contract §6.5 "The review host": `PLANNOTATOR_URL_HOST` from cpd's environment, else the detected primary IPv4, else `localhost`; the same function that sets `PLANNOTATOR_URL_HOST` for the Lead): the URL plannotator prints (its `plan-review` answer carries no URL, §15.5). If that file is missing or not a port number, the row shows without the `󰏌` and is not linked. With no pending review there is no row; the boxes move down one row only while the row is shown.
- **Chart:** the Plan's Jobs as cards (reference `flow.rs`, ported), inside a blank margin of 2 columns and 1 row that belongs to the chart's canvas, not to the scroll window (which is the whole body, so scrolled content reaches the pane's edges; at scroll (0, 0) nothing touches them); cancelled Jobs hidden. Each Job is a pill: ` <icon> <key> ` left-aligned and `│<tier>[│a<n>] ` right-aligned (`a<n>` only from the second Attempt on), the `│` (U+2502) in the pill's style (User, Plan tui-visuals). A `running` Job's icon spins `󰝦󰪞󰪟󰪠󰪡󰪢󰪣󰪤󰪥` (U+F0766, then U+F0A9E … U+F0AA5) on the tree's clock (one frame every 100 ms, `frame = (unix ms / 100) % 9`); every other status, `merging` `󰮕` included, stays static. Messages are one row at the bottom of the pane: `no Plan yet`, `loading… (first fetch)`, or the note.
- **PR boxes** (R34, A15; contract §2.5). The chart is split into **one box per PR** that is not `cancelled`, each a bordered block (`Borders::ALL`, colour 8) holding the cards and Dependency arrows of that PR's Jobs, laid out inside it as the single chart was. The box's title, on its top border, reads ` <key> · <repo name> 󱦰 <base> · <pull request> `: the PR key in bold; the repo's directory name (the last component of `prs.repo`); its base; then `#<number> <state>` once the PR has a pull request, where `<state>` is `open`, `merged`, or `closed` while the pull request is closed without a merge (`pull_request.state` is `closed`, A11), in the palette's PR state colours (reference `pr_state`); `<state>` reads `conflict`, in the failed colour (red), instead of `open` when the pull request is `open` and its `mergeable` is `conflicting` (`unknown` is not a conflict; `merged` and `closed` never read `conflict`); else the PR's status (`waiting`, `dispatch`) dim. Once the PR has a pull request, ` 󰏌` follows `#<number> <state>`, and the whole `#<number> <state> 󰏌` is one link to `pull_requests.url` (Links, below; hs-d187, hs-d219, B1); the PR key, the repo, the base and the spaces around the link are not linked. The boxes are laid out **top to bottom** in the order of the needs graph, one row per level: a box sits below every box it needs, at a level one more than the highest level among its needs; boxes of the same level sit side by side in that row, by key (hs-d115). Each need is an arrow from the needed box's bottom border to the needing box's top border: a `┬` on the bottom border, a line with `╭╮` corners, and a `󱦳` (U+F19B3 md-arrow_down_thin) into the top border. Inside a box the Jobs still flow left to right, and a box is as wide as its own chart and title. The view also re-reads its snapshot at least every 2 s, with the Event-driven coalescing as before: a conflict clears with no Event (`mergeable` changes silently; only becoming `conflicting` appends `pr-feedback`), so without the timer a box would keep `conflict`.

  ```
  ┌ schema · api 󱦰 main · #31 open ──┐
  │  󰗠 types │S1 ── 󰗠 gql │S2       │
  └────────────────┬─────────────────┘
            ╭──────┴──────╮
            󱦳             󱦳
  ┌ web · web 󱦰 main ──┐ ┌ docs · api 󱦰 main ┐
  │  󰪞 page │S1       │ │  󰝦 readme │H2    │
  └───────────────────┘ └──────────────────┘
  ```

  A PR with no Jobs shows an empty box (` no Jobs `, dim). A Plan with one PR shows one box; a Plan with no PR yet shows the message `no PR yet: cpd pr add` (dim). Selection and Enter work on Job cards across boxes: ←→ move between cards within a row of boxes, crossing into the neighbouring box, and ↑↓ also cross between rows of boxes; the wheel and the scroll offsets apply to the whole canvas.

- **Pulse** (User, hs-d229, hs-d232): with a Manager in the chat slot, the pane shows Pulse instead of the Plan view: one row per Plan of the Manager's Context, with a sparkline of the Plan's activity, so a stalled Plan stands out. Reads only what the store holds; no write.
  - **When.** The newest `session-moved` Event with `data.place = 'chat'` (over all Sessions) belongs to a Session with role `manager`. With a Lead or a side fork in the slot the Plan view is unchanged; so it is with the placeholder or a history pane (message ` no Lead session in the centre`). The pane's tmux header stays ` Plan `.
  - **Rows.** The Plans of that Context that are not archived and either not done, or done (`plans.done_at`) inside the window (the n buckets shown, see Buckets). In tree order (`plans.created_at`).
  - **Row layout,** inside the Plan view's margin (MARGIN: 2 columns left/right, 1 row top): `<dot> <plan id padded to the longest id>  <sparkline>  <󰗠N right-aligned in 4>  <age right-aligned in 9>`. The dot is the Plan Lead Session's tree dot (`read::Dot`, drawn with `palette::dot_span(dot, frame, false)` and `palette::frame`; no hard-coded glyphs). The id is plain text. The row is 21 + id_width + n columns.
  - **Buckets.** 5 minutes each, aligned to wall-clock multiples of 5 min since the Unix epoch; the last one is the current, partial bucket. The window is n buckets ending now, n = max(12, pane width - 2*2 (margin) - (21 + id_width)). If n < 12, 12 is used and the line is cut at the pane's edge.
  - **Activity** (what a bucket counts). The Events of the Plan (`events.plan`) of kinds plan-phase, plan-review, plan-done, job-added, job-edited, job-merged, job-failed, job-retried, job-cancel-requested, job-cancelled, attempt-started, attempt-status, attempt-retry, attempt-nudge, all-merged, pr-feedback, pr-added, pr-edited, pr-cancelled, pr-started, pr-opened, pr-merged, escalation, escalation-answered, escalation-expired, error; plus Lead turns: `session-state` Events with `data.to = 'working'` whose Session has role `lead` (the Plan's Lead and its side forks). Not counted: session-moved, session-registered, session-renamed, session-archived, session-message, inbox-*, context-*, plan-created, plan-renamed, plan-archived, watch-*.
  - **Glyphs.** A bucket with a pr-merged Event: a green `󰗠` (`Meaning::Done`). Else with activity: a bar `▁▂▃▄▅▆▇█` of level ceil(count * 8 / max) (1..8), where max is the largest bucket count over ALL rows (one scale, so rows compare); green when the bucket holds a job-merged Event, else the default foreground. Else, when an Attempt of the Plan was running during the bucket (`attempts.started_at .. ended_at`, or until now while not ended): a dim `▁` (`Meaning::Nothing`). Else blank.
  - **󰗠N.** The count of job-merged plus pr-merged Events in the window, green (`Meaning::Done`); blank when 0.
  - **Age.** Minutes since the Plan's latest activity Event (any time, not only the window); `0m` while one of its Attempts is not ended (status starting, running or merging). Under 60 min `Nm`, under 48 h `Nh`, else `Nd`. Dim. Blank when the Plan has no activity Event at all.
  - **Quiet.** A Plan is quiet when all four hold: it has queued or running Jobs (drafts do not count); no Attempt of it is not ended; its latest activity is 30 min old or more (no activity at all also counts); its Lead's dot is not a needs-you dot (`Dot::needs_you()` false). The age cell then reads `quiet <age>` (e.g. `quiet 41m`) in `palette::fg(Meaning::Review)`. The 30 minutes are a constant, not a config key.
  - **Clicks and wheel.** A left click on a row runs the Plan view's `Actions::open_lead(<the Plan's lead_session>)` (session open, then focus the chat slot); a failure shows in the message row like the view's other notes. The wheel scrolls the rows (3 rows a notch, clamped). No keys, no selection.
  - **Re-read.** A new Pulse snapshot on any Event of the Manager's Context (`events.context`) and on `session-moved`, coalesced to at most one per 500 ms (COALESCE); also every 60 s (the window moves and ages tick; a running Attempt is not an Event), and on pane resize (the bucket count changes). Between reads only the dot spinner redraws, on the view's frame timer.
  - **Empty.** A Manager whose Context has no row shows ` no Plans yet` (dim) in the message row.

    Example, a 60-column pane, id_width 10, so n = 60 - 4 - 31 = 25 buckets:

    ```
        󰝦 cross-repo  █▃▁▁▄󰗠▁▁▁󰗠▁▁▂▁▂▁▁▄▁▁        󰗠17         4m
        󰪥 manager     ▁▁▁▁▁▃  󰗠  ▅ █▄  ▃▁▇▅▃▁▂     󰗠6         3m
        󰝦 quality     ▁▄▃  ▁ 󰗠 ▁▄▄  󰗠▁▇█▁          󰗠6  quiet 41m
    ```

**`widgets`** (reference `dispatch`), top to bottom: one blank row at the top, the first section without a header, and the section headers (design rule above), each with one blank row above and one below, in place of the shared separators and the bold section titles, as `Layout::vertical([Length(1), Length(2), Length(1), header, Length(1), Min(1), Length(1), header, Length(1), Length(1), Length(1), header, Length(1), Length(1)])`: blank row, machine, then each section header (`──[ PRs ]──`, `──[ git ]──`, `──[ Jira ]──`) with one blank row above it and one below it, then PRs, git, Jira. The title of the pane is ` Deploy ` (contract §6.1).
1. the machine (no header): its hostname in bold (` <hostname>`) and ` load a b c`, the first three fields of `/proc/loadavg` (on macOS `sysctl -n vm.loadavg`), re-read every 2 seconds;
2. header `PRs`, then **one flat list of every `open` row of `pull_requests` with `authored` 1** (hs-d96, D11; contract §2.2 "Pull request"; review requests are Inbox items, §21, not the User's pull requests, so rows with `authored` 0 are not listed), oldest `created_at` first, two lines each: the first ` <owner>/<repo>#<n>` in bold (`repo` and `number`), the second `  <open|draft> <checks icon> <review>  <title>`: `draft` when the row's `draft` is set, else `open`; the checks icon in the palette's `checks` colours (reference `checks`): `󰗠` passed (U+F05E0 md-check_circle), `󰅙` failed (U+F0159 md-close_circle), `󰦖` pending (U+F0996 md-progress_clock), `󰍴` none (U+F0374 md-minus); the review `approved`, `changes requested` or `no review`; then, when `pull_requests.mergeable = 'conflicting'`, ` conflict` in `palette::fg(Meaning::Failed)` (red; hs-d219, B3), for example `  open 󰗠 approved conflict  <title>` (`'mergeable'` and `'unknown'` show nothing); the title dim and cut first when the column is narrow. The pull requests of the PRs of the Plan whose Lead is in the chat slot (`prs.pull_request`) are drawn reversed and bold on both lines. The ` conflict` flag keeps its red under the reversed-bold and the dim styles (hs-d219). Plans without a PR are not listed. With no open row: ` no open PRs` (dim);
3. header `git`, then ` (future)` (dim);
4. header `Jira`, then ` (future)` (dim).

Each PR row has two areas. The **`󰏌`** sits at the right end of the first line and links to `pull_requests.url` (Links, below). The **row** is the rest of both lines. The PR's **owner** is the Plan of the `prs` row whose `pull_request` is this pull request (the most recently created Plan when several exist); its Lead is `plans.lead_session`. On an owned PR, a left click anywhere on the two lines except the `󰏌` cell runs `cpd session open <lead>`, then focuses the chat slot, as the Plan view's open-Lead action does. A PR no Plan owns (for example one of the User's work repos) is drawn dim on both lines, and a click on it does nothing. A click on the `󰏌` does nothing in the view: the terminal opens the link. When `session open` refuses or fails, the column's last row shows its message in `palette::note()` until the next click.

**Scrolling the PRs section** (User, hs-n259). The section is as tall as the pane leaves it, and a pull request is drawn whole (two lines) or not at all. A mouse wheel event over the section scrolls it by one pull request (two lines) per notch, down or up; wheel events elsewhere in the column do nothing. The offset counts pull requests (the first one drawn) and is clamped between 0 and the largest offset at which the last pull request still reaches the section's bottom, so scrolling stops there. When pull requests are hidden above, the section's first row is ` ▲ <n> more` (dim); when some are hidden below, its last row is ` ▼ <n> more` (dim); `<n>` counts pull requests, not lines. A cue row replaces one row of the list, so a pull request that does not fit beside the cues is hidden and counted. With every pull request visible there is no cue. A left click works on the rows as drawn: the hit areas are those of the pull requests on screen, and a cue row is no hit. The offset lives in the view's memory only: it starts at 0, a restart of the view resets it, a refresh keeps it (clamped when the list shrinks), and nothing is written to disk.

The column has no engine line, no slots, no Escalations, no Events and no footer hint. The Event log is the `logs` view (R36, below).

**Links** (User, hs-d187, hs-d191). The cockpit shows OSC 8 links: to each PR's pull request (in a Plan view box title, the whole `#<number> <state> 󰏌`; on a Deploy column row, the `󰏌`) and to a pending Plan review's plannotator page (the whole row text; hs-d219). `tui/src/link.rs` writes a link over cells a view has already drawn, in pieces of at most 2 cells, each piece's cell symbol wrapped in `ESC ] 8 ; ; <url> BEL` … `ESC ] 8 ; ; BEL`; this is ratatui's hyperlink-example technique, a workaround for ratatui issue #902, and the visible text and its columns do not change. The terminal opens a link with its modifier-click (Cmd+click in Ghostty). There is no copy fallback. tmux forwards the links because contract §6.1 sets `terminal-features` `xterm-ghostty:hyperlinks`. Verified by the User on 2026-10-18: in a pane of `cpd:main` with mouse capture on, Cmd+click in Ghostty, through tmux and ssh, opened a whole link, a link written as per-2-cell pieces, a one-cell `↗` link (the glyph then; now `󰏌`) and `http://localhost:19438`.

**`statusbar`** (R36, User decision) is one row across the full width at the bottom of `cpd:main`; it has no keys, takes no keyboard focus (a click on it is passed on with `send-keys -M`, no `select-pane`, §6.3) and draws no border title. **Its left side holds action buttons only; its right side holds read-only information.** The middle expands.
- **Left, in order:** three one-glyph buttons, `⧉` (reset layout), `?` (legend) and `▤` (logs), one space apart. Each is drawn like a section header's title block: one cell wide, grey (the palette's Nothing colour, colour 8) with `Modifier::REVERSED`, so the glyph shows in the background colour. A click on `⧉` runs reset-layout (below); on `?` opens the legend popup; on `▤` opens the logs popup. Tooltips are not built (`BACKLOG.md`).
- **Right, in order:** a failure note when the last click failed (cleared on the next click), then the Inbox counter `inbox <n>`, where `<n>` is the number of visible open items (the count of the tree's Inbox header, §17), drawn in the colour of the most urgent of them (red, yellow, magenta or white; dim `inbox 0` when there is none), then the workers indicator `workers N/M`, where `N` is the number of Attempts holding a slot and `M` is config `concurrency`. The workers indicator gets the full width, as cpd 1's; it shortens to `N/M` when the row is too narrow and is dropped when not even that fits; the Inbox counter is dropped before it. A click on the counter does nothing.

**`legend`** is what every colour and icon of the cockpit means, from the palette. It opens in a tmux popup (`display-popup -E -T ' legend ' -w <w> -h <h>`, sized to the legend and at most the client's size) from the tree's `?` key or the StatusBar's legend button, because the columns are narrower than the legend; any key closes it. It has no `@cpd_view` pane of its own. Its section titles are `Jobs: Plan pills and Context counters`, `Session dots` and `Pulse`; the Job statuses it lists are those the chart draws, so `cancelled` is not among them, and `running` is listed with the spinner frames `󰝦󰪞󰪟󰪠󰪡󰪢󰪣󰪤󰪥`. `Session dots` lists each glyph (`󰪥`, `󰝦󰪞󰪟󰪠󰪡󰪢󰪣󰪤󰪥`, `󰝦`, `󰝦`) and each `󰪥` colour (red, yellow, magenta, white) with its meaning, as §17's dot rule gives them (the same colours mark the Inbox's items and the StatusBar's `inbox <n>`; the legend says so in one line), then the precedence line `red > yellow > magenta > white > 󰝦 > 󰝦 > 󰝦`, drawn with the real glyphs and colours. Its PR states list is `open`, `merged`, `closed` and `conflict` (in the failed colour, red), as the plan view's PR box titles show them. Its sample pill has the same form, ` <icon> key │S1 `. Pulse section: a bar = activity per 5 min; a green bar = a Job merged; a green 󰗠 = a PR merged; a dim ▁ = a Worker running; 'quiet Nm' (Review colour) = queued or running work with no activity for 30 min.

**`logs`** (R36) is the **Event log**: every Event (§5), one line each: its time of day (`HH:MM:SS`, dim), then its `text`, or its kind when the `text` is empty, an `error` Event in the failed colour. It opens in a tmux popup (`display-popup -E -T ' logs '`, at most 80 % of the client's width and height) from the StatusBar's `▤` button and from a click on an `engine-error` or `engine-down` Inbox item, and has no `@cpd_view` pane of its own. With `--event <id>` (an Event id of §5; the Inbox click passes the item's `event`) the view opens **at that Event**: its line is selected (reversed) and scrolled into view, and it does not follow until the User scrolls to the bottom; an id outside the latest 500 Events is read on its own too. Without it, it opens at the bottom. The newest Event is last; the view follows live while it shows the bottom, and stops following when the User scrolls up (it follows again at the bottom). ↑↓, j/k and PgUp/PgDn scroll; `q` or Esc closes it.

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
| `tiers` | error | each `tiers.*` model is in pi's catalog, and `lead_tier` is a valid Tier (§9) |
| `pi_adapter` | error | the `pi_adapter` directory has a `package.json` |
| `lead_packages` | warn | reports, for each `lead_pi_packages` entry and the adapter, whether a Lead gets it by `-e` or from which settings file (§6.5); warns when an entry is neither declared nor resolvable |
| `watch` | warn | the engine runs when any Plan is in `dispatch` or `deploy` |

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

- **Package:** `pi/package.json` with `"keywords": ["pi-package"]` and `"pi": {"extensions": ["./extension.ts"], "skills": ["./skills"]}`; host packages are `peerDependencies: "*"` (cpd 1 §15, kept). The skill `cpd-lead` is the cpd 2 skill.
- **Location:** `pi/`, installed to `~/.local/share/cpd/pi` by `scripts/install-cpd.sh`. (History: before the switch-over it lived beside cpd 1's `pi/`, in a second directory.)
- **Activation:** at pi's `session_start`, only when `ctx.mode` is `tui` and `CPD_SESSION` is set. Change from cpd 1: cpd 1 activated in any herdr or tmux pane (`HERDR_ENV`, `TMUX_PANE`); now cpd marks its own panes with `CPD_SESSION` (§6.5), so a pi the User starts by hand is left alone.
- **Identity:** `CPD_SESSION` must equal `ctx.sessionManager.getSessionId()`. If not, the adapter delivers one error message (below) and stays inactive: it would otherwise report another Session's state.
- **Calls:** one-shot commands through `pi.exec("cpd", args)`, whose stdout is parsed as the JSON envelope at the boundary (a non-JSON stdout, or `ok: false`, is an error with the code). Each call passes a timeout of 30 s and the abort signal of the adapter's run (aborted at `session_shutdown`); a call that times out or is aborted is reported like a failed call (the error delivery below, code `tool_timeout` for a timeout). The follower is a child process (`node:child_process` `spawn`, argv, stdin `/dev/null`) read line by line.
- **Errors** are delivered to the Session as `pi.sendMessage({customType: "cpd", content: "cpd adapter: <what>: <code>: <message>", display: true, details: {kind: "error", code, …}}, {triggerTurn: true, deliverAs: "followUp"})` (cpd 1's `errorDelivery`, kept). The adapter never throws into pi.

### 15.2 Start-up and the cursor

At `session_start`, in this order:
0. **Protocol check** (hs-a4): `cpd --version`, whose `protocol` (§1.2) is compared with the number the adapter was built for (4 today). Equal: nothing is said. Different: one message to the Session, `cpd adapter: protocol mismatch: protocol_mismatch: cpd speaks protocol N, this adapter M; reload pi (/reload)`, with `details: {kind: "error", code: "protocol_mismatch", cpd: N, adapter: M}`, delivered like the other errors (§15.1), once per installed cpd binary (the identity of the file `cpd` resolves to), not once per follower restart. The adapter keeps following Events either way; it never types into a pane. A `--version` that fails or lacks `protocol` is an ordinary error with its code, once per error code.
1. `cpd session show <id>`: role, Plan, Context, `plan_lead`, `registered_event`, name.
2. `cpd session state idle --session <id>` (`stopped → idle`, §3.6); then `working` if `ctx.isIdle()` is false.
3. **Name:** if pi's session name differs from the Session's `name`, `pi.setSessionName(name)`: a `session-renamed` missed while pi was down still applies.
4. **Cursor** (§5.3 "No loss, no duplicates"): the greater of `registered_event` and the `details.id` of the last `cpd` custom message in the session's entries whose `details.delivered_to` is this Session's id. The adapter puts `delivered_to` into every message it delivers (amendment 5, §19). Why: a fork's file starts with its source's messages, whose Event ids belong to the source's stream; counting them could skip Events addressed to the fork.
5. **Blocking reasons** (Plan Lead only; a Manager has none): one per `open` Escalation of its Plan (`cpd plan show`), and one for the Plan's **latest** Plan review when it is pending and this session opened it (its `cpd-plan-review` entries, §15.5): the latest is the one `cpd plan show` reports (`review.id`), and an undecided older review never opens a reason (hs-d224).
6. **Follow:** spawn `cpd events follow --session <id> --since <cursor>`.

The cursor in memory advances after each line is handled, delivered or not. Only delivered messages persist it, so after a restart the Events handled without a message (`escalation-answered`, `escalation-expired`, `session-renamed`, `plan-review`) are seen again; every handler below is idempotent for that reason.

**The follower ends** (EOF, exit, or a failure line): the adapter restarts it with `--since <in-memory cursor>` after 1 s, then backing off to at most 30 s. A failure line or a non-zero exit is delivered as an error message once per error code, until a line is read again. A well-formed Event (§5.2 fields) of a kind the adapter does not know is skipped silently: no message, and the cursor moves past it (an adapter that does not know a kind cannot need it, so an install that adds a kind does not flood older adapters). A malformed line (not JSON, or §5.2 fields missing or of the wrong type) is delivered as an error message and skipped, and the cursor moves past it, so one bad line cannot stop delivery. Before it spawns the follower again, the adapter repeats the protocol check of step 0; the message is sent only when the installed binary is a different one from the last that mismatched. At `session_shutdown` the follower is killed.

### 15.3 What each Event does

Events arrive in id order; the follower already applied §11's `--session` rule, so a Lead receives its Plan's Events and a Worker or side fork only those addressed to it. Delivery is `pi.sendMessage({customType: "cpd", content: event.text, display: true, details: {...event, delivered_to: <id>}}, {triggerTurn, deliverAs: "followUp"})` (§5.3).

| Kind | Plan Lead | Worker, side fork |
|---|---|---|
| `job-merged`, `job-failed`, `all-merged`, `pr-feedback`, `plan-done`, `error` | message, **turn**. `all-merged` and `pr-feedback` carry the PR's key in `data.pr`; the Lead answers `all-merged` with `cpd pr open --pr <key>`, and an `error` with code `pr_start_failed` with `cpd pr start <key>` once the cause is fixed (R34, A9) | |
| `attempt-retry`, `job-cancelled` | message, no turn | |
| `pr-started`, `pr-merged` | message, no turn (R34: a started PR's Jobs are now scheduled; a merged PR may start the PRs that need it, §5.3) | |
| `escalation` | message, **turn**; opens the blocking reason `escalation:<id>` labelled `cpd: Worker question` | |
| `escalation-answered`, `escalation-expired` | closes `escalation:<id>`; no message | |
| `plan-review` with `status: pending` | opens it in plannotator (§15.5); no message. A review with a higher id than another pending review of this session **closes** that one's `plan-review:<review>` reason: only the latest review of a Plan blocks the Lead (§15.5 "Latest only") | |
| `plan-review` with `status: approved` or `rejected`, for a pending entry of this session | a decision made outside plannotator (§15.5 "Deciding elsewhere"): closes `plan-review:<review>`; message `cpd-plan-decision`, **turn** (own custom type, not `cpd`). For any other review: ignored | |
| `session-renamed` (`session` = itself) | `pi.setSessionName(name)` and `ctx.ui.notify("cpd: session renamed to \"<name>\"")`; no message, no turn | same |
| `attempt-nudge` (`session` = itself) | | message with `data.prompt` as content, **turn** |
| `session-message` (`session` = itself) | message, `deliverAs: "followUp"`, **turn only when `data.wake` is true** (`triggerTurn: false` otherwise, hs-d209 D3); for every Lead (Plan Lead, side fork) and the Manager (the `--session` rule delivers Events addressed to a Session to it). Content `Message from <sender>: <text>`, `<sender>` = the sender Session's name and id (`<name> (<id>)`), or `the User` when `data.from` is null. With `data.wake`, an idle Lead starts a turn at once; a working or blocked one gets it after its running turn, never interrupted. Without it, the message is in the Lead's context at its next turn and starts none | |
| `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived` | ignored: no message, no turn | ignored |
| every other kind | ignored | ignored |

**The Manager** (B3-A5) is a Session of role `manager`: it has no Plan, so §15.2 step 5 (blocking reasons) and §15.5 (plannotator, the tool `cpd_plan_review`) do not apply to it and nothing is opened or registered for them. Its follower follows only Events addressed to it (§11), so of this table only `session-message` and `session-renamed` reach it: a Plan's Events never wake it. It learns of them through the digest at turn start (§15.7), which only a Manager gets.

This is §5.3's table; the `plan-review` row is the Plan review protocol §5.3 leaves to this file (amendment 6, §19). A blocking reason opened twice or closed when not open changes nothing (cpd 1 `blocked.ts`, kept), which makes replays harmless.

### 15.4 Agent states: `session state`

The adapter keeps three facts and reports the state they give (the model of `tmux-agent-state.ts`, §3.6):

| Fact | Set by |
|---|---|
| `active` | true at pi's `agent_start`; false at `agent_settled` when `ctx.isIdle()` is true |
| its own blocking reasons | §15.3 (`escalation:<id>`) and §15.5 (`plan-review:<review>`, the Plan's latest review only), each with a label |
| blocking dialogs | pi's `ui_prompt_start` and `ui_prompt_end` (pi 1.2.0 `ExtensionAPI`, emitted around every blocking extension UI prompt: `select`, `confirm`, `input`, `editor`, `custom`; depth-counted, so nested prompts give one pair) open and close **one** blocking reason labelled `dialog`. The dialog reason covers `ask_user_question` (hs-n47) |

Reported state: `blocked` with the label of the most recently opened reason while any reason or signal is open; else `working` while `active`; else `idle`. On every change it runs `cpd session state <state> [--label <label>] --session <id>`. Calls are serialised: at most one in flight, and only the latest wanted state is sent next, so states never arrive out of order. A refused call (`illegal_transition`, for example after the engine marked the Session `stopped`) is reported as an error once and the next change is tried normally. At `session_shutdown`: `session state stopped`, waited for at most 2 s; `session run` reports it as well if pi exits without it (§10.8).

**Reply runs** (hs-d278; contract §2.2 `unread`, §3.6). The adapter also records whether the current run is a **reply run**, and reports it when the run's end moves the Session to `idle`: `cpd session state idle --reply --session <id>`; any other call omits `--reply`. A run is a reply run when it started at pi's `before_agent_start` with `event.messageType` `"user"` (a prompt the User submitted, or `pi.sendUserMessage()`), or with `"custom"` and the adapter's own message for a `session-message` Event whose `data.from` is null (`cpd session send` with no sender: "Message from the User"), or when pi's `input` event with `source: "interactive"` arrived during it (the User typed, also as a steer or follow-up). A run started by an engine Event, or by another Session's message, is not a reply run unless the User also typed during it. The flag is cleared when the run starts and read at the `idle` report; an older adapter sends no `--reply`, so its Sessions are never unread until it reloads (protocol 4, §1.2). by `ask` itself and back to `working` when it returns (§8.1); its adapter is mid-turn meanwhile and reports nothing.

**Names.** On pi's `session_info_changed`, when pi's name differs from the last name the adapter knows, it runs `cpd session rename <name> --session <id>`, so a `/name` typed in pi reaches the TUI. The `session-renamed` it causes comes back and changes nothing.

### 15.5 The Plan review through plannotator

Replaces cpd 1 §15's `cpd_plan_review` flow; the plannotator event API is the one cpd 1 uses (t4), parsed at the boundary because it is not a stable contract.

**Requesting.** The Lead runs `cpd plan review`, directly or through the tool `cpd_plan_review` (no parameters; registered for Plan Leads only). The tool runs `cpd plan review --plan <CPD_PLAN>` and returns its `next` text; when the result is `"already": true` and this session has no pending entry for that review, the tool opens it itself (below), so a review that failed to open can be retried. The tool's name is kept so §6.5's `--exclude-tools` list for Workers stays right.

**Opening**, on a `plan-review` Event with `status: pending` for its Plan, **whatever the Plan's phase** (a review of the drafts of a running Plan, §4.6, opens exactly like one in phase `plan`; the adapter does not read the phase and its list of Event kinds does not change), or from the tool:
1. Skip it when the session already has a `cpd-plan-review` entry for this review, or when it is not the Plan's latest review (`cpd plan show`, `review.id`): a replayed or superseded request.
2. Read `run/<plan>/plan-<review>.md` (§4.6).
3. `pi.events.emit("plannotator:request", {requestId, action: "plan-review", payload: {planContent, planFilePath}, respond})`, waiting at most 15 s for `respond`. A `handled` response with `status: pending` gives plannotator's own review id.
4. `pi.appendEntry("cpd-plan-review", {plan, review, plannotator_id, status: "pending"})`, so the mapping survives `/reload` and resume; open the blocking reason `plan-review:<review>` labelled `cpd: plan review`, and close the reasons of this session's older reviews ("Latest only").

If plannotator does not answer, answers `unavailable` or `error` (for example `Port 19432 in use`), the adapter delivers an error message with plannotator's error (turn) and writes nothing; the review stays `pending` in the database.

**Latest only** (hs-d224, bug hs-n158). The reason `plan-review:<review>` follows only the Plan's **latest** review, because nobody decides a superseded review (`plan lock` and the Plan inbox read only the latest, contract §3.1, §4.6): it exists while the review is `pending` **and** is the Plan's latest review. Review ids are `r<k>`, compared by `k`.
- On a `plan-review` Event with `status: pending`, every reason of a review with a lower `k` closes, before the new review is opened.
- At start-up (§15.2 step 5) only the review `cpd plan show` reports (`review.id`) can open a reason, when this session has a pending entry for it; the entries of older reviews open nothing. When `plan show` fails (an error message, §15.2), the highest `k` among this session's entries stands for it.
- The entries of superseded reviews stay `pending` and plannotator's pages of them may stay open; only the blocking reason changes. A reason closed twice, or one that is not open, changes nothing, so replays are harmless.

**Deciding**, on `plannotator:review-result` `{reviewId, approved, feedback?}` for a pending entry:
1. Write the feedback, if any, to `$CPD_HOME/run/<plan>/feedback-<review>.md` (overwritten on a retry; no temporary directory is left behind); run `cpd plan decide <review> --approve | --reject [--feedback-file <f>] --plan <plan>`. `review_decided` means another caller decided it: the adapter reads the decision with `cpd plan show` and continues with it.
2. Append the entry with `status` `approved` or `rejected`; close `plan-review:<review>`.
3. Tell the Lead with `pi.sendMessage({customType: "cpd-plan-decision", …}, {triggerTurn: true, deliverAs: "followUp"})` (cpd 1 `decisionText`, kept): `Plan review r3 was APPROVED.` plus the feedback, then `Next: cpd plan lock` in phase `plan`, or in a later phase that the drafts are queued and no lock is needed (the Event's `data.jobs` lists them), or for a rejection `Revise the plan (job edit, job add, job cancel), then call cpd_plan_review again.`; when `current` is false, it says the Jobs changed since that review and asks for a new one. The custom type differs from `cpd`, so this message never moves the cursor.

**Deciding elsewhere**, on a `plan-review` Event with `status` `approved` or `rejected` for a pending entry of this session: any caller other than plannotator decided the review (`cpd plan decide` from the CLI, the User or a script), and the database already holds the decision, so the adapter does **not** run `plan decide`. Otherwise the Lead would stay `blocked` and never be told. It does what "Deciding" does after its own `plan decide`:
1. Read `cpd plan show` for `current` (and for the feedback when the Event's `data.feedback` is not a string).
2. Append the entry with the status; close `plan-review:<review>`.
3. Tell the Lead with the same `cpd-plan-decision` message (`triggerTurn`, followUp), with the Event's `data.feedback`, plus a last line saying the review was decided outside plannotator and that its page stays open.
4. plannotator's page cannot be closed: its event API (`plannotator-events.ts`, 0.28.17) has no request for it (`plan-mode`, `plan-review`, `review-status`, `code-review`, `annotate`, `annotate-last`, `archive`); `closeKeepingDraft` in `server/serverPlan.ts` is only on the in-process server object, which the adapter cannot reach. The message says so.

A failure of `plan show` is an error message and leaves the entry pending. An Event for a review with no pending entry (replayed, another Plan's, already decided) changes nothing. A later `plannotator:review-result` (or recovery's `completed`) for an entry decided this way is ignored, because the entry is no longer pending; one that arrives while this path runs is ignored too, so the Lead is told once.

**Recovery** at `session_start` (t4 F5, kept): for each pending entry, `plannotator:request` `review-status` every second until it is not pending: `completed` is handled as a result; `missing` (plannotator restarted) appends the entry as `lost`, closes the reason, and tells the Lead to call `cpd_plan_review` again, which reopens it (above).

**Several Leads at once** (follow-up "Plan reviews collide across Leads"; hs-d35, hs-d40, hs-d50). plannotator in remote mode listens on one port, `PLANNOTATOR_PORT` or 19432, and a second Lead's review fails with `Port 19432 in use after 5 retries` while the first is open (docs: plannotator 0.28.17 `server/network.ts`; seen on 2026-10-14). So each Plan gets its own port from the config range:
- config `plannotator_ports` (contract §8.2), default `19432-19441`: up to 10 Plan reviews open at once; each Plan gets the lowest free port of the range and keeps it (`run/<plan>/plannotator-port`, contract §6.5); plannotator prints each review's URL in its Lead's chat;
- `session run` assigns the port when it starts pi and passes it as `PLANNOTATOR_PORT`; `PLANNOTATOR_REMOTE=1` and `PLANNOTATOR_URL_HOST` (the review host, contract §6.5) are set by cpd (§19 amendment 8). A range that is full refuses with `no_port` (§10.8);
- links name the review host (contract §6.5), so the Macs need no ssh forwards;
- `doctor` no longer checks the port (§14.1).

Rejected alternatives: one shared review server (cpd would have to run plannotator's server outside pi, which plannotator does not offer to extensions), and a queue (a review waiting for another Plan's User decision could wait for hours). Not verified: that two plannotator servers in one range do not disturb each other's sessions.

### 15.6 Reload after an install

Decision hs-d141 (research: `docs/research/pi-reload.md`), superseding its first answer; contract §5.3 "Reload after an install" and §6.7. After `scripts/install-cpd.sh` the adapter reloads its own pi (`ctx.reload()`: extensions, skills, prompts, themes, context files) without the User typing and without typing into a pane. Nothing reaches the model.

**Detection.** At `session_start` the adapter records the identity `(dev, ino, mtimeMs)` of its own directory (`dirname` of `pi/lib/`, from `import.meta.url`; contract §8 `pi_adapter`) with `statSync`. An `unref`'d timer polls it every 5 seconds (a constant of the adapter, not a config key) and is cleared at `session_shutdown`. The install replaces the directory as a whole (`rm -rf`, `mv`), so a new package has a new identity. `ENOENT` is the install in progress, not a change: the poll waits. A different identity of an existing directory is an install. The cpd binary is not watched (it changes first, before the directory is complete). The adapter runs only in `tui` mode (§15.1), so no other mode polls.

**The command.** The adapter registers `cpd-reload-after-install`. pi runs an extension's command with a full `ExtensionCommandContext` only through `pi.sendUserMessage("/cpd-reload-after-install", {expandPromptTemplates: true})` (pi 1.2.0 `agent-session.js` `prompt()`); without the flag the text goes to the model, and pi's own `/reload` cannot be reached this way. The handler checks the guard, then `await ctx.reload()`; nothing after it uses the old `ctx`. The new instance records the new identity at its `session_start`, which is why a successful reload is not repeated.

**The guard**, in this order, checked by the adapter before it triggers the command and again in the handler (the handler is authoritative; only it has `ctx`, and a User typing the command by hand is harmless):
1. `ctx.isIdle()` (mid-turn pi's `ctx.reload()` silently does nothing yet resolves, so it cannot be left to pi);
2. `ctx.hasPendingMessages()` is false;
3. the adapter's own state (§15.4) is `idle`: not `working`, and not `blocked`, so no Worker question, Plan review or `herdr:blocked` signal is open (pi's `isIdle()` does not see dialogs, and a reload closes an open dialog and loses its answer);
4. `ctx.ui.getEditorText() === ""` (no unsent typing).

**Retries.** A refused reload (the guard fails at the trigger or in the handler) does nothing and is **not** handled: the recorded identity stays the old one, so the next tick checks the guard again and triggers again; there is no limit and no backoff beyond the poll interval. At most one trigger is in flight. **At settle:** a Lead that gets a message every few seconds is idle for gaps shorter than the 5 s interval, so no tick may ever see it idle (`docs/research/pi-reload.md` "Why a busy Lead did not reload"). So the adapter also decides at pi's `agent_settled` (pi has no automatic work left), after its own state is updated: for a package that **already passed pre-validation** and is still the directory's identity it runs the same guard and, when it passes, the same trigger. It adds no guard, does not shorten the stability or validation steps (a directory not yet validated waits for its ticks), and keeps one trigger in flight.

**Logged.** Nothing is delivered to the Session and no `cpd` command is run for it. The adapter notifies with `ctx.ui.notify` when it reloads (`cpd: adapter updated, reloading`); a reload that fails (pi's "Reload failed: …") is pi's own message. `ctx.ui.notify` is not written to the session file, so the notice is not evidence afterwards: the log below is.

**Log** (hs-d141, finding of `docs/research/pi-reload.md` "Why a busy Lead did not reload"). Every reload decision is one line appended to `$CPD_HOME/run/adapter/<session id>.log` (`CPD_HOME` default `~/.cpd`, `CPD_SESSION`; created with its directory at the first line; one file per Session, never rotated, small because a waiting or refusing line is not repeated while it is the last one). A line is `<ISO-8601 UTC time> <text>`; the texts, in the order they occur:
- `start: <reason>`: the adapter started (`session_start`; `reload` after a reload, `startup` or `new`, `resume`, `fork` otherwise); `poll: watching <dir> (<identity>) every <ms> ms`: the poll began, with the identity it will compare with;
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

Batch B2 (hs-d79, hs-d132; narrowed by hs-d209, D1). A Manager sees what other Sessions wrote to its Context, and what its Plans did, without polling.

**Leads get none** (hs-d209, D1, D2; this supersedes hs-d79 for Leads). Nothing from the Context is pushed into a Lead's turn: a Session of role `lead` (a Plan Lead, a side fork) gets no `cpd-context` message. A Lead reads the Context on demand, `cpd context show --since <seq>`, at the points the cpd-lead skill names: grounding; before drafting each batch; before answering an Escalation from the records; before `pr open`.

**When.** At pi's `before_agent_start`, for every run of a Session whose role is `manager`; never a Lead, never a Worker. It never starts or interrupts a turn: it only adds a message to the run that is starting.

**What.** The adapter returns one message, `customType: "cpd-context"`, `display: true`, listing the Context records written **by other Sessions** (`by.session` ≠ this Session) since its cursor, oldest first, each in full, except a `brief`, which is shown short: its id and first line, with its Plan (§3.3; the whole text is `cpd context show --record <id>`). Nothing is returned when there are none. Format:

```
Context building-cpd: 2 new records since your last turn.
- hs-d41 decision (2026-10-17): Use one table for sessions, supersedes hs-d12.
- hs-q9 answer (2026-10-17): Yes, keep it. Answers hs-q8.
```

Header `Context <c>: <N> new records since your last turn.`; each line `- <id> <kind> (<date>): <text>`; an answer names its question, a superseding decision names what it supersedes. At most 20 records are listed; more end with `and <K> more: cpd context show --context <c> --since <seq>`, with `<seq>` the `seq` before the first record not listed.

**Display** (hs-d221). The message's `content` is the full text and goes to the model whole. In the chat window the adapter shows one line of counts instead, never the full text, unless the User expands the message (pi's expand toggle), which then shows the full `content`. The adapter registers a pi message renderer for `cpd-context` once at load (`pi.registerMessageRenderer`; it only changes the display). The line, dim, is `cpd-context: ` and then the parts below in this order, joined by ` · `; a part whose count is 0 is omitted and nothing else is shown:

```
cpd-context: 6 records · 17 Plan Events · 10 Plans pending
cpd-context: 1 record · 1 Plan Event · 1 Plan pending
```

`<n> record(s)` is the number of new records of the message (the length of `details.records`), `<n> Plan Event(s)` the number of Events of the `Plans:` section (all of them, listed or not), `<n> Plan(s) pending` the number of Plans with a line in `Pending work:`. The counts come from the message's `details`, which carries `plan_events` and `plans_pending` besides `records`. A message whose `details` lack either (one written before this) is shown in full.

**Cursor**, in `context-record` Event ids (not the Event cursor of §15.2):
1. Start = the Session's `registered_event` (its first message already held the records up to then).
2. Then the greatest `details.event` of the session file's `cpd-context` messages whose `details.delivered_to` is this Session: the fork-safe rule of §15.2 step 4, for the same reason.

**Reading.** `cpd events list --context <c> --kind context-record --since <cursor>` gives the new `context-record` Events (each `data.seq`); then `cpd context show --context <c> --since <smallest seq - 1>` gives the records with `by`. The adapter skips its own records (`by.session` = this Session). The message's `details` is `{event: <highest context-record Event id read>, delivered_to: <id>, records: [ids], plan_events: <n>, plans_pending: <n>}`; `event` counts own records too, so they are not read again. If every record read is its own, no message is added and the cursor stays (the next read passes them again and skips them again). No schema change.

**Failure.** A failed read adds no message and leaves the cursor, so the next run reads again. It is reported once as an error message (§15.1, no turn: `triggerTurn: false`), and again only after a read has succeeded.

**The Manager's digest** (B3-A5, hs-d86). For a Session of role `manager` the message adds, after the records, a section:

```
Plans:
- 2026-10-18T09:12:00Z pay-4521: Plan phase dispatch.
- 2026-10-18T09:40:00Z pay-4522: Job api-1 failed.
```

One line per Event of kinds `plan-created`, `plan-phase`, `pr-opened`, `pr-merged`, `job-failed`, `escalation` and `plan-done` for the Manager's Context (`events list --context <c> --since <cursor>`, filtered by kind), oldest first, `- <at> <plan>: <event text>`. At most 20 lines; more end with `and <K> more: cpd events list --context <c> --since <id>`, `<id>` the id of the last Event listed. **One cursor** serves both sections, in Event ids: the records are read as above, the section from the same `--since`, and `details.event` is the highest Event id of either read (`context-record` or a listed kind). The message is returned when either section has something and carries nothing else when neither has. A failed read of the section fails the whole message as in "Failure". The Manager's follower is not involved (§15.3): the digest is the only way a Plan's Events reach it, and it never starts a turn.

**Pending work** (hs-d159, P2). After `Plans:`, the Manager's message adds a section, computed fresh at every Manager turn start from `cpd context pending --context <c>` (§3.11), not from the cursor:

```
Pending work:
- pay-4521 (dispatch), PR api dispatch #212: 1 merged on its Lead Branch, 1 queued, 1 running, 0 failed, 0 draft
  - retry-core merged: Retry policy
  - retry-cli queued: Retry flag
  - retry-ui running: Retry button
```

Per active Plan (not `done`, not archived), one line `- <plan> (<phase>), PR <key> <status>[ #<n>]: <m> merged on its Lead Branch, <q> queued, <r> running, <f> failed, <d> draft` (one line per PR that is not `merged` and not `cancelled`; ` #<n>` only once the pull request exists), each followed by up to 10 lines `  - <job> <status>: <title>` for that PR's Jobs in `seq` order, then `  and <K> more` when there are more. Touched files are not listed: the Manager runs `cpd context pending` for them. The section is omitted when no Plan is active. Because it is not cursor-based it alone does not make the message appear when `Plans:` and the records have nothing new: the message is returned when either of them has something, or when `Pending work:` is not empty; `details.event` is unchanged by it. A failed read of the section fails the message as in "Failure".

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

## 16. The Worker contract

### 16.1 The Worker Spec

Rendered by the driver at A3 to `run/<plan>/specs/<attempt>.md` from a template compiled into the binary, so nothing in a repo or worktree can change what Workers are told. Content, kept from cpd 1 `templates/worker.md` (cpd 1 §16) with the changes marked:

| Section | Content |
|---|---|
| Title | `# Worker Spec: <job>, Attempt <n>` |
| Who you are | Attempt `<n>` of Job `<job>` (`<title>`) in Plan `<plan>` (was "Workflow"); attempt id, Tier, worktree path |
| Rules | work only in this worktree and touch only the listed files; commit your work; before submitting, run the quick check yourself: the commands of `pre-commit` from the worktree's `.config/wt.toml`, listed verbatim, or, when the file has no `pre-commit` table, the line `This repo defines no quick check.` (change: cpd 1 said `./verify.sh`; R20: no fallback to `pre-merge`); never edit the protected paths, listed: `.config/wt.toml`, `.config/cpd.toml`, `.cpd/`, and each entry of the repo's protected list (§7.5; change: cpd 1 hard-coded `verify.sh`); never ask the User anything; tear down everything it started before it submits (private tmux servers, background processes, temporary homes, a `cpd watch` it started; hs-d34, hs-d39); if continuing would require guessing, `cpd ask <attempt> "<question>"`, which blocks until the Lead answers (change: no `--wf`, the pane has `CPD_PLAN`); one blocker at a time; commit a skeleton within 10 minutes, then at least every 20 minutes; run only commands that exit: `timeout` around every test and measurement, no synthetic benchmarks, never `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run` or any other command that does not exit (hs-n97, hs-n107); do not stop until you have submitted |
| Job | Goal, Context, Files you may touch (`touches`), Acceptance, Out of scope (or "Nothing is listed. Still do only what the Goal and Acceptance require.") |
| Previous Attempts | only when the Job has ended Attempts: for each, number, Tier, outcome and cause, and what explains it: the conflicting files (`merge_conflict`), or one sentence for every other cause (change: cpd 1 showed only feedback) |
| Answers already given | new: every `answered` Escalation of earlier Attempts, question and answer, so a Worker does not ask again |
| Finish | `cpd attempt submit <attempt>`; on success stop at once; on a refusal, what each code means: `wrong_worktree` (run it from the worktree), `dirty_tree`, `no_commits`, `gate_tampered` (revert the protected paths), `check_failed` (fix, commit, submit again; the tail is in `details`), `check_error` (the check could not run: ask the Lead), `attempt_not_running` (the Attempt was stopped: stop) |

**A review Job's Spec** (B2) differs from the table in these rows: *Who you are* says it is a review Job and that it is a fork of the Lead; *Rules* replace "commit your work" and the quick check with **commit nothing**, and tell it to read the diff of the PR's Lead Branch against the PR's base (`<lead branch>` and `<base>` are given) and the Plan's Specs, to write its findings to `$CPD_HOME/run/<plan>/reviews/<job>.md` (absolute path given, non-empty), and not to edit the repo; *Finish* adds `review_commits` (you committed: undo it and submit again) and `review_missing` (write the findings file) to the refusal codes and drops `no_commits` and `check_failed`. The findings are Markdown; the Lead reads them and turns them into fix Jobs before `pr open`.

The Worker is started as §6.5 says, with the first message `Read <spec path> and follow it exactly. Your attempt id is <attempt>.` and `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT` in its environment.

**Nudge** (A5, once): an `attempt-nudge` Event to the Worker's Session with `data.turns` = the Worker's assistant-turn count when nudged (a resumed driver reads its nudge baseline from it) and `data.prompt` = `You ended your turn without running cpd attempt submit <attempt>. Finish the Spec and submit, or run cpd ask <attempt> "<question>" if you are blocked.` (cpd 1 §12 step 7, kept), delivered with a turn (§15.3).

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
| `tree` (Context column) | `cpd context list`, `cpd session list` | `context-created`, `context-grounded`, `plan-created`, `plan-phase`, `plan-done`, `session-registered`, `session-state`, `session-moved`, `session-renamed`, `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived`, `inbox-opened`, `inbox-closed`, `inbox-snoozed`, `inbox-dismissed` | Contexts, their Plans' Lead Sessions, then the Sessions forked by `session fork` as siblings under their Context (B1), each with its title or name, state, place, `archived_at` (archived objects are in the read model), Job counters (Plan Leads only) and **dot** (rule below), and the **Inbox section** (§13.2). The snapshot reads the Inbox through `cpd_core::inbox::model::list`, the same read as `cpd inbox list` (§21.1): the open, visible items (snoozed and dismissed ones are hidden; `engine-down` included), in its order, with each item's `kind`, `urgency`, `label`, `session`, `plan`, `event`, `url` and `opened_at`; and each Session's state and place for the glyphs the Inbox does not give (the spinner, `󰝦`). The Inbox, not the Sessions' state, says who needs the User, so the snapshot no longer reads the Plan's pending review or its pull requests' checks for the dots. A snooze ends by the clock, with no Event, so the tree re-reads at least every 2 s as well as on Events (and on every `inbox-*` Event), and redraws every 100 ms for the spinner; an item's age is redrawn every second from `opened_at` |
| `plan` (centre top) | `cpd plan show --plan <p>` for the Plan of the Lead in the chat slot (its `prs`, each PR's key, repo, base, needs, status, `pr`, `pull_request` (its `state`, for a closed pull request) and `jobs`, for the PR boxes, §13.2); when a Manager is in the chat slot, the Context's Plans read through `read::tree()` with each Plan's Lead SessionNode's dot, Job counts, done_at and archived_at; the activity Events of the window and each Plan's latest activity Event; the Attempts not ended or ended inside the window | every Event with `plan = p` (the `pr-*` kinds and `pr-feedback` included: they change a box's title or add a box); `session-moved` with `place: "chat"` (the slot changed: switch Plan and take a new snapshot); when a Manager is in the slot, every Event of the Manager's Context and `session-moved` | one box per PR that is not `cancelled`, top to bottom by needs and linked by them, each with the flowchart of its Jobs, Attempts and Tiers (R34, A15); with a Manager, Pulse (one row per Plan of the Context). Also read: the Plan's latest Plan review (its status and number) and `run/<plan>/plannotator-port`, for the pending-review row, and each pull request's `url`, for the `󰏌` links (§13.2). Also read: each pull request's `mergeable`, for the `conflict` title (§13.2). A conflict clears with no Event (only becoming `conflicting` appends `pr-feedback`), so the view also re-reads its snapshot at least every 2 s, as the `widgets` view does, with the same coalescing. Not drawn, so not read: open Escalations |
| `widgets` (Deploy column) | the `open` rows of `pull_requests` with `authored` 1 (contract §4.4), with their `mergeable` (the ` conflict` flag, §13.2) and the PRs of the chat slot's Plan (`prs.pull_request`; the Lead Session's place is `session-moved` `chat`); each open pull request's `url` and its owner's Lead (`prs.pull_request` → `plans.lead_session`); and the host's load average (`/proc/loadavg`; on macOS `sysctl -n vm.loadavg`), re-read every 2 s with no Event | any Event whose id moved, and at least every 2 s: pull requests no Plan owns append no Event (hs-d96), so the poll's changes reach the column by the timer (`pr-opened`, `pr-feedback`, `pr-merged`, `plan-phase`, `plan-done` and `session-moved` move the Event id as before) | the machine's name and load, and the open pull requests with their links and owners. No engine, slots, Escalations or Events |
| `statusbar` (bottom row) | the slots and config `concurrency`: the Attempts not ended, and `concurrency` | any (re-read when the store's highest Event id moved, coalesced to 500 ms, at least every 2 s); no subprocess, no `gh` | `workers N/M` |
| `logs` (popup of the StatusBar's `▤` button) | the latest 500 Events (the `events list` default), oldest first | every Event: it follows live while the view shows the bottom (R36) | the Event log |
| `worker` (beside the chat) | `cpd attempt show <attempt>` for the Worker's session file | `session-state` for its Session; `attempt-status` for its Attempt | the session file itself, read-only (vision decision 5), and the state |
| `history` (chat slot, an archived Session) | the Session's file, named by `cpd session history <id>` (its `sessions.file`) | none: the view is static and re-reads nothing | the Session's pi file itself, read-only, once |
| `chat` (placeholder) | none | none | the static text `No Lead here: pick a Plan in the session tree` |

**Rates.**
- Each view follows Events with no selection and filters by kind itself, through `cpd events follow` or the same poll in process (§5.4): one `PRAGMA data_version` read per `events.poll_ms` (200 ms), which costs no I/O while nothing changes (§5.4).
- On an Event of a kind it shows, a view re-runs its snapshot (it never applies an Event's `data` itself), coalesced to at most one snapshot per view every 500 ms, whatever the Event rate.
- **Pulse** re-reads every 60 s (the window moves and ages tick; a running Attempt is not an Event), and on pane resize (the bucket count changes), coalesced as other views (COALESCE).
- The `worker` view does not follow Events: every 500 ms it re-reads its snapshot from the store (`attempt-status` and `session-state` are read, not followed) and re-reads its session file when its size or modification time changes; the file is not in the database.
- Clocks such as "running 4 m" are redrawn every second from timestamps already read, without a read. The spinner (`󰝦󰪞󰪟󰪠󰪡󰪢󰪣󰪤󰪥` on the tree's dots and on a `running` Job's pill) is redrawn every 100 ms from data already read, without a read.
- If a follower ends, the view takes a new snapshot and follows again from its `events_cursor`.

**The tree's dot rule.** Each row shows one glyph. The filled `󰪥` comes **only from open, visible Inbox items** (hs-d165, hs-d167: the Inbox is the single source of "needs you"; it replaces the per-case rules the tree derived from Session states, Plan phases, Plan reviews and pull request checks, and cpd 1's `@asking`, `@settled_at` and `@seen_at` pane options). The other glyphs are derived from the read model with no write. Nothing is written by the tree.

| Glyph | Meaning | Style |
|---|---|---|
| `󰪥` U+F0AA5 | the row has an open, visible Inbox item (below; the last frame) | the colour of its most urgent item |
| `󰝦󰪞󰪟󰪠󰪡󰪢󰪣󰪤󰪥` U+F0766 (nf-md-circle_outline), then U+F0A9E … U+F0AA5 (nf-md-circle_slice_1 … 8) | the Lead is working (`state = working`); 9 frames, one every 100 ms, on one shared clock (`frame = (unix ms / 100) % 9`), so every spinner turns together | terminal default foreground (no colour) |
| `󰝦` U+F0766 | idle, nothing needed (the first frame) | terminal default foreground |
| `󰝦` U+F0766 | pi stopped (`state = stopped`) | dim |

| `󰪥` colour | Which item (contract §2.2 gives each kind its urgency) |
|---|---|
| red | the most urgent item is red: `dialog` while its Plan is in `dispatch` or `deploy`, `job-stuck`, `pr-failing`, `pr-changes` |
| yellow | yellow: `pr-ready`, `review-requested` |
| magenta | magenta: `dialog` in `grounding`, `plan`, `done` or with no Plan, `plan-review` |
| white | white: `unread` (contract §2.2 gives its rule: a finished turn that answered the User, hs-d278; this table does not define it) |

**Which items fill which row.** A **Session** row's `󰪥` is the most urgent open, visible item **linked to that Session** (`inbox_items.session`): `dialog` and `unread` for any Lead, fork or Manager; and, for a **Plan's own Lead** (`plan_lead`, never a fork), also the items of its Plan: `plan-review`, `job-stuck` and the `pr-*` items of the Plan's pull requests (an item whose `plan` is the Plan). A **Context** row's `󰪥` comes **only from its Manager Session's own items** (`dialog` and `unread` linked to the Manager; hs-d215, hs-d217): it is not the colour of the Context's most urgent Session, and the Context's other items (`job-stuck`, `plan-review`, `question`, `pr-*`, `engine-error`, and the items of its Leads and forks) **do not colour the Context row**; they show only in the Inbox section (§13.2). `engine-down` and `engine-error` are linked to no Session, so they fill no dot. A hidden (snoozed or dismissed) item fills no dot, and when it closes (its cause ends) the dot falls back to the glyph rules below at once. **Own state first** (hs-d183): a `working` Session shows the spinner whatever its items; any other state shows its most urgent item's `󰪥`, else `󰝦` (idle or blocked with no item) or `󰝦` dim (stopped). **A Context row** (hs-d215) is its Manager Session's dot by the same rule, the Session with `role = 'manager'` in that Context, not archived (one per Context, created the first time the Context row is opened, hs-d154); a Context with no Manager yet, or only an archived one, shows `󰝦` (stopped, dim). It is no longer the most urgent Session of the Context. **Archived rows** keep their glyph, drawn in the palette's darker grey (`Meaning::Nothing`), never animated. No phase colours anywhere on dots.

**Precedence.** A row shows its state's spinner when working, else the most urgent item's `󰪥` when it has one, else its state's glyph; the order of the dots is `red > yellow > magenta > white > working > idle > stopped`. Archived rows take no Inbox item.

**Actions.** The views change state only through commands: selecting a Lead runs `cpd session open <id>`; a click on a Context row runs `cpd context manager --context <c>` first (§13.2); the new-Plan key runs `cpd plan new …`; the close key runs `cpd session close <id>`; each item of the tree's right-click menu runs the command §13.2 names; a click on an owned PR row of the Deploy column runs `cpd session open <lead>`, then `select-pane` on the chat slot; parking runs `cpd session park`; opening a Job's Worker opens a `cpd tui view worker` pane beside the chat (`split-window`, `@cpd_view worker`), which is the view's own layout, not Ledger state. The StatusBar's `⧉` click resets the layout (`resize-pane`, the view's own layout, not Ledger state), its legend button opens the legend popup and its `▤` button the logs popup (§13.2). A click on an Inbox item runs `cpd session open <session>` (then `select-pane`, as a Session row), opens the logs popup at the item's Event, or is an OSC 8 link the terminal opens; `s` and `d` and the right-click menu's entries run `cpd inbox snooze` and `cpd inbox dismiss` (§21).

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
| 8 | §6.5, environment | Lead panes also get `PLANNOTATOR_PORT` and `PLANNOTATOR_REMOTE=1` and `PLANNOTATOR_URL_HOST` set by cpd (several Plan reviews at once; amended 2026-10-19, contract §6.5 "The review host") | §15.5 |
| 9 | §6.3, tmux commands | `respawn-pane`, for a dead pane of the cockpit's layout | §13.1 |
| 10 | §1.2, output | `session run`, `tui view` and `tui --attach` own their terminal instead of printing one JSON value (the outline's "builds and attaches the cockpit") | §1.2 |
| 11 | §1.2 table of codes | the error codes this file introduces: `context_required`, `session_required`, `invalid_review_id`, `invalid_session_id`, `invalid_record_id`, `context_exists`, `question_answered`, `not_a_git_repo`, `base_not_found`, `plan_exists`, `illegal_phase`, `no_draft_jobs`, `review_decided`, `dependency_cycle`, `worktree_exists`, `watch_start_failed`, `spec_invalid`, `tier_invalid`, `job_key_exists`, `needs_unknown`, `needs_unavailable`, `nothing_to_edit`, `job_merged`, `job_not_failed`, `job_not_ready`, `spawn_failed`, `attempt_not_running`, `session_malformed`, `driver_lock_missing`, `jobs_not_done`, `rebase_conflict`, `not_a_lead`, `grounded_lead`, `no_cockpit`, `wrong_pane`, `cursor_ahead`, `not_a_terminal`, `doctor_failed`, `pi_not_found`, `catalog_failed`, `file_unreadable`, `invalid_name`; most are cpd 1's (cpd 1 §11 and its `src/`) | §1.3 and each command |
| 12 | §6.1, global options | the options `focus-events on`, `pane-active-border-style fg=blue` and `pane-border-format` (titles: ` Context `, ` Plan `, ` Dispatch `, ` Worker · <job> (read-only) `; none for Leads, the chat placeholder and the StatusBar), the root `MouseDown1Pane` binding and the `pane-died` hook that respawns only a pane with `@cpd_view`: all set at every server start, not only by `cpd tui` | §13.1 |
| 13 | §6.2, §6.3 | a Worker view pane carries `@cpd_view worker`, `@cpd_job` and `@cpd_attempt`, opens with `split-window -h -d -l 50%` and retargets with `respawn-pane -k`; tmux commands `respawn-pane -k`, `set-hook` and `display-popup` | §13.2 |
| 14 | §6.4, §3.6, §5.2, §1.2 | `session close <id>`: kills a Lead's pane, the Session is `stopped` with `session-moved {place: "none"}`, the Lead in the chat slot refused (`in_chat_slot`, exit 3) | §10.9 |
| 15 | §6.3, reset-layout | `cpd tui --respawn` restarts every live view; a StatusBar added to an existing `main` is followed by reset-layout; `cpd tui` applies §6.1's options to a running server | §13.1 |
| 16 | §2.2 Plan, §4.5, §7.6 | `pr poll` stores the PR's title, review decision and checks summary in `plans.pr_title`, `pr_review_decision` and `pr_checks`, added by migration 2 | §9.2 |
| 17 | §6.2 (the views) | the Dispatch column is the machine's hostname and load, the PRs of the Plans in the cockpit, git (future) and Jira (future); no engine, slots, Escalations or Events | §13.2, §17 |
| 18 | §6.2 (the views) | the plan view's top row is ` <context id> ▸ ` and the tabs only; one tab per Lead Session of the Context with a pane; the tree's keys are ↑↓/jk, Enter, `n`, `x`, `?` and its cursor, clicks and prompts are the reference's | §13.2 |
| 19 | §6.2 (the views) | what each view reads, in process; the tree's dot (needs you) rule from Events; the plan row no longer reads Escalations, the Plan review or the PR | §17 |
| 20 | §6.2 (the views) | the legend opens from the tree's `?` and the StatusBar button, not from the Dispatch column; its section titles say Context, and it lists no `cancelled` | §13.2 |
| 21 | §6.2 (the views), R36 | the StatusBar's left side holds action buttons only (`⧉ `, ` ? legend `, `▤`) and its right side read-only information; the Event log is the `logs` view, a popup the `▤` button opens, not part of the Dispatch column | §13.2, §17 |
| 22 | §2.2 Context, Plan, Session | `archived_at` on the three; "Archived" (the archive rule, the cascade, the `archived` refusals); titles editable, ids immutable | §3.7–§3.9, §4.9–§4.11, §10.10–§10.11, §1.3 |
| 23 | §4.5 | Migration 3: `archived_at` on `contexts`, `plans`, `sessions`; `events` recreated with the five new kinds | §3.2, §4.2, §10.1 |
| 24 | §5.2 | Event kinds `context-renamed`, `plan-renamed`, `context-archived`, `plan-archived`, `session-archived` | §3.7–§3.9, §4.9–§4.11, §10.10–§10.11, §17 |
| 25 | §5.3, delivery table | the adapter ignores the five kinds: no message, no turn | §15.3 |
| 26 | §2.2 "Archived", §1.3 table of codes | refusals `archive_blocked` and `archived` (exit 3); `in_chat_slot` for archive | §1.3, §4.1, §10.3, §10.4 |
| 27 | §6.2 (the views) | the tree: labels are titles and names, `…` rows for archived objects, the right-click menu | §13.2, §17 |
| 28 | §6.2, chat slot rule and `@cpd_view` values | the chat slot is the pane of `cpd:main` that has `@cpd_session`, or `@cpd_view` `chat` or `history`; new value `history`, a pane running `cpd tui view history <id>` with `@cpd_history <id>`; at most one placeholder or history pane, only in the slot; the pane title table lists `history` | §10.4, §10.5, §10.12, §13.2, §17 |
| 29 | §6.3, §6.4 | tmux commands: `respawn-pane -k` also puts the history view in the slot; `set -p` also sets `@cpd_view history` and `@cpd_history`; `session open` kills a history pane like the placeholder; `session history` parks a Lead in the slot first; `session run` archives on the User's quit | §10.4, §10.8, §10.12 |
| 30 | §2.2 "Archived", §6.2 (the views) | the tree: archived rows drawn at their live depth, normal guides, `Meaning::Nothing` colour, no blank row; a click on `…` toggles, a click on a guide does nothing; Archive without `y/N`; Archive of the chat slot's Lead opens another Session or parks first; archived rows offer Rename… and Unarchive; a click on an archived Session runs `session history` | §13.2 |
| 31 | §2.2 "Archived", §6.7 | auto-archive when pi exits with status 0 and the wrapper did not cause it: park if in the chat slot, then `session archive` or (a Plan's own Lead) `plan archive`; a refusal archives nothing and keeps the resume prompt | §10.8 |

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

Contract §2.2 "Inbox item", §3.7 E8, §4.4, §5.2. The Inbox is the single source of "needs you": one stored item per thing that needs the User, opened and closed by the engine (E8) from its cause, never by a command. The commands read the items and hide them. Area `core/src/inbox/`. This section describes the core kinds (`dialog`, `plan-review`, `unread`, `job-stuck`, `engine-error`, `question`, and `engine-down`, which is not stored) and the PR poll's (`pr-failing`: checks failed or a merge conflict, `pr-changes`, `pr-ready` and `review-requested`; contract §2.2 gives their conditions, urgency and labels, and their `url` is the pull request's page).

An item as printed:

```json
{"id": 12, "kind": "job-stuck", "urgency": "red", "label": "inbox/core-docs failed", "subject": "job:inbox/core-docs", "context": "building-cpd", "plan": "inbox", "job": "core-docs", "session": null, "record": null, "event": 48311, "url": null, "opened_at": "2026-10-14T21:40:02.114Z", "closed_at": null, "snoozed_until": null, "dismissed_at": null, "age_seconds": 1260}
```

`age_seconds` is `closed_at` (or now, for an open item) minus `opened_at`. The item `engine-down` has `id: null`, `subject: "engine"` and no links.

### 21.1 `inbox list`

```
cpd inbox list [--context <c>] [--all] [--closed] [--limit <n>]
```

| Flag | Meaning |
|---|---|
| `--context <c>` | keep the items of that Context (`engine-down` has none, so it is dropped) |
| `--all` | also list the snoozed and dismissed items, each with `hidden: "snoozed"` or `"dismissed"` |
| `--closed` | list closed items instead of open ones, newest `closed_at` first |
| `--limit <n>` | with `--closed`: at most n items; default 50 (1–5000) |

| Precondition | Code | Exit |
|---|---|---|
| `--all` and `--closed` are not given together, and `--limit` comes only with `--closed` | `usage` | 2 |
| `--context` names a Context that exists | `context_not_found` | 4 |

Effects: none; it takes no lock (§4.2). Open visible items come in the order of §2.2 (red, yellow, magenta, white; oldest `opened_at` first), with `engine-down` first when `locks/watch.lock` is free. A snoozed item whose `snoozed_until` has passed is visible again. `hidden` counts the open items now hidden, whether or not `--all` lists them. `events_cursor` is the highest Event id, so a follower can start from it.

```json
{"ok": true, "engine": {"running": true}, "items": [ {"id": 12, "kind": "job-stuck", "…": "…"} ], "hidden": {"snoozed": 1, "dismissed": 0}, "events_cursor": 48320}
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

Effects, one transaction: **rows** `inbox_items.dismissed_at`; **Event** `inbox-dismissed` `{item}`. The item stays hidden for the rest of its life; because an item lives only while its cause holds, a new cause is a new item and shows again.

```json
{"ok": true, "item": {"id": 12, "dismissed_at": "2026-10-14T21:41:02.114Z", "…": "…"}}
```

Idempotent: yes. An item already dismissed returns `"already": true` and writes nothing, with no Event.
