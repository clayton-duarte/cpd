# Job `git-wt`: worktrees, Gate, merge queue, cleanup, PR rebase (library)

Tier S3 · Needs: `config-tiers` · Read `_common.md` first. Contract sections: §3 (branch names), §12 steps 2, 9, 11, 12, §11 (`pr rebase` behaviour, `attempt submit` Gate rule). Trial evidence (normative recipes): `docs/trials/t3-worktrees-merge.md` (F1–F11 and "Recipes").

## Goal
A tested TypeScript library around `wt` and `git` with the exact non-interactive recipes and failure classification proven in trial t3.

## Files you own
`src/git/git.ts`, `src/git/wt.ts`, `src/git/attempt-worktree.ts`, `src/git/gate.ts`, `src/git/merge.ts`, `src/git/lead-branch.ts`, `src/git/pr-rebase.ts`, `test/git/**` (including a fixture repo builder under `test/git/fixtures.ts`).

## Requirements
Every `wt` call uses `--yes` (global, before the subcommand), `-C <dir>`, stdin `/dev/null`, and `--format json` where available.
- `wt.ts`: an argv builder and runner for the subset used (`switch --create`, `merge`, `hook pre-merge`, `remove`, `step rebase`), with JSON outputs parsed by zod.
- `lead-branch.ts`: `createLeadBranch(repo, leadBranch, base)` returns `{path}` (its own worktree, t3 F1). Idempotent: an existing branch returns its path via `wt switch <branch> --format json` (`action: existing`).
- `attempt-worktree.ts`:
  - `createAttemptWorktree(leadWorktree, branch, leadBranch)` returns `{path}`. It blocks while setup runs and asserts the merge-base equals the Lead HEAD. On failure it cleans up (t3 F1) and throws `setup_failed` with the hook exit code and a stderr tail.
  - `removeAttemptWorktree(repo, leadWorktree, branch)`: t3 "attempt remove" recipe (worktree, then the guarded `git branch -d`). herdr closing is not yours.
- `gate.ts`:
  - `tamperCheck(worktree, base, branch)` returns the forbidden paths touched (`.config/wt.toml`, `verify.sh`, `.cpd/`);
  - `runGate(worktree)` returns `{passed, exitCode, outputTail}` (`wt hook pre-merge`; 64 = gate failed; any other non-zero = `gate_error`);
  - `treeClean(worktree)`, `commitsAhead(worktree, base)`.
- `merge.ts`: `mergeAttempt({repo, leadWorktree, leadBranch, attemptWorktree, branch})`, under `withLock(merge-<sha1(repo)>.lock)`, implementing the t3 "attempt merge" recipe:
  - preflight: Lead clean, Attempt clean, tamper check;
  - snapshot the pre-merge SHA;
  - `wt merge <lead> --no-squash --no-remove`;
  - classify into `merged | gate_failed | conflict | cas_lost | dirty_target | tampered | unknown`, aborting the rebase on conflict;
  - CAS-lost retried up to 3 times;
  - on `merged`, run the post-merge Gate in the Lead worktree and roll back with `git reset --keep <pre>` if it fails (result `post_gate_failed`).
  - Returns `{result, preSha, postSha?, detail}`.
- `pr-rebase.ts`: `rebaseLeadOntoBase({leadWorktree, leadBranch, base})` returns `{result: rebased | conflict, files?}`. Clean case only (`git merge-tree` check, `wt step rebase`, abort on failure; never leave the Lead mid-rebase). Pushing is not yours.
- `git.ts`: small helpers (`revParse`, `mergeBase`, `diffNames`, `statusPorcelain`, `realpath`).

## Acceptance
Integration tests with real `git` + `wt` in temp repos, `HOME`/`XDG_CONFIG_HOME` sandboxed, and a fixture repo with `.config/wt.toml` (`pre-start` writes `.setup-done` after a short sleep; `pre-merge` = `./verify.sh || exit 64`; `verify.sh` fails when `FAIL_GATE` exists):
1. Create Lead and Attempt worktrees: setup finished before return (timestamp check); merge-base assertion holds; a failing `pre-start` throws `setup_failed` and leaves no worktree or branch.
2. Merge two Attempts where the second needs a rebase: linear history, original messages and authors, new SHAs, Lead worktree clean (t3 F4).
3. Each failure class reproduced and asserted, with the Lead ref unchanged after each failure:
   - `gate_failed` (FAIL_GATE);
   - `conflict` (same line edited);
   - `cas_lost` (forced by moving the Lead ref between snapshot and merge in a test hook, or by two concurrent merges; the retry succeeds);
   - `dirty_target`;
   - `tampered` (Attempt edits `.config/wt.toml`);
   - `post_gate_failed` (Lead gate failing after merge; rolled back).
4. `removeAttemptWorktree` deletes the worktree and branch of a merged Attempt.
5. `rebaseLeadOntoBase`: a clean case rebases; a conflict case returns the conflicting files and leaves the Lead worktree unchanged and not mid-rebase.

## Out of scope
The Ledger, herdr, pushing, the `attempt submit` command itself.
