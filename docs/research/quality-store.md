# Research: correctness and quality of the store and Events (`store-work`)

Plan `quality`, Job `store-work-2` (Attempt 1), base `main` at `b603b43`. Research only: no code changed.

## Summary

The store and Events are in good shape. Contract §4.4's DDL and the real database agree exactly (every table, index and hand-written trigger of §4.4 is byte-for-byte, whitespace aside, in the migrated copy of the live `cpd.db`; the 36 generated triggers list every column; the live schema hashes to `SCHEMA_HASHES[5]`). The 41 Event kinds, their scope columns and their `data` fields match §5.2 and the `events.kind` CHECK. `events::append` takes a `&WriteTxn`, so an Event outside the transaction of its change cannot be written. On the copy: 7043 `txns`, 5647 Events, 9389 `history` rows; Event and history ids never decrease against `txn`, every `events.at` equals its `txns.at`, and `PRAGMA integrity_check` and `foreign_key_check` are clean. No defect was found in the transaction, busy or migration code that is reachable from today's callers. What remains are three real gaps, all in guards that exist for one path but not another: `Store::write` never re-checks `user_version` (so an old process keeps writing after another migrated; `events follow` does check), a migration is committed without checking its result against the embedded schema hash, and no test reads back `foreign_keys` after the migrating open. Plus one data drift found in `history` (`pull_requests.created_at` is written twice with different values). Two items the User already decided are cited, not re-proposed (the `events follow` restart on a new install, hs-d140, is on `cross-repo/plumbing`; install and reload guards belong to `pi-pin`). Query latency is `sql-perf`'s: one line below.

## Findings

| id | class | what | size |
|---|---|---|---|
| store-W1 | Work | `Store::write` does not re-check `user_version`: a process that opened before a migration keeps writing to the newer schema | Job |
| store-W2 | Work | A migration is committed without comparing the schema it produced with `SCHEMA_HASHES`; a wrong result bricks every write (`store_tampered`) after the commit | Job |
| store-W3 | Work | No test reads `foreign_keys` / `legacy_alter_table` back after the migrating open or a failed migration | Job |
| store-W4 | Work | `events follow` on `main` never restarts on a new install (hs-d140's second half); only the store-error exit is in `main` | covered |
| store-W5 | Work | `pr open` inserts `pull_requests.created_at` as cpd's time, the next poll overwrites it with GitHub's (14 of 31 rows) | Job |
| store-W6 | Work | `Caller::current()` makes every write (and the first open of a new home) fail when the process's cwd was deleted | Job |
| store-W7 | Work | Non-store failures are all `store_failed` (stdout write, malformed Event, unreadable cwd); `SQLITE_LOCKED` is not `store_busy` | Job |
| store-W8 | Work | `setup_lock` waits for `locks/store.lock` without a limit and without a diagnostic | Job |
| store-G1 | Good | Event `data` is a free-form `json!` at 79 sites, checked by key names at run time only | Plan |
| store-G2 | Good | The store's tests sit in two crates and the schema-hash assertion is made three times; `schema.rs` is 80 % tests | Job |
| store-G3 | Good | `Store::read` hands out a full `Connection`; "reads cannot write" rests on the `cpd_txn()` trick | Job |
| store-G4 | Good | The trigger error strings are a second, untyped contract between the SQL files and `From<rusqlite::Error>` | Job |
| store-G5 | Good | The list of history tables is kept three ways | Job |
| store-G6 | Good | `Store::write` opens a `txns` row even when its closure changes nothing (27 empty transactions on the copy) | Job |
| store-G7 | Good | Docs: §5.4 "End" omits the failure exit that commands.md §11.2 and hs-d140 define; §4.4 still speaks of "the store Job" | Job |

Fast: nothing measured here. One pointer for `sql-perf`: on the copy 1584 of 7043 `txns` (22 %) are `pr poll`, 1462 of them write only `prs.pr_polled_at` (a `txns` row, a `history` row and WAL pages every poll).

## Work

### store-W1: `Store::write` does not re-check `user_version`

- **What.** `Store::open` refuses `store_too_new` once (`refuse_too_new`), but a process keeps its `Store` for its whole life (the engine, a driver, a Lead's adapter wrapper). `Store::write` begins `BEGIN IMMEDIATE`, inserts the `txns` row and runs the closure without reading `user_version` again. `events follow` does read it on every poll (`events/mod.rs:145`).
- **Where.** `core/src/store/mod.rs:254-281` (`write`), `core/src/store/mod.rs:152-157` (the one check at open); contrast `core/src/events/mod.rs:140-153`.
- **Evidence.** Read from the code: `grep -n "user_version\|refuse_too_new" core/src/store/mod.rs` gives uses at `migrate` (154), `migrate_in_txn` (174-175) and `current_schema`; none in `read`/`write`. The incident this guards is recorded in hs-n73 ("five adapters' followers … keep running after reporting store_too_new", schema 4 binary on a schema 5 database). Whether an old writer's INSERTs fail or silently succeed after a migration depends on the migration (a rebuilt table, new CHECKs for new Event kinds) and was not run: **unverified**. Migration 4's own text shows the class: it moves columns off `plans`, so an old binary's `UPDATE plans SET pr_number …` would fail with `store_failed` and no hint that the binary is old.
- **Impact.** After an install, every old process that is still alive writes through a schema it does not know: at best a `store_failed` with a SQLite message, at worst a write that the new binary's invariants did not expect. The window is from the install to the last old process's exit (hs-n73: the old followers were still running 37 minutes after the install).
- **Proposed fix.** Inside `write`, after `BEGIN IMMEDIATE` and before the `txns` insert, `PRAGMA user_version` (the lock is held, so it costs no extra I/O) and `refuse_too_new`. One test: open a `Store`, bump `user_version` from a second connection, `write` must return `store_too_new` and append nothing.
- **Size.** Job.
- **Overlap.** `pi-pin` (hs-n81, hs-d151, hs-d152) guards the install and the spawn of drivers (`install-cpd.sh` refuses an older build; drivers start from the recorded exe; `cpd --version` reports `store_schema`). Its diff of `core/src/store/mod.rs` adds `current_schema` only (`git diff origin/main...origin/cpd/pi-pin/cpd -- core/src/store/mod.rs`), no write-time check. This finding is the gap that remains for processes already running.

### store-W2: a migration commits without verifying its own result

- **What.** `migrate_in_txn` applies the migrations, checks foreign keys, sets `user_version` and commits. It never hashes `sqlite_schema` before the commit. If the result differs from `SCHEMA_HASHES[VERSION-1]`, `open` commits it, then `check_schema` (after the lock is dropped) records a mismatch and every later write is refused `store_tampered` ("a trigger was changed outside cpd"), a misleading message for a bad migration, on a database that can no longer be rolled back.
- **Where.** `core/src/store/mod.rs:202-215` (no hash before `tx.commit()`), `core/src/store/mod.rs:217-221` (`check_schema`, only at open, after `drop(setup)` at line 100).
- **Evidence.** Read from the code (`sed -n 196,222p core/src/store/mod.rs`). What does hold today: CI asserts the hash of every version on an empty database (`schema.rs:283`), of a migrated one against a fresh one (`schema.rs:1041`), and the real database passes: the SQL below re-computed the hash of the copy's `sqlite_schema` and it equals `SCHEMA_HASHES[5]`:

      python3 - $T/cpd.db   # json.dumps(rows, separators=(',',':')) of SELECT type,name,sql ... ORDER BY type,name, sha256
      a1b657a955520d2dd6dd71e143cee1fd8aba88fafc9284a7e8ab89dfc05c82af
      grep -n a1b657 core/src/store/schema.rs   ->   26:    "a1b657a955520d2dd6dd71e143cee1fd8aba88fafc9284a7e8ab89dfc05c82af",

  So the live database is fine; the gap is for the next migrations (the `manager` branch adds four: `0007`-`0010`, one of them imports files) and for a database whose old shape differs from the synthetic ones in the tests.
- **Impact.** A migration whose result differs on a real database (data-dependent DDL such as `fill_prs`, a hand-edited schema, a sqlite version that formats `sql` differently) commits and bricks writes instead of failing `migration_failed` with the database untouched. Fail loud, but before the one-way door.
- **Proposed fix.** After the foreign-key check, `schema_hash(&tx)` and compare with `SCHEMA_HASHES[VERSION-1]`; on mismatch return `migration_failed` with `expected`/`found` (rolls back). One test with a migration stub is not possible without a seam; test with `at_version`-style helper that pre-creates an extra object so the hash differs.
- **Size.** Job.
- **Overlap.** `manager` (migrations 0007-0010) will touch the same function (`git diff origin/main...origin/cpd/manager/cpd -- core/src/store/mod.rs` changes `migrate` / `migrate_in_txn` signatures); do this after it merges or as part of the rebase.

### store-W3: no test reads `foreign_keys` or `legacy_alter_table` back

- **What.** `Store::migrate` turns `foreign_keys` OFF and `legacy_alter_table` ON for the migrating transaction and restores them afterwards (`mod.rs:159-168`). Every other write of the process relies on `foreign_keys = ON` (§4.1) and on `legacy_alter_table = OFF`. No test checks either value on a live `Store`.
- **Where.** `core/src/store/mod.rs:159-168`; tests `cli/tests/store.rs`, `core/src/store/schema.rs`.
- **Evidence.**

      grep -rn "foreign_keys\|legacy_alter" --include=*.rs . | grep -v "^./target" | grep -v pragma_update
      ./core/src/store/mod.rs:147: (doc comment)
      ./core/src/store/schema.rs:105: (doc comment)

  Only `pragma_update` (setters, 12 hits) and two comments exist; no `pragma_query_value(... "foreign_keys" ...)`. The restore code is correct as read (it runs whether `migrated` is `Ok` or `Err`), so this is a missing pin, not a bug found.
- **Impact.** The process that migrates runs the rest of its command, the engine for its lifetime, with foreign keys off if the restore ever regresses; that would turn every missing parent into a silent write. Nothing would fail.
- **Proposed fix.** Two assertions: after `Store::open` on a version-1 database, `PRAGMA foreign_keys` = 1 and `legacy_alter_table` = 0 on `store.conn` (through a `store.read`); same after a `migration_failed` open (a failed `Store::open` drops its `Store`, so the failure case needs a `core` unit test that calls `Store::connect` and `migrate` directly).
- **Size.** Job (30 min).
- **Overlap.** none.

### store-W4: `events follow` never restarts on a new install on `main` (hs-d140, second half)

- **What.** hs-d140 (User accepted): "`cpd events follow` exits non-zero on any store error (`store_too_new` included) and re-execs itself when its executable path changes … so no follower outlives an install (hs-n73)". On `main` the first half holds: every `store.read(...)?` and the `user_version` check return `Err`, which `main` prints as the failure envelope with its exit code (commands.md §11.2). The second half is absent: a follower whose binary was replaced with the same schema runs forever on the deleted executable.
- **Where.** `core/src/events/mod.rs:112-182` (the loop; no `exe` handling).
- **Evidence.**

      grep -n "exe\|exec" core/src/events/mod.rs        # no output
      git diff origin/main...origin/cpd/cross-repo/plumbing -- core/src/events/mod.rs
        +    let exe = Exe::current()?;
        +        if exe.replaced() {
        +            return Err(exe.exec(restart_args(std::env::args_os().skip(1), cursor)));

  I did not run `events follow` (forbidden by the Spec); the exit behaviour above is from the code.
- **Impact.** The hs-n73 incident repeats for every install that does not change the schema.
- **Proposed fix.** None: `cross-repo` PR `plumbing` carries it (with `restart_args` and a unit test).
- **Size.** covered.
- **Overlap.** Plan `cross-repo`, PR `plumbing` (in dispatch); hs-d140, hs-n73. Two things to check in review there, both **unverified**: `exe.replaced()` is tested after the `until`/`reader_gone` checks and before the sleep, so a replaced exe is noticed at most one poll (`events.poll_ms`, 200 ms) late, which is fine; and the restart keeps `--until-idle`, whose `until` is then recomputed from the new cursor, so an `--until-idle` follower that is replaced mid-run would print Events committed after its first start. Scripts and tests use `--until-idle` for seconds, so this is theoretical.

### store-W5: `pull_requests.created_at` is written twice with different values

- **What.** §4.4: `pull_requests.created_at` is "GitHub's". `pr open` inserts the row (when no poll stored it first) with `created_at: pulls::txn_time(tx)`, cpd's own millisecond time. The next `pr poll` compares the polled row with the stored one, sees a different `created_at` (GitHub's has second precision) and `UPDATE`s it, bumping `changed_at`.
- **Where.** `core/src/pr/open.rs:68` (the insert's value), `core/src/pr/pulls.rs:170-190` (`upsert` updates `created_at`), `core/src/pr/pulls.rs:196-215` (`insert`). The gh query already reads `createdAt` (`core/src/pr/gh.rs:238`).
- **Evidence.**

      sqlite3 $T/cpd.db "select count(*) from history where tbl='pull_requests' and op='insert'"   ->  31
      sqlite3 $T/cpd.db "select count(*) from history where tbl='pull_requests' and op='update'
        and json_extract(before,'$.created_at') is not json_extract(after,'$.created_at')"          ->  14
      sqlite3 $T/cpd.db "select h.txn, json_extract(before,'$.created_at'), json_extract(after,'$.created_at') …"
      4946|2026-10-17T23:58:07.612Z|2026-10-17T23:58:06Z      (clayton-duarte/dotfiles#90)
      5322|2026-10-18T00:21:30.997Z|2026-10-18T00:21:30Z      (clayton-duarte/cpd#18)
      5749|2026-10-18T01:13:57.584Z|2026-10-18T01:13:56Z      (clayton-duarte/cpd#22)

  The "before" value is the `pr open` transaction's time (ms), the "after" is GitHub's (seconds, a second earlier).
- **Impact.** Low. The value is corrected by the first poll, 5 s later; until then `created_at` is wrong, and every `pr open` writes one avoidable `history` row. Whether the `found` value `pr open` gets from `gh` carries `createdAt` is **unverified**; the poll's query does.
- **Proposed fix.** In `pr open`, take `created_at` from `gh` (add `createdAt` to the `gh pr view/create` result), or leave the insert to the poll and only link; either way an upsert of an unchanged pull request writes nothing (what `pulls.rs:157-161` already promises).
- **Size.** Job.
- **Overlap.** Plan `pr-poller` (done, hs-d96/hs-d97); Plan `pr-view-sync` (grounding: "PR merges not showing in the Dispatch column") touches the same sync, check there first.

### store-W6: a deleted cwd makes every write fail

- **What.** `Caller::current()` calls `std::env::current_dir()` before every write transaction (it fills `txns.cwd NOT NULL`) and returns `store_failed` "The current directory is unreadable" when it fails. It is also called in `migrate_in_txn`, so the first open of a new home fails and leaves a half-created `cpd.db` (WAL, `application_id` 0, version 0).
- **Where.** `core/src/store/mod.rs:170` (`migrate_in_txn`), `mod.rs:268` (`write`), `mod.rs:372-386` (`Caller::current`).
- **Evidence.** On a scratch `CPD_HOME` (config copied from the copy, empty database), cwd deleted:

      D=$(mktemp -d); cd $D && rmdir $D && CPD_HOME=$H timeout 60 target/release/cpd events list
      {"ok":false,"error":{"code":"store_failed","message":"The current directory is unreadable: No such file or directory (os error 2).","details":{"path":"/tmp/sw3-h-of3t/cpd.db"}}}
      ls $H  ->  config.toml cpd.db locks      (cpd.db created, version 0)
      (from an existing directory) cpd events list --limit 1  ->  {"ok":true,"events":[],"next":0}   # the next open migrates normally

  The write path is read from the code, not run (a write on the copy is forbidden by the Spec): **unverified by execution**. Drivers and the engine run with `cwd = $CPD_HOME` (`driver/start.rs:280`, `watch/pr.rs:92`), so they are not exposed; a Worker or Lead command run in a worktree that was removed under it is (dev-speed's worktree cleanup at PR merge makes that more likely).
- **Impact.** Low: a failure with a clear message and nothing half-written in a table; a reproducible, idempotent state (the next open repairs it). The audit column is the only reason to need the cwd.
- **Proposed fix.** Record `cwd` as `""`/`"(deleted)"` when unreadable (the column is information, not a key), or fail with a code that names it (`cwd_unreadable`).
- **Size.** Job (small).
- **Overlap.** `dev-speed` (worktree cleanup at PR merge) widens the exposure.

### store-W7: non-store failures are `store_failed`; `SQLITE_LOCKED` is not `store_busy`

- **What.** Three failures that are not the database's carry `store_failed`: a stdout write error other than EPIPE in `events follow`, a malformed Event (`append::malformed`, "a bug in the calling command", documented in the code), and the unreadable cwd (W6). The adapters and the User see "store failed" and exit 1 for all. `From<rusqlite::Error>` maps `SQLITE_BUSY` only; `SQLITE_LOCKED` (`DatabaseLocked`) falls through to `store_failed`.
- **Where.** `core/src/events/mod.rs:163`, `core/src/events/append.rs:78-80`, `core/src/store/mod.rs:372-386`, `core/src/store/mod.rs:427-470`.
- **Evidence.** Read from the code (the `match` at `mod.rs:434-466` has the arms `DatabaseBusy`, four `ConstraintViolation` message tests and the default). Whether `SQLITE_LOCKED` can occur here (WAL, no shared cache) is **unverified**; I believe not on a plain connection.
- **Impact.** Low. Triage reads the wrong subsystem, and an `append` bug (a missing `data` field) rolls back a whole state change and reports "store failed" (exit 1); the contract says exit 1 means "something unexpected", which is right, but the code does not say which part.
- **Proposed fix.** One `internal`-style code for "a bug in cpd" (malformed Event) and plain `io` errors for stdout, leaving `store_failed` for SQLite; map `DatabaseLocked` with `DatabaseBusy`. Needs an addition to the error table of contract §1.2 and commands.md, so decide first whether the User wants a new code.
- **Size.** Job.
- **Overlap.** none.

### store-W8: `setup_lock` waits forever and says nothing

- **What.** Every `Store::open` takes `locks/store.lock` exclusively for its setup and "waits for it without a limit". The holder's work is bounded by `busy_ms` per statement, not in total; with `manager`'s migration 0007 (imports Context stores from files, inside the lock) the holder's time grows with the size of the stores.
- **Where.** `core/src/store/mod.rs:305-326`.
- **Evidence.** Read from the code (the `loop` at 319 retries only on `EINTR`). A hung holder (SIGSTOP, a stalled filesystem) would hold every other `cpd` command: **unverified** (not run; `flock` releases when the holder dies, which covers the crash case).
- **Impact.** Low, until a holder hangs; then every `cpd` command, including `doctor`, hangs with no message.
- **Proposed fix.** `LockExclusive | NonBlock` with a deadline loop (e.g. `store_busy_ms` plus the longest migration) and a `lock_timeout` error (the code exists, exit 5) naming the lock file.
- **Size.** Job.
- **Overlap.** `manager` (migration 0007).

## Good

### store-G1: Event `data` is free-form JSON, checked by key name at run time

- **What.** 79 mentions of `events::append`/`NewEvent` in `core/src`, 29 of them build `data` with `json!` inline; `append` validates the key set against `kind.rs`'s `Spec` (names only, not value types) and rejects a mismatch with `store_failed`, rolling back the command's state change. A typo in a field name on a path no test reaches fails in production, after the state change ran.
- **Where.** `core/src/events/append.rs:89-173`, `core/src/events/kind.rs:96-148`.
- **Evidence.** `grep -rn "events::append\|events::NewEvent\|NewEvent::new" core/src --include=*.rs | wc -l` -> 79; `grep -rn "NewEvent::new" … | grep -c "json!"` -> 29. The kind table itself is right: a script compared §5.2's table with `kind.rs` and the DDL: 41 kinds, same names and order in all three, `data` fields identical except `plan-phase`, where code allows `jobs`, `pr`, `prs` as optional and the contract's prose lists them per phase (consistent). `cli/tests/events.rs:282` appends every kind but with synthetic data, not the callers'.
- **Impact.** Unverified that any call site is wrong; the design makes it possible without a compile error.
- **Proposed fix.** One constructor per kind (`Event::job_merged(plan, job, attempt, sha, tier)`) so the data shape is a signature; `Spec` becomes the test oracle. Do it kind by kind as call sites are touched.
- **Size.** Plan.
- **Overlap.** none.

### store-G2: the store's tests live in two crates and repeat one assertion

- **What.** `core/src/store/schema.rs` is 1280 lines, 253 of them code (the tests start at line 254). The same area is tested again in `cli/tests/store.rs` (1127 lines), which imports `cpd_core::store`, not the binary. The fresh-database schema hash is asserted three times: `schema.rs:283` (every version), `schema.rs:1041` (`a_migrated_database_has_the_schema_of_a_new_one`) and `cli/tests/store.rs:657`.
- **Where.** As above.
- **Evidence.** `grep -n "^mod tests" core/src/store/schema.rs` -> 255; `wc -l cli/tests/store.rs core/src/store/schema.rs` -> 1127, 1280; `grep -rn schema_hash cli/tests/store.rs core/src/store/schema.rs`. The cost of the first one is `tests-build`'s (it is 3.3 s of the core lib's 7.8 s, `docs/research/quality-tests-build.md:251`).
- **Impact.** A reader looking for "what pins the schema" has three places; a new migration touches `schema.rs` tests and `cli/tests/store.rs`.
- **Proposed fix.** Move `schema.rs`'s tests to `core/src/store/schema_tests.rs` (or `core/tests/`), leave `cli/tests/store.rs` for what needs a second process (the writers, `--version`), delete the duplicate at `cli/tests/store.rs:657`.
- **Size.** Job.
- **Overlap.** `tests-build` (suite time and structure).

### store-G3: `Store::read` hands out a full `Connection`

- **What.** `read` gives `&Connection`, and `WriteTxn` derefs to `Connection`, so any SQL can run in either. "A read cannot write" holds because every table's triggers call `cpd_txn()`, which errors outside `write` (`a_read_transaction_cannot_write`, `cli/tests/store.rs:543`). Statements that fire no trigger are not stopped: DDL, `TEMP` tables, `PRAGMA`s, and on a `WriteTxn` a raw `COMMIT`/`ROLLBACK`.
- **Where.** `core/src/store/mod.rs:44-62`, `mod.rs:232-245`.
- **Evidence.** Read from the code and the test; the escape routes were **not run** (unverified).
- **Impact.** None found at present (all callers use plain `SELECT`/`INSERT`/`UPDATE`). Explicit-over-implicit: the guard is an implementation side effect.
- **Proposed fix.** `PRAGMA query_only = ON` for the duration of `read` (and off after), and the message "outside a cpd write transaction" becomes SQLite's own `attempt to write a readonly database`; a `WriteTxn` wrapper exposing `execute`/`prepare`/`query_row` and not `Deref` would remove the raw `COMMIT`.
- **Size.** Job.
- **Overlap.** none.

### store-G4: trigger error strings are an untyped contract

- **What.** `job_locked`, `illegal_transition`, `pr_locked`, `needs_other_pr` appear as string literals in the SQL files (`0001.sql`, `0004.sql`, `0005.sql`) and are matched by `message == "…"` in four near-identical arms of `From<rusqlite::Error>`. `append_only: …` and `txns.id must be cpd_txn()` are not mapped and surface as `store_failed` (by design; contract §4.3 speaks of "its `append_only` error").
- **Where.** `core/src/store/mod.rs:427-470`.
- **Evidence.** `grep -rn "RAISE(ABORT" core/src/store/migrations | sed 's/.*RAISE/RAISE/' | sort | uniq -c`; tests pin all four (`cli/tests/store.rs:371, 398`, `schema.rs` trigger tests), so a rename would fail a test, not ship.
- **Impact.** Low. A table (`const TRIGGER_ERRORS: [(&str, ErrorCode, &str); 4]`) replaces four arms and gives the SQL authors one list to extend.
- **Proposed fix.** As written; or leave, the tests already guard it.
- **Size.** Job (small).
- **Overlap.** none.

### store-G5: the history tables are listed three ways

- **What.** `HISTORY_TABLES` (current, used only by tests), `VERSION_1_HISTORY_TABLES` (frozen), and the explicit `create_history_triggers` calls in each migration. A new table needs all three; a test (`cli/tests/store.rs:213-250`) compares the list with the database, so a miss fails there.
- **Where.** `core/src/store/schema.rs:40-67`, `schema.rs:70-146`.
- **Evidence.** `grep -rn HISTORY_TABLES --include=*.rs .` -> `store/mod.rs:28` (re-export), `schema.rs:40`, `cli/tests/store.rs:15, 228, 282` and nothing in production.
- **Impact.** Low: a pub constant that only tests read.
- **Proposed fix.** Move it into the tests, or derive the expectation from `sqlite_schema` (every table but `txns`/`history`/`events`).
- **Size.** Job (small).
- **Overlap.** none.

### store-G6: empty write transactions

- **What.** `Store::write` always inserts a `txns` row, even when the closure then changes nothing. On the copy, 27 of 7043 `txns` have neither a `history` row nor an Event: 21 `attempt run`, 2 `watch`, 4 `migrate` (the migrations without data, expected).
- **Where.** `core/src/store/mod.rs:268-278`.
- **Evidence.**

      sqlite3 $T/cpd.db "select command, count(*), min(id), max(id) from txns t where not exists (select 1 from history h where h.txn=t.id) and not exists (select 1 from events e where e.txn=t.id) group by 1"
      attempt run|21|4493|6841
      migrate|4|1|4195
      watch|2|1918|5139

- **Impact.** Audit noise and a WAL write per call; 0.3 % of the rows. Not a correctness issue (§4.3 requires the row for every transaction).
- **Proposed fix.** None needed unless the 21 `attempt run` ones are a driver step that opens a write only to read; worth a look by whoever works on the driver (`driver/run.rs`).
- **Size.** Job (small).
- **Overlap.** none.

### store-G7: documentation drift

- **What.** (a) contract §5.4 "End" says the follower runs "until killed or until stdout closes (EPIPE), and then exits 0", with no failure exit; commands.md §11.2 documents the failure line and exit, and hs-d140 adds the restart on a new install. (b) contract §4.4 (line 1054) says "The store Job generates these triggers" and §4.5 migration 4 step 1 (line 1202) says "unverified: the store Job must test the whole procedure": build instructions that are now history (the tests exist). (c) `Store::read`'s doc says "Writes inside it fail: `cpd_txn()` refuses outside `Store::write`", which is true only for tables with triggers (G3).
- **Where.** `docs/contract.md` §5.4 "End" (see Evidence), `docs/contract.md:1054` and `:1202`, `core/src/store/mod.rs:232-235`.
- **Evidence.** `grep -n '^- \*\*End:\*\*' docs/contract.md`; `grep -n "store Job" docs/contract.md` -> lines 1054 and 1202.
- **Impact.** Low; a reader of the contract alone would think follow never fails.
- **Proposed fix.** One sentence in §5.4 pointing to commands.md §11.2, after `plumbing` merges (it changes the same behaviour, W4).
- **Size.** Job (small).
- **Overlap.** `cross-repo` (`plumbing`) updates the docs of follow; wait for it.

## Already good, keep

- **The schema is the contract.** `python3` comparison of §4.4's `sql` blocks with the copy's `sqlite_schema`: 52 contract objects, 0 missing, 0 different (a negative control with a changed `NOT NULL` is reported as one difference). The 36 other objects are the generated `_hist_ins/_hist_upd/_no_delete/_hist_del` triggers, and each lists every column of its table. `cli/tests/store.rs:191` (`the_schema_is_contract_4_4_as_written`) pins this in CI.
- **Events cannot leave their transaction.** `append(tx: &WriteTxn, …)`; the INSERT reads `txns` by `cpd_txn()`, so `txn` and `at` come from the transaction. On the copy `events.at = txns.at` for all 5647 rows and `txn` never decreases with `id` (events or history).
- **Migrations are one transaction, restored on every path, serialised.** `BEGIN IMMEDIATE`, re-read of `user_version` under the lock, foreign-key check before commit, pragma restore on success and failure, `locks/store.lock` for the WAL switch. Tests: failure of migration 1 rolls back (`store.rs:702`), an FK break in 2→6 changes nothing (`schema.rs:1053`), four processes opening a new home migrate once (`store.rs`), every old version migrates with data (v1, v2, v3, v4, v5).
- **The renumbering history (hs-d68, hs-n69) is guarded by hash.** Two branches adding the same migration number conflict in `MIGRATIONS`, `SCHEMA_HASHES` and the file name; the test of every version's hash catches an edited migration. Six `migrate` rows exist on the copy (txn 1, 245, 1689, 2227, 4195, 4825), one per version bump.
- **`events follow`** is written conservatively: `data_version` is read before the fetch, so a commit between them is refetched, never missed; a fresh read transaction per poll; `cursor_ahead` for a foreign cursor; EPIPE and `POLLHUP|POLLERR` end it (the G1 check); a store error exits non-zero.
- **Error mapping** for the four trigger refusals and `store_busy` (with `busy_ms` and `path`) is pinned by tests, including two real processes (`a_writer_that_cannot_get_the_lock_in_time_is_store_busy`).
- **Test hygiene.** `KillOnDrop` in `cli/tests/store.rs` plus its own test (`a_guarded_child_is_gone_after_a_panic`); `a_failed_write_rolls_back_everything_including_its_txns_row`; no sleeps used as synchronisation (the writers signal through a file, `wait_started`).
- **Bundled SQLite is 3.53.2** (`libsqlite3-sys-0.38.2`), above the contract's 3.45.

## Method

Everything below ran from the worktree `/home/clayton/projects/cpd.quality--store-work-2-a1`, each command under `timeout`. No command touched the live `~/.cpd`, its engine or `-L cpd`.

- **Copy.** `T=$(mktemp -d /tmp/sw3-XXXX); sqlite3 ~/.cpd/cpd.db ".backup $T/cpd.db"; cp ~/.cpd/config.toml $T/; cp -r ~/.cpd/contexts $T/` then `[tmux] socket = "quality-<pid>"` appended to `$T/config.toml`. No tmux server was started.
- **Build.** `timeout 900 scripts/cargo.sh build --release -p cpd` (1 m 54 s). Binary `target/release/cpd`, run as `CPD_HOME=$T`: `--version` (`schema 6, store_schema 6`), `events list`, `context show --context building-cpd` (read the Context records hs-n73, hs-d140, hs-n69, hs-d68, hs-n12, hs-d63, hs-n27, hs-n35, hs-d139, hs-n60), `doctor` (on the copy; its `pi_adapter` check fails because the copy has no adapter, unrelated).
- **Schema vs contract.** A python script took the `sql` blocks of §4.4, stripped comments and whitespace, and compared each `CREATE TABLE/INDEX/TRIGGER` with `sqlite_schema` of the copy: 52 objects in the contract, 88 in the database (16 tables, 8 indexes, 64 triggers), 0 missing, 0 different; the remaining 36 triggers are the generated ones, each checked for every column of its table. `sqlite3 $T/cpd.db "pragma user_version; pragma application_id; pragma journal_mode; pragma integrity_check; pragma foreign_key_check"` -> `6`, `1668310066`, `wal`, `ok`, no rows. The hash of the copy was recomputed in python (same JSON as `schema_hash`) and compared with `SCHEMA_HASHES[5]` (equal).
- **Events vs §5.2.** A script compared kind names and order (contract table, `kind.rs`, DDL CHECK: 41 each) and the `data` required/optional field names of every kind (one difference, `plan-phase`, explained in G1).
- **History queries** (copy, `sqlite3`, each in under 2 s): `txns`/`events`/`history` counts and sequences, monotonicity of `txn` by `id` with a window function, `events.at` vs `txns.at`, transactions without Events grouped by command and changed column (every one is data-only by §5.1: `pr_polled_at`, `base_sha`, `file`, `driver_pid`, `pull_requests` poll fields, and five `stop_request` writes by the engine's E2, which are requests whose end carries the Event), transactions without history or Events.
- **Scratch home** for W6: an empty `CPD_HOME` with the config copied, cwd deleted, `events list`.
- **Not run, by the Spec:** `events follow`, `watch`, `tui`, `session run`, `attempt run`, any write command, `cargo test`. Every claim about those paths is from the code and says so.
- **In-flight Plans read:** `git diff origin/main...origin/cpd/{cross-repo/plumbing,pi-pin/cpd,manager/cpd} -- core/src/store core/src/events` (plumbing: `events/mod.rs` +43; pi-pin: `store/mod.rs` +44, `current_schema` only; manager: migrations 0007-0010 and `store/mod.rs`/`schema.rs`, a `home` argument to `migrate` and `archive_old_stores` after the commit).
- **Teardown.** The copy, the scratch homes, `/tmp/sw3-*`, and the left-over `/tmp/qs-*` and `/tmp/sw-*` of earlier Attempts were deleted before submit.
