# Reset-based pacing implementation handoff

This is policy maintenance, not a started/resumed QA discovery run.

## Authorization and decision

On 2026-11-16, after an official authenticated account reading exposed the exact
weekly reset, the User requested updating the budget script for a more accurate
window. Asked whether to replace fixed Toronto weekdays and the Sunday 8 p.m.
hold with continuous pacing from time remaining, preserving fresh telemetry and
new-window authorization, the User replied: “yes, do it”.

Reserve = `100 * (official resets_at - now) / (10080 * 60 seconds)`.
Stop at or below the reserve. No reset weekday or release-hour assumptions.
Legacy fixed/calendar policies remain supported for other adopters.

## Local changes and review dependency

- Canonical source checkout: `/home/clayton/projects/skills-reset-pacing`.
- Source branch: `feat/reset-based-pacing`.
- Source commit: `3634051354021725dc611be4cabff3c6c06a0cc9`.
- Source base: merged skills daily-pacing change from main.
- CPD worktree: `/home/clayton/projects/cpd-reset-pacing`.
- CPD branch: `feat/reset-based-pacing`, stacked on CPD #1362 head
  `0631f36da25699c4fbd27ef9363cded647535cc4`.
- Original CPD worktree is untouched; its local main was behind remote main.
- Consumer snapshot is copied from the exact source commit, with updated hashes.
- Config selects `reset_pacing: true`; documentation records supersession.

Neither branch was pushed; no issue or PR was created. Source review/merge must
precede consumer publication. Refresh the source pin if review/merge changes its
revision, and reconcile CPD with current main/#1362 before publishing.
Do not merge either change autonomously.

## Validation

- Source: 42 Node tests and 6 Python reader tests passed.
- Consumer: 44 Node budget/compatibility/snapshot tests passed.
- Node syntax checks and `git diff --check` passed.
- `scripts/check-portable.sh` passed (22 files, 0 exempt).
- No configured JS formatter/linter; syntax/whitespace are not semantic lint.
- Regression cases include exact floors, any reset weekday, DST, malformed
  policies, stale/future samples, expired windows, old run authorization,
  account refusals, final pre-write checks and reset changes during pagination.
- No full product build or remote CI was run for this dependency-free tooling.

## Latest live inspection (historical snapshot, not ongoing permission)

Authenticated official sample: `2026-11-16T15:14:56.779431+00:00`.
Weekly remaining: 84%; window: 10,080 minutes.
Official reset: `2026-11-23T00:17:22+00:00`.
Account available; spend control not reached.

The modified CPD status command computed reserve `91.09446808862434%`,
spendable `0%`, `allowed: false`, `run_required: true`, both creation flags false.
Counts were 0 issues/2 PRs today; null cardinal caps. No run descriptor was
created or revived. Stop reason: requested implementation is prepared locally;
the live allowance refuses QA and guarded publication. Reauthenticate only if
the reusable official login is actually unavailable, not on ordinary restarts.

Next safe action: User review of local changes. Obtain fresh telemetry before
any later QA/publication and preserve explicit current-window run authority;
do not bypass the guard to create review resources while it refuses creation.
