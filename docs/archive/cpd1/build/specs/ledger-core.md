# Job `ledger-core`: beads Ledger, Workflow and Job commands, fingerprints

Tier S3 · Needs: `config-tiers` · Read `_common.md` first. Contract sections: §6, §8, §11 (`workflow *`, `job *`, `plan decision`). Trial evidence (normative recipes): `docs/trials/t2-ledger.md` ("Recipes" and F1–F9).

## Goal
`cpd` becomes the single writer of a per-Workflow beads Ledger, with the Workflow and Job surface and the invariants beads cannot enforce (draft-only edits, fingerprints, actor audit).

## Files you own
`src/ledger/bd.ts`, `src/ledger/schema.ts`, `src/ledger/workflow.ts`, `src/ledger/jobs.ts`, `src/ledger/plan.ts`, `src/ledger/fingerprint.ts`, `src/ledger/audit.ts`, `src/commands/{workflow-init,workflow-show,workflow-phase,job-add,job-edit,job-list,job-show,job-cancel,job-retry,plan-decision}.ts`, `test/ledger/**`, `test/commands/{workflow,job,plan-decision}*.test.ts`.

## Requirements
- `bd.ts`:
  - The single gateway to `bd`: env and cwd exactly as §8, every call under `withLock(locks/ledger.lock)` (except read-only calls, which may skip the lock). Use `--json`.
  - Check the exit code first: 13 means `cas_lost`, 14 means `ledger_frozen`. Parse with zod.
  - `initLedger(wf)` implements the t2 init recipe (custom statuses and types).
- `schema.ts`: zod models for the bd issue, dependency, history and event shapes you use (field names from t2 F9).
- `workflow.ts`: `initWorkflow` (§11 `workflow init`, writes `workflow.json` atomically), `readWorkflow`, `writeWorkflow`, `getPhase`, `setPhase` with the legal transition table (context→plan via command; plan→dispatch, dispatch→pr, pr→done only through exported functions used by later Jobs).
- `jobs.ts`:
  - `addJob` (draft in `plan`, appended+queued in `dispatch`, with all §11 validations, cycle check before writing, spec-file zod schema);
  - `editJob` (draft only, else `job_locked` exit 3);
  - `listJobs`, `showJob` (Job + child Attempts + comments), `readyJobs` (`bd ready -t job -n 0` filtered to `queued`), `getJobByKey`;
  - `cancelJob` (no running Attempt → `cancelled` with reason; running → write-once `cancel_requested`);
  - `retryJob` (`failed` only; write-once `retry_<n>_tier`; status `queued`).
- `fingerprint.ts`: canonical JSON (sorted keys) and sha256 per §8; `writeFingerprints`, `verifyJob` (`locked_job_tampered`).
- `plan.ts`:
  - `planFingerprint(wf)`;
  - `recordPlanDecision(wf, reviewId, state, feedback?)` (write-once per state);
  - `latestApprovedFingerprint(wf)`;
  - `lockPlan(wf)`, the Ledger part of `plan lock`: verify approval for the current fingerprint, validate the DAG, write fingerprints, `bd batch` all drafts → `queued`, phase → `dispatch`. Exported for the `watch` Job. The `plan lock` command file is NOT yours.
- `audit.ts`: list `bd history --events` entries whose actor ≠ `cpd` for a Workflow (used by `workflow show` `tamper`).
- Commands as in §11, output shapes exactly as written there.

## Acceptance
Integration tests with real `bd` in a sandboxed `HOME`/`CPD_HOME`:
1. `workflow init` leaves the product repo byte-identical (status, local config, hooks, file checksums, as in t2 F1) and creates the §4 layout. A second init gives `workflow_exists`.
2. The t2 F3 graph (A; B and C need A; D needs B and C; E): `readyJobs` at each step (A merged via a test-only close helper that respects the `in_review` rule, B merged, C cancelled → D stays blocked, F appended in `dispatch` → ready).
3. `job edit` on a queued Job gives exit 3 `job_locked`. Editing a draft works.
4. Fingerprints: after `lockPlan`, a direct `bd update <job> -d tampered` (run in the test as a different actor) makes `verifyJob` fail `locked_job_tampered`, and `workflow show` lists the tamper event.
5. `lockPlan` without an approved decision for the current fingerprint gives `plan_not_approved`. After `recordPlanDecision(approved)` it succeeds. Changing a draft after approval invalidates it.
6. Invalid inputs (bad key, bad Tier, unknown `needs`, cycle, empty `touches`) are each refused with a specific code and nothing is written (check `bd list` before and after).
7. `job cancel` without a running Attempt gives `cancelled`. `job retry` on a non-failed Job is refused.

## Out of scope
Attempts, Escalation records and metrics (Job `ledger-attempts`); plan rendering; the `plan lock` command; dispatch.
