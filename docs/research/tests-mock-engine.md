# tests-mock: inventory of `cli/tests/engine` and `cli/tests/store` against the rule

Static analysis only (rg, sed, `nextest list`); no test was run. A claim I did not read in the source is marked `unverified`.
Paths are from the repo root; `common/mod.rs:N` is `cli/tests/common/mod.rs`. Time of writing: main at `94eef646`.

The rule: tests test only our code; tmux, pi, gh, curl, ssh, plannotator, the network and real waiting are mocked; git is mocked
except in a few adapter contract tests; no test leaks, flakes, sleeps or leaves a process.

Test counts: `rg -c '#\[(test|tokio::test)'` equals `nextest list` for every file (engine 191, store 110). The Job text said 186 for
engine: the real count is 191 (121 + 32 + 38 in the small files). `store.rs:1239 child_writer` is counted but is not a test
(see 3).

## 0. How the harness shapes every row (read once)

- `TestEnv::empty/new` (`common/mod.rs:714-770`) makes a temp dir, starts a **reaper** `sh teardown.sh watch <test pid>` in its own
  process group (`:249-260`; its loop is `sleep 1` while the dir exists, `TEARDOWN_SH`) and writes a denied-tool shim for each
  of `DENIED_TOOLS` (gh, pi, curl, wget, ssh; `:305`). `Drop` kills the reaper and runs `teardown.sh` (`:774-788`): `kill`, `sleep 0.05` loops (up to 1 s), `rm -rf`.
  Every test that makes a `TestEnv` therefore starts 2 shell processes of our own and reaps them. Own shim: allowed, but not free.
- `TestEnv::new` needs the **system `sqlite3` CLI** (`common/mod.rs:664-672`, used by the teardown to read `attempts.driver_pid`)
  and, on macOS, `perl`, `ps`, `flock` shim (`:641`). That is an external that is not the bundled rusqlite. Not a test of it, but a hard
  dependency of every row below.
- **No tmux server starts unless a test (or `cpd watch --ensure`, `cpd tui`, an E3 driver) creates one.** `TestEnv::tmux`
  (`:1044`) is the one API that runs the real `tmux -L <cpdt-socket>`; `tmux_socket` is put in `config.toml` by every engine fixture
  so the engine's own `Tmux::new(socket)` (`engine/src/watch/engine.rs:380`) talks to it. The teardown stops it (`stop_tmux`).
- Seams today: `core/src/host/tmux.rs` is a concrete `Tmux` struct that runs `Command::new(program)` (`:58-59`), with
  `with_program` (`:483`) as the only substitution point; no trait. `core/src/git/run.rs:57` is `Command::new("git")` behind a free
  function; no trait. `core/src/clock.rs` only formats times (`timestamp`, `now`); it is **not** an injectable clock. Product time
  overrides exist as env vars (`host/limit.rs`: `CPD_TEST_*`, debug builds): `PR_POLL`, `DEADLINE_<TOOL>`, `DOCTOR_DEADLINE`,
  `DRIVER_POLL`, `LEAD_POLL`, `KILL_PAUSE`, `FILE_POLL`, `SOURCE_POLL`; the engine tick is `watch.tick_seconds` (minimum 0.1 s).
- Waits: `wait_until`/`poll` (`common/mod.rs:396-421`) poll every 10 ms up to `WAIT` = 30 s (`:388`); `wait_whole_ticks`
  (`:1147`) waits for `n+1` fake-`gh` search calls, one per tick (0.1 s, `TICK_SECONDS` `:79`) when the engine runs with
  `PR_POLL_EVERY_TICK`, else one per `pr_poll_seconds` (whole seconds).
- **Every engine test runs real tmux without creating a server**: each tick the engine calls `tmux list-panes -a` (E2
  `engine/src/watch/liveness.rs:372`, E13 `sleep.rs:87`, E10 `titles.rs:150`) and `server_running` (workers, `workers.rs:95`)
  against `PATH`'s `tmux` and the config socket. With no server the real binary exits fast (unverified how the engine reads it; it
  is a real `tmux` fork per tick). Counted as "tmux (engine-side)" below.

## 1. Summary table

Legend. tmux: **`TestEnv::tmux`** = tests create sessions/windows/panes with the real binary; **ensure** = through `cpd watch
--ensure` / `cpd tui` (the product creates the server); **engine-side** = only the engine's own per-tick calls above, no server;
**fake** = a shim on `bin/`. git: **fx** = `Fx` set-up: `TestEnv::git_repo` (`common/mod.rs:970`: copy of a template, `git
update-index --refresh`), 3x `git config`, `create_lead_worktree` (`git worktree add -b`, `worktree.rs:569`) (`watch.rs:133-141`).
Processes: **engine** = real `cpd watch` child (`Engine`, `watch.rs:63-98`, SIGKILL on drop). Class letters as in the Job; a test
has one class (the most demanding); counts in the last column are exact only where I list them.

| file | tests | real tmux | real git | own processes / signals | sleep or poll | other external | class |
|---|---:|---|---|---|---|---|---|
| engine/watch.rs | 121 | `TestEnv::tmux` 44 tests (see 2.1-2.4); ensure 15; engine-side all | fx in ~106 tests; `git` direct in 1067, 1126, 2703 | engine in ~100; E3 drivers detached `setsid` (8); `cpd pr poll` per tick; signals 4 | `wait_until` ~100; `wait_whole_ticks` 40 tests; `thread::sleep` 6 (`:639 :685 :807 :846 :874 :1412`); shim `sleep` 10 | none (gh, pi, cpd are shims `:127-129`) | 44 T (8 also G), 23 C, 4 K install, 3 K check, rest K/C/G mixed: see 2 |
| engine/doctor.rs | 32 | fake `tmux` shim | fake `git` shim | `node` real (allowed), `sleep 120` shim x4 killed by product group kill | `Instant` once (`:782`); deadlines 5 s, 1 s, 0.3 s, 5 s (`:725-786`) | `node` ESM probe (allowed: test runner) | 28 K, 4 C |
| engine/watch_curate_inbox.rs | 5 | engine-side | none | engine | `wait_until` | fake gh/pi | K |
| engine/watch_distill_inbox.rs | 1 | engine-side | none | engine | `thread::sleep(2*0.1+1 s)` `:103` | fake gh/pi | C |
| engine/watch_inbox.rs | 10 | `TestEnv::tmux` in `fx()` `:65` (all 10, one live pane) | none | engine | `wait_whole_ticks` 9; `wait_until` | fake gh | 10 T (pane only to keep E2 away) |
| engine/watch_loop_guard_inbox.rs | 4 | `TestEnv::tmux` in `fx()` `:61` (all 4) | none | engine | `wait_until` | fake gh | 4 T |
| engine/watch_sleep.rs | 6 | `TestEnv::tmux` in `fx()` `:58` (all 6) + a second session in `:202` | none | engine | `wait_whole_ticks` 5 (+`sleep_seconds = 1`, backdated by `ago(120)`) | fake gh | 6 T |
| engine/watch_team.rs | 7 | engine-side | none | engine; node shim `sleep {n}` `:81` | tick 1 s and `pr_poll_seconds` 1 s (**real 1 s ticks**, `:60`); `wait_whole_ticks` 4 | fake node/pi/gh | 7 C |
| engine/watch_titles_width.rs | 2 | `TestEnv::tmux` (7 calls: new-session, set -p, display) | none | engine | `wait_whole_ticks` 1 | fake pi/gh | 2 T (+R: width is read from a pane) |
| engine/watch_worker_archive.rs | 3 | engine-side | `git_repo` `:50` (fx) | engine | `thread::sleep(1500 ms)` `:141` (3 tests use `some_ticks`: 1 call site) | fake pi/gh | 1 C, 2 K (+G for the fixture) |
| engine/main.rs | 0 | | | | | | module list only |
| store/events.rs | 35 | none | none | `cpd events follow` child (`Follower` `:693-731`, killed on `Drop`) in 10 tests; `cpd` runs 18 | `thread::sleep(hold)` `:589`; polls 20 s (`:718 :788 :926`); follower polls on its own | none | 23 K, 12 C (see 2.8) |
| store/store.rs | 68 | none | none | `Writer` child = this test binary re-run (`:1239-1370`) in 6; `sleep 60` child `:1306`; `cpd` runs 8 | `thread::sleep` 1100/1500/300/100 ms (`:1445 :1746 :1767 :1769 :1796 :1798 :1871`); `Instant` asserts | system `sqlite3` CLI (harness) | 52 K, 13 C, 1 X, 1 helper-not-a-test, 1 K-with-sh (`:1156`) |
| store/store_window.rs | 4 | none | none | `cpd` runs 2 | none | none | K |
| store/store_asleep.rs | 1 | none | none | none | none | none | K |
| store/store_local_needs.rs | 2 | none | none | none | none | none | K (own `tempdir`, no `TestEnv`, `:27-31`) |
| store/main.rs | 0 | | | | | | module list only |

Totals (engine 191 + store 110 = 301), by the section 2 clusters (a test counted once, its most demanding class): T 76 (watch.rs 44, inbox 10,
loop_guard 4, sleep 6, titles 2, and the 3 install tests counted in watch.rs); C about 50 (watch.rs 23, watch_team 7, doctor 4, store 13, a few
small-file ones); G 6 that really need git (the rest of the ~106 Fx tests need only a path); X 8 (section 3); R 2 (titles width); K the rest,
about 130. Counts are by static reading; the exact per-test class list is not produced (unverified beyond the clusters).

## 2. Clusters

Size unit: Worker Jobs (one Job = ~150 turns, <= ~40 tests rewritten). The design Job owns the fake tmux and the PR breakdown;
the "seam" column only names what exists or is missing.

### 2.1 Supervision, `watch --ensure`, `cpd tui`: the product creates a real tmux server (T, X) - 15 tests, 1.5 Jobs

`watch.rs`: 395 `ensure_starts_the_engine_in_the_work_session_once`, 453 `two_concurrent_ensures_...`, 484
`ensure_lists_panes_again_...` (fake `tmux` that `exec`s the real one, `:861-868`-style), 504 `ensure_fails_when_the_engine_does_not_start`,
578 `a_crashed_engine_is_back_within_seconds`, 588, 599, 610 `..._window_was_killed_...`, 619, 631 `a_plain_ensure_waits_...`, 651,
664 `..._killed_with_its_tmux_server_...` (`cpd tui`, `kill_tmux_server` `common/mod.rs:1036`), 679 `an_engine_that_cannot_start_...`, 887
`ensure_says_the_exit_status_is_unknown_...`, 3379 `ensure_starts_the_engine_for_a_caller_without_a_utf8_locale` (4 locales, 2 ensures each).
- External: tmux `new-session`, `new-window`, `respawn-pane`, `kill-window`, `kill-server`, `list-panes -a -F`, `list-windows`, `set-hook
  pane-died|window-unlinked`, `set -g`, hook firing, `#{pane_dead_status}`, UTF-8 client mode; plus real `cpd` re-exec into the pane.
- Real time waited: `BACK` = 10 s ceiling (`:519`, "within seconds"); `thread::sleep` 2 s (`:639`) and 5 s (`:685`, asserts 3..=14 starts, i.e.
  a rate bound on the product's 2 s back-off in `engine/src/watch/ensure.rs`, unverified constant); fake `cpd` `sleep 1` (`:455`).
- Fix: what is *our* code is the decision list in `ensure.rs` (`list_panes`, `server_running`, `create_session`, `new_window`,
  `respawn_pane`, `kill_window`, `apply_server_options`, lines 122-272): drive it through a fake tmux behind the `Tmux` struct
  (needs a trait or `with_program` pointing at a scripted shim; **new seam**). The back-off (`:679`) and the plain-ensure lock wait (`:631`)
  need an injectable clock or a recorded-call assertion with no sleep. Hook firing, `pane_dead_status` and locale output are X (section 3).
- Size: 1.5 Jobs (fake tmux scenarios for ensure + the two time tests), after the fake tmux exists.

### 2.2 Workers pill text (T) - 5 tests, 0.3 Job

`watch.rs`: 744 `a_stuck_job_wakes_its_context_manager`, 791, 819, 841 `without_a_tmux_server_...`, 858 `a_tmux_failure_of_the_workers_step_...`.
- tmux: `new-session keepalive`, `show -gv @cpd_workers`, `set -g @cpd_workers`, `has-session` (helper `tmux_server`/`option`,
  `:775-790`). Waits: 2.5 s `thread::sleep` at 807, 846, 874 ("not set again over more than one tick", `time-claim`).
- Fix: fake tmux recording `set -g` calls; assert the recorded calls after `wait_whole_ticks`, or better a unit test of
  `workers.rs:90-110` with a stub tmux. Seam: new (tmux trait). The three 2.5 s sleeps become tick-count waits on a fake `gh`
  counter, which already exists (`poll_searches`, `common/mod.rs:1138`).

### 2.3 E2 pane/window liveness and kill-window failure (T, X) - 6 tests, 0.5 Job

`watch.rs`: 1126 `e2_recovers_a_merge_whose_driver_died...` (also G: `create_attempt_worktree`, `git merge --ff-only`, `merging_api` `:1099`),
1178, 1193, 1210 (`failing_kill_window` `:1160`: a shim that fails `kill-window` and otherwise `exec`s the real tmux),
1227 `e2_stops_sessions_whose_pane_is_dead_or_gone`, 1283 `e2_skips_a_pane_with_an_unknown_view...`.
- tmux subcommands: `new-session -P -F #{pane_id}`, `new-window`, `new-window ... sh -c 'exit 3'` (dead pane), `set -g remain-on-exit on`,
  `list-panes`, `kill-window`, `set -p @cpd_view`. Real time: only the ticks (0.1 s).
- Fix: fake tmux with a scripted `list-panes` reply (`#{pane_id} ... #{pane_dead}`), recorded `kill-window`; the "first kill fails" case is a
  scripted failure. 1126 also needs the git fixture (see 2.5).

### 2.4 E3/E7 schedule: the engine starts a real detached driver (T, G, C) - 8 tests, 1 Job

`watch.rs`: 1310 `e3_one_tick_schedules_a_ready_job...`, 1348, 1367, 1393 `a_pending_tmux_restart_...`, 1428 `e3_starts_the_driver_after_the_engines_binary_is_replaced`,
1758 `e3_never_starts_a_draft...`, 2111 `the_open_job_is_ready_only_when...` (4 engines, `:2107`), 2703 `a_prerequisite_seen_merged_by_e5_...` (tmux, git, flock).
- What runs for real: `new-session cpd-work` (`:1313`, `:1341`: `sleep 600`/`SLEEP_SECONDS`=120 pane), a detached `cpd` driver (setsid) that calls
  `create_attempt_worktree` (`git worktree add`, `worktree.rs:513`), `new-window`, a fake pi (`exit 0` or `sleep 120`), then `wait_until_for(.., 60 s,
  attempt ended)` at `:1330`, `:1444`. 1393 sleeps 2.5 s (`:1412`).
- Leak risk: the driver is outside the test's process group (recorded in `attempts.driver_pid`, stopped only by `TEARDOWN_SH` `recorded()`); a
  killed test leaves it to the reaper. `Engine` has a `Drop` kill (`:90-97`) but not the driver.
- Fix: the E3 decision (who starts, in what order, concurrency, urgency, back-off) is unit-testable at the `schedule.rs` level with a stub spawner; the
  full "driver starts and ends" test is an end-to-end test that belongs in `flow/` with fake tmux+git. Seams: spawner (new), tmux (new), git (new or
  `GitWrapper`, `common/mod.rs:564`).
- Size: 1 Job (the 4 pure-ordering tests are cheap; the three "driver really starts" tests are the cost).

### 2.5 `Fx` set-up with real git, in nearly every engine test (G) - ~106 tests, 1 Job for the fixture + 0 per test

`watch.rs:113-175` (`fx`, `fx_bare`), and `fx_open_pr`, `fx_cancelled_open_job`, `Titles::new` (`:2742`), `watch_worker_archive.rs:50`.
- git subcommands: `update-index -q --refresh` (once per env), `config user.name|user.email|commit.gpgsign` x3, then `worktree add -b`
  (`create_lead_worktree`, `worktree.rs:569`) with its `rev-parse`, `branch` checks; copy of a template repo made once by `git init/add/commit`
  (`common/mod.rs:970-994`). Real git time: tens of ms each, five+ forks per test under load average 40-120 (`unverified` per-test cost).
- Most of these tests do not test git: the git repo is only a path recorded in `prs.lead_worktree` / `jobs`. Fix: a fixture that writes the rows with
  a plain directory and no git (`Fx::git_free`), keeping git only where the product reads the worktree (1126, 1067 no, `r2`/`r3` removal tests at
  1193-1210, 2703). Seam: none needed (delete the call); where the product runs `git` (merge recovery, worktree removal, lead sync) it needs the git fake
  (new, `GitWrapper` at `common/mod.rs:564` is an existing shim used by `tooling/harness.rs`, `pr/git_guard.rs`).
- Size: 1 Job for `Fx` + a sweep of the 6 tests that really need git.

### 2.6 E10 retitle (T, R) - 7 tests, 0.4 Job

`watch.rs`: 2876-3047 (`e10_*`: 7 tests) on `Titles` (`:2742-2870`): `new-session ... -x 100 -y 30`, `new-window` x n, `set -p @cpd_view chat`, one live pane per Session so E2 leaves it,
`pane_width` (`display -p #{pane_width}`), fake pi that `sleep`s until a file exists (`:2910`, up to 120 s). `watch_titles_width.rs` (2 tests, 7 `TestEnv::tmux` calls).
- Fix: stub tmux for `list_panes` / `pane_width` (`titles.rs:150`); the retitle decision and the width argument are unit tests of `titles.rs`. Waits are `wait_until`
  on retitled rows plus the pi shim: no sleeping on wall time (comment `:2745`). Seam: tmux trait (new).

### 2.7 Inbox / loop guard / Sleep (E8, E13) tests that need a live pane only to keep E2 off (T) - 20 tests, 0.5 Job

`watch_inbox.rs` 10, `watch_loop_guard_inbox.rs` 4, `watch_sleep.rs` 6 (`fx()`: `new-session x sleep 600`; sleep also `new-session cpd -n main` + `set -p @cpd_session`, `:202-205`).
- The pane is a prop. Fix: a fake tmux whose `list-panes` answers a live pane (one fixture helper, no server) removes the real tmux from all 20; or make them DB-only
  tests of E8/E13 by seeding sessions as `stopped`/`idle` without a pane (unverified that E2 would then not stop them: `liveness.rs:372`).
- Time: ticks of 0.1 s; `watch_sleep` uses `sleep_seconds = 1` but backdates `state_at` by `ago(120)` so no real 1 s is waited (read at `:140-153`).
  `wait_whole_ticks(3)` after the positive signal (`wait_asleep`) is an absence window of ~0.4 s (C: an event wait on a tick counter, which exists).

### 2.8 Engine tests waiting on real ticks and sleeps (C) - 23 tests, 1 Job

- `thread::sleep`: `watch.rs:639 685 807 846 874 1412` (2, 5, 2.5, 2.5, 2.5, 2.5 s = 17.5 s total; 6 tests), `watch_worker_archive.rs:141` (1.5 s, 1 call per test in 3 tests),
  `watch_distill_inbox.rs:103` (1.2 s), `watch.rs:3627` (`a_marker_written_by_mark_restart_silences_...`: sleep, unverified length).
- Shim `sleep` inside fakes (10 tests): `:455` (`sleep 1`), `:2306` (`sleep 0.2` gh), `:2344` (`sleep 0.1`), `:1692` (busy store `until [ -e go ]; do sleep 0.01`),
  `:2468 :2640` (`sleep 120` gh hang killed by `CPD_TEST_DEADLINE_GH`), `:2910` (`sleep 0.1` loop up to 120 s), watch_team `:81` (`sleep {n}`).
- Real product time: `e5_polls_with_a_plan_in_pr_too...` (`:2364`) waits one whole `pr_poll_seconds` = 1 s (config key, no override); watch_team.rs runs 1 s ticks and 1 s poll
  (`:60`), so each of its 7 tests costs several seconds; `e5_polls_every_pr_poll_seconds...` (`:2300`) and `...due_time...` (`:2339`) assert gaps with `date`-style timestamps written by a
  shell `$(now)` (wall-clock assertions, flake under load: `gap in 300..4000`, `:2349`).
- e9 curate (`watch.rs:3811-3967`, 7 tests): real `cpd context curate` runs with fake pi per tick, `wait_until_for(.., 20-30 s)` (`:3831 :3885 :3897 :3913 :3917 :3958 :3979`).
- Fix: injectable clock/tick source for the engine loop (`engine.rs:399-414`: `Instant::now`, `thread::sleep(NAP)`) so tests step ticks; or event waits on `watch` heartbeats
  (none exist: comment `watch_worker_archive.rs:139` "no per-poll signal from the engine"). For the poll-gap tests the product needs a recorded due-time, asserted pure (`pr::due`, `pr.rs:36`).
  The cheapest fix with no product change: replace absence sleeps with `wait_whole_ticks(n)` (positive signal, exists) - 7 tests.
- Size: 1 Job (clock seam in engine loop + 23 rewrites would be over 40 tests: split off the 7 e9 and the 5 poll-timing tests if the clock seam is not built).

### 2.9 `cpd watch` as a process: lock, signals, restart markers, `--check` (K, C) - ~25 tests, keep

`watch.rs`: 344 `watch_holds_the_lock_...sigterm`, 371 `sighup`, 383, 434, 3468-3627 (restart-marker tests, 11: start/stop the engine, `Signal::SIGTERM`), 3764-3803 (`watch --check`, 3),
710 (kill without watch-stopped), 1732 `watch_stopped_waits_for_a_busy_store`, 1687 busy store. Own process, own signals, reaped by `Engine::drop`; allowed. The marker tests
backdate marker files, so no wait on a 10 s / 120 s bound (`:3492 :3505 :3534`). Keep as K; at most trim the engine's real tick wait.
Install script (`scripts/install-cpd.sh`, own script): 3215, 3251, 3322, 3348 are K (fake cargo, `checkout::Checkout`); 3165, 3183, 3195 need a real tmux server (`new-session cpd-work`) -> T,
fixed by the same fake tmux (the script calls `tmux -L <socket> list-windows`, `:3139`, itself a tmux shim candidate).

### 2.10 DB-only engine absence/transition tests (K with C-tail) - ~50 tests, 0 extra

`watch.rs` E1 (902), E2 non-tmux (934-1067), E4 open Job (1829-2075), E5 (2187-2662 except the 2300-2364 timing ones), E3 back-off (1523, 1562), tamper (1467, 1489). Engine + fixture SQL + fake gh/pi; the only real
time is `wait_whole_ticks` (0.1 s). K, but they pay the Fx git cost (2.5) and the per-tick `tmux list-panes` fork (section 0).

### 2.11 doctor (K, C) - 32 tests

All run `cpd doctor` with `PATH = bin/` only (`doctor.rs:117`): `git`, `tmux`, `gh`, `pi` are shims (`:99-107`), `node` is the real node linked in (`:42`, `:109`; allowed). 28 K. 4 C: `a_hung_gh_auth...` (`:737`, 5 s deadline),
`a_hung_tool_version...` (`:748`, 1 s), `a_hung_pi_node...` (`:761`, 0.3 s), `a_hung_catalog_probe...` (`:777`, 5 s, asserts `elapsed < 20 s`, `time-claim` `:784`): each waits the real deadline with a `sleep 120` shim
(`:721`) that the product kills with `killpg` (`core/src/host/deadline.rs:110-128`). Fix: `CPD_TEST_DEADLINE_*` can drop to 0.1-0.3 s except where the comment `:723` says spawns take >1 s on a loaded Mac; a better fix is a
fake `Command` runner in `host/deadline.rs`. Size: 0.3 Job. Seam: `host/deadline.rs::output` (existing, an env override, not an injectable runner).

### 2.12 store (K, C) - 110 tests, 1 Job for the 13 C

- K (94): SQLite schema, triggers, history, migrations, event kinds, list/follow-until-idle; only rusqlite and the test's temp dir. The 65 that use `TestEnv` pay the reaper + teardown (section 0); `store_local_needs.rs` and
  `store_asleep.rs` pay none. Keep; a `TempHome` without reaper would cut 2 processes per test (not required by the rule).
- C (13), real time of the product's store-lock/ busy wait (`core/src/store/lock.rs:133-188`, `mod.rs:125-132` 50 ms naps, `lock.rs:89` 100 ms): `store.rs:1370 1393 1440 1498 1561 1677 1740 1760 1784 1811 1823 1841`, `events.rs:598`.
  Waits: 1.1 s (`1440`, a real `sleep` *is* the slow work), 1.5 s (`1740`, `events.rs:601`), 0.6 s (`1760 1784`), 0.3 s (`1677 1811 1823`), 0.1 s (`1871`); `1393`/`1498`/`1370`/`1561` spawn a **child test binary**
  (`Writer`, `current_exe` + `store::child_writer --exact`, `:1239-1370`) holding a lock until a release file. Own process, `KillOnDrop` (`:1275`), no external: allowed, but they depend on
  `--test-threads=1` argv shape and on `cargo nextest` accepting an unfiltered `child_writer`.
- Fix: a clock/sleeper seam in `store/lock.rs` and `store/mod.rs` (`Instant::now` + `thread::sleep`) so "waits longer than busy_ms" runs in 0 s; the child-process ones can stay (two connections in one process suffice for `1393`, `1498`; unverified that
  the holder record `store-writer-<pid>.json` identifies a thread). Fixed delays 300/1500 ms in the *test thread* (`1746-1798`) are the same seam.
- `events.rs` follow (10 tests, `Follower` `:693`): a real `cpd events follow` child polling the store; `next()` is a blocking `read_line` with no timeout (`:709`): a follower that never prints hangs the test until nextest's
  own timeout; `exit()` has a 20 s poll. Fix: run `follow` in-process with a `poll` of 0 and an in-memory `Write`; the re-exec test (`:904`) needs a real exe replace (`exe.replaced()`, `core/src/events/mod.rs:171-175`) and stays an own-binary test.

## 3. Tests that test an external (X)

| test | what it really checks | action |
|---|---|---|
| `watch.rs:610`, `:619`, `:651`, `:664`, `:578-599` (the *hook-fires* half) | tmux runs `pane-died` / `window-unlinked` hooks and restarts the window | split: argv of `apply_server_options` (our) as a unit test; delete the firing check, or keep as `scripts/` contract on a fixture |
| `watch.rs:3379` `ensure_starts_..._without_a_utf8_locale` | tmux prints `_` for tabs without a UTF-8 locale (hs-d463) | keep the existing fixture test `:3749-3803` (fake tmux printing `_`); delete the real-tmux loop; assert `-u` in `Tmux::argv` (`tmux.rs:501`) |
| `watch.rs:887` `ensure_says_the_exit_status_is_unknown_when_tmux_has_none` | tmux's empty `#{pane_dead_status}` after SIGKILL | fixture: a `list-panes` line with an empty status |
| `watch.rs:1227`, `:1283` | tmux's `remain-on-exit`, `pane_dead`, pane ids | fixtures of `list-panes` lines (a parse test exists: `tmux.rs:1177`) |
| `watch.rs:484` `ensure_lists_panes_again_...` (engine-W10) | tmux's `list-panes` failing while another call's `new-session` starts the server | scripted fake tmux failure, no real server |
| `store.rs:1306` `a_guarded_child_is_gone_after_a_panic` | the test file's own `KillOnDrop` plus `kill -0` | delete (tests a test helper, not product code; starts `sleep 60`) |
| `store.rs:1239` `child_writer` | not a test: returns at once unless `STORE_CHILD_OUT` is set; counted by nextest | move to a `[[bin]]` or a `harness = false` helper, or an `#[ignore]` entry; unverified which |
| `store.rs:1156` `a_write_from_a_deleted_cwd_...` | `getcwd` failing in a deleted directory (OS) yields `(unreadable)` in our code | keep (K): our code's reaction, an `sh` of our own |
| `common/mod.rs:1036 kill_tmux_server`, `Engine`, `Follower` | harness, not tests | none |

## 4. Leak and flake risks

1. **Real tmux server per T test** (about 76 tests): socket under `TMUX_TMPDIR`, `cpdt-<pid>-<n>` (`common/mod.rs:714-735`); stopped only by the reaper/`Drop` teardown (`stop_tmux`). A test that is SIGKILLed relies on the
   reaper `sh` (`start_reaper` `:249`), which polls every 1 s and is itself a process in a different process group; the `docs/research/leak-cpdt.md` (a Linux study, not re-read here) predates the macOS branch. On macOS the teardown reads `ps -E`
   (an Apple binary hides its environment: `mine()` falls back to the dir string, `TEARDOWN_SH`), so an Apple-signed `sleep` pane can survive (unverified).
2. **Detached E3 drivers** (2.4): `setsid` children recorded in the store; if the store write is lost (engine killed first) the driver has no recorded pid and the teardown cannot find it. The test waits up to 60 s (`:1330 :1444`) for the driver to end.
3. **Hooks restarting the engine while the server dies** (`kill_tmux_server` `:1036`, comment `hygiene.md 3a`): the supervising `pane-died` hook starts a new engine and a new server in the env being removed; the harness unsets hooks first. Tests 578-664, 3379.
4. **Load-dependent timing**: `BACK` 10 s (`watch.rs:519`) and `WAIT` 30 s under load 40-120; gap asserts `300..4000 ms` (`:2349`) and `>= 200`, `>= 300` ms (`:2321-2323`) from a shell `date` that can be delayed; `5 s` back-off count `3..=14` (`:685`); store waits `>= 1400 ms`
   (`store.rs:1754`) and `< 5 s` (`:1700`); doctor `elapsed < 20 s` (`doctor.rs:786`) and the 1 s/0.3 s deadlines (`:752 :767`: the comment at `:723` says a spawn took over 1 s at Gate start).
5. **Absence tests by sleep**: `watch.rs:807 846 874 1412`, `watch_worker_archive.rs:141`, `watch_distill_inbox.rs:103` (a fixed duration standing for "no event came"): flake only on slow ticks, never on fast ones; `wait_whole_ticks` (positive signal)
   is the existing cure and is used by 40 tests.
6. **Blocking reads without timeout**: `store/events.rs:709` `Follower::next` and `store.rs:1670`-style `rx.recv_timeout(10 s)` (that one has a bound). `events.rs:709` hangs until nextest kills the test if the follower never prints.
7. **Per-test fork count**: `TestEnv` = 2 shells (+ `sqlite3` read in the teardown, `recorded()` only when `cpd.db` exists); `Fx` = ~6 git forks + 1 `cpd pr poll` per 0.1 s tick + 1 tmux `list-panes` per tick (when `list-panes` is reached). At
   load average 40-120 these forks are the slowest part; none leaks, but they widen every time window above.
8. **Shared `CARGO_TARGET_TMPDIR` templates** (`store_template`, `git_repo_template`, `common/mod.rs:1178 1203`): made once, renamed in atomically; a concurrent maker's whole copy wins; no leak. A stale template after a crash between `copy` and `rename` leaves `*.partial` files (`cpd-store-<pid>.partial`), never removed (unverified: no cleanup in `store_template`).
9. **Engine drop**: `Engine::drop` SIGKILLs and waits (`watch.rs:90-97`); `watch_*_inbox.rs` wrappers poll 10 s for the child's exit (`:38`, `:30`, `:27`) then leave it to the group teardown; a child that ignores SIGTERM stays until `Drop` of the `TestEnv` (group kill + 1 s grace).
10. **`two_concurrent_ensures` and the install tests** start background `sh -c 'flock .. sleep 120'` (`watch.rs:3141`) outside the group, killed by the test with `kill <pid>` after the script (`:3156-3159`): a panic between start and kill leaves a `sleep 120` until the env's `mine()` check; their own comment limits it to 120 s ("the longest any fake sleeps", `common/mod.rs:63-66`).
