# Store repair: architecture

Plan `store-repair`, Job `repair-docs`. This note records the store repair design for a diverged migration history (bug b23 follow-up), the accepted assumptions A1 to A4, and what `origin/main` does today. It is a design note: the repair mechanism is **not built**.

## Status

**Proposed, not implemented.** Accepted assumptions (from the Job's Context; the decisions pro-d562 and pro-d568 they follow are not in this repository, so their text is unverified here):

- **A1.** `schema_migrations` is never modified. A repair is appended to `schema_migration_repairs`.
- **A2.** `core/src/store/verdict.rs` honours valid repair mappings when it verifies migration checksums.
- **A3.** Host repair enforces a copy dry-run, a backup under `run/backups`, interactive confirmation, and transaction rollback.
- **A4.** The repair solves mac-pro b23 with `20261110T2200-draft-waiting-on`.

Verified on `origin/main` (`9109589e4`), by reading source and grepping `core/src/store/`, `core/src/host_repair.rs` and `cli/src/`:

- There is no `schema_migration_repairs` table, no repair code and no repair command. `grep -i repair core/src/store/` matches only migration comments and the `workflow-repair` migration, which is unrelated (`workflows.repair`, contract §7.6).
- `cpd host repair` resets the checkout only. It does not open or write `cpd.db`, it runs no dry run, it asks for no confirmation, and it writes its patch backup to `run/cpd-install-backups/`, not `run/backups/` (`core/src/host_repair.rs`, `save_patch`).

Until a decision moves A1 to A4 into code, every "Proposed" statement below is a plan, not behaviour.

## Current state (verified on `origin/main`)

**The ledger.** `schema_migrations` is created by `RUNNER_DDL` (`core/src/store/schema.rs:193-205`). Columns: `name` (PK), `compat`, `checksum` (sha256 of the migration `.sql` text, 64 hex), `seq` (UNIQUE), `txn` (FK to `txns`), `converted`. One row per applied migration, written by `Store::record` (`core/src/store/mod.rs`, `fn record`) inside the migrating transaction.

**The triggers today.** Three triggers on `schema_migrations` (`schema.rs:202-204`):

| Trigger | Event | Effect |
|---|---|---|
| `schema_migrations_own` | `BEFORE INSERT WHEN NEW.txn IS NOT cpd_txn()` | `RAISE(ABORT, 'schema_migrations.txn must be cpd_txn()')` |
| `schema_migrations_no_update` | `BEFORE UPDATE` | `RAISE(ABORT, 'append_only: schema_migrations')` |
| `schema_migrations_no_delete` | `BEFORE DELETE` | `RAISE(ABORT, 'append_only: schema_migrations')` |

`schema_migrations` is not in the generated history triggers. The checksum is a hash of the file, not of the migration's effect.

**The checksum refusal.** `verdict_with` loads `SELECT name, compat, checksum FROM schema_migrations ORDER BY seq` (`verdict.rs`, `verdict_with`) and passes it to `decide` (`verdict.rs:191`). Rule R3 (`verdict.rs:199-212`): for each applied row whose name this binary embeds, if the stored checksum differs from the sha256 of the embedded text, return `ErrorCode::MigrationChanged` (`migration_changed`, exit 1). The error carries `name`, `recorded` and `embedded`. R3 runs before R4 (the breaking floor, `verdict.rs:213`) and O3 (the breaking barrier, `verdict.rs:223`). It is the only place `migration_changed` is produced. Migrations are checked inside `migrate_in_txn` before any write, so a refusal writes nothing.

**The dry run that exists.** `verdict()` is a read-only verdict (`cpd --version` calls it). Install's dry run (`core/src/install/mod.rs`, `dry_run_migrations`) copies `cpd.db`, `-wal` and `-shm` to `$HOME/.cache/cpd-migrate-scratch.<pid>`, runs `cpd watch --check --migrate-scratch` with `CPD_HOME` set to the copy, and removes the copy. The live store is never opened by that dry run.

**The isolation guard, and its gap.** Commit `abba74dfd` (#1075) added a guard in `open_catalog` (`core/src/store/mod.rs`, around line 286): a store that needs migration is refused `migration_order` with `live: true` unless the home is isolated. `CpdHome::is_isolated` is true whenever `CPD_HOME` is set (`core/src/home.rs`, `resolve`, returns `(path, true)`). So any `CPD_HOME`, including a worktree build pointed at the live store, passes the guard. This is a gap, verified from source. It bears on A4, and no decision on it is recorded in this repository.

**Transactions.** Migrations run in one `BEGIN IMMEDIATE` (`mod.rs`, `migrate_in_txn`), committed at the end; a failed step returns `Err` before commit. Pinned by `a_failed_named_migration_rolls_the_whole_transaction_back` (`core/src/store/mod.rs`).

## Append-only `schema_migration_repairs` table

Proposed. A1: `schema_migrations` keeps its schema and its triggers unchanged; a repair is a new row here, never an edit there.

Proposed shape (one row per accepted repair):

| Column | Meaning |
|---|---|
| `id` | INTEGER PRIMARY KEY |
| `name` | the migration name; FOREIGN KEY to `schema_migrations(name)` |
| `recorded_checksum` | the checksum the store holds in `schema_migrations` for `name`, the one being repaired (64 hex) |
| `accepted_checksum` | the checksum of the embedded `.sql` text this repair maps to (64 hex) |
| `txn` | the repairing transaction, FK to `txns(id)` |
| `reason` | free text, required, non-empty |

Proposed: the table is append-only with the same guard pattern as `schema_migrations`. Its triggers are `schema_migration_repairs_own` (insert requires `txn = cpd_txn()`), `_no_update` and `_no_delete` (both raise `append_only: schema_migration_repairs`). Not decided: the `reason` vocabulary, and whether a second repair of the same `name` is allowed.

A repair is undone only by restoring a backup (see Rollback), because the table cannot be edited.

## Trigger guards

Proposed guards, beyond the append-only triggers above:

- A repair row is refused unless `schema_migrations` holds a row with `name = NEW.name` and `checksum = NEW.recorded_checksum`. A repair cannot claim a checksum the store never recorded.
- A repair row is refused unless `NEW.accepted_checksum` is the sha256 of the text this binary embeds for `NEW.name`. That check cannot be done in SQL; the writer checks it in Rust and the trigger only checks the row shape (64 hex, non-empty `reason`). Not decided: whether this check belongs in Rust alone or also in a trigger.

## Verdict reconciliation

Proposed (A2). R3 changes from "any differing checksum refuses" to "a differing checksum refuses unless a valid repair maps it":

1. For each applied row whose name this binary embeds, compare `checksum` with the embedded sha256 (today's R3).
2. If they differ, look for a repair row for `name` with `recorded_checksum = checksum` and `accepted_checksum = embedded`.
3. If one exists, the row passes R3. If none exists, return `migration_changed` as today, with the same fields.

A repair row that does not match on both sides is not honoured and does not weaken R3. R4 and O3 are unchanged: a repair clears only R3 for its one name.

This changes the refusal that contract §4.5a pins (`migration_changed` for an applied migration with other text). §4.5a says a behaviour change to those contracts waits for a demonstrated defect and a Lead decision. A2 is the Lead's accepted assumption and is the decision this note relies on. The change to §4.5a's pinned test has not been made here; it is part of the implementation Job.

## Copy dry-run

Proposed (A3). Before a repair writes to the live `cpd.db`, it runs the same steps on a copy:

1. Copy `cpd.db`, `-wal` and `-shm` to a scratch directory, as install's dry run does (`dry_run_migrations`).
2. Insert the repair row and run the migration in that copy with `CPD_HOME` set to the scratch directory.
3. Verify the copy with the read-only verdict (`verdict()`). It must be `opens`.
4. Only then continue to the live store. The scratch copy is removed on success and on failure.

Today host repair runs none of this (see Current state). Install's dry run is the existing model to reuse, not a second one.

## Backup and interactive confirmation

Proposed (A3), in the order the Job lists them:

- **Backup.** Before the live write, take a consistent SQLite online-backup snapshot at `$CPD_HOME/run/backups/<ts>/cpd.db`, where `<ts>` is UTC `YYYYMMDDTHHMMSSZ` as host repair already stamps. This is distinct from `run/cpd-install-backups/`, which holds the checkout patch. The backup is taken after the dry run passes and before the transaction opens.
- **Confirmation.** The repair asks the User at a terminal to confirm, showing the migration name, both checksums and the backup path, and continues only on an explicit yes. Not decided: whether this replaces or adds to the Session approval file that `cpd host approve` writes today (10 minutes, one repair, `approval::live`). Neither is built.
- **Transaction.** The repair insert and any migration step run in one `BEGIN IMMEDIATE` transaction. A failure before commit drops it and the store is unchanged. Failure after commit leaves the repair row in place; that is the ledger working as designed.

## Rollback procedure

Proposed. The ledger cannot be edited, so rollback restores the backup.

1. Stop every writer of the store. Not decided: the exact stop command. Do not rely on this list being complete until it is written in the implementation.
2. Move the live `cpd.db`, `cpd.db-wal` and `cpd.db-shm` aside together, then restore only the snapshot `cpd.db` from `$CPD_HOME/run/backups/<ts>/`. Never combine the restored snapshot with old live sidecars. The restored store has no repair row.
3. Check the result with `cpd --version`, which runs the read-only verdict and migrates nothing (verified in `cli/src/cli.rs`).
4. If the checksum is still refused, restore the migration's file from `origin/main`, as the `migration_changed` message says.

Host checkout rollback is separate and exists today: `cpd host repair` saves the old HEAD under `refs/cpd-host/<ts>` and the diff under `run/cpd-install-backups/`.

## A4: mac-pro b23 and `20261110T2200-draft-waiting-on`

- `core/src/store/migrations/20261110T2200-draft-waiting-on.sql` is on `origin/main` (commit `17d20311b`, #1065). Its header is `-- cpd-migration: additive`; its body is `ALTER TABLE jobs ADD COLUMN waiting_on TEXT;`. Its sha256 is `ce607fabfb0036c67828eb91f400d3bdf848b7da5ce221c1704a1367588bd2af`, the same for the worktree file and the `origin/main` blob (checked with `shasum -a 256` at `9109589e4`).
- On a store that never applied this name, the migration is pending and is applied, not checked (`decide`, the `known`/`pending` split). R3 fires only when a store recorded a different text under this name.
- **Unverified:** whether the draft text applied to the mac-pro store at 2026-11-10 19:15 EST equals `ce607fab…`. I did not read that store. If it differs, origin/main refuses with `migration_changed` for this name, and A2's repair would be the fix.
- The isolation gap above means nothing on `origin/main` stops a worktree build with `CPD_HOME` set at the live store from applying a draft. That is one path by which a draft could reach a live store. Whether it caused b23 is unknown: contract §4.5a records the provenance as unknown.

## Source

- `core/src/store/schema.rs` `RUNNER_DDL` (`schema_migrations`, triggers), `core/src/store/verdict.rs` `verdict_with` and `decide` (R3, R4, O3), `core/src/store/mod.rs` `open_catalog` (isolation guard), `record`, `migrate_in_txn`.
- `core/src/home.rs` `CpdHome::resolve`, `is_isolated`.
- `core/src/host_repair.rs` `repair`, `save_patch`; `core/src/install/mod.rs` `dry_run_migrations`.
- Contract §4.5 and §4.5a (`docs/contract.md`); `docs/commands.md` §9.11.

## Schema reconciliation

**Proposed, not built.** Decisions pro-d562, pro-d568 and pro-d576 are the basis (their text is not in this repository, so it is unverified here, as A1 to A4 are). Verified on this commit's base by reading source: `core/src/store/repair.rs` appends `schema_migration_repairs` rows and rebuilds nothing, and `cpd host repair` detects checksum divergence only. Until the implementation Job lands, every statement in this section is design.

When `cpd host repair` finds drift, it does more than checksum repair. Drift is either a diverged migration (the A1 to A4 path above) or a **drifted table**: a table whose definition differs from main's expected schema. The repair rebuilds each drifted table in main's form and records the diverged migrations, in one transaction.

### What counts as drift

- **Table definition.** A table's columns, column types, constraints (`CHECK`, `FOREIGN KEY`, `UNIQUE`) or table options differ from main's. Example from the Job's Context: `jobs` carrying an unmerged `CHECK` constraint on `waiting_on`.
- **Column order.** Column order is not in the canonical hash, because `canonical_entries` sorts columns (`core/src/store/schema.rs`, `table_parts`). It is still drift, because the generated history triggers list columns in `cid` order (`schema.rs` `history_triggers`, the `object` and `changed` lists). Example from the Job's Context: `incident` with diverged column order. Source reading, not run: the generated-trigger check (`generated_mismatch`) reports this.
- **Not drift.** The `schema_migration_repairs` table and its triggers (excluded by name from the canonical hash, `schema.rs` `canonical_entries`), and the row contents of `schema_migrations` (not hashed).
- **Pending migrations.** A migration the binary embeds but the store has not applied is not a diverged migration: it is not in `schema_migrations`, so the checksum path never sees it. How the schema comparison treats its objects is **not decided** (see Open points).

The comparison reads main's expected schema from the reference database that `verdict.rs` builds from the binary's migrations (`reference_database`). For a store with additive migrations this binary lacks, the comparison is table by table (`subset_mismatch`), as the migrate path does today.

### SQLite 12-step table rebuild

Each drifted table is rebuilt with SQLite's documented procedure for schema changes that `ALTER TABLE` cannot make (SQLite's "making other kinds of table schema changes" steps; the list below is from that procedure as recalled, unverified against the live SQLite page in this session):

1. Turn foreign keys off (`PRAGMA foreign_keys = OFF`), if they are on.
2. Start a transaction (`BEGIN IMMEDIATE`).
3. Record every index, trigger and view on table `X`.
4. `CREATE TABLE X_new` with the new definition.
5. `INSERT INTO X_new (<columns>) SELECT <columns> FROM X`.
6. `DROP TABLE X`.
7. `ALTER TABLE X_new RENAME TO X`.
8. Recreate the indexes and triggers of `X` (the generated history triggers come from `reconcile`, `schema.rs`).
9. Drop and recreate each view that reads `X`.
10. `PRAGMA foreign_key_check`; any row returned fails the rebuild.
11. Commit.
12. Turn foreign keys back on, if step 1 turned them off.

Three points specific to cpd:

- **`PRAGMA foreign_keys` is a no-op inside a transaction.** Step 1 must run before `BEGIN IMMEDIATE` (step 2). The Job's Context lists the pragma among the steps "inside" the transaction; the runner (`Store::migrate`, `core/src/store/mod.rs`) sets it before `BEGIN`, and this section does the same.
- **Pragma set.** The runner also sets `legacy_alter_table = ON` for the rebuild (`mod.rs`, `migrate`), and restores both pragmas after. The rebuild does the same.
- **Views.** Step 9 is not optional for `jobs`: the view `job_tenants` (migration `20261101T0901-job-tenancy.sql`) reads `jobs`.

### Order of the procedure

The steps, in order, as `cpd host repair` runs them:

1. **Detect.** Diverged migrations (`detect_divergences`) and drifted tables (the schema comparison above). Nothing drifted: no store keys in the output, as today.
2. **Copy dry run.** A consistent SQLite online-backup snapshot is created at `run/scratch-repair-<pid>/cpd.db`. The 12-step rebuild of every drifted table and the `schema_migration_repairs` insert run on the copy. The copy must then pass the checks the migrate path runs before commit (`migrate_in_txn`): `foreign_key_check` is empty, and the schema comparison matches main's expected schema. The copy is removed on success and on failure. The live store is not touched.
3. **Confirmation.** The command lists each diverged migration (name, recorded and accepted checksum) and each drifted table, then the backup path it will write. `-y` or `--yes` confirms. Otherwise it asks `[y/N]`, and only an explicit yes continues. Answering N refuses before any write.
4. **Backup.** A consistent SQLite online-backup snapshot is created at `$CPD_HOME/run/backups/<ts>/cpd.db` (`<ts>` UTC `YYYYMMDDTHHMMSSZ`), before the live transaction opens. The backup is distinct from `run/cpd-install-backups/`.
5. **Apply to the live store.** One `BEGIN IMMEDIATE` transaction runs the same rebuilds and the same `schema_migration_repairs` insert as the copy, then the same checks, then commits. A failure before commit rolls the transaction back, and the live store is unchanged. The live run uses the same code path as the copy, so the copy's result is the live result, not a second implementation.

The copy run is the only gate before the live write. A live apply that skipped the checks of step 2 would be a defect, not a shortcut.

### Rollback

The rebuilds are in the transaction, so a failure before commit needs no restore. After commit, rollback is the backup in step 4, restored as in the Rollback procedure above. The rebuilt tables are not undone by any other path: the ledger is append-only.

### Open points

- **Not decided: pending migrations in the schema comparison.** Whether a pending migration's objects are compared as absent (drift), or expected once the migration applies. The Job's Context requires that the comparison "accounts for" them; the rule is not written.
- **Not decided: the `reason` text** for the rebuilt tables' ledger rows. Today it is `cpd host repair`.
- **Observed, unverified by running: no store lock.** `repair.rs` takes no store lock, unlike `migrate` (`settled_setup_lock`). A rebuild in a repair that runs beside a writer may conflict. The rebuild needs the lock or a busy-timeout rule before implementation.
- **Observed, unverified by running: the next txn id is read before `BEGIN`** (`repair.rs`, `apply_repair_in_txn`). A concurrent writer can take the same id. The rebuild's transaction must read it inside `BEGIN IMMEDIATE`.

## Consistent repair snapshots (QA #1120)

Repair backups and scratch copies use SQLite's online backup API through the
existing rusqlite dependency's `backup` feature. The source is opened read-only;
SQLite includes committed WAL data in the snapshot, without separately copying
a live WAL or SHM file. The returned database file can be restored independently.
An existing destination is refused rather than overwritten, and a missing
source is an error. A failed backup is not returned as a successful artifact.

This supersedes the raw three-file copy method in earlier versions of this
document. SQLite documents both the [copying risk](https://www.sqlite.org/howtocorrupt.html#_backup_or_restore_while_a_transaction_is_active)
and the [online backup API](https://www.sqlite.org/backup.html).
`VACUUM INTO` is another safe snapshot option, but vacuums/reorganizes the copy;
the backup API preserves the existing database-image backup intent and does not
require new libraries. Its existing rusqlite convenience method returns an
error on busy/locked outcomes instead of introducing an unbounded retry loop.
Sources inspected 2026-11-13: rusqlite 0.40.2 `src/backup.rs`,
[versioned documentation](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Connection.html#method.backup),
and [issue #1120](https://github.com/clayton-duarte/cpd/issues/1120).

Stop all processes using the live store before restoring. Move aside the whole
old database/WAL/SHM set and restore the standalone snapshot, then validate it
before resuming. This changes repair snapshots only; upgrade's separate backup
implementation is not implicitly covered. No live-host repair is performed by
this change.
