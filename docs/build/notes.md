# Stage 2 Lead notes

Running record of Attempt outcomes, Lead decisions and proposed changes. Reports and briefs live in `/tmp/cpd-build/` during the run and are copied to `docs/build/reports/` when Stage 2 ends.

## Attempts

| Attempt | Tier | Outcome | Notes |
|---|---|---|---|
| scaffold-a1 | S2 | rejected | Command lookup fell through to `Object.prototype` keys (`cpd constructor` gave exit 1 instead of 2); a single argv word `"job add"` matched. Correctly flagged the Spec's command count as wrong (29, not 30). |
| scaffold-a2 | S3 | approved, merged (`95f0ada`) | Started from a1's commit (cherry-pick) per feedback; fixed all points. Post-merge Gate on `cpd/v0` passed (50 tests). |

## Lead errors

- `scaffold` Spec said "30 commands"; the contract lists 29. The Worker followed the contract and reported it under Deviations, which is the intended behaviour.

## Proposed contract changes (to decide after `e2e`)

1. **Retry from the previous Attempt.** `scaffold-a2` reused a1's commit and only fixed the review findings, saving a full rewrite. Proposal: when an Attempt is rejected and the Reviewer's feedback is a fix list, the next Attempt's worktree starts from the rejected Attempt's branch, rebased onto the Lead Branch HEAD. Its Spec says "fix these findings". A rewrite stays the default for failures that are not review rejections (crash, timeout, merge conflict).

## Appended work (to dispatch through `cpd` later)

- `scaffold-polish` (S1): from the `scaffold-a2` review's non-blocking notes:
  - `exec.run` swallows stdin EPIPE with an empty handler;
  - `version()` parses `package.json` without zod;
  - `node --test` also runs `test/helpers/index.ts` as a test file.
