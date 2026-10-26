# In-process migration of the flow tests, first half (`mr-flow-a`)

Scope: `cli/tests/flow/{ask, attempt, config, context, context_curation, context_digest, context_distill, context_manager,
context_manager_by_user, context_pending, context_pending_waiting, driver, driver_idle, driver_reload}.rs` = **230 tests**
(`rg -c '#\[test\]'`), plus **37 deleted tests** to restore (section 3). Every claim is read from the code in this worktree
or from the five reference documents; `unverified` marks a guess. No test or build was run. Names of seams (`Ports`, `Runner`,
`FakeRunner`, `Spawn`, `run_with`, `testkit`) come from `tests-mock-ports.md` and ADR 0013 and do not exist yet.

Vocabulary: `tmux-route` and `git-route` are the in-process routes **inside** `FakeRunner`, so each implies `runner`; a bare
`runner` is a scripted `Call -> reply` (a hook, `node`, `pi`, `gh`, `cpd watch --ensure`).

## Findings that shape the plan

1. **`cpd::cli::run(argv) -> Result<Output, CpdError>` is `pub` on main today** (`cli/src/cli.rs:93`) and the exit code is
   `CpdError::exit().code()` (`core/src/error.rs:334`). A test that starts no external process needs no `Ports`: `Fx::cpd`
   over `cli::run` suffices, and it becomes `run_with` later by editing one helper. The command `Args` structs have private
   fields (`core/src/context/mod.rs:255-266`), so a test cannot build a `Command` by value: argv is the only entry, there is no
   "library call" below `cli::run`. This is the seam set **N0 = {testkit}** below: 115 of the 230 tests.
2. Environment is read from the process (`CpdHome::from_env`, `core/src/env.rs:23`; `locate_pi` reads `PATH`,
   `core/src/config/catalog.rs:50`; `node()` reads `HOME`/`XDG_DATA_HOME`, `catalog.rs:94`). Ports doc §4.4 option 1 (set once
   at test start) is **not enough** for tests that change the environment between calls of one test: `a_record_names_its_session_and_plan`
   (`context.rs:296-299`: `CPD_SESSION` then `PI_SESSION_ID`), `a_retry_returns_the_existing_record` (`CPD_SESSION`, `context.rs:503`),
   `show_takes_the_context_of_the_selected_plan` (`CPD_PLAN`, 777), `retire_refuses_inside_an_agent_session` (loops over `CPD_SESSION`,
   `CPD_ATTEMPT`, `context_curation.rs:180`), `ask_takes_the_plan_from_the_environment` (`ask.rs:218`),
   `an_empty_cpd_home_variable_is_usage` (`config.rs:541`) and `tiers_prefers_pis_own_node_over_node_on_path` (`XDG_DATA_HOME`
   and `PATH` per call, `config.rs:775-800`). `Fx::cpd_with(env)` must set and restore around the call (process-global, `unsafe`
   `set_var` in edition 2024: `unverified` which edition) or `Ports.env` (ports doc option 2) must exist. Either way these 7 tests
   cannot share a process with another test that runs at the same time: the Gate runner is `unverified`.
3. **Two product spawns bypass the port today**: `core/src/context/distill.rs:434` (`git -C <repo> grep -l -w <id> -- docs *.md`,
   a direct `Command::new("git")`; the git-route table has a row for it, `tests-mock-git-route.md:40`) and `distill.rs:504`
   (`pi` through `wait_limited`, `distill.rs:~990-1001` unit tests start a real `sleep 30` and `sh`). Moving both into the Runner is
   a product change that PR2 depends on (job `impl` in PR2).
4. A wall clock is missing from `clk` (ports doc line 76). Tests that depend on it: `driver_log_lines_start_with_a_time`
   (deleted), `ago()` in `context_distill.rs:58`, `seed()` in `context_digest.rs:28` (SQL `strftime('now', '-N minutes')`). If the
   product reads a faked wall clock and the tests seed from SQLite's real `now`, the "recent days" and 30-day windows break:
   guess, `unverified`; the migration must seed from the same `Clock`.
5. `ManualClock::sleep` semantics decide whether a blocking `ask` or a driver needs a thread (advance-and-return vs park):
   ports doc names `advance`, `elapsed`, `naps` only. `unverified`; the two blocking `ask` tests and every driver test depend on it.

## 1. Inventory

"Starts" lists what a test starts today: `cpd` = the built binary through `TestEnv::cpd/command`; `fake(..)` = a shell wrapper
on PATH; `wait` = `wait_until`/`wait_some` polls; `sleep` = `thread::sleep`.

| path | tests | starts today | target |
|---|---|---|---|
| `cli/tests/flow/ask.rs` | 9 | `cpd`; `spawn_ask` child with `KillOnDrop` (a real `sleep 60` in `a_guarded_child_is_gone_after_a_panic`); `wait` x2 (`ask_blocks_until_answered_and_prints_the_answer`, `every_ask_is_a_new_escalation`); real `Instant` deadline and 1 s poll in `ask/question.rs:43-53` | I (8); the guard test is deleted with its helper (section 5) |
| `cli/tests/flow/attempt.rs` | 30 | `cpd` through `cpd_in`; real git (`env.git`, `fx.commit`, worktrees via `create_lead_worktree`/`create_attempt_worktree`); `sh -c` hooks (`hooks.pre-commit`, `[test-commands]` build/list/run, scripts that `kill -9 $$`); no tmux, no pi | I (30) |
| `cli/tests/flow/config.rs` | 47 | `cpd`/`cpd_isolated` (`PATH=env.bin()`); `env.fake("node", ..)` x~15; fake pi installs (`install_pi`, symlinks, `FAKE_CODING_AGENT` JS) read by `node`; **real `node`** (`real_node()` symlinked, lines 610, 922); a thread holding the migration lock with `Instant` asserts (`migrating_for`, 723-757) | I (46), S (1) |
| `cli/tests/flow/context.rs` | 41 | `cpd`; `env.fake("git")` recorder (`no_record_write_runs_git_or_makes_a_directory`, 235-245); a private tmux server `hs_tmux()` (`sleep` panes, `list-panes`, `rename-window`) in 4 archive tests (938, 977, 992, 1015); otherwise SQL fixtures only | I (41) |
| `cli/tests/flow/context_curation.rs` | 14 | `cpd`, SQL and library `propose(&mut store, ..)`; nothing external | I (14, no fakes) |
| `cli/tests/flow/context_digest.rs` | 11 | `cpd`; `fake_cpd_panes(WATCH_OK)` (a shell `cpd` answering `watch --ensure`, used by `plan new`); `manager_cockpit` fake pi + a pane that `exec sleep` (1 test, 335-337) | I (11) |
| `cli/tests/flow/context_distill.rs` | 33 | `cpd`; `fake_pi` shell script per test (`pi_says`, `pi_answers`, `pi_cut_after`), `env.fake("node")` catalog, fake gh/cpd; real git repos (`repo_with`, 2 tests); a `Flock` held by the test; `wait` x1 import | I (33) |
| `cli/tests/flow/context_manager.rs` | 4 | `cpd`; real tmux on a private server (`cockpit`, `panes`, `window_names`); fake `pi` that dumps its environment (`pi_envs`, `wait_until` polls); `fake_cpd_views` | I (4) |
| `cli/tests/flow/context_manager_by_user.rs` | 2 | same: real tmux, fake pi, `wait_until` (`pi_argv`) | I (2) |
| `cli/tests/flow/context_pending.rs` | 6 | `cpd`; nothing external | I (6, no fakes) |
| `cli/tests/flow/context_pending_waiting.rs` | 2 | `cpd`; helpers imported from `plan_render_cancelled.rs` (`env()`, `pr()`), not in scope | I (2, no fakes) |
| `cli/tests/flow/driver.rs` | 25 | `cpd job dispatch` starts a detached **driver** (`cpd attempt run`, fd 3 = lock, setsid) in a real tmux server; fake `pi` shell scripts that run real git and `cpd attempt submit`; real git + `post-rewrite` hook; `sh -c "exec 3<..; exec cpd attempt run"` (4 tests); `wait_some` x many (60 s), `common::poll`, `kill(pid,0)`, stall tests walk the real process tree and signal it (`driver/stall.rs`) | I (25; the fd-3 comparison also gets a U test, section 5.3) |
| `cli/tests/flow/driver_idle.rs` | 2 | same driver fixture, Linux only (`#![cfg(target_os = "linux")]`); reads `/proc/<pid>/stat` and `ionice -p` | I (2) |
| `cli/tests/flow/driver_reload.rs` | 4 | same driver; a copy of the built binary renamed over itself so the driver `exec`s it; a `tmux` shell wrapper that lists `/proc/$$/fd`; `thread::sleep(POLL * 8)`; fake pi | I (3), U (1) |
| **total** | **230** | | I 227, U 1, S 1, no test 1 |

## 2. Seams

### 2.1 Minimum set per file (tests per set; letters are the rows of 2.2)

| file | sets (tests) |
|---|---|
| ask.rs | A 6 (`ask_refuses_an_attempt_that_is_not_running`, `ask_refusals_before_the_attempt`, `ask_takes_the_plan_from_the_environment` with `--timeout-minutes 0`, `answer_*` x3); B 2 (the two blocking tests); X 1 |
| attempt.rs | A 8 (`read_*` x3, `show_returns_every_field..`, `show_refuses_what_does_not_exist`, `list_*` x3: `core/src/attempt/{read,show}.rs` call no git, no hook, no `Command::new`: `rg` found none); E1 10 (`submit_moves_the_attempt..`, `..works_from_a_subdirectory..`, `..twice..`, `..caller_outside_the_worktree..`, `..dirty_tree..`, `..no_commit..`, `..protected_paths`, `submit_checks_its_arguments_and_selection`, `a_work_jobs_checks_are_one_span..`, `a_review_jobs_submit_records_no_check_span`); E2 12 (quick check x4 incl. `a_refused_quick_check_ends_the_checks_span..`, kind checks x7 incl. `the_tests_commands_are_not_given_a_test_slot` and `list_refuses..`, `show_names_the_check_log_once_a_quick_check_has_run`) |
| config.rs | A 28 (25 `config*` and `context_*` key tests, plus `tiers_without_pi_on_path_is_pi_not_found`, `tiers_with_a_broken_pi_install_is_pi_not_found`, `tiers_checks_pi_before_the_catalog_and_config_before_both`: pi lookup is PATH + filesystem, no spawn); B 1 (`config_does_not_wait_for_a_migration`); C 16 (the `tiers` tests that run `node`); D 1 (`tiers_waits_out_a_migration_longer_than_store_busy_ms`); S 1 (`tiers_with_a_model_unknown_to_real_pi_modules_is_tier_model_unknown`, section 5) |
| context.rs | A 36; C 1 (`no_record_write_runs_git_or_makes_a_directory`: a `FakeRunner` with no route panics on any spawn); G 4 (`hs_tmux` archive tests) |
| context_curation / context_pending / context_pending_waiting | A 14 / A 6 / A 2 (checked: no `fake`, `tmux`, `git`, `sleep`, `wait_until`, `command(` in the three files) |
| context_digest.rs | A 6; C 4 (the tests that call `new_plan_with_brief`: `plan new` runs `cpd watch --ensure` through `ensure_watch`, `core/src/plan/lock.rs:168`; ports doc line 39); H 1 (`a_managers_first_message_holds_the_digest_and_the_uncurated_records`: `plan new` + `context manager` + a cockpit) |
| context_distill.rs | A 4 (`a_context_with_nothing_in_force..`, `the_context_is_required..`, `an_archived_context_is_skipped..`, `a_context_under_the_floor..`: refuse before pi); C 27 (pi through the Runner, `node` catalog; `the_distiller_gets_no_cpd_variable` asserts the recorded `Call.env_remove`; `a_second_run..` holds a real `flock` on a tempdir file, section 5); E2 2 (`docs_md_lists_the_grep_hits..`, `a_documented_target_that_is_not_a_grep_hit..`: `git grep`) |
| context_manager.rs / _by_user.rs | G 4 / G 2: `context manager` only calls `has_session`, `new_window`, `set_pane_session` (`core/src/context/manager.rs:92-107`); pi is started by the pane, so the pi argv and environment assertions move to the new-window command (section 5.7) |
| driver.rs | A 5 (`dispatch_refuses_*` x5, with an SQL-only fixture: they refuse before any worktree exists; `..lock_is_held` uses a real `flock` on a tempdir file); I 4 (the `attempt run` fd-3 tests); K 16 (every test that runs a driver loop) |
| driver_idle.rs | J 1 (`a_dispatched_attempts_driver_runs_at_nice_19..`: assert `Launch.idle` and the argv prefix `host/priority.rs::idle_prefix()`); K 1 (`the_worker_pi_runs_at_nice_19..`: the pane command of the Worker; `unverified` whether it can be a U test of the command builder) |
| driver_reload.rs | K 3; L 1 (`after_a_reload_fd_3_is_the_lock_and_the_drivers_children_do_not_hold_it`) |

### 2.2 Needs-set table (this table orders the PRs; sums to 230, and 267 with the 37 deleted tests)

| | needs-set (minimum) | tests today | + deleted | total | files |
|---|---|---|---|---|---|
| A | {testkit} (`Fx` over `cli::run`, env at start; **no `run_with`, no Ports**) | 115 | 0 | 115 | config 28, context 36, curation 14, attempt 8, ask 6, digest 6, pending 6, driver 5, distill 4, pending_waiting 2 |
| B | {clk, run_with} | 3 | 1 | 4 | ask 2 (+1), config 1 |
| C | {runner, run_with, testkit} | 48 | 0 | 48 | distill 27, config 16, digest 4, context 1 |
| D | {runner, clk, run_with} | 1 | 0 | 1 | config 1 |
| E1 | {git-route, run_with} | 10 | 2 | 12 | attempt |
| E2 | {runner, git-route, run_with} | 14 | 12 | 26 | attempt 12 (+12), distill 2 |
| G | {tmux-route, run_with} | 10 | 3 | 13 | context 4 (+1), manager 4 (+2), by_user 2 |
| H | {runner, tmux-route, run_with} | 1 | 0 | 1 | digest |
| I | {spawn, run_with} | 4 | 0 | 4 | driver |
| J | {git-route, spawn, run_with} | 1 | 0 | 1 | driver_idle |
| K | {git-route, tmux-route, spawn, clk, runner, run_with} (the driver loop; gh is a `runner` reply) | 20 | 19 | 39 | driver 16 (+18), driver_idle 1, driver_reload 3 (+1) |
| L | U: no seam (unit test next to `core/src/driver/lock.rs`) | 1 | 0 | 1 | driver_reload |
| S | smoke, real binary | 1 | 0 | 1 | config |
| X | no test (deleted with its helper) | 1 | 0 | 1 | ask |
| | **sum** | **230** | **37** | **267** | |

Reading: run_with is needed by every row except A and L; a bare library call below `run_with` exists only for the unit rows
(L, and the `idle_prefix()` assertion of J). Row A can start **before** `ports`, `clk` or any fake lands (finding 1).

## 3. Deleted tests of the scope (`tests-mock-deleted.md`, 37 rows). The files no longer hold the bodies: the sets of five attempt tests were read with `git show adb53fc8^:cli/tests/flow/attempt.rs`, every other set is classified from the name and the area's other tests, `unverified`

The row for `context_delete.rs` and `repo_config.rs` in that file belongs to other scopes.

| test | target file | set / why |
|---|---|---|
| `ask_times_out_and_expires_the_escalation` | ask.rs | B: `--timeout-minutes` deadline on `ManualClock`, no sleep |
| `submit_refuses_an_attempt_that_is_not_running` | attempt.rs | E1: four statuses; the fixture needs worktrees |
| `submit_takes_the_protected_list_from_the_policy_of_the_jobs_pr` | attempt.rs | E1: two repos, `git diff` names in two worktrees (`tests-mock-git-route.md`) |
| `a_command_that_cannot_run_is_tests_error_and_changes_nothing` | attempt.rs | E2: `kill -9 $$` becomes a scripted reply with a signalled `ExitStatus` (`ExitStatusExt::from_raw(9)`); the `hook_invalid` case needs no process |
| `a_kind_check_is_a_span_of_the_attempt_that_ends_also_when_it_refuses` | attempt.rs | E2 |
| `a_test_commands_table_with_a_list_is_read` | attempt.rs | E2: asserts the rendered `list` command (`{{ test }}`) in the recorded `Call` |
| `a_test_job_must_change_a_path_its_test_files_name_or_is_refused_test_files_missing` | attempt.rs | E2 (git diff; runner `unverified` for this one) |
| `a_test_job_whose_named_tests_pass_is_refused_tests_not_red_and_checked_again_at_every_submit` | attempt.rs | E2 |
| `a_test_jobs_quick_check_and_tests_run_in_one_checks_span` | attempt.rs | E2 |
| `a_test_or_impl_job_in_a_repo_with_no_tests_table_is_refused_tests_unconfigured` | attempt.rs | E2 |
| `an_impl_job_may_not_touch_the_files_of_the_test_job_it_needs_unless_the_lead_approved_the_path` | attempt.rs | E2 |
| `list_refuses_a_name_that_matches_no_test_for_an_impl_job_too_and_passes_when_every_name_exists` | attempt.rs | E2 |
| `only_an_impl_job_is_guarded_and_only_the_files_of_the_test_jobs_it_needs` | attempt.rs | E2 |
| `submit_refuses_a_quick_check_that_could_not_run` | attempt.rs | E2: a spawn error reply (`Failed::Io`) |
| `the_kind_checks_run_after_the_quick_check_and_a_work_job_runs_none` | attempt.rs | E2: order of recorded `Call`s |
| `archive_refused_by_any_one_blocker_changes_nothing` | context.rs | G: panes and the chat slot on the tmux route |
| `a_managers_pi_gets_no_plannotator_variable_from_its_callers` | context_manager.rs | G: assert the new-window `Launch`/command env (`env_remove`), not a dumped file |
| `a_stopped_manager_without_a_pane_is_started_again_on_its_file` | context_manager.rs | G |
| `a_dispatched_attempt_runs_its_worker_merges_and_ends_merged` | driver.rs | K |
| `a_dead_worker_pane_ends_crashed_through_the_ladder` | driver.rs | K (pane dead on the tmux route) |
| `a_driver_step_that_meets_a_held_write_lock_waits_and_the_attempt_is_not_crashed` | driver.rs | K (+ `Store::open_with`/lock waits `_with`, ports doc line 76) |
| `a_driver_waits_out_a_migration_longer_than_store_busy_ms` | driver.rs | K |
| `a_running_worker_is_watched_without_listing_panes_at_every_poll` | driver.rs | K: count `list-panes` calls on the route |
| `the_workers_window_is_killed_when_the_driver_sees_merging` | driver.rs | K |
| `driver_log_lines_start_with_a_time` | driver.rs | K + a wall clock (finding 4) |
| `job_merged_turns_when_the_plan_has_drafts_and_nothing_queued_or_running` | driver.rs | K |
| `a_merged_open_job_does_not_turn` | driver.rs | K |
| `an_attempt_of_a_pr_in_another_repo_is_cut_from_and_merged_into_that_prs_branch` | driver.rs | K, two repos |
| `an_open_job_with_no_conflict_merges_nothing_gates_in_a_slot_pushes_and_opens_the_pull_request` | driver.rs | K + `gh` reply + `push` to a modelled remote (`git-route:23`) |
| `an_open_job_with_a_conflicting_base_is_refused_base_conflict_until_the_worker_merges_the_base` | driver.rs | K + `merge-tree` conflict (`git-route:26`) |
| `an_open_jobs_checks_are_one_span_that_ends_in_the_transaction_that_makes_the_attempt_merging` | driver.rs | K |
| `an_open_jobs_gate_failure_is_gate_failed_and_pushes_nothing_creates_nothing_and_fails_the_attempt` | driver.rs | K + hook reply |
| `an_open_jobs_gh_failure_after_the_push_ends_the_attempt_crashed_with_gh_failed_and_the_pr_stays_dispatch` | driver.rs | K + failing `gh` reply |
| `an_open_job_adopts_a_pull_request_merged_on_github_with_no_gate_and_no_push` | driver.rs | K + `gh` reply |
| `an_update_job_with_a_conflicting_base_is_refused_until_the_worker_merges_it_and_its_merge_pushes_the_resolution` | driver.rs | K + conflict + push |
| `a_feedback_fix_job_on_an_open_pr_reaches_the_remote_through_the_update_job` | driver.rs | K + push |
| `a_driver_reloads_at_the_merging_safe_point_and_completes_the_merge` | driver_reload.rs | K + `Spawn::exec` recorded |

Count: ask 1, attempt 14, context 1, context_manager 2, driver 18, driver_reload 1 = 37.

## 4. Proposed migration PRs (4; order = fewest seams first)

The scope is 267 tests and the cap is 4 x 60 = 240: **PR1 exceeds it** (115 tests). PR1 is mechanical (same assertions,
`Fx::cpd` for `TestEnv::cpd`, ports tk-1 keeps the names), so one Worker can do it in 90 minutes only if `Fx` exists with those
names; otherwise the Lead splits it at ask+attempt+config+pending+digest (56) / context+curation+distill+driver (59) into a fifth PR.

| key | needs | files | tests | Jobs (kind: scope) |
|---|---|---|---|---|
| `mf-a1` | {testkit} (`Fx` over `cli::run`, env at start) | config (28), context (36), context_curation (14), attempt (8), ask (6), context_digest (6), context_pending (6), driver (5), context_distill (4), context_pending_waiting (2) | 115 | 1 test: config.rs + ask.rs (34). 2 test: context.rs, context_curation.rs (50). 3 test: context_pending*.rs, context_digest.rs, context_distill.rs (A rows), 4 test: attempt.rs read/show/list (8) + the 5 `dispatch_refuses_*` with an SQL-only fixture. 5 docs: drop migrated files from the ADR 0013 per-file table. Delete `a_guarded_child_is_gone_after_a_panic` and `KillOnDrop` in job 1 (section 5.1) |
| `mf-a2` | {runner, clk, run_with, testkit}; product: distill spawns through the Runner (finding 3) | config (16 tiers + 1 migration wait + 1 `config_does_not_wait` + 1 smoke), context_distill (27), context_digest (4), context (1), ask (2 + 1 deleted) | 54 | 1 impl: `distill.rs:434` and `:504` through `Runner` (port-run2: output cap in `wait_limited` moves after the reply, `unverified`). 2 test: config tiers on a scripted `node`, PATH/HOME set at start; migration waits on `ManualClock`. 3 test: context_distill.rs 27 (+2 with git in PR3). 4 test: ask blocking + restored `ask_times_out..`; digest `plan new`; context no-git. 5 docs |
| `mf-a3` | {git-route, tmux-route, runner, run_with} | attempt (22 + 14 deleted), distill (2), context archive (4 + 1), context_manager (4 + 2), by_user (2), digest (1) | 52 | 1 test: attempt submit E1 (10 + 2 restored). 2 test: attempt kind checks E2 (12 + 12 restored), distill `git grep` (2). 3 test: context archive on the tmux route (+1 restored). 4 test: context_manager x2 files (6 + 2 restored) asserting the new-window command. 5 test: digest manager test. 6 docs |
| `mf-a4` | all + `Spawn` (`launch`, `exec`, `hand_over`, `idle`, process facts) and a route that runs a launched `cpd attempt run` child through `run_with` (ports doc §4.2) | driver (20 + 18 deleted), driver_idle (2), driver_reload (4 + 1) | 45 | 1 test: `attempt run` fd-3 tests on `FakeSpawn.inherited` + `lock.rs` unit (L). 2 test: the 16 driver-loop tests (the 0.05 s `CPD_TEST_DRIVER_POLL`/`KILL_PAUSE` polls become `ManualClock.advance`). 3 test: restored open/update Job tests with `gh` and push replies. 4 test: stall x3 on process facts. 5 test: driver_idle + driver_reload (`FakeSpawn.exec`). 6 docs |

PR order is forced by seams: mf-a1 can open today; mf-a2 after `clk` and `port-run2`; mf-a3 after `port-git`, `port-tmux`; mf-a4
after `ports` (Spawn) and the driver route. mf-a4 is the riskiest: ports doc "decision 3" (route a launched `cpd` child to `run_with`
synchronously) must be settled before it starts.

## 5. Not migratable as asked (subject is the OS behaviour of an external)

1. **`ask.rs::a_guarded_child_is_gone_after_a_panic`**: tests the test helper `KillOnDrop` with a real `sleep 60` child. After
   migration no test spawns a child, so the helper and the test go together (policy [tests]: only authored code is tested; this is
   a deletion because the subject disappears, not a Gate failure). Lead confirms.
2. **`config.rs::tiers_with_a_model_unknown_to_real_pi_modules_is_tier_model_unknown`** and
   `tiers_reads_efforts_from_pi_with_pis_node`: run the real `node` on our embedded catalog script (`core/src/config/catalog.rs`,
   `node` symlink at 610 and 922). Proposal: the second becomes a scripted `node` reply (row C: `tiers_runs_the_catalog_module_from_argv`
   already covers the mapping); the first stays the one **S** smoke for config, with the real binary, a fake pi directory and the
   real `node`. The JS itself is `node` code: `unverified` whether a `node:test` file under `pi/test/` should carry it instead.
3. **`driver.rs::a_driver_without_the_lock_as_fd_3_exits_without_writing`**, `..refuses_an_attempt_that_is_not_starting`,
   `..ending_on_store_too_new..`, `..refusing_for_another_reason..`: use `sh -c 'exec 3<file; exec cpd attempt run'`, i.e. fd
   inheritance, an OS fact. `driver/lock.rs::inherited` (187-219) reads process fd 3. Proposal: `FakeSpawn` hands the driver an
   `Inherited` for a given tempdir `File` (or none, or another file); the fd-3 path comparison is a U test in `lock.rs`
   (`inherited` takes the fd number as a parameter, a one-line product change, `unverified`). The four stay I (set I).
4. **`driver_reload.rs::after_a_reload_fd_3_is_the_lock_and_the_drivers_children_do_not_hold_it`** (Linux `/proc/$$/fd`): OS
   behaviour of `exec` and close-on-exec. Proposal: U tests in `core/src/driver/lock.rs` with `fcntl(F_GETFD)` on a tempfile:
   `Inherited::keep_across_exec` (165) clears `FD_CLOEXEC`, `close_on_exec` (170) sets it, and `inherited` sets it on fd 3 (219) so
   children never hold the lock. `exec` itself is `Spawn::exec`.
5. **`driver_reload.rs` other tests**: "same pid, same argv" is the OS property of `exec`. In-process they assert that
   `FakeSpawn` recorded `exec(installed, [attempt run api-a1 --plan pay])`, that the lock `File` is the one handed over (not
   closed), that the cpd `--version` verdict (`store.opens`) was a scripted `Runner` reply, and that the Attempt still merges once
   the fake `exec` re-enters the driver. The time-claim `thread::sleep(POLL * 8)` becomes `ManualClock::advance` with a count of
   `--version` calls. How a fake `exec` re-enters (new thread, same lock) is open: `unverified`, decide in mf-a4 job 1.
6. **`driver_idle.rs`** (both tests, Linux only): assert the kernel's nice and I/O class of real processes. The authored code is
   `host/priority.rs::idle_prefix()` (`nice -n 19 ionice -c 3` on Linux, `nice -n 19` elsewhere; `priority.rs:6-15`), applied at
   `driver/start.rs:315` and to the Worker's command. Proposal: a U test of `idle_prefix()` per platform, an I test that the
   dispatched driver `Launch.idle` is true and its argv starts with the prefix, an I test that the Worker's new-window command starts with
   it. The `#![cfg(target_os = "linux")]` goes: the assertions are the same on each platform's prefix. Nothing then proves that
   the kernel applies it: that is `nice`, not our code.
7. **`context_manager*.rs`** assertions about pi's argv and environment (`pi_envs`, `pi_argv`: `--model`, `--session-id`, `-n`,
   `-- @<path>`, `CPD_SESSION`, `PLANNOTATOR_*`): they observe two layers at once (`context manager` builds the pane command;
   `cpd session run` starts pi). Proposal: here assert the `new-window` command and env on the tmux route (`lead_spawn`, `manager.rs:104`);
   the pi argv belongs to the session area (`mig-session-*`, `launch_pi`, `session/run.rs`). `unverified`: that `lead_spawn`
   carries the `--by-user` and first-message arguments, or `session run` derives them from the Session row.
8. **Stall tests** (`driver.rs`: `a_stalled_tool_call_has_its_processes_killed..`, `a_tool_call_with_its_result_is_not_a_stall`,
   `stall_seconds_0_kills_nothing`): walk the real process tree (`driver/stall.rs`, `libproc` on macOS, `/proc` on Linux) and send
   signals to a real `sleep`. Proposal: process facts (children of a pane pid, `cmdline`, `kill`) behind `Spawn` (ports doc class P);
   `FakeSpawn` holds a table of pids; the stall rule (idle time of the session file against `stall_seconds`, a pending tool call)
   is tested against it with `ManualClock`. Not covered then: the real `libproc`/`/proc` walk: one **S** smoke for the driver area
   at most (`unverified`: whether another PR already carries it).
9. **Real `flock`** in `a_second_run_of_a_context_is_refused_while_the_first_is_alive` and `dispatch_refuses_when_the_driver_lock_is_held`:
   the test opens the lock file itself and the product's `try_lock` conflicts in the same process (flock is per open file
   description), with no child, no sleep. Proposal: keep as is on a tempdir file. Lead decides whether a kernel lock on a tempfile
   counts as an "external" for policy [tests]; if so, the lock becomes a `Locker` port, a larger change not proposed here.
