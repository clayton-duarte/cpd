# Job `retry-carry-over`: a retry can start from the previous Attempt's commits

Tier S3 · Needs: none (appended: found when `watch-a1` was rejected for one defect and `watch-a2` had to start from scratch at S5) · Contract sections: §12 steps 3 and 8, §16 (`templates/worker.md`).

## Goal
When an Attempt fails or is rejected, its worktree and branch are kept (§12 step 8), but the next Attempt's Worker Spec does not mention them, so the next Worker rebuilds everything from the Lead HEAD even when the Reviewer asked for one small fix. Tell the next Worker where the previous work is and when to reuse it.

## Files you own
`templates/worker.md`, `src/render/spec.ts`, `test/render/**` (including the goldens; regenerate them only with `CPD_REGEN_GOLDENS=1` and review the diff).

## Requirements
- For every previous Attempt in the "Previous Attempts" section, also render its branch (Attempt metadata `branch`) and, when the branch still exists, the command that lists its commits: `git log --oneline <lead-branch>..<branch>`.
- Add one instruction to that section: if a previous Attempt was rejected by the Reviewer (cause `review_rejected`) and the feedback asks for specific fixes, start by cherry-picking that Attempt's commits (`git cherry-pick <lead-branch>..<branch>`), then make only the requested fixes; if it failed for another cause (crash, timeout, conflict, Gate), decide yourself whether its commits are worth reusing.
- If the branch no longer exists, render the branch name with "(removed)" and no commands.
- Keep everything else in the template unchanged.

## Acceptance
1. Golden tests for an Attempt 2 whose Attempt 1 was `review_rejected` (branch present) and one whose Attempt 1 `crashed` (branch removed): the section shows the branch, the commands or "(removed)", and the instruction.
2. Attempt 1 Specs are byte-identical to today's goldens.
3. `./verify.sh` passes.

## Out of scope
Cherry-picking automatically in `attempt run`; changing the ladder.
