# tests-mock-mig-engine: in-process migration of the engine tests

Research for Job `mr-engine` (Plan `tests-mock`, attempt `mr-engine-a1`). Static: every claim is read with `rg`/`read`; no test, no build.
`unverified` marks a guess. Seam names (`clk`, `runner`, `tmux-route`, `git-route`, `spawn`, `run_with`, `testkit`, `Ports`) come
from `0013-tests-in-process.md` and `tests-mock-{ports,tmux-route,git-route}.md`; none of them is on main, so their APIs are `unverified`.
Scope: `cli/tests/engine/*.rs` (13 files, 136 tests) plus the engine-crate unit tests that start a shell `tmux` or a real process (5): **141 tests**.
(`tests-mock-engine.md` counts 191 for engine and store together; this document counts only the engine scope.)

## 0. Two facts that decide the plan

1. **The engine tick is already a list of step functions.** `engine/src/watch/engine.rs` `Engine::tick` runs 16 `fn(&mut Engine)` steps
   (`restart::drain, workers::run, escalations::run, liveness::run, schedule::run, open::run, pr::reap, pr::start, start_prs, inbox::run,
   distill::run, titles::run, team::run, archive::run, sleep::run, archive_leads::run`), each `pub(super)`. `Engine::for_test(home, tmux)`
   (`engine.rs`, `#[cfg(test)]`) builds an engine on a temp home. Today every integration test starts the real `cpd watch` child and polls
   (`wait_until`, `wait_whole_ticks`, `thread::sleep`), though its subject is ONE step. A step that touches only the store (`open::run` E4,
   `inbox::run` E8, `archive::run` E12, `archive_leads::run` E14, and `schedule::run` up to `driver::start`) needs **no seam**: a unit test
   next to the code calls the step N times (N ticks, no wait). The pattern exists: `watch/incident_tests.rs`, `watch/workers/no_inbox.rs`.
   Time: steps read `OffsetDateTime::now_utc()` (`inbox.rs`, `archive_leads.rs`, `distill.rs`, `titles.rs`, `liveness.rs`), the fixtures use `ago(..)`
   relative to it, so a test needs `clk` only when it asserts an exact boundary, a cadence or a start-up downtime.
2. **A whole-engine tick needs `runner` anyway**: `pr.rs` runs `<recorded exe> pr poll` with `Command::new(&exe)` every tick (`PR_POLL_EVERY_TICK`),
   `workers::run` runs tmux, `team.rs` runs `node`. So a test that runs `Engine::tick` or `run(ports, until)` needs the default routes of `testkit`
   (empty tmux server, `pr poll` reply, empty gh search). Tests written as one step call do not.

Seam legend used below: `clk` = `Clock`/`ManualClock` plus `watch::run(ports, until)` and the start-up stage taking a clock (PR `ports-5` of the ports
research); `runner` = `Runner`/`FakeRunner` scripted routes (`fake(name, body)`, `fake_gh`, `fake_pi`, `calls`); `tmux-route` = the in-process tmux of
`tests-mock-tmux-route.md`; `git-route` = the git route; `spawn` = `Spawn`/`FakeSpawn` (launch log, `alive`, `exit`); `run_with` = the CLI entry in-process
(`cpd ...` through `main_with`). `testkit` (`Fx`, route fixtures) is needed by every `I` row and is not repeated. **U** = unit next to the code, **I** = in-process on `Ports`, **S** = smoke with the real binary, **X** = leaves the scope.

## 1. Inventory

| file | tests | what its tests start today | target |
|---|---|---|---|
| `cli/tests/engine/doctor.rs` | 27 | `cpd doctor` child via `TestEnv::cpd`; `env.fake` shell wrappers for git/tmux/gh/pi/node; a real `sleep 120` for the two hung-tool tests (`link_sleep`, `HANG`) with `CPD_TEST_DEADLINE_*` (real 0.3 to 1 s waits); `Flock` on `locks/watch.lock` (`hold_watch_lock`); `real_node()`; real SQLite files | I (26 library calls on `doctor::run`, 1 `run_with`) |
| `cli/tests/engine/watch.rs` | 70 | `cpd watch` child (`Fx::start*`, `wait_until`, `wait_whole_ticks`, `thread::sleep(2500 ms)` twice in the `@cpd_workers` tests); real tmux (`env.tmux`, `new-session sleep 600`); real git (`fx_bare`: `env.git_repo`, `create_lead_worktree`, `fx.env.git` in `merging_api`); `fake_gh`/`fake_pi`/`fake("cpd")`/`fake("tmux")` shells; real signals (`Signal::SIGTERM/INT/HUP/KILL`); `Flock` (`live_driver`, `hold_write_lock`'s second SQLite connection); `copy_executable(CPD, ..)`; `scripts/install-cpd.sh` with a fake `cargo`, python `sys.addaudithook` (6 `install_*` tests) | mixed: U 7, I 56, S 1, X 6 (section 2) |
| `cli/tests/engine/watch_inbox.rs` | 9 | `cpd watch` child; real tmux `new-session sleep 600` (a live pane so E2 leaves the Lead alone); `fake_gh`, `fake("cpd")`; `wait_until` | U (E8 `inbox::run`) |
| `cli/tests/engine/watch_curate_inbox.rs` | 5 | same child + tmux; `env.cpd(["inbox","list"])`, `["context","retire"/"docs"]` children | I (E8 + `run_with`) |
| `cli/tests/engine/watch_loop_guard_inbox.rs` | 4 | `cpd watch` child; real tmux pane; `fake_gh`, `fake("cpd")` | U (E8) |
| `cli/tests/engine/watch_sleep.rs` | 6 | `cpd watch` child; real tmux (chat slot: `set -p @cpd_session`); `wait_asleep` polls | I (E13 `sleep::run` lists panes) |
| `cli/tests/engine/watch_team.rs` | 6 | `cpd watch` child; a fake `node` shell helper that `sleep`s (`fx(period, sleep)`); `fake_pi`, `fake_gh`; `wait_until` | I (E11 `team::run`, E8 review item) |
| `cli/tests/engine/watch_worker_archive.rs` | 3 | `cpd watch` child; tmux private server; `thread::sleep(1500 ms)` in `live_attempt_keeps_its_worker_unarchived`; `env.cpd(["attempt","read"/"show"])` | U 2 (E12 `archive::run`), I 1 (`run_with`) |
| `cli/tests/engine/watch_archive_leads.rs` | 2 | `cpd watch` child; `env.fake_tmux()` + a session; `wait_until` | U (E14 `archive_leads::run`) |
| `cli/tests/engine/watch_titles_width.rs` | 2 | `cpd watch` child; real tmux (`new-session -x WIDTH`, `display -p #{pane_width}`); fake `pi`; the detached `cpd session retitle` child | I (E10 `titles::run` spawn + tmux width) |
| `cli/tests/engine/watch_distill_inbox.rs` | 1 | `cpd watch` child; `thread::sleep(2 ticks + 1 s)` (`two_ticks`); `env.cpd(["inbox","list"])` | I (E8 + `run_with`) |
| `cli/tests/engine/write_executable.rs` | 1 | `copy_executable(CPD, ..)` + a running script: tests the helper `write_executable` (ETXTBSY) | X (leaves with the shell fakes) |
| `cli/tests/engine/main.rs` | 0 | the module list | stays |
| `engine/src/watch/{incident_tests,incident_option_tests}.rs`, `watch/workers/no_inbox.rs` | 2 + 1 + 1 | `Tmux::new(..).with_program(<sh script that exits 0>)` written with `std::fs::write` + `chmod` | I (tmux-route) |
| `engine/src/watch/mod.rs` `tests::a_detached_childs_pid_is_in_its_file_until_it_is_reaped` | 1 | a real `sleep 30`, `kill(SIGKILL)`, `wait_until` (`thread::sleep(20 ms)`, `Instant`) | I (spawn) |

Counts: `rg -c '#\[test\]'` per file. Not in scope on purpose: `watch/poll_run_tests.rs` (4: threads and channels, no external; `Instant::now` only compares two reads),
`watch/wake_tests.rs` (7: `next_tick` is pure, time is an argument), `report.rs`, `liveness.rs`, `schedule.rs`, `titles.rs` etc. units (no process).
Allowlist lines this scope removes: `scripts/tripwire/allow.txt:119` (`cpd-engine watch::`), `pi/test/sleep-allow.txt:43-44` (`engine/src/watch/engine.rs 3`: `Instant::now` x2 and
`thread::sleep(NAP)` in the loop, go with `clk` in `ports-5`; `engine/src/watch/mod.rs 3`: the `wait_until` helper above, goes with its test).

## 2. Seams

Per test, the minimum set (a step call that stays in one step needs less than the whole-engine test it replaces). Reasons that are not obvious:

- **doctor.rs**: `doctor::run(_args)` (`doctor/mod.rs`) takes no ports and probes with `Command::new` through `deadline::output` (`doctor/tools.rs run_tool`). It needs `runner`
  (git, gh, pi, node as scripted routes; `tmux -V` is route G1 of the tmux research or a scripted reply). The two hung-tool tests need `clk` so the deadline is virtual (`Failed::Timeout`).
  The usage test (`an_argument_is_usage`) is clap parsing: `run_with`. The 24 others are library calls (config, store, tamper, tiers, adapter, watch lock).
- **watch.rs start-up and markers** (13 + 2): `Engine` start (`engine.rs` downtime and `consume_marker(.., OffsetDateTime::now_utc())`, lock mtime `SystemTime::now()` in `lock.rs`) needs `clk`;
  the three tests that call `cpd watch --mark-restart` also need `run_with`. `the_bound_is_ten_seconds...` says in its doc comment that exactly 10.0 s "would be a race with the process start": with `ManualClock` it is exact.
- **E2** (`liveness::run`): kills windows and lists panes through `engine.tmux`; `merging_api`/`e2_recovers_a_merge...` run real `git add/commit/merge`: `git-route`. The other 8 need only `tmux-route`.
- **E3** (`schedule::run` -> `driver::start`): git worktree, tmux window, detached driver: `tmux-route`, `git-route`, `spawn`. `e3_spawn_failures_requeue...` also `clk` (the `2^k` back-off is measured between `ended_at` and now; today `CPD_TEST_BACKOFF_UNIT` and real waits).
  `e3_starts_the_driver_after_the_engines_binary_is_replaced` becomes: the `FakeSpawn` launch's argv[0] is the exe recorded at start (no file is replaced).
- **E5** (`pr::*`): runs `<exe> pr poll` through the runner; the 502 / timeout / cut-response tests need the poll's own error to reach the engine, so `cpd` routes to `run_with` and `gh` is a runner route; time (`pr_poll_seconds`, rate-limit reset, `took`) needs `clk`.
  The two E5 update-Job tests need no time. Whole-poll tests could use a scripted `pr poll` reply instead of `run_with` (cheaper; `unverified` that the failing-record logic only reads the reply).
- **Store busy** (2): the product reaction is Rust (`persist`, `is_store_busy`); SQLite's wait is real time inside rusqlite. Open the store with `store_busy_ms = 0` (`Store::open_persistent`, `engine.rs`)
  so the lock error is instant and drive the retry through `clk` (`unverified` that the retry loop sleeps through a clock; today it is `persist(&home, stopping)`). The second connection holding the lock is real SQLite on a temp file: kept.
- **E9 retry** (`e9_retries_a_failed_run...`): `distill.rs` spawns `<exe> context distill` (`Spawn`), which runs `pi` (`runner`): `FakeSpawn` launch routed to `run_with` (testkit helper) plus a `pi` route; `clk` for the quiet period.
- **`watch --check`** (3): the verdict is `run_with(["cpd","watch","--check"])` plus a tmux route that prints `_` for tabs (`tmux_printing_underscores`): route fault injection, `unverified` that the route has it. The "writes no file, no row" assertions read a real SQLite file: kept.
- **flock**: `live_driver` (11 uses), `hold_watch_lock`, `watch.lock`: a lock on our own file in a temp dir; no external software. Kept real (`unverified` that policy reads it so; if not, `Spawn.alive` is not enough, a `Lock` seam is a new item).

### Needs-set table (decides which PR starts after which seam; sums to 141)

| needs-set | tests | what |
|---|---|---|
| none (U) | 24 | `watch_inbox` 9, `watch_loop_guard_inbox` 4, `watch_archive_leads` 2, `watch_worker_archive` 2, `watch.rs` E4 x6 (`e4_adds_one_open_job...`, `..as_a_draft_while..`, `..queues_at_once..`, `..leaves_a_queued..`, `..gives_the_open_job_the_tier..`, `..adds_no_open_job_to_a_waiting_pr..`), `a_stuck_job_wakes_its_context_manager` |
| clk | 14 | `watch.rs`: 2 downtime tests, 10 marker tests, `watch_stops_on_sighup`, `watch_stopped_waits_for_a_busy_store` |
| runner | 26 | `doctor.rs` 24; `watch.rs` `the_engine_neither_imports_nor_syncs_a_context_store`, `e5_polls_with_the_engines_recorded_executable` |
| clk + runner | 8 | `watch_team` 6, `doctor.rs` 2 hung-tool tests |
| tmux-route | 23 | `watch_sleep` 6, `watch.rs` `@cpd_workers` 2, E2 x8, `--ensure` x3; engine-crate shell-tmux units 4 |
| tmux-route + git-route | 1 | `e2_recovers_a_merge_whose_driver_died_after_the_compare_and_swap` |
| spawn | 2 | `e1_expires_open_escalations_whose_asker_is_dead` (`escalations.rs alive`), `mod.rs a_detached_childs_pid...` |
| tmux-route + spawn | 2 | `watch_titles_width` 2 |
| tmux-route + git-route + spawn | 4 | E3: `one_tick_schedules`, `starts_a_job_whose_stored_tier..`, `starts_one_urgent_job_beyond..`, `starts_the_driver_after_the_engines_binary_is_replaced` |
| clk + tmux-route + git-route + spawn | 1 | `e3_spawn_failures_requeue_at_the_same_tier..` |
| run_with | 11 | `doctor::an_argument_is_usage`, `watch_refuses_unknown_flags`, `watch_curate_inbox` 5, `watch_distill_inbox` 1, `watch_worker_archive` `attempt read/show`, E4 `the_way_back_from_a_cancelled_open_job` and `pr_cancel_cancels..` |
| clk + run_with | 3 | `--mark-restart` x3 (`mark_restart_writes..`, `..with_no_watch_started..`, `a_marker_written_by_mark_restart_silences..`) |
| runner + run_with | 2 | E5 update Job x2 |
| clk + runner + run_with | 8 | E5 poll: `polls_every_pr_poll_seconds..`, `reports_a_failing_poll_once`, 502, timeout, `counts_the_failing_window..`, cut response, `pauses_below_100..`, `a_store_busy_for_longer_than_store_busy_ms..` |
| clk + runner + run_with + spawn | 1 | `e9_retries_a_failed_run_with_the_next_smaller_batch` |
| run_with + tmux-route | 3 | `watch --check` x3 |
| S (real binary) | 1 | `watch_holds_the_lock_refuses_a_second_and_stops_on_sigterm` (section 5) |
| X (leaves the scope) | 7 | `install_*` x6 (`rs-install`), `write_executable.rs` 1 |
| **total** | **141** | |

Needs `run_with` (CLI entry): 11 + 3 + 2 + 8 + 1 + 3 = **28**. Library call only (U or I): 141 - 28 - 1 S - 7 X = **105**.

## 3. The deleted tests of the scope (`tests-mock-deleted.md`: 50 of the 230)

Target is the same file, the test restored under its name unless noted. PR = section 4.

| deleted test | target file | seam | PR |
|---|---|---|---|
| `a_hung_gh_auth_is_a_failed_warning_and_doctor_finishes` | `doctor.rs` | runner, clk | B |
| `doctor_has_no_plannotator_port_check` | `doctor.rs` | runner | B |
| `each_tool_must_be_at_or_above_its_minimum` | `doctor.rs` | runner | B |
| `untouched_locked_jobs_pass_tamper` | `doctor.rs` | runner | B |
| `a_tick_with_no_change_writes_no_transaction` | `watch_inbox.rs` | none (E8 twice, count txns) | A |
| `e11_never_holds_the_tick_while_the_helper_runs` | `watch_team.rs` | clk, runner (a gated `node` route) | B |
| `an_engine_error_sends_no_message_to_a_manager` | `watch.rs` | none (`Engine::fail`) | A |
| `e3_does_not_start_a_job_on_a_tier_outside_the_worker_range_and_raises_one_error_per_job` | `watch.rs` | none | A |
| `e3_never_starts_a_draft_and_the_open_job_waits_while_one_remains` | `watch.rs` | none | A |
| `e3_reports_a_tampered_job_once_and_does_not_start_it` | `watch.rs` | none | A |
| `e4_adds_a_fresh_open_job_after_a_merged_one_on_a_pr_still_dispatch` | `watch.rs` | none | A |
| `e4_adds_no_open_job_to_a_dispatch_pr_whose_open_job_the_lead_cancelled` | `watch.rs` | none | A |
| `e4_adds_no_open_job_to_a_pr_whose_open_job_is_queued_or_failed` | `watch.rs` | none | A |
| `the_open_job_is_ready_only_when_every_other_job_is_merged_or_cancelled_and_one_is_merged` | `watch.rs` | none | A |
| `e3_does_not_start_a_job_in_its_backoff` | `watch.rs` | clk | B |
| `e5_a_cut_gh_response_is_an_error_only_after_the_window_and_then_once` | `watch.rs` | clk, runner, run_with | C |
| `e5_a_failure_that_is_not_transient_is_an_error_at_once_and_clears_the_record` | `watch.rs` | clk, runner, run_with | C |
| `e5_polls_at_its_due_time_not_at_the_next_tick` | `watch.rs` | clk, runner, run_with | C |
| `e5_polls_with_a_plan_in_pr_too_and_does_not_poll_more_often_than_every_pr_poll_seconds` | `watch.rs` | clk, runner, run_with | C |
| `e5_raises_one_error_after_ten_minutes_of_the_same_call_failing_and_none_before` | `watch.rs` | clk, runner, run_with | C |
| `e9_curates_the_backlog_one_batch_per_run` | `watch.rs` | clk, spawn, runner, run_with | C |
| `e9_reads_the_floor_from_config_toml_at_every_tick` | `watch.rs` | same | C |
| `e9_skips_a_context_under_the_floor_until_it_reaches_it` | `watch.rs` | same | C |
| `e9_skips_an_archived_context` | `watch.rs` | same | C |
| `e9_starts_again_at_60_when_the_state_changes` | `watch.rs` | same | C |
| `e9_stops_after_a_failed_run_of_batch_1` | `watch.rs` | same | C |
| `e2_asks_live_drivers_to_stop_on_timeout_or_cancel_with_escalations_pausing_the_clock` | `watch.rs` | tmux-route, clk | D |
| `e2_kills_the_window_of_a_dead_driver_it_ends_though_the_first_kill_window_fails` | `watch.rs` | tmux-route (fault: first `kill-window` fails) | D |
| `e2_skips_a_pane_with_an_unknown_view_and_reports_no_error` | `watch.rs` | tmux-route | D |
| `e2_stops_sessions_whose_pane_is_dead_or_gone` | `watch.rs` | tmux-route | D |
| `r2_removes_the_worktree_though_every_kill_window_fails` | `watch.rs` | tmux-route, git-route | D |
| `r3_kills_the_window_of_a_merge_that_did_not_happen_though_the_first_kill_window_fails` | `watch.rs` | tmux-route, git-route | D |
| `the_engine_counts_waiting_and_queued_jobs` | `watch.rs` | tmux-route (the `@cpd_workers` text) | D |
| `e3_starts_an_urgent_job_before_an_older_one` | `watch.rs` | tmux-route, git-route, spawn | D |
| `a_prerequisite_seen_merged_by_e5_still_lets_e7_start_its_dependent_in_the_same_tick` | `watch.rs` | runner, run_with, tmux-route, git-route, spawn, clk | D |
| `e10_counts_the_failures_in_a_row_from_the_last_success` | `watch.rs` (or `watch_titles_width.rs`) | tmux-route, spawn, runner, run_with, clk | D |
| `e10_does_not_report_an_empty_answer_or_a_preamble_within_the_limit_and_writes_no_title` | same | same | D |
| `e10_reports_a_failed_retitle_once_and_retries_only_what_is_due` | same | same | D |
| `e10_reports_a_titling_that_keeps_failing_past_the_limit_once` | same | same | D |
| `e10_retitles_an_idle_session_whose_turn_ended_after_its_title_and_no_other` | same | same | D |
| `e10_runs_at_most_two_retitles_at_once` | same | same (`FakeSpawn` launches, two alive) | D |
| `e10_skips_a_session_whose_title_lock_is_held_and_counts_held_locks` | same | same | D |
| `ensure_lists_panes_again_when_the_first_list_panes_fails_while_the_server_runs` | `watch.rs` | tmux-route (fault: first `list-panes` fails) | D |
| `ensure_starts_the_engine_for_a_caller_without_a_utf8_locale` | `watch.rs` | tmux-route (assert `-u` in the recorded argv) | D |
| `ensure_starts_the_engine_in_the_work_session_once` | `watch.rs` | tmux-route | D |
| `a_plain_ensure_waits_for_the_ensure_lock_another_call_holds` | `watch.rs` | tmux-route, clk (real flock on the ensure lock) | D |
| `a_crashed_engine_is_back_within_seconds` | `watch.rs` | tmux-route (hook string, section 5) | D |
| `an_engine_that_got_sigkill_is_back_within_seconds` | `watch.rs` | tmux-route (section 5) | D |
| `an_engine_killed_with_its_tmux_server_is_back_once_the_cockpit_is_rebuilt` | `watch.rs` | tmux-route (section 5) | D |
| `install_restarts_a_running_engine_with_ensure_and_says_so` | `cli/tests/tooling/install_script.rs` (`rs-install`) | not engine | X |

50 rows: 44 in `watch.rs`, 4 in `doctor.rs`, 1 in `watch_inbox.rs`, 1 in `watch_team.rs`.

## 4. Proposed migration PRs (4; sizes include the restored tests)

Order = fewest seams first. Counts: existing tests of section 2 plus restored tests of section 3. Total 182 = 133 existing + 49 restored (S 1, X 7 and the 1 restored `install_*` leave).

**PR `mr-engine-u`, needs: none. 33 tests (24 existing + 9 restored).** Files: `watch_inbox.rs`, `watch_loop_guard_inbox.rs`, `watch_archive_leads.rs`, `watch_worker_archive.rs` (2 of 3), `watch.rs` E4 x6 and E8 stuck-job, the 9 restored.
It can start today (needs neither `clk` nor `Ports`); the tests move to `engine/src/watch/*_tests.rs` next to the step they test (existing pattern).
1. docs: one page that names the step each file's tests exercise and the `Fx` rows the steps read (so the Lead can see which test is a step test).
2. test: an engine-crate `testkit` module (`for_test` home, `fixture(sql)`, `add_job`, `add_running`, `ago`) cut from `cli/tests/engine/watch.rs` `Fx` (without repo or tmux). Red: it fails to compile (stubs).
3. test: E4/E8 files (`watch_inbox` 9, `loop_guard` 4, E4 x6, stuck-job, restored `an_engine_error..`, E3 no-start x3, E4 x4, `a_tick_with_no_change...`): one step call per tick, no thread.
4. test: E12/E14 (`archive::run`, `archive_leads::run`, 4 tests) with the `Flock` for a live driver.
5. impl: delete the replaced integration tests and the `cpd-engine watch::` allowlist line only after PR D (this PR deletes nothing needed by later PRs).
`unverified`: that the E3 no-start tests (`schedule::run`) do not reach `driver::start` (they all stop before it: back-off, tier refusal, draft, tamper).

**PR `mr-engine-lib`, needs: clk, runner. 54 tests (48 existing + 6 restored).** Files: `doctor.rs` (26 + 4 restored), `watch_team.rs` (6 + 1), `watch.rs` start-up/markers (14), E6, `e5_polls_with_the_engines_recorded_executable`, `e3_does_not_start_a_job_in_its_backoff`.
1. docs: the `doctor::run(args, &Ports)` and `watch::run(ports, until)` call shapes the tests use (names `unverified` until `ports-5` lands).
2. test: `doctor.rs` 30 on `FakeRunner` routes (healthy tools as a `testkit` fixture; hung tool = `Failed::Timeout` by `ManualClock`).
3. test: start-up and markers (14) on `run(ports, until)` with a `ManualClock`: `until` is the first tick; no signal, no `wait_until`.
4. test: `watch_team.rs` 7 (gated `node` route for `e11_never_holds_the_tick`), E6, E5-exe, back-off.
5. impl: remove `real_node`, `link_sleep`, `HANG`, `CPD_TEST_DEADLINE_*` reads that nothing else uses (`unverified`: `rg` first).

**PR `mr-engine-cli`, needs: clk, runner, run_with, spawn. 36 tests (25 existing + 11 restored).** Files: `watch_curate_inbox.rs` 5, `watch_distill_inbox.rs` 1, `watch_worker_archive.rs` 1, `doctor::an_argument_is_usage`, `watch_refuses_unknown_flags`, `--mark-restart` x3, E4 x2 (`pr open`/`pr cancel`), E5 x10 (2 update + 8 poll) + 5 restored, E9 retry + 6 restored.
1. docs: the `cpd` -> `run_with` route of the `FakeRunner` (the poll and the distill child run in-process, synchronously) and which tests rely on it.
2. test: `run_with` tests (usage, unknown flags, `--mark-restart` x3, curate/distill/attempt-read, E4 x2).
3. test: E5 (15) on a `cpd` route to `run_with` plus a `gh` route; the busy-store test with `store_busy_ms = 0` and `ManualClock`.
4. test: E9 (7) on `FakeSpawn` launches routed to `run_with` plus a `pi` route.
5. impl: if the `cpd` route is not enough for the 502/cut/timeout reporting, the scripted `pr poll` reply variant (section 2, E5).

**PR `mr-engine-hosts`, needs: tmux-route, git-route, spawn (+ run_with, clk, runner from the earlier PRs). 59 tests (36 existing + 23 restored).** Files: `watch_sleep.rs` 6, `watch_titles_width.rs` 2, `watch.rs` `@cpd_workers` 2, E1, E2 x9, E3 x5, `--ensure` x3, `watch --check` x3, the 4 engine-crate shell-tmux units, `mod.rs record_detached`, 23 restored (E2 x4, R2/R3, E3 urgent, E7, E10 x7, `ensure` x3 and the plain-ensure lock test, hook x3, counts).
1. docs: the tmux fault injections the tests need (first `kill-window` fails, first `list-panes` fails, tabs printed as `_`, window command exits with 7 / by a signal) and which `FakeSpawn` calls E1/E3/E10 read.
2. test: engine-crate units (4 shell-tmux, `record_detached`) and E13/E1/workers (11): these need only `tmux-route`/`spawn` and can go before the rest of the PR.
3. test: E2 + R2/R3 (15), `--ensure` (6), `watch --check` (3).
4. test: E3 (6) + E7 + E10 (11) on `FakeSpawn`: launch argv, `alive`, `exit(pid, code)`; the driver is not run (its tests are in flow).
5. impl: delete `scripts/tripwire/allow.txt:119` and `pi/test/sleep-allow.txt:43-44`, the integration files that no longer have a test, `TestEnv::tmux`'s engine users.
Risk: this PR is the largest; if it does not fit in 90 minutes, split off Job 4 as `mr-engine-hosts-2` (the Lead then has 5 PRs; the brief says at most 4).

## 5. Not migratable as asked

| test | subject | proposal that keeps policy [tests] |
|---|---|---|
| `watch_holds_the_lock_refuses_a_second_and_stops_on_sigterm` | real SIGTERM and SIGINT to a real `cpd watch` (`catch_signals`, `sigaction`, the process-global `STOP`) | keep ONE smoke S (real binary, temp home, empty `PATH` so no `gh`/`tmux` runs; it asserts `watch-started`, `watch-stopped`, exit 0 and the refused second start); `unverified` that an empty `PATH` leaves the engine quiet enough (E5 reports `gh` missing as an error Event, which this test does not assert). The in-process part (stop request, one `watch-stopped`) is `watch_stops_on_sighup`, already in PR `lib`; the signal-to-flag mapping stays in `catch_signals` (3 lines, covered by the smoke) |
| the 3 deleted supervision tests (`a_crashed_engine_is_back_within_seconds`, `an_engine_that_got_sigkill..`, `an_engine_killed_with_its_tmux_server..`) | tmux FIRING the `pane-died` hook and running `cpd watch --ensure` (`core/src/host/tmux.rs` `PANE_DIED_HOOK`, `apply_server_options`) | tmux's behaviour is external: test only that `apply_server_options` installs the exact hook string and that the command it runs (`watch --ensure`) starts one engine in a rebuilt cockpit; drop the "comes back within seconds" assertions (a fake tmux that fires hooks is `unverified` and tests the fake) |
| `watch.rs` `install_*` x6 and the deleted `install_restarts_a_running_engine_with_ensure_and_says_so` | `scripts/install-cpd.sh`: a shell script with a fake `cargo`, `flock`, python `os.rename` audit hook (`record_renames`) | not engine code: `rs-install` moves the logic to `cpd install` (ADR `0013`, `rs-install`, 51 tests in `install_script.rs`); these 7 go there with `FakeRunner` calls in order and a temp dir for the two-rename adapter swap. The python audit has no in-process equivalent: assert the `rename(2)` calls through a fs port or by the final tree (`unverified`) |
| `write_executable.rs` (1) | the test helper `write_executable` against ETXTBSY | not our code (a helper): delete with `TestEnv::fake`/`copy_executable` when `testkit` replaces them (`tk-2`); no replacement test |
| `live_driver`, `hold_watch_lock`, `hold_write_lock` (12 + 3 + 2 tests) | `flock` and a second SQLite connection on a temp file | kept real: our own lock file, not external software (`unverified` that policy reads it so). If it does not, the only way to fake a held lock is a `Lock` port, which no document proposes: ask |
| the busy-store pair | SQLite's real busy wait | `store_busy_ms = 0` plus a held connection (section 2); `unverified` |

Nothing else of the scope is OS behaviour: no pty, no `exec` replacing the process (`restart.rs` and `titles.rs`/`distill.rs` spawn children, which `FakeSpawn` records).
