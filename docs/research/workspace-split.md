# Research: split the workspace by ownership area (Plan quality, Job cy-split, hs-n300 Phase 4, hs-d350)

This is a proposal for the User's Plan review. It moves no product code. hs-d350 asks for crates that follow the ownership areas of hs-d324, so that different Leads' PRs land in different crates, and for the hot files (the schema and Event-kind registries, `docs/commands.md`, `docs/contract.md`) to be cut the same way. This document gives the design, measures what it buys against Phase 0 (`docs/research/build-cycle.md`), describes how reverse dependencies select tests, and ends with a decision.

Every number was measured on hs unless it says "computed" (arithmetic on measured numbers), "inferred" or "unverified".

- **Base:** main `1e12a94b`. It already has #159 (dependencies at `opt-level = 3`, nextest priorities) and #160 (the schema-pin design). Phase 0's base was `c5e11bbb`, so the times here are not paired with Phase 0's.
- **Prototype:** local branch `scratch/cy-split-a1/engine`, commit `ea74201e`. It was never pushed and was deleted after the measurements (§3.1).

## Summary

- **The proposed graph has no cycles, but today's `cpd-core` has one 17-module cycle.** One strongly connected component holds `args`, `attempt`, `config`, `context`, `driver`, `env`, `events`, `git`, `host`, `job`, `merge`, `plan`, `policy`, `pr`, `session`, `spec` and `store` (§1.1). Only the modules on top of that cycle can become crates without cutting edges first: `watch`, `doctor`, `inbox`, `ask` and `metrics`. So the hs-d324 areas *store and migrations*, *events* and *PR plumbing* each need edges cut before they can be crates. Five layers take 7 cuts, 10 references in all (§1.2 lists them). The full graph is close to a chain: `base ← store ← flow ← {pr ← engine, tui} ← cpd`.
- **The build gain is real only for an area that leaves `cpd-core` and also gets its own test binary.** The prototype moved the *engine steps* (`watch` and `doctor`) into a crate `cpd-engine` and gave the engine tests a binary of their own. After an edit in the engine (medians, under load 19-48, sccache on, §3.2):
  - scoped build (the edit, then the area's test binary): **9.8 s → 2.6 s**;
  - quick check: **5.1 s → 2.3 s**;
  - whole-workspace test build: **11.7 s → 7.7 s**.

  The two halves contribute about equally (§3.3). The crate takes `cpd-core`'s ~4.5 s rebuild down to `cpd-engine`'s 0.5 s. The small test binary takes `it`'s 5-6 s compile down to about 1 s. Edits in areas that stayed put (store, tree) gained nothing. A store edit paid one more crate in the chain: quick check 6.7 → 7.9 s, slower in 5 of 8 pairs.
- **Test execution is unchanged, and it dominates a scoped loop once the build is small.** Measured scoped runs: engine area 147 tests in **19.7 s** (the same on both trees), tree area 247 in 11.0 s, store area 203 in 16.3 s (§3.4).
- **Conflicts barely shrink.** I replayed every merge on the `cpd/*` branches with `git merge-tree`: 154 of 268 conflict, with 511 file conflicts and 1,254 hunks.
  - **Code:** the crate split removes **none** of the code conflicts. It moves whole files, and two PRs that edit one file still conflict wherever it lives. 61 % of all hunks have both sides changing the same base line.
  - **Docs:** the per-area doc cut removes **49 hunks (3.9 %)** and makes **4 of the 154 merges** clean (§3.5).

  The hot spots of hs-n285 are same-area pairs (dialogs×views, #119×#107, views×scroll-persist). No split moves those apart. Sequencing does (hs-d324).
- **The registries stay as they are.** `SCHEMA_HASH` hashes one database and is a real ordering interlock in 8 of the 14 measured conflicts. A crate cannot split it, so pin-design's (c), `scripts/repin.sh`, stands. The kind pin `PINNED` goes, as pin-design decided. The `Kind` enum and the §5.2 table stay one registry each (§2.1).
- **Decision (§5): approve a narrowed split, reject the rest.** Approve:
  1. per-area integration-test binaries (no product code moves);
  2. the `cpd-engine` crate, with `restart` passed into the cockpit the way `RunCommand` already is;
  3. cutting `commands.md` §13.2 and §17 into one file per view, and contract §4.5's migration entries into one file per migration.

  Reject, for now:
  - per-view `tui` crates: `cpd-tui` rebuilds in 1-3 s, which is all they could save;
  - layering the 17-module cycle into `cpd-base`, `cpd-store` and `cpd-flow`: those crates sit low in the chain, so an edit there still rebuilds everything above it. It gains no build time and removes no conflicts.

  Defer `cpd-pr` (PR plumbing). It needs 3 cuts, and its gain is inferred, not measured.

## 0. Method

- **Areas.** These are hs-d324's, as hs-d350 and hs-n285 name them: tree and Inbox, Deploy column, Plan view, status line, store and migrations, events, PR plumbing, engine steps, pi adapter.
- **Evidence:**
  - `cargo metadata` of main (3 members: `cpd-core`, `cpd-tui`, `cpd`; 225 packages);
  - the module graph, from `crate::<module>` references, with Tarjan's strongly connected components (a throwaway Python script);
  - hs-n285 (`cpd context show --context building-cpd --record hs-n285`);
  - pin-design's `docs/research/schema-pin.md` (#160);
  - the conflict history (§3.5).
- **Builds.**
  - Target directories were my own: `/tmp/cy-split-a1/target-{today,split}`. sccache was on (`scripts/cargo.sh` exports `RUSTC_WRAPPER=sccache`).
  - The cold builds took a Gate slot (`flock … gate-a.lock`). The scoped test runs went through `scripts/cargo.sh nextest`, which takes a test slot on main (#153).
  - Load is the 1-minute average from `uptime` before each run. It was 16-49 throughout.
  - Each per-unit time comes from cargo's `--timings` report.
- **Edits.** Each edit inserts `let _cy_edit = <n>;` as the first line of the first `pub fn` of a file (*body*), then `git checkout`s the file (*revert*), as Phase 0 did. today and split were interleaved edit by edit.
- **Commands measured:**
  - **ws**: `scripts/cargo.sh build --workspace --tests`, the workspace's test build. It builds what `nextest run --workspace --no-run` builds; the adapter refuses `test --workspace` even with `--no-run`.
  - **scoped**: `scripts/cargo.sh build -p cpd --test <binary>`, the build of a Worker's targeted run.
  - **qc**: `scripts/cargo.sh build --workspace`, the quick check.

  Each command had its own block of edits, after two warm-up builds. Switching between `-p cpd` and `--workspace` rebuilds `cpd` because the feature set differs (Phase 0 §7), so a block that mixed them would have measured the switch.
- **Discarded:** two earlier rounds of scoped builds ran as `nextest run --no-run`. On main that takes a test slot and can wait for one, so the time may include waiting. They are not used.

## 1. The proposed crates

### 1.1 Today

| crate | lines | depends on | holds |
|---|---|---|---|
| `cpd-core` | 44.6k (`src`) | none | every area but the cockpit, and the shared modules |
| `cpd-tui` | 18.7k | `cpd-core` | the cockpit: the views and the read model |
| `cpd` | 0.45k (`src`), **44.9k tests** | `cpd-core`, `cpd-tui` | the root command tree; `cli/tests/it`, **one** integration binary of 71 modules (1,392 tests on main) |
| `pi/` | TypeScript | none | the pi adapter; `npm test`, outside Cargo |

**`cpd-core`'s module graph**, measured from its `crate::` references:

- **One cycle of 17 modules:** `args attempt config context driver env events git host job merge plan policy pr session spec store`.
- **Outside the cycle** (no module in it uses them):
  - `ask`, `doctor`, `inbox`, `metrics`, `watch`;
  - the foundations `clock`, `home`, `lock`, `output`, plus the `error`/`ids` pair (a 2-cycle of its own).

The mutual pairs (references each way) are: `plan↔pr` (3/22), `host↔session` (1/13), `context↔store` (5/2), `driver↔merge` (3/3), `attempt↔driver` (2/3), `driver↔job` (2/2), `plan↔session` (4/1), `args↔plan` (1/2), `context↔plan` (1/2), `events↔store` (2/1), `attempt↔merge` (1/3), `merge↔pr` (1/1), `error↔ids` (2/1).

**`cpd-tui`'s module graph has no cycle.** The views are almost independent, with four edges between them:

- `tree → widgets`: the scroll window helpers and `open_legend`;
- `tree → merge`: `Pull`, `open`;
- `widgets → plan`: `pulse::legend`, `merged_icon`;
- `widgets → merge`.

### 1.2 The target crates

Each crate is listed below its dependencies. "Cuts" are the references that point the wrong way and must be inverted or moved before the crate can exist. They were counted on main's `crate::` references with this partition: **7 module edges, 10 references in all**. Line counts are main's.

| crate | modules | lines | area(s) (hs-d324) and owner (hs-n285) | depends on | cuts needed |
|---|---|---|---|---|---|
| `cpd-base` | `ids`, `error`, `args`, `clock`, `home`, `lock`, `output`, `host` | 5.5k | shared, no area. Edited only by a Job of its own (hs-d9); the Manager owns it | none | `args → plan::PrRow` (1); `host → session::PROGRAM` (1: move the constant into `host`) |
| `cpd-store` | `store` (with `migrations/`, schema, verdict), `events` | 7.3k | **store and migrations**, **events**. One crate because `store↔events` is a cycle (`Kind::ALL` against `WriteTxn`). Owner: the Manager's "Store and events" lane (#107, #119, inbox/attn, pi-pin/install-safe, quality/schemapin), one PR in flight at a time | base | `store → context::legacy` (2: the legacy Context-store conversion moves into `store`); `events → env` (1) |
| `cpd-flow` | `config`, `policy`, `git`, `env`, `context`, `plan`, `job`, `attempt`, `driver`, `merge`, `spec`, `ask`, `session`, `metrics`, `slot`, `inbox` (its model, which the tree reads) | 24.0k | the CPD model and its commands: not one of hs-d324's nine areas. Owners: cross-repo (flow, stacks), manager (Context, Session), inbox (`inbox`), quality (`slot`) | base, store | none inside it: the rest of the cycle stays in this crate |
| `cpd-pr` | `pr` | 5.7k | **PR plumbing**. Owner: Plan cross-repo ("cross-repo alone") | flow | `plan → pr` (3: `plan lock` calls `pr::start`; `pulls::PullRequest`), `merge → pr::start::tamper_bases` (1), `attempt → pr` (1) |
| `cpd-engine` | `watch`, `doctor` | 4.2k | **engine steps** (E1-E11, `doctor`). Owner per step: unverified, since hs-n285 names no engine lane | flow, pr | none (prototyped, §3.1). The prototype made 8 core items `pub` (`config::catalog`, `context::distill`, `attempt::ledger`, `driver::lock`, `driver::worker`, `session::tell_managers`, `session::pane_gone`, `host::pi::declared`) |
| `cpd-tui` | `cockpit`, the views (`tree`, `plan`, `widgets`, `logs`, `worker`, `history`, `merge`, `dialog`, …), `read/` | 18.7k | **tree and Inbox**, **Deploy column**, **Plan view**, **status line**: one module each, not a crate each (§3.2: the crate rebuilds in 1-3 s). Owners: one hs-n285 lane each, each with several Plans (tree: quality/views, pr-links, inbox, tui-visuals; Deploy and Plan view: quality/views, tui-visuals, pr-links; status line: pi-pin, tui-visuals, tui-dots, release) | flow | `cockpit → watch::restart`: pass it in from `cpd`, as `RunCommand` is today. The prototype took the shortcut `tui → engine` instead (§3.1) |
| `cpd` | `cli`, `interface`, `main`; integration tests **one binary per area** (§4.2) | 0.45k + tests | the root command tree. Owner: the Manager (shared file) | all | none |
| `pi/` | unchanged | | **pi adapter**. Owner: inbox, pr-links (hs-n285) | | |

**The graph** (no cycles):

```
cpd-base ← cpd-store ← cpd-flow ← cpd-pr ← cpd-engine ← cpd
                          ↑                                ↑
                          └──────────── cpd-tui ───────────┘
```

**What stays shared, and why:**

- **`ids` and `error`:** every area returns `CpdError` and names objects by their ids. `crate::error` alone is used 151 times (Phase 0 §4).
- **`home` and `lock`:** every path and every lock file.
- **`output`:** the one JSON value of contract §1.2.
- **`host`:** processes, tmux and deadlines.
- **The store handle `Store` and `WriteTxn`:** every area reads and writes through one database (contract §4.1-§4.3). Its schema is one canonical hash, so it cannot be split by area.

**Public API of each crate.** It is what the crates above it use today. Beyond `cpd-base`'s, the main items are:

- `cpd-store`: `Store`, `WriteTxn`, `Persist`, `verdict`, `SCHEMA_HASH`, the migrations list; `Kind`, `NewEvent`, `append`, `PROTOCOL`.
- `cpd-flow`: each module's command enum and `run`, as `cli.rs` dispatches them today (`context::run`, `plan::run`, …), plus what `cpd-pr`, `cpd-engine` and `cpd-tui` call: `plan::{PrRow, Phase, jobs}`, `driver::{lock, worker}`, `attempt::ledger`, `session::{tell_managers, pane_gone, PROGRAM}`, `inbox::model`.
- `cpd-pr`: `pr::cmd`, `pr::start`, `pr::poll`.
- `cpd-engine`: `watch::{Args, run, restart}`, `doctor::run`.
- `cpd-tui`: `run`, `bare`, `on_a_terminal` (unchanged).

Any `pub(crate)` item that a crate above uses becomes `pub`. That was 8 items for the engine. The rest is unverified: the prototype widened all of core's `pub(crate)` to `pub` for speed.

## 2. The hot files

### 2.1 The registries (with pin-design, `docs/research/schema-pin.md`)

| registry | what a split does to it | proposal |
|---|---|---|
| `SCHEMA_HASH` (`core/src/store/schema.rs`) | Nothing. The schema is one SQLite database, and the hash is its canonical form. A hash per area crate would lose the interlock that pin-design found load-bearing in 8 of 14 conflicts: a migration that sorts before another branch's `breaking` one | Keep pin-design's (c): `scripts/repin.sh` resolves the conflict (Plan quality, PR `pin`, in flight). Migrations stay in `cpd-store`, whatever area writes them |
| `PINNED`, the Event-kind hash (`core/src/events/kind.rs`) | Nothing | Deleted by PR `pin` (pin-design §3). It protects nothing the adapter test and the events test do not |
| the `Kind` enum and `kinds!` specs (`kind.rs`), §5.2's table, `event_kinds` rows, `pi/lib/event.ts` `KINDS` | A per-area `Kind` would need one enum spread across crates, and the adapter matches on one list | Keep one registry. The history has 7 `kind.rs` file conflicts and 3 in `pi/lib/event.ts`. Those I read were adjacent appends of different areas' kinds (for example `inbox-*` against `session-message`); the rest were not checked. One kind per line in `KINDS` removes pin-design's pair `kl`, but insertion points are still shared (unverified gain) |
| `cli/src/cli.rs` `Command` enum, `tui/src/lib.rs` `View` enum, `core/src/lib.rs` module list, `cli/tests/it/main.rs` module list | A crate per area adds one line here per crate, then nothing | Unchanged: 3, 3, 0 and 0 file conflicts in the history (`cli.rs`, `tui/src/lib.rs`, `core/src/lib.rs`, `it/main.rs`) |

### 2.2 The docs as per-area files

The cut measured in §3.5:

- **`docs/commands.md` §13.2 (`tui view`) and §17 (what the TUI reads):** one file per view. `docs/commands/tui-tree.md` (with §21 `inbox`), `tui-plan.md`, `tui-deploy.md` (`widgets`, `merge`, `legend`), `tui-status.md` (the StatusBar, `logs`, `tui action`), `tui-worker.md` (`worker`, `history`, `chat`), `tui-dialog.md`.
  - §13.2's table of views stays in `commands.md` as an index: one row per view, its name and its link.
  - Each view's keys, rows and reads move to the view's file.
  - These two sections hold 117 + 67 of the 274 `commands.md` hunks of the history, and all 35 that a cut removes.
- **The rest of `commands.md`:** stays one file. §3-§12, §14-§16 and §21-§23 are one area each already. Their conflicts are same-section edits that a file split keeps (§3.5).
- **`docs/contract.md`:** two registries move.
  - §4.5's migration entries go one file per migration, `docs/migrations/<name>.md`, beside the SQL. That removes 10 of its 154 hunks: each PR appended its entry at the same point.
  - §5.2's kind rows go per area, in each area's file. That removes 2 hunks.
  - §4.4 (DDL) stays whole. `cli/tests/it/store.rs` reads its ```` ```sql ```` blocks between `### 4.4 DDL` and `### 4.5`.
  - The other sections stay. Their 140 hunks are same-section edits.
- **Cross-references keep working:**
  - Each moved section keeps its number as its anchor: `tui-plan.md` begins `## 13.2.plan` and the index links to `commands/tui-plan.md`, so "commands.md §13.2" in code comments and records still finds the index row and its link.
  - `rg '§13\.2|§17'` lists the citations to point at the file (not counted).
  - Three readers of the docs change in the same PR:
    - `tui/src/logs/mod.rs` `docs_agree`: an `include_str!` of `commands.md` and `contract.md`, in `#[cfg(test)]`, so a doc edit rebuilds `cpd-tui`'s test unit;
    - `cli/tests/it/store.rs` (§4.4, unchanged by this cut);
    - `cli/tests/it/interface.rs` (`docs/interface.md`, unchanged).
  - `docs/implementing.md` "The one-area rule" names the area files.

## 3. The measured gain

### 3.1 The prototype

Scratch worktree `/tmp/cy-split-a1/split`, branch `scratch/cy-split-a1/engine` off `1e12a94b`, never pushed, deleted after.

- `git mv core/src/{watch,doctor} engine/src/`, a new member `cpd-engine` (`engine/Cargo.toml`, `lib.rs`), and `crate::<x>` → `cpd_core::<x>` in the moved files.
- core's `pub(crate)` → `pub`: a shortcut, see §1.2.
- `cli` depends on `cpd-engine` for `watch` and `doctor`.
- `cpd-tui` depends on `cpd-engine` for `watch::restart`. That is the shortcut the proposal replaces with an injected function. So in the prototype an engine edit also rebuilds `cpd-tui`, and the gain below is a lower bound.
- `cli/tests/it/{watch,watch_inbox,doctor}.rs` → `cli/tests/engine/`, with `main.rs` and `#[path = "../it/common/mod.rs"] mod common;`. `watch_team.rs` (7 tests) stayed in `it` by oversight.
- 55 files, +223 / −168 lines.

`nextest list` counts **2,438 tests on both trees**:

| | `cpd-core` | `cpd-engine` | `cpd-tui` | `cpd` | `cpd-core::migrations` | `cpd::it` | `cpd::engine` |
|---|---|---|---|---|---|---|---|
| today | 556 | | 465 | 6 | 19 | 1,392 | |
| split | 514 | 42 | 465 | 6 | 19 | 1,287 | 105 |

The engine area's 147 tests pass on the prototype (§3.4).

### 3.2 Incremental build after an edit, by area: today against the split

Wall seconds: the median, then (min-max, number of builds). Half the builds are body edits and half are reverts; the commands are those of §0. The files edited:

- engine: `core/src/watch/engine.rs`, or `engine/src/watch/engine.rs` on the split;
- store: `core/src/store/verdict.rs`;
- tree: `tui/src/tree/view.rs`.

Each area's scoped binary is `it` on today; on the split it is `engine` for the engine and `it` for the others.

| area of the edit | command | today | split | load |
|---|---|---|---|---|
| **engine steps** (moved) | scoped | **9.8** (9.3-16.0, 4) | **2.6** (2.0-3.3, 4) | 29-48 |
| | qc | **5.1** (3.2-9.9, 8) | **2.3** (1.5-4.8, 8) | 22-29 |
| | ws | 11.7 (7.5-19.3, 8) | 7.7 (5.4-16.2, 8) | 19-31 |
| **store and migrations** (stayed in `cpd-core`) | scoped | 10.0 (7.3-28.8, 4) | 10.4 (6.9-18.5, 4) | 27-36 |
| | qc | 6.7 (5.0-11.0, 8) | 7.9 (4.5-14.0, 8) | 22-30 |
| | ws | 15.2 (7.3-22.4, 8) | 15.6 (9.1-28.8, 8) | 18-34 |
| **tree** (stayed in `cpd-tui`) | scoped | 7.0 (6.1-9.4, 4) | 9.2 (7.9-12.9, 4) | 25-33 |
| | qc | 2.8 (1.4-5.9, 8) | 2.1 (1.5-6.5, 8) | 23-29 |
| | ws | 8.6 (4.8-27.1, 8) | 8.5 (5.2-17.8, 8) | 17-49 |

The units behind it, from `--timings` (median seconds, rounds 3-4):

| unit | today | split |
|---|---|---|
| `cpd-core` lib after an engine edit | 3.9-4.8 | not rebuilt |
| `cpd-engine` lib after an engine edit | | **0.5-0.9** |
| `cpd-core` lib after a store edit | 4.2-6.9 | 4.2-8.0 (then also `cpd-engine` 0.6-2.1) |
| `cpd-tui` lib | 1.2-3.2 | 1.1-2.4 |
| `it` test binary | 5.2-9.2 | 4.5-8.2 (1,287 tests) |
| `engine` test binary | | **0.8-2.6** (105 tests) |

What this shows:

- **An area that leaves the bottom crate gains its rebuild:** `cpd-core` 4-5 s against `cpd-engine` 0.5 s.
- **A small test binary gains the `it` compile:** 5-9 s against about 1 s.
- **An area that stays gains nothing.** A store edit pays one more crate in the chain (`cpd-engine` 0.6-2.1 s). The quick check went from 6.7 s to 7.9 s. That is inside the noise of single builds, but the split was slower in 5 of 8 interleaved pairs.
- **`it` barely shrinks with 105 fewer tests** (5.2-9.2 s against 4.5-8.2 s). Its compile is not proportional to what it holds at this size. Each extra binary also compiles `common` again (944 lines).
- **The workspace test build gains little** (11.7 → 7.7 s for the engine edit) because it still rebuilds `it`.

### 3.3 Which half buys what (the decomposition)

Two more blocks, 4 builds each, under load 29-36:

| edit | scoped binary | today | split | what differs |
|---|---|---|---|---|
| store (stays in `cpd-core`) | today `it`, split `engine` | 13.5 (11.9-14.7) | **8.1** (6.2-16.8) | **the test binary only**: `it` 5.2-8.0 s against `engine` 1.0-3.1 s |
| engine (moved) | `it` on both | 15.1 (7.2-25.9) | **9.8** (5.2-12.3) | **the crate only**: `cpd-core` 3.1-7.5 s against `cpd-engine` 0.4-1.5 s |
| engine | today `it`, split `engine` (§3.2) | 9.8 | **2.6** | both |

So each half is worth about 4-6 s for an edit whose tests are in its area. The test-binary half helps *every* area once each area has its binary (computed). The crate half helps only the crates at the top of the chain (§1.2: `cpd-engine`; `cpd-pr` after its cuts, inferred).

**Areas not prototyped** (inferred from the units above, not measured):

- **PR plumbing as `cpd-pr`:** like the engine (5.7k lines), once `plan↔pr` is cut.
- **Tree, Deploy, Plan view, status line:** stay in `cpd-tui`. A per-view crate could save at most `cpd-tui`'s 1-3 s. Their own test binaries would save `it`'s 5-9 s.
- **Store and migrations, events:** at the bottom of the chain. Nothing to gain from a crate.
- **pi adapter:** not Rust.

### 3.4 The test time of a scoped run (measured, nextest, a test slot each)

| area | filter (today) | tests | nextest run time | slowest | load |
|---|---|---|---|---|---|
| engine steps | `(package(cpd-core) & test(/^(watch\|doctor)::/)) \| (binary_id(cpd::it) & test(/^(watch\|watch_inbox\|doctor)::/))`; split: `package(cpd-engine) \| binary_id(cpd::engine)` | 147 | **19.7 s** on both trees | 15.6 s (`watch_inbox::a_restarted_engine_does_not_tell_the_manager_of_the_same_failure_again`) | 33 / 19 |
| tree and Inbox | `(package(cpd-tui) & test(/^tree::/)) \| (binary_id(cpd::it) & test(/^tui_tree::/))` | 247 | **11.0 s** | 9.5 s | 17 |
| store and migrations | `(package(cpd-core) & test(/^store::/)) \| binary_id(cpd-core::migrations) \| (binary_id(cpd::it) & test(/^(store\|store_window)::/))` | 203 | **16.3 s** | 7.7 s | 24 |

A split leaves test execution alone (19.7 s on both trees). An engine edit's loop, build plus run, goes from about 29.5 s to 22.3 s (computed: 9.8 + 19.7 and 2.6 + 19.7). Below the handoff's 30 s target either way, the run now dominates, and its floor is the slowest test (§3.4's 15.6 s). Waiting for a test slot (29-76 s here, #153) is not counted, and it is larger than either.

### 3.5 Conflicts: what the history says the split removes

**The method.**

1. Every merge commit on `refs/heads/cpd/*` and `refs/remotes/origin/cpd/*`: 268, from 2026-10-15 to 2026-10-20. Every one was redone with `git -c merge.conflictStyle=diff3 merge-tree --write-tree <parent1> <parent2>`.
2. **154 conflict**: 511 file conflicts and 1,254 hunks (7 file conflicts have no text hunk: modify/delete). The Job's grep (`git log --merges -i --grep=resolved --grep=conflict` over the same refs) finds 67 merges, of which 63 conflict on replay. 138 of the 154 bring `main` in.
3. Each hunk's two sides were compared with its base, and each changed line was given the unit it would live in under §2.2's cut:
   - **`commands.md`:** the `##` section's area. In §13.2 and §17, the view (from the table row's first cell, or the `**\`view\`**` paragraph it is in).
   - **`contract.md`:** the `###` section's area. §4.5 entries are one unit each, §5.2 rows go by the kind's area, and §4.4 lines by their `CREATE TABLE` or kind.
   - **Code files:** the file's area. A crate split moves whole files, so two PRs editing one file still conflict.
4. A hunk is **removed** when its two sides' changed lines fall in disjoint units, which means different files after the cut.

| | merges | clean after the cut | file conflicts | hunks |
|---|---|---|---|---|
| all replayed | 154 | **4** | 511 → 501 | 1,254 → **1,205** (−49, 3.9 %) |
| the grep's set | 63 | 4 | 180 → 172 | 461 → 440 |
| `commands.md` alone | 100 | | 100 → 94 | 274 → 239 (−35: §13.2 18, §17 17) |
| `contract.md` alone | 59 | | 59 → 55 | 154 → 140 (−14: §4.5 10, §4.4 2, §5.2 2) |
| code (crate split) | | | no change | 805 → 805 |

The 4 merges that would have been clean all had `docs/commands.md` as their only conflict, and all were on the Deploy-column branches:

- `44a43064eb`, `b82b7aa9f6` and `deba0a0b9b`: `tui-visuals/deploy-host`;
- `62552ffb67`: `pr-conflicts/deploy-flag`.

**Why so little.** Of the 1,254 hunks:

- **761 (61 %)** have both sides changing the same base line. No file split separates them.
- **137** are both sides inserting at the same point: lists, tables, the end of a section. Only these and the 49 cross-area hunks are what the cut addresses.
- **307** are adjacent edits of different lines, almost all within one area's section.

The unit mapping is a heuristic (a keyword-free structural one). I checked 10 hunks by eye, and the removed ones were what the table says (adjacent rows of different views). The exact count is not verified hunk by hunk.

**Against hs-n285's map** (15 branches, 2026-10-19):

- `commands.md` (11 PRs) and `contract.md` (7) are the only hot spots a cut touches, and then by 13 % and 9 % of their hunks.
- `core/src/store/schema.rs` is the pin (§2.1).
- `tui/src/tree/*` and `cli/tests/*` were pairs inside one area (dialogs×views, views×scroll-persist; onebin), so a split by area does not separate them.

## 4. Tests selected by reverse dependencies (handoff Phase 3's `test-changed`)

### 4.1 From a diff to the tests

1. **Changed files**:
   - `git diff --name-only $(git merge-base origin/main HEAD)`;
   - `git diff --name-only HEAD` (uncommitted);
   - `git ls-files --others --exclude-standard` (new files).
2. **Fallback to the full run.** Run all of it if any of these changed:
   - `Cargo.toml`, `Cargo.lock`, `rust-toolchain*`;
   - any `build.rs`. `core/build.rs` lists the named migrations, and `cli/build.rs` embeds HEAD;
   - `core/src/store/migrations/**` (the schema, every area's store);
   - `.config/**` (nextest, wt hooks), `scripts/cargo.sh`;
   - the shared test harness `cli/tests/it/common/**`;
   - a file that maps to no package and is not in the list below.

   The handoff's risk list (§7: build scripts, features, non-Rust inputs) is this list.
3. **Files to packages:** each workspace member's directory from `cargo metadata --no-deps --format-version 1` (`manifest_path`), longest prefix wins. Then files outside Cargo:
   - `pi/**` → `npm test` (not Rust);
   - `docs/interface.md` → `binary_id(cpd::it) & test(/^interface::/)`;
   - `docs/contract.md` → the store's §4.4 test (`test(/^store::/)`) and `cpd-tui`'s `docs_agree`;
   - `docs/commands.md` → `docs_agree`;
   - other docs → nothing.
4. **Reverse dependencies:** the transitive closure of reverse edges over the workspace members' `dependencies` (normal and dev) in `cargo metadata`. **nextest already computes it**: `rdeps(<package>)` is a filterset. It was measured on the prototype (`nextest list --workspace -E 'rdeps(cpd-engine)'` lists `cpd-engine`, `cpd-tui`, `cpd`, `cpd::it`, `cpd::engine`; `rdeps(cpd-tui)` lists `cpd-tui` and `cpd`'s binaries). The script then needs no graph code of its own: `-E 'rdeps(a) | rdeps(b)'`.
5. **The integration tests:** `cpd`'s tests drive the whole binary, so `rdeps(anything)` includes all of them. To narrow it, each integration module or binary belongs to one area. The rule: for a change in crate X, run the integration tests of every area whose crate is in `rdeps(X)`.
   - **Today** (one `it` binary), each area is a module filter: for example, engine = `binary_id(cpd::it) & test(/^(watch|watch_inbox|watch_team|doctor)::/)`.
   - **After §4.2**, each area is a binary: `binary_id(cpd::engine)`.

   This is a heuristic, not a proof. An integration test of one area may start another area's code through the binary (a tree test starts the engine). The Gate's full run stays the proof, and the script is the inner loop's.
6. **Run** `scripts/cargo.sh nextest run --workspace -E '<union>'`, then print what was selected and why (file → package → area).

**On today's three crates** this degenerates. Any `core` change selects `rdeps(cpd-core)`, which is everything. The script then picks by module path instead: `core/src/<m>/` → `test(/^<m>::/)` in `cpd-core`, plus `it`'s modules of area `<m>`. That is the handoff's "degenerates to name filters". With the split, `rdeps` does the work for the crates at the top of the chain (`cpd-engine`, `cpd-pr`, `cpd-tui`), and lower crates still select almost everything (§1.2's chain).

### 4.2 The integration binaries per area

- **The layout:** `cli/tests/<area>/main.rs` with `#[path = "../common/mod.rs"] mod common;`, as the prototype did. `common` moves from `cli/tests/it/common/` to `cli/tests/common/`.
- **The area table:** it lives in `cli/tests/<area>/main.rs` (its `mod` lines), so there is one source of truth. A test checks that every `.rs` under `cli/tests/` is in exactly one binary.
- **The areas:**

  | binary | modules |
  |---|---|
  | `engine` | `watch*`, `doctor` |
  | `pr` | `pr*`, `merge`, `git*` |
  | `flow` | `plan*`, `job`, `attempt`, `driver*`, `review*`, `spec`, `ask`, `metrics`, `context*`, `policy`, `config` |
  | `session` | `session*`, `host` |
  | `store` | `store*`, `events` |
  | `tui` | `tui*`, `inbox*`, `bare` |
  | `tooling` | `*_script`, `cargo_script`, `npm_fresh`, `install_script`, `harness`, `dead_code`, `interface`, `no_repo_lock`, `slot`, `e2e*` |

  That is 7 binaries. The split by module name is a proposal, not measured per binary.
- **The cost:** 6 more links (0.2-0.9 s each, in parallel; Phase 0 §2), and `common` compiled 7 times. #126 merged the per-file binaries into one, which made the cold build cheaper. 7 is a middle ground whose cold-build cost is unmeasured: the Job measures it before and after.

## 5. Migration plan and decision

### 5.1 Decision

| part | verdict | why (measured) |
|---|---|---|
| A. Integration tests, one binary per area (§4.2) | **approve** | −4 to −6 s per scoped build for any area (§3.3: 13.5 → 8.1 s). No product code moves. It makes area selection a binary, not a regex |
| B. `cpd-engine` = `watch` + `doctor`, `restart` injected into the cockpit | **approve** | engine edits: scoped 9.8 → 2.6 s, quick check 5.1 → 2.3 s (§3.2). It is the only area outside the cycle with no cut needed |
| C. `commands.md` §13.2 and §17 per view; §4.5 entries per migration (§2.2) | **approve, timed** | −45 of 1,254 hunks, and the only 4 merges that would have been clean. It is a move every open PR will conflict with once, so land it when the "tree and Inbox", "Deploy" and "Plan view" lanes have no PR in flight (hs-d324) |
| D. Per-view `tui` crates | **reject** | `cpd-tui` rebuilds in 1-3 s (§3.2). Conflicts inside it are same-file (§3.5) |
| E1. `cpd-base`, `cpd-store`, `cpd-flow`: layering the 17-module cycle | **reject** | It is cheap: 4 cuts, 5 references (§1.2). But these crates are the bottom of the chain, so an edit in them rebuilds everything above, as today. The measured proxy is a store edit with one more crate above it: 6.7 → 7.9 s. Code conflicts do not change |
| E2. `cpd-pr` (PR plumbing) | **defer** | 3 cuts, 5 references (`plan → pr`, `merge → pr`, `attempt → pr`). Its gain is inferred from B (5.7k lines at the top of the chain), not measured. Measure it the way §3.1 measured the engine once B has landed, if PR plumbing's edits become the loop that matters |
| F. Registries | **no split** | `SCHEMA_HASH` by `repin.sh` and `PINNED` deleted (PR `pin`, in flight); `Kind` stays one (§2.1) |

hs-d18 ("cpd's code is three crates") and `docs/implementing.md`'s Layout change with B. The User's approval of B amends hs-d18.

### 5.2 The Jobs, in the User's shape (hs-d341)

Order: **A first**. It touches no product code, and every area gains from it. Then **B**, which builds on A's `engine` binary. Then **C**, at a quiet moment. Each is one PR (hs-d342), and its open Job follows (hs-d340).

| PR | Job | kind | touches | acceptance |
|---|---|---|---|---|
| **areabins** (A) | ab-docs | docs | `docs/implementing.md` ("The one-area rule", "Tests"), `docs/commands.md` §15.12 (its examples: `--test <area>`), `.config/nextest.toml` (the filter comment) | they name `cli/tests/<area>/` and the area table of §4.2, and say `--test <area>` is a Worker's targeted build. The quick check passes |
| | ab-tests | red tests, a Worker other than ab-impl's | `cli/tests/it/layout.rs` (new) | named tests that fail against today's layout for a reason other than a build error: (1) every `.rs` under `cli/tests/` but `common/` is a module of exactly one `cli/tests/<area>/main.rs`; (2) the binaries are those of `docs/implementing.md`'s table |
| | ab-impl | implementation | `cli/tests/**` (moves only), `cli/Cargo.toml` if a `[[test]]` is needed | ab-tests pass. `nextest list --workspace` counts the same tests as before (number in the commit message). Cold `build --workspace --tests` and a `cli/src` edit's `build -p cpd --test <area>` measured before and after, in a slot, paired |
| **engine** (B) | en-docs | docs | `docs/implementing.md` (Layout: four members, `cpd-engine`; the module map), `CONTEXT.md` if it names the crates | the Layout says `cpd-engine` holds `watch` and `doctor`, depends on `cpd-core`, and `cpd-tui` does not depend on it (`restart` is passed in). The quick check passes |
| | en-tests | red tests | `cli/tests/tooling/crates.rs` (new) | a named test reading `cargo metadata` fails today: `cpd-engine` exists with `engine/src/watch` and `engine/src/doctor`; `cpd-tui` does not depend on `cpd-engine`; `cpd-core` has no `watch` or `doctor` module |
| | en-impl | implementation | `engine/**` (new, by `git mv`), `core/src/lib.rs`, `core/src/{config,context,attempt,driver,session,host}/` (8 items to `pub`), `cli/Cargo.toml`, `cli/src/cli.rs`, `tui/src/{lib,cockpit}.rs` (the injected `restart`), `Cargo.toml`, `Cargo.lock` | en-tests pass. The engine area's targeted tests pass (§3.4's filter). An `engine/src/watch/engine.rs` edit's scoped build is measured against today's, paired |
| **docsplit** (C) | dc-docs | docs only (no test Job, hs-d341) | `docs/commands.md`, `docs/commands/*.md` (new), `docs/contract.md` §4.5, `docs/migrations/*.md` (new), `docs/implementing.md` step 4, `scripts/new-migration.sh` (writes the entry file), `tui/src/logs/mod.rs` (`docs_agree` reads the new files) | every §13.2 and §17 sentence is in exactly one view file (a script's diff of the sentences shows none lost). `rg '§13\.2\|§17'` citations resolve. The `docs_agree` test passes, and so does the §4.4 store test, which is unchanged |

### 5.3 Risks

- **Load noise.** Single builds moved 2-4× with the load (§3.2's ranges). Each Job measures paired and interleaved, as here, not against these numbers.
- **The `-p` surprise** (Phase 0 §7). Every new package selection is a new feature set. The first `-p cpd --test <area>` in a target directory rebuilds `cpd` and some dependencies once (measured here: 6-32 s for the first switch).
- **The adapter and the slots.** `scripts/cargo.sh nextest …` takes a test slot even for `--no-run` and `list`, and the Worker guard refuses `test --workspace --no-run`. A `test-changed` script must build with `cargo build --tests` and run through nextest, or it waits for slots it does not need.
- **API widening.** B makes 8 items `pub`, which is the one-area rule's concern (hs-d9). Each should be named in en-impl's commit.
- **Docs readers.** Three tests read docs by section titles (§2.2). C must move their anchors with it.
- **What stays unmeasured:** the cold build with 7 test binaries; `cpd-pr`'s gain; the hunk mapping beyond the sample of 10.

### 5.4 The log (handoff §8)

| # | Change | Status | Cold build | Warm no-op | Incremental (1 file) | Full run | Notes |
|---|---|---|---|---|---|---|---|
| 6 | A: integration binaries per area | PROPOSED | unmeasured (ab-impl) | | scoped build after a store edit 13.5 → 8.1 s (prototype, `engine` binary) | unchanged (same tests) | 7 binaries |
| 7 | B: `cpd-engine` crate | PROPOSED | | | engine edit: scoped 9.8 → 2.6 s, qc 5.1 → 2.3 s, ws 11.7 → 7.7 s; store edit qc 6.7 → 7.9 s | unchanged | prototype deleted |
| 8 | C: per-view doc files | PROPOSED | | | n/a | n/a | −45 conflict hunks of 1,254 in the history |
| 9 | D, E1: tui per-view crates; layering the 17-module cycle | REJECTED (proposal) | | | at most 1-3 s (tui); none for low crates | | |
| 10 | E2: `cpd-pr` | PROPOSED, deferred | | | inferred like row 7 | | 3 cuts |

### 5.5 Report (handoff §9)

1. **What changed.** Only this file. A prototype on a local scratch branch was measured and deleted, and nothing was pushed.
2. **Measured.** §3.2-§3.5.
3. **Surprising:**
   - `cpd-core` is one 17-module cycle (§1.1).
   - A crate split removes no code conflict, and 61 % of the conflict hunks are both sides on the same line (§3.5).
   - `it` compiles about as slowly with 105 fewer tests (§3.2).
   - nextest has `rdeps()` built in (§4.1).
   - `scripts/cargo.sh nextest` takes a test slot for `--no-run` and `list` too (§5.3).
4. **Recommendation.** A, then B, then C (§5.2). Phase 3's `test-changed` script is simplest on top of A: a binary per area plus `rdeps()`.
5. **Open questions for the User:**
   - (a) Does B's approval amend hs-d18 (three crates)?
   - (b) Should the engine steps have one owner Plan (hs-n285 names none)?
   - (c) Should C wait for a moment with no PR in flight in the tree, Deploy and Plan-view lanes, or land as the next PR in those lanes?
