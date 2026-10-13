# Job `ledger-hardening`: crash-safe plan lock and appends, resilient `workflow show`

Tier S2 · Needs: none (appended from Stage 2 reviews) · Contract sections: §8, §11 (`plan lock`, `job add` in phase `dispatch`, `job edit`, `workflow show`).

## Goal
Fix four defects the `ledger-core-a1` and `ledger-attempts-a1` reviews found, without changing any command's output on the happy path.

## Files you own
`src/ledger/plan.ts`, `src/ledger/jobs.ts`, `src/ledger/attempts.ts`, `src/commands/workflow-show.ts`, `src/commands/job-edit.ts`, `test/ledger/plan.test.ts`, `test/ledger/jobs.test.ts`, `test/ledger/attempts.test.ts`, `test/commands/workflow-show.test.ts`, `test/commands/job-edit.test.ts`, `test/ledger/hardening.test.ts`.

## Requirements
- **`lockPlan` is resumable.** Today it writes fingerprints, then runs the `bd batch` that queues the Jobs, then sets the phase. A crash between those steps leaves a Workflow that can never be locked: a re-run gives `no_jobs` (no drafts left) or `fingerprint_exists`. Make a re-run finish the lock:
  - if the phase is `plan` and every non-cancelled Job is already `queued` with a stored fingerprint that matches, only set the phase;
  - a stored fingerprint equal to the recomputed one is not an error;
  - a different stored fingerprint is still `fingerprint_exists`.
- **The dispatch-phase `addJob` is resumable in the same way.** If the Job exists as `draft` with a matching stored fingerprint, a re-run with the same input flips it to `queued` instead of failing.
- **`workflow show` survives a hand-broken Job.** If a Job's metadata no longer parses, list it under a new `invalid: [{id, key?, issues}]` array and still return the other Jobs and the `tamper` list. Every other command stays fail-loud (`ledger_invalid`).
- **`job edit` loads the pi Tier catalog only when `--tier` or `--review-tier` is given.**
- **`attempts.ts`:** the rollback in `startAttempt` (`.catch(() => undefined)`) must not hide a second failure. Log it with `logEvent` (level `error`) and add it to the thrown error's `details.rollbackError`.

## Acceptance
1. Fault-injection tests (real `bd`): make `lockPlan` throw after each of its writes in turn (inject the failure through a test seam you add, not by editing `bd`), then re-run it; every case ends locked, with phase `dispatch` and the same fingerprints. The same for a dispatch-phase `addJob`.
2. A Job whose metadata was broken with a hand-run `bd update` appears in `workflow show`'s `invalid` list, together with the `tamper` entry for that edit; `job show` on it still fails `ledger_invalid`.
3. `job edit <k> --title x` works with a broken pi catalog path (test seam or env), while `--tier` with the same broken path fails as before.
4. A forced rollback failure in `startAttempt` shows up in `logs/cpd.jsonl` and in the error's `details.rollbackError`.
5. `./verify.sh` passes.

## Out of scope
Any change to command output on the happy path; Gate speed (Job `gate-speed`).
