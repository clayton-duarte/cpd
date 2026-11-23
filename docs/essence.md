# CPD’s essence after the live trial

**CPD should let an engineer own more verified outcomes without spending more attention coordinating them.** Keep the interface that helps the engineer see and direct the work. Remove process that serves itself instead of the outcome.

This is living product direction informed by the November 23 live trial, not a claim that the installed system or runtime already conforms. The original upstream essay is preserved byte-for-byte as the [pre-live-trial essence](archive/essence-pre-live-trial.md). It is distinct from the historical report titled “CPD documentation and essence,” which lives outside this repository at `/Users/Clayton.Duarte/personal/cpd-opencode/reports/CPD documentation and essence.md`; that audit is not the archived upstream essay. The links within this repository point to the archived upstream artifact and the accepted delivery-principles evidence.

## What must survive delegation

- **Intent and authority.** Preserve what was decided, why, what the executor may do, and what needs a fresh human decision. Approval attaches to material scope, risk, and authority—not how work is arranged into rows. Silence is not consent; final merge authority remains with the User.
- **An owned outcome.** Track outcomes independently of PR lifecycle. One outcome may need zero, one, or many PRs. A merged PR does not prove the outcome was met; a closed unmerged PR does not abandon it. Make the next actor and action explicit.
- **Fitting evidence.** Require evidence appropriate to the deliverable: a behavior check, reviewed document and working references, exact revision and deployment verification, cited finding, or evidence that no change was necessary. A green process state or agent assertion is not proof of a different fact.
- **Recoverable continuity.** Decisions, scope, evidence, failed Attempts, and unfinished work survive reconnects and replacement agents. Keep failed Attempts as evidence. Distinguish an explicit pause, deferral, handoff, or disposition from delivery. A blocked item names an actor and a concrete next action. Do not archive an agent with active work unless ownership is transferred or work is explicitly deferred.

## What the evidence supports

The November 23 live-trial observations supplied with User decision pro-d659 (2026-11-23 15:33 EST) were specific: CI failure on PR #1414 woke its owning Lead; approved work remained idle after a verdict and other approved work remained drafts after Lock; a retry was queued against a merged Workflow and stale archive metadata hid a closed PR; red failed jobs outlasted deliberate stops while duplicate review/question cards and informational bug notices competed with actionable decisions; and a Merge card reappeared after acknowledgement, with a later click triggering a branch update, while merged fixes did not automatically reach running components. These observations motivate outcome-driven wakes, explicit dispositions, actionable-only Inbox items, and verifying effects rather than clicks. They are observations reported in the decision context—not independently re-run here, not frequency estimates, and not certification of present behavior.

A separate accepted simplification-run record gives concrete, bounded evidence: three exploration Jobs preceded documentation execution, two drafts were cancelled and recreated, two plan reviews occurred, and seven serial documentation Jobs took roughly eight minutes to reach the first documentation Attempt. Its author notes that the exhaustive-accounting brief contributed overhead and that one execution does not isolate cause or establish typical delay. See [Evidence behind the delivery decisions](delivery-principles.md#evidence-behind-these-decisions). Preserve that uncertainty; do not turn a single run into a universal runtime claim.

## Subtract before adding

Enforce boundaries that protect outcomes, authority, evidence, and concrete harm. Do not prescribe work structure, agent count, or a sequence of docs, tests, implementation and PRs without a necessary reason. Prefer one executor for cohesive work; split only when it buys meaningful parallelism, independent verification, distinct authority/deliverables, or a unit otherwise too large to specify and verify coherently. Review material changes to scope, risk or authority—not row arrangement. This direction does not authorize bypassing current safeguards; legitimate approval, recovery, cancellation and evidence requirements remain.

Keep the Inbox actionable-only: show decisions or actions the engineer can take now, and wake an owner because an outcome needs action—not to report inert state. A successful approval should let authorized work advance, not create more bookkeeping for the User.

For blocked or aborted work, recover, replace, explicitly dispose of, or escalate it. Reject unchanged retries: identify what changed or what supported recovery makes a retry viable. Retain failed Attempts and unlanded evidence; do not make red history look delivered. If no safe disposition exists, name the exact blocker, next actor and action.

Reconcile ambiguous effects before retrying an operation. For deployment, use a quiet, component-specific rollout that preserves work, then verify the actual running instance; a click, branch update or merged PR is not deployment proof.

Treat bugs and incidents differently. **CPD owns bugs**: assign, execute, verify and deliver the root fix. **The supervisor owns incidents** that prevent CPD from doing that: contain the symptom, restore a usable path, record the intervention, and queue the root fix through CPD. A manual nudge or workaround is evidence of a gap, not a reason to normalize supervision.

The practical test is: **Can the engineer stop watching the cockpit and return to verified outcomes or a small number of real decisions?** Keep the UI that helps direct the work. Parallel execution is a consequence of doing this well, not a success metric.
