# QA run handoff

- Run ID and canonical User authorization reference:
- Config source/commit; complete repository allowlist and base branches:
- Descriptor (paste its exact JSON); scope marker; authorized reset batch or none:
- Current phase/selected issue; base/head SHAs, branch and worktree status:
- Current guard JSON (timestamp); day/run counts and remaining caps:
- Findings/duplicates/retracted hypotheses and links:
- Backlog prioritization and research/decision links:
- Reproducer (inline minimal code/fixture + exact command; no temp-only dependency):
- Changed behavior, tests and environment; original/fixed results:
- Baseline failures reproduced on unchanged code, blocked checks and limits:
- PR URLs, inspected head/base SHAs and mergeability (including pending/conflicted),
  latest CI/review status and unfinished upkeep:
- Human-input questions/labels; no open question is silently decided:
- Authorized weekly window/reset and immutable run start (if configured):
- Standing authorization reference and scheduler enabled/disabled state (if configured);
  weekly window is audit only in this mode, not a request for renewed approval:
- Account meter, if supplied: source/time, remaining percent before/after,
  resolution/uncertainty, unrelated usage; never infer a precise token amount:
- Stop reason and next safe action; User review/merge remains pending:

A new session reads this record, the issue/PR updates, config and Protocol,
rechecks GitHub budgets and verifies current base. It never trusts these copied
counts as the ledger or restarts an exhausted run under a new ID.
