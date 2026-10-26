# Migrating the session tests in-process (Job `mr-session`)

Scope: `cli/tests/session/*.rs` (17 files, 165 tests; `main.rs` has none) and the in-crate tests of `core/src/session` and `core/src/driver` that spawn a process or sleep (9 tests, §1.2). Read from the code with `rg` and `read`; no test or build was run. Names of `Runner`, `FakeRunner`, `Ports`, `run_with`, `testkit`, `ManualClock`, `FakeSpawn` come from `0013-tests-in-process.md` and `tests-mock-ports.md` and are `unverified` against code (none is on main). `tmux-route` is a route of `FakeRunner`, so it includes `runner`; "runner" below means a handler of another program (`pi`, `<cpd> --version`) or call recording without a tmux route.

Reading the code gave four facts that decide the plan:

1. **One construction site of tmux in the session code:** `Env::tmux()` (`core/src/session/mod.rs`, `Tmux::new(socket)`). It is called by `archive`, `unarchive`, `history`, `fork`, `open`, `park`, `close`, `rename` (`row.rs retitle`), `retitle` (`set_header`) and `run` (`check_pane`). The commands that never call it are `list`, `show`, `state`, `send`, `resume`, `retier`, `first-message`, `report`, `board`. Refusals come first in each tmux command (`fork`: `not_a_lead`, `plan_not_found`, `file_unreadable` precede `has_session`; `archive_lead`: `plan_lead` and `blockers` precede `env.tmux()`), so a refusal test is store-only; a successful `rename`, `retitle` or `archive` still runs `list-panes` today (a real `tmux` exec that fails with no server), so it needs the route even with no cockpit.
2. **The pane command is never run by a fake tmux:** `moves.rs` builds `["cpd","session","run",<id>]` from a constant `PROGRAM`, not from `Exe::current()` (that one is read in `session/run.rs:56`). `fake_cpd_views`, `fake_pi` and `cockpit(...)` therefore become records of what `new-window`/`respawn-pane` got; `fork`/`open`/`history`/`close` need no `spawn`. Only the wrapper (`session run`) needs `spawn` and `clk`.
3. **`session run` waits on a terminal and on time** (`session/run.rs`: `thread::sleep(file_poll)`, `patiently` backoff, `SystemTime::now()` against `PANE_WAIT`; `reload.rs`: `Instant::now()` plus `STOP_POLL`; `fork_wait.rs`: `SystemTime::now()` against the file's mtime, `SOURCE_POLL`). The tests of the wrapper need `clk` and `spawn`. They also need a way to feed Enter to the wrapper (see Gaps).
4. **`CPD_TEST_*` overrides** (`CPD_TEST_ADAPTER_WATCHDOG`, `CPD_TEST_WATCHDOG_POLL`, `CPD_TEST_FILE_POLL`, `limit("SOURCE_POLL", ..)`, `limit("LEAD_POLL", ..)`) exist only to shorten real waits. With `ManualClock` they have no reader and are deleted by the wrapper PR.

## 1. Inventory

Targets: **U** unit next to the code (no seam), **I** in-process on `Ports`, **S** smoke with the real binary. "Starts today" is what the test runs: `cpd` = the built binary through `TestEnv::cpd` (a process per call), `tmux` = a real tmux server on a private socket, `fake` = a shell script from `env.fake*`, `sleep` = `thread::sleep`, `wait` = `wait_until` poll.

### 1.1 `cli/tests/session`

| File | Tests | Starts today | Target |
|---|---|---|---|
| `main.rs` | 0 | module list | stays; one `mod` per file kept |
| `pill_lines.rs` | 5 | nothing (reads `SERVER_OPTIONS`, `BORDER_FORMAT`, `STATUS_FORMAT`) | U in `core/src/host/tmux.rs` |
| `plannotator_env.rs` | 2 | nothing (`host::pi::lead(..)` returns a `Launch`) | U in `core/src/host/pi.rs` |
| `host.rs` | 25 | a real tmux server on a private socket (`server()`, most tests), fake `tmux` shell scripts (`env.fake`, `tmux_recording`) and a re-exec of the test binary (`list_panes_is_empty_only_when_tmux_says_no_server_runs`, `CPD_TEST_FAKE_TMUX_CASE`), `cpd doctor/tui/watch` per locale (`every_tmux_call_of_cpd_is_a_utf8_client...`), a source scan, a real `SIGKILL` after `sleep(100 ms)` (`kill_program`), `wait_until`; nothing in 6 | 6 U (the 5 static ones and the source scan), 17 I (library calls on `Tmux` over `FakeRunner`), 2 not migratable (§5) |
| `board.rs` | 6 | `cpd` (a process per call) | I (`run_with`); 0 S |
| `session_list_archived.rs` | 2 | `cpd` | I |
| `session_report.rs` | 3 | `cpd` | I |
| `session_resume.rs` | 5 | `cpd` | I |
| `session_send.rs` | 18 | `cpd` (some with `CPD_SESSION`) | 17 I, 1 S (`the_help_usage_puts_the_id_first`: clap `--help`, no external) |
| `session_retier.rs` | 7 | `cpd` (5); `cpd session open` in a real tmux with `fake_cpd_views`, `fake_pi`, `wait` (2) | 5 I, 2 I (wrapper) |
| `session_asleep.rs` | 3 | `cpd` (1); real tmux cockpit and `fake_pi` (`open_clears_asleep_at`); `archive` through `list-panes` (`archive_clears_asleep_at`) | I |
| `session_first_message.rs` | 3 | `cpd`, `context new` and `plan new` with `fake_pi`/`fake_cpd_views`, a real tmux cockpit in one | I |
| `session_titles.rs` | 14 | `cpd session retitle` with a fake `pi` script, one real tmux pane (`the_title_is_set_as_cpd_title_on_the_sessions_live_pane`), a held flock | I |
| `session_titles_glance.rs` | 4 | `cpd session retitle` with a fake `pi` | I |
| `session_reload.rs` | 12 | real tmux pane, a copy of the `cpd` binary as "the installed binary", a fake `pi` script with real `SIGTERM`/`SIGKILL`, `wait_until`, `thread::sleep` (`POLL * 3`, `POLL * 5`, 2500 ms), a second SQLite connection holding a write lock | I (wrapper) |
| `session_watchdog.rs` | 1 | real tmux pane, `cpd session run`, fake `pi`, `wait`, `sleep` | I (wrapper) |
| `session.rs` | 55 | `cpd` (14 store-only); real tmux cockpit, `fake_pi`, `fake_cpd_views`, `wait` (29); a wrapper in a pane, `wait`, `sleep`, `capture-pane` (12) | 2 I (library), 12 I, 29 I, 12 I (wrapper); 1 S (`list_gives_a_session_leading_two_plans_one_row`) |
| **Total** | **165** | | |

### 1.2 In-crate tests that spawn or sleep (outside the 165)

`scripts/tripwire/allow.txt` lines `cpd-core session::` and `cpd-core driver::` are whole-module allowances ("may start a real tmux"); no unit test of those modules names tmux. What they really start:

| File (test fn) | Starts | Target |
|---|---|---|
| `core/src/session/run.rs` `an_adopted_pi_running_is_not_exited_and_its_pid_is_kept`, `an_adopted_pi_reports_its_exit_code_and_keeps_reporting_it`, `an_adopted_pi_killed_by_a_signal_reports_it`, `an_exited_pi_is_not_signalled_again`, `a_pid_that_is_not_a_child_fails_at_the_first_wait` (5) | `/bin/sh -c`, `exec sleep 60`, `sleep(10 ms)`, `sleep(100 ms)`, `Instant::now` | I on `spawn` (handle of an adopted pid); `a_pid_that_is_not_a_child...` see §5 |
| `core/src/session/run.rs` `patiently_tries_again_after_a_transient_error_and_returns_what_is_not` (1) | production `thread::sleep(wait)` of `patiently` (`run.rs:277`) | I on `clk` |
| `core/src/driver/run.rs` `a_zombie_or_reaped_process_is_not_alive` (1) | `Command::new("true")`, `Instant::now`, `sleep(10 ms)` | see §5 |
| `core/src/driver/stall.rs` `a_stall_is_killed_unless_a_check_span_is_open`, `descendants_are_found_and_the_root_is_not_one` (2) | `sh -c "sleep 30 & wait"`, `sleep 30`, `ps -o command=`, `SystemTime::now` | see §5 |

The other in-crate tests of `core/src/session` (`reload.rs` x4, `retitle.rs` x16, `state.rs`, `row.rs`, `moves.rs`, `fork_wait.rs`) and `core/src/driver/worker.rs` showed no spawn, tmux or sleep in their `#[cfg(test)]` part (`rg` over the part after `#[cfg(test)]`); `driver/lock.rs` has 2 tests and was not read beyond that (`unverified`). `driver/worker.rs the_wait_log_is_due_every_period_and_not_before` takes an `Instant` as an argument (`due(now)`), so it needs nothing. `pi/test/sleep-allow.txt` lines `core/src/session/run.rs 4`, `driver/run.rs 3`, `driver/stall.rs 2`, `driver/worker.rs 1` count test and production sleeps together: `run.rs` has 5 `thread::sleep` (production `:129`, `:277`, `:315`; in tests `:395`, `:438`), which does not match 4, so the PR that lowers the count reconciles it.

## 2. Seams

### 2.1 Needs per file

| Needs-set | Name |
|---|---|
| none | N0 |
| `testkit` | N1 |
| `testkit`, `run_with` | N2 |
| `runner` (call recording, scripted replies, `took`), library calls | R |
| `tmux-route`, library calls | T |
| `testkit`, `run_with`, `tmux-route` | N3 |
| `testkit`, `run_with`, `tmux-route`, `runner` (`pi` handler) | N4 |
| `testkit`, `run_with`, `tmux-route`, `runner` (`<cpd> --version` handler), `spawn`, `clk` | N5 |

| File | N0 | N1 | N2 | R | T | N3 | N4 | N5 | not migratable | Tests |
|---|---|---|---|---|---|---|---|---|---|---|
| `pill_lines.rs` | 5 | | | | | | | | | 5 |
| `plannotator_env.rs` | 2 | | | | | | | | | 2 |
| `host.rs` | 6 | | | 7 | 10 | | | | 2 | 25 |
| `board.rs` | | | 6 | | | | | | | 6 |
| `session_list_archived.rs` | | | 2 | | | | | | | 2 |
| `session_report.rs` | | | 3 | | | | | | | 3 |
| `session_resume.rs` | | | 5 | | | | | | | 5 |
| `session_send.rs` | | | 18 | | | | | | | 18 |
| `session_retier.rs` | | | 5 | | | | | 2 | | 7 |
| `session_asleep.rs` | | | 1 | | | 2 | | | | 3 |
| `session_first_message.rs` | | | 1 | | | 2 | | | | 3 |
| `session_titles.rs` | | | | | | | 14 | | | 14 |
| `session_titles_glance.rs` | | | 1 | | | | 3 | | | 4 |
| `session_reload.rs` | | | | | | | | 12 | | 12 |
| `session_watchdog.rs` | | | | | | | | 1 | | 1 |
| `session.rs` | | 2 | 12 | | | 29 | | 12 | | 55 |

### 2.2 Files grouped by needs-set (sums to 165)

| Needs-set | Files (tests) | Tests | Needs `run_with` |
|---|---|---|---|
| N0 none | `pill_lines` (5), `plannotator_env` (2), `host` (6: `pane_border_style...`, `popup_and_menu_styles...`, `status_format...`, `button_glyphs...`, `the_statusbar_is_no_view`, `no_source_spawns_tmux...`) | 13 | no |
| R `runner` | `host` (7: `a_tmux_call_whose_tmux_tmpdir_is_missing...` (no call must reach the runner), `a_failing_call_is_tmux_failed...`, `list_panes_is_empty_only_when...`, `list_panes_parses_for_a_caller_without_a_utf8_locale`, `every_tmux_method_runs_tmux_with_u_first`, `apply_status_line_sets_the_global_cpd_version_option`, `every_tmux_call_of_cpd_is_a_utf8_client...`) | 7 | no |
| T `tmux-route` | `host` (10: `starts_the_server_once...`, `a_killed_view_pane_respawns...`, `respawn_killing_restarts...`, `create_session_returns...`, `builds_the_cockpit...`, `a_worker_window_gets_its_cwd...`, `a_dead_pane_stays_and_is_respawned`, `set_pane_title_runs_set_p...`, `a_leads_border_shows...`, `a_pane_with_an_unknown_view...`) | 10 | no |
| not migratable as asked | `host` (2: `a_right_click_is_tmuxs_default_binding...`, `a_server_started_by_cpd_reads_the_popup_and_menu_styles_back...`) | 2 | no |
| N1 `testkit` | `session.rs` (`every_short_form_resolves_to_the_full_id`, `a_plan_id_wins_over_a_prefix...`: they resolve on a `Store`, no command) | 2 | no |
| N2 `testkit` + `run_with` | `board` 6, `session_list_archived` 2, `session_report` 3, `session_resume` 5, `session_send` 18, `session_retier` 5, `session_asleep` 1, `session_first_message` 1 (`..._refuses_a_worker_session`), `session_titles_glance` 1 (`a_width_below_9_is_refused`), `session.rs` 12 (`list_*` 2, `show_*`, `state_*` 3, `archive_refuses_the_chat_slot...`, `unarchive_is_refused_while...`, `open_and_fork_of_an_archived_session_are_refused...`, `an_ambiguous_prefix...`, `a_prefix_of_fewer_than_4...`, `unmatched_values_...`) | 54 | yes |
| N3 `+ tmux-route` | `session.rs` 29 (every `fork`/`open`/`park`/`close`/`history` test, `rename_*` x3, `archive_*` x3 that reach tmux, `unarchive_clears...`, `e2_*` x2 and `open_*dead*` x2 through the engine tick, `every_short_form_...`), `session_asleep` 2, `session_first_message` 2 | 33 | yes |
| N4 `+ runner (pi)` | `session_titles` 14, `session_titles_glance` 3 | 17 | yes |
| N5 `+ spawn + clk` | `session_reload` 12, `session_watchdog` 1, `session_retier` 2, `session.rs` 12 (`fork_waits...`, `fork_stops_waiting...`, `a_fresh_lead_with_a_first_message_over_128_kib...`, `a_fork_with_a_message_file_over_128_kib...`, `run_refusals`, `run_gives_the_lead_remote...`, `quitting_*` x3, `a_pi_that_exits_with_status_1...`, `a_refused_archive_prints...`, `a_reload_is_not_a_quit...`) | 27 | yes |
| **Total** | | **165** | |

Which seam gates which PR: N0/R/T need only `ports` + `tmux-route` (no `run_with`, no `testkit`), N1/N2 need only `testkit` + `run_with`, N3/N4 need both plus `tmux-route`, N5 needs `spawn` and `clk` on top. Per-test calls that are `unverified` (not traced to the end): that the three `session_first_message` tests split 2/1 as above (`context new` and `plan new --by-user` are in the flow code; whether they call `Env::tmux` was not read), that `unarchive` reaches no tmux in a refusal (`archive.rs:222` sits after the checks, order not read), and that `e2_*` (`engine(...)` in `session.rs`) can be driven as a library call of `cpd-engine`; if not, those 2 move to the engine migration and N3 drops to 31.

### 2.3 What the fakes must offer that the tests use

- **tmux-route:** `has-session`, `new-session`, `new-window`, `respawn-pane`, `kill-pane`, `kill-window`, `list-panes -F`, `set -p @cpd_*`, `swap-pane`, `rename-window`, `display-message`, `capture-pane` (the wrapper's screen); **fault injection by subcommand** (`tmux_refusing` in `fork_and_open_leave_no_window_when_a_step_after_new_window_fails`, a failing `rename-window` in `open_writes_session_moved_before_the_rename...`, `.fake(..)` in two tests); **a pane's program killed from the test** (`kill_program`, `dying_leads`) with the `pane-died` hook run by the fake (host.rs `a_killed_view_pane_respawns_and_a_killed_lead_or_engine_pane_stays_dead`; the real test needs the hook order of tmux 3.4 and a 100 ms `sleep` against a lost `SIGCHLD`, both of which the fake makes moot); the pane's recorded argv, cwd and env (replaces `fake_cpd_views`, `fake_pi`, `env.calls("tmux")`). Whether `FakeTmux` has fault injection and a kill is `unverified` (read `tests-mock-tmux-route.md`, not code).
- **runner (`pi`):** stdin captured (`stdin_is_the_text_of_the_last_two_turns...`), stdout, exit code and stderr scripted (`a_pi_that_fails_is_title_failed_with_its_stderr_tail`), argv recorded (`env.calls("pi")`), "no call" asserted (`..._with_no_pi`).
- **runner (`<cpd> --version`):** `session_reload.rs` copies the real binary as the "installed" one and asks it for its verdict (`a_lead_wrapper_follows_the_real_verdict_of_the_new_binary`, `a_lead_wrapper_does_not_reload_into_a_cpd_older_than_the_store...`). In-process the route for the installed path answers by calling `run_with(["--version"])` on the same store, so the verdict is still the real one (`asks_to_open_store` in `host/exe.rs`).
- **spawn:** a handle for the wrapper's pi (signal, wait, exit, adopt a pid, `exec`), recording the argv (`--session`, `--session-id`, `--model`, `--`, `@<file>`) and env (`PLANNOTATOR_*`).
- **clk:** `ManualClock::advance`, with the wrapper's polls registered as naps; file mtimes (`backdate`) as the fixture's own `std::fs::File::set_modified`, no process.

### 2.4 Gaps in the ports for this scope (`unverified`, each one blocks a named test)

1. **stdin of the wrapper:** `fork_stops_waiting_for_a_stopped_source_without_a_file_and_retries_on_enter` and `a_pi_that_exits_is_handled_as_before_with_no_error_event` press Enter in a pane (`send-keys`); a wrapper called as a library needs an injected `Read`. Not in `Ports { clock, runner, spawn }`. Proposal: a `stdin: &mut dyn BufRead` argument of the wrapper's function, not a port.
2. **`CPD_SESSION`, `PLANNOTATOR_URL_HOST`, `PLANNOTATOR_REMOTE`** are read from the environment (`cpd_with(env, &[("CPD_SESSION","lead2")], ..)`, 35 reads in the code, `tests-mock-ports.md` §4.4): option 1 there (set once at test start; each nextest test is its own process) is enough here.
3. **`review_host(&mut stderr)`** (`host/addr.rs`, called by `session/run.rs:169`) binds a `UdpSocket`; see §5 (`addr::choose` already takes the detector as a closure).

## 3. The deleted tests of this scope (`tests-mock-deleted.md`, `cpd::session`, 3)

| Test | Target file | Seam | Notes |
|---|---|---|---|
| `host.rs a_tmux_call_past_its_deadline_is_killed_with_its_group_and_is_tool_timeout` | `host.rs` (R) | `runner` + `clk` (a handler that `took(d)` longer than `Call.deadline`, answered by `Failed::Timeout`) | Keeps the contract "a call past its deadline is `tool_timeout` with the argv". The "killed with its group" half is the real runner's `killpg` (§5), pinned in the `port-run` PR, not here. |
| `session_watchdog.rs a_pi_alive_with_its_state_stopped_for_sixty_seconds_is_restarted_with_one_error_event` | `session_watchdog.rs` (N5) | `testkit`, `run_with`, `tmux-route`, `spawn`, `clk` | `ManualClock::advance(60 s)` over the wrapper's poll; assert one `SIGTERM` on the pi handle, a second pi launched, one `error` Event. Replaces `CPD_TEST_ADAPTER_WATCHDOG` and `CPD_TEST_WATCHDOG_POLL`. |
| `session_watchdog.rs a_pi_whose_adapter_reports_idle_within_the_limit_is_left_alone` | `session_watchdog.rs` (N5) | same | `advance(59 s)`, write `idle`, `advance(5 s)`: no signal, one launch, 0 `error` Events. The "left alone" claim is a count of handle calls, not a wait for nothing. |

The 3 are restored in PR `mr-session-host` (1) and `mr-session-wrapper` (2); after it the scope has 168 tests. The one surviving test of `session_watchdog.rs` (`a_pi_that_exits_is_handled_as_before_with_no_error_event`) sleeps `POLL * 3` ("time-claim"): it becomes `advance(3 polls)` and an assertion on the pi launch count.

## 4. Proposed migration PRs

Order: the files that need the fewest seams first. PRs 1 and 2 need no common seam and can run in parallel after `ports`; 3 needs both; 4 needs `spawn` and `clk` (the clk PR is in the Lead branch, not on main).

### PR `mr-session-host`

- **Needs:** `runner`, `tmux-route` (library only; no `run_with`, no `testkit` beyond a tempdir).
- **Files:** `cli/tests/session/host.rs` (25), `pill_lines.rs` (5), `plannotator_env.rs` (2).
- **Tests:** 32 + 1 restored. Of the 32: 13 U, 7 + 10 I, 2 in §5.
- **Jobs:**
  1. `test` red: `host.rs` R and T tests as library calls on `Tmux` over `FakeRunner`/`FakeTmux` (argv, `-u`, errors, `list_panes` parse, `kill_program` through the fake).
  2. `impl`: `Tmux::with_runner(&dyn Runner)` (replaces `with_program` and `with_tmpdir`'s spawn); the `TMUX_TMPDIR` refusal stays before the runner call.
  3. `test`: the 13 N0 tests move beside the code (`core/src/host/tmux.rs`, `host/pi.rs`); the source scan `no_source_spawns_tmux_with_a_command_of_its_own` is kept and replaced by the tripwire of `tests-mock-tmux-route.md` when that PR lands.
  4. `test`: restore `a_tmux_call_past_its_deadline...` on `runner` + `clk`.
  5. `docs`: delete the 2 tests of §5 and the `DefaultDirServer` real-tmux cleanup; delete the `scripts/tripwire/allow.txt` lines `cpd::session host::`, `pill_lines::`, `plannotator_env::` (lines 22-24).

### PR `mr-session-store`

- **Needs:** `testkit`, `run_with`.
- **Files:** `board.rs` (6), `session_list_archived.rs` (2), `session_report.rs` (3), `session_resume.rs` (5), `session_send.rs` (18), `session_retier.rs` (5 of 7), `session_asleep.rs` (1 of 3), `session_first_message.rs` (1 of 3), `session_titles_glance.rs` (1 of 4), `session.rs` (14 of 55).
- **Tests:** 56 (54 I, 2 on a `Store`); 2 of them stay S with the real binary: `session_send.rs the_help_usage_puts_the_id_first`, `session.rs list_gives_a_session_leading_two_plans_one_row`.
- **Jobs:**
  1. `test` red: `session_send.rs` and `board.rs` (24) through `run_with` on a `testkit` home (the biggest files; `cpd_with` becomes a `Fx` that sets `CPD_SESSION` once).
  2. `test`: `session_resume`, `session_report`, `session_list_archived`, `session_retier` (5), `session_asleep`, `session_first_message`, `session_titles_glance` (1 each).
  3. `test`: the 14 store-only tests of `session.rs`.
  4. `impl`: the helpers (`setup`, `exec_sql`, `fixture`, `events`, `refused`, `show`) move from `session_send.rs`/`session.rs` to `testkit` (they are copied across files today: `session_titles_glance.rs` says "copies of those of `session_titles.rs`"); one source of truth.
  5. `docs`: delete the `cpd::session` lines of `scripts/tripwire/allow.txt` of the files that are fully migrated (`board::`, `session_list_archived::`, `session_report::`, `session_resume::`, `session_send::`); the others stay until PRs 3 and 4.

### PR `mr-session-cockpit`

- **Needs:** `testkit`, `run_with`, `tmux-route`, `runner` (`pi`).
- **Files:** `session.rs` (29 of 55), `session_titles.rs` (14), `session_titles_glance.rs` (3), `session_asleep.rs` (2), `session_first_message.rs` (2).
- **Tests:** 50, all I. Over 45 of them are `cockpit(&env)` fixtures: one shared `cockpit()` on `FakeTmux` replaces the 3 copies (`session.rs`, `session_first_message.rs`, `session_retier.rs`).
- **Jobs:**
  1. `test` red: `session_titles.rs` and `_glance.rs` (17) on the `pi` handler (stdin, stdout, exit, stderr tail, no call) and `the_title_is_set_as_cpd_title...` on `FakeTmux`.
  2. `test`: `fork`, `open`, `park`, `close`, `history` (18 of `session.rs`), including the two fault-injection tests.
  3. `test`: `rename`, `archive`, `unarchive`, `open_*`, `e2_*`, `every_short_form...` (11 of `session.rs`), `session_asleep` (2), `session_first_message` (2).
  4. `impl`: `Env::tmux()` takes the `Ports` runner (one construction site, `session/mod.rs`); `retitle::run_pi` and `retitle::set_header` go through the same runner.
  5. `docs`: delete the `cpd::session` lines `session::`, `session_titles::`, `session_titles_glance::`, `session_asleep::`, `session_first_message::` of `scripts/tripwire/allow.txt` (all their tests are migrated once PRs 2-3 have landed; `session::` also needs PR 4).

### PR `mr-session-wrapper`

- **Needs:** `testkit`, `run_with`, `tmux-route`, `runner`, `spawn`, `clk`.
- **Files:** `session_reload.rs` (12), `session_watchdog.rs` (1 + 2 restored), `session_retier.rs` (2), `session.rs` (12), and the 9 in-crate tests of §1.2.
- **Tests:** 27 + 2 restored + 9 in-crate = 38.
- **Jobs:**
  1. `impl`: `session::run` takes `Ports` and an injected stdin; `reload`, `fork_wait` and `patiently` read `clk` (replaces `CPD_TEST_*` and `limit(..)`); `PiProc` is built on a `spawn` handle.
  2. `test`: `session_watchdog.rs` (1 + 2 restored) with `ManualClock::advance`.
  3. `test`: `session_reload.rs` (12): the exec is a recorded `Spawn::exec(argv)` (`[installed, session, run, id, --adopt, pid]`), the adopted pi is a handle, `pi_stop_seconds` is a `clk` advance (`a_pi_that_ignores_sigterm_is_killed_after_pi_stop_seconds...` asserts `TERM` then `KILL` at `advance(1 s)`, not a wall-clock lower bound).
  4. `test`: the wrapper tests of `session.rs` (12) and `session_retier.rs` (2).
  5. `test`: the 6 in-crate tests of `session/run.rs` on a fake handle and `clk`; the 3 of `driver` as in §5.
  6. `docs`: delete the `cpd::session` lines `session_reload::`, `session_watchdog::`, `session_retier::`, the `cpd-core session::`/`driver::` lines of `scripts/tripwire/allow.txt`, and the lines `session_reload.rs 11`, `session_watchdog.rs 1`, `session.rs 2` of `pi/test/sleep-allow.txt`, and lower its `core/src/session/run.rs 4` to 0.

Sums: 32 + 56 + 50 + 27 = 165; plus 3 restored (168) and 9 in-crate.

## 5. Not migratable as asked

Each is a test whose subject is the behaviour of something we do not own. The proposal keeps policy `[tests]`: no process, no real tmux, no real time.

| Test | Subject | Proposal |
|---|---|---|
| `host.rs a_right_click_is_tmuxs_default_binding_that_passes_it_to_a_pane_that_captures_the_mouse` | tmux 3.4's own default `MouseDown3Pane` binding read back with `list-keys` | Delete. Keep the part that is ours: a U test that `SERVER_OPTIONS` has no `MouseDown3Pane` binding and has the `MouseDown1Status` one (`run-shell -b "cpd tui action ..."`). |
| `host.rs a_server_started_by_cpd_reads_the_popup_and_menu_styles_back_as_default` | what a real tmux answers to `show -gv` (it skips options an older tmux does not know) | Delete; `the_popup_and_menu_styles_are_server_options_set_to_default...` already pins the 3 `set -gq` lines (U). |
| `host.rs a_tmux_call_past_its_deadline_is_killed_with_its_group...` (deleted, §3) | `killpg` of a real process group | Keep `tool_timeout` on `FakeRunner` (`took`); the group kill is the real runner's glue, unit-tested in the `port-run` PR or not at all. |
| `session_reload.rs` `assert_eq!(f.pane_pid(), pid)` (in 9 of the 12) | exec replacing the process keeps its pid | Drop the assertion; assert `Spawn::exec` was called once with the exact argv and that the same wrapper function then ran again with `--adopt <pid>` and did not signal pi. |
| `session_reload.rs` `alive(pid)`, `pi.signals`, `pi.pids` files | a real fake-pi script reacting to `SIGTERM`, `SIGKILL` | The pi handle records `signal(TERM)`, `kill()`, `wait()`; "ignores SIGTERM" is a handle option. |
| `session.rs a_fresh_lead_with_a_first_message_over_128_kib...`, `a_fork_with_a_message_file_over_128_kib...` | the kernel's `MAX_ARG_STRLEN` (131 072 bytes per argv string, E2BIG at `exec`) | Assert our invariant: the pane's pi argv is `--`, `@<file>` with every element under 131 072 bytes, and the file holds the whole text. E2BIG itself is the kernel's. |
| `session.rs run_gives_the_lead_remote_and_the_review_host` (second half) | `host::addr::primary_ipv4` binds and connects a `UdpSocket` to `OUTSIDE` and reads the local address (the kernel's routing table; no packet) | The override and `PLANNOTATOR_REMOTE=1` become a `Launch` assertion (as `plannotator_env.rs`). The detect rule is ours and already injectable: `addr::choose(var, detect, warn)` takes the detector as a closure, so a U test gives it `Ok(10.0.0.2)`, a loopback and an `Err` and asserts `localhost` plus one warning line. `primary_ipv4` itself (3 lines of socket) is left untested. |
| `session_titles.rs a_held_lock_is_title_failed_running_with_no_pi` | `flock` via `std::fs::File::try_lock` | Keep: the test holds a second file description of a tempfile in the same process, no process or time is involved, and the code under test is our `lock::try_lock` mapping. If the Lead reads "flock" as external, the alternative is a `Locks` fake, which this scope does not need (YAGNI). |
| `session_reload.rs a_wrapper_whose_first_write_meets_a_held_write_lock...` | SQLite `busy_timeout` (C code sleeping on real time) while a second connection holds `BEGIN IMMEDIATE` | Keep with a tempfile store and `store_busy_ms = 1`; the "Waiting for the store" backoff (0.5 s, 1 s) is `patiently` on `clk`, so the 2500 ms `sleep` becomes `advance`. Whether `Store::open_with` gives SQLite's own wait to `clk` is `unverified`: it does not, so keep the busy timeout minimal. |
| `session/run.rs a_pid_that_is_not_a_child_fails_at_the_first_wait` | `waitpid` on a pid that is not a child returns `ECHILD` | Delete; assert in the fake handle that an error from `wait` is reported once (the logic is ours, the `ECHILD` is the kernel's). |
| `session/run.rs an_adopted_pi_killed_by_a_signal_reports_it`, `an_adopted_pi_reports_its_exit_code...` | `ExitStatus` of a real child | A handle that returns a scripted `ExitStatus`; the mapping signal to code is ours (`ExitStatus::from_raw` on a fixed value is in-process and needs no child). |
| `driver/run.rs a_zombie_or_reaped_process_is_not_alive` | what `kill(pid, 0)` or `ps` says about a zombie (`alive` is written 6 times, `tests-mock-ports.md` §3) | `Procs::alive` as a P fact of `spawn` (the PR that owns `alive`); here the test is deleted and the zombie rule is a U test on the parsed `ps` state letter (`Z`), if `alive` parses one: `unverified`, read `driver/run.rs:98`. |
| `driver/stall.rs a_stall_is_killed_unless_a_check_span_is_open`, `descendants_are_found_and_the_root_is_not_one` | `ps` output of a real process tree | `Runner` route for `ps` returning a table; the walk `descendants(root)` is ours (`stall.rs:150`), the kill goes through the pi handle. |
| `host.rs every_tmux_call_of_cpd_is_a_utf8_client_whatever_the_callers_locale` | what tmux does with `LANG`/`LC_ALL` | In the R group: assert `-u` is first for every `Tmux` method under each `Locale` set on the call env of the `FakeRunner`; the entry points (`doctor`, `tui`, `watch --ensure`) are covered by the source scan. |

No test of this scope needs a pty, a real network call, or an installed binary after the change: the `cpd` copy of `session_reload.rs` (`copy_executable(CPD, installed)`) and the real `cpd --version` verdict become the `<cpd> --version` handler of §2.3.
