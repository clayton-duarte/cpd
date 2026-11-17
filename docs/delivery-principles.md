# CPD delivery principles

Accepted by the User on 2026-11-16. These are project-wide product decisions, not preferences local to one Context. They refine the original CPD essence analysis without changing its verbatim historical text.

## Enforce contracts, not choreography

CPD makes delegated agent work dependable from intent to verified, reviewed delivery. It enforces authority, authorized scope, required outcome evidence and real dependencies. Within those boundaries, agents choose their execution strategy.

The delivery contract establishes:

- **Authority:** who may decide or perform an action.
- **Scope:** the authorized outcome and its boundaries.
- **Evidence:** what demonstrates that this particular outcome is acceptable.
- **Coordination:** which real dependencies prevent work from starting.
- **Recovery:** how authorized work continues after failure or interruption.

Preferred sequences are guidance unless a necessary invariant justifies a mandatory transition. Parallelism serves delivery; maximizing agents, tasks or activity is not the goal.

## Roles are boundaries, not mandatory ceremony

The Manager settles product intent with the User, the Lead coordinates bounded work, and Workers execute it. Distinct responsibilities do not require elaborate handoffs for every request. An already-clear request can be delegated immediately, specified as one Job and completed by one Worker.

## Keep cohesive work cohesive

Start clear, cohesive work as one execution unit. Split only when the cost of handoffs, lost context, repeated reading, scheduling and integration is justified by:

- Independent work that benefits from parallel execution.
- Meaningful verification by a separate author.
- Distinct permissions or deliverables requiring separate ownership.
- Work too large to specify or verify coherently as one unit.

Bounded work does not mean fragmenting everything. Separate file families alone do not establish a dependency; each mandatory ordering needs a real reason.

## Resolve uncertainty at the appropriate boundary

- Ask the appropriate decision maker about intent or permission.
- Inspect evidence to resolve factual uncertainty.
- Let the authorized executor investigate implementation uncertainty within scope.

Some uncertainty prevents safe work; some can be resolved during reversible execution. Complete knowledge of every implementation detail is not a prerequisite to starting authorized work.

## Approve the contract, not its representation

Approval attaches to the authorized scope, risk and authority—not the serialization or arrangement of Jobs. A material change to that contract may require approval. Renaming or reorganizing Jobs, improving a Spec, or discovering an actual file path does not by itself require renewed approval.

Keep human attention for genuine decisions, rather than repeated approval of already-authorized work.

## Match evidence to the deliverable

Documentation changes need evidence about accuracy, discoverability and references. Behavior changes need evidence about behavior. Refactors need evidence that behavior was preserved. Meaningful independent verification remains valuable where the outcome requires it.

Do not require unrelated milestones merely because they belong to a usual sequence. A stopped agent, passing test, commit, merged pull request and production deployment establish different facts. Activity prescriptions, such as arbitrary commit intervals, do not establish a successful outcome.

## Justify constraints before enforcing them

Deterministic enforcement is valuable only after establishing that the constraint deserves to exist. An enforced unnecessary rule can stop delivery. Reassess the need for a rule before strengthening its enforcement.

Guardrails must not accumulate into another rigid process. Avoid new universal simplification protocols, elaborate small/medium/large or fast-lane eligibility taxonomies, extra agents reviewing procedural compliance, and guards that merely compensate for contradictory guards.

## Recoverability is correctness

For every state preventing progress, identify who can resolve it and ensure that the resolving action does not depend on the condition being resolved. Legitimate resume, cancellation and recovery must be supported without an external bypass.

Recovery does not fabricate success or waive required evidence. Continuing or cancelling work is distinct from declaring its result acceptable.

Deadlocks, internal loops and circular prerequisites are design failures, not merely inconvenient edge cases. Prefer fewer mandatory transitions, not merely a shorter mandatory workflow.

## Evidence behind these decisions

During the documentation-simplification Plan, observation of the Lead transcript and registered work showed three exploration Jobs before documentation execution, two cancelled/recreated drafts, two plan reviews, and seven serial documentation Jobs. Setup took roughly eight minutes before the first documentation Attempt started. The first three successful Jobs produced exploration findings, not documentation edits.

The observed problems included repeated discovery to recover missing handoff detail, renewed review of already-authorized work, a serial chain without an established reason for every link, and activity-based commit timing instructions. The Manager's exhaustive-accounting brief also contributed overhead. These are accepted observations of that execution; they do not establish every underlying cause or the quality of its finished artifacts.

The failure pattern was: **agents optimized for satisfying the process instead of using the minimum process needed to deliver the result.**

## Intended behavior and implementation

These decisions define product direction. They do not certify that existing runtime machinery already conforms, authorize bypassing current safety controls, or settle unspecified transition and migration designs. Documentation must expose such mismatches rather than hiding them or claiming they are resolved.

CPD should make unauthorized actions difficult, missing evidence visible, and legitimate progress easy. It constrains the boundaries of work—not every movement inside them.
