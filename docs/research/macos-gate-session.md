# macOS Gate: `session` binary triage (nextest)

Research only; no code changed. Darwin arm64 (`pro`), tmux 3.8, `ZDOTDIR` set. Covered elsewhere: none. Logs: /tmp/gatemac-session-{A,B,B2-nozdotdir}.log (not in the repo).

## Summary
Run A (`TMPDIR` = default `/var/folders/7_/zb1y2vw95tl8x_r6s4p0yqn40000gq/T/gate-run.XXXXXX`): 167 tests, 85 pass, **82 fail**
(81 `FAIL`, 1 `SIGABRT`; no TIMEOUT, no LEAK). Test time 21 s (wall 2m43 with the build); load average 18.5 at start, 63 at end.

| Class | Tests | Cluster |
|---|---|---|
| path-length | 55 | C1 |
| test-assumption | 23 | C2 (21: `ZDOTDIR` reorders `PATH` in the panes), C3 (1: `/proc/self`), C4 (1: unwaited assertion) |
| load-flake | 3 | C5 |
| other | 1 | C5 (`history_refusals_change_nothing`: fails 1 of 3 alone) |
| macos-code | 0 | none found in the session binary's product code |
| timeout | 0 | (C2 tests wait the 30 s `WAIT` of `common::wait_until`, then panic; they are not nextest timeouts) |

Run B (short `TMPDIR=/tmp/gm.XXXXXX`, `binary_id(cpd::session)`, `ZDOTDIR` still set): 143 pass, 24 fail (1 leaky); 138.6 s; load 68 at start, 60 at end.
Run B2 (as B with `unset ZDOTDIR`, the control for C2): 161 pass, 6 fail (C3, C4, C5 x4); 103 s; load 89 at start, 124 at end.
Alone runs (short TMPDIR, `ZDOTDIR` unset), 3 each, load average (1 min) 91-117.
B-only: `session_asleep::list_shows_asleep_at` passed in A and B2 and 3 of 3 alone, but was a `LEAK-FAIL` in B (exit 0, "leaked handles", stderr `Error: in prepare, unable to open database file (14)`
from teardown.sh's `sqlite3` on a removed `cpd.db`, `common/mod.rs:157`): a load-flake of the teardown, not in A, so in no cluster.

## Clusters
### C1 path-length (55 tests, test-assumption of the harness, shared by every binary that uses `TestEnv`)
H1 confirmed. Every `tmux` call fails `error connecting to /private/var/folders/7_/.../T/gate-run.X/cpd-test-X/tmux/tmux-503/cpdt-57542-0 (File name too long)`
(A log). Length: `/private/var/folders/7_/zb1y2vw95tl8x_r6s4p0yqn40000gq/T/` 56 + `gate-run.XXXXXX/` 16 + `cpd-test-XXXXXX/` 16 + `tmux/tmux-503/` 14 + `cpdt-<pid>-<n>` 12-14
(with `GATE_RUN` `cpdt-<run>-<pid>-<n>` about 20) = 114-122 > 103 (104 bytes with the NUL; tmux resolves `/var` to `/private/var`, so the realpath counts).
Cause: `cli/tests/common/mod.rs:230` `envs_dir()` (the Gate's `TMPDIR`), `:632` `tempdir_in(envs_dir)` prefix `cpd-test-`, `:651` the `tmux` subdirectory, `:749` `.env("TMUX_TMPDIR", self.root().join("tmux"))`,
`:635` socket name; `cli/tests/session/host.rs:26` (`Tmux::with_tmpdir(.., env.root().join("tmux"))`) and `:48`; `teardown.sh` `:167-173` uses `$dir/tmux`.
Fix (1 Job, S2; the same Job fixes every area, so the Lead should give it to one fix Job of the PR): in `TestEnv::empty` keep the tmux dir short when
`<root>/tmux/tmux-<uid>/<socket>` would pass 100 characters: make `<root>/tmux` a symlink to a short dir `/tmp/cpdt-<pid>-<n>` (created by `empty`, removed by `teardown.sh` and `Drop`), pass the short path as `TMUX_TMPDIR`
(`common/mod.rs:749`, `host.rs:26`) and as `$tmux` to teardown.sh. Linux: the branch is not taken under `/dev/shm` or `/tmp` (the path is under 100), so nothing changes. Keep the leak check of `scripts/gate-run.sh` (it scans `/tmp/tmux-<uid>`; the short dir is removed by teardown).

### C2 test-assumption: `ZDOTDIR` reaches the pane's shell (21 tests)
Tests: every C2 row of `## Tests` (session_reload, session_watchdog, and `session::` quit/run tests that start a pane wrapper).
Symptom: `timed out waiting for the first pi` (`common/mod.rs:360`, `session_reload.rs:177`, `session_watchdog.rs:95`) after 30 s, even with a short `TMPDIR`. I read the pane while it waited
(`tmux capture-pane`): `Error: Session file is not a valid pi session: .../2026_lead1.jsonl  pi exited. Press Enter ...`: the REAL pi (`~/.pi/agent/bin/pi`, whose `session-manager.js:674` prints that) ran, not the fake `bin/pi`.
Cause: tmux runs a pane command with `$SHELL -c` (zsh here); zsh reads `$ZDOTDIR/.zshenv` even when non-interactive, and `ZDOTDIR=/Users/Clayton.Duarte/.config/zsh` is inherited by the test (`TestEnv::command` `common/mod.rs:736-751` overrides `HOME` but drops only `CPD_`, `PI_`, `GIT_`, `XDG_`, `PLANNOTATOR_`, `GH_`, `GITHUB_`, `SSH_`, proxy and `TMUX*`).
That file's `PATH` block (`~/.config/zsh/.zshenv:65-103`, I read it only in part: unverified which line puts `~/.pi/agent/bin` first) puts the real pi before the env's `bin/`. Proof by effect: `unset ZDOTDIR` makes all 21 pass in run B2.
Fix (1 Job, S1): add `"ZDOTDIR"` to the dropped variables of `TestEnv::command` (`common/mod.rs:736`) (and in `Tmux` env of host tests if they start panes). Linux: the variable is unset in CI; nothing changes. `macos-code`: none, the product has no fault.
The Gate must run where `ZDOTDIR` is set: the Lead's shell has it (unverified for other Workers).

### C3 test-assumption: `/proc/self` (1 test, fails alone 3 of 3)
`host::a_tmux_call_whose_tmux_tmpdir_is_missing_is_refused_and_starts_no_server_in_tmp`: `host.rs:48:121` `std::fs::metadata("/proc/self").unwrap().uid()` (ENOENT) panics inside `DefaultDirServer::drop` (`host.rs:44-56`), a panic in a destructor: SIGABRT.
Fix (1 Job with C4, S1): the uid with `nix::unistd::getuid()` (nix is already a test dependency, `session_reload.rs`), `/tmp/tmux-<uid>` unchanged. The macOS default dir of tmux is `/tmp` too (`_PATH_TMP`), so the test's meaning holds (unverified; the test body past the drop was not reached).
Latent, same file: `host.rs:587` `running()` reads `/proc/<pid>/stat` (no caller found by grep); `common/mod.rs:388` `wait_for_flock_waiter` reads `/proc/locks` (no session test calls it); `TEARDOWN_SH` `alive`/`mine` (`common/mod.rs:123-128`) and its `exec setsid` (`:198`) are Linux-only,
so on a Mac the kill-safety reaper does nothing (a killed test leaves its tmux server and panes; I had to kill one `cpd session run` by hand). Not a failing test here.

### C4 test-assumption: no wait (1 test, fails alone 3 of 3)
`session::fork_registers_a_stopped_lead_in_a_parked_window`: `session.rs:534` asserts at once that the fake `cpd` recorded `session run <new>`, but the parked pane starts `cpd` after `fork` returned. Slower shell start on a Mac: with a 3 s sleep inserted before the assert (local experiment, reverted) the test passes.
Fix (S1, same Job as C3): `wait_until("the pane's cpd session run", || env.calls("cpd").contains(&..))` instead of `assert!`. Linux unchanged (the wait returns at once when it is already true).

### C5 load-flake: tmux prints a partial list (4 tests)
`session_retier::a_resumed_lead_runs_on_the_rows_current_tier`, `session::close_kills_a_parked_pane_and_writes_what_e2_writes`, `session::e2_replaces_a_dead_lead_in_the_chat_slot_by_the_placeholder`, `session::history_refusals_change_nothing` (alone: 3/3, 3/3, 3/3 pass; the last 2 of 3).
Fail only in the full run at load 90-120. `common/mod.rs:604` shows `tmux_failed: tmux list-panes printed unexpected output` with `"stdout":"%0\t@0\tcpd\tmain\t0\t"` (a line cut mid-field, exit 0), and `session.rs:184` `index out of bounds: the len is 5` in the test's own `panes()` (a raw `tmux list-panes` cut the same way).
So tmux 3.8's client returned truncated stdout through a pipe under load; cpd (`core/src/host/tmux.rs`, the `list-panes` parser) and the test helper then read the cut line. Waits on: nothing (no time or poll limit): a race in tmux's output flush. Cause of the cut: unverified (I infer a client exit before the flush).
Fix (1 Job, S2, optional): in the `list-panes`/`list-sessions` callers retry once when stdout is non-empty and does not end in `\n` (a complete output always does), and the same in `panes()`/`panes_report` of the tests. Linux: only a malformed read is retried, a good one is read once. If the Lead does not want a retry, accept it as a load-flake of this Mac: the Gate would fail 1 in a few runs.
A fix Job should first reproduce it with `timeout 600 scripts/cargo.sh nextest run -p cpd --test session -E 'binary_id(cpd::session)'` at load over 60.

## Tests
`F` = fail. Columns B / B2 are run B and run B2 (see Summary). `Alone` = 3 runs, short `TMPDIR`, `ZDOTDIR` unset.

| Test (`cpd::session`) | Class | Cluster | A | B / B2 | Alone |
|---|---|---|---|---|---|
| host::a_dead_pane_stays_and_is_respawned | path-length | C1 | F | pass / pass | - |
| host::a_failing_call_is_tmux_failed_with_argv_exit_and_stderr | path-length | C1 | F | pass / pass | - |
| host::a_killed_view_pane_respawns_and_a_killed_lead_or_engine_pane_stays_dead | path-length | C1 | F | pass / pass | - |
| host::a_leads_border_shows_its_title_reversed_between_caps_and_a_titleless_lead_shows_nothing | path-length | C1 | F | pass / pass | - |
| host::a_pane_with_an_unknown_view_is_skipped_not_a_failed_listing | path-length | C1 | F | pass / pass | - |
| host::a_right_click_is_tmuxs_default_binding_that_passes_it_to_a_pane_that_captures_the_mouse | path-length | C1 | F | pass / pass | - |
| host::a_server_started_by_cpd_reads_the_popup_and_menu_styles_back_as_default | path-length | C1 | F | pass / pass | - |
| host::a_tmux_call_whose_tmux_tmpdir_is_missing_is_refused_and_starts_no_server_in_tmp | test-assumption | C3 | F | fail / fail | F F F |
| host::a_worker_window_gets_its_cwd_environment_and_argv | path-length | C1 | F | pass / pass | - |
| host::builds_the_cockpit_and_finds_its_panes_by_option | path-length | C1 | F | pass / pass | - |
| host::create_session_returns_the_first_window_and_pane_once | path-length | C1 | F | pass / pass | - |
| host::list_panes_is_empty_only_when_tmux_says_no_server_runs | path-length | C1 | F | pass / pass | - |
| host::list_panes_parses_for_a_caller_without_a_utf8_locale | path-length | C1 | F | pass / pass | - |
| host::respawn_killing_restarts_a_live_pane_in_place | path-length | C1 | F | pass / pass | - |
| host::set_pane_title_runs_set_p_on_cpd_title_and_a_missing_pane_fails_with_its_argv | path-length | C1 | F | pass / pass | - |
| host::starts_the_server_once_with_the_contract_options | path-length | C1 | F | pass / pass | - |
| session::a_fork_with_a_message_file_over_128_kib_starts_pi_with_the_copied_file | path-length | C1 | F | pass / pass | - |
| session::a_fresh_lead_with_a_first_message_over_128_kib_starts_pi_with_its_file | path-length | C1 | F | pass / pass | - |
| session::a_pi_that_exits_with_status_1_is_not_archived | test-assumption | C2 | F | fail / pass | - |
| session::a_refused_archive_prints_the_refusal_and_offers_the_resume_in_place | path-length | C1 | F | pass / pass | - |
| session::a_reload_is_not_a_quit_even_though_pi_exits_0_on_sigterm | test-assumption | C2 | F | fail / pass | - |
| session::archive_kills_the_pane_of_an_idle_parked_fork_and_appends_one_event | path-length | C1 | F | pass / pass | - |
| session::archive_of_a_lead_with_no_pane_closes_nothing | path-length | C1 | F | pass / pass | - |
| session::archive_refuses_a_working_waiting_or_blocked_session_and_changes_nothing | path-length | C1 | F | pass / pass | - |
| session::archive_refuses_the_chat_slot_a_plan_lead_a_worker_and_an_unknown_session | path-length | C1 | F | pass / pass | - |
| session::close_kills_a_parked_pane_and_writes_what_e2_writes | load-flake | C5 | F | pass / fail | P P P |
| session::close_of_a_stopped_lead_with_a_pane_appends_only_session_moved | path-length | C1 | F | pass / pass | - |
| session::close_refusals_and_nothing_to_close | path-length | C1 | F | pass / pass | - |
| session::close_refuses_the_lead_in_the_chat_slot_and_leaves_everything | path-length | C1 | F | pass / pass | - |
| session::e2_kills_the_window_of_a_dead_parked_lead | path-length | C1 | F | pass / pass | - |
| session::e2_replaces_a_dead_lead_in_the_chat_slot_by_the_placeholder | load-flake | C5 | F | pass / fail | P P P |
| session::every_session_command_accepts_the_short_forms | path-length | C1 | F | pass / pass | - |
| session::fork_and_open_leave_no_window_when_a_step_after_new_window_fails | path-length | C1 | F | pass / pass | - |
| session::fork_records_the_pane_before_it_gets_the_go_signal | path-length | C1 | F | pass / pass | - |
| session::fork_registers_a_stopped_lead_in_a_parked_window | test-assumption | C4 | F | fail / fail | F F F |
| session::fork_stops_waiting_for_a_stopped_source_without_a_file_and_retries_on_enter | path-length | C1 | F | pass / pass | - |
| session::fork_waits_until_the_source_is_idle_then_forks_and_resumes | path-length | C1 | F | pass / pass | - |
| session::history_parks_the_lead_in_the_slot_and_open_kills_the_history_pane | path-length | C1 | F | pass / pass | - |
| session::history_refusals_change_nothing | other | C5 | F | pass / fail | P F P |
| session::history_replaces_the_placeholder_in_place_and_park_brings_it_back | path-length | C1 | F | pass / pass | - |
| session::open_and_park_keep_at_most_one_placeholder_only_in_the_slot | path-length | C1 | F | pass / pass | - |
| session::open_and_park_refusals | path-length | C1 | F | pass / pass | - |
| session::open_and_park_take_a_context_s_manager_and_park_it_in_its_context_window | path-length | C1 | F | pass / pass | - |
| session::open_gives_a_new_pane_the_plans_title_for_the_plans_lead_and_the_name_for_a_fork | path-length | C1 | F | pass / pass | - |
| session::open_leaves_one_window_for_a_lead_with_a_dead_pane | path-length | C1 | F | pass / pass | - |
| session::open_records_the_pane_before_it_gets_the_go_signal | path-length | C1 | F | pass / pass | - |
| session::open_replaces_a_dead_lead_in_the_chat_slot | path-length | C1 | F | pass / pass | - |
| session::open_resumes_a_titled_lead_with_its_dynamic_title_and_a_rename_keeps_it | path-length | C1 | F | pass / pass | - |
| session::open_writes_session_moved_before_the_rename_so_a_failing_rename_keeps_the_swap_recorded | path-length | C1 | F | pass / pass | - |
| session::quitting_a_plan_s_own_lead_archives_the_plan_with_its_fork | test-assumption | C2 | F | fail / pass | - |
| session::quitting_pi_archives_a_side_fork_and_closes_its_pane | test-assumption | C2 | F | fail / pass | - |
| session::quitting_the_lead_in_the_chat_slot_parks_it_first_and_the_placeholder_holds_the_slot | path-length | C1 | F | pass / pass | - |
| session::rename_appends_session_renamed_once | path-length | C1 | F | pass / pass | - |
| session::rename_sets_the_title_of_a_forks_live_pane_to_its_name | path-length | C1 | F | pass / pass | - |
| session::rename_works_on_an_archived_session | path-length | C1 | F | pass / pass | - |
| session::run_gives_each_plan_its_port_and_keeps_it_across_a_restart | test-assumption | C2 | F | fail / pass | - |
| session::run_gives_the_lead_remote_and_the_review_host | test-assumption | C2 | F | fail / pass | - |
| session::run_refusals | path-length | C1 | F | pass / pass | - |
| session::run_with_the_range_full_says_no_port_and_waits_for_enter | test-assumption | C2 | F | fail / pass | - |
| session::unarchive_clears_archived_at_with_one_event_and_touches_no_pane | path-length | C1 | F | pass / pass | - |
| session_asleep::archive_clears_asleep_at | path-length | C1 | F | pass / pass | - |
| session_asleep::open_clears_asleep_at | path-length | C1 | F | pass / pass | - |
| session_first_message::first_message_of_a_lead_holds_header_brief_and_records_written_after_registration | path-length | C1 | F | pass / pass | - |
| session_first_message::first_message_of_a_manager_holds_header_and_context | path-length | C1 | F | pass / pass | - |
| session_reload::a_failing_store_read_is_said_once_and_retried_with_pi_untouched | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_lead_wrapper_does_not_reload_into_a_cpd_older_than_the_store_and_reloads_into_one_that_can_open_it | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_lead_wrapper_follows_the_real_verdict_of_the_new_binary | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_lead_wrapper_whose_pi_exits_does_not_exec_a_cpd_older_than_the_store | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_newer_store_does_not_end_a_wrapper_and_the_new_cpd_that_opens_it_adopts_pi | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_parked_idle_lead_restarts_its_pi_on_its_session_file_after_pi_changes | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_pi_that_changed_before_the_install_is_restarted_at_idle_by_the_new_wrapper | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_pi_that_ignores_sigterm_is_killed_after_pi_stop_seconds_and_the_wrapper_starts_pi_again_for_a_new_pi | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_working_lead_survives_an_install_the_wrapper_re_execs_and_pi_is_never_signalled | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_wrapper_whose_first_write_meets_a_held_write_lock_waits_and_starts_pi_when_it_is_released | test-assumption | C2 | F | fail / pass | - |
| session_reload::a_wrapper_whose_session_does_not_exist_still_ends_with_that_error | path-length | C1 | F | pass / pass | - |
| session_reload::an_install_re_execs_an_idle_wrapper_at_once_keeping_pi_and_a_new_pi_then_restarts_it_at_idle | test-assumption | C2 | F | fail / pass | - |
| session_retier::a_fresh_lead_started_after_a_retier_runs_on_the_new_tier | path-length | C1 | F | pass / pass | - |
| session_retier::a_resumed_lead_runs_on_the_rows_current_tier | load-flake | C5 | F | pass / fail | P P P |
| session_titles::the_title_is_set_as_cpd_title_on_the_sessions_live_pane | path-length | C1 | F | pass / pass | - |
| session_watchdog::a_pi_alive_with_its_state_stopped_for_sixty_seconds_is_restarted_with_one_error_event | test-assumption | C2 | F | fail / pass | - |
| session_watchdog::a_pi_that_exits_is_handled_as_before_with_no_error_event | test-assumption | C2 | F | fail / pass | - |
| session_watchdog::a_pi_whose_adapter_reports_idle_within_the_limit_is_left_alone | test-assumption | C2 | F | fail / pass | - |
