# quality-sql-perf: every SQL query cpd runs, measured on a copy (Fast)

Area `sql-perf`, Plan `quality`, Job `sql-perf`, Attempt 1. Base `main` at `4bdfb7e`. All numbers are from a copy of the real database (`.backup` of `~/.cpd/cpd.db`, taken 2026-10-17 22:49 local, schema 6), never from the live store. The correctness of the store is Job `store-work`'s; nothing here judges it.

## Summary

No statement the engine tick, the cockpit views or `events follow` run is slow: the whole engine tick is 0.30 ms of SQL per 5 s tick, a tree snapshot 1.5 ms, a widgets snapshot 1.3 ms, an idle `events follow` poll is four syscalls and no read. The measurable Fast problems are elsewhere, and all of them grow with history rather than with load. (1) `cpd plan show` and `cpd doctor` scan the whole `history` table on every call (`core/src/store/mod.rs:293`): 5.7 ms of CPU in-process, about 9.8 ms in a fresh process, 3,782 `pread64` calls, roughly 70 % of the 13.9 ms the command takes (the share is derived by difference, not profiled: perf is unavailable on this host); the table is 23.4 MB of the 29.1 MB file and gains about 3,000 rows a day (2,713 and 3,575 on the two full days). (2) That table grows because every `UPDATE` writes the full row twice: a Job status change, which changes one column, writes 15 KB of history. (3) `plan list` runs 81 statements and loads every Job's full text to count statuses. (4) The cockpit's job-count statement (`tui/src/read/shared.rs:328`) scans all of `jobs` (1.7 MB, two thirds of it overflow pages) and is the largest statement in every tree, widgets and statusbar refresh: a `jobs(plan, status)` index cuts it from 0.85 to 0.28 ms and the tree snapshot from 1.51 to 0.93 ms. (5)–(7) are small: two unindexed `lead_session` lookups in the session query, an `events(kind, id)` index, and 14 follower processes of 9 MB each. Everything was measured with SQLite CLI 3.45.1 while cpd bundles SQLite 3.53.2, so query plans are unverified against the bundled build; the command latencies (measurement 4) come from the real binary.

## Findings

| id | class | what | size |
|---|---|---|---|
| sql-F1 | Fast | `store.tamper()` scans all of `history` (8,655 rows, 23 MB) on every `plan show` and `doctor`: 5.7 ms in-process, ~9.8 ms in a fresh process, ~70 % of `plan show` (derived) | Job |
| sql-F2 | Fast | `history` is 80 % of the database; each `UPDATE` stores the whole row twice (a Job status flip = 15 KB); ~3,000 rows / day | Plan |
| sql-F3 | Fast | `plan list` is N+1 (81 statements for 20 Plans) and loads every Job's full text to count statuses: 2.4 ms SQL vs 1.05 ms for the read model's four statements | Job |
| sql-F4 | Fast | the job-count statement of every cockpit refresh scans `jobs` (overflow pages): 0.85 ms of a 1.5 ms tree snapshot; index `jobs(plan, status)` → 0.28 ms | Job |
| sql-F5 | Fast | the Session query scans `plans` and `contexts` once per Session (no `lead_session` index): `session list` 1.21 ms; index `plans(lead_session)` → 0.92 ms | Job |
| sql-F6 | Fast | no index on `events(kind)`: the engine read of every widgets refresh walks back to the last `watch-*` Event; `watch --ensure`, `events follow --kind` scan an id range | Job |
| sql-F7 | Fast | one `cpd events follow` process per live Session: 14 × 9 MB = 126 MB RSS, 0.03 % CPU in total (observed, read-only `ps`) | Plan |

Ranked inside the class by impact; the table lists F7 last because it is observation only. No `sql-W` or `sql-G` finding met: the one thing near Work (the 1,458 `prs` history rows whose `before` equals `after`) was the old `pr_polled_at` poller, gone since migration 6; it is evidence for F2, not a defect now.

**Threshold for "slow" (measurement 5).** A statement is slow at ≥ 1 ms median CPU, or a set of statements run together on one tick or refresh at ≥ 1 ms in sum. Why: 1 ms is a fifth of the ~4.7 ms a `cpd` process spends before and around its first query (measurement 4), and the shortest interval any caller waits between runs is 200 ms. Over the threshold: `C9` (5.7 ms), `C1` (1.21 ms), `plan list` (2.4 ms in sum), the tree snapshot (1.51 ms) and the widgets/statusbar snapshot (1.27 ms), whose largest statement is `T4` (0.85 ms, just under, tested because it dominates both sets). Each has one candidate, tested on a copy (F1, F3, F4, F5). F2 and F6 are not slow statements; F6's index was tested for completeness.

## sql-F1: the tamper report scans the whole `history` table

- **Class / size / overlap.** Fast. Job (one statement and its test). Overlap: none (`store-work` owns what the tamper report means; this changes how the same rows are found).
- **What.** `Store::tamper()` runs `SELECT h.id, h.txn FROM history h WHERE NOT EXISTS (SELECT 1 FROM txns t WHERE t.id = h.txn) ORDER BY h.id` to find history rows without a `txns` row. The planner scans the `history` table in rowid order (`SCAN h`), reading its 23 MB of pages, instead of the 100 KB covering index `history_txn`. `plan show` (`core/src/plan/show.rs:146`) and `doctor` (`core/src/doctor/state.rs:53`) call it on every run; the pi adapter calls `plan show` at every Plan Lead start and every review open (`pi/lib/adapter.ts:180`, `:300`).
- **Where.** `core/src/store/mod.rs:293`.
- **Evidence.**
  - `sqlite3 $T/cpd.db "EXPLAIN QUERY PLAN <query>"` → `SCAN h` and `CORRELATED SCALAR SUBQUERY 1 / SEARCH t USING INTEGER PRIMARY KEY (rowid=?)`.
  - Timer, N=15, in one warm session: **5.65 ms median, 6.36 max** CPU (wall 6 ms). Every other statement measured is ≤ 1.4 ms.
  - Fresh process (what a command pays), `./cli.sh 20 … sqlite3 $T/cpd.db "<query>"`: **12.01 ms median** (11.23–13.40) against 2.26 ms for `select 1`; `strace -e trace=pread64`: **3,702** reads.
  - `strace -f -e trace=pread64 cpd <command>` on the copy: `plan show --plan quality` **3,782** `pread64`, `plan list` 639, `session list` 308, `context show` 71, `events list --since 4990` 23. The 3,100 extra reads of `plan show` are this scan.
  - `plan show --plan quality` 13.85 ms median (N=20) against `plan list` 10.10 ms and `events list --since 5006` 4.69 ms. By difference, the scan is most of the 9 ms above the 4.7 ms floor: **derived, not profiled** (no in-process profile: `perf_event_paranoid=4`).
  - Growth: 8,655 history rows today = 5.65 ms warm = 0.65 µs per row; 603, 2,713, 3,575 and 1,764 (to 02:49Z) rows were written on 10-15…10-18, so about +2 ms warm per day if the rate holds (extrapolation, unverified).
- **Impact.** Every `plan show` pays it, and the adapter runs `plan show` at each Plan Lead start and review open. The latency rises with the table, by about 2–3 ms a day at the rate above (extrapolated: unverified); nothing breaks, the command gets slower with age.
- **Proposed fix (tested, not committed).** `… FROM history h INDEXED BY history_txn WHERE NOT EXISTS (…) ORDER BY h.id`: plan `SCAN h USING COVERING INDEX history_txn` + `USE TEMP B-TREE FOR ORDER BY` (empty in the normal case). Warm **5.65 → 1.65 ms** (N=15; max 1.74); fresh process **12.01 → 6.47 ms** (N=20), **186** reads instead of 3,702. Two other rewrites tested (`NOT IN`, `LEFT JOIN … IS NULL`) stay on `SCAN h` (4.46 and 4.89 ms): do not use them. Still linear in history rows (a covering scan plus one `txns` probe per row); F2 is what bounds it.

## sql-F2: `history` is 80 % of the database

- **Class / size / overlap.** Fast (database size, write volume). Plan: it changes contract §4.4 ("the triggers always list every column"), `history_triggers` and needs a migration with a new schema hash. Overlap: hs-d63 keeps `run/` for the User and says nothing about the database; `manager` migrations 0007–0009 add tables whose history triggers would follow the same rule. No Context record sets a history policy (searched `cpd context show --context building-cpd` for history, retention, size, VACUUM).
- **What.** Each `<table>_hist_upd` trigger writes `json_object` of every column for `before` and again for `after`, so an `UPDATE` that changes one column stores the row twice. `jobs` rows carry the Job's goal, context, touches and acceptance (average 7.3 KB, max 15 KB).
- **Where.** `core/src/store/schema.rs:196-217` (`history_triggers`); contract §4.4; the triggers in `$T/cpd.db` (`.schema`).
- **Evidence.**
  - `dbstat` (`SELECT name, sum(pgsize) FROM dbstat GROUP BY name`): `history` 23,355,392 B of 29,069,312 B (80.3 %), `history_tbl_pk` 434,176, `history_txn` 102,400, `jobs` 1,695,744, `events` 1,282,048. `PRAGMA page_count` 7,097 × 4,096 B; `freelist_count` 0.
  - `SELECT tbl, op, count(*), sum(length(before)+length(after)) FROM history GROUP BY tbl, op`: `jobs` update 527 rows, 7,806 KB, **15,167 B per row** (27 % of the file); `prs` update 2,988 rows, 4,688 KB; `attempts` update 1,188 rows, 2,331 KB; `sessions` update 2,388 rows, 2,323 KB.
  - Which columns changed in the last five `jobs` updates (`json_each` of `before` against `after`): `status` twice, `status, merged_sha` once, `status, cancel_reason, cancel_requested_at` once, `status, cancel_reason, cancel_requested_at, failed_cause` once. The other ~16–19 columns, 7 KB of text, are written again each time.
  - Per day (before+after text): 1,429 KB (10-15), 7,077 KB (10-16), 6,159 KB (10-17), 4,915 KB (10-18, to 02:49Z); file size 29.07 MB after 2.4 days of use (txns 2026-10-15T16:50Z…2026-10-18T02:49Z), so about 12 MB a day at this pace (extrapolation: ~4 GB a year; unverified).
  - The 4,688 KB of `prs` history is mostly the old poller: 2,939 `pr poll` rows, 1,458 of them with `before` equal to `after`, the rest a `pr_polled_at` timestamp (a column migration 6 removed). From 22Z on, `pr poll` opens a write transaction only 2–11 times an hour instead of 60 (`SELECT substr(at,1,13), count(*) FROM txns WHERE command='pr poll' GROUP BY 1`), as `core/src/pr/poll.rs:87` promises. It is ballast, not a live cost.
  - The CPU of the trigger is not the problem: `json_object` of all 200 Jobs, 20 columns, takes ~2 ms, ~10 µs per row. Bytes are: each commit writes ~15 KB to the WAL under `synchronous = FULL` (`core/src/store/mod.rs:120`). The commit latency is **unverified**: write commands are not allowed on the copy and `sqlite3` cannot run the triggers (no `cpd_txn()`).
  - `sqlite3 $T/cpd.db ".backup …"`, N=5: 59–66 ms (the copy used for every measurement here).
- **Impact.** Unbounded growth of the file and, through F1, of the scan; larger WAL per write; larger `.backup`. Reads other than F1 never touch `history`.
- **Proposed fix.** Store only the changed columns plus the primary key in `update` rows (`before` and `after` as objects of the changed keys), or leave the long immutable columns (`goal`, `context`, `touches`, `acceptance`, `out_of_scope`) out of `update` rows. The `insert` and `delete` rows keep the full row, so the history stays replayable. This is a contract change (§4.4, the tamper rules) and a migration, hence Plan size. Whether anything reads old `before`/`after` of an update as a full row: only `store/mod.rs:293` and a test (`core/src/pr/pulls.rs:374`) query `history` in the workspace (`grep -rn "FROM history"`); no other reader found.

## sql-F3: `plan list` is N+1 and reads every Job's text

- **Class / size / overlap.** Fast. Job. Overlap: none (the TUI plans use the read model, not this command).
- **What.** For each Plan, `list` runs `jobs::load` (two statements, all 20 columns of every Job, each of `touches`, `acceptance`, `out_of_scope` parsed as JSON) and `pr_values` (two statements plus one `pulls::load` per PR), only to print `counts` and PR fields. 1 + 4 × 20 = 81 statements, 1.47 MB of Job text read and validated (`SELECT sum(length(goal)+length(context)+length(touches)+length(acceptance)+length(out_of_scope)+length(title)) FROM jobs` → 1,468,502 B for 200 Jobs).
- **Where.** `core/src/plan/list.rs:24-25`; `core/src/plan/jobs.rs:69,75`; `core/src/plan/show.rs:94-100`. The read model does the same counts in one `GROUP BY` (`tui/src/read/shared.rs:328`), and its header documents "a fixed number of queries whatever the size of the Ledger" (`tui/src/read/mod.rs`).
- **Evidence.** The 81 statements of `plan list` run in one `sqlite3` session, N=15: **2.38 ms CPU median** (max 2.63). The four read-model statements `T7+T4+T5+T6` (heads, counts, open escalations, pending reviews): **1.05 ms** (max 1.12). `plan list` through the binary: 10.10 ms median (N=20, max 12.09) against 4.69 ms for a one-query `events list`; the other ~3 ms is Rust-side row handling and JSON, **unverified** (no profiler). `strace` counts 639 `pread64` for `plan list`.
- **Impact.** Cost per Plan is ~4 statements and the text of all its Jobs, so it is linear in Plans and in Job text; at 20 Plans the command is 5.4 ms above the one-query floor (10.10 against 4.69 ms), 2.4 ms of it SQL.
- **Proposed fix.** Counts from `SELECT plan, status, count(*) FROM jobs GROUP BY plan, status`, PRs from one query over `prs LEFT JOIN pull_requests`, as `plan_heads` does; keep `jobs::load` for `plan show`, which needs the text. Tested only as the statement count and SQL time above (no code written).

## sql-F4: the job-count statement scans `jobs` in every cockpit refresh

- **Class / size / overlap.** Fast. Job (an index; one migration). Overlap: `manager` owns migrations 0007–0009, so the index migration is 0010 or later and needs its `SCHEMA_HASHES` entry (`core/src/store/schema.rs`) and a contract §4.4 line; `tui-visuals`/`tui-dots` read through `plan_heads` and do not change this statement.
- **What.** `plan_heads` counts Jobs per Plan and status with `FROM jobs j … GROUP BY j.plan, j.status, 4`. No index covers `status`, so it reads the `jobs` table: 200 rows in 414 pages, 276 of them overflow pages (`dbstat`: `jobs` internal 1, leaf 137, overflow 276), because a Job row averages 7.3 KB and its `status` column sits after the long text columns (I infer the scan has to follow the overflow chains; the timing below is the evidence). It runs in the tree snapshot, the widgets snapshot (also the statusbar) and the plan flow head.
- **Where.** `tui/src/read/shared.rs:328`; users: `tui/src/read/tree.rs:100`, `tui/src/read/widgets.rs:155`, `tui/src/read/flow.rs:171`.
- **Evidence.** `EXPLAIN QUERY PLAN`: `SCAN j USING INDEX sqlite_autoindex_jobs_1`, `USE TEMP B-TREE FOR GROUP BY`. N=15: **0.852 ms median, 0.885 max**, the largest statement of every refresh. Sets (sum of medians): tree snapshot **1.513 ms**, widgets/statusbar snapshot **1.274 ms**. A tree snapshot may run every 500 ms while Events flow (`tui/src/tree/mod.rs:49`), the widgets and statusbar views every 500 ms–2 s (`tui/src/widgets/mod.rs:34-38`): at most 0.3 % of a core, so this is headroom, not a defect.
- **Proposed fix (tested, not committed).** `CREATE INDEX jobs_plan_status ON jobs(plan, status)` on a second copy (`ixC.db`): plan `SCAN j USING INDEX jobs_plan_status` (still not covering: the `EXISTS` needs `key`), **T4 0.852 → 0.278 ms** (max 0.313), tree snapshot **1.513 → 0.928 ms**, widgets snapshot **1.274 → 0.694 ms**. Side effects on the same copy: `E4` now seeks `jobs_plan_status (plan=? AND status=?)` (0.035 → 0.037 ms: no change at this size), `E3a` (READY, per tick) stays `SCAN j` (0.084 → 0.085 ms), `P4` (`jobs::load`) also picks the new index for `plan = ?` (0.083 → 0.080 ms, plus a temp B-tree: unchanged). Cost: one more index to maintain on a table updated ~500 times in 2.4 days.

## sql-F5: the Session query scans `plans` and `contexts` per Session

- **Class / size / overlap.** Fast. Job (an index; the same migration as F4). Overlap as F4.
- **What.** `SESSION_SELECT` (cockpit) and `SELECT` (CLI) test `EXISTS (SELECT 1 FROM plans p WHERE p.lead_session = s.id)` and the same for `contexts`; neither column is indexed, so each Session row scans both tables (`SCAN p`, `SCAN c` in the plan). The per-Session lookups of `events` use `events_session (session=?)` and filter `kind` row by row.
- **Where.** `core/src/session/row.rs:89-97` (`session list`, `session show`, 221 rows with `WHERE 1`); `tui/src/read/shared.rs:236-247` (tree, 23 Leads; worker view, 1 row).
- **Evidence.** `EXPLAIN QUERY PLAN` of `C1`: `SCAN s`, `CORRELATED SCALAR SUBQUERY 2 / SCAN p`, `CORRELATED SCALAR SUBQUERY 3 / SCAN c`, `USE TEMP B-TREE FOR ORDER BY`. N=15: **C1 1.211 ms median, 1.277 max** (the second largest statement), `T3` (tree, 23 rows) 0.332 ms. `session list` through the binary: 9.61 ms median (N=20) with 308 `pread64`; its SQL is 12 % of that.
- **Proposed fix (tested, not committed).** `CREATE INDEX plans_lead ON plans(lead_session); CREATE INDEX contexts_lead ON contexts(lead_session);` (`ixB.db`): the plan becomes `SEARCH p USING COVERING INDEX plans_lead (lead_session=?)`, same for `c`; **C1 1.229 → 0.918 ms** (max 0.993). `events(session, kind, id)` alone (`ixA.db`) gives C1 1.184 ms and T3 0.332 → 0.276 ms: weak, so I do not propose it. All four indexes together (`ix.db`): C1 0.844 ms, T3 0.248 ms. At 221 Sessions × 20 Plans the scans are small; they are O(Sessions × Plans) and Sessions are never deleted (they are archived).

## sql-F6: no index on `events(kind)`

- **Class / size / overlap.** Fast. Job (bundle with F4's migration). Overlap as F4.
- **What.** Four reads select Events by `kind` with no kind index: the widgets engine read, `watch --ensure`, `events follow --kind` (the tree's follower passes every kind of `tui/src/tree/follow.rs:14`) and the cockpit's chat-slot lookup. `SELECT … FROM events WHERE kind IN ('watch-started','watch-stopped') ORDER BY id DESC LIMIT 1` plans as `SCAN events`: it walks back from the newest Event to the last `watch-*` (416 Events back today, `4590` against `5006`), so its cost is the Events since the last engine start. The engine restarts often now (`watch-started` at ids 3731, 4178 and 4590 of 5,006) but nothing bounds it.
- **Where.** `tui/src/read/widgets.rs:279` (`W2`, every widgets/statusbar snapshot); `core/src/watch/ensure.rs:76` (`ENS`); `core/src/events/select.rs:95` (`EV6`, `--kind`); `tui/src/cockpit.rs:184` (`K4`).
- **Evidence.** N=15 CPU: `W2` 0.034 ms, `ENS` 0.068 ms (scans ids above 4000), `EV6` (`--since 0`) 0.406 ms, `K4` 0.018 ms. All small today.
- **Proposed fix (tested, not committed).** `CREATE INDEX events_kind ON events(kind, id)` (`ix2.db`): `W2` 0.034 → 0.014 ms, `ENS` 0.068 → 0.011 ms, `EV6` 0.413 → 0.175 ms (plan `SEARCH events USING INDEX events_kind (kind=? AND id>?)`; `W2` and `EV6` gain a `USE TEMP B-TREE FOR ORDER BY`), `K4` unchanged (0.017 ms). Events is the table that every command appends to, with three indexes already (`events_plan`, `events_context`, `events_session`): a fourth costs one more index insert per Event; the saving is real but small, so take it only with F4.

## sql-F7: one follower process per live Session

- **Class / size / overlap.** Fast (memory of long-running processes). Plan (the adapter). Overlap: `pi-pin` (adapter reload) and hs-n73 (followers outliving an install) touch the same processes; this finding is only the cost.
- **What.** The pi adapter starts `cpd events follow --session <id> --since N` per live Session; each is a process with its own SQLite connection.
- **Where.** `pi/lib/follower.ts:69` (the spawn); `core/src/events/mod.rs:112-180` (the loop).
- **Evidence** (read-only `ps -eo pid,etimes,times,rss,args`, 22:59, nothing signalled): 14 `cpd events follow --session …` processes, RSS 8.1–9.3 MB each, **126 MB** in sum, **17 s** of CPU in 60,729 s of process time (0.03 %); `cpd watch` 13.9 MB, 4 s in 5,205 s (0.08 %). `cpd tui view tree|plan|widgets|statusbar` 11.5–12 MB each. `strace -f -tt` of an idle `events follow` on the copy (5 s): per 200 ms poll `fcntl(F_SETLK RDLCK)`, `fstat`, `fcntl(F_UNLCK)`, `poll(fd 1)` — four syscalls, no `pread`: the "no I/O while nothing changes" claim of contract §5.4 holds. The ~9 MB of each process is the binary and SQLite's baseline; I did not split it (unverified).
- **Impact.** 126 MB for 14 Sessions is acceptable on this machine; it scales with live Sessions (`concurrency` 6 Workers + Leads).
- **Proposed fix.** None until it matters: a follower in the adapter's own process would remove the processes (a design change in the adapter, Plan size). Listed for the User to decide.

## Measurements

### 1. Size of the copy

`sqlite3 $T/cpd.db`, each count N=1 (counts do not vary):

| table | rows | table | rows |
|---|---:|---|---:|
| `history` | 8,655 | `plans` | 20 |
| `txns` | 6,396 | `pull_requests` | 25 |
| `events` | 5,006 | `plan_reviews` | 19 |
| `context_records` | 297 | `escalations` | 46 |
| `sessions` | 221 | `dependencies` | 173 |
| `attempts` | 210 | `prs` | 29 |
| `jobs` | 200 | `tier_catalog` | 6 |
| `contexts` | 2 | `pr_needs` | 2 |

`PRAGMA page_count` 7,097; `PRAGMA page_size` 4,096; `freelist_count` 0; `journal_mode` wal; `user_version` 6. Size by object (`dbstat`): `history` 23,355,392 B, `jobs` 1,695,744, `events` 1,282,048, `txns` 577,536, `attempts` 552,960, `history_tbl_pk` 434,176, `events_session` 204,800, `context_records` 167,936, `events_context` 131,072, `events_plan` 118,784, `history_txn` 102,400, `sessions` 98,304.

Files: the copy is 29,069,312 B (no WAL: `.backup` checkpoints). The live files, observed read-only with `ls -la` at 22:49 and 22:57: `cpd.db` 28,889,088 B, `cpd.db-wal` 4,264,232 B (unchanged between the two looks), `cpd.db-shm` 32,768 B.

### 2 and 3. Query plans and times

Every statement the engine, the views, `events follow` and the five measured commands run. `CPU` is `user + sys` from `.timer on` (µs resolution); `wall` is the shell's `real`, which has 1 ms resolution, hence mostly 0. Each statement: one warm-up run, then N=15 in one `sqlite3` session with the rows sent to `/dev/null`; median and max of 15. Parameters are real ids from the copy (`plan quality`, Lead `62b1c19b…`, Attempt `store-work-a1`, Event ids near 5,000). The query text of each id is in Appendix A. `E` engine tick (every 5 s unless noted), `EV` Event fetches (follow, list), `T` tree, `W` widgets/statusbar, `L` logs view, `P` plan view, `K` worker view, `C` commands.

Sets (sum of medians): engine tick `E1+E2a+E2c+E3a+E3b+E4+E7` **0.299 ms** per 5 s tick (+ `E2b` 0.009 ms per live Attempt); tree snapshot `T1…T8` **1.513 ms**; widgets/statusbar snapshot **1.274 ms**; plan-view snapshot (`P1…P6`, `EV0`) 0.232 ms; an idle `events follow` poll runs no statement (`PRAGMA data_version`, four syscalls).

**Every `SCAN` of a table, by statement** (row counts from measurement 1; a `SCAN` reads the whole table or a whole index, a `SEARCH` seeks):
- `events` (5,006 rows): `W2` (walks back from the newest Event to the last `watch-*`), `K4` (back to the first `chat` move), `L1` (the newest 200 ids), `W7` (`LIMIT 0`, reads nothing). The `id > cursor` fetches (`EV1`…`EV7`, `C7`, `L2`, `P9`, `ENS`) are id-range scans, bounded by the cursor, except `--since 0`.
- `jobs` (200 rows, 1.7 MB): `E3a` (every tick), `T4` (every cockpit refresh; F4).
- `attempts` (210): `E2a` (every tick, in `(plan, id)` order, filtering `status <> 'ended'`); `E3b` and `W1` read only the partial index `attempts_active`.
- `sessions` (221): `E2c` (every tick), `T3`, `P1`, `W5` (inside a subquery), `C1`. `plans` (20) and `contexts` (2): `T7`, `T8`, `W3`, `C2`, and once per Session inside `T3`, `K3` and `C1` (F5).
- `escalations` (46): `E1` (every tick), `T5`, `W6`. `plan_reviews` (19): `T6`. `context_records` (297): `T1`, `T2`.
- `pull_requests` (25) and `prs` (29): `E5` runs `SCAN prs` once per pull request (no index on `prs(pull_request)`), `E4`, `E7`, `E5b`, `W4`. `sqlite_schema` (88): `OPEN`, every `Store::open`. `history` (8,655): `C9` (F1).

**Every `USE TEMP B-TREE`**: `FOR ORDER BY` in `E2b`, `E7`, `OPEN`, `EV4`, `T3`, `T7`, `W1`, `W5`, `W6`, `P1`, `P4`, `P7`, `C1`, `C2`, `C4` (each on at most 287 rows); `FOR GROUP BY` in `T4`. None is on a table-sized result except `C4` (the 287 records of a Context) and `C1` (221 Sessions).

The ones on the per-tick path (`E1`, `E2a`, `E2c`, `E3a`, `E4`, `E7`) cost 15–84 µs each today. I measured no scale effect (no synthetic data was allowed), so "these tables only grow, so the scans grow" is an inference from the plans, labelled unverified; none is proposed for an index on this evidence.


| id | source | rows | SCAN / TEMP B-TREE | CPU ms med / max | wall ms med / max |
|---|---|---:|---|---:|---:|
| E1 | `core/src/watch/escalations.rs:25` | 0 | SCAN escalations idx sqlite_autoindex_escalations_1 | 0.015 / 0.023 | 0 / 0 |
| E2a | `core/src/watch/liveness.rs:57` | 3 | SCAN a idx sqlite_autoindex_attempts_1 | 0.070 / 0.097 | 0 / 1 |
| E2b | `core/src/watch/liveness.rs:210` (once per live Attempt) | 0 | TEMP B-TREE ORDER BY | 0.009 / 0.015 | 0 / 0 |
| E2c | `core/src/watch/liveness.rs:276` | 13 | SCAN sessions idx sqlite_autoindex_sessions_1 | 0.058 / 0.066 | 0 / 1 |
| E3a | `core/src/watch/schedule.rs:14` (READY) | 0 | SCAN j | 0.084 / 0.111 | 0 / 1 |
| E3b | `core/src/watch/schedule.rs:27` | 1 | SCAN attempts cov.idx attempts_active | 0.006 / 0.010 | 0 / 0 |
| E4 | `core/src/watch/completion.rs:16` (DONE, params NULL) | 0 | SCAN r idx sqlite_autoindex_prs_1 | 0.035 / 0.048 | 0 / 1 |
| E7 | `core/src/watch/engine.rs:96` (STARTABLE) | 0 | SCAN r idx sqlite_autoindex_prs_2; TEMP B-TREE ORDER BY | 0.031 / 0.050 | 0 / 0 |
| E6 | `core/src/context/contexts.rs:233` (every context_sync_seconds, per Context) | 287 | seeks only | 0.046 / 0.050 | 0 / 1 |
| E5 | `core/src/pr/poll.rs:83` (inside the `pr poll` subprocess) | 3 | SCAN pull_requests cov.idx sqlite_autoindex_pull_requests_1; SCAN prs | 0.039 / 0.053 | 0 / 0 |
| E5b | `core/src/pr/pulls.rs:117` (owned_open, columns trimmed) | 1 | SCAN r idx sqlite_autoindex_prs_1 | 0.017 / 0.024 | 0 / 1 |
| OPEN | `core/src/store/schema.rs:237` (schema_hash, every Store::open) | 88 | SCAN sqlite_schema; TEMP B-TREE ORDER BY | 0.066 / 0.078 | 0 / 1 |
| ENS | `core/src/watch/ensure.rs:76` (watch --ensure) | 1 | id-range scan of events | 0.068 / 0.080 | 0 / 1 |
| EV0 | `core/src/events/select.rs:165` (highest_id) | 1 | seeks only | 0.008 / 0.012 | 0 / 0 |
| EV1 | `core/src/events/select.rs:95` (fetch, no filter) | 16 | id-range scan of events | 0.022 / 0.061 | 0 / 0 |
| EV2 | `core/src/events/select.rs:95` (fetch, --plan) | 124 | seeks only | 0.113 / 0.122 | 0 / 1 |
| EV3 | `core/src/events/select.rs:95` (fetch, --context) | 500 | seeks only | 0.401 / 0.408 | 0 / 1 |
| EV4 | `core/src/events/select.rs:95` (fetch, Lead session) | 150 | TEMP B-TREE ORDER BY | 0.362 / 0.424 | 0 / 1 |
| EV5 | `core/src/events/select.rs:95` (fetch, other session) | 4 | seeks only | 0.016 / 0.024 | 0 / 0 |
| EV6 | `core/src/events/select.rs:95` (fetch, --kind) | 61 | id-range scan of events | 0.406 / 0.432 | 0 / 1 |
| EV7 | `core/src/events/select.rs:95` (fetch, --since 0 whole log) | 500 | id-range scan of events | 0.354 / 0.365 | 0 / 1 |
| T1 | `tui/src/read/tree.rs:82` | 2 | SCAN context_records cov.idx sqlite_autoindex_context_records_1 | 0.027 / 0.033 | 0 / 0 |
| T2 | `tui/src/read/tree.rs:89` | 1 | SCAN q idx sqlite_autoindex_context_records_1 | 0.155 / 0.163 | 0 / 1 |
| T3 | `tui/src/read/shared.rs:236` (SESSION_SELECT, role = 'lead') | 23 | SCAN s; SCAN p; SCAN c; TEMP B-TREE ORDER BY | 0.332 / 0.356 | 0 / 1 |
| T4 | `tui/src/read/shared.rs:328` (plan_heads counts) | 26 | SCAN j idx sqlite_autoindex_jobs_1; TEMP B-TREE GROUP BY | 0.852 / 0.885 | 1 / 1 |
| T5 | `tui/src/read/shared.rs:351` (plan_heads open escalations) | 0 | SCAN escalations idx sqlite_autoindex_escalations_2 | 0.022 / 0.029 | 0 / 0 |
| T6 | `tui/src/read/shared.rs:360` (plan_heads pending reviews) | 0 | SCAN plan_reviews idx sqlite_autoindex_plan_reviews_2 | 0.012 / 0.018 | 0 / 0 |
| T7 | `tui/src/read/shared.rs:366` (plan_heads) | 20 | SCAN p idx sqlite_autoindex_plans_1; TEMP B-TREE ORDER BY; TEMP B-TREE ORDER BY | 0.103 / 0.167 | 0 / 1 |
| T8 | `tui/src/read/tree.rs:103` | 2 | SCAN contexts idx sqlite_autoindex_contexts_1 | 0.010 / 0.012 | 0 / 0 |
| W1 | `tui/src/read/widgets.rs:251` (slots) | 3 | SCAN attempts idx attempts_active; TEMP B-TREE ORDER BY | 0.012 / 0.020 | 0 / 0 |
| W2 | `tui/src/read/widgets.rs:279` (engine) | 1 | SCAN events | 0.034 / 0.042 | 0 / 0 |
| W3 | `tui/src/read/widgets.rs:207` (labels) | 20 | SCAN p | 0.056 / 0.077 | 0 / 0 |
| W4 | `tui/src/read/widgets.rs:225` (pulls) | 22 | SCAN r idx sqlite_autoindex_prs_1 | 0.030 / 0.039 | 0 / 0 |
| W5 | `tui/src/read/widgets.rs:172` (open_prs) | 4 | SCAN q; SCAN s; TEMP B-TREE ORDER BY; TEMP B-TREE ORDER BY | 0.107 / 0.130 | 0 / 1 |
| W6 | `tui/src/read/widgets.rs:298` (escalations) | 0 | SCAN e; TEMP B-TREE ORDER BY | 0.028 / 0.058 | 0 / 0 |
| W7 | `tui/src/read/widgets.rs:320` (recent_events, LIMIT 0 from the widgets view) | 0 | SCAN events | 0.010 / 0.013 | 0 / 0 |
| L1 | `tui/src/read/logs.rs:24` | 1 | SCAN events | 0.022 / 0.028 | 0 / 1 |
| L2 | `tui/src/read/logs.rs:24 + select.rs:95` (logs: fetch after the cursor, no limit) | 206 | id-range scan of events | 0.156 / 0.168 | 0 / 1 |
| P1 | `tui/src/read/flow.rs:386` (chat_slot; also cockpit.rs:184-like) | 1 | SCAN s; TEMP B-TREE ORDER BY | 0.076 / 0.148 | 0 / 1 |
| P2 | `tui/src/read/flow.rs:199` (pr_needs) | 0 | seeks only | 0.007 / 0.014 | 0 / 0 |
| P3 | `tui/src/read/flow.rs:205` (prs of a Plan) | 1 | seeks only | 0.013 / 0.019 | 0 / 1 |
| P4 | `core/src/plan/jobs.rs:75` (jobs::load) | 10 | TEMP B-TREE ORDER BY | 0.083 / 0.155 | 0 / 1 |
| P5 | `core/src/plan/jobs.rs:69` (dependencies) | 13 | seeks only | 0.011 / 0.015 | 0 / 0 |
| P6 | `tui/src/read/flow.rs:269` (attempts of a Plan) | 8 | seeks only | 0.034 / 0.067 | 0 / 0 |
| P7 | `tui/src/read/flow.rs:314` (open escalations of a Plan) | 0 | TEMP B-TREE ORDER BY | 0.010 / 0.028 | 0 / 0 |
| P8 | `tui/src/read/flow.rs:337` (latest review) | 1 | seeks only | 0.010 / 0.016 | 0 / 1 |
| P9 | `tui/src/plan/mod.rs:163 -> select.rs:95` (the plan view's idle Event poll) | 6 | id-range scan of events | 0.016 / 0.023 | 0 / 0 |
| K1 | `tui/src/read/worker.rs:105` (attempt_row) | 1 | seeks only | 0.030 / 0.042 | 0 / 0 |
| K2 | `tui/src/read/worker.rs:150` | 0 | seeks only | 0.010 / 0.014 | 0 / 0 |
| K3 | `tui/src/read/shared.rs:236` (sessions by id, worker view) | 1 | SCAN p; SCAN c | 0.074 / 0.092 | 0 / 1 |
| K4 | `tui/src/cockpit.rs:184` (last_chat_lead) | 1 | SCAN e | 0.018 / 0.029 | 0 / 0 |
| C1 | `core/src/session/row.rs:89` (`session list`, filter 1: every Session) | 221 | SCAN s; SCAN p; SCAN c; TEMP B-TREE ORDER BY | 1.211 / 1.277 | 1 / 2 |
| C2 | `core/src/plan/row.rs:175` (`plan list`: PLAN_WITH_PR, filter 1) | 20 | SCAN p idx sqlite_autoindex_plans_1; TEMP B-TREE ORDER BY; TEMP B-TREE ORDER BY | 0.103 / 0.156 | 0 / 1 |
| C3 | `core/src/plan/show.rs:175` (`plan show` attempts) | 8 | seeks only | 0.020 / 0.027 | 0 / 0 |
| C4 | `core/src/context/mod.rs:342` (`context show` records) | 287 | TEMP B-TREE ORDER BY | 0.435 / 0.498 | 0 / 1 |
| C5 | `core/src/context/mod.rs:376` (`context show` plan_repos, per Plan record) | 1 | seeks only | 0.008 / 0.012 | 0 / 0 |
| C6 | `core/src/context/mod.rs:304` (`context list` open questions, per Context) | 1 | seeks only | 0.159 / 0.188 | 0 / 1 |
| C7 | `core/src/events/select.rs:95` (`events list --since 4990`, limit 500) | 16 | id-range scan of events | 0.022 / 0.026 | 0 / 1 |
| C8 | `core/src/plan/lead.rs:100` | 1 | seeks only | 0.012 / 0.019 | 0 / 0 |
| C9 | `core/src/store/mod.rs:293` (`plan show` and `doctor`: store.tamper(), orphan history rows) | 0 | SCAN h | 5.651 / 6.360 | 6 / 6 |

### 4. Command latency through the built `cpd`

`cpd` from this worktree on the copy (`CPD_HOME=$T`); each command 1 warm-up + N=20, median / max of wall time including the harness's 1.62 ms (`/bin/true`). The query ids are those of measurement 3 / Appendix A.

| command (`cpd …`) | no other connection: median / max ms | a connection held open: median / max ms | stdout bytes | read-model queries shared |
|---|---:|---:|---:|---|
| `--version` | 3.06 / 4.43 | 2.93 / 3.41 | 58 | none (process start; no store) |
| `config` | 2.86 / 4.30 | 2.81 / 2.96 | 1,544 | none (reads `config.toml`, no store) |
| `events list --since 5006 (0 rows)` | not run | 4.69 / 5.81 | 36 | `EV1`/`C7` (`core/src/events/select.rs:95`, 0 rows) and `Store::open` (`OPEN`, schema hash) |
| `events list --since 4990` | 5.03 / 8.13 | 4.89 / 5.39 | 5,251 | `EV1`/`C7` (16 rows); the same fetch the follower, the logs view (`L2`) and the plan view (`P9`) run |
| `events list (since 0, limit 500)` | 7.93 / 9.63 | 7.74 / 9.20 | 177,444 | `EV7` (500 rows) |
| `plan list` | 10.37 / 11.29 | 10.10 / 12.09 | 18,058 | `query_heads` (`core/src/plan/show.rs:85`), then per Plan `P5`+`P4` (`jobs::load`), `P2`+`P3`-like (`pr_values`) (finding F3) |
| `plan show --plan quality` | 13.87 / 15.02 | 13.85 / 15.17 | 89,749 | `P4`, `P5` (`jobs::load`), `P2`/`P3`-like (`pr_values`), `C3` (attempts), `P7`, `P8`, `EV0`, and `C9` (the tamper scan, F1) |
| `plan show --plan cross-repo` | 16.45 / 18.05 | 16.62 / 17.47 | 357,805 | as above, Plan with 38 Jobs and 36 Attempts |
| `session list` | 9.93 / 10.94 | 9.61 / 10.75 | 129,865 | `C1` (221 Sessions, same shape as `T3`/`K3`), `EV0` |
| `context show --context building-cpd` | 8.52 / 9.72 | 8.63 / 9.33 | 284,068 | `C4` (287 records), `C5` per Plan record, the `contexts` row |

### 5. Candidates for the slow statements, tested on second copies

Copies: `cp $T/cpd.db $T/<name>.db`, then one `CREATE INDEX` per copy (no `ANALYZE`: cpd never runs it, `grep -rn "ANALYZE\|optimize" core/src` is empty). Same method as measurement 3. Nothing is committed; any index is a migration (`0010` or later, after `manager`'s 0007–0009) with its `SCHEMA_HASHES` entry and a contract §4.4 line, because the schema check refuses a database whose `sqlite_schema` differs from the binary's.

| finding | candidate | copy | statement | before: CPU ms med / max | after: CPU ms med / max | plan after |
|---|---|---|---|---:|---:|---|
| F1 | rewrite with `INDEXED BY history_txn` | `cpd.db` | `C9` | 5.65 / 6.36 | 1.65 / 1.74 | `SCAN h USING COVERING INDEX history_txn`, `USE TEMP B-TREE FOR ORDER BY` |
| F1 | same, in a fresh `sqlite3` process (wall, N=20) | `cpd.db` | `C9` | 12.01 / 13.40 | 6.47 / 7.99 | 3,702 → 186 `pread64` |
| F3 | one `GROUP BY` for counts, one query for PRs | `cpd.db` | `plan list`: 81 statements → `T7+T4+T5+T6` | 2.38 / 2.63 | 1.05 / 1.12 | (statement count, no code) |
| F4 | `jobs(plan, status)` | `ixC.db` | `T4` | 0.852 / 0.885 | 0.278 / 0.313 | `SCAN j USING INDEX jobs_plan_status` |
| F4 | same | `ixC.db` | tree snapshot (sum) | 1.513 | 0.928 | |
| F4 | same | `ixC.db` | widgets snapshot (sum) | 1.274 | 0.694 | |
| F5 | `plans(lead_session)`, `contexts(lead_session)` | `ixB.db` | `C1` | 1.229 / 1.364 | 0.918 / 0.993 | `SEARCH p USING COVERING INDEX plans_lead (lead_session=?)`, same for `c` |
| F5 | `events(session, kind, id)` | `ixA.db` | `C1` / `T3` | 1.229 / 0.339 | 1.184 / 0.276 | `SEARCH e USING INDEX events_session_kind (session=? AND kind=?)`; weak, not proposed |
| F4+F5+F6 | all four indexes | `ix.db` | `C1` / `T3` / `T4` | 1.211 / 0.332 / 0.852 | 0.844 / 0.248 / 0.282 | |
| F6 | `events(kind, id)` | `ix2.db` | `W2` / `ENS` / `EV6` / `K4` | 0.034 / 0.067 / 0.413 / 0.018 | 0.014 / 0.011 / 0.175 / 0.017 | `SEARCH events USING INDEX events_kind (kind=?)` |

Control: on the four-index copy (`ix.db`) 55 of the 59 statements measured there stay within ±0.015 ms of their time on `cpd.db`; the four that moved are `T3`, `T4`, `C1` (intended) and `EV7`, whose first reading on `ix.db` (0.515 ms against 0.354) was noise: repeated twice, 0.355 ms on both copies. The table in measurement 3 is the `cpd.db` run.

### Live processes (read-only observation)

`ps -eo pid,etimes,times,rss,args` at 22:59, `ls /proc/<pid>/fd | grep -c cpd.db`, nothing signalled and nothing attached: see F7.

## Already good, keep

- **Every statement is index-driven or a bounded scan, and the whole workload is small.** Of 60 statements measured, 59 are ≤ 1.3 ms of CPU; the engine tick is 0.30 ms of SQL per 5 s tick (0.006 % of a core), a tree snapshot 1.5 ms (at most every 500 ms), a widgets snapshot 1.3 ms. The one exception is F1.
- **The views read in one transaction with a fixed number of statements.** `tui/src/read/` has no N+1: tree 8 statements, widgets 12, plan view 8 (`tui/src/read/mod.rs` says so and the code agrees); per-Session lookups are correlated subqueries inside one statement, not extra round trips.
- **An idle `events follow` is free.** Per 200 ms poll: `fcntl` read-lock, `fstat`, `fcntl` unlock, `poll` on stdout — four syscalls, zero `pread` (`strace -f -tt`, 5 s, on the copy). A busy poll is one `id > cursor` range seek: `EV1` 0.022 ms; the Lead's `OR` over `plan`, `context`, `session` (`EV4`) plans as a multi-index OR on the three Event indexes (0.36 ms for 150 rows).
- **The Event indexes fit the filters.** `events_plan(plan, id)`, `events_context(context, id)`, `events_session(session, id)` serve `--plan`, `--context`, `--session`; `events list --since N` is a rowid range scan.
- **The partial indexes work.** `attempts_active` makes the slot count `E3b` a 6 µs covering scan (`SCAN attempts USING COVERING INDEX attempts_active`); `attempts_one_active` is the unique guard.
- **`pr poll` writes only when something changed.** `core/src/pr/poll.rs:87`; since 22Z `pr poll` opened a write transaction 2–11 times an hour (`txns` by hour) against 60 before.
- **Reads never wait for writers.** WAL plus `Store::read` (a deferred transaction on the snapshot); the live WAL stayed at 4,264,232 B between two looks eight minutes apart.
- **The schema check is cheap.** `OPEN` (`sqlite_schema`, 88 rows, 41 KB) is 0.066 ms; the whole `Store::open` is not the cost of a command: `config` (no store) 2.81 ms against `events list` with 0 rows 4.69 ms, with the 1.6 ms of the harness included in both.
- **Process footprint is small.** `cpd watch` 13.9 MB and 0.08 % CPU after 87 min; a follower 9 MB and 0.03 %.

## Method

- **The copy.** `T=$(mktemp -d /tmp/sqlperf-XXXX)`; `sqlite3 ~/.cpd/cpd.db ".backup $T/cpd.db"` (the only access to the live store, a read); `cp ~/.cpd/config.toml $T/`; `cp -r ~/.cpd/contexts $T/`; `printf '\n[tmux]\nsocket = "quality-%s"\n' $$ >> $T/config.toml` (the file had no `[tmux]` table). Every `cpd` command ran as `CPD_HOME=$T target/release/cpd …` with the binary built in this worktree (`timeout 900 scripts/cargo.sh build --release -p cpd`, 1 m 10 s, bundled SQLite 3.53.2 per `strings`). Only read commands ran on the copy: `config`, `plan list`, `plan show`, `session list`, `context show`, `events list`, `events follow` (5 s, killed by `timeout -s KILL`). `watch`, `tui`, `session open/run`, `pr`, `job dispatch` were not run; `tiers` ran once as a smoke test on the copy. `PRAGMA user_version` of the copy was still 6 after all of them.
- **Statements (measurements 2 and 3).** 60 statements transcribed from the sources listed in Appendix A, with real ids from the copy. `sqlite3 -version` 3.45.1 (the bundled one is 3.53.2, so a plan can differ: unverified). `EXPLAIN QUERY PLAN <statement>` for the plans. For times, one `sqlite3 $T/cpd.db` session per statement: `.timer on`, `.output /dev/null`, 1 warm-up and N=15 runs, then `Run Time: real / user / sys` parsed; CPU = user + sys. Median and max of 15. Harness: `/tmp/sqlperf-w/run.py`, `queries.py` (deleted at teardown). The machine's load average was 2.7–6 during the runs (14.9 during the build, before the first measurement); repeats of the slow statements (`C1`, `T3`, `T4`, `EV3`, `EV7`) agreed within 0.03 ms.
- **Commands (measurement 4).** `date +%s%N` around each run in a bash loop (`cli.sh`): 1 warm-up and N=20, median / min / max, twice (a second round agreed within 0.5 ms), once with no other connection to the copy and once with a `sqlite3` process holding the WAL open (like the live engine). The loop's own cost is in every figure: `/bin/true` takes 1.62 ms median (N=20).
- **Reads of the OS.** `strace -f -e trace=pread64` on `plan show`, `plan list`, `session list`, `context show`, `events list`, and on `sqlite3` for `C9`; `strace -f -tt` on `events follow`; `/usr/bin/time -v` (max RSS 7.2 MB `--version`, 8.8 MB `events list`, 12.7 MB `plan show --plan cross-repo`); `ps` for live processes. `perf record` is refused here (`perf_event_paranoid = 4`), so no in-process profile exists: every "share of a command" above is by difference and says so.
- **Not measured, and why.** The write path (commit latency under `synchronous = FULL`, the history triggers, `Store::open`'s `store.lock` flock held across every open): write commands are forbidden on the copy and `sqlite3` has no `cpd_txn()`; no number is given. Scale: no synthetic data was allowed, so growth is extrapolated from today's row counts and labelled. The correctness of the store is Job `store-work`'s.
- **Teardown.** `$T`, `/tmp/sqlperf-w`, `/tmp/sqlperf-build.log`, `/tmp/sqlperf-T` and `/tmp/qs-*` removed; no `sqlite3` holder, no tmux server of mine, nothing else left running.

## Appendix A: the statements, their plans and times

`E` engine, `EV` Event fetches (follow and list), `T` tree, `W` widgets, `L` logs, `P` plan view, `K` worker view, `C` commands. Plans are `EXPLAIN QUERY PLAN` on `cpd.db` with SQLite CLI 3.45.1. The table is measurement 3.

### E1: `core/src/watch/escalations.rs:25`

```sql
SELECT plan, id, asker_pid FROM escalations WHERE status = 'open' ORDER BY plan, id;
```

```
`--SCAN escalations USING INDEX sqlite_autoindex_escalations_1
```

### E2a: `core/src/watch/liveness.rs:57`

```sql
SELECT a.plan, a.id, a.status, a.started_at, a.stop_request, j.cancel_requested_at IS NOT NULL, r.repo
               FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job JOIN prs r ON r.plan = j.plan AND r.key = j.pr
              WHERE a.status <> 'ended' ORDER BY a.plan, a.id;
```

```
|--SCAN a USING INDEX sqlite_autoindex_attempts_1
|--SEARCH j USING INDEX sqlite_autoindex_jobs_1 (plan=? AND key=?)
`--SEARCH r USING INDEX sqlite_autoindex_prs_1 (plan=? AND key=?)
```

### E2b: `core/src/watch/liveness.rs:210 (once per live Attempt)`

```sql
SELECT asked_at, coalesce(answered_at, expired_at) FROM escalations WHERE plan = 'quality' AND attempt = 'store-work-a1' ORDER BY asked_at;
```

```
|--SEARCH escalations USING INDEX sqlite_autoindex_escalations_2 (plan=? AND attempt=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### E2c: `core/src/watch/liveness.rs:276`

```sql
SELECT id, role, state, tmux_pane, name FROM sessions WHERE state <> 'stopped' OR tmux_pane IS NOT NULL ORDER BY id;
```

```
`--SCAN sessions USING INDEX sqlite_autoindex_sessions_1
```

### E3a: `core/src/watch/schedule.rs:14 (READY)`

```sql
SELECT j.plan, j.key FROM jobs j JOIN plans p ON p.id = j.plan
  JOIN prs r ON r.plan = j.plan AND r.key = j.pr
  WHERE j.status = 'queued' AND p.phase IN ('dispatch', 'pr') AND r.status IN ('dispatch', 'open')
    AND NOT EXISTS (SELECT 1 FROM dependencies d JOIN jobs n ON n.plan = d.plan AND n.key = d.needs
                     WHERE d.plan = j.plan AND d.job = j.key AND n.status <> 'merged')
  ORDER BY j.seq;
```

```
|--SCAN j
|--CORRELATED SCALAR SUBQUERY 1
|  |--SEARCH d USING COVERING INDEX sqlite_autoindex_dependencies_1 (plan=? AND job=?)
|  `--SEARCH n USING INDEX sqlite_autoindex_jobs_1 (plan=? AND key=?)
|--SEARCH p USING INDEX sqlite_autoindex_plans_1 (id=?)
`--SEARCH r USING INDEX sqlite_autoindex_prs_1 (plan=? AND key=?)
```

### E3b: `core/src/watch/schedule.rs:27`

```sql
SELECT count(*) FROM attempts WHERE status <> 'ended';
```

```
`--SCAN attempts USING COVERING INDEX attempts_active
```

### E4: `core/src/watch/completion.rs:16 (DONE, params NULL)`

```sql
SELECT r.plan, r.key FROM prs r
  WHERE (NULL IS NULL OR r.plan = NULL) AND (NULL IS NULL OR r.key = NULL)
    AND r.status = 'dispatch' AND r.all_merged_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.status IN ('queued', 'running'))
    AND EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.status = 'merged')
  ORDER BY r.plan, r.key;
```

```
|--SCAN r USING INDEX sqlite_autoindex_prs_1
|--CORRELATED SCALAR SUBQUERY 1
|  `--SEARCH j USING INDEX sqlite_autoindex_jobs_1 (plan=?)
`--CORRELATED SCALAR SUBQUERY 2
   `--SEARCH j USING INDEX sqlite_autoindex_jobs_1 (plan=?)
```

### E7: `core/src/watch/engine.rs:96 (STARTABLE)`

```sql
SELECT r.plan, r.key FROM prs r JOIN plans p ON p.id = r.plan
  WHERE p.phase = 'dispatch' AND r.status = 'waiting' AND r.start_failed_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM pr_needs n JOIN prs q ON q.plan = n.plan AND q.key = n.needs
                     WHERE n.plan = r.plan AND n.pr = r.key AND q.status <> 'merged')
  ORDER BY p.created_at, r.plan, r.key;
```

```
|--SCAN r USING INDEX sqlite_autoindex_prs_2
|--CORRELATED SCALAR SUBQUERY 1
|  |--SEARCH n USING COVERING INDEX sqlite_autoindex_pr_needs_1 (plan=? AND pr=?)
|  `--SEARCH q USING INDEX sqlite_autoindex_prs_1 (plan=? AND key=?)
|--SEARCH p USING INDEX sqlite_autoindex_plans_1 (id=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### E6: `core/src/context/contexts.rs:233 (every context_sync_seconds, per Context)`

```sql
SELECT id FROM context_records WHERE context = 'building-cpd';
```

```
`--SEARCH context_records USING COVERING INDEX sqlite_autoindex_context_records_1 (context=?)
```

### E5: `core/src/pr/poll.rs:83 (inside the `pr poll` subprocess)`

```sql
SELECT repo, number FROM pull_requests WHERE NOT EXISTS (SELECT 1 FROM prs WHERE prs.pull_request = pull_requests.id);
```

```
|--SCAN pull_requests USING COVERING INDEX sqlite_autoindex_pull_requests_1
`--CORRELATED SCALAR SUBQUERY 1
   `--SCAN prs
```

### E5b: `core/src/pr/pulls.rs:117 (owned_open, columns trimmed)`

```sql
SELECT r.plan, r.key, pr.id, pr.repo, pr.number FROM prs r JOIN pull_requests pr ON pr.id = r.pull_request WHERE r.status = 'open' ORDER BY r.plan, r.key;
```

```
|--SCAN r USING INDEX sqlite_autoindex_prs_1
`--SEARCH pr USING INTEGER PRIMARY KEY (rowid=?)
```

### OPEN: `core/src/store/schema.rs:237 (schema_hash, every Store::open)`

```sql
SELECT type, name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name;
```

```
|--SCAN sqlite_schema
`--USE TEMP B-TREE FOR ORDER BY
```

### ENS: `core/src/watch/ensure.rs:76 (watch --ensure)`

```sql
SELECT EXISTS (SELECT 1 FROM events WHERE id > 4000 AND kind = 'watch-started' AND json_extract(data, '$.pid') = 12345);
```

```
|--SCAN CONSTANT ROW
`--SCALAR SUBQUERY 1
   `--SEARCH events USING INTEGER PRIMARY KEY (rowid>?)
```

### EV0: `core/src/events/select.rs:165 (highest_id)`

```sql
SELECT coalesce(max(id), 0) FROM events;
```

```
`--SEARCH events
```

### EV1: `core/src/events/select.rs:95 (fetch, no filter)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 4990 ORDER BY id LIMIT 500;
```

```
`--SEARCH events USING INTEGER PRIMARY KEY (rowid>?)
```

### EV2: `core/src/events/select.rs:95 (fetch, --plan)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 4000 AND plan = 'quality' ORDER BY id LIMIT 500;
```

```
`--SEARCH events USING INDEX events_plan (plan=? AND id>?)
```

### EV3: `core/src/events/select.rs:95 (fetch, --context)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 4000 AND context = 'building-cpd' ORDER BY id LIMIT 500;
```

```
`--SEARCH events USING INDEX events_context (context=? AND id>?)
```

### EV4: `core/src/events/select.rs:95 (fetch, Lead session)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 4000 AND (plan = 'quality' OR (plan IS NULL AND context = 'building-cpd') OR session = '62b1c19b-62a5-492e-ae06-12ec8db845a0') ORDER BY id LIMIT 500;
```

```
|--MULTI-INDEX OR
|  |--INDEX 1
|  |  `--SEARCH events USING INDEX events_plan (plan=? AND id>?)
|  |--INDEX 2
|  |  `--SEARCH events USING INDEX events_context (context=? AND id>?)
|  `--INDEX 3
|     `--SEARCH events USING INDEX events_session (session=? AND id>?)
`--USE TEMP B-TREE FOR ORDER BY
```

### EV5: `core/src/events/select.rs:95 (fetch, other session)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 4000 AND session = '0838fefc-1b82-45c4-b184-877d9070873c' ORDER BY id LIMIT 500;
```

```
`--SEARCH events USING INDEX events_session (session=? AND id>?)
```

### EV6: `core/src/events/select.rs:95 (fetch, --kind)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 0 AND kind IN ('error', 'escalation') ORDER BY id LIMIT 500;
```

```
`--SEARCH events USING INTEGER PRIMARY KEY (rowid>?)
```

### EV7: `core/src/events/select.rs:95 (fetch, --since 0 whole log)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 0 ORDER BY id LIMIT 500;
```

```
`--SEARCH events USING INTEGER PRIMARY KEY (rowid>?)
```

### T1: `tui/src/read/tree.rs:82`

```sql
SELECT context, count(*) FROM context_records GROUP BY context;
```

```
`--SCAN context_records USING COVERING INDEX sqlite_autoindex_context_records_1
```

### T2: `tui/src/read/tree.rs:89`

```sql
SELECT q.context, count(*) FROM context_records q WHERE q.kind = 'question'
               AND NOT EXISTS (SELECT 1 FROM context_records a
                               WHERE a.context = q.context AND a.kind = 'answer' AND a.question = q.id)
              GROUP BY q.context;
```

```
|--SCAN q USING INDEX sqlite_autoindex_context_records_1
`--CORRELATED SCALAR SUBQUERY 1
   `--SEARCH a USING INDEX sqlite_autoindex_context_records_1 (context=?)
```

### T3: `tui/src/read/shared.rs:236 (SESSION_SELECT, role = 'lead')`

```sql
SELECT s.id, s.role, s.context, s.plan, s.attempt, s.name, s.file, s.forked_from, s.tier,
       s.tmux_pane, s.state, s.state_label, s.state_at, s.created_at,
       (SELECT json_extract(e.data, '$.place') FROM events e
         WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1),
       EXISTS (SELECT 1 FROM plans p WHERE p.lead_session = s.id),
       EXISTS (SELECT 1 FROM contexts c WHERE c.lead_session = s.id),
       (SELECT e.id FROM events e WHERE e.session = s.id AND e.kind = 'session-state'
           AND json_extract(e.data, '$.to') = 'idle' ORDER BY e.id DESC LIMIT 1),
       (SELECT e.id FROM events e WHERE e.session = s.id AND e.kind = 'session-moved'
           AND json_extract(e.data, '$.place') = 'chat' ORDER BY e.id DESC LIMIT 1),
       s.archived_at
  FROM sessions s WHERE s.role = 'lead' ORDER BY s.created_at, s.id;
```

```
|--SCAN s
|--CORRELATED SCALAR SUBQUERY 1
|  `--SEARCH e USING INDEX events_session (session=?)
|--CORRELATED SCALAR SUBQUERY 2
|  `--SCAN p
|--CORRELATED SCALAR SUBQUERY 3
|  `--SCAN c
|--CORRELATED SCALAR SUBQUERY 4
|  `--SEARCH e USING INDEX events_session (session=?)
|--CORRELATED SCALAR SUBQUERY 5
|  `--SEARCH e USING INDEX events_session (session=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### T4: `tui/src/read/shared.rs:328 (plan_heads counts)`

```sql
SELECT j.plan, j.status, count(*),
                j.status = 'running' AND EXISTS (SELECT 1 FROM attempts a
                  WHERE a.plan = j.plan AND a.job = j.key AND a.status = 'merging')
           FROM jobs j WHERE NULL IS NULL OR j.plan = NULL GROUP BY j.plan, j.status, 4;
```

```
|--SCAN j USING INDEX sqlite_autoindex_jobs_1
|--USE TEMP B-TREE FOR GROUP BY
|--CORRELATED SCALAR SUBQUERY 1
|  `--SEARCH a USING INDEX sqlite_autoindex_attempts_2 (plan=? AND job=?)
`--CORRELATED SCALAR SUBQUERY 1
   `--SEARCH a USING INDEX sqlite_autoindex_attempts_2 (plan=? AND job=?)
```

### T5: `tui/src/read/shared.rs:351 (plan_heads open escalations)`

```sql
SELECT plan, count(*) FROM escalations WHERE status = 'open' AND (NULL IS NULL OR plan = NULL) GROUP BY plan;
```

```
`--SCAN escalations USING INDEX sqlite_autoindex_escalations_2
```

### T6: `tui/src/read/shared.rs:360 (plan_heads pending reviews)`

```sql
SELECT DISTINCT plan FROM plan_reviews WHERE status = 'pending' AND (NULL IS NULL OR plan = NULL);
```

```
`--SCAN plan_reviews USING INDEX sqlite_autoindex_plan_reviews_2
```

### T7: `tui/src/read/shared.rs:366 (plan_heads)`

```sql
SELECT p.id, p.context, p.title, p.jira, p.phase, p.lead_session, r.pr_number,
                r.pr_url, r.pr_opened_at, q.state, q.checks, p.created_at, p.locked_at, r.all_merged_at, p.done_at, p.archived_at
           FROM plans p LEFT JOIN prs r ON r.plan = p.id
                AND r.key = (SELECT key FROM prs WHERE plan = p.id ORDER BY created_txn, key LIMIT 1)
                LEFT JOIN pull_requests q ON q.id = r.pull_request
          WHERE NULL IS NULL OR p.id = NULL ORDER BY p.created_at, p.id;
```

```
|--SCAN p USING INDEX sqlite_autoindex_plans_1
|--SEARCH r USING INDEX sqlite_autoindex_prs_1 (plan=? AND key=?) LEFT-JOIN
|--CORRELATED SCALAR SUBQUERY 1
|  |--SEARCH prs USING INDEX sqlite_autoindex_prs_2 (plan=?)
|  `--USE TEMP B-TREE FOR ORDER BY
|--SEARCH q USING INTEGER PRIMARY KEY (rowid=?) LEFT-JOIN
`--USE TEMP B-TREE FOR ORDER BY
```

### T8: `tui/src/read/tree.rs:103`

```sql
SELECT id, title, jira, lead_session, sync_failing, archived_at FROM contexts ORDER BY id;
```

```
`--SCAN contexts USING INDEX sqlite_autoindex_contexts_1
```

### W1: `tui/src/read/widgets.rs:251 (slots)`

```sql
SELECT plan, id, tier, status, started_at FROM attempts WHERE status <> 'ended' ORDER BY started_at, plan, id;
```

```
|--SCAN attempts USING INDEX attempts_active
`--USE TEMP B-TREE FOR ORDER BY
```

### W2: `tui/src/read/widgets.rs:279 (engine)`

```sql
SELECT kind, at, data FROM events WHERE kind IN ('watch-started', 'watch-stopped') ORDER BY id DESC LIMIT 1;
```

```
`--SCAN events
```

### W3: `tui/src/read/widgets.rs:207 (labels)`

```sql
SELECT p.id, s.name,
                (SELECT json_extract(e.data, '$.place') FROM events e
                  WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1)
           FROM plans p JOIN sessions s ON s.id = p.lead_session;
```

```
|--SCAN p
|--SEARCH s USING INDEX sqlite_autoindex_sessions_1 (id=?)
`--CORRELATED SCALAR SUBQUERY 1
   `--SEARCH e USING INDEX events_session (session=?)
```

### W4: `tui/src/read/widgets.rs:225 (pulls)`

```sql
SELECT r.plan, r.key, r.pr_number, r.status, q.state, q.title, q.review_decision, q.checks
           FROM prs r JOIN pull_requests q ON q.id = r.pull_request ORDER BY r.plan, r.key;
```

```
|--SCAN r USING INDEX sqlite_autoindex_prs_1
`--SEARCH q USING INTEGER PRIMARY KEY (rowid=?)
```

### W5: `tui/src/read/widgets.rs:172 (open_prs)`

```sql
SELECT q.repo, q.number, q.title, q.draft, q.checks, q.review_decision,
                EXISTS (SELECT 1 FROM prs r WHERE r.pull_request = q.id AND r.plan =
                        (SELECT s.plan FROM sessions s
                           JOIN events m ON m.id = (SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-moved')
                          WHERE s.role = 'lead' AND s.plan IS NOT NULL AND json_extract(m.data, '$.place') = 'chat'
                          ORDER BY m.id DESC LIMIT 1))
           FROM pull_requests q WHERE q.state = 'open' ORDER BY q.created_at, q.repo, q.number;
```

```
|--SCAN q
|--CORRELATED SCALAR SUBQUERY 3
|  |--SEARCH r USING INDEX sqlite_autoindex_prs_2 (plan=?)
|  `--SCALAR SUBQUERY 2
|     |--SCAN s
|     |--SEARCH m USING INTEGER PRIMARY KEY (rowid=?)
|     |--CORRELATED SCALAR SUBQUERY 1
|     |  `--SEARCH e USING INDEX events_session (session=?)
|     `--USE TEMP B-TREE FOR ORDER BY
`--USE TEMP B-TREE FOR ORDER BY
```

### W6: `tui/src/read/widgets.rs:298 (escalations)`

```sql
SELECT p.context, e.plan, a.job, e.id, e.attempt, e.question, e.asked_at
           FROM escalations e JOIN attempts a ON a.plan = e.plan AND a.id = e.attempt
                              JOIN plans p ON p.id = e.plan
          WHERE e.status = 'open' ORDER BY e.asked_at, e.plan, e.id;
```

```
|--SCAN e
|--SEARCH a USING INDEX sqlite_autoindex_attempts_1 (plan=? AND id=?)
|--SEARCH p USING INDEX sqlite_autoindex_plans_1 (id=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### W7: `tui/src/read/widgets.rs:320 (recent_events, LIMIT 0 from the widgets view)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events ORDER BY id DESC LIMIT 0;
```

```
`--SCAN events
```

### L1: `tui/src/read/logs.rs:24`

```sql
SELECT coalesce(min(id), 1) - 1 FROM (SELECT id FROM events ORDER BY id DESC LIMIT 200);
```

```
|--CO-ROUTINE (subquery-1)
|  `--SCAN events
`--SEARCH (subquery-1)
```

### L2: `tui/src/read/logs.rs:24 + select.rs:95 (logs: fetch after the cursor, no limit)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 4800 AND id <= 5006 ORDER BY id LIMIT 9223372036854775807;
```

```
`--SEARCH events USING INTEGER PRIMARY KEY (rowid>? AND rowid<?)
```

### P1: `tui/src/read/flow.rs:386 (chat_slot; also cockpit.rs:184-like)`

```sql
SELECT s.id, s.name, s.plan, m.id FROM sessions s
                   JOIN events m ON m.id = (SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-moved')
                  WHERE s.role = 'lead' AND s.plan IS NOT NULL AND json_extract(m.data, '$.place') = 'chat'
                  ORDER BY m.id DESC LIMIT 1;
```

```
|--SCAN s
|--SEARCH m USING INTEGER PRIMARY KEY (rowid=?)
|--CORRELATED SCALAR SUBQUERY 1
|  `--SEARCH e USING INDEX events_session (session=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### P2: `tui/src/read/flow.rs:199 (pr_needs)`

```sql
SELECT pr, needs FROM pr_needs WHERE plan = 'quality' ORDER BY pr, needs;
```

```
`--SEARCH pr_needs USING COVERING INDEX sqlite_autoindex_pr_needs_1 (plan=?)
```

### P3: `tui/src/read/flow.rs:205 (prs of a Plan)`

```sql
SELECT r.key, r.repo, r.base, r.status, r.pr_number, q.state FROM prs r LEFT JOIN pull_requests q ON q.id = r.pull_request WHERE r.plan = 'quality' AND r.status <> 'cancelled' ORDER BY r.key;
```

```
|--SEARCH r USING INDEX sqlite_autoindex_prs_1 (plan=?)
`--SEARCH q USING INTEGER PRIMARY KEY (rowid=?) LEFT-JOIN
```

### P4: `core/src/plan/jobs.rs:75 (jobs::load)`

```sql
SELECT seq, key, pr, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, retry_tier, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at FROM jobs WHERE plan = 'quality' ORDER BY seq;
```

```
|--SEARCH jobs USING INDEX sqlite_autoindex_jobs_1 (plan=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### P5: `core/src/plan/jobs.rs:69 (dependencies)`

```sql
SELECT job, needs FROM dependencies WHERE plan = 'quality' ORDER BY job, needs;
```

```
`--SEARCH dependencies USING COVERING INDEX sqlite_autoindex_dependencies_1 (plan=?)
```

### P6: `tui/src/read/flow.rs:269 (attempts of a Plan)`

```sql
SELECT a.job, a.id, a.number, a.tier, a.origin, a.status, a.outcome, a.cause, a.started_at, a.submitted_at,
                a.ended_at, a.worker_session, s.state, s.state_label, s.state_at
           FROM attempts a LEFT JOIN sessions s ON s.id = a.worker_session
          WHERE a.plan = 'quality' ORDER BY a.job, a.number;
```

```
|--SEARCH a USING INDEX sqlite_autoindex_attempts_2 (plan=?)
`--SEARCH s USING INDEX sqlite_autoindex_sessions_1 (id=?) LEFT-JOIN
```

### P7: `tui/src/read/flow.rs:314 (open escalations of a Plan)`

```sql
SELECT id, attempt, question, asked_at FROM escalations WHERE plan = 'quality' AND status = 'open' ORDER BY asked_at, id;
```

```
|--SEARCH escalations USING INDEX sqlite_autoindex_escalations_2 (plan=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### P8: `tui/src/read/flow.rs:337 (latest review)`

```sql
SELECT id, number, status, fingerprint, feedback, requested_at, decided_at FROM plan_reviews WHERE plan = 'quality' ORDER BY number DESC LIMIT 1;
```

```
`--SEARCH plan_reviews USING INDEX sqlite_autoindex_plan_reviews_2 (plan=?)
```

### P9: `tui/src/plan/mod.rs:163 -> select.rs:95 (the plan view's idle Event poll)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 5000 ORDER BY id LIMIT 500;
```

```
`--SEARCH events USING INTEGER PRIMARY KEY (rowid>?)
```

### K1: `tui/src/read/worker.rs:105 (attempt_row)`

```sql
SELECT a.tier, a.origin, a.status, a.outcome, a.cause, a.branch, a.worktree, a.worker_session, a.spec_path,
                a.submitted_sha, a.submitted_at, a.check_tail, a.merged_sha, a.nudged, a.stop_request, a.metrics,
                a.started_at, a.ended_at, j.title, j.status
           FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
          WHERE a.plan = 'quality' AND a.id = 'store-work-a1';
```

```
|--SEARCH a USING INDEX sqlite_autoindex_attempts_1 (plan=? AND id=?)
`--SEARCH j USING INDEX sqlite_autoindex_jobs_1 (plan=? AND key=?)
```

### K2: `tui/src/read/worker.rs:150`

```sql
SELECT id, question, status, answer, asked_at, answered_at, expired_at FROM escalations WHERE plan = 'quality' AND attempt = 'store-work-a1' ORDER BY number;
```

```
`--SEARCH escalations USING INDEX sqlite_autoindex_escalations_2 (plan=? AND attempt=?)
```

### K3: `tui/src/read/shared.rs:236 (sessions by id, worker view)`

```sql
SELECT s.id, s.role, s.context, s.plan, s.attempt, s.name, s.file, s.forked_from, s.tier,
       s.tmux_pane, s.state, s.state_label, s.state_at, s.created_at,
       (SELECT json_extract(e.data, '$.place') FROM events e
         WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1),
       EXISTS (SELECT 1 FROM plans p WHERE p.lead_session = s.id),
       EXISTS (SELECT 1 FROM contexts c WHERE c.lead_session = s.id),
       (SELECT e.id FROM events e WHERE e.session = s.id AND e.kind = 'session-state'
           AND json_extract(e.data, '$.to') = 'idle' ORDER BY e.id DESC LIMIT 1),
       (SELECT e.id FROM events e WHERE e.session = s.id AND e.kind = 'session-moved'
           AND json_extract(e.data, '$.place') = 'chat' ORDER BY e.id DESC LIMIT 1),
       s.archived_at
  FROM sessions s WHERE s.id = '0838fefc-1b82-45c4-b184-877d9070873c' ORDER BY s.created_at, s.id;
```

```
|--SEARCH s USING INDEX sqlite_autoindex_sessions_1 (id=?)
|--CORRELATED SCALAR SUBQUERY 1
|  `--SEARCH e USING INDEX events_session (session=?)
|--CORRELATED SCALAR SUBQUERY 2
|  `--SCAN p
|--CORRELATED SCALAR SUBQUERY 3
|  `--SCAN c
|--CORRELATED SCALAR SUBQUERY 4
|  `--SEARCH e USING INDEX events_session (session=?)
`--CORRELATED SCALAR SUBQUERY 5
   `--SEARCH e USING INDEX events_session (session=?)
```

### K4: `tui/src/cockpit.rs:184 (last_chat_lead)`

```sql
SELECT e.session FROM events e JOIN sessions s ON s.id = e.session
              WHERE e.kind = 'session-moved' AND json_extract(e.data, '$.place') = 'chat'
                AND s.role = 'lead' AND s.plan IS NOT NULL
              ORDER BY e.id DESC LIMIT 1;
```

```
|--SCAN e
`--SEARCH s USING INDEX sqlite_autoindex_sessions_1 (id=?)
```

### C1: `core/src/session/row.rs:89 (`session list`, filter 1: every Session)`

```sql
SELECT s.id, s.role, s.context, s.plan, s.attempt, s.name, s.file, s.forked_from, s.cwd, s.tier,
       s.tmux_pane, s.state, s.state_label, s.state_at, s.created_at,
       (SELECT json_extract(e.data, '$.place') FROM events e
         WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1),
       EXISTS (SELECT 1 FROM plans p WHERE p.lead_session = s.id),
       EXISTS (SELECT 1 FROM contexts c WHERE c.lead_session = s.id),
       (SELECT min(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-registered'),
       s.archived_at
  FROM sessions s WHERE 1 ORDER BY s.created_at, s.id;
```

```
|--SCAN s
|--CORRELATED SCALAR SUBQUERY 1
|  `--SEARCH e USING INDEX events_session (session=?)
|--CORRELATED SCALAR SUBQUERY 2
|  `--SCAN p
|--CORRELATED SCALAR SUBQUERY 3
|  `--SCAN c
|--CORRELATED SCALAR SUBQUERY 4
|  `--SEARCH e USING INDEX events_session (session=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### C2: `core/src/plan/row.rs:175 (`plan list`: PLAN_WITH_PR, filter 1)`

```sql
SELECT p.id, p.context, p.title, p.jira, r.key, r.repo, r.base, r.lead_branch, r.lead_worktree, p.phase, p.lead_session, r.pr_number, r.pr_url, r.pr_opened_at, r.all_merged_at, p.created_at, p.locked_at, p.done_at, p.archived_at FROM plans p LEFT JOIN prs r ON r.plan = p.id
    AND r.key = (SELECT key FROM prs WHERE plan = p.id ORDER BY created_txn, key LIMIT 1) WHERE 1 ORDER BY p.created_at, p.id;
```

```
|--SCAN p USING INDEX sqlite_autoindex_plans_1
|--SEARCH r USING INDEX sqlite_autoindex_prs_1 (plan=? AND key=?) LEFT-JOIN
|--CORRELATED SCALAR SUBQUERY 1
|  |--SEARCH prs USING INDEX sqlite_autoindex_prs_2 (plan=?)
|  `--USE TEMP B-TREE FOR ORDER BY
`--USE TEMP B-TREE FOR ORDER BY
```

### C3: `core/src/plan/show.rs:175 (`plan show` attempts)`

```sql
SELECT job, id, number, tier, origin, status, outcome, cause, worker_session, started_at, ended_at FROM attempts WHERE plan = 'quality' ORDER BY job, number;
```

```
`--SEARCH attempts USING INDEX sqlite_autoindex_attempts_2 (plan=?)
```

### C4: `core/src/context/mod.rs:342 (`context show` records)`

```sql
SELECT seq, id, kind, at, machine, by_session, by_plan, text, supersedes, question, plan_id, plan_machine, plan_jira FROM context_records WHERE context = 'building-cpd' ORDER BY seq;
```

```
|--SEARCH context_records USING INDEX sqlite_autoindex_context_records_1 (context=?)
`--USE TEMP B-TREE FOR ORDER BY
```

### C5: `core/src/context/mod.rs:376 (`context show` plan_repos, per Plan record)`

```sql
SELECT repo FROM prs WHERE plan = 'quality' AND status <> 'cancelled' ORDER BY key;
```

```
`--SEARCH prs USING INDEX sqlite_autoindex_prs_1 (plan=?)
```

### C6: `core/src/context/mod.rs:304 (`context list` open questions, per Context)`

```sql
SELECT count(*) FROM context_records q WHERE q.context = 'building-cpd' AND q.kind = 'question'
                   AND NOT EXISTS (SELECT 1 FROM context_records a
                                   WHERE a.context = q.context AND a.kind = 'answer' AND a.question = q.id);
```

```
|--SEARCH q USING INDEX sqlite_autoindex_context_records_1 (context=?)
`--CORRELATED SCALAR SUBQUERY 1
   `--SEARCH a USING INDEX sqlite_autoindex_context_records_1 (context=?)
```

### C7: `core/src/events/select.rs:95 (`events list --since 4990`, limit 500)`

```sql
SELECT id, at, kind, context, plan, job, attempt, session, text, data FROM events WHERE id > 4990 ORDER BY id LIMIT 500;
```

```
`--SEARCH events USING INTEGER PRIMARY KEY (rowid>?)
```

### C8: `core/src/plan/lead.rs:100`

```sql
SELECT json_extract(data, '$.window') FROM events WHERE session = '62b1c19b-62a5-492e-ae06-12ec8db845a0' AND kind = 'session-moved' ORDER BY id DESC LIMIT 1;
```

```
`--SEARCH events USING INDEX events_session (session=?)
```

### C9: `core/src/store/mod.rs:293 (`plan show` and `doctor`: store.tamper(), orphan history rows)`

```sql
SELECT h.id, h.txn FROM history h WHERE NOT EXISTS (SELECT 1 FROM txns t WHERE t.id = h.txn) ORDER BY h.id;
```

```
|--SCAN h
`--CORRELATED SCALAR SUBQUERY 1
   `--SEARCH t USING INTEGER PRIMARY KEY (rowid=?)
```
