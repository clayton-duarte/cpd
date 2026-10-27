---
status: refined by ADR-0014
---

# After Dispatch starts, the Plan is append-only

Once the User approves the Plan, every Job is Locked: its Spec, Tier and Dependencies never change and nothing is deleted. Change happens only by re-attempting a Job, cancelling it, or appending new Jobs. This keeps a truthful history for retrospectives and post-mortems, at the cost of the Workflow accumulating cancelled and superseded Jobs.
