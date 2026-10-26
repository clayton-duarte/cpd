# Research: in-process migration of the tui tests, first half (`mr-tui-a`)

Scope: `cli/tests/tui/{tui,bare,tui_tree,tui_tree_delete,tui_tree_asleep,tui_tree_archive_manager,tui_worker,tui_pulse,tui_idle,tui_loop_guard,tui_chat_pad,tui_reset_layout,tui_runner}.rs` and the in-crate tests of `tui/src/{tree,widgets,cockpit}` that start a process or sleep.
Method: read from the code with `rg` and `read`, plus `git show adb53fc8^:` for the deleted tests. No build and no test was run. A claim not read from the code is labelled `unverified`. Counts are `rg -c '#\[test\]'`. Of the 3 tests of `tui_runner.rs` and of `tui.rs`'s harness group, one is an `#[ignore]`d child that a sibling re-runs; they are counted because the file count counts them.

**Scope total: 143 tests in the 13 files, plus 1 in-crate test that starts a process = 144.** The 16 deleted tests of section 3 are not in the 144 (they are not in the files); the PRs of section 4 add them back (144 + 16 = 160).

Seam names. The brief's seams are `clk`, `runner`, `tmux-route`, `git-route`, `spawn`, `run_with`, `testkit`. This scope needs no `git-route`. I add one name, **`p6`**, which is `ports-6` of `tests-mock-ports.md` (§ "ports-6", line 85 and 201): the terminal, the event source and the clock injected into the tui view loops (`tui/src/pump.rs:69` `Pump::new(event::read)`, `worker/mod.rs` `event::poll`, `tree/mod.rs:149-275` `Instant::now`/`SystemTime::now`, `lib.rs` `backed_off` 3 s sleep), `RunCommand` and `Restart` as `Arc<dyn Fn>`, `cockpit.rs exec_attach`, and `lib.rs on_a_terminal()` (`std::io::stdin().is_terminal()`). Without `p6` no tui view loop runs on `TestBackend`. Two implications, so the table stays exact: **`tmux-route` includes `runner`** (the route and the adapter are built on it: `tests-mock-tmux-route.md` §4 PR 5-6), and **`p6` includes `clk`**. `testkit` is the `TestEnv` replacement (fixture store, home, config); every file uses `TestEnv` today, so every file needs at least `testkit` to drop the `TestEnv` exec-and-reap machinery.

## 1. Inventory

"Starts today" are call-site counts (`rg`) in the file, not tests. Targets: **U** unit next to the code, no seam; **I** in-process on the named fakes; **S** smoke with the real binary (none proposed here: every `cpd tui` view needs a terminal, and the policy forbids a real tmux).

| File | Tests | What its tests start today | Target |
|---|---|---|---|
| `cli/tests/tui/tui.rs` | 21 | a private **real tmux** server per test (52 `env.tmux`/`tmux_out`), the `cpd` binary through `TestEnv` (18 `env.cpd`, `cpd tui`, `plan new`), real outer tmux clients on a pty (`Outer::attach`, `send-keys` mouse reports), `kill -9` of pane processes (`kill`), `env.fake` shell wrappers (5), a child re-run of the test binary (`a_command_has_no_gh_ssh_or_proxy_variable`, `std::env::current_exe`), 19 `wait_until`, 1 `thread::sleep` | 12 I + 6 U + 3 removed (harness) |
| `cli/tests/tui/bare.rs` | 7 | real tmux (17), bare `cpd` through `env.cpd` (6), real `tmux attach` by `exec`, a real older tmux binary (`a_real_older_tmux_server_is_restarted_onto_the_installed_tmux`), `env.fake` (8), 3 waits | 7 I |
| `cli/tests/tui/tui_tree.rs` | 65 | the `cpd tui view tree` binary in a pane of a real tmux (`Pane::start`, `capture-pane`, `send-keys`), a shell-script fake `cpd` and fake `tmux` (73 `fake*`, `fake_dialogs`), `env.cpd session state/inbox dismiss` (7), real `flock` on `locks/watch.lock`, 171 waits, 3 sleeps (some are time claims) | 8 U + 57 I |
| `cli/tests/tui/tui_tree_delete.rs` | 3 | real tmux pane, fake `cpd` and fake `tmux` scripts (9), 13 waits, 1 sleep | 3 I |
| `cli/tests/tui/tui_tree_asleep.rs` | 3 | real tmux pane, fake `cpd` script, `capture-pane -e`, 5 waits, a 500 ms sleep (`click_on_asleep_row_resumes_it_in_the_chat_slot`) | 1 U + 2 I |
| `cli/tests/tui/tui_tree_archive_manager.rs` | 2 | real tmux pane, shell fake `cpd` that keeps `in_chat_slot` and a marker file, fake `tmux` popup that confirms, real `flock` | 2 I |
| `cli/tests/tui/tui_worker.rs` | 19 | `cpd tui view worker` in a real tmux pane (`Pane::start_command` with the cockpit's `pane-died` hook), `capture-pane`, `send-keys`, `env.cpd` for the refusals (6), 26 waits, a 6 s sleep (`a_view_that_fails_to_start_backs_off_instead_of_looping`) | 9 U + 10 I |
| `cli/tests/tui/tui_pulse.rs` | 12 | nothing: `read::pulse` and `read::manager_slot` on a `Store` in a temp home with a fixed `now_ms`; `TestEnv` only makes the home | 12 U (already in-process) |
| `cli/tests/tui/tui_idle.rs` | 3 | `cpd tui view …` processes in real tmux panes, `/proc/<pid>/io` `rchar` and `syscw` and CPU time of those processes over a `CPD_IDLE_SECS` (5 s) window, 4 sleeps, real `flock` | 3 I (new oracle, §5) |
| `cli/tests/tui/tui_loop_guard.rs` | 2 | the tree binary in a real tmux pane, `capture-pane -e` | 2 U |
| `cli/tests/tui/tui_chat_pad.rs` | 2 | real tmux: a real 20-row outer client on a second server (`-L …-pad`), `env.fake` `cpd`/`gh` scripts; the history view binary in a pane | 2 I |
| `cli/tests/tui/tui_reset_layout.rs` | 1 | `cpd tui action reset` through `TestEnv` against a shell-script recording `tmux` | 1 I |
| `cli/tests/tui/tui_runner.rs` | 3 | this test binary re-run in a real tmux pane (`#[ignore]`d `a_view` + 2 tests), reads tmux's `#{mouse_any_flag} #{alternate_on}`, 1 sleep | 2 U on a writer + the child, removed |
| in-crate `tui/src/widgets/mod.rs` | 1 | `a_missing_hostname_file_or_load_is_not_an_error` calls `hostname()`, which falls to `Command::new("hostname")` (`widgets/mod.rs:460`) when `/proc/sys/kernel/hostname` and `$HOSTNAME` give nothing (macOS) | 1 I |

In-crate tests of `tui/src/tree`, `tui/src/widgets`, `tui/src/cockpit` that start a process or sleep: **only the one above**. Read: the 5 tests of `cockpit.rs` (`reset_steps`, `popup_args`, `tmux_names_the_socket…`), the 7 of `widgets/host.rs` (parsers of `nvidia-smi`, `ioreg`, `/proc/stat`, `ps`, `vm_stat` text), the 8 of `tree/folds.rs` and the 3 `tempfile` users of `tree/view.rs` touch a temp directory only; the `Instant::now()` calls in `tree/view.rs` tests (2838-3220) build a `t0` and add offsets, they never sleep. The real `Command::new` sites that are not tested in-crate: `tree/actions.rs:389,471` (the `cpd` child of every tree action), `widgets/host.rs:60,79` (`nvidia-smi` stream, `ioreg`, `ps`). Their behaviour is reached only through the files above (`actions.rs`) or not at all (`host.rs`: parsers are tested, the spawn is not; unverified that a test exercises `Gpus::start`).

Facts about the code that decide the targets (all read):
- `cpd_tui::run(args, version, run_command, restart)` (`tui/src/lib.rs`) is a library entry: `cockpit::run`, every `view::run`, and `cockpit::action` are reachable with no CLI. `tui::Args` derives `clap::Args`, not `Parser`: a test builds it directly. Only a **parse error** (`tui view nope`, `tui --bogus`, `tui view worker` with no Attempt: error code `usage`, exit 2) needs the root parser, which `cli` owns: that is `run_with`.
- `on_a_terminal()` reads the real stdin and stdout. A test calling `run` in-process is deterministic only where nextest captures stdout; under `--no-capture` in a terminal it would try to draw. `p6` must inject it.
- `Tmux::new(socket)` is built inside the views (`worker/mod.rs:88 close_pane`, `cockpit.rs`, `widgets/mod.rs`); it reaches the fake only through the `tmux-adapter` PR (`tests-mock-tmux-route.md` PR 6).
- The tree watches the store directory with `notify` (`tree/mod.rs:152 read::watch::Watch::start`) and falls back to polling `poll_ms` when it cannot start. A real OS watcher in an in-process test is an external; use the fallback (`Schedule::new(Instant, poll)` on the injected clock). `unverified:` that the view can be told to skip the watcher; `p6` must add that switch.
- The snooze test (`the_inbox_follows_an_engine_that_starts_and_a_snooze_that_ends_with_no_event`, `tui_tree.rs:1864`) sets `snoozed_until` with SQLite `strftime('now','+8 seconds')`: a wall clock in the fixture. In-process it uses a fixed timestamp and `ManualClock`.

## 2. Seams

Minimum seams per file (the union over its tests; per-test sets are in the table below):

| File | Minimum seams |
|---|---|
| `tui.rs` | `testkit`, `tmux-route`, `p6`, `run_with` (2 of 21 tests need `run_with`; 12 need `tmux-route`; 6 need nothing; 3 only `testkit`) |
| `bare.rs` | `testkit`, `tmux-route`, `p6`, `spawn` (`exec_attach` is an `exec`: G6 of the tmux route; `FakeSpawn` must record the `exec` argv, `unverified:` it has an `exec` form) |
| `tui_tree.rs` | `testkit`, `tmux-route`, `p6` (8 tests only `testkit`; 11 `p6`; 20 `p6`+`runner`; 26 `p6`+`tmux-route`) |
| `tui_tree_delete.rs` | `testkit`, `tmux-route`, `p6` |
| `tui_tree_asleep.rs` | `testkit`, `runner`, `p6` |
| `tui_tree_archive_manager.rs` | `testkit`, `tmux-route`, `p6` |
| `tui_worker.rs` | `testkit`, `tmux-route`, `p6`, `run_with` (1 test) |
| `tui_pulse.rs` | `testkit` |
| `tui_idle.rs` | `testkit`, `p6` |
| `tui_loop_guard.rs` | `testkit` |
| `tui_chat_pad.rs` | `testkit`, `tmux-route`, `p6` |
| `tui_reset_layout.rs` | `testkit`, `tmux-route` |
| `tui_runner.rs` | `p6` (a writer for the terminal-restore bytes) |
| in-crate `widgets/mod.rs` | `runner` (`hostname` through `Runner`; `tests-mock-ports.md:44`) |

Why each set (the tests are named in the per-test column; a number is a count of tests):

- **`tui_tree` on `View` only (8, `testkit`):** render and key tests with no command, no popup, no follow: `the_tree_of_a_prepared_store`, `an_inbox_of_two_items_is_the_pill_title…`, `the_tree_leaves_the_first_and_the_last_column_blank`, `an_empty_store_says_so`, `a_tree_taller_than_its_share_scrolls_with_the_wheel_between_its_cues`, `a_link_kinds_label_reads_whole…` (OSC 8: assert the cells on `TestBackend`, the tmux half is §5), `the_selected_plan_rows_name_is_blue…`, `the_selected_context_rows_name_is_blue…`. `tree/view.rs` already has 116 unit tests on `View`; these move next to them (U). `unverified:` `View::draw` and `Tree` are reachable from `tui/src/tree` tests with a `Tree` built by `read::tree(&mut store)` on a testkit store.
- **`tui_tree` loop only (11, `testkit`+`p6`):** startup and follow: `the_view_needs_a_terminal` (tty fact), `the_tree_captures_the_mouse_so_tmuxs_default_right_click_binding_does_not_run` (the `Capture` the view passes to `run_view`), the two folds-file tests (`an_invalid_folds_file…`, `a_missing_folds_file…`: the loop's start reads the file and logs), and the 7 follow tests: `a_session_waiting_for_an_agent…`, `the_tree_follows_the_events_of_the_store`, `a_failed_snapshot_keeps_the_tree…`, `the_tree_starts_no_subprocess` (FakeRunner call log empty), `a_follow_that_fails_is_shown_in_the_footer`, `the_tree_follows_the_archive_events`, the snooze test above. Four of them create state with `env.cpd session state` / `inbox dismiss`; they do **not** need `run_with` if they write the store the way `the_tree_follows_the_archive_events` already does (`sql` of the row plus its Event); keep that rewrite in the Job (a decision, not a guess: the test's subject is the view's follow, not the command).
- **`tui_tree` with commands (20, `testkit`+`p6`+`runner`):** every test that presses a key or clicks and asserts the `cpd` argv the view ran (`enter_opens_the_lead…`, `a_click_on_a_context_title…`, `a_right_click_opens_the_menu…`, `the_inbox_sections_toggle_row…`, `a_collapsed_context_…_survive_a_restart_of_the_view` (a second view over the same home; no `respawn-pane` needed), the engine-down tests, the item commands (`d`, `s`, jump), …). Today the fake is a shell script `fake_cpd` (`tui_tree.rs:~455`); `tree/actions.rs:389` spawns `PROGRAM`. In-process: `FakeRunner` route `cpd` with scripted JSON replies, asserting argv. `the_tree_keeps_drawing_and_reading_keys_while_a_command_runs` has a 4 s `sleep` in the fake and a wall-clock `<3 s` claim: `FakeRunner` `took` plus `ManualClock` (`tests-mock-ports.md` pr2-1 names `took`; `unverified:` it exists for a child spawned from a view thread).
- **`tui_tree` with popups (26, `testkit`+`p6`+`tmux-route`):** every test with `fake_dialogs`/`popups` (new plan, close, archive chains, rename, delete, jump of an item, merge spinner, …), plus `question_mark_opens_the_legend_popup` (`list-clients` with none: the logged `NO_CLIENT`). The tmux route needs the scripted popup answer `FakeTmux::on_popup` (`tests-mock-tmux-route.md` G5, line 96): `dialog::ask_with` reads the `--out` file the popup would write. `a_click_on_a_stuck_job_item…` and the Copy-id tests assert tmux calls (`set-buffer -w`); the route records them.
- **`tui.rs`/`bare.rs`/`tui_chat_pad`/`tui_reset_layout` (cockpit):** `cockpit::run` and `bare` over the fake tmux route; `RunCommand` (`fn` pointer today) records `watch --ensure` through a `thread_local` until `p6` makes it `Arc<dyn Fn>` (`unverified:` that a `thread_local` recorder is enough). `assert_layout` in `tui.rs` reads **tmux's geometry** (`pane_left/top/width/height` after real splits); the fake does not model geometry (`tests-mock-tmux-route.md:72`: "geometry is not modelled"), so those assertions become the ordered `split-window`/`resize-pane` argv (the `reset_steps` unit tests in `cockpit.rs:663+` already pin the sizes).
- **`bare.rs` needs `spawn`** where it reaches `exec_attach` (`cockpit.rs:507`, `client(program).args(..).exec()`): `bare_in_a_terminal_builds_the_cockpit…`, `it_removes_an_older_cpds_statusbar_pane`, `a_server_that_is_not_older_needs_no_restart…`, the older-tmux test. The two refusals (`without_a_terminal…`, `with_a_terminal_only_on_stdin…`) and `inside_the_cockpit_it_does_not_nest_an_attach` stop before it.
- **`run_with` (3 tests, all others in the scope are a library call):** `a_server_started_by_plan_new_has_every_server_option` (`context new` and `plan new` are CLI commands of other crates: `tui.rs:222`), `tui_view_routes_by_name` (`tui view nope`, `tui view`, `tui --bogus`: clap `usage`, exit 2), `an_unknown_attempt_or_plan_is_refused_before_drawing` (its `tui view worker`/`tui view worker api-a1` cases are `usage`/`plan_required`; the `usage` ones are the root parser's). `the_view_needs_a_terminal` (`tui_tree.rs`, `tui_worker.rs`) is a library call: `run(Args{View::…})` returns `not_a_terminal` once `p6` injects the tty fact.

### Needs-set table (sums to the scope's 144)

| Needs-set | Library call only / `run_with` | Tests (files) | Sum |
|---|---|---|---|
| none (U) | library | `tui.rs` 6 (status line, §5) | 6 |
| `testkit` | library | `tui_pulse` 12, `tui_loop_guard` 2, `tui_worker` 9 (renders), `tui_tree` 8, `tui_tree_asleep` 1, `tui.rs` 3 (the harness self-tests: removed with `TestEnv`) | 35 |
| `p6` | library | `tui_runner` 3 | 3 |
| `runner` | library | in-crate `widgets/mod.rs` 1 | 1 |
| `testkit`, `p6` | library | `tui_tree` 11, `tui_worker` 8, `tui_idle` 3, `tui_chat_pad` 1 | 23 |
| `testkit`, `p6`, `runner` | library | `tui_tree` 20, `tui_tree_delete` 1, `tui_tree_asleep` 2 | 23 |
| `testkit`, `p6`, `tmux-route` | library | `tui_tree` 26, `tui_tree_delete` 2, `tui_tree_archive_manager` 2, `tui_worker` 1, `bare.rs` 3, `tui.rs` 1 | 35 |
| `testkit`, `tmux-route` | library | `tui.rs` 9, `tui_chat_pad` 1, `tui_reset_layout` 1 | 11 |
| `testkit`, `p6`, `tmux-route`, `spawn` | library | `bare.rs` 4 | 4 |
| `testkit`, `tmux-route`, `run_with` | **run_with** | `tui.rs` 1 (`a_server_started_by_plan_new…`) | 1 |
| `testkit`, `p6`, `run_with` | **run_with** | `tui.rs` 1 (`tui_view_routes_by_name`) | 1 |
| `testkit`, `run_with` | **run_with** | `tui_worker` 1 (`an_unknown_attempt_or_plan…`) | 1 |
| **Total** | | library 141, `run_with` 3 | **144** |

Per-file check of the sum: `tui` 6+3+9+1+1+1 = 21; `bare` 3+4 = 7; `tui_tree` 8+11+20+26 = 65; `tui_tree_delete` 1+2 = 3; `tui_tree_asleep` 1+2 = 3; `tui_tree_archive_manager` 2; `tui_worker` 9+8+1+1 = 19; `tui_pulse` 12; `tui_idle` 3; `tui_loop_guard` 2; `tui_chat_pad` 1+1 = 2; `tui_reset_layout` 1; `tui_runner` 3; in-crate 1.

What this decides: 41 tests (the 35 and the 6) need no seam but `testkit`, so their PR can start first; 24 need `runner` and `p6` but no `tmux-route` (23 + the hostname test); `tmux-route` is needed by 51 of the 144 (35 + 11 + 4 + 1) and by all 16 deleted tests; `run_with` by 3, and none of those blocks another test.

## 3. The deleted tests of the scope (`tests-mock-deleted.md`, binary `cpd::tui`: 16)

The 16 are read from `git show adb53fc8^:cli/tests/tui/{tui,bare}.rs` (deleted by #329 and #340). Bodies read: the `tui.rs` ones except the four `tui_adds_*` one-liners and `…both_are_missing` (their helper `assert_repaired_after_killing` is read by name only: `unverified:` its body). The three `bare.rs` bodies were not read; their notes are inferred from the names and from `cockpit.rs` (`unverified`). The target file is the file they were in; the layout of tmux that they assert (`assert_layout`, pane sizes) is replaced by the ordered tmux argv (§2). `cockpit::run` is entered as a library call, so `run_with` is not needed by any of them.

| Test | Target file | Seam | Note |
|---|---|---|---|
| `bare.rs::when_it_is_not_safe_nothing_is_killed_the_restart_is_marked_pending_and_the_servers_binary_attaches` | `bare.rs` | `tmux-route`, `p6`, `spawn`, `testkit` | `Restart` fake (`now → Busy`, `mark`); the route answers `#{version}`; attach recorded as `exec` |
| `bare.rs::a_restart_records_the_pane_of_a_parked_session_before_it_gets_the_go_signal` | `bare.rs` | `tmux-route`, `p6`, `spawn`, `testkit` | goes through the engine's `restart::restart` (`cpd_engine::watch::restart`) via `Restart.now`: the engine's own seams (clock/sleep, `unverified:`) are `mig-engine`'s; if that is not ready, keep the `Restart` fake and test the ordering in the engine |
| `bare.rs::it_restarts_views_that_run_an_older_binary_once` | `bare.rs` | `tmux-route`, `p6`, `spawn`, `testkit` | `respawn_old_views` compares `Tmux::pid_exe(pid)` (`cockpit.rs:~640`, a real `/proc/<pid>/exe` or `libproc` read) with the installed exe's `(dev, ino)`: **needs `pid_exe` behind the route or an injected fn** (not in the brief's set; `tests-mock-tmux-route.md:20` `server_exe` is the same real read) |
| `tui.rs::tui_marks_a_restart_of_an_older_tmux_server_pending_and_does_not_kill_it` | `tui.rs` | `tmux-route`, `p6`, `testkit` | the fake's `#{version}` older (G1); `Restart.mark` file-based in the home; stderr line (`cockpit.rs:70-85`, `eprintln!`) needs a stderr sink: `p6` |
| `tui.rs::tui_respawn_warns_on_stderr_while_a_preview_is_on_and_not_otherwise` | `tui.rs` | `tmux-route`, `p6`, `testkit` | `@cpd_preview` option via the route; the warning is an `eprintln!`: stderr sink (`p6`) |
| `tui.rs::tui_adds_a_missing_chat_placeholder_to_an_existing_main` | `tui.rs` | `tmux-route`, `testkit` | `assert_repaired_after_killing(&["chat"])`: kill a pane in the fake, run `tui`, assert the added `split-window` |
| `tui.rs::tui_adds_a_missing_plan_view_to_an_existing_main` | `tui.rs` | `tmux-route`, `testkit` | same helper, `["plan"]` |
| `tui.rs::tui_adds_a_missing_tree_to_an_existing_main` | `tui.rs` | `tmux-route`, `testkit` | same helper, `["tree"]` |
| `tui.rs::tui_adds_a_missing_widgets_view_to_an_existing_main` | `tui.rs` | `tmux-route`, `testkit` | same helper, `["widgets"]` |
| `tui.rs::tui_adds_the_plan_view_and_the_chat_slot_when_both_are_missing` | `tui.rs` | `tmux-route`, `testkit` | same helper, two panes |
| `tui.rs::tui_builds_main_in_a_cockpit_that_has_only_a_park_window` | `tui.rs` | `tmux-route`, `testkit` | `new-session … park-pay`, then `tui`: `built: true`, windows `["park-pay","main"]`, second run `built: false`; the status line assertion becomes the options (`status`, `status-format[0]` equal `STATUS_FORMAT`) |
| `tui.rs::tui_builds_the_cockpit_once_and_starts_the_engine` | `tui.rs` | `tmux-route`, `testkit` | `out["built"]`, `socket`, `attach` argv, `watch.started`; the four `cpd tui view <v>` spawns are read from the recorded `new-session`/`split-window` argv, not from a fake `cpd`; the engine start is the recorded `RunCommand` call |
| `tui.rs::tui_finishes_a_build_that_stopped_after_the_first_pane` | `tui.rs` | `tmux-route`, `testkit` | kill 3 panes in the fake, run `tui`, assert the first pane untouched (same pane id) and a second run changes nothing |
| `tui.rs::tui_keeps_a_lead_in_the_chat_slot_when_the_plan_view_is_missing` | `tui.rs` | `tmux-route`, `testkit` | needs `swap-pane` and `@cpd_session` on the fake (`unverified:` both are in `fakes/tmux/cmds.rs`; check `KNOWN`) |
| `tui.rs::tui_removes_an_old_statusbar_pane_then_resets_the_layout` | `tui.rs` | `tmux-route`, `testkit` | `@cpd_view=statusbar` pane removed, `status*` options set back; the sizes are the `resize-pane` argv of `reset_steps` |
| `tui.rs::tui_respawns_a_dead_pane_of_the_layout_in_place_and_sets_the_hook_again` | `tui.rs` | `tmux-route`, `testkit` | needs the fake to mark a pane dead (`#{pane_dead}`; `unverified:` a `FakeTmux::kill_process` or `__ctl` op exists) and `set-hook -gu pane-died` to be a state change; asserts `respawn-pane` of the dead panes and the hook set again |

The 11 in the `tmux-route`+`testkit` rows, with the 11 live ones of the same set (9 `tui.rs`, `tui_chat_pad` 1, `tui_reset_layout` 1), are PR 4's cockpit batch. The dead helper code of these 3 `bare.rs` tests (`bare.rs` between `inside_the_cockpit…` and `a_server_that_is_not_older…`, e.g. the `events list` and `session open` helpers at lines 343 and 427-429) is still in the file; `unverified:` it is dead code; the migration Job restores or removes it.

## 4. Proposed migration PRs (4)

Order: fewest seams first. Test counts include the deleted ones where they land. 41 + 50 + 35 + 34 = 160 = 144 + 16. Each PR is 15 to 60 tests.

### `mig-tui-a-1` — needs `testkit` (41 tests)
Files: `tui_pulse.rs` (12), `tui_loop_guard.rs` (2), `tui_worker.rs` (9 renders), `tui_tree.rs` (8 view-only), `tui_tree_asleep.rs` (1 render), `tui.rs` (6 status-line + 3 harness).

1. **test** (S2): `tui_pulse.rs` (12) and `tui_loop_guard.rs` (2) from `TestEnv` to the testkit fixture (the loop-guard tests render the tree `View` to a `TestBackend`: the dot's colour and the row's red `palette::TIER_RED` are read from cells, not from `capture-pane -e`).
2. **test** (S3): the 9 worker render tests and `asleep_row_is_colour_7…` move to `tui/src/worker/` and `tui/src/tree/` as units on `TestBackend` (`the_roles_are_in_the_palettes_colours` reads `Cell::style` instead of tmux escapes).
3. **test** (S3): the 8 view-only tree tests move next to `tree/view.rs`'s 116.
4. **test** (S2): `tui.rs` status line: replace the six `Outer` tests with units that pin `STATUS_FORMAT` (`core/src/host/tmux.rs:256-322`): the option names, `long_fits!`, the `#{n:@cpd_preview}` + 12 and `#{n:@cpd_version}` + 3 arithmetic as the format string's text. Rendering is tmux's (§5).
5. **impl** (S1): delete the 3 harness self-tests of `tui.rs` (`an_unfaked_gh_is_denied_with_exit_97`, `a_command_has_no_gh_ssh_or_proxy_variable`, its `#[ignore]`d child) together with the `TestEnv` fake dispatcher; their contract (no `gh`/`pi`/`curl`/`ssh` is reachable, no ambient `GH_TOKEN`/proxy variable reaches a command) moves to `FakeRunner` (denied program) and `Ports.env` (`tests-mock-ports.md` §4.4, `unverified`).

### `mig-tui-a-2` — needs `p6` (includes `clk`) and `runner` (50 tests)
Files: `tui_tree.rs` (11 + 20), `tui_tree_delete.rs` (1), `tui_tree_asleep.rs` (2), `tui_worker.rs` (8), `tui_idle.rs` (3), `tui_chat_pad.rs` (1), `tui_runner.rs` (3), in-crate `widgets/mod.rs` (1).

1. **impl** (S2): `runner.rs`: `Restore<W: Write>` (and the terminal's raw-mode restore behind a trait) so the restore bytes go to an injected writer; `tui_runner.rs` becomes 2 units that `catch_unwind` a panicking body and an `Err` body and assert `DisableFocusChange`/`DisableMouseCapture` bytes and the leave-alternate-screen byte (§5).
2. **test** (S3): `tui_worker.rs` loop tests (8): follow, scroll, the `backed_off` test with `ManualClock` and an injected tty fact (§5), the two refusals (`the_job_form_of_an_unknown_job…`, `the_view_needs_a_terminal`).
3. **test** (S3): `tui_tree.rs` loop-only tests (11), with the watcher skipped and `poll_ms` driven by `ManualClock`; the snooze test with a fixed timestamp.
4. **test** (S3): `tui_tree.rs` command tests (20) on `FakeRunner` route `cpd`, with `tui_tree_delete` 1 and `tui_tree_asleep` 2 (the 500 ms sleep and the 4 s command become `ManualClock` advances).
5. **test** (S3): `tui_idle.rs` (3) on counters: draws (`TestBackend` flush count) and snapshot reads (a read counter in the view loop: new `p6` code, `unverified:` none exists) over a 5 s `ManualClock` window; `tui_chat_pad.rs` history view (1).
6. **impl** (S1): `widgets/mod.rs hostname()` through `Runner`, the test keeps its `/proc` fixture cases and drops the real `hostname`.

### `mig-tui-a-3` — needs `tmux-route` and `p6` (35 tests: the whole `testkit`+`p6`+`tmux-route` row)
Files: `tui_tree.rs` (26), `tui_tree_delete.rs` (2), `tui_tree_archive_manager.rs` (2), `tui_worker.rs` (1: `scrolling_leaves_the_end_and_q_closes_the_pane`, `kill-pane` of `$TMUX_PANE`), `bare.rs` (3 refusals/no-nest) and `tui.rs` (1: `tui_attach_without_a_terminal_is_refused_before_anything_is_touched`).

1. **impl** (S2): the popup-driven helper (`fake_dialogs`, `confirm`, `cancel`, `type_in`, `wait_for_popups` of `tui_tree.rs:474-530`) rewritten on `FakeTmux::on_popup`, one helper shared by the tree files.
2. **test** (S3): `tui_tree.rs` dialogs and confirms (13): `n`, `x`, archive chain, rename, delete.
3. **test** (S3): `tui_tree.rs` the other 13 (Copy id, jumps, merge spinner, stuck job, legend popup), `tui_tree_delete` 2, `tui_tree_archive_manager` 2 (`in_chat_slot` refusal kept as a stateful `FakeRunner` reply).
4. **test** (S2): `bare.rs` 3 refusals (tty fact false, `$TMUX` set), `tui.rs` 1, `tui_worker.rs` 1.

### `mig-tui-a-4` — needs `tmux-route`, `p6`, `spawn`, `run_with` (34 tests; the 3 `run_with` tests in the last Job)
Files: `tui.rs` (9 live + 13 deleted + 2 `run_with`), `bare.rs` (4 live + 3 deleted), `tui_chat_pad.rs` (1), `tui_reset_layout.rs` (1), `tui_worker.rs` (1 `run_with`). Count: 11 live + 13 deleted `tui.rs`-cockpit-and-friends (`tui.rs` 9, `tui_chat_pad` 1, `tui_reset_layout` 1; deleted `tui.rs` 13) = 24; `bare.rs` 4 + 3 deleted = 7; `run_with` 3. 24 + 7 + 3 = **34**.

1. **docs** (S1): one table in the PR description of the `split-window`/`resize-pane`/option argv sequence each cockpit test pins, to replace `assert_layout` (tmux's geometry, §2).
2. **test** (S3): `tui.rs` cockpit (9 live + 11 deleted): `tui` build, repair of missing panes, dead-pane respawn, `--respawn`, `action` (`reset`, failing action, status-bar click as `cockpit::action(range, client)`), pane title and border options as option strings (§5).
3. **impl** (S2): `Tmux::pid_exe` and `installed exe` behind an injected fn or the route (§3), `FakeSpawn::exec` recording the argv, a `Restart` fake.
4. **test** (S3): `bare.rs` 4 live + 3 deleted, `tui.rs` 2 deleted with stderr.
5. **test** (S2): `tui_chat_pad.rs` (1) and `tui_reset_layout.rs` (1) on the recorded `resize-pane`/option calls.
6. **test** (S2): the 3 `run_with` tests (`plan new` server options; `tui_view_routes_by_name`; the `usage` cases of `an_unknown_attempt_or_plan…`), after `run_with` lands.

## 5. Not migratable as asked

Each subject is OS or external behaviour. The proposal keeps policy [tests]: no real tmux, no real signal, no pty.

1. **The status line as drawn (6 tests: `tui.rs` `the_status_line_is_one_full_line_of_joined_pills_in_the_long_workers_form_from_50_columns`, `…_short_workers_form_below_50_columns`, `…_starts_and_ends_with_one_grey_line_cell…`, `…_fill_is_grey_and_spans_the_free_width…`, `…_shows_the_unpadded_preview_pill…`, `…_shows_no_preview_once_the_option_is_unset`).** They attach a real outer client (`Outer::attach`, `tui.rs:~696 status_line`) and read row 20 with `capture-pane -e`. The width and the fill come from tmux's format engine, including `#{P/i/:}` (the junctions), which the fake keeps "as written" (`cli/src/fakes/tmux/format.rs:2`). ADR 0013 already deletes these ("X tests: status rendering"). Proposal: U tests that pin `STATUS_FORMAT` as a string (option names, widths, the incident pill) and keep `scripts/preview-tui.sh` as the manual check; no smoke (a real tmux is forbidden).
2. **Pane title pill and border colours, and the click on a button (3 tests: `a_pane_title_is_an_unpadded_pill_grey_and_blue_when_focused…`, `a_pane_border_is_grey_and_the_focused_panes_border_is_blue`, `a_click_on_a_statusbar_button_runs_its_action_on_the_clicking_client`).** The colours are tmux's rendering of our option strings; the click is tmux's mouse binding running `run-shell`. Proposal: assert the option strings (`pane-border-format`, `pane-border-style`, the binding text from `core/src/host/tmux.rs`) through the route, and call `cockpit::action(range, client)` directly for the click.
3. **`a_killed_view_respawns_at_once_and_the_engine_is_started_again` (`tui.rs:237`).** A real `SIGKILL` of a pane process and tmux's `pane-died` hook. Proposal: the fake marks a pane dead; the cockpit's `respawn_dead` and the `PANE_DIED_HOOK` string (`core/src/host/tmux.rs`) are asserted (as the deleted `tui_respawns_a_dead_pane…`); the hook running `watch --ensure --wait` is the engine's and tmux's. Counted as `testkit`+`tmux-route` above.
4. **`tui_idle.rs` (3 tests).** Their oracle is the kernel: `/proc/<pid>/io` `rchar` (bytes read) and `syscw` (write calls) and CPU time of real processes. In-process there is no process. Proposal: count what the claim is about: `terminal.draw` flushes (`TestBackend` write count; `idle_views_write_nothing_to_the_terminal`) and snapshot reads (a counter in `read::tree/plan/widgets`; `idle_*_read_next_to_nothing`), over a `ManualClock` window with no Event. The "fat store reads megabytes" fixture stays (300 `FAT_JOBS` rows) and is asserted as "no snapshot ran", not as bytes. The macOS CPU-time branch (decision pro-d3) is dropped with the process.
5. **`tui_runner.rs` (2 tests + the `#[ignore]`d child `a_view`).** The subject is what a program asked of the terminal, read from tmux (`#{mouse_any_flag} #{alternate_on}`; the fake prints empty for those, G8). Proposal: `Restore<W: Write>` in `tui/src/runner.rs`: `Drop` writes the `Disable*` bytes to the injected writer, also while a panic unwinds (`catch_unwind` in the test) and after an `Err`. The raw-mode ioctl and the alternate-screen leave are `crossterm`'s and `ratatui::restore()`'s: not asserted (external); they run against a `Vec<u8>`/`TestBackend`. `unverified:` that `ratatui::restore()` prints to stderr when stdin is no tty: put the call behind the same trait.
6. **`tui_chat_pad.rs::the_status_line_is_one_row_and_the_panes_reach_the_row_above_it`.** A real 20-row client and tmux's window height 19 and pane bottoms: tmux's layout. Proposal: assert the options (`status on`, `status-format[0]` contains `─`, `status-format[1]` empty) through the route; drop the geometry. The second test (`the_history_view_leaves_the_last_row_of_its_pane_empty`) migrates to `TestBackend` (the pane is the `TestBackend`'s height; `unverified:` the history view has a loop entry reachable with a backend: `history/mod.rs:46 run(args)`).
7. **`tui_worker.rs::a_view_that_fails_to_start_backs_off_instead_of_looping` (`:440`).** It needs tmux's `pane-died` hook restarting the view and a 6 s wall-clock window. `backed_off` (`lib.rs`) sleeps `BACKOFF` (3 s) only when `on_a_terminal()`. Proposal: with `p6` (tty fact true, `ManualClock`) assert one 3 s sleep and the error returned, and that the message names `nosuch-a9`; the restart loop is tmux's and `PANE_DIED_HOOK` is pinned in `host.rs`.
8. **`bare.rs::in_a_terminal_it_builds_the_cockpit_starts_the_engine_and_attaches` and the other `exec_attach` paths (`cockpit.rs:507`).** `exec` replaces the process: no test can observe the result. Proposal: `exec_attach` takes `FakeSpawn`; the test asserts the `tmux -L <socket> attach -t cpd` argv (the route's G6 is the same gap).
9. **`bare.rs::a_real_older_tmux_server_is_restarted_onto_the_installed_tmux` (`:501`).** It needs two real tmux binaries of different versions. Proposal: the route answers `#{version}` `1.0` for the server and `-V` `3.6a` (G1), `Restart` is a fake; the real restart's sequence (`restart::restart`) is the engine's scope. Counted as `tmux-route`+`p6`+`spawn`.
10. **Flock (`env_without_engine` and `locks/watch.lock` held by `Flock::lock` + `std::mem::forget` in `tui_tree.rs:56,1873`, `tui_tree_archive_manager.rs:24`, `tui_idle.rs`).** `flock` is the OS's, on a real file. It is not in the policy's list of externals and works in-process on a tempfile if a second open file description conflicts (it does on Linux and macOS; `unverified:` read from `flock(2)`, not run). Proposal: keep it, hold the guard in the testkit fixture so it is dropped at the end of the test (the `mem::forget` leaks the descriptor until the process ends: a leak by the policy).
11. **The OS file watcher (`read::watch::Watch`, `notify`) and the stdin reader (`Pump::new(event::read)`) of the tree.** Real inotify/FSEvents and a thread blocked on stdin are externals and sources of flakes. Proposal: `p6` injects an event source and lets a test start the view with no watcher (the polling fallback of `tree/mod.rs:152-158`).
12. **`a_link_kinds_label_reads_whole_on_the_screen_through_tmux` (`tui_tree.rs:1888`).** The "through tmux" half is tmux's handling of OSC 8. Proposal: keep the cell check on `TestBackend` (the label's symbols are whole, no stray cell); the tmux half is dropped (counted as `testkit` only).
