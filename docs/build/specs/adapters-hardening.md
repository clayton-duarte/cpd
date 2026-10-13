# Job `adapters-hardening`: small correctness fixes in the git, herdr and exec adapters

Tier S2 · Needs: none (appended from Stage 2 reviews) · Contract sections: §1, §2, §12 step 11.

## Goal
Fix five defects found by the Stage 2 reviews. Each is small; none may change behaviour on the happy path.

## Files you own
`src/git/merge.ts`, `src/git/git.ts`, `src/herdr/agents.ts`, `src/lib/exec.ts`, `src/cli.ts` (only the `version()` helper), `test/git/merge.test.ts`, `test/git/git.test.ts`, `test/herdr/agents.test.ts`, `test/lib/exec.test.ts`, `test/cli.test.ts` (only to add a `version` test).

## Requirements
- **`merge.ts`:** when `wt merge` exits 0, decide success from git facts, not from wt's JSON: the Lead Branch moved from `<pre>` to a commit that contains the Attempt's commits (the Lead's new HEAD equals the Attempt branch HEAD after the rebase). If wt's JSON does not parse, log a warning and continue from those facts. If git shows the Lead did not move as expected, run `git reset --keep <pre>` in the Lead worktree and return an explicit failure result (new code `merge_unverified`, exit 1). Never throw after the Lead moved without that check.
- **`git.ts` `rebaseInProgress`:** catch only `ENOENT` from `realpath`; rethrow anything else.
- **`herdr/agents.ts` `startAgent`:** find herdr 0.9.3's real minimum for `agent start --timeout` (run it against `herdr --session cpd-test`, never the default session) and validate the same bound, so cpd refuses invalid values before calling herdr. Record the transcript in the report.
- **`lib/exec.ts`:** the stdin `error` handler ignores only `EPIPE`; any other stdin error makes the run fail loudly.
- **`cli.ts` `version()`:** parse `package.json` with zod (`{version: string}`), failing with a clear error if it is malformed.

## Acceptance
1. Merge test (real git + wt fixtures): a fake `wt` on `PATH` that performs the real merge (delegate to the real `wt`) but prints invalid JSON results in a successful merge outcome plus a logged warning. A fake that exits 0 without moving the Lead gives `merge_unverified`, and the Lead is unchanged.
2. `rebaseInProgress` rethrows a non-ENOENT error (simulate with a path in a directory without permission).
3. `startAgent` rejects a timeout below herdr's minimum without calling herdr; the minimum is backed by a recorded `herdr` transcript.
4. exec: EPIPE is ignored (child that exits before reading stdin); another stdin error fails the run.
5. `version()` test with the real `package.json`.
6. `./verify.sh` passes.

## Out of scope
The merge-lock wait (handled in `attempt-run` by retrying `lock_timeout`); any other refactor.
