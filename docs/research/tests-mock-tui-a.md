# tests-mock: inventory of the first half of `cli/tests/tui` (Job `inv-tui-a`, Attempt 1)

Static analysis only: read with `rg`/`sed`; names from `scripts/cargo.sh nextest list --workspace -E 'binary_id(cpd::tui)'`. No test was run.
Rule: owner policy [tests] (pro-d6, pro-d9, pro-d10). A claim I did not check by reading the code is marked `unverified`.
Paths are relative to `cli/tests/tui/` unless they start with `core/`, `tui/`, `cli/`.

Counts are `rg -c '#\[(test|tokio::test)'` per file. That count includes the two `#[ignore]`d child tests (`tui.rs:1219`, `tui_runner.rs:19`) that nextest does not list (it lists 35 for `tui`, 2 for `tui_runner`).
`tui_titles.rs` is a helper module (`CAP_LEFT`, `title()`, no test) and `main.rs` the module list.
**git:** no file of the scope calls `git_repo`, `local_origin`, `keyed_repo`, `GitWrapper` or the git CLI (rg over the scope: no hit). Class G is empty here.

## 1. Summary table

Real-tmux APIs: `TestEnv::tmux` (`common/mod.rs:1044`, `tmux -L <socket> ...` on the env's private server); `cpd tui` / `cpd plan new` (cpd starts the server itself through `Tmux`, `core/src/host/tmux.rs:458`);
`Outer` (`tui.rs:504`, a second private server whose one pane is a real `tmux attach` client); `run_in_terminal` (`bare.rs:85`, a second server used as a pty); the `start*` helpers (a pane running `cpd tui view ...`).
Every `TestEnv::new` starts a reaper (allowed, our own shim) and its teardown runs `tmux kill-server` (`common/mod.rs:123-240`).

| file | tests | real tmux (API) | real git | own processes / signals | sleep or poll | other external | class |
|---|---|---|---|---|---|---|---|
| tui.rs | 36 | 31 start a server via `cpd tui`/`plan new` + `TestEnv::tmux`; 9 of them also `Outer` (up to 4 per test, `status_line` `:1016`); 1 probe `list-sessions` (`:994`); 4 none (`:1004`, `:1189`, `:1206`, `:1219`) | none | real engine (`cpd watch` in `cpd-work:watch`, started by `cpd tui`); `kill()` SIGKILLs a pane pid (`:249`); child test binary (`:1207`); `sleep 120` / `sleep 3600` panes | `wait_until` ×22 (30 s); `thread::sleep(500ms)` in `Outer::click` (`:535`) | tmux rendering, mouse, status format; `/dev/pts/N` opened (`:249`) | T 23, X 9, K 4 |
| bare.rs | 16 | `run_in_terminal` in 13; `TestEnv::tmux` in 15; a real `kill-server` by the old server's own binary | none | real engine, tick 1 s (`:34`) in 3 (`:672`, `:709`, `:733`); copies of the tmux and cpd binaries (`:242`, `:272`, `:332`) | `thread::sleep(3500ms)` ×3 (`:680`, `:685`, `:745`); `wait_until` ×9 (+ `wait_until_for`); `EXIT_WAIT` 90 s (`:96`) | `lsof -d txt` (macOS, `:151`), `/proc/<pid>/exe`; `/usr/bin/tmux -V` and a real older tmux (`:825`) | T 12, C 3, X 1 |
| tui_idle.rs | 4 | `start()` (`:115`): `new-session` ×5 panes, `capture-pane` | none | 5 real `cpd tui view` processes; noise thread writing the store every 300 ms (`:236`); `mem::forget(flock)` (`:38`) | `thread::sleep` 3 s + window 5 s (`CPD_IDLE_SECS`) (`:240`, `:255`); 100 ms sample loop (`:251`) | `ps -o time=` (`:149`, macOS); `/proc/<pid>/io`, `stat` (Linux, `:136`); the real widgets view runs `ps`, `vm_stat`, `ioreg`, `sysctl`, `nvidia-smi` or reads `/proc` (`tui/src/widgets/host.rs:43-127`) | C/X 4 |
| tui_worker.rs | 19 | `Pane::start_command` (`:94`): `set-option`, `set-hook pane-died`, `new-session`, `respawn-pane`, `send-keys`, `capture-pane`, `has-session`; 16 tests | none | `cpd tui view worker` in a pane; a shell loop under the pane-died hook (`:439`) | `sleep(6 s)` (`:447`); `wait_screen` | the hook re-run by tmux | R 15, C 1, K 3 |
| tui_dialog.rs | 13 | `start_titled` (`:26`): a new server per case (3 in `:162`, 4 in `:177`); `send-keys` (keys, raw SGR click and wheel), `capture-pane`, `has-session` | none | `cpd tui view dialog` in a pane | `wait_screen` / `wait_until` (32 lines) | tmux key and mouse parsing | R 12, K 1 |
| tui_pulse.rs | 12 | none | none | none (`TestEnv::new` only) | none: fixed `NOW` (`:12`) | none | K 12 |
| tui_logs.rs | 16 | `start()` (`:198`) in 5 tests (`:235`, `:293`, `:311`, `:334`, `:471`): `send-keys` ×12, `has-session` ×5, `capture-pane` | none | `cpd tui view logs`/`legend` in a pane; `fake_tmux` (`:369`) is a shim in 1 | `wait_screen` / `wait_until`; `poll_ms = 50` | tmux wheel/key parsing | R 5, K 11 |
| inbox.rs | 15 | none | none | none (engine lock held by `Flock`, `:126`) | none | `date` is a fake shim (`:337`); wall-clock `now` bracketed (`:300`) | K 15 |
| inbox_rules.rs | 11 | `setup` (`:16`) runs `cpd plan new`: a real server and a park window, in all 11 | none | fake `cpd` shim with `sleep` (`:19`) | none | none | T 11 |
| tui_loop_guard.rs | 2 | `screen_when` (`:60`): `set-option`, `new-session`, `capture-pane` | none | `cpd tui view tree` in a pane; `mem::forget(flock)` (`:23`) | `wait_screen` ×2; `poll_ms = 50` | none | R 2 |
| tui_runner.rs | 3 (1 ignored child) | `new-session`, `display-message -p #{mouse_any_flag} #{alternate_on}`, `capture-pane` (`:55-70`) | none | the test binary itself in a pane (`:55`) | `thread::sleep(50ms)` in the child (`:29`); `wait_until` ×2 | tmux's own record of terminal modes | R 3 |
| tui_titles.rs | 0 | none | none | none | none | none | helper |
| tui_chat_pad.rs | 2 | #1: `cpd tui` + a server `<socket>-pad` + `attach` (`:50-52`); #2 `new-session`, `send-keys G`, `capture-pane` | none | real engine via `cpd tui` (#1); `catch_unwind` + `kill-server` (`:54`, `:81`) | `wait_until` ×4 | tmux window size (19 rows), status row | X 1, R 1 |
| main.rs | 0 | none | none | none | none | none | module list |
| **total** | **149** | | | | | | K 46, T 46, R 38, X 11, C 8, G 0 |

## 2. Clusters

Size unit: one Worker Job (about 150 turns, at most about 40 rewritten tests). I do not design the fake tmux or the PR breakdown (out of scope).
Existing tmux seam: `Tmux` (`core/src/host/tmux.rs:458-570`) is a concrete struct; every call is `command()` (`:499`) = `<program> -u -L <socket> -f /dev/null <args>`, with `with_program` (`:483`) and `with_tmpdir` (`:474`). It is a process seam, not a trait: a fake is a program (a shim, as in `bare.rs:57`, `tui_logs.rs:369`) or a trait to add.
Time seam: `tui/src/pace.rs:19` (`wait_for_next_frame(now_ms)`, pure); `core/src/clock.rs` exists per the Job text, I did not read it (`unverified`).

### A. Cockpit build and repair (T): 23 tests of `tui.rs`
- Tests: `tui_builds_the_cockpit_once_and_starts_the_engine` (`:152`) … `tui_respawn_restarts_the_views_of_a_cockpit_for_a_caller_without_a_utf8_locale` (`:1263`). Lines: `:152`, `:218`, `:320`, `:328`, `:343`, `:378`, `:407`, `:450`, `:476`, `:764`-`:784` (5 repairs through `assert_repaired_after_killing`, `:733`), `:789`, `:809`, `:828`, `:880`, `:939`, `:994`, `:1166`, `:1243`, `:1263`.
- External: tmux. Subcommands the tests call: `set`/`set-option` (`-g`, `-p`, `-u`, `-gu`) ×22, `show -gv|-s|-v` ×14, `display -p` ×14, `resize-pane` ×7, `new-session` ×4, `new-window` ×3, `swap-pane` ×3, `select-pane` ×3, `kill-pane` ×3, `split-window` ×2, `set-hook`, `list-panes`, `list-windows`, `list-sessions`, `has-session`, `list-keys -T root`, `display-message`, `kill-server`, `capture-pane`. Called by cpd itself (`Tmux` methods, `core/src/host/tmux.rs:589-870`): `has-session`, `new-session`, `new-window`, `split-window`, `swap-pane`, `rename-window`, `respawn-pane`, `kill-pane`, `kill-window`, `resize-window`, `select-layout`, `set-environment`, `set-hook`, `bind-key`, `list-panes -F`, `display-message -p #{version}`, `kill-server`.
- Also real: `cpd tui` starts the engine in a pane (`:160` asserts `watch.started`), which polls a fake `gh` on a real tick. `kill()` (`:249`) SIGKILLs a pane's process while holding its tty to dodge a tmux 3.4 race (`:231-248`).
- Fix: a fake tmux (sessions, windows, panes, `@cpd_*` options, a recorded call list) behind the `Tmux` seam; tests assert the model and the calls. The engine start becomes a recorded `watch --ensure` call (the shim `cpd` does this for `session run`: `common/mod.rs:1092`).
- Hard parts: the `pane-died` hook runs inside tmux (`:343`, `:378`); `list-keys` output (`:199-208`); `capture-pane` of the placeholder view (`:178`, needs a running `cpd tui view chat`, becomes a view unit test); the server version (`:939`).
- Locale tests (`:1243`, `:1263`, 4 servers each) only pin that cpd passes `-u` (`core/src/host/tmux.rs:501`): a pure argv assertion.
- Size: 1 Job (23 tests).

### B. Bare `cpd` and the tmux restart (T, C, X): 16 tests of `bare.rs`
- Tests: `without_a_terminal_it_is_the_usage_error_and_does_nothing` (`:179`) … `a_real_older_tmux_server_is_restarted_onto_the_installed_tmux` (`:848`); the 16 `#[test]`s at `:178`, `:191`, `:201`, `:216`, `:238`, `:268`, `:290`, `:534`, `:583`, `:622`, `:671`, `:708`, `:732`, `:771`, `:800`, `:847`.
- External: a real tmux server restart (the old server killed by its own binary, `core/src/host/tmux.rs:494`, `:802`); a pty from a 2nd tmux server (`:85`); `attach` (a shim exits 0, `:57`); `lsof` / `/proc/<pid>/exe` (`:149`).
- tmux subcommands: those of A plus `list-clients -t =cpd -F #{client_pid}` ×2, `attach -t cpd`, `display -p #{pid}` and `#{version}` (answered `1.0` by shell, `:337-372`), `new-window`, `split-window`.
- Fix: the fake of A with a settable server version and a recorded `kill-server`; a pty (or an injected terminal predicate; `unverified` how cpd decides to attach, I did not read it). `wait_views_run` (`:165`, "every view runs this exe", via `lsof`) becomes "the recorded `cpd tui view` argv went through the new binary".
- C: three tests wait real engine ticks (`tick_seconds = 1`, `:34`): `:680` and `:685` (3.5 s each, `:672`), `:745` (`:732`), and `:709` waits `wait_until("the step to fail twice")` on 1 s ticks. Limit waited for: the engine tick, 1 s here. Fix: step the engine's tick with an injected clock instead of a `cpd watch` process (`unverified` that the tick is callable in-process).
- X: `:848` uses the real `/usr/bin/tmux` if older, else returns early ("NOT APPLICABLE", `:842`): a silent pass; it tests tmux's restart across versions.
- Size: 1 Job (16 tests, heavy fixtures: `fixture` `:436`, `older_server_run_by` `:337`).

### C. Status line, border and click through a real client (X): 9 tests of `tui.rs` + 1 of `tui_chat_pad.rs`
- Tests: `tui.rs:557`, `:641`, `:706`, the six `the_status_line_*` (`:1085`, `:1095`, `:1105`, `:1122`, `:1140`, `:1154`), `tui_chat_pad.rs:39`. List: `rg -n 'Outer::attach|status_line\(' tui.rs tui_chat_pad.rs`.
- External: tmux. Our format string (`STATUS_FORMAT`, `core/src/host/tmux.rs`) is expanded by tmux (`#{R:...}`, `#{?...}`, ranges, `MouseDown1Status`, `display-popup`, `capture-pane -e`). `Outer::click` waits 500 ms per click for tmux's double-click timer (`:535`).
- Fix: assert the format string (as `assert_status_line`, `tui.rs:127`, already does) and model the width choice of the workers pill (long from 50 columns, `:1073`); what tmux draws from it is tmux's. Or delete.
- Size: with D's X tests, 1 small Job, about 4 replacement tests.

### D. Views in a real pane, read with `capture-pane` (R): 38 tests
- `tui_worker.rs` 15 (`:166`-`:490`, rg `Pane::start|job_pane`), `tui_dialog.rs` 12 (`:115`-`:320`), `tui_logs.rs` 5, `tui_loop_guard.rs` 2 (`:79`, `:87`), `tui_runner.rs` 3 (`:74`, `:80`, child `:19`), `tui_chat_pad.rs:88`. Each starts a real server, runs the built `cpd tui view ...`, waits on `capture-pane` text (`wait_screen`, 10 ms poll, 30 s limit) and presses keys or SGR mouse bytes with `send-keys`.
- tmux subcommands: `new-session -d -x -y -P -F`, `capture-pane -p [-e]`, `send-keys [-l]`, `has-session` (closed pane = no session), `set-option remain-on-exit`, `set-hook pane-died`, `respawn-pane -k` (`tui_worker.rs:94-131`), `display-message -p #{mouse_any_flag} #{alternate_on}` (`tui_runner.rs:70`).
- Fix: ratatui `TestBackend` (already used, `tui_logs.rs:49`) with synthetic key/mouse events into the view's `key`/`mouse`. The loops read `crossterm::event` directly (`tui/src/worker/mod.rs:136`, `tree/mod.rs:239`, `logs/mod.rs:102`), so an event-source seam is needed in `tui/src` (size `unverified`). Not in `core/src/host`.
- Duplicates of the cpd-tui unit tests (matched by name and subject, assertions not diffed: `unverified` as exact equivalence):
  - `tui_dialog.rs` 12 of 13. `:115`, `:124` ↔ `tui/src/dialog/view_tests.rs` `every_kind_draws_a_rounded_frame_with_its_title_pill_on_the_top_border`, `confirm_draws_the_question_and_both_buttons`; `:162`, `:177`, `:192` ↔ `enter_or_y_confirms_and_esc_n_or_q_cancels`, `a_click_on_a_pill_acts_caps_included_and_nowhere_else_on_the_row_does`, `a_click_on_the_border_or_a_wall_does_nothing`; `:208`-`:260` ↔ `input_types_edits_and_moves_the_cursor`, `input_refuses_a_bad_value_inside_the_popup_and_the_user_keeps_typing`, `an_empty_input_is_refused`, `tui/src/dialog/input_tests.rs`; `:280`, `:320` ↔ `text_has_one_button_and_scrolls_with_keys_and_the_wheel`, `tui/src/dialog/scroll.rs`. The `--out` file is covered by `dialog/tests.rs` `a_result_file_is_read_and_deleted`.
  - `tui_worker.rs` about 9 of 15. `:166` ↔ `worker/render.rs` `a_session_file_is_the_conversation`, `a_fixture_session_file_renders_as_the_reference_does`; `:193` ↔ `the_palette_colours_who_speaks`; `:211`, `:254`, `:282`, `:309`, `:331`, `:347` ↔ `worker/mod.rs` `the_foot_of_an_attempt_form_names_the_attempt_and_never_the_cause`, `the_header_says_why_a_job_is_cancelled_failed_or_waiting`, `worker/view.rs` `the_header_is_pinned_wrapped_and_stops_at_five_rows`; `:420` ↔ `worker/view.rs` `scrolling_leaves_the_end_and_reaching_it_follows_again`, `only_q_and_escape_end_the_view`. Not duplicates (follow loop `worker/mod.rs:136-217`, no unit test found by name): `:378`, `:404`, `:453`, `:465`, `:478`, `:490`.
  - `tui_logs.rs` 5: `:235` ↔ `logs/view.rs` `keys_and_the_wheel_scroll_and_scrolling_to_the_bottom_follows_again`, `the_view_follows_at_the_bottom_and_stays_put_when_scrolled_up`; `:293`, `:311` ↔ `logs/view.rs` `the_error_takes_the_last_row_until_it_is_cleared`, `tui/src/runner.rs:131`; `:334` ↔ `tui/src/widgets/legend.rs`, `dialog/scroll.rs`; `:471` ↔ in-process `tui_logs.rs:418`, `:442`.
  - `tui_loop_guard.rs`: the click is `tui/src/tree/inbox.rs:834`; the red draw is `tree/look.rs` (exact match `unverified`). `tui_runner.rs`: not duplicated (`tui/src/runner.rs` has one unit test); the code is `Restore::drop` (`runner.rs:53`), which writes to stdout. `tui_chat_pad.rs:88`: `history/view.rs` has 5 tests, names not read.
- Size: 2 Jobs (about 25 of the 38 are deletable duplicates; about 13 need the event-source seam); 1 Job if duplicates go first.

### E. Waits on a real limit of the product (C): 8 tests
`tui_idle.rs` 4 (F), `bare.rs` `:672`, `:709`, `:733` (B), and `tui_worker.rs:440` `a_view_that_fails_to_start_backs_off_instead_of_looping`: `sleep(6 s)`, then "started at most 4 times". The limit is the view's back-off after a failed start (where it lives: `unverified`). Fix: back-off from an injected clock, or assert the recorded wait before the next start. Size: with B.

### F. `tui_idle.rs`: CPU of real idle TUI processes (C/X): 4 tests
- What they do (`:182-262`): start 1-5 real views, a thread writes the store every 300 ms, sleep 3 s, then sample `/proc/<pid>/io` (`rchar`, `syscw`; Linux) or `ps -o time=` plus screen text every 100 ms (macOS, pro-d3, pro-d4) over 5 s. Asserts: `rchar < 300 KB`, `syscw ≤ 0` (widgets ≤ 2 + window/2), CPU ≤ 20 % of the window on macOS (a `guess:` in the file, `:178-180`), a spinner writes ≥ 10 times (macOS ≥ 3 screens).
- What the product does when idle (read from the code): **it does not block on input.** Views wake on a timer: tree and plan `event::poll(pace::wait_now())` (`tui/src/tree/mod.rs:239`, `plan/mod.rs:515`; 1..=100 ms, `pace.rs:19`), worker `TICK` 100 ms (`worker/mod.rs:46`, `:136`), widgets 200 ms, 100 ms while a spinner turns (`widgets/mod.rs:54`, `:260`), logs `events.poll_ms`, default 200 (`core/src/config/file.rs:368`; `logs/mod.rs:102`). On a wake with nothing changed they must: not call `terminal.draw` (tree draws only when `redraw`, the frame or the Inbox age second changed, `tree/mod.rs:225-238`); not take a snapshot (500 ms after an Event only, `tree/mod.rs:80`; plan `PULSE_REFRESH` 60 s, `plan/mod.rs:67`); sample the host at most every 2 s (`widgets/mod.rs:60`). "No timer wakeups" is not the product's contract; "at most 10 wakes a second, no draw, no read" is.
- Assert without a real process: run the view loop over an injected event source and clock, with a counting `Backend` (`draw`/`flush`) and a counting reader (store reads). Feed N simulated ticks with no Event; assert `draws == 0`, `reads == 0`, host samples = N·tick/2 s, requested wait ≤ 200 ms. With a spinner shown: one draw per `palette::frame`. Pure parts already have unit tests: `tick` (`widgets/mod.rs:464-474`), `snapshot_due` (`tree/mod.rs:367`), `due`/`pulse_due` (`plan/mod.rs:638`, `:954`), `no_write_to_the_terminal_outside_the_frame` (`tui/src/terminal_writes.rs:93`). The missing piece is the loop itself: the same seam as D.
- Not assertable without a process: CPU time and bytes read (OS accounting, X). Delete `usage()`, `parse_ps_time`, the CPU share and the noise thread; the wake/read counts replace them.
- Size: 1 Job (4 tests + the loop seam shared with D).

### G. Store-only tests that start a tmux server by accident (T): 11 tests of `inbox_rules.rs`
`rg -n 'setup\(' inbox_rules.rs` = all 11 (`:37`-`:310`). `setup` (`:16`) runs `cpd context new` and `cpd plan new`, which create a real server and a park window (`session run` is a `sleep 120` shim, `:19-24`). The tests assert Inbox conditions from the store (`:30`); tmux is not under test. tmux subcommands (via `plan new`): `has-session`, `new-session`, `new-window`, `set -p @cpd_session`, `list-panes`.
Fix: the fake of A, or insert the rows with SQL as `tui_loop_guard.rs:33` does (no tmux, no cpd). Size: 1 small Job, or fold into A.

### H. Already allowed (K): 46 tests
`tui_pulse.rs` 12; `inbox.rs` 15 (`snooze 15m` brackets the real `now` with 1 ms slack, `:316-319`: the only load-sensitive spot; `date` is a shim); `tui_logs.rs` 11 (TestBackend, `popup_args`, the shim-tmux action test `:381`); `tui.rs` 4 (`:1004`, `:1189`, `:1206`, `:1219`); `tui_worker.rs` 3 (`:369`, `:504`, `:518`); `tui_dialog.rs:337`. The 5 in-process `tui_logs.rs` tests (`:76`-`:155`) look like duplicates of `tui/src/logs/view.rs` (10 unit tests): deletion candidates, matched by name only.

## 3. Tests of an external (class X)

| test | why |
|---|---|
| `tui.rs:557` click on a StatusBar button | tmux's mouse parsing, `MouseDown1Status` range lookup, double-click timer (real 500 ms); our code is `cpd tui action` (covered by `:476`, `:450`, `tui_logs.rs:381`) |
| `tui.rs:641` pane title pill | tmux expands `pane-border-format` and draws it; asserts SGR cells of `capture-pane -e` |
| `tui.rs:706` pane border colours | tmux draws the border |
| `tui.rs:1085`, `:1095`, `:1105`, `:1122`, `:1140`, `:1154` | tmux evaluates `#{?...}`/`#{R:─,...}` of our format for a client width and draws it |
| `tui_chat_pad.rs:39` | tmux's window height for a 20-row client (19) |
| `bare.rs:848` | tmux's own restart across versions; silent skip when no older tmux (`:842`) |
| `tui.rs:343`, `:378` (part) | pins tmux's `SIGCHLD`/utempter race and `pane-died` ordering (`:231-248`) as much as cpd's hook string |
| `tui_runner.rs:70` | tmux's `#{mouse_any_flag}`/`#{alternate_on}` as oracle of terminal modes; the code under test is `Restore::drop` |
| `tui_idle.rs` CPU and IO budgets | OS accounting (`/proc/<pid>/io`, `ps`), not our code |
| `bare.rs:149` `exe_of` | `lsof` and `/proc/<pid>/exe` as the oracle of "this pane runs the new binary" |

Fate (delete, adapter contract test on recorded tmux output fixtures such as `list-panes -F` parsing, `core/src/host/tmux.rs:869`, or a unit test of the format string) is for the design Job.

## 4. Leak and flake risks

The Gate runs at load 40-120 on this Mac; `WAIT` is 30 s (`common/mod.rs:388`), `bare::` waits 90 s (`bare.rs:96`; its comment says 66-77 s in the Gate, the file's measurement, not mine); nextest's slow limit is 30 s × 3 (`.config/nextest.toml:25`).

1. Many servers per test: `tui_dialog.rs:162` starts 3, `:177` 4; `tui.rs:1243`, `:1263` 4 each (one per locale) with an engine each; status-line tests up to 4 `Outer` servers; `bare.rs` 2 per test, up to 5 `run_in_terminal` in `:238`. Teardown (`common/mod.rs:123-240`) stops `<socket>` and `<socket>-*` under the env's TMUX_TMPDIR (verified in `stop_tmux`), so `-outer-*`, `-pad`, `-outerN` are covered. No scope socket name outside that pattern.
2. Long-lived shims: panes run `sleep 120` (`SLEEP_SECONDS`, `common/mod.rs:75`, "so leftovers of a killed run end by themselves") but `sleep 3600` panes (`tui.rs:221`, `:416`, `:648`, `:813`, `:837`, `:907`, `:1040`; `bare.rs:225`) outlive that bound if teardown never runs (a SIGKILLed test run). Whether the reaper stops them: `unverified` (they are in tmux's process group, not the env's).
3. `mem::forget(Flock)` (`tui_idle.rs:38`, `tui_loop_guard.rs:23`): the lock lives to the end of the test process; harmless under nextest (one process per test), `unverified` otherwise.
4. `tui_idle.rs`: a noise thread writes during the sleeps; a panic in the measured block is handled by `Stop` (`:214`). The macOS CPU budget (20 % of 5 s) measures the scheduler of a loaded Mac: load-dependent by construction.
5. `tui_worker.rs:447` counts starts in 6 s: under load the hook fires late, so the count only goes down; the `≥ 1` bound is the only guard, so the test can pass without proving back-off (`unverified`).
6. `bare.rs:680`, `:685`, `:745`: "nothing happens in 3.5 s" absence tests with plain `thread::sleep` (annotated `time-claim`). Under load fewer than three 1 s ticks may run in the window, so "held for three ticks" is not shown, and a late kill after the window is invisible.
7. `Outer::click` sleeps 500 ms per click (`tui.rs:535`): 1.5 s of wall clock in `:557`.
8. `bare.rs:848` passes without running anything when `/usr/bin/tmux` is not older than the PATH tmux (`:825-842`): green on a machine that never ran it.
9. `bare.rs:151`: one `lsof` subprocess per 10 ms poll over 4 panes (`wait_views_run`, macOS).
10. A real engine runs in every `cpd tui` test (`tui.rs` `env()` users, `bare.rs:34`, `tui_chat_pad.rs:16`): a `cpd watch` process polling a fake `gh`; `pane-died` supervision restarts it if killed, so the order of hook removal in teardown matters (`common/mod.rs:219`).
11. `tui.rs:249` `kill()` holds a pane's `/dev/pts/N` open until its guard drops; an assertion failure between `kill` and `drop` drops it by unwinding (fine). Reuse of that tty by another env in the window: `unverified`.
12. `tui_runner.rs:55`: a child test binary in a pane followed by `; sleep 120`: the pane lives until `kill-server`.
13. `tui.rs:1207` runs the test binary as a child and waits (`common::output`): reaped, allowed.

## 5. Totals

K 46 (keep; about 5 duplicates deletable), T 46 (A, B, G: about 3 Jobs once the fake exists), R 38 (D: 1-2 Jobs, most deletable), X 11 (delete or contract fixtures: 1 small Job), C 8 (B, E, F: 1-2 Jobs), G 0.
