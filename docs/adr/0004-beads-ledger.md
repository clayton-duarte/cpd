---
status: accepted
supersedes: ADR-0001
---

# The Ledger is beads, written only through `cpd`

Each Workflow's Jobs, Dependencies, Attempts, reviews and Escalations live in a beads (`bd`) workspace stored outside the product repository (`BEADS_DIR`), instead of a custom SQLite schema. beads already provides the dependency graph, the ready set, atomic claims, an audit trail and Dolt version history, plus epics and cross-repo dependencies we want after the MVP.

## Consequences

- beads cannot enforce Locked or append-only (`edit`, `delete`, `compact` exist). `cpd` is the only writer and enforces both; nothing else may call `bd` mutating commands, and `compact`/`prune`/`purge` are never used. Dolt history remains as forensic backup.
- Embedded Dolt allows one writer at a time, which `cpd` serialises.
- A Dependency is satisfied when its blocker is closed, so `cpd` closes a Job only when it is Merged.
- `bd` must not modify the product repository (no `AGENTS.md`, hooks or git config changes).
