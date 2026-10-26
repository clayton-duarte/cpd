# What one Gate's nextest run costs: CPU per test and the 99% tui test

## Verdict

- **No full run was made** (Lead's answer to `gc-cost-a1-q1`: a Worker may not run the suite; steps 1-3 and the whole-run totals were replaced). The Gate's wall, user, sys, peak processes and peak RSS sum are **not measured**; every figure below is of single named tests run alone, 3 repetitions, median.
- 60 named tests (the 52 heaviest by calmest logged wall that still exist, plus the 8 heaviest of `cpd-tui`), all exit 0: **27.4 CPU-s in total (8.7 user, 18.7 sys; sys is 68%)**, mean 0.46 CPU-s per test; sys > user in 47 of 60 tests (verified, table below).
- The most CPU one test takes alone is **2.06 s** (`cpd-core::migrations the_frozen_check_still_fails_on_an_edit_...`: 4 real git clone/commit/script rounds); next the `cpd::flow gate::*` / `cpd::pr pr_open::` tests at 1.0-1.5 s (real git + `cpd` child processes). Only 1 test reaches 2 s. **No spinner**: none of the 60 has user+sys >= 90% of its wall for 1 s or more (the highest ratio is 0.87, 0.54 s).
- Start and exit of one debug test binary (300 execs per binary, aggregate): 4.8-6.6 ms user+sys, 4-8 MB RSS (verified, base table). Estimate (**inference, not a Gate measurement**): 5,167 x 4.8-6.6 ms = **25-34 CPU-s** for process start and exit (9-18 CPU-s if the 3.1 ms of the shell loop's own fork+exec of `/usr/bin/true` is subtracted). Test bodies of the other ~5,100 tests were not measured, so the share of start-and-exit in the Gate is **not known**.
- **The 99% cpd-tui test (unverified inference)**: the only measured `cpd-tui` test that is CPU-dense is `icons_tests::the_cockpit_sources_and_tests_spell_none_of_the_old_glyphs`: 0.47 s CPU in 0.54 s wall (87%), 6.8 G instructions, user 0.44 s, `tui/src/icons_tests.rs:104-141`. A process that lives 0.5-1 s under load and spends it computing shows ~99% in `ps`. Only 8 of 1,018 `cpd-tui` tests were measured (running all is forbidden), so another one may be it. Saving by fixing it: ~0.4 CPU-s per Gate (about 1% of the 25-34 CPU-s start/exit estimate).
- H1 killed earlier (rs-src). H3c ("the OS delays exec/exit"): **open, consistent, not proven**: sys time is 68% of the sampled CPU and a trivial test costs 4.8-6.6 ms of user+sys for an exec that does nothing, 1.5-2x a `/usr/bin/true` loop iteration; I could not see the scanners' CPU (no root). H3a, H2, H3b: not tested here.
- Gates fit: not measurable without a full run. Lower bound for one Gate: start/exit 25-34 CPU-s + the 60 sampled tests 27 CPU-s = **>= ~55 CPU-s** (about 4 s of 14 cores). A calm Gate takes ~180 s wall (Plan logs), so the Gate is mostly waiting, not computing (inference): the heavy cost is process and file-system churn (sys time), not test CPU.

## Evidence

Machine: mac-pro, 14 cores, 48 GB (`sysctl hw.memsize` = 51539607552), cargo-nextest 0.9.148, commit 467f9503. `uptime`: 23:32 load 85.6/75.3/67.5 before the build; 23:36 load 48 when measuring began; 23:49 load 56.7. Measurements ran at that load (CPU-seconds do not depend on it; real seconds do).

**Build** (`timeout 1500 scripts/cargo.sh build --workspace --tests`, sccache on, target dir had a warm cache): wall 74 s (`build_wall=74`). Not `nextest run --no-run`: the Worker adapter refuses `cargo.sh nextest run --workspace`.

**Candidates**: from the 8 Gate logs in /tmp/cpd-leak-flake/leak-logs (PASS/FAIL lines with wall time), ranked by the *smallest* wall of a test over the logs (the calmest sample), keeping tests that still exist in `nextest list --workspace` of this commit (the logs are of older bases: 5,158 of 5,842 names exist; renamed heavy tests such as `watch::an_engine_killed_with_its_tmux_server...` and the 150 s install test are lost, a limit of this sample). 52 of those + 8 `cpd-tui`. Wall under load overstates CPU, so the logged wall is only a selector.

**How measured**: each test binary run directly (`<bin> --exact <name> --nocapture`) from its package dir with `NEXTEST=1 NEXTEST_BINARY_ID NEXTEST_TEST_NAME CARGO_MANIFEST_DIR`, the tripwire first on PATH, an own TMPDIR, under `/usr/bin/time -l` (rusage includes waited children). 3 repetitions, median, 0 failures, each confirmed `1 passed`. `user`/`sys` have 10 ms resolution; instructions retired are exact.

### Top 30 by user+sys (median of 3)

| binary | test | real s | user s | sys s | RSS MB | Minstr |
|---|---|---|---|---|---|---|
| cpd-core::migrations | `the_frozen_check_still_fails_on_an_edit_a_delete_or_an_additive_rename` | 7.46 | 0.53 | 1.53 | 6 | 546 |
| cpd::flow | `gate::the_gate_runs_while_the_attempt_is_merging_yellow` | 11.07 | 0.48 | 1.06 | 24 | 801 |
| cpd::pr | `pr_open::the_open_jobs_merge_runs_the_pr_gate` | 10.34 | 0.50 | 1.03 | 23 | 817 |
| cpd::flow | `gate::the_merge_gate_runs_the_pre_commit_at_a_job_merge` | 6.07 | 0.47 | 1.02 | 24 | 744 |
| cpd::flow | `gate::the_pr_gate_runs_the_pre_merge_at_an_open_job_merge` | 6.55 | 0.41 | 0.85 | 42 | 705 |
| cpd::flow | `gate::the_pr_gate_failure_fails_the_open_job_with_gate_failed` | 4.14 | 0.35 | 0.72 | 24 | 699 |
| cpd::flow | `secret_guard::names_are_validated_before_anything_else` | 4.90 | 0.52 | 0.54 | 20 | 729 |
| cpd::flow | `gate::the_merge_gate_failure_fails_the_job_with_gate_failed` | 4.85 | 0.33 | 0.72 | 24 | 693 |
| cpd-core::repin | `rerunning_changes_nothing` | 3.08 | 0.23 | 0.66 | 7 | 203 |
| cpd::flow | `gate::the_submit_runs_no_quick_check_and_no_gate` | 6.96 | 0.27 | 0.61 | 21 | 506 |
| cpd::flow | `gate::a_gate_that_cannot_run_fails_the_job_with_gate_error` | 1.97 | 0.31 | 0.57 | 23 | 676 |
| cpd-core::migrations | `migrations::the_frozen_check_passes_on_a_header_only_correction_of_a_breaking_migration` | 5.68 | 0.20 | 0.53 | 6 | 162 |
| cpd::tooling | `harness::concurrent_fake_calls_are_recorded_whole` | 3.63 | 0.20 | 0.49 | 13 | 330 |
| cpd-core::migrations | `migrations::the_frozen_check_passes_on_a_breaking_restore_to_an_older_name` | 7.92 | 0.18 | 0.48 | 6 | 150 |
| cpd::tooling | `preview_script::on_sets_the_preview_option_to_the_worktree_and_sha_after_the_views_are_respawne` | 9.02 | 0.15 | 0.50 | 11 | 177 |
| cpd::tooling | `preview_script::off_respawns_the_five_views_with_the_installed_cpd_runs_tui_and_removes_the_sta` | 20.67 | 0.14 | 0.51 | 11 | 170 |
| cpd-core::migrations | `migrations::the_frozen_check_passes_on_a_renumber_of_a_migration_the_pr_adds` | 11.06 | 0.17 | 0.47 | 6 | 170 |
| cpd::engine | `doctor::each_tool_must_be_at_or_above_its_minimum` | 9.94 | 0.19 | 0.41 | 24 | 2977 |
| cpd-core::migrations | `the_frozen_check_fails_on_a_rename_that_changes_the_body` | 8.26 | 0.15 | 0.43 | 6 | 156 |
| cpd::tui | `tui_host_click::a_new_incident_is_announced_again` | 11.28 | 0.24 | 0.32 | 22 | 256 |
| cpd-core::migrations | `migrations::the_frozen_check_refuses_a_renumber_of_a_migration_on_the_base` | 4.58 | 0.14 | 0.39 | 6 | 151 |
| cpd::tooling | `preview_script::off_unsets_the_preview_option_and_twice_is_the_same` | 4.85 | 0.12 | 0.40 | 11 | 157 |
| cpd::tooling | `preview_script::off_twice_succeeds` | 4.95 | 0.11 | 0.37 | 11 | 157 |
| cpd-tui | `icons_tests::the_cockpit_sources_and_tests_spell_none_of_the_old_glyphs` | 0.54 | 0.44 | 0.03 | 10 | 6779 |
| cpd::tooling | `preview_script::on_builds_into_the_preview_dir_only` | 5.52 | 0.09 | 0.31 | 11 | 154 |
| cpd::tooling | `preview_script::on_respawns_the_five_views_with_the_preview_and_leaves_lead_and_worker_alone` | 3.27 | 0.09 | 0.28 | 11 | 153 |
| cpd::engine | `watch::watch_holds_the_lock_refuses_a_second_and_stops_on_sigterm` | 1.07 | 0.13 | 0.23 | 20 | 499 |
| cpd::tooling | `install_script::a_bare_tmux_in_a_script_is_refused_and_tmux_u_is_not` | 0.87 | 0.10 | 0.25 | 9 | 107 |
| cpd::tooling | `version::the_install_script_builds_with_the_checkout_s_short_commit` | 1.49 | 0.07 | 0.26 | 11 | 163 |
| cpd::tooling | `harness::calls_come_back_in_the_order_they_started` | 2.84 | 0.07 | 0.25 | 11 | 169 |

### Top 15 by sys

| binary | test | real s | user s | sys s | RSS MB | Minstr |
|---|---|---|---|---|---|---|
| cpd-core::migrations | `the_frozen_check_still_fails_on_an_edit_a_delete_or_an_additive_rename` | 7.46 | 0.53 | 1.53 | 6 | 546 |
| cpd::flow | `gate::the_gate_runs_while_the_attempt_is_merging_yellow` | 11.07 | 0.48 | 1.06 | 24 | 801 |
| cpd::pr | `pr_open::the_open_jobs_merge_runs_the_pr_gate` | 10.34 | 0.50 | 1.03 | 23 | 817 |
| cpd::flow | `gate::the_merge_gate_runs_the_pre_commit_at_a_job_merge` | 6.07 | 0.47 | 1.02 | 24 | 744 |
| cpd::flow | `gate::the_pr_gate_runs_the_pre_merge_at_an_open_job_merge` | 6.55 | 0.41 | 0.85 | 42 | 705 |
| cpd::flow | `gate::the_pr_gate_failure_fails_the_open_job_with_gate_failed` | 4.14 | 0.35 | 0.72 | 24 | 699 |
| cpd::flow | `gate::the_merge_gate_failure_fails_the_job_with_gate_failed` | 4.85 | 0.33 | 0.72 | 24 | 693 |
| cpd-core::repin | `rerunning_changes_nothing` | 3.08 | 0.23 | 0.66 | 7 | 203 |
| cpd::flow | `gate::the_submit_runs_no_quick_check_and_no_gate` | 6.96 | 0.27 | 0.61 | 21 | 506 |
| cpd::flow | `gate::a_gate_that_cannot_run_fails_the_job_with_gate_error` | 1.97 | 0.31 | 0.57 | 23 | 676 |
| cpd::flow | `secret_guard::names_are_validated_before_anything_else` | 4.90 | 0.52 | 0.54 | 20 | 729 |
| cpd-core::migrations | `migrations::the_frozen_check_passes_on_a_header_only_correction_of_a_breaking_migration` | 5.68 | 0.20 | 0.53 | 6 | 162 |
| cpd::tooling | `preview_script::off_respawns_the_five_views_with_the_installed_cpd_runs_tui_and_removes_the_sta` | 20.67 | 0.14 | 0.51 | 11 | 170 |
| cpd::tooling | `preview_script::on_sets_the_preview_option_to_the_worktree_and_sha_after_the_views_are_respawne` | 9.02 | 0.15 | 0.50 | 11 | 177 |
| cpd::tooling | `harness::concurrent_fake_calls_are_recorded_whole` | 3.63 | 0.20 | 0.49 | 13 | 330 |

### Remaining 30 measured tests (by user+sys)

| binary | test | real s | user s | sys s | RSS MB | Minstr |
|---|---|---|---|---|---|---|
| cpd::tui | `tui_host_click::a_click_opens_the_host_manager_and_sends_the_incident_once` | 1.26 | 0.12 | 0.18 | 22 | 244 |
| cpd::tooling | `preview_script::no_cockpit_exits_1_for_on_after_building_and_for_off` | 2.35 | 0.06 | 0.23 | 11 | 155 |
| cpd::tooling | `preview_script::a_refused_on_and_a_missing_cockpit_leave_the_preview_option_alone` | 7.99 | 0.06 | 0.22 | 11 | 148 |
| cpd::tooling | `preview_script::on_starts_the_engine_on_the_installed_cpd_before_the_previews_tui` | 2.28 | 0.07 | 0.20 | 11 | 146 |
| cpd::tui | `tui_incident_click::a_click_opens_the_incident_lead` | 1.36 | 0.10 | 0.16 | 22 | 240 |
| cpd::flow | `context_engine::the_bridge_answers_ping_stats_reset_index_and_search` | 0.62 | 0.13 | 0.12 | 23 | 77 |
| cpd::tooling | `preview_script::on_refuses_a_preview_that_cannot_read_the_store_and_changes_nothing_in_tmux` | 3.51 | 0.05 | 0.18 | 11 | 149 |
| cpd::engine | `watch::e3_spawn_failures_requeue_at_the_same_tier_never_fail_the_job_and_back_off` | 0.23 | 0.14 | 0.05 | 25 | 2386 |
| cpd::tooling | `tripwire_script::the_prefix_matches_the_start_of_the_test_name` | 4.19 | 0.04 | 0.13 | 9 | 134 |
| cpd::tooling | `tripwire_script::an_allowed_pair_runs_the_tmux_behind_it_with_the_same_argv` | 2.92 | 0.04 | 0.13 | 9 | 131 |
| cpd::flow | `context_search::out_of_force_records_are_hidden_unless_all` | 0.97 | 0.08 | 0.09 | 25 | 1187 |
| cpd::tui | `tui_incident_click::a_click_with_no_lead_reports_on_the_client` | 0.60 | 0.05 | 0.10 | 19 | 239 |
| cpd::tooling | `tripwire_script::the_tripwires_own_directory_is_left_off_the_path_before_the_lookup` | 1.65 | 0.03 | 0.12 | 9 | 131 |
| cpd::tooling | `tripwire_script::an_allowed_call_with_no_tmux_behind_the_tripwire_exits_127` | 1.96 | 0.03 | 0.12 | 9 | 129 |
| cpd::tui | `tui_incident_click::a_click_with_no_incident_reports_on_the_client` | 0.66 | 0.05 | 0.09 | 19 | 239 |
| cpd::tooling | `tripwire_script::a_pair_not_in_the_list_exits_127_and_is_logged_without_running_tmux` | 1.25 | 0.03 | 0.11 | 9 | 130 |
| cpd::tooling | `tripwire_script::comment_and_blank_lines_of_the_list_are_ignored` | 4.48 | 0.03 | 0.10 | 9 | 129 |
| cpd::flow | `context_search::the_over_fetch_retries_once_and_hidden_hits_do_not_count` | 0.21 | 0.05 | 0.05 | 25 | 801 |
| cpd-tui | `worker::phi_bold_tests::the_worker_foot_draws_a_bold_phi` | 0.11 | 0.09 | 0.00 | 18 | 1755 |
| cpd::tui | `tui::an_unfaked_gh_is_denied_with_exit_97` | 1.11 | 0.02 | 0.06 | 11 | 89 |
| cpd::session | `session_watchdog::a_pi_whose_adapter_reports_idle_within_the_limit_is_left_alone` | 0.07 | 0.03 | 0.02 | 25 | 562 |
| cpd::engine | `doctor::a_history_row_without_a_txn_fails_tamper_only` | 0.08 | 0.02 | 0.02 | 24 | 414 |
| cpd::engine | `doctor::a_changed_schema_fails_store_and_tamper` | 0.09 | 0.01 | 0.02 | 23 | 350 |
| cpd::engine | `doctor::a_database_that_is_not_cpds_fails_store` | 0.05 | 0.00 | 0.01 | 19 | 123 |
| cpd-tui | `widgets::legend::tests::the_legends_session_dots_list_the_cyan_dot_and_the_precedence` | 0.01 | 0.00 | 0.00 | 10 | 40 |
| cpd-tui | `read::dot::tests::a_cyan_item_gives_its_session_a_cyan_dot_between_magenta_and_white` | 0.01 | 0.00 | 0.00 | 9 | 40 |
| cpd-tui | `pump_scripted_tests::the_production_pump_is_not_inline` | 0.01 | 0.00 | 0.00 | 9 | 39 |
| cpd-tui | `pump_scripted_tests::an_inline_background_job_is_finished_when_submit_returns_and_the_next_starts_after_it_is_taken` | 0.01 | 0.00 | 0.00 | 9 | 40 |
| cpd-tui | `pump_scripted_tests::a_scripted_pump_with_nothing_left_and_no_time_to_wait_for_says_the_script_did_not_end_the_loop` | 0.01 | 0.00 | 0.00 | 9 | 39 |
| cpd-tui | `phi::tests::only_the_phi_glyph_is_bold` | 0.01 | 0.00 | 0.00 | 9 | 39 |

### Per binary (17 binaries; only the measured tests are summed)

| binary | tests in binary | measured | user s | sys s | user+sys s |
|---|---|---|---|---|---|
| cpd | 8 | 0 | 0.00 | 0.00 | 0.00 |
| cpd::engine | 162 | 6 | 0.49 | 0.74 | 1.23 |
| cpd::flow | 629 | 11 | 3.40 | 6.35 | 9.75 |
| cpd::pr | 365 | 1 | 0.50 | 1.03 | 1.53 |
| cpd::session | 155 | 1 | 0.03 | 0.02 | 0.05 |
| cpd::store | 111 | 0 | 0.00 | 0.00 | 0.00 |
| cpd::tooling | 248 | 20 | 1.58 | 5.16 | 6.74 |
| cpd::tui | 300 | 6 | 0.58 | 0.91 | 1.49 |
| cpd::bin/cpd | 0 | 0 | 0.00 | 0.00 | 0.00 |
| cpd::bin/fake-tmux | 0 | 0 | 0.00 | 0.00 | 0.00 |
| cpd-core | 1976 | 0 | 0.00 | 0.00 | 0.00 |
| cpd-core::hooks_idle | 1 | 0 | 0.00 | 0.00 | 0.00 |
| cpd-core::migrations | 24 | 6 | 1.37 | 3.83 | 5.20 |
| cpd-core::repin | 4 | 1 | 0.23 | 0.66 | 0.89 |
| cpd-core::tmux_formats | 2 | 0 | 0.00 | 0.00 | 0.00 |
| cpd-engine | 164 | 0 | 0.00 | 0.00 | 0.00 |
| cpd-tui | 1018 | 8 | 0.53 | 0.03 | 0.56 |

Binaries `cpd::bin/cpd`, `cpd::bin/fake-tmux` have 0 tests. Total in the list: 5167 tests.

### Baseline: one exec of a debug test binary, one trivial test

`base.sh`: 300 execs of one named test in a `/bin/sh` loop inside ONE `/usr/bin/time -l` (user/sys have 10 ms resolution so one exec is below it; aggregate instead). The `cpd::flow`/`cpd::pr` rows are the cheapest tests of those binaries, which run the `cpd` binary or a store (not pure).

| binary | test | real s (300) | user s | sys s | (user+sys)/exec ms | max RSS MB |
|---|---|---|---|---|---|---|
| cpd-core | host::tmux::tests::ids_have_their_sigil | 14.10 | 0.52 | 0.91 | 4.8 | 4.6 |
| cpd-tui | widgets::needs_tests::a_clean_pr_whose_merge_runs_needs_the_frame_clock | 8.36 | 0.86 | 1.13 | 6.6 | 8.5 |
| cpd::flow | spec::the_spec_bounds_the_worker_s_commands | 11.06 | 0.88 | 1.04 | 6.4 | 12.2 |
| cpd::pr | git::a_malformed_hooks_table_is_policy_invalid | 4.93 | 0.79 | 1.06 | 6.2 | 10.3 |
| control: `/usr/bin/true` x300 in the same loop | | 10.96 | 0.25 | 0.69 | 3.1 | |

Per exec about 1.6-1.8 M instructions and 14-20 M cycles (rusage `instructions retired`/`cycles elapsed`, /usr/bin/time of 300 execs: 472-539 M instructions) and ~50 involuntary context switches. Real 0.01-0.05 s per exec: the loop's real time varies 5-14 s with the load. Start-and-exit estimate (inference): 5,167 x 4.8-6.6 ms = 25-34 CPU-s.

### Spinners and sys > user

- Spinners (user+sys >= 90% of wall, wall >= 1 s): **none** of the 60 (highest ratio: icons_tests 0.87 at 0.54 s; `watch::e3_spawn_failures...` 0.83 at 0.23 s; `phi_bold_tests` 0.82 at 0.11 s).
- sys > user: 47 of 60; the script-and-git tests (`cpd::tooling preview_script::*`, `tripwire_script::*`, `cpd::flow gate::*`, `cpd-core::migrations`) are 2-4x sys over user. user > sys: the 8 `cpd-tui` tests and `cpd::engine doctor::each_tool...`/`e3_spawn_failures` (pure compute: 2.4-3.0 G instructions).

### What burns the CPU (tests >= 2 s, or CPU-dense)

1. `cpd-core::migrations the_frozen_check_still_fails_on_an_edit_a_delete_or_an_additive_rename`: 2.06 s (0.53 user, 1.53 sys), `core/tests/migrations.rs:596-616`. Four cases, each `Clone::new()` (`:494-506`: `git init --bare`, `git clone`, add, commit, push, checkout = 7 real `git` processes), `commit()` (2 more) and `check()` (`:515-521`: runs `scripts/check-migrations-frozen.sh`, which runs git again). About 40 process starts at 5-6 ms each plus git's own work. Avoidable: a template clone copied per case (`cp -R`) instead of init/clone/push, or one `git` fixture per test run shared by the 4 cases. The 5 sibling `migrations::the_frozen_check_*` tests (same `Clone`) cost 3.1 CPU-s more: ~8.3 CPU-s of the sample are this fixture.
2. `cpd::flow gate::*` and `cpd::pr pr_open::the_open_jobs_merge_runs_the_pr_gate`: 0.9-1.5 s each, 9 tests, 10.3 CPU-s together (`cli/tests/flow/gate.rs:84,238-266`: `fixture()` makes a real git repo and worktrees, then each test runs several `cpd` child processes (`submit`, `merge`, hook scripts); one of them waits for a paused hook). Sys 2x user = process creation. Avoidable only by sharing the fixture (the git template at `cli/tests/common/mod.rs:1402 git_repo_template` already exists for the repo).
3. `cpd-core::repin rerunning_changes_nothing`: 0.89 s CPU in 3.1 s wall (0.23 user, 0.66 sys): runs the repin script twice with real git/curl-less tools (script not read; unverified cause).
4. `cpd::flow secret_guard::names_are_validated_before_anything_else`: 1.06 s, 729 M instructions (not read; unverified cause).
5. **cpd-tui** `icons_tests::the_cockpit_sources_and_tests_spell_none_of_the_old_glyphs`: 0.47 s, 6.8 G instructions. `tui/src/icons_tests.rs:150-157` walks every file of `tui/src` and `cli/tests` (107,197 lines, 5.0 MB), and for each line calls `spells` 8 times (`OLD` has 8 code points, `:16`); `spells` (`:105-116`) allocates `line.to_ascii_uppercase()` and a `format!` per call and scans it. About 860 k string allocations and 8 x 5 MB of scans. Avoidable: uppercase the line once (or skip lines with no hex digit run) and test `contains` for all 8 chars in one pass: roughly 5-10x less, about 0.4 CPU-s saved.
6. `cpd::engine doctor::each_tool_must_be_at_or_above_its_minimum`: 2.98 G instructions, 0.60 s (0.19 user, 0.41 sys, 396 voluntary switches): runs a real tool probe per tool (not read in detail; unverified).
7. `cpd::engine watch::e3_spawn_failures_requeue_at_the_same_tier...` : 0.19 s CPU in 0.23 s wall, 2.4 G instructions (`cli/tests/engine/watch.rs:1563-1603`): pure compute, 3 back-off rounds of `watch_first_tick` over an in-process sqlite store; no busy loop (the clock is a `ManualClock`, `:1569-1573`).

No test of the sample shows a busy loop, `yield_now`, thread explosion or large terminal buffer. Not read: the 150 s install test and the renamed heavy tests (not in this commit).

## Reproduction

Build, list, then measure each candidate (one at a time, 3 reps) and the baselines. Everything lives in a `mktemp -d` outside the repo. None runs the suite.

```sh
scripts/cargo.sh build --workspace --tests            # compile only
scripts/cargo.sh nextest list --workspace --list-type binaries-only --message-format json > $D/bins.json
scripts/cargo.sh nextest list --workspace --message-format json > $D/list.json
# cands.tsv: <binary_id> TAB <test name>; one.sh and runall.sh below; base.sh for the baseline
CPD_COMMIT=$(git rev-parse --short HEAD) $D/runall.sh "$PWD" $D/cands.tsv $D/out.tsv
$D/base.sh "$PWD" $D/bins.json cpd-core host::tmux::tests::ids_have_their_sigil 300
```

one.sh (columns of out.tsv: binary, test, rep, exit, real, user, sys, maxrss, reclaims, faults, vol cs, invol cs, instructions, cycles):

```sh
#!/bin/sh
# usage: one.sh <worktree> <bins.json> <outfile> <binary_id> <test> [reps]
# Runs ONE named test of a built test binary, as nextest would (same env, cwd = package dir), under /usr/bin/time -l.
wt=$1 bins=$2 out=$3 bid=$4 name=$5 reps=${6:-1}
bin=$(python3 -c "import json,sys;print(json.load(open('$bins'))['rust-binaries']['$bid']['binary-path'])")
pk=$(python3 -c "import json;print(json.load(open('$bins'))['rust-binaries']['$bid']['package-id'].split('#')[0].split('/')[-1])")
case $pk in cpd.*|cpd-*) pk=${pk}; esac
case $bid in cpd-core*) dir=core;; cpd-engine*) dir=engine;; cpd-tui*) dir=tui;; *) dir=cli;; esac
i=0
while [ $i -lt $reps ]; do
  i=$((i+1))
  t=$(mktemp -d "${GCC_TMP:-/tmp}/run.XXXXXX")
  r=$t/rusage
  ( cd "$wt/$dir" && TMPDIR=$t NEXTEST=1 NEXTEST_BINARY_ID=$bid NEXTEST_TEST_NAME=$name CARGO_MANIFEST_DIR=$wt/$dir \
    CPD_COMMIT=${CPD_COMMIT:-x} NODE_DISABLE_COMPILE_CACHE=1 PATH="$wt/scripts/tripwire:$PATH" \
    /usr/bin/time -l -o "$r" "$bin" --exact "$name" --nocapture >"$t/stdout" 2>"$t/stderr" ); st=$?
  ran=$(grep -c ' 1 passed' "$t/stdout"); [ "$ran" = 1 ] || { echo "NOT RUN $bid $name" >&2; st=99; }
  awk -v b="$bid" -v n="$name" -v st=$st -v rep=$i '
    /real/ {real=$1; user=$3; sys=$5}
    /maximum resident/ {rss=$1} /page reclaims/ {rc=$1} /page faults/ {pf=$1} /^ *[0-9]+ +voluntary/ {vc=$1} /involuntary/ {ic=$1}
    /instructions retired/ {ins=$1} /cycles elapsed/ {cyc=$1}
    END {printf "%s\t%s\t%d\t%d\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", b,n,rep,st,real,user,sys,rss,rc,pf,vc,ic,ins,cyc}' "$r" >> "$out"
  rm -rf "$t"
done
```

runall.sh:

```sh
#!/bin/sh
# usage: runall.sh <worktree> <cands.tsv> <out.tsv>  (3 reps each, 900 s limit per test run)
wt=$1; export GCC_TMP=$(mktemp -d /tmp/gcc/tmp.XXXXXX)
while IFS="$(printf '\t')" read -r b t; do
  timeout 900 /tmp/gcc/one.sh "$wt" /tmp/gcc/bins.json "$3" "$b" "$t" 3
done < "$2"
rm -rf "$GCC_TMP"; echo done >> /tmp/gcc/progress
```

base.sh:

```sh
#!/bin/sh
# usage: base.sh <worktree> <bins.json> <binary_id> <test> <n>: N execs of one named test inside ONE /usr/bin/time -l (user/sys resolution is 10 ms, so aggregate)
wt=$1; bins=$2; bid=$3; name=$4; n=$5
bin=$(python3 -c "import json;print(json.load(open('$bins'))['rust-binaries']['$bid']['binary-path'])")
case $bid in cpd-core*) dir=core;; cpd-engine*) dir=engine;; cpd-tui*) dir=tui;; *) dir=cli;; esac
t=$(mktemp -d /tmp/gcc/tb.XXXXXX)
cd "$wt/$dir" && TMPDIR=$t NEXTEST=1 NEXTEST_BINARY_ID=$bid NEXTEST_TEST_NAME=$name CARGO_MANIFEST_DIR=$wt/$dir \
 /usr/bin/time -l -o $t/r /bin/sh -c 'i=0; while [ $i -lt '$n' ]; do i=$((i+1)); "$0" --exact "$1" --nocapture >/dev/null 2>&1; done' "$bin" "$name"
printf '%s n=%s ' "$bid" "$n"; awk '/real/{printf "real %s user %s sys %s ",$1,$3,$5} /maximum res/{printf "maxrss %.1fMB ",$1/1e6} /instructions/{printf "ins %.0fM ",$1/1e6} /cycles/{printf "cyc %.0fM ",$1/1e6} /voluntary context/{printf "vcsw %s ",$1} END{print ""}' $t/r
rm -rf $t
```

For the whole-run totals (not done here), the Gate's own Job may run, against a fixed main: `/usr/bin/time -l` around `scripts/cargo.sh nextest run --workspace`, with a `[scripts.wrapper.<n>]` and `[[profile.default.scripts]] filter = 'all()' run-wrapper` in a `--config-file` outside the repo whose script is `exec /usr/bin/time -l -o "$DIR/$$.rusage" "$@"` (unverified: not run here).

## What this means for the fix

- The sample does not support "the tests burn the machine": the heaviest test alone takes 2 CPU-s, 59 of 60 under 1.6 CPU-s, no spinner. What the sample does show is **sys-heavy, process-heavy work**: 68% of its CPU is system time and a do-nothing test binary exec costs 5-7 ms user+sys. That matches the Manager's 51% sys and is consistent with per-exec cost under endpoint scanning (H3c), but I could not see the scanners; unproven.
- Biggest saving I can state from numbers: sharing the `git` fixture of `core/tests/migrations.rs` (7 tests, ~8.3 CPU-s of the sample) and of `cpd::flow gate::` (9 tests, ~10 CPU-s): up to ~15 CPU-s per Gate. Cost: test-code change; risk: shared state between tests (each is a process of its own, so a template copied per test is safe).
- Fixing the icons scan saves ~0.4 CPU-s; it is the only plausible 99% `cpd-tui` test I found. It does not change the Gate's load.
- Start-and-exit (25-34 CPU-s, estimate) is the one cost every test pays; merging tests into fewer binaries is not an option under nextest's process-per-test model.
- To answer the User exactly (Gate CPU, peak processes, peak RSS, Gates per 14 cores) one real run under `/usr/bin/time -l` is needed, which this Job was not allowed to make; the Plan's Gate itself (the PR's open Job) could record `/usr/bin/time -l` of its nextest step at no extra cost.
