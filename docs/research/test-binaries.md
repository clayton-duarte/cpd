# One merged integration-test binary against today's 40 (tests-build-F5)

Research for finding `tests-build-F5` (`docs/research/quality-tests-build.md`), Job `tb-onebin` of Plan `quality`. It does not decide; the Lead and the User do. The experiment's code is **not committed**: it is described under "What the merge changes" and is mechanical. Anything not measured is labelled *unverified*.

## Summary

- Merging `cli/tests/*.rs` into one `cli/tests/it/main.rs` binary cuts **`target/` from 2.88 GB to 1.28 GB (-55%)**, the clean test build's CPU time by **-28%**, and the rebuild after a `core/` edit by **-73% CPU** (25 s → 9.5 s wall at median; wall is noisy). The **nextest wall time does not change** (same 1434 tests, within noise).
- It costs: editing one test file now recompiles the whole test crate (about +2.7 s, 1 s → 3.8 s), and 54 files change in a mechanical, one-commit way (below).
- **Recommendation: adopt it**, in its own Job, after the other test-file Jobs of Plan `quality` have merged (the move renames every `cli/tests/*.rs`, so it conflicts with any in-flight edit of them). Details at the end.

## Setup

- Machine: 16 cores, 62 GB, ext4 on NVMe. **The machine was shared with other Workers during every run**: load average at the start of each run was 28-94 (listed per run below). Wall times therefore move by up to 3×; user+sys CPU time (`/usr/bin/time`) moves by ±10% and is the number to read. Cache: `RUSTC_WRAPPER=` (sccache off), `CARGO_INCREMENTAL` default, no `CARGO_TARGET_DIR`: each variant ran in a private copy of the worktree (`rsync`, no `target/`) with its own `target/`, so neither disturbed the other's fingerprints.
- Baseline `main`: this branch at `640737d` (`main` `c7011f7` plus `use crate::args::{Name, PlanArg, Text, Title}`, which `main` needs to build: hs-n183), **53** `cli/tests/*.rs` files → **53** test executables (the finding counted 40; files were added since), plus 4 unit-test executables (`cpd` lib and bin, `cpd-core`, `cpd-tui`). `[profile.dev] debug = "line-tables-only"` (tests-build-F2) is already in `Cargo.toml`, so the sizes below are smaller than the finding's.
- Variant `it`: the same tree with the 53 files moved to `cli/tests/it/` (+ `common/`), `main.rs` = `mod common;` and one `mod <file>;` per file: **1** test executable. 1434 tests run in both (`nextest list`: 880 in `cpd::it`, 6 + 286 + 262 in the units).
- Each measurement N=3, variants interleaved (main, it, main, it, ...). Commands:
  - clean build: `rm -rf target; scripts/cargo.sh test --workspace --no-run`
  - size: `du -sb target` and `du -sb target/debug/deps` right after the clean build
  - incremental: after the build and test run, append `pub const TB_PROBE_n: u32 = n;` to `core/src/lib.rs`, then `scripts/cargo.sh test --workspace --no-run`
  - nextest: `scripts/cargo.sh nextest run --workspace --no-fail-fast` on a built tree (as the Gate and CI run it)

## 1. Clean test build (`test --workspace --no-run`)

| run | main wall | main CPU (user+sys) | it wall | it CPU (user+sys) | load at start (main / it) |
|---|---|---|---|---|---|
| 1 | 78.3 s | 227.2 s | 88.6 s | 182.9 s | 75 / 76 |
| 2 | 53.5 s | 242.7 s | 43.5 s | 150.4 s | 50 / 29 |
| 3 | 93.6 s | 265.1 s | 79.9 s | 175.3 s | 62 / 76 |
| **median** | **78.3 s** | **242.7 s** | **79.9 s** | **175.3 s** | |

CPU: **-28%** (range of `it` 150-183 s against `main` 227-265 s: no overlap). Wall: no difference visible under this load (the link steps of `main` run in parallel and the build is bounded by `libsqlite3-sys` and the crates on the critical path, tests-build-F3).

## 2. `target/` size (deterministic, same in all three runs)

| | main | it |
|---|---|---|
| `target/` | 2,882,422,5xx B (2.88 GB) | 1,283,233,5xx B (1.28 GB) |
| `target/debug/deps` | 2,095,286,277 B (2.10 GB) | 681,506,842 B (0.68 GB) |
| test + bin executables | 57 | 5 |

**-55%** on `target/`, **-67%** on `deps`. Executables of `main` are 8 MB (`cargo_script`) to 41 MB (`pr`), 1.6 GB in all (not 75-104 MB: F2 already halved them); `it` is one 65 MB file. Each of up to six Workers' worktrees (hs-d136) saves about 1.6 GB.

## 3. Incremental rebuild after a `core/` edit (`test --workspace --no-run`)

| run | main wall | main CPU | it wall | it CPU | load at start (main / it) |
|---|---|---|---|---|---|
| 1 | 25.4 s | 58.4 s | 10.1 s | 15.1 s | 59 / 67 |
| 2 | 10.2 s | 55.4 s | 9.5 s | 17.1 s | 33 / 32 |
| 3 | 25.8 s | 54.9 s | 8.4 s | 15.1 s | 93 / 54 |
| **median** | **25.4 s** | **55.4 s** | **9.5 s** | **15.1 s** | |

CPU **-73%** (55 → 15 s); wall median -63%, and 10.2 s against 9.5 s when the machine was quiet (run 2: load 33). The cost of `main` is relinking 53 executables (the finding's "relinks all 44"). A core edit with 880 tests is one crate compile + one link for `it`.

**The other side** (one edit appended to `cli/tests/pr.rs`, the largest file with 1905 lines; same command, N=3, load 51-53):

| | main | it |
|---|---|---|
| wall | 1.56, 1.09, 0.86 s | 3.80, 3.78, 2.45 s |
| CPU | 1.36, 1.19, 1.38 s | 3.80, 3.93, 3.17 s |

Editing a test file costs about **+2.7 s** with `it` (the whole 30 k-line test crate is recompiled, incrementally), 1 s with `main`. The saving on a `core/` edit (+15 s CPU) is paid for by this only after about six test-file edits per `core/` edit.

## 4. nextest wall time (`nextest run --workspace --no-fail-fast`, 1434 tests)

First set (N=3, after each clean build): `main` 180.3, 129.4, 294.1 s; `it` 243.6, 150.8, 190.4 s. **Invalid for `it`**: 5 extra failures (see "What the merge changes", the three tests that run their own binary by test name), fixed afterwards. Second set, valid, N=3 interleaved on the already built trees:

| run | main wall | main CPU | it wall | it CPU | load at start (main / it) |
|---|---|---|---|---|---|
| 1 | 170.0 s | 575.7 s | 165.1 s | 587.9 s | 45 / 61 |
| 2 | 178.2 s | 593.0 s | 252.7 s | 629.6 s | 54 / 91 |
| 3 | 268.3 s | 623.0 s | 152.2 s | 562.2 s | 74 / 42 |
| **median** | **178.2 s** | **593.0 s** | **165.1 s** | **587.9 s** | |

**No difference**: CPU 593 vs 588 s, wall inside the load noise (152-253 s against 170-268 s). Both ran 1434 tests, 1431 passed, **3 failed in both**: `watch::e5_pauses_below_100_requests...`, `watch::e5_polls_every_pr_poll_seconds_after_the_previous_poll_ended...`, `watch::e5_polls_with_a_plan_in_pr_too_...` (the E5 poll tests of hs-n178, Plan tui-dots' Job `e5-fix`; not touched here). nextest runs each test in its own process, so merging changes nothing for it: the saved cost is all in building and linking.

One run of cargo's own runner on the merged binary (`cargo test -p cpd --test it`, N=1, threads of one process, load 53): 877 passed, the same 3 E5 failures, 1 ignored, 227 s. So the per-file statics (`TMUX_ENV` in `host.rs`, `ONE_AT_A_TIME` in `driver_reload.rs`, `set_var` in `git.rs`/`host.rs`) did not break a run, but they are now shared by all tests of one process under `cargo test` (unverified beyond this one run; nextest is not affected).

## What the merge changes (the experiment, not committed)

1. `git mv cli/tests/*.rs cli/tests/it/`, `git mv cli/tests/common cli/tests/it/common`, new `cli/tests/it/main.rs` (54 lines: `mod common;` and `mod x;` per file).
2. In the 50 files that have `mod common;`: replace it by `use crate::common;` (a `mod common;` inside `ask.rs` would look for `it/ask/common.rs`).
3. Five hard-coded paths and names that the move breaks (each found by a failing run, not by reading):
   - `tui_plan.rs`: `include_str!("../../tui/src/plan/fixtures/cpd-chart.txt")` → one more `../` (compile error).
   - `harness.rs`: `no_fake_sleeps_longer_than_sleep_seconds` reads `CARGO_MANIFEST_DIR + "/tests"` → `"/tests/it"` (it would scan nothing and pass).
   - Three tests that start `current_exe()` with a test name now need the module path: `harness.rs` `"--exact", "killed_test_helper"` → `"harness::killed_test_helper"`; `store.rs` `"child_writer"` → `"store::child_writer"`; `tui.rs` `the_ambient_gh_ssh_and_proxy_variables_do_not_reach_a_command` → `tui::...`. Without it the child runs 0 tests and the parent fails: 5 failures in the first nextest set (`harness::a_killed_test_leaves_no_tmux_server_process_dir_or_reaper`, `tui::a_command_has_no_gh_ssh_or_proxy_variable`, and three `store` tests that run `child_writer`).
4. `.config/nextest.toml`: `binary_id(cpd::host)` → `binary_id(cpd::it) and test(/^host::/)`, same for `driver_reload` (the groups `tmux-env` and `driver-reload` must keep matching, or `host` and `driver_reload` tests run in parallel). Not verified by a deliberate break; the 1434 tests passed with it.
5. Not changed by the experiment, to change with it: `docs/implementing.md:119` (`nextest run -p cpd --test host` → `nextest run -p cpd -E 'test(/^host::/)'` or a name filter), and the comments in `.config/nextest.toml` (`cli/tests/host.rs`).

Not affected: test names in nextest (`cpd::it host::x` instead of `cpd::host x`), `CARGO_BIN_EXE_cpd`, `CARGO_MANIFEST_DIR` uses elsewhere (`cargo_script.rs`, `command_index.rs`, `preview_script.rs` use `../scripts` and `../docs` from the manifest dir, not from the file).

## Recommendation

**Adopt one binary** (`cli/tests/it/main.rs`, cargo's own guidance), as one Job at a quiet point:

- Gains are certain and large where Workers pay: -1.6 GB of disk per worktree, -28% clean-build CPU, -73% CPU on the `core/` edit that every Job makes. The nextest run, which the Gate pays, neither gains nor loses.
- Losses are small: +2.7 s after editing a test file; a compile error in any test file now stops all test builds (already true for `cargo test --workspace --no-run`).
- Do it **after** the Jobs that edit `cli/tests/*.rs` have merged (tests-build W1/W2/G1/G2 and every feature Plan's tests), and in one commit of renames (`git mv`) so history follows; a rename conflicts with every in-flight edit of those files. The Lead resolves the conflict with `main` (hs-d220).
- Acceptance for that Job: `nextest list` gives the same 1434 tests as before; the three self-spawning tests of item 3 pass; `no_fake_sleeps_longer_than_sleep_seconds` still scans files (add an assertion that it saw more than 50 files); the nextest groups of item 4 still apply (check with `nextest show-config test-groups`).
- Do **not** merge the binary for the sake of the nextest wall time: it is not measurably affected. If the User wants speed there, it is tests-build-F4 (constants that tests wait on) and W4 (load-proof waits).
