---
status: accepted
---

# Work is organised as Contexts and Plans

A **Context** is the parent: the shared knowledge between tasks that together make a product, optionally mapped to a Jira Epic, shared by two or more Leads. A Context is not a session; a new session must belong to a Context, an existing one or a new one. A **Plan** is something actionable: today's Workflow, one repo and one PR for now, optionally mapped to a Jira task. Each Plan is a deliverable on its own and can be part of something bigger. Example (User): the Context is "building cpd"; the Plan is what we are doing now, the TUI research and workflow preparation.

Ideally grounding happens once per Context. For simplicity, for now a new Plan starts as a fork of the Context's grounded Lead session. Both uses of "Context" stay (decided: keep both): the entity, and the first phase of every Lead session, which is grounding on a Context. If the Context already exists, that phase is skipped or appended to.

## Consequences

- `CONTEXT.md` defines Context (entity, distinct from the Context phase) and Plan (the entity, today's Workflow, distinct from the Plan phase).
- Each Plan has one Lead; the Lead session is the only one the User interacts with. Jobs go to Workers and are not drawn in the cockpit's tree; they appear in the plan view.
- Tabs in the cockpit jump between the Plans within the same Context.
- Grounding once per Context, without forking, is a later improvement.
- lead-fork's herdr-tab fork command is likely replaced by "a new Plan forks the Context's Lead" (likely, not decided).
