# tests-mock: inventory of the second half of `cli/tests/tui` against the rule

Job `inv-tui-b`, attempt 1. Static analysis only (`rg`, `sed`, reading the sources); no test was run. Paths are relative to `cli/tests/tui/`
unless they start with `tui/` or `cli/`. Claims not read in the source are marked `unverified`.

The rule: tests test only our code; tmux, pi, gh, curl, ssh, the network and real waiting are mocked; git is mocked except adapter contract
tests; SQLite, files in the test's temp dir, our own scripts and processes our own shims start (all reaped) are allowed.
Classes: K keep, T needs the fake tmux, G fake git, C clock or event wait instead of real time, X tests an external, R render/input test that
becomes a unit test with no pane.

## Facts that hold for every test of the scope

- Every `TestEnv::new()/empty()` starts a real `sh teardown.sh watch <pid>` reaper that loops `sleep 1` while the test lives
  (`cli/tests/common/mod.rs:249-260`, `:230-231`), and its teardown polls with `sleep 0.05` (`:186`). This is a real process and a real
  wait in all 216 tests, including those that touch nothing else (`tui_read`). It is the harness's, allowed as "our own script", but it is
  the one process every test leaves for up to a second after it ends (see section 4).
- The first tmux call (`TestEnv::tmux`, `common/mod.rs:1044-1046`) starts a real private server (`-L cpdt-*`, under the env's `TMUX_TMPDIR`).
  `wait_until`/`poll`/`wait_screen` poll every 10 ms for at most `WAIT` = 30 s (`common/mod.rs:388-420`, `:510-515`), so each
  `wait_*` is a wall-clock poll of real tmux (`capture-pane`) or of a file or the store.
- No file of the scope runs the git CLI: `rg 'git_repo|local_origin|env\.git|Command::new\("git"\)'` finds nothing in them. The `git` word in the
  files is a URL, a Job key (`rs-git`) or a comment. So the class G does not occur here.
- The view is always the built `cpd` run as `cpd tui view <name>` (`CPD` = `env!("CARGO_BIN_EXE_cpd")`), with `cpd` on `PATH` replaced by
  `env.fake("cpd", ...)` (a recording shell shim, `common/mod.rs:1058`) when the test asserts the command the view ran. The `gh`, `pi`,
  `curl`, `ssh` are denied shims by default (`common/mod.rs:305`, `:758-761`).
- Cited TestBackend duplicates are in `tui/src` (647 unit tests pass in 2.8 s, per the Job).

## 1. Summary table

Counts equal `rg -c '#\[(test|tokio::test)' <file>`. "tmux API" names the harness entry. Sleeps are `thread::sleep` calls (shim sleeps in the text).

| file | tests | real tmux (API) | real git | own processes / signals | sleep or poll | other external | class |
|---|---|---|---|---|---|---|---|
| tui_tree.rs | 67 | `env.tmux` via `Pane::start` (`:266-289`), `new-session`/`capture-pane`/`send-keys`/`display`/`list-panes`/`respawn-pane`/`show-buffer`; 23 tests wrap the real tmux in a shim (`fake_dialogs` `:480`, `fake_tmux_without_popups` `:1812`, inline `:1773`) | none | fake `cpd` shim (`:455`); `Flock` held as the engine (`:58`, `:1955`); shim `sleep 4` (`:837`) | `thread::sleep` 3 s, 6 s, 66 s (`:1835`, `:1856`, `:1877`); `Instant` bound `< 3 s` (`:846`); snooze that ends in `+8 seconds` of real time (`:1949`); 92 `wait_*` sites | `ps -o pid=,args= --ppid` (`:968`) | K 1, R 55, T 6, C 5 (clusters below) |
| tui_tree_delete.rs | 3 | `env.tmux` via `Pane::start` (`:107`), `new-session`/`capture-pane`/`send-keys`; `fake_dialogs` shim (`:190`) | none | fake `cpd` (`:170`) | `thread::sleep(500 ms)` (`:266`); 9 `wait_*` | none | R 2, C 1 |
| tui_tree_archive_manager.rs | 2 | `env.tmux` (`:97`, `:102`, `:121`), `new-session`/`capture-pane`/`send-keys`; shim `fake_confirming_dialogs` (`:83`) | none | fake `cpd` (`:65`) | `wait_until` on a file `archived` (`:149`, `:166`) | none | R 2 (T for popups) |
| tui_tree_asleep.rs | 3 | `env.tmux` via `Pane::start` (`:59`), `new-session`/`capture-pane`/`send-keys` | none | fake `cpd`, `Flock` as engine (`:25`) | `thread::sleep(500 ms)` (`:149`); 4 `wait_*` | none | R 2, C 1 |
| tui_read.rs | 38 | none | none | `std::process::Command::new("true")` spawned and waited (`:1147`); `Flock` as engine (`:300`) | none (`Instant` at `:1354` is printed, not asserted) | none; reads `tui/src` source text at `:1263` (file read) | K 38 |
| tui_widgets.rs | 44 | `env.tmux` via `tmux_out` (`:642`): `new-session`, `split-window`, `new-window`, `set-hook`, `capture-pane`, `send-keys`, `display`, `list-panes`, `set-buffer`/`show-buffer`; `fake_tmux` shim (`:924`) | none | cockpit panes run `sleep 120` (`:658`, `:661`, `SLEEP_SECONDS`) | `thread::sleep` 8 (2 s `:736`, 600 ms x7 `:882`...`:1207`); 13 `wait_*` | none | K 29 (TestBackend, no tmux), T 15 |
| tui_plan.rs | 25 | `env.tmux` via `tmux_out` (`:81`): `new-session`, `capture-pane -p/-e`, `send-keys`, `split-window`, `respawn-pane`, `display`, `list-panes`; shim `:839` | none | shim `sleep 4` on `split-window` (`:839`) | `thread::sleep` 1 s, 3 s, 300 ms x2, 1500 ms (`:504`, `:526`, `:632`, `:688`, `:702`); 57 `wait_*` | `ps --ppid` (`:753`) | K 1, R 15, C 3, T 6 |
| tui_plan_cancelled.rs | 3 | `env.tmux` via `start_view` (`:42`), `new-session`/`capture-pane` | none | real `cpd job cancel`/`pr cancel` to prepare the store | 6 `wait_*` (`screen_when` `:51`) | none | R 3 |
| tui_plan_waiting.rs | 3 | shares `start_view` of `tui_plan_cancelled` | none | none | 4 `wait_*` | none | R 3 |
| tui_merge.rs | 16 | `env.tmux` via `start_on` (`:152`): `new-session`, `capture-pane`, `send-keys`, `has-session`, `run-shell -b`, `display-popup`, `display-message`, `list-clients`; real second server `Outer` (`:216-248`, `attach`); shim around the real tmux (`:113`) | none | `Running` child (`:608`); `Command::new("true")` (`:548`); fake `cpd` (`:136`) | shim `sleep 0.05` loop up to 1200 times = 60 s (`:69`, `:578`); `sleep 120` (`:390`); `CLOSES` = 15 s (`:31`); 11 `wait_*` | fake `gh` shim | R 4, K 2, T 10 |
| tui_history.rs | 5 | `env.tmux` via `Pane::start` (`:68`), `new-session`/`capture-pane`/`send-keys`/`list-panes` | none | none | `wait` (`:101`) | none | R 3, K 2 |
| tui_checklist.rs | 7 | `env.tmux` via `tmux_out` (`:14`): `new-session`, `capture-pane`, `send-keys`, `has-session` | none | none | 15 `wait_screen` + 3 `wait_until` | none | R 6, K 1 (T for the pane) |

Total 216 tests. Real git: none. Real network: none (gh/curl denied or shimmed).

## 2. Clusters

Job size: one Job = one Worker, about 150 turns, at most about 40 tests rewritten. The proposed seam is a fake tmux in front of
`core/src/host/tmux.rs` (the adapter the cpd code calls) plus a render/input unit test over the view state (as `tui/src` already has).
Naming the design is out of scope; the seam is named so the design Job can pick.

### tui_tree.rs (67)

| # | cluster | tests | pattern | external touched | fix and seam | Jobs |
|---|---|---|---|---|---|---|
| T1 | Render snapshot of a store | 8 | `tui_tree.rs:531,561,579,592,890,998,2164,2178` (first `the_tree_of_a_prepared_store`, last `the_selected_context_rows_name_is_blue...`) | tmux `new-session -x -y`, `capture-pane -p` (and `-e` for colour, `:313`) | R: draw the same state with `TestBackend` as `tui/src/tree/rows.rs`, `look.rs`, `margin_tests.rs`, `counter_tests.rs`, `tree/view.rs` do | 0.25 (all 8 duplicate; delete) |
| T2 | Key, click, menu and popup, running a recorded `cpd` command | 41 | `rg -n 'fake_cpd\(' tui_tree.rs` plus `wait_for_calls`; first `a_tree_taller...scrolls_with_the_wheel` (`:429`), last `a_refusal_of_an_item_command_is_logged` (`:2090`); ids 1, 7-18, 20-21, 28, 31-47, 52-53, 58, 61-65 | tmux `send-keys` (keys, `-l` SGR mouse `:390-407`), `capture-pane`; 23 of them `display-popup` + `list-clients` via a shim over the real tmux (`:480`, `:1812`) | R: `tui/src/tree/view.rs` already has unit tests on the same events with the same names, e.g. `n_opens_an_input_popup...` (`:892`), `x_opens_a_confirm_popup...` (`:927`), `archive_asks_to_confirm...` (`:1437`), `archive_of_the_only_live...` (`:1520`), `rename_opens_an_input_popup...` (`:1652`), `new_context_opens_an_input_popup...` (`:1764`), `a_click_on_each_kind_runs_its_action` (`:2109`), `a_click_on_a_pr_ready_item_whose_merge_runs...` (`:2215`), `a_right_click_on_an_item_row...` (`:2257`), `enter_on_an_item_jumps_as_a_click_does` (`:2429`), and `tree/actions.rs:563-568` pins the command words. Per-test overlap beyond these names `unverified`. Gaps become new unit tests; T (fake tmux with `display-popup`/`list-clients` answers) only for the popup-dialog hand-off, if kept | 1 |
| T3 | Follow Events and snapshot errors | 4 | `:902,939,975,1077` (`the_tree_follows_the_events_of_the_store` ... `the_tree_follows_the_archive_events`) | tmux capture; store polled by the view at `poll_ms = 50` (config `:67`); view snapshot every 500 ms (`tui/src/tree/mod.rs:80`) | R/C: a view test that feeds a snapshot and an event, no tmux, no waiting; `tui/src/tree/view.rs:1209`-`2079` and `tree/folds.rs` cover the state side | 0.25 |
| T4 | Wall-clock claims | 5 | `rg -n 'thread::sleep\|sleep 4\|8 seconds' tui_tree.rs`: `:834` (shim sleep 4 s and `< 3 s`), `:1821` (3 s), `:1848` (6 s), `:1865` (**66 s**), `:1944` (snooze `+8 seconds`) | real time: the product's own timers: engine-down told after `DOWN_FOR` 60 s (`tui/src/tree/view.rs:42`), snapshot 500 ms, snooze wake | C: inject the clock. `tui/src/tree/view.rs:2786-2900` already drives `watch_engine(secs(t0, 60), ...)` with a fake time, so `:1848` and `:1865` duplicate it (delete); `:1821` and `:1944` need an injected `Clock` (`core/src/clock.rs`) in the view; `:834` becomes a unit test with a command runner that does not return | 0.25 |
| T5 | Tests of tmux behaviour through the view | 6 | `:958` `the_tree_starts_no_subprocess` (`ps --ppid`), `:1127` `..._captures_the_mouse...` (`#{mouse_any_flag}`), `:1512` `copy_id_...` (`show-buffer`), `:1564` `..._survive_a_restart_of_the_view` (`respawn-pane -k`), `:1763` `a_pr_ready_item_whose_merge_runs...` (shim, marker file), `:1968` `a_link_kinds_label_reads_whole_on_the_screen_through_tmux` (OSC 8 through tmux's terminal) | tmux `display -p #{pane_pid}`, `display-message #{mouse_any_flag}`, `set-buffer`/`show-buffer`, `respawn-pane`, `display-popup`, `list-clients`; real `ps` | `:958` becomes a check that the view code starts no process (the `Command` injected; `tui_read.rs:1255` style) or is deleted; `:1127` pins the bytes written to the terminal (`tui/src/terminal_writes.rs`); `:1512` asserts the `set-buffer` argv on a fake tmux; `:1564` is `tui/src/tree/view.rs:1209` (delete); `:1763` T on the fake tmux (marker file is ours); `:1968` R on the `Buffer`'s link cells (`tui/src/link.rs`) | 0.5 |
| T6 | Folds file and no-terminal refusal | 3 | `:602` `the_view_needs_a_terminal` (K: the binary without a pty), `:1597`, `:1616` (`an_invalid_folds_file...`, `a_missing_folds_file...`) | tmux `new-session`; files only | `:602` keep; the two folds tests duplicate `tui/src/tree/folds.rs` (unverified per test), R | 0.25 |

(The 67 split as T1 8 + T2 41 + T3 4 + T4 5 + T5 6 + T6 3 = 67.)

### The small tree files

| cluster | tests | pattern | external | fix and seam | Jobs |
|---|---|---|---|---|---|
| `tui_tree_delete.rs` | 3 | `:221,245,271` (`delete_item_only_on_archived_or_asleep_rows`, `popup_lists_what_goes_and_n_keeps_it`, `y_deletes_the_plan`) | tmux `new-session`/`capture-pane`/`send-keys`, `display-popup` shim (`:190`); `sleep 500 ms` negative claim (`:266`) | R: the popup text and the command are unit tests of the tree actions (`tui/src/tree/actions.rs`); the negative "no `--yes` after N" becomes "no command was returned" with no sleep | 0.1 |
| `tui_tree_archive_manager.rs` | 2 | `:137,154` (`context_archive_*`) | tmux, a shim `cpd` that writes a file `archived` (`:149`) | R: the archive steps (slot freeing, parking) are the same as `tui/src/tree/view.rs:1520` | 0.05 |
| `tui_tree_asleep.rs` | 3 | `:121,141,154` | tmux capture/send-keys; `sleep 500 ms` negative claim (`:149`) | R (colour 7: `tui/src/tree/counter_tests.rs:98` already pins it, so `:121` is a duplicate) and input | 0.1 |

### tui_read.rs (38), K

| tests | pattern | external | fix | Jobs |
|---|---|---|---|---|
| 38 | `rg -c '#\[test\]' tui_read.rs`; first `the_tree_is_contexts_then_plans_then_lead_sessions_with_their_states` (`:313`), last `with_no_engine_the_snapshot_leads_with_engine_down...` (`:1387`) | bundled SQLite in the env's temp dir, `Flock`, own `true` child reaped (`:1147-1151`) | K; none to change. Only the `TestEnv` reaper is the harness's. The test at `:1255` reads `tui/src` source files (`rg`-style), allowed (files) but is a lint, not a behaviour test | 0 |

### tui_widgets.rs (44)

| # | cluster | tests | pattern | external | fix and seam | Jobs |
|---|---|---|---|---|---|---|
| W1 | TestBackend snapshots (no tmux) | 29 | `:250-613` (21), `:896`, `:1223-1333` (6), `:1346` (K: no terminal) | SQLite in the env | K. Most duplicate `tui/src/widgets/column.rs` and `legend.rs` (e.g. `an_unowned_pr_is_read_only...` `column.rs:482`, `the_job_statuses_listed_are_those_the_chart_draws...` `legend.rs:495`, `session_dots_lists_every_glyph...` `legend.rs:401`); a duplicate is a delete, not a fix (per test `unverified`) | 0 (deletes only, 0.25) |
| W2 | Pane input / render in the cockpit | 15 | `:704,725,744,763,776,809,867,937,1019,1043,1073,1097,1132,1192,1365`; `start`/`cockpit` (`:651`-`:668`) | tmux `new-session`, `split-window -h`, `new-window`, `set -p`, `set-hook pane-died`, `list-panes -F`, `send-keys -l` mouse, `capture-pane`, `display`; `fake_tmux` answers `list-clients` and `display-popup` and passes the rest to the real tmux (`:924`); 8 `thread::sleep` (2 s at `:736`, 600 ms at `:882,889,976,1038,1057,1157,1207`) | R for the click, wheel, menu and merge-icon cases (they exercise `widgets::column` hit areas, `tui/src/widgets/column.rs:917-926`); T for the cockpit focus tests (`go to lead` uses `select-pane`/`@cpd_session` options on real tmux) with a fake tmux; the 600 ms "nothing was logged" claims become an assertion on the returned action list; `:725` `q does not end the view` (2 s) becomes a key-handler unit test | 1 |

### tui_plan.rs (25), `tui_plan_cancelled.rs` (3), `tui_plan_waiting.rs` (3)

| # | cluster | tests | pattern | external | fix and seam | Jobs |
|---|---|---|---|---|---|---|
| P1 | Chart render | 11+3+3 = 17 | `tui_plan.rs:275,293,336,349,385,394,449,533,551,563,911`; all of `tui_plan_cancelled.rs` and `tui_plan_waiting.rs` (`:158,176,197`; `:27,51,82`) | tmux `new-session -x 1500` (wide), `capture-pane -p/-e`; `cpd job cancel`/`pr cancel` real child processes (cancelled file) | R: `tui/src/plan/flow.rs`, `plan/boxes.rs` already draw charts with a `TestBackend` and cover the same cards (`flow.rs:736,746`, `boxes.rs:436,567,639,797`); the store side is `tui_read` (K) | 0.5 |
| P2 | Follow, error and scroll-file | 7 | `:462,483,514,580,608,670,716`; `thread::sleep` 1 s (`:504`), 3 s (`:526`), 300 ms poll loop (`:632`), 1500 ms (`:702`); `respawn-pane -k` (`:704`) | real time: the product's refresh (3 s), `SAVE_EVERY` = 1 s (`tui/src/plan/mod.rs:292`), `COALESCE`; tmux `respawn-pane`, capture | C/R: `tui/src/plan/mod.rs` has `a_write_waits_a_second_after_the_last...`, `the_offsets_round_trip...` (`:795`), `an_invalid_file_is_refused...` (`:812`), `a_scroll_file_that_is_reset...` (`:1026`), `switching_plans_and_back_restores_the_offset...`; the sleeps are the negative claims they already cover | 0.5 |
| P3 | Worker pane open and no-subprocess | 6 | `:738` (`ps --ppid`, gh asks nothing), `:779,805,828,856` (`split-window`, shim `sleep 4` at `:839`, `Instant` `< 3 s` at `:852`), `:954` (`q`) | tmux `split-window`, `send-keys`, `list-panes`, `display`; real `ps` | T: the fake tmux records `split-window` argv; `:828` becomes a unit test with a non-returning opener (no 4 s sleep); `:738` as T5 `:958` | 0.5 |
| P4 | No terminal | 1 | `:970` `without_a_terminal_it_is_refused_before_anything_is_read` | none (binary without a pty) | K | 0 |

### tui_merge.rs (16)

| # | cluster | tests | pattern | external | fix and seam | Jobs |
|---|---|---|---|---|---|---|
| M1 | Dialog render and keys | 4 | `:256,266,282,296` | tmux `new-session -x 58 -y 5`, `capture-pane`, `send-keys`, click `-l` (`:206`) | R: `tui/src/merge/mod.rs` has 9 unit tests | 0.1 |
| M2 | Argv and no-terminal | 2 | `:309,335` | none (built binary, `merge::popup_args`) | K | 0 |
| M3 | Real client of a second tmux server | 3 | `rg -n 'Outer::attach' tui_merge.rs`: `:354,467`, and the `start_on` + `keep` session test at `:382` | a second real tmux server whose pane runs `tmux attach` (`:216-248`), `display-popup -E` on the real client, `run-shell -b`, `has-session`; `sleep 120` (`:390`); `CLOSES` = 15 s waits | T: assert the `display-popup` and `run-shell -b` argv on the fake tmux (`merge::popup_args` is already pinned at `:335`); "the popup is gone while gh blocks" is tmux's `run-shell -b` behaviour (section 3, X) | 0.5 |
| M4 | The action `cpd tui action merge` on a fake `gh` and a tmux shim | 7 | `:431,445,496,633,654,678,695` | `fake_tmux`/`fake_tmux_before` wrap the real tmux (`:113`; the failure paths call it on a client that does not exist), `gh` shim (a loop `sleep 0.05` up to 60 s waiting on `release`, `:69`, `:578`), a `Running` child (`:608`) | T: fake tmux that records argv (`display-popup`, `display-message`, `run-shell`); C: replace the `release` loop with a gh shim that returns, and the markers (`put_marker`) are files, so K | 0.5 |

(The 16 split as M1 4 + M2 2 + M3 3 + M4 7 = 16.)

### tui_history.rs (5), tui_checklist.rs (7)

| cluster | tests | pattern | external | fix and seam | Jobs |
|---|---|---|---|---|---|
| History render and keys | 3 | `tui_history.rs:120,141,151` | tmux `new-session`/`capture-pane`/`send-keys`/`list-panes` | R: `tui/src/history/view.rs:89-134` (5 unit tests: top line, body, scroll, `q`) already cover them (duplicates, per test `unverified`) | 0.1 |
| History refusals and no terminal | 2 | `:172,187` | none (the binary exits before drawing) | K | 0 |
| Checklist render, keys and outcome | 7 | `tui_checklist.rs:98,121,148,162,180,199,217` | tmux `new-session`, `capture-pane`, `send-keys`, `has-session`; the outcome file is ours | R: only `tui/src/dialog/view_tests.rs:561` covers checklists (a click); the rest are new unit tests on `Dialog` with `TestBackend`; `:217` (refusal of a short field) needs no pane (K) | 0.25 |

### Totals

Jobs, all clusters: about 5.5, in practice 5 Worker Jobs (the fake tmux itself and the plan/widgets/tree rewrites; counts above sum to
about 218 tests of which about 60 are deletions as duplicates `unverified`).

## 3. Tests of an external (X)

Delete, or turn into an adapter contract test on fixtures (`core/src/host/tmux.rs`), per the rule:

| test | why |
|---|---|
| `tui_tree.rs:1127` the tree captures the mouse | asserts `#{mouse_any_flag}`, a tmux format that reflects whether tmux parsed our DECSET; the product's part is the bytes it writes (`tui/src/terminal_writes.rs`) |
| `tui_tree.rs:1512` copy_id (`show-buffer`) | the second half asserts tmux's paste buffer (`:1524-1525`); ours is the `set-buffer` argv |
| `tui_widgets.rs:1073` copy link (`set-buffer -w`) | same (`show-buffer` at `:1091`); the name says `set_buffer_w_and_says_nothing` |
| `tui_tree.rs:958`, `tui_plan.rs:738` no subprocess | measure the pane's children with real `ps` and `#{pane_pid}`; ours is "the view starts no `Command`" |
| `tui_tree.rs:1968` link kind's label reads whole through tmux, `tui_widgets.rs:704`, `tui_plan.rs:449-458` (`]8;;` in `capture-pane -e`) | check tmux's OSC 8 and cell handling ("the link is drawn in chunks, so a capture can land mid-draw", `tui_tree.rs:1979`); ours is the link cells in the `Buffer` (`tui/src/link.rs`) |
| `tui_plan.rs:608-716` "frames can reach the pane in two writes (hs-n206)", the 300 ms "holds still" loop (`:632`) | waits for tmux to settle the pane; the product's state is the offset |
| `tui_merge.rs:349,382,467` popup closes while `gh pr merge` blocks, "the pending `run-shell -b` job is lost if the server exits" (`:387-389`) | tmux server semantics of `display-popup -E` and `run-shell -b`; ours is the argv we pass |
| `tui_merge.rs:216-248` `Outer` (a client of a second server) | a harness around tmux itself, not our code |
| `tui_tree.rs:1564`, `tui_plan.rs:670` `respawn-pane -k` restarts | tmux restarting a pane; ours is the file read at start (`folds`, `plan-scroll.json`) |

## 4. Leak and flake risks

| risk | where | why |
|---|---|---|
| A `sleep` shim child outlives the test | `tui_tree.rs:837` (`sleep 4`), `tui_plan.rs:839` (`sleep 4` before the real `split-window`), `tui_merge.rs:390` and `tui_widgets.rs:658,661` (`sleep 120` panes), `tui_merge.rs:69,578` (a gh shim waiting up to 60 s) | killed only by `TestEnv::teardown` (process group, `common/mod.rs:771-787`) or by the reaper; a Gate timeout or a panic in `Drop` leaves it for up to 120 s |
| Reaper process per test | all 216 tests | `sh teardown.sh watch` polls every 1 s (`common/mod.rs:231`); after the test ends it runs once more (up to 1 s) |
| A second tmux server | `tui_merge.rs:216-248`, `:354,467` | `Outer` kills its server on `Drop` (`:243-247`); the env teardown also kills `<socket>-outer` (`common/mod.rs:196`, `:601`), so a leak needs both to fail; the `-outer` socket path length limit is a stated risk (`common/mod.rs:601`) |
| Time limits that depend on the Mac's load (load 40-120) | 92 + 57 + 13 + 11 + 9 + 6 + 4 `wait_*` sites; `WAIT` 30 s is the one bound (`common/mod.rs:388`); `CLOSES` 15 s (`tui_merge.rs:31`) | a `capture-pane` is a process start per 10 ms poll, so the wait itself adds load; a pane that is slow to draw shows an old frame (the comments say so: `tui_tree.rs:1979`, `tui_plan.rs:638`, hs-n206) |
| Negative claims by sleeping | `tui_tree_delete.rs:266` (500 ms), `tui_tree_asleep.rs:149` (500 ms), `tui_widgets.rs:882,889,976,1038,1057,1157,1207` (600 ms), `:736` (2 s), `tui_tree.rs:1835,1856` (3 s, 6 s), `tui_plan.rs:504,526,702` | pass when the view is slow (the check ran before it acted), so they can miss a regression on a loaded machine; a flake in the other direction cannot happen, but they cost 24 s of real time |
| A positive claim with an upper time bound | `tui_tree.rs:846` (`< 3 s` while a 4 s shim runs), `tui_plan.rs:852` (`< 3 s`) | fail when the machine stalls for 3 s |
| The 66 s test | `tui_tree.rs:1877` | one test holds a worker for 66 s of real time and a tmux server |
| Real snooze wake | `tui_tree.rs:1949` (`strftime(... 'now', '+8 seconds')`) | depends on the wall clock and the view's wake; `unverified` how it behaves if the view is 8 s late |
| Directory leaks | `TestEnv` dirs in `$TMPDIR`/`/dev/shm` (`common/mod.rs:270-273`) | removed by `teardown`; a test killed by the Gate is cleaned by the reaper (`:230`) |
| `std::mem::forget(Flock)` | `tui_tree_asleep.rs:27`, `tui_tree_delete.rs`/`tui_tree_archive_manager.rs` (same helper) | holds `watch.lock` until the test process exits; lock is on a file in the env, so harmless, but it is a leaked file descriptor per test (the process exits at the end) |
