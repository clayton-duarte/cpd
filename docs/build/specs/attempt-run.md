# Job `attempt-run`: the Attempt lifecycle, `attempt submit`, `attempt read`, `review decide`

Tier S4 · Needs: `ledger-attempts`, `git-wt`, `herdr`, `queue-ask`, `metrics`, `templates-render` · Read `_common.md` first. Contract sections: **§12 (normative, every step)**, §7 (Attempt Tier), §10 kinds, §11 (`attempt run|submit|read`, `review decide`), §14. Trial evidence: `docs/trials/t1-spawn.md` (F4, F5, F7, F8, F10), `docs/trials/t3-worktrees-merge.md`.

## Goal
Implement one Attempt end to end by composing the already-merged modules, so that an Attempt either ends Merged or fails with a recorded cause and the right next step (ladder, failure, Lead message), never silently.

## Files you own
`src/dispatch/attempt-run.ts`, `src/dispatch/verdict.ts`, `src/dispatch/attempt-submit.ts`, `src/commands/{attempt-run,attempt-submit,attempt-read,review-decide}.ts`, `test/dispatch/attempt-*.test.ts`, `test/dispatch/verdict.test.ts`, `test/commands/{attempt-run,attempt-submit,attempt-read,review-decide}.test.ts`.

## Requirements
- `attempt-run.ts`: §12 steps 1–12 exactly, as a sequence of small functions with injected dependencies (ledger, git, herdr, queue, metrics, render, clock), so the lifecycle can be tested with fakes. Each step logs to `logs/attempt-<id>.log`.
  - Decision table for step 7. "Not submitted" must be decided from the Ledger (`submitted_at`) plus the session's last `stopReason` plus `agentGet`, never from herdr idle alone.
  - Step 8 uses `attemptTier`/`nextTier` (`config-tiers`) and `retryCap`.
  - Step 11 maps every `mergeAttempt` result to the §12 handling.
  - The slot is acquired by `cpd watch` and passed in as `--slot <i>`. Release it on every exit path (`finally`).
- `verdict.ts`: `parseVerdict(lastAssistantText)` returns `{verdict, feedback}` or `invalid` (exactly one fenced `json` block, zod schema, last block wins if more than one; document the choice).
- `attempt-submit.ts`, `cpd attempt submit <attempt-id> --wf <wf>`: §11 preconditions in order (`wrong_worktree`, `dirty_tree`, `no_commits`, `gate_tampered`, `gate_failed` with output tail), using `git-wt`'s gate functions. Writes the Gate output to `reviews/<attempt-id>.gate.txt`, then `markSubmitted`.
- `cpd attempt read <attempt-id> [--lines N]`: last N lines of the Worker session (assistant text and tool-call names) using `herdr/session.ts`.
- `cpd review decide <attempt-id> (--approve | --reject) --feedback <text>`: only for an Attempt waiting after `review-needed`. It continues at step 10 or 11.

## Acceptance
1. Lifecycle tests with fakes, one per path, each asserting Ledger calls, queue messages, pane closes, slot release and final state:
   - happy path → Merged + `job-merged`;
   - reviewer reject → ladder → `attempt-retry` with next Tier;
   - reject at the last allowed Attempt → `job-failed`;
   - `timed_out` → `job-failed`, no ladder;
   - crash → ladder;
   - no submit after the nudge → ladder;
   - `setup_failed` → `job-failed`;
   - verdict invalid twice → `review-needed`, then `review decide --approve` → merge;
   - `merge_conflict` → ladder;
   - `post_gate_failed` → `job-failed`.
2. `parseVerdict` table tests: valid approve, valid reject, missing block, malformed JSON, extra text after the block.
3. `attempt submit` integration with real git + wt fixture repos (from `git-wt` test fixtures): each refusal code reproduced, then success. The Ledger part may be faked.
4. `CPD_IT=1` integration (`herdr --session cpd-test`, toy repo, real `bd`, Worker and Reviewer `github-copilot/claude-haiku-4.5:low`, Lead session = any small pi session file you create for the fork): one Attempt runs to Merged. Report the transcript and metrics.

## Out of scope
Scheduling and timeouts (Job `watch`); the Lead skill.
