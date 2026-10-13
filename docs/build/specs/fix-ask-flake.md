# Job `fix-ask-flake`: make the `cpd ask` tests deterministic and unable to hang a Gate

Tier S2 · Needs: `queue-ask` (merged) · Read `_common.md` first.

## Goal
A failure in `test/commands/ask.test.ts` must never leave a blocked `cpd ask` child process behind. That already happened: the first test hit ENOENT on the queue dir before answering, and its `cpd ask` child blocked the whole Gate for 43 minutes until it was killed.

## Files you own
`test/commands/ask.test.ts`, `test/commands/answer.test.ts` (only if needed).

## Requirements
- Create the Lead queue dir in each test's setup, or make the polling helper tolerate ENOENT. The race the `queue-ask-a2` review described must be impossible.
- Every `cpd ask` child spawned by a test gets a short `--timeout-minutes` (≤ 0.5) and is killed in a `finally` (or `t.after`) whatever happens. No test may wait longer than 60 s in total.
- Do not change production code. If you find a production defect, describe it under Escalations.

## Acceptance
1. Run `node --test test/commands/ask.test.ts` 20 times in a loop; all 20 pass. Paste the loop output summary in the report.
2. Inject a failure (temporarily make the answer step throw), run once, and show that the test fails within 60 s and leaves no `cpd ask` process (`pgrep -f "cpd.mjs ask"` is empty). Restore it.
3. `./verify.sh` passes.
