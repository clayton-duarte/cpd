# Stage 2 build: common rules for every Job (read first)

You are a Worker in **CPD**, executing one **Attempt** of one **Job** to build the `cpd` CLI itself. Read this file, then your Job Spec, then work autonomously until the Job is done. Nobody will answer questions while you run.

## Read these before coding

1. `docs/cli-contract.md`: **normative**. Implement exactly what your Spec assigns from it. Its §1 (conventions) and §2 (output and exit codes) apply to all code.
2. Your Job Spec: goal, files you own, acceptance criteria, out of scope.
3. Only as needed: `CONTEXT.md` (vocabulary), `docs/requirements.md`, and the trial report your Spec names under `docs/trials/` (the proven command recipes).

## Where you work

- Your current directory is your **Attempt worktree** (a git worktree of `~/projects/cpd` on branch `cpd/v0--<job>-a<n>`, created from the Lead Branch `cpd/v0`). Work and commit only here.
- `node_modules` is installed by the worktree's `pre-start` hook (`npm ci`); do not run `npm install` (it would change the lockfile), except in Job `scaffold`, which creates the lockfile.
- Modules your Job depends on are already merged into your base. Use their exported functions; do not modify them. If one is missing or wrong, record it under **Escalations** in your report and work around it only if the Spec allows.

## Rules

- **Touch only the files your Spec lists** (plus new test files under the test path it names). Never edit `package.json`, `package-lock.json`, `tsconfig.json`, `src/cli.ts`, `.config/wt.toml` or `verify.sh` unless your Spec owns them. Never add a dependency.
- **Code style:** TypeScript with erasable syntax only, ES modules, `node:` imports, zod at every boundary, subprocesses via `spawn` argv (never a shell string), no silent fallbacks, small pure functions where possible. Follow the patterns already in `src/lib/`.
- **Tests:**
  - Write tests with `node:test` that pin the contract, not the implementation.
  - Every acceptance item must be covered by a test or by a recorded command transcript in your report.
  - Integration tests against real `bd`, `wt` and `git` are expected; run them in temp dirs with `HOME` set to a temp dir (use the helpers in `test/helpers/`).
  - Anything that needs a herdr server or a real pi model must be skipped unless `CPD_IT=1`.
- **Gate:** run `./verify.sh` (typecheck plus all tests). It must pass before you finish.
- **Commits:** one or more commits on your branch with clear messages (`<area>: <what>`), working tree clean at the end. Do not merge, rebase onto other branches, push, or touch other branches.
- **Isolation:**
  - No changes outside your worktree except temp dirs under `/tmp/cpd-build/<job>/` and your report.
  - Never run commands against the User's default herdr session. If your Spec needs herdr, use only `herdr --session cpd-test …`; the session server may need starting with `herdr --session cpd-test server &`, and you must not stop it.
  - Never `pi install`, `brew install` or `npm install -g`.
  - Never touch `~/.cpd`, `~/.pi/agent` (other than pi's own session files), `~/.config` or the dotfiles, unless your Spec says so.
- **Tools available:** node 22.22, npm, git 2.50, `wt` 0.80.0, `bd` 1.3.1, `herdr` 0.9.3, `gh` (read-only use), `jq`, `sqlite3`, `python3` 3.9, pi 1.2.0 (the trials ran on 1.1.0; if pi behaviour differs from a trial report, trust what you observe and report the difference).
- **Already merged helpers to reuse:**
  - `test/cli.test.ts` skips the `not_implemented` check for any command whose file no longer contains `not_implemented`, so do not edit it.
  - Tiers: `nextTier`/`attemptTier` return a parsed Tier or `null`; call `formatTier()` before storing or printing one.
  - Slots: `slots.ts` has `setSlotPid(index, attempt, pid)` to hand a slot to the process that will release it.

## Report

Write `/tmp/cpd-build/reports/<attempt-id>.md` (create the directory) with:

```md
# <attempt-id>: <job title>
Commits: <sha list>. Gate: pass/fail (paste the last lines of ./verify.sh).
## What I built (files, public functions/commands)
## Acceptance checklist (item | result | evidence: test name or transcript)
## Deviations from the Spec or contract (and why)
## Escalations (questions you could not resolve; what you assumed)
```

When done, reply with only the report path.
