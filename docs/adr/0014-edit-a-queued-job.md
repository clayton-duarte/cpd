---
status: accepted
refines: ADR-0003
---

# ADR 0014: Edit a queued Job's Spec before its first Attempt

## Context

ADR-0003 promised that once a Plan is approved every Job is Locked: its Spec, Tier and Dependencies never change. On another Plan one wrong sentence in a queued Job's Spec cost cancelling the Job and, because cancelling fails every queued dependent (hs-d647), its whole chain, then re-adding all of them. Nothing had read that Spec yet: no Worker had started.

## Decision

The User decided (Context record pro-d79) that the Lead may correct the Spec of a queued Job before its first Attempt.

- `cpd job edit <key> --spec <file>` is allowed on a Job that is `queued`, has no Attempt row of any status, and is not an `open` or `update` Job, in any phase after the lock. Only the Spec changes: `goal`, `context`, `touches`, `acceptance`, `out_of_scope` and, for a `test` Job, `tests` and `test_files`.
- Title, Tier, PR, kind and Dependencies stay Locked, and so does every other Job: a started, merged, failed or cancelled one, a queued one that had an Attempt, an `open` or `update` Job. They are refused `job_locked`.
- Only the Lead edits (`not_a_lead` when `CPD_ATTEMPT` is set). There is no Plan review and no User approval; the Lead tells the User in chat. The plan fingerprint covers drafts only, so a pending review stays current.
- One write transaction re-reads status and Attempts, updates the Spec, recomputes the Locked-Job fingerprint and appends a `job-edited` Event. The `history` rows keep the old and the new text. The engine starts an Attempt in a transaction of its own, so the edit or the Attempt wins, never both.
- The trigger `jobs_locked` takes the exception (contract §4.4; migration `job-edit-queued`).

Everything else of ADR-0003 stands: nothing is deleted, and change after a Job has started is by retry, cancel or append.

## Consequences

- The trigger no longer stops a raw write to the Spec of such a Job. The fingerprint (the stored one no longer matches a write made outside cpd), `history` and `txns` still show it.
- Nothing is lost: the replaced text stays in `history`.

## Rejected alternative

A User approval per edit. The edit corrects a Spec no Worker has read, and the User sees what changed in chat; an approval dialog for each typo would cost more than the cancel-and-re-add it replaces.
