# cpd upgrade: isolated build, dry run, swap and rollback

Status: **decided** by pro-d567 (upgrade by button or command; the version tag shows that a new version is available; no automatic upgrade) and pro-d569 (build on each host from CI-green origin/main in an isolated directory; migration dry run on a copy of the store; backup; swap; restart; rollback on a failed health check). Store boundary guards: pro-d562. The command is `cpd upgrade` (commands.md §14.4c). The contract of its failures and of the version banners is `docs/contract.md` §3.7c.

**As of this document the code does not do this.** `cpd upgrade` does not exist: the `Command` enum in `cli/src/cli.rs` has no `upgrade` variant, and the only way to replace the binary is `cpd install --root <checkout>` (commands.md §14.4), which builds from the checkout the User points it at. The steps below are the design the implementing Jobs (`upgrade-tests`, `upgrade-impl`) must make true. Where a step is a step of `cpd install`, this page says so and does not restate it: contract §3.7 is the source for it.

## 1. Decisions in one place

1. **Upgrade is asked for, never automatic** (pro-d567). Nothing installs a new build on its own. A host upgrades when the User presses the WebUI's upgrade button or runs `cpd upgrade`.
2. **The version tag announces, it does not act** (pro-d567). When a newer CI-green commit exists on origin/main than the one running, the version tag shows "new version available". The tag offers the upgrade; it does not start one.
3. **Build here, from green** (pro-d569). Each host builds the commit itself, from origin/main, and only if CI was green for that commit (§2). No prebuilt binary is downloaded.
4. **Build away from the working tree** (pro-d569). The build runs in an isolated directory that cpd owns. The host checkout's working tree is never written by an upgrade.
5. **Prove the migrations before touching the live store** (pro-d569). The new build runs its migrations on a copy of `cpd.db` first (§3). A failure there changes nothing on the host.
6. **Back up before the first live write** (pro-d569). The store is copied while the engine is stopped, before the swap (§4).
7. **Swap atomically, keep the old build** (pro-d569). The new binary replaces the old by one `rename(2)`; the old binary is kept beside it for rollback (§4).
8. **Roll back on a failed health check** (pro-d569). The health check (§5) decides. A failure restores the backup and the old binary, then starts the old engine (§6). The store is restored, not migrated back: no migration is ever un-applied (§7).
9. **The store guard stays** (pro-d562). A live store is migrated only by a path that is allowed to migrate it (§8, open point 2). No other path migrates it.

## 2. Isolated build directory

- **Where.** `$CPD_HOME/upgrade/src` is a clone of the repo, owned by cpd; `$CPD_HOME/upgrade/target` is its `CARGO_TARGET_DIR`. Both are created on the first upgrade and reused by later ones (the clone is fetched, not re-cloned). Neither is in the host checkout (`~/projects/cpd`), which is the mirror of origin/main and may be reset by `cpd host repair` (commands.md §9.17).
- **Which commit.** The **green commit**: the newest commit of `origin/main` for which CI is green. The command fetches `origin main` into the clone, reads the head SHA of `origin/main` and checks that its CI check is `success`. A commit with no green CI is not built: the command refuses with `not_green` (exit 3) and names the SHA. It never falls back to an older green commit by itself.
- **How it is checked out.** `git checkout --detach <green sha>` in the clone, after `git clean -ffdx` in the clone (the directory is cpd's, so cleaning it touches nothing else). The clone's working tree is never shared with another checkout.
- **How it is built.** The same command as the bootstrap `scripts/install-cpd.sh` uses, run in the clone: `CPD_COMMIT=<short sha> scripts/cargo.sh build --release --locked -p cpd` (commands.md §14.4). `--locked` makes the build use the committed `Cargo.lock`. The result is `$CPD_HOME/upgrade/target/release/cpd`.
- **Idempotence.** A build of a green SHA that already exists in the target directory is reused by the same SHA; `cpd upgrade` never builds the same SHA twice in one run.

**Open point 1 (CI-green on main).** `.github/workflows/ci.yml` runs on `pull_request` and `workflow_dispatch` only (verified, lines 4-9). No CI run is attached to a commit on `main`, so "CI-green for origin/main's head" has no source of truth today. The implementing Job must either add a `push` trigger for `main` to `ci.yml`, or define green as the last successful CI run of the pull request that merged the commit. The Lead decides which; this page does not choose.

## 3. Migration dry run on a copy of cpd.db

This is the dry run of `cpd install` (contract §3.7 "The dry run on a scratch copy", commands.md §14.4 step 4b), run by `cpd upgrade` with the built binary from §2:

1. A scratch home is made under the temp dir. It holds a copy of `$CPD_HOME/config.toml`, and a copy of `$CPD_HOME/cpd.db` made by SQLite (`VACUUM INTO`, which includes the committed WAL pages; a raw file copy is never used, because the live engine writes the WAL).
2. The built binary runs with `CPD_HOME` set to the scratch home, through the internal dry-run command, which opens the store the way the engine's start does and migrates the copy.
3. The scratch home is removed in every case.

A failed copy or a non-zero exit fails the upgrade at step `dry_run` (§6 does not run: nothing was changed). The error's store code, when it has one (`store_too_new`, `migration_changed`, `migration_order`, `store_failed`), is in `details.code`. The live store is never opened for writing by this step.

The dry run runs while the old engine keeps running, so the copy can be a little stale. The residual risk is the one contract §3.7 names: the real migration runs later on the live store and can still fail on rows written after the copy. §6 covers that case.

## 4. Backup and atomic binary swap

Run only after the dry run passed, in this order. The engine is stopped first (the retirement of contract §3.7 "The install's engine order", step 3: the old engine is told to stop and the upgrade waits for `locks/watch.lock` to be free, bounded by `CPD_INSTALL_LOCK_WAIT_SECS`).

1. **Backup.** The store and the config are copied into `$CPD_HOME/backups/<UTC YYYYmmddTHHMMSSZ>-<old version>/`: `cpd.db` by `VACUUM INTO` (never a raw copy), and `config.toml`. The copy is checked by opening it read-only with the verdict of contract §4.5 (`store.opens` must be true). A backup that fails its check fails the upgrade at step `backup`, before the swap. Backups are never deleted by an upgrade (§8, open point 3).
2. **Stage the binary.** The new `cpd` is copied to `<dest dir>/cpd.new.<pid>`, where `<dest dir>` is the directory of the installed `cpd` (`~/.local/bin`), so the rename below stays on one filesystem.
3. **Keep the old binary.** The installed `cpd` is copied to `<dest dir>/cpd.previous` (replacing an older one). The old binary is what rollback restores.
4. **Swap.** `rename(2)` of `cpd.new.<pid>` over `cpd`. It is one syscall: a process that runs `cpd` sees the old file or the new one, never a partial one. It is the rule the adapter swap of contract §3.7 uses.
5. **Adapter and payload.** The rest of the install follows, in the order of contract §3.7: the adapter packages and the adapter swap (steps 4a and 5 of commands.md §14.4), and the WebUI payload refresh (step 7a). Each is a rename of a path that the old build can also be restored from.

A failure in steps 1 to 3 fails the upgrade at `backup` or `swap` with nothing replaced: the installed binary and the store are as they were, and the engine is restarted with the old binary (§6, steps 4 and 5 only). A failure in step 4 is the same. A failure in step 5 is a failure after the swap and goes to §6.

## 5. Restart and health check

After the swap, `cpd upgrade` starts the engine with the new binary: `cpd watch --ensure` (contract §3.7, step 3 of the install's engine order, which `--mark-restart` precedes so the restart is not reported as a crash). Then it checks, in this order, failing at the first:

1. **Binary.** `<new cpd> --version` exits 0, `protocol` is an integer, `store.opens` is `true` and `store.pending` is empty (contract §4.5, commands.md §1.2).
2. **Engine.** `locks/watch.lock` is held, by a process whose `pid` is the `pid` of a `watch-started` Event appended after the swap began.
3. **WebUI.** Port 3333 answers an HTTP `GET /` with status 200, and the WebUI's version check (contract §5, "WebUI version skew") finds no skew: the running binary's `protocol` equals the protocol of the WebUI it serves.

Each probe runs under the deadline of its tool (contract §1.2, `tool_timeout`), and the whole check has one deadline, a constant in the code with its reason. A probe that times out counts as a failure. The health check is a read of the new build's own state; it does not write to the store. The probes, their deadline and the exact `watch-started` check are for the implementing Job to pin (§8, open point 4).

If all three pass, the upgrade succeeds: it appends one `upgrade-finished` Event with the old and new SHAs and the backup path, and exits 0. If the check fails, §6 runs.

## 6. Rollback sequence

Rollback runs when the health check (§5) fails, or when a step after the swap fails. It runs in this order, and the order is the point: the old binary can open only the old store, so the store is restored before the old binary runs.

1. **Stop the new engine.** Its window is killed as the install does (contract §3.7 step 3); cpd never sends `SIGKILL` to an engine. The upgrade waits for `locks/watch.lock` to be free.
2. **Restore the store.** `cpd.db` from the backup of §4 is copied to a temporary file beside the live `cpd.db`, and renamed over it. The live `-wal` and `-shm` files are removed first, with the engine stopped, so the restored file is the only state. The restored store is checked with the verdict (`store.opens` true).
3. **Restore the config.** `config.toml` from the backup is copied back, by the same rename rule. (`cpd config --migrate`, step 4 of the install, rewrites only when a migration applies; the backup is the file before it.)
4. **Restore the binary.** `cpd.previous` is renamed over `cpd` (one `rename(2)`). The adapter and payload are restored by the same rule as their swap (§4 step 5).
5. **Start the old engine.** `cpd watch --ensure` with the restored binary, then the check that `locks/watch.lock` is held.

If every step of the rollback passes, the upgrade fails with `upgrade_failed` (exit 1, §6 of commands.md §14.4c), `details.step` naming the step that failed first, `details.rolled_back` `true` and `details.backup` the path of the backup. If a rollback step fails, the rollback stops at that step and the upgrade fails with `upgrade_rollback_failed` (exit 1), `details.step` naming the rollback step, `details.rolled_back` `false` and `details.backup` the path. Nothing retries a failed rollback step by itself: the host is then in a state only the User can clear (contract §3.7c, "The banner").

## 7. Store rollback semantics

- **A migration is never un-applied.** The migrations are append-only and frozen (`scripts/check-migrations-frozen.sh`; contract §4.5). The old binary cannot open a store that a newer build has migrated: its verdict is `store_too_new` (contract §4.5, R4). Restoring the old binary alone therefore does not roll the store back. Only the backup does.
- **Rollback restores the backup, and only the backup.** The store after a rollback is the store as it was when §4 took the backup. Nothing is migrated backwards and no row is edited.
- **Writes made under the new build are lost.** Rows written between the new engine's start and the rollback's restore are discarded with the rest of the new build's state. The upgrade does not merge them back. The failure's `error` Event (contract §3.7c) names the backup path, so a User who needs a row from that window can read it from the backup's file by hand.
- **The backup is consistent.** It is made by `VACUUM INTO` with the engine stopped, so it holds no half-written transaction. It is checked by the verdict before the swap (§4 step 1).
- **Only one rollback target.** The backup is the state before this upgrade. An upgrade does not roll back to an older backup by itself; a second rollback is a manual act.
- **Breaking migrations do not change this.** A breaking migration is a barrier for the verdict (contract §4.5, O3), which is a rule about which binaries may open the store. It does not make a migrated store readable by an older binary, so the backup is still the only way back.

## 8. Open points

1. **CI-green on main.** See §2. Blocks the implementation of "green" until decided.
2. **Who may migrate the live store (pro-d562).** `core/src/store/mod.rs:287-297` refuses to migrate a store at the default home unless the home is explicitly isolated (`core/src/home.rs:113`): "Cannot migrate live store ... without explicit isolation". The upgrade migrates the live store, so the implementation needs a path that is allowed to do it, for example a verified-release permission that only the upgrade's own run passes. That path does not exist in the code (verified by reading the two files). The name of the permission and the rule for granting it are for the Lead to decide. Until then `cpd upgrade` cannot migrate a live store, and its implementation is blocked.
3. **Backup retention.** Backups accumulate under `$CPD_HOME/backups/`. No pruning rule is decided; an upgrade deletes nothing.
4. **Health-check probes and deadline.** §5 lists the probes; the exact `watch-started` match, the HTTP check and the deadline's value are for the implementing Job to choose and pin in a test.
5. **The version tag's check interval.** How often the engine compares the running commit with the green commit of origin/main is not decided. It is a constant in the code, not a config key, with its reason in the code.
6. **CPD.app.** The desktop app is not upgraded by `cpd upgrade`. Its retirement is the `retire-app` Workflow's, out of this page's scope.
