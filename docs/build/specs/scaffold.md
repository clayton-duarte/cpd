# Job `scaffold`: repository skeleton, CLI router, shared helpers

Tier S2 · Needs: none · Read `_common.md` first. Contract sections: §1, §2, §3, §11 (command list only).

## Goal
Create the TypeScript project skeleton so every later Job only adds its own files. That means:
- a CLI router that knows every command in the contract, with each command a stub;
- shared helpers for output, errors and subprocesses;
- the test harness;
- this repo's Gate.

## Files you own
`package.json`, `package-lock.json`, `tsconfig.json`, `verify.sh`, `.config/wt.toml`, `.gitignore`, `bin/cpd.mjs`, `src/cli.ts`, `src/commands/*.ts` (stubs, one per command), `src/lib/output.ts`, `src/lib/errors.ts`, `src/lib/exec.ts`, `test/helpers/*.ts`, `test/cli.test.ts`, `test/lib/*.test.ts`.

## Requirements
- `package.json`:
  - `"name": "cpd"`, `"private": true`, `"type": "module"`, `"engines": {"node": ">=22.18"}`, `"bin": {"cpd": "bin/cpd.mjs"}`;
  - `"scripts": {"test": "node --test", "typecheck": "tsc --noEmit -p ."}`;
  - dependencies: `zod` (current 4.x); devDependencies: `typescript` (current), `@types/node` (22.x). Commit the lockfile.
- `tsconfig.json`: strict, `noEmit`, `erasableSyntaxOnly`, `module`/`moduleResolution` `nodenext`, `allowImportingTsExtensions` (imports use `.ts` extensions), `target` ES2023, include `src`, `test`, `bin`.
- `verify.sh` (executable): `set -euo pipefail; npx tsc --noEmit -p . && node --test`.
- `.config/wt.toml`: `[pre-merge]` with `verify = "./verify.sh || exit 64"`. No `pre-start` (`npm ci` comes later if needed; note it in the report).
- `bin/cpd.mjs`: `#!/usr/bin/env node`, imports `../src/cli.ts` and calls `main(process.argv.slice(2))`. Executable. Must work when invoked through a symlink.
- `src/cli.ts`:
  - Parses `<group> <command>` or a single-word command.
  - Dispatches with an explicit static table (no auto-discovery) to `src/commands/<group>-<command>.ts` or `src/commands/<command>.ts`, exactly as named in contract §1, for **every** command in §11: `doctor`, `tiers`, `lead start|attach`, `workflow init|show|phase`, `job add|edit|list|show|cancel|retry`, `plan render|decision|lock`, `watch`, `attempt run|submit|read|list|show`, `review decide`, `ask`, `answer`, `pr open|poll|rebase`.
  - `cpd --help` prints the command list as JSON `{ok:true, commands:[...]}`.
  - Unknown command → exit 2 `unknown_command`. `cpd --version` → `{ok:true, version}`.
- `src/commands/*.ts` stubs: each exports `run(argv)`, which returns an error `not_implemented` (exit 1) naming the command.
- `src/lib/output.ts`: `Result` type (ok, or error with code, message, details, exitCode), and `emit(result)`, which prints one JSON line to stdout and sets the process exit code per contract §2.
- `src/lib/errors.ts`: a `CpdError` class with code, message, details and exitCode; helpers `usage()`, `refused()`, `notFound()`, `timeout()`, `failure()`. `src/cli.ts` catches any thrown error and emits it (unknown errors → exit 1 `unexpected` with message).
- `src/lib/exec.ts`: `run(cmd, args, {cwd, env, timeoutMs?, input?})`, which returns `{code, stdout, stderr}` (spawn with argv, stdin `/dev/null` unless `input`), plus `runJson(...)`, which also parses stdout with a given zod schema and fails loud with `bad_tool_output` (stdout and stderr tails in details).
- `test/helpers/`:
  - `tmpDir()` with automatic cleanup;
  - `sandboxEnv()`: an env with `HOME` and `CPD_HOME` under a temp dir, `PATH` preserved;
  - `fakeBin(name, script)`, which creates an executable in a temp bin dir that logs its argv (JSON lines) to a file and runs the given shell script;
  - `runCli(args, opts)`, which runs `bin/cpd.mjs` as a subprocess and returns `{code, json, stderr}`.

## Acceptance
1. `./verify.sh` passes on a fresh clone after `npm ci`.
2. `test/cli.test.ts`:
   - `--help` lists all 29 commands;
   - each stub returns exit 1 with `error.code === "not_implemented"`;
   - an unknown command exits 2;
   - `--version` works;
   - running through a symlink to `bin/cpd.mjs` works.
3. `test/lib/*.test.ts`: `exec.run` captures stdout, stderr and the exit code; `runJson` fails with `bad_tool_output` on invalid JSON; `emit` prints exactly one JSON line and the right exit code for each error helper.
4. `fakeBin` records argv correctly (tested).

## Out of scope
Any real command behaviour; config; anything under `src/` other than the files listed.
