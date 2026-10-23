# tests-mock: inventory of `cli/tests/tooling` against the rule

Job `inv-tooling`, attempt 1. Static analysis only (rg, sed, `nextest list`); no test was run. Paths are in `cli/tests/tooling/` unless
prefixed; `common/` is `cli/tests/common/mod.rs`. A claim I did not read in the source is marked `unverified`.

The rule: tests test only code we wrote; no real tmux, pi, gh, curl, ssh, plannotator, network, sleep or wall-clock poll; git mocked
except a few adapter contract tests; no leak, flake or orphan.

Existing facts, checked:

- `TestEnv::empty()` (common/mod.rs:722) starts **no tmux**. It starts one real process per env, the *reaper* (`start_reaper`,
  common/mod.rs:249: `sh teardown.sh watch <pid>`, own process group), writes `teardown.sh`, and writes DENIED shims for gh, pi, curl,
  wget, ssh (common/mod.rs:305). So every TestEnv test runs a real `sh` reaper, even a pure one. A tmux server exists only when a test
  calls `env.tmux(..)` (common/mod.rs:1044, `tmux -L cpdt-<pid>-<n>`) or runs a `cpd` that starts one.
- `git_repo` (common/mod.rs:970) copies a template made once by real `git init/add/commit`, then runs a real `git update-index`.
- `poll` (common/mod.rs:409) is a `thread::sleep(10 ms)` loop; `wait_until` bounds it by `WAIT` = 30 s (common/mod.rs:388).
- `nextest list` gives **197** tests for `binary_id(cpd::tooling)`; `rg -c '#\[(test|tokio::test)'` gives 201 because it also counts
  `#[test]` inside string literals (test_changed_script.rs:41,42,45; tests_script.rs:27). The table uses the `rg -c` number (the
  acceptance rule) and shows the nextest number where it differs.

## 1. Summary table

"poll" = `common::poll`/`wait_until` (10 ms `thread::sleep` loop, up to 30 s). Classes: K T G C X R as in the Job.

| file | tests | real tmux (API) | real git (for what) | own processes / signals | sleep or poll | other external | class |
|---|---|---|---|---|---|---|---|
| cargo_script.rs | 12 | none | none | `sh` with shim cargo/sccache | none | scripts/cargo.sh; fake cargo, fake sccache on PATH (:16,:137) | K |
| dead_code.rs | 1 | none | none | none | none | reads source files | K |
| e2e_cross.rs | 1 | `TestEnv::tmux` (:214,:281,:349); engine, Lead, Workers run in real panes (`plan new` starts the server, :345) | `init --bare`, `push`, `commit`, `rev-parse`, `merge-base`, `branch`, `log`, `status` on temp repos (:275,:304-309,:425-541) | real `cpd watch` engine, drivers, fake-pi panes; `kill(engine, SIGTERM)` (:572) | `wait`/`wait_event` up to 90 s (:185,:192), `wait_whole_ticks` (:455,:560), `sleep 0.1` loop in the fake pi (:52) | fake gh, pi, node | T + G + C |
| e2e.rs | 1 | `TestEnv::tmux` (:211,:241,:299,:523) | same (:154-159,:389-488) | same; `kill(engine, SIGTERM)` (:524); `set-hook -gu pane-died` on the real server (:523) | `wait`/`wait_event` 30-90 s (:218,:404,:481); `sleep 0.1` (:46,:51) | fake gh, pi, node | T + G + C |
| gate_run.rs | 8 | **bare `tmux -L` in shell bodies**, on tmux's *default* dir `/tmp/tmux-<uid>` (:100,:181); `tmux kill-server` in `Cleanup` (:72) | none | a real detached tmux server as the "left process" (:100); `kill -KILL` (:66); a second `gate-run.sh` + `exec sleep 120` (:199) | `poll(WAIT)` for a pid to die (:157); `wait_some` (:204) | script calls `ps -axww -E`, `tmux`, `kill` (gate-run.sh:48,76,84,139) | T (3) + C (2), K (rest) |
| harness.rs | 36 (3 are gated helpers that return at once: :450,:621,:700) | `TestEnv::tmux` on a real server (:97,:441,:523,:588); real tmux behind a recording wrapper (:577) | `git_repo`, `git config --global` (:63,:883) | `sleep 100/600`, `detached()`, `KillOnDrop`, helpers = this test binary re-run (:450,:621,:700), SIGKILL of it | `poll` 5-10 s (:112,:436,:607,:659,:665); a 10 s negative window (:607); `thread::sleep(120 s)` in helpers (:463,:640) | `ps`, `kill`, `/proc` scan (:402-430) | T (4), X (see 3.1), K (lints, rest) |
| install_script.rs | 52 | real tmux behind `logging_tmux` (:373; 9 tests: :424,:446,:467,:486,:508,:524,:548,:1314,:1335); `running_old_engine` (:400) uses `TestEnv::tmux` | `Checkout::new` = real `clone` + `git_repo` (common/checkout.rs:22-40) in 42 tests; the script runs `fetch`, `cat-file`, `show`, `rev-parse` (install-cpd.sh:48-76); tests run `add`/`commit` (:87,:115) | real `flock --no-fork ... sleep 12/120` holders (:240,:206), `kill` (:217), a `sleep 120` child (:911) | `wait_until` lock held (:241,:411,:559); the product waits real time (`sleep 0.25` loop, install-cpd.sh:268): tests wait 12 s (:305), 2 s (:321), 1 s (:486); fake `sleep 2` (:292) | scripts/install-cpd.sh; real `python3` (:653), real `flock` (:820), fake cargo (:39) | T (9), G (42), C (3-4), K (rest) |
| interface.rs | 3 | none | none | runs built `cpd --help` | none | none | K |
| layout.rs | 4 | none | none | none | none | real `cargo metadata`, `cargo tree` (:181,:263) | K (+ cargo, 2.9) |
| no_repo_lock.rs | 2 | none (socket name in config only, :80) | real `worktree add`, `update-ref`, `push`, `commit`, `rev-parse`, `status` through cpd core, and a **slow git shim that runs the real git** (:250-310) | built `cpd attempt run`, `pr rebase`; `Child` killed on drop (:325) | shim `sleep 0.1` loop, barrier up to 10 s (:285); Gate hook waits up to 60 s (:392); `wait_until_for` 120 s (:332,:416) | none | G + C |
| npm_fresh.rs | 4 | none | none | `sh` with shim npm | none | scripts/npm-fresh.sh; fake npm (:23) | K |
| pi_check.rs | 4 | none: tmux is a PATH shim (:20,:62) | none | `sh pi-check.sh` with fake pi/tmux | none in the test; the script has `sleep 0.1`/`0.5` loops (pi-check.sh:101,:129,:182), not reached with the shim (unverified) | scripts/pi-check.sh; writes `/tmp/tmux-<uid>/` outside the env (:104-110) | K (named exception, 3.3) |
| preview_script.rs | 11 | none: tmux is a PATH shim (:43) | `git_repo` for the worktree (:37); the script runs `rev-parse` (preview-tui.sh:96) | `sh preview-tui.sh` | none | fake cargo (:64), fake installed cpd | K |
| prune_script.rs | 6 | none | `init --bare`, `worktree add/list/remove`, `branch`, `push`, `fetch`, `commit` (:29-146); script: `fetch`, `worktree list --porcelain`, `status`, `merge-base`, `branch -D` (prune-worktrees.sh:111-188) | none | none | fake gh (:219) | G |
| slot.rs | 14 | none | none | `cpd slot` children; `SIGKILL`/`SIGTERM` to them (:58,:128,:132,:150) | `wait_until` ready files (:82,:87,:106,:125,:148); `sleep 0.05` loops (:21 bound 60 s, :143 unbounded); `exec sleep 120` (:123) | scripts/cargo.sh and package.json `test` with fake cargo/node/cpd (:180,:268) | C (5), K (9) |
| tag_script.rs | 7 | none | `init --bare`, `clone`, `commit`, `tag`, `push`, `checkout`, `cat-file`, `rev-parse`, `remote set-url` (:37-178); script runs `git -C` (tag-version.sh:14) | none | none | none | G |
| test_changed_script.rs | 17 (nextest 14) | none | real `git_repo`, `add`, `commit`, `rev-parse` (:99-153); script runs `merge-base`, `diff --name-only`, `ls-files --others` (test-changed:41-48) | none | none | **real `cargo metadata`**, run by the script on a scratch workspace (:144 puts the real cargo first) | G (+ cargo) |
| tests_script.rs | 6 (nextest 5) | none | none | real `cargo build`, `cargo nextest`, `tsc`, `node --test` in a scratch workspace (:74-83) | none | **real cargo, cargo-nextest, rustc, tsc** (:14,:74); the repo's `node_modules` | cargo shim (2.9) |
| version.rs | 12 | none | `BuildScript` puts a git shim that execs the real git and asserts it is never called (:50); `Checkout` + `rev-parse` in one test (:131-141) | real `rustc` compiles `cli/build.rs` in 3 tests (:55) | none | real `rustc` (:54); `sync-version.sh`, `new-migration.sh` on a fixture (:186) | K (+ rustc), G (1) |

Row counts: 12+1+1+1+8+36+52+3+4+2+4+4+11+6+14+7+17+6+12 = 201 (`rg -c`), nextest 197.

Per script test: which external the script calls, and can PATH shims mock it?

| script | externals | already shimmed by its tests | mockable by PATH shim |
|---|---|---|---|
| cargo.sh | cargo, sccache | both | yes |
| npm-fresh.sh | npm, node | npm | yes |
| install-cpd.sh | git, cargo, python3, flock, tmux, `cpd` | cargo, cpd, tmux (fake in ~11 tests, real behind a wrapper in 9), python3 (real, wrapped) | yes for tmux, python3, flock; git needs a git shim (2.2) |
| preview-tui.sh | tmux, git (`rev-parse`), cargo, `cpd` | all but git | yes |
| prune-worktrees.sh | git, gh, jq, `cpd` | gh, cpd | git: many porcelain calls, shim is costly (2.6) |
| tag-version.sh | git | none | git only (2.6) |
| test-changed | git, node, cargo (`metadata`) | none | yes, with canned output |
| tests.sh | cargo, nextest, node, tsc | none | cargo, nextest, tsc yes; node is allowed |
| gate-run.sh | ps, kill, tmux, node | none | yes; its leak scan is the code under test (2.5) |
| pi-check.sh | tmux, pi, node, npm | pi, tmux | named Gate exception (3.3) |
| `cpd slot` (slot.rs) | flock through our own code | n/a | n/a |

## 2. Clusters

Size: one Job = one Worker, ~150 turns, at most ~40 tests rewritten. Seams: `core/src/host/tmux.rs` is the tmux adapter; the git adapter
is in core. The design of the fake tmux and git is the design Job's.

### 2.1 T-install: install script against real tmux behind a recording wrapper (9 tests)

- List: `rg -n 'logging_tmux\(' install_script.rs`. First `a_failing_watch_check_leaves_the_old_engine_and_the_views_alone` (:424), last
  `a_mark_restart_that_fails_is_printed_and_the_retirement_goes_on` (:1335). The wrapper (:373) `exec`s `common::real_tmux()`.
- tmux subcommands: `new-session -d -s -n` and `list-windows -t =work -F` (test side, :400-415), `kill-window` (script, asserted :439,:478,:536),
  `respawn-pane`, `list-panes`, `has-session` (script side; exact list `unverified`).
  `running_old_engine` (:400) starts two sessions with panes `sleep 120` and `flock --no-fork lock sleep 120` (:404-405).
- `the_install_restarts_the_engine_of_a_real_cockpit_for_a_caller_without_a_utf8_locale` (:548) runs the real `cpd tui` (real cockpit + engine)
  three times, once per locale, and asserts every tmux call is `tmux -u` (T + R): the biggest.
- Fix: PATH tmux shim that logs and answers canned output (:248 `fake_tmux` already does this in ~11 other install tests). "The old engine holds
  the lock" stays a real `flock ... sleep` of ours (allowed if reaped); the shim's `kill-window` kills the recorded pid. Seam: PATH shim.
  Size: **1 Job**, 9 tests (with 2.3).

### 2.2 G-checkout: real `git clone`/`fetch` behind the install script (42 tests)

- Pattern: `rg -c 'Checkout::new' install_script.rs version.rs` = 42 + 1. `common/checkout.rs:22-40` makes an origin with `git_repo`, `add`, `commit`,
  `clone`. The script runs `git fetch origin main`, `cat-file -e`, `show origin/main:<file>`, `rev-parse --short HEAD` (install-cpd.sh:48,56,57,76);
  three tests also run `git add/commit/remote set-url` (:87-88,:115-116,:140).
- Fix: a PATH git shim answering those four subcommands from a fixture tree, plus one real-git contract test of the same four calls.
  Seam: new PATH git shim. Size: **1-2 Jobs**: the ~20 migration-refusal tests (:67-137 and neighbours) need the shim; the other ~22 only need `Checkout`
  to stop cloning (they never look at git).

### 2.3 C-install: real waits for the old engine's lock (3 + 1 tests)

- `an_old_engine_that_holds_its_lock_for_longer_than_ten_seconds_is_waited_for` (:305, **12 s** real), `an_old_engine_that_never_frees_its_lock_...` (:321,
  **2 s**), `a_lock_the_old_engine_keeps_past_the_wait_...` (:486, **1 s**), and `the_follow_ups_run_after_a_slow_config` (:287, a fake `sleep 2`).
  The product waits with `sleep 0.25` bounded by `CPD_INSTALL_LOCK_WAIT_SECS` (default 60; install-cpd.sh:200,:268). Three `time-claim:` comments (:309,:325,:497)
  say there is no per-poll signal: under the rule they are exemptions to remove.
- Fix: a PATH `sleep` shim that counts calls, and a `flock` shim reporting "held" for N calls: zero real time. Seam: PATH shims. Size: inside 2.1's Job (same file).

### 2.4 T-e2e: the two whole-Plan flows (2 tests)

- `e2e.rs:282 a_whole_plan_runs_from_context_new_to_pr_poll` and `e2e_cross.rs:338 a_plan_across_two_repos_runs_a1_then_b_then_a2_and_a_review_job`.
  `plan new` starts a real tmux server and the Lead's pane; the engine runs in `cpd-work:watch`, Workers in windows. The tests read `list-panes -a`, `show-environment -g`,
  unset the `pane-died` hook on the real server (e2e.rs:523), SIGTERM the engine (:524).
- Waits: the engine's real tick 0.1 s (`common::TICK_SECONDS`), A9 kill pauses and driver polls of 50 ms (`common::PLAN_POLLS`); test limits 30-90 s.
- Fix: with a fake tmux that tracks sessions/windows/panes/hooks and runs a pane's command as a child process, the flows run unchanged through
  `core/src/host/tmux.rs`. Real git stays on temp repos (2.6) or goes to a fake. Seam: the tmux adapter + a fake server. Size: **1 Job each**, after the fake exists.

### 2.5 T-gate: `gate_run.rs` uses tmux as the "leaked process" (3 tests)

- `rg -n 'tmux -L' gate_run.rs` = :100 (used by `the_hint_for_a_left_process_names_a_command_this_os_has` :116 and `a_process_left_with_the_run_id_fails_the_run_and_is_stopped` :148)
  and :181 (`a_socket_named_with_the_run_id_fails_the_run` :176). Subcommands: `new-session -d -P -F '#{pid}'`, `kill-server`.
- The first two use tmux only because on macOS `ps -E` hides the environment of Apple platform binaries (:93-95): any non-platform binary that **our** shim starts (a copy of
  `node`) does the same. The socket test makes a real socket file under the real `/tmp/tmux-<uid>` (:81), because the script hardcodes that dir (gate-run.sh:36); without
  touching the script, create the file by shell as pi_check.rs:104 does (still writes `/tmp/tmux-<uid>`; `Cleanup` removes it).
- The other 5 tests are K. Size: **1 Job** (8 tests).

### 2.6 G-fixture: real git in the script tests (prune 6, tag 7, test-changed 14, version 1)

- `rg -n 'git\(' prune_script.rs tag_script.rs test_changed_script.rs`. prune: `prunes_exactly_the_worktrees_whose_work_is_on_main` (:111) .. `a_failing_cpd_exits_2_and_removes_nothing` (:341);
  the script decides by `merge-base --is-ancestor` and parses `worktree list --porcelain`: a fake git is most expensive here. tag: `tags_the_commit_that_last_changed_the_version` (:93) ..
  `a_local_tag_whose_push_failed_is_pushed_by_the_next_run` (:176): `tag -a`, `push` to a local bare repo, needs a model of refs. test-changed: `a_core_change_runs_core_and_its_reverse_dependencies` (:245) ..
  `a_change_outside_every_crate_runs_nothing` (:378): the script reads `merge-base`, `diff --name-only`, `ls-files --others`: a shim returning a fixture list makes them pure.
- Fix: test-changed = git shim (3 subcommands) + cargo metadata fixture (2.9): **1 Job**. prune + tag (13 tests): keep real git as **git contract tests** on temp repos (the rule allows a few);
  a zero-real-git outcome needs a refs/worktree model: **+1-2 Jobs** (design decides).

### 2.7 G-lock: `no_repo_lock.rs` (2 tests)

- `two_plans_merge_into_their_prs_of_one_repo_at_once_...` (:353) and `a_merge_during_pr_rebases_gate_lands_...` (:399). The first installs a **slow git shim that runs the real git**
  except the merge's `update-ref`, blocking up to 10 s until two merges arrive (:250-310); the second pauses a `pre-merge` hook in a `sleep 0.1` loop (:389-395, up to 60 s) while a merge lands.
  Both use real `worktree add`, `update-ref`, `push`, `rev-parse` through cpd core (`create_attempt_worktree`, :18).
- They test our concurrency, not git. Fix: a fake git with refs and a barrier that is a file/FIFO rendezvous, not `sleep`. Seam: the core git adapter. Size: **1 Job** (2 big tests).

### 2.8 C-poll: real polling of own processes (slot 5, harness ~7, gate_run 2, no_repo_lock 2)

- `rg -n 'wait_until|wait_some|poll\(' slot.rs harness.rs gate_run.rs no_repo_lock.rs`. All wait on **our** shims (ready file, pid, lock): allowed processes, but each is a 10 ms `thread::sleep`
  loop with a 5-30 s budget (the flake risk under load 40-120). Absence windows always spend their whole time: harness.rs:607 (10 s).
- Fix: replace a ready-file poll with a blocking read of a FIFO/pipe the shim writes (an event, not a poll). Seam: a `common::` helper. Size: **1 Job**, ~15 tests; takes harness.rs, so merge with 2.10.

### 2.9 X-cargo: real `cargo`/`rustc`/`nextest`/`tsc` (layout 3, test_changed 14, tests_script 6, version 3)

- `layout.rs:181 cargo metadata --no-deps` and `:263 cargo tree` (`the_binaries_are_those_of_the_area_table` :144, `the_four_crates_...` :204, `every_member_resolves_the_features_...` :294);
  tests_script.rs runs `cargo build`, `nextest run/list`, `tsc --noEmit`, `node --test` in a scratch workspace (`rg -n 'Scratch::new' tests_script.rs`);
  version.rs:55 compiles `cli/build.rs` with the real `rustc`; test_changed lets the script call real `cargo metadata`.
- cargo is an external we do not control. Whether `cargo tree` needs the network (stale registry) is `unverified`; nested builds in tests_script are the heaviest tests of the binary (timing `unverified`, not run).
- Fix: a PATH cargo shim with canned `metadata`/`tree`/`nextest` output; `tsc` and `node` stay (node is the allowed runner). version.rs: test `build.rs`'s logic as a unit test instead of compiling it.
  layout: the source-file checks are K; its cargo calls need the shim or fixture JSON. Size: **1 Job** (~25 tests).

### 2.10 X-harness: the harness's own teardown tests (harness.rs, 8 tests)

See 3.1. Size: **1 Job** (delete or rewrite on the fake tmux), together with 2.8.

### 2.11 K: nothing to mock

cargo_script 12, npm_fresh 4, interface 3, dead_code 1, preview 11, pi_check 4, harness lints (:168,:209,:227,:253,:269,:287 and the rest), install lints (`check-portable.sh`, :1127-1297, 10 tests),
and 13 install pi-swap tests (:739-992) that use a real `python3` behind a wrapper (an interpreter is not in the Job's external list: ask the Lead whether it counts; `unverified`).
No Job needed, apart from dropping the per-env reaper where a test has no process of its own.

Estimate: 2.1 (1, with 2.3), 2.2 (1-2), 2.4 (2), 2.5 (1), 2.6 (1-3), 2.7 (1), 2.8+2.10 (1-2), 2.9 (1) = **9-13 Workers** (`unverified`: depends on the fake tmux/git design).

## 3. Tests that test an external (X)

### 3.1 Harness teardown tests: they test tmux, `ps`, `kill`, the OS (harness.rs)

| test | line | why |
|---|---|---|
| `tmux_server_is_private_and_killed_on_drop` | 94 | real tmux makes a socket and dies with its owner |
| `a_killed_test_leaves_no_tmux_server_process_dir_or_reaper` | 468 | SIGKILLs a copy of this test binary; real tmux and `sleep` must die: OS + tmux behaviour |
| `a_dropped_env_leaves_no_tmux_server_process_dir_or_reaper` | 507 | same, through Drop |
| `a_server_the_test_kills_is_not_restarted_by_its_supervising_hooks` | 588 | real tmux must not run `pane-died` after `kill-server` (hygiene.md 3a); a 10 s negative window |
| `a_killed_test_leaves_no_git_wrapper_process` | 645 | process death after SIGKILL; the wrapper is our script, the assertion is the OS's |
| `a_process_that_leaves_the_group_and_is_not_recorded_outlives_the_teardown` | 722 | documents an OS limit (`setsid sleep`) |
| `a_recorded_pid_that_is_not_the_envs_is_spared` | 733 | pid recycling, an OS behaviour |
| `teardown_is_idempotent_and_spares_the_test_process` | 751 | ours: K |

Proposal: with a fake tmux, delete `tmux_server_is_private...` and `a_server_the_test_kills...` and the tmux half of 468/507 (what they guard disappears with real tmux). The reaper tests
(468/507/645/722/733) guard `teardown.sh`, our script: keep them as K on a plain process, not tmux. The helpers `killed_test_helper`, `wrapper_helper`, `envs_helper` (:450,:621,:700) are not tests.

### 3.2 Other X

- `harness.rs:59 git_repo_has_one_commit_on_main` and `:883 the_env_gitconfig_turns_automatic_maintenance_off` test real git on a template/config we wrote: one **git contract test** on a fixture, delete the duplicates.
- `layout.rs:294 every_member_resolves_the_features_of_the_workspace_build`: the resolver is cargo's; our Cargo.toml is the input. Keep as a script-level check, not a unit test (decision `unverified`).
- `tests_script.rs` (6): `tests.sh` maps names to commands; the tests prove it through real cargo and nextest (cargo's job): mock cargo (2.9).
- `gate_run.rs:116 the_hint_for_a_left_process_names_a_command_this_os_has`: tests which of `/proc` and `ps -E` the OS has (:117-125): keep only the hint text, with a fake `ps`.
- `install_script.rs:739-992`, the pi swap "on a python without renameat2": python3/ctypes behaviour through our wrapper; real python3 is an external, our shell swap is the code under test.

### 3.3 `pi_check.rs`, a named Gate exception

`pi_check.rs` uses fakes (pi, tmux), not real pi. What `scripts/pi-check.sh` needs when run for real (the Gate exception; I did not run it): a real `pi` binary (arg 1), real `tmux` (21 call sites),
`node`, `npm` (a test run and `npm run`), a private tmux server whose socket lives under `$TMPDIR` or `/tmp` (the 100-character socket-path rule, tested at pi_check.rs:80,:89,:96), `sleep` in its
teardown loops (pi-check.sh:101,:129,:182) and the fake `cpd` it writes (:147-156, `exec sleep 60`). The four tests touch only its tmux-dir arithmetic and write under `/tmp/tmux-<uid>` (:104-110)
outside the env, with a drop guard. Class K; no change.

## 4. Leak and flake risks

1. **Real tmux outside the test's directory.** gate_run.rs:100,:181 start servers on `/tmp/tmux-<uid>` (the command unsets `TMUX_TMPDIR`). `Cleanup` (:56-80) kills by socket name; after a SIGKILL of the run both server and
   socket stay (unverified; the Gate's leak check names such sockets, gate-run.sh:107). pi_check.rs:104-110 writes socket files to `/tmp/tmux-<uid>` too (a drop guard, not run on SIGKILL).
2. **Wall-clock waits under load 40-120.** harness.rs:297 has its own `WAIT` of **10 s** (the common one is 30 s): the killed/dropped-env tests (:468,:507,:588) poll for process death at that limit, and real tmux teardown slows under load.
   docs/research/macos-gate-tooling.md records macOS failures for this area (not re-read for this file: `unverified`).
3. **Absence windows that always spend their time:** harness.rs:607 (10 s, every run), install :486 (1 s), :321 (2 s), :305 (12 s). `wait_whole_ticks` (e2e_cross.rs:455,:560) counts engine polls, so it is load-safe (`common/mod.rs:1147`).
4. **Process leaks when a test panics before its guard.** install_script.rs:899-926 spawns `sleep 120` (:911), kills and waits it at :925-926, but with no `Drop` guard: a panic in `mac_install` before that leaves it up to 120 s (reaped by the env's process group
   at teardown, `unverified`). :905-909 spawns and waits `true`, fine. no_repo_lock.rs:325 (`Child` Drop) and slot.rs (kill) are guarded.
   Every TestEnv starts a real reaper `sh` (common/mod.rs:249): ~197 shells and a `teardown.sh` run each (:774). That is the orphan net the rule wants, at a cost per test.
5. **Shell loops whose only exit is a condition:** e2e.rs:46,:51 and e2e_cross.rs:52 (`until ...; do sleep 0.1; done`, **no bound**: a failing test leaves a loop until the env teardown kills the group), slot.rs:143 (`while :`, no bound); bounded: slot.rs:21 (60 s), no_repo_lock.rs:285,:392 (60 s).
6. **e2e depends on a hook race:** e2e.rs:523 unsets `pane-died` on the real server before SIGTERM of the engine (supervised engines restart, hygiene.md 3a); the harness needs a test for it (:588).
7. **Helpers that re-run the test binary** (`killed_test_helper` :450, `wrapper_helper` :621, `envs_helper` :700): if the parent dies between spawn and READY, the child sleeps `SLEEP_SECONDS` = 120 s (:463,:640): bounded, but an orphan for 2 minutes.
8. **Scripts that wait on real time with no injection point:** install-cpd.sh:268 (`sleep 0.25`), pi-check.sh:101,:129,:182. A PATH `sleep` shim covers scripts; the product `cpd` has `CPD_TEST_*` overrides (common/mod.rs:84-99).
9. **Load dependence from real cargo/rustc:** tests_script (nested `cargo build` and `nextest`), test_changed (`cargo metadata` per test, 14 tests), version.rs (3 `rustc` compiles of `build.rs`), layout (`cargo tree`, `metadata`). Timing `unverified`.
10. **Existing lints in harness.rs** `no_test_sleeps_for_a_result_or_bounds_the_time_a_result_takes` (:227) and `every_wait_polls_through_the_harness` (:287) already forbid sleeps >= 250 ms without a `// time-claim:`; install_script.rs:309,:325,:497 (waits)
    and :854,:921 (clock set, not waited: fine) use it. The three waiting claims are rule violations to fix (2.3). The lints are K; keep them and, once 2.3 lands, forbid `time-claim` for waits.
