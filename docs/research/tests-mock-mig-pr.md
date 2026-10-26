# tests-mock: migrating the `pr` tests in-process (mr-pr-a1)

Docs only. Everything below was read with `rg` and `read` in this worktree (base 885fa97a) and in `~/.cpd/ref/tests-mock/*`; no test and no build ran. `unverified` / `guess:` mark what was inferred. Names of `Runner`, `FakeRunner`, `Ports`, `run_with`, `testkit`, `Clock` come from the ref documents (not on main). Counts are `#[test]` per file; per-test classes come from reading each file's helpers and test bodies (a script grouped tests by what their body calls; I read the five biggest files in full).

**Conflict in the Spec's numbers (read first).** "At most 4 PRs" with "15 to 60 tests each" holds at most 240 tests. The scope is 320 current tests (317 in `cli/tests/pr/` + the 3 of `core/src/git/gate.rs`) plus 94 deleted tests to restore = 414; 6 current + 5 deleted cannot migrate (§5) and 1 current is a helper, leaving 313 + 89 = 402 to rewrite. ADR 0013 itself plans `mig-pr-1..3` at ~105 tests each (`0013-tests-in-process.md`, the Job table). §4 therefore proposes 4 PRs of 39 / 102 / 106 / 155 tests cut at seam boundaries, and names the file-level split points that give 7 PRs of at most 60 (§4, last paragraph). The Lead picks; I did not choose silently.

**Legend.** Seams: `C` clk, `R` runner (FakeRunner with scripted replies for any program: `sh` hooks, `gh`, `cpd`, `pi`, and scripted git failures), `T` tmux-route, `S` spawn, `W` run_with (the CLI entry in-process), `K` testkit (fixtures, `Repo` handle, `Fx`). `git-route` is split by the Jobs of `tests-mock-git-route.md` §6 (`unverified`, not yet built): `G1` = Jobs 1-2 (model + `Repo` handle + read-only subcommands), `G3` = `git-worktree` (update-ref, branch, worktree add/list/remove, write-tree/read-tree), `G4` = `git-remote` (fetch, push, ls-remote), `G5` = `git-merge` (`merge-tree`), `G6` = `git-rebase`, `G7` = `git-fixture` (argv-style `commit`/`add`/`clone`... used by `env.git(..)`). **Assumption:** the migrated fixtures build repos with the Job 1 `Repo` handle, not with argv, so no row needs `G7`; a test that keeps `env.git(dir, ["commit", ..])` needs `G7` (which itself needs G3, G4, G5). Each set is the union over its group; one test may need less. Every row also implies the seams that its seam needs (`G*` ⇒ `R`; `W` ⇒ `R`, `C`, `S` as `Ports` fields).

## 1. Inventory

Today every `cli/tests/pr/*` test goes through `TestEnv` (`cli/tests/common/mod.rs`: `TestEnv::new`/`empty` start a reaper shell script, write a `teardown.sh`, copy a store template, write `DENIED_TOOLS` stubs; `command()` sets `PATH` so `fake_gh`/`fake`/`fake_pi`/`fake_tmux` shell scripts win) and `env.cpd(..)` execs `CARGO_BIN_EXE_cpd`.

| path | tests | what they start today | target |
|---|---|---|---|
| cli/tests/pr/main.rs | 0 | `mod` list only (19 lines) | stays |
| cli/tests/pr/git.rs | 51 | real git on `env.git_repo` repos (`lead`/`attempt`/`tamper`/`rule S`/gate worktree), `sh -c` hooks (`hooks()` fixture), `sleep` + a spawned `git` lock holder (243, 906), `ps -o stat` (`git.rs:496`), the test binary re-exec'd (`hook_env_helper`, `git.rs:542`), a `true` child (`dead_pid`, 899) | I (library calls on `Ports`, no `W`): 49; §5: 1 (`a_background_child_of_a_finished_hook_is_killed_when_the_hook_returns`); delete: 1 (`hook_env_helper`: a helper, not a test; the env test asserts `Call.env` of the fake) |
| cli/tests/pr/git_guard.rs | 6 | the authored git wrapper script under real `sh`; 1 test runs the real `git` behind it | U: 2 (`writing_it_twice...`, `the_harness_takes...`: write files, run nothing); S: 2 of the other 4 (see §5); §5: the rest |
| cli/tests/pr/merge.rs | 6 | real git through `TestEnv`, `cpd_core::merge::{merge, take_free_driver_lock}` called as a library, real rebase | I (library call, no `W`) |
| cli/tests/pr/pr.rs | 103 | `env.cpd(..)`, real git, `fake_gh` scripts, `sh` gate hooks, `paused_gate` scripts polled with `wait_until`, a spawned `cpd pr push/rebase` child (7 `paused` tests), `Flock` on slot files (`pr.rs:3080 hold_slot`), a `true` child for a dead pid (`pr.rs:781`) | I (with `W`) |
| cli/tests/pr/pr_cleanup.rs | 14 | `cpd pr poll`, real git, gh answers in files, `true` child (`pr_cleanup.rs:594 dead_pid`), a `pre-merge` of `sleep 0.1` x600 (`:626`) | I (`W`) |
| cli/tests/pr/pr_cleanup_cancel.rs | 3 | same helpers as `pr_cleanup.rs` | I (`W`) |
| cli/tests/pr/pr_crud.rs | 67 | `cpd pr add/edit/cancel/...`, real git (`keyed_repo_with_tests`, `legacy()` with a bare origin), `fake_gh` in 1 | I (`W`) |
| cli/tests/pr/pr_local_needs.rs | 5 | `cpd pr add/edit`, real git `keyed_repo` | I (`W`) |
| cli/tests/pr/pr_merge.rs | 14 | `cpd pr merge` + `poll`, a `fake_gh` that records argv and keeps state in a flag file (`gh_read_back`), no git | I (`W`); S: 1 (`a_bad_argument_is_usage_and_gh_is_not_called`) |
| cli/tests/pr/pr_push.rs | 11 | `cpd pr push`, real git with a bare origin, `sh` hooks, `paused_gate` + `wait_until` (389, 417) | I (`W`) |
| cli/tests/pr/pr_start.rs | 25 | `cpd plan lock`/`pr start`, real git, a fake `cpd` for `watch --ensure`, `fake_gh`; 5 tests spawn a real `cpd watch` (`Fx::engine`, `:223`) and poll (`wait_until`, `wait_whole_ticks`); 1 runs 8 threads | I (`W`); the 5 engine tests also need `C`, `T` |
| cli/tests/pr/pr_view.rs | 6 | `cpd pr view`, `fake_gh`, no git | I (`W`) |
| cli/tests/pr/repo_hooks.rs | 6 | `cpd plan lock`, `attempt submit`, `pr push` with `sh` hooks, real git; the Worker Spec test (`:329`) also starts a fake tmux, a fake pi, a detached driver and `wait_some` polls | I (`W`): 5; U: 1 (`the_worker_spec_lists_the_pre_commit_commands_of_the_repo_file`: call `driver::worker::render_spec` (`core/src/driver/worker.rs:136`) on a store; no driver, tmux or pi; `unverified` that its inputs build without them) |
| core/src/git/gate.rs (in-crate) | 3 | tempdir + `Flock` on slot files; `with_every_slot_busy...` uses a thread and a wall-clock `wait_for` (`:70`); `a_slot_held_by_a_dead_process...` spawns `sleep` (`:125`) | U: 1 (`a_free_slot_is_taken_at_once...`); I: 1 (needs `C`); §5: 1 |
| other in-crate tests of `core/src/{git,pr,merge}` | 59 | only `tempfile` stores or pure functions: `pr/pulls.rs` 12, `pr/gh.rs` 22, `pr/status.rs` 6, `pr/feedback.rs` 3, `git/hooks/def.rs` 5, `git/hooks/template.rs` 2, `pr/open.rs` 2, `pr/merge.rs` 2, `pr/cancel.rs` 1, `git/hooks/exec.rs` 1, `git/run.rs` 3 (pure matchers: `is_admin_dir_race...`, `worktree_add_retries_the_reproduced_shapes_only`, `fetch_retries_only_the_placeholder_head`) | already U, **outside the 320**; the `cpd-core git::`/`pr::`/`merge::` lines of `scripts/tripwire/allow.txt` (111, 114, 116) can go once `gate.rs` is migrated (`unverified`: that no other in-crate test of those modules needs them) |

S picks (3 for the area): `pr_merge::a_bad_argument_is_usage_and_gh_is_not_called` (real exit code 2 and JSON error; rewrite with no `gh` on `PATH`), and two of `git_guard.rs` (§5). They stay counted in their rows below.

## 2. Seams

Needs-set of every group of tests (the file-level sets of §1, with the files split where their tests differ):

| # | needs-set | files (tests) | tests |
|---|---|---|---|
| 1 | `{}` (U) | git_guard 2 (`writing_it_twice`, `the_harness_takes`), gate.rs `a_free_slot_is_taken_at_once...` 1, repo_hooks Worker Spec 1 | 4 |
| 2 | `{C}` | gate.rs `with_every_slot_busy_a_gate_waits_logs_it_and_takes_the_slot_that_frees` | 1 |
| 3 | `{R,K}` | git.rs hooks: `no_file_and_no_table...`, `a_passing_hook...`, `a_failing_hook...`, `only_the_last_60_lines...`, `a_table_runs_together...`, `commands_of_a_table_run_concurrently`, `a_pipeline_runs_its_blocks...`, `output_of_concurrent_commands...`, `a_malformed_hooks_table...`, `a_wt_toml_in_the_tree_is_not_read`, `a_command_killed_by_a_signal...`, `cockpit_pi_and_cpd_variables_do_not_reach_hooks`, `the_commands_of_a_hook_can_be_listed_in_file_order` | 13 |
| 4 | `{K,G1}` | git.rs tamper: `deleting_a_protected_path_is_refused` .. `base_ref_is_origin_slash_base_only_with_an_origin` (608-727) | 10 |
| 5 | `{R,K,G1,G3}` | git.rs `the_command_runs_with_sh_in_the_worktree_root`, `commands_get_the_variables...`, `target_worktree_path_is_undefined...`, `a_failing_pre_start_refuses_the_gate...`, `a_failing_pre_merge_refuses_the_gate...` | 5 |
| 6 | `{K,G1,G3}` | git.rs rule S x3 (775, 794, 807) | 3 |
| 7 | `{S,K,G1,G3}` | git.rs `the_sweep_removes_the_prs_gate_worktrees_of_dead_pids_only` | 1 |
| 8 | `{C,R,K,G1,G3}` | git.rs 243, 906, 991, 1015, 1036, 1064 (retries of `git/run.rs` with scripted replies, as `tests-mock-git-route.md` line 8 says) | 6 |
| 9 | `{K,G1,G3,G4}` | git.rs lead/attempt/rule P without hooks: 85, 136, 153, 208, 273, 292 | 6 |
| 10 | `{R,K,G1,G3,G4}` | git.rs lead/attempt with a `pre-start`: 62, 108, 125, 168, 192 | 5 |
| 11 | `{K,G1,G3,G4,G6}` | merge.rs (6); `G5` too if the merge check uses `merge-tree` (`unverified`) | 6 |
| 12 | `{W,R,K}` | pr_view 6, pr_merge 14 | 20 |
| 13 | `{W,K,G1,G3}` | pr_local_needs 5 (`pr add` imports `git::worktree::list`, `core/src/pr/add.rs:18`), pr_crud 62, pr.rs `pr_add_*` 3 (2831, 2842, 2859) | 70 |
| 14 | `{W,R,K,G1,G3,G4}` | pr_crud legacy 5 (`add_moves_the_legacy...`, `add_completes_a_legacy_move...`, `add_refuses_push_failed...`, `add_keeps_refusing...`, `edit_title_of_a_merged_or_cancelled_pr_is_pr_ended`), pr_cleanup 12, pr_cleanup_cancel 3, repo_hooks 5, pr_push 11, pr.rs poll group 33 (1298-2172 + `poll_covers_every_open_pr...`) | 69 |
| 15 | `{W,S,R,K,G1,G3,G4}` | pr_cleanup `a_gate_worktree_whose_pid_is_dead...`, `a_dead_gate_worktree_of_a_pr_cleaned_up_earlier...` | 2 |
| 16 | `{W,R,K,G1,G3,G4,G5}` | pr_start 20 (not the engine ones), pr.rs open group 39 (452-1021 and the 8 `open_*` of 2658-3055, minus the spawn test) | 59 |
| 17 | `{W,S,R,K,G1,G3,G4,G5}` | pr.rs 776-795 (dead gate pid at `open`) | 1 |
| 18 | `{W,R,K,G1,G3,G4,G5,G6}` | pr.rs rebase group: 21 `rebase_*` + `rebase_runs_r_first...` + `rebase_touches_only_its_pr` | 23 |
| 19 | `{W,C,R,K,G1,G3,G4,G5,G6}` | pr.rs `rebase_waits_for_a_live_driver_to_record_its_merge` (waits on the driver lock) | 1 |
| 20 | `{W,C,R,K,G1,G3,G4,G5}` | pr.rs slot tests `with_one_slot...`, `a_gate_that_finds_a_free_slot...`, `with_two_slots...` (3092-3134; `git/gate.rs` slot waits on real time) | 3 |
| 21 | `{W,C,T,R,K,G1,G3,G4,G5}` | pr_start engine tests: `e7_starts_a_dependent_pr...`, `a_failed_start_marks_the_pr...`, `e7_holds_a_pr_for_a_need_of_another_plan...`, `e7_starts_a_stacked_pr_in_the_tick_...`, `e7_unstacks_a_waiting_pr...` (`T`: `unverified` that the tick touches tmux; the env sets `[tmux] socket` and E2 lists panes) | 5 |
| 22 | not a seam: §5 | git_guard 4, git.rs `a_background_child...` 1, gate.rs `a_slot_held_by_a_dead_process...` 1 | 6 |
| 23 | not a test | git.rs `hook_env_helper` (helper run as a test by `git.rs:542`) | 1 |
| | **sum** | | **320** |

Check: 4+1+13+10+5+3+1+6+6+5+6+20+70+69+2+59+1+23+1+3+5+6+1 = 320 (317 `cli/tests/pr` + 3 `gate.rs`).

Needs `run_with` (rows 12-21): 253 tests. Library call only (rows 1-11, 22, 23): 67 tests. Nothing needs `T` except row 21; nothing needs the `G7` argv fixtures under the §1 assumption.

Restored (deleted) tests add (§3): +19 to row 14, +16 to row 16, +1 `{W,C,R,K,G1,G3,G4}`, +41 `{K,G1,G3,G4,G6}` (merge.rs), and 12 git.rs ones with the sets in §3; 5 are §5.

## 3. The 94 deleted tests of the scope (`tests-mock-deleted.md`)

All return to the file of the same name under `cli/tests/pr/` (ADR 0013 lines 116, 135: same module, same test name where behaviour is the same). "pr-poll" = `{W,R,K,G1,G3,G4}`, "pr-open" = `{W,R,K,G1,G3,G4,G5}` (G6 for the one named `..._rebase_...`), "merge" = `{K,G1,G3,G4,G6}`.

| test (file) | seam |
|---|---|
| git.rs `a_fetch_outlasts_a_worktree_add_in_progress` | `{C,R,K,G1,G3,G4}`: scripted `bad object worktrees/` replies, virtual clock |
| git.rs `a_git_call_past_its_deadline_is_killed_with_its_group_and_is_tool_timeout` | §5 (the kill); the `tool_timeout` mapping is `{R}` |
| git.rs `a_hook_is_killed_with_its_process_group_at_the_timeout` | §5 (the kill); the timeout mapping is `{R}` |
| git.rs `a_lead_worktree_from_origin_has_no_upstream_and_its_push_writes_no_config` | `{K,G1,G3,G4}` |
| git.rs `a_merged_attempt_is_removed_with_branch_d_and_an_unmerged_one_is_refused` | `{K,G1,G3}` |
| git.rs `a_protected_path_the_attempt_has_as_the_base_has_it_is_allowed` | `{K,G1}` |
| git.rs `an_undefined_variable_or_worktrunk_filter_fails_before_anything_runs` | `{R,K}` (asserts the fake saw no call) |
| git.rs `each_protected_path_is_refused` | `{K,G1}` |
| git.rs `rule_p_a_rejection_whose_remote_tip_lacks_the_commit_stands` | `{K,G1,G3,G4}` |
| git.rs `rule_p_an_ancestor_pushed_after_its_descendant_is_a_success_that_pushed_nothing` | `{K,G1,G3,G4}` |
| git.rs `rule_s_two_merges_synced_in_the_reverse_order_end_at_the_tip_and_clean` | `{K,G1,G3}` |
| git.rs `rule_s_two_syncs_at_once_both_end_with_the_lead_worktree_at_the_tip` | `{K,G1,G3}` (two threads on the route's mutex) |
| git.rs `rule_s_waits_out_an_index_lock_held_for_one_second` | `{C,R,K,G1,G3}` (scripted lock failure + virtual clock) |
| git.rs `the_gate_runs_pre_start_then_pre_merge_in_a_detached_worktree_at_g_and_removes_it` | `{R,K,G1,G3}` |
| git_guard.rs `it_ends_at_the_real_git_when_another_wrapper_is_on_path_too` | §5 |
| git_guard.rs `it_finds_the_real_git_when_its_own_directory_is_on_path_twice` | §5 |
| merge.rs `a_failed_push_of_an_update_job_ends_the_attempt_crashed_with_push_failed_after_the_fast_forward` | merge (scripted push rejection) |
| merge.rs `a_job_of_a_pr_that_is_not_stacked_gets_no_extra_base_for_a_protected_file_only_another_branch_has` | merge |
| merge.rs `a_merge_leaves_another_prs_orphan_in_its_repo_to_r` | merge |
| merge.rs `a_merge_pushes_the_lead_branch_to_origin` | merge |
| merge.rs `a_protected_file_deleted_on_the_base_and_in_the_attempt_is_merged` | merge |
| merge.rs `a_push_that_origin_already_contains_is_no_push_failed` | merge |
| merge.rs `a_rejected_push_leaves_the_job_merged_with_one_push_failed_error` | merge |
| merge.rs `a_rejected_push_of_one_prs_branch_is_that_attempts_push_failed_only` | merge |
| merge.rs `a_repo_without_origin_does_not_push_and_appends_no_error` | merge |
| merge.rs `a_stacked_prs_job_that_changes_a_protected_file_to_anything_else_is_tampered` | merge |
| merge.rs `a_stacked_prs_job_that_merges_the_base_prs_protected_file_changes_is_merged` | merge |
| merge.rs `a_stacked_prs_job_that_merges_the_root_base_brings_its_protected_file_changes_without_tampering` | merge |
| merge.rs `a_staged_edit_in_the_lead_worktree_is_kept_and_reported_stale` | merge |
| merge.rs `an_attempt_that_already_contains_the_lead_tip_lands_as_is_with_its_merge_commit` | merge |
| merge.rs `an_attempt_that_changes_a_protected_file_to_anything_but_the_bases_is_tampered` | merge |
| merge.rs `an_attempt_that_merges_a_base_with_changed_protected_files_is_merged` | merge |
| merge.rs `an_impl_jobs_diff_that_changes_a_guarded_red_test_ends_tampered_unless_the_lead_approved_the_path` | merge |
| merge.rs `an_untracked_file_in_the_lead_worktree_does_not_stop_the_merge` | merge |
| merge.rs `an_update_jobs_merge_pushes_what_it_landed` | merge |
| merge.rs `cas_lost_when_the_lead_branch_keeps_moving_outside_cpd` | merge + `{R}` (scripted `update-ref` CAS miss) |
| merge.rs `each_merge_pushes_its_own_prs_branch_to_its_own_origin` | merge |
| merge.rs `in_one_repo_a_merge_pushes_its_prs_branch_and_not_the_others` | merge |
| merge.rs `in_one_repo_each_merge_lands_on_its_own_prs_branch_only` | merge |
| merge.rs `in_two_repos_each_merge_lands_on_its_own_prs_branch_only` | merge |
| merge.rs `merge_conflict_aborts_the_rebase_and_goes_up_the_ladder` | merge |
| merge.rs `merge_conflict_with_the_cap_reached_fails_the_job` | merge |
| merge.rs `merged_rebases_fast_forwards_syncs_the_lead_worktree_and_records_it` | merge |
| merge.rs `merging_an_attempt_that_is_not_merging_is_refused` | `{K}` (store only; no git call expected) |
| merge.rs `r_after_step_8_ends_merged_at_the_tip_and_finishes_the_stale_lead_worktree` | merge |
| merge.rs `r_of_one_repo_leaves_the_orphans_of_another_repo` | merge |
| merge.rs `r_on_a_git_failure_changes_nothing_and_reports_merge_check_failed_once` | merge + `{R}` (scripted git failure) |
| merge.rs `r_pushes_after_it_records_a_merge` | merge |
| merge.rs `r_recovers_the_orphans_of_its_repo_per_pr_and_pushes_each_prs_branch` | merge |
| merge.rs `r_removes_the_driver_lock_of_the_attempt_it_ended` | merge |
| merge.rs `rule_s_finishes_a_dead_drivers_stale_lead_worktree_and_the_merge_leaves_its_attempt_to_r` | merge |
| merge.rs `step_9_failure_keeps_the_merge_and_reports_a_stale_lead_worktree` | merge + `{R}` (scripted failure) |
| merge.rs `tampered_also_by_the_protected_paths_of_the_prs_policy` | merge |
| merge.rs `tampered_when_the_diff_touches_a_protected_path` | merge |
| merge.rs `the_cas_and_the_dirty_check_are_those_of_the_prs_own_branch_and_worktree` | merge |
| merge.rs `the_tamper_list_is_that_of_the_prs_own_policy` | merge |
| merge.rs `two_merges_of_one_pr_at_once_both_land_and_the_lead_worktree_ends_at_the_tip` | merge (two threads on the route's mutex) |
| pr.rs `a_lookup_with_a_not_found_alias_and_exit_1_still_applies_the_others` | pr-poll |
| pr.rs `a_merged_pr_makes_its_dependent_startable_but_poll_does_not_start_it` | pr-poll |
| pr.rs `a_poll_reply_without_the_review_request_search_is_gh_failed_and_writes_nothing` | pr-poll |
| pr.rs `a_pr_opened_by_cpd_is_authored_and_a_row_a_pr_points_to_stays_while_in_no_search` | pr-poll |
| pr.rs `a_pr_whose_owner_file_is_gone_is_reported_and_the_other_plans_pr_still_merges` | pr-poll |
| pr.rs `a_pull_request_closed_without_a_merge_is_not_looked_up_again` | pr-poll |
| pr.rs `a_pull_request_github_cannot_find_loses_its_flags_and_is_not_read_again` | pr-poll |
| pr.rs `a_pull_request_in_both_searches_is_authored_and_review_requested_and_each_flag_clears_only_when_a_read_says_so` | pr-poll |
| pr.rs `a_pull_request_one_search_misses_keeps_its_row_and_the_open_list` | pr-poll |
| pr.rs `a_reopened_plan_whose_new_pr_merges_is_done_again_with_a_second_plan_done` | pr-poll |
| pr.rs `a_review_request_is_stored_as_not_authored_and_kept_until_a_read_says_it_is_gone` | pr-poll |
| pr.rs `a_search_that_lists_fewer_nodes_than_its_issue_count_deletes_and_clears_nothing` | pr-poll |
| pr.rs `a_second_identical_poll_writes_nothing` | pr-poll |
| pr.rs `a_second_poll_waits_for_the_lock` | pr-poll + `C` (the poll lock wait on a virtual clock) |
| pr.rs `a_slot_held_by_a_killed_gate_is_free_at_once` | §5 |
| pr.rs `a_value_outside_the_mergeable_sets_fails_the_poll_and_stores_nothing` | pr-poll |
| pr.rs `an_owned_pull_request_that_left_the_search_and_is_closed_is_told_to_the_plan_once` | pr-poll |
| pr.rs `an_owned_pull_request_that_left_the_search_and_is_merged_takes_the_merged_path` | pr-poll |
| pr.rs `an_owned_row_a_search_misses_is_read_and_an_open_answer_with_the_same_values_changes_nothing` | pr-poll |
| pr.rs `both_hooks_see_the_slot_the_gate_holds_and_no_other_cpd_variable` | pr-open (asserts `Call.env` of the hook) |
| pr.rs `dispatch_rows_carry_label_number_state_checks_decision_and_title` | pr-poll |
| pr.rs `improvements_are_stored_and_tell_nobody` | pr-poll |
| pr.rs `open_adopting_a_merged_pull_request_keeps_the_plan_in_dispatch_while_another_pr_is_not_open` | pr-open |
| pr.rs `open_adopting_a_merged_pull_request_supersedes_the_pending_reviews` | pr-open |
| pr.rs `open_adopting_a_merged_pull_request_updates_the_row_the_poll_stored_open` | pr-open |
| pr.rs `open_adopts_a_merged_pull_request_without_the_gate_and_the_plan_is_done` | pr-open |
| pr.rs `open_adopts_a_pr_an_earlier_run_created` | pr-open |
| pr.rs `open_adopts_an_open_pull_request_of_a_lead_branch_that_is_behind_its_base` | pr-open |
| pr.rs `open_adopts_the_pull_request_a_concurrent_open_created_when_its_create_fails` | pr-open (two threads, scripted `gh pr create` failure) |
| pr.rs `open_after_another_open_opened_the_pr_during_its_gate_is_already_and_pushes_nothing` | pr-open (a hook handler that runs the second `run_with`) |
| pr.rs `open_again_is_already_and_touches_nothing` | pr-open |
| pr.rs `open_an_unknown_plan_is_plan_not_found` | `{W,K}` (no git, no gh) |
| pr.rs `open_by_hand_goes_on_when_the_open_job_failed_was_cancelled_or_merged_and_counts_it_for_nothing` | pr-open |
| pr.rs `open_does_not_fetch_the_base_again_after_the_gate_so_a_base_that_moves_then_refuses_nothing` | pr-open |
| pr.rs `open_fails_loud_when_gh_cannot_find_the_pull_request_it_created` | pr-open |
| pr.rs `open_gates_pushes_creates_the_pr_and_records_it` | pr-open |
| pr.rs `open_push_and_rebase_run_no_r_for_another_prs_orphaned_merge` | pr-open + `G6` |

Totals: git.rs 14 (12 + 2 §5), git_guard.rs 2 (§5), merge.rs 41, pr.rs 37 (36 + 1 §5) = 94.

## 4. Proposed migration PRs

Order puts the fewest seams first. Counts include the restored tests of §3 and exclude §5, the helper (row 23) and nothing else: 313 current + 89 restored = 402. Jobs per PR are of kind `docs`, `test`, `impl`; every `test` Job ends with the migrated tests passing in-process and the old exec test deleted in the same commit.

**PR `mr-pr-1`: no git** (39 tests). Needs `{clk, runner, run_with, testkit}` (testkit only for the `Repo` handle's tempdir; no `port-git` Job).
Files: git.rs hooks (13 + 1 restored), git_guard.rs (2 U), gate.rs (2), pr_view.rs (6), pr_merge.rs (14), repo_hooks Worker Spec (1).
1. impl: thread `Ports` into `git/hooks/exec.rs` and `Slots`/`take` in `git/gate.rs` (clock for the wait).
2. test: git.rs hooks (rows 3 + `an_undefined_variable...`) and the `gate.rs` pair, in place in `core/src/git/gate.rs`.
3. test: pr_view.rs + pr_merge.rs on `run_with` with a stateful `gh` handler (replaces the flag file of `gh_read_back`); 1 S.
4. test: U tests (git_guard 2, Worker Spec via `render_spec`) and the S picks of git_guard (§5).
5. docs: drop the `cpd-core git::` line of `scripts/tripwire/allow.txt` if no other test of `git::` needs it.

**PR `mr-pr-2`: git read + worktree** (102). Needs `{clock, runner, spawn, run_with, testkit, git-route Jobs 1-3}`.
Files: pr_local_needs 5, pr_crud 62, pr.rs `pr_add_*` 3, git.rs rows 4-8 (25) + 7 restored.
1. test: pr_local_needs.rs + the three `pr_add_*` (8).
2. test: pr_crud.rs, 62 (split point: the `add` tests and the `edit`/`cancel` tests, at `pr_crud.rs:~600`).
3. test: git.rs tamper (10, +2 restored), rule S (3, +3 restored: reverse order, two syncs, index lock), gate worktrees and sweep (6, +2 restored: `branch -d`, `the_gate_runs_pre_start_then_pre_merge...`) (26).
4. test: git.rs retries with scripted replies and `ManualClock` (6).
5. impl (only if needed): `Spawn::alive(pid)` in `git/snapshot.rs` sweep (`tests-mock-ports.md` D).

**PR `mr-pr-3`: git remote** (106). Needs PR 2's set plus `git-route Job 4`.
Files: pr_cleanup 14, pr_cleanup_cancel 3, pr_crud legacy 5, repo_hooks 5, pr_push 11, pr.rs poll group 33 (+20 restored), git.rs lead/attempt/rule P 11 (+4 restored).
1. test: pr.rs poll group, 33 + 19 + `a_second_poll_waits_for_the_lock` (53).
2. test: pr_cleanup.rs + pr_cleanup_cancel.rs (17; 2 need `S`, 1 needs two concurrent `run_with` calls and a hook handler that blocks on a `Barrier`, no sleep).
3. test: pr_push.rs (11) + repo_hooks.rs (5) (paused-gate tests: two threads, one blocked in the `sh` handler) (16).
4. test: git.rs lead/attempt/rule P (11, +4 restored) + pr_crud legacy (5) (20).

**PR `mr-pr-4`: merge-tree, rebase, engine** (155). Needs PR 3's set plus `git-route Jobs 5, 6` and `tmux-route` (5 tests).
Files: pr_start 25, pr.rs open 39 + 1 + slots 3 (+16 restored), pr.rs rebase 24, merge.rs 6 (+41 restored).
1. test: pr_start.rs 20 (`plan lock` all-or-nothing, remote Lead Branch reuse, stacked starts).
2. test: pr_start.rs engine 5 (E7) driven by `ManualClock::advance` and the engine entry `watch::run(&ports, until)` of `tests-mock-ports.md` (`unverified`), replacing `Fx::engine` and `wait_until`.
3. test: pr.rs open group 39 + the dead-pid test 1 + 3 slots + 16 restored (59).
4. test: pr.rs rebase 24 (7 `paused`-style: handler-blocked threads, no sleep).
5. test: merge.rs 47 (6 + 41) on `merge::merge` with `Ctx`; `cas_lost...` and `r_on_a_git_failure...` use scripted replies.

**If the 60 cap holds: 8 PRs** (402 tests; seam boundaries and file boundaries, 4 of them cannot be cut smaller without cutting inside a file): `mr-pr-1` 39; `mr-pr-2a` git.rs 32 + pr_local_needs 5 + pr_add 3 = 40; `mr-pr-2b` pr_crud 62 (2 over: cut inside the file, `unverified` where); `mr-pr-3a` pr.rs poll 53; `mr-pr-3b` cleanup 17 + push 11 + repo_hooks 5 + git.rs lead 15 + legacy 5 = 53; `mr-pr-4a` pr.rs open 59; `mr-pr-4b` pr_start 25 + pr.rs rebase 24 = 49; `mr-pr-4c` merge.rs 47. 39+40+62+53+53+59+49+47 = 402. Each keeps the `needs` of its group in §2.

## 5. Not migratable as asked (subject is the OS behaviour of an external)

| test | why | proposal that keeps policy [tests] |
|---|---|---|
| git_guard.rs `it_refuses_git_stash...` | the subject is the authored wrapper script run by real `sh` | S: 1 test, the whole `stash` argv table in one run (the 3-smoke cap of the area) |
| git_guard.rs `it_says_so...` x2 (no real git on `PATH`: exit 127) | same | S: 1 test, both `PATH` layouts in it; with the S above and `pr_merge`'s usage test that makes the area's 3 |
| git_guard.rs `it_runs_other_commands_through_the_real_git` (+ deleted `it_ends_at_the_real_git...`, `it_finds_the_real_git...`) | needs a real `git` behind the wrapper: policy forbids it | replace the target of `exec` by a symlink to `/usr/bin/true` in the same S test (guess: the Lead decides whether `true` counts as an external; if it does, delete) |
| git.rs `a_background_child_of_a_finished_hook_is_killed_when_the_hook_returns` (and deleted `a_hook_is_killed_with_its_process_group_at_the_timeout`, `a_git_call_past_its_deadline_is_killed_with_its_group_and_is_tool_timeout`) | the subject is `killpg` on a real child (`host/deadline`, `git/hooks/exec.rs`) | test the mapping only: `FakeRunner` replies a timeout `Outcome`, assert `tool_timeout` / the hook error and the `Call` carried the deadline. The real kill cannot be tested under policy [tests]; say so in the `RealRunner` doc (the Lead decides) |
| gate.rs `a_slot_held_by_a_dead_process_is_free_at_once` (and deleted pr.rs `a_slot_held_by_a_killed_gate_is_free_at_once`) | the subject is the kernel releasing a `flock` when the holder dies (`core/src/git/gate.rs:125`: a real `sleep` inherits the descriptor) | delete both: it is the OS's guarantee; keep a U test that a slot whose `Flock` is dropped is taken at once (tempdir, no process) |

The other tests that look OS-bound are migratable: `git.rs a_command_killed_by_a_signal_is_an_error_not_a_failure` (a scripted signal `Outcome`; `unverified` that the type has one), `cockpit_pi_and_cpd_variables_do_not_reach_hooks` (assert the fake's `Call.env`; `git/hooks/exec.rs withholds_the_cockpit_pi_and_cpd` already pins the filter), the six `sleep`/lock-race git.rs tests (scripted replies + `ManualClock`), `eight_pr_starts_in_one_repo_at_once...` (8 threads on the route's mutex; it no longer proves git's own ref locking, which is git's), the `Flock` slot waits (`pr.rs` 3092-3134, in-process `Flock` on a tempdir with `ManualClock`; flock is the OS's but only taken, not tested).
