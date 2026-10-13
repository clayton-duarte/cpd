# Stage 2 Lead notes

Running record of Attempt outcomes, Lead decisions and proposed changes. Reports and briefs live in `/tmp/cpd-build/` during the run and are copied to `docs/build/reports/` when Stage 2 ends.

## Attempts

| Attempt | Tier | Outcome | Notes |
|---|---|---|---|
| scaffold-a1 | S2 | rejected | Command lookup fell through to `Object.prototype` keys (`cpd constructor` gave exit 1 instead of 2); a single argv word `"job add"` matched. Correctly flagged the Spec's command count as wrong (29, not 30). |
| scaffold-a2 | S3 | approved, merged (`95f0ada`) | Started from a1's commit (cherry-pick) per feedback; fixed all points. Post-merge Gate on `cpd/v0` passed (50 tests). |
| config-tiers-a1 | S3 | approved, merged | All acceptance items met. Found pi had updated itself to **1.2.0** mid-session; `doctor` accepts `>=1.1.0 <2.0.0` (contract amended). Added `setSlotPid` (adopted into the `attempt-run` Spec). Had to edit `test/cli.test.ts` (scaffold-owned) so implemented commands skip the stub test; fixed for all later Jobs. |
| metrics-a1 | S2 | approved, merged | Totals checked against jq. Post-merge Gate on `cpd/v0`: 108 tests pass. |
| git-wt-a1 | S3 | rejected | `tamperCheck` could be bypassed by **renaming** `.config/wt.toml` (git diff rename detection hides the old path). |
| git-wt-a2 | S4 | approved, merged | `--no-renames`; rename tests at gate and merge level. Started from a1 (cherry-pick). |
| herdr-a1 | S3 | rejected | `lead attach` for an unknown Workflow left a half-created `workflows/<wf>/locks/` dir. |
| herdr-a2 | S4 | approved, merged | Existence check before the lock. Started from a1. |
| queue-ask-a1 | S2 | rejected | The "atomic write" race test ran reader and writer in one thread, so it proved nothing. |
| queue-ask-a2 | S3 | approved, merged | Reader in a separate process, failing-first shown. Lead added: expired questions are renamed `<id>.expired.json`, never deleted (ids never reused; contract amended). |
| ledger-core-a1 | S3 | approved, merged | All 7 acceptance items with real bd. Found `bd update --metadata` merges rather than replaces (t2 F5 was wrong). Post-merge Gate on `cpd/v0`: 221 pass, 17 skipped (`CPD_IT`). |

## Lead errors and process fixes

- Specs live on `main`, while Attempt worktrees branch from `cpd/v0`. After Spec updates on `main`, the Lead Branch had stale docs. Fix: rebase `cpd/v0` onto `main` between batches (`wt step rebase main`, clean because `main` only changes `docs/`). In `cpd` proper this does not arise, because Specs are rendered from the Ledger into the Workflow state dir.

- `scaffold` Spec said "30 commands"; the contract lists 29. The Worker followed the contract and reported it under Deviations, which is the intended behaviour.

## Proposed contract changes (to decide after `e2e`)

1. **Retry from the previous Attempt.** `scaffold-a2` reused a1's commit and only fixed the review findings, saving a full rewrite. Proposal: when an Attempt is rejected and the Reviewer's feedback is a fix list, the next Attempt's worktree starts from the rejected Attempt's branch, rebased onto the Lead Branch HEAD. Its Spec says "fix these findings". A rewrite stays the default for failures that are not review rejections (crash, timeout, merge conflict).

## Self-hosting milestones

- After increment 2: `cpd doctor` passes on this Mac, `cpd workflow init --wf v0` created the build's own Ledger in `~/.cpd`, and `cpd lead attach --wf v0` attached this Lead session (pane `w1:p5`). From increment 3 on, Workers escalate with `cpd ask`. `~/.cpd/config.json` was written by hand with exactly the content the `dotfiles` Job will render for this Mac.

## Appended work (to dispatch through `cpd` later)

- `scaffold-polish` (S1): from the `scaffold-a2` review's non-blocking notes:
  - `exec.run` swallows stdin EPIPE with an empty handler;
  - `version()` parses `package.json` without zod;
  - `node --test` also runs `test/helpers/index.ts` as a test file.
- `ledger-hardening` (S2), from the `ledger-core-a1` review:
  - `lockPlan` and the dispatch-phase `addJob` write fingerprints before the `bd batch`/flip, so a crash in between leaves fingerprints for draft Jobs (`fingerprint_exists` on re-lock);
  - `workflow show` fails with `ledger_invalid` before it can list tamper events if a Job's metadata was broken by hand;
  - `job edit` loads the pi catalog even for title-only edits.
- `git-wt-hardening` (S2), from the `git-wt-a2` review:
  - the merge lock wait (60 s) is shorter than two Gate runs, so a queued merge should retry on `lock_timeout` or wait longer;
  - malformed JSON from a successful `wt merge` throws after the Lead moved, without rollback;
  - `rebaseInProgress` swallows all realpath errors.
- `queue-ask` follow-ups: `watch` must ignore `*.expired.json`; the first test in `test/commands/ask.test.ts` can flake on ENOENT (create the queue dir in setup).
