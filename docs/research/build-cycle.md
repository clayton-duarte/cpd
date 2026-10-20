# Research: the build and test cycle's baseline (Plan quality, Job cy-baseline, hs-n300 Phase 0)

Phase 0 of the handoff `~/.cpd/run/handoffs/HANDOFF-rust-test-speed.md` (hs-n300): measure the cycle as it is, change nothing, and say whether the time goes to compile, link or test execution. Main `c5e11bbb` (`origin/main` = this Job's base, no #159: dependencies still at `opt-level 0`, no `scripts/test-time.sh` changes, no per-test limit). Machine hs: 16 CPUs, 62 GiB, shared with other Workers' builds and Gates.

Every number here was measured on hs unless it says "computed" (arithmetic on measured numbers), "inferred" or "unverified".

## Summary

- **The inner loop is compile, not link, not tests.** An edit plus the targeted tests a Worker runs takes **3.7-12 s** end to end (§5); the tests themselves take 0.01-0.04 s. Link is 0.3-1.5 s of it. The linker is already **rust-lld** (rustc 1.99's default on this target, §1), so the handoff's "mold" item has nothing to replace.
- **The floor of an edit is the `it` test binary.** `cli/tests/it` is 44,856 lines (1,343 tests) in one crate; the `cpd` library it depends on is 452 lines. Any edit in `core`, `tui` or `cli` rebuilds it: **6.7-11 s of rustc**, 70-90 % of a `cli` edit's whole build, even for a `touch` (§4).
- **A cold build of the tests is 47-63 s and 190-230 CPU-s** with or without sccache (§2). sccache only shortens the dependency phase (9 s against 15-20 s before the first workspace unit starts); the workspace crates are built incrementally, which sccache does not cache. The workspace's own crates are 68-69 % of the cold wall clock (computed from `--timings`); link is about 1 % (§2).
- **The Gate's full run is test execution.** The run is **157-198 s** (sum of test times 2,428-2,974 s, 6.0 of the 16 CPUs busy), against 39 s + 38 s of cold build or about 6 s + 7-23 s of incremental build (§3, §6).
- **Doctests: none exist.** `scripts/cargo.sh test --doc --workspace` finds 0 tests in the 3 libraries and takes 1.2-1.5 s (§3.2). Every doc code block is ```` ```text ````.
- **Surprises** (§7): `-p <crate>` rebuilds 12 dependencies once after a `--workspace` build (20-28 s); every `git commit` rebuilds `cpd` (the `build.rs` embeds HEAD, 4 s); cargo's no-op is 0.3-0.5 s.

## 0. Conditions and method

- Tools: cargo 1.99.0 (5f94df478 2026-08-27), rustc 1.99.0 (b940084d7 2026-09-28), cargo-nextest 0.9.148, sccache 0.7.7, `cc` 13.3.0 as the link driver, Linux 6.8.0, `nproc` 16. No `.cargo/config.toml`; `[profile.dev] debug = "line-tables-only"` is the only profile deviation (Cargo.toml).
- **Load.** hs never was calm: the 1-minute load average at the start of each measurement was **20-48** (one number per row below, `uptime` before each run, the full text is in the Attempt's logs, not committed). That is 1.3-3 runnable tasks per CPU from other Workers' builds and Gates. Read every time as "under load 20-48"; rows were run in sequence, not interleaved, so only the pairs said to be paired compare.
- **Target directory**: `CARGO_TARGET_DIR=/tmp/cy-a1/target`, my own, so no other build was disturbed or waited for. Cold = `scripts/cargo.sh clean` first. nextest wrote its JUnit report under the worktree's `target/` (as `docs/research/test-speed.md` §1 says).
- **sccache**: "off" = `RUSTC_WRAPPER=` (the opt-out of `scripts/cargo.sh`); "on" = unset, so `scripts/cargo.sh` exports `RUSTC_WRAPPER=sccache`. For the cache to be cold and the numbers to be mine, sccache ran on its own server (`SCCACHE_SERVER_PORT=4999`) and its own directory (`SCCACHE_DIR=/tmp/cy-a1/sccache`), emptied for the "cold cache" run; the server was started inside the measured scope so its CPU is counted. Incremental and inner-loop rows are with sccache on (what Workers have); a paired comparison with it off follows in §4.
- **Slots**: every cold build, every full run and the doctests ran under a Gate slot (`flock … gate-a.lock`; slot a was free each time). The small incremental builds did not.
- **CPU and memory** come from a cgroup scope per measurement (`systemd-run --user --scope`, `cpu.stat` and `memory.peak`, which cover every descendant, also detached tmux servers and the sccache server) and from `/usr/bin/time -v` (the largest single process: "max RSS"). `memory.peak` counts page cache too, so it is an upper bound on what the processes hold.
- **Compile against link**: cargo's `--timings` gives each unit's compile-plus-link time; the link itself is timed from `strace -f --seccomp-bpf -e trace=execve,exit_group` (the lifetime of each `rust-lld` process, the same technique as `test-speed.md` §4). Link figures are those lifetimes (`cc` and `collect2` add about 10 ms).
- The measuring scripts were throwaway shell and Python in `/tmp/cy-a1` (removed at the end); nothing in the repository changed but this file.

## 1. Counts

| what | count |
|---|---|
| workspace members (`cargo metadata --no-deps`) | **3**: `cpd-core` (`core`), `cpd` (`cli`), `cpd-tui` (`tui`); plus `pi/` in TypeScript, outside Cargo |
| source lines | `core/src` 44,646 and `core/tests` 782; `cli/src` **452** and `cli/tests` **44,856**; `tui/src` 18,721 |
| test binaries (`nextest list`) | **6**: `cpd` lib 6 tests, `cpd::it` **1,343** (one binary of 70 files, `cli/tests/it/main.rs`, #126), `cpd::bin/cpd` 0, `cpd-core` lib 503, `cpd-core::migrations` 19, `cpd-tui` lib 429 |
| tests | **2,300** listed; a run starts 2,297 (3 skipped; 4 `#[ignore]` attributes by grep: the link between the two is unverified) |
| doctest binaries / doctests | 3 (`cpd`, `cpd_core`, `cpd_tui`) / **0** |
| test binaries linked by a cold test build | 6, plus the `cpd` bin; `rust-lld` ran **36 times** in all (the other 29: proc-macros and build scripts) |
| linker | **rust-lld**: `rustc` 1.99 runs `cc` with `-fuse-ld=lld` through the toolchain's `gcc-ld/ld.lld` and `rust-lld` (seen in the `execve`s of every link). No `mold`, no `.cargo/config.toml`. |

## 2. Cold build of the tests

`scripts/cargo.sh clean`, then `scripts/cargo.sh nextest run --workspace --no-run --timings`, in a slot. CPU = user + system seconds of the whole scope; "peak" = cgroup `memory.peak`; "max RSS" = the largest process (rustc, `cpd::it`).

| run | sccache | load (1 min) | wall (s) | CPU (s) | peak (MiB) | max RSS (MiB) |
|---|---|---|---|---|---|---|
| 1 | off | 45.5 | **47.0** | 191 | 2,634 | 1,112 |
| 2 | off | 29.9 | **63.3** | 218 | 2,625 | 1,105 |
| 3 | off | 26.8 | **54.9** | 204 | 2,588 | 1,102 |
| 4 | on, cache cold | 26.9 | **58.9** | 230 | 2,810 | 1,114 |
| 5 | on, cache warm | 40.5 | **54.4** | 159 | 2,733 | 1,121 |

(An earlier run with the cold-cache sccache server outside the measured scope: 74.2 s at load 28; its CPU was not counted, so the run above replaced it.)

- **No wall-clock gain from sccache, a CPU gain of about a fifth.** The five wall times overlap (47-63 s off, 54-59 s on); the spread of the three identical "off" runs (47-63 s) is as large as any difference. The warm cache saves CPU: 159 s against 191-218 s. sccache's own counts for run 5: 113 of 113 cacheable compilations hit; 52 calls were non-cacheable (30 for `crate-type`, 9 for `incremental`, the rest build-script and cargo probes).
- **The dependency phase is what the cache shortens.** From `--timings`, the first workspace library unit (`cpd-core`) starts at 15.0 / 20.1 / 16.7 s without sccache and at 9.0 s with a warm cache. The workspace units cannot be cached: they are `incremental` (dev profile), which sccache refuses; in run 5 their time was 94 unit-seconds against 67-91 without.
- **Where the wall clock goes (computed from `--timings`, the three "off" runs):** 153 dependency units add up to 135-186 unit-seconds and end by 15-20 s; the 14 workspace units add up to 67-91 unit-seconds and the chain `cpd-core` → `cpd-tui` → `it` is **31.7 / 42.8 / 37.8 s of the 46.7 / 62.9 / 54.6 s, 68-69 %**. The slowest units in run 1: `cpd::it` test 16.8 s, `cpd-core` test 15.5 s, `cpd-core` library 14.2 s, the `libsqlite3-sys` build script (compiles the bundled SQLite in C) 9.6 s, `cpd-tui` test 7.6 s and library 5.2 s, then dependencies at 3-5 s each (`minijinja`, `syn`, `rustix`, `clap_builder`, `time`).
- **Link is small.** In run 1 the 7 links of the final binaries took 2.2 s added up (`it` 0.46 s, the others 0.15-0.38 s); all 36 `rust-lld` runs 6.7 s, 3.5 % of the 191 CPU-seconds. The last link on the critical path (`it`, 0.46 s) is 1 % of the 47 s.
- **Memory**: the cold build peaks at 2.6-2.8 GiB for the whole scope; a single rustc process at about 1.1 GiB.

### 2.1 The Gate's two builds, cold (no sccache, slot)

`npm run verify` builds twice: `scripts/cargo.sh build --workspace`, then the tests. Measured from a clean target directory, one after the other:

| step | load | wall (s) | CPU (s) | peak (MiB) |
|---|---|---|---|---|
| `build --workspace` (dev profile) | 25.2 | **39.2** | 133 | 1,619 |
| `nextest run --workspace --no-run` after it | 30.7 | **37.6** | 103 | 2,066 |

Together 76.8 s of build before a test runs. The second does not reuse the first's workspace artifacts (the test profile builds the libraries again with `cfg(test)`), only the dependencies.

## 3. Warm no-op, full run, doctests

### 3.1 Warm no-op

`scripts/cargo.sh nextest run --workspace --no-run`, nothing changed (3 runs each, load 22):

| sccache | wall (s) | CPU (s) |
|---|---|---|
| on | 0.28, 0.28 | 0.37 |
| off | 0.41, 0.45, 0.44 | 0.5 |

cargo's own "Finished … in 0.08-0.16 s". The first no-op after a build took **4.2 s** on its own: I had committed in between, and `cli/build.rs` (`CPD_VERSION` = version + `git rev-parse --short HEAD`, with `rerun-if-changed` on `.git/HEAD` and the branch ref) reruns on every commit and rebuilds `cpd` (§7).

### 3.2 Full run, warm

`scripts/test-time.sh` (builds, then `nextest run --workspace --profile timing`), in a slot, main `c5e11bbb`, 2,297 tests, all pass:

| run | load before → after (1 min) | wall (s) | sum (s) | wall − sum/16 (s) | CPU of the scope (s) | CPUs busy | peak (MiB) |
|---|---|---|---|---|---|---|---|
| 1 | 35.9 → 36.7 | **197.8** | 2,974 | 11.9 | 1,002 | 4.8 | 1,874 (includes a rebuild of a reverted file by the script's own `--no-run`; max RSS 718 MiB) |
| 2 | 20.8 → 28.9 | **157.1** | 2,428 | 5.4 | 946 | 6.0 | 841 (nothing compiled; max RSS 54 MiB) |

(`CPUs busy` = scope CPU over the scope's wall clock, including the no-op build.) Per binary, run 2 (sum s, tests, slowest): `cpd::it` 2,182 s, 1,341 tests, 68.7 s; `cpd-core` 225 s, 502, 17.6 s; `cpd-tui` 14.2 s, 429, 0.65 s; `cpd-core::migrations` 6.0 s, 19; `cpd` 0.12 s, 6. The two slowest tests are the 60 s adapter-watchdog ones (66-69 s). The `cpd::it` binary is 90 % of the sum; the 956 library tests (`cpd-core`, `cpd-tui`, `cpd`, `migrations`) are 245 s of 2,428, 10 %.

The numbers match `docs/research/test-speed.md` (which measured the base at 120 s calm and about 1,500 s of sum for main): at load 20-36 the same suite takes 157-198 s and 2,428-2,974 s, the same 30-100 % inflation that document found. The wall clock is the sum divided by 16 plus a small tail (5-12 s here). More tests run at once than the CPUs can serve only partly, because the tests mostly wait (the test-speed document's finding): 4.8-6.0 of 16 CPUs busy.

### 3.3 Doctests

`scripts/cargo.sh test --doc --workspace`, in a slot, load 34.6, two runs: **1.5 s and 1.2 s**, 214 and 206 MiB peak. It runs `rustdoc` on 3 libraries and finds **0 doctests** in each (`running 0 tests`). The doc code blocks the Job named (`core/src/events/append.rs`, `core/src/args.rs`, `tui/src/cockpit.rs`; `core/src/plan/render.rs` has fences only inside strings and a comment, no fenced block) are all ```` ```text ````, which rustdoc does not compile. So "nextest skips doctests" costs nothing today; the gap is a rule for the future (a ```` ```rust ```` block would not run in the Gate).

## 4. Incremental build after a trivial edit

`scripts/cargo.sh nextest run --workspace --no-run --timings`, sccache on, warm target directory, each build under strace for the link times. Three states per file: **touch** (only mtime), **body** (inserted `let _cy_edit = 1;` as the first line of a `pub fn` body in the file: a real change to the function's code, none to its API), **revert** (`git checkout` of the file: another change; the file is clean again afterwards). "units" = cargo units rebuilt (library, test and bin units, build scripts included).

| file | what it is | state | load | wall (s) | crates compiled | units | `it` unit (s) | links (n, sum s; `it`) |
|---|---|---|---|---|---|---|---|---|
| `core/src/error.rs` | core, used by every area (151 `crate::error` uses) | touch | 25.0 | 14.2 | `cpd-core`, `cpd-tui`, `cpd` | 10 | 8.0 | 7, 3.7 s; 0.84 |
| | | body | 23.9 | 22.7 | same | 11 | 11.0 | 7, 5.0 s; 0.81 |
| | | revert | 25.7 | 17.6 | same | 10 | 9.1 | 7, 3.1 s; 0.48 |
| `core/src/metrics/mod.rs` | core, leaf area (3 users) | touch | 22.1 | 13.1 | same | 10 | 8.7 | 7, 3.1 s; 0.45 |
| | | body | 19.9 | 19.9 | same | 10 | 10.7 | 7, 5.3 s; 0.71 |
| | | revert | 23.1 | 17.3 | same | 10 | 10.2 | 7, 3.8 s; 0.78 |
| `tui/src/fold.rs` | tui, leaf | touch | 26.3 | 12.1 | `cpd-tui`, `cpd` | 7 | 10.0 | 5, 3.2 s; 0.88 |
| | | body | 29.3 | 12.3 | same | 7 | 9.8 | 5, 3.5 s; 0.82 |
| | | revert | 33.5 | 10.0 | same | 7 | 8.0 | 5, 2.7 s; 0.59 |
| `cli/src/interface.rs` | cli, leaf | touch | 30.4 | 8.8 | `cpd` | 5 | 8.3 | 4, 2.1 s; 0.74 |
| | | body | 29.5 | 8.2 | same | 5 | 7.2 | 4, 2.2 s; 0.71 |
| | | revert | 28.0 | 7.1 | same | 5 | 6.7 | 4, 1.9 s; 0.56 |

(The wall clock is the strace-wrapped build; the same builds without the `--timings` and strace in §4.1 are within noise.) What this shows:

- **Every edit rebuilds `cpd::it`**, the 44.9k-line integration binary: 6.7-11.0 s, **70-90 % of a `cli` edit's build** (8.2 s of 8.8 s for a touch) and the longest unit in every row. A touch with no content change costs the same as a body edit (8.0 against 11.0 s for `error.rs`, 8.3 against 7.2 s for `interface.rs`): rustc reruns the whole test crate when a dependency's rlib is rebuilt (why its incremental cache does not absorb this is unverified).
- **The crate level is not precise**: an edit in the leaf `core/src/metrics/mod.rs` costs the same as one in `error.rs` (13-20 s against 14-23 s); `cpd-core`, `cpd-tui` and `cpd` and all 6 test binaries rebuild. A `tui` edit skips `cpd-core` (7 units, 10-12 s); a `cli` edit rebuilds only `cpd` (5 units, 7-9 s).
- **The links**: 4-7 `rust-lld` runs, 1.9-5.3 s added up (they overlap); the last one, `it`, ends the build and takes 0.45-0.88 s, **3-10 % of the wall clock**.
- **sccache does nothing here** (paired, `core/src/error.rs` body then revert, two rounds, load 26-48; wall seconds): on 14.3 / 15.4 / 17.5 / 16.1; off 17.2 / 15.0 / 18.9 / 16.2. The differences are inside the load noise (the workspace units are `incremental`, which sccache does not cache).
- **Peak** 0.8-1.9 GiB for the scope (cgroup `memory.peak`; the `tui` and `cli` rows 0.8-1.1 GiB).

### 4.1 The quick check (pre-commit hook)

`npm run typecheck` 2.7, 2.0, 2.1 s (load 31). `scripts/cargo.sh build --workspace` after a `core/src/error.rs` edit and its revert (4 builds, load 29-32): **5.6, 5.8, 6.6, 5.6 s** (`cpd-core`, `cpd-tui`, `cpd` rebuilt). The dev-profile `cpd-core` does not rebuild the test units, so the quick check is the cheapest build in this document.

## 5. The Worker's inner loop (hs-d338)

The Worker edits a module and runs only the tests that module has (`docs/implementing.md`, the commands of `docs/commands.md`: `scripts/cargo.sh nextest run -p <crate> -E 'test(/^<module>::/)'`). Each row: revert, a `body` edit with a new value, then one command, timed end to end (build plus tests), 3 reps, sccache on, warm target directory, load 31-37 (strace runs of the same commands were made once more for the links).

| edit | command | tests | rep 1 | rep 2 | rep 3 | rebuilt | test execution | links |
|---|---|---|---|---|---|---|---|---|
| `core/src/metrics/mod.rs` (leaf area of core) | `-p cpd -E 'test(/^metrics::/)'` (the module's tests are in `cli/tests/it/metrics.rs`) | 8 | 11.7 s | 11.7 s | 10.0 s | `cpd-core`, `cpd-tui`, `cpd` (the `it` binary needs all three) | 0.02 s | 4, 1.4-1.5 s; `it` 0.4 |
| `core/src/error.rs` (core, wide) | `-p cpd-core -E 'test(/^error::/)'` | 6 | **27.9 s** | 5.4 s | 5.1 s | `cpd-core` only (rep 1 also 12 dependencies, see §7) | 0.01 s | 2, 0.5 s |
| `tui/src/fold.rs` | `-p cpd-tui -E 'test(/^fold::/)'` | 6 | 7.7 s (rebuilt `cpd-core` too: the previous row's revert) | 3.8 s | 3.7 s | `cpd-tui` | 0.02-0.04 s | 1, 0.3-0.4 s |

(Strace runs of the same commands: core leaf 10.6 and 9.9 s; core wide 19.5 s with the dependencies and 5.3 s; `tui` 7.3 and 3.1 s.) For the same `error.rs` edit the workspace test build (§4) is 14-23 s and the pre-commit `build --workspace` 5.6-6.6 s.

**The inner loop is compile.** Test execution is 0.01-0.04 s of 3.7-11.7 s (under 1 %); link is 0.3-0.5 s of the `-p cpd-core` and `-p cpd-tui` rows (6-10 %) and 1.4-1.5 s added up of the `-p cpd` row (the `it` link, 0.4 s, is 4 %). The rest is rustc: one crate's incremental rebuild (3-5 s for `cpd-core`, 3 s for `cpd-tui`), and for an edit whose tests live in `cpd::it`, the 7-11 s rebuild of that binary. **Scoping by package is worth 2-4×** (`-p cpd-core` 5 s against 14-23 s for the whole workspace's tests), but a Worker whose tests live in `cli/tests/it` pays the `it` rebuild (10-12 s) whatever it scopes. Every steady-state loop is under the handoff's 30 s target (3.7-11.7 s); only the first `-p cpd-core` run in a target directory comes near it (27.9 s, §7). The `it` compile is the floor for any edit whose tests are there.

## 6. The conclusion: compile, link or test execution

| | compile (rustc) | link (rust-lld) | test execution |
|---|---|---|---|
| **Inner loop** (§5, one edit and its tests) | 3-11 s, **over 90 %** | 0.3-1.5 s, 4-10 % | 0.01-0.04 s, under 1 % |
| **Incremental test build** (§4) | 7-23 s, 90 %+ | 1.9-5.3 s added up, last link 3-10 % of wall | n/a |
| **Cold test build** (§2) | 47-63 s: dependencies to 15-20 s, then the workspace chain 32-43 s (68-69 %) | 2.2 s for the 7 final links (about 1 %) | n/a |
| **The Gate's full run on a fresh target** (§2.1, §3.2) | 39 + 38 = 77 s (28-33 % of 234-275 s) | about 2-3 s | 157-198 s (**57-72 %**) |
| **The Gate's full run, warm after a change** (§3.2, §4, §4.1) | about 6 + 7-23 s (7-16 %) | about 1-3 s | 157-198 s (**84-93 %**) |

- **Inner loop: compile dominates**, and inside the compile the `it` test binary is the largest single unit of every build that touches it. **Link is not the bottleneck** (rust-lld already; 0.2-0.9 s per link). **Test execution is not** (scoped tests run in tens of milliseconds).
- **Gate's full run: test execution dominates**: 157-198 s, 2,428-2,974 s added up, spent mostly waiting (4.8-6.0 of 16 CPUs busy), with `cpd::it` 90 % of the sum. A cold Gate adds 77 s of build (a third); a warm one 13-29 s (7-16 %).
- **The handoff's premises that do not hold here**: "mold over ld" (the linker is rust-lld, links take under a second each); "merge the integration binaries to cut links" (done in #126: one `it` link is 0.5-0.9 s, but the one binary is recompiled whole on any change, 7-11 s, which is now the cost to measure against what the merge saved); "doctests are skipped by nextest" (there are none).

## 7. Surprises

- **`-p <crate>` after a `--workspace` build rebuilds dependencies once.** In two target directories the first `-p cpd-core` run compiled 12 dependencies again (`syn`, `hashbrown`, `rustix`, `time`, `hashlink`, `rusqlite`, `serde_derive`, `clap_derive`, `tempfile`, `clap`, `serde`): 27.9 s (and 19.5 s under strace) against 5.1-5.4 s afterwards. I infer it is a different feature set for the selected packages (cargo unifies features per selection); unverified, not checked with `cargo tree -e features`. It is a one-time cost per target directory, but a Worker's first `-p` run pays it.
- **A commit rebuilds `cpd`.** `cli/build.rs` watches `.git/HEAD` and the branch ref for `CPD_VERSION`; after my skeleton commit the next build compiled `cpd` again (4.2 s, 0.7 GiB). Every Worker commit costs this before the next test.
- **sccache does not help the loop that matters.** Warm sccache cuts CPU (159 against 191-218 CPU-s cold) and the dependency phase, not the incremental builds (not cached) nor the workspace chain.
- **Touch costs the same as an edit** for the `it` binary (§4).
- **`cargo build --workspace` and the test build do not share the workspace units** (§2.1): the Gate builds the workspace twice.

## 8. The log (handoff §8)

All times under load 20-48 on hs, sccache on unless noted, main `c5e11bbb`; the sections above give each number's conditions.

| # | Change | Status | Cold build | Warm no-op | Incremental (1 file) | Full run | Notes |
|---|--------|--------|-----------|-----------|---------------------|----------|-------|
| 0 | Baseline | MEASURED | 47.0 / 63.3 / 54.9 s sccache off; 58.9 s (cold cache) and 54.4 s (warm) on; CPU 159-230 s; peak 2.6-2.8 GiB | 0.28 s on, 0.41-0.45 s off | `core` 13-23 s (wide and leaf alike); `tui` 10-12 s; `cli` 7-9 s; the `it` unit 6.7-11 s of each; links 0.2-0.9 s each | 157.1 s (load 21-29) and 197.8 s (load 36); sum 2,428 / 2,974 s; 2,297 tests; doctests 0 (1.2-1.5 s) | compile dominates the loop, test execution the Gate's run; the linker is already rust-lld |
| 1 | nextest | TODO | | | | | already on main (`.config/nextest.toml`); the baseline above includes it |
| 2 | linker | TODO | | | | | rust-lld is the default (§1); mold would replace a link of 0.2-0.9 s |
| 3 | debuginfo | TODO | | | | | `line-tables-only` is on main; `debug = 0` not tried |
| 4 | merge integration tests | TODO | | | | | done on main (#126); baseline includes it |
| 5 | scoped-run script | TODO | | | | | `-p` scoping measured (§5), no script yet |

## 9. Report (handoff §9)

1. **What changed.** Only this file: the Phase 0 measurements. No code or configuration.
2. **Measured.** §2-§6: cold 47-63 s and 190-230 CPU-s; no-op 0.3-0.5 s; incremental 7-23 s by crate; full run 157-198 s; doctests none; the inner loop 3.7-11.7 s with tests under 1 %.
3. **Surprising.** The linker is already rust-lld and cheap (§1, §2). One 44.9k-line test crate is recompiled whole on any edit and is the longest unit of every incremental build (§4). sccache does nothing for incremental builds and little for the cold one's wall clock (§2, §4). `-p` after `--workspace` rebuilds dependencies once (§7). A commit rebuilds `cpd` (§7). There are no doctests (§3.3).
4. **Recommendation for Phase 1.** Phase 1's list is mostly done or empty (nextest and `line-tables-only` are on main; the linker is lld; sccache is on). What is left, in the order of expected payoff for the loop, each one change with a before/after:
   1. **`debug = 0` for the dev profile** (the handoff's second experiment), and **`debug = false` for dependencies only** (`[profile.dev.package."*"]`): a config line each; the first cuts the work of the 7-11 s `it` rebuild and the links, the second the cold build's 153 dependency units. Check the backtraces stay readable first.
   2. **Hold #159's `opt-level = 3` dependencies against the cold build**: it makes dependencies compile slower (cold, and the 12 re-built by the `-p` surprise) while it speeds the tests; its number belongs in this log's row 0 comparison, not in a guess.
   3. **Do not pursue mold**: 36 links in 6.7 s, the 7 final ones in 2.2 s; the best case is a fraction of a second per edit.
   4. **Phase 3 (scoped runs) is where the loop's seconds are**: `-p cpd-core` is 5 s against 14-23 s for the workspace; a `test-changed` script that picks `-p` from the changed files is worth more than any build flag. It cannot remove the `it` rebuild for edits whose tests are in `cli/tests/it`.
   5. **Phase 4 question, not for now**: the `it` binary (44.9k lines, 1,343 tests) is the floor of the loop, and Phase 2 merged it. Whether it should be split again by area (separate binaries per area link in 0.2-0.5 s each but would compile only what an edit reaches) is the measurement the handoff's order did not have; propose it only with a before/after.
   For the Gate's full run (157-198 s), build settings are at most a third of a cold Gate and a tenth of a warm one: the lever is test execution (`docs/research/test-speed.md`).
5. **Open questions for the owner.** (a) hs was never calm (load 20-48): do the numbers need a repeat at a quiet hour before Phase 1 compares against them, or is the load the normal condition to optimise for? (b) May the dev profile drop to `debug = 0` (no line tables in backtraces), or is a readable backtrace in a failing Gate required? (c) Is it acceptable that a Worker's first `-p` run in a new target directory costs 20-28 s (§7)? (d) `npm test` (the TypeScript half of the Gate) was not measured here (outside the Rust cycle): should the Phase 0 baseline include it?
