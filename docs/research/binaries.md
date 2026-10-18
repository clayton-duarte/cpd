# Separate binaries or one binary? (hs-q5)

Research for hs-q5 (User, 2026-10-17), Job `binaries-research` of Plan `dev-speed`. It does not decide; the User does. Sources: the Context `building-cpd` (`cpd context show --context building-cpd`, read 2026-10-18), `git log origin/main` (3369692), `gh pr list/view`, `gh run list`, the code at 3369692, and measurements in this worktree. Anything not checked is labelled *inferred* or *unverified*.

## Summary

1. None of the collisions so far came from having one binary. They came from **one shared source tree** (the same files edited by parallel Plans), **one linear store schema** (migration numbers taken twice), and **installs onto one shared `~/.cpd`** (a build whose schema does not match the store's).
2. Every one of the 7 feature PRs since #8 touched `core/`, `cli/` and `docs/`; 5 of 7 added a store migration; 5 of 7 touched `tui/`; 4 of 7 touched `pi/` (table E2). Features cut across components, so separate binaries would still be released together.
3. All binaries would share one store. `store_too_new` is checked by every process that opens it, and views open it directly, so a migration would still force every binary to upgrade at once (hs-n9, hs-n73, hs-n79).
4. What separate binaries *would* buy: an install that changes only the cockpit would not restart Leads and drivers (hs-d33, hs-d38). Option 3 can get the same thing more cheaply by changing the reload policy.
5. What they would cost: N builds, N installs and N reload paths, a compatibility test matrix, and more mixed-version states of the kind that crashed 12 Attempts in ~7 s in hs-n79.
6. **Recommendation: keep the single binary (option 1) and add the versioned interfaces of option 3**, starting with the guards against the collisions we actually saw: an install guard against older schemas, migration numbers reserved per Plan, a schema compatibility window, and an Event-kind/protocol check between the adapter and cpd.
7. Open questions for the User are at the end. The main one: is the pain restarts, migration renumbering, or preview installs of Lead Branches? Each needs a different fix.

## The evidence

### E1. Collisions, by kind

| # | Collision | Where it shows | Records | PRs / commits / files |
|---|---|---|---|---|
| C1 | Parallel Plans edit the same files | Rules: one owner per shared file, "whoever lands second rebases" (7 records) | hs-d9, hs-n4, hs-n29, hs-n34, hs-n45, hs-n59, hs-n64, hs-n71, hs-n77, hs-n78 | 29 overlapping files between cross-repo and main (hs-d89). Overlap between merged PRs, measured (E3): #11∩#13 = 37 files, #9∩#11 = 26, #8∩#11 = 24 |
| C2 | Store migration numbers taken twice | tree-menu and cross-repo both wrote migration 3; plan-reopen and pr-poller both wrote 0005; manager reserves 0007/0008 ahead of time | hs-d68, hs-n29, hs-n34, hs-d89, hs-n45, hs-n59, hs-n64, hs-n68, hs-n69, hs-n71, hs-n72 | #9 (migration 3) vs #11 (renumbered to 4); #13 (1237d94) merged 22:01:53Z, reverted by #14 (969a977) at 22:02:58Z, #12 (a935d90, its own 0005) merged 22:07:19Z, #15 (revert of the revert) conflicted and was closed 22:10:15Z, #16 (a6c5824) relanded it as 0006 at 22:34:48Z. That is 33 min and one extra PR and Job. `core/src/store/migrations/0001..0006.sql` |
| C3 | An install restarts everything; old processes break on a new schema | Lead wrappers left on the old binary; event followers stuck after `store_too_new`; every running Attempt crashed when a schema-5 build was installed over a schema-6 store | hs-d33, hs-d38, hs-d49, hs-d55, hs-d109, hs-d140, hs-n9, hs-n73, hs-n74, hs-n79 | `scripts/install-cpd.sh` (rebuilds and restarts engine and views); `core/src/store/mod.rs:342` `refuse_too_new`. This Job's own Attempts a1–a3 died that way: `~/.cpd/run/dev-speed/logs/attempt-binaries-research-a1.log` ends in `reload: new cpd installed, re-executing` then `store_too_new … schema version 6, newer than this cpd's 5`; the a2 and a3 logs are empty. Across the Plan, 12 attempt logs (4 Jobs × a1–a3) have mtimes between 20:02:15 and 20:02:22 (measured with `ls --time-style=full-iso`) |
| C4 | The Gate builds and tests everything | Quick check = typecheck + `cargo build --workspace`; full Gate = `npm run verify` (typecheck, `npm test`, build, `cargo test --workspace`) at `pr open`/`pr rebase` | hs-d5, hs-d137 | `.config/wt.toml`, `package.json` `verify`, `.github/workflows` (CI from #10). Times in E4 |
| C5 | A Lead Branch needs main mid-Plan | A preview install of a Lead Branch cut before a migration hits `store_too_new`. Merging main into a Lead Branch broke `pr rebase` and the A8 merge step | hs-d104, hs-d113, hs-d116, hs-n55, hs-n52, hs-n67, hs-d139 | tui-visuals cut from 044ce97 (schema 3) while main went to schema 4 with #11 (hs-n55); cross-repo hand-pushed 10d608b to #11 (hs-n52) |
| C6 | The adapter's protocol drifted from the binary | The adapter's Event `KINDS` lacked the five rename/archive kinds that #9 added; the adapter did not see a dialog signal | hs-n44, hs-d117, hs-n24 | Fixed in #11 (adapter-kinds Job). `pi/lib/event.ts` (KINDS), `pi/lib/cpd.ts:38` (`exec("cpd", args)`). The adapter is **already** a separately shipped artifact, installed by the same script |

### E2. What each feature PR touched

Measured with `git diff-tree -r --name-only <sha>` on each squash commit of `origin/main` (#10 is CI only and #14 is a revert, so both are left out).

| PR | Commit | Files | core | cli | tui | pi | docs | migration |
|---|---|---|---|---|---|---|---|---|
| #8 hardening | 15d51c7 | 46 | ✓ | ✓ | | | ✓ | |
| #9 tree-menu | e81f2f2 | 39 | ✓ | ✓ | ✓ | | ✓ | 0003 |
| #11 cross-repo | deb6ace | 110 | ✓ | ✓ | ✓ | ✓ | ✓ | 0004 |
| #12 plan-reopen | a935d90 | 11 | ✓ | ✓ | | ✓ | ✓ | 0005 |
| #13 pr-poller | 1237d94 | 47 | ✓ | ✓ | ✓ | ✓ | ✓ | 0005 (reverted) |
| #16 pr-poller reland | a6c5824 | 48 | ✓ | ✓ | ✓ | ✓ | ✓ | 0006 |
| #17 tree-menu followups | 3369692 | 30 | ✓ | ✓ | ✓ | | ✓ | |
| **Total (of 7)** | | | 7 | 7 | 5 | 4 | 7 | 5 |

The files touched by the most of #8, #9, #11, #12, #13 and #17 (counted with `sort | uniq -c`): `docs/contract.md` and `docs/commands.md` (6 of 6); `cli/tests/store.rs` and `cli/tests/plan.rs` (5); `core/src/store/schema.rs`, `core/src/session/mod.rs`, `core/src/plan/row.rs`, `core/src/error.rs`, `tui/src/read/shared.rs` and five `cli/tests/*.rs` (4). #12 and #13 shared 8 files, among them `core/src/store/migrations/0005.sql`, `core/src/store/schema.rs` and `pi/skills/cpd-lead/SKILL.md`.

### E3. Overlap between merged PRs (files in both)

Measured from the same file lists (`comm -12`).

| | #9 | #11 | #12 | #13 | #17 |
|---|---|---|---|---|---|
| **#8** | 10 | 24 | 4 | 14 | 8 |
| **#9** | | 26 | 4 | 13 | 20 |
| **#11** | | | 10 | 37 | 16 |
| **#12** | | | | 8 | 2 |
| **#13** | | | | | 7 |

### E4. The build and the tests

The machine is `hs`: AMD Ryzen 7 3800X (8 cores, 16 threads), 62 GiB RAM, Linux 6.8, rustc/cargo 1.99.0, no sccache (hs-d137's `RUSTC_WRAPPER` is not in `scripts/cargo.sh` at 3369692). It was **heavily loaded** by other Workers of this Plan: the 1-minute load average was 25 at the start and 60–67 during the runs (read from `/proc/loadavg` before each step). So the times below are upper bounds. Each command ran once, in this worktree, through `scripts/cargo.sh`, timed with `date +%s.%N` around it; cargo's own `Finished … in` agrees to within 1 s.

| Step | Time | Note |
|---|---|---|
| `cargo build --workspace`, cold (no `target/`, crates already in `~/.cargo`) | 66.2 s | |
| same, warm, nothing changed | 0.3 s | |
| after `touch core/src/lib.rs` | 14.8 s | rebuilds core, tui, cli |
| after `touch tui/src/lib.rs` | 7.2 s | rebuilds tui, cli |
| after `touch cli/src/main.rs` | 3.9 s | rebuilds cli |
| `cargo test --workspace`, first run after the build | 280.0 s, of which 34.2 s compiling tests | stopped early: one test failed, `cli/tests/tui.rs:142` (view order, `tui_builds_the_cockpit_once_and_starts_the_engine`), so the later test binaries did not run. Not reproduced in the next run; *inferred* load-dependent |
| `cargo test --workspace`, warm, all passed | 226.9 s | 1072 tests; test binaries run one after another (their own times add up to 224.7 s) |
| — of which `cli/tests/*` (41 binaries) | 211.0 s, 685 tests | binary-driven tests, every component goes through `cpd` |
| — of which the `cli/tests/tui_*` binaries | 46.2 s, 120 tests | the cockpit's tests live in `cli/tests/` (hs-d18) |
| — `cpd-core` unit tests | 13.6 s, 186 tests | |
| — `cpd-tui` unit tests | 0.07 s, 195 tests | |
| `npm run typecheck` | 1.4 s | `/usr/bin/time` |
| `npm test` (pi adapter) | 23.3 s | 66 tests; 1 failed once in an `after` hook (`ENOTEMPTY` removing `/tmp/pi-test-*`, `pi/test/fake.ts:152`); *inferred* flaky under load |
| CI on GitHub (`gh run list`, `updatedAt − createdAt`, the rust and pi jobs in parallel) | 260–343 s per successful run | 10 runs, 2026-10-16/17 |

Size (measured): `core/src` 25,606 lines in 120 files, `tui/src` 10,084, `cli/src` 236, `cli/tests` 23,775, `pi/` 2,492 (`.rs`/`.ts`, `wc -l`). The debug `cpd` binary is 119 MB; the installed release binary (`~/.local/bin/cpd`) is 12.4 MB. `target/` is 4.7 GB after these runs.

The cost of a build is in `core/` and in the tests, not in linking one binary. Cargo already recompiles only the changed crate and the crates that depend on it. Since `core` sits under everything, a core change rebuilds everything however many binaries there are. The test suite (227 s) dominates the quick check's build (≤ 15 s incremental).

### E5. What the single binary couples today

One build of `cpd` (`cli/src/main.rs` → `cpd::main_with`) is every process cpd runs:

| Process | Command | Started by | Reloads on install (contract §3.7) |
|---|---|---|---|
| engine | `cpd watch` | `install-cpd.sh`, `cpd watch --ensure` | killed and restarted by the script |
| cockpit views | `cpd tui view …` | `cpd tui` | `cpd tui --respawn` by the script |
| Lead wrapper | `cpd session run` | the cockpit / `session open` | re-execs at the Lead's next idle and restarts pi (SIGTERM, resume) (§6.7, hs-d49) |
| Attempt driver | `cpd attempt run` | engine E3 | re-execs between steps (§3.4, hs-d55) |
| event follower | `cpd events follow` | the pi adapter | exits non-zero on store errors and re-execs on a new binary (hs-d140) |
| PR poll | `cpd pr poll` | engine E5, as a subprocess | each run is new |
| pi adapter | TypeScript in `~/.local/share/cpd/pi` | pi, `-e <adapter>` | copied by the same script; `/reload` planned (hs-d109) |

So one version covers the engine, views, drivers, Lead wrappers, the adapter protocol (CLI JSON plus `events follow` lines) and the store schema. That version is not even named: workspace `version = "0.0.0"` (`Cargo.toml`), and `cpd --version` prints `{"ok":true,"version":"0.0.0"}`. The schema's only identity is `user_version` (1..6) plus a schema hash per version (`core/src/store/schema.rs`, contract §4.3/§4.5). A process whose number is lower refuses with `store_too_new`. A process with the same number but a different hash (two different "migration 5"s) refuses writes as `store_tampered`.

The views open the store themselves (`Store::open` in `tui/src/tree/mod.rs:59`, `tui/src/plan/mod.rs:193`, `tui/src/logs/mod.rs:57`). Any process that opens the store migrates it (`core/src/store/mod.rs:152`), so the newest binary to start moves the schema for everyone.

The migrations so far, by what an older reader could survive (read from `core/src/store/migrations/*.sql`; whether old code would actually work is *inferred*):

| Migration | PR | Change | An older binary could keep running? |
|---|---|---|---|
| 2 | cockpit parity, 0a55ca1 (on main with #7) | 3 × `ADD COLUMN` | yes (additive), *inferred* |
| 3 | #9 | 3 × `ADD COLUMN`, `events` rebuilt with a wider kind check | yes for reads; its writes stay valid, *inferred* |
| 4 | #11 | moves columns `plans` → `prs`, rebuilds 4 tables | no |
| 5 | #12 | two triggers relaxed | yes, *inferred* |
| 6 | #16 | `pull_requests`; `prs` rebuilt without the poll columns | no (an old `pr poll` writes dropped columns) |

## The three options

1. **Today:** one binary, one install, every process reloads on install (hs-d3, hs-d33).
2. **Separate binaries per component over the same store and CLI**, for example `cpd` (CLI and drivers), `cpd-watch`, `cpd-tui`, and the adapter, each built, installed and reloaded on its own.
3. **One binary and one build, with stable, versioned interfaces:** a schema compatibility window (each migration says whether older binaries may keep using the store), a named protocol version for the adapter and the views, migration ids that cannot collide, and an install that refuses to go backwards.

### Collision by collision

| Collision | 1. One binary (what happened) | 2. Separate binaries | 3. One binary + versioned interfaces |
|---|---|---|---|
| C1 shared files | Rebases, file owners, "whoever lands second" (hs-d9, hs-n71, hs-n77, hs-n78) | **Same.** The shared files are source files (`docs/contract.md`, `core/src/store/schema.rs`, `cli/tests/*`), and a binary split does not move them. If the components went to separate repos, the 7-of-7 core+cli+docs PRs would become cross-repo Plans (R34) with needs between PRs: *inferred* slower | **Same** |
| C2 migration numbers | Renumbering at rebase (#9/#11), and a merge, a revert and a reland (#13/#14/#15/#16, hs-n69) | **Same**, because the store is still shared and linear. Splitting the store per component would not help: migrations 3, 4 and 6 rebuilt tables that every component uses (`events`, `plans`, `prs`) | **Fixed if** migrations get ids that cannot collide (ranges reserved per Plan when its contract is locked, as hs-n71/hs-n72 already do by hand). A plain compatibility window does not fix it |
| C3 install restarts everything; old processes break | Every install reloads every process. Stale wrappers (hs-n9) and followers (hs-n73); a schema-5 build over schema 6 crashed every running Attempt (hs-n79) | **Partly better:** a cockpit-only install restarts only views, not Leads or drivers. **Worse at schema changes:** each binary embeds its own schema version, so a migration still forces all of them to upgrade together, and there are more mixed-version combinations to get wrong (hs-n79 with N binaries) | **Better:** an install guard refuses a build older than the store, so hs-n79 is refused at install. A compatibility window keeps old processes running across additive migrations (2, 3, 5 of 5, *inferred*). Narrower reloads need the User to change hs-d33 ("nothing keeps running old code") |
| C4 Gate | Quick check ≤ 15 s incremental, 66 s cold; tests 227 s; CI 260–343 s (E4) | **No gain for real PRs:** all 7 touched `core/`, which everything links, so a per-binary Gate would still run nearly everything. **Extra cost:** cross-version tests (old cpd-tui with new store, and so on) | Same Gate, plus a few compatibility tests (an older binary against a newer store), *inferred* small |
| C5 Lead Branch needs main | Preview installs hit `store_too_new` (hs-n55), so main had to be merged mid-Plan (hs-d113), which `pr rebase` and A8 could not carry (hs-n52, hs-n67; fixed by hs-d139) | **Not fixed:** a branch-built `cpd-tui` links the branch's `core`, so it embeds the branch's schema and refuses the newer store just the same. It would be fixed only if the views read through the installed `cpd`'s CLI instead of the store, which is a protocol boundary that option 3 can add too | **Fixed for additive migrations** by the window; for breaking ones (4, 6) still `store_too_new`. A preview against its own `CPD_HOME` would avoid it entirely (open question) |
| C6 adapter protocol drift | Kinds missing in `pi/lib/event.ts` until #11 (hs-n44, hs-d117), although one script installs both | **Same or worse:** it is already a separate artifact, and more boundaries mean more lists to keep in sync | **Fixed by a check:** the adapter's kinds and protocol version are tested against what `cpd` declares (a test or a `cpd` command listing kinds; *inferred* design) |

### Costs

| Cost | 1. One binary | 2. Separate binaries | 3. One binary + versioned interfaces |
|---|---|---|---|
| Build | One workspace build; incremental per crate already (E4) | Same compile work (core is shared), plus one more `[[bin]]`/crate per binary and its link | Same as 1 |
| Install | One script, one copy, one reload policy | N copies, N version checks, a reload policy per binary; dotfiles' install-on-every-apply (hs-d146) must stay consistent across N | One script plus a guard (refuse older schema, and the engine spawns drivers by the installed path, as hs-n79 suggests) |
| Testing | Binary-driven suite against one `cpd` (`cli/tests/common/mod.rs`) | The same suite plus a version matrix; the harness must find N binaries | Same suite plus a handful of "old binary, newer store" tests |
| Skill / adapter coordination | Skill and adapter ship with the binary; drift still happened (C6) | Each binary's CLI becomes a published interface for the skill and the adapter; more surfaces to version | One declared protocol version and kinds list, checked in a test |
| Contract | One `contract.md` / `commands.md` (hs-d4) | Per-binary compatibility rules in the contract | A compatibility section in §4.5 and a protocol version in §5.3 |

### What none of them fixes

- **Two Plans editing the same source file** (C1). That is source layout and file ownership (hs-d9), not binary layout. *Inferred*: splitting the two contract docs into per-area files would cut the most-shared files (6 of 6 PRs), but that is a docs change, not a binary change.
- **A shared store schema.** Any number of binaries over one SQLite store share one `user_version`. They are coupled by the schema whatever the process layout.
- **A semantic change that crosses components.** The phase rename `pr` → `deploy` (hs-n71, hs-n72) edits core, tui, the skill and a migration together.

## Recommendation

Keep the single binary (hs-d3/R32) and make its interfaces explicit (option 3). The evidence:

- every collision in E1 has a cause that separate binaries leave in place (shared files, the shared linear schema, installs onto a shared store) or make worse (more mixed-version states, more protocol lists to keep in sync);
- every feature PR so far touched core, cli and docs together (E2), so separate release cycles would almost never be used;
- the build already compiles per crate, and the time goes to the tests (E4), which a split does not shorten because everything depends on core.

Concrete first steps, smallest first (each is a later Plan's job, and none is done here):

1. **Install guard** (hs-n79): `scripts/install-cpd.sh` refuses a build whose schema `VERSION` is below the store's `user_version` (one `cpd` command reports both, *unverified* which one exists), and the engine spawns drivers by the installed path, never `/proc/self/exe (deleted)`. This would have saved the 12 crashed Attempts.
2. **Migration ids that cannot collide** (C2): reserve migration numbers per Plan in a Context record when its contract Job is locked (done by hand in hs-n71/hs-n72), so two open PRs never claim the same number. Git already reports the clash as an add/add conflict on `0005.sql` once one PR is on main (#15). The cost in hs-n69 came from finding it only after a merge.
3. **A schema compatibility window** (C3, C5): each migration declares the oldest binary schema that may still open the store (additive ones keep it; 4 and 6 would not). `refuse_too_new` and the schema hash check accept that window. Contract §4.5 first (hs-d4).
4. **A named version and protocol check** (C6): a real version in `cpd --version` (today `0.0.0`) and the Event kinds list exported by `cpd` and checked by the adapter's tests.
5. Only then, if restarts are still the main cost: let the User decide whether hs-d33 may allow, say, Leads to keep running across cockpit-only installs. With step 3 this no longer needs a separate binary.

## Open questions for the User

1. Which pain matters most: restarts on install (C3), migration renumbering (C2), or preview builds of Lead Branches (C5)? Each has a different first step above.
2. Should hs-d33 ("nothing keeps running old code") stay absolute, or may an install leave Leads and drivers running when the store stays compatible?
3. Should Lead Branch preview builds (hs-d104) keep installing over the shared `~/.cpd`, or run with their own `CPD_HOME` (a copy of the store)? The second avoids `store_too_new` with no window at all, but the preview does not see live Plans.
4. Are migration numbers reserved per Plan acceptable, or should migrations be identified by name instead?
5. Would loosening the `events.kind` CHECK (validated in code instead) be acceptable? New Event kinds caused the `events` rebuilds in migrations 3 and 4. This trades one database-level invariant for fewer migrations, and it touches "make illegal states unrepresentable".
6. Is there a component the User expects to evolve on its own cadence (for example the cockpit through tui-visuals, or the adapter tied to pi's pinned version, hs-d134)? If one exists, it is the candidate for a split; the evidence so far shows none.
