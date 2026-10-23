# Inventory: `cli/tests/session` against the rule (Plan `tests-mock`, Job `inv-session`)

Static analysis only (rg and sed; no test was run, no tmux was started). Every claim carries `file:line` of the tree at
this commit. A claim I did not verify says `unverified`. Test counts are `rg -c '#\[(test|tokio::test)' <file>`; they sum
to 167 (the Job's number). `cli/tests/session/main.rs` holds no test.

The rule: tests test only our code; tmux, pi, gh, curl, ssh, the network and real waiting are mocked; git is mocked except a
few adapter contract tests. Classes: **K** keep, **T** needs the fake tmux, **G** fake git, **C** clock or event wait,
**X** tests an external, **R** render or input test that must become a unit test.

## Facts the findings rest on (verified)

- `TestEnv::new` (`cli/tests/common/mod.rs:714`) starts no tmux. It starts one **reaper** (`sh teardown.sh watch <pid>`,
  `mod.rs:249-260`) per env, and `Drop` runs `sh teardown.sh` (`mod.rs:774-788`, `1157`). It denies only `gh pi curl wget ssh`
  (`DENIED_TOOLS`, `mod.rs:304`): **`tmux` is not denied**, so every `cpd` the tests run finds the real `tmux` on `PATH`
  (`mod.rs:849-852`), and the real `tmux` is also the `TestEnv::tmux` helper (`mod.rs:1044`).
- Nothing in this scope calls `git_repo`, `local_origin`, `TestEnv::git` or `Command::new("git")` (`rg -i 'git_repo|local_origin|Command::new\("git"'`
  gives no hit in `cli/tests/session`). **No test here needs G.** No gh/curl/ssh either, except one `fake_gh` for the engine
  (`session.rs:1454`). No pi: it is always `fake_pi` (a shell script) or `fake_cpd_*`.
- The product's own real limits are in code, with a debug-only env override `CPD_TEST_<NAME>` (`core/src/host/limit.rs:11-20`)
  that only shortens the real wait. Used: `FILE_POLL` 0.5 s (`core/src/session/run.rs:39`), `WATCHDOG_POLL` 2 s (`run.rs:42`),
  `ADAPTER_WATCHDOG` 60 s (`core/src/session/reload.rs:32,81`), `SOURCE_POLL` 2 s (`core/src/session/fork_wait.rs:20,57`),
  `DEADLINE_TMUX` (`host.rs:630`), `pi_stop_seconds` 10 s and `fork_quiet_seconds` 5 s (`core/src/config/file.rs:331-334`).
  `core/src/clock.rs` is only a timestamp formatter (`clock.rs:1-14`): **there is no injectable clock**.
- `Tmux` is a concrete struct built by `Tmux::new(socket)` in the command code (`core/src/plan/lead.rs:68,124`,
  `core/src/context/manager.rs:92`, `core/src/session/mod.rs:355-357`); it spawns the program named `tmux` on `PATH`.
  The only seam today is `Tmux::with_program(path)` (used `host.rs:696-760`) and a script named `tmux` first on `PATH`
  (`env.fake("tmux", ...)`, `mod.rs:1058`).
- The "fake tmux" shims that exist **exec the real tmux**: `tmux_recording_go_signals` (`session.rs:561-575`),
  `tmux_refusing_the_session_mark` (`session.rs:552-556`) and the shim at `session.rs:887`. They are spies on a real server.
- `wait_until` waits up to 30 s, polls every 10 ms with `thread::sleep` (`mod.rs:390-419`). Every wait in this scope is a
  wall-clock poll of that kind.
- Pane commands that leave `sleep` processes: `SLEEP_SECONDS = 120` (`mod.rs:75`) in `cockpit()` (`session.rs:151-169`),
  `fake_pi(.., "exec sleep 120")`, `fake_cpd_views` (`mod.rs:1085-1086`), `session_first_message.rs:24-26`,
  `session_reload.rs:230`, `session_watchdog.rs:81`, `session_titles.rs:361`; and `sleep 600` in `host.rs:31,384`
  (`sleeper()`, used by most `host.rs` server tests). Those are the `sleep 120` leftovers on pro, plus a longer `sleep 600`.

## 1. Summary table

Own processes/signals: R = the env reaper (every file with a `TestEnv`; not repeated). `real tmux` names the API.
Classes count tests; they sum to the file's test count.

| file | tests | real tmux (API) | real git | own processes / signals | sleep or poll | other external | class |
|---|---|---|---|---|---|---|---|
| `board.rs` | 6 | no | no | R | none | none | K6 |
| `host.rs` | 26 | 17 tests: `Tmux::*` on a private server (`server()` `host.rs:25`), `s.env.tmux` (`:97,108,144,...`); `DefaultDirServer` runs `tmux kill-server` in `/tmp/tmux-<uid>` (`:44-57`) | no | R; panes `sleep 600/120`; `SIGKILL` by pid (`kill_program` `:235-240`); child re-run of the test binary with a fake tmux (`:575-584`); fake tmux `sleep 120 &` group kill (`:635,640`) | `sleep(100ms)` `:237`; `wait_until` x8 (`:259,264,285,..`); a real 1 s tmux deadline (`:630-675`); `/proc` read (`:586`) | real `tmux` (the subject) | K9 T7 X10 |
| `pill_lines.rs` | 3 | no (reads `SERVER_OPTIONS`, `BORDER_FORMAT`, `STATUS_FORMAT` consts) | no | R | none | none | K3 |
| `session_asleep.rs` | 3 | 1 test: `cockpit()` + `session open` (`:30-42`) | no | R; panes `sleep 120` | none | fake pi | K2 T1 |
| `session_first_message.rs` | 3 | 2 tests: `plan new` -> `Tmux::create_session` (`plan/lead.rs:71`), `cockpit()` local copy (`:17-30`) | no | R; panes `sleep 120` | none | fake pi, fake cpd | K1 T2 |
| `session_list_archived.rs` | 2 | no | no | R | none | none | K2 |
| `session_reload.rs` | 12 | all 12: `new-session`, `new-window`, `set`, `display-message`, `capture-pane`, `kill-window` via `env.tmux` (`:46-52,75,172-176,274`) and `cpd session run` of an installed copy of `cpd` | no | R; wrapper `cpd session run`; fake pi shell loop with `sleep 0.2` (`:226-230`); `kill(pid,0)` and `SIGTERM`/`SIGKILL` of pi (`:354-356`, product) | `thread::sleep` x10 (`:260,284,312,343,400,414,432,461,475,514`: 0.15-0.45 s, one 2.5 s); `wait_until` x27; `pi_stop_seconds=1` real wait (`:359-388`) | fake pi; `CPD_TEST_FILE_POLL/WATCHDOG_POLL` | C12 (T inside) |
| `session_report.rs` | 3 | no | no | R; `events follow --until-idle` (exits) | none | none | K3 |
| `session_resume.rs` | 5 | no | no | R | none | none | K5 |
| `session_retier.rs` | 7 | 2 tests (`:200,225`): `cockpit()` + `session open` | no | R; panes `sleep 120` | `wait_until` x2 (`:216,236`) | fake pi | K5 T2 |
| `session_send.rs` | 18 | no (`session send` has no tmux: `rg Tmux core/src/session` finds none in the send path) | no | R; `events follow --until-idle` (exits) `:77` | none | none | K18 |
| `session_titles_glance.rs` | 4 | no server; a successful title reaches `tmux.list_panes()` (`core/src/session/retitle.rs:163`), a real `tmux` client against no server (which of the 4: unverified) | no | R; fake pi shell | none | fake pi | K4 (+T0) |
| `session_titles.rs` | 14 | 1 test: `new-session`, `set -p`, `show -pqv` (`:359-378`); the others reach `retitle.rs:163` with no server (unverified per test) | no | R; fake pi shell; `File::try_lock` on a test file (`:416-418`) | none | fake pi | K13 T1 |
| `session_watchdog.rs` | 3 | 3 tests: `new-session`, `new-window`, `set`, `capture-pane`, `kill-window` (`:76-91,44`) and `cpd session run` | no | R; wrapper; fake pi loop with `sleep 0.2` (`:73-76`) | `thread::sleep` x2 (`:132,146`); `wait_until` x5; the product watchdog limit 0.3 s (`:23-24`); wait up to 30.3 s (`:112`) | fake pi | C3 (T inside) |
| `session.rs` | 58 | 42 tests start a real server: `cockpit()` (`:151-169`), `parked_fork()` (`:1251`), `env.tmux` (`:97,215-232,...`), `run_lead`/`new-window` (`:1585-1597`); 3 more spawn a real client with no server (`rename` -> `row.rs:292`, `archive` -> `archive.rs:224`) | no | R; panes `sleep 120`; a real `cpd watch` engine child (`Engine`, `:1448-1462`); a fake `cpd` that loops `sleep 0.1` (`:1426`); fake pi loop `sleep 0.1` (`:1690`); real `tmux` spies (`:552-575,887`) | `thread::sleep(POLL*3)` x2 (`:671,686`, 0.3 s); `wait_until` x50; E2 tick 0.2 s (`:1458`); `CPD_TEST_FILE_POLL/SOURCE_POLL` 0.1 s (`:141-148,168`) | fake pi, fake cpd, fake gh | K13 T32 C10 R3 |
| **total** | **167** | 77 tests start a real server (static count, unverified by a run); about 13 more run a real client with no server | none | | | | K84 T45 C25 X10 R3 |

Class totals from the cells: K = 6+9+3+2+1+2+0+3+5+5+18+4+13+0+13 = **84**; T = 7+1+2+2+1+32 = **45** (host 7, asleep 1,
first_message 2, retier 2, titles 1, session 32); C = 12+3+10 = **25** (reload 12, watchdog 3, session 10); X = **10** (host);
R = **3** (session). 84+45+25+10+3 = 167.

## 2. Clusters

Size: one Job = one Worker, about 150 turns, at most about 40 rewritten tests. The fake tmux itself and the PR breakdown
are the design Job's; "seam" names what the cluster would bind to.

**A. `cpd session open/fork/park/close/archive/history/run` against a real cockpit (T, 32 tests in `session.rs`).**
- Tests: `rg -n 'cockpit\(&env\)|parked_fork\(' cli/tests/session/session.rs`; first `fork_registers_a_stopped_lead_in_a_parked_window`
  (`:506`), last `every_session_command_accepts_the_short_forms` (`:1967`). Also `session_asleep.rs:30`, `session_retier.rs:200,225`,
  `session_first_message.rs:34,60`, `session_titles.rs:359`: 6 more (38 with the 32).
- External: tmux subcommands `new-session -d -x -y -P -F`, `set-environment -g`, `new-window -d -e -P -F`, `split-window`, `swap-pane`,
  `set -p @cpd_*`, `list-panes -a -F`, `capture-pane -p -J`, `kill-pane`, `kill-window`, `rename-window`, `send-keys`,
  `display -p`, `list-windows -F`, `show -pqv`, `respawn-pane` (adapter `core/src/host/tmux.rs`). No git, no wait except 3 `wait_until`.
- The assertions read tmux state back (`panes()` `:177`, `window_names()` `:206`, `slot()` `:190`). A fake tmux must keep
  sessions, windows, panes, user options and a pane's program and exit.
- Fix: a stateful fake `tmux` program on `PATH` (the cpd binary spawns `tmux` by name, `session/mod.rs:355`), and `TestEnv`
  gets `tmux` in `DENIED_TOOLS` unless a test installs the fake. The tests keep their assertions; `cockpit()` and `panes()` talk to the fake.
- Size: 1 Job for `session.rs` (32 tests) and 1 Job for the 6 small-file tests plus the remaining `session.rs` run tests' follow-ups: in practice 1-2 Jobs, after the fake exists.

**B. The pane program `cpd session run` as a real wrapper in a real pane (C + T, 21 tests, plus 3 run tests counted in A).**
- `session_reload.rs` all 12 (`:235` first, `:550` last), `session_watchdog.rs` all 3 (`:100,127,138`), `session.rs` quit tests
  (`quitting_pi_archives_a_side_fork_and_closes_its_pane` `:1743` to `a_reload_is_not_a_quit_even_though_pi_exits_0_on_sigterm`
  `:1836`, 6) and the run tests `run_gives_each_plan_its_port_and_keeps_it_across_a_restart` (`:1608`), `run_gives_the_lead_remote_and_the_review_host`
  (`:1633`), `run_refusals` (`:1540`) (the last 3 counted in cluster A's T32).
- The real time limits waited for: wrapper loop `FILE_POLL` 0.5 s (tests set 50-100 ms), `WATCHDOG_POLL` 2 s (set 50-100 ms),
  `ADAPTER_WATCHDOG` 60 s (set 0.3 s, `session_watchdog.rs:23-24`), `pi_stop_seconds` 10 s (set 1 s and **the test asserts it
  elapsed >= 1 s**, `session_reload.rs:359-373`), the lock wait of the first write (a real 2.5 s `thread::sleep`, `:514`),
  `SOURCE_POLL` 2 s (set 0.1 s, `session.rs:168`). These are real waits shortened, not removed.
- Own processes: the wrapper itself, an "installed" copy of `cpd` renamed over (`session_reload.rs:116-125`), a fake pi with a
  `while :; do ...; sleep 0.2; done` loop and `trap TERM` (`:226-230`), and signals sent by the product (`SIGTERM`, then `SIGKILL`
  after `pi_stop_seconds`); the tests check them by `kill(pid, 0)` (`:354`). These processes are allowed ("our own shims"),
  but they live in a real tmux pane, so their parent is the tmux server, not the env's process group.
- Fix: the wrapper loop (`core/src/session/run.rs`, `reload.rs`) needs a seam for time and for the child: an injected `Clock`
  (sleep and now) and an injected pane/child trait, so the test drives ticks by hand (no `CPD_TEST_*` env, no `thread::sleep`). New seam;
  the existing one is only `limit()` (`host/limit.rs`), which shortens the wait but keeps it real.
- Size: 2 Jobs: (1) the clock seam in `session/run.rs` + `reload.rs` and the 15 reload/watchdog tests; (2) the 6 quit tests and
  3 run tests on the fake tmux (after A).

**C. Engine plus dying leads (C + T, 4 tests in `session.rs`).**
- `e2_kills_the_window_of_a_dead_parked_lead` (`:1463`), `open_leaves_one_window_for_a_lead_with_a_dead_pane` (`:1480`),
  `e2_replaces_a_dead_lead_in_the_chat_slot_by_the_placeholder` (`:1500`), `open_replaces_a_dead_lead_in_the_chat_slot` (`:1520`).
- They start a real `cpd watch` (`Engine`, `:1448-1462`, `tick_seconds = 0.2`, killed and waited in `Drop`) and a fake `cpd`
  whose `session run` loops `while [ ! -e flag ]; do sleep 0.1; done; exit 1` (`:1426`). Waits: `wait_until` for pane death and window removal.
- Fix: run one engine tick as a function (`E2` in `core/src/driver`) against the fake tmux, no `cpd watch` child, no `sleep`.
  The seam is the driver's tick function plus fake tmux. Size: 1 Job together with the other engine inventory (these are 4 tests; fold into B2).

**D. Wrapper screen and keys (R, 3 tests).**
- `fork_waits_until_the_source_is_idle_then_forks_and_resumes` (`session.rs:651`, `capture-pane`, `send-keys Enter`, two 0.3 s
  `thread::sleep`s `:671,686`), `fork_stops_waiting_for_a_stopped_source_without_a_file_and_retries_on_enter` (`:716`),
  `run_with_the_range_full_says_no_port_and_waits_for_enter` (`:1654`).
- They test the text our wrapper prints ("Press Enter to retry.", "Waiting for the source Lead ...", "pi exited. Press Enter to resume")
  and that Enter retries. Fix: make the wrapper's prompt loop a function over `Read`/`Write` and a `Clock`, and assert on the
  captured output and the input. Seam: `core/src/session/fork_wait.rs` and `run.rs`. The "time-claim" sleeps become ticks of the injected clock.
- Size: 1 Job (3 tests + the `fast_wrapper` helper removed).

**E. `host.rs`: our tmux adapter against a real server (T 7 and X 10).**
- Tests: `rg -n '^fn ' cli/tests/session/host.rs`, first `a_tmux_call_whose_tmux_tmpdir_is_missing_is_refused_and_starts_no_server_in_tmp` (`:62`),
  last `every_tmux_call_of_cpd_is_a_utf8_client_whatever_the_callers_locale` (`:762`).
- T (argv, parse and error mapping of our code, fixture = what tmux prints): `:62`, `:296`, `:458`, `:472`, `:554`, `:594`, `:630`.
  Already fake and K: `:698`, `:743`, `:762` (`Tmux::with_program(fake)`), `:786` (a source scan), `:121,129,511,538,544` (constants).
- Seam: the existing `Tmux::with_program` and the fake program; the output of `list-panes -F` as a fixture string.
  `:630` waits for a real 1 s deadline (`CPD_TEST_DEADLINE_TMUX=1`) and asserts `elapsed < 4 s`; it needs the deadline passed in
  (an injected clock) rather than a 1 s real wait, and its `running(pid)` reads `/proc` (`:586`), which does not exist on macOS (see section 4).
- Size: 1 Job (17 tests; 9 K stay).

**F. K tests whose `cpd` spawns the real `tmux` binary with no server (about 13 tests).**
- `session rename` -> `row::retitle` (`core/src/session/row.rs:289-296`), `session archive` -> `archive.rs:224`, `session retitle` ->
  `retitle.rs:163`. Test names: `rename_appends_session_renamed_once` (`session.rs:463`), `rename_works_on_an_archived_session`
  (`:1180`), `archive_of_a_lead_with_no_pane_closes_nothing` (`:1294`), the success tests of `session_titles.rs` and `session_titles_glance.rs`
  (which exactly: unverified). The real binary only prints "no server running", but it is a real external, and `TestEnv::new` does not deny it.
- Fix: no test change. Put `tmux` in `DENIED_TOOLS` (`mod.rs:304`) so these tests fail loudly if they reach it, and give the tests that need
  it a fake. One small Job (harness change, shared with the other areas: not counted here).

Sum of Jobs for this scope: A 2 + B 2 + D 1 + E 1 = **6** (C folded into B2; F is a harness change, not counted). The 13 K-only files need none.

## 3. Tests that test an external (X, 10 in `host.rs`)

They assert tmux's own behaviour, not what our adapter sends. Delete, or keep as one adapter contract test on a recorded fixture.
- `a_right_click_is_tmuxs_default_binding_that_passes_it_to_a_pane_that_captures_the_mouse` (`host.rs:105`): tmux's default `MouseDown3Pane` binding text.
- `a_server_started_by_cpd_reads_the_popup_and_menu_styles_back_as_default` (`:140`): tmux 3.8's option defaults; it even skips what the installed tmux does not know (`:152-154`).
- `starts_the_server_once_with_the_contract_options` (`:75`) with `assert_server_options` (`:171-228`): reads each option, hook and binding back from a live server. The
  part that is ours (the `SERVER_OPTIONS` table) is already checked by `:121`, `:129` and `pill_lines.rs`; the readback is tmux.
- `a_leads_border_shows_its_title_reversed_between_caps_and_a_titleless_lead_shows_nothing` (`:490`): tmux expands `#{T:pane-border-format}`; the format string is ours and is checked by `pill_lines.rs:20`.
- `list_panes_parses_for_a_caller_without_a_utf8_locale` (`:677`): 4 real servers to show tmux's `-F` tab output in a non-UTF-8 client (the tmux behaviour); the fix (`-u` first) is
  already pinned by `:698` and `:762`.
- `a_killed_view_pane_respawns_and_a_killed_lead_or_engine_pane_stays_dead` (`:242`): tmux's `pane-died` hook order and SIGCHLD handling (the comment `:231-234` says tmux 3.4 loses the signal).
- `respawn_killing_restarts_a_live_pane_in_place` (`:274`), `a_dead_pane_stays_and_is_respawned` (`:436`): `remain-on-exit` and `respawn-pane -k` semantics.
- `a_worker_window_gets_its_cwd_environment_and_argv` (`:402`): that tmux hands `cwd`, `-e` variables and argv to the pane's program verbatim.
- `builds_the_cockpit_and_finds_its_panes_by_option` (`:321`) is mixed: its pane-size and `swap-pane` claims are tmux's (`:366-376`); the finding of panes by option (`layout::*`) is ours and can use a `list-panes` fixture.
  It is the tenth of the X10 (the others are `:75,105,140,242,274,402,436,490,677`).

## 4. Leak and flake risks

1. **A real tmux server per test, 77 tests (static count).** Each starts a server and panes with `sleep 120` (`SLEEP_SECONDS`, `mod.rs:75`) or `sleep 600`
   (`host.rs:31,384`). Teardown depends on `TestEnv::drop` and the reaper (`mod.rs:249-260,774-788`). A test killed with SIGKILL leaves the
   server until the reaper's script sees the pid gone (reaper not verified on macOS: `unverified`). The `sleep` leftovers on pro match these commands.
2. **Panes outlive the env's process group.** tmux starts a pane's program from the server, so `cpd session run`, the fake pi loops (`sleep 0.2`, `sleep 0.1`) and `sleep 120` are not
   in the env's pgid (`command()` `mod.rs:839-868` sets `process_group` only for children it spawns). Only `kill-server` in the teardown ends them (`unverified`: the teardown script was not read in full).
3. **`DefaultDirServer`** (`host.rs:44-57`) runs `tmux kill-server` and removes a socket in `/tmp/tmux-<uid>` outside the env: the one place a test writes outside its temp dir (test `:62`).
4. **`host.rs:586` `running()` reads `/proc/<pid>/stat`.** On macOS that file does not exist, `is_ok_and` gives `false`, and `wait_until("the fake tmux's child to die", || !running(pid))`
   (`:638`) passes at once whatever happened: the group-kill claim is unchecked on pro (inferred from the code; macOS not run).
5. **`kill_program` sleeps 100 ms** (`host.rs:237`) to dodge a tmux 3.4 SIGCHLD loss; a flake workaround for an external.
6. **Real-time claims shorter than the Gate's load.** `session_watchdog.rs:23-24` (limit 0.3 s, poll 0.1 s), `session_reload.rs:260-475` (`POLL*3..9` = 0.15-0.45 s windows where
   "nothing happened"), `session.rs:671,686` (0.3 s), `session_reload.rs:514` (2.5 s), `host.rs:630-675` (`elapsed < 4 s` for a 1 s deadline). Absence is asserted after a sleep: at load average 40-120 a
   wrapper that has not had a CPU slice in the window passes the absence test vacuously (flake toward green) while the presence waits (30 s) flake toward red. Two are labelled `time-claim` in the source (`session.rs:668`, `host.rs:669`).
7. **`session_reload.rs:359-373`** asserts `changed_at.elapsed() >= 1 s` for `pi_stop_seconds`: a lower bound on real time, good under load, but the test needs 1 s of wall clock each run.
8. **`session_watchdog.rs:112`** waits up to `LIMIT*2 + 30 s` for a second restart: a loaded Mac can take the full 30 s before it fails.
9. **Absence of a `kill-window` on failure paths.** The 12 reload tests and the 3 watchdog tests call `kill-window` at the end of the success path (`session_reload.rs:274,294,...`); `session_watchdog.rs:44` does it in `Drop`. A panic before it leaves the pane until `TestEnv::drop`.
10. **The shims find the real tmux with `std::env::var_os("PATH")`** (`session.rs:553,562,887`, `host.rs:577`); `mod.rs:299` says to read `inherited_path()` instead, because a wrapper dir first on `PATH` would make
    the shim exec itself. A test run under the Gate's wrapper dirs could loop (`unverified`).
11. **A real `cpd watch` child** (`session.rs:1448-1462`): killed and waited in `Drop`, but it is a long-running real process started with `Stdio::null()`, ticking every 0.2 s, in 2 tests.
12. **A child re-run of the test binary** (`host.rs:575-584`, 2 tests) starts a second copy of the whole test process with a fake tmux first on `PATH`; it exits, but a hang in it would hang the parent (no timeout beyond the Gate's).
13. **Fake shims that poll with `sleep`**: `session.rs:1426,1690`, `session_reload.rs:226-230`, `session_watchdog.rs:73-76`. Allowed (our own shims, all in tmux panes), but they are the polls that need
    a real second to react and the processes that survive a test that panics before the teardown.
14. **`list-panes` by a real `tmux` client against no server** for about 13 K tests (cluster F): a real external spawned from every rename, archive and title write; it leaves nothing but depends on a `tmux` on `PATH`.
