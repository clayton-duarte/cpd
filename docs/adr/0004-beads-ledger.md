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
- `bd` must not modify the product repository (no `AGENTS.md`, hooks or git config changes): init with `--skip-agents --skip-hooks`, never `--stealth` (trial t2).
- Because beads cannot protect Locked Jobs, `cpd` stores a fingerprint of each Locked Job outside beads, verifies it before every Dispatch step, and audits `bd history --events` for writes by any actor other than `cpd` (trial t2).
- A Cancelled Job uses a custom frozen status, not `closed`, so its dependents stay blocked.
