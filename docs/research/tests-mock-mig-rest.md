# Research: in-process migration of the store, tooling, harness and core tests (Job `mr-rest`, attempt `mr-rest-a1`)

Static research of this worktree (base `885fa97a`). Nothing was run except `rg`, `wc`, `read`. Counts are `rg -c '#\[test\]'` today; `unverified` / `guess:` label the rest. Names `Runner`, `FakeRunner`, `Ports`, `run_with`, `testkit` (`Fx`) come from `~/.cpd/ref/tests-mock/*` and are unverified here (`cli::main_with(argv, &mut out)` exists, `cli/src/lib.rs:16`; the rest does not). Scope: `cli/tests/store` 110, `cli/tests/tooling` 261 (20 test files + `main.rs`), `core/tests` 36, `pi/test/pi-check.test.ts` 11 (the only pi test that runs a `scripts/` script; `gate.test.ts`, `tripwire-allow.test.ts`, `testguard.test.ts` only read text) = **418**. `cli/tests/common` has no test: it is the harness (§1.1).

**Findings that change the brief (read first).**
1. Four counted tests are not tests: `store.rs::child_writer` (re-runs the test binary as a child, gated by `STORE_CHILD_OUT`), `harness.rs::wrapper_helper` and `envs_helper` (helpers gated by an env var), and `store.rs::a_guarded_child_is_gone_after_a_panic` (tests the test's own `KillOnDrop` with a real `sleep 60` and `kill -0`). They die with the exec harness (§2, row *die*).
2. **39 tests have a shell script as their subject** (`cargo.sh`, `npm-fresh.sh`, `pi-check.sh`, the `cpd slot` users) and cannot run in-process: §5 needs a Lead decision (named exception or shrink). 34 of them do not exist in any `rs-*` PR.
3. ADR counts are stale (harness 34 → 32; `install_script` 52 → 51; `version` 4 → 12; `e2e.rs` is already gone). `tests-mock-ports.md` §6.5 already makes the **red Jobs of `rs-install/tag/prune/gate-run/tools`** carry the script tests: I assign them to those PRs, not to a second PR of mine (§4).
4. `events follow` writes to `std::io::stdout().lock()` and polls the reader (`core/src/events/mod.rs:134,172`), sleeps `thread::sleep(poll)` (`:181`) and re-execs through `exe.exec` (`:179`): it needs a **writer sink** (not in `Ports` as designed) to run under `run_with` (guess: 1 small impl Job).
5. `harness.rs::no_test_mutates_the_process_environment` forbids what `tests-mock-ports.md` §4.4 option 1 proposes (set env/cwd at the start of a test). One of the two must change; the guard is the cheaper (it is a source scan).
6. `.config/nextest.toml:76` names `install_script::the_install_restarts_the_engine_of_a_real_cockpit`: no such test exists (`rg` on `cli/tests`): a stale override, not in this scope's fix.

## 1. Inventory

Targets: **U** unit/no seam; **I** in-process on `Ports` (seams in §2); **S** smoke with the real binary (≤3 per area); **X** stays an exec test (§5); **die** the subject disappears with the exec harness. "TestEnv" = `cli/tests/common/mod.rs` (a temp home, a process-group reaper `start_reaper` :251, `TEARDOWN_SH` :123, a private tmux socket, `sqlite3`).

| path | tests | what its tests start today | target |
|---|---|---|---|
| store/events.rs | 35 | TestEnv (reaper) in all; `env.cpd` for `events list`/`follow` in 26; a real `cpd events follow` child with piped stdout (`Follower`, :700) in 10; a thread that migrates with `sleep` (:580); `wait_until`/`poll` | 9 U, 25 I (run_with), 1 I (+spawn exec) |
| store/store.rs | 68 | TestEnv in 49; `env.cpd version …` 7; `sqlite3` CLI 2 (:`a_connection_without_cpd_txn…`, `schema_migrations_records…`); 11 real child writers (`Writer`, :1318; re-run test binary; `flock`, `sleep`, `wait_until`); `thread::sleep(1100)` in `a_slow_write…` | 47 U, 7 I, 12 I (clk/spawn), 2 die |
| store/store_asleep.rs | 1 | TestEnv::empty only for a temp home | U |
| store/store_local_needs.rs | 2 | rusqlite on `prefix_database`, no process | U |
| store/store_window.rs | 4 | TestEnv; 2 run `env.cpd` | 2 U, 2 I |
| tooling/cargo_script.rs | 12 | real `sh scripts/cargo.sh` with `write_executable` fake `cargo`/`sccache` | X |
| tooling/dead_code.rs | 1 | reads `*.rs` | U |
| tooling/fake_tmux.rs | 57 | 14 `bin_*`: the built `fake-tmux` bin (`CARGO_BIN_EXE_fake-tmux`), `bounded()` kill by a timeout thread; 4 `harness_*`: TestEnv hooks; 39 `adapter_*`: `Tmux` against the bin | see §2 |
| tooling/gate_run.rs | 10 | `sh scripts/gate-run.sh` with real leftover processes, dirs, tmux-socket files; `SLEEP_SECONDS`, `wait_until` | 6 I, 2 S, 2 die |
| tooling/harness.rs | 32 | TestEnv itself: real tmux server, real reaper, git wrapper process, flock shim; 3 are helpers | see §2 |
| tooling/install_script.rs | 51 | `Checkout` (`common/checkout.rs`: a scratch copy of `scripts/` with an `origin/main`), real `git`, shell fakes for `cpd`/`tmux`/python3, `sleep`, `flock` (python3); 10 run `check-portable.sh` | 41 I, 10 U |
| tooling/interface.rs | 3 | runs `CARGO_BIN_EXE_cpd --help` (`help_tree`, :14) | U (`Cli::command()`) |
| tooling/layout.rs | 4 | 2 read files; 2 run real `cargo metadata` / `cargo tree` (:177, :253) | 2 U, 2 X |
| tooling/no_repo_lock.rs | 2 | built `cpd attempt run` and `pr rebase`, real `git` with a slow wrapper first on PATH, `wait_until` | I (all seams) |
| tooling/npm_fresh.rs | 4 | real `sh scripts/npm-fresh.sh`, fake `npm` | X |
| tooling/pill_parity.rs | 1 | nothing (a pure parser) | U |
| tooling/pill_sites.rs | 2 | reads source files | U |
| tooling/preview_script.rs | 11 | `sh scripts/preview-tui.sh`, fake `tmux`/`cargo`/`cpd`, 3 `git` | I |
| tooling/prune_script.rs | 5 | `sh scripts/prune-worktrees.sh`, real `git` (11), fake `gh`/`cpd`/`jq` | I (or delete) |
| tooling/slot.rs | 14 | 7: built `cpd slot` (real `flock`, `kill`, `wait_until`); 7: `cargo.sh` and `npm test` through a fake `cpd` | 6 I, 1 S, 7 X |
| tooling/tag_script.rs | 7 | `sh scripts/tag-version.sh`, real `git` (19) | I |
| tooling/test_changed_script.rs | 17 | `sh scripts/test-changed` in a real `git` repo with real `cargo metadata` | I |
| tooling/tests_script.rs | 6 | `sh scripts/tests.sh` running real `cargo`, `nextest`, `tsc`, `node` in a scratch workspace | I |
| tooling/tripwire_script.rs | 10 | `sh scripts/tripwire/tmux` with a copied `allow.txt` | die (1 S new) |
| tooling/version.rs | 12 | 3 compile `cli/build.rs` with real `rustc` in a git checkout; 5 run `sync-version.sh`; 1 runs `cpd --version`; 2 run `new-migration.sh`/`install-cpd.sh` with a fake cargo; 1 reads `Cargo.toml` | see §2 |
| core/tests/ask_fork.rs | 2 | `host::pi::extensions_to_pass`, `Config::parse`: pure | U |
| core/tests/distiller_launch.rs | 2 | `host::pi::distiller`: pure | U |
| core/tests/hooks_idle.rs | 1 | real `git init`, `run_command` runs a real `sh` under real `nice`/`ionice` (`git/hooks/exec.rs:112`) | I (runner), OS fact X |
| core/tests/migrations.rs | 19 | 13 rusqlite on `core/src/store/migrations/` + a source scan; 3 run `check-migrations-frozen.sh` in real git clones; 3 run `new-migration.sh` + `sync-version.sh` | 13 U, 3 I, 3 U after a port |
| core/tests/repin.rs | 4 | `scripts/repin.sh` in a real git repo with a real merge conflict | U after a port |
| core/tests/tier_g.rs | 3 | pure | U |
| core/tests/worker_tiers.rs | 5 | pure | U |
| pi/test/pi-check.test.ts | 11 | `scripts/pi-check.sh` + `pi-check-pty.py` on a real pty, node-script fakes (`spawnSync`, :41) | X (named exception pro-d9) |

### 1.1 What `cli/tests/common` provides, and what `testkit` replaces (`common/mod.rs` 1343 lines, `checkout.rs` 52)

| harness piece (line) | fate |
|---|---|
| `TestEnv::new/empty` temp home, `owner_policy`, `repo_policy`, `write_config`, `store()`, `read`, `query`, `sql` (:741-948), `Run`/`refused` (:550, :703) | **stay** as `Fx` (tk-1): the temp home and the env/cwd isolation (now `Fx` fields, since in-process there is no child env) |
| `env.cpd/cpd_in/cpd_with/command(CPD)` (:880, :979-989) | replaced by `run_with` |
| `fake(name, body)`, `fake_gh`, `fake_pi`, `calls`, `fake_cpd_views`, `DENIED_TOOLS` (:307), `write_executable`, `copy_executable`, `write_flock_shim`, `GitWrapper` (:580) | replaced by `FakeRunner` routes (default-deny = `DENIED_EXIT` 97) |
| `fake_tmux`, `tmux`, `tmux_state`, `kill_tmux_server`, `real_tmux` (:1341), `tmux_tmpdir_for` (:640) | replaced by the tmux route (`State` stays) |
| `git_repo`, `keyed_repo`, `github_origin`, `local_origin`, `git` (:1006-1070) | replaced by git-route fixtures (tk-3) |
| `wait_until(_for)`, `poll`, `wait_some`, `wait_screen`, `wait_whole_ticks`, `poll_searches`, `answer_reads`, `wait_for_flock_waiter`, `backdate` (:408-537, :1195-1204) | replaced by `ManualClock` helpers (tk-5) |
| `spawn`, `output`, `start_reaper`, `TEARDOWN_SH`, `teardown`, `pgid` handling (:123-260, :395-405, :801) | **die** (no child, no reaper) |
| `Locale`, `tripwire_env_words` (:1289-1333), `Checkout` | die with the tripwire / move to `rs-install` |
| `PI_REPLY`, `CURATES_ALL`, `pay_search`, `pay_lookup`, `PLAN_POLLS` (data) | stay |

## 2. Seams

Seam names: ∅ none (a library call on a temp dir, or text), `clk`, `spawn`, `runner`, `git` (git-route), `tmux` (tmux-route), `run_with`, `testkit`. "+rs-x" = also waits for the product PR that ports the script to Rust (`tests-mock-ports.md` §5). Every claim names the code it comes from.

### 2.1 Per file

| file (tests) | minimum seams: tests |
|---|---|
| events.rs (35) | ∅ 9 (`append` API: `events::append`, no process) · run_with+testkit 16 (`list` 8; `follow --until-idle` 8: `cli::run`, no clock wait) · run_with+testkit+clk 9 (`Follower` tests: poll `thread::sleep(poll)` `events/mod.rs:181`, a writer sink: finding 4) · run_with+testkit+clk+spawn 1 (`follow_re_execs_…`: `exe.exec`, `:179`) |
| store.rs (68) | ∅ 47 (`Store::open`, rusqlite on a temp dir; the 2 `sqlite3` CLI tests become `raw(&env)` rusqlite) · run_with+testkit 7 (6 `version_*`, `a_write_from_a_deleted_cwd…`: cwd must be an injected value, `ports` §4.4) · clk 1 (`a_slow_write_is_logged…`: `store/mod.rs:524-541` `Instant`) · clk+spawn 11 (the cross-process writers/holders → threads; `store/lock.rs:60` `alive` = `kill(pid,0)`, `:133` `Instant`, `lock.rs:89` `sleep`; 2 tests use a dead pid → `FakeSpawn.alive`) · die 2 |
| store_asleep (1), store_local_needs (2) | ∅ |
| store_window.rs (4) | ∅ 2 · run_with+testkit 2 (`it_reads_and_writes_such_a_store`, `a_store_whose_newest_breaking…`) |
| fake_tmux.rs (57) | ∅ 12 (`bin_*` through `cpd::fakes::tmux::run` as a library call, `fakes/tmux` `run/Invocation/State`) · runner+tmux 2 (`bin_a_hook_…`, `bin_a_run_shell_b_hook_…`: the nested `run_shell`, `tmux-route` §G7) · runner+tmux 39 (`adapter_*`: job `tmux-conformance` of port-tmux) · die 4 (`harness_*`) |
| harness.rs (32) | ∅ 4 (source guards: `no_test_mutates_the_process_environment`, `no_test_sleeps_for_a_result…`, `the_shared_helpers_are_defined_only_in_the_harness`, `every_wait_polls_through_the_harness`; rewritten to the new helpers) · testkit 7 (`an_env_is_made_under_the_runs_tmpdir`, `refused_*` 4, `sql_writes_the_envs_store`, `a_new_env_has_a_migrated_store…`) · clk 2 (`wait_until_returns_once_ready…`, `wait_some_says_the_last_state…`) · runner 3 (`fakes_shadow_the_real_programs_and_record_calls`, `concurrent_fake_calls_are_recorded_whole`, `calls_come_back_in_the_order_they_started`: the route's call log) · runner+git 1 (`git_repo_has_one_commit_on_main`) · run_with+testkit 1 (`commands_get_an_isolated_environment`) · die 14 (`tmux_server_is_private…`, `a_dropped_env_leaves_…`, `a_killed_test_leaves_no_git_wrapper_process`, `wrapper_helper`, `envs_helper`, `a_process_that_leaves_the_group…`, `a_recorded_pid_that_is_not_the_envs_is_spared`, `teardown_is_idempotent…`, `the_tmux_dir_stays_in_the_env…`, `a_command_does_not_inherit_zdotdir`, `the_env_gitconfig_turns_automatic_maintenance_off`, `the_flock_shim_holds_the_lock…`, `no_fake_sleeps_longer_than_sleep_seconds`, `executables_are_written_and_copied_only_through_the_harness`) |
| install_script.rs (51) | runner+git 10 (migration refusal 6 + store verdict 4: the `git fetch/show/cat-file` route and the built `cpd` as a route) · runner+tmux+spawn+clk 17 (follow-ups, old-engine lock wait >10 s, `kill-window`, `tag` steps, `mark-restart`) · spawn+clk 14 (adapter swap, cleanup of old `pi.*` > 60 min and dead `pi.tmp.<pid>`, compact tools; `fs::rename`) · ∅ 10 (`check-portable.sh` → a Rust scanner test over strings) · all +rs-install |
| gate_run.rs (10) | spawn 6 (+rs-gate-run: `Spawn::with_env`, ports §5.5) · S 2 (the shell wrapper) · die 2 (the two `tripwire log` tests) |
| slot.rs (14) | run_with+testkit+clk+spawn 6 (`cpd slot`: real `flock` on a temp file stays, `lock.rs:89` sleeps, a signal = `Spawn::signal`) · X-OS 1 (`a_killed_holder_frees_its_slot…`) · X 7 (the `cargo_script_*` / `npm_test_*` ones: shell) |
| version.rs (12) | ∅ 4 (`cargo_version_is_zero…` + 3 `build.rs` tests once its logic is a function: `rustc` today at :51) · ∅+rs-tag 6 (5 `sync_*`, `new_migration_runs_the_sync`) · run_with+testkit 1 (`cpd_reports_the_commit…`) · runner+git 1 (`the_install_script_builds_with_the_checkout_s_short_commit`, +rs-install) |
| layout.rs (4) | ∅ 2 · X 2 (`cargo metadata`, `cargo tree`: subject is cargo's resolver) |
| no_repo_lock.rs (2) | run_with+testkit+runner+git+clk+spawn 2 (`attempt run` driver, `pr rebase` Gate; the slow `git` becomes a route barrier) |
| preview_script.rs (11) | runner+git+tmux 11 (+ a Rust `cpd preview`; not in the ADR: ask) |
| prune_script (5), tag_script (7) | runner+git (+rs-prune, +rs-tag) |
| test_changed_script (17) | runner+git (+rs-tools: canned `git diff`/`status` and `cargo metadata`) |
| tests_script (6) | runner (+rs-tools: canned `cargo`/`nextest`/`node`/`tsc`) |
| cargo_script (12), npm_fresh (4) | X (§5) |
| tripwire_script (10) | die (the allow-list logic dies with `allow.txt`; ADR "Tripwire script stays (1 line)") |
| dead_code (1), pill_parity (1), pill_sites (2), interface (3) | ∅ |
| core ask_fork (2), distiller_launch (2), tier_g (3), worker_tiers (5) | ∅ (move next to the code) |
| core migrations.rs (19) | ∅ 13 (already in-process) · runner+git 3 (frozen check, +a Rust `frozen` check: `git diff --name-status -M --diff-filter=MDR` on the git route, `-M` unverified in the fake) · ∅+port 3 (`new_migration_*`) |
| core repin.rs (4) | ∅+port (the conflict-hunk rewrite as a function over text; `scripts/repin.sh` keeps 5 lines of `git` glue) |
| core hooks_idle.rs (1) | runner (the `Launch` argv has `idle_prefix()`, `exec.rs:112`; needs `port-run2` pr2-3) |
| pi-check.test.ts (11) | X |

### 2.2 Needs-set table (decides which PR starts after which seam; sums to 418)

| # | needs | tests | which |
|---|---|---|---|
| 1 | ∅ | 125 | store 61 · core 12 + 13 · tooling 9 (dead_code, pill, interface, layout 2) · harness guards 4 · check-portable 10 · version 4 · fake_tmux `bin_*` 12 |
| 2 | ∅ after a Rust port of a script | 13 | sync 5, `new_migration_*` 4, repin 4 |
| 3 | testkit | 7 | harness |
| 4 | clk | 3 | store slow write, harness 2 |
| 5 | spawn | 6 | gate_run |
| 6 | spawn+clk | 25 | store 11, install adapter swap 14 |
| 7 | runner | 10 | harness 3, tests_script 6, hooks_idle |
| 8 | runner+git | 44 | harness 1, install 10+1, tag 7, prune 5, test_changed 17, frozen 3 |
| 9 | runner+tmux | 41 | fake_tmux 39+2 |
| 10 | runner+git+tmux | 11 | preview |
| 11 | runner+tmux+spawn+clk | 17 | install follow-ups |
| **needs `run_with` (the CLI entry in-process)** | | | |
| 12 | run_with+testkit | 27 | store 7+2, events 16, version 1, harness 1 |
| 13 | run_with+testkit+clk | 9 | events follow streaming |
| 14 | run_with+testkit+clk+spawn | 7 | events re-exec 1, slot 6 |
| 15 | run_with+testkit+runner+git+clk+spawn | 2 | no_repo_lock |
| 16 | stays exec (§5) | 39 | cargo_script 12, npm_fresh 4, slot 7+1, layout 2, gate wrapper 2, pi-check 11 |
| 17 | die | 32 | harness 14, fake_tmux `harness_*` 4, tripwire 10, gate_run 2, store 2 |
| | **total** | **418** | |

Library-call-only rows (no `run_with`): 1-11 = 299. `run_with` rows: 12-15 = 45. Start order by seams: row 1-2 now; 3-4 after testkit/clk; 5-6 after clk+spawn; 7-8 after port-run/port-git; 9-11 after port-tmux; 12-15 after `run_with` (ports-3) and the follow sink.

## 3. The deleted tests of the scope (`tests-mock-deleted.md`: 231 rows; 1 in this scope)

| deleted test | file today | target | seam |
|---|---|---|---|
| `cpd::tooling` `a_killed_test_leaves_no_tmux_server_process_dir_or_reaper` (SIGKILL of a helper test; the reaper must stop the tmux server and remove its dir) | `cli/tests/tooling/harness.rs` (its sibling `a_dropped_env_leaves_no_tmux_server_process_dir_or_reaper` :428 is still there; both are *die* in §2) | **not restored as a test**: its subject (a test-owned tmux server and a reaper) is deleted by `mig-final`. The fact it protected ("a test leaves nothing behind") is enforced by the guard of `tests-mock-ports.md` §6.4 (no `Command::new`/`sleep` outside the allow-list) and by the Gate leak check. If the Lead wants a test: one in `gate_run` (row 5) over `Spawn::with_env`: a leftover with the run id is found and stopped | spawn (`FakeSpawn.with_env`) |

The other 230 rows are in `engine`, `flow`, `pr`, `session`, `tui`: none belongs here (`rg 'cpd::(tooling|store)'` gives that one row; no row names `core/tests` or `pi/test`).

## 4. Proposed migration PRs (4; fewest seams first)

The 15-60 bound cannot hold for all: 418 tests, 4 PRs, and 111 of them belong to other PRs already planned (below). PR1 and PR2 are over 60 because most of PR1 is one mechanical edit per file; the Worker time is per file, not per test (guess).

### `mr-pure` (needs: none; 86 tests)
Files: `store/{store,store_asleep,store_local_needs,store_window,events}.rs` (61 ∅), `core/tests/{ask_fork,distiller_launch,tier_g,worker_tiers}.rs` (12), `tooling/{dead_code,pill_parity,pill_sites,interface,layout}.rs` (9), the 4 guards of `harness.rs`.
1. test (S2): `store/*` ∅ tests use a temp home from `tempfile`, not `TestEnv::new` (no reaper, no tmux, no `sqlite3`: the two `sqlite3` CLI tests use `raw(&env)`).
2. test (S1): the 12 pure `core/tests` move to unit tests next to `host::pi` and `tier` (`core/src`, exact module unverified).
3. test (S2): `interface.rs` calls `Cli::command()` instead of `CARGO_BIN_EXE_cpd --help`; `layout.rs` splits its 2 pure tests from the 2 cargo ones (left X).
4. impl (S1): the 4 harness guards move to `tooling/guards.rs` and gain a check that `TestEnv::new` is not called in `store/`.
5. docs (S1): `docs/implementing.md` Tests: the store area is in-process.

### `mr-inproc` (needs: `run_with`, `testkit`, `clk`, `spawn`; 64 tests)
Files: `store/{store,events,store_window}.rs`, `tooling/{slot,version,harness}.rs`.
1. test (S2): clk 3 + testkit 7 (harness): `ManualClock` helpers' own tests, `Fx` isolation tests (`an_env_is_made…`, `refused_*`, `sql_writes…`).
2. test (S3): store clk+spawn 11 and clk 1: a writer thread held by a channel (not a sleep), `FakeSpawn.alive` for the two dead-pid tests; real SQLite `busy_timeout` stays a short real wait inside C (§5).
3. test (S2): `run_with` 27: `events list`, `follow --until-idle`, `version_*`, the window and deleted-cwd tests, `cpd --version`.
4. impl (S2): a writer sink and a clock for `events follow` (`core/src/events/mod.rs:134,172,181`), `Spawn::exec` for the re-exec.
5. test (S3): `follow` streaming 9 + re-exec 1; `slot` 6.
6. docs (S1): remove the `store_*` and `slot` words from `docs/implementing.md` "real flock" notes (unverified wording).

### `mr-scripts` (needs: `runner`, `git`, `tmux`; 36 tests, plus the Rust ports of 4 small scripts)
Files: `core/tests/{migrations,repin,hooks_idle}.rs`, `tooling/{preview_script,version}.rs`, `install_script.rs` (the 10 `check-portable` tests).
1. test (S2): `check-portable.sh` → a Rust test over strings (patterns of `scripts/check-portable.sh`, 175 lines, the 10 tests keep their names).
2. impl (S2): `repin` and `check-migrations-frozen` as functions (text hunks; `Runner` git); `new-migration` as `cpd` dev command with `rs-tag`'s `version sync`; their 10 tests move (repin 4 ∅, frozen 3 runner+git, `new_migration_*` 3 ∅).
3. impl (S2): `cli/build.rs` logic as a function in a file both `build.rs` and a test include (unverified: `#[path]`); the 3 tests read its result, not `rustc`.
4. test (S3): `hooks_idle` asserts the argv of the `Launch` (`idle_prefix()`), the `nice` fact is §5.
5. test (S3): `preview_script` 11 on runner+git+tmux after a Rust `cpd preview` (ask the Lead: not in the ADR).

### `mr-install` (needs: `runner`, `git`, `tmux`, `spawn`, `clk`; 50 tests, + `rs-install`, `rs-gate-run`)
Files: `tooling/{install_script,gate_run,no_repo_lock,version}.rs` (41 + 1 + 6 + 2).
1. test (S3): migration refusal and verdict 10 (+ the version/commit one): `Checkout` becomes a `git` route fixture.
2. test (S3): adapter swap, cleanup, compact tools 14 (`fs::rename` of a symlink; the 3 python3 tests keep a name without "python", `a_missing_python3_is_refused…` has no subject: §5).
3. test (S3): follow-ups, engine lock wait, tag, mark-restart 17.
4. test (S2): gate leak check 6 on `Spawn::with_env`; 1 smoke for the wrapper.
5. test (S3): `no_repo_lock` 2 on all seams (the slow `git` is a route barrier; the two windows overlap check uses `ManualClock`).
6. docs (S2): `docs/implementing.md` / `contract.md` text that names `install-cpd.sh`, `gate-run.sh`.

**Owned by other PRs (111 tests), not duplicated here:** port-tmux job `tmux-conformance` takes the 39 `adapter_*` (`tests-mock-tmux-route.md` job 7); the 14 `bin_*` stay until `mig-final`; **`rs-tag/prune/tools`** red Jobs take `tag_script` 7, `prune_script` 5, `test_changed_script` 17, `tests_script` 6, `sync_*` and `new_migration_runs_the_sync` 6 (`tests-mock-ports.md` §6.5; if `prune-worktrees.sh` is no longer used, delete first: ask); testkit `tk-2`/`tk-3` take the 4 harness runner/git tests; 13 `migrations.rs` tests need no change. Note: `rs-install` red (rsi-1) and my `mr-install` list the same 51 tests: dispatch one (guess: mine, with 10 moved to `mr-scripts`).

## 5. Not migratable as asked (subject is OS behaviour or a shell script)

Policy [tests] forbids a shell script or a real external in a test; each item names the way to keep it. Decision for the Lead: items 1-3 need a named exception; I recommend none of them be silent.

| # | tests | why | proposal |
|---|---|---|---|
| 1 | `cargo_script` 12, `npm_fresh` 4, `slot` 7 (`cargo_script_*`, `npm_test_*`) | the subject is `sh` resolving a program and exporting `RUSTC_WRAPPER`; `cargo.sh` builds `cpd`, so it cannot be a `cpd` subcommand (`ports` §5.4) | keep the 23 as the one **named exception "bootstrap scripts"** (like pro-d9), their fakes written by the test into a temp dir (no real `cargo`/`sccache`/`npm`); or cut `cargo.sh` to resolve-and-exec and test the rest as pure argv (a Rust `slot`-argv function: `cargo.sh:` "full run" rule). Unverified that the exception is allowed |
| 2 | `pi-check.test.ts` 11 | a real pty, SIGTERM of pi and its children (`pi-check.test.ts:157-236`), `python3` pty runner | the existing named exception pro-d9 (`0013…` "Stays"): no change; the 3 `rm` retry tests (:368-399) could be a pure function of the script's cleanup (guess) |
| 3 | `layout` 2 (`cargo metadata`, `cargo tree`) | cargo's resolver is the subject | move them to a Gate step (`cargo tree -e features` diff) or read `Cargo.lock` as text (U, guess: not equal in power) |
| 4 | `slot::a_killed_holder_frees_its_slot_even_though_its_command_lives_on` | the kernel releases a `flock` at process death; `FakeSpawn` cannot | **S1** of the tooling area: a real `cpd slot` and a real kill |
| 5 | `gate_run` wrapper 2 (`a_clean_run_of_true…`, `a_failing_command_keeps_its_status…`) | `sh` exports `GATE_RUN`, runs the command, `exec`s `cpd gate finish` | **S2**: one real run of the wrapper (merge the two) |
| 6 | `tripwire_script` 10 | the allow-list matching dies with `allow.txt` (ADR "What dies in mig-final") | delete the 10 with `mr-final`; **S3**: one test that the 1-line tripwire denies and logs |
| 7 | `store` 11 cross-process | two connections and a holder on one SQLite file: the busy wait is SQLite's own `busy_timeout` (`store/mod.rs:212`), real time inside C | threads in-process, `busy_ms` 20-50 ms, the hold released by a channel (no sleep); 3 of them (`a_second_writer_waits…`, `a_writer_that_cannot_get_the_lock_in_time…`, `a_persisting_store_waits…`) keep a short real wait: unverified that this is "no sleep" under the guard; else a `Store` busy handler port |
| 8 | `store::a_write_from_a_deleted_cwd_succeeds_and_records_unreadable` | a process whose cwd was removed | `cwd` as a `Ports`/`Fx` value (`std::env::current_dir()` at one place); the OS error is a fake `io::Error` |
| 9 | `events::follow_re_execs_when_its_binary_is_replaced…` | `exec` replaces the process | `Spawn::exec` fake records the argv; `restart_args` (`events/mod.rs:190`) is already a pure function |
| 10 | `hooks_idle` OS fact (nice 19 + ionice idle in the child, the caller keeps its nice) | a kernel scheduling class | the unit test asserts the argv `idle_prefix()`; the `nice` fact is `nice(1)`'s, not ours (not tested) |
| 11 | `install_script` python3: `a_missing_python3_is_refused_before_the_binary_is_replaced` and 3 `…without_renameat2` | the subject was python's `os.replace`, which `rs-install` removes (`ports` §5.1) | no python in Rust: the missing-python test has no subject (delete by "check the fence": the fence is the `renameat2` incident hs-d355, covered by the `fs::rename` test); the 3 others keep their behaviour under names without "python" |
