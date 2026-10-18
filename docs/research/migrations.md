# Named migrations and an additive compatibility window

Design for Job `migrations-design` of Plan `dev-speed`, for the User's approval before any code (hs-a4, the User's decisions of 2026-10-18 through the Lead: named migrations applied by name, option b; an additive window, option a). Sources: the code at `da3b80f` (this worktree's base, `main`), `docs/contract.md`, `docs/commands.md`, `docs/research/binaries.md`, the Context `building-cpd` (`cpd context show`, read 2026-10-18), and Plan manager's branch `cpd/manager/cpd`, which has migrations `0007`–`0010`. Anything I did not check is labelled *unverified* or *inferred*. Line numbers are at `da3b80f`.

## Summary

1. A migration is a file `core/src/store/migrations/<YYYYMMDDTHHMM>-<slug>.sql`. Its name is its identity, and its first line says `additive` or `breaking after <n>`. A build script makes the list from the directory, so two Plans never edit the same line to add one.
2. The store records each applied migration in a new table `schema_migrations`: name, compat, checksum, order and the schema hash after it. `PRAGMA user_version` is frozen at 1000. Every binary from before the conversion then refuses with the `store_too_new` code it already has.
3. Lists that migrations extend become lookup tables that migrations only insert into: `event_kinds` and `record_kinds`, referenced by foreign keys. A table's history triggers become derived: after migrating, the runner regenerates them from the table's columns, and the tables that get them are listed in a third lookup table, `history_tables`.
4. Order: additive migrations commute and may be applied in any order. A breaking migration is a barrier: it must sort after every migration that exists when it lands. A build test enforces this through its `after <n>` count, and the runner refuses to apply one out of order.
5. The schema check compares `sqlite_schema` with the hash this store last recorded, before any migration runs. The embedded per-version hashes stay only to convert legacy stores.
6. Window: a binary opens the store when it knows the store's newest breaking migration (its floor). It may read and write a store that also has additive migrations it does not know.
7. One function decides open or refuse. `Store::open`, `cpd --version` (new `store.opens`), the install guard, the reload guard, `events follow` and `doctor` all use its answer; none of them compares numbers itself.
8. What stays coupled: one store per machine, breaking migrations still serialised across Plans, and shared kind lists in code and docs. An "additive" migration can still break an old reader that parses values into a closed enum. Details in "What stays coupled".
9. Six implementation Jobs (contract, runner and conversion, window, additive check, kind tables, guards). Nine open questions, the first about Plan manager's in-flight `0007`–`0010`.

## Today (what the design replaces)

| Fact | Where |
|---|---|
| Migrations are Rust functions in a fixed array; index `i` takes the schema to version `i + 1` | `core/src/store/schema.rs:15-16` (`MIGRATIONS`), `:70-147` (`migration_1..6`) |
| One embedded schema hash per version; the check compares with `SCHEMA_HASHES[VERSION - 1]` | `schema.rs:18-29`, `core/src/store/mod.rs:217-221` (`check_schema`) |
| `VERSION` is the number of migrations; the store's version is `PRAGMA user_version`, set once at the end | `schema.rs:31-32`, `mod.rs:212`, `mod.rs:345-347` |
| Open migrates first, then checks the hash | `mod.rs:95-103` |
| Missing migrations are applied by skipping the first `user_version` entries | `mod.rs:192` |
| A newer store is refused with `store_too_new` | `mod.rs:359-369` (`refuse_too_new`), called at `mod.rs:154` and `:175` |
| `events follow` re-reads `user_version` on every poll and refuses with its own copy of that comparison | `core/src/events/mod.rs:139-151` |
| `cpd --version` reports `schema` = `VERSION` and `store_schema` = `user_version` (read-only) | `cli/src/cli.rs:103-109`, `mod.rs:330-343` (`current_schema`) |
| The reload guard parses those two integers and compares them | `core/src/host/exe.rs:199-207` |
| The install guard parses them with `sed` and compares them | `scripts/install-cpd.sh:18-33` |
| `events.kind` is a `CHECK (kind IN (…))`, rebuilt in full by every migration that adds a kind | `migrations/0001.sql:246`, `0003.sql:9-47`, `0004.sql:119-151`; manager's `0009.sql:8-41` rebuilds it again for `session-message` |
| `context_records.kind` is a `CHECK (kind IN (…))`, rebuilt by migration 4 | `0001.sql:48`, `0004.sql:153-180`; manager's `0010` rebuilds it again for `brief` (hs-n88) |
| History triggers are generated from a table's columns; the tables are listed in `HISTORY_TABLES`; every migration that changes a table's columns drops them and creates them again | `schema.rs:34-54`, `:187-233`, `:80-98`, `:108-121`, `:139-147` |
| Reads of Events keep `kind` as a string; Context record kinds are a closed enum | `core/src/events/select.rs:17`; `core/src/ids.rs:279` |
| The contract's rules | `docs/contract.md:690` (§4.3 schema check), `:1174-1183` (§4.5), `:570` (§3.4 reload check), `:624` (§3.7 install guard), `:1524` (§6.7 reload check); `docs/commands.md:56` (`--version`), `:75` (codes) |

Two SQLite facts the design relies on. I checked both with Python's `sqlite3` 3.45.1, not with the rusqlite bundle, so for that bundle they are *unverified*:
- `ALTER TABLE … ADD COLUMN` appends the column's text to the table's `sql` in `sqlite_schema`. Two migrations that each add a column to the same table, applied in the two orders, leave different `sql` text and so different hashes.
- A `REFERENCES lookup(name)` column refuses an unknown value under `PRAGMA foreign_keys = ON`, which every connection sets (`mod.rs:119`). `PRAGMA schema_version` changes on every schema change.

## The design, as numbered rules

### N. Names and declarations

- **N1. Name.** `<YYYYMMDDTHHMM>-<slug>`: the UTC minute the author created it, then a slug `[a-z][a-z0-9-]{0,39}`, e.g. `20261019T0905-plan-pins`. The file is `core/src/store/migrations/<name>.sql`. Names order lexicographically, which is chronological. A build test refuses any other shape.
- **N2. Legacy names.** The migrations that exist at the conversion keep their numbers as a prefix: `0001-initial`, `0002-cockpit-parity`, `0003-archive`, `0004-cross-repo`, `0005-plan-reopen`, `0006-pr-poller` (and `0007`–`0010` if Plan manager lands first, Q1). `0` sorts before `2`, so the legacy migrations always come first. The legacy list is a frozen constant (`LEGACY`). Each entry keeps its Rust function, and its `SCHEMA_HASHES` entry is used only by the conversion (C2).
- **N3. Declaration.** The first line of every new migration is one of:

  ```sql
  -- cpd-migration: additive
  -- cpd-migration: breaking after 7
  ```

  `breaking after <n>` means that exactly `n` migrations sort before this one: the ones its author wrote it against. The build parses the header, and a missing or malformed header fails the build.
- **N4. The list is the directory.** A `core/build.rs` lists `migrations/*.sql`, sorts the names and generates the array of `(name, compat, sql)`. Adding a migration therefore adds one file and edits no shared line, unlike `schema.rs:16` today. A migration that needs work SQL cannot do, such as manager's `0007` importing files from `$CPD_HOME`, gets a `<name>.rs` next to its `.sql`, with `pub fn run(conn, home)`, which `build.rs` wires in. The legacy six keep their functions in `schema.rs`.
- **N5. Frozen once merged.** A migration file on `main` is never edited, renamed or deleted. Two checks enforce this:
  - The runtime checksum check (R3) catches an edit on any store that has applied the migration.
  - A Gate check fails when the diff against `origin/main` modifies or deletes a file in `migrations/` (`git diff --diff-filter=MDR origin/main...HEAD -- core/src/store/migrations/`). It belongs in `.config/wt.toml`, which a Worker may not edit, so it is the Lead's (Job split, J4).

### R. What the store records

- **R1.** A new table, created by the runner (not by a migration), in the same transaction as the first migration after the legacy prefix:

  ```sql
  CREATE TABLE schema_migrations (
    name        TEXT    PRIMARY KEY CHECK (length(name) BETWEEN 6 AND 54),
    compat      TEXT    NOT NULL CHECK (compat IN ('additive', 'breaking')),
    checksum    TEXT    NOT NULL CHECK (length(checksum) = 64),   -- sha256 of the .sql text
    seq         INTEGER NOT NULL UNIQUE CHECK (seq >= 1),        -- order of application on this store
    txn         INTEGER NOT NULL REFERENCES txns(id),            -- the migrating transaction
    schema_hash TEXT    NOT NULL CHECK (length(schema_hash) = 64), -- §4.3 hash after that transaction
    converted   INTEGER NOT NULL CHECK (converted IN (0, 1))     -- 1: a legacy migration recorded by the conversion
  ) STRICT;
  ```

  It has three triggers: `schema_migrations_own` (`txn` must be `cpd_txn()`, like `events_own`), `_no_update` and `_no_delete`. So the `sqlite3` shell cannot write it (§4.3).
- **R2.** `PRAGMA user_version` is set to **1000** by the conversion and never changes again. It now means only "this store uses named migrations". Every integer-era binary has `VERSION` ≤ 10, so it refuses such a store with its existing `store_too_new` (`mod.rs:359`), and its `--version` reports `store_schema: 1000`.
- **R3. Checksum.** For every applied migration whose name the binary knows, the recorded `checksum` must equal the sha256 of the binary's embedded text. Otherwise the binary refuses with the new code `migration_changed` (exit 1), whose details give the name and both checksums. This replaces what the per-version hash gave us today: a store that applied one text cannot silently meet another text under the same name.
- **R4. The floor** ("the oldest schema allowed to open it") is derived, not stored twice: it is the `breaking` row with the highest `seq`. Rule O3 guarantees that it is also the breaking row with the latest name.

### K. Lists that migrations extend (problem 1)

- **K1. Lookup tables.** The runner's first named migration (`kind-tables`, a breaking migration, J5) creates:

  ```sql
  CREATE TABLE event_kinds    (name TEXT PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 40)) STRICT, WITHOUT ROWID;
  CREATE TABLE record_kinds   (name TEXT PRIMARY KEY CHECK (length(name) BETWEEN 1 AND 40)) STRICT, WITHOUT ROWID;
  CREATE TABLE history_tables (name TEXT PRIMARY KEY, no_delete INTEGER NOT NULL CHECK (no_delete IN (0, 1))) STRICT, WITHOUT ROWID;
  ```

  It fills them with today's kinds (`0004.sql:123-135`, `:157`) and today's `HISTORY_TABLES` with their `no_delete` flags (`schema.rs:40-54`; `false` for `dependencies`, `pr_needs` and `pull_requests`). It rebuilds `events` and `context_records` once more, the last time either is rebuilt for a kind. In the rebuilt tables `kind TEXT NOT NULL REFERENCES event_kinds(name)`, resp. `REFERENCES record_kinds(name)`, replaces the `CHECK`. The other checks of `context_records` stay, since they name specific kinds (`0004.sql:169-171`).
- **K2. Adding a kind is one insert,** in an additive migration: `INSERT INTO event_kinds (name) VALUES ('session-message');`. Two such migrations commute, and neither can drop the other's kind, because neither rewrites the list. The three lookup tables are themselves listed in `history_tables` with `no_delete = 1`. So their rows are append-only and an insert from the `sqlite3` shell fails on `cpd_txn()`, the same protection as every other table (§4.3).
- **K3. What stays a `CHECK`.** State-machine columns stay checks: `plans.phase`, `jobs.status`, `prs.status`, `attempts.status`, `sessions.role` and `state`, and the transition triggers. A new state changes what old code must handle, so changing one is a breaking migration by definition. Only lists that old binaries can ignore become lookup tables. Whether `attempts.cause` (`0001.sql:161`) should also become one is Q3.
- **K4. History triggers are derived** (the `HISTORY_TABLES` trap):
  - **K4a.** A migration never *creates* a generated `<t>_hist_ins`, `<t>_hist_upd` or `<t>_no_delete`. It may *drop* them, which SQLite needs before `DROP COLUMN` (manager's `0007.sql:1-8` does exactly that).
  - **K4b.** After the last migration of a migrating transaction, and before recording the hash, the runner **reconciles**: for each row of `history_tables` it generates the SQL from the table's current columns (`schema.rs:196-233`, unchanged). It keeps a trigger whose `sqlite_schema.sql` is identical to that SQL, and drops and recreates any trigger that differs or is missing. The result depends only on the final columns, so it is the same in any order of application.
  - **K4c.** A migration that creates a table that needs history inserts its row into `history_tables`. The Rust constant `HISTORY_TABLES` goes away, and `cli/tests/store.rs:228` and `:282` read the table instead.
  - **K4d.** The generator's output is part of the schema. Changing `history_triggers` changes every store's triggers, so it ships only together with a breaking migration.
  - **K4e.** The hand-written delete-history triggers (`dependencies_hist_del`, `pr_needs_hist_del`, `pull_requests_hist_del`, `pull_requests_owned`) stay owned by their migrations.
  - **K4f.** Dropping a table listed in `history_tables` is not supported until it is needed (YAGNI). The reconcile fails loudly (`migration_failed`) when a listed table is missing.
- **K5. Rebuilt tables** are the other trap: a rebuild copies only the columns its author knew. Every rebuild (`CREATE <t>_new`, copy, `DROP`, `RENAME`) is **breaking**, and rule O3 makes its author see every migration that sorts before it.

### C. Classification and the additive check (problem 4, first half)

- **A1. Additive** means that a binary that does not know the migration can still read and write the store correctly at the SQL level. Every statement must be one of:
  - `CREATE TABLE`, plus an insert into `history_tables` for it;
  - `CREATE INDEX` that is not `UNIQUE` on an existing table (any index on a new table);
  - `CREATE TRIGGER` on a table created by the same migration;
  - `ALTER TABLE <existing> ADD COLUMN` that is nullable, or `NOT NULL` with a constant `DEFAULT` (a `CHECK` or `REFERENCES` on the new column is allowed, since a null passes both);
  - `INSERT` into `event_kinds`, `record_kinds` or `history_tables`, or into a table the migration created.

  Anything else is **breaking**: a rebuild, `DROP`, `RENAME`, `UPDATE` or `DELETE` of existing rows, or a new or changed trigger or unique index on an existing table. Migration 5's relaxed triggers (`0005.sql`) would be breaking under this rule, though old code would survive them (Q7).
- **A2. The additive test** (`cargo test`, so the quick check's build and the full Gate both run it) runs every migration declared `additive` against the name-ordered prefix before it, and asserts:
  1. **The statement kinds.** An SQLite authorizer callback on the connection allows only the actions of A1: `SQLITE_CREATE_TABLE`, `_CREATE_INDEX`, `_CREATE_TRIGGER`, `_ALTER_TABLE`, `_INSERT` (into the allowed tables), `_READ` and `_SELECT`. It denies `_UPDATE`, `_DELETE`, every `_DROP_*` and `_PRAGMA`. *Unverified:* that rusqlite 0.40 exposes the authorizer. That needs its `hooks` feature (`Cargo.toml:17` enables only `bundled` and `functions`), a deviation whose reason is this check. Without it, the catalog diff below still catches every schema change, and only `UPDATE`/`DELETE` of rows would go unchecked.
  2. **The catalog diff.** Every `sqlite_schema` row that existed before is unchanged after, except that the `sql` of a table may only have grown by `ADD COLUMN`. For each existing table, its old `pragma_table_xinfo` rows are an unchanged prefix of the new ones, and each new column has `notnull = 0` or a non-null `dflt_value`. Every new trigger is on a new table. Every new index on an existing table has `unique = 0` (`pragma_index_list`).
- **A3. The order test**, for every migration declared `breaking after n`: exactly `n` names sort before it in the binary's list. A test also refuses an `additive` header on a migration that fails A2. A `breaking` header on a migration that would pass A2 is allowed, because the label is conservative.
- **A4.** `scripts/new-migration.sh <slug> additive|breaking` writes the file with the name from `date -u +%Y%m%dT%H%M` and the header. For a breaking migration it fills in `after <n>` by counting the files.

### O. Order (problem 2)

- **O1. A fresh store** applies the legacy prefix, creates `schema_migrations` (R1), then applies every named migration in name order, in one `BEGIN IMMEDIATE` transaction as today (`mod.rs:169-215`).
- **O2. An existing store** applies the binary's unapplied migrations in name order after what it already has, so the application order is the order recorded in `seq`. An unapplied migration may sort before an applied one: Plan A's migration lands on `main` after Plan B's, which `hs` already applied. This is **allowed when the migration is additive.** Additive migrations commute in effect: new tables, indexes and kinds are distinct objects and rows, and added columns are distinct columns. Column *order* can differ between stores. That is harmless because cpd names its columns: `grep` found no `SELECT *` and no `INSERT` without a column list in `core/src`, `cli/src` or `tui/src` (2026-10-18). A test that pins this is in J4.
- **O3. A breaking migration is a barrier.** Two sides of the same rule:
  - **At build** (A3): its `after n` must equal the number of migrations sorting before it. So any migration that lands on `main` after it, with an earlier name, fails the test, and must be renamed to a newer timestamp, with `scripts/new-migration.sh` and a `git mv`. Two Plans with breaking migrations: the second to land fails the same test, and its author must re-check the rebuild against the first (hs-d68's "whoever lands second" stays, only for breaking migrations).
  - **At run time:** the runner refuses to apply a breaking migration (new code `migration_order`, exit 1, nothing written) when the store has any applied migration that sorts after it, or any applied migration the binary does not know. Either means the rebuild was written without seeing something the store has.
- **O4. The schema hash per store** (§4.3): with O2, `hs` and a fresh store can legitimately differ in column order, so no single embedded hash can describe every store. Instead:
  - **S1.** Open reads the latest `schema_migrations.schema_hash` (highest `seq`) and compares it with the hash of `sqlite_schema` (`schema.rs:237-252`, unchanged). This happens **before** migrating. Today the order is the reverse (`mod.rs:99-101`); with recorded hashes, migrating first would let a migration record a tampered schema as the truth.
  - **S2.** A mismatch is `store_tampered` for writes and a `tamper` entry for reads, as today (`mod.rs:259-268`). On a mismatch the runner applies nothing.
  - **S3.** After the reconcile (K4b), the migrating transaction records the new hash on every row it inserts.
  - **S4.** Under §4.3's threat model this is as strong as today. Changing the recorded hash needs `cpd_txn()` (R1). An agent who drops the triggers, edits, and recreates them exactly is already out of scope (`contract.md:692`). What we lose: the binary no longer knows by itself what any store's schema should be; it trusts the hash chain the store has recorded since the conversion (Q2).
  - **S5.** A test still asserts that a store converted from a version-6 database and a fresh database end with equal hashes when they applied the same migrations in the same order (as today's `schema.rs:1041`).

### W. The compatibility window (problem 4, second half)

- **W1. One decision.** `store::verdict(conn, binary_list) -> Verdict` is computed from the read-only state: `user_version`, the `schema_migrations` rows, and the binary's list. In this order:

  | Store state | Verdict |
  |---|---|
  | no file, or `user_version = 0` | opens: fresh (O1) |
  | `1 ≤ user_version ≤ LEGACY.len()` | opens: convert (C1) |
  | `LEGACY.len() < user_version < 1000` | `store_too_new`: a numbered migration this binary lacks |
  | `user_version = 1000`, no `schema_migrations` | `store_failed` (not a store cpd wrote) |
  | `user_version > 1000` | `store_too_new` (a later scheme) |
  | a known name with another checksum | `migration_changed` (R3) |
  | the floor (R4) is not in the binary's list | `store_too_new`; details: `floor`, `unknown` (all applied names the binary lacks) |
  | an unapplied breaking migration of the binary breaks O3 | `migration_order` |
  | an unapplied additive migration that sorts before the floor | `migration_order` (unreachable when A3 holds; it fails loudly anyway) |
  | otherwise | opens; `pending` = the binary's unapplied names, applied in name order |

  Knowing the floor implies knowing every migration that sorts before it, by A3 in the binary that contains the floor. So an older binary that knows the floor lacks only additive migrations named after it. These cannot change what it reads or writes at the SQL level (A1).
- **W2. In the window** an older binary reads and writes normally and checks the hash against the recorded one (S1), which needs no knowledge of the newer schema. If it has unapplied migrations of its own, which happens with a Lead Branch build, it applies them by O2/O3 and runs the reconcile (K4b) from `history_tables`. The reconcile covers tables created by migrations the binary does not know, because the list lives in the store.
- **W3. `store_too_new` keeps its code and exit,** but changes its meaning: "this store has a breaking migration this cpd does not know (or a later scheme)". Its details become `{floor, unknown}` instead of `{version, supported}` (`mod.rs:362-368`). The adapter keys on the code only (`pi/test/follower.test.ts:48-51`), so it is unaffected (*inferred* from that test).
- **W4. Who uses the verdict.** Every place that compares schemas today calls `verdict`, and nothing compares on its own:
  - **`Store::open`:** refuses with the verdict's code, else checks the hash (S1), then migrates `pending`. It re-computes the verdict inside `BEGIN IMMEDIATE`, because another process may have migrated since (as `mod.rs:173-175`).
  - **`cpd --version`** (`cli/src/cli.rs:103-109`) adds the verdict, read-only, never creating or migrating:

    ```json
    {"ok": true, "version": "0.0.0", "schema": 1000, "store_schema": 1000,
     "store": {"opens": true, "refusal": null, "unknown": ["20261019T1410-session-messages"], "pending": []}}
    ```

    `store` is `null` without a database. `refusal` is `{code, message}` when `opens` is false. `schema` and `store_schema` stay **only** for the drivers and wrappers still running integer-era code during the conversion. Their old `exe.rs:199-207` must keep accepting the new binary, so they become a projection of the verdict: `schema` is always 1000; `store_schema` is `null` without a database, 1000 when `store.opens`, and 1001 when not. Old code reloads exactly when the new verdict says the store opens. When to remove them is Q4.
  - **Reload guard** (`core/src/host/exe.rs:170-208`): reloads when `store.opens` is `true`, or `store` is `null`. The log line becomes `reload skipped: <path> cannot open the store: <refusal.message>`.
  - **Install guard** (`scripts/install-cpd.sh:23-33`): refuses when `"opens":false` and prints `refusal.message`. The advice "merge main into this branch" is printed only for `store_too_new`. The other refusals name their own fix: for `migration_changed`, the branch's migration differs from the one this store applied; for `migration_order`, rename the breaking migration.
  - **`events follow`** (`core/src/events/mod.rs:139-151`): reads `PRAGMA schema_version` with `data_version` on each poll, instead of `user_version`, and re-computes the verdict only when it changed. When the store no longer opens, the follower exits with the verdict's code, which keeps hs-d140.
  - **`doctor`** (`core/src/doctor/state.rs:25-29`): the `store` check reports the verdict's refusal as today. A new `warn` check, `store_window`, passes when `unknown` is empty. Otherwise its message is "this cpd is older than the store (additive migrations <names>); install main".
- **W5. hs-d151's prose rule** ("a preview of a Lead Branch must contain main") becomes enforced by the guard. A preview that lacks only additive migrations of `main` installs and runs. One that lacks a breaking migration is refused as today. Whether the prose rule can go is part of Q5.

### Conversion (problem 3)

- **C1. When.** The first binary with named migrations opens a store with `1 ≤ user_version ≤ LEGACY.len()` and no `schema_migrations` table. Today that is `hs`, `air` and `pro`, all at 6 (*unverified* for `air` and `pro`; `hs` reports `store_schema 6` per hs-n100).
- **C2. Steps,** in **one** `BEGIN IMMEDIATE` transaction under `locks/store.lock` (`mod.rs:308-326`), with the same pragmas as today (`mod.rs:152-167`):
  1. Re-read `user_version` and check that `schema_migrations` still does not exist; if another process converted meanwhile, take the named path instead.
  2. Check `schema_hash == SCHEMA_HASHES[user_version - 1]`. On a mismatch, refuse with `store_tampered` and convert nothing: a tampered store must not become the trusted base of the hash chain.
  3. Apply the legacy functions `user_version + 1 ..= LEGACY.len()`, as today's loop does (`mod.rs:192-199`).
  4. `CREATE TABLE schema_migrations` (R1), and insert one row per legacy migration: its legacy name, its compat (1: `breaking`; 2: `additive`; 3–6: `breaking`, under A1), the checksum of its `.sql` text, `seq = i`, `txn` = this migration's `txns` row, and `converted = 1`. The legacy SQL files' history is in git, so no row loses information. The original application times were never recorded, so none are invented.
  5. Apply the named migrations in name order (O1), the first being `kind-tables` (K1). Then reconcile (K4b), check `PRAGMA foreign_key_check` (as `mod.rs:202-211`), record the hash on every inserted row (S3), set `user_version = 1000`, and commit.
- **C3. Idempotent.** Any failure rolls the whole transaction back and leaves the store at its old `user_version`, readable by the old binary. A rerun starts from step 1. Once committed, step 1 never matches again. Nothing is deleted: `kind-tables` copies every `events` and `context_records` row with its id, as migrations 3 and 4 did (`0003.sql:34-35`, `0004.sql:145-146`).
- **C4. Old binary meets a converted store:** `user_version = 1000 > VERSION`.
  - `Store::open` refuses with `store_too_new` (`mod.rs:359`).
  - Its `events follow` exits with `store_too_new` (`events/mod.rs:145`).
  - Its `--version` reports `store_schema: 1000 > schema`, so an old `install-cpd.sh` (`:30`) and an old reload guard (`exe.rs:203-204`) both refuse it.

  This is correct: `kind-tables` is breaking, so old code must not write the converted store.
- **C5. New binary meets an unconverted store:** it converts (C2). Its `--version` reports `store_schema: 1000` (it opens), so integer-era drivers and wrappers still running reload into it, as they do for any newer binary today. This is the same one-time disruption as migrations 4 and 6. A new binary meets a store at `LEGACY.len() < user_version < 1000` when another branch added numbered migrations, such as Plan manager's `0007`–`0010` if the conversion lands first. It refuses with `store_too_new` and converts nothing (Q1).
- **C6. Test:** version-1 to version-6 databases with rows in every table convert. Each keeps every row and id, passes `foreign_key_check`, has six `converted = 1` rows, and ends with the same hash as a fresh store (S5). A version-6 database with a changed trigger is refused and unchanged. Converting twice is a no-op.

## Worked example

Two Plans each add one Event kind and one table:
- **Plan `pins`** writes `20261019T0905-plan-pins.sql`.
- **Plan `msgs`** writes `20261019T1410-session-messages.sql`.

Both run after the conversion (applied names: `0001-initial` … `0006-pr-poller`, `20261019T0800-kind-tables`, floor = `kind-tables`).

```sql
-- cpd-migration: additive
INSERT INTO event_kinds (name) VALUES ('plan-pinned');
CREATE TABLE plan_pins (plan TEXT PRIMARY KEY REFERENCES plans(id), pinned_at TEXT NOT NULL) STRICT;
INSERT INTO history_tables (name, no_delete) VALUES ('plan_pins', 1);
```

```sql
-- cpd-migration: additive
INSERT INTO event_kinds (name) VALUES ('session-message');
CREATE TABLE session_messages (id INTEGER PRIMARY KEY, session TEXT NOT NULL REFERENCES sessions(id), text TEXT NOT NULL) STRICT;
INSERT INTO history_tables (name, no_delete) VALUES ('session_messages', 1);
```

Each Plan also edits the Kind enum (`core/src/events/kind.rs`), §5.2 and `pi/lib/event.ts`; those shared lines are covered in "What stays coupled". **Neither touches `schema.rs`, and their migration files cannot conflict.**

| Case | On `hs` (converted, running `main`) | On a fresh store |
|---|---|---|
| `pins` merges first, then `msgs` | install of `main`+pins: verdict opens, pending `[plan-pins]`; applied `seq 8`. Old processes (main without pins) still running: unknown additive `plan-pins` after the floor, so they open and keep working until they reload. Install of `main`+pins+msgs: pending `[session-messages]`, `seq 9`. | name order: plan-pins, session-messages |
| `msgs` merges first, then `pins` | `session-messages` applied as `seq 8`. Then `main`+pins: pending `[plan-pins]`, whose name sorts *before* the applied `session-messages`. It is additive, so O2 allows it, and it is applied as `seq 9`. | name order: plan-pins, session-messages (same as above) |
| A Lead Branch preview of `pins` while `msgs` is on `main` but the branch lacks it | install guard: `store.opens` true (unknown `session-messages` is additive), pending `[plan-pins]`. The preview installs; hs-d151's "merge main first" is not needed. | – |

**Hashes.** In all three orders, `sqlite_schema` ends with the same rows: two new tables, their six generated triggers, and no changed existing row. So every `hs` order and the fresh store record the **same** hash (S5 can assert it). Each store still checks only against its own record (S1). Suppose instead that both Plans had added a nullable column to `plans`. The second row would then show `plans.sql` with the columns in the two orders, so `hs` and fresh would record **different** hashes, and both would pass their own check (O4). Rule K4b would regenerate `plans_hist_*` from each store's own column order.

**If `pins` were breaking** (it rebuilds `plans` to add a `CHECK`, `-- cpd-migration: breaking after 7`):
- **`msgs` lands first.** On `pins`' rebase, A3 fails: 8 migrations now sort before it. Its author runs `scripts/new-migration.sh`, renames the file to the current minute, writes `after 8`, and checks that the rebuild keeps `msgs`' changes.
- **A preview of `msgs` was installed on `hs` before `pins` landed,** so `session-messages` is applied and `pins` is not yet renamed. Then `main`+pins' build reports `opens: false, migration_order`, and the install guard refuses it. No process crashes, and the store is untouched.
- **`pins` lands first.** It becomes the floor. A binary without it reports `store_too_new` and is refused at install and at reload, as today.

## What stays coupled (problem 5)

- **One store per machine, one floor.** A breaking migration still forces every process on the machine (engine, views, drivers, Lead wrappers, followers) onto a binary that knows it. The window helps additive migrations only. Of the 5 migrations after the first, binaries.md judged 2 (4 and 6) unable to keep old binaries running (`docs/research/binaries.md:100-108`). Under A1's stricter rule, 4 of the 5 (3, 4, 5 and 6) are breaking.
- **Breaking migrations are serialised across Plans.** The A3 count is a deliberate collision: a rebuild must include everything before it, so "whoever lands second" re-checks. Today every migration collides. After this design only breaking ones do, and they fail loudly in a test instead of in a merge, revert and reland (hs-n69).
- **Additive is a schema property, not a code property.** A1/A2 prove that old SQL still works. They do not prove that old code accepts new *values*:
  - Event kinds are read as strings (`select.rs:17`), but the adapter has its own `KINDS` list (binaries.md C6; out of scope here, Jobs `protocol-contract`/`protocol-code`).
  - Record kinds are parsed into a closed enum (`ids.rs:279`), so an old binary that meets a `brief` record would fail if it parses it (*inferred*; on `main` records are still read from the git store, §4.6).
  - New columns are null in rows that old binaries write, so new code may not assume them set.

  Rule: a migration that adds a value which old code parses into a closed type is declared `breaking`. No test can enforce that (Q6).
- **Shared source files remain:** `core/src/events/kind.rs`, contract §4.4 and §5.2, `commands.md`, and `pi/lib/event.ts` are still edited by every Plan that adds a kind, with adjacent-line conflicts (hs-d9). Only the migration list stops colliding.
- **The history-trigger generator and the runner's own DDL** (`schema_migrations`, K1's tables) are frozen. Changing them is a breaking change for every store.
- **Previews share the store.** A branch build installed for review applies the branch's migrations to `~/.cpd` before review ends. If the migration then changes, that store refuses the merged build with `migration_changed` until someone repairs it by hand. Today the same case gives a hash mismatch, `store_tampered`. This is Q5.
- **Stores can differ byte for byte** (column order, history JSON key order) when additive migrations arrived in different orders. Debugging must not assume `hs` equals fresh.
- **No downgrade past a breaking migration**, as today.

## Contract sections to amend (before the code, hs-d4)

| Where | Change |
|---|---|
| contract §1.2, second table, exit `1` (`contract.md:63`) | add `migration_order`, `migration_changed` |
| contract §1.4 `store.lock` (`:93`) | unchanged; the setup now also covers the conversion |
| contract §3.4 "Check before reload" (`:570`), §3.7 install guard (`:624`), §6.7 "Check before reload" (`:1524`) | "`store_schema` is `null` or `<= schema`" becomes "`store` is `null` or `store.opens` is true" (W4); the messages of W4 |
| contract §4.3 schema check (`:690`) | compare with the store's latest recorded hash, before migrating (S1–S4); embedded hashes only for the conversion (C2) |
| contract §4.4 DDL (`:694-700` and the DDL) | "the schema at the current version, 6" becomes "after migration `<name>`"; add `schema_migrations`, `event_kinds`, `record_kinds`, `history_tables`; `events.kind` and `context_records.kind` as foreign keys (K1); the derived-trigger rule (K4) |
| contract §4.5 (`:1174-1183`) | replace the first four bullets with rules N, R, A, O, W and the conversion; keep migrations 2–6 as the legacy record; add `kind-tables`; replace "drops and recreates the history triggers" (`:1182`) with K4 |
| contract §5.2 (`:1294`) and §4.6 record kinds | a kind exists when its row is in `event_kinds` / `record_kinds`, added by an additive migration (K2) |
| contract §5.4 (`:1373`) | the follower re-checks the verdict when `PRAGMA schema_version` changes (W4) |
| commands.md §1.2 `--version` (`commands.md:56`) | the `store` object; `schema`/`store_schema` as the legacy projection (W4) |
| commands.md §1.3 common refusals (`:75`) | `store_too_new` (new meaning and details), `migration_order`, `migration_changed` |
| commands.md §11.2 `events follow` (`:1527`) | `store_too_new` comes from the verdict |
| commands.md §14.1 `doctor` (`:1680-1716`) | `store` reports the verdict; new `warn` check `store_window` |
| `docs/implementing.md:59` | the `--version` shape |

## Proposed implementation Jobs

The order is linear because J2, J3 and J5 share `core/src/store/{mod.rs,schema.rs}`, and parallel Jobs never edit the same file (hs-d9).

| Job | ~min | Files | Needs | Delivers |
|---|---|---|---|---|
| **J1 `migrations-contract`** | 60 | `docs/contract.md`, `docs/commands.md`, `docs/implementing.md` | this document approved | the amendments above |
| **J2 `named-runner`** | 90 | `core/build.rs` (new), `core/src/store/mod.rs`, `core/src/store/schema.rs`, `core/src/store/migrations/*` (`git mv` to legacy names), `core/src/error.rs` (`migration_changed`), `cli/tests/store.rs` | J1 | N1–N4, R1–R3, the conversion C1–C6, S1–S5; any unknown applied migration is still `store_too_new` |
| **J3 `window`** | 60 | `core/src/store/mod.rs`, new `core/src/store/verdict.rs`, `core/src/error.rs` (`migration_order`), `cli/tests/store.rs` | J2 | `verdict` (W1), the floor (R4), O2/O3 at run time, W2–W3 |
| **J4 `migration-checks`** | 60–90 | new `core/tests/migrations.rs`, `scripts/new-migration.sh`, `Cargo.toml` (rusqlite `hooks`, if A2.1 holds) | J3 | A2, A3, A4, the no-`SELECT *` test (O2). The Gate check N5 is a `.config/wt.toml` change, which the Lead makes (protected for Workers) |
| **J5 `kind-tables`** | 60–90 | `core/src/store/migrations/<ts>-kind-tables.sql`, `core/src/store/schema.rs` (reconcile K4b, `HISTORY_TABLES` removed), `cli/tests/store.rs` (`:228`, `:282`) | J3 | K1–K4, the first breaking named migration |
| **J6 `version-guards`** | 90 | `cli/src/cli.rs`, `core/src/host/exe.rs`, `scripts/install-cpd.sh`, `core/src/events/mod.rs`, `core/src/doctor/state.rs`, `cli/tests/{store,session_reload,doctor,events}.rs` | J3 (J5 for a realistic test store) | W4: `--version` `store` object and projection, both guards, the follower, `doctor` |

J4 and J6 can run in parallel with J5: the file sets do not overlap, apart from `cli/tests/store.rs` in J6, which can instead get its own new test file. Installing the result on `hs` converts `~/.cpd`. That is a one-way door: after it, only named-migration builds can be installed. It should be a Lead step with the User's go, after `air` and `pro` have no running Attempts.

## Open questions for the User

1. **Plan manager's `0007`–`0010`** (`cpd/manager/cpd`, hs-n72, hs-n82, hs-n88) are in flight with numbers. If manager lands first, `LEGACY` grows to ten, and the conversion records ten. If this work lands first, manager renames its four files to named migrations: `0009` becomes an `INSERT INTO event_kinds`, `0007`/`0008`/`0010` are breaking, and `0010`'s `context_records` rebuild uses `record_kinds`. Which lands first?
2. Is it acceptable to **move the trusted schema hash into the store** (S4)? The alternative pins a fresh-store hash in the code, and every Plan that adds a migration would edit that same line.
3. Should **`attempts.cause`** (and any other list) become a lookup table too, or only Event and record kinds (K3)?
4. Should `--version`'s **legacy `schema`/`store_schema`** stay forever, or be removed once `hs`, `air` and `pro` are converted (W4)?
5. **Previews on the shared store:** may a Lead Branch build with unmerged migrations still be installed over `~/.cpd`? The alternatives are its own `CPD_HOME` (binaries.md open question 3) or refusing branches whose migrations are not on `main`. And should hs-d151's "merge main first" give way to the guard (W5)?
6. Is the rule "a new value that old code parses into a closed type makes the migration breaking" enough, or must readers of extendable lists accept unknown values (`RecordKind`)?
7. Should a trigger-only change on an existing table that only *relaxes* it (like migration 5) count as additive? A1 calls it breaking, which is conservative, and A2 cannot prove "relaxes".
8. Is the **timestamp form** `YYYYMMDDTHHMM-slug` (minute, no `Z`, UTC by rule) right, and are the **legacy names** `0001-initial` … `0006-pr-poller` acceptable?
9. Do you want the new `doctor` warn check `store_window` (W4), or is `--version`'s `store.unknown` enough?
