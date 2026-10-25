# Research: in-process migration of the tui tests, second half (`mr-tui-b`)

Scope: `cli/tests/tui/{tui_widgets,tui_read,tui_plan,tui_plan_waiting,tui_plan_cancelled,tui_merge,tui_logs,tui_dialog,tui_checklist,tui_history,inbox,inbox_rules}.rs` (201 tests, counted with `rg -c '#\[test\]'`) and the in-crate tests of `tui/src/{plan,dialog,merge,logs}` that start a process (4 tests, section 1b).

Read from code with `rg` and `read`; nothing was built or run. A claim marked `unverified` or `guess:` is not read from code. Seam names come from `~/.cpd/ref/tests-mock/` and are `unverified` against code (`Runner`, `FakeRunner`, `Ports`, `run_with`, `testkit` do not exist yet).

Seam vocabulary: `clk`, `runner`, `tmux-route`, `git-route`, `spawn`, `run_with`, `testkit` as in the Job, plus **`view`** = what `tests-mock-ports.md` calls ports-6 ("terminal and event source injected for `TestBackend`, clock for the view loops"; ADR 0013 line 62: views take `(events, clock, backend)`). The Job's list has no name for it, yet 20 + 1 + 14 tests of this scope cannot run without it (every `show` loop calls `event::poll`/`event::read` and draws on a `DefaultTerminal`: `tui/src/logs/mod.rs:88-123`, `tui/src/dialog/mod.rs:234`, `tui/src/history/mod.rs:73`, `tui/src/merge/mod.rs:253`, `tui/src/plan/mod.rs:429`). **The Lead must add `view` as a seam (or ports-6 as a dependency).**

`testkit` is read as: a temp `$CPD_HOME` with a store, no reaper process, no `cpd` binary (today `TestEnv::new()`). `run_with` implies `testkit`.

## 1. Inventory

### 1a. Test files

"Starts today" counts are `rg -c` hits in the file (a hit is a line; indicative, not tests).

| path | tests | what its tests start today | target |
|---|---|---|---|
| `cli/tests/tui/tui_widgets.rs` | 44 | `TestEnv` (all). 15 tests run the built `cpd tui view widgets`/cockpit in a pane of a private real tmux (`pane_env`, `start`, `cockpit`, `tmux_out` 37 hits, `wait_for_screen`/`wait_until` 54 hits); `fake_tmux` (line ~924) is a shell script that `exec`s the real tmux; `env.fake(..)` 8 hits; 8 `thread::sleep` "time-claim" waits (600 ms); `env.cpd` 1 (needs-a-terminal). The other 29 draw `widgets::draw` or the legend on a `TestBackend` (28) or call `popup_args` (1), 1 of them is `env.cpd` | U 11 (legend, palette: no store), I 33 (testkit / view / tmux-route), run_with 1 |
| `cli/tests/tui/tui_read.rs` | 43 | `TestEnv` + `Store` (library calls `read::tree/flow/widgets/worker`). 1 test (`a_command_that_changes_a_session_shows_in_the_next_snapshot`, l.862) runs the `cpd` binary twice (`session state`, `session rename`) with `write_tmux_config`+`fake_tmux`; 1 test (`the_engine_is_never_running_stopped_or_crashed`, l.1255) spawns `true` for a dead pid (`spawn_and_reap`); 1 test (`the_read_model_spawns_nothing_and_has_no_write_statement`, l.1356) scans the sources; 4 tests hold `locks/watch.lock` with `nix::fcntl::Flock` (l.315, 707, 745, 1502) | U 1, I 42 (testkit; 1 also run_with+tmux-route) |
| `cli/tests/tui/tui_plan.rs` | 25 | every test runs the built `cpd tui view plan` in a real tmux pane (`start_view`, `tmux_out` 37 hits, `screen_when`/`wait_until` 57 hits, 5 `sleep`); `centre()` (l.~745) builds a cockpit centre on the private server with an `env.fake("cpd")` shell script that `exec sleep`s for `tui view worker`; `the_view_starts_no_subprocess..` (l.730) runs `ps --ppid` and reads `env.calls("gh"/"pi")`; `..survives_a_restart..` (l.662) starts a second view process; `without_a_terminal..` (l.962) `env.cpd` | I 25 (testkit 12, view 7, view+runner 1, view+tmux-route 4, run_with 1) |
| `cli/tests/tui/tui_plan_waiting.rs` | 3 | real tmux pane + the built binary through `tui_plan_cancelled::{env, plan, start_view, screen_when}` (`wait_until` polls, `fake_gh`) | I 3 (testkit) |
| `cli/tests/tui/tui_plan_cancelled.rs` | 3 | same pane + binary; `env.fake_gh("exit 1")`; `pr cancel` through `env.cpd` (l.~176); `capture-pane -e` for colours | I 3 (testkit 2, run_with 1) |
| `cli/tests/tui/tui_merge.rs` | 16 | `fake_tmux_before` shell scripts (23 `fake` hits) wrapping the real tmux; `gh_noting` shell fakes for `gh`; `Running` (l.13) spawns the real `cpd tui action merge` as a child; `argv_of` runs `sh -c` to split a command line (test l.346); 10 real-tmux pane hits; 21 polls | U 4, I 12 (testkit 1, run_with 1, view+tmux-route 2, run_with+runner 5, run_with+runner+tmux-route 3) |
| `cli/tests/tui/tui_logs.rs` | 16 | 6 tests run `cpd tui view logs` in a real pane (`tmux_out` 15, `wait_until`/`wait_screen` 18); `fake_tmux` script (l.~365, `exit 1` for anything but `display`/`display-popup`) + `env.cpd` for the action test; 6 tests draw `Log` on a `TestBackend` over a store, 2 call `popup_args` | U 2, I 14 (testkit 6, view 6, run_with 1, run_with+tmux-route 1) |
| `cli/tests/tui/tui_dialog.rs` | 13 | 11 tests run `cpd tui view dialog <kind>` in a real pane and `send-keys`/click it (`tmux_out` 6, `wait` 32); 1 draws on a `TestBackend`; 1 is `env.cpd` (needs a terminal, usage) | U 12, I 1 (run_with) |
| `cli/tests/tui/tui_checklist.rs` | 7 | 4 pane tests (`send-keys`, `capture-pane`); 2 draw on a `TestBackend`; `an_items_file_with_a_short_field_is_refused` (l.217) runs `sh -c '<cpd> tui view dialog checklist … > file'` inside a tmux session and polls `has-session` | U 6, I 1 (run_with) |
| `cli/tests/tui/tui_history.rs` | 5 | `Pane` struct (l.~100) runs `cpd tui view history` in a real tmux session (`tmux_out` 3, `#{pane_dead}`, `wait` 7); 5 `env.cpd` | I 5 (testkit 2, view 1, run_with 2) |
| `cli/tests/tui/inbox.rs` | 15 | `TestEnv` + `Store`; every test runs the built `cpd inbox …`/`cpd events list` through `env.cpd` (4 hits, one process per call); `env.fake("date", …)` shell script (l.337, 346, 353); `Flock` on `locks/watch.lock` (l.125). No tmux, no sleep | I 15 (run_with 13, run_with+runner 2) |
| `cli/tests/tui/inbox_rules.rs` | 11 | `setup()` (l.16) runs `cpd context new` and `cpd plan new` (14 `env.cpd` hits) which start the Lead in the **real tmux** (`core/src/plan/lead.rs:68-78`: `Tmux::new(..).create_session/new_window`), with an `env.fake("cpd", …)` shell script that `exec sleep`s for `session run` and answers `watch --ensure`; then `cpd session state …` and `inbox::rules::conditions` (library) | I 11 (run_with+tmux-route) |
| **sum** | **201** | | |

Targets by letter: U 36 · I 165 · S 0 (S: see section 5, copies not moves).

### 1b. In-crate tests that start a process (`tui/src/{plan,dialog,merge,logs}`)

`rg 'Command::new|thread::sleep|#!/bin/sh|set_permissions'` over the four directories finds:

| path | tests | what it starts | target |
|---|---|---|---|
| `tui/src/dialog/tests.rs` (`recording_tmux`, l.7-20: writes an executable `#!/bin/sh` `tmux` and `Tmux::with_program`) | 3: `show_error_opens_a_text_popup_titled_with_what_failed_and_saying_why` (l.30), `a_failed_command_of_a_view_is_a_log_line_and_the_error_dialog` (l.42), `the_popup_opens_without_tmuxs_border_and_without_a_tmux_title` (l.117) | a shell script as tmux (fork+exec per call) | I (tmux-route: `FakeTmux` records `display-popup`, answers `list-clients` 100x30) |
| `tui/src/merge/marker.rs` (`dead_pid`, l.134: `Command::new("true")` + `wait`) | 1: the test that calls `dead_pid` (stale marker) | a real `true` process | U, no seam: use a constant pid no process has (`tui/src/merge/action_d550.rs:85` already writes `"pid": 999999999` for the same purpose; `alive` is `kill(pid, None)` at `marker.rs:115-118`); or `spawn` once `Spawn::alive` exists |

No `thread::sleep` in the four directories. `tui/src/plan/mod.rs`, `logs/` and `merge/dialog_tests.rs` start nothing.

## 2. Seams

### 2a. Minimum seam set per file (test counts)

Letters are the needs-sets of 2b. `testkit` is implied wherever a store is used.

| file | A `{}` | B `{testkit}` | C `{run_with}` | D `{run_with, runner}` | E `{run_with, tmux-route}` | G `{run_with, runner, tmux-route}` | H `{view}` | I `{view, runner}` | J `{view, tmux-route}` | total |
|---|---|---|---|---|---|---|---|---|---|---|
| tui_widgets | 11 | 18 | 1 | | | | 6 | | 8 | 44 |
| tui_read | 1 | 41 | | | 1 | | | | | 43 |
| tui_plan | | 12 | 1 | | | | 7 | 1 | 4 | 25 |
| tui_plan_waiting | | 3 | | | | | | | | 3 |
| tui_plan_cancelled | | 2 | 1 | | | | | | | 3 |
| tui_merge | 4 | 1 | 1 | 5 | | 3 | | | 2 | 16 |
| tui_logs | 2 | 6 | 1 | | 1 | | 6 | | | 16 |
| tui_dialog | 12 | | 1 | | | | | | | 13 |
| tui_checklist | 6 | | 1 | | | | | | | 7 |
| tui_history | | 2 | 2 | | | | 1 | | | 5 |
| inbox | | | 13 | 2 | | | | | | 15 |
| inbox_rules | | | | | 11 | | | | | 11 |
| **sum** | **36** | **85** | **22** | **7** | **13** | **3** | **20** | **1** | **14** | **201** |

How each letter is justified (file, test, call):

- **A, no seam, a unit test next to the code.** `tui_dialog` (12) and `tui_checklist` (6): the pane only forwards keys and clicks to `Dialog::press_key`/`press_mouse` (`tui/src/dialog/view.rs:435`, `:511`), draws with `Dialog::draw_in` (`:340`) and `show` writes the `Outcome` with `write_outcome` (`dialog/mod.rs:248`); all `pub(crate)`, so the tests move into `tui/src/dialog/` (the in-crate files `view_tests`/`select_tests`/`input_tests` already do this for 24 tests). `tui_merge` 266, 277, 293, 346: `on_key`, `on_mouse`, `draw`, `start_args`, `popup_args` of `tui/src/merge/mod.rs:97-181` are pure (in-crate `merge/dialog_tests.rs` has the same kind); l.346 replaces `sh -c` by the string the unit already builds (`sh_quote`). `tui_logs` 355, 463: `logs::popup_args` is a pure `pub fn` (`logs/mod.rs:49`). `tui_widgets` 11: the legend tests (1223, 1233, 1262, 1309, 1324, 1333) and 5 render tests with no `TestEnv` (265, 288, 418, 443, 461). `tui_read` 1356: reads source files, no process.
- **B, `testkit`.** A store in a temp home and a library call, no process: `tui_read` 41 (`read::tree/flow/chat_slot/widgets/worker`; includes `the_engine_is_never_running..` with a constant dead pid and the two `Flock` tests, a kernel lock on a temp file); `tui_widgets` 18 (`widgets::draw` on a `TestBackend`; includes 896 and the draw half of 704); `tui_logs` 6 (`Log` on a `TestBackend` over a store); `tui_merge` 307 (`stored_title`); `tui_history` 120, 141 (`history::View::{set_lines, draw}` are `pub`, `history/view.rs:23-40`); the plan chart: `tui_plan` 12 + `tui_plan_waiting` 3 + `tui_plan_cancelled` 158, 197 draw the same `Centre` (`plan/mod.rs:77`) that the in-crate `box_tests.rs`, `needs_tests.rs`, `margin_tests.rs` and `layout_sample` (`plan/mod.rs:723`) already draw on a `TestBackend`; they are pane tests only because nobody exposed the chart. Moved to `tui/src/plan/` (`Centre` is `pub(crate)`). `testkit` would also be needed in the tui crate (guess: dev-dependency).
- **C, `run_with`.** The test asserts the CLI contract (argv in, exit code and error JSON out): `cpd inbox list|show|snooze|dismiss` and `cpd events list` (`inbox.rs`, 13 tests); the needs-a-terminal/usage refusals (`tui_widgets` 1346, `tui_plan` 962, `tui_logs` 324, `tui_dialog` 337, `tui_merge` 320, `tui_history` 187 and 172), which need the private `run_with(argv, on_a_terminal)` of `cli/src/cli.rs` (`tests-mock-ports.md:91`; `on_a_terminal=false` replaces the real isatty); `tui_checklist` 217 (`args_invalid` JSON, today captured by a shell redirect: needs the `Output::Owned` sink, `ports.md:92`, unverified); `tui_plan_cancelled` 176 (`pr cancel`, `core/src/pr/cancel.rs` has no tmux call).
- **D, `run_with`+`runner`.** `inbox.rs` 332 and 346: `snooze … tomorrow` runs `date +%z` (`env.fake("date", …)` today; port-run2 per ports doc). `tui_merge` 442, 456, 644, 689, 706: the action `merge::action(home, tmux, pull, client, run_command)` (`merge/mod.rs:281`, `pub(crate)`) runs `cpd pr merge` and the poll through `RunCommand`, which run `gh` (`gh_noting` fakes `gh` today); called through `run_with(["tui","action","merge",..])`. That these five need no tmux is `unverified` (the success path of the tests' names opens no dialog; `tui_merge` 470/507/665 do).
- **E, `run_with`+`tmux-route`.** `inbox_rules` 11: `setup()` runs `plan new`, whose Lead goes into tmux (`core/src/plan/lead.rs:68-78`: `has_session`, `create_session`/`new_window`, `set_pane_session`); with `FakeTmux` no pane program exists, so the `exec sleep` fake `cpd` disappears. Whether `plan new` also calls `cpd watch --ensure` (the fake answers it) is `unverified` (`ensure_watch` is called only from `plan/lock.rs:33,139`). Cheaper alternative: replace `setup` by SQL rows for Context/Plan/Lead and keep `run_with` only for `session state`, which touches no tmux in `session/state.rs` (read: `tmux_pane` columns only): then these 11 are C. `tui_read` 862: `session rename` sets the pane title through tmux (`env.fake_tmux()` today; unverified what `rename` calls). `tui_logs` 380: `tui action logs --client` runs `display -p -c` and `display-popup` (`FakeTmux` answers a 200x60 client).
- **G, `run_with`+`runner`+`tmux-route`.** `tui_merge` 470, 507, 665: a refused/failed merge opens the failure dialog on the clicking client (`dialog::report`/`show_error_on`, `dialog/mod.rs:496-508`: `list-clients`, `display -p -c`, `display-popup`), 507 with a scripted `display-popup` failure (`FakeTmux` must be able to fail a call: `unverified`) and the marker-exists check at the moment of the popup.
- **H, `view`, no other seam.** The pane tests whose subject is the loop, with a store: `tui_widgets` 725, 744, 763, 809, 1019, 1366; `tui_plan` 478, 509, 572, 600, 662, 708, 946; `tui_logs` 155, 234, 292, 310, 333, 470; `tui_history` 151. They press keys/clicks/wheel and read the buffer; with a scripted event source and a `ManualClock` the 8 + 5 `thread::sleep` "time-claim" waits (`tui_widgets`, `tui_plan`) and the `wait_until` polls disappear (the loop returns after the last scripted event). Store faults (`tui_plan` 478 renames a table; 509 lowers the highest Event id) are SQL on the temp store, no seam. `tui_plan` 662 "restarts" the view: two loops in one test, no process. `tui_plan` 600 and 662 read and write the scroll file in the temp home.
- **I, `view`+`runner`.** `tui_plan` 730 `the_view_starts_no_subprocess_and_asks_gh_nothing`: asserts the view ran no `gh`/`pi` and no child. In-process the claim becomes "the loop makes zero `Runner` calls" (`FakeRunner` records; `view`).
- **J, `view`+`tmux-route`.** `tui_widgets` 776, 867, 1043 (click or menu "Go to Lead" runs `session open` in the view's own process, asserted through `moved_events` and `focused_session`: `FakeTmux` must model `list-panes @cpd_session` and the select/move of the chat slot: `unverified`), 937, 1073, 1097 (the popup args `display-popup … cpd tui view merge '<pr>' --client <c>` and `set-buffer -w -- <url>`, with a client list), 1132, 1192 (live marker: `std::process::id()` is the live pid, a constant is the dead one); `tui_plan` 771, 797, 820, 848 (worker pane beside the chat: `split-window`/`respawn-pane`/`join-pane` with `FakeTmux`); `tui_merge` 360, 393 (the confirm closes the view at once and issues `run-shell -b` with the action argv; the "merge then poll in the background" order moves to the D tests of `tui_merge`).

`clk` is not a minimum for any set: no test needs a clock the `view` seam does not carry. Exact times would need `Clock::utc` (ports-3): `inbox.rs` 312 and 332 bound `snoozed_until` by two `OffsetDateTime::now_utc()` calls, which also passes in-process; `clk` makes them exact (optional). `git-route` and `spawn` are needed by no test of this scope (`rg 'git'` over the 12 files finds no git; the only process liveness is `alive(pid)`, `read/widgets.rs:329` and `merge/marker.rs:115`, which a constant pid and the test's own pid cover).

### 2b. Groups by needs-set (this table decides the order)

| set | needs | tests | files |
|---|---|---|---|
| A | none | 36 | dialog 12, checklist 6, widgets 11, merge 4, logs 2, read 1 |
| B | testkit | 85 | read 41, widgets 18, plan 12, logs 6, waiting 3, cancelled 2, history 2, merge 1 |
| C | run_with | 22 | inbox 13, history 2, plan 1, cancelled 1, merge 1, logs 1, dialog 1, checklist 1, widgets 1 |
| D | run_with, runner | 7 | merge 5, inbox 2 |
| E | run_with, tmux-route | 13 | inbox_rules 11, read 1, logs 1 |
| G | run_with, runner, tmux-route | 3 | merge 3 |
| H | view | 20 | plan 7, widgets 6, logs 6, history 1 |
| I | view, runner | 1 | plan 1 |
| J | view, tmux-route | 14 | widgets 8, plan 4, merge 2 |
| **sum** | | **201** | |

Needs `run_with` (the CLI entry in process): C 22 + D 7 + E 13 + G 3 = **45**. Only a library call: A 36 + B 85 = **121**. Needs the view loop: H 20 + I 1 + J 14 = **35**. 45 + 121 + 35 = 201.

After which seam a PR can start: A and B after `testkit` only (121 tests: 60% of the scope); C and D after `run_with` + `runner`; E and G after `run_with` + `tmux-route`; H, I, J after `view`.

## 3. Deleted tests of the scope

`tests-mock-deleted.md` lists 230 tests (`rg` over it: `cli/tests/engine/doctor.rs`, `engine/watch.rs`, `engine/watch_inbox.rs`, `tui/bare.rs`, `tui/tui.rs`, others). **None is in this scope.** The only near matches: `cli/tests/engine/watch_inbox.rs::a_tick_with_no_change_writes_no_transaction` (engine tick, not the `cpd inbox` CLI of `cli/tests/tui/inbox.rs`) and the 3 + 12 tests of `cli/tests/tui/bare.rs` and `tui.rs` (cockpit build and restart, the other half of the tui area). Nothing to restore here; the migration PRs of this scope delete no test (each test moves or is replaced one-for-one, section 4).

| deleted test | target file | seam |
|---|---|---|
| (none in scope) | | |

## 4. Proposed migration PRs (4)

Order: fewest seams first. Counts sum to 201 (the 4 in-crate tests of 1b are extra, in PR 4 and PR 1). A test that moves out of `cli/tests/tui/` is deleted there in the same PR (moved, not removed).

### PR `mig-tui-b1`: units and renders on a store (needs: testkit) , 59 tests + 1 in-crate

Files: A + part of B: `tui_dialog` (12 of 13), `tui_checklist` (6 of 7), `tui_widgets` 11 A, `tui_merge` 4 A, `tui_logs` 2 A + 6 B, `tui_read` 1 (source scan), `tui_plan` 12 + `tui_plan_waiting` 3 + `tui_plan_cancelled` 2 (the chart on `Centre`). Count: 12 + 6 + 11 + 4 + 8 + 1 + 17 = 59; plus `merge/marker.rs` dead pid (constant) = 60.

1. docs: list the 59 tests with their new place (`tui/src/dialog/`, `tui/src/plan/`, `tui/src/merge/`, `tui/src/logs/`, `tui/src/widgets/`); note the testkit dev-dependency of the tui crate.
2. test + impl: dialog and checklist (18 tests) as unit tests on `Dialog` + `write_outcome` (no seam); delete the pane versions.
3. test + impl: merge (4), logs (8), widgets legend/palette (11), source scan (1) as units; replace `sh -c` (tui_merge 346) by the unit string; `marker.rs` constant pid.
4. test + impl: plan chart (17): `Centre` over `read::flow` of a testkit store drawn on a `TestBackend`; keep the SGR-colour assertions (`capture-pane -e`) as `Buffer` cell `fg`/`bg` assertions (`palette::CANCELLED`, as `tui_logs.rs` `selected_rows` already does).
5. impl: delete the replaced pane helpers (`start_view`, `screen_when`, `styled_screen`, `colours_of`) once unused.

### PR `mig-tui-b2`: the read model and the widgets draw (needs: testkit), 60 tests

Files: `tui_read` 41 B, `tui_widgets` 18 B, `tui_merge` 307.

1. docs: map the 18 widgets draw tests (including the draw half of 704: OSC 8 link cell at the right edge, as l.528 already asserts) and the 41 reads.
2. test + impl: `tui_read` onto `testkit` (no `TestEnv`; `Flock` tests stay, temp file; `the_engine_is_never_running..` uses a constant dead pid).
3. test + impl: `tui_widgets` draw tests onto `testkit`.
4. impl: remove `tui_widgets` pane helpers no longer used by the remaining 8 + 6 tests only if no test of PR 4 needs them (leave otherwise).

### PR `mig-tui-b3`: the CLI in process (needs: run_with, runner), 31 tests

Files: C 22 + D 7 + `tui_history` 2 B = `inbox` 15, `tui_history` 4, `tui_merge` 6 (320 + the 5 actions), and 6 refusals (`tui_plan` 962, `tui_plan_cancelled` 176, `tui_logs` 324, `tui_dialog` 337, `tui_checklist` 217, `tui_widgets` 1346).

1. docs: for each test the argv and the expected `Output`/error JSON; the Output sink for `Owned` commands (checklist 217).
2. test: `inbox.rs` (15) on `run_with`; the Fixture's `Flock` stays (temp file); `date` becomes a `FakeRunner` reply (332, 346).
3. test: the 6 refusals and `tui_history` (4: render 120/141 on `View`, refusals 172/187 on `run_with`), with `on_a_terminal=false`.
4. test: `tui_merge` actions (442, 456, 644, 689, 706) on `run_with` + `FakeRunner` replies for `gh pr merge`/`gh pr view`/the poll; the marker check inside the reply closure (no `Running` child, no polling).
5. impl: delete `gh_noting`, `Running`, `argv_of` from `tui_merge.rs`; delete `env.fake("date")` use.

### PR `mig-tui-b4`: the views with tmux and the Lead (needs: run_with, runner, tmux-route, view), 51 tests + 3 in-crate

Files: E 13 + G 3 + H 20 + I 1 + J 14; in-crate `dialog/tests.rs` 3 on `FakeTmux`.

1. docs: the `FakeTmux` commands each test needs (`list-clients`, `display -p -c`, `display-popup` with an injectable failure, `set-buffer -w`, `run-shell -b`, `list-panes -F #{@cpd_session}`, `select-pane`, `split-window`/`respawn-pane`/`join-pane`): `unverified` which exist in `fakes::tmux` (35 subcommands per ADR 0013 line 35).
2. test: `inbox_rules` 11 (prefer the SQL setup of 2a-E if `plan new` is not worth the Lead pane), `tui_read` 862, `tui_logs` 380, `tui_merge` 470, 507, 665 (G), and the 3 in-crate dialog tests.
3. test: `view` loops with no tmux (H 20): `tui_widgets` 6, `tui_plan` 7, `tui_logs` 6, `tui_history` 1; every `thread::sleep` and `wait_until` becomes "feed the events, read the buffer".
4. test: `view` + `FakeTmux` (J 14 + I 1): widgets 8, plan 4, merge 2 and `tui_plan` 730 (zero `Runner` calls).
5. impl: delete `fake_tmux`, `popups`, `moved_events`' tmux reads, `centre()`, `recording_tmux` and the fake `cpd`/`date`/`gh` scripts; the `view` seam is a prerequisite and is not in this Job.

Risk: 54 tests on four seams. If a Worker would not finish in 90 minutes, split along the lettered sets: (E, G, in-crate: 17 tests after tmux-route) and (H, I, J: 35 tests after view); this is the only split that keeps seam order (it makes 5 PRs, beyond the cap of 4, so it is the Lead's call).

## 5. Not migratable as asked

None of the 201 tests has an external's OS behaviour as its subject except the points below.

- **Mouse capture and terminal modes**: `tui_widgets.rs::in_a_pane_each_arrow_is_a_link_at_the_right_edge_and_the_view_captures_the_mouse` (l.704) asserts `#{mouse_any_flag}` of a real pty: the terminal's reaction to the escape sequences `run_view` writes (`tui/src/runner.rs`: raw mode, alternate screen, mouse and focus reporting). Proposal: keep the draw and OSC 8 half in PR 2 (B, `TestBackend`), drop the flag line here: `cli/tests/tui/tui_runner.rs` (header l.5: reads `mouse_any_flag` and `alternate_on` of the pane after the program ended; not in this scope, other half of the area) pins it; if the Lead moves that file, the assert belongs to the one smoke with the real binary on a pty that exists today only through tmux (`unverified`: no test of this scope may run real tmux).
- **`the_*_needs_a_terminal` (7 tests: `tui_widgets` 1346, `tui_plan` 962, `tui_logs` 324, `tui_dialog` 337, `tui_merge` 320, `tui_history` 187)**: the real `isatty` on stdin/stdout. They are migratable through `run_with(argv, on_a_terminal=false)` (C). Smoke proposal (S, copies, not moves, at most 3 per area): **one** `assert_cmd` run of `cpd tui view history l1` with piped stdio expecting exit 3 `not_a_terminal` (`tui_history.rs::the_view_needs_a_terminal`), and **one** `cpd inbox list` with its JSON on stdout (`inbox.rs`). No S for tmux (policy [tests]).
- **`Flock` on `locks/watch.lock`** (`tui_read.rs` l.315, 707, 745, 1502 via `engine()`; `inbox.rs::Fixture::engine`, l.172, 211, 229): `flock(2)` is a kernel call on a temp file the test creates, with no process; the lock is taken by the test's own process, which conflicts with a second open of the same file in the same process (flock is per open file description; `unverified` for `engine_running` in-process, but `tui_read.rs` already reads it in-process today). Keep real; counted in B and C.
- **`tui_merge.rs::the_popup_on_a_client_closes_at_once_on_the_confirm_while_gh_is_merging` (l.360) and `..the_confirm_closes_the_view_at_once..` (l.393)**: their subject is that the popup process ends while `gh` is still running in a detached `tmux run-shell -b`, that is, a real process outliving its parent. Proposal (policy [tests] keeps no real process): split the claim into (a) the view returns after issuing exactly one `run-shell -b <action argv>` (J, `FakeTmux` records, no wait) and (b) the action merges then polls (D). The "outlives" part is tmux's `-b`, an external; not tested.
- **`tui_widgets.rs::..go_to_lead..` focus asserts (`focused_session`, `moved_events`)** read real tmux pane state. Not an OS subject; they stay in J on the condition that `FakeTmux` models `@cpd_session` and the chat-slot move (`unverified`). If it does not, the tests keep only the `session-moved` Event assertion (`moved_events`) and drop the focus.
- **`tui_read.rs::the_read_model_spawns_nothing` (l.1356)** scans `tui/src/read/*.rs` for forbidden strings. It starts no process and stays as a unit (A); it is a source check, not a behaviour test.
