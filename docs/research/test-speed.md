# Research: where the Rust test run's time goes (Plan quality, Job sp-profile, hs-d284)

Base `8df1af8` (main). Machine hs: 16 CPUs, shared with other Workers' builds and Gates (each number says what ran beside it). Target (hs-d284): `scripts/cargo.sh nextest run --workspace`, build excluded, at most **30 s on the clock** and **300 s added up**, every test kept.

Every number here was measured on hs unless it says "computed" (arithmetic on measured numbers) or "unverified".

## Summary

- **Before** (3 runs with no other test run beside them): **120.5-123.6 s on the clock, 1,408-1,487 s added up, 1,728 tests** (§2). With another test run beside it the same suite takes 125-149 s and 1,525-2,018 s. **Today's main** (`74d5f99`, with #104) has 1,830 tests: run beside the base under load 75-113 its sum matched the base's on the shared tests, and its 102 extra tests add about 100 s calm (computed, §2.1), so main's before is about **1,500 s**.
- **The clock is the sum divided by 16, plus a 30 s tail.** For the first 90 s all 16 test threads are busy; then only the two `session_watchdog` tests (67-69 s, started at 52 s) run. With a nextest `priority` on the slow tests (config only, measured) the tail is gone: clock = sum / 16 + 1.8 s (§3). So the clock follows the sum: 30 s on the clock needs a sum under about 450 s at 16 threads, with the long tests started first.
- **The test threads mostly wait**: the run uses 5.5-5.8 of the 16 CPUs (§3).
- **About 0.5 s of an integration test is fixed cost** (§4), against an average of 0.80 s for the 908 integration tests under 2 s: the teardown (0.29-0.38 s; 0.19 s of it is killing the env's own reaper, then a 0.1 s grace and a second `/proc` scan) and creating and migrating a fresh store (0.17-0.27 s). A run has 1,126 `TestEnv`s (§5).
- **103 tests are over 2 s** (median of the 3 runs), 607 s of the 1,408 s. By cause (§6): real product limits the tests cannot shorten (the 60 s adapter watchdog, the 10 s pi stop, the 10 s doctor deadline, the 2^k s spawn back-off, the 5 s kill grace) 194 s; 1 s engine ticks and the "several ticks" sleeps around them 144 s; the wrapper's and driver's 0.5-2 s polls 123 s; the cockpit's 2 s refresh 21 s; fixture cost (one test builds 72 envs) 98 s; real work (store::schema building every version's database) 27 s.
- **The drafted Jobs do not reach 300 s** (§7, computed): they leave about **525 s** (clock ~35 s). Adding dependencies at `opt-level = 3` (measured per test, new Job sp-build) gives about 465 s; long tests at 0.5 s instead of 1 s about 420 s. **300 s needs more than these**: the average short integration test must fall from 0.80 s to about 0.18 s, and the cost left after the fixes is each `cpd` command's and each `Store::open`'s schema check (17-30 ms each, several per test) and the tests' 100 ms poll sleeps. The largest measured single gain is C1 (the teardown leaves the reaper alone): **0.17-0.31 s per test**, measured on 5 tests (§7).
- **The recommended split** (§8): sp-build (new: dependencies optimised and nextest priorities, config only, first), sp-fixed, sp-limits (widened to the wrapper's and driver's polls), sp-engine (widened to the cockpit's refresh; PR timing is on main), sp-schema, sp-poll (new: the shared wait and the per-test cost left, the Job that must find the last ~120 s), sp-guard (narrowed to the per-test limit and the check).

## 1. The tool: `scripts/test-time.sh`

`scripts/test-time.sh [nextest args]` builds the tests (`nextest run --no-run`, not timed), then runs `scripts/cargo.sh nextest run --workspace --profile timing [args]` and prints, from the JUnit report of the `timing` profile (`.config/nextest.toml`: the default profile plus `junit.path`):

- the load before and after (`uptime`);
- **wall**: nextest's run time (its `Summary`, build excluded) and the clock time of the whole command;
- **sum**: every test's time added up, and the **count** (failed, errors);
- the 30 slowest tests;
- per test binary: summed time, count, slowest test.

It exits with nextest's status after printing. Extra arguments go to nextest, e.g. `-E 'binary_id(cpd::plan)'` for a subset or `--config-file` for an experiment. nextest 0.9.148 writes the report under `<workspace>/target/nextest/timing/` even with `CARGO_TARGET_DIR` set (observed), which is where the script reads it.

## 2. Before: the full run, seven times

`scripts/test-time.sh` on `8df1af8`, back to back, 2026-10-19. "Beside it" comes from `ps` every 5 s during runs 5-6 (a monitor script), and from `ps` before and after for runs 1-4.

| run | start | load before (1/5/15 min) | wall (s) | sum (s) | tests | CPU (user+sys / wall) | beside it |
|---|---|---|---|---|---|---|---|
| 1 | 09:27 | 7.05 / 9.22 / 6.84 | 124.8 | 1,525.2 | 1,728 | n/a | another Worker's serial `cargo test --workspace` |
| 2 | 09:30 | 6.68 / 9.54 / 7.41 | 132.9 | 1,673.1 | 1,728 | 5.75 | the same `cargo test --workspace` |
| 3 | 09:38 | 10.19 / 10.79 / 9.13 | 148.6 | 2,017.5 | 1,728 | 5.54 | another Worktree's Gate (`nextest run --workspace`), whole run |
| **4** | 09:41 | 10.23 / 14.12 / 11.01 | **120.5** | **1,420.3** | 1,728 | 5.76 | nothing seen before or after |
| **5** | 09:43 | 5.28 / 11.53 / 10.52 | **120.6** | **1,408.1** | 1,728 | 5.74 | nothing (monitor: only this run; load 5.3-10.2) |
| **6** | 09:45 | 4.98 / 10.07 / 10.16 | **123.6** | **1,487.3** | 1,728 | 5.82 | nothing until 09:47:35, then another Gate's nextest and a release build for the last 30 s (the tail) |
| 7 | 10:42 | 26.45 / 30.30 / 31.19 | 174.5 | 2,496.4 | 1,728 | n/a | another Worktree's `nextest run --workspace` the whole run, load 26-40 (the check run of the committed script) |

**The before numbers are runs 4-6: wall 120.5 / 120.6 / 123.6 s, sum 1,420 / 1,408 / 1,487 s, 1,728 tests.** All 1,728 pass in every run. A second test run beside it adds 10-30 % to the sum and the clock (runs 1-3), and about 75 % at load 26-40 (run 7); the Gate log numbers in the brief (124 s / 1,471 s; 160 s / 2,295 s) are in the same range. The sum depends on the load, so a before/after comparison must be paired (same conditions, interleaved: §7 does).

Distribution (run 5): 621 tests under 0.1 s (5.2 s together; nearly all unit tests), 82 at 0.1-0.3 s (18.1 s), 148 at 0.3-0.5 s (61.6 s), 513 at 0.5-1 s (374.5 s), 261 at 1-2 s (341.3 s), 73 at 2-5 s (225.8 s), 21 at 5-10 s (141.8 s), 9 over 10 s (239.7 s).

By kind (run 5): 1,004 integration tests (`cli/tests/`, the `cpd::*` binaries) take 1,297.8 s; 908 of them under 2 s take 723.2 s, **0.80 s on average**. The 724 unit tests take 110.3 s, of which `cpd-core` store tests 80.9 s (85 tests) and the rest of `cpd-core` 24.8 s (305 tests); `cpd-tui` 3.1 s (310 tests).

Per binary (run 5, the largest): `session_watchdog` 142.6 s (3 tests), `cpd-core` 105.7 s (390), `watch` 104.0 s (41), `pr` 80.9 s (75), `plan` 63.8 s (49), `session` 63.1 s (53), `driver` 55.0 s (25), `session_reload` 54.4 s (10), `merge` 49.9 s (40), `config` 44.8 s (30), `doctor` 44.6 s (32), `job` 40.1 s (32), `git` 39.2 s (48), `tui_tree` 36.9 s (45), `context_distill` 32.9 s (13). The full table is what `scripts/test-time.sh` prints.

### 2.1 Second Attempt: the base and today's main side by side, under heavy load

Attempt sp-profile-2-a1, 2026-10-19 16:15-16:40 UTC (12:15-12:40 on hs's clock), did not wait for a calm machine: beside it ran three other Worktrees' nextest runs (a Gate's `npm run verify`, a `nextest run --workspace --no-fail-fast`, a `nextest run -p cpd -E 'binary(pr) | … | binary(watch)'`) and `node --test` runs (`ps` before). Two `scripts/test-time.sh` full runs, started 1 minute apart and run at the same time: the base `8df1af8` (this branch) and today's main `74d5f99` (a scratch worktree with this script and the `timing` profile copied in; main has #104 and 1,830 tests).

| build | load before → after (1 min) | wall (s) | sum (s) | tests | result |
|---|---|---|---|---|---|
| base `8df1af8` | 74.9 → 113.3 | 575.6 | 7,835.8 | 1,728 | all pass |
| main `74d5f99` | 99.0 → 77.0 | 560.5 | 8,028.1 | 1,830 | all pass |

At load 75-113 the suite takes 4.7× its calm clock and 5.6× its calm sum (base: 575.6 s / 120.6 s; 7,836 s / 1,408 s, run 5), and the multiplier is not uniform (the multi-env tests of §6.5 grow most: `config_refuses_each_bad_file_with_its_key_path` 152 s against 22 s calm; the `session_watchdog` tests, which wait out a fixed 60 s, least: 70-71 s against 67-69 s). So these two runs compare builds with each other, not with the 30 s / 300 s target. What they show (on the same conditions, run at the same time):

- **main has 102 more tests** (113 new, 11 renamed or gone): 552 s of main's sum under this load, about 100 s calm (computed with the 5.6× above: unverified). The target applies to main, so its sum before the speed Jobs is about **1,500 s** calm (computed: run 5's 1,408 s + ~100 s), not 1,408 s.
- On the 1,717 tests both have, main's sum is 7,476 s against 7,801 s (−4 %, inside the noise of one pair). #104's changes show only where they are large: `session_reload`'s SIGTERM test (`pi_stop_seconds = 3`) 15.3 → 9.5 s, `e2e_cross` 69.4 → 49.9 s; the `watch` binary is 430 → 495 s and `session` 351 → 399 s (noise; #104's own commit message measured `session` unchanged).
- The `session_watchdog` tests (60 s limit) are unchanged on main: sp-limits still owns them.

The per-test numbers of §4-§7 are the base's and still hold on main for every test #104 did not touch (unverified per test: not re-measured).

## 3. Where the clock goes

From the JUnit timestamps (each test's start and duration) of run 5, the tests running at once, per 5 s: **16.0 from 0 to 90 s, then 4.8, 2.7 and 2.0 until the end.** The last 30 s are the two `session_watchdog` tests: nextest starts tests in binary order, these start at 52.0 s and take 66.9 and 68.5 s. For every run, wall − sum / 16 = 22.5-32.6 s: that is the tail.

**Measured with priorities** (run "prio", 10:06, a temporary `--config-file` = the repo's plus `priority = 100` for the binaries with the slowest tests; another Worker's Gate ran beside it the whole time, load 8-33): wall 140.8 s, sum 2,222.9 s, **wall − sum / 16 = 1.8 s**; the two watchdog tests started first and the last 5 s still had 12 tests running. So with priorities **the clock is the sum / 16 + about 2 s**, provided no single test is longer than that. At the target, 300 s / 16 + 2 s ≈ 21 s.

The threads are not busy computing: user + sys CPU over the run is 5.5-5.8 CPUs of 16 (`/usr/bin/time` around the script, runs 2-6; it counts only processes the run waits for, so detached tmux servers and reapers are missing: a lower bound). More test threads than CPUs would shorten the clock further at the same sum (unverified: not measured; the sum would rise somewhat with contention).

## 4. The cost of each phase, per test

Method: one test run alone with `strace --seccomp-bpf -f -ttt -e trace=process -e signal=none` (only process syscalls stop the tracee, so the overhead is small: the floor test takes 0.49-0.55 s without strace, 5 runs, and 0.56-0.58 s with it, 3 runs); a script turns the trace into a timeline of every process (start at its `execve`, end at its exit). The test's own work in between (in-process `Store::open`, fixture SQL) is the gap between processes. Load 4-17 during these (another Worker's serial `cargo test`).

### (a) A minimal TestEnv test: `cpd::plan new_refuses_an_empty_brief_file_and_writes_nothing`

0.68-0.69 s in runs 4-6; alone 0.49-0.55 s (5 runs), traced alone 564 / 566 / 575 ms:

| phase | ms | what |
|---|---|---|
| process start, `TestEnv::new` | 4-5 | temp dir, 5 deny shims, `.gitconfig`, `teardown.sh` |
| reaper start | 1 | `sh -c 'setsid sh teardown.sh watch <pid> &'`; the reaper then loops on `sleep 1` (in the background, not on the test's path) |
| `cpd context new` (first command: **creates and migrates the store**) | **173-182** | the store's creation: 15 migrations, the history triggers, the schema check. Timed alone (`date` around it, fresh `CPD_HOME`, 5 runs): first command 207-266 ms, the second 29-35 ms, `events list` 26-40 ms, `--help` 6-8 ms |
| git fixture (`git_repo`) | 13-15 | `init` 2.7, `add` 2.0, `commit` 6.3-7.8 of which `git maintenance run --auto` 1.4-1.8 (60 ms in the brief came from a full strace, which stops every syscall) |
| test: write the brief, one query (a new `Store::open`) | ~20 | in-process |
| `cpd plan new` (refused, `usage`) | 4-5 | refused before it opens the store |
| test: two more queries, each a new `Store::open` | ~38 | in-process |
| **teardown** | **289-306** | `tmux kill-server` 1.7; **scan 1**: `grep -laF <dir> /proc/[0-9]*/cmdline` 6-8 plus `find /proc -mindepth 2 -maxdepth 2 -name cwd -lname '<dir>*'` 60-65, ~100 with the glob and the pipeline; it finds **the env's own reaper** (`sh teardown.sh watch`: its command line contains the directory), sends it SIGTERM, then **`sleep 0.1`** (101 ms) before re-checking; **scan 2**: ~90; `rm -rf` 2-3 |

So the floor is: teardown ~300 ms, store creation ~180 ms, everything else ~85 ms. The reaper kill alone costs ~190 ms (the 0.1 s grace plus the second scan); a run with the reaper not targeted (experiment in a scratch copy, see §7) does one scan and no grace.

### (b) A typical engine test: `cpd::watch e2_kills_the_window_of_a_dead_driver_it_ends_though_the_first_kill_window_fails`

1.96 s median in runs 4-6 (1.90-2.50; the median `watch` test: 1.90-2.20 s); traced alone 2,118 / 2,077 ms:

| phase | ms | what |
|---|---|---|
| TestEnv, git repo, `git config` ×3, Lead worktree (`git worktree add`, 7 git processes) | 0-95 | git: 15 processes |
| **store creation and fixtures, in the test process** | **~320** | `Fx::store()` opens a new `Store` for every `fixture`/`query` (the first creates and migrates it) |
| tmux fixture (`new-session`, `new-window`) | ~25 | |
| **`cpd watch` until the test stops it** | **1,209-1,289** | the first tick runs E2 at once; the claim needs the second E2 pass, **one `tick_seconds = 1` later**; the test polls every 100 ms (each poll a fresh `Store::open`); 0.82 s of the test thread is in `thread::sleep` |
| teardown | 358-383 | as (a), two scans and the 0.1 s grace |

### (c) A typical cockpit test: `cpd::tui_tree enter_opens_the_lead_of_a_plan_and_a_fork_with_session_open`

0.69 s median in runs 4-6 (0.67-0.74; the median `tui_tree` test: 0.68-0.73 s); traced alone 752 ms:

| phase | ms | what |
|---|---|---|
| TestEnv, **store creation and fixture SQL in-process** | ~230 | |
| `tmux new-session` running `cpd tui view tree` | 16 | |
| view start until the test sees its first screen | ~40 | |
| keys and screen reads (`send-keys` ×5, `capture-pane`), the fake `cpd session open` ×2 | ~120 | polls of `capture-pane` |
| teardown | 327 | as (a) |

### (d) A test that starts node: `cpd::config tiers_reads_efforts_from_pi_with_pis_node`

No Rust test runs the pi adapter (`pi/lib/`, tested by `npm test`, outside this target); the Rust tests that start a real `node` are the Tier catalog probe's (`cpd tiers`, `cpd doctor`). 0.60 s median in runs 4-6 (0.52-0.61); traced alone 626 ms:

| phase | ms | what |
|---|---|---|
| TestEnv | 6 | |
| `cpd tiers` | 246 | of which **`node` 30 ms** (the catalog module, fake pi packages); the rest is the store's creation (the catalog is cached in the store) |
| teardown | 372 | as (a) |

### Other per-unit costs

- **Every `cpd` command that opens the store costs 26-40 ms** (`events list` on an existing store, 5 runs), against 6-8 ms for `cpd --help`. The difference is `Store::open`: the connection set-up and the schema check of §4.3 (`generated_mismatch`, `canonical_hash`) at every open, in a debug build (`opt-level 0`, also for the bundled SQLite C code and `sha2`). With dependencies at `opt-level = 3` (a separate target dir built with `--config 'profile.dev.package."*".opt-level=3'`, 5 runs each): **first command 207-266 → 129-143 ms, later commands 29-35 → 18-22 ms**.
- **A copied store**: `cpd context new` in a home whose `cpd.db` is a copy of a freshly created one takes **32-35 ms** (5 runs), against 207-266 ms creating it.
- **Store opens per test** (strace `-e trace=openat` of whole binaries, opens of `cpd-home/cpd.db`): `pr` 75 tests: 291 by the test processes, 157 by `cpd`; `plan` 49: 134 and 561; `watch` 41: 489 and 95 (plus the engine's own); `tui_tree` 45: 57 and 90. So about 3-14 opens per test, each 17-30 ms (the attribution of threads to processes is approximate).
- **`/proc` scan alternatives**: reading every `/proc/<pid>/cmdline` and `readlink /proc/<pid>/cwd` in one process (a Python stand-in for an in-process Rust scan, ~940 processes): 30-44 ms (5 runs), against ~100 ms for the shell pipeline.

## 5. Suite-wide counts

One full run under `strace --seccomp-bpf -f -ttt -T -e trace=process,clock_nanosleep,nanosleep` (09:48, another Gate's nextest beside it; 170.9 s, 1,728 tests; 1,740 test processes, the 12 more being helper runs the harness's own tests start). Under strace and that load the times are inflated, so this section gives **counts** and **shares**; the per-unit times are §4's.

| what | count per run | share of the traced sum (2,558 s) |
|---|---|---|
| `TestEnv`s (teardowns) | **1,126** (956 integration tests have one or more; 912 exactly one; 8 have 5 or more, up to 72) | teardown.sh 821 s, 32 % |
| reapers | 1,128 | (background) |
| `thread::sleep` in the test processes (fixed sleeps and poll loops) | | 503 s, 20 % |
| `cpd` spawned by a test directly | 3,066 (all `cpd` processes: 3,458) | 449 s, 18 %, of which `cpd watch` (engines, 52) 97 s, `context new` (156, creates the store) 55 s, `doctor` 38 s, `plan new` 34 s, `pr add` 32 s, `events list` (391) 19 s |
| git | 16,010 | 153 s; `git maintenance run --auto`: 1,216 runs, 3.6 s in all (negligible) |
| tmux | 6,042 | |
| node | 132 | 5.0 s |
| processes in all | 49,089 | |

## 6. Every test over 2 s, by cause

The 103 tests whose median over runs 4-6 is over 2 s: 607 s of run 5's 1,408 s (§6.1 194 s, §6.2 144 s, §6.3 123 s, §6.4 21 s, §6.5 98 s, §6.6 27 s). "sleep" is the time the test process spent in `thread::sleep` and "envs" the number of `TestEnv`s (both from the traced run of §5). The change named is the one that removes the cause; the Job is §8's.

### 6.1 A real limit the test cannot shorten (a product constant, or a hold the test chose)

| median (s) | test | the limit | change |
|---|---|---|---|
| 68.5, 66.9 | `session_watchdog` `a_pi_whose_adapter_reports_idle_within_the_limit_is_left_alone`, `a_pi_alive_with_its_state_stopped_for_sixty_seconds_is_restarted_with_one_error_event` | `ADAPTER_WATCHDOG` = 60 s and `WATCHDOG_POLL` = 2 s (`core/src/session/run.rs:48,51`); the tests sleep the limit out and check every 100 ms | inject the limit and the poll (a test-only override, like `CPD_TEST_DEADLINE_<TOOL>` of `core/src/host/deadline.rs`, or config keys); then ~1-2 s each |
| 7.2 | `session_watchdog a_pi_that_exits_is_handled_as_before_with_no_error_event` | a fixed 5 s sleep to show the wrapper waits for Enter (absence), and the 2 s poll | the same injected poll; a shorter absence window |
| 12.0 | `session_reload a_pi_that_ignores_sigterm_is_killed_after_ten_seconds_and_the_wrapper_starts_pi_again_for_a_new_pi` | `PI_STOP` = 10 s (`core/src/session/run.rs:46`) | inject `PI_STOP`: done on main by #104 (`timeouts.pi_stop_seconds`, 3 in the test) |
| 10.6 | `doctor a_hung_catalog_probe_is_killed_at_10_seconds` | doctor's `DEADLINE` = 10 s (`core/src/doctor/mod.rs:23`; the test asserts `deadline_seconds = 10`); the other doctor deadlines already take `CPD_TEST_DEADLINE_<TOOL>` | let the catalog probe take the same override |
| 10.1 | `watch e3_spawn_failures_requeue_at_the_same_tier_never_fail_the_job_and_back_off` | the spawn back-off 2^k s (`backoff_seconds`, cap `BACKOFF_CAP_SECONDS` = 300, `core/src/watch/schedule.rs`), waited out over 1 s ticks | inject the back-off unit |
| 6.0 | `cpd-core host::deadline::tests::a_group_that_ignores_sigterm_is_killed_after_the_grace` | `GRACE` = 5 s between SIGTERM and SIGKILL (`core/src/host/deadline.rs:26`) | a grace parameter in the unit under test |
| 3.5, 2.1 | `store a_writer_that_cannot_get_the_lock_in_time_is_store_busy`, `a_second_writer_waits_for_the_first_and_reads_do_not` | the test's own writer holds the lock 3,000 / 1,500 ms | hold until the second writer has given up / entered (a positive signal) |
| 2.6 | `doctor a_hung_pi_node_is_a_failed_check` | `CPD_TEST_DEADLINE_NODE=1` (1 s, the override's floor: whole seconds) plus the kill | sub-second deadlines in the override |
| 2.3 | `git a_hook_is_killed_with_its_process_group_at_the_timeout` | the hooks' `GRACE` = 2 s (`core/src/git/hooks/exec.rs:75`) and a fixed 1.5 s sleep | inject the grace; wait for the survivor's absence positively |
| 2.8 | `watch e5_kills_a_hung_poll_child_and_its_group_at_the_deadline_with_tool_timeout` | the poll child's deadline, already shortened to 2 s by `CPD_TEST_DEADLINE_CPD=2` (whole seconds), and 1 s ticks | sub-second deadlines in the override; sub-second tick (§6.2) |

### 6.2 Engine ticks (`watch.tick_seconds = 1`, an integer ≥ 1 on the base; a decimal ≥ 0.1 on main since #104) and the "several ticks" sleeps around them

Each test waits for one to three 1 s ticks, and most also sleep a fixed 1.5-3.5 s ("two ticks", "several ticks": absence checks); `pr_poll_seconds` is also whole seconds. Change: fractional ticks and poll periods (PR timing, #104: on main since `3944b57`, after this base; it put the slowest of these tests on a 0.2 s tick, §2.1), a 0.1 s tick in the tests, absence windows of a few ticks.

| median (s) | sleep (s) | tests |
|---|---|---|
| 24.1 | 20.6 | `e2e_cross a_plan_across_two_repos_runs_a1_then_b_then_a2_and_a_review_job` (117 `cpd`, 441 git; fixed sleeps 2.5 s and 3 s; 1 s ticks and polls through a whole Plan) |
| 15.1, 5.9 | 14.3, 4.7 | `context_distill the_engine_starts_a_run_only_after_the_quiet_period_and_never_two_at_once` (the quiet period is backdated, not waited: the time is a fake pi that sleeps 3 s, then fixed sleeps of 4 s and 3 s over ticks), `the_engine_tries_a_failed_seq_once_until_a_newer_record_arrives` |
| 7.8 | 7.4 | `watch e5_polls_every_pr_poll_seconds_after_the_previous_poll_ended_whatever_the_plans` (several `pr_poll_seconds` periods) |
| 7.5, 5.8 | 4.9, 4.0 | `pr_start e7_starts_a_dependent_pr_from_a_fresh_fetch_once_its_prerequisite_is_merged`, `a_failed_start_marks_the_pr_and_appends_one_error_until_pr_start_retries` |
| 5.4 | 3.2 | `e2e a_whole_plan_runs_from_context_new_to_pr_poll` |
| 5.0, 4.9, 4.8, 4.8, 4.6, 4.4, 3.6, 3.6, 3.5, 3.5, 3.4, 3.4, 3.1, 3.0, 2.7, 2.6 | 1.3-4.2 | `watch`: `e5_reports_a_failing_poll_once`, `e5_pauses_below_100_requests_until_reset_and_reports_one_gh_rate_limited`, `e5_polls_with_a_plan_in_pr_too_and_does_not_poll_more_often_than_every_pr_poll_seconds`, `e3_never_starts_a_draft_and_e4_waits_while_one_remains`, `e3_does_not_start_a_job_in_its_backoff`, `e4_sends_all_merged_again_after_a_job_is_appended_and_merged`, `e2_removes_the_lock_of_a_dead_driver_it_ends_and_does_not_recreate_it`, `e2_skips_a_pane_with_an_unknown_view_and_reports_no_error`, `e3_reports_a_tampered_job_once_and_does_not_start_it`, `e4_appends_all_merged_once_when_no_job_is_left_to_run`, `e4_waits_while_a_job_is_queued`, `the_engine_neither_imports_nor_syncs_a_context_store`, `e2_asks_live_drivers_to_stop_on_timeout_or_cancel_with_escalations_pausing_the_clock`, `r2_removes_the_worktree_though_every_kill_window_fails`, `e2_stops_sessions_whose_pane_is_dead_or_gone`, `e3_starts_the_driver_after_the_engines_binary_is_replaced` (the typical engine test of §4 b, 1.9 s, has the same cause) |
| 4.7, 4.4, 2.8 | 3.9, 3.8, 1.2 | `watch_inbox a_failed_job_is_stuck_only_after_stuck_job_minutes` (backdated minutes, then a fixed 3.5 s), `a_tick_with_no_change_writes_no_transaction` (fixed 3.5 s), `a_blocked_lead_in_dispatch_is_a_red_dialog_until_it_works_again` |

### 6.3 The wrapper's and the driver's polls

`session run` checks its sources every `SOURCE_POLL` = 2 s, files every `FILE_POLL` = 0.5 s, the watchdog every 2 s (`core/src/session/run.rs:37-48`); the driver polls every `POLL` = 0.5 s (`core/src/driver/run.rs:37`), its Lead every `LEAD_POLL` = 2 s and pauses `KILL_PAUSE` = 1 s (`core/src/driver/worker.rs:27,335`). The tests wait for these and add fixed "several polls" sleeps (1.5-5 s). Change: injectable poll periods (test-only override), absence windows of a few polls.

| median (s) | sleep (s) | tests |
|---|---|---|
| 10.2, 7.8, 6.7, 4.3, 4.0, 4.0, 2.8 | 2.2-9.5 | `session_reload`: `a_newer_store_does_not_end_a_wrapper_and_the_new_cpd_that_opens_it_adopts_pi` (fixed 4.5 + 2.5 s), `a_failing_store_read_is_said_once_and_retried_with_pi_untouched` (4.5 s), `a_lead_wrapper_does_not_reload_into_a_cpd_older_than_the_store_and_reloads_into_one_that_can_open_it` (2.5 + 2.5 s), `a_working_lead_survives_an_install_the_wrapper_re_execs_and_pi_is_never_signalled`, `a_pi_that_changed_before_the_install_is_restarted_at_idle_by_the_new_wrapper`, `an_install_re_execs_an_idle_wrapper_at_once_keeping_pi_and_a_new_pi_then_restarts_it_at_idle`, `a_parked_idle_lead_restarts_its_pi_on_its_session_file_after_pi_changes` |
| 9.4, 3.4, 2.8, 2.8 | 8.4, 2.7, 2.1, 2.1 | `session`: `fork_waits_until_the_source_is_idle_then_forks_and_resumes` (`fork_quiet_seconds = 1` already; the 2 s source poll and fixed 3 + 3 s), `a_reload_is_not_a_quit_even_though_pi_exits_0_on_sigterm`, `fork_stops_waiting_for_a_stopped_source_without_a_file_and_retries_on_enter`, `a_fork_with_a_message_file_over_128_kib_starts_pi_with_the_copied_file` |
| 7.7, 3.0 | 6.6, 2.0 | `review_job the_worker_starts_only_once_the_lead_is_idle` (fixed 5 s + 1 s; `LEAD_POLL` 2 s), `a_cancel_while_the_driver_waits_for_the_lead_ends_the_attempt_cancelled` |
| 6.6, 6.4, 6.0, 3.6, 3.4, 2.6, 2.5, 2.1 | 1.3-5.9 | `driver`: `stall_seconds_0_kills_nothing`, `a_tool_call_with_its_result_is_not_a_stall`, `a_running_worker_is_watched_without_listing_panes_at_every_poll` (1.5 + 3 s), `a_stalled_tool_call_has_its_processes_killed_and_the_worker_keeps_running` (`stall_seconds = 1` already), `one_failed_list_panes_does_not_end_a_running_attempt_crashed` (1.5 s), `a_merge_conflict_ends_failed_and_the_ladder_queues_the_next_attempt_with_the_files`, `a_failed_kill_window_is_retried_and_appends_no_error`, `an_idle_worker_is_nudged_once_then_ends_no_submit` |
| 7.0, 2.9, 2.8, 2.4, 2.3 | 1.1-6.0 | `driver_reload` (one at a time: test group `driver-reload`): `a_driver_does_not_reload_into_a_cpd_older_than_the_store_and_reloads_into_one_that_can_open_it` (4 s), `after_a_reload_fd_3_is_the_lock_and_the_drivers_children_do_not_hold_it`, `a_nudged_worker_that_idles_again_after_a_reload_ends_no_submit`, `a_driver_reloads_while_running_keeps_its_lock_and_the_attempt_merges`, `a_driver_reloads_at_the_merging_safe_point_and_completes_the_merge` |
| 3.4 | 3.0 | `git the_log_has_a_hooks_line_before_the_hook_ends` (the hook itself sleeps 3 s) |

### 6.4 The cockpit's refresh

The views refresh every `REFRESH` = 2 s and coalesce for 0.5 s (`tui/src/plan/mod.rs:53,57`, `tui/src/widgets/mod.rs:44,46`, `tui/src/statusbar/mod.rs:47,49`), the tree snapshots every 0.5 s (`tui/src/tree/mod.rs:52`). Change: injectable refresh periods for the views, waits on the screen not on time.

| median (s) | sleep (s) | tests |
|---|---|---|
| 4.7, 2.6 | 3.8, 1.9 | `tui_statusbar the_inbox_counts_the_visible_open_items_in_the_colour_of_the_most_urgent`, `the_workers_follow_the_slots` |
| 4.7, 2.2, 2.2 | 3.8, 1.6, 1.6 | `tui_plan a_conflict_shows_in_the_box_title_and_clears_with_no_event_within_the_refresh` (fixed 1.5 s), `the_view_starts_no_subprocess_and_asks_gh_nothing`, `without_a_port_file_the_pending_review_row_has_no_arrow` |
| 2.2, 2.1 | 1.4, 1.4 | `tui_tree the_tree_follows_the_events_of_the_store`, `the_tree_follows_the_archive_events` |

### 6.5 Fixture cost (several envs, many `cpd` and git processes)

No wait: the time is §4's fixed costs, repeated. Change: sp-fixed's cuts (each env 0.4-0.5 s cheaper), and one env per test where the cases do not need their own (the 72-case loop).

| median (s) | envs | cpd / git | tests |
|---|---|---|---|
| 22.2 | 72 | 72 / 0 | `config config_refuses_each_bad_file_with_its_key_path` (one `TestEnv` per bad file) |
| 7.0, 2.3 | 12, 4 | 12 / 12, 4 / 3 | `doctor each_tool_must_be_at_or_above_its_minimum`, `a_tool_that_is_not_on_path_fails` |
| 5.2, 3.6 | 9, 6 | 9, 6 | `config tiers_catalog_failures_are_catalog_failed`, `tiers_with_a_broken_pi_install_is_pi_not_found` |
| 5.2, 4.1 | 5, 4 | 25, 12 | `plan archive_refused_by_any_one_blocker_changes_nothing`, `context archive_refused_by_any_one_blocker_changes_nothing` |
| 2.0, 2.0, 2.1 | 1 | 12-26 / 29-40 | `plan plans_moving_to_phase_plan_at_once_all_move`, `a_review_after_the_lock_covers_the_drafts_and_its_approval_queues_exactly_them`; `plan_cancel cancels_jobs_closes_open_prs_and_the_lead_and_archives_the_plan` |
| 4.0 | 1 | 32 / 0 | `context_distill nothing_the_audit_lists_is_lost` |
| 3.1, 2.9, 2.7 | 3, 2, 2 | 19-20 | `job add_draft_after_the_lock_is_an_appended_draft_that_stays_editable_and_cancellable`, `add_draft_needs_only_a_title_and_stores_the_tier_and_the_spec_as_null`, `edit_completes_a_stub_draft_with_its_tier_and_its_spec` |
| 3.5, 2.2 | 7, 6 | 0 / 245, 132 | `git each_protected_path_is_refused`, `a_bad_list_file_is_config_invalid` |
| 3.4, 2.3, 2.3 | 3, 2, 2 | 0 / 246, 185, 152 | `merge an_attempt_that_merges_a_base_with_changed_protected_files_is_merged`, `a_protected_file_deleted_on_the_base_and_in_the_attempt_is_merged`, `an_attempt_that_changes_a_protected_file_to_anything_but_the_bases_is_tampered` |
| 2.5 | 3 | 4 / 95 | `attempt submit_refuses_an_attempt_that_is_not_running` |
| 2.8, 2.7 | 3, 2 | 4 / 144, 2 / 198 | `pr pr_add_no_needs_and_needs_override_the_reopen_default`, `open_and_rebase_run_r_for_the_repos_orphaned_merges_first` |
| 2.4, 2.2, 2.2 | 1, 3, 1 | 20, 3, 16 | `pr_crud edit_refusals_write_nothing`, `add_to_an_archived_plan_is_archived`, `add_refusals_write_nothing` |

### 6.6 Real work (CPU)

| median (s) | tests | what |
|---|---|---|
| 8.3, 8.0, 3.0 | `cpd-core store::schema::tests::a_legacy_store_of_every_version_converts_keeping_every_row_and_id`, `migrating_an_empty_database_gives_every_versions_schema_hash`, `a_tampered_legacy_store_is_refused_and_left_as_it_was` | build a database of every legacy version from scratch, migrate it, hash it (no process, no sleep: 0 s in `thread::sleep`); 13-14.5 s in the loaded Gate logs of the brief, 10.5-13.1 s in runs 1-2, 21-26 s under strace and load: CPU-bound, so they grow with the load |
| 2.9, 2.9, 2.0 | `cpd-core store::verdict::tests::inside_the_window_the_objects_of_unknown_migrations_are_not_checked`, `inside_the_window_the_objects_this_binary_created_are_checked`, `store::tests::two_plans_kind_migrations_applied_in_either_order_keep_both_kinds` | reference databases built per case |

Change: build each version's database once per test binary (a `OnceLock`), in memory or with `synchronous = OFF` for fixtures; dependencies at `opt-level = 3` (§7) speed the SQLite and hashing work for every one of these (measured on three of them, §7.1: −40 % on the store::verdict test, −30 % median on the legacy conversion, no clear gain on `migrating_an_empty_database_gives_every_versions_schema_hash`).

## 7. Changes, their savings, and the totals after them

### 7.1 Measured: C1 (the teardown leaves the reaper alone) and C4 (dependencies optimised)

Two scratch builds, outside this repo's diff: **proto** = this commit with `cli/tests/common/mod.rs`'s teardown changed so the reaper writes its pid to `<dir>/reaper.pid` and the teardown adds it to the protected pids (4 lines; the reaper still ends by itself within 1 s once the directory is gone); **opt** = this commit built with `--config 'profile.dev.package."*".opt-level=3'` (dependencies optimised, the workspace's crates not). Five tests, each run alone with the test binary and `--exact`, the three builds interleaved, 7 times each (10:38-10:40; the machine was loaded, 1-min load 30-53, two other Workers' test runs beside it). Median ms (min-max):

| test | base | C1 (proto) | C4 (opt) |
|---|---|---|---|
| `plan new_refuses_an_empty_brief_file_and_writes_nothing` (a) | 783 (602-1,066) | **559** (467-663) | **574** (504-725) |
| `pr poll_with_a_done_plan_reruns_its_clean_up` (the median `pr` test) | 1,063 (811-1,306) | **831** (675-1,413) | **938** (696-1,475) |
| `tui_tree enter_opens_the_lead_of_a_plan_and_a_fork_with_session_open` (c) | 857 (670-985) | **587** (476-776) | **678** (589-810) |
| `config tiers_reads_efforts_from_pi_with_pis_node` (d) | 631 (518-753) | **460** (326-499) | **587** (431-654) |
| `watch e2_kills_the_window_of_a_dead_driver_it_ends_though_the_first_kill_window_fails` (b) | 2,270 (1,844-3,211) | **1,962** (1,771-3,620) | **2,001** (1,831-2,212) |

**C1 saves 171-308 ms per test (median 232 ms); C4 saves 44-269 ms (median 179 ms).** The traced proto floor test does one scan and no grace (teardown 174 ms under load 20). Under this load both savings are larger than on a calm machine (C1 calm: 0.19 s, §4; C4 calm per command: 29-35 → 18-22 ms). Whole-suite A/B runs on a subset (240 tests) were tried and abandoned: back to back, the same build's sum moved from 411 to 785 s with the load, so a suite-level difference of a few percent cannot be seen on hs at these hours; per-test interleaving can.

**C4 on the store unit tests** (second Attempt, 16:23-16:36 UTC, load 37-88 with two full test runs and other Workers' Gates beside it): the base `cpd-core` test binary against the same commit built with dependencies at `opt-level = 3` (`CARGO_TARGET_DIR` apart, `cargo test -p cpd-core --lib --no-run --config 'profile.dev.package."*".opt-level=3'`: 3 min 36 s at `-j 4` with sccache under that load), each test alone with `--exact`, the two builds interleaved, 5 rounds. Median s (all 5):

| test | base | C4 (opt) | opt faster in |
|---|---|---|---|
| `store::verdict::tests::inside_the_window_the_objects_this_binary_created_are_checked` | 4.19 (3.95, 4.19, 4.50, 4.69, 4.08) | **2.52** (2.39, 2.65, 2.39, 2.64, 2.52) | 5 of 5 |
| `store::schema::tests::a_legacy_store_of_every_version_converts_keeping_every_row_and_id` | 22.1 (59.0, 36.8, 13.0, 22.1, 20.8) | **15.5** (31.5, 15.5, 11.4, 36.8, 12.5) | 4 of 5 |
| `store::schema::tests::migrating_an_empty_database_gives_every_versions_schema_hash` | 20.6 (19.0, 15.7, 20.6, 69.0, 28.8) | 29.0 (45.5, 18.6, 29.0, 31.4, 28.2) | 2 of 5 |

So C4 takes ~40 % off a SQLite-bound unit test (the verdict test, the steadiest pair); on the two schema tests the load's noise is as large as the effect, and the last shows none (guess: its time is in the workspace's own code, which C4 leaves at `opt-level = 0`, or in file I/O; unverified). sp-schema measures its own before with C4 merged.

A test in the suite takes ~1.3× its time alone (the floor test: 0.50-0.55 s alone, 0.68-0.69 s in runs 4-6), so savings measured alone understate the suite's.

### 7.2 Every change

"Per env" savings multiply by the 1,126 `TestEnv`s of a run (§5).

| # | change | Job | saving on the sum | how known |
|---|---|---|---|---|
| C1 | **The teardown does not target the env's own reaper** (it records its pid; the reaper ends by itself once the directory is gone): no 0.1 s grace, no second scan | sp-fixed | **0.19 s per env ≈ 215 s** (calm) | measured: per env §4, per test §7.1 |
| C2 | Replace the two-process `/proc` scan (`grep` + `find`) by one in-process scan, or by `PR_SET_CHILD_SUBREAPER` on the test process and a walk of its own descendants | sp-fixed | ~0.06-0.09 s per env ≈ 70-100 s | computed from §4 (scan ~100 ms; a one-process scan 30-44 ms); the subreaper walk is unverified |
| C3 | **A migrated template store** made once per test binary and copied into each env's `cpd-home` before the first command (tests that need no store, e.g. `store version_on_an_empty_home_reports_no_store_and_creates_nothing`, opt out) | sp-fixed | ~0.17-0.19 s per env that creates a store; ≤ 1,126 envs ≈ 150-200 s | measured per env (§4: 207-266 → 32-35 ms); the number of envs that create a store is unverified (≤ 1,126) |
| C4 | **Dependencies at `opt-level = 3` in the dev profile** (`[profile.dev.package."*"] opt-level = 3`: SQLite's C, `sha2`, `serde_json`, …; the workspace's own crates stay at 0, so their rebuilds stay fast) | new: sp-build | per test 44-269 ms (§7.1); per command and per `Store::open` ~11 ms; store creation ~90 ms; the store unit tests (§6.6 and the 79 others, 81 s) are SQLite-bound: −40 % on a store::verdict test, −30 % median on a legacy conversion, none seen on the schema-hash test (§7.1, measured under load) | measured per test and per command |
| C5 | Keep one `Store` open per test instead of a new one per query and poll | sp-fixed | ~20-25 ms per avoided open; test processes open 1.3-11.9 times per test (§4) ≈ 60 s | computed |
| C6 | `config_refuses_each_bad_file_with_its_key_path` with one env (rewrite `config.toml` per case), and one env per test in the other tests of §6.5 where the cases allow it | sp-fixed | 22 → ~1 s; §6.5 98 s → ~25 s with C1-C3 | computed |
| C7 | Real limits injectable (§6.1): the adapter watchdog and its poll, `PI_STOP`, doctor's catalog deadline, the spawn back-off unit, the kill graces; sub-second `CPD_TEST_DEADLINE_<TOOL>` | sp-limits | §6.1 194 s → ≤ 1.5 s each ≈ 175 s | computed |
| C8 | 0.1 s engine ticks and poll periods (§6.2, on #104's fractional ticks), absence windows of a few ticks | sp-engine | §6.2 144 s → ≤ 1 s each (e2e ≤ 5 s) ≈ 115 s | computed |
| C9 | Injectable wrapper and driver polls (§6.3) and view refresh periods (§6.4) | sp-limits (§6.3) / sp-engine (§6.4), §8 | §6.3 + §6.4 144 s → ≤ 1 s each ≈ 110 s | computed |
| C10 | store::schema and store::verdict fixtures built once per binary (§6.6) | sp-schema | 27 s → ≤ 1 s each ≈ 21 s | computed |
| C11 | **nextest `priority` for the tests that stay over a few seconds** | sp-guard | 0 on the sum; **~30 s on the clock** now, ~2 s after the others | measured (§3) |

### 7.3 The totals after them (computed)

A model over run 5's 1,728 test times: each integration test loses its envs × the per-env savings (C1 0.19 + C2 0.07 + C3 0.17 s) and C5's 0.066 s, never below 0.1 s per env; the tests of §6.1 end at 1.5 s, those of §6.2-6.4 and §6.6 at 1 s; C4 takes a further ~0.07 s off each integration test (~6 store opens or commands × 11 ms) and a third off the store unit tests (from §7.1's −0-40 % on three of them: a computed guess).

| after | integration tests under 2 s (908) | tests over 2 s | unit tests | **sum** | clock with priorities (sum / 16 + 2 s) |
|---|---|---|---|---|---|
| now (run 5) | 724 | 607 | 78 | **1,408** | 90 s (now 121 s without priorities) |
| the drafted Jobs: C1-C3, C5-C10 | 317 | 130 | 78 | **~525** | ~35 s |
| + C4 (sp-build) | 278 | 128 | 58 | **~465** | ~31 s |
| + the long tests at 0.5 s, not 1 s | 278 | 83 | 58 | **~420** | ~28 s |

So **30 s on the clock is within reach with C4 and priorities; 300 s added up needs more than these changes.** After them the 908 integration tests under 2 s still average ~0.31 s; 300 s needs ~0.17 s. What is left in them, by size (unverified split): each `cpd` command and each `Store::open` (the schema check of contract §4.3 at every open, 17-22 ms optimised, several per test: making it cheaper, e.g. keyed on SQLite's `schema_version`, is a contract question); the tests' `wait_until` helpers polling every 100 ms (10+ copies, docs/research/quality-tests-build.md G2; half a period per wait on average; 503 s of the traced sum of §5 is in `thread::sleep`, most of it the long tests'); git and tmux fixtures (~15-100 ms per test). More test threads than CPUs shorten the clock but not the sum (§3; unverified).

## 8. Recommended Job split (final)

The Lead's drafts in PR `speed` (sp-fixed, sp-limits, sp-engine, sp-schema, sp-guard) cover the causes. Changes: two new Jobs (sp-build, sp-poll), the priorities moved from sp-guard into sp-build (a measured 30 s on the clock today, config only), wider touches for sp-limits and sp-engine, and the cockpit refresh and the wrapper's polls given an owner. main has moved since this research's base: #104 (PR timing, `3944b57`) is on main, with fractional ticks (`watch.tick_seconds` ≥ 0.1), `timeouts.pi_stop_seconds`, and a 0.2 s tick and `pi_stop_seconds = 3` in the slow engine and reload tests (§2.1 measures main). So sp-engine no longer waits for PR timing, and part of §6.1 (`PI_STOP`) and §6.2 is done.

Each Job: one commit per change, `scripts/test-time.sh` before and after in the commit message, paired (same conditions, interleaved), with the subset it touches (`-E 'binary_id(...)'`) and single tests run alone 5-7 times (§7.1), since whole runs on a loaded hs move 10-75 % (§2). The savings are each against today's suite (computed unless marked); they overlap (C4 speeds what C3 removes), so they do not add up exactly: §7.3 gives the totals.

| order | Job | tier | needs | touches | does | saving on the sum | on the clock |
|---|---|---|---|---|---|---|---|
| 1 | **sp-build** (new) | S2 | none | `Cargo.toml`, `.config/nextest.toml` | C4: `[profile.dev.package."*"] opt-level = 3` with its reason beside it (dependencies only; the workspace's crates stay at 0). C11: a `priority` override in the default profile for the binaries with the tests over 10 s (a1's `--config-file` filter, §3), so they start first | C4: 44-269 ms per integration test (measured, §7.1) ≈ 70 s calm (computed: ~6 commands or opens × 11 ms × 1,004 tests); store unit tests: −40 % on a store::verdict test, −30 % median on a legacy conversion, none seen on the schema-hash test (measured, §7.1) | C11: −25-30 s now (measured: wall − sum/16 from 22.5-32.6 s to 1.8 s) |
| 2 | **sp-fixed** (keep, widened) | O3 | none (parallel with sp-build) | `cli/tests/common/`, `cli/tests/` | C1 (teardown leaves its reaper alone; first, the largest measured gain), C3 (a migrated template store per test binary, copied into each env), C2 (one in-process `/proc` scan), C5 (one `Store` per test), C6 (one env in the multi-env tests of §6.5) | C1 ≈ 215 s (measured per env), C3 150-200 s, C2 70-100 s, C5 ~60 s, C6 ~20 s plus §6.5's rest: ≈ 480 s together (§7.3 model) | ≈ −30 s (sum / 16) |
| 3 | **sp-limits** (keep, widened) | O3 | sp-fixed (both edit `cli/tests/`) | `cli/tests/`, `core/src/`, `docs/contract.md`, `docs/commands.md` | §6.1: inject the adapter watchdog and `WATCHDOG_POLL` (`core/src/session/run.rs`), doctor's catalog `DEADLINE` (`core/src/doctor/mod.rs`), the back-off unit (`core/src/watch/schedule.rs`), the kill graces (`core/src/host/deadline.rs`, `core/src/git/hooks/exec.rs`), sub-second `CPD_TEST_DEADLINE_<TOOL>`; positive signals for the store lock holds. §6.3 (not in the draft): `SOURCE_POLL`, `FILE_POLL` (`core/src/session/run.rs`), the driver's `POLL`, `LEAD_POLL`, `KILL_PAUSE` (`core/src/driver/`), and the tests' "several polls" sleeps as absence windows of a few polls. `PI_STOP` is done on main (#104) | §6.1 194 s → ~15 s, §6.3 123 s → ~25 s: ≈ 275 s, less what #104 took (§2.1) | removes the 67-69 s watchdog tests (the tail without priorities) |
| 4 | **sp-engine** (keep, widened) | O3 | sp-limits | `cli/tests/`, `tui/src/` (new: the views' `REFRESH`), `docs/contract.md` if the refresh override is named there | §6.2 on #104's fractional ticks (0.1 s in the tests) with absence windows of a few ticks; §6.4 (not in the draft): an injectable refresh period for the plan, widgets and statusbar views (`REFRESH` = 2 s in `tui/src/{plan,widgets,statusbar}/mod.rs`), waits on the screen | §6.2 144 s → ~25 s, §6.4 21 s → ~5 s: ≈ 135 s, less what #104 took (§2.1) | ≈ −8 s |
| 5 | **sp-schema** (keep) | S3 | sp-build (its before number must include C4) | `core/src/store/` | §6.6: build each legacy version's database once per binary (`OnceLock`), in memory or `synchronous = OFF` for fixtures; same for store::verdict's reference databases | 27 s → ≤ 5 s ≈ 21 s (computed), less C4's share | ≈ −1 s; the longest unit test (8-15 s) leaves the clock's critical path |
| 6 | **sp-poll** (new) | O3 | sp-engine (both edit the same tests' waits) | `cli/tests/common/`, `cli/tests/` | one shared `wait_until` (with the harness) polling every 10-20 ms on an open `Store`, replacing the 10+ copies at 100 ms; then the per-test cost left after sp-fixed, measured per phase as §4 does, and cut | must find the last ~120 s (computed: §7.3, ~420-465 s left after Jobs 1-5); unmeasured | ≈ −8 s |
| 7 | **sp-guard** (keep, narrowed) | S3 | all of the above | `.config/nextest.toml`, `docs/implementing.md` | nextest `slow-timeout` with `terminate-after` as the per-test limit in the default profile; the priorities of sp-build trimmed to what is still long; the measuring rule (paired runs, `scripts/test-time.sh`) in `docs/implementing.md`; the 30 s / 300 s check | 0 | keeps it |

Acceptance changes proposed:

- **sp-fixed**: "a minimal TestEnv test at most 0.1 s on hs" is at the edge of what C1 + C2 + C3 + C5 give (computed: 0.49-0.55 s alone now, − 0.19 C1 − ~0.15 C3 − ~0.06 C2 − ~0.02 C5 ≈ 0.10 s; the test's own work is ~85 ms, §4 a) and depends on the load (the same test takes ~1.3× its time alone inside a full run, §7.1). Proposed: "at most 0.15 s run alone (median of 5), at most 0.1 s with sp-build merged".
- **sp-limits**: "every listed test takes at most 2 s on hs" holds for §6.1 and §6.3 (computed ≤ 1.5 s each); add the wrapper and driver poll tests of §6.3 to its list.
- **sp-engine**: "each engine and cockpit test at most 1 s (e2e at most 5 s)" stays; its "needs PR timing" is met on main.
- **sp-guard**: "at most 30 s wall and 300 s summed": 30 s is reachable after Jobs 1-5 (computed ~31-35 s; with sp-poll ~28 s); **300 s is not reached by Jobs 1-5** (computed ~420-465 s): it depends on sp-poll, and if the per-open schema check (17-22 ms optimised, several per test) turns out to be the floor, a cheaper check is a contract change (§4.3) for the Manager, not a test Job. sp-guard should report what remains rather than fail on it. These totals are over the base's 1,728 tests; main's 102 more (~100 s calm, §2.1) add to them in proportion (computed: ~30-35 s after the same cuts, if they are like the rest: unverified).
