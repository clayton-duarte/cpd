# Research: ports, Spawn, run_with, thin mains, rs-* (Job `dz-ports`, attempt `dz-ports-a1`)

Static research on `origin/main` of this worktree plus `cpd/tests-mock/clk` (Clock, read only). Nothing was run except `rg`, `wc` and `sed`. Rule of the Plan: tests test only our code; tmux, git, gh, pi, node, the network, processes and real waiting are mocked (ADR 0013, `docs/adr/0013-tests-in-process.md`). Every count is from `rg` on this tree today; "guess:" and "unverified" label the rest.

**Findings that change the brief (read first).**
1. `deadline::output` has **eight** call sites, not nine (§1.A). Fourteen more places start a process and wait for it **without** `deadline::output` (§1.B), so a `Runner` that covers only the eight leaves real children in gh, pi, hooks, `--version` and more. I propose a PR `port-run2` for them (§6.2).
2. The ADR sizes are stale: `install-cpd.sh` is 287 lines with **51** tests (ADR: ~80), `prune-worktrees.sh` 241 lines / 5 tests, `gate-run.sh` 159 / 10, `tag-version.sh` 44 / 7, `sync-version.sh` 81 / 12 (`version.rs`). `assert_cmd` (ADR "smoke") is in no `Cargo.toml` or `Cargo.lock`.
3. Process-global state blocks `run_with` as the brief states it: `STARTED_AS` (`OnceLock`, `host/exe.rs:78`), the engine's `STOP` flag set by `sigaction`, the driver's inherited fd 3 (`lock::inherited`), 35 `env::var(_os)` reads in product, 15 clap `env =` attributes, 6 `current_dir()` calls, `process::exit` in `cli::slot_exit`. `Ports` of `{clock, runner, spawn}` is not enough: it needs `self_exe` and an environment (§4.4). Under nextest each test is a process of its own (`.config/nextest.toml`), so setting env/cwd at the start of a test is safe there, not under `cargo test` (guess: acceptable, the Gate is nextest).
4. `gate-run.sh` **wraps** `npm run verify:steps`, which builds cpd. It cannot be replaced by the cpd it builds. Only its after-check can move into Rust (§5.5).
5. `install-cpd.sh` cannot be pure Rust: it builds the binary. It becomes `cargo build` plus `exec <built cpd> install` (§5.1).

## 1. Every place that starts a process or waits

Legend: **R** = `Runner` (run to completion, capture), **S** = `Spawn` (child outlives the call, is signalled or reaped, or `exec`s), **P** = process facts (`alive`, `exe_of`, `cmdline`), tests reach it by area `F`low `P`r `S`ession `E`ngine `T`ui `O`store `X`tooling, `u` = in-crate unit tests with real children today.

### A. `deadline::output` (the real Runner, `core/src/host/deadline.rs`: `process_group(0)`, 10 ms poll, SIGKILL of the group, 5 s grace, 1 s drain)

| site | starts | port | tests |
|---|---|---|---|
| `git/run.rs run` | git (19 files use it) | R `git` | all but X, u |
| `host/tmux.rs Tmux::output` | tmux (14 `Tmux::new` in core, 5 engine, 11 tui) | R `tmux` | all, u |
| `inbox/team.rs github_lookup` | `gh api graphql` | R `gh` | F, E |
| `secret/op.rs run` | `op` with stdin bytes (writer thread) | R `op` | F, u |
| `config/catalog.rs read` | `node` catalog script, env, cwd | R `node` | F, E |
| `engine doctor/tools.rs run_tool` | any tool `--version` | R | E |
| `engine watch/pr.rs poll` | `<cpd> pr poll` | R | E, P |
| `engine watch/team.rs start` | `node` in a thread | R | E |

### B. Waiters that do not use `deadline::output` (each has its own try_wait loop)

| site | starts | port | tests |
|---|---|---|---|
| `pr/gh.rs run_timed` (29 callers, 60 s, no process group, kills the child only) | `gh` | R `gh` | P, F |
| `context/distill.rs wait_limited` | `pi`, stderr to a log file, `CPD_*` stripped, cwd | R | F |
| `context/distill.rs:434` | `git grep` direct, bypasses `git::run` | R `git` | F |
| `host/exe.rs asks_to_open_store` (10 s, 20 ms poll) | `<cpd> --version` | R | S, E, X |
| `git/hooks/exec.rs start/wait` | `sh -c <hook>`, idle prefix, group, stdin context, merged output read by a thread, hooks of a block run in parallel | R, one thread per hook | P, F, u |
| `session/retitle.rs run_pi` | `pi` with stdin text, `wait_with_output`, **no deadline** | R | S, E |
| `plan/lock.rs ensure_watch` | `cpd watch --ensure` through PATH | R | F, P |
| `host/tmux.rs installed_version` | `tmux -V`, no deadline | R `tmux` | S, E |
| `engine watch/restart.rs rebuild_cockpit` | `<cpd> tui`, no deadline | R | E |
| `tui tree/actions.rs call`, `curate_flow call_cpd` | `<cpd> <argv>` | R (or in-process `RunCommand`) | T |
| `tui widgets/host.rs run` | `ioreg`, `sysctl`, `ps`, `vm_stat` (macOS) | R | T |
| `tui widgets/mod.rs hostname` | `hostname` | R | T |
| `inbox/cmd.rs local_offset` | `date +%z` | R (guess: `time`'s local offset would delete it; unverified) | F |
| `driver/stall.rs cmdline` (macOS) | `ps -o command= -p` | P | F |

### C. Children that outlive the call, are signalled or replace the process

| site | what | port | tests |
|---|---|---|---|
| `driver/start.rs spawn` | `<cpd> attempt run`, `setsid`, log file, fd 3 handed over (`lock::hand_over`), nice prefix, reaper thread | S | F, E, u |
| `engine watch/distill.rs spawn` | `<cpd> context distill`, setsid, pid file `record_detached`, reaper thread | S | E |
| `engine watch/restart.rs spawn` | `<cpd> watch --restart-tmux`, same shape | S | E |
| `engine watch/titles.rs spawn/reap` | `<cpd> session retitle`, `Child::try_wait` | S | E |
| `session/run.rs launch_pi`, `PiProc` | `pi` as the wrapper's child, `waitpid`, `signal`, `adopt`; `stop_pi` and `reload.rs` | S | S, u |
| `host/exe.rs Exe::exec`, `tui/cockpit.rs exec_attach` | `exec` of cpd or `tmux attach` | S `exec` | S, T |
| `tui widgets/host.rs Gpus::start` | `nvidia-smi -lms` streaming, killed in `Drop` | S stream | T |
| `slot.rs execute`, `cli.rs slot_exit` | `cpd slot --`, forwards signals, `process::exit` | none: leaves with the no-slots decision (guess: pro-d29, unverified) | X |
| `cli/src/fakes/tmux/hooks.rs run_shell` | `/bin/sh -c` for fake hooks | R `sh` route (the fake's own) | all |

### D. Liveness, signals, process facts (all `kill(2)`, `/proc`, `libproc`)

`alive` is written **six times** as `fn alive`: `store/lock.rs:60`, `driver/run.rs:98` (zombie-aware), `driver/stall.rs:189`, `engine escalations.rs:54`, `tui merge/marker.rs:115`, `tui read/widgets.rs:329`; and twice inline as `kill(pid, None)`: `store/mod.rs:855` (writer holders), `git/snapshot.rs:48` (`sweep`). (`tui/src/merge/marker_d550.rs:10` is a test helper: `Command::new("true")`.) Also: `driver/stall.rs` (`stat`, `all_pids`, signal and `terminate` with GRACE), `driver/worker.rs retry_kill`, `host/tmux.rs pid_exe`, `server_exe`, `on_server_binary` (`/proc/<pid>/exe`, libproc), `tui cockpit.rs:640`, `session/run.rs` signal. All move to `Spawn` (§4.2): one `alive`, one `signal`, one `exe_of`.

### E. Not a process, not a port

Threads (`pump.rs`, `poll_run.rs`, `op.rs` writer, reaper threads) and file reads of `/proc/stat` and `/proc/meminfo` (`tui widgets/host.rs`, Linux only, read only; unverified: left real, a fixture file under the test home is the alternative). `no tokio::time` exists in product (`rg`: 0 hits).

### F. Tests today (what the ports must replace)

`TestEnv::command/cpd/cpd_in/cpd_with` + `CARGO_BIN_EXE_cpd` (`cli/tests/common/mod.rs`, 1,318 lines): `env.cpd*` call counts F 724, S 243, T 98, P 88, E 23, O 18, X 6. Long-running `cpd` children: the engine (`engine/watch*.rs`, `pr/pr_start`, `session.rs:1388`), `pr rebase/push` held by a hook (`pr.rs:446`, `pr_cleanup.rs:572`), `events follow` (`store/events.rs:701`), `ask` (`flow/ask.rs:118`), drivers (`flow/driver*.rs`), `context distill`, the `session run` wrapper (`session.rs:99`, `host.rs:768`), `tui` panes. `env.fake(name, body)` / `fake_tmux()`: 124 uses; `Tmux::with_program`: 13. A source scan that pins "tmux is spawned only in `host::tmux::client`" lives at `cli/tests/session/host.rs:820` and must be rewritten as the Runner guard (§6.4).

## 2. Time (what `Clock` on `clk` leaves)

`clk` gives `Clock { now() -> Instant, sleep(Duration) }`, `SystemClock`, `ManualClock { advance, elapsed, naps }`, and `Store::open_with`, locks and waits `<name>_with` (kept beside the old name). **Missing for ports:** a wall clock. 28 `OffsetDateTime::now_utc()`, 12 `SystemTime::now()` and the string helper `clock::now()` (about 60 callers, guess: imprecise count) give timestamps and file ages. Proposal: `Clock::utc(&self) -> OffsetDateTime`; `ManualClock` = fixed epoch + `elapsed`; `clock::now()` stays for the product and reads `SystemClock`. 158 product sites use `Instant::now`, `thread::sleep`, `SystemTime::now` or `now_utc` (`rg`).

Loops that need the clock, grouped by the PR that threads it (the files and lines are today's):

| PR | loops |
|---|---|
| `port-run` / `port-run2` (inside the Runner, so they leave product code) | `deadline.rs` POLL/GRACE/DRAIN, `gh.rs` 10 ms, `distill.rs wait_limited`, `exe.rs` 20 ms, `hooks/exec.rs` wait, `git/run.rs:111` and `git/lead.rs:141` jitter sleeps (these two stay: retry between runs) |
| `ports-4` driver and session | `driver/run.rs:531`, `worker.rs:274-292, 371`, `stall.rs:84,89,204-209` (mtime vs `SystemTime`), `session/run.rs:81,119,129,277,303-315`, `reload.rs:55-80,128`, `fork_wait.rs:57,65`, `host/tmux.rs:798` (wait for the server, 10 s / 50 ms) |
| `ports-5` engine | `engine.rs:344,376,408-442` (tick, NAP), `restart.rs:90,293-298,339-344`, `pr.rs:52,123,238`, `team.rs:62-80,138`, `ensure.rs:72,171-199,265-296`, `lock.rs:57` (file mtime), `report.rs:206-242`, `titles.rs:90,220`, `sleep.rs:68`, `distill.rs:50`, `inbox.rs:34`, `liveness.rs:267`, `schedule.rs:90`, `poll_run.rs:31,37` |
| `ports-6` tui | `pump.rs:69`, `worker/mod.rs:111-136`, `tree/view.rs:719`, `plan/mod.rs:159-162`, `palette.rs:14`, `menu.rs:136`, `folds.rs:104`, `viewlog.rs:25`, `marker.rs:40`, `logs/mod.rs:104`, `lib.rs` back-off sleep 3 s |
| `ports-7` rest of core | `ask/question.rs:43-50`, `plan/cancel.rs:111-117`, `pr/rebase.rs:489-515`, `pr/poll.rs:193-206`, `events/mod.rs:181` (follow poll), `inbox/rules.rs:47,50`, `inbox/cmd.rs`, `secret/mod.rs`, `context/distill.rs:515-520,875-886` |

## 3. Entries today and what `run_with` must thread

- `cli/src/main.rs`: `host::exe::record_start()`, then `cpd::main_with(args_os, &mut stdout.lock()) -> ExitCode`. `lib.rs main_with` = `output::emit(cli::run(argv), out)`.
- `cli/src/cli.rs`: `run(argv) -> Result<Output, CpdError>` calls a private `run_with(argv, on_a_terminal)` (name clash: the new public one is `run_with(&Ports, argv)`; rename the private to `run_on`). `dispatch(command)` calls `<area>::run(args)`, and each area opens `Env::open()` itself (96 `Env::open*` sites in core): ports must ride on `Env` (`Env.ports`, cloned `Arc`s) or every area needs a parameter.
- `Output::{Success, Owned}`: `Owned` means the command owned the terminal and printed itself (`session run`, `tui`, `cockpit`). In-process tests need a stdout/stderr sink for it too (guess: `Ports.out`, see §4.4).
- engine: `watch::run(args)` loops `while !stopping()`; `STOP` is an atomic set by `catch_signals` (`sigaction`, process-global). `Engine::for_test(home, tmux)` (`engine.rs:77`, `#[cfg(test)]`) already builds an engine for step tests: the seam is half there.
- driver: `attempt run` → `driver::run(RunArgs)`: `lock::inherited` (dup of fd 3), `Exe::current()`, `Tmux::new`.
- tui: `cpd_tui::run(args, version, run_command, restart)`; `RunCommand = fn(Vec<OsString>) -> Result<Output, CpdError>` (a fn pointer: it cannot capture `Ports`; becomes `Arc<dyn Fn>`) and `Restart` (a struct of four fn pointers, same). Views call `ratatui::init()` and `event::read` directly; `TestBackend` needs the terminal and the event source injected (`ports-6`).

### 3.1 Which entries tests start as a binary, and the in-process call that replaces each

| started today (`rg -l 'CARGO_BIN_EXE\|env.command'`) | in-process replacement |
|---|---|
| one-shot `env.cpd*` (all areas, 1,200 calls) | `Fx::cpd*` → `run_with(&ports, argv, out)` |
| `cpd watch` engine (`engine/watch*.rs`, `pr/pr_start.rs`, `session.rs:1388`) | `watch::run(&ports, until)` stepped by `ManualClock::advance` (or one `tick`); `E3`-`E10` steps already have `Engine` unit tests |
| `attempt run`, `session run`, `context distill`, `pr rebase/push` held open | `run_with` called by the test on the same thread; the holding is a `FakeRunner` reply with `took` (hook) or a `FakeSpawn` pid that has not exited |
| `events follow`, `ask` | `run_with` on a thread, `ManualClock::advance` per poll; the poll calls `clock.sleep` (ports-7) |
| `cpd tui` views in panes | view `run(&ports, term, events)` on `TestBackend` (ports-6); the panes themselves are the fake tmux |
| `fake-tmux` bin (`tooling/fake_tmux.rs`, 57 tests, 3 `Command` uses) | `fakes::tmux::run` route, no binary (port-tmux) |
| `--help`, `--version` (`tooling/interface.rs`, `version.rs`) | `run_with`; one real-binary smoke test each (sm-1) |
| shell scripts (`install_script`, `prune_script`, `tag_script`, `gate_run`, `test_changed_script`, `tests_script`, `preview_script`, `cargo_script`, `npm_fresh`, `tripwire_script`: 133 tests) | §5 |

## 4. Design

### 4.1 `Ports` (cpd-core, `core/src/ports.rs`)

```rust
#[derive(Clone)]
pub struct Ports { pub clock: Arc<dyn Clock>, pub runner: Arc<dyn Runner>, pub spawn: Arc<dyn Spawn> }
impl Ports { pub fn real() -> Ports }            // SystemClock, ProcessRunner, ProcessSpawn
```
Fakes live under feature `fakes` of cpd-core (the Plan's name; ADR 0013 says `testkit`, guess: the ADR's word is the harness, the Plan's the feature): `Fakes { ports, clock: Arc<ManualClock>, runner: Arc<FakeRunner>, spawn: Arc<FakeSpawn> }` from `Fakes::new()`. Convention from `clk`: every existing public signature stays; each function that needs a port gains `<name>_with(&Ports, ..)` and the old name calls it with `Ports::real()`. Exception: `Env`, which gains the field because 96 sites cannot all change.

### 4.2 `Spawn`

```rust
pub trait Spawn: Send + Sync {
    fn spawn(&self, l: &Launch) -> io::Result<Pid>;               // detached or child; reaps (replaces the reaper threads)
    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>>; // PiProc, titles reap
    fn signal(&self, p: Pid, s: Signal) -> io::Result<()>;
    fn alive(&self, p: Pid) -> bool;                              // the eight sites, zombie-aware
    fn exe_of(&self, p: Pid) -> Option<PathBuf>;                  // pid_exe
    fn cmdline(&self, p: Pid) -> Option<String>;                  // stall.rs on macOS
    fn exec(&self, l: &Launch) -> io::Error;                      // returns only on failure
    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)>; // Gpus (unverified need)
    fn self_exe(&self) -> Result<PathBuf, CpdError>;              // replaces STARTED_AS
}
pub struct Launch { program, args, env_set, env_remove, cwd, setsid: bool, stdout_to: Option<PathBuf>, hand_over: Option<RawFd>, idle: bool }
```
`FakeSpawn`: pids from a counter, a table `Pid -> {Launch, Running | Exited(status)}`, `launches()`, `signals()`, `exit(pid, code)`; `signal` of TERM/KILL marks the pid exited by that signal; `alive` reads the table; `exec` records and returns `io::Error::other("faked exec")`; `self_exe` returns a configured path. A launch whose program is the `self_exe` and whose argv is a cpd command can be routed to `run_with` by the test **synchronously** (decision for the Lead: routing it onto a thread would put virtual time under a second thread, guess: avoid).

### 4.3 `Runner` (named here only for the Spawn boundary; `port-run` owns it)

```rust
pub trait Runner: Send + Sync { fn run(&self, c: &Call) -> Result<Output, Failed>; }
pub struct Call { program, args, env_set, env_remove, cwd, stdin: Option<Vec<u8>>, deadline: Duration, stderr_to: Option<PathBuf> }
```
What the 22 sites need beyond today's `deadline::output(cmd, deadline, tool)`: stdin bytes (op, hooks, retitle), stderr to a file (pi distill), env removal by prefix (`CPD_*`), cwd. `Failed::{Io(e), Timeout}` stays: callers match `Io(NotFound)` (op, doctor), so a `FakeRunner` with no route for a program returns `Io(NotFound)`. `FakeRunner` routes by program name to a `Handler(&Call, &dyn Clock) -> Reply { status, stdout, stderr, took }`; it advances the `ManualClock` by `took` and returns `Timeout` when `took > deadline`: this is how a hung `gh` is tested with no sleep. `tmux` and `git` routes are `port-tmux` and `port-git`.

### 4.4 Gaps in the brief's `Ports`

(a) `self_exe` is in `Spawn` (§4.2). (b) **Environment and cwd:** 35 `env::var(_os)` reads, 15 clap `env =`, 6 `current_dir()`. Option 1, recommended (KISS, unverified for the cost): tests set them once at test start, safe under nextest, and `Fx` (testkit) does it and restores nothing (process dies). Option 2: `Ports.env: Arc<dyn Env>`, 50 sites change. (c) **stdout/stderr of `Output::Owned` commands and `eprintln!`**: `run_with(&Ports, argv, out: &mut dyn Write)` captures the JSON; the `eprintln!` sites are not captured (guess: 40, unverified count) and are left to the test's stderr. (d) **fd 3 and `sigaction`**: `lock::inherited` and `catch_signals` are called by the mains, not by `run`, so `run_with` skips them (the driver's `hand_over` becomes `Launch.hand_over`, the engine's stop flag becomes a parameter `until: &AtomicBool`).

### 4.5 `run_with` and the thin mains

```rust
pub fn run_with(ports: &Ports, argv: impl IntoIterator<Item = OsString>, out: &mut dyn Write) -> ExitCode
pub fn main_with(argv, out) -> ExitCode { run_with(&Ports::real(), argv, out) }   // kept: old name
```
`main.rs` stays three lines: `record_start`, `catch_signals` (only for `watch`, where it is today), `main_with`. Binaries: `cpd` (real ports) and `fake-tmux` (feature `fakes`, today `cli/src/bin/fake-tmux.rs`, deleted by the tmux PRs). For the smoke tests: `--help` and `--version` use the real `cpd` (no external); a second bin `cpd-fake` (`required-features = ["fakes"]`, main with `Fakes`) would carry an engine end to end, gated like `fake-tmux` today (guess: needed by at most 2 smoke tests; unverified; the alternative is none and the smoke set is only `--help`, `--version`, an unknown command, a missing home).

## 5. The scripts (`rs-*`)

Rust lives in `cpd-core` (logic) and `cli/src/<area>.rs` (clap), over `Ports` with `Runner` for git/gh/cpd and `fs` for files. Each new command updates `docs/commands.md` and the command-tree test `cli/tests/tooling/interface.rs`.

### 5.1 `install-cpd.sh` → `cpd install` (rs-install; 51 tests)

What the script does, in order: fetch `origin/main` and refuse when a migration of this build is not on it (git `fetch`, `cat-file`, `show`); require python3; build release with `CPD_COMMIT` (`scripts/cargo.sh`); run the **built** binary with `--version` and check its store verdict, then open the store (`cpd` as a child); atomic replace of `~/.local/bin/cpd` (`mv -f`); adapter swap (symlink `pi -> pi.<id>` by one `rename(2)` through python3 `os.replace`; migrates a real directory to `pi.legacy`; removes old `pi.*` older than 60 min and dead `pi.tmp.<pid>`); then follow-ups on the new binary: `cpd config`, `watch --check`, `tui --respawn`, `watch --mark-restart`, `tmux kill-window <work>:watch`, `watch --ensure`, a check that an engine holds the lock (python3 `fcntl`); finally `scripts/tag-version.sh` (a failure only warns). **Stays in shell:** `cargo build --release --locked` with `CPD_COMMIT`, then `exec target/release/cpd install --root <checkout>` (the build is the bootstrap; unverified that nothing else must run before). **Rust:** the migration check (Runner git), the verdict check (Runner on the built path), the binary replace (`rename`), the adapter swap (`std::fs::rename` of a symlink over a symlink is atomic on Linux and macOS; unverified on macOS, the incident hs-d355 was `renameat2`), cleanup (`Spawn::alive` for scratch pids), the lock check (`cpd_core::lock`, no python3), the follow-ups (Runner on the installed path), the tag (`cpd tag`, in-process). `scripts/check-portable.sh` keeps guarding the remaining shell (its 4 install-script tests stay).
Tests port to `Fakes`: Runner routes for `git`, `cpd` (the built and the installed one), `tmux`; a temp `HOME`; the 51 tests keep their names. The "recording `tag-version.sh`" helper (`install_script.rs:545`) becomes a call count on the in-process tagger.

### 5.2 `prune-worktrees.sh` → `cpd prune` (rs-prune; 5 tests)

Uses `git` (13 calls), `gh pr`, `jq` (3) and `cpd plan` (4) to ask which branches cpd still runs. In Rust the protected set is a store query (no `cpd` child, no `jq`); `git` and `gh` go through the Runner. The script's header calls itself "one-time cleanup" (hs-d138..d144): **ask the Lead whether it is still used before porting** (delete first; I found no caller outside its tests and docs, `rg`).

### 5.3 `tag-version.sh` + `sync-version.sh` → `cpd tag`, `cpd version sync [--print]` (rs-tag; 7 + 12 tests)

`tag-version`: git only (`fetch`, `show origin/main:Cargo.toml`, `ls-remote`, `log -G`, `tag -a`, `push`), one idempotent algorithm, every failure a `tag-version:` line: a pure function over a git Runner. `sync-version`: reads `PROTOCOL` from `core/src/events/kind.rs`, counts migrations, rewrites four crates in `Cargo.toml` and `Cargo.lock`: file work, no process; callers are `scripts/new-migration.sh` and `cli/tests/tooling/version.rs`. Both scripts shrink to `exec cpd …` where a built cpd exists (they are dev tools; `new-migration.sh` calls `sync-version.sh`: it becomes `cpd version sync`).

### 5.4 `tests.sh`, `test-changed` (rs-tools; 6 + 17 tests)

Their tests run real `cargo`, `nextest`, `tsc`, `node` in scratch workspaces: externals. Selection logic (changed files → crates → reverse deps → `-p`/`--test`/`-E`) is ours and pure over `git diff` and `cargo metadata` output (Runner routes with canned JSON); name classification (`pi/test/<file>::<title>` vs a Rust name) too. `scripts/cargo.sh` stays shell (cargo is not on PATH; it is the bootstrap, and its sccache and slot logic is shell tested by `cargo_script.rs`, 12 tests, which fake `cargo` as a script: guess: it is a shell script under test, so it stays an exec test of `sh`, allowed by ADR? unverified, ask). `npm-fresh.sh` (4 tests), `preview-tui.sh` (11), `test-time.sh`, `check-portable.sh`, `pi-check.sh`, `tripwire/`: stay shell (guess: they are shell-only on purpose; unverified).

### 5.5 `gate-run.sh` → shell wrapper + `cpd gate finish` (rs-gate-run; 10 tests)

`npm run verify` = `scripts/gate-run.sh npm run verify:steps` (`package.json:14`, pinned by `pi/test/gate.test.ts:10`). The script makes the run directory and `GATE_RUN`, runs the command, then lists leftovers (processes with `GATE_RUN` in their environment: `/proc/<pid>/environ` on Linux, `ps -axww -E` on macOS; tmux sockets named with the id; paths under the run's TMPDIR), kills and removes only those, prints `gate-run: FAIL` and exits 1. Proposal: the shell keeps `mktemp`, the exports and running the command, then `exec <target/*/cpd> gate finish --run <id> --status $?`; the check is a function over `Spawn` (a new `Spawn::with_env(key, value) -> Vec<Pid>` replaces the two OS scans) and `fs`. **Risk:** if the build in `verify:steps` failed there is no cpd: the wrapper then returns the command's status as today and skips the check (unverified: today a failed build still runs the check, `gate-run.sh` header). Decision for the Lead: accept that, or keep the scan in shell and port only the report.

### 5.6 Jobs and tests per script (the `rs-*` shape)

Each `rs-*` PR is `red` (the ported tests on `Fakes`, failing for the right reason against stubs) then `code` (the module and the shrunken script) then, when the script is a bootstrap, `doc` (`docs/implementing.md`, `contract.md §3.7` text that names the script).

## 6. Proposed PRs and Jobs

Tier is my estimate (unverified). Kinds: `red` tests + stubs, `code`, `doc`. Max 8 Jobs a PR.

### 6.1 `ports` (needs `clk`; `port-run`, `port-tmux`, `port-git` run in parallel)

| job | kind | tier | needs | scope |
|---|---|---|---|---|
| ports-1 | red | S3 | | tests of `Ports::real`, `Fakes`, `FakeSpawn` (launch log, signals, exit, `alive`), `Clock::utc`; an in-process `run_with(["cpd","--version"])` test; stubs |
| ports-2 | code | S3 | 1 | `Spawn`, `ProcessSpawn` (one `alive`, `signal`, `exe_of`, `cmdline`, `exec`, `stream`), `FakeSpawn`; delete the eight `alive` sites |
| ports-3 | code | S3 | 2 | `Ports`, `Env.ports`, `run_with`/`main_with`, `Clock::utc`, `self_exe` replacing `STARTED_AS`; mains stay thin |
| ports-4 | code | S3 | 3 | driver and session: `driver/start.rs`, `stall.rs`, `session/run.rs` (`PiProc`), `reload.rs`, `host/exe.rs exec`, `git/snapshot.rs`, `tmux.rs` pid facts; clock for their loops (§2) |
| ports-5 | code | S3 | 3 | engine: `watch/{distill,titles,restart}.rs` spawns, `escalations.rs`, `run(ports, until)` for the stop flag, `Engine::for_test` replaced by a `Ports` constructor, clock for the engine loops |
| ports-6 | code | S3 | 3 | tui: `RunCommand` and `Restart` become `Arc<dyn Fn>` with ports, `cockpit.rs exec_attach`, `Gpus`, terminal and event source injected for `TestBackend`, clock for the view loops |
| ports-7 | code | S3 | 3 | rest of core loops (§2 last row) |
| ports-8 | doc | S2 | 4-7 | `docs/implementing.md` Tests and ports sections, `contract.md` text on `STARTED_AS` |

### 6.2 New: `port-run2` (not in the brief; needs `port-run`)

`pr2-1` red (S2: fake `gh`/`pi`/hook replies, a hung `gh` by `took`), `pr2-2` code (S3: `gh.rs run_timed`, `distill.rs`, `retitle.rs`, `exe.rs asks_to_open_store`, `plan/lock.rs`, `tmux.rs installed_version`, `inbox/cmd.rs`), `pr2-3` code (S3: `git/hooks/exec.rs` over one Runner call per hook on a thread each; behaviour change: a hook's output arrives when it ends, not line by line, unverified that nothing depends on the order), `pr2-4` code (S2: tui `actions.rs`, `widgets/host.rs`, `hostname`, `restart.rs rebuild_cockpit`).

### 6.3 `testkit` (needs `ports`, `port-tmux`, `port-git`)

`tk-1` code S3: `Fx` in `cli/tests/common` (temp home and config, env and cwd set at start, `cpd`/`cpd_in`/`cpd_with` via `run_with`, `Run { code, stdout, stderr }`; the same names as `TestEnv`, so the migrations are a rename). `tk-2` code S3: `fake(name, body)`, `fake_gh`, `fake_pi`, `calls(name)` as Runner routes (124 uses). `tk-3` code S3: git fixtures over the fake git. `tk-4` code S3: tmux fixtures (`tmux_state`, panes, `fake_cpd_views`). `tk-5` code S3: virtual-time helpers replacing `wait_until`, `poll`, `wait_some`, `wait_screen`. `tk-6` code S3: `TestBackend` and scripted-key helpers. `tk-7` red+code S2: the guard test, §6.4. Red tests: each helper has a test first (tests are the proof), so each `tk-n` is one Job with its own tests (guess: red and code in one Job is acceptable for helpers; the Lead decides).

### 6.4 The guard (Prose is not enforcement)

One test in cpd-core that scans product sources for `Command::new`, `std::process::Command`, `thread::sleep`, `Instant::now`, `SystemTime::now`, `now_utc`, `process::exit`, `exec(` and fails outside an allowlist of files (the real adapters `host/deadline.rs`, the process spawn, `clock.rs`, the mains). It replaces `cli/tests/session/host.rs:820` and follows the pattern of `core/src/env.rs the_open_sequence_is_written_once`. It lands last (`lint-gate`), with the allowlist at its real size then, because it cannot pass before the last site moves.

### 6.5 `rs-*` and `smoke`

| PR | Jobs | needs |
|---|---|---|
| rs-install | rsi-1 red S3 (51 tests); rsi-2 code S3 (migration and verdict checks, binary replace); rsi-3 code S3 (adapter swap, cleanup); rsi-4 code S3 (follow-ups, lock check, tag call); rsi-5 code S2 (shell shrinks, `check-portable`); rsi-6 doc S2 | ports, testkit, rs-tag |
| rs-tag | rst-1 red S2; rst-2 code S2 (`cpd tag`); rst-3 code S2 (`cpd version sync`, `new-migration.sh`) | ports, port-git |
| rs-prune | rsp-1 red S2; rsp-2 code S2 (after the Lead says it is still used) | port-git, port-run |
| rs-gate-run | rsg-1 red S2; rsg-2 code S3 (`gate finish`, `Spawn::with_env`); rsg-3 code S2 (wrapper, `gate.test.ts` unchanged) | ports |
| rs-tools | rtl-1 red S3; rtl-2 code S3 (`test changed`); rtl-3 code S2 (`test run`) | ports, port-git, port-run |
| smoke | sm-1 red+code S2 (`--help`, `--version`, unknown command, missing home with the real `cpd`; `assert_cmd` or `Command` on `CARGO_BIN_EXE_cpd`: `assert_cmd` is not a dependency, and the brief says take upstream: take it pinned if there are 5 or more tests, unverified); sm-2 S2 (one end-to-end on `cpd-fake`, optional) | ports |

### 6.6 Migration order (the `mig-*` PRs), by seams, then by execs removed

`env.cpd*` calls are the cost (each is a process start of the real binary): F 724, S 243, T 98, P 88, E 23, O 18, X 6. Tests per area: F 531, T 357, P 317, S 166, E 131, O 110, X 263; 213 deleted tests to restore (`tests-mock-deleted.md`: P 94, F 65, E 50, S 2, T 2).
1. **store** (110) and the flow files that start no external (spec, policy, plan outline/show/render, metrics, secret guard, repo config): need only `ports` + `tk-1`. Biggest early win per effort: it proves `Fx`.
2. **pr** (317 + 94 restored): `port-git`, `port-run`/`port-run2`, `tk-2`, `tk-3`.
3. **flow** rest (about 400 + 65): pi/gh/node/tmux/driver routes, `Spawn` for `attempt run`; the largest exec count.
4. **session** (166): tmux + `Spawn` + clock (`ports-4`, `tk-4`).
5. **engine** (131 + 50 restored): everything, plus `tk-5` virtual time; do after `ports-5`.
6. **tui** (357): `ports-6`, `tk-6`; the R class (133) is already unit-shaped (`tests-mock-design.md §2`).
7. **tooling** (263): after the `rs-*`.

## 7. Open decisions for the Lead

1. Add `port-run2` (§6.2), else the Runner leaves 14 real waiters. 2. Environment: option 1 (set at test start) or `Ports.env` (§4.4). 3. Route a launched `cpd` child to `run_with` synchronously (§4.2). 4. `cpd-fake` bin for smoke (§4.5). 5. Is `prune-worktrees.sh` still used (§5.2)? 6. `gate-run` when the build failed (§5.5). 7. Do `cargo.sh`, `preview-tui.sh`, `npm-fresh.sh` stay shell and keep exec tests of `sh` (§5.4)? The rule says tests test only our code; they are our code, in shell. 8. Feature name `fakes` (Plan) vs `testkit` (ADR).
