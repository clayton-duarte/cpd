# Job `escalation-expiry`: close expired Escalations in the Ledger

Tier S2 · Needs: `ledger-attempts`, `queue-ask` (both merged) · Read `_common.md` first. Contract: §11 (`cpd ask`, expired questions are renamed `<esc-id>.expired.json`), §8.

## Goal
An Escalation whose question expired (timeout or dead asker) stays open in the Ledger today, and an open Escalation makes `failAttempt` and `markMerged` refuse with `escalation_open` (found by the `ledger-attempts-a1` review). Add the missing operation.

## Files you own
`src/ledger/escalations.ts` (add a function only; keep existing exports unchanged), `test/ledger/escalations.test.ts`.

## Requirements
- `expireEscalation(wf, escId, {reason: "timeout" | "orphan", expiredAt})`:
  - comment `expired: <reason>`, write-once metadata `expired_at` and `expired_reason`, then close with reason `expired`;
  - idempotent (an already closed Escalation returns `{recorded: false}`);
  - refuses an Escalation that was answered (`escalation_answered`, exit 3).
- `openEscalations` must not list expired ones.

## Acceptance (real `bd`)
1. Record, expire, then `failAttempt` and `markMerged` paths are no longer blocked.
2. A second expire is a no-op. Expiring an answered Escalation gives `escalation_answered`.
3. `./verify.sh` passes.
