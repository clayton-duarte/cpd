# Job `ledger-attempts`: Attempts, Escalation records, metrics records

Tier S3 · Needs: `ledger-core` · Read `_common.md` first. Contract sections: §8, §11 (`attempt list|show`), §12 (the Ledger writes named in each step), §14. Trial evidence: `docs/trials/t2-ledger.md` (F4, Recipes rows `attempt *`, `escalation *`, `metrics record`, `job merged`).

## Goal
The Ledger functions the dispatch engine calls for every Attempt state change, all compare-and-swap guarded and write-once, plus the two read commands.

## Files you own
`src/ledger/attempts.ts`, `src/ledger/escalations.ts`, `src/ledger/metrics.ts`, `src/commands/{attempt-list,attempt-show}.ts`, `test/ledger/{attempts,escalations,metrics}.test.ts`, `test/commands/attempt-{list,show}.test.ts`.

## Requirements
Use `src/ledger/bd.ts` from `ledger-core` for every call.
- `startAttempt(wf, jobKey, {tier, worktree?, branch?})`:
  - CAS Job `queued → running` (exit 13 → `cas_lost`, return null, no Attempt created);
  - create the Attempt child (`in_progress`, number = existing + 1, `started_at`);
  - refuse if an open Attempt exists;
  - verify the Job fingerprint first.
- `recordAttemptFacts(wf, attemptId, facts)`: write-once metadata keys (`worktree, branch, agent, session_path, started_at, …`); a second write of the same key gives `metadata_write_once`.
- `markSubmitted(wf, attemptId, commitSha)`: Attempt `in_review` + `submitted_at` + provenance `commit` (40-hex); Job → `in_review`.
- `recordVerdict(wf, attemptId, {verdict, feedback})`: comment + `outcome`.
- `failAttempt(wf, attemptId, cause, {requeue: boolean})`: close the Attempt with `outcome=failed|rejected|timed_out|cancelled` + `cause` + `ended_at`; the Job becomes `queued` when `requeue`, else `failed` (or `cancelled` for cause `cancelled`).
- `markMerged(wf, attemptId, mergedSha)`: Attempt closed `approved`; Job `metadata.merged_sha`; `bd close <job> --reason merged`, only from `in_review` (else `illegal_transition`); provenance `land`.
- `attemptCount(wf, jobKey)`, `listAttempts(wf, {status?})`, `getAttempt(wf, attemptId)`, `runningAttempts(wf)`.
- `escalations.ts`:
  - `recordEscalation(wf, attemptId, {escId, question, askedAt})`: child issue of the Attempt, idempotent by `escId` (a second call is a no-op);
  - `recordAnswer(wf, escId, {answer, answeredAt})`: comment + `answered_by` + close;
  - `openEscalations(wf)`.
  - Closing an Attempt with an open Escalation is refused by beads (t2 F4). Surface it as `escalation_open`.
- `metrics.ts`: `recordMetrics(wf, attemptId, metrics, {prefix: "metrics" | "review_metrics"})`: flat numeric keys per §14, write-once.
- Commands `attempt list [--status]` and `attempt show <attempt-id>` per §11.

## Acceptance
Integration tests with real `bd`:
1. A full lifecycle on one Job: start, then submit, then reject (requeue), then start Attempt 2, then submit, then approve, then merged. Final Ledger state as in t2 F4 (Attempts in order with Tiers, outcomes, feedback comments, provenance cut/commit/land). The Job is closed with reason `merged`.
2. Two concurrent `startAttempt` calls on the same queued Job (two processes): exactly one returns an Attempt.
3. Write-once: a second `recordAttemptFacts` with the same key fails. `recordMetrics` twice fails.
4. `markMerged` when the Job is not `in_review` gives `illegal_transition`, and nothing is written.
5. Escalation: record, then a duplicate record is a no-op, then `openEscalations` lists it, then closing the Attempt gives `escalation_open`, then answer, then close succeeds.
6. `failAttempt(..., {requeue: false})` makes the Job `failed`, and its dependents stay blocked (`readyJobs`).

## Out of scope
Deciding when to call these (dispatch Jobs); the Tier ladder; herdr; git.
