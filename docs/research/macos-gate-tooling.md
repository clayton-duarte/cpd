# macOS Gate: the `tooling` nextest binary

Research only (no code changed). Mac `pro` (Darwin arm64), 49-character default TMPDIR
(`/var/folders/7_/zb1y2vw95tl8x_r6s4p0yqn40000gq/T/`) plus `gate-run.<id>`; commit `CPD_COMMIT` = base of this branch.
Method: run A = `cpd slot -- scripts/cargo.sh nextest run -p cpd --test tooling --no-fail-fast` under that TMPDIR; run B =
the same binary under `TMPDIR=/tmp/gm.XXXXXX` (no slot); alone = one test per run, 3 times, short TMPDIR.

## Summary

- Run A: 173 tests, 140 passed, **33 failed**, 0 timed out; 89 s (nextest), 1m52s wall; load 16 -> 34.
- Run B: 144 passed, 26 failed, 3 timed out (e2e, e2e_cross, prune_script); 183 s; load 63 (B overlapped other Gates).
- H1 (socket path length) is true for **5** tests only (C1), and for 2 more tests the failure is a `//` in TMPDIR (C2).
  Most failures (22) are not path-related: they fail the same way under a short TMPDIR.
- Alone runs (4 in parallel, load 74-124, so e2e/e2e_cross fail there for load; re-checked one at a time at load ~40).
- Failures of A: 33 = 4 covered elsewhere + 29 analysed.

| class | tests | clusters |
|---|---|---|
| path-length | 5 (+ load-flake in B for 2 of them) | C1 |
| test-assumption | 2 (`//` in TMPDIR) + 6 (harness: /proc, setsid) + 13 (`flock` CLI) + 1 (BSD `sed -i`) = 22 | C2, C3, C4, C6 |
| macos-code | 2 (BSD `sed` `\|` in scripts/install-cpd.sh) | C5 |
| load-flake | 1 (prune_script; also seen on e2e/e2e_cross in B) | C7 |
| timeout / other | 0 | |

Covered elsewhere (not analysed): `gate_run::a_clean_run_prints_nothing_and_returns_the_commands_status`,
`gate_run::a_process_left_with_the_run_id_fails_the_run_and_is_stopped`, `gate_run::a_socket_named_with_the_run_id_fails_the_run`,
`gate_run::another_runs_leftovers_do_not_fail_this_run` (`pi_check.rs` and `cargo_script.rs` had no failure).

Caveat: A ran without `GATE_RUN` set (the spec's A command); the Gate adds 7 characters to every socket name (C1), so no
test that passes here gets shorter.

## Clusters

### C1 `path-length`: tmux socket longer than 103 bytes
Tests: `e2e::a_whole_plan_runs_from_context_new_to_pr_poll`, `e2e_cross::a_plan_across_two_repos_runs_a1_then_b_then_a2_and_a_review_job`,
`harness::tmux_server_is_private_and_killed_on_drop`, `no_repo_lock::a_merge_during_pr_rebases_gate_lands_and_reaches_origin_through_the_rebases_push`,
`no_repo_lock::two_plans_merge_into_their_prs_of_one_repo_at_once_with_overlapping_windows_and_no_error`.
Cause: `TestEnv` sets `TMUX_TMPDIR=<env root>/tmux` (cli/tests/common/mod.rs:749; teardown at :162-175 scans
`$dir/tmux/*/$socket`); tmux resolves /var to /private/var and fails with `error connecting to
/private/var/folders/.../T/gate-run.g625TK/cpd-test-TUs5ob/tmux/tmux-503/cpdt-2100-0 (File name too long)`: 114 characters
(122 with `GATE_RUN` in the name, `cpdt-<run>-<pid>-<n>`); the limit is 103. The prefix alone (`/private` + TMPDIR + `gate-run.x`) is 73, so no
rename of `cpd-test-*` fits: the tmux dir must leave TMPDIR.
Fix (1 Job): in `TestEnv::new`/`command` (common/mod.rs) put the tmux dir at a short path when
`<root>/tmux/tmux-<uid>/<socket>` would pass 100 characters: `TMUX_TMPDIR=/tmp` (sockets `/tmp/tmux-<uid>/<socket>`, which
scripts/gate-run.sh already scans, plan A2), else keep `<root>/tmux`. Update `stop_tmux` in TEARDOWN_SH (it must find the
socket in either place, and `rm -f` the socket file, since `rm -rf "$dir"` no longer holds it) and the assertions of
`harness::tmux_server_is_private_and_killed_on_drop` (harness.rs:90-110, which looks in `env.root().join("tmux")`). Linux
unchanged: the short-path branch is taken only above the length limit (`/dev/shm` paths are shorter).
In B these tests did not fail on the path; `e2e`/`e2e_cross` still reached the 90 s nextest limit (C7).

### C2 `test-assumption`: `//` in TMPDIR
Tests: `preview_script::on_builds_into_the_preview_dir_only`, `preview_script::on_respawns_the_five_views_with_the_preview_and_leaves_lead_and_worker_alone`.
Cause: the default TMPDIR ends in `/`, so scripts/gate-run.sh:22-24 (`mktemp -d "$base/gate-run.XXXXXX"`) gives `.../T//gate-run.x`.
scripts/preview-tui.sh:66 resolves the worktree with `cd ... && pwd`, which collapses `//`; the tests compare against the
unnormalised `p.worktree` (preview_script.rs:151, :198). Both pass in B.
Fix (<1 Job, fold into C1's Job): `envs_dir()` (common/mod.rs:230) returns `PathBuf::from(tmpdir).components().collect()`
(drops the double slash). Linux unchanged (`/dev/shm`, `/tmp/cg` have none). Alternative: `base=${TMPDIR%/}` in gate-run.sh.
Note: `/var` -> `/private/var` is not a problem here (the script keeps the logical path).

### C3 `test-assumption`: no `flock` command on macOS (13 tests)
Tests: `install_script::` `a_failing_config_leaves_the_old_engine_alone_and_says_so`, `a_failing_watch_check_leaves_the_old_engine_and_the_views_alone`,
`a_good_install_marks_the_restart_after_the_check_and_right_before_the_kill_window`,
`a_good_install_verifies_then_respawns_the_views_then_retires_the_old_engine_and_starts_the_new_one`,
`a_kill_window_that_fails_says_the_engine_was_not_stopped_and_still_runs_the_old_binary`,
`a_lock_the_old_engine_keeps_past_the_wait_means_the_new_engine_is_never_started`, `a_mark_restart_that_fails_is_printed_and_the_retirement_goes_on`,
`a_new_engine_that_cannot_start_after_the_retirement_says_the_old_one_was_stopped`, `an_old_engine_that_holds_its_lock_for_longer_than_ten_seconds_is_waited_for`,
`an_old_engine_that_never_frees_its_lock_fails_the_install_with_the_reason`, `the_install_restarts_the_engine_of_a_real_cockpit_for_a_caller_without_a_utf8_locale`,
`the_follow_ups_run_after_a_slow_config`, `the_installer_goes_on_to_the_views_and_the_engine_after_migrating_on_a_python_without_renameat2`.
Cause: the tests run the util-linux `flock` (`which flock` is empty here): install_script.rs:232 (`lock_held`, `Command::new("flock")`, panics
NotFound), :240 (`old_engine`, `flock --no-fork`), :206-207 (`STARTS_ENGINE` fake), :405 (tmux pane `exec flock --no-fork`), :1000-1001. The
script under test is fine: scripts/install-cpd.sh:202-206 already takes the lock with python `fcntl.flock`.
Fix (1 Job): a `flock` shim, written once by the harness into a dir on every env's PATH (`env.bin()`) and used by `lock_held`/`old_engine` by
absolute path, supporting `[-n|--no-fork] <file> <cmd...>`. A perl shim was checked here: `$^F=255; open '>>'; flock LOCK_EX(|LOCK_NB); exec @ARGV`
holds the lock across `exec`, `-n` exits 1 while held, 0 when free (perl, unlike python3, exists in the python-less PATH test, :905, which links
`flock` into its tools dir). Files: cli/tests/common/mod.rs (shim), cli/tests/tooling/install_script.rs. Linux unchanged: the shim is used only
where `flock` is not found (or always: it is semantically the same). Unverified: that all 13 pass afterwards (the next failure behind `flock` was not reachable).

### C4 `test-assumption`: BSD `sed -i` in the release fake
Test: `release_script::release_opens_the_pr_with_one_release_commit`. Cause: the fake `cargo` runs `sed -i "s/^version = .*/.../" Cargo.toml`
(release_script.rs:58); BSD `sed -i` takes the script as the backup suffix and fails, so Cargo.toml is not bumped (commit holds CHANGELOG.md,
Cargo.lock; test :144 expects Cargo.toml too). Fix (<1 Job, release_script.rs only): `sed "..." Cargo.toml > Cargo.toml.new && mv Cargo.toml.new Cargo.toml`;
same on both seds. Linux unchanged.

### C5 `macos-code`: BSD sed has no `\|` in scripts/install-cpd.sh:95
Tests: `install_script::the_store_verdict_refuses_with_its_message_and_the_fix_of_its_code`, `install_script::another_refusal_code_prints_the_message_alone`.
Cause: `sed -n 's/.*"refusal": *{.*"message": *"\(\(\\.\|[^"\\]\)*\)".*/\1/p'` (BRE alternation `\|` is a GNU extension); BSD sed prints nothing, so
the refusal reads `this build cannot open .../cpd.db: ` with an empty message (stderr of both tests). Verified here: `sed -En 's/.*"refusal": *\{.*"message": *"((\\.|[^"\\])*)".*/\1/p'`
prints `the verdict words`. Fix (<1 Job): that line only (`sed -E` is POSIX-2024 and in GNU/BSD; line 94 works as is). Linux unchanged: same output.

### C6 `test-assumption`: the harness uses /proc and `setsid` (6 tests)
Tests: `harness::a_dropped_env_leaves_no_tmux_server_process_dir_or_reaper`, `harness::a_killed_test_leaves_no_git_wrapper_process`,
`harness::a_killed_test_leaves_no_tmux_server_process_dir_or_reaper`, `harness::a_process_that_leaves_the_group_and_is_not_recorded_outlives_the_teardown`,
`harness::a_server_the_test_kills_is_not_restarted_by_its_supervising_hooks`, `harness::teardown_is_idempotent_and_spares_the_test_process`.
Cause (all fail alone 3/3): harness.rs:316, :384 `alive` and :392 `processes_mentioning` read /proc (the last `unwrap`s `read_dir("/proc")`: NotFound at :393,
and the git-wrapper helper panics there, so no READY, :625; the tmux helper prints no DIR, :461; `alive(std::process::id())` is false, :706); harness.rs:311
`detached` runs the `setsid` command (absent on macOS), so `wait_until(...session of its own)` times out (common/mod.rs:360).
The same in the harness itself, TEARDOWN_SH (common/mod.rs:123 `alive` from /proc/<pid>/stat, :128 `mine` from /proc/<pid>/environ, :198 `exec setsid`): on macOS the
reaper of a killed test exits at once, so nothing is torn down for a killed test (it works for a dropped env).
macOS equivalents: liveness `kill -0` plus `ps -o stat= -p <pid>` (Z = zombie); session `ps -o sess= -p`... (macOS prints 0: use `ps -o pgid=,tpgid=`
or have the helper print `perl -MPOSIX -e 'print getsid(pid)'`; unverified, check in the Job); the environment `ps -E -o command= -p <pid>` (own processes only) for `mine`;
command lines `ps -axo pid=,command=` for `processes_mentioning`; `setsid` -> `perl -MPOSIX -e 'POSIX::setsid(); exec @ARGV'` (also for the reaper's `exec setsid`).
Fix (2 Jobs: (a) TEARDOWN_SH in common/mod.rs, (b) harness.rs helpers and their asserts): each branches on `uname` (Linux keeps /proc and `setsid`), so
Linux behaviour is unchanged. SKIP-ONLY is not needed. Risk: `ps -E` on macOS may not show the environment of a process whose env is hidden; if so, `mine` can
compare the start `ps -o lstart=` recorded at spawn instead.

### C7 `load-flake`: the 90 s nextest limit (`slow-timeout 30s x3`, .config/nextest.toml:33)
Tests: `prune_script::prunes_exactly_the_worktrees_whose_work_is_on_main` (3/3 alone, 12 s; TIMEOUT in B at load 63: ~40 sequential git processes in
prune_script.rs:111-); `e2e::a_whole_plan_runs_from_context_new_to_pr_poll` (3/3 at load ~40, 13 s; fails at load 90-120);
`e2e_cross::...` (1/3 at load 40-56: 50 s when it passes, TIMEOUT otherwise). They wait for engine events with `wait_event(..., 90)` and the process limit.
Not a defect of the Mac's tools; both e2e tests are also in C1. Fix: none proposed (the Gate would give them a quiet machine); if they time out after C1,
raise `terminate-after` for `binary_id(cpd::tooling) and test(/^e2e/)` with a measurement (the rule of nextest.toml:31-32). 0 Jobs now.

## Tests

A = run A (Gate TMPDIR), B = run B (short TMPDIR), alone = 3 runs, short TMPDIR (P pass, F fail).

| test | class | cluster | A | B | alone 3x |
|---|---|---|---|---|---|
| e2e::a_whole_plan_runs_from_context_new_to_pr_poll | path-length | C1 | fail | timeout | F F F at load ~100; P P P at load ~40 |
| e2e_cross::a_plan_across_two_repos_runs_a1_then_b_then_a2_and_a_review_job | path-length | C1 | fail | timeout | F F F at load ~100; P F F at ~45 |
| harness::tmux_server_is_private_and_killed_on_drop | path-length | C1 | fail | pass | - |
| no_repo_lock::a_merge_during_pr_rebases_gate_lands_and_reaches_origin_through_the_rebases_push | path-length | C1 | fail | pass | - |
| no_repo_lock::two_plans_merge_into_their_prs_of_one_repo_at_once_with_overlapping_windows_and_no_error | path-length | C1 | fail | pass | - |
| preview_script::on_builds_into_the_preview_dir_only | test-assumption | C2 | fail | pass | - |
| preview_script::on_respawns_the_five_views_with_the_preview_and_leaves_lead_and_worker_alone | test-assumption | C2 | fail | pass | - |
| install_script::a_failing_config_leaves_the_old_engine_alone_and_says_so | test-assumption | C3 | fail | fail | F F F |
| install_script::a_failing_watch_check_leaves_the_old_engine_and_the_views_alone | test-assumption | C3 | fail | fail | F F F |
| install_script::a_good_install_marks_the_restart_after_the_check_and_right_before_the_kill_window | test-assumption | C3 | fail | fail | F F F |
| install_script::a_good_install_verifies_then_respawns_the_views_then_retires_the_old_engine_and_starts_the_new_one | test-assumption | C3 | fail | fail | F F F |
| install_script::a_kill_window_that_fails_says_the_engine_was_not_stopped_and_still_runs_the_old_binary | test-assumption | C3 | fail | fail | F F F |
| install_script::a_lock_the_old_engine_keeps_past_the_wait_means_the_new_engine_is_never_started | test-assumption | C3 | fail | fail | F F F |
| install_script::a_mark_restart_that_fails_is_printed_and_the_retirement_goes_on | test-assumption | C3 | fail | fail | F F F |
| install_script::a_new_engine_that_cannot_start_after_the_retirement_says_the_old_one_was_stopped | test-assumption | C3 | fail | fail | F F F |
| install_script::an_old_engine_that_holds_its_lock_for_longer_than_ten_seconds_is_waited_for | test-assumption | C3 | fail | fail | F F F |
| install_script::an_old_engine_that_never_frees_its_lock_fails_the_install_with_the_reason | test-assumption | C3 | fail | fail | F F F |
| install_script::the_install_restarts_the_engine_of_a_real_cockpit_for_a_caller_without_a_utf8_locale | test-assumption | C3 | fail | fail | F F F |
| install_script::the_follow_ups_run_after_a_slow_config | test-assumption | C3 | fail | fail | F F F |
| install_script::the_installer_goes_on_to_the_views_and_the_engine_after_migrating_on_a_python_without_renameat2 | test-assumption | C3 | fail | fail | F F F |
| release_script::release_opens_the_pr_with_one_release_commit | test-assumption | C4 | fail | fail | F F F |
| install_script::the_store_verdict_refuses_with_its_message_and_the_fix_of_its_code | macos-code | C5 | fail | fail | F F F |
| install_script::another_refusal_code_prints_the_message_alone | macos-code | C5 | fail | fail | F F F |
| harness::a_dropped_env_leaves_no_tmux_server_process_dir_or_reaper | test-assumption | C6 | fail | fail | F F F |
| harness::a_killed_test_leaves_no_git_wrapper_process | test-assumption | C6 | fail | fail | F F F |
| harness::a_killed_test_leaves_no_tmux_server_process_dir_or_reaper | test-assumption | C6 | fail | fail | F F F |
| harness::a_process_that_leaves_the_group_and_is_not_recorded_outlives_the_teardown | test-assumption | C6 | fail | fail | F F F |
| harness::a_server_the_test_kills_is_not_restarted_by_its_supervising_hooks | test-assumption | C6 | fail | fail | F F F |
| harness::teardown_is_idempotent_and_spares_the_test_process | test-assumption | C6 | fail | fail | F F F |
| prune_script::prunes_exactly_the_worktrees_whose_work_is_on_main | load-flake | C7 | pass | timeout | P P P |
| gate_run::* (4 tests) | covered elsewhere | - | fail | fail | not run |
