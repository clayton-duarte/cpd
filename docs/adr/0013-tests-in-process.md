---
status: accepted
---

# ADR 0013: In-process tests with fakes for tmux, git, time and processes

## Context

The test suite is slow: 2066 of 3642 tests (57%) start the `cpd` binary, tmux, git and sh. Each subprocess costs 6-50 ms on pro vs 0.45-1 ms on homeserver, totalling 7775 of 7991 test-seconds (97%) in exec time alone. The Gate's target is 133 seconds; today it takes 518-690 seconds.

**Measurement (docs/research/tests-mock-*.md audit)**:
- **Scope**: 3,295 Rust tests (1,342 units + 191 engine + 110 store + 167 session + 201 tooling + 365 tui + 508 flow + 411 pr), about 963 TypeScript tests
- **Current layout**: tests reach subprocess shims (fake-tmux binary, real git, TestEnv exec paths) and real tmux, git, processes in the teardown (reaper, leak check)
- **Exec cost amplification on pro**: `/usr/bin/true` 6-16 ms vs 0.45 ms; `git` 22-50 ms vs 1 ms; `sh -c true` 90-135 ms vs 0.45 ms

## Decision

Replace every external subprocess with an in-process fake or mock behind a trait. The shape is determined (pro-d27, pro-d28, pro-d29 by the User):

1. **Main.rs is thin** over lib.rs; no product seams beyond test feature gates
2. **Unit tests** (`#[cfg(test)] mod tests` beside the code) are the large majority
3. **Integration tests** in `cli/tests/` are in-process calls to the library API, never `CARGO_BIN_EXE`
4. **A handful of binary smoke tests** use `assert_cmd` with everything external faked
5. **External systems** (tmux, git, processes, time, the filesystem where needed) sit behind traits
6. **Fakes are never binaries or shell scripts**; in-memory only
7. **Time** is injected as `Clock`; no test sleeps, polls the wall clock or has a deadline
8. **The Gate** is `npm run verify`: cargo format check, clippy, build, the tests, and the pi check

## Ports (seams)

| Trait | Purpose | Real adapter | Fake | Test seam | Notes |
|---|---|---|---|---|---|
| **`Clock`** | System time, sleep | `SystemClock` reads wall time | `ManualClock` (virtual time, no sleep) | `&dyn Clock` at loop entry | `core/src/clock.rs`; injected at every loop (driver, session, slot, ask, plan, context, pr, engine tick, TUI view, pi adapter `waitFor`) |
| **`Runner`** | Every subprocess (tmux, git, gh, pi, node, sh, cpd) | `SystemRunner` = `deadline::output` | `FakeRunner` scripts `Output`s by program file name; routes `tmux` and `git` (in-process handlers, added by PRs port-tmux and port-git) | `&dyn Runner` / `_with` forms; `Tmux::with_runner` | `core/src/host/runner.rs`; deadline on the `ManualClock`; `deadline.rs` tests are the real contract tests |
| **`Spawn` / `Procs`** | Long-lived children, signals | `std::process::Child`, `nix::sys::signal::kill`, `/proc/<pid>/…` | `FakeSpawn` tracks pids, `kill` marks dead, stubs signal delivery | Builders in tests, records in the fake tmux route (pane pids are fake) and `FakeSpawn` state | `core/src/host/spawn.rs` (new); drivers, session panes, pollers, `kill(pid, 0)` |

**Tmux and git are not ports.** They are routes of the `FakeRunner`, chosen by the program's file name (`tmux`, `git`): the tmux route of `cli/src/fakes/tmux` and the git route (`GitRoute`, not a `Git` trait or a `FakeGit`) are in-process handlers that take what a `Command` carries (argv, env, cwd). `Tmux::with_program` and `CPD_TEST_GIT` go away. (Lead decision of 2026-10-25, refining this ADR.)

## Entry points and thin mains

**`Ports`** is `{ clock, runner, spawn }`, one `Arc<dyn T>` for each trait:
```rust
pub struct Ports {
    pub clock: Arc<dyn Clock>,
    pub runner: Arc<dyn Runner>,
    pub spawn: Arc<dyn Spawn>,
}

impl Ports {
    pub fn real() -> Self { … }  // SystemClock, SystemRunner, real Spawn
}
```

**Amendment: ambient, scoped ports** (a gap found by the Plan's Workers: only `pr` and `tag` handed `Ports` on, so a test through `run_with` still ran the real tools from every other area; about 45 wrappers called `SystemRunner`, `ProcessSpawn` or `Ports::real()` themselves). `Ports::scope(&ports, f)` sets a thread-local `Option<Ports>` for `f` and restores the previous value on exit (a drop guard, panic included); `Ports::current()` is the scoped ports, else `Ports::real()`. `run_with` scopes the whole parse-and-dispatch, and every old-name wrapper, `Env::open*`, the `Tmux` defaults and `Exe` read `Ports::current()`. Why: threading `&Ports` through ~45 signatures and every caller is one large change that blocks the migrations; the ambient scope is one named, tested seam they can use at once. A thread a command spawns captures `Ports::current()` and scopes it inside. How to replace it later: give one wrapper a `&Ports` parameter (its `_with` form already has it), switch its callers, and the guard test keeps a new wrapper from naming the real ports meanwhile.

**`cli/src/cli.rs`**: `pub fn run_with(ports: &Ports, argv: Vec<OsString>, out: &mut dyn Write) -> Result<Output, CpdError>`

**`cli/src/main.rs`** (stays thin): only `fn main() { Ports::real(); cli::run_with(…) }`

**Engine**: `pub async fn run(ports: &Ports, until: Option<Instant>) -> Result<()>`; the driver and session entries take `ports` and pass it to loops.

**TUI**: Views take `(events, clock, backend)` where `events: &dyn EventSource`, `clock: &dyn Clock`, `backend: &mut dyn Backend`; ratatui's `TestBackend` and a fake event stream replace the terminal.

**Fakes live in the crate that owns the trait**, behind a cargo feature `testkit`:
- `core/src/fakes/{clock,tmux,git,spawn}.rs` with unit tests only
- `cli/src/fakes/tmux` (the bin-level fake, `fake-tmux` binary; shared API with the lib)
- Feature `testkit` in `Cargo.toml` (cpd-core, cpd-engine, cpd-tui); dev-dependencies of the test binaries turn it on

## Exec inventory

Complete list of every `Command::new`, `.spawn()`, and process operation outside `#[cfg(test)]` modules and `*_tests.rs` files:

| File | Line | Operation | Port | Reason |
|---|---|---|---|---|
| `core/src/git/run.rs` | 57 | `Command::new("git")` | `Runner` (route `git`) | Version control spawn point |
| `core/src/host/deadline.rs` | 120+ | `subprocess.spawn()` | `Runner` | Timeout wrapper |
| `core/src/host/tmux.rs` | 501 | `Command::new("tmux")` | `Runner` (route `tmux`) | Terminal multiplexer |
| `core/src/clock.rs` | 10 | `SystemTime::now()` | `Clock` | Current time |
| `core/src/host/spawn.rs` | 50+ | Signal operations | `Spawn` | Process signals |
| `engine/src/watch/engine.rs` | 300+ | Timer reads | `Clock` | Engine tick |
| `tui/src/*/mod.rs` | Various | `Instant::now()` | `Clock` | View timing |
| `cli/src/bin/pi-check.rs` | (exception) | Real pi run on pty | `n/a` | Special case: bootstrap verification |

unverified: exact line numbers and completeness until `rg` audit run during implementation.

## Filesystem

Rule: a temp dir per test is real and allowed. A trait only where:
- Multiple tests share one resource (a git origin, a test fixture, configuration)
- The test cannot use a temp dir (rare)

**RAM disk** (PR ramdisk, if run): Gate's `TMPDIR` is a ramdisk; tests use it by default. No special handling needed.

## TUI

Views tested on ratatui's `TestBackend` through the `(events, clock, backend)` loop:
- **R tests** (133 today) → become unit tests with `TestBackend` and injected `KeyEvent`/mouse
- **T tests** (83 today) → stay integration tests, assert on `env.tmux_state()` (fake pane options, popups)
- **C tests** (18 today) → injected `ManualClock`, no sleep
- **X tests** (11 today: `Outer`, status rendering) → deleted; `scripts/preview-tui.sh` stays as manual check

## Smoke tests

At most 10 `assert_cmd` tests checking binary wiring with everything external faked:
- `cpd --help`, `cpd --version`
- `cpd watch --check` against a fake store and config
- One e2e: `cpd plan new` → `cpd tui` build → pane attach (with fake tmux, fake engine tick, fake pi)

## Test layout

From Rust Book ch. 11 and this codebase's practice:

| Scope | Location | Module naming |
|---|---|---|
| **Unit** | `<crate>/src/<module>.rs` with `#[cfg(test)] mod tests` | `<module>::<test>` in `nextest` |
| **Integration** (per-area binary) | `cli/tests/<area>/<module>.rs` | `<module>::<test>` in binary `cpd::<area>` |
| **Harness** | `cli/tests/common/mod.rs` | `TestEnv`, `Beacon`, `Wrapper` |

One `main.rs` per area (engine, flow, pr, session, store, tui, tooling); the module list in `main.rs` is the single source of truth.

## Migration recipe

For each test file under `cli/tests/<area>/`:

1. **Classify** (read the test, count the external calls): which port(s)?
2. **Port or delete**:
   - **K (kept)**: uses no external subprocess → no change
   - **T (tmux)**: calls `env.tmux()` or asserts on `tmux` output → the `tmux` route of the `FakeRunner` or delete
   - **G (git)**: calls `env.git()` or real git commands → the `git` route of the `FakeRunner` or delete
   - **R (runner/script)**: calls a script or `env.fake_*()` → shim with `Runner` or delete
   - **C (clock)**: has a sleep, poll, or `Instant` → `ManualClock`, inject at loop
   - **X (delete)**: no seam possible (rendering, real tmux client) → gone
3. **Move or rewrite**: 
   - If unit-testable: move to `<crate>/src/<module>.rs` with `#[cfg(test)]`
   - If integration needed: rewrite on the ports, keep same name where behaviour is the same
4. **Delete old**: remove the original test file or test function
5. **Restore**: restore the deleted tests of the file (docs/research/tests-mock-deleted.md) on the fake, then delete their rows.
6. **Measure**: `scripts/test-time.sh` before and after (one PR per file, ~150 tests per PR)

## What dies in mig-final

Deleted by the last migration PR:

| Artifact | Why | Replaced by |
|---|---|---|
| `cli/tests/common::TestEnv` exec paths | Tests call fakes directly | `env.command()` → `FakeRunner` (env setup stays) |
| `cli/src/fakes/` and `cli/src/bin/fake-tmux` | Moved to core's module | `core/src/fakes/tmux` (lib) |
| `scripts/tripwire` | Real tmux never called | Unconditional deny (leftover guard only) |
| `cli/tests/tooling::real_tmux()` | No test calls real | Deleted |
| TEARDOWN_SH reaper tmux part | Tests own no tmux process | Reaper stays for drivers, sessions |
| Gate leak check tmux part | No test-owned server | Check stays for driver, runner processes |
| Allowlist `allow.txt` | Guard now unconditional | Tripwire script stays (1 line) |

The Gate then refuses any test that spawns a real tmux or git.

## Per-file migration plan

One row per file with held tests, grouped by area and PR. Counts by `rg -c '#\[(test|tokio::test)'`. Target layer: `unit` = move to `<crate>/src/<module>`, `integration` = rewrite on ports in `cli/tests/`, `deleted` = removed, or mixed.

### engine (131 tests total)

| File | Tests | Deleted | Target | PR |
|---|---|---|---|---|
| watch.rs | 68 | 44 | integration (tmux, clock) | mig-engine-1 |
| doctor.rs | 27 | 4 | unit + integration | mig-engine-1 |
| watch_inbox.rs | 9 | 1 | integration (tmux) | mig-engine-2 |
| watch_curate_inbox.rs | 5 | – | integration | mig-engine-2 |
| watch_distill_inbox.rs | 1 | – | integration | mig-engine-2 |
| watch_loop_guard_inbox.rs | 4 | – | integration | mig-engine-2 |
| watch_sleep.rs | 6 | – | integration (clock) | mig-engine-2 |
| watch_team.rs | 6 | 1 | integration (clock) | mig-engine-2 |
| watch_titles_width.rs | 2 | – | integration | mig-engine-1 |
| watch_worker_archive.rs | 3 | – | integration | mig-engine-1 |

### flow (531 tests total)

| File | Tests | Deleted | Target | PR |
|---|---|---|---|---|
| plan.rs | 45 | 5 | integration (tmux) | mig-flow-1 |
| job.rs | 73 | – | integration (git, runner) | mig-flow-2 |
| attempt.rs | 30 | 14 | integration (git, clock) | mig-flow-2 |
| driver.rs | 25 | 18 | integration (tmux, clock) | mig-flow-3 |
| context.rs | 41 | 1 | integration (git) | mig-flow-4 |
| config.rs | 47 | – | unit + integration | mig-flow-1 |
| context_distill.rs | 33 | – | integration (git, clock) | mig-flow-2 |
| review.rs | 1 | – | integration (git) | mig-flow-3 |
| spec.rs | 14 | – | unit | mig-flow-1 |
| secret.rs | 0 | – | integration (tmux) | mig-flow-1 |
| secret_op.rs | 17 | – | integration (git) | mig-flow-2 |
| policy_scopes.rs | 18 | – | unit | mig-flow-1 |
| policy_fallback.rs | 15 | – | unit | mig-flow-1 |
| review_job.rs | 7 | 2 | integration | mig-flow-3 |
| context_curation.rs | 14 | – | integration | mig-flow-4 |
| policy.rs | 14 | – | unit | mig-flow-1 |
| plan_outline.rs | 20 | – | integration | mig-flow-1 |
| plan_cancel.rs | 1 | 11 | integration (clock) | mig-flow-1 |
| plan_show.rs | 10 | – | unit | mig-flow-1 |
| context_digest.rs | 11 | – | unit | mig-flow-4 |
| ask.rs | 9 | 1 | integration (clock) | mig-flow-3 |
| repo_cmds.rs | 0 | 6 | deleted file | mig-flow-4 |
| metrics.rs | 8 | – | unit | mig-flow-1 |
| secret_grants.rs | 6 | – | integration | mig-flow-2 |
| driver_reload.rs | 4 | 1 | integration (tmux, clock) | mig-flow-3 |
| secret_guard.rs | 9 | – | unit | mig-flow-1 |
| repo_config.rs | 6 | 1 | unit | mig-flow-1 |
| context_pending.rs | 6 | – | unit | mig-flow-4 |
| context_pending_waiting.rs | 2 | – | unit | mig-flow-4 |
| plan_new_by_user.rs | 2 | – | integration | mig-flow-1 |
| context_manager_by_user.rs | 2 | – | integration | mig-flow-4 |
| driver_idle.rs | 2 | – | integration (clock) | mig-flow-3 |
| review_close.rs | 4 | – | integration | mig-flow-3 |
| context_manager.rs | 5 | 1 | integration | mig-flow-4 |
| plan_delete.rs | 4 | 3 | integration (git) | mig-flow-4 |
| context_delete.rs | 0 | 1 | deleted file | mig-flow-4 |
| plan_render_waiting.rs | 2 | – | unit | mig-flow-1 |
| plan_render_cancelled.rs | 4 | – | unit | mig-flow-1 |
| plan_overlap.rs | 12 | – | integration | mig-flow-1 |
| review_plannotator.rs | 8 | – | integration | mig-flow-3 |

### pr (317 tests total)

| File | Tests | Deleted | Target | PR |
|---|---|---|---|---|
| pr.rs | 103 | 37 | integration (git) | mig-pr-1 |
| pr_crud.rs | 67 | – | integration (git) | mig-pr-2 |
| merge.rs | 6 | 41 | integration (git) | mig-pr-1 |
| git.rs | 51 | 14 | integration (git, runner) | mig-pr-3 |
| pr_local_needs.rs | 5 | – | unit | mig-pr-1 |
| pr_merge.rs | 14 | – | integration (git) | mig-pr-2 |
| pr_push.rs | 11 | – | integration (git) | mig-pr-2 |
| pr_view.rs | 6 | – | unit | mig-pr-1 |
| pr_start.rs | 25 | – | integration (git, clock) | mig-pr-3 |
| pr_cleanup.rs | 14 | – | integration (git) | mig-pr-3 |
| pr_cleanup_cancel.rs | 3 | – | integration (git) | mig-pr-3 |
| repo_hooks.rs | 6 | – | integration (git) | mig-pr-3 |
| git_guard.rs | 6 | 2 | integration (git) | mig-pr-3 |

### session (166 tests total)

| File | Tests | Deleted | Target | PR |
|---|---|---|---|---|
| session_reload.rs | 12 | – | integration (tmux, runner, clock) | mig-session-1 |
| host.rs | 26 | – | unit + integration (tmux) | mig-session-1 |
| session.rs | 55 | – | integration (tmux) | mig-session-1 |
| session_watchdog.rs | 1 | 2 | integration (tmux, clock) | mig-session-2 |
| board.rs | 6 | – | integration (tmux) | mig-session-1 |
| session_send.rs | 18 | – | integration (tmux, runner) | mig-session-1 |
| session_resume.rs | 5 | – | integration (tmux) | mig-session-1 |
| session_asleep.rs | 3 | – | integration | mig-session-2 |
| session_first_message.rs | 3 | – | integration (runner) | mig-session-1 |
| pill_lines.rs | 5 | – | unit | mig-session-1 |
| session_titles.rs | 14 | – | unit | mig-session-1 |
| plannotator_env.rs | 2 | – | unit | mig-session-1 |
| session_list_archived.rs | 2 | – | unit | mig-session-2 |
| session_report.rs | 3 | – | unit | mig-session-2 |
| session_retier.rs | 7 | – | unit | mig-session-2 |
| session_titles_glance.rs | 4 | – | unit | mig-session-2 |

### store (110 tests total)

| File | Tests | Deleted | Target | PR |
|---|---|---|---|---|
| store.rs | 68 | – | unit + integration | mig-engine-1 |
| events.rs | 35 | – | unit | mig-engine-1 |
| store_window.rs | 4 | – | integration | mig-engine-2 |
| store_local_needs.rs | 2 | – | unit | mig-engine-2 |
| store_asleep.rs | 1 | – | unit | mig-engine-2 |

### tui (357 tests total)

| File | Tests | Deleted | Target | PR |
|---|---|---|---|---|
| tui_tree.rs | 65 | – | unit (delete 6 real-tmux render tests) | mig-tui-1 |
| tui_widgets.rs | 44 | – | integration (tmux, backend) | mig-tui-2 |
| tui.rs | 33 | 1 | integration (tmux, tmux render deleted) | mig-tui-1 |
| tui_worker.rs | 19 | – | unit | mig-tui-1 |
| tui_plan.rs | 25 | – | unit | mig-tui-1 |
| tui_dialog.rs | 13 | – | unit (TestBackend) | mig-tui-1 |
| inbox.rs | 15 | – | integration (tmux) | mig-tui-2 |
| bare.rs | 9 | 1 | integration (tmux) | mig-tui-1 |
| tui_merge.rs | 16 | – | unit (TestBackend) | mig-tui-2 |
| tui_logs.rs | 16 | – | unit | mig-tui-1 |
| tui_history.rs | 5 | – | unit | mig-tui-1 |
| tui_checklist.rs | 7 | – | unit | mig-tui-1 |
| tui_idle.rs | 3 | – | integration (clock, backend) | mig-tui-3 |
| inbox_rules.rs | 11 | – | unit | mig-tui-2 |
| tui_loop_guard.rs | 2 | – | unit + integration | mig-tui-3 |
| tui_runner.rs | 3 | – | unit | mig-tui-1 |
| tui_chat_pad.rs | 2 | – | unit | mig-tui-1 |
| tui_plan_cancelled.rs | 3 | – | unit | mig-tui-1 |
| tui_plan_waiting.rs | 3 | – | unit | mig-tui-1 |
| tui_tree_asleep.rs | 3 | – | unit | mig-tui-1 |
| tui_tree_archive_manager.rs | 2 | – | unit | mig-tui-1 |
| tui_tree_delete.rs | 3 | – | unit | mig-tui-1 |
| tui_pulse.rs | 12 | – | unit | mig-tui-1 |
| tui_read.rs | 42 | – | unit | mig-tui-1 |
| tui_reset_layout.rs | 1 | – | unit | mig-tui-1 |
| tui_titles.rs | 0 | – | unit | mig-tui-1 |

### tooling (253 tests total)

| File | Tests | Deleted | Target | PR |
|---|---|---|---|---|
| harness.rs | 34 | – | integration (TestEnv behaviour) | mig-tooling |
| install_script.rs | 51 | – | integration (tmux route, git route) | mig-tooling |
| fake_tmux.rs | 57 | – | integration (fake-tmux binary) | mig-tooling |
| cargo_script.rs | 12 | – | unit (sccache, cargo.sh) | mig-tooling |
| slot.rs | 14 | – | integration (clock) | mig-tooling |
| e2e.rs | 0 | – | deleted file | mig-tooling |
| tag_script.rs | 7 | – | unit (tag-version.sh logic) | mig-tooling |
| prune_script.rs | 5 | – | unit (prune-worktrees.sh logic) | mig-tooling |
| test_changed_script.rs | 17 | – | unit (test-changed logic) | mig-tooling |
| tests_script.rs | 6 | – | integration (real cargo, nextest, tsc in scratch workspace) | mig-tooling |
| preview_script.rs | 11 | – | unit (preview-tui.sh) | mig-tooling |
| npm_fresh.rs | 4 | – | unit | mig-tooling |
| gate_run.rs | 10 | – | integration (gate-run.sh) | mig-tooling |
| layout.rs | 4 | – | unit (test binary layout) | mig-tooling |
| version.rs | 12 | – | unit (version string) | mig-tooling |
| no_repo_lock.rs | 2 | – | unit | mig-tooling |
| interface.rs | 3 | – | unit (command tree) | mig-tooling |
| dead_code.rs | 1 | – | unit (unused code check) | mig-tooling |
| pill_parity.rs | 1 | – | unit (pill rendering) | mig-tooling |
| pill_sites.rs | 2 | – | unit (pill locations) | mig-tooling |

## Tests deleted before the migration

On 2026-10-25 the incident PR tripfix (#329) deleted 213 exec-based tests that failed Gates under load (e2e.rs was deleted earlier by PR #322), and docs/research/tests-mock-deleted.md lists them. The PR that migrates a file restores the tests listed for it there as in-process tests on fakes, keeps their names, and deletes their rows from that file in the same PR.

| Area | File | Deleted | PR that restores them |
|---|---|---|---|
| engine | watch.rs | 44 | mig-engine-1 |
| engine | doctor.rs | 4 | mig-engine-1 |
| engine | watch_inbox.rs | 1 | mig-engine-2 |
| engine | watch_team.rs | 1 | mig-engine-2 |
| flow | plan.rs | 5 | mig-flow-1 |
| flow | attempt.rs | 14 | mig-flow-2 |
| flow | driver.rs | 18 | mig-flow-3 |
| flow | context.rs | 1 | mig-flow-4 |
| flow | review_job.rs | 2 | mig-flow-3 |
| flow | plan_cancel.rs | 11 | mig-flow-1 |
| flow | ask.rs | 1 | mig-flow-3 |
| flow | repo_cmds.rs | 6 | mig-flow-4 |
| flow | driver_reload.rs | 1 | mig-flow-3 |
| flow | repo_config.rs | 1 | mig-flow-1 |
| flow | context_manager.rs | 1 | mig-flow-4 |
| flow | plan_delete.rs | 3 | mig-flow-4 |
| flow | context_delete.rs | 1 | mig-flow-4 |
| pr | pr.rs | 37 | mig-pr-1 |
| pr | merge.rs | 41 | mig-pr-1 |
| pr | git.rs | 14 | mig-pr-3 |
| pr | git_guard.rs | 2 | mig-pr-3 |
| session | session_watchdog.rs | 2 | mig-session-2 |
| tui | tui.rs | 1 | mig-tui-1 |
| tui | bare.rs | 1 | mig-tui-1 |

## Scripts table

| File | Lines | Tests today | Decision | Stays shell | Moves to Rust | PR |
|---|---|---|---|---|---|---|
| `install-cpd.sh` | ~200 | install_script (52) | Moves to Rust as `cpd install` subcommand | thin bootstrap only | binary swap, adapter swap, follow-ups | rs-install |
| `prune-worktrees.sh` | ~80 | prune_script (6) | Moves to Rust as `cpd prune` | – | full logic | rs-prune |
| `tag-version.sh` + `sync-version.sh` | ~100 | tag_script (7), version (4) | Moves to Rust as `cpd tag` | – | version bump, git tag, push | rs-tag |
| `gate-run.sh` | ~150 | gate_run (8), layout | Moves to Rust as `cpd gate` | – | leak check, finish report | rs-gate-run |
| `tests.sh` | ~50 | tests_script (6) | Stays shell, thin wrapper | calls `cpd test` subcommand | test discovery, nextest call | rs-tools (helper) |
| `test-changed` | ~80 | test_changed_script (17) | Stays shell, thin wrapper | calls `cpd test changed` subcommand | path analysis, nextest filter | rs-tools (helper) |
| `pi-check.sh` | ~200 | pi_check (4) + pi adapter tests | Drop tmux, run pi on pty (Option B of spec notes) | bootstrap check | pty runner, activation gate | mig-pi (part 1) |
| `pi-check-pty.py` | ~150 | – | Stays (pty runner for pi-check.sh) | – | – | (no PR) |
| `scripts/pi-check.sh` (exception) | – | Stays | Named exception of pro-d9 | – | – | (no PR) |
| `cargo.sh` | ~50 | cargo_script (12) | Stays shell | sccache setup, cargo lookup | – | (no PR) |
| `npm-fresh.sh` | ~30 | npm_fresh (4) | Stays shell | npm ci, clean node_modules | – | (no PR) |
| `new-migration.sh` | ~50 | – | Stays shell | migration template | – | (no PR) |
| `repin.sh` | ~40 | – | Stays shell | SCHEMA_HASH pin, conflict resolution | – | (no PR) |
| `check-migrations-frozen.sh` | ~30 | – | Stays shell | migration freeze check | – | (no PR) |
| `check-portable.sh` | ~100 | – | Stays shell + guard | portability lint | guard script (stays) | (no PR) |
| `preview-tui.sh` | ~80 | preview_script (11) | Stays shell | manual preview, tmux session | – | (no PR) |
| `sync-version.sh` | (part of tag-version.sh) | – | Merged into `cpd tag` | – | – | rs-tag |

**Scripts moving to Rust** (rs-* PRs): install-cpd.sh, prune-worktrees.sh, tag-version.sh, gate-run.sh, plus helpers in rs-tools (test discovery, path analysis). Tests move or are rewritten on the ports; the shell files stay thin wrapper stubs that call `cpd` subcommands, or are deleted.

## PR list and Job count

| Key | Shape | Needs | Delivers | Gate effect | Jobs |
|---|---|---|---|---|---|
| `ramdisk` | setup | – | RAM disk config for Gate | `TMPDIR=/dev/shm` on Linux | 1 (script) |
| `design` | ADR | – | This document, implementing.md update | None (doc) | 1 (spec) |
| `clk` | seam | – | `Clock` trait, `ManualClock`, `SystemClock` | None direct | 2 |
| `port-run` | seam | `clk` | `Runner` trait, `SystemRunner`, `FakeRunner`, feature `fakes` | None direct | 2 |
| `port-tmux` | seam + bin | `clk` | `tmux` route of the `FakeRunner` (in-process handler from `cli/src/fakes/tmux`), `Tmux::with_runner` | None direct | 3 |
| `port-git` | seam | `clk` | `git` route of the `FakeRunner` (in-process handler) | None direct | 2 |
| `ports` | product | `clk`, `port-*` | `Ports` struct `{ clock, runner, spawn }`, thin main, `run_with`, engine/session/tui signatures | None direct | 2 |
| `testkit` | harness | `ports` | `#[cfg(testkit)]` features, `ManualClock` tests, trait unit tests | Saves build (10s) | 1 |
| `mig-pr-1..3` | tests | `testkit`, `port-git` | Migrate `cli/tests/pr/*` (317 tests, ~105 each) | Saves ~100 s | 3 |
| `mig-flow-1..4` | tests | `port-*`, `testkit` | Migrate `cli/tests/flow/*` (531 tests, ~132 each) | Saves ~80 s | 4 |
| `mig-tui-1..3` | tests | `port-tmux`, `testkit` | Migrate `cli/tests/tui/*` (357 tests, ~119 each) | Saves ~60 s | 3 |
| `mig-engine-1..2` | tests | `port-*`, `testkit` | Migrate `cli/tests/engine/*` + `cli/tests/store/*` (241 tests, ~120 each) | Saves ~50 s | 2 |
| `mig-session-1..2` | tests | `port-*`, `testkit` | Migrate `cli/tests/session/*` (166 tests, ~83 each) | Saves ~20 s | 2 |
| `rs-install` | product | `ports` | Rust subcommand for install-cpd.sh logic | Saves ~5 s (Shell → Rust) | 1 |
| `rs-prune` | product | `ports` | Rust subcommand for prune-worktrees.sh | Saves ~2 s | 1 |
| `rs-tag` | product | `ports` | Rust subcommand for tag-version.sh + sync-version.sh | Saves ~1 s | 1 |
| `rs-gate-run` | product | `ports` | Rust subcommand for gate-run.sh | Saves ~3 s | 1 |
| `rs-tools` | product | `ports` | Rust subcommand for other scripts | Saves ~1 s | 1 |
| `mig-tooling` | tests | `testkit`, `rs-*` | Migrate `cli/tests/tooling/*` (253 tests, ~253 each) | Saves ~15 s | 1 |
| `smoke` | tests | `ports` | Assert_cmd tests: help, version, one e2e | None direct | 1 |
| `lint-audit` | audit | `mig-*` | Exec inventory check, per-file plan audit | None (doc) | 1 |
| `lint-fix` | audit | `lint-audit` | Fix any audit findings | None direct | 1 |
| `lint-gate` | gate | all | Add to Gate's lint suite | Enforces no new leaks | 1 |
| `final` | cleanup | all `mig-*` | Delete artifact list above (TestEnv exec, `real_tmux()`, etc.); re-baseline | Unlocks next plan | 1 |

**Total: ~50 Jobs**. Parallel after `testkit`: all `mig-*` and `rs-*` (independent). Gate effect: **133 s target**, measured on pro at load 20-38.

---

*Mark claims unverified and ask the Lead before any User decision point.*
