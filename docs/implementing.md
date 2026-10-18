# cpd 2: implementing

Read this first, then [contract.md](contract.md), [commands.md](commands.md) and [CONTEXT.md](../CONTEXT.md), which are normative. This file only says where things go in the code.

## The one-area rule

Each Job edits **only its own area** (`core/src/<area>/`, or `tui/src/` for the cockpit) **plus its tests** (`cli/tests/<area>*.rs`). It does not edit `core/src/lib.rs`, `cli/src/cli.rs`, the shared modules (`core/src/error.rs`, `output.rs`, `ids.rs`, `args.rs`, `home.rs`), `cli/tests/common/`, another area, any `Cargo.toml` or `Cargo.lock`. This is how parallel Jobs merge without conflicts. Need a change outside your area: ask the Lead (`cpd ask`), don't make it.

Within your area you may add files (`core/src/<area>/foo.rs`, declared in your `mod.rs`) and use any other area's public items once that area has landed.

## Layout

- `Cargo.toml`: the workspace, with three members (edition 2024). Dependencies point one way: `cli` depends on `core` and `tui`; `tui` depends on `core`; `core` depends on neither.

| Directory | Package (library / binary) | Holds |
|---|---|---|
| `core/` | `cpd-core` (library `cpd_core`) | every area of the module map below except `tui`, and the shared modules (`args`, `error`, `home`, `ids`, `output`). `core/src/lib.rs` is the crate root: it declares every module, once. |
| `tui/` | `cpd-tui` (library `cpd_tui`) | the whole cockpit (`tui`, `tui view`, commands.md §13): `tui/src/lib.rs`, the views and the read model (`tui/src/read/`). It uses `cpd_core` for the store, events, host and the shared modules. |
| `cli/` | `cpd` (binary `cpd`) | `cli/src/main.rs`, which only calls `main_with`, and `cli/src/cli.rs`: the root `Command` enum, `run` and `main_with`. Also `cli/tests/`, the binary-driven integration tests. |

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
| `watch` | rs-watch | `watch` (§12) | §3.7 (E1–E7) |
| `doctor` | rs-doctor | `doctor` (§14.1) | |
| `tui` | the TUI Job | `tui`, `tui view` (§13) | §6.2 |

`attempt run` and `job dispatch` are declared in `attempt` and `job` but their argument types (`driver::RunArgs`, `driver::DispatchArgs`) and functions (`driver::run`, `driver::dispatch`) are the driver's, so the driver Job never edits those modules.

## The command tree

`cli/src/cli.rs` holds the root `Command` enum; each variant delegates to its area (`cpd_core::context::run(cmd)`, `cpd_core::watch::run(args)`, …, and `cpd_tui` for the cockpit). Each area owns its subcommand enum and its argument types. Every command of commands.md §2 is implemented, and `every_documented_command_exists` (`cli/tests/command_index.rs`) fails when one is documented but absent from the tree.

To add a command:
1. In your module, define its arguments as a `#[derive(clap::Args)]` struct, and name it from the root or the area's subcommand enum (`watch::Args`, `config::ConfigArgs`, `driver::RunArgs`, …).
2. Make `run` return `Ok(Output::success(json!({…})))` or `Err(CpdError::…)`.
3. Test it in your own test file.

Parsing:
- `--help` and `--version` print one JSON value too: `{"ok": true, "help": "<text>"}` and `{"ok": true, "version": "…", "schema": <N>, "store_schema": <M or null>}`: `schema` is the newest store schema the binary supports (the store module's `VERSION`, the number of its migrations), `store_schema` is `PRAGMA user_version` of `$CPD_HOME`'s database opened read-only, without migrating and without the schema check (`null` when the file does not exist). `--version` never creates, migrates or writes the database.
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
- **`home::CpdHome`** (contract §1.4, commands.md §1.4): `CpdHome::from_env()` is `$CPD_HOME`, else `~/.cpd`; an empty or relative `CPD_HOME`, or no home directory, is `usage` (exit 2). Every path under it has a method: `config()`, `db()`, `context_dir()`, `watch_lock()`, `repo_lock(git_common_dir_realpath)`, `attempt_lock()`, `context_lock()`, `watch_log()`, `plan_md()`, `plan_review_md()`, `lead_message()`, `session_message()`, `worker_spec()`, `review_brief()`, `review_diff()`, `review_check()`, `review_gate()`, `driver_log()`, `hook_log(…, Hook)`. Never build these paths by hand.

## Tests

- **Unit tests** go in the module they test (`#[cfg(test)] mod tests`), in your area, in `core/` or `tui/`.
- **Integration tests** go in `cli/tests/<area>.rs` (or `<area>_<topic>.rs`), whichever crate the area is in: they run the built `cpd` binary. They start with `mod common;` and use the harness `cli/tests/common/mod.rs`:
  - `TestEnv::new()`: a temporary directory with its own `$CPD_HOME` (`cpd_home()`, empty), `$HOME` (`home()`, with a git identity), and `bin/` first on `PATH`; `CPD_*`, `PI_*`, `GIT_*`, `XDG_*`, `TMUX` and `TMUX_PANE` are removed from the environment.
  - `env.cpd(&[…])` / `env.cpd_in(cwd, &[…])` run the built binary and return a `Run` (`exit`, `stdout`, `stderr`); `run.json()` asserts exactly one JSON value, `run.ok()` the success envelope, `run.error()` the failure's `error` object.
  - `env.git_repo("name")`: a repo on `main` with one commit; `env.git(dir, &[…])`.
  - `env.tmux(&[…])` and `env.tmux_socket()`: a private tmux server (own `TMUX_TMPDIR` and socket name, killed when the `TestEnv` drops). Put the socket name in the test's `config.toml` as `tmux.socket`.
  - `env.fake_pi(body)`, `env.fake_gh(body)`, `env.fake(name, body)`: shell scripts on `PATH` that record their argv (`env.calls("pi")`) and then run `body`.
  - `env.command(program)`: any other program in the same environment.
- Never touch the real `~/.cpd`, the tmux server `-L cpd`, pi, or the network.
- A test must fail first, for the right reason.

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
| `sha1` 0.10 | | the per-repo lock's name `repo-<sha1>.lock` (§1.4, §7.4); use `CpdHome::repo_lock` |
| `ratatui` 0.30 | default (its `crossterm` backend) | the cockpit's views (`tui view`, commands.md §13.2), as on the spike's branch `cpd/cpd-ui` (history); crossterm is used through its re-export `ratatui::crossterm`, so it is not a dependency of its own |
| `tui-tree-widget` 0.24 | | the `tree` view's collapsible Context → Plan → Session tree, as on `cpd/cpd-ui` |
| `tempfile` 3 | dev only | temporary `$CPD_HOME`, repos and files in tests |

Deliberately absent: a regex crate (the id patterns of §2.1 are checked by hand in `ids`), an error-derive crate (`CpdError` is one struct), a hex crate (see `sha2`), `libc` (`nix` covers it), crossterm as a direct dependency (see `ratatui`).

## Running cargo, the Gate, installing

- cargo is not on `PATH` for Workers and the Gate. Run it as `scripts/cargo.sh <args>` (`$CARGO`, else `cargo` on `PATH`, else `${CARGO_HOME:-$HOME/.cargo}/bin/cargo`; exit 127 with a message when none exists). rustfmt and clippy are not installed.
- The Gate is `npm run verify`: the typecheck and tests of the pi adapter `pi/`, then `scripts/cargo.sh build --workspace` and `scripts/cargo.sh test --workspace`. Run it before you submit.
- `scripts/install-cpd.sh` builds `--release` and installs `~/.local/bin/cpd` and the adapter `pi/` at `~/.local/share/cpd/pi`, replacing the binary atomically; safe to rerun. After an install, running Lead wrappers and Attempt drivers reload themselves (contract §3.4, §6.7).

## Restarting the cockpit tmux server

cpd requires tmux >= 3.6 (contract §6.1: older versions can lose a pane's `SIGCHLD`, so `pane-died` never respawns a cockpit view). A running tmux server keeps the version it started with. `scripts/restart-tmux.sh` restarts the cpd tmux server (`tmux -L <socket>`, default `cpd`) so it runs the installed tmux.

Run it, by hand, once after tmux is upgraded on a machine whose cpd tmux server still runs the old version, and only while no Attempt runs. Restarting stops every pi in the server; Leads resume from their session files. It is never run automatically.

## Checking a pi version

`scripts/pi-check.sh [<pi command>]` (default `pi`) checks that the adapter `pi/` of the checkout it lives in loads and activates in that pi, offline: an empty temporary `PI_CODING_AGENT_DIR`, `PI_OFFLINE=1`, no credentials, nothing written outside a temporary directory (removed on exit). It prints `pi --version`, then:

1. **Load:** `pi --no-extensions --no-session -e pi/ --mode rpc </dev/null` with no `CPD_*` variables must exit 0 (an extension that throws at load exits 1).
2. **Activation:** the adapter only activates in interactive mode, so pi runs in a private `tmux` server with `CPD_SESSION` equal to pi's session id (`--session-id`), `CPD_HOME` set, and a fake `cpd` first on `PATH`. It passes when the fake was called with `events follow`.

Exit 0 with `pi-check: ok (<version>)`; exit 1 with `pi-check: FAIL: <step and why>` and the tail of pi's output. It needs `tmux`; it does not run `npm test` (those tests use fakes and never run pi).

dotfiles runs it (hs-d145) after updating the `cpd` checkout and before activating a new pinned pi on a machine: `scripts/pi-check.sh <path of the new pi>`; a failure leaves the active pi as it was.
