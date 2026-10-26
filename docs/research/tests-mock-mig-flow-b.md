# In-process migration of the flow tests, second half (Job `mr-flow-b`)

Docs only. Base `885fa97a`. Everything below is read from the code at that commit (`rg`, `read`) or from the five documents in `~/.cpd/ref/tests-mock/`; no test and no build was run. **verified** = read at the path named; `unverified` / `guess:` = inferred. `Runner`, `FakeRunner`, `Ports`, `run_with`, `testkit`, `FakeSpawn`, `ManualClock` do not exist on this commit: their names and behaviour are those of the ref documents (`unverified` here).

Scope: `cli/tests/flow/` minus `ask attempt config context* driver*`: 23 files, **300 tests today** (`rg -c '#\[test\]'`), plus **28 deleted tests** of the same scope (`tests-mock-deleted.md`, rows 95-122), all in files of the scope (`repo_cmds.rs` no longer exists).

Seam names used: `run_with` (the CLI entry in-process, ports-1), `testkit` (the `Fx`/`TestEnv` over `Fakes`), `runner` (a *scripted* `FakeRunner` route: a reply per program, stdin bytes, exit codes, `NotFound` for an unrouted program; the recorded call log alone is part of `run_with`, because `Ports` always holds a `FakeRunner`), `tmux-route`, `git-route`, `spawn` (`FakeSpawn`: `launches()`, `exit`), `clk` (`ManualClock`). A test that only calls a library function needs no seam of its own beyond the fake it names.

## 1. Inventory

"Starts today" is what the test file starts; every file but `metrics.rs`, `spec.rs` runs the built `cpd` binary through `TestEnv::cpd` (a `Command::new(CPD)` per call, `cli/tests/common/mod.rs`) and, when `TestEnv::new` is used, a store in a temp `$CPD_HOME`. Target: U = no seam, I = in-process on `Ports`, S = smoke with the real binary. **S: none proposed for this scope** (ADR 0013 "Smoke tests" lists `--help`, `--version`, `watch --check` and one e2e, all outside it). The ADR's per-file table calls `policy*`, `plan_show`, `plan_render_*`, `repo_config`, `secret_guard`, `metrics`, `spec` "unit": only `metrics.rs` and `spec.rs` are (they call `cpd_core` functions and start nothing); the others call `env.cpd(..)`, so they are **I** on `run_with`.

| File | Tests | Starts today | Target |
|---|---|---|---|
| `job.rs` | 73 | `cpd` binary (47 `env.cpd` sites in 69 tests, store rows by SQL); `env.fake_pi("exit 0")` (an empty script that must exist, `setup` l.56, the tier catalog is cached so pi never runs); **4 tests** (`touches_fx` l.2097: `env.git_repo`, `git config/add/commit` → `git ls-tree` from `check_touches`, `core/src/job/add.rs:314`) start real git | I |
| `metrics.rs` | 8 | nothing (`cpd_core::` parse of a file in a temp dir) | U |
| `plan_cancel.rs` | 1 | the binary only; `an_unknown_plan_is_plan_not_found` refuses before any tmux/gh/git call (its `setup` is the 11 deleted tests' world: tmux config, fake `cpd` panes, keyed repo) | I |
| `plan_delete.rs` | 4 | binary; `plan new` (real tmux, fake `cpd` pane script); a real bare origin and `git push`/`worktree add -b` for the Lead and Attempt worktrees (`world_ending` l.67-93); `git branch --format` | I |
| `plan_new_by_user.rs` | 2 | binary; real tmux cockpit (`new-session`, `split-window`); a fake `pi` shell script that `exec sleep`s and is polled with `wait_until("the fake pi")` (`pi_argv`) | I |
| `plan_outline.rs` | 20 | binary (31 sites) on a store filled by SQL; nothing else | I |
| `plan_overlap.rs` | 12 | binary (`plan render`, `plan review`) on SQL rows; nothing else | I |
| `plan_render_cancelled.rs` | 4 | binary on SQL rows + `job cancel`; nothing else | I |
| `plan_render_waiting.rs` | 2 | binary through `plan_render_cancelled::{env,render}`; nothing else | I |
| `plan_show.rs` | 10 | binary (`plan show/list/render`) on SQL rows | I |
| `plan.rs` | 45 | binary (184 sites); real tmux in all 45 (`plan new` opens the cockpit/window; `env.tmux(list-panes/display-message/set -p/show -pqv/kill-server/rename-window)`, `pane_env` polls a file the fake pane script writes: `wait_until` l.89); **33** also real git (`keyed_repo`, `with_origin` bare origin, `git push/ls-remote/config`, hook `pre-receive` in the bare origin l.~1010); **13** run `plan lock` (`ensure_watch` spawns `cpd watch --ensure`, `core/src/plan/lock.rs:185`, answered by the fake `cpd` shell script, `WATCH_OK`/`WATCH_FAILS`); 5 pass `CPD_PLAN` by `cpd_with` (l.49); `env.fake("tmux", ..)` wrapper in 1 (`new_leaves_no_window_..`) | I |
| `policy.rs` | 14 | binary; 4 tests (`show_as`, l.~71) set `CPD_SESSION`/`CPD_ATTEMPT` on a raw `Command` | I |
| `policy_fallback.rs` | 15 | binary; symlinks in a temp dir (real FS, allowed); **5** (`pr_fx`, f10, f11) real git: `git_repo`, `update-ref`, `remote add` | I |
| `policy_scopes.rs` | 18 | binary (`policy show`, `config`) on files in a temp home | I |
| `repo_config.rs` | 6 | binary (`policy show` of repo files in a temp home) | I |
| `review.rs` | 1 | binary (`plan review` refusals) | I |
| `review_close.rs` | 4 | binary; `planned()` = real tmux (`plan new`), fake `cpd` pane script, `keyed_repo`, `pr add`, `plan phase` | I |
| `review_job.rs` | 7 | binary; `fx()` = real git repo + `cpd_core::git::create_lead_worktree` called in the test process; **3** (`the_worker_is_a_fork_..`, `the_worker_starts_only_once_the_lead_is_idle`, `a_cancel_while_the_driver_waits_..`) dispatch: a detached driver, a real tmux Lead pane, a fake `pi` shell script (`REVIEWER`, l.~219: `git add/commit/reset` in the pane), `wait_status`/`wait_end` polls, `CPD_TEST_*_POLL` overrides (l.28) | I |
| `review_plannotator.rs` | 8 | binary (17 sites) on a Session row written by SQL; nothing else | I |
| `secret_grants.rs` | 6 | binary with `CPD_SESSION` per call (`Fx::run`); a fake `op` script on `PATH` used only to assert it was never called | I |
| `secret_guard.rs` | 9 | same as `secret_grants.rs` | I |
| `secret_op.rs` | 17 | binary; a fake `op` shell script that logs argv/stdin (`Fx::op_says`, `op_log`); 1 test removes `op` from `PATH` (`a_missing_op_is_secret_op_missing`); 1 runs the `sqlite3` CLI (`secret_commands_write_nothing_to_the_store`, `sqlite3 <db> .dump`) | I |
| `spec.rs` | 14 | nothing (`cpd_core` spec parse/write in a temp dir) | U |
| **Sum** | **300** | | |

One direct sleep: `review_job.rs:370` (`std::thread::sleep(POLL * 5)` in `the_worker_starts_only_once_the_lead_is_idle`, to prove nothing starts: a `ManualClock` `advance` of 5 polls). The other sleeps are `wait_until` polls (`plan.rs` `pane_env`, `plan_new_by_user.rs` `pi_argv`, `review_job.rs` `wait_status`/`wait_end`) and the engine's own `sleep`/`Instant` in `plan/cancel.rs:111-125` (`wait_until_ended`).

## 2. Seams

### 2.1 Minimum set per file

Tests of one file differ; where they do, the file is split (counts verified by reading each test and the helpers it calls, `plan.rs` transitively by script over its helper graph).

| File | Seams (minimum) | Why / what is not needed |
|---|---|---|
| `metrics.rs`, `spec.rs` | none (library call) | call `cpd_core` and a temp dir (real FS is allowed) |
| `job.rs` 69 | `run_with`, `testkit` | SQL rows; `fake_pi` only has to exist (a file) |
| `job.rs` 4 (`touches_fx`) | + `git-route` | `check_touches` runs `ls-tree -r --name-only -z` (`job/add.rs:314`) after `Path::is_dir()` of the repo (l.307): the fake repo must also be a real directory, or that check moves behind the `Runner` (decision, `unverified`) |
| `plan_outline`, `plan_overlap`, `plan_show`, `plan_render_cancelled`, `plan_render_waiting`, `policy`, `policy_scopes`, `repo_config`, `review`, `review_plannotator` | `run_with`, `testkit` | SQL fixtures + the CLI; `policy.rs`'s 4 `show_as` tests need per-call env (`CPD_SESSION`, `CPD_ATTEMPT`) in `run_with` (`unverified` how `run_with` takes env) |
| `secret_guard`, `secret_grants` | `run_with`, `testkit` | the fake `op` is only a "was not called" probe: the empty call log of the always-present `FakeRunner` says the same |
| `secret_op` 17 | `run_with`, `testkit`, `runner` | `secret/op.rs:48-65` is `Command::new("op")` through `deadline::output` (one of the nine callers of PR `port-run`, ports doc §3); needs a reply per `get/list/create/edit/delete`, the stdin bytes (the secret) recorded, and `NotFound` for the missing-`op` test. No `clk`: the approval expiry test backdates the file against `OffsetDateTime::now_utc()` (`secret/grants.rs:180`, `secret/mod.rs:300`) so it is deterministic on real time (`Clock::utc` would only be nicer, ports doc: "Missing: a wall clock") |
| `policy_fallback` 10 | `run_with`, `testkit` | f1-f9: symlinks in a temp dir |
| `policy_fallback` 5 | + `git-route` | f10/f11: `git init`, `update-ref`, `remote add`, read by `policy/origin.rs` (`config --get remote.origin.url`, `remote`) |
| `review_job` 4 | `run_with`, `testkit` | `add --review` on a PR whose Lead worktree row is written by SQL; `check_touches` returns early for no `touches`: `fx()` starts git only as a convenience (`unverified` that these 4 do not read the worktree: split `fx()` into store-only and git) |
| `review_job` 3 | `run_with`, `testkit`, `tmux-route`, `git-route`, `spawn`, `clk` | dispatch starts a driver (`FakeSpawn`), a Lead pane (route), the worktree/merge git calls (`worktree add`, `rebase`, `update-ref`, `branch -D`: git doc §3), and the driver's poll (`CPD_TEST_DRIVER_POLL` → `ManualClock`); the reviewer script becomes test-side calls (`attempt submit`, route-side commits) |
| `plan_cancel` 1 | `run_with`, `testkit` | refuses on an unknown Plan before any external call |
| `plan_new_by_user` 2 | `run_with`, `testkit`, `tmux-route`, `spawn`, `clk` | `plan new` creates windows (route); the pane's `cpd session run` becomes a test-side `run_with(["session","run",id])`, whose pi launch (`session/run.rs:180-188`) is a `FakeSpawn` launch the test reads with `launches()` instead of polling a script (loops of `session/run.rs`: `clk`, ports doc §2, `unverified` here) |
| `plan.rs` 12 | `run_with`, `testkit`, `tmux-route` | `new*`, `rename*`, the dynamic-title test, `a_plan_with_no_pr_..`: `plan new` + `list-panes`/`display-message`/`set -p`; `setup()` makes a repo they never use |
| `plan.rs` 20 | + `git-route` | the 4 `archive*`/`unarchive` tests via `plan_with_fork` (`new_plan` with a repo), `pr add`, `plan review/decide`, `approved()` without `plan lock`: `ls-remote`, `worktree`, `config` |
| `plan.rs` 13 | + `runner` | tests that call `plan lock`: its `ensure_watch` runs `cpd watch --ensure` (`plan/lock.rs:185`, a `Command` that is not in `port-run`'s nine: needs its own job, ports doc), a scripted reply (`WATCH_OK` json, `WATCH_FAILS`, "nonsense" stdout); git side adds `merge-tree --write-tree` (the conflict test l.1158 needs a real conflict result: git doc §3 "abort criterion"), `push`/`ls-remote` with a remote |
| `review_close` 4 | `run_with`, `testkit`, `tmux-route`, `git-route` | `planned()` = `plan new` + `pr add` + `plan phase`; no `plan lock`, no engine call. **Lever**: build that state by SQL as `plan_outline.rs` does and the 4 drop to `run_with`, `testkit` (`unverified` that `plan review` needs nothing from the tmux pane) |
| `plan_delete` 4 | `run_with`, `testkit`, `tmux-route`, `git-route` | `world_ending` = `plan new` (tmux) + bare origin + 2 worktrees; `plan delete` runs `worktree remove`, `branch -D/-d`, removes real directories (`plan/delete.rs`) — the route must create the directories the product `remove_tree`s (git doc "What the product also reads off disk") |

Cross-cutting facts (all read): (a) per-call env vars reach the CLI as the process environment today (`policy.rs` `show_as`, `secret_*` `Fx::run`, `plan.rs` `cpd_with` 5 tests, `review_job.rs` `CPD_TEST_*_POLL`): `run_with` must take the env as an argument, and `std::env::var("PLANNOTATOR_PORT")` in `plan/lead.rs:48` and `review_host` (l.53) read the *test process's* environment, so the 1 plan test that sets them (`plan.rs` l.~196-205) needs a `run_with` env parameter or the read moves into `Env`; (b) the pane's environment: `plan/lead.rs:42-57` builds `Spawn { cwd, env, argv }` and `host/tmux.rs:411-427` passes it as `-c` and `-e K=V` to `new-session/new-window`; the test reads it today from `env > pane-<id>.env` written by a real pane process (`pane_env`). In-process it must read what the route recorded for `-e`/`-c`: whether `FakeTmux` stores them is `unverified` (tmux doc lists `pane_current_path` and `-e` handling, not that it keeps the env); (c) `host::Spawn` (tmux window spec) and the new `Spawn` trait of `ports` share a name.

### 2.2 Needs-set table

"Entry" separates what needs `run_with` from what is a library call. The 300 tests sum exactly.

| # | Needs-set | Entry | Files | Tests |
|---|---|---|---|---|
| A | { } | library call | `metrics` 8, `spec` 14 | 22 |
| B | { `run_with`, `testkit` } | `run_with` | `job` 69, `plan_outline` 20, `plan_overlap` 12, `plan_show` 10, `plan_render_cancelled` 4, `plan_render_waiting` 2, `policy` 14, `policy_scopes` 18, `policy_fallback` 10, `repo_config` 6, `review` 1, `review_plannotator` 8, `secret_guard` 9, `secret_grants` 6, `review_job` 4, `plan_cancel` 1 | 194 |
| C | { `run_with`, `testkit`, `runner` } | `run_with` | `secret_op` 17 | 17 |
| D | { `run_with`, `testkit`, `git-route` } | `run_with` | `job` 4, `policy_fallback` 5 | 9 |
| E | { `run_with`, `testkit`, `tmux-route` } | `run_with` | `plan` 12 | 12 |
| F | { `run_with`, `testkit`, `tmux-route`, `git-route` } | `run_with` | `plan` 20, `review_close` 4, `plan_delete` 4 | 28 |
| G | { `run_with`, `testkit`, `tmux-route`, `git-route`, `runner` } | `run_with` | `plan` 13 (`plan lock`) | 13 |
| H | { `run_with`, `testkit`, `tmux-route`, `spawn`, `clk` } | `run_with` | `plan_new_by_user` 2 | 2 |
| I | { `run_with`, `testkit`, `tmux-route`, `git-route`, `spawn`, `clk` } | `run_with` | `review_job` 3 | 3 |
| | | | **Total** | **300** (22 + 194 + 17 + 9 + 12 + 28 + 13 + 2 + 3) |

Which PR of the Plan unblocks which row (names from the ADR/ports/route documents, `unverified` here): A needs none; B needs `ports` (+`clk`, `port-run`, `port-tmux`, `port-git` only because `Ports` is built from them) and `testkit` tk-1 (the `Fx` over `Fakes`, and `TestEnv::cpd` dispatching to `run_with`); C adds the `port-run` job for `secret/op.rs`; D, F, G add the `git-route` jobs (`rev-parse remote config symbolic-ref ls-remote ls-tree`, then `worktree/branch/update-ref`, then `fetch/push/merge-tree/merge-base`, git doc §6); E, F, G add the `tmux-route`; G also needs the `runner` job for `plan/lock.rs:185`; H, I add `spawn` (ports-2) and `clk` (ports-4: `session/run.rs`, `driver`).

The deleted tests add to these rows (section 3): `plan` 5 → E/F; `plan_delete` 3 → F; `plan_cancel` 11 → J below; `repo_cmds` 6 and `repo_config` 1 → K; `review_job` 2 → L.

## 3. The deleted tests of the scope (28)

Read from `adb53fc8^` (the version before the deletion), by reading each body and helpers. "Subject" says what it proves; the seam is the minimum for that subject.

| Test (file) | Subject | Seam |
|---|---|---|
| `plan.rs::archive_refused_by_any_one_blocker_changes_nothing` | 5 loops over `plan archive` refused by a queued/running Job, an open Escalation, a pending review, a working fork; no event, no pane closed (`plan_with_fork`) | F: `run_with`, `testkit`, `tmux-route`, `git-route` |
| `plan.rs::lock_reuses_a_remote_lead_branch_that_lacks_a_base_that_merely_moved_ahead` | `plan lock` reuses a remote branch behind the base (`remote_lead_branch_then_main_moves`) | G + `merge-base --is-ancestor`, `merge-tree`, remote refs (git doc §3) |
| `plan.rs::plans_moving_to_phase_plan_at_once_all_move` | 6 `plan phase plan` on 6 threads behind a `Barrier`, each spawning a binary; the subject is concurrent writes of one SQLite store | F (6 `plan new` first); the threads call `run_with` on one temp-dir store (see section 5) |
| `plan.rs::the_leads_pane_carries_the_plans_title_and_plan_rename_keeps_it_current` | `@cpd_title` set/unset on panes (`show -pqv`, `set -p -u`, `kill-pane`) | E (`set -p`/`show` on panes: route, `unverified` that `-u` is kept) |
| `plan.rs::unarchive_clears_the_cascade_with_one_event_each_and_touches_no_pane` | `plan unarchive`: 3 events, no pane started | F |
| `plan_cancel.rs::cancels_jobs_closes_open_prs_and_the_lead_and_archives_the_plan` | full cancel: Jobs, `gh pr close`, Lead pane killed, archive | J |
| `plan_cancel.rs::reason_defaults_to_plan_cancelled` | default reason text | J |
| `plan_cancel.rs::a_rerun_writes_nothing_and_calls_nothing` | second cancel writes no event, calls no `gh` | J (call log of `gh`) |
| `plan_cancel.rs::a_rerun_finishes_what_a_failed_run_left` | rerun after a failed `gh pr close` | J (scripted `gh` failure) |
| `plan_cancel.rs::a_failing_job_cancel_halfway_is_finished_by_a_rerun` | a bad row (running Job with no Attempt) fails `job cancel` midway; the rerun waits for the engine to end 2 Attempts (`spawn_cancel` child + `wait_until` + `engine_ends_attempts`) | J + `clk`: `plan/cancel.rs:111` loops on `Instant::now()` + `sleep(every)`: run on a thread, `advance` per poll, end the Attempts by SQL between polls |
| `plan_cancel.rs::a_plan_with_nothing_left_to_cancel_is_only_archived` | archive only | J |
| `plan_cancel.rs::a_tdd_pr_with_its_last_test_and_impl_jobs_does_not_stop_a_plan_cancel` | shape check ignored by cancel | J |
| `plan_cancel.rs::a_plan_cancel_stops_a_running_last_impl_job_of_a_tdd_pr_without_the_shape_check` | 2 stop requests then engine ends Attempts | J + `clk` (as above) |
| `plan_cancel.rs::a_pending_plan_review_is_superseded_and_the_cancel_goes_on` | pending review → `superseded` | J |
| `plan_cancel.rs::the_lead_in_the_chat_slot_blocks_and_nothing_is_written` | refusal when the Lead is in `main` | J (window named `main`, `rename-window`) |
| `plan_cancel.rs::a_gh_failure_for_a_pull_request_already_closed_is_not_a_failure` | `gh pr close` fails, `gh pr view` says `MERGED` | J (scripted `gh` replies) |
| `plan_delete.rs::deletes_rows_files_and_worktrees_of_an_archived_plan` | rows, run dir files, 2 worktrees, local branches gone | F |
| `plan_delete.rs::refuses_with_live_work_or_an_unmerged_pr` | refusals | F |
| `plan_delete.rs::tamper_check_stays_clean_after_a_delete` | `Store::tamper()` after delete; raw `rusqlite` `DELETE` refused | F (the store part is a library call) |
| `repo_cmds.rs::a_cpd_toml_in_the_repo_tree_does_not_configure_the_test_commands` | a `cpd.toml` in the repo tree is ignored | K: `run_with`, `testkit`, `git-route` |
| `repo_cmds.rs::a_diff_touching_a_protected_path_of_the_repo_file_is_tampering` | `git diff --name-only` against the protected paths | K (`tamper.rs`: `diff --name-only --no-renames -z a...b`) |
| `repo_cmds.rs::a_diff_touching_wt_toml_or_cpd_toml_is_not_tampering` | same, not protected | K |
| `repo_cmds.rs::a_test_job_submit_builds_and_runs_with_the_test_commands_of_the_repo_file` | `attempt submit` runs the repo file's `build`/`test` commands (`sh -c` in the worktree, `hooks/exec.rs:113`) | K + `runner` (hook reply, argv/cwd read from the call log: see section 5) |
| `repo_cmds.rs::an_impl_job_submit_runs_the_named_tests_with_the_run_command_of_the_repo_file` | same, the `run` command | K + `runner` |
| `repo_cmds.rs::pr_add_of_a_tdd_pr_takes_the_test_commands_from_the_repo_file` | `pr add --shape tdd` reads the repo file | K |
| `repo_config.rs::pr_add_of_a_repo_without_a_file_is_repo_unconfigured` | `pr add` refusal; its `setup` starts tmux needlessly | K (git-route only; drop the tmux fixture) |
| `review_job.rs::a_normal_job_without_commits_is_still_refused` | `attempt submit` of a normal Job with no commit → refusal, polls the driver | L: I-set (`spawn`, `clk`, `git-route`, `tmux-route`) |
| `review_job.rs::submit_without_commits_is_a_review_jobs_privilege_and_it_needs_its_findings_and_no_commit` | review Job may submit with no commit, needs findings | L |

Sets: **J** = { `run_with`, `testkit`, `tmux-route`, `git-route`, `runner` (`gh`) } and `clk` for the 2 waiting tests; **K** = { `run_with`, `testkit`, `git-route` } (+ `runner` for 2); **L** = set I. Rows above: 5 (`plan`) + 11 (`plan_cancel`) + 3 (`plan_delete`) + 6 (`repo_cmds`) + 1 (`repo_config`) + 2 (`review_job`) = 28.

## 4. Proposed migration PRs (4)

The Plan's PRs are fixed at 4 maximum; 300 + 28 tests cannot fit 15-60 tests in 4 PRs (max 240), so the counts are *verified in-process* tests. Most of B only change by the `TestEnv::cpd` → `run_with` flip, which belongs to `testkit` tk-1 (`unverified` that one helper flip is enough; the files whose tests build a raw `Command` are named in each PR). Edited-test estimates are in the Job lines. Order: fewest seams first.

**`mrfb-1`**: needs {`run_with`, `testkit`}; files `metrics`, `spec`, `plan_outline`, `plan_overlap`, `plan_show`, `plan_render_cancelled`, `plan_render_waiting`, `policy`, `policy_scopes`, `repo_config`, `review`, `review_plannotator`, `secret_guard`, `secret_grants`, `plan_cancel` (rows A and B minus `job`, `policy_fallback`, `review_job`; the `job` 69 are in `mrfb-2`, the 4 store-only `review_job` in `mrfb-4`): **133 tests** (22 + 111).
1. docs: the per-file migration table for the PR (what changes per test; nothing else).
2. test: a failing in-process test of `run_with(["policy","show"])` with `CPD_SESSION`/`CPD_ATTEMPT` as env (the 4 `show_as` tests of `policy.rs`), and of `secret_guard`'s "op not called" through the call log.
3. impl: `policy.rs` `show_as` and the `Fx` of `secret_*` on `run_with` (≈ 4 + 15 tests edited); `env()` of `review_plannotator`, `repo_config`, `plan_cancel` on the `testkit` env.
4. impl: `plan_outline`, `plan_overlap`, `plan_show`, `plan_render_*`, `policy_scopes`, `review`: nothing but the helper flip (verify they pass; 0 edits expected).
5. impl: delete the fake `op` probe of `secret_guard`/`secret_grants` (`Fx::new` writes an `op` script on `PATH`), replaced by the call log.

**`mrfb-2`**: needs {`run_with`, `testkit`, `runner`, `git-route`}; files `job` 73, `secret_op` 17, `policy_fallback` 15 + restored `repo_cmds` 6, `repo_config` 1: **112 tests**.
1. docs: scripted-op reply table (`get/list/create/edit/delete`: argv, stdin, exit codes) and the hook-reply convention for `attempt submit`.
2. test: failing in-process `secret_op` tests: `get` returns the scripted value, stdin of `create/edit` is the secret and appears in no store row, missing `op` = `NotFound` → `secret_op_missing`.
3. impl: `secret_op.rs` on `runner`; replace the `sqlite3 .dump` of `secret_commands_write_nothing_to_the_store` by reading every table through the test's own `rusqlite` handle (`sqlite3` is an external CLI) (17 edited).
4. impl: `job.rs` 4 `touches_fx` tests (`git-route`: `git_repo` + commit + `ls-tree`; a real dir for the repo path) and `policy_fallback` 5 `pr_fx` tests (15 edited).
5. test+impl: restore the 6 `repo_cmds` and 1 `repo_config` tests from `adb53fc8^` as in-process (K): hook reply and `diff --name-only` tamper lists from the route.

**`mrfb-3`**: needs {`run_with`, `testkit`, `tmux-route`, `git-route`, `runner`}; files `plan` 45, `review_close` 4, `plan_delete` 4 + restored `plan` 5, `plan_delete` 3: **61 tests** (E 12 + F 28 + G 13 + 8 restored).
1. docs: how the Lead pane's `-c`/`-e` and `@cpd_*` options are read from the route (replaces `pane_env`), and the `cpd watch --ensure` scripted replies.
2. test: failing in-process tests for `plan new` (window, pane id, `-e CPD_HOME CPD_SESSION CPD_PLAN`, cwd) and `plan lock` with `WATCH_OK`/`WATCH_FAILS` replies.
3. impl: the 12 tmux-only `plan.rs` tests and `review_close` (4) on `testkit` (`planned()` by SQL if `plan review` needs no pane, section 2.1); remove the `env.fake("tmux", ..)` wrapper of `new_leaves_no_window_..` by a scripted route failure for `set @cpd_session`.
4. impl: the 20 git `plan.rs` tests and `plan_delete` 4 on `git-route`; replace the `pre-receive` hook of `a_push_failure_at_lock_..` by a scripted `push` reply (exit 1, stderr "refused by hook").
5. impl: the 13 `plan lock` tests; `lock_refuses_..._conflicts_..` needs `merge-tree` conflict output (git doc abort criterion).
6. test: restore the 5 `plan.rs` and 3 `plan_delete.rs` deleted tests.

**`mrfb-4`**: needs {`run_with`, `testkit`, `tmux-route`, `git-route`, `runner`, `spawn`, `clk`}; files `plan_new_by_user` 2, `review_job` 7 (its 4 store-only tests share `fx()`; they are in set B), `plan_cancel` restored 11, `review_job` restored 2: **22 tests** (9 of the 300).
1. docs: the `ManualClock` + thread recipe for `plan cancel` and the driver (`advance` per poll, end Attempts by SQL between polls).
2. test: failing `plan_new_by_user` tests reading `FakeSpawn::launches()` for pi's argv after `run_with(["session","run",id])`.
3. impl: restore the 11 `plan_cancel` tests (`gh` replies: `pr close`, `pr view`; `clk` for the 2 waiting ones).
4. impl: `review_job` 3 + restored 2 (reviewer's commit/reset done by route calls; `attempt submit` in the test).
5. impl: remove `CPD_TEST_DRIVER_POLL`/`CPD_TEST_KILL_PAUSE`/`CPD_TEST_LEAD_POLL` from the product once the three consumers (`driver*`, `review_job`, the other half) use `ManualClock` (coordinate with the first-half Lead; `unverified` count of consumers).

## 5. Not migratable as asked

No test of the scope has an OS behaviour of an external as its subject (no signal, pty, flock of an external, `exec`, installed binary). Seven change *how* the subject is observed; each keeps policy [tests]:

| Test | Today | Proposal |
|---|---|---|
| `secret_op.rs::secret_commands_write_nothing_to_the_store` | `sqlite3 <db> .dump` (external CLI) | read every table through `rusqlite` in the test and assert the secret is in none of them (the store is ours; `rusqlite` is a library) |
| `plan.rs::a_push_failure_at_lock_still_locks_and_appends_one_push_failed_error` | a `pre-receive` hook in a real bare origin: git's own hook behaviour | a scripted `git push` reply (exit 1, stderr `refused by hook`); the subject (`push_failed` event, the Plan still locked) is ours |
| `plan.rs` (`pane_env` users: `new_registers_the_plan..`, `lock_*`, `plan_with_fork` helpers) | the environment of a real pane process | assert the `-e`/`-c` the product passed to `new-window` (`plan/lead.rs:42-57`), read from the route: the subject becomes "cpd asked tmux for this env" (tmux inherits it, external) |
| `plan_new_by_user.rs` (2) | argv of the real fake-`pi` | `FakeSpawn::launches()[0].argv` after `run_with(["session","run",id])` |
| `repo_cmds.rs` restored `a_test_job_submit_..`, `an_impl_job_submit_..` | the repo file's commands really run through `sh -c` | assert argv and cwd of the hook call in the `FakeRunner` log and the reply it maps to (shell execution is an external) |
| `plan.rs::plans_moving_to_phase_plan_at_once_all_move` (restored) | 6 processes race on the SQLite file lock/busy timeout | 6 threads calling `run_with` on one temp-dir `Store` behind a `Barrier` (no sleep); if `store/lock.rs` behaves differently between threads than between processes (`unverified`), keep it in the store area (`cli/tests/store`) as a store test, not in flow |
| `review_job.rs` `REVIEWER` script (`git add/commit/reset` in a pane) | a shell run by a real tmux pane | the test makes the same commits through the git-route handle and calls `attempt submit` itself |

Open decisions for the Lead (not blockers): the repo directory must exist for `check_touches` (`job/add.rs:307`) and `plan/delete.rs` (a real dir made by the fixture, or an `is_dir` through the `Runner`); how `run_with` receives the per-call environment (section 2.1); whether `FakeTmux` keeps `-e`/`-c` of `new-window` for later reads; whether `ManualClock::sleep` blocks a thread until `advance` (ports doc §"events follow, ask" says so) or has a per-nap hook.
