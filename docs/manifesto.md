# CPD manifesto

Status: draft, based on the primer endorsed by the User on 2026-11-14.

CPD makes agent work dependable from intent through delivery. Parallel execution
serves that purpose: CPD preserves decisions, specifies work clearly, enforces
agreed rules, and keeps the human available for decisions.

## Principles

1. **Decisions survive conversations.** A replacement agent can recover the work
   from durable records and a brief. Retention follows the User's authority.
2. **Responsibilities are distinct.** Product understanding, planning, and
   execution have clear owners. The agent discussing the product remains
   responsive while execution proceeds.
3. **Delegation requires a complete specification.** A Worker receives enough
   context to act without reconstructing its parent's conversation.
4. **Expected behavior comes first.** Document the behavior before implementing
   it. The default delivery sequence is documentation, independently authored
   failing tests, then implementation; exceptions are explicit.
5. **Completion requires evidence.** Acceptance checks and delivery outcomes
   establish progress. A stopped agent, a commit, an approval, a merge, and a
   deployment are different facts.
6. **Rules become enforceable operations.** Instructions teach the rules;
   tooling supplies their guarantees. Copying instructions alone does not
   preserve those guarantees.
7. **Uncertainty reaches the appropriate decision maker.** Agents use settled
   decisions and escalate what would otherwise require guessing.
8. **The process can improve from evidence.** Failures and refusals reveal where
   specifications, checks, and coordination need to change.

## Reuse

CPD's responsibilities should remain understandable independently of an agent
harness, model provider, programming language, or interface. Evaluate reuse in
this order: durable decisions and briefs; Plan and Job specifications;
acceptance and delivery protocols; coordination, attention, and hosting.

The [glossary](../CONTEXT.md) defines the concepts. The [contract](contract.md)
defines their behavior, authority, and exceptions. Implementation profiles and
agent instructions follow those requirements.
