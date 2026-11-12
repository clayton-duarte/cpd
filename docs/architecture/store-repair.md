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

- **Backup.** Before the live write, copy `cpd.db`, `-wal` and `-shm` to `$CPD_HOME/run/backups/<ts>/`, where `<ts>` is UTC `YYYYMMDDTHHMMSSZ` as host repair already stamps. This is distinct from `run/cpd-install-backups/`, which holds the checkout patch. The backup is taken after the dry run passes and before the transaction opens.
- **Confirmation.** The repair asks the User at a terminal to confirm, showing the migration name, both checksums and the backup path, and continues only on an explicit yes. Not decided: whether this replaces or adds to the Session approval file that `cpd host approve` writes today (10 minutes, one repair, `approval::live`). Neither is built.
- **Transaction.** The repair insert and any migration step run in one `BEGIN IMMEDIATE` transaction. A failure before commit drops it and the store is unchanged. Failure after commit leaves the repair row in place; that is the ledger working as designed.

## Rollback procedure

Proposed. The ledger cannot be edited, so rollback restores the backup.

1. Stop every writer of the store. Not decided: the exact stop command. Do not rely on this list being complete until it is written in the implementation.
2. Replace `cpd.db`, `cpd.db-wal` and `cpd.db-shm` with the copies in `$CPD_HOME/run/backups/<ts>/`. The restored store has no repair row.
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

Skeleton (reconcile-docs-a1). Content follows.
