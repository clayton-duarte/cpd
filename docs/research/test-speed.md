# Research: where the Rust test run's time goes (Plan quality, Job sp-profile, hs-d284)

Base `8df1af8` (main). Machine hs: 16 CPUs, shared with other Workers' builds and Gates (each number says what ran beside it). Target (hs-d284): `scripts/cargo.sh nextest run --workspace`, build excluded, at most **30 s on the clock** and **300 s added up**, every test kept.

Every number here was measured on hs unless it says "computed" (arithmetic on measured numbers) or "unverified".

## Summary

- **Before** (3 runs with no other test run beside them): **120.5-123.6 s on the clock, 1,408-1,487 s added up, 1,728 tests**. With another test run beside it the same suite takes 125-149 s and 1,525-2,018 s (§2).
- **The clock is the sum divided by 16, plus a 30 s tail.** For the first 90 s all 16 test threads are busy; then only the two `session_watchdog` tests (67-69 s, started at 52 s) run. With a nextest `priority` on the slow tests (config only, measured) the tail is gone: clock = sum / 16 + 1.8 s (§3). So the clock target follows from the sum target once the long tests start first and none is near 30 s: 300 s / 16 + 2 s ≈ 21 s.
- **The test threads mostly wait.** The run used 5.5-5.8 of 16 CPUs (user + sys / wall).
- **The fixed cost of an integration test is about 0.5 s of its 0.8 s average** (§4): the teardown (0.29-0.38 s, a third of it killing the env's own reaper and then a 0.1 s grace and a second `/proc` scan), and creating and migrating a fresh store (0.17-0.23 s). 1,126 `TestEnv`s run per suite (§5).
- **Over 2 s: 103 tests** (median of the 3 runs), 1,000 s of the 1,408 s. By cause (§6): real product limits that tests cannot shorten (the 60 s adapter watchdog, the 10 s pi stop, the 10 s doctor deadline, the 2^k s spawn back-off, the 5 s kill grace), 1 s engine ticks and the "several ticks" sleeps written around them, the wrapper's and driver's 2 s polls, the cockpit's 2 s refresh, fixture cost (a test that builds 72 envs), and real work (the store::schema unit tests building every version's database).
- **The plan reaches 300 s only with sp-fixed doing more than its draft says** (§7): the drafted Jobs remove about 1,000 s and leave about 420-460 s (computed). The rest is the per-open schema check (every `Store::open`, 17-30 ms, about 6-14 per test), the cockpit's and engine's poll granularity, and many tests' 100 ms poll sleeps. Recommended additions: optimise dependencies in the dev profile (measured, §7), keep one store open per test, and nextest priorities (measured). 30 s on the clock follows once the sum is under about 450 s with priorities set.

## 1. The tool: `scripts/test-time.sh`

`scripts/test-time.sh [nextest args]` builds the tests (`nextest run --no-run`, not timed), then runs `scripts/cargo.sh nextest run --workspace --profile timing [args]` and prints, from the JUnit report of the `timing` profile (`.config/nextest.toml`: the default profile plus `junit.path`):

- the load before and after (`uptime`);
- **wall**: nextest's run time (its `Summary`, build excluded) and the clock time of the whole command;
- **sum**: every test's time added up, and the **count** (failed, errors);
- the 30 slowest tests;
- per test binary: summed time, count, slowest test.

It exits with nextest's status after printing. Extra arguments go to nextest, e.g. `-E 'binary_id(cpd::plan)'` for a subset or `--config-file` for an experiment. nextest 0.9.148 writes the report under `<workspace>/target/nextest/timing/` even with `CARGO_TARGET_DIR` set (observed), which is where the script reads it.

## 2. Before: the full run, six times

`scripts/test-time.sh` on `8df1af8`, back to back, 2026-10-19. "Beside it" comes from `ps` every 5 s during runs 5-6 (a monitor script), and from `ps` before and after for runs 1-4.

| run | start | load before (1/5/15 min) | wall (s) | sum (s) | tests | CPU (user+sys / wall) | beside it |
|---|---|---|---|---|---|---|---|
| 1 | 09:27 | 7.05 / 9.22 / 6.84 | 124.8 | 1,525.2 | 1,728 | n/a | another Worker's serial `cargo test --workspace` |
| 2 | 09:30 | 6.68 / 9.54 / 7.41 | 132.9 | 1,673.1 | 1,728 | 5.75 | the same `cargo test --workspace` |
| 3 | 09:38 | 10.19 / 10.79 / 9.13 | 148.6 | 2,017.5 | 1,728 | 5.54 | another Worktree's Gate (`nextest run --workspace`), whole run |
| **4** | 09:41 | 10.23 / 14.12 / 11.01 | **120.5** | **1,420.3** | 1,728 | 5.76 | nothing seen before or after |
| **5** | 09:43 | 5.28 / 11.53 / 10.52 | **120.6** | **1,408.1** | 1,728 | 5.74 | nothing (monitor: only this run; load 5.3-10.2) |
| **6** | 09:45 | 4.98 / 10.07 / 10.16 | **123.6** | **1,487.3** | 1,728 | 5.82 | nothing until 09:47:35, then another Gate's nextest and a release build for the last 30 s (the tail) |

**The before numbers are runs 4-6: wall 120.5 / 120.6 / 123.6 s, sum 1,420 / 1,408 / 1,487 s, 1,728 tests.** All 1,728 pass in every run. A second test run beside it adds 10-30 % to the sum and the clock (runs 1-3); the Gate log numbers in the brief (124 s / 1,471 s; 160 s / 2,295 s) are in the same range. The sum depends on the load, so a before/after comparison must be paired (same conditions, interleaved: §7 does).

Distribution (run 5): 621 tests under 0.1 s (5.2 s together; nearly all unit tests), 82 at 0.1-0.3 s (18.1 s), 148 at 0.3-0.5 s (61.6 s), 513 at 0.5-1 s (374.5 s), 261 at 1-2 s (341.3 s), 73 at 2-5 s (225.8 s), 21 at 5-10 s (141.8 s), 9 over 10 s (239.7 s).

By kind (run 5): 1,004 integration tests (`cli/tests/`, the `cpd::*` binaries) take 1,297.8 s; 908 of them under 2 s take 723.2 s, **0.80 s on average**. The 724 unit tests take 110.3 s, of which `cpd-core` store tests 80.9 s (85 tests) and the rest of `cpd-core` 24.8 s (305 tests); `cpd-tui` 3.1 s (310 tests).

Per binary (run 5, the largest): `session_watchdog` 142.6 s (3 tests), `cpd-core` 105.7 s (390), `watch` 104.0 s (41), `pr` 80.9 s (75), `plan` 63.8 s (49), `session` 63.1 s (53), `driver` 55.0 s (25), `session_reload` 54.4 s (10), `merge` 49.9 s (40), `config` 44.8 s (30), `doctor` 44.6 s (32), `job` 40.1 s (32), `git` 39.2 s (48), `tui_tree` 36.9 s (45), `context_distill` 32.9 s (13). The full table is what `scripts/test-time.sh` prints.

## 3. Where the clock goes

From the JUnit timestamps (each test's start and duration) of run 5, the tests running at once, per 5 s: **16.0 from 0 to 90 s, then 4.8, 2.7 and 2.0 until the end.** The last 30 s are the two `session_watchdog` tests: nextest starts tests in binary order, these start at 52.0 s and take 66.9 and 68.5 s. For every run, wall − sum / 16 = 22.5-32.6 s: that is the tail.

**Measured with priorities** (run "prio", 10:06, a temporary `--config-file` = the repo's plus `priority = 100` for the binaries with the slowest tests; another Worker's Gate ran beside it the whole time, load 8-33): wall 140.8 s, sum 2,222.9 s, **wall − sum / 16 = 1.8 s**; the two watchdog tests started first and the last 5 s still had 12 tests running. So with priorities **the clock is the sum / 16 + about 2 s**, provided no single test is longer than that. At the target, 300 s / 16 + 2 s ≈ 21 s.

The threads are not busy computing: user + sys CPU over the run is 5.5-5.8 CPUs of 16 (`/usr/bin/time` around the script, runs 2-6; it counts only processes the run waits for, so detached tmux servers and reapers are missing: a lower bound). More test threads than CPUs would shorten the clock further at the same sum (unverified: not measured; the sum would rise somewhat with contention).

## 4. The cost of each phase, per test

Method: one test run alone with `strace --seccomp-bpf -f -ttt -e trace=process -e signal=none` (only process syscalls stop the tracee, so the overhead is small: the floor test takes 0.49-0.55 s without strace, 5 runs, and 0.56-0.58 s with it, 3 runs); a script turns the trace into a timeline of every process (start at its `execve`, end at its exit). The test's own work in between (in-process `Store::open`, fixture SQL) is the gap between processes. Load 4-17 during these (another Worker's serial `cargo test`).

### (a) A minimal TestEnv test: `cpd::plan new_refuses_an_empty_brief_file_and_writes_nothing`

564 / 566 / 575 ms (3 traced runs):

| phase | ms | what |
|---|---|---|
| process start, `TestEnv::new` | 4-5 | temp dir, 5 deny shims, `.gitconfig`, `teardown.sh` |
| reaper start | 1 | `sh -c 'setsid sh teardown.sh watch <pid> &'`; the reaper then loops on `sleep 1` (in the background, not on the test's path) |
| `cpd context new` (first command: **creates and migrates the store**) | **173-182** | the store's creation: 15 migrations, the history triggers, the schema check. Timed alone (`date` around it, fresh `CPD_HOME`, 5 runs): first command 207-266 ms, the second 29-35 ms, `events list` 26-40 ms, `--help` 6-8 ms |
| git fixture (`git_repo`) | 13-15 | `init` 2.7, `add` 2.0, `commit` 6.3-7.8 of which `git maintenance run --auto` 1.4-1.8 (60 ms in the brief came from a full strace, which stops every syscall) |
| test: write the brief, `Store::open`, one query | ~20 | in-process |
| `cpd plan new` (refused, `usage`) | 4-5 | refused before it opens the store |
| test: second query (`Store::open` again) | ~25 | in-process |
| **teardown** | **289-306** | `tmux kill-server` 1.7; **scan 1**: `grep -laF <dir> /proc/[0-9]*/cmdline` 6-8 plus `find /proc -mindepth 2 -maxdepth 2 -name cwd -lname '<dir>*'` 60-65, ~100 with the glob and the pipeline; it finds **the env's own reaper** (`sh teardown.sh watch`: its command line contains the directory), sends it SIGTERM, then **`sleep 0.1`** (101 ms) before re-checking; **scan 2**: ~90; `rm -rf` 2-3 |

So the floor is: teardown ~300 ms, store creation ~180 ms, everything else ~85 ms. The reaper kill alone costs ~190 ms (the 0.1 s grace plus the second scan); a run with the reaper not targeted (experiment in a scratch copy, see §7) does one scan and no grace.

### (b) A typical engine test: `cpd::watch e2_kills_the_window_of_a_dead_driver_it_ends_though_the_first_kill_window_fails`

1.93 s median in runs 4-6 (the median of `watch.rs` is 1.9 s); traced 2,118 / 2,077 ms:

| phase | ms | what |
|---|---|---|
| TestEnv, git repo, `git config` ×3, Lead worktree (`git worktree add`, 7 git processes) | 0-95 | git: 15 processes |
| **store creation and fixtures, in the test process** | **~320** | `Fx::store()` opens a new `Store` for every `fixture`/`query` (the first creates and migrates it) |
| tmux fixture (`new-session`, `new-window`) | ~25 | |
| **`cpd watch` until the test stops it** | **1,209-1,289** | the first tick runs E2 at once; the claim needs the second E2 pass, **one `tick_seconds = 1` later**; the test polls every 100 ms (each poll a fresh `Store::open`); 0.82 s of the test thread is in `thread::sleep` |
| teardown | 358-383 | as (a), two scans and the 0.1 s grace |

### (c) A typical cockpit test: `cpd::tui_tree enter_opens_the_lead_of_a_plan_and_a_fork_with_session_open`

0.71 s median (the median of `tui_tree.rs` is 0.71 s); traced 752 ms:

| phase | ms | what |
|---|---|---|
| TestEnv, **store creation and fixture SQL in-process** | ~230 | |
| `tmux new-session` running `cpd tui view tree` | 16 | |
| view start until the test sees its first screen | ~40 | |
| keys and screen reads (`send-keys` ×5, `capture-pane`), the fake `cpd session open` ×2 | ~120 | polls of `capture-pane` |
| teardown | 327 | as (a) |

### (d) A test that starts node: `cpd::config tiers_reads_efforts_from_pi_with_pis_node`

No Rust test runs the pi adapter (`pi/lib/`, tested by `npm test`, outside this target); the Rust tests that start a real `node` are the Tier catalog probe's (`cpd tiers`, `cpd doctor`). 0.62 s median; traced 626 ms:

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

One full run under `strace --seccomp-bpf -f -ttt -T -e trace=process,clock_nanosleep,nanosleep` (09:48, another Gate's nextest beside it; 170.9 s, 1,728 tests). Under strace and that load the times are inflated, so this section gives **counts** and **shares**; the per-unit times are §4's.

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

The 103 tests whose median over runs 4-6 is over 2 s; 1,000 s of run 5's 1,408 s. "sleep" is the time the test process spent in `thread::sleep` and "envs" the number of `TestEnv`s (both from the traced run of §5). The change named is the one that removes the cause; the Job is §8's.

### 6.1 A real product limit (a constant the test cannot shorten)

| median (s) | test | the limit | change |
|---|---|---|---|
| 68.5, 66.9 | `session_watchdog` `a_pi_whose_adapter_reports_idle_within_the_limit_is_left_alone`, `a_pi_alive_with_its_state_stopped_for_sixty_seconds_is_restarted_with_one_error_event` | `ADAPTER_WATCHDOG` = 60 s and `WATCHDOG_POLL` = 2 s (`core/src/session/run.rs:48,51`); the tests sleep the limit out and check every 100 ms | inject the limit and the poll (a test-only override, like `CPD_TEST_DEADLINE_<TOOL>` of `core/src/host/deadline.rs`, or config keys); then ~1-2 s each |
| 7.2 | `session_watchdog a_pi_that_exits_is_handled_as_before_with_no_error_event` | a fixed 5 s sleep to show the wrapper waits for Enter (absence), and the 2 s poll | the same injected poll; a shorter absence window |
| 12.0 | `session_reload a_pi_that_ignores_sigterm_is_killed_after_ten_seconds_and_the_wrapper_starts_pi_again_for_a_new_pi` | `PI_STOP` = 10 s (`core/src/session/run.rs:46`) | inject `PI_STOP` |
| 10.6 | `doctor a_hung_catalog_probe_is_killed_at_10_seconds` | doctor's `DEADLINE` = 10 s (`core/src/doctor/mod.rs:23`; the test asserts `deadline_seconds = 10`); the other doctor deadlines already take `CPD_TEST_DEADLINE_<TOOL>` | let the catalog probe take the same override |
| 10.1 | `watch e3_spawn_failures_requeue_at_the_same_tier_never_fail_the_job_and_back_off` | the spawn back-off 2^k s (`backoff_seconds`, cap `BACKOFF_CAP_SECONDS` = 300, `core/src/watch/schedule.rs`), waited out over 1 s ticks | inject the back-off unit |
| 6.0 | `cpd-core host::deadline::tests::a_group_that_ignores_sigterm_is_killed_after_the_grace` | `GRACE` = 5 s between SIGTERM and SIGKILL (`core/src/host/deadline.rs:26`) | a grace parameter in the unit under test |
| 3.5, 2.1 | `store a_writer_that_cannot_get_the_lock_in_time_is_store_busy`, `a_second_writer_waits_for_the_first_and_reads_do_not` | the test's own writer holds the lock 3,000 / 1,500 ms | hold until the second writer has given up / entered (a positive signal) |
| 2.6 | `doctor a_hung_pi_node_is_a_failed_check` | `CPD_TEST_DEADLINE_NODE=1` (1 s, the override's floor: whole seconds) plus the kill | sub-second deadlines in the override |
| 2.3 | `git a_hook_is_killed_with_its_process_group_at_the_timeout` | the hooks' `GRACE` = 2 s (`core/src/git/hooks/exec.rs:75`) and a fixed 1.5 s sleep | inject the grace; wait for the survivor's absence positively |
| 2.8 | `watch e5_kills_a_hung_poll_child_and_its_group_at_the_deadline_with_tool_timeout` | the poll child's deadline and 1 s ticks | sub-second tick (§6.2) |

### 6.2 Engine ticks (`watch.tick_seconds = 1`, an integer ≥ 1 on main) and the "several ticks" sleeps around them

Each test waits for one to three 1 s ticks, and most also sleep a fixed 1.5-3.5 s ("two ticks", "several ticks": absence checks); `pr_poll_seconds` is also whole seconds. Change: fractional ticks and poll periods (PR timing, #104, not on main yet), a 0.1 s tick in the tests, absence windows of a few ticks.

| median (s) | sleep (s) | tests |
|---|---|---|
| 24.1 | 20.6 | `e2e_cross a_plan_across_two_repos_runs_a1_then_b_then_a2_and_a_review_job` (117 `cpd`, 441 git; fixed sleeps 2.5 s and 3 s; 1 s ticks and polls through a whole Plan) |
| 15.1, 5.9 | 14.3, 4.7 | `context_distill the_engine_starts_a_run_only_after_the_quiet_period_and_never_two_at_once` (the quiet period is backdated, not waited: the time is a fake pi that sleeps 3 s, then fixed sleeps of 4 s and 3 s over ticks), `the_engine_tries_a_failed_seq_once_until_a_newer_record_arrives` |
| 7.8 | 7.4 | `watch e5_polls_every_pr_poll_seconds_after_the_previous_poll_ended_whatever_the_plans` (several `pr_poll_seconds` periods) |
| 7.5, 5.8 | 4.9, 4.0 | `pr_start e7_starts_a_dependent_pr_from_a_fresh_fetch_once_its_prerequisite_is_merged`, `a_failed_start_marks_the_pr_and_appends_one_error_until_pr_start_retries` |
| 5.4 | 3.2 | `e2e a_whole_plan_runs_from_context_new_to_pr_poll` |
| 5.0, 4.9, 4.8, 4.8, 4.6, 4.4, 3.6, 3.6, 3.5, 3.5, 3.4, 3.4, 3.1, 3.0, 2.7, 2.6, 2.0, 1.9 | 1.0-4.2 | `watch`: `e5_reports_a_failing_poll_once`, `e5_pauses_below_100_requests_until_reset_and_reports_one_gh_rate_limited`, `e5_polls_with_a_plan_in_pr_too_and_does_not_poll_more_often_than_every_pr_poll_seconds`, `e3_never_starts_a_draft_and_e4_waits_while_one_remains`, `e3_does_not_start_a_job_in_its_backoff`, `e4_sends_all_merged_again_after_a_job_is_appended_and_merged`, `e2_removes_the_lock_of_a_dead_driver_it_ends_and_does_not_recreate_it`, `e2_skips_a_pane_with_an_unknown_view_and_reports_no_error`, `e3_reports_a_tampered_job_once_and_does_not_start_it`, `e4_appends_all_merged_once_when_no_job_is_left_to_run`, `e4_waits_while_a_job_is_queued`, `the_engine_neither_imports_nor_syncs_a_context_store`, `e2_asks_live_drivers_to_stop_on_timeout_or_cancel_with_escalations_pausing_the_clock`, `r2_removes_the_worktree_though_every_kill_window_fails`, `e2_stops_sessions_whose_pane_is_dead_or_gone`, `e3_starts_the_driver_after_the_engines_binary_is_replaced`, `e2_kills_the_window_of_a_dead_driver_it_ends_though_the_first_kill_window_fails` (§4 b), `r3_kills_the_window_of_a_merge_that_did_not_happen_though_the_first_kill_window_fails` |
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
| 2.0, 2.0, 2.0, 2.1, 1.9 | 1 | 21-26 / 30-40 | `plan plans_moving_to_phase_plan_at_once_all_move`, `a_review_after_the_lock_covers_the_drafts_and_its_approval_queues_exactly_them`, `lock_refusals_change_nothing`; `plan_cancel cancels_jobs_closes_open_prs_and_the_lead_and_archives_the_plan`, `a_failing_job_cancel_halfway_is_finished_by_a_rerun` |
| 4.0 | 1 | 32 / 0 | `context_distill nothing_the_audit_lists_is_lost` |
| 3.1, 2.9, 2.7 | 3, 2, 2 | 19-20 | `job add_draft_after_the_lock_is_an_appended_draft_that_stays_editable_and_cancellable`, `add_draft_needs_only_a_title_and_stores_the_tier_and_the_spec_as_null`, `edit_completes_a_stub_draft_with_its_tier_and_its_spec` |
| 3.5, 2.2, 2.0 | 7, 6, 4 | 0 / 245, 132, 152 | `git each_protected_path_is_refused`, `a_bad_list_file_is_config_invalid`, `a_protected_path_the_attempt_has_as_the_base_has_it_is_allowed` |
| 3.4, 2.3, 2.3 | 3, 2, 2 | 0 / 246, 185, 152 | `merge an_attempt_that_merges_a_base_with_changed_protected_files_is_merged`, `a_protected_file_deleted_on_the_base_and_in_the_attempt_is_merged`, `an_attempt_that_changes_a_protected_file_to_anything_but_the_bases_is_tampered` |
| 2.5 | 3 | 4 / 95 | `attempt submit_refuses_an_attempt_that_is_not_running` |
| 2.8, 2.7 | 3, 2 | 4 / 144, 2 / 198 | `pr pr_add_no_needs_and_needs_override_the_reopen_default`, `open_and_rebase_run_r_for_the_repos_orphaned_merges_first` |
| 2.4, 2.2, 2.2 | 1, 3, 1 | 20, 3, 16 | `pr_crud edit_refusals_write_nothing`, `add_to_an_archived_plan_is_archived`, `add_refusals_write_nothing` |

### 6.6 Real work (CPU)

| median (s) | tests | what |
|---|---|---|
| 8.3, 8.0, 3.0 | `cpd-core store::schema::tests::a_legacy_store_of_every_version_converts_keeping_every_row_and_id`, `migrating_an_empty_database_gives_every_versions_schema_hash`, `a_tampered_legacy_store_is_refused_and_left_as_it_was` | build a database of every legacy version from scratch, migrate it, hash it (no process, no sleep: 0 s in `thread::sleep`); 13-14.5 s in the loaded Gate logs of the brief, 12.6-13.1 s in runs 1-2, 21-26 s under strace and load: CPU-bound, so they grow with the load |
| 2.9, 2.9, 2.0 | `cpd-core store::verdict::tests::inside_the_window_the_objects_of_unknown_migrations_are_not_checked`, `inside_the_window_the_objects_this_binary_created_are_checked`, `store::tests::two_plans_kind_migrations_applied_in_either_order_keep_both_kinds` | reference databases built per case |

Change: build each version's database once per test binary (a `OnceLock`), in memory or with `synchronous = OFF` for fixtures; dependencies at `opt-level = 3` (§7) speed the SQLite and hashing work for every one of these (the size of that gain on these tests is unverified).

## 7. Changes, their savings, and the totals after them

"Per env" savings multiply by the 1,126 `TestEnv`s of a run (§5). Savings on the clock assume priorities (§3): clock ≈ sum / 16 + 2 s.

| # | change | Job | saving on the sum | how known |
|---|---|---|---|---|
| C1 | **The teardown does not target the env's own reaper** (it records its pid; the reaper ends by itself once the directory is gone): no 0.1 s grace, no second scan | sp-fixed | **~0.19 s per env ≈ 215 s** | measured per env (§4: 101 ms grace + 89-93 ms second scan); A/B on a subset: see below |
| C2 | Replace the two-process `/proc` scan (`grep` + `find`) by one in-process scan, or by `PR_SET_CHILD_SUBREAPER` on the test process and a walk of its own descendants | sp-fixed | ~0.06-0.09 s per env ≈ 70-100 s | computed from §4 (scan ~100 ms; a one-process scan 30-44 ms); the subreaper walk is unverified |
| C3 | **A migrated template store** made once per test binary and copied into each env's `cpd-home` (or created by the first `TestEnv` and reused) | sp-fixed | ~0.17-0.19 s per env that creates a store; ≤ 1,126 envs ≈ 150-200 s | measured per env (§4: 207-266 → 32-35 ms); the number of envs that create a store is unverified (≤ 1,126) |
| C4 | **Dependencies at `opt-level = 3` in the dev profile** (`[profile.dev.package."*"] opt-level = 3`: SQLite's C, `sha2`, `serde_json`; the workspace's own crates stay at 0) | new (sp-build, below) | per `cpd` command and per `Store::open` 29-35 → 18-22 ms; store creation 207-266 → 129-143 ms; suite: see the A/B below | measured per command; suite A/B below |
| C5 | Keep one `Store` open per test (the helpers open a new one for every query and poll) | sp-fixed | ~20-25 ms per avoided open × 3-14 per test ≈ 60-200 s | computed (§4 counts and per-open cost) |
| C6 | `config_refuses_each_bad_file_with_its_key_path` with one env (rewrite `config.toml` per case) and the other multi-env tests of §6.5 | sp-fixed | 22 → ~1 s, ≈ 30 s for §6.5 beyond C1-C3 | computed |
| C7 | Real limits injectable (§6.1): adapter watchdog and its poll, `PI_STOP`, doctor's catalog deadline, spawn back-off, kill grace; sub-second test deadlines | sp-limits | §6.1 is 230 s; to ≤ 2 s each ≈ 205 s | computed |
| C8 | 0.1 s engine ticks and poll periods (§6.2), shorter "several ticks" windows | sp-engine | §6.2 is ~180 s; to ≤ 1 s each (e2e ≤ 5 s) ≈ 140 s | computed |
| C9 | Injectable wrapper/driver polls (§6.3) and view refresh (§6.4) | sp-limits / sp-engine (not in either draft yet) | §6.3 + §6.4 ≈ 150 s; to ≤ 1 s each ≈ 120 s | computed |
| C10 | store::schema fixtures built once (§6.6) | sp-schema | ~20 s (to ≤ 1 s each) | computed |
| C11 | **nextest `priority` for the long tests** (or no long tests) | sp-guard | 0 on the sum; **~30 s on the clock** | measured (§3) |

**A/B on a subset** (`plan`, `pr`, `tui_tree`, `config`, `watch`: 240 tests), interleaved base / C1 / C4, three times each, with at most one other test run beside: AB_RESULTS

**The totals after all of them (computed):** from run 5's 1,408 s: C1-C3 and C5 take ~0.45-0.55 s off each of the 908 integration tests under 2 s (0.80 s on average → ~0.30 s) and off every env of the slow ones; C6-C10 bring the 103 tests over 2 s to ~1 s each (~110-130 s together). That leaves **~270 s for the 908 short integration tests + ~120 s for the long ones + ~40 s of unit tests ≈ 430-460 s**, and a clock of ~29-31 s with priorities: **the drafted Jobs alone do not reach 300 s.** What closes the gap, in order of size:

1. C4 (optimised dependencies): every remaining `cpd` command and `Store::open` ~35-40 % cheaper (measured per command); with ~3,000 `cpd` commands and several thousand opens per run this is of the order of 50-100 s (computed, unverified on the suite until the A/B above).
2. The minimal integration test after C1-C3 is ~0.15-0.2 s, not 0.1 s (computed: ~85 ms of non-fixed work in §4 a, plus one scan ~30-100 ms): sp-fixed's "at most 0.1 s" needs C2 and C5 too.
3. The 100 ms poll sleeps of the tests' `wait_until` helpers (10+ copies, docs/research/quality-tests-build.md G2): every wait costs half a period on average; 503 s of the traced sum is in `thread::sleep`. A 10-20 ms period (cheap once a poll no longer reopens the store, C5) takes ~40 ms off each wait (unverified count of waits).
4. More test threads than CPUs: the threads use 5.5-5.8 CPUs of 16 (§3); unverified.

## 8. Recommended Job split

The Lead's drafts in PR `speed` (sp-fixed, sp-limits, sp-engine, sp-schema, sp-guard) cover the causes; proposed changes:

- **sp-fixed** (keep; widen): C1 (first: largest, smallest change), C2, C3, C5, C6. Its acceptance "a minimal TestEnv test at most 0.1 s" needs C1 + C2 + C3 + C5 together (§7 item 2). Measure each with `scripts/test-time.sh -E 'binary_id(cpd::plan)'` before/after, paired.
- **sp-build** (new, S2, touches `Cargo.toml`): C4, dependencies at `opt-level = 3` in the dev profile. One line plus its reason; it costs a slower clean build of the dependencies (once per worktree with sccache; not measured here) and must say so. Can run first: independent of the others.
- **sp-limits** (keep): §6.1, plus the wrapper and driver polls of §6.3 (the same mechanism: an injected constant), which its draft does not name.
- **sp-engine** (keep, after PR timing): §6.2 and the cockpit refresh of §6.4 (not in its draft).
- **sp-schema** (keep): §6.6, the store::schema and store::verdict tests.
- **sp-guard** (keep, last): C11 priorities for whatever stays over a few seconds, and the per-test limit; its "30 s / 300 s" check uses `scripts/test-time.sh`, paired with a baseline run in the same conditions (§2: the sum moves 10-30 % with one other run beside it).

Order: sp-build and sp-fixed first (largest, independent), sp-schema in parallel, then sp-limits, sp-engine, sp-guard.
