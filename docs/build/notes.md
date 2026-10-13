# Stage 2 Lead notes

Running record of Attempt outcomes, Lead decisions and proposed changes. Reports and briefs live in `/tmp/cpd-build/` during the run and are copied to `docs/build/reports/` when Stage 2 ends.

## Attempts

| Attempt | Tier | Outcome | Notes |
|---|---|---|---|
| scaffold-a1 | S2 | rejected | Command lookup fell through to `Object.prototype` keys (`cpd constructor` gave exit 1 instead of 2); a single argv word `"job add"` matched. Correctly flagged the Spec's command count as wrong (29, not 30). |
| scaffold-a2 | S3 | approved, merged (`95f0ada`) | Started from a1's commit (cherry-pick) per feedback; fixed all points. Post-merge Gate on `cpd/v0` passed (50 tests). |
| config-tiers-a1 | S3 | approved, merged | All acceptance items met. Found pi had updated itself to **1.2.0** mid-session; `doctor` accepts `>=1.1.0 <2.0.0` (contract amended). Added `setSlotPid` (adopted into the `attempt-run` Spec). Had to edit `test/cli.test.ts` (scaffold-owned) so implemented commands skip the stub test; fixed for all later Jobs. |
| metrics-a1 | S2 | approved, merged | Totals checked against jq. Post-merge Gate on `cpd/v0`: 108 tests pass. |

## Lead errors and process fixes

- Specs live on `main`, while Attempt worktrees branch from `cpd/v0`. After Spec updates on `main`, the Lead Branch had stale docs. Fix: rebase `cpd/v0` onto `main` between batches (`wt step rebase main`, clean because `main` only changes `docs/`). In `cpd` proper this does not arise, because Specs are rendered from the Ledger into the Workflow state dir.

- `scaffold` Spec said "30 commands"; the contract lists 29. The Worker followed the contract and reported it under Deviations, which is the intended behaviour.

## Proposed contract changes (to decide after `e2e`)

1. **Retry from the previous Attempt.** `scaffold-a2` reused a1's commit and only fixed the review findings, saving a full rewrite. Proposal: when an Attempt is rejected and the Reviewer's feedback is a fix list, the next Attempt's worktree starts from the rejected Attempt's branch, rebased onto the Lead Branch HEAD. Its Spec says "fix these findings". A rewrite stays the default for failures that are not review rejections (crash, timeout, merge conflict).

## Appended work (to dispatch through `cpd` later)

- `scaffold-polish` (S1): from the `scaffold-a2` review's non-blocking notes:
  - `exec.run` swallows stdin EPIPE with an empty handler;
  - `version()` parses `package.json` without zod;
  - `node --test` also runs `test/helpers/index.ts` as a test file.
