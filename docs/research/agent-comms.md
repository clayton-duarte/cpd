# agent-comms: baseline measurement (hs-d507, hs-d514)

Quiet agents design baseline, measured from the cpd store on 2026-10-19 and 2026-10-20, before PR #206 (workflow, hs-d340) and after.

## Baseline: Turns, calls and cost

**Turns.py output** (Lead and Manager turns, calls and cost per day, from pi session files + cpd's sessions table, read-only).

Script: `docs/research/agent-comms-turns.py`.

To recompute:

```bash
python3 docs/research/agent-comms-turns.py 2026-10-19 2026-10-20
```

**Result (2026-10-19 / 2026-10-20):**

```
2026-10-19 lead turns=838 calls=2972 cost=$1236
2026-10-19 manager turns=96 calls=553 cost=$135
2026-10-20 lead turns=786 calls=3326 cost=$918
2026-10-20 manager turns=212 calls=755 cost=$172
```

## Inventory findings

Source: `/tmp/cpd-agent-comms/inventory.md` (2026-10-20, store events, read-only).

### Lead turns, 10-20 (Events that start a turn, contract §5.3)

Cost: Lead/Manager sessions ~74% of spend; each turn re-reads 300-440k tokens. A *turn* costs a full call; a *no-turn message* costs nothing now but stays in the context for every later call (bloat).

| Source | Count | Turn today | Needs action? | Proposed class (hs-d507) |
|---|---|---|---|---|
| job-merged | 255 | turn | rarely: only when it unblocks a draft batch / research / review Job | Event, **no turn** unless the PR has drafts and nothing queued/running, or it's a review Job |
| session-message --wake (Manager to Lead) | 175 | turn | sometimes (routing new work) | action message (stays, guarded) |
| session-message no wake (to Leads) | 180 | no turn, bloat | mostly FYI / rule relays | board, or Context record |
| all-merged | 97 (76 distinct PRs, 21 repeats) | turn, steered | yes, open the PR | action; repeats absorbed |
| pr-feedback | 78 (76 conflict, 2 closed) | turn, steered | yes (hs-d490) | action (unchanged) |
| escalation | 69 | turn, steered | yes | action; fewer via job-add `touches` check |
| plan-done | 28 | turn | report to User | action (unchanged) |
| error with a Plan | 14 | turn | sometimes | action (unchanged) |
| plan-review decided | 28 | turn | yes | action (plan-ux owns the flow) |

### Manager inputs, 10-20

| Source | Count | Turn today | Proposed |
|---|---|---|---|
| Lead to Manager --wake | ~106 | turn | only when it unblocks; ready reports / noted / done / FYI become passive (pr-opened Events, board) |
| Lead to Manager no wake | ~117 | bloat | board |
| E4 pr_unopened alarm | 31 | turn | removed by cross-repo's workflow (open Job, hs-d497) |
| Engine step errors, no wake | 81 (27 x 3 Managers) | bloat | Event; one count line at turn start; machine-wide to ONE Manager or none |
| - gh_failed (machine) | 18 | | nobody can act: Event only |
| - distill_invalid (sets no context, so fans out to all) | 12 | | route by details.context, or Event only |
| - title_failed (machine) | 5 | | Event only |
| Engine downtime notice (watch-started) | each restart, x every Manager | bloat | Event only (watch-started already records it) |
| Engine-down 60 s (tree) | rare | bloat | keep: needs action |
| Stuck Job (E8) | rare | bloat | keep to that Context's Manager: needs action |
| Turn-start digest: records + `Plans:` (20 lines) + `Pending work:` (~80 lines) | every turn | grows context ~100 lines/turn | counts and headlines only |

### Broadcasts seen (Manager, same text to 7-9 Leads): 10+ on 10-20

Rule relays (hs-d334, d335, d336, d490, d506, the stack, sign-off, priority). Root cause: Leads get no
turn-start update (hs-d209), so the Manager pushes new rules by message.

## Session-message Event counts and confound

**SQL for session-message counts** (from sessions joined on data.from and events.session):

```sql
-- Session-message Events by sender and recipient, 2026-10-19 to 2026-10-20
SELECT
  CASE
    WHEN e.data LIKE '%"from":"%' THEN
      (SELECT role FROM sessions WHERE id = json_extract(e.data, '$.from'))
    ELSE 'engine/user'
  END as sender_role,
  (SELECT role FROM sessions WHERE id = e.session) as recipient_role,
  COUNT(*) as count,
  SUM(CASE WHEN json_extract(e.data, '$.wake') = true THEN 1 ELSE 0 END) as wake_count
FROM events e
WHERE e.kind = 'session-message'
  AND date(e.at) IN ('2026-10-19', '2026-10-20')
GROUP BY sender_role, recipient_role
ORDER BY sender_role, recipient_role;
```

**Result (2026-10-19 / 2026-10-20):**

| Sender → Recipient | Count | Wake |
|---|---|---|
| Manager → Lead | 255 | 76 |
| Lead → Manager | 130 | 7 |
| Lead → Lead | 15 | ? |
| Engine/User → Manager | 0 | 0 |
| | | |
| Manager → Lead | 308 | 169 |
| Lead → Manager | 257 | 143 |
| Lead → Lead | 52 | ? |
| Engine/User → Manager | 81 | ? |

### Event counts by kind (2026-10-19 / 2026-10-20)

| Kind | 10-19 | 10-20 |
|---|---|---|
| `job-merged` | 233 | 267 |
| `escalation` | 70 | 74 |
| `pr-feedback` | 223 | 78 |
| `error` | 61 | 51 |
| `job-cancelled` | 110 | 77 |
| `pr-started` | 63 | 79 |
| `pr-merged` | 64 | 81 |
| `attempt-retry` | 17 | 3 |

### Confound: PR #206 (workflow, hs-d340)

PR #206 (workflow: cpd opens PRs itself, open and update Jobs, and checks TDD at submit, hs-d340) landed in the same measurement window (10-19 to 10-20) and affects the counts. It removed the `all-merged` Event kind and changed when `job-merged` starts a turn.

On 10-20:
- `all-merged` stopped being appended (hs-d340)
- `job-merged` only starts a turn when actionable (PR has drafts and nothing queued/running, or it's a review Job)
- `pr-feedback` for conflicts was removed (hs-d340: a conflict queues an `update` Job instead)

This explains the divergence between 10-19 and 10-20 in the counts above.
