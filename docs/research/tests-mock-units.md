# tests-mock: inventory of the unit and crate tests (core, engine, tui, cli/src) against the rule

Attempt `inv-units-a1`. Static analysis (`rg`, `sed`, `nextest list`); no test was run. Rule: tests test only our code; tmux, pi, gh, curl, ssh, plannotator, the network, real git (outside a few adapter contract tests) and real waiting are mocked; no test leaks, flakes, sleeps or leaves an orphan. A claim I did not read in the source is marked `unverified`.
Counts are `rg -c '#\[(test|tokio::test)'` per file: 1,342 in 174 files. `nextest list` gives 591 (core) + 684 (tui) + 40 (engine) + 6 (cli) + 32 (core/tests; `hooks_idle` is `cfg(linux)`, so 31 on macOS): core has 13 more than `rg` from `macro_rules! conversion_tests` (`core/src/store/schema/tests.rs:1848`) and 2 fewer from `#[ignore]` (`core/src/host/exe.rs:429`, `core/src/store/verdict.rs:571`).

**Result.** 1,298 tests are class K, 21 class C, 9 class G, 14 class X (12 real tmux, 1 UDP, 1 flock/dup in `core/src/lock.rs:95`), 0 class T, 0 class R. Real tmux: 12 tests, all in `core/src/host/tmux/{junctions,palette}.rs`. Real git: 9 tests (`core/tests/{migrations,repin,hooks_idle}.rs`, `core/src/policy/origin.rs`). Real waits or wall-clock limits: 21 tests in 13 files. Unit tests have no reaper; only `cli/tests/common` has one (section 1b). A fake `tmux` already exists (`Tmux::with_program`, a script; 7 tests use it); there is no fake git and no clock or sleeper seam in `core/src/host/deadline.rs`, `core/src/lock.rs`, `core/src/session/run.rs`. The TUI render tests already run on a buffer (29 files use `TestBackend` or `Buffer`, checked with `rg -l`), so no TUI test needs a pane.

## 1. Summary table

Classes: K keep, T fake tmux, G fake git or git contract test, C injectable clock or event wait, X tests an external, R render without pane. `–` is none. Files with `0` tests only declare or help test modules. A file with several patterns says how many of its tests have each. `cli/src` has one test file (`cli/src/cli.rs`); `interface.rs`, `lib.rs`, `main.rs` have none.

| file | tests | real tmux | real git | own processes/signals | sleep or poll | other external | class |
|---|---|---|---|---|---|---|---|
| cli/src/cli.rs | 6 | – | – | – | – | – | K |
| core/src/args.rs | 7 | – | – | – | – | – | K |
| core/src/attempt/ledger.rs | 1 | – | – | – | – | – | K |
| core/src/attempt/read.rs | 3 | – | – | – | – | – | K |
| core/src/clock.rs | 1 | – | – | – | – | – | K |
| core/src/config/catalog.rs | 1 | – | – | – | – | – | K |
| core/src/config/repo.rs | 2 | – | – | – | – | – | K |
| core/src/config/tier.rs | 7 | – | – | – | – | – | K |
| core/src/context/distill.rs | 8 | – | – | `sleep 30` killed at 200 ms :990; `sh -c exit 3` :997 | `wait_limited` real 200 ms limit :990 (POLL 50 ms :46) | – | C |
| core/src/context/inforce.rs | 9 | – | – | – | – | – | K |
| core/src/context/record.rs | 3 | – | – | – | – | – | K |
| core/src/context/render.rs | 4 | – | – | – | – | – | K |
| core/src/driver/lock.rs | 2 | – | – | – | – | – | K |
| core/src/driver/run.rs | 4 | – | – | `true` child :777 | 10 ms poll ≤10 s :780-783 | – | C |
| core/src/driver/stall.rs | 6 | – | – | `sh -c "sleep 30 & wait"` :262, `sleep 30` :292, SIGKILL via `terminate`; scans the process table | 200×10 ms poll :263-269 | – | C |
| core/src/driver/worker.rs | 5 | names only (`retry_kill` closures) | – | – | `Duration::ZERO`; `Instant::now` as a base :453 | – | K |
| core/src/env.rs | 3 | – | – | – | – | – | K |
| core/src/error.rs | 6 | – | – | – | – | – | K |
| core/src/events/kind.rs | 4 | – | – | – | – | – | K |
| core/src/events/mod.rs | 3 | – | – | – | – | – | K |
| core/src/events/typed.rs | 3 | – | – | – | – | – | K |
| core/src/git/gate.rs | 3 | – | – | flock + `sleep 600` holder, SIGKILL :125-142; thread :100 | slots polled 5 ms :62, `wait_for` spin ≤120 s :70-76 | – | C |
| core/src/git/hooks/def.rs | 5 | – | – | – | – | – | K |
| core/src/git/hooks/exec.rs | 1 | – | – | – | – | – | K |
| core/src/git/hooks/template.rs | 2 | – | – | – | – | – | K |
| core/src/git/run.rs | 3 | – | – | – | – | – | K |
| core/src/home.rs | 10 | names only (`tmux_restart_*` paths) | – | – | – | – | K |
| core/src/host/addr.rs | 4 | – | – | – | – | `UdpSocket` bind+connect, 1 of 4 :82-86 (OS routing) | X (1), K (3) |
| core/src/host/deadline.rs | 4 | – | – | `sh` + `sleep 30` grandchildren :154,178,195; SIGTERM/SIGKILL | real 1 s deadline :180; 200 ms + 500 ms grace :196-198; `wait_gone` ≤5 s :142-150; `alive` reads `/proc` :138 (Linux) | – | C |
| core/src/host/exe.rs | 16 | – | – | `/bin/sh` scripts as fake `cpd` :312; `exec sleep 60` :363; re-execs the test binary :419 | real 200 ms limit :363; ETXTBSY retry `sleep 50 ms` :383 | writes then execs scripts (ETXTBSY) | C |
| core/src/host/git_guard.rs | 5 | – | – | – | – | – | K |
| core/src/host/layout.rs | 3 | – | – | – | – | – | K |
| core/src/host/limit.rs | 2 | – | – | – | – | – | K |
| core/src/host/pi/dirs.rs | 4 | – | – | – | – | – | K |
| core/src/host/pi/launch.rs | 13 | – | – | – | – | – | K |
| core/src/host/pi/packages.rs | 6 | – | – | – | – | – | K |
| core/src/host/pi/session_file.rs | 1 | – | – | – | – | – | K |
| core/src/host/tmux.rs | 20 | fake `tmux` script `dying_server` :1280 (1 test); 19 pure | – | fake script via `sh` | ETXTBSY warm-up `sleep 20 ms` ×100 :1303-1307 | – | K (fake exists) |
| core/src/host/tmux/junctions.rs | 9 | real: `Server::new` → `Tmux::with_tmpdir`; new-session, split-window, resize-*, select-layout, list-panes, display-message, set | – | real tmux server + panes `sleep 600` :38-47 | – | – | X |
| core/src/host/tmux/no_inbox.rs | 1 | – | – | – | – | – | K |
| core/src/host/tmux/palette.rs | 3 | real: `Server::new` :122-136; new-session, split-window, set-option, select-pane, display-message | – | real tmux server + panes `sleep 600` :131,134 | – | – | X |
| core/src/ids.rs | 15 | – | – | – | – | – | K |
| core/src/inbox/cmd.rs | 1 | – | – | – | – | – | K |
| core/src/inbox/model.rs | 11 | – | – | – | – | – | K |
| core/src/inbox/rules.rs | 58 | – | – | – | – | `std::env::set_var` :855,960 (process-global) | K (env risk) |
| core/src/inbox/team.rs | 14 | – | – | – | – | – | K |
| core/src/job/guard.rs | 2 | – | – | – | – | – | K |
| core/src/job/open.rs | 4 | – | – | – | – | – | K |
| core/src/job/spec.rs | 5 | – | – | – | – | – | K |
| core/src/lock.rs | 5 | – | – | thread :88 | `thread::sleep` 100 ms :89 | flock on temp files; dup(2) semantics :95 | C (1), X (1), K (3) |
| core/src/output.rs | 7 | – | – | – | – | – | K |
| core/src/plan/cancel.rs | 1 | – | – | – | `wait_until_ended` real 30 ms / 5 ms :218 | – | C |
| core/src/plan/display.rs | 0 | – | – | – | – | – | K |
| core/src/plan/fingerprint.rs | 5 | – | – | – | – | – | K |
| core/src/plan/jobs.rs | 3 | – | – | – | – | – | K |
| core/src/plan/list.rs | 2 | – | – | – | – | – | K |
| core/src/plan/mod.rs | 0 | – | – | – | – | – | K |
| core/src/plan/new.rs | 1 | – | – | – | – | – | K |
| core/src/plan/overlap.rs | 1 | – | – | – | – | – | K |
| core/src/plan/prs.rs | 1 | – | – | – | – | – | K |
| core/src/plan/render.rs | 10 | – | – | – | – | – | K |
| core/src/plan/row.rs | 1 | – | – | – | – | – | K |
| core/src/plan/show.rs | 2 | – | – | – | – | – | K |
| core/src/policy/mod.rs | 16 | – | – | – | – | – | K |
| core/src/policy/origin.rs | 5 | – | `git init`, `remote add`, `remote set-url` :95-110 (1 of 5) | `git` CLI | – | – | G |
| core/src/pr/cancel.rs | 1 | – | – | – | – | – | K |
| core/src/pr/feedback.rs | 3 | – | – | – | – | – | K |
| core/src/pr/gh.rs | 22 | – | – | – | – | – | K |
| core/src/pr/merge.rs | 2 | – | – | – | – | – | K |
| core/src/pr/open.rs | 2 | – | – | – | – | – | K |
| core/src/pr/pulls.rs | 12 | – | – | – | – | – | K |
| core/src/pr/status.rs | 6 | – | – | – | – | – | K |
| core/src/session/fork_wait.rs | 1 | – | – | – | – | – | K |
| core/src/session/moves.rs | 1 | – | – | – | – | – | K |
| core/src/session/port.rs | 14 | – | – | – | – | – | K |
| core/src/session/reload.rs | 4 | – | – | – | – | – | K |
| core/src/session/retitle.rs | 16 | – | – | – | – | – | K |
| core/src/session/row.rs | 1 | – | – | – | – | – | K |
| core/src/session/run.rs | 8 | – | – | `/bin/sh -c "exec sleep 60"` ×5 :411,428,449,461,480; `trap '' TERM` :483; SIGTERM/SIGKILL | 10 ms poll ≤10 s :416-422; sleep 100 ms :465, 300 ms :484; `stop_pi` 500 ms :485 | pid 1 probe :474 | C |
| core/src/session/state.rs | 4 | – | – | – | – | – | K |
| core/src/store/lock.rs | 4 | – | – | `true` child :251 | real `acquire` wait 200 ms :256 (1 of 4) | flock | C |
| core/src/store/mod.rs | 18 | – | – | – | – | – | K |
| core/src/store/schema.rs | 0 | – | – | – | – | – | K |
| core/src/store/schema/tests.rs | 75 | – | – | – | – | – | K |
| core/src/store/verdict.rs | 19 | – | – | – | – | – | K |
| core/tests/hooks_idle.rs | 1 | – | `git init`, `commit` :36-37 | `sh`, `nice`, `ionice` :9; `cfg(linux)` only | – | OS: /proc, ionice | G / X |
| core/tests/migrations.rs | 19 | – | `init --bare`, `clone`, `add`, `commit`, `push`, `checkout -b`, `remote set-url` :478-491,541 (3 of 19, via `Clone::new`) | `scripts/check-migrations-frozen.sh` :502 (git fetch, diff); `new-migration.sh` :571 (`date`) | – | – | G (3), K (16) |
| core/tests/repin.rs | 4 | – | `init`, `add`, `commit`, `checkout -b`, `merge`, `ls-files`, `status`, `diff` :96-162 (all 4) | `scripts/repin.sh` :141 | – | – | G |
| core/tests/tier_g.rs | 3 | – | – | – | – | – | K |
| core/tests/worker_tiers.rs | 5 | – | – | – | – | – | K |
| engine/src/doctor/mod.rs | 1 | – | – | – | – | – | K |
| engine/src/doctor/tools.rs | 2 | – | – | – | – | – | K |
| engine/src/watch/distill.rs | 2 | – | – | – | – | – | K |
| engine/src/watch/engine.rs | 0 | – | – | – | – | – | K |
| engine/src/watch/liveness.rs | 7 | – | – | – | – | – | K |
| engine/src/watch/mod.rs | 1 | – | – | `sleep 30` :128; SIGKILL :132; reaper thread :102 | `wait_until` 20 ms ≤30 s :117-121 | – | C |
| engine/src/watch/pill_texts.rs | 1 | – | – | – | – | – | K |
| engine/src/watch/pr.rs | 2 | – | – | – | – | – | K |
| engine/src/watch/report.rs | 9 | – | – | – | – | – | K |
| engine/src/watch/restart.rs | 2 | – | – | – | – | – | K |
| engine/src/watch/schedule.rs | 3 | – | – | – | – | – | K |
| engine/src/watch/titles.rs | 5 | – | – | – | – | – | K |
| engine/src/watch/workers.rs | 4 | – | – | – | – | – | K |
| engine/src/watch/workers/no_inbox.rs | 1 | fake `tmux` script (`with_program` :25) | – | `sh` script | – | – | K (fake exists) |
| tui/src/background.rs | 1 | – | – | thread | `thread::sleep` 200 ms :74; wall `<100 ms` assertion :78; 5 ms poll :65 | – | C (flake) |
| tui/src/chat.rs | 2 | – | – | – | – | – | K |
| tui/src/cockpit.rs | 5 | – | – | – | – | – | K |
| tui/src/dialog/input_tests.rs | 7 | – | – | – | – | – | K |
| tui/src/dialog/mod.rs | 0 | – | – | – | – | – | K |
| tui/src/dialog/scroll.rs | 10 | – | – | – | – | – | K |
| tui/src/dialog/tests.rs | 14 | fake `tmux` script `recording_tmux` :7-19 (5 of 14 use it) | – | `sh` script written then exec'd (ETXTBSY) | – | – | K (fake exists) |
| tui/src/dialog/validate.rs | 5 | – | – | – | – | – | K |
| tui/src/dialog/view.rs | 0 | – | – | – | – | – | K |
| tui/src/dialog/view_tests.rs | 24 | – | – | – | – | – | K |
| tui/src/failed_pill_tests.rs | 5 | – | – | – | – | – | K |
| tui/src/fold.rs | 13 | – | – | – | – | – | K |
| tui/src/history/view.rs | 5 | – | – | – | – | – | K |
| tui/src/icons_tests.rs | 7 | – | – | – | – | – | K |
| tui/src/input_box_tests.rs | 8 | – | – | – | – | – | K |
| tui/src/lib.rs | 0 | – | – | – | – | – | K |
| tui/src/link.rs | 8 | – | – | – | – | – | K |
| tui/src/logs/margin_tests.rs | 1 | – | – | – | – | – | K |
| tui/src/logs/mod.rs | 1 | – | – | – | – | – | K |
| tui/src/logs/view.rs | 10 | – | – | – | – | – | K |
| tui/src/menu.rs | 10 | – | – | – | – | – | K |
| tui/src/merge/action_d550.rs | 3 | – | – | – | – | – | K |
| tui/src/merge/marker.rs | 4 | – | – | `true` child :135 (2 of 4) | – | – | K |
| tui/src/merge/marker_d550.rs | 2 | – | – | `true` child :10 (1 of 2) | – | – | K |
| tui/src/merge/mod.rs | 9 | – | – | – | – | – | K |
| tui/src/moon_tests.rs | 9 | – | – | – | – | – | K |
| tui/src/no_notes.rs | 2 | – | – | – | – | – | K |
| tui/src/pace_tests.rs | 7 | – | – | thread | `sleep` 1200 ms :70 + wall `<250 ms` assertion :85 (1 of 7) | real clock `SystemTime::now` :76 | C (flake) |
| tui/src/palette.rs | 17 | – | – | – | – | – | K |
| tui/src/palette/colours.rs | 8 | – | – | – | – | – | K |
| tui/src/pill_colours.rs | 6 | – | – | – | – | – | K |
| tui/src/plan/boxes.rs | 30 | – | – | – | – | – | K |
| tui/src/plan/flow.rs | 35 | – | – | – | – | – | K |
| tui/src/plan/margin_tests.rs | 1 | – | – | – | – | – | K |
| tui/src/plan/mod.rs | 20 | – | – | – | – | – | K |
| tui/src/plan/pulse.rs | 9 | – | – | – | – | – | K |
| tui/src/read/dot.rs | 11 | – | – | – | – | – | K |
| tui/src/read/pulse.rs | 3 | – | – | – | – | – | K |
| tui/src/read/shared.rs | 1 | – | – | – | – | – | K |
| tui/src/runner.rs | 1 | – | – | – | – | – | K |
| tui/src/scroll.rs | 2 | – | – | – | – | – | K |
| tui/src/section.rs | 11 | – | – | – | – | – | K |
| tui/src/terminal_writes.rs | 1 | – | – | – | – | – | K |
| tui/src/trace_tests.rs | 7 | – | – | – | – | – | K |
| tui/src/tree/actions.rs | 9 | – | – | – | – | – | K |
| tui/src/tree/actions_by_user_tests.rs | 2 | – | – | – | – | – | K |
| tui/src/tree/ask.rs | 11 | – | – | – | – | – | K |
| tui/src/tree/counter_tests.rs | 4 | – | – | – | – | – | K |
| tui/src/tree/cursor.rs | 5 | – | – | – | – | – | K |
| tui/src/tree/fixing_tests.rs | 3 | – | – | – | – | – | K |
| tui/src/tree/folds.rs | 8 | – | – | – | – | – | K |
| tui/src/tree/inbox.rs | 19 | – | – | – | – | – | K |
| tui/src/tree/inbox_indent.rs | 2 | – | – | – | – | – | K |
| tui/src/tree/kinds.rs | 1 | – | – | – | – | – | K |
| tui/src/tree/look.rs | 19 | – | – | – | – | – | K |
| tui/src/tree/margin_tests.rs | 4 | – | – | – | – | – | K |
| tui/src/tree/menu.rs | 5 | – | – | – | – | – | K |
| tui/src/tree/mod.rs | 1 | – | – | – | – | – | K |
| tui/src/tree/rows.rs | 45 | – | – | – | – | – | K |
| tui/src/tree/slug.rs | 7 | – | – | – | – | – | K |
| tui/src/tree/view.rs | 117 | – | – | – | – | – | K |
| tui/src/updating_label.rs | 6 | – | – | – | – | – | K |
| tui/src/viewlog.rs | 2 | – | – | – | – | – | K |
| tui/src/widgets/column.rs | 37 | – | – | – | – | – | K |
| tui/src/widgets/host.rs | 7 | – | – | – | – | – | K |
| tui/src/widgets/legend.rs | 17 | – | – | – | – | – | K |
| tui/src/widgets/mod.rs | 4 | – | – | – | – | – | K |
| tui/src/worker/mod.rs | 3 | – | – | – | – | – | K |
| tui/src/worker/render.rs | 5 | – | – | – | – | – | K |
| tui/src/worker/view.rs | 6 | – | – | – | – | – | K |

### 1b. Shared harness `cli/tests/common/{mod.rs,checkout.rs}` (0 tests; used by 109 files)

Corrects the Job's premise: `TestEnv::empty` starts no tmux server, only a socket name and `TMUX_TMPDIR` (`mod.rs:722-760`); a server exists once `cpd` or `env.tmux` runs a tmux command.

| function (file:line) | real tmux | real git | process | sleep or poll |
|---|---|---|---|---|
| `TestEnv::empty` `mod.rs:722` (every `new`, `:714`) | – | – | `start_reaper` `:249`: one `sh` per env, group leader, loops `sleep 1` `:231`; needs the `sqlite3` CLI `:664`; writes deny shims for gh, pi, curl, wget, ssh `:305,749` (tmux, plannotator are **not** denied) | – |
| `TestEnv::teardown` `:774`, `Drop` `:1157`, `TEARDOWN_SH` `:123-243` | `set-hook -gu`, `list-panes`, `kill-server` `:201-207` | – | `sh teardown.sh`, `ps`, `perl` (macOS) | `sleep 0.05` ×20 per stop `:186` |
| `TestEnv::tmux` `:1044`, `kill_tmux_server` `:1036`, `real_tmux` `:1273` | yes, any subcommand | – | – | – |
| `TestEnv::command` `:839` (all of `cpd`, `cpd_in`, `cpd_with` `:937-947`, `run_ok` `:957`) | – | – | joins the env's process group | – |
| `git` `:964`, `git_repo` `:970`, `keyed_repo` `:997`, `github_origin` `:1005`, `local_origin` `:1018`, `drop_unreachable_origin` `:1025`; `checkout.rs:21` | – | init, add, commit once per binary, then `copy_tree` + `update-index --refresh` per call; remote add/remove, config `insteadOf`, update-ref, clone (`checkout.rs`) | `git` | – |
| `fake` `:1058`, `fake_pi`, `fake_gh` `:1106-1111` | – | – | `sh` shims; the record loop counts `*.rec` files | – |
| `fake_cpd_views` `:1085`, `fake_cpd_panes` `:1092` | pane programs | – | `exec sleep 120` (`SLEEP_SECONDS` `:75`) | – |
| `poll` `:409`, `wait_until` `:396`, `wait_until_for` `:401`, `wait_some` `:503`, `wait_screen` `:510`, `wait_whole_ticks` `:1147` | `wait_screen` reads `capture-pane` through a closure | – | – | `thread::sleep(10 ms)` `:418`, ≤30 s (`WAIT` `:388`) |
| `wait_for_flock_waiter` `:426` | – | – | reads `/proc/locks` (Linux only; `expect` panics on macOS, unverified by run) | via `wait_until` |
| `answer_reads` `:457` | – | – | FIFOs + scoped thread | `poll` 100 ms per read `:476` |
| `GitWrapper::run` `:578` | – | real git behind the wrapper | child in the env's group | `poll` ≤10 s `:582` |
| `backdate` `:381` | – | – | – | uses `SystemTime::now` |

## 2. Clusters

Job size: one Job = one Worker, about 150 turns, at most about 40 tests rewritten. Seam names are proposals; the design Job decides.

| # | pattern | tests | list with | external and its real limit | proposed fix and seam | Jobs |
|---|---|---|---|---|---|---|
| A | real tmux server in a unit test | 12 | `rg -n 'Server::new' core/src/host/tmux/` (first `junctions.rs:70`, last `palette.rs:214`) | `new-session`, `split-window`, `resize-window`, `resize-pane`, `select-layout`, `select-pane`, `set(-option)`, `list-panes`, `display-message -p`; panes run `sleep 600` | they assert cells that tmux itself computes from `STATUS_FORMAT` and the pill options: class X, see 3. Existing seam: `Tmux::with_program` (`tmux.rs`). Delete, or keep one contract test per format on recorded `display-message` output | 1 (12 tests) |
| B | real children and wall-clock limits of our process code | 21 | `rg -n 'Command::new\("(sleep\|sh\|true\|/bin/sh)"\)' core/src engine/src` plus `rg -n 'wait_gone\|adopt\(' core/src` | `sleep 30/60/600`, `sh -c`, SIGTERM/SIGKILL, `killpg`; limits: `deadline.rs` 1 s, 200 ms+500 ms grace; `distill.rs` 200 ms (POLL 50 ms `:46`); `exe.rs` 200 ms; `session/run.rs` 100/300/500 ms; `store/lock.rs` 200 ms; `plan/cancel.rs` 30 ms; `lock.rs` 100 ms | the limits are already parameters. Needed: a sleeper/clock seam under `host::deadline` (`POLL` `:70,126,148`), `distill::wait_limited` `:868-886`, `driver::stall` and `session::run::PiProc` (`try_wait`, `signal`), with a fake child that records signals; one contract test with a real, reaped child | 2 (about 12 + 9) |
| C | wall-clock assertion on a thread | 2 | `background.rs:78`, `pace_tests.rs:70-85` | 200 ms and 1.2 s real sleeps, asserts `<100 ms`, `<250 ms` | replace the sleeping job by a channel the test releases; `wait_for_next_frame(now_ms)` is already pure (`pace_tests.rs:25`) | 0.3 |
| D | real git CLI in temp repos | 9 | `rg -ln '"git"' core/tests core/src/policy` | `init`, `add`, `commit`, `checkout -b`, `merge`, `push` (local bare), `clone`, `remote add/set-url`, `ls-files --unmerged`, `status`, `diff`; scripts `check-migrations-frozen.sh` (`fetch`, `diff --name-status -M`), `repin.sh` (`rev-parse --show-toplevel`, `ls-files`, `add`) | scripts: a fake `git` on `PATH` (`sh`, canned output, like `TestEnv::fake`), one contract test of each script on fixtures; `origin.rs`: split `repo_key` into `origin_url` (`git/run.rs:232`) and a pure `parse_origin` (exists, `:16`) | 1 (9 tests) |
| E | process-global env | 3 | `rg -n 'set_var' core/src cli/src` | `PLANNOTATOR_URL_HOST` (`rules.rs:855,960`), `CPD_PLAN` (`cli.rs:323`) | pass the value as an argument; safe only under nextest (process per test, `.config/nextest.toml`) | 0.2 |
| F | script written then exec'd (ETXTBSY) | 8 | `rg -n 'recording_tmux\|dying_server\|a_refused_file_is_asked' core/src tui/src engine/src` | a `sh` script as `tmux` or `cpd`: `exe.rs:369` (1), `tmux.rs:1280` (1), `dialog/tests.rs:7` (5), `engine/.../no_inbox.rs:20` (1) | an in-process fake behind a trait instead of a script; the `Tmux` seam is `with_program(path)`, so a fake needs a trait there (design Job) | shared with the fake-tmux design |
| G | OS network or priority behaviour | 2 | `addr.rs:82`, `hooks_idle.rs:32` | UDP route lookup; `nice 19` and `ionice` of a child (Linux only) | see 3 | 0.2 |

Total: about 5.7 Jobs; B is the one with real design work (it needs a seam in three modules).

## 3. Tests of an external (class X)

- `core/src/host/tmux/junctions.rs` (9) and `palette.rs` (3): tmux expands `#[...]` styles and `#{...}`, draws pane borders, resizes; the product code is the string `STATUS_FORMAT`. Delete, or fixtures of recorded output as a contract test.
- `core/src/host/addr.rs:82-86`: the kernel's route choice for an unsent UDP `connect`; it asserts only `if let Ok(ip)`, so on a host without a route it asserts nothing. Delete (`choose` is already tested with injected detectors, `:56-80`).
- `core/tests/hooks_idle.rs:30`: that a child of `run_command` has nice 19 and the idle I/O class is the kernel and `ionice`; `idle_prefix()` (`core/src/host/priority.rs:8`) has no unit test. Replace by a test of the argv prefix; the file compiles to nothing on macOS (`:4`).
- `core/src/lock.rs:95` (`dropping_a_copy_of_a_descriptor_keeps_the_lock`): flock/dup semantics of the OS; keep as a contract test of the wrapper or delete.
- Not X, listed so they are not removed: `host/deadline.rs`, `driver/stall.rs`, `session/run.rs` signal tests test our escalation code on processes our shims start (allowed); they are class C for their waits.

## 4. Leak and flake risks

1. **No reaper in unit tests.** `.config/nextest.toml` fails a test that leaves a process holding its pipes (`leak-timeout` 100 ms) and kills it at 90 s, but nothing stops `sleep` children or a tmux server when the test binary is SIGKILLed (a timeout kill). Affected: `host/tmux/{junctions,palette}.rs` (server and `sleep 600` panes; `Drop` of `Server` `palette.rs:109` runs only on unwind), `session/run.rs:428,449,461,480,483` (`sleep 60`), `driver/stall.rs:262,292` (`sleep 30`), `host/deadline.rs:178,195`, `git/gate.rs:125` (`sleep 600` holding a flock), `engine/src/watch/mod.rs:128`.
2. **Children are not reaped when an assert fails first.** `session/run.rs:411` drops the `Child`; the kill is after the asserts (e.g. `:428-432`). `gate.rs:142`, `stall.rs:287,299`, `engine mod.rs:132` likewise. They then run 30-600 s.
3. **Vacuous on macOS.** `host/deadline.rs:138` reads `/proc/<pid>/stat`, which does not exist on macOS: `alive` is false, so `wait_gone` returns true at once and the two "grandchild outlived" asserts (`:188`, `:206`) prove nothing on pro. `driver/stall.rs` has a libproc branch (`:116-122`), and `driver/run.rs:98-106` too, so those are real there.
4. **Load-dependent timing** (load 40-120): `tui/background.rs:78` (`<100 ms` after a 200 ms sleeping job), `pace_tests.rs:85` (`<250 ms` between draws over 1.5 s), `driver/stall.rs:263-269` (the child must exist within 200×10 ms, then `assert_eq!(child.len(), 1)` `:276`), `git/gate.rs:70-76` (a spin of `yield_now` for up to 120 s: CPU burn), `deadline.rs:182,200` (`< 4 s` upper bounds), `exe.rs:365` (`< 20 s`), `store/lock.rs:258` (`< 5 s`). None of these appear in `docs/research/macos-gate-*.md` (checked with `rg`).
5. **ETXTBSY.** Tests write a script and exec it at once: `exe.rs:383` (retry with `sleep 50 ms`, 1 test), `tmux.rs:1305` (100×`sleep 20 ms`), `dialog/tests.rs:7-19` (no retry). `.config/nextest.toml` says one process per test makes the fork race impossible; under `cargo test` (threads) it is real (inference).
6. **Env and the user's home.** `policy/origin.rs:95-98` runs `git` with only `-C`: the user's `~/.gitconfig` and `GIT_*` are inherited (`TestEnv::command` scrubs them, this test does not; `core/tests` use `git_env`). `rules.rs:855,960` and `cli.rs:323` mutate the process environment.
7. **Self re-exec.** `exe.rs:419` runs the test binary again with `--ignored` (`exec_child` `:430`); one extra process, reaped by `.status()`; a failure inside it shows only as a status.
8. **pid reuse.** `dead_pid()` (`tui/merge/marker.rs:134`, `marker_d550.rs:9`, `store/lock.rs:251`, `driver/run.rs:777`) assumes the reaped pid is not reused during the test (inference: low risk).
9. **Harness (`cli/tests/common`)**: the reaper `sh` of each env polls `sleep 1` until its test process dies (`mod.rs:231`); a test that is killed leaves up to 1 s of `sh`, then the teardown runs. `wait_for_flock_waiter` needs `/proc/locks` (`:426`). The deny list lacks tmux and plannotator (`:305`), so a test that forgets a fake runs the real `tmux`.
