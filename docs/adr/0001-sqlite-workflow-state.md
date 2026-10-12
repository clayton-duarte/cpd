---
status: superseded by ADR-0004
---

# SQLite is the single source of truth for a Workflow

Workflow state (Jobs, Specs, Dependencies, Attempts, reviews, Escalations, events) lives in one SQLite database per Workflow, stored outside the repository so it survives worktree removal. We chose it over Markdown/YAML plan files because the state must be queryable (which Jobs are Ready, which parallel Jobs overlap), must outlive the Lead's context window, and its invariants (Locked Jobs, no deletes, valid Tiers) can be enforced by constraints and triggers instead of by prompt.
