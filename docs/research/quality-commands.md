# Research: the command layer (Plan `quality`, Job `commands`)

Area: `core/src/{plan,job,pr,session,context,config,doctor}/`, `core/src/{args,error,output,ids,home}.rs`, `cli/src/`. Base: `main` at `e8ad70d` (the worktree's HEAD). Every `path:line` below is at that base. `origin/main` moved to `8d73d96` while this was written (dev-speed `cleanup`, #22, rewrote `core/src/pr/poll.rs`'s clean-up); where that matters the finding says so.

## Summary

The command layer is in good shape: of the 52 commands of these areas, the 25 I ran against the copy of the live store (about 65 invocations, mostly refusal paths, see Method) answered with the exit code and error code `commands.md` gives, and the 8 integration-test files of the area pass (`plan` 34, `job` 26, `context` 38, `config` 29, `doctor` 27, `session` 37, `plan_show` 9, `command_index` 4 tests). The hs-n54/hs-n61/hs-n65 family (Plans without a PR) is fixed at this base (`core/src/plan/row.rs:75` is a `LEFT JOIN`, `cli/tests/plan.rs:478` pins it) and I found no other query of that shape in these areas. What is left is small: **Work** has 10 findings, the worst being that none of the `git` network calls has a timeout while some run under the per-repo lock or the Context lock (W1), that `pr view` reads one page of inline review comments (W2), and that a fork's `session run` can wait for its source forever (W3); the rest are drifts from `commands.md` (title validation, a `details.next` that is not a command, a refusal order, config messages). **Fast** has no real problem: every read command answers in 3 to 16 ms (median) on the copy; two things are worth a Job, the history scan that `plan show` runs for its `tamper` field (about 9 of 16 ms, growing with the history table) and the 111 statements of `plan list`. **Good** is mostly duplication: the same 3-line "open config and store" is pasted 22 times, `bad_row` 6 times, `plan_not_found` 12 times, four structs for "a Plan row", and a block of dead code the build already warns about.

## Findings

| id | class | what | size |
|---|---|---|---|
| commands-W1 | Work | no timeout on any `git` network call; `pr open`/`pr rebase` hold the per-repo lock and `context sync` the Context lock (waited for without limit) across them | Job |
| commands-W2 | Work | `pr view` reads one page (30) of inline review comments | Job |
| commands-W3 | Work | `session run` waits for a fork's source forever when the source has no session file | Job |
| commands-W4 | Work | `pr poll` looks up a pull request closed without a merge on every tick, for ever (unverified at run time) | Job |
| commands-W5 | Work | `session open` changes tmux in 3 steps before its one write; a failure between leaves rows and panes disagreeing (unverified) | Job |
| commands-W6 | Work | `doctor` runs 7 subprocesses with no timeout, one of them `gh auth status` (network) | Job |
| commands-W7 | Work | `context rename`, `plan rename` (and the `new` commands) accept titles `commands.md` §3.7 forbids: 201 characters, newlines | Job |
| commands-W8 | Work | `pr add` on an archived Plan answers `details.next = "cpd plan unarchive <plan>"`, which is a usage error | Job |
| commands-W9 | Work | `plan new` checks `archived` before the idempotent rerun, against §4.1's order | Job |
| commands-W10 | Work | `config` messages: doubled full stop, and a wrong reason for a value above `u32::MAX` | Job |
| commands-F1 | Fast | `plan show` spends about 9 of its ~15 ms in `Store::tamper`'s full scan of `history` | Job |
| commands-F2 | Fast | `plan list` runs 111 statements for 20 Plans (N+1); `plan show` loads Jobs and PRs twice | Job |
| commands-F3 | Fast | a cold Tier catalog costs 0.68 s (median); `doctor` always pays it; `job add/edit/retry` open the store twice | Job |
| commands-F4 | Fast | `session list`'s correlated subqueries scan `plans` and `contexts` per Session (no index) | Job |
| commands-G1 | Good | dead code: `plan/row.rs`'s `PlanRow` (4 build warnings) and 7 stale `#[allow(dead_code)] // Job pr-poll2` | Job |
| commands-G2 | Good | the open-config-and-store block is pasted 22 times; `bad_row` 6, `plan_not_found` 12, `write_file` 2, `sorted_needs` 2, `plan_phase` 2 | Plan |
| commands-G3 | Good | four structs for "a Plan row" and three for "a PR row" in these areas | Job |
| commands-G4 | Good | tests never run a `details.next`; per-file test helpers copied 12 times | Job |
| commands-G5 | Good | `context/mod.rs` and `session/run.rs` hold what the other areas split into modules | Job |

Ranking inside each class is by impact. "Known, still open" items (not re-proposed) are listed after the Good section.

## Work

Commands checked against `docs/commands.md` (✔ ran it against the copy and compared exit code and code; ✎ read the code against the section; both where both): `context` list ✔, show ✔✎, new ✔✎ (invalid id), decide/note/ask/answer ✎, sync ✎, rename ✔✎, archive/unarchive ✎; `plan` list ✔✎, show ✔✎, new ✎, phase ✔✎, render ✔✎ (ran on the copy, see Method), review/decide ✎, lock ✎, rename ✔✎, archive/unarchive ✔✎; `job` add ✔✎, edit ✔✎, cancel ✎, retry ✔✎, list ✔✎, show ✔✎ (`dispatch` is the driver's); `pr` open ✔ (refused `jobs_not_done`, see Method), poll ✔ (refused), rebase ✎, add ✎, edit ✎, cancel ✎, start ✎, view ✔✎; `session` list ✔✎, show ✔✎, fork/open/park/close ✎, state ✔✎, rename ✎, archive/unarchive ✎, history ✎, run ✎; `doctor` ✎ (run once by mistake, see Method), `config` ✔✎, `tiers` ✔✎; the shared modules: id boundaries of §2.1 ✔ (Plan 1/2/24/25 characters, Context 1/2/40/41, Job 32/33, Session 128/129, uppercase, leading digit, trailing dash), `CPD_HOME` empty and relative ✔, `--help`/`--version` ✔, the JSON key sets of `plan list/show`, `job list/show`, `session list/show`, `context list/show` against the examples ✔.

### commands-W1 — `git` network calls have no timeout, under locks

- **What:** `gh` has a 60 s timeout (15 s for the poller). `git` has none: `git::run::run` and `context::git::Git::output` use `Command::output()`. The calls include `fetch`, `push`, `ls-remote`, run by `pr open` (push), `pr rebase` (fetch, push), `pr add` (`ls-remote`, `push --delete`), `plan lock` and `pr start` (fetch, `ls-remote`, push) and `context sync` (fetch, push). `pr open` and `pr rebase` hold the per-repo lock across them (waited for at most `lock_wait_seconds`, then `lock_timeout`), so one stalled remote blocks every other merge of that repo for as long as it stalls. `context sync` holds the Context lock, which is waited for **without limit**, so `context decide/note/ask/answer` and `plan new` of that Context wait behind it.
- **Where:** `core/src/git/run.rs:35-47` (`run`), `core/src/context/git.rs:289-312` (`output`), `core/src/context/lock.rs:344-361` (the unbounded `flock`; its doc says "contract §4.6 gives no timeout"), `core/src/pr/gh.rs:20,24` (the `gh` timeouts, for contrast), `core/src/pr/open.rs:34-45`, `core/src/pr/rebase.rs:26-62`.
- **Evidence:** `rg -n "timeout|Duration" core/src/git/run.rs core/src/context/git.rs core/src/context/lock.rs` prints only `core/src/context/lock.rs:21` (a comment); `rg -n "timeout" core/src/git/*.rs` finds only the `pre-start` hook's (`worktree.rs:116,148`); `rg -n "TIMEOUT" core/src/pr/gh.rs` lists the `gh` ones (`:20`, `:24`, `:180`). Not reproduced: it needs a remote that stalls; **unverified at run time**. Today no Context has a remote (`git -C contexts/<id> remote -v` is empty on the copy), so E6 never touches the network now; the PR paths do.
- **Impact:** a network stall turns into a hung command that holds a lock other commands and the engine's merges wait on. The failure is silent: no error, no Event.
- **Proposed fix:** one `git` runner with a timeout (the `gh` one's `try_wait` loop, shared), `GIT_TERMINAL_PROMPT=0` already set; a `timeout` code path to `git_failed` with `argv` and `exit: null`. For the Context lock, either a `lock_timeout` like the repo lock's, or fetch outside the lock and take it only for the merge. Needs a contract line (§4.6 says no timeout).
- **Size:** Job. **Overlap:** none in a Context record or in-flight Plan (cross-area: `core/src/git/` is the engine/merge area's).

### commands-W2 — `pr view` reads one page of inline comments

- **What:** inline review comments come from `gh api repos/{owner}/{repo}/pulls/<n>/comments` without `--paginate` and without `per_page`, so GitHub returns its default page. Beyond 30 comments the Lead's `pr view` (the detail behind a `pr-feedback`, hs-d98) silently omits the rest.
- **Where:** `core/src/pr/gh.rs:227-231`.
- **Evidence:** `gh api --help | rg -n -i "paginate|per_page"` prints `44: In --paginate mode, all pages of results will sequentially be requested until` and `63: --paginate  Make additional HTTP requests to fetch all pages of results`; `rg -n "paginate|per_page" core/src` prints nothing. The default page size of 30 is GitHub's documented REST default; I did not read a live pull request (no `gh` calls), so the truncation itself is **unverified**.
- **Impact:** a Lead fixing review feedback sees at most the first 30 inline comments of a pull request and does not know there are more.
- **Proposed fix:** `…/comments?per_page=100` with `--paginate`. The installed `gh` is 2.45.0 and its `api --help` has no `--slurp`, so pages most likely print as consecutive JSON arrays (unverified: check on a real pull request): parse them with `serde_json::Deserializer::into_iter`. A test with a two-page fake `gh`.
- **Size:** Job. **Overlap:** none.

### commands-W3 — a fork's `session run` can wait for its source for ever

- **What:** `wait_for_source` loops every 2 s until the source Lead has a session file, is `idle` or `stopped`, has a quiet file and a finished turn. It has no exit: no timeout, no check that the source row still exists as a Lead, is not archived, or that its pane can ever produce a file. A Lead whose pi never wrote a session file (the User closed it before the first turn) makes `existing_file` return `None` for ever, and the fork's pane shows "Waiting for the source Lead to finish its turn" for ever. The grounded Lead that `plan phase plan` creates is exactly such a fork of the Plan's Lead.
- **Where:** `core/src/session/run.rs:315-334` (`wait_for_source`), `:261-263` (`existing_file`).
- **Evidence:** read from the code; the scenario is **unverified** (it needs a Lead that never ran a turn, then `plan phase plan`). `cli/tests/session.rs` has tests for the wait's happy path only (`rg -n "Waiting for the source" cli/tests/session.rs`).
- **Impact:** a parked pane that never starts; no Event, no error; the Plan's Lead (`plan new` with a grounded Lead) is not affected, only the pane that waits.
- **Proposed fix:** give up after `fork_quiet_seconds × k` or when the source is `stopped` with no file, printing why and the `Enter` to retry that `assign_port` already uses.
- **Size:** Job. **Overlap:** none.

### commands-W4 — a pull request closed without a merge is looked up on every poll

- **What:** `pr poll` looks up with `gh` every `open` PR's pull request that is not in the search result. A pull request that was **closed without a merge** is not in the `is:open` search, and its PR stays `open` in cpd (commands.md §9.2, "A pull request closed without a merge (A11)"), so each tick (default `pr_poll_seconds = 5`) adds one lookup call for as long as it stays so. `Reason::Closed` fires once; the lookups do not stop.
- **Where:** `core/src/pr/poll.rs:62-73`, `core/src/pr/pulls.rs:112-116` (`owned_open` selects `r.status = 'open'` whatever the pull request's `state`), `core/src/config/file.rs:232` (default 5).
- **Evidence:** read from the code. **Unverified at run time:** the copy has no `open` PR row (`sqlite3 $T/cpd.db "select p.plan,p.key,q.state from prs p join pull_requests q on q.id=p.pull_request where p.status='open'"` prints nothing), so I could not observe it. If it holds, it is 720 extra GraphQL calls an hour per Lead-abandoned PR set (the lookup batches all of them in one call), on top of the search's 720.
- **Impact:** a steady drain of the 5 000-points-an-hour budget that E5's rate guard (`gh_rate_limited`) then reacts to, and a `gh` subprocess every tick.
- **Proposed fix:** do not look up a pull request whose stored `state` is already `closed` (the row is already right); only look up rows whose state is `open`.
- **Size:** Job. **Overlap:** none (dev-speed #22 changes only the clean-up half of `poll.rs`).

### commands-W5 — `session open` mutates tmux three times, then writes once

- **What:** `swap_pane`, then `rename_window` or `kill_pane`, then (only afterwards) the one transaction that records `session-moved` for both Leads. If `rename_window` or `kill_pane` fails after the swap, the command returns `tmux_failed` with the panes swapped and no row or Event written, so `session list`'s `place` (the latest `session-moved`) says the old Lead is in the chat slot while tmux shows the new one. It heals at the next move of either Lead.
- **Where:** `core/src/session/moves.rs:200-217` (`open`); the same order in `park_lead` (`:253-257`) and in `session history`'s park.
- **Evidence:** read from the code; **unverified** (a fake `tmux` failing `rename-window` would show it). The `place` comes only from Events (`core/src/session/row.rs:90-92`).
- **Impact:** a wrong `place` (the tree's chat marker, `session close`'s `in_chat_slot` guard, which also checks the live pane) until the next move. Low, but `close_lead` trusts `place == "chat"` first.
- **Proposed fix:** write the two `session-moved` rows right after `swap_pane` succeeds (the swap is the fact), then do the rename/kill, whose failure then only leaves a window name.
- **Size:** Job. **Overlap:** the hs-n27 / G11 family (a window left when `set_pane_session` fails; see "Known, still open").

### commands-W6 — `doctor` has no timeout on its 7 subprocesses

- **What:** `doctor` runs, in sequence and with `Command::output()`: `git --version`, `tmux -V`, `pi --version`, `gh --version`, `gh auth status` (a network call), pi's `node --version`, and pi's catalog module (`node -e … catalog.mjs`, which it never reads from the cache). A hung `gh auth status` or `pi --version` hangs `doctor`, the command the User runs when something is wrong.
- **Where:** `core/src/doctor/tools.rs:184-196` (`run_tool`), `:220-225` (`gh_auth`), `core/src/doctor/pi.rs:17-31` (`tiers`, "asked of pi each time").
- **Evidence:** read from the code. Not measured: running `doctor` starts `pi` and `gh`, which the Plan's measurement rules forbid; I ran it once by mistake (see Method) and did not time it.
- **Impact:** `doctor` can hang instead of reporting the failing tool.
- **Proposed fix:** the shared runner of W1 with a 10–20 s timeout; a timeout is a failed check with `message`.
- **Size:** Job. **Overlap:** W1 (one runner).

### commands-W7 — titles are not validated as `commands.md` says

- **What:** §3.7 says `context rename --title` is "1–200 characters like `context new`'s, no control characters" and §4.9 says `plan rename`'s is "as `plan new`'s". The code types every title as `Text` (1 to 20 000 characters, only NUL refused), so a 201-character or multi-line title is accepted, stored, put in Event texts with `{title:?}` and into the Lead's first message header (`You are the Lead of Plan … "<title>"`), and drawn in the cockpit's tree. §3.1 and §4.1 state no limit, so the documents disagree with each other as well.
- **Where:** `core/src/args.rs:101-127` (`Text`), `core/src/context/mod.rs:213` and `:145` (title: `Text`), `core/src/plan/mod.rs:155` and `:74`, `core/src/plan/new.rs:242` (header), `docs/commands.md:300-304,567-572`.
- **Evidence:** `cpd context rename nope --title "$(python3 -c 'print("a"*201)')"` prints `{"ok":false,"error":{"code":"context_not_found",…}}` (exit 4; a validated title would be exit 2 first), the same with `--title "$(printf 'a\nb')"`, and `cpd plan rename --plan nope --title "$(printf 'a\tb')"` prints `plan_not_found`. (All against the copy; `nope` does not exist, so nothing is written.)
- **Impact:** a title with a newline breaks the one-line tree label and the Lead's message header; the doc cannot be tested against until it states the limit.
- **Proposed fix:** a `Title` type in `args.rs` (1–200, no control characters, `invalid_name`-like code or `usage`) used by `context new/rename` and `plan new/rename`; state it in §3.1 and §4.1. Decide whether Job titles (`job add --title`, `Text` today) follow it.
- **Size:** Job. **Overlap:** none.

### commands-W8 — `pr add`'s `archived` refusal names a command that does not parse

- **What:** `pr add` on an archived Plan returns `details.next = "cpd plan unarchive <plan>"`; `plan unarchive` takes `--plan <id>`, not a positional argument. (`session unarchive`'s refusal of the same shape, `core/src/session/archive.rs:229`, is right: `cpd plan unarchive --plan <p>`.)
- **Where:** `core/src/pr/add.rs:109`.
- **Evidence:** `cpd plan unarchive quality` on the copy prints `{"ok":false,"error":{"code":"usage","message":"unexpected argument 'quality' found",…}}` and `cpd plan unarchive --help` shows only `--plan <id>`. `rg -n "plan unarchive" core/src` lists the two spellings. The test that covers it, `cli/tests/pr_crud.rs:258-266` (`add_to_an_archived_plan_is_archived`), asserts the code and exit only, which is why this went unseen.
- **Impact:** a Lead (an agent) following `details.next` literally gets a usage error.
- **Proposed fix:** `format!("cpd plan unarchive --plan {plan}")`, and a test that parses every `details.next` the area can print (G4).
- **Size:** Job (small). **Overlap:** none.

### commands-W9 — `plan new` checks `archived` before the idempotent rerun

- **What:** §4.1 orders the refusals `context_not_found`, `plan_exists` (unless every flag matches: `already`), then `archived`. The code checks the Context's `archived_at` first, so a rerun of an identical `plan new` after its Context was archived answers `archived` (exit 3) instead of `already: true`.
- **Where:** `core/src/plan/new.rs:71-87` (archived at `:72`, the `find`/`already` at `:80-87`).
- **Evidence:** read from the code against `docs/commands.md:377-388` ("checked in the order listed", §1.1); not run (`plan new` starts tmux, which the measurement rules forbid).
- **Impact:** very low (rerunning `plan new` into an archived Context); the idempotence statement ("Idempotent: yes") is false for it.
- **Proposed fix:** move the archived check after the existing-Plan branch, before the insert.
- **Size:** Job (small). **Overlap:** none.

### commands-W10 — `config` messages

- **What:** (a) a message ending in a full stop gets another one appended: `…expected ^[a-z][a-z0-9]{0,7}$..` and `…for example S3..`. (b) A value above `u32::MAX` is reported as "must be at least 1": the arm `max == u32::MAX` does not say the value is too large.
- **Where:** `core/src/config/file.rs:323,339` (`invalid` wraps a `why` that already ends in `.`; the `why` comes from `core/src/ids.rs` and `core/src/config/tier.rs:89`), `:371-374` (`int`).
- **Evidence:** with a scratch `CPD_HOME` holding `version = 1`, `machine = "homeserver"`: `{"…"message":"config.toml: `machine` is invalid: Invalid machine name \"homeserver\": expected ^[a-z][a-z0-9]{0,7}$.."…}`; with `lead_tier = "O9"`: `…expected <O|S|H><0-5>, for example S3..`; with `[watch] tick_seconds = 4294967296`: ``…`watch.tick_seconds` is invalid: must be at least 1, got 4294967296.``
- **Impact:** cosmetic and misleading, in the message the User reads first when a config is wrong. Every other key I tried (unknown key, `0`, wrong type, bad `tmux.socket`, `version = 2`) is refused with `config_invalid` and the right `details.key`.
- **Proposed fix:** trim the trailing `.` in `invalid`; `int` reports "at most {max}" when `i > max`.
- **Size:** Job (small). **Overlap:** none.

### Documents that lag the commands (no finding of their own)

`job list` prints `review` and `job show` prints `tamper`, `plan show`'s Jobs print `fingerprint` and `retry_tier`; `docs/commands.md` §5.6/§5.7's examples show none of them. A `plan` Context record's `plan` omits `jira` when null, §3.3 writes `{id, machine, jira}`. Verified from the JSON key sets (Method, last command).

## Fast

Everything measured here is read commands against the copy (`CPD_HOME=$T`, private tmux socket `quality-commands-<pid>`), the release binary built from this worktree, 20 runs each, wall clock from `date +%s%N` around the process, the machine at load average 17 to 20 on 16 cores (other Workers building), so absolute numbers carry about ±25 % (two full passes differed by that much). The copy has no engine holding the database, so each command also creates and removes the `-wal`/`-shm` files; a pass with a connection held open (`sqlite3` kept alive on the copy) gave the same medians within noise.

### Required measurements

| command | median | max | min | SQL statements (setup 6 + BEGIN/COMMIT) | subprocesses |
|---|---|---|---|---|---|
| `cpd --version` | 3.1 ms | 4.0 | 2.9 | 0 | 0 |
| `cpd --help` | 4.2 ms | 8.5 | 3.1 | 0 | 0 |
| `cpd config` | 2.8 ms | 3.6 | 2.6 | 0 (no store) | 0 |
| `cpd tiers` (cache hit) | 5.5 ms | 6.8 | 4.8 | 11 | 0 |
| `cpd plan list` (20 Plans) | 11.0 ms | 13.4 | 10.2 | **111** | 0 |
| `cpd plan show --plan cross-repo` (36 Jobs, 767 Events) | 16.0 ms | 28.1 | 15.4 | 25 | 0 |
| `cpd plan show --plan quality` (7 Jobs) | 17.1 ms (earlier pass) | 24.6 | 16.0 | 24 | 0 |
| `cpd job list --plan cross-repo` | 7.8 ms | 10.0 | 5.8 | 12 | 0 |
| `cpd job show r34-store --plan cross-repo` | 8.0 ms | 9.1 | 6.2 | 14 | 0 |
| `cpd session list` (197 Sessions) | 10.1 ms | 13.3 | 9.4 | 10 | 0 |
| `cpd session show <id>` | 5.3 ms | 6.5 | 5.0 | 9 | 0 |
| `cpd context list` (2 Contexts, 286 records) | 5.4 ms | 6.4 | 5.2 | 16 | 0 |
| `cpd context show --context building-cpd` | 8.9 ms | 13.8 | 8.0 | 29 | 0 |
| `cpd plan render --plan quality` (a write: `run/quality/plan.md`, on the copy) | 8.1 ms | 11.7 | 6.7 | — | 0 |
| cold `cpd tiers` (cache miss, fresh DB each run, N=10) | **680 ms** | 985 | 663 | — | 1 (`node` catalog module) |

How: `/tmp/qc-bench.sh N cmd…` (a bash loop timing each process), SQL counts from a throwaway build of a copy of the worktree in `/tmp/qc-src` with rusqlite's `trace` feature printing every statement (the repo was not changed): `QC_SQL=1 …/cpd <cmd> 2>&1 >/dev/null | grep -c QCSQL`; subprocesses from `strace -f -e trace=execve` (one `execve`, the process itself, for every row above). The floor is `cpd --version` ≈ 3 ms (process start; a 12 MB binary) plus ≈ 3–4 ms for `Store::open` (a flock, 5 pragmas, the schema hash): `job list` is the cheapest store command at 7.8 ms.

**Write commands and the other tools, from the code** (not run: these start git, tmux, gh or pi). One measured exception, on a scratch `CPD_HOME` (not the copy), since it starts only `git`: `context note` = 25 SQL statements, 2 `git` calls (`add`, `commit`; `commit` starts one `git maintenance run --auto`), median 19.4 ms, max 203 ms (N=12). Counted from the code:

| command | git | gh | tmux | other |
|---|---|---|---|---|
| `plan new` | 0 | 0 | 3 (`has-session`, `new-window` or `new-session`, `set-option`) | 0 |
| `plan lock` | per started PR ≥ 5 (`fetch`, `ls-remote`, `merge-base`, `worktree add`, `push -u`) | 0 | 0 | `pre-start` hook per PR, then `cpd watch --ensure` (which runs tmux) |
| `plan phase plan` | 0 | 0 | 0 or 3 (the grounded Lead's window) | 0 |
| `job add`/`edit`/`retry` | 0 | 0 | 0 | 0 on a catalog cache hit; 1 `node` on a miss |
| `pr add` | 7 on an insert (`rev-parse --git-common-dir`, `worktree list`, `remote get-url`, `symbolic-ref`, `rev-parse --verify`, `rev-parse` ×2) | 0 | 0 | 0 |
| `pr open` | `status`, push, + the Gate hook | 2 (`pr list`, `pr create`) | 0 | the Gate |
| `pr rebase` | ≥ 8 (`status`, `fetch`, `rev-parse` ×3, `merge-tree`, `rebase`, push) | 0 | 0 | the Gate |
| `pr poll` | per merged PR: `worktree remove`, `branch -D` (more with #22) | 1 search + 1 lookup when an owned PR left the search | `list-panes` when something merged | 0 |
| `pr view` | 0 | 2 (`pr view`, `api …/comments`) | 0 | 0 |
| `session fork` | 0 | 0 | 3 | 0 |
| `session open` | 0 | 0 | 4 to 6 (`list-panes`, `new-window`, `set-option`, `swap-pane`, `rename-window`/`kill-pane`) | 0 |
| `context decide`/`note`/`ask`/`answer` | 2 (`add`, `commit`) | 0 | 0 | 0 |
| `context sync` (a Context with a remote) | ≥ 6 (`remote get-url`, `fetch`, `rev-parse` ×2, `merge`, `push`) | 0 | 0 | 0 |
| `doctor` | 1 (`--version`) | 2 (`--version`, `auth status`) | 1 (`-V`) | 3 (`pi --version`, `node --version`, the catalog module) |

### commands-F1 — `plan show` spends most of its time in the `history` scan of `tamper`

- **What:** `plan show` fills `tamper` with `Store::tamper()`, which scans all of `history` for rows whose `txn` has no `txns` row (`SELECT h.id, h.txn FROM history h WHERE NOT EXISTS (SELECT 1 FROM txns t WHERE t.id = h.txn) ORDER BY h.id`). With the foreign key `history.txn REFERENCES txns(id)` on and `foreign_keys = ON` at every open, the report can only be non-empty after the database was changed outside cpd.
- **Where:** `core/src/store/mod.rs:286-302`, called from `core/src/plan/show.rs:146` (and `core/src/doctor/state.rs:53`).
- **Evidence:** the trace of `plan show --plan cross-repo` (three runs): the read transaction (22 statements) ends at 5.8 ms, the `history` query starts at 5.3–6.0 ms and its `COMMIT` is at 13.8, 14.8 and 15.0 ms, so 8.5–9.0 ms of a ≈ 15 ms process; `QC_SQL=1 /tmp/qc-target/release/cpd plan show --plan cross-repo` lines `QCSQL 24 t=6018us: SELECT h.id, h.txn FROM history h …` and `QCSQL 25 t=15004us: COMMIT`. `EXPLAIN QUERY PLAN` is `SCAN h` + `SEARCH t USING INTEGER PRIMARY KEY` (8 094 `history` rows, 5 942 `txns`); `sqlite3 .timer` runs it in 8–13 ms (`sys` time dominates: page reads). It is linear in the history table.
- **Impact:** every `plan show` (the pi adapter calls it at start-up and for each review check: `pi/lib/adapter.ts:180,300,353,388`) pays ~9 ms today and more with each Plan.
- **Proposed fix:** drop the history scan from `plan show` (leave it to `doctor`, whose job it is), or remember the highest checked `history.id`. Decide with store-sql: the cheap form is `tamper` limited to `h.id > <last checked>`.
- **Size:** Job. **Overlap:** the `store-sql` research Job (cross-area: `core/src/store/`); no Context record or in-flight Plan.

### commands-F2 — `plan list` is N+1, `plan show` repeats reads

- **What:** `plan list` runs, per Plan, `jobs::load` (2 statements) and `pr_values` (2 + one `pull_requests` read per PR with a pull request); 20 Plans make 111 statements and 11.0 ms against `job list`'s 7.8. `plan show` runs `jobs::load` and the PR reads a second time inside `plan_fingerprint` (called by `latest_review`): the trace shows statements 12–13 and 17–18 identical (Jobs), 9 and 19 (`pr_needs`), 10 and 20 (`prs`).
- **Where:** `core/src/plan/list.rs:21-24`, `core/src/plan/show.rs:92-130` and `:214-239`, `core/src/plan/fingerprint.rs`.
- **Evidence:** `QC_SQL=1 …/cpd plan list 2>&1 >/dev/null | grep -c QCSQL` prints 111; the same for `plan show --plan cross-repo` prints 25, and the repeated `SELECT job, needs FROM dependencies WHERE plan = 'cross-repo'` is in the trace twice (t=2608 and t=4537 µs, the Jobs query 1.25 ms and 0.9 ms).
- **Impact:** small today (≤ 3 ms); `plan list` is a Lead's command, not the cockpit's (the TUI reads in-process). Worth doing only with F1.
- **Proposed fix:** one query per table for all Plans (`WHERE plan IN (…)` or a single scan grouped in Rust); pass the loaded `jobs` and `prs` into `plan_fingerprint` instead of reloading.
- **Size:** Job. **Overlap:** none.

### commands-F3 — the Tier catalog: cold cost, `doctor`, double open

- **What:** (a) `tiers`/`check_tier` read pi's catalog from `tier_catalog` per `pi_version`, and run pi's Node module on a miss. The miss costs 0.68 s (median of 10; max 0.99 s) against 5.5 ms for a hit: every pi upgrade makes the next `job add`/`job edit --tier`/`job retry` (and engine R) pay it. (b) `doctor`'s `tiers` check never uses the cache, so `doctor` always pays ≥ 0.6 s. (c) `config::catalog::load` opens a second `Store` (flock, pragmas, schema hash) although its callers (`job add/edit/retry`, `pr`'s `lock`, merge) already hold one; `tiers` (one open plus `locate_pi`) is 2.7 ms over `config` (3.1 vs 5.5 ms), about what the second open adds to each `job add`.
- **Where:** `core/src/config/catalog.rs:114-129` (`load`, the second `Store::open` at `:122`), `:134-` (`read`, the `node` call), `core/src/config/cache.rs`, `core/src/job/mod.rs:182` (`check_tier`), `core/src/doctor/pi.rs:23`.
- **Evidence:** cold run: `for i in 1..10: rm -f $T2/cpd.db*; time CPD_HOME=$T2 cpd tiers` with `$T2` a scratch home holding the copy's `config.toml`: `985 691 667 676 679 676 663 683 711 666` ms. The `Store::open` at `catalog.rs:122` is read from the code; (c)'s cost is inferred from `tiers` vs `config`, **unverified** for `job add` itself (a write command, not run).
- **Impact:** low and one-off per pi upgrade; `doctor` is slow for ever. The first `job add` after an upgrade is the surprise.
- **Proposed fix:** pass the caller's `Store` (or the cached `Catalog`) into `Env::check_tier`; let `doctor` use `load` and check the cache row against a fresh read only with a flag. A refresh at `pi upgrade` time (pi-pin owns the pin) would remove the cold path from `job add`.
- **Size:** Job. **Overlap:** Plan `pi-pin` (pi pinning, `pi reload`) touches the same upgrade moment but not the catalog cache.

### commands-F4 — `session list`'s per-row subqueries

- **What:** the `SELECT` shared by `session list/show/find` has four correlated subqueries per Session; two of them (`EXISTS (SELECT 1 FROM plans p WHERE p.lead_session = s.id)`, the same for `contexts`) have no usable index, and `session list` takes 3.8 ms of its 10 ms in this one statement for 197 Sessions.
- **Where:** `core/src/session/row.rs:84-96` (`SELECT`), `:195-` (`query`).
- **Evidence:** `EXPLAIN QUERY PLAN` of the statement on the copy: `SCAN s`, `CORRELATED SCALAR SUBQUERY 2 / SCAN p`, `CORRELATED SCALAR SUBQUERY 3 / SCAN c`, `SEARCH e USING INDEX events_session (session=?)` for the other two, `USE TEMP B-TREE FOR ORDER BY`; trace: `QCSQL 8 t=7762us` (the statement) to `QCSQL 9 t=11522us`.
- **Impact:** 197 × 20 plan rows ≈ 4 000 row visits today; grows with Sessions × Plans.
- **Proposed fix:** an index on `plans(lead_session)` and `contexts(lead_session)` (a migration, so only with another migration), or compute the two sets once and join. Not urgent.
- **Size:** Job. **Overlap:** the `store-sql` research Job (indexes); Plan `manager` owns migrations 0007–0009.

## Good

### commands-G1 — dead code the build already warns about, and stale allows

- **What:** `plan/row.rs`'s `PlanRow`/`PlanPr` are almost unused: the build warns `multiple fields are never read` (twice), `method to_json is never used`, `function query is never used`; only `row::load` is called (for `phase`/`context`). Separately, 7 items carry `#[allow(dead_code)] // Job `pr-poll2` calls it.` although they are called now, two of them placed above their doc comment.
- **Where:** `core/src/plan/row.rs:39-70,160,174`; `core/src/pr/gh.rs:426,455`, `core/src/pr/pulls.rs:112,224`, `core/src/pr/status.rs:45,104,116`, `core/src/context/record.rs:160`.
- **Evidence:** `scripts/cargo.sh build --release -p cpd 2>&1 | rg "warning"` (the four `cpd-core` warnings: `core/src/plan/row.rs:42,57,160,174`); `rg -n "allow\(dead_code\)" core/src`.
- **Impact:** noise that hides real warnings; the Job `pr-poll2` finished long ago.
- **Proposed fix:** delete `PlanRow`'s unused half (keep `phase`/`context` loading or point callers at `show::load_head`), delete the 7 allows and see what the compiler says (the cockpit's two warnings, `tui/src/plan/actions.rs:30,44`, are the tui area's).
- **Size:** Job. **Overlap:** none.

### commands-G2 — one idea, many copies

- **What:** the block `CpdHome::from_env()?; Config::load(&home)?; Store::open(&home, u64::from(config.timeouts.store_busy_ms))?` is written 22 times (`rg -n "timeouts.store_busy_ms" core/src tui/src`): `plan/mod.rs:198`, `job/mod.rs:175`, `pr/mod.rs:159`, `session/mod.rs:208`, `context/mod.rs:266`, `events/mod.rs:93`, `attempt/mod.rs:102`, `ask/mod.rs:67`, the engine, the driver and eight cockpit modules; five areas also define their own `Env` struct around it. Similar: `fn bad_row` ×6 in core (`job/mod.rs:188`, `session/mod.rs:238`, `attempt/mod.rs:109`, `ask/question.rs:215`, `plan/mod.rs:209`, `pr/row.rs:78`) and a 7th in `tui/src/read/shared.rs:18`; `ErrorCode::PlanNotFound` built by hand in 12 files (`rg -n "ErrorCode::PlanNotFound" core/src`); `write_file` twice with different guarantees (`plan/mod.rs:224` writes a temp file and renames, `session/moves.rs:79` writes in place); `sorted_needs` (`job/mod.rs:214`, `pr/mod.rs:217`) and `plan_phase` (`job/mod.rs:193`, `pr/mod.rs:197`) twice; `FileUnreadable` built in 6 places (`job/spec.rs:27`, `session/moves.rs:75`, `plan/mod.rs:217`, `metrics/mod.rs:111`, `session/history.rs:24`, `session/run.rs:302`).
- **Where / Evidence:** the greps above.
- **Impact:** every change to the open sequence (the `store_busy_ms` source, a timeout, F3's shared catalog) is a 22-site edit; `session/moves.rs`'s non-atomic `write_file` is a file a reader can see half written (`run/<plan>/messages/<session>.md`).
- **Proposed fix:** an `env` module in core (`Env::open()`), constructors on `CpdError` (`plan_not_found(&PlanId)`, `bad_row`, `file_unreadable`), one `write_file`, one `sorted_needs` generic over the id type. `docs/implementing.md`'s "one-area rule" tells Jobs not to touch the shared modules, which is how the copies spread; the rule would need the shared module to exist first.
- **Size:** Plan (touches every area; split by area as Jobs after the shared pieces land). **Overlap:** none; cross-area: `tui/` (the 8 cockpit copies).

### commands-G3 — several structs for the same row

- **What:** "a Plan" is read into `plan::row::PlanRow` (mostly dead, G1), `plan::show::Head`, `pr::row::PlanRow`, `plan::new::Created`; "a PR" into `plan::fingerprint::PrSpec`, `pr::start::Pr`, `pr::row::PrRow`. Each has its own SQL and its own `bad_row` parse.
- **Where:** `core/src/plan/row.rs:54`, `core/src/plan/show.rs:23`, `core/src/pr/row.rs`, `core/src/plan/new.rs:22`; `core/src/plan/fingerprint.rs`, `core/src/pr/start.rs:56`, `core/src/pr/row.rs`.
- **Evidence:** `rg -n "struct (PlanRow|Head|Created|PrSpec|Pr|PrRow)\b" core/src`.
- **Impact:** a new column (Plan `archived_at` came in migration 3) is added in four places; the hs-n54 family happened in exactly this seam (a copy that joined `prs`).
- **Proposed fix:** one `PlanHead` and one `PrRow` loaded by the plan area, used by `pr`, `job` and `session`.
- **Size:** Job. **Overlap:** none.

### commands-G4 — `details.next` is never run; test helpers copied

- **What:** nothing executes the `details.next` strings (W8 hid there). The area's tests also copy their helpers: `fn exec(tx` in 12 test files, `fn refused(` in 12, `fn store(env` in 4, `fn events(env` in 5, instead of `cli/tests/common/mod.rs` (276 lines).
- **Where:** `cli/tests/pr_crud.rs:258-266`; `rg -l "fn exec\(tx|fn refused\(" cli/tests`.
- **Evidence:** the greps; W8.
- **Impact:** drift between what refusals promise and what parses; a changed helper has to be changed 12 times.
- **Proposed fix:** a `TestEnv::refused` in `common` that also feeds `details.next` to `Cli::try_parse_from` (the argv words after `cpd`); fold the copies into `common`. Cross-area: `cli/tests/common` is the `tests-build` Job's.
- **Size:** Job. **Overlap:** the `tests-build` research Job.

### commands-G5 — modules that do not follow the layout

- **What:** `plan/`, `job/`, `pr/` split each command into a file; `context/mod.rs` (432 lines) keeps the clap types, `list`, `show`, `plan_repos` and the `write` glue in one file, and `session/run.rs` (408 lines) holds the reload decision, the port wait, the fork wait and the pane check together.
- **Where:** `core/src/context/mod.rs:296-405`, `core/src/session/run.rs`.
- **Evidence:** `wc -l core/src/context/*.rs core/src/session/*.rs`.
- **Impact:** low; `context list` is the one command with its SQL in `mod.rs`, which is where its N+1 (3 queries per Context) hides.
- **Proposed fix:** `context/list.rs`, `context/show.rs`; split `session/run.rs` into `run.rs` and `reload.rs`/`fork_wait.rs`.
- **Size:** Job. **Overlap:** none.

## Known, still open (cited, not re-proposed)

- **hs-n27 / hs-n35, `docs/research/teardown.md` G11:** a Lead window is left when `set_pane_session` fails after `new-window`. Still in the code at this base in all three places: `core/src/plan/lead.rs:75-81`, `core/src/session/moves.rs:140-142` (`fork`) and `:195-197` (`open`). The deferred Plan of hs-n35 covers it. **G4** of the same note (a done Plan's clean-up stops at the first error and never reruns, `core/src/pr/poll.rs`) is rewritten by dev-speed #22 on `origin/main`, outside this base.
- **hs-n54 / hs-n61 / hs-n65 (Plans with no PR):** fixed at this base by `3369692 tree-menu (followups) (#17)`: `core/src/plan/row.rs:75` is `plans p LEFT JOIN prs r …`, `core/src/plan/show.rs` reads `plans` alone, and `cli/tests/plan.rs:478` (`a_plan_with_no_pr_is_read_by_phase_list_show_archive_and_unarchive`) pins phase, list, show, archive and unarchive. Other queries of the same shape: `rg -n "JOIN prs" core/src` lists 10 inner joins, none in these areas drops a Plan (they all start from `jobs` or `attempts`, which exist only after a PR started); `plan/new.rs:38` joins `sessions`, which a Plan always has (foreign key). hs-n65's transient `plan_not_found` is not reproducible from the code (`row::load` is one SELECT with a LEFT JOIN); I cannot say it is the same cause (unverified).
- **hs-n52 / hs-n67 / hs-d139 (`pr rebase` drops base merges):** `core/src/pr/rebase.rs:54-57` is still `git rebase <onto>`; `pr push` and the merge-step skip are on `origin/cpd/cross-repo/plumbing` (`a9b4ea2`), not on `main`.
- **hs-n73 / hs-d140 (followers outliving an install), hs-n81 / hs-d151 / hs-d152 (install/reload):** `events follow` and the reload guard are other areas' and Plan `pi-pin`'s; `core/src/session/run.rs:144-156` already asks the new binary (`may_exec`).
- **hs-n60 (all-merged once per PR):** E4, the engine area's.

## Already good, keep

- **Every refusal I ran has the documented code and exit** (§1.3): `plan_required`/`context_required`/`session_required` (2), `invalid_*_id` at the §2.1 boundaries (2), `*_not_found` (4), `usage` for clap failures with `details.help`, `nothing_to_edit`, `tier_invalid`, `illegal_transition`, `jobs_not_done`; `CPD_HOME` empty or relative is `usage`.
- **The output envelope** (`core/src/output.rs`): exactly one JSON value, `ok` first, a failed write to stdout is exit 1 (tested with a broken pipe), `Output::Owned` only for the commands §1.2 names.
- **`CpdError`/`ErrorCode`** (`core/src/error.rs`): one macro list, each code with its exit; a test per property (unique snake_case, exit table of §1.2, no code exits 0).
- **Parse at the boundary:** ids, `Text`, `Name`, `Label`, `Tier`, `Jira` are types that cannot hold an invalid value; clap keeps their codes (`invalid_plan_id`), and `plan_falls_back_to_cpd_plan` pins the `CPD_PLAN` fallback including a bad value.
- **Idempotence by construction in `job`:** each writing command decides in a read transaction, so a rerun that changes nothing writes no `txns` row, then decides again inside the write transaction (`core/src/job/mod.rs:9-12`); `plan review`/`decide`/`lock` use compare-and-swap on the phase, the fingerprint and the review read.
- **The `gh` gateway** (`core/src/pr/gh.rs`): one place, typed parsing, every failure `gh_failed` with `argv`, `exit` and a stderr tail; both pipes drained while the child runs; timeouts (60 s, 15 s for the poller); the poll parses stdout whatever the exit code (a `NOT_FOUND` alias) and treats `errors`, a missing `data`, a null alias without `NOT_FOUND` as failures, never as "no PRs"; the search query text is a constant pinned by a test; `lookup` validates `owner/name` before it reaches a query string.
- **`pr poll`'s no-op path:** a poll that finds nothing new opens no write transaction (`core/src/pr/poll.rs:75-88`), and the flock wait is bounded (60 s).
- **Context records:** append-only JSONL, rewound when `git commit` fails (`core/src/context/contexts.rs:155-160`), an index rebuilt by `import_locked` after a crash, retries recognised, and the git identity and `commit.gpgsign=false` set per call.
- **Config:** unknown keys, wrong types, out-of-range values and a bad `machine` are all `config_invalid` with `details.key` (the messages need W10's trim); `version`, `machine` required.
- **Speed where it was cheap to get:** no read command spawns a process, the store open is 6 statements, every command is ≤ 16 ms, the Job queries use the `(plan, …)` autoindexes (`EXPLAIN QUERY PLAN` on `dependencies`, `jobs`, `attempts`, `escalations`, `context_records`).
- **Tests:** the 8 test files run in 0.04–11.6 s each (`session` is the slow one, 11.6 s, 37 tests); each command has success, `already` and refusal-writes-nothing cases (`add_refusals_write_nothing`, `edit_refusals_write_nothing`, …); `every_documented_command_exists` pins the command tree to §2.

## Method

The copy: `T=$(mktemp -d); sqlite3 ~/.cpd/cpd.db ".backup $T/cpd.db"; cp ~/.cpd/config.toml $T/; cp -r ~/.cpd/contexts $T/`, then `printf '\n[tmux]\nsocket = "quality-commands-%s"\n' $$ >> $T/config.toml`. No tmux server was ever started on that socket (`tmux -L quality-commands-… ls` was not needed: no command that starts tmux was run). Every measurement and check below ran with `CPD_HOME=$T`; `$T` and the scratch homes are removed.

Commands run (against `$T` unless said):
- Build: `scripts/cargo.sh build --release -p cpd` (binary `target/release/cpd`, 12 MB), 8 warnings (`grep -c warning`).
- Latency: `/tmp/qc-bench.sh 20 target/release/cpd <cmd>` for the commands of the table (N=20 each; the table is the last pass, an earlier pass is in the text where they differ); cold `tiers`: 10 runs, each `rm -f $T2/cpd.db*` in a scratch home with the copy's `config.toml`.
- SQL: a copy of the worktree in `/tmp/qc-src` with `rusqlite` `features += ["trace"]` and a 12-line `conn.trace` hook in `Store::connect`, built with `CARGO_TARGET_DIR=/tmp/qc-target scripts/cargo.sh build --release -p cpd`; `QC_SQL=1 /tmp/qc-target/release/cpd <cmd> 2>&1 >/dev/null | grep QCSQL`.
- Subprocesses: `strace -f -e trace=execve -o /tmp/qc-ex.txt target/release/cpd <cmd>`, `grep -c "execve("` (1 for each read command), and the same for `context note` on a scratch home.
- Query plans: `sqlite3 $T/cpd.db "EXPLAIN QUERY PLAN …"` for the Job, session, history, Context-record and plan queries; `sqlite3 .timer` for the `history` scan.
- Contract checks: the refusal battery (about 65 invocations: `plan show` with no/invalid/unknown Plan, `plan list --phase nope`, `job list/show/add/edit/retry` with bad input, `session show/list/state` with bad input, `context show/new/rename`, `plan/context rename` with a long or multi-line title, `pr view/poll/open --plan …`, `plan phase done`, `plan archive` with no Plan, `plan unarchive quality`, an unknown command), a JSON key-set comparison against §4.2, §4.3, §5.6, §5.7, §10.1, §10.2, §3.2, §3.3 (Python over the output), and a table of ten `config.toml` variants in a scratch `CPD_HOME`.
- Tests: `scripts/cargo.sh test -p cpd --test <plan|job|context|config|doctor|session|plan_show|command_index>`: all pass.
- Reading: every file of `core/src/{plan,job,pr,session,context,config,doctor}/` that a command runs, `args.rs`, `error.rs`, `output.rs`, `cli/src/cli.rs`, `core/src/git/run.rs`, `core/src/store/mod.rs`; `docs/commands.md` §1–§5, §9, §10, §14; Context records hs-n12, n27, n35, n52, n54, n60, n61, n65, n67, n73, n81, d63, d136, d139, d140, d151, d152, d153, d157 (from `cpd context show --context building-cpd` on the copy).

Things I did that the measurement rules do not list, said plainly: (1) I ran `cpd doctor` once against the copy by mistake while looking at the read commands' output; it runs `git/tmux/pi/gh --version`, `gh auth status` and `node` (read-only probes, no writes) and its only effect on the copy is none; I did not run it again or time it. (2) `plan render` was run on the copy for its latency; it writes `$T/run/quality/plan.md`. (3) `pr open --plan quality` and `pr poll --plan nope` were run on the copy to see the refusals; `pr open` was refused at `jobs_not_done` (the Plan's Jobs are running) before any git, gate or `gh` call; had the Jobs been merged it would have run the Gate and pushed from the live Lead worktree, so I did not repeat it. (4) Two read commands, `cpd context list` and `cpd context show`, were first run once without `CPD_HOME`, so against the live `~/.cpd` (read-only; that is the rule's own way to render the Context records). (5) A scratch `CPD_HOME` (empty home, no tmux, only `git`) was used for `context new/note/decide/rename` to count their statements and `git` calls, and for the cold `tiers` runs (which start pi's `node` catalog module).

Not measured, and why: `doctor`'s latency (starts pi and `gh`); the write commands' SQL and process counts other than `context note` (they start tmux, git worktrees, `gh` or pi); anything on a stalled remote or a real GitHub page (W1, W2, W4 are read from code and the cited tool output only).

Cleanup before submit: `$T` and the scratch homes removed, `/tmp/qc-src` and `/tmp/qc-target` removed, no background process left (`sleep`/`sqlite3` keep-alives ended by their own timeout, checked with `ps`).
