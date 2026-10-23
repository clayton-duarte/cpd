# Why tests leaked on macOS, and the leftovers (tests-mock, job leak, attempt leak-a1)

Static analysis only: no test was run. Taken 2026-10-23 12:41-12:50 EDT on pro (uid 503, load 40-120). Every claim is marked **verified** (read in a process, file or the code, in this session) or **unverified** (inferred).

## Summary

- 55 leftover processes (before cleanup), in five classes (A-E below) plus the pane processes under them. Not one started after the macOS Gate fix (#278, merged 2026-10-23 09:00): the youngest leftover is 11 h 20 min old (started 01:21 on Oct 23). **verified** (etime of every row).
- The biggest class (34 servers, A) is not an OS problem: tests of the unmerged branch `bifrost/provider` (`cli/tests/flow/provider_flow.rs`) run `cpd plan new`, which starts a **real tmux server on the default socket `cpd`** (a `park-pl` window and `cpd session run <uuid>`); the harness teardown only looks for the env's own `cpdt-*` socket, so nothing ever stops it. It leaks on Linux the same way. **verified**.
- The `tui_idle` (B), `bare::` / `e2e_cross` (C) and `harness` teardown-test (D) leftovers all predate #278, whose changes are exactly the macOS branches of `TEARDOWN_SH` (`alive`, `mine`, the perl `setsid`). The worktrees that ran them are gone, so which code they ran is **unverified**; the dates are verified.
- The Gate's leak check (`scripts/gate-run.sh`) saw none of A, B, D, E: they ran in targeted Worker runs (no `GATE_RUN` in their environment, **verified**) or hid their environment (Apple `/bin/sh`, **verified**).
- Selection and counts: see Cleanup.

## Step 1: leftovers found (before any kill)

`ps -axww -o pid=,ppid=,etime=,command=`, then `lsof -a -p <pid> -d cwd -Fn` and `ps eww -o command= -p <pid>` (the environment of non-Apple binaries: the Homebrew tmux and the copied `old-tmux`; an Apple binary shows none). Every process below has ppid 1. All their cwd directories (`cpd-test-*`) are already deleted: only a process kept the path.

Protected, found and not touched: pid 5683 (`tmux -u -L cpd ... new-session -d -s cpd-work`, CPD_HOME=/Users/Clayton.Duarte/.cpd, 2 d 1 h) and its clients 5582 (2 d 1 h) and 65309 (17 h, `attach -t cpd`).

### Class A: `tmux -u -L cpd` servers of `provider_flow::*` (34), `new-session -d -s cpd -n park-pl ... -- cpd session run <uuid>`

`NEXTEST_BINARY_ID=cpd::flow`, no `GATE_RUN`, `CPD_HOME=<cwd dir>/cpd-home`, `TMUX_TMPDIR=<cwd dir>/tmux`. Five tests, run by seven targeted runs in `cpd.bifrost--provider-impl-a1` (Oct 22 22:09-22:25). Test name (last segment) -> pids:

- `a_context_without_an_owner_is_owner_unset_and_an_owner_without_a_policy_file_is_owner_unknown_for_tiers` (6): 22690, 28861, 52859, 81177, 84671, 89483
- `an_owner_policy_without_a_model_table_is_github_copilot_and_behaves_as_before` (7): 22861, 29104, 52927, 70980, 81089, 84564, 89304
- `an_owner_whose_policy_says_bifrost_makes_a_bifrost_context_where_the_machine_allows_it` (7): 23217, 29537, 52769, 70903, 81109, 84596, 89432
- `job_add_in_a_bifrost_plan_checks_the_bifrost_catalog_and_the_machines_providers` (7): 23695, 29362, 52778, 70942, 81053, 84632, 89340
- `tiers_inside_a_bifrost_plan_defaults_to_bifrost_by_flag_and_by_cpd_plan` (7): 23490, 29487, 53032, 71147, 81481, 85012, 89848

Full table:

| pid | etime | test (NEXTEST_TEST_NAME) | worktree (CARGO_MANIFEST_DIR) | cwd dir |
|---|---|---|---|---|
| 22690 | 14:13:15 | a_context_without_an_owner_is_owner_unset_and_an_owner_without_a_policy_file_is_owner_unknown_for_tiers | cpd.bifrost--provider-impl-a1 | cpd-test-e5xeIk |
| 22861 | 14:13:15 | an_owner_policy_without_a_model_table_is_github_copilot_and_behaves_as_before | cpd.bifrost--provider-impl-a1 | cpd-test-HMa8Gh |
| 23217 | 14:13:16 | an_owner_whose_policy_says_bifrost_makes_a_bifrost_context_where_the_machine_allows_it | cpd.bifrost--provider-impl-a1 | cpd-test-79mISu |
| 23490 | 14:13:15 | tiers_inside_a_bifrost_plan_defaults_to_bifrost_by_flag_and_by_cpd_plan | cpd.bifrost--provider-impl-a1 | cpd-test-b3lyP5 |
| 23695 | 14:13:17 | job_add_in_a_bifrost_plan_checks_the_bifrost_catalog_and_the_machines_providers | cpd.bifrost--provider-impl-a1 | cpd-test-gCUhcF |
| 28861 | 14:16:46 | a_context_without_an_owner_is_owner_unset_and_an_owner_without_a_policy_file_is_owner_unknown_for_tiers | cpd.bifrost--provider-impl-a1 | cpd-test-aLp6v4 |
| 29104 | 14:16:46 | an_owner_policy_without_a_model_table_is_github_copilot_and_behaves_as_before | cpd.bifrost--provider-impl-a1 | cpd-test-Ei97DA |
| 29362 | 14:16:47 | job_add_in_a_bifrost_plan_checks_the_bifrost_catalog_and_the_machines_providers | cpd.bifrost--provider-impl-a1 | cpd-test-sbxyZJ |
| 29487 | 14:16:47 | tiers_inside_a_bifrost_plan_defaults_to_bifrost_by_flag_and_by_cpd_plan | cpd.bifrost--provider-impl-a1 | cpd-test-QUC1cY |
| 29537 | 14:16:47 | an_owner_whose_policy_says_bifrost_makes_a_bifrost_context_where_the_machine_allows_it | cpd.bifrost--provider-impl-a1 | cpd-test-7awcX2 |
| 52769 | 14:18:46 | an_owner_whose_policy_says_bifrost_makes_a_bifrost_context_where_the_machine_allows_it | cpd.bifrost--provider-impl-a1 | cpd-test-R8kF04 |
| 52778 | 14:18:48 | job_add_in_a_bifrost_plan_checks_the_bifrost_catalog_and_the_machines_providers | cpd.bifrost--provider-impl-a1 | cpd-test-ILhWZ9 |
| 52859 | 14:18:49 | a_context_without_an_owner_is_owner_unset_and_an_owner_without_a_policy_file_is_owner_unknown_for_tiers | cpd.bifrost--provider-impl-a1 | cpd-test-3lI5Wf |
| 52927 | 14:18:50 | an_owner_policy_without_a_model_table_is_github_copilot_and_behaves_as_before | cpd.bifrost--provider-impl-a1 | cpd-test-k2ATjz |
| 53032 | 14:18:50 | tiers_inside_a_bifrost_plan_defaults_to_bifrost_by_flag_and_by_cpd_plan | cpd.bifrost--provider-impl-a1 | cpd-test-RNW22d |
| 70903 | 14:21:04 | an_owner_whose_policy_says_bifrost_makes_a_bifrost_context_where_the_machine_allows_it | cpd.bifrost--provider-impl-a1 | cpd-test-XXgTQ8 |
| 70942 | 14:21:06 | job_add_in_a_bifrost_plan_checks_the_bifrost_catalog_and_the_machines_providers | cpd.bifrost--provider-impl-a1 | cpd-test-qDGC49 |
| 70980 | 14:21:06 | an_owner_policy_without_a_model_table_is_github_copilot_and_behaves_as_before | cpd.bifrost--provider-impl-a1 | cpd-test-WoDPA1 |
| 71147 | 14:21:06 | tiers_inside_a_bifrost_plan_defaults_to_bifrost_by_flag_and_by_cpd_plan | cpd.bifrost--provider-impl-a1 | cpd-test-gN0EpD |
| 81053 | 14:20:32 | job_add_in_a_bifrost_plan_checks_the_bifrost_catalog_and_the_machines_providers | cpd.bifrost--provider-impl-a1 | cpd-test-OozLjS |
| 81089 | 14:20:32 | an_owner_policy_without_a_model_table_is_github_copilot_and_behaves_as_before | cpd.bifrost--provider-impl-a1 | cpd-test-n5Sf0B |
| 81109 | 14:20:32 | an_owner_whose_policy_says_bifrost_makes_a_bifrost_context_where_the_machine_allows_it | cpd.bifrost--provider-impl-a1 | cpd-test-3lU2mA |
| 81177 | 14:20:33 | a_context_without_an_owner_is_owner_unset_and_an_owner_without_a_policy_file_is_owner_unknown_for_tiers | cpd.bifrost--provider-impl-a1 | cpd-test-A23uVw |
| 81481 | 14:20:33 | tiers_inside_a_bifrost_plan_defaults_to_bifrost_by_flag_and_by_cpd_plan | cpd.bifrost--provider-impl-a1 | cpd-test-qG7vpo |
| 84564 | 14:20:20 | an_owner_policy_without_a_model_table_is_github_copilot_and_behaves_as_before | cpd.bifrost--provider-impl-a1 | cpd-test-ziAopu |
| 84596 | 14:20:20 | an_owner_whose_policy_says_bifrost_makes_a_bifrost_context_where_the_machine_allows_it | cpd.bifrost--provider-impl-a1 | cpd-test-XbXg66 |
| 84632 | 14:20:21 | job_add_in_a_bifrost_plan_checks_the_bifrost_catalog_and_the_machines_providers | cpd.bifrost--provider-impl-a1 | cpd-test-SPDLez |
| 84671 | 14:20:21 | a_context_without_an_owner_is_owner_unset_and_an_owner_without_a_policy_file_is_owner_unknown_for_tiers | cpd.bifrost--provider-impl-a1 | cpd-test-oxl6L8 |
| 85012 | 14:20:21 | tiers_inside_a_bifrost_plan_defaults_to_bifrost_by_flag_and_by_cpd_plan | cpd.bifrost--provider-impl-a1 | cpd-test-XlY7XB |
| 89304 | 14:09:38 | an_owner_policy_without_a_model_table_is_github_copilot_and_behaves_as_before | cpd.bifrost--provider-impl-a1 | cpd-test-q16vdx |
| 89340 | 14:09:39 | job_add_in_a_bifrost_plan_checks_the_bifrost_catalog_and_the_machines_providers | cpd.bifrost--provider-impl-a1 | cpd-test-qgaqBb |
| 89432 | 14:09:39 | an_owner_whose_policy_says_bifrost_makes_a_bifrost_context_where_the_machine_allows_it | cpd.bifrost--provider-impl-a1 | cpd-test-uDazJN |
| 89483 | 14:09:39 | a_context_without_an_owner_is_owner_unset_and_an_owner_without_a_policy_file_is_owner_unknown_for_tiers | cpd.bifrost--provider-impl-a1 | cpd-test-4cCHXt |
| 89848 | 14:09:39 | tiers_inside_a_bifrost_plan_defaults_to_bifrost_by_flag_and_by_cpd_plan | cpd.bifrost--provider-impl-a1 | cpd-test-gr7gsZ |

### Class B: `tmux -L cpdt-<pid>-<n> new-session ... cpd tui view <view>` servers of `tui_idle::*` (9)

`NEXTEST_BINARY_ID=cpd::tui`, no `GATE_RUN`, targeted runs in `cpd.bifrost--cyan-impl-a1` (Oct 22 22:12-22:26) and `cpd.gatemac--g3-tests-proc-a1` (23:00). Each server has 2-6 `cpd tui view ...` pane processes (tree, plan, widgets, logs, worker) as children, cwd the deleted env dir (38 pane processes listed by `lsof` cwd, all older than 13 h).

| pid | etime | test (NEXTEST_TEST_NAME) | worktree (CARGO_MANIFEST_DIR) | cwd dir |
|---|---|---|---|---|
| 4920 | 14:15:55 | a_spinner_on_screen_keeps_turning | cpd.bifrost--cyan-impl-a1 | cpd-test-9jHRRK |
| 5112 | 14:15:55 | idle_dispatch_views_read_next_to_nothing | cpd.bifrost--cyan-impl-a1 | cpd-test-QUIYal |
| 5355 | 14:15:55 | idle_tree_and_plan_read_next_to_nothing | cpd.bifrost--cyan-impl-a1 | cpd-test-eAFL9s |
| 5394 | 14:15:56 | idle_views_write_nothing_to_the_terminal | cpd.bifrost--cyan-impl-a1 | cpd-test-6GssNQ |
| 74118 | 13:45:56 | idle_tree_and_plan_read_next_to_nothing | cpd.gatemac--g3-tests-proc-a1 | cpd-test-SVZUil |
| 86561 | 14:13:01 | a_spinner_on_screen_keeps_turning | cpd.bifrost--cyan-impl-a1 | cpd-test-ZPmlNL |
| 86765 | 14:13:01 | idle_dispatch_views_read_next_to_nothing | cpd.bifrost--cyan-impl-a1 | cpd-test-9DF1Q8 |
| 86992 | 14:13:01 | idle_tree_and_plan_read_next_to_nothing | cpd.bifrost--cyan-impl-a1 | cpd-test-V4kBvZ |
| 87093 | 14:13:01 | idle_views_write_nothing_to_the_terminal | cpd.bifrost--cyan-impl-a1 | cpd-test-feBZzY |

### Class C: servers of a Gate run that had `GATE_RUN` (3)

Copies of tmux in the env (`.../old-tmux/tmux`, the `bare::` tests' "older server") and the Homebrew tmux of an `e2e_cross` run, with `GATE_RUN` in their environment and `cpd watch` / `cpd session run` pane children (53425, 56677, 30605, 14547).

| pid | etime | test (NEXTEST_TEST_NAME) | worktree (CARGO_MANIFEST_DIR) | cwd dir |
|---|---|---|---|---|
| 14442 | 20:48:05 | a_plan_across_two_repos_runs_a1_then_b_then_a2_and_a_review_job | cpd.mcp-policy.mcp.gate-17658 GATE_RUN=OWtMqg | cpd-test-sGSuxd |
| 22141 | 20:41:24 | once_nothing_runs_the_engine_restarts_the_server_detached_and_every_session_is_back_in_its_place | cpd.bifrost.spike.gate-63902 GATE_RUN=cJSJxt | cpd-test-iTht0D |
| 23007 | 20:41:24 | when_it_is_not_safe_nothing_is_killed_the_restart_is_marked_pending_and_the_servers_binary_attaches | cpd.bifrost.spike.gate-63902 GATE_RUN=cJSJxt | cpd-test-AmdSXV |

### Class D: `sh -c trap 'tmux -L cpdt-... new-session -d "sleep 100 # <dir>"; exit 0' TERM; ... while :; do sleep 1 & wait $!; done` shells (8)

`/bin/sh` is `/bin/bash` here (`lsof`: txt `/bin/bash`), an Apple binary: no environment shown, so no test name. Each is the "driver that restarts tmux when stopped" of `cli/tests/tooling/harness.rs::put_up` (lines 373-381), so the test is `harness::a_dropped_env_leaves_no_tmux_server_process_dir_or_reaper` or the helper of `harness::a_killed_test_leaves_no_tmux_server_process_dir_or_reaper` (two sockets per Gate: pids 74094/74104, 75696/75927). They still loop: `pgrep -P` shows a fresh `sleep` child each time (**verified**). Sockets `cpdt-abcdef-*` name a `GATE_RUN` of `abcdef` while their cwds are `gate-run.q9mc3i`, `gate-run.MD6wqQ`, `gate-run.quNFPo`, `gate-run.3U7Itu`; two (21029, 21154) are in `.../T/cpd-test-*` (targeted runs).

| pid | etime | test (NEXTEST_TEST_NAME) | worktree (CARGO_MANIFEST_DIR) | cwd dir |
|---|---|---|---|---|
| 21029 | 13:44:09 | (shell: env hidden by SIP) | (env hidden) | cpd-test-wcOZKo |
| 21154 | 13:44:09 | (shell: env hidden by SIP) | (env hidden) | cpd-test-FS1ZoQ |
| 38674 | 13:40:57 | (shell: env hidden by SIP) | (env hidden) | cpd-test-oMhmhl |
| 75537 | 13:43:11 | (shell: env hidden by SIP) | (env hidden) | cpd-test-v7zo36 |
| 76770 | 13:43:10 | (shell: env hidden by SIP) | (env hidden) | cpd-test-luVSFT |
| 77658 | 13:41:46 | (shell: env hidden by SIP) | (env hidden) | cpd-test-PFcK62 |
| 77743 | 13:41:47 | (shell: env hidden by SIP) | (env hidden) | cpd-test-xvDNrX |
| 84565 | 13:40:42 | (shell: env hidden by SIP) | (env hidden) | cpd-test-jtZY2X |

### Class E: debug tmux (1)

3605: `/opt/homebrew/bin/tmux -vv -u -L cpdt-3348-0 ...`, `cd /private/tmp/tmuxlog` (the `-vv` log directory: `tmux-client-*.log`), `NEXTEST_TEST_NAME=tui::tui_marks_a_restart_of_an_older_tmux_server_pending_and_does_not_kill_it`, `CARGO_MANIFEST_DIR=cpd.gatemac--open-gate-a1`, `TMUX_TMPDIR=/tmp`, 11 h 20 min (Oct 23 01:21). A Worker's own tmux debugging of the #278 work (a wrapper that adds `-vv`). Its env dir `cpd-test-n48CtM` still exists (held as cwd of no process; mtime 01:21).

No `zsh -c sleep 120` / `sleep 120` older than 1 h exists any more (the ones present, ppid = a live tmux, were 1-40 s old: live Gates).

## Step 2: root causes

### What the harness does (read, **verified**)

`TestEnv::empty` starts the reaper `sh teardown.sh watch <test pid>` (own process group, `cli/tests/common/mod.rs` `start_reaper`). `Drop` -> `teardown()` kills the reaper and runs `teardown.sh <pgid>`: SIGTERM/SIGKILL the group, `stop_tmux` (only sockets `$tmuxdir/tmux-*/$socket` and `$socket-*`, i.e. `cpdt-...`), then the pids cpd *records* (`attempts.driver_pid`, `distill.pid`, `tmux-restart.pid`) while `mine` says they are the env's, then `rm -rf $dir`. A killed test never runs Drop; the reaper does the same when `alive <test pid>` is false. A process that leaves the group (tmux daemonises with setsid; `detached`) and is neither on a `cpdt-` socket of the env nor recorded in the store is never stopped (stated in the `TEARDOWN_SH` comment). `nextest.toml`: `leak-timeout = 100ms, result = fail`, `slow-timeout = 30 s x 3` (tmux-restart group 60 s x 3): a test killed at the limit never runs Drop (only the reaper cleans, and it is in its own group so nextest's group kill does not reach it).

### Class A: verified

Why cleanup did not run: the server is not on a `cpdt-` socket. `core/src/config/file.rs:370` defaults `tmux.socket` to `cpd`; `provider_flow.rs` `write_providers` writes the config with `write_config` (no `[tmux] socket`), where every test on main that runs `plan new` uses `write_tmux_config` or its own `[tmux] socket = env.tmux_socket()` (checked: tui.rs, inbox_rules.rs, tui_tree.rs, e2e.rs, e2e_cross.rs, and the files that call `write_tmux_config`). `cpd plan new` (`core/src/plan/new.rs:263`, `park_window`) starts the Lead in `park-pl` on that socket with the env's `TMUX_TMPDIR`. The teardown's `stop_tmux` globs `tmux-*/cpdt-...` and never sees `.../tmux-503/cpd`; the server is out of the group (setsid by tmux), its pane `cpd session run <uuid>` is not a recorded driver. The env directory is removed (Drop worked: all 34 cwd dirs are gone) but the server stays with a deleted cwd. It is the same on Linux. It is also a real tmux in a test, against the rule. The five tests are not on main (`git log --all -S provider_flow` -> 7578be55, branch `cpd/bifrost/provider`): the Plan `bifrost` Job that adds them must either give them `write_tmux_config` or (the migration) a fake tmux.

Where the Gate's leak check missed it: it never ran. These were targeted runs of a Worker (`cargo nextest run -p cpd --test flow`), with no `GATE_RUN` in the env of the servers (**verified**: `ps eww` shows none). Had it been a Gate, the check would have seen them: Homebrew tmux shows its environment, `GATE_RUN=<id>` would match (and the paths in `$RUN`).

### Class B: `tui_idle` (real tmux, `new-session` through `TestEnv::tmux`)

- Verified: the tests use `env.tmux_socket()` (`tui_idle.rs:37-48`), so the socket is `cpdt-<pid>-<n>` and the current teardown would find and kill it; the server command has no `-u -f /dev/null`: it is `tmux -L <sock> new-session` run by the test. `TMUX_TMPDIR` is `<env>/tmux` (a short in-env path), where on current main `env_tmux_tmpdir` returns `/tmp` for a macOS TMPDIR (path 118 > 100 with `DERIVED_SOCKET_ROOM` 20; computed on `/private/var/folders/7_/<26>/T/cpd-test-XXXXXX/tmux/tmux-503/cpdt-NNNN-N`): so these ran code without the room.
- Verified: they started 22:12-22:26 on Oct 22, 10 h before #278 (09:00 Oct 23); on the pre-#278 harness the macOS branches of `alive`/`mine` did not exist. `alive` read `/proc/<pid>/stat` (absent on macOS), so the reaper's `while [ -d $dir ] && alive $2` ended at once or never. Which of the two is **unverified** (the worktrees `cpd.bifrost--cyan-impl-a1` and `cpd.gatemac--g3-tests-proc-a1` are deleted and the test binary is gone).
- Unverified, a candidate that still applies on main: `tui_idle` measures CPU over a window (`window()`) in a real tmux; under load 70-120 a test at the 30 s x 3 limit is killed by nextest (no Drop); only the reaper cleans up, and it polls with `sleep 1` at load 100.
- Missed by the leak check: not a Gate run (no `GATE_RUN`, **verified**).

### Class C: `GATE_RUN` servers of Oct 22 ~15:55-16:00

- Verified: tmux copies and Homebrew tmux show `GATE_RUN=cJSJxt` / `OWtMqg` in their environment, so `run_pids` (`ps -axww -E` and ` GATE_RUN=<id>`) would list them *if `finish` ran*. Their TMPDIRs (`/tmp/cg/gate-run.cJSJxt`, `/tmp/g1/gate-run.OWtMqg`) are gone now (neither directory exists), so something removed them without killing the servers.
- Unverified: the most likely cause is a Gate wrapper killed before `finish` (a SIGKILL of `gate-run.sh`, which has no trap), e.g. cpd's Gate timeout; with the wrapper dead, nothing runs the check, and the tests' own Drop/reaper did not clean (pre-#278 code, as B). `gate-run.sh` has no `trap` for TERM/INT: **verified** (read; its `finish` runs only after `"$@"` returns).
- The `bare::` tests start the "older tmux server" by copying tmux to `<env>/old-tmux/tmux`; this server is on a `cpdt-<GATE_RUN>-<pid>-0` socket so the teardown would find it in `/tmp` or the env; they ran before the macOS teardown. Unverified.

### Class D: the harness's own driver shells

- Verified: still looping, never received SIGTERM (the trap would start a tmux server and exit; no `sleep 100 # <dir>` server exists). They were not stopped by the teardown: the driver is a `setsid` process, stopped only if recorded (`record_driver`) and `mine`. On macOS Apple `/bin/sh` hides its environment (`ps -E` shows the command only, **verified** on pid 21029), so before #278 `mine` (CPD_HOME in the environment) was false for every one of them; the command-line fallback ("the command holds the env dir") is the #278 fix, and all eight started before #278 (Oct 22 22:57-23:01).
- Verified: two per Gate run and two in the same second in the targeted runs: the two tests that call `put_up` (`a_dropped_env_...`, and the helper of `a_killed_test_...`), so they leaked every time, not by load.
- Unverified: whether the current harness stops them; it needs a run of `harness::a_dropped_env...` on this Mac, which starts a real tmux and is excluded from this Job. The test would have failed in those Gates (`leftovers_after_wait` lists the `driver` pid), so these are the runs of a red Gate/worker.
- Missed by the leak check: **verified** in two ways. (1) `ps -axww -E` shows no environment for Apple `/bin/sh`, so `grep " GATE_RUN=<id>"` cannot match a shell, on any Gate. (2) The sockets are named `cpdt-abcdef-<pid>-0` while the Gate's directory ids are `q9mc3i`, `MD6wqQ`, `quNFPo`, `3U7Itu`: the socket scan `"$d"/*-"$GATE_RUN"-*` cannot match either. Why the tests used `abcdef` is unverified (the string is in no file of main: `rg abcdef`; a test or a wrapper in those worktrees set `GATE_RUN=abcdef`, but `gate-run.sh` exports its own id).

### Class E

Verified: a manual `-vv` server of a Worker's debug session (`/private/tmp/tmuxlog`), with `TMUX_TMPDIR=/tmp`, on `cpdt-3348-0` in `/tmp/tmux-503`; started 01:21, no `GATE_RUN`. The test (`tui::tui_marks_a_restart...`) was run by hand with a `tmux` wrapper; the harness teardown would only find it if its socket were in the env's `TMUX_TMPDIR` (`/tmp` here, so the `cpdt-3348-0` file is not in `$tmuxdir` of a test whose dir is gone). Unverified beyond that.

### Also found: 53 stale sockets

`/tmp/tmux-503/cpdt-abcdef-<pid>-<n>-outer-*` (the `bare.rs`/`tui_merge.rs` derived sockets `<socket>-outer-<name>`), all with no server behind, from Oct 22 (e.g. 23:15). Their teardown glob is `$tmuxdir/tmux-*/$socket-*`, which removes them only when `$tmuxdir` is the same directory as the one tmux used; they remained. Same cause as D (`abcdef`).

## Step 3: cleanup
