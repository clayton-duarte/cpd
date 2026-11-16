# CLI and WebUI argv parity

This page documents the parity guard that proves every `cpd` command line the WebUI builds is accepted by the `cpd` CLI parser. It states what the guard checks, how it runs, what it does not check, and its status against the decisions that touch it.

Decision context: `pro-d573` (2026-11-12) keeps two interfaces over one Rust core, an engine API for the PWA and the CLI (JSON) for agents, and rule 3 of that decision requires a check that "fails when an action exists in one interface and not the other, or when their shapes differ". The argv guard below is the only parity check in the tree today, and it covers the argv half of that rule, not the action or shape halves.

## Status

- **Built.** The guard exists at the commit this page was written against. Its pieces are listed in "The parts" below.
- **Scheduled for removal, by an older decision.** `docs/contract.md` §5.6 is headed **Removed** (decision `pro-d529`, Plan `drop-cli-tui`). It says the argv contract goes with the WebUI's Node argv bridge (`web/src/server/cpd-argv.ts`), which Plan 4 `webui-core-ws` deletes (`docs/architecture/pi-native-core.md` §5.3, §7). The binary keeps `cpd argv-check` until the code removal lands. `docs/commands.md` §2 lists it as hidden.
- **Open for the Lead.** `pro-d573` is newer than `pro-d529`, and it names no argv guard. Whether this guard is kept and extended to the action and shape checks of `pro-d573`, or replaced by them, is not decided in these documents. Until the Lead decides, this page describes the guard as it is and does not treat it as kept.

## The parts

| Part | Where | Role |
|---|---|---|
| `CpdCommand`, `toArgv`, `allCpdCommands()` | `web/src/server/cpd-argv.ts` | The one union of commands the web sends, the one builder that turns each into argv, and the enumeration of every variant with the optional-flag combinations the call sites produce. |
| The guard test | `web/src/server/cockpit/argv-contract.test.ts` | Builds the argv of every command from `allCpdCommands()`, asks the CLI to parse them, and fails on any refusal. |
| `cpd argv-check` | `cli/src/argv_check.rs`, dispatched from `cli/src/cli.rs` before clap | The parse-only check. It is hidden from `cpd --help`. |

The contract the parts follow is `docs/contract.md` §5.6 and `web/CONTRACT.md` §18. No web module may spawn `cpd` with an argv written at the call site: every command goes through `toArgv`, and the modules that spawn `cpd` consume only its output.

## How the guard runs

1. The test calls `allCpdCommands()` and maps each command to its argv with `toArgv`. Every variant of the union appears, and so does every optional-flag combination the call sites produce, so a variant with no entry has no test.
2. Before parsing, require a known command such as `plan list` and a currently supported command with multiple named options. The structural check must cover nested command flags without depending on retired tier-configuration commands.
3. A missing `cpd` binary fails the test with a message to build it. It never skips.
4. The test starts one `cpd argv-check` process with the argv list on stdin, with a 30-second timeout, and reads its stdout and exit code.
5. The test passes only when the exit code is `0` and stdout, trimmed, is exactly `{"ok":true}`.

The binary the test uses is `CPD_BIN` when that variable is set, otherwise `target/debug/cpd` in the repository root. The Gate runs the test in the web client's test step, `verify:web` (`pnpm --filter cpd-web-client run test`, reached from `verify:steps`).

## The `argv-check` interface

Source: `cli/src/argv_check.rs`. The check touches nothing else: no dispatch, no store, no `$CPD_HOME`, no network and no git.

- **Input.** stdin is a JSON array of string arrays. Each inner array is one argv after the program name, so `[["inbox","list"],["plan","show","--plan","p1"]]` is `cpd inbox list` and `cpd plan show --plan p1`.
- **Success.** Every argv parses. stdout is exactly `{"ok":true}` and the exit code is `0`.
- **Failure.** One or more argv fail to parse. stdout is `{"ok":false,"failures":[{"argv":[...],"error":"..."}]}`, one entry per failing argv in input order. `argv` is the inner array as received, and `error` is the parser's message. Argv that parse are not listed. The exit code is non-zero.
- **Parsed.** `--help` and `--version` count as parsed, since clap reports them as success.
- **Bad input.** Input that is not a JSON array of string arrays fails with a `usage` error and a non-zero exit. Stdin that cannot be read fails with an `internal` error and a non-zero exit. Neither is a passing run.

## What the guard proves

- Every argv that `toArgv` builds for an enumerated command is accepted by the real clap grammar of the same build. A CLI grammar change that breaks a web argv fails the Gate before merge.
- A web argv added as a new `CpdCommand` variant is covered, because the test enumerates the union through `allCpdCommands()`.

## What the guard does not prove

- **Call sites.** Nothing checks that a web module spawns `cpd` only through `toArgv`. The test covers the argv in `allCpdCommands()`. A call site that writes its own array is not caught. This is a convention (`docs/contract.md` §5.6, "No hand-built argv"), not an enforced rule.
- **Existence and semantics.** Parsing an argv does not show the command exists in the core's dispatch, or that it does what the web expects.
- **Reply shape.** The guard does not check the JSON a command prints, so the action and shape halves of `pro-d573` rule 3 are not covered.
- **Command-level parity.** The guard does not compare the set of actions in the engine API with the set of CLI commands.

## Verification of this page

- The interface was run from the worktree's own build (`target/debug/cpd`): `[["inbox","list"],["plan","show","--plan","p1"]]` exits 0 with `{"ok":true}`; `[["inbox","list"],["bogus-cmd"]]` exits 1 with a failures document naming `["bogus-cmd"]`; non-JSON stdin exits 2 with a `usage` error.
- The installed `cpd` on the author's machine (`0.27.135-dev`, protocol 27) has no `argv-check` and was not used for this.
- The guard test's own result (green or red) is not stated here and was not run for this page. Only the Gate run of `verify:web` establishes it.
