---
status: accepted
---

# The control plane is the `cpd` CLI

Everything deterministic in a Workflow (Ledger writes, scheduling, concurrency cap, timeouts, retry ladder, worktrees, merge queue, Gate, metrics, PR polling) is implemented as `cpd` commands that any agent or script can call, rather than as tools inside one agent's extension system. v0 runs pi only, so the pi-specific parts (waking the Lead without clobbering input, plan review UI glue) are a thin pi extension over `cpd`. This keeps the path open to Claude Code and Hermes without rewriting the core, at the cost of a small process boundary between the Lead and its tools.
