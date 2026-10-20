# cpd 2: implementing

Read this first, then [contract.md](contract.md), [commands.md](commands.md) and [CONTEXT.md](../CONTEXT.md), which are normative. This file only says where things go in the code.

## The one-area rule

Each Job edits **only its own area** (`core/src/<area>/`, or `tui/src/` for the cockpit) **plus its tests** (`cli/tests/<binary>/<area>*.rs`, where `<binary>` is the one the area table under Tests gives the area). It does not edit `core/src/lib.rs`, `cli/src/cli.rs`, the shared modules (`core/src/error.rs`, `output.rs`, `ids.rs`, `args.rs`, `home.rs`), `cli/tests/common/`, another area's tests, another area, any `Cargo.toml` or `Cargo.lock`. This is how parallel Jobs merge without conflicts. Need a change outside your area: ask the Lead (`cpd ask`), don't make it.

Within your area you may add files (`core/src/<area>/foo.rs`, declared in your `mod.rs`) and use any other area's public items once that area has landed.

## Layout

- `Cargo.toml`: the workspace, with three members (edition 2024). Dependencies point one way: `cli` depends on `core` and `tui`; `tui` depends on `core`; `core` depends on neither.

| Directory | Package (library / binary) | Holds |
|---|---|---|
| `core/` | `cpd-core` (library `cpd_core`) | every area of the module map below except `tui`, and the shared modules (`args`, `error`, `home`, `ids`, `output`). `core/src/lib.rs` is the crate root: it declares every module, once. |
| `tui/` | `cpd-tui` (library `cpd_tui`) | the whole cockpit (`tui`, `tui view`, commands.md §13): `tui/src/lib.rs`, the views and the read model (`tui/src/read/`). It uses `cpd_core` for the store, events, host and the shared modules. |
| `cli/` | `cpd` (binary `cpd`) | `cli/src/main.rs`, which only calls `main_with`, and `cli/src/cli.rs`: the root `Command` enum, `run` and `main_with`. Also `cli/tests/`, the binary-driven integration tests: one test binary per area, `cli/tests/<area>/` (the area table under Tests). |

**The command runner.** The cockpit runs commands in-process (a click runs `session open`, the new-Plan key runs `plan new`) through the root command tree, which `cli` owns and `tui` cannot name (`tui` does not depend on `cli`). So `cli` passes the runner to the cockpit explicitly: `cpd_tui::run` takes a function `fn(Vec<OsString>) -> Result<Output, CpdError>` (`cli::run`), and the cockpit calls it wherever it runs a command. `Output` and `CpdError` are `cpd_core`'s.

### Module map

| Module (`core/src/<module>/`; `tui` is `tui/src/`) | Owner (Job) | Commands (commands.md) | Contract |
|---|---|---|---|
| `store` | rs-store | none | §4.1–§4.5 |
| `events` | rs-events | `events list`, `events follow` (§11) | §5 |
| `config` | rs-config | `config`, `tiers` (§14.2, §14.3) | §8, §9 |
| `git` | rs-git | none | §7.1–§7.3, §7.5 |
| `host` | rs-host | none | §6 |
| `context` | rs-context | `context …` (§3) | §2.2, §4.6 |
| `session` | rs-session | `session …` (§10) | §3.6, §6 |
| `plan` | rs-plan | `plan …` (§4) | §3.1 |
| `job` | rs-job | `job add/edit/cancel/retry/list/show` (§5) | §3.2 |
| `merge` | rs-merge | none | §7.4 |
| `attempt` | rs-attempt-cmds | `attempt submit/read/show/list` (§6.1–§6.3) | §3.4, §7.3 |
| `ask` | rs-ask | `ask`, `answer` (§8) | §3.5 |
| `spec` | rs-spec-metrics | none: the Worker Spec (§16.1) | §3.4 A3 |
| `metrics` | rs-spec-metrics | none: session metrics | §2.2 Attempt |
| `driver` | rs-driver | `attempt run` (§6.4), `job dispatch` (§5.5) | §3.4 |
| `pr` | rs-pr | `pr …` (§9): `pr open`, `pr poll`, `pr rebase`, and since R34 `pr add`, `pr edit`, `pr cancel`, `pr start` (§9.4–§9.7); owns the tables `prs` and `pr_needs`, the PR selection by `--pr` (§1.3) and the PR start (contract §3.8, §7.2), which `plan lock` and engine E7 call | §2.5, §3.8, §7.2, §7.6 |
| `watch` | rs-watch | `watch` (§12) | §3.7 (E1–E9) |
| `inbox` | Plan inbox | `inbox list/show/snooze/dismiss` (§21) | §2.2 Inbox item, §3.7 (E8) |
| `doctor` | rs-doctor | `doctor` (§14.1) | |
| `tui` | the TUI Job | `tui`, `tui view` (§13) | §6.2 |

`attempt run` and `job dispatch` are declared in `attempt` and `job` but their argument types (`driver::RunArgs`, `driver::DispatchArgs`) and functions (`driver::run`, `driver::dispatch`) are the driver's, so the driver Job never edits those modules.

**The distilled Context** (contract §4.6, hs-d194) lives in the `context` area:
- `core/src/context/inforce.rs`: the records in force as of a `seq` (D2), pure over the records read in one transaction;
- `core/src/context/digest.rs`: the digest files (find the latest, write `.md.tmp`, rename, prune), the validation (citations, size; D3) and the one rendering of "the Context" for a first message (D6), which `plan new` (`core/src/plan/`) and `context manager` call;
- `core/src/context/distill.rs`: `context distill` (D4, D5): the lock, `prompt.md` and `records.md`, the pi run and its timeout, the `error` Event;
- `core/src/context/distill_prompt.md`: the distiller's prompt, embedded with `include_str!`;
- engine E9 in `core/src/watch/distill.rs` (it only decides and spawns, as E3 does);
- paths through new `CpdHome` methods (`context_digest_dir(context)`, `distill_lock(context)`), and `context_distill_tier` and `timeouts.distill_minutes` in `config`: shared modules and other areas, so the Job that builds it asks the Lead for those edits (the one-area rule).

## The command tree

`cli/src/cli.rs` holds the root `Command` enum; each variant delegates to its area (`cpd_core::context::run(cmd)`, `cpd_core::watch::run(args)`, …, and `cpd_tui` for the cockpit). Each area owns its subcommand enum and its argument types. Every command of commands.md §2 is implemented, and `cli/tests/tooling/interface.rs` keeps docs/interface.md, the index of commands.md §2, equal to `cpd --help`'s tree, both ways, and every command in `cli/src/interface.rs`'s table.

To add a command:
1. In your module, define its arguments as a `#[derive(clap::Args)]` struct, and name it from the root or the area's subcommand enum (`watch::Args`, `config::ConfigArgs`, `driver::RunArgs`, …).
2. Make `run` return `Ok(Output::success(json!({…})))` or `Err(CpdError::…)`.
3. Test it in your own test file.

Parsing:
- `--help` and `--version` print one JSON value too: `{"ok": true, "help": "<text>"}` and `{"ok": true, "version": "…", "schema": 1000, "store_schema": <1000, 1001 or null>, "protocol": <P>, "store": <the verdict or null>}`. `store` is what `store::verdict` answers (contract §4.5, commands.md §1.2): `{"opens", "refusal", "unknown", "pending"}`, from `$CPD_HOME`'s database opened read-only, without migrating and without the schema check (`null` when the file does not exist). `schema` and `store_schema` are only the projection the integer-era guards still read (always `1000`; `null`, `1000` when the store opens, `1001` when not) and go away in a later change (hs-d178 D1). No guard compares numbers: they read `store`. `--version` never creates, migrates or writes the database.
- Every clap failure is `usage` (exit 2) with clap's text in `details.help`, except a value that failed one of our own `FromStr` types (ids, `Text`, `Name`), which keeps that type's code (`invalid_plan_id`, `invalid_name`, …). So parse ids by typing the field (`plan: PlanId`), not by validating strings later.

## Output and errors

- **`output::Output`** (`core/src/output.rs`): `Output::success(json!({…}))` prints `{"ok": true, …fields}`, with `"ok"` first. `Output::Owned` prints nothing and exits 0: only for the commands that write their own stdout or own their terminal (commands.md §1.2, amendment 10): `events follow`, `session run`, `tui view`, `tui --attach`. Their failures before taking the terminal are still returned as `Err`. `output::to_line` is one JSON value plus a newline, for `events follow`'s lines.
- **`error::CpdError`** (`core/src/error.rs`): `CpdError::new(ErrorCode::CasLost, "One sentence.").with("attempt", id.to_string())`. Printed as `{"ok": false, "error": {"code", "message", "details"}}`; `details` is always an object. `to_json()` gives the `error` object alone, the shape of an `error` Event's data (add `source` there).
- **`error::ErrorCode`**: every code of contract.md and commands.md, each with its exit code (`Exit`: 1 failure, 2 usage, 3 refused, 4 not found, 5 timeout). A code cannot be printed with the wrong exit.
- **Adding an error code:** the documents name every code; use those. If you truly need a new one, ask the Lead first: it is an amendment to the contract, and `core/src/error.rs` is shared. The change is one line in the `error_codes!` list, under its exit code.

## Shared helpers

- **`ids`** (contract §2.1): `ContextId`, `PlanId`, `JobKey`, `AttemptId` (`job()`, `number()`), `EscalationId` (`attempt()`, `number()`), `ReviewId`, `SessionId` (`SessionId::new_v4()` for sessions cpd starts), `MachineName` (`config_invalid` when wrong), `RecordId` and `RecordKind` (`letter()`, `as_str()`). Each has `parse`, `FromStr`, `Display` and serde as a string; holding one proves it is valid.
- **`args`** (commands.md §1.3, §1.5): `PlanArg` (`--plan`, else `CPD_PLAN`; `.require()` gives `plan_required`), `SessionArg` (`--session`, else `CPD_SESSION`; `session_required`; it and every positional Session argument are resolved to a `SessionId` by `session::resolve`, below), `Text` (1–20 000 characters, no NUL; else `usage`) and `Name` (1–80, no control characters; else `invalid_name`). Flatten them into your clap structs. `--context` falls back to the selected Plan's Context, which needs the store: the context Job writes that resolution.
- **Short Session ids** (commands.md §1.3, hs-a3): the resolution (full id, Plan id, `<plan>·<prefix>` / `<plan>.<prefix>`, bare prefix of at least 4 characters; `session_not_found`, `session_ambiguous`) lives in one place, `core/src/session/resolve.rs` (the `session` area, which reads the store), called at the boundary by every command that takes a Session, so no command compares raw strings and downstream code only holds a `SessionId`. `args::SessionArg` and the positional `<id>` of the session commands stay strings until then; the cockpit and the pi adapter keep passing full ids.
- **`home::CpdHome`** (contract §1.4, commands.md §1.4): `CpdHome::from_env()` is `$CPD_HOME`, else `~/.cpd`; an empty or relative `CPD_HOME`, or no home directory, is `usage` (exit 2). Every path under it has a method: `config()`, `db()`, `context_dir()`, `watch_lock()`, `attempt_lock()`, `context_lock()`, `watch_log()`, `plan_md()`, `plan_review_md()`, `lead_message()`, `session_message()`, `worker_spec()`, `review_brief()`, `review_diff()`, `review_check()`, `review_gate()`, `driver_log()`, `hook_log(…, Hook)`. Never build these paths by hand.

## Migrations

The schema changes by named migrations (contract §4.5; design `docs/research/migrations.md` as amended by hs-d178). To add one:
1. `scripts/new-migration.sh <slug> additive|breaking` writes `core/src/store/migrations/<YYYYMMDDTHHMM>-<slug>.sql` with its header (UTC minute; a breaking one gets `after <n>`). No shared list is edited: the build step lists the directory. A migration that needs Rust beside its SQL has a `<name>.rs` with `pub fn run`.
2. Write the SQL. **Additive** only when every statement is one of contract §4.5 A1's (new table plus its `history_tables` row, a non-unique index, a trigger on a table it creates, a nullable or constant-default `ADD COLUMN`, an `INSERT` into `event_kinds`, `record_kinds` or `history_tables`). Everything else, and any value old code parses into a closed type (a record kind), is `breaking`. Never create `T_hist_ins`, `T_hist_upd` or `T_no_delete`: the runner generates them (K4).
3. Adding an Event kind is `INSERT INTO event_kinds (name) VALUES ('<kind>');` plus `core/src/events/kind.rs`, `pi/lib/event.ts`, contract §5.2, and `PROTOCOL` raised in both.
4. Run `scripts/repin.sh` to write `SCHEMA_HASH` (contract §4.3; it also resolves a merge conflict on it) and amend contract §4.4 in the same change; `cargo test` fails with the new value when it is stale. The additive, order and orders tests (§4.5) run in the same `cargo test`.
5. A migration file on `main` is never edited, renamed or deleted. A breaking migration whose `after <n>` no longer holds after a merge of `main` is renamed to a newer timestamp (`git mv`) and its rebuild re-checked.
6. Tests build stores with `TestEnv` (`cli/tests/common/mod.rs`); never touch the real `~/.cpd`. Converting a User's `~/.cpd` is a Lead step with the User's go.

## Tests

- **Unit tests** go in the module they test (`#[cfg(test)] mod tests`), in your area, in `core/` or `tui/`.
- **Integration tests** go in `cli/tests/<area>/<module>.rs` (or `<module>_<topic>.rs`), whichever crate the area is in: they run the built `cpd` binary. There is **one test binary per area**, `cli/tests/<area>/main.rs` (docs/research/workspace-split.md §4.2; docs/research/test-binaries.md): its `mod` lines are the area table below, the one source of truth, and a test checks that every `.rs` under `cli/tests/` (but `common/`) is in exactly one binary. A new file is declared in its area's `main.rs` with `mod <module>;`, and a test's name is `<module>::<test>` in the binary `cpd::<area>`. A file starts with `use crate::common;` and uses the harness `cli/tests/common/mod.rs` (each `main.rs` has `#[path = "../common/mod.rs"] mod common;`):
  - `TestEnv::new()`: a temporary directory with its own `$CPD_HOME` (`cpd_home()`, empty), `$HOME` (`home()`, with a git identity), and `bin/` first on `PATH`; `CPD_*`, `PI_*`, `GIT_*`, `XDG_*`, `TMUX` and `TMUX_PANE` are removed from the environment.
  - `env.cpd(&[…])` / `env.cpd_in(cwd, &[…])` run the built binary and return a `Run` (`exit`, `stdout`, `stderr`); `run.json()` asserts exactly one JSON value, `run.ok()` the success envelope, `run.error()` the failure's `error` object.
  - `env.git_repo("name")`: a repo on `main` with one commit; `env.git(dir, &[…])`.
  - `env.tmux(&[…])` and `env.tmux_socket()`: a private tmux server (own `TMUX_TMPDIR` and socket name, killed when the `TestEnv` drops). Put the socket name in the test's `config.toml` as `tmux.socket`.
  - `env.fake_pi(body)`, `env.fake_gh(body)`, `env.fake(name, body)`: shell scripts on `PATH` that record their argv (`env.calls("pi")`) and then run `body`.
  - `env.command(program)`: any other program in the same environment.
- **The area table** (the binaries of `cli/tests/`; a module is in the binary whose row matches its file name):

  | binary (`cli/tests/<area>/`) | modules |
  |---|---|
  | `engine` | `watch*`, `doctor` |
  | `pr` | `pr*`, `merge`, `git*` |
  | `flow` | `plan*`, `job`, `attempt`, `driver*`, `review*`, `spec`, `ask`, `metrics`, `context*`, `policy`, `config` |
  | `session` | `session*`, `host` |
  | `store` | `store*`, `events` |
  | `tui` | `tui*`, `inbox*`, `bare` |
  | `tooling` | `*_script`, `cargo_script`, `npm_fresh`, `install_script`, `harness`, `dead_code`, `interface`, `no_repo_lock`, `slot`, `e2e*`, `layout`, `version` |

- **A Worker's targeted build is `--test <area>`**: it builds and links only that area's binary (`scripts/cargo.sh nextest run -p cpd --test flow plan_cancel::`, or `scripts/cargo.sh build -p cpd --test flow`), not every area's. Run the binary of the area you changed, and with a module or test name after it to run less (the Gate runs all of them). A change to `cli/tests/common/` is every binary's: say so in the Spec's acceptance and run the binaries it touches.
- The harness needs the `sqlite3` CLI on `PATH`: its teardown reads the env's store for the drivers' pids (the first `TestEnv::new()` panics without it).
- Never touch the real `~/.cpd`, the tmux server `-L cpd`, pi, or the network.
- nextest runs every test in a process of its own, so a `static` or a lock that the tests of a file share coordinates nothing there (under `cargo test` they share one process): do not write one. Never `set_var`/`remove_var` in a test (`harness.rs` fails on it): put the variable on the `Command` (`env.command(..).env(..)`), or `Tmux::with_tmpdir` for tmux. Write and copy executables with `common::write_executable`/`copy_executable` (ETXTBSY, `harness.rs` checks it). Tests that must not overlap go in a test group of `.config/nextest.toml` (`max-threads = 1`, the reason beside it; select a file's tests with `binary_id(cpd::<area>) and test(/^<file>::/)`). A test that runs its own binary again (`std::env::current_exe()`) passes the full name, `"<file>::<test>"`, to `--exact`.
- A test must fail first, for the right reason.
- **Speed (hs-d284).** The target on hs (16 CPUs) is `scripts/cargo.sh nextest run --workspace`, build excluded, in at most **30 s on the clock** and **300 s of test time added up**, with every test kept: a test is never skipped, filtered or weakened to be fast. The sum matters as much as the clock: 300 s over about 1,900 tests is about 0.15 s per test. A test that waits for a real limit gets a short limit through the product's own configuration or a test override the contract names (`docs/contract.md` §1.2), never a weaker assertion, and never a fixed sleep to get a result; waits poll a positive signal.
  - **Measure with `scripts/test-time.sh`** (`scripts/test-time.sh -E 'binary_id(cpd::flow)'` for a subset): it prints the load, the wall (nextest's run time) and the sum, the 30 slowest tests and each binary's sum. Put the wall and the sum before and after in the commit message of a speed change; a change without a measured gain is not made.
  - **Measure paired.** hs is shared, and the same suite's sum moves 10-75 % with the load (the load average is in the report: say it). Compare two builds in the same conditions, interleaved (A, B, A, B, ...), never one run before and one after at different hours; for one test, run it alone 5-7 times per build (`scripts/cargo.sh nextest run -p cpd --test flow <name>`) and compare medians. Keep both builds as commits or a patch file (`git diff > /tmp/x.patch`, `git apply -R`), never `git stash`. No synthetic load, at most 2 test runs at once.
  - **The per-test limit** is `slow-timeout` in `.config/nextest.toml` (the default profile): a test over it fails the run. The numbers it comes from and the reason are beside the setting; raise it only with a measurement. The `priority` override there lists the tests that wait for a real limit, so they start first; `=name` drops out silently when a test is renamed, so check it with `scripts/cargo.sh nextest list --workspace -E '<the filter>'`.

## Dependencies

Every dependency is declared once, in the root `Cargo.toml` `[workspace.dependencies]`, and used with `x.workspace = true` in the `Cargo.toml` of each crate that uses it (`core/`, `cli/`, `tui/`); a crate declares only what its own code imports. `ratatui` and `tui-tree-widget` are declared only in `cpd-tui`; `tempfile` is a dev-dependency of the crates whose tests use it. **Later Jobs may not add, remove or change dependencies or features**, so parallel Jobs never conflict on `Cargo.lock`. The TUI Job (`tui-cockpit`) added the last planned ones, ratatui and tui-tree-widget. If you need something not listed, ask the Lead.

| Crate | Features | Why |
|---|---|---|
| `rusqlite` 0.40 | `bundled`, `functions` | the database (§4.1): SQLite bundled into the binary (3.53, so `STRICT` and JSON functions exist); `functions` registers `cpd_txn()` (§4.3) |
| `serde` 1 | `derive` | typed parsing at the boundary: config, Specs, records, session lines, `gh` output |
| `serde_json` 1 | | JSON output (§1.2), Events' `data`, Context records (§4.6), pi session files, canonical JSON for fingerprints (§2.3: its default map is sorted, so keys come out sorted; do not enable `preserve_order`) |
| `clap` 4 | `derive`, `env` | the command tree; `env` for the `CPD_PLAN` / `CPD_SESSION` fallbacks (§1.3) |
| `toml` 1 | | `config.toml` (§8), `.config/wt.toml` (§7.3), `.config/cpd.toml` (§7.5); use serde `deny_unknown_fields` |
| `minijinja` 3 | | rendering the hooks' `{{ … }}` / `{% … %}` templates (§7.3), with `UndefinedBehavior::Strict` so an undefined variable or unknown filter fails before anything runs (`hook_invalid`) |
| `nix` 0.31 | `fs`, `process`, `signal` | `flock(2)` locks (§1.4, `Flock`; it derefs to the `File`), the driver lock's fd hand-over (§3.4: clear close-on-exec with `fcntl`, `dup2` to fd 3, `setsid`), probing pids (`kill(pid, None)`, E1), killing a hook's process group at its timeout (`killpg`, §7.3), SIGTERM/SIGINT for `watch` (§12) |
| `uuid` 1 | `v4` | Session ids cpd chooses (§2.1); use `ids::SessionId::new_v4()` |
| `time` 0.3 | `formatting`, `parsing`, `macros` | RFC 3339 UTC times with milliseconds (§1.3) |
| `sha2` 0.10 | | the Locked-Job and plan fingerprints (§2.3) and the schema hash (§4.3); `format!("{:x}", Sha256::digest(…))` gives hex (0.10, not 0.11, for that) |
| `ratatui` 0.30 | default (its `crossterm` backend) | the cockpit's views (`tui view`, commands.md §13.2), as on the spike's branch `cpd/cpd-ui` (history); crossterm is used through its re-export `ratatui::crossterm`, so it is not a dependency of its own |
| `tui-tree-widget` 0.24 | | the `tree` view's collapsible Context → Plan → Session tree, as on `cpd/cpd-ui` |
| `tempfile` 3 | dev only | temporary `$CPD_HOME`, repos and files in tests |

Removed: `sha1` 0.10, which named the per-repo lock `repo-<sha1>.lock`; hs-d243 removed the lock (contract §1.4, §7), and with it `CpdHome::repo_lock` and the dependency (the one exception to the rule above, decided with the lock's removal).

Deliberately absent: a regex crate (the id patterns of §2.1 are checked by hand in `ids`), an error-derive crate (`CpdError` is one struct), a hex crate (see `sha2`), `libc` (`nix` covers it), crossterm as a direct dependency (see `ratatui`).

## Running cargo, the Gate, installing

- cargo is not on `PATH` for Workers and the Gate. Run it as `scripts/cargo.sh <args>` (`$CARGO`, else `cargo` on `PATH`, else `${CARGO_HOME:-$HOME/.cargo}/bin/cargo`; exit 127 with a message when none exists). rustfmt and clippy are not installed.
- The Gate is `npm run verify`: the typecheck and tests of the pi adapter `pi/`, then `scripts/cargo.sh build --workspace` and `scripts/cargo.sh nextest run --workspace` (it sets `CPD_COMMIT`, see "The version string" below). **One build, once dl-config confirms it loses nothing** (Plan quality): nextest's test build already builds every binary the tests run, `cpd` included, so the separate `build --workspace` builds the workspace a second time (from a clean target directory: 39 s, then 38 s, docs/research/build-cycle.md §2.1) and the Gate drops it. This holds only when the step is gone from `package.json`; until then the Gate builds twice. The quick check (the `pre-commit` hook of `.config/wt.toml`) keeps its own `typecheck` and `build --workspace`. Run it before you submit. CI's `rust` job runs the same command.
- The Rust tests run with [cargo-nextest](https://nexte.st), not `cargo test` (hs-d195). `cargo test` runs the test binaries one after another, and a binary spends most of its time waiting (tmux, the engine's tick, fixed sleeps), so the machine idles while one long binary ends; nextest runs the tests of every binary at once, each in a process of its own. Measured on 16 cores under other Workers' load (load average 20–38), 1307 tests: `scripts/cargo.sh test --workspace` 359 s; `scripts/cargo.sh nextest run --workspace` 105, 123 and 97 s in three runs, all green (the research, starting the binaries 4 at a time: 54 s against 175–200 s serial, `docs/research/quality-tests-build.md`, tests-build-F1). That gain is the reason for not using `cargo test`. `.config/nextest.toml` is nextest's defaults plus the test groups (see Tests). dotfiles installs it on every machine, at the version pinned in its `home/.chezmoidata/nextest.yaml`. The Gate runs it (no GitHub Actions, hs-d283).
- Run one test file or one test with `scripts/cargo.sh nextest run -p cpd -E 'test(/^host::/)'` (a file's tests) or `scripts/cargo.sh nextest run --workspace <part of the test's name>`.
- nextest does not run doctests. The workspace has none (`scripts/cargo.sh test --workspace --doc` finds 0 tests in `cpd`, `cpd_core` and `cpd_tui`); when one is written, the Gate and CI also run that command.
- `scripts/install-cpd.sh` builds `--release` with `CPD_COMMIT` set to the checkout's short commit hash ("The version string" below) and installs `~/.local/bin/cpd` and the adapter `pi/` at `~/.local/share/cpd/pi`, replacing the binary atomically; safe to rerun. Before replacing anything it refuses a build whose `cpd --version` has `store.opens` false, and a Lead Branch build that carries a migration file not on `origin/main` (contract §3.7). **Its follow-ups fail loud and wait** (hs-n278): the engine restart, `cpd tui --respawn` and every `cpd config` it runs are never skipped silently and run even while a migration holds `locks/store.lock`, waiting for it (contract §4.2); each failure prints why on stderr, the later follow-ups still run, and the script exits non-zero at the end. **The engine restart waits for `locks/watch.lock` to be free (60 s, `CPD_INSTALL_LOCK_WAIT_SECS`), runs `cpd watch --ensure` once, checks that the lock is then held, and exits non-zero saying why when the old engine keeps the lock or the new one does not run** (hs-n288). After an install, running Lead wrappers and Attempt drivers reload themselves (contract §3.4, §6.7).

## Restarting the cockpit tmux server

cpd requires tmux >= 3.6 (contract §6.1: older versions can lose a pane's `SIGCHLD`, so `pane-died` never respawns a cockpit view). A running tmux server keeps the version it started with. Bare `cpd` restarts the cpd tmux server (`tmux -L <socket>`, default `cpd`) onto the installed tmux when it is safe and attaches (commands.md §13.3, contract §6.1); when it is not safe it marks a restart pending, the engine starts no new Attempt until nothing runs, then restarts the server detached and reopens every Lead and Manager pane in place (hold, drain, restart; commands.md §13.3). There is nothing to run by hand and no script.

## The scoped loop

The full suite is the Gate's (hs-d338): while you edit, run only the tests your change can reach. Measured on hs (docs/research/build-cycle.md §5), an edit and its scoped tests take 3.7-12 s, almost all of it rustc; the tests themselves take 0.01-0.04 s. With one binary per area, a module edit's scoped run (`scripts/test-changed`, build included) took 1.6-2.2 s on hs (Plan quality, lp-areas), against 10-11 s when all integration tests were one binary.

- **`scripts/test-changed [--dry-run] [<base>]`** runs the tests that the changed files can reach, with `scripts/cargo.sh nextest run`. `<base>` is a git revision, `origin/main` by default; the changed files are those that differ between `<base>` (its merge base with `HEAD`) and the working tree: committed, staged, unstaged and untracked files. The selection:
  1. A changed file belongs to the crate whose directory holds it (`cargo metadata`: `core/` is `cpd-core`, `tui/` is `cpd-tui`, `cli/` is `cpd`). The run is those crates **and their reverse dependencies** (`-p <crate>` for each): a change in `core/` runs `cpd-core`, `cpd-tui` and `cpd`; a change in `tui/` runs `cpd-tui` and `cpd`; a change in `cli/` runs `cpd`.
  2. **Integration tests are selected by area binary** (one binary per area, `cli/tests/<area>/main.rs`, see Tests; a test's name is `<module>::<test>`): a changed `cli/tests/<area>/<module>.rs` selects that module in its binary only, `-p cpd --test <area> -E 'test(/^<module>::/)'`; a changed `cli/tests/<area>/main.rs`, or any other file of that area's directory that is no module, selects that binary: `-p cpd --test <area>`. `--test` makes cargo build and link only the named binaries. Changes in several areas run the union, each with its own filter (`-E 'binary(<area>) & test(/^<module>::/)'`, or `-E 'binary(<area>)'` for a whole binary). A directory of `cli/tests/` that holds no `main.rs` (`common/`) is shared by every binary, so it selects the whole crate `cpd`, as in 1, and so does a change in `cli/src/` beside it.
  3. When several rules select tests, the run is their union.
  4. **The full suite** (`nextest run --workspace`) is the fallback when a changed file is a root-level input that no crate's selection can bound: the root `Cargo.toml`, `Cargo.lock`, `rust-toolchain*`, any `build.rs`, anything under `.config/`, anything under `core/src/store/migrations/`, and `scripts/cargo.sh`. It wins over 1-3.
  5. A changed file outside every crate and not in 4 (`docs/`, `pi/`, other scripts) selects no Rust test. With nothing selected, `test-changed` says `test-changed: nothing to run` on stderr, runs nothing and exits 0.
- **`--dry-run`** prints the one command it would run (for example `scripts/cargo.sh nextest run -p cpd-core -p cpd-tui -p cpd`) on stdout, and nothing else, and does not run it; it exits 0. Use it to see what a change reaches.
- Otherwise it runs that command and exits with nextest's status. Inside its selection it runs every test: it never skips, filters or weakens one (the selection is by files; the Gate runs everything).
- A Worker session refuses an unfiltered full run (hs-d338), so where the fallback in 4 applies there, `scripts/test-changed --dry-run` shows the full command and you run the tests you changed by name (`-p cpd --test <area> -E 'test(/^<module>::/)'`, see Running cargo).
- **bacon** (a developer's tool, not a dependency; the Gate does not use it) watches the checkout and reruns on save. `bacon.toml` at the root has two jobs: `fast`, the default (`bacon`), runs `scripts/test-changed`; `full` (`bacon full`) runs the whole suite, `scripts/cargo.sh nextest run --workspace`. Both go through `scripts/cargo.sh`, so sccache and the `$CARGO` lookup apply.

## Debuginfo

The `dev` profile builds with `debug = 0` (no debuginfo), and `test` inherits it, so cargo builds, nextest and the Gate all use it. Why: debuginfo costs build time and `target/` space (tests-build-F2: full debuginfo 48-62 s and 4.7G, line tables 33-40 s and 2.4G, for a clean test build), and the tests never read it. The measurement for `debug = 0` is in the commit that sets it (Plan quality, dl-config), and `Cargo.toml` keeps the reason beside the setting.

- **What a failure shows.** A panic still prints its message and `file:line:col` (the panic location is compiled in, it is not debuginfo). A backtrace (`RUST_BACKTRACE=1`) lists the function names, from the symbol table, with no `at file:line` under a frame; neither do the frames of dependencies and `std`.
- **Line tables back for a debugging session**, with no edit to `Cargo.toml`: set `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, for example `CARGO_PROFILE_DEV_DEBUG=line-tables-only RUST_BACKTRACE=1 scripts/cargo.sh nextest run -p cpd-core -E 'test(/^<module>::/)'` (it applies to `test` too). `CARGO_PROFILE_DEV_DEBUG=true` gives full debuginfo, for a debugger. A different setting is a different build: cargo rebuilds the crates when you switch it on and again when you unset it, so use a `CARGO_TARGET_DIR` of its own for a long session.

## The version string

`cpd --version`'s `version` is `<Cargo.toml version>-<commit>` (commands.md §1.2): the workspace version, then a short commit hash.

- **The hash comes from the environment, not from git.** `cli/build.rs` reads `CPD_COMMIT` (a short hash such as `070ddee`) and declares `cargo:rerun-if-env-changed=CPD_COMMIT`; it runs no `git` command and watches no `.git` file. So a commit in a worktree rebuilds nothing (the build script used to watch `HEAD` and the branch ref, which rebuilt `cpd` on every commit, 4 s: docs/research/build-cycle.md §7), and a build whose `CPD_COMMIT` changes rebuilds `cpd` and reports the new hash.
- **Who sets it:** `scripts/install-cpd.sh` (the installed binary), `scripts/release.sh` and the Gate (`npm run verify`), each to `git rev-parse --short HEAD` of the checkout it builds. An installed or released binary therefore reports exactly the commit it was built from.
- **The fallback**, when `CPD_COMMIT` is unset or empty (a plain `cargo build` or test run in a worktree): `<version>-dev`, for example `0.0.5-dev`. It names no commit: a build never claims a commit it was not given. `-unknown` is no longer produced. No dirty marker is added and git tags are not read.
- `CPD_VERSION` (the value `cli/src/cli.rs` reads with `env!`) is set by `build.rs` alone.

## Checking a pi version

`scripts/pi-check.sh [<pi command>]` (default `pi`) checks that the adapter `pi/` of the checkout it lives in loads and activates in that pi, offline: an empty temporary `PI_CODING_AGENT_DIR`, `PI_OFFLINE=1`, no credentials, nothing written outside a temporary directory (removed on exit). It prints `pi --version`, then:

1. **Load:** `pi --no-extensions --no-session -e pi/ --mode rpc </dev/null` with no `CPD_*` variables must exit 0 (an extension that throws at load exits 1).
2. **Activation:** the adapter only activates in interactive mode, so pi runs in a private `tmux` server with `CPD_SESSION` equal to pi's session id (`--session-id`), `CPD_HOME` set, and a fake `cpd` first on `PATH`. It passes when the fake was called with `events follow`.

Exit 0 with `pi-check: ok (<version>)`; exit 1 with `pi-check: FAIL: <step and why>` and the tail of pi's output. It needs `tmux`; it does not run `npm test` (those tests use fakes and never run pi).

dotfiles runs it (hs-d145) after updating the `cpd` checkout and before activating a new pinned pi on a machine: `scripts/pi-check.sh <path of the new pi>`; a failure leaves the active pi as it was.
