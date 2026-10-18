# Research: the test suite and the build (Plan quality, Job tests-build)

Base `e8ad70d` (main at the Plan's start; `origin/main` has since landed Plan dev-speed: `scripts/cargo.sh` sccache, `pr_cleanup.rs`, `prune_script.rs`). Machine: 16 cores, 62 GB, load 10-28 during every measurement (other Workers were building; hs-d136), so absolute times are pessimistic and noisy; every number says its N.

**Summary.** The suite is sound: three full `scripts/cargo.sh test --workspace` runs (1105 passed, 0 failed, 1 ignored each) and a fourth pass over the test binaries, plus 25 runs of the four most timing-sensitive binaries at load 34-37 (16 extra busy loops) and one run with 4 binaries in parallel, produced **no failure and no flake**. Every documented command (64 of 64 in commands.md §2) and every error code (116 of 116) appears in some test; the harness isolates `HOME`, `CPD_HOME`, tmux and git. What is wrong is small: one test that cannot fail on its own claim (`job.rs:964`), test runs that are killed leave tmux servers, processes and 51 temp directories behind (observed now), about 63 s of fixed sleeps in 36 places that assert "nothing happened" or an upper time bound (they can pass vacuously or flake under load, not observed), and no enforcement that a test cannot reach the real `gh`/`pi`/network (hs-d27: it holds today by convention). The build is cheap to iterate (an edit in `core/` rebuilds in 3.6 s, in `tui/` in 1.9 s) but slow to start (clean dev 27 s, clean release 65 s, of which the bundled SQLite C compile is 61 s) and heavy on disk (a test build is **4.4 GB per worktree**, 3.3 GB of it 44 test binaries of 75-104 MB). The largest, cheapest speed-ups: run the test binaries in parallel (**175-200 s -> 54 s**, measured), and `debug = "line-tables-only"` in the dev profile (**4.4 GB -> 2.2 GB, clean test build 45 s -> 31 s**, measured on the env var, N=1). Nine compiler warnings (6 in the build, 3 in tests) are never gated.

## Findings

| id | class | what | size |
|---|---|---|---|
| tests-build-W1 | Work | `edit_pr_refusals_write_nothing` counts the store's writes and never compares them | Job |
| tests-build-W2 | Work | A killed test run leaves tmux servers, processes and `/tmp/cpd-test-*` directories (51 dirs, 1 server now) | Job |
| tests-build-W3 | Work | 36 fixed sleeps (about 63 s) and 3 elapsed-time upper bounds: absence tests can pass vacuously, bounds can flake under load | Job |
| tests-build-W4 | Work | Nothing stops a test from reaching the real `gh`, `pi` or network: ambient env and the real `PATH` pass through (hs-d27 by convention only) | Job |
| tests-build-W5 | Work | ETXTBSY is worked around per file (`driver_reload.rs`), not in `TestEnv::fake` | Job |
| tests-build-F1 | Fast | Cargo runs the 44 test binaries one after another: 175-200 s; run in parallel they take 54 s | Job |
| tests-build-F2 | Fast | Dev profile carries full debug info: 4.4 GB per worktree; `line-tables-only` halves it and cuts the clean test build by a third | Job |
| tests-build-F3 | Fast | Release clean build: `libsqlite3-sys` compiles for 61 s of 82 s on the critical path | Job (covered by dev-speed if sccache is installed) |
| tests-build-F4 | Fast | The slowest tests wait on constants the tests cannot shorten: whole-second engine tick, `PI_STOP` = 10 s; `e2e_cross` takes 20.7 s | Plan |
| tests-build-F5 | Fast | 40 integration-test binaries, each 75-104 MB: 3.3 GB of the 4.4 GB | Plan |
| tests-build-F6 | Fast | One core unit test is the whole core unit-test time (3.3 s alone, 6.1 s in the suite) | Job |
| tests-build-G1 | Good | Nine warnings (dead code in `core/src/plan/row.rs`, `tui/src/plan/actions.rs`, three in tests) and nothing gates them | Job |
| tests-build-G2 | Good | Test helpers are copied per file: `wait_until` 10 copies with deadlines of 10/20/30/60 s, `refused` 7 identical copies, `sql` 12, `write_config` 5 | Job |
| tests-build-G3 | Good | `std::env::set_var` in a multi-threaded test binary (`git.rs:476`, `host.rs:35`) | Job |

## Work

### tests-build-W1: a test that cannot fail on its own claim

- **What.** `edit_pr_refusals_write_nothing` records `(txns, evs)` before a series of refused `job edit --pr` calls and never reads them again. Its name says the refusals write nothing; the test checks only the error codes and details. A refusal that wrote a txn or an Event would pass. The compiler says so on every build.
- **Where.** `cli/tests/job.rs:964-991`, the binding at `cli/tests/job.rs:972`. Compare `cli/tests/job.rs:957-960` (the previous test does assert `(count(txns), count(events)) == (txns, evs)`).
- **Evidence.** `scripts/cargo.sh test --workspace --no-run`:
  ```
  warning: unused variable: `txns`
     --> cli/tests/job.rs:972:10
  warning: unused variable: `evs`
     --> cli/tests/job.rs:972:16
  ```
  `awk 'NR>=964 && NR<=991 && /count\(/' cli/tests/job.rs` prints only line 972.
- **Impact.** The "writes nothing" contract of `job edit --pr` refusals (commands.md §5) has no test, while the suite reports it covered. Verified by reading; I did not mutate the code to prove a write goes unnoticed.
- **Proposed fix.** Add `assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));` at the end of the test. It must fail first if a refusal writes (check by temporarily making one refusal write).
- **Size.** Job (minutes).
- **Overlap.** None.

### tests-build-W2: a killed test run leaves tmux servers, processes and temp directories

- **What.** `TestEnv` cleans up in `Drop` (`cli/tests/common/mod.rs:262-266`: `tmux kill-server`; `TempDir` removes the directory). A test process that is SIGKILLed or timed out (a Worker's Attempt ends) runs no `Drop`. Observed now: one private tmux server and its pane processes survive for hours, and 51 `/tmp/cpd-test-*` directories (about 23 MB) are left from four killed runs. `docs/research/teardown.md` recorded `/tmp/cpd-test-*` as 0 ("TestEnv removes its directory"): that holds only for runs that end normally.
- **Where.** `cli/tests/common/mod.rs:262-266` (the only teardown); `cli/tests/common/mod.rs:58-60` (the socket name carries the test process pid, `cpdt-<pid>-<n>`).
- **Evidence.** Read-only observation, nothing signalled:
  ```
  $ ps -eo pid,ppid,lstart,etime,args | grep 'tmux -L cpdt-'      # excluding servers of tests running now
  3724650  1  Sat Oct 17 20:02:16 2026  01:53:51  tmux -L cpdt-3722019-23 -u set-option -g remain-on-exit on ; new-session -d -s tree -x 46 -y 16 /home/clayton/projects/cpd.dev-speed--sccache-cargo-a1/target/debug/cpd tui view tree
  $ ls /proc/3722019                                             # its test process
  gone
  $ ps -eo pid,etime,args | grep 'events follow --since 1'        # a pane child of that server, also 2 h old (pid 3724777)
  $ ls -ldt --time-style=+%F_%T /tmp/cpd-test-* | awk '{print substr($6,1,13)}' | sort | uniq -c
   1 2026-10-16_13   6 2026-10-16_14   15 2026-10-16_17   17 2026-10-17_20   1 2026-10-17_21 ...
  $ ls -d /tmp/cpd-test-* | wc -l ; du -sc /tmp/cpd-test-* | tail -1
  51 ; 23596 total
  ```
  (the 17 dirs of 20:02 are from the same killed run as the server above: Worker `dev-speed--sccache-cargo-a1`).
- **Impact.** Leaked processes hold fds and memory until the machine reboots; the directories grow `/tmp` (about 23 MB for 51 dirs; small). Each leaked tmux server also keeps `cpd tui view` and `cpd events follow` children running (hs-n73 class). No effect on the live `-L cpd` server.
- **Proposed fix.** `TestEnv::new` first sweeps `cpd-test-*` directories under the temp dir whose owner process is gone (put the pid in the directory prefix: `cpd-test-<pid>-`) and kills their private tmux server (`-L cpdt-<pid>-*`); no new tool. Or have `scripts/prune-worktrees.sh` (dev-speed) also remove them; it already owns Worker leftovers.
- **Size.** Job.
- **Overlap.** hs-n27 / hs-n35 (deferred teardown gaps) are about runtime teardown, not tests; Plan dev-speed `cleanup` (`scripts/prune-worktrees.sh`, hs-n78) is the natural home of a sweep; `docs/research/teardown.md` line 21 (claims 0).

### tests-build-W3: fixed sleeps and elapsed-time bounds

- **What.** Two kinds of timing dependence (hs-d31: a flaky test is never acceptable):
  1. **Absence after a fixed sleep.** 36 unconditional `thread::sleep` of 0.3-5 s, about **63 s** in total, before an assertion that something did *not* happen ("a1 is not merged: nothing starts, however many ticks pass", "the engine does not try again", "the source is still working"). If the engine or tmux is slower than the sleep (load 40 happens, hs-d136), the assertion passes without having exercised the case: the test cannot fail for the right reason. Examples: `pr_start.rs:548` (2.5 s) and `:599` (3.5 s), `session.rs:455`, `:466` (3 s each) and `:1380`, `session_reload.rs:202`, `:222`, `:251`, `:282`, `watch.rs:461`, `:511`, `:584`, `:635`, `:711`, `:734`, `:750`, `e2e_cross.rs:424`, `:529`, `review_job.rs:416` (5 s), `driver_reload.rs:316` (4 s), `merge.rs:659`, `:925`, `:947`, `tui_tree.rs:369`, `:383`, `:481`, `:663`, `:719`, `tui_plan.rs:473`, `:552`, `tui_logs.rs:243`, `tui.rs:264`, `events.rs:737`, `host.rs:176`, `git.rs:450`.
  2. **Upper bounds on elapsed time.** `git.rs:334` asserts three concurrent `sleep 0.6` finish in under 1.5 s (900 ms margin); `git.rs:449` asserts a 300 ms timeout returns in under 900 ms (600 ms margin); `git.rs:326` under 5 s. A load spike inside the window fails them.
- **Where.** The lines above; wait loops that poll (`wait_until`, 50-200 ms) are fine and not counted.
- **Evidence.** Count and sum: `grep -rn 'sleep(Duration::from_\(secs\|millis\)([0-9]*))' cli/tests | grep -v 'from_millis(\(50\|100\|200\))'` lists the sleeps; adding their literals gives 63.1 s (excluding 10-20 ms retry loops in `pr.rs:1197`, `events.rs:688`, `:745`, `store.rs:1030`). Not reproduced as a failure: the three suite runs at load 15-25 (0 failures) and a stress run: 16 `while :; do :; done` loops on top of the other Workers (load 34-37), `git` binary 10 runs, `host` 5, `tui_logs` 5, `tui_worker` 5: 25 of 25 runs passed (`git` 1.96-2.64 s each). Whether any absence test passes vacuously today is **unverified** (needs a mutation per test).
- **Impact.** A passing absence test proves less than it says; the `elapsed` bounds are the first candidates for a flake. Measured cost of the sleeps: up to 63 s of wall time, spread over 15 binaries.
- **Proposed fix.** For absence: wait for a positive signal that the case was exercised (N engine ticks via the `watch-*`/tick Events or the `watch.log` line, or a recorded call count that stops changing), then assert absence. For `git.rs`: replace elapsed bounds by a causal check (the three commands each write a "started" file, none finishes before all started; the timeout test asserts `Timeout` and that `survivor.txt` never appears after the group kill, which it already does at `:450`).
- **Size.** Job per file group; the whole list is a Plan of three Jobs.
- **Overlap.** hs-n12 (the tui_logs flake, fixed by hardening) is a different cause (interleaved fake-tmux calls) and is fixed in `common/mod.rs:fake`. Fast overlap: F4.

### tests-build-W4: nothing enforces hs-d27 (no real `gh`, `pi`, network, `$HOME`)

- **What.** `TestEnv::command` removes only variables starting `CPD_`, `PI_`, `GIT_`, `XDG_` and `TMUX`/`TMUX_PANE`, then puts `bin/` first on `PATH` *in front of the real `PATH`*. So `GH_TOKEN`, `GITHUB_TOKEN`, `GH_HOST`, `SSH_AUTH_SOCK`, `NPM_*`, proxy variables reach every spawned `cpd`, and a test that forgets `fake_gh()`/`fake_pi()` runs the real program with the developer's credentials (HOME is the test home, but a token variable is enough for `gh`).
- **Where.** `cli/tests/common/mod.rs:122-134` (scrub and `PATH`).
- **Evidence.** Static: every test file that runs a `pr open|poll|rebase|merge` command installs a fake `gh` (`for f in cli/tests/*.rs ...` check: the one hit, `events.rs`, is a table of event field names, not a command). Dynamic: the whole suite (44 binaries, env scrubbed of `TMUX`/`CPD_*`, `GH_TOKEN`/`GITHUB_TOKEN` set to `shimcheck`) with a directory in front of `PATH` holding shims `gh pi curl wget ssh chezmoi claude` that log and exit 99: SHIMRESULT. Run with an empty temporary `CPD_HOME` as the ambient value: it was still empty after all four passes (no test used the ambient `CPD_HOME`).
- **Impact.** Today no test reaches the outside (see evidence), so this is a missing guard, not a bug: the next test written without a fake would reach the network on one developer's machine and not on CI (CI has no `gh` auth), a classic "passes on CI, fails locally" or the reverse.
- **Proposed fix.** `TestEnv::new` installs deny-by-default shims for `gh`, `pi`, `curl`, `wget`, `ssh` in `bin/` (exit 97, message "no fake installed"); `fake()` overwrites them. Remove `GH_*`, `GITHUB_*`, `SSH_*`, `*_PROXY` from the child env in `command`.
- **Size.** Job.
- **Overlap.** None (hs-d27 is the policy; this is its enforcement, AGENTS.md "prose is not enforcement").

### tests-build-W5: ETXTBSY is handled per file, not in the harness

- **What.** A script written by `fs::write` and executed soon after can fail with `Text file busy` when another thread forks between the open and the close (the child holds the write fd until it execs). The project knows: production code retries (`core/src/host/exe.rs:372-378`), one unit test dodges it by running `/bin/sh` (`exe.rs:411-413`), and `driver_reload.rs` serialises all its tests with a mutex ("The tests run one at a time", `driver_reload.rs:8-9`, `:28`). `TestEnv::fake` writes a script (`common/mod.rs:221`) and every other binary runs its tests in parallel threads that spawn processes, with no such protection.
- **Where.** `cli/tests/common/mod.rs:200-224`; `cli/tests/driver_reload.rs:8-9,28`.
- **Evidence.** The race is real in this repo (the three places above). In 4 full passes (4 x 1105 tests) and 25 stress runs there was no `Text file busy` in any output (`grep -i 'text file busy' /tmp/.../suite-*.log` empty). The exposure in `fake` is therefore **unverified**: the window is a few microseconds and the script runs much later.
- **Impact.** A rare, load-dependent failure, which hs-d31 forbids; the mutex in `driver_reload.rs` also costs time (in the suite its tests take 7-11 s each but 1.3-2.2 s alone: they wait for the lock, see F4).
- **Proposed fix.** Write the file under a temp name, `fsync`/`close`, and `rename` it into place from `fake` (rename does not help ETXTBSY by itself, the retry does): retry the *spawn* of fakes in the harness is not possible since cpd spawns them; the robust fix is the one the repo already uses in `Exe`: one `write_script` helper that creates the file, then does `sync_all` and a small retry on `ETXTBSY` of a `--version`-style probe execution before the test continues. Alternatively keep as is and record the decision.
- **Size.** Job.
- **Overlap.** None.

## Fast

### Measurements (this area's required list)

All on the worktree's own `target/`, `scripts/cargo.sh`, no sccache (not installed here: `which sccache` is empty), load at start 10-28.

| measurement | N | values (s) | median | max |
|---|---|---|---|---|
| clean dev build, `build --workspace` (122 crates compiled) | 3 | 22.0, 27.4, 30.8 | 27.4 | 30.8 |
| clean release build, `build --release -p cpd` | 3 | 60.5, 82.3, 64.7 | 64.7 | 82.3 |
| clean `test --workspace --no-run` (44 test executables) | 1 | 45.2 | - | - |
| incremental `build --workspace`, `touch core/src/ids.rs` | 10 | 2.19-3.00 | 2.61 | 3.00 |
| ... edit a function in `core/src/ids.rs` (a new private fn) | 10 | 3.39-4.05 | 3.61 | 4.05 |
| ... `touch tui/src/palette.rs` | 10 | 1.01-1.21 | 1.14 | 1.21 |
| ... edit in `tui/src/palette.rs` | 10 | 1.17-2.55 | 1.88 | 2.55 |
| incremental `test --workspace --no-run`, edit in `core/` | 10 | 4.12-6.40 | 5.16 | 6.40 |
| ... edit in `tui/` | 10 | 1.84-2.75 | 2.11 | 2.75 |
| full suite `test --workspace --no-fail-fast` (wall, 3 runs; sum of binary times 168 s median) | 3 | 200.5, 175.0, 179.3 | 179.3 | 200.5 |
| the same binaries run 4 at a time (`xargs -P 4`) | 1 | 53.6 | - | - |

Clean build critical path (`cargo build --workspace --timings`, `cargo-timing.html` of the dev build): `cpd-core` 14.4 s (starts at 12.4 s, after `libsqlite3-sys` 7.7 s and `minijinja` 5.3 s), then `cpd-tui` 5.1 s; the other 150 units overlap. The release build's critical path is `libsqlite3-sys` (61 s).

**Per test binary** (the harness's own "finished in", 3 full-suite runs; binaries run one after another):

| binary | tests | median | max |
|---|---|---|---|
| tests/e2e_cross.rs | 1 | 20.79 | 22.14 |
| tests/session_reload.rs | 6 | 12.08 | 12.09 |
| tests/driver_reload.rs | 4 | 11.65 | 20.00 |
| tests/session.rs | 37 | 11.15 | 11.18 |
| tests/pr.rs | 56 | 11.03 | 11.64 |
| core lib unit tests (`cpd_core`) | 201 + 1 ignored | 7.76 | 9.93 |
| tests/watch.rs | 27 | 9.40 | 9.78 |
| tests/review_job.rs | 9 | 8.15 | 8.30 |
| tests/pr_start.rs | 13 | 8.01 | 8.13 |
| tests/tui_tree.rs | 32 | 7.55 | 7.60 |
| tests/merge.rs | 34 | 6.92 | 9.59 |
| tests/e2e.rs | 1 | 6.31 | 6.44 |
| tests/job.rs | 26 | 4.79 | 5.03 |
| tests/store.rs | 35 | 4.52 | 5.10 |
| tests/pr_crud.rs | 20 | 4.40 | 4.40 |
| tests/driver.rs | 19 | 4.22 | 4.25 |
| tests/attempt.rs | 23 | 4.11 | 4.45 |
| tests/events.rs | 29 | 3.22 | 4.01 |
| tests/tui_read.rs | 21 | 3.21 | 3.51 |
| tests/tui_plan.rs | 14 | 2.95 | 3.23 |
| tests/plan.rs | 34 | 2.46 | 3.64 |
| tests/tui_statusbar.rs | 3 | 2.32 | 2.33 |
| tests/ask.rs | 10 | 2.10 | 5.10 |
| tests/tui_widgets.rs | 20 | 2.05 | 4.74 |
| tests/git.rs | 39 | 1.97 | 2.12 |
| tests/host.rs | 11 | 1.92 | 2.08 |
| tests/context.rs | 38 | 1.85 | 1.87 |
| tests/doctor.rs | 27 | 1.58 | 1.58 |
| tests/tui_worker.rs | 8 | 1.47 | 2.83 |
| tests/tui_logs.rs | 9 | 1.38 | 1.51 |
| tests/plan_show.rs | 9 | 1.18 | 1.19 |
| tests/tui.rs | 10 | 1.04 | 1.55 |
| tests/config.rs | 29 | 0.83 | 5.99 |
| tests/tui_history.rs | 5 | 0.60 | 0.67 |
| tests/pr_view.rs | 5 | 0.56 | 0.59 |
| tests/harness.rs | 8 | 0.19 | 0.26 |
| tui lib unit tests (`cpd_tui`) | 201 | 0.04 | 0.16 |
| the other 7 (`command_index`, `review`, `spec`, `cargo_script`, `metrics`, cli lib and bin, 3 doc-test runs) | 31 | <= 0.03 | 0.03 |

**The 12 slowest tests.** In-suite = one pass of every binary, tests in parallel inside the binary (`RUSTC_BOOTSTRAP=1 <binary> -Z unstable-options --report-time`, N=1); alone = `--exact`, 5 runs each at load 19-26.

| test | in-suite (s) | alone median / max (s) |
|---|---|---|
| e2e_cross `a_plan_across_two_repos_runs_a1_then_b_then_a2_and_a_review_job` | 21.97 | 20.66 / 23.44 |
| session_reload `a_pi_that_ignores_sigterm_is_killed_after_ten_seconds_...` | 12.29 | 11.61 / 12.31 |
| driver_reload `after_a_reload_fd_3_is_the_lock_and_the_drivers_children_do_not_hold_it` | 11.21 | 2.15 / 2.23 |
| session `fork_waits_until_the_source_is_idle_then_forks_and_resumes` | 10.52 | 8.95 / 9.04 |
| driver_reload `a_driver_reloads_while_running_keeps_its_lock_and_the_attempt_merges` | 9.07 | 1.49 / 1.57 |
| watch `e5_polls_every_pr_poll_seconds_after_the_previous_poll_ended_whatever_the_plans` | 8.85 | 7.35 / 7.44 |
| pr_start `e7_starts_a_dependent_pr_from_a_fresh_fetch_once_its_prerequisite_is_merged` | 8.02 | 6.44 / 6.48 |
| review_job `the_worker_starts_only_once_the_lead_is_idle` | 7.80 | 6.98 / 7.16 |
| driver_reload `a_driver_reloads_at_the_merging_safe_point_and_completes_the_merge` | 7.40 | 1.30 / 1.64 |
| core `store::schema::tests::migrating_an_empty_database_gives_every_versions_schema_hash` | 6.13 | 3.30 / 3.78 |
| driver_reload `a_driver_does_not_reload_into_a_cpd_older_than_the_store_...` | 6.07 | 6.16 / 7.15 |
| e2e `a_whole_plan_runs_from_context_new_to_pr_poll` | 5.98 | 6.10 / 6.30 |

The three `driver_reload` tests with "in-suite" 7-11 s and "alone" 1.3-2.2 s are waiting on the file's `ONE_AT_A_TIME` mutex (W5), not slow themselves.

**Binary size** (`target/release/cpd`, no `[profile]` in `Cargo.toml`, so Cargo's release defaults): 12,401,680 B; stripped (`strip -o`) 9,976,752 B (the symbol table is 2.4 MB, 19%). `size -A`: `.text` 7.87 MB, `.eh_frame` 0.72 MB, `.rodata` 0.50 MB, `.gcc_except_table` 0.37 MB. Contributors by symbol (`nm --size-sort -S -C`, attributed by the crate path in the demangled name; generic instances count for the first known crate in the name, so this is approximate): `cpd-core` 2.93 MB (36%), bundled SQLite C 1.74 MB (21.5%), `minijinja` 0.95 MB (11.6%), `cpd-tui` 0.50 MB (6.2%), `clap_builder` 0.42 MB (5.2%), `serde_json` 0.18 MB, `ratatui-core` 0.13 MB, `toml` 0.10 MB, `rusqlite` 0.09 MB, `time` 0.08 MB (total of sized symbols 8.12 MB). Largest single symbols: `sqlite3VdbeExec` 49 KB, `minijinja::vm::Executor::eval_impl` 43 KB, `sqlite3Pragma` 24 KB, `clap_builder::parser::Parser::get_matches_with` 22 KB, `cpd_core::plan::render::markdown` 18 KB. Nothing stands out; the size question belongs to `docs/research/binaries.md` (hs-q5: "installed release binary 12.4 MB", same file). The debug binary is 119 MB there.

**Dependencies** (`scripts/cargo.sh tree --workspace --depth 1`, `-d`): 13 direct workspace dependencies (clap, minijinja, nix, ratatui, rusqlite, serde, serde_json, sha1, sha2, time, toml, tui-tree-widget, uuid) + tempfile; `Cargo.lock` has 226 packages, 122 compile on Linux. Duplicates (`-d`): `hashbrown` 0.16.1 (via `kasuari` <- `ratatui-core`) and 0.17.1 (via `hashlink` <- `rusqlite`, `lru`, `ratatui-core`), `syn` 2.0.119 (derive_more, strum) and 3.0.9 (clap_derive, darling, serde_derive). All come from upstream; `syn` 3 is 4.0 s and `syn` 2 is 2.4 s of the clean build. Nothing to do (AGENTS.md: take the dependency).

### tests-build-F1: cargo runs the test binaries serially

- **What.** `cargo test --workspace` runs the 44 test executables one after another; each binary is mostly idle (sleeping, waiting for tmux or the engine tick), so the machine is not busy while one long binary ends. The binaries are independent processes with private `TestEnv`s.
- **Where.** `.github/workflows/ci.yml` (step "Test"), `package.json:12` (`verify`), `.config/wt.toml` (pre-merge `verify`: the Gate).
- **Evidence.** Three full runs: 3:20.5, 2:55.0, 2:59.3 (wall, `/usr/bin/time -v`; sums of the binaries' own times 168-200 s). The same 44 binaries started with `xargs -P 4` (each as cargo would run it, env scrubbed, same empty ambient `CPD_HOME`): **53.6 s wall**, 44 of 44 exit 0, 1105 passed, 0 failed (N=1, load 25-37). No cross-binary interference appeared.
- **Impact.** The Gate (`npm run verify`) and every PR check wait for the serial run; 3 minutes -> about 1 minute is the largest single speed-up found, and it is paid at every `pr open`, `pr rebase` and CI run.
- **Proposed fix.** `cargo nextest run --workspace` (upstream tool, runs tests across binaries in parallel; not installed here, **unverified** that it is as fast), or `cargo test --no-run --message-format=json` and a small parallel runner. A deviation from `cargo test` needs its reason written (AGENTS.md: convention over configuration): this is the measured gain. Keep 3 consecutive green parallel runs as the acceptance (hs-d31).
- **Size.** Job.
- **Overlap.** Plan dev-speed (`cache`: sccache) speeds compiles, not test runs; no overlap.

### tests-build-F2: dev profile with full debug info

- **What.** `Cargo.toml` has no `[profile]` table: dev builds get `debug = true` (full DWARF) for 122 crates and 44 test binaries.
- **Where.** `Cargo.toml` (no `[profile.dev]`).
- **Evidence.** Clean `scripts/cargo.sh test --workspace --no-run` into a private `CARGO_TARGET_DIR` per setting, the setting given as an environment variable (no file changed), N=1 each, load at start 20 / 26 / 31:
  ```
  debug=default            : clean test build 45.2 s, target 4447 MB, then a core edit + test build 12.9 s
  debug=line-tables-only   : clean test build 31.2 s, target 2231 MB, core edit + test build  7.4 s
  debug=0                  : clean test build 28.1 s, target 1131 MB, core edit + test build  6.7 s
  ```
  The sizes are deterministic; the times moved with the load (single runs, treat as direction). `du -sh target/debug/deps` = 3.6 GB; the debug `cpd` is 119 MB (`docs/research/binaries.md`).
- **Impact.** Up to six Workers each hold a worktree with its own `target/` (hs-d136): about 27 GB of disk for builds at once, written and linked on every build. Backtraces keep file:line with `line-tables-only`; a debugger loses variables.
- **Proposed fix.** `[profile.dev] debug = "line-tables-only"` (or `debug = 1`) with the reason written next to it; re-measure with N >= 3 before adopting.
- **Size.** Job.
- **Overlap.** Plan dev-speed `cleanup` (`scripts/prune-worktrees.sh`) frees worktrees after merge; this makes each smaller. Not covered by sccache.

### tests-build-F3: the release clean build waits for SQLite's C compile

- **What.** `libsqlite3-sys` (rusqlite `bundled`, `Cargo.toml:21`, reason in `docs/implementing.md:94`) compiles the SQLite amalgamation in its build script at `-O3`; `cpd-core` cannot start before it ends.
- **Where.** `Cargo.toml:21`; `scripts/install-cpd.sh:12` (`build --release --locked`).
- **Evidence.** `scripts/cargo.sh build --release -p cpd --timings` on a clean `target/` (`cargo-timing.html`, parsed `UNIT_DATA`): `libsqlite3-sys build-script (run)` 60.7 s from t = 4.7 s; `cpd-core` starts at 66.0 s and takes 15.9 s; total 82.3 s. In the dev build the same unit is 7.7 s. Three clean release builds: 60.5, 64.7, 82.3 s.
- **Impact.** Paid once per clean `target/` (every new worktree that builds release, CI without a cache, the first install after a toolchain bump). Rebuilds after a code edit skip it (incremental table above).
- **Proposed fix.** None in this repo beyond what dev-speed does: the `cc` crate used here (1.8.0) prefixes the C compiler with `RUSTC_WRAPPER` when it is `sccache` (`~/.cargo/registry/src/*/cc-1.8.0/src/lib.rs:4092`, `rustc_wrapper_fallback`), so `scripts/cargo.sh` exporting `RUSTC_WRAPPER=sccache` (on `origin/main`, `scripts/cargo.sh:25-26`) caches this C compile too, **if sccache is installed**; it is not on this machine (`which sccache` empty): **unverified** that it is installed on the Workers' machines.
- **Size.** Job (confirm after the sccache install; no code).
- **Overlap.** Plan dev-speed `cache` PR (#18 on `origin/main`, hs-d137, hs-n78) and `docs/research/binaries.md` (hs-q5).

### tests-build-F4: the slowest tests wait on constants

- **What.** The long tests are long because of values they cannot shorten: the engine's `tick_seconds` is a whole number of seconds (the e2e tests set `tick_seconds = 1`, `e2e_cross.rs:159`, `e2e.rs:111`), so every hand-off through the engine costs 1-2 s; `PI_STOP` is 10 s (`core/src/session/run.rs:39`) and `session_reload.rs:257` asserts it (`>= 10 s`); `session.rs:455,466` sleep 3 s twice.
- **Where.** `core/src/watch/engine.rs:213` (the tick nap), `core/src/session/run.rs:39`, the tests above.
- **Evidence.** `e2e_cross` alone: 20.66 s median (N=5), 6 steps through the engine; `watch.rs e5_...` 7.35 s; `session_reload ...ten_seconds...` 11.61 s; the sleeps of W3 total 63 s. See the tables above.
- **Impact.** About 90 s of the serial suite (e2e_cross + session_reload + the e5 tests + the fork test), wall time that does not scale with the machine.
- **Proposed fix.** Decide with the User (contract change): a sub-second tick and a configurable `PI_STOP` in `[timeouts]` for tests (`fork_quiet_seconds` already is), or wake the engine on a store Event instead of the next tick.
- **Size.** Plan (contract §3.7, commands §12, config).
- **Overlap.** Cross-area (engine): `docs/research/quality-engine.md` engine-F/W notes on the tick; the E5 timing (`quality-engine.md:123`) is the same period question.

### tests-build-F5: 40 integration-test binaries

- **What.** Every `cli/tests/*.rs` is its own binary linking `cpd-core`, `cpd-tui`, `rusqlite` (bundled SQLite) and `ratatui`; each is 75-104 MB in debug.
- **Where.** `cli/tests/` (40 files), `cli/Cargo.toml` (autodetected).
- **Evidence.** `ls -l target/debug/deps/{ask,attempt,command_index,git,pr,store,e2e}-*`: 13.9 MB (`e2e`, a thin one) to 104.1 MB (`pr`), most 75-93 MB; `du -sh target/debug/deps` 3.6 GB of a 4.4 GB `target/`; an incremental `core/` edit relinks all 44 (5.2 s median against 3.6 s for `build`).
- **Impact.** Disk (3.3 GB per worktree) and relinking. The gain from one binary is **unverified** (not tried); the cost is that tests of different files would share one process, so the per-file statics (`TMUX_ENV` in `host.rs`, `ONE_AT_A_TIME` in `driver_reload.rs`) become cross-file and F1's parallelism moves from binaries to tests.
- **Proposed fix.** Not before F2; measure one merged binary on a branch first. Cargo's own guidance is a single `tests/it/main.rs`.
- **Size.** Plan.
- **Overlap.** None.

### tests-build-F6: the core unit-test time is one test

- **What.** `migrating_an_empty_database_gives_every_versions_schema_hash` creates and migrates one database per schema version (`SCHEMA_HASHES`, 9+ versions) under a temp dir, 3.3 s of the core lib's 7.8 s.
- **Where.** `core/src/store/schema.rs:282-290`.
- **Evidence.** `--exact` ×5 alone: 3.78, 3.60, 3.30, 2.23, 2.66 s; in the suite 6.13 s. `/tmp` is ext4 on NVMe (`df -T /tmp`), not tmpfs. That the cost is `fsync` per migration is **unverified** (guess: each `at_version` commits per migration on a durable journal).
- **Impact.** Small: it sets the length of the core unit run, which cargo runs before the 40 integration binaries.
- **Proposed fix.** Use `PRAGMA synchronous = OFF` (or an in-memory connection) in the test's `connection()` helper; keep the hash assertions.
- **Size.** Job (minutes).
- **Overlap.** Plan manager adds migrations 0007-0009: every new version adds one more database to this test.

## Good

### tests-build-G1: nine warnings and nothing gates them

- **What.** Seven are dead code, one an unused variable pair, one an unused method in a test helper. None fails the build or CI.
- **Where.** `core/src/plan/row.rs:42` (`PlanPr` fields `key repo base lead_branch lead_worktree pull_request all_merged_at` never read), `:57` (`PlanRow` fields `title jira pr ...` never read), `:160` (`PlanRow::to_json` never used), `:174` (`row::query` never used); `tui/src/plan/actions.rs:30` (field `run_command` never read), `:44` (`Actions::open_lead` never used); tests: `cli/tests/store.rs:954` (`KillOnDrop::into_inner` never used), `cli/tests/job.rs:972` (two unused variables: this is W1).
- **Evidence.** `scripts/cargo.sh build --workspace` (clean): `warning: multiple fields are never read --> core/src/plan/row.rs:42:9`, `... row.rs:57:9`, `method to_json is never used --> row.rs:160:12`, `function query is never used --> row.rs:174:15`, `field run_command is never read --> tui/src/plan/actions.rs:30:5`, `method open_lead is never used --> tui/src/plan/actions.rs:44:12`; `... cpd-tui (lib) generated 2 warnings`, `cpd-core (lib) generated 4 warnings`. `scripts/cargo.sh test --workspace --no-run` adds the three test warnings. `grep -n 'warnings\|RUSTFLAGS' .github/workflows/ci.yml package.json .config/wt.toml scripts/cargo.sh`: no `-D warnings`.
- **Impact.** The dead `PlanRow` read path (`find`/`load` are used, `query`/`to_json` are not) suggests `plan show`/`plan list` read the table another way; the warning that hides W1 shows nobody reads warnings.
- **Proposed fix.** Delete the dead items after checking nothing in flight needs them (`tui/src/plan/actions.rs` came with the plan view; its `open_lead` is the tab click that the tree later replaced: unverified); fix the test warnings; then gate: `RUSTFLAGS=-D warnings` in the CI build step and in `scripts/cargo.sh`'s Gate use (or `[workspace.lints.rust] warnings = "deny"`), with the reason written.
- **Size.** Job.
- **Overlap.** Plan manager renames `Phase::Pr` to `Deploy` in `core/src/plan/row.rs` (hs-d112, hs-n78: "whoever lands second rebases"): do the `row.rs` deletion after it lands or in the same PR. Plan tui-dots/tui-visuals touch `tui/src/plan/`.

### tests-build-G2: test helpers copied per file

- **What.** The same helpers are redefined in many test files, with different constants: `wait_until` in 10 files (and 6 variants named `wait`, `wait_for`, `wait_status`, `wait_end`, `wait_event`, `wait_screen`), deadlines 10 s (7 uses), 20 s (3), 30 s (13), 60 s (6); `fn refused(run: &Run, code: &str, exit: i32) -> Value` identical in `ask.rs attempt.rs job.rs plan.rs pr_start.rs review_job.rs session.rs`; `sql` in 12 files; `write_config` in 5; `fake_cpd` in 5 variants.
- **Where.** e.g. `cli/tests/{tui_logs,tui,pr_start,session,tui_plan,plan,tui_statusbar,watch,session_reload,driver_reload}.rs` (`wait_until`); `common/mod.rs` has none of them.
- **Evidence.** `grep -ln 'fn wait_until' cli/tests/*.rs | wc -l` = 10; `grep -ln 'fn refused(run: &Run, code: &str, exit: i32) -> Value' cli/tests/*.rs` = 7 files; `grep -c 'fn sql(' cli/tests/*.rs | grep -vc ':0'` = 12; `grep -rn 'from_secs(\(10\|20\|30\|60\))' cli/tests/*.rs | grep -i 'deadline\|elapsed'` counts above.
- **Impact.** One wait policy would mean one place to make waits load-proof (W3) and one message format; today a 10 s deadline (`host.rs:55`, `tui_worker.rs:138`, `tui_history.rs:123`) sits beside 60 s ones. 24 k lines of tests; AGENTS.md DRY.
- **Proposed fix.** `common::wait_until(what, timeout, f)` with one default (30 s), `common::refused`, `TestEnv::sql`; migrate file by file.
- **Size.** Job per 5 files; Plan for all.
- **Overlap.** None.

### tests-build-G3: `set_var` in a multi-threaded test binary

- **What.** Two tests mutate the process environment while other test threads run: `git.rs:476-480` sets `TMUX`, `TMUX_PANE`, `PI_GITTEST`, `CPD_GITTEST`, `GITTEST_KEEP` and never restores them, with no `SAFETY` comment (Rust 2024 makes `set_var` `unsafe` because of exactly this); `host.rs:35` sets `TMUX_TMPDIR` under the `TMUX_ENV` mutex (a `SAFETY` comment, but only the tests that take the lock are protected, and the value stays for the rest of the process).
- **Where.** `cli/tests/git.rs:476-480`, `cli/tests/host.rs:20-36`.
- **Evidence.** The lines cited. No failure seen in 4 passes.
- **Impact.** Undefined-behaviour class (concurrent `getenv`/`setenv` in libc) and hidden coupling: `TMUX` set to a fake path in `git.rs` would reach any later `Command` in that binary that does not scrub it.
- **Proposed fix.** `git.rs`: run the hook in a child with `Command::env("TMUX", ...)` (the hook runner is a function of the child's env, or test through `env_clear` of the `Command`); `host.rs`: pass `TMUX_TMPDIR` to `Tmux::new` instead of the process env if the type allows it.
- **Size.** Job.
- **Overlap.** None.

## Already good, keep

- **Flake record.** 4 full passes (3 `cargo test --workspace`, 1 per-binary), 1 parallel pass, 25 stress runs at load 34-37: 0 failures, 0 `Text file busy`. The hs-n12 fix holds: `TestEnv::fake` claims call numbers with `O_EXCL` and publishes `<n>.done` last (`common/mod.rs:200-224`), and `tui_logs` passed 5 of 5 under stress and in every full pass.
- **The harness** (`cli/tests/common/mod.rs`): `HOME` and `CPD_HOME` per test, a private tmux server per `TestEnv` killed on drop, `TMUX_TMPDIR` private, git global/system config off, `CPD_*`/`PI_*`/`GIT_*`/`XDG_*`/`TMUX*` removed, fake `pi`/`gh` that record argv (`harness.rs` tests all of that, 8 tests). With an empty temporary `CPD_HOME` set in the ambient environment, the directory was still empty after every pass: no test used the ambient one.
- **Contract coverage.** All 64 commands of commands.md §2 are exercised by some test (`command_index.rs` also checks that each exists in clap); 116 of 116 error codes appear in a test (the five not found by string, `migration_failed`, `store_tampered`, `tmux_failed`, `spawn_failed`, `not_awaiting_review`, are covered by `ErrorCode::` variants in `store.rs:707,608`, `driver/worker.rs:371`, ... or are the removed code of `commands.md:2008`). I found **no** named contract behaviour without a test in the areas I probed (commands, error codes, events table `events.rs:200-218`); this is a probe, not a proof.
- **Waits poll with a deadline and a message** (`wait_until(what, ...)` prints what it waited for and the screen), not blind sleeps, in the 16 files that wait on a state; panics name the Attempt id and log path (`review_job.rs:179`).
- **Incremental builds are fast** (core edit 3.6 s, tui edit 1.9 s, N=10) and the dev build has six crates' worth of parallelism; the workspace pins every dependency once in `Cargo.toml:11-26` with a reason in `docs/implementing.md`; the dependency tree has two upstream duplicates only (`hashbrown`, `syn`).
- **Tests that cannot reach the machine:** `cargo_script.rs` and `harness.rs` test their scripts with stub `PATH`s; the one `#[ignore]` (`exe.rs:424`) is a child-process helper that the parent test runs on purpose (it is why every run says "1 ignored").

## Method

Worktree `/home/clayton/projects/cpd.quality--tests-build-a1`, branch base `e8ad70d`. Nothing was run against the live `~/.cpd`, the live engine or the `-L cpd` tmux server, and no copy of `~/.cpd/cpd.db` was needed: these measurements start no cpd command against a store except the tests' own temporary ones. Every test run used `env -u TMUX -u TMUX_PANE -u CPD_ATTEMPT -u CPD_PLAN -u CPD_SESSION CPD_HOME=<empty temp dir>` (removed at the end); the harness already scrubs these for the cpd it spawns. Processes I started (busy loops of the stress run, the test runs) were stopped; the private `target-*` dirs under `/tmp/tb-a1/` and the temp dirs were removed. Read-only observation of other Workers' processes: `ps`, `ls /proc`, `ls /tmp`.

Commands (all from the worktree):

- Clean builds: `scripts/cargo.sh clean`, then `/usr/bin/time -v scripts/cargo.sh build --workspace` (x3, #2 with `--timings`), `/usr/bin/time -v scripts/cargo.sh build --release -p cpd` (x3, #2 with `--timings`); warnings read from the build log.
- Incremental: a loop of 10, each `touch core/src/ids.rs` or appending `fn bench_N() -> u32 { N }` to it (and the same for `tui/src/palette.rs`), `/usr/bin/time -f %e scripts/cargo.sh build --workspace` and `test --workspace --no-run`; `git checkout` restored the files.
- Suite: `scripts/cargo.sh test --workspace --no-run` (17.6 s with the deps built), then 3 x `/usr/bin/time -v scripts/cargo.sh test --workspace --no-fail-fast`; per-binary times from `finished in`.
- Per test: each executable from `cargo test --no-run` run directly (cwd = its package) with `RUSTC_BOOTSTRAP=1 <exe> -Z unstable-options --report-time`; the top 12 re-run alone with `--exact` x5.
- Stress: 16 x `sh -c 'while :; do :; done'` for about 100 s while running the `git` binary 10x, `host`, `tui_logs`, `tui_worker` 5x each (load 34-37); the loops were killed (checked with `ps`).
- Parallel binaries: the 44 executables through `xargs -P 4`.
- No-reach check: the 44 executables with a directory of `gh pi curl wget ssh chezmoi claude` shims (log + exit 99) first on `PATH` and `GH_TOKEN`/`GITHUB_TOKEN` set.
- Debug info: `CARGO_TARGET_DIR=/tmp/tb-a1/tgt-<x> [CARGO_PROFILE_DEV_DEBUG=<v>] scripts/cargo.sh test --workspace --no-run`, `du -sm`.
- Size: `size -A`, `strip -o`, `nm --size-sort -S -C target/release/cpd`, grouped by a script over the demangled names.
- Dependencies: `scripts/cargo.sh tree --workspace --depth 1`, `... -d`, `-e normal`.
- Static: `grep -rn 'sleep\|Duration::from' cli/tests`, `grep` for `set_var`, helper names, `#[ignore]`; a script that lists `#[test]` functions without any assertion-like call (16 hits; read the six suspicious ones: only `job.rs:964` cannot fail on its claim); error-code and command coverage by string search in `cli/tests` and `core/src`/`tui/src`.
- Context: `cpd context show` (read-only) for hs-d11, hs-d27, hs-d31, hs-d63, hs-d136, hs-d137, hs-d157, hs-n12, hs-n78, hs-q5.
