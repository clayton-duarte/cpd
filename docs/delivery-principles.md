# CPD delivery principles

Accepted by the User on 2026-11-16. These are project-wide product decisions, not preferences local to one Context. The living [essence after the live trial](essence.md) applies these principles to the November 23 trial observations; the [pre-live-trial essence](archive/essence-pre-live-trial.md) remains unchanged historical text.

## Own outcomes, not execution activity

CPD makes delegated agent work dependable from intent to verified delivery, without making the engineer supervise every execution step. Track the outcome independently of PR lifecycle: an outcome can require no PR, one PR, or several. A merge, closure, commit, green check or stopped agent proves only that event, not delivery of the outcome. Preserve the User's final authority to merge.

CPD should enforce authority, authorized scope, required evidence, real dependencies and recoverability. Within those boundaries, agents choose how to execute. Preferred sequences are guidance unless a necessary invariant justifies a mandatory transition. Keep the UI that helps the engineer see and direct work; keep the Inbox actionable-only and wake owners because an outcome needs action.

## Roles are boundaries, not mandatory ceremony

The Manager settles product intent with the User, the Lead coordinates bounded work, and Workers execute it. Distinct responsibilities do not require elaborate handoffs for every request. An already-clear request can be delegated immediately as one Job to one Worker.

## Keep cohesive work cohesive

Prefer a single executor for cohesive work. Split only when the real benefit justifies handoff, context, scheduling and integration costs:

- Independent work benefits from parallel execution.
- Separate authorship provides meaningful independent verification.
- Distinct permissions or deliverables need separate ownership.
- Work is too large to specify or verify coherently as one unit.

Separate file families alone do not establish dependency. Review material changes to authorized scope, risk or authority, not the arrangement or renaming of Jobs. Preserve legitimate controls and do not bypass them under the banner of simplification.

## Resolve uncertainty at the appropriate boundary

- Ask the appropriate decision maker about intent or permission.
- Inspect evidence to resolve factual uncertainty.
- Let the authorized executor investigate implementation uncertainty within scope.

Some uncertainty prevents safe work; some can be resolved during reversible execution. Complete knowledge of every implementation detail is not a prerequisite to starting authorized work.

## Match evidence to the deliverable

Documentation needs evidence of accuracy, discoverability and working references. Behavior changes need evidence about behavior. Refactors need evidence that behavior was preserved. Deployment claims require verification of the running instance. Meaningful independent verification remains valuable where the outcome requires it.

Do not require unrelated milestones merely because they belong to a usual sequence. Preserve failed Attempts as evidence. Explicit deferral or disposition is not delivery; a blocked item names a concrete next actor and action. Reject unchanged retries; establish what changed or which supported recovery makes retry viable. Reconcile ambiguous effects before retrying. Quiet, component-specific deployment must preserve work and verify the actual running component.

A stopped agent, passing test, commit, merged PR and production deployment establish different facts. Activity prescriptions do not establish a successful outcome.

## Recoverability is correctness

For every state preventing progress, identify who can resolve it and a supported action that does not depend on the condition being resolved. Legitimate resume, cancellation and recovery must be supported without an external bypass. Recovery does not fabricate success or waive required evidence. Continuing, cancelling and delivering are distinct outcomes.

CPD owns bugs: assign, execute, verify and deliver the root fix. The supervisor owns incidents that prevent CPD from acting: contain the symptom, restore a usable path, record the intervention and queue the root fix through CPD. A workaround is not proof the root cause was fixed.

Keep the Inbox for actions the engineer can take now. Wake owners when an outcome requires action; do not turn stale state, informational notices or bookkeeping into decisions.

## Evidence behind these decisions

During the documentation-simplification Plan, observation of the Lead transcript and registered work showed three exploration Jobs before documentation execution, two cancelled/recreated drafts, two plan reviews, and seven serial documentation Jobs. Setup took roughly eight minutes before the first documentation Attempt started. The first three successful Jobs produced exploration findings, not documentation edits.

The observed problems included repeated discovery to recover missing handoff detail, renewed review of already-authorized work, a serial chain without an established reason for every link, and activity-based commit timing instructions. The Manager's exhaustive-accounting brief also contributed overhead. These are accepted observations of that execution; they do not establish every underlying cause or the quality of its finished artifacts.

The November 23 live-trial observations supplied with User decision pro-d659 (2026-11-23 15:33 EST) include CI failure on PR #1414 waking its owning Lead; approved work stalled after verdict/Lock; a retry against a merged Workflow and stale archive metadata hiding a closed PR; failed jobs and duplicate/informational cards obscuring actionable work; and a Merge card reappearing after acknowledgement with a later click triggering a branch update, while merged fixes did not automatically reach running components. These are concrete observations reported in the decision context, not independently re-run here, frequency estimates or proof of current runtime behavior. The living [essence](essence.md) summarizes them and their implications.

The failure pattern in the simplification run was: **agents optimized for satisfying the process instead of using the minimum process needed to deliver the result.** It is evidence for reconsidering unnecessary choreography, not grounds to remove controls that protect authority, outcomes, evidence or concrete harm.

## Intended behavior and implementation

These principles define product direction. They do not certify that existing runtime machinery conforms, authorize bypassing current safety controls, or settle unspecified transition and migration designs. Documentation must expose such mismatches rather than hide them or claim they are resolved.

CPD should make unauthorized actions difficult, missing evidence visible, and legitimate progress easy. It constrains the boundaries of work—not every movement inside them.
