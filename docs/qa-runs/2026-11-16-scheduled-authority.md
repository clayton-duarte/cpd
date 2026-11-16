# Scheduled QA setup handoff — 2026-11-16

## Authority and scope

The User requested daily 3 a.m. Toronto cron wakes of this existing OpenCode
chat on homeserver, corrected weekly reauthorization as a bug, then instructed
implementation. Exact recorded authority:
https://github.com/clayton-duarte/cpd/issues/1115#issuecomment-6921840796

This is policy/scheduler setup, not a started QA discovery run. No new agent,
session fork, blanket permission grant, product deployment or autonomous merge.
Standing recurring authority replaces manual weekly approval, not live budget
checks. Optional run IDs/start/caps remain immutable audit scopes.

## Source and consumer

- Source: [skills #10](https://github.com/clayton-duarte/skills/pull/10), branch
  `feat/scheduled-qa-authority`, commit
  `e5e3accbb8f81d108951c7d2a97645f0c276f743`.
- Source checkout: `/home/clayton/projects/skills-reset-pacing`.
- CPD worktree: `/home/clayton/projects/cpd-scheduled-qa`, same branch name.
- CPD base: `154a9feac3b2fc299fa35a469217500bb359c0f4` (merged #1363).
- CPD's manifest records the exact source package revision and file hashes.
- CPD remains draft until source review/merge, exact-tree verification and
  repinning to the canonical merge revision. User reviews and merges both.

## Scheduler implementation

`scripts/qa-wake.py` sends a durable queued/resuming prompt to an exact existing
session through `opencode api`. It checks target identity/location/archive
state, active execution and pending inbox; takes a POSIX lock; records an
at-most-once daily attempt before POST. Ambiguous failure requires inspection,
not automatic resend. It never creates sessions or grants tool permissions.

The hourly minute-zero cron checks Toronto's hour in Python and only contacts
OpenCode at 3 a.m.; other hours invoke no model/API. This avoids relying on
cron-specific timezone extensions. The host was verified as homeserver with
America/New_York timezone; no timezone change is needed.

The exact current session was verified via read-only API. Its target settings
are staged privately at `/home/clayton/.local/state/cpd-qa/wake.pending.json`,
outside Git; no credentials are copied. The original primary checkout is left
untouched and is behind remote main, so it must be safely fast-forwarded after
consumer merge and before preflight/activation. Review local work first.

The existing unrelated cron entry is preserved. **No QA cron has been installed,
no live wake prompt sent, no new model session created, and no discovery loop
started.** The wrapper is not an atomic idle-only scheduler or a distributed
group lock; a racing manual message can queue the wake and unrelated operators
must still respect exclusive ownership.

## Checks

- Source: 46 Node tests, 6 Python reader tests passed; syntax/whitespace passed.
- CPD: 49 Node tests passed (including the adapter invoking 8 isolated Python
  wake tests); snapshot integrity passed.
- Actual Rust terminology guard passed, compiled directly from its unchanged
  std-only source to scan this checkout.
- Portable check passed (25 scripts, no exemptions); `git diff --check` passed.
- No JS formatter/linter configured; syntax/whitespace are not semantic lint.
- No full product build or completed remote CI is claimed.

Live post-change status at `2026-11-16T16:13:48.232088+00:00`: 83% remaining,
official reset `2026-11-23T00:17:22+00:00`, account available, spend control not
reached. The guard computed reserve `90.51021428571428%`, zero spendable,
`allowed: false`, and `run_required: false`. Standing authority correctly removes
the weekly descriptor gate while still refusing general QA at the reserve.
Counts: 0 issues/5 PRs today across the group. This is historical telemetry, not
an ongoing permission sample.

## Next action after User review

1. Merge source, verify merged package, update CPD pin, mark consumer ready.
2. After CPD merge, update the clean primary checkout without discarding work.
3. Review the private target config and cron PATH (Node/Codex/Git/GitHub CLI).
   Run non-delivering `--check`; busy during this conversation is an expected skip.
4. Back up current crontab, add one uniquely marked QA entry while preserving the
   unrelated existing entry, and verify the installed entry. Do not trigger a
   real wake merely to test setup.
5. Cron invokes the same session; its first action is fresh usage and guard
   inspection. Budget remains authoritative; no weekly approval question.
   Missing credentials/permissions pause rather than enabling unrestricted tools.

See [schedule setup](../qa-schedule.md) for cron, API, failure and disable rules.
