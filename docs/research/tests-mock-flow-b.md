# Inventory: second half of cli/tests/flow against the tests rule

Static analysis only (rg, sed; one `nextest list`). No test was run. Rule: owner policy `[tests]` (`cpd policy show`), Context records pro-d6, pro-d9, pro-d10.
Paths are under `cli/tests/flow/` unless they start with `core/` or `cli/`. Claims I did not read in the source are marked `unverified`.

Counts: 17 files, 187 tests (each file's count is `rg -c '#\[(test|tokio::test)'`). Totals by class (a test has one class, its heaviest):
K 142, T 21, T+G(+C) 22, G 2. No R and no X test in scope as a whole (section 3 lists the partial ones).

## 0. How a test in this half reaches an external

- `TestEnv::new()` (`cli/tests/common/mod.rs:714`) starts no tmux. It makes a temp dir, a `bin/` of deny shims for `gh pi curl wget ssh` (`common/mod.rs:305`, `:749`), and one `sh teardown.sh watch <pid>` reaper process per env (`start_reaper` `:249`, started in `empty()` `:722`). The reaper and `teardown()` (`:774`) are our own shim and are reaped by `Drop` (`:1157`).
- A real tmux server starts only when (a) the test calls `env.tmux(..)` (`common/mod.rs:1044`, a real `tmux -L cpdt-*`), or (b) the built `cpd` calls its tmux adapter (`core/src/host/tmux.rs`: `Tmux::has_session/create_session/new_window/...`). Path (b) is taken by `plan new` (`core/src/plan/lead.rs:68-74`), `context manager` (`core/src/context/manager.rs:92-108`), `plan cancel` (`core/src/plan/cancel.rs:41`), `plan/context archive` (`core/src/plan/archive.rs:212`), `session fork/open/park/close` (unverified: not read).
- Panes run `sleep 120` (`SLEEP_SECONDS`, `common/mod.rs:75`): in cockpit helpers, in `fake_cpd_views` (`:1085`), in `fake_cpd_panes` (`:1092`) and in the fake pi of the manager/by-user files. Teardown kills them.
- A test that only reads store/files after `cpd ...` needs no tmux and no git: it is K, even though `TestEnv` exists.
- Real git runs in two ways: the test calls `env.git/git_repo/keyed_repo/local_origin` (`common/mod.rs:964-1022`), or the built `cpd` runs git (`pr add`: `core/src/pr/add.rs:532,684,733,749,758`; `git/worktree.rs:178-678`; `plan/delete.rs:255-268`; `context/distill.rs:435` `git grep`).
- Waits in tests: `wait_until/poll` = 10 ms `thread::sleep` loop up to `WAIT` 30 s (`common/mod.rs:388-419`); `wait_screen` is not used in this scope (rg).

## 1. Summary table

Legend, tmux column: `TestEnv::tmux` = test calls real tmux directly; `cpd->tmux` = the built cpd starts/uses the server; `-` none. Class: K, T, G, C as in the brief.

| file | tests | real tmux (API) | real git (what for) | own processes/signals | sleep or poll | other external | class |
|---|---|---|---|---|---|---|---|
| context.rs | 40 | 5 (`:899,:938,:971,:986,:1009`) `TestEnv::tmux` in `hs_tmux/cockpit/pane` (`:817-851`), `cpd->tmux` kill-window; the other 35 none | none: `:203` puts a *fake* `git` that fails, K | `sleep 120` panes (`:831,:840`), reaper | none | none | 35 K, 5 T |
| context_distill.rs | 33 | none (config names a socket, `:32-34`, no tmux call) | 2 (`:408,:712`): `git_repo`+`git add/commit` (`:395-403`), cpd `git grep` (`distill.rs:435`) | cpd spawns the fake pi and polls it every 50 ms (`distill.rs:46,:886`) | no test wait; `wait_until` is imported (`:10`) but unused (rg: one hit, the import); product poll 50 ms, limit 15 min (`distill.rs:500`, `config/file.rs:332`) | fake pi, fake node, fake gh, fake cpd (`:38-52`); `flock` on own temp file (`:211`, allowed) | 31 K (C-low), 2 G |
| context_curation.rs | 14 | none | none | none | none | none | K |
| context_digest.rs | 11 | 5 `cpd->tmux`: `plan new` in `new_plan_with_brief` (`:61`; tests `:68,:171,:214,:314,:328`), the last also `context manager` (`:328`) | none | `sleep 120` Lead pane via `fake_cpd_panes` (`:20`); `manager_cockpit` fake pi `sleep` (`:243-252`) | none | fake cpd/pi | 6 K, 5 T |
| context_manager.rs | 7 | `TestEnv::tmux` in `cockpit/panes/slot/window_names` (`:40-90`), `set-environment` (`:188`); `cpd->tmux` | none | `sleep 120` panes, fake pi `exec sleep` (`:24`) | `wait_until` (`:101`, env files of the fake pi), 30 s | fake pi, fake cpd views | T |
| context_manager_by_user.rs | 2 | `TestEnv::tmux` `cockpit` (`:23-33`); `cpd->tmux` | none | `sleep 120` panes | `wait_until` (`:44`) for the pi call | fake pi, fake cpd | T |
| context_delete.rs | 2 | `cpd->tmux`: `world()` runs `plan new` (`plan_delete.rs:69`), `plan archive` | real: `keyed_repo`, bare init, `push`, `worktree add` via `world` (`plan_delete.rs:73-94`); cpd `worktree remove/prune`, `branch -D` | `sleep 120` Lead pane (`fake_cpd_panes`, `plan_delete.rs:39`) | none | none | T+G |
| context_pending.rs | 6 | none | none (`git` hits are `github.com` URLs) | none | none | none | K |
| context_pending_waiting.rs | 2 | none | none | none | none | none | K |
| plan_outline.rs | 20 | none | none | none | none | none (`plannotator` is text, `:131,:482`) | K |
| plan_cancel.rs | 13 | all 13 via `setup` (`:81-97`): `plan new` + `session fork` (`cpd->tmux`); 2 also `TestEnv::tmux` (`cockpit :36`, `run_lead :366`, `capture :375`) | all 13: 3x `keyed_repo` (`:90`) + `pr add` runs git (`pr/add.rs`) | `sleep 120` panes, `spawn_cancel` child (`:178`, reaped by `finish :184`) | `wait_for_pane` x2 per test (`:66,:94,:96`); `wait_until` (`:196,:340,:393,:399,:471`); product `plan cancel` sleeps 250 ms per round up to 60 s (`cancel.rs:30,:33,:125`) | fake gh (`:85`), fake cpd, fake pi (`:394`) | T+G+C |
| plan_overlap.rs | 12 | none | none | none | none | none | K |
| plan_show.rs | 10 | none | none (`/r/pay` are strings in rows) | none | none | none | K |
| plan_delete.rs | 7 | `cpd->tmux`: every test calls `world` (`plan new`, `:69`) | all 7: `world` (`:73-94`) real bare repo, push, `worktree add`; `local_branches/remote_branches` run `git branch` (`:48-58`) | `sleep 120` Lead pane | none (`asleep` hits are a column name) | none | T+G |
| plan_render_cancelled.rs | 4 | none | none | none | none | none | K |
| plan_render_waiting.rs | 2 | none | none | none | none | none | K |
| plan_new_by_user.rs | 2 | `TestEnv::tmux` `cockpit` (`:23-33`); `cpd->tmux` `plan new` | none | `sleep 120` panes | `wait_until` (`:44`) for the pi call | fake pi, fake cpd | T |

Test counts re-checked with `rg -c '#\[(test|tokio::test)'`: 40, 33, 14, 11, 7, 2, 2, 6, 2, 20, 13, 12, 10, 7, 4, 2, 2 = 187.
`nextest list -E 'binary_id(cpd::flow)'` ran and lists the `cpd::flow` binary (the whole flow binary, not only this half: unverified per-file counts from it).

## 2. Clusters

Size = Worker Jobs (one Job = one Worker, about 150 turns, at most about 40 rewritten tests). Seams are proposals; the design Job decides.

### A. Cockpit and pane tests of `context archive` (context.rs) — 5 tests, T
- Tests: `archive_covers_plans_leads_forks_and_the_manager_with_one_event_each` (`:899`) ... `archive_and_unarchive_of_an_unknown_context_are_refused` (`:1009`); list with `rg -n 'hs_tmux\(\)' context.rs`.
- Real tmux subcommands the test itself runs: `new-session -d`, `set -p @cpd_view`, `new-window -d`, `set -p @cpd_session`, `list-panes -a` (`context.rs:831-851`). cpd then closes panes (`Tmux::list_panes`, `kill_window`/`kill_pane`, `host/tmux.rs:795,:856,:869`).
- Ignore-the-session tests: `archive_and_unarchive_of_an_unknown_context_are_refused` (`:1009`) uses `hs_tmux` only for the config and never starts a pane (refused before any tmux call: `unverified`, the refusal is `context_not_found` at `:1011`); it is K once the socket config is dropped.
- `archive_refused_by_any_one_blocker_changes_nothing` (`:938`) and `...chat_slot...` (`:971`) assert that no pane closed.
- Fix: the Tmux adapter `core/src/host/tmux.rs` behind a fake `tmux` program (`Tmux::with_program`, `:483`) holding panes in a table; the assertions (`live_panes`) read the table.
- Size: 0.2 Job (shares the fake with B).

### B. Manager and Lead start (context_manager 7, by_user 2, plan_new_by_user 2, context_digest 5) — 16 tests, T
- Listing: `rg -n 'cockpit\(|new_plan_with_brief\(' context_manager*.rs plan_new_by_user.rs context_digest.rs`.
- Real tmux: `has-session`, `new-session`, `new-window`, `split-window`, `set -p` (pane options), `list-panes -a`, `list-windows`, `rename-window`, `swap-pane` (session open, `context_manager.rs:313-338`, unverified for the product), `set-environment -g` (`context_manager.rs:188`).
- Tests that need tmux only to start a session the test ignores: the 5 `context_digest` tests (they read `run/<plan>/lead-message.md` or `manager-message.md`, `context_digest.rs:56-59,:336`; `fx` `:18-24` makes the fake panes only so that `plan new` succeeds); `plan_new_by_user` x2 and `context_manager_by_user` x2 read only the argv of the fake pi (`:42-47`) which cpd put in the pane command — a fake tmux that records the spawn command is enough (unverified: the argv reaches pi through the pane's `cpd session run`; the test reads the fake pi calls, so the pane must really run `session run`; a fake tmux would have to record the `Spawn` instead).
- Waits: `wait_until` on a fake pi that a real pane starts (`context_manager.rs:101`, `by_user:44`, `plan_new_by_user:44`), up to 30 s, 10 ms polls: load dependent. With a fake tmux these polls go away.
- Tests that assert tmux layout (`slot`, `panes`, `window_names`): `the_first_call_registers_the_manager_and_starts_pi_in_its_window` (`:111`), `a_second_call_changes_nothing` (`:226`), `a_stopped_manager_without_a_pane_is_started_again_on_its_file` (`:248`), `refusals_register_nothing` (`:290`), `session_open_brings_the_manager_into_the_chat_slot` (`:313`): need a fake tmux that models panes, windows, the `chat` slot and swap-pane (the heaviest piece).
- `a_managers_pi_gets_no_plannotator_variable_from_its_callers` (`:182`) tests the environment cpd gives a pane when the tmux server's global environment holds `PLANNOTATOR_*`: with a fake tmux it becomes an assertion on the spawn's env/`-e` arguments (cpd code). The tmux-inherits-global-env part is tmux behaviour (section 3).
- Fix and seam: fake tmux behind `Tmux`/`with_program` (existing adapter); plus asserting the `Spawn` that `new_window` receives.
- Size: 0.5 Job (16 tests) once the fake tmux exists.

### C. `plan cancel` fixtures (plan_cancel.rs) — 13 tests, T+G+C
- Listing: `rg -n 'setup\(' plan_cancel.rs` (13 calls: `:218` ... `:544`; first `cancels_jobs_closes_open_prs_and_the_lead_and_archives_the_plan` `:216`, last `a_gh_failure_for_a_pull_request_already_closed_is_not_a_failure` `:542`).
- Every test pays `setup` (`:81-97`): config, 3 `keyed_repo` + 3 `pr add` (cpd runs `rev-parse --verify`, `rev-parse --git-common-dir`, `symbolic-ref refs/remotes/origin/HEAD`, `branch -m`: `pr/add.rs:532,684,733,749,758`), `plan new` (tmux `has-session`, `create-session`, `new-window`, `set-option` pane), `session fork` (tmux `new-window`), 2x `wait_for_pane`, 3x `job cancel`.
- Tmux needed only to start a session the test ignores: all but 5 tests: `cancels_jobs...` (`:216`, `has_pane`), `a_rerun_finishes...` (`:290`, `has_pane :300,:308`), `the_cancelled_plans_port...` (`:380`), `the_lead_in_the_chat_slot_blocks...` (`:528`) and the pane checks in `:216`. The other 8 need a Lead session row and a pane id only for the cancel's tmux closing step.
- Git needed only to have a repo: all 13 (the repo only has to answer `pr add`; no test reads repo content: `rg -n 'git' plan_cancel.rs` shows only `keyed_repo`).
- Waits (C): product `wait_until_ended` sleeps `POLL` 250 ms between looks, limit 60 s (`core/src/plan/cancel.rs:30,:33,:125`). It already takes `limit` and `every` (`:109`) and its unit test uses 30 ms/5 ms (`:218`), but the binary uses the constants. Tests: `cancel_with_engine` (`:194`, used `:224,:270,:278,:295,:396,:452,:502,:547`) and `spawn_cancel` (`:339,:470`) wait for the stop request with `wait_until` (10 ms poll) and then write the engine's rows. Each costs at least one real 250 ms sleep in cpd plus the test poll. Fix: an environment/config seam for `every` (config key or `CPD_TEST_*` variable, the repo has `PR_POLL_EVERY_TICK` as precedent, `common/mod.rs:84`) or an injectable clock/sleeper in `plan::cancel`.
- `the_cancelled_plans_port_is_free_for_another_plan` (`:380`): real pane, real `cpd session run`, `capture-pane`, `send-keys Enter`, `wait_until` on screen text (`:393,:399`), and per `macos-gate-rest.md:54` the pane ran the real `pi` on macOS. It tests our port allocator through a terminal prompt: R/T; becomes a unit test of the port choice and of the retry prompt with an input stream.
- Fix: gh already fake (K). git: fake git for `pr add` (G). tmux: fake (T). Waits: seam above (C).
- Size: 1 Job (13 tests, all through one `setup`: rewriting `setup` rewrites them).

### D. `plan delete` and `context delete` worlds (plan_delete 7, context_delete 2) — 9 tests, T+G
- Listing: `rg -n 'world\(' plan_delete.rs context_delete.rs`; first `without_yes_lists_what_goes_and_deletes_nothing` (`plan_delete.rs:158`), last `refuses_when_a_plan_has_live_work` (`context_delete.rs:58`).
- Real git, subcommands by the test: `init --bare`, `push origin main`, `worktree add -b`, `push origin <branch>`, `branch --format` (`plan_delete.rs:76-94,:49`), `update-index --refresh` (`common/mod.rs:978`), `config url.<bare>.insteadOf`. By cpd: `worktree remove --force --force`, `worktree prune`, `branch -D` (`core/src/plan/delete.rs:255-268`).
- Git needed to have a repo/worktree: the delete product removes worktrees and branches, so the repo is the thing under test. The asserts `local_branches`/`remote_branches` (`:232,:241,:274,:280`) and `!w.lead_worktree.exists()` check git's effect.
- tmux needed only to start a Lead session the test ignores: all 9 (`world_ending` `:69` calls `plan new` only to get a Lead row; `plan archive` `:136` closes its pane).
- Fix: replace `plan new` in `world` with a SQL fixture of the Lead (as `context_delete.rs:11-17` does for the Manager); put git behind the existing git module (`core/src/git/worktree.rs`) with a fake git for "delete calls `worktree remove`/`branch -D` in this order" and keep 1-2 contract tests on real git fixtures for "remove really removes the branch" (git contract test).
- Size: 0.4 Job (9 tests).

### E. `git grep` of the distiller (context_distill) — 2 tests, G
- `docs_md_lists_the_grep_hits_of_the_contexts_repos` (`:408`) and `a_documented_target_that_is_not_a_grep_hit_rejects_only_that_line` (`:712`): `repo_with` (`:395`) runs `git_repo` (init, add, commit via template) and `git add -A`, `git commit` (`:402-403`); cpd runs `git -C <repo> grep -l -w <id> -- docs '*.md'` (`distill.rs:435`).
- The grep is git behaviour; our code is the filtering of hits and the `docs.md` file. Fix: fake git printing hits (seam `core/src/git`, or a `git` shim already supported by `TestEnv::fake`), plus one contract test of the exact `git grep` argv on a fixture.
- Size: 0.1 Job.

### F. Distiller run loop (context_distill, remaining 31) — K with C-low
- Real: fake pi child, cpd polls `try_wait` every 50 ms (`distill.rs:46,:886`): each run adds up to 50 ms of real sleep inside the product, no sleep in the tests, no limit hit (limit 15 min, no test reaches it: `rg -n 'timeout' context_distill.rs` empty). Not flaky; no change required. Optional: `distill.rs` `POLL` injected (C), 0.1 Job; not proposed.
- `a_second_run_of_a_context_is_refused_while_the_first_is_alive` (`:211`) holds a real `flock` on a file in the test's temp dir: allowed.
- `pi_that_cannot_start_is_distill_failed_spawn` (`:182`) runs cpd with `PATH=/nonexistent`: allowed (no process left).

### G. Pure `cpd` + SQLite tests (K) — 142 tests
- context.rs 35, context_distill 31, context_curation 14, context_digest 6, context_pending 6, context_pending_waiting 2, plan_outline 20, plan_overlap 12, plan_show 10, plan_render_cancelled 4, plan_render_waiting 2.
- They run the built `cpd` and read the store/files. Cost is process spawns (`env.cpd` per command); no external. `context.rs:203 no_record_write_runs_git_or_makes_a_directory` uses a failing fake `git` shim to prove cpd runs no git: K.
- No change. (A later speed Job could move them to in-process calls: out of scope.)

### Job total
A 0.2 + B 0.5 + C 1 + D 0.4 + E 0.1 = about 2.2 Jobs; tests touched: A 5, B 16, C 13, D 9, E 2 = 45 (the 9 of D include the 2 of context_delete; 45 of the 46 non-K tests: the 46th is `context.rs:1009`, which only drops its socket config). Depends on the fake tmux and the fake-git seam from the design Job.

## 3. Tests of an external, or partly of one

No test in scope is wholly X. Partial or borderline:

- `context_manager.rs:182 a_managers_pi_gets_no_plannotator_variable_from_its_callers`: the half "a new tmux pane starts from the server's global environment" (`set-environment -g`, `:188`) is tmux behaviour. Keep only the assertion on what cpd passes (move to the fake-tmux spawn check).
- `context_manager.rs:313 session_open_brings_the_manager_into_the_chat_slot`: asserts `swap-pane`/window placement as tmux reports it (`slot`, `window_names`). Our code is the choice of slot and place; tmux layout itself is not under test, but only a model can replace it.
- `plan_cancel.rs:380 the_cancelled_plans_port_is_free_for_another_plan`: asserts what a real terminal shows (`capture-pane` text `Press Enter to retry.`) and that `send-keys Enter` reaches the prompt. Our code is the port and the prompt; the pane/keys are tmux. R: unit test.
- `plan_delete.rs:177,:232,:241,:274,:280` and `context_delete.rs:50`: assert `git branch` lists and that worktree directories are gone. Git removes them; our code decides which. Keep as the adapter contract test on a fixture repo (one test), the rest on a fake git.
- `context_distill.rs:408` and `:712`: git's `grep -w` matching semantics (`notes.txt` and `docs/other.md` `hs-d10` are excluded by `-w` and the `*.md` pathspec, `:412`). That is git's behaviour; keep one contract test on fixtures.

## 4. Leak and flake risks

1. Real `sleep 120` per pane and per fake pi (`common/mod.rs:75`): 2-6 per tmux test (`plan_cancel setup` starts a Lead and a fork pane; `cockpit` helpers start 2). They live for 2 minutes if teardown fails. Teardown is a reaper `sh` (one per env, `common/mod.rs:249`, stopped by `Drop`, `:1157`) plus `teardown.sh`; a test killed with SIGKILL relies on the reaper noticing the pid (`unverified`: the watch loop is in `TEARDOWN_SH`, not read).
2. A private tmux server per tmux test (`cpdt-*` under `TMUX_TMPDIR`, `common/mod.rs:815`). A hook may restart a server while it dies (`kill_tmux_server` doc, `:1033-1042`); no test in scope calls it (rg), so the shutdown is only `teardown.sh`. Path length of the socket is a macOS failure (`macos-gate-rest.md:96`, `plan_new_by_user::` "path-length").
3. Load-dependent waits: `wait_for_pane` (`plan_cancel.rs:66`, 2 per test, 13 tests) waits for a real pane to start the fake `cpd` and write an env file; `wait_until` for a fake pi (`context_manager.rs:101`, `by_user:44`, `plan_new_by_user:44`). The limit is 30 s (`WAIT`). At load 40-120 a tmux server start plus `sh` plus `cpd` (fake `cpd` shim `exec`s the real binary) can pass 30 s: unverified, I did not measure. They are the likeliest flake sources of this half.
4. `plan_cancel` setup spawns about 40 processes per test (3 `git`, 3 `cpd pr add`, 3 `cpd job cancel`, `plan new`, `session fork`, tmux, two shims) before the test's own work; under load this is the dominant time.
5. `spawn_cancel` (`plan_cancel.rs:178`) starts `cpd plan cancel` in the background; `finish` (`:184`) reaps it. A panic between `spawn_cancel` and `finish` (`:339-345`, `:470-476`; e.g. a `wait_until` timeout) leaves a child that polls for up to 60 s (`cancel.rs:30`) until `TestEnv::drop` kills the process group (`command()` sets `process_group`, `common/mod.rs:863`). Reaped by Drop: low risk, but not by the test.
6. Product sleeps inside tests: `plan cancel` 250 ms rounds (`cancel.rs:125`); distiller 50 ms (`distill.rs:886`). Time is real but bounded and not a flake.
7. `the_cancelled_plans_port_is_free_for_another_plan` (`plan_cancel.rs:380`): a pane that on macOS ran the real `pi` (`macos-gate-rest.md:54`) and the screen match on text: fails 0/3 on macOS (`macos-gate-rest.md:94`). Depends on terminal size and timing: flake and external leak (real pi).
8. Temp dirs: every `TestEnv` makes `cpd-test-*` under `$TMPDIR` or `/dev/shm` (`common/mod.rs:270-273`); removed by `teardown`/`Drop`. `git_repo` templates and `store_template` are shared files under `CARGO_TARGET_TMPDIR` made by rename (`common/mod.rs:970-995`): by design they persist, not a leak of a process.
9. Fake `cpd` shims in `bin/` (`context_distill.rs:52`, `fake_cpd_*`) `exec` the real binary: no extra process, but a test that calls `fake_cpd_panes` also makes `watch --ensure` a no-op (`WATCH_OK`), so no engine starts (verified by `common/mod.rs:1092-1103`). `context_distill.rs:32` sets `[watch] tick_seconds` and `fake_gh` for an engine that no test in the file starts (rg: no `watch` call): dead setup, K.
10. Flock tests: `context_distill.rs:211` holds a real lock with `nix::Flock` and drops it before the end: no leak.
