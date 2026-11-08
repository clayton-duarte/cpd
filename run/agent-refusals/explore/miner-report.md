# miner-report a1 — Before/after refusal counts since 2026-11-07T00:00:00Z

Job `mine-report`, attempt 1 (Worker, Tier H3). Deliverable of Workflow `miner`: re-run the miner over the same window as air-d80 and compare with the hand-done scout ranking.

## Method

- The refusal miner of this branch (`pi/tools/refusal-miner.ts`, mine-impl): `node pi/tools/refusal-miner.ts --since 2026-11-07T00:00:00.000Z`, run 2026-11-08 02:45 EST from the worktree `/Users/claytonduarte/projects/cpd.agent-refusals.miner`. It scanned **279 session files** in every directory under `~/.pi/agent/sessions/` (all keys: the home key plus every run-directory key) and found **607 refusals**. Read-only; it writes no file.
- **Before** = the scout counts: `$CPD_HOME/run/agent-refusals/explore/scout-ranking.md` — 356 refusals, snapshot as of 2026-11-07 22:11 EST, **home-key scope only** (64 files under `--Users-claytonduarte--`; Worker and scout sessions outside it by construction).
- **After** = the miner counts above (all keys, live sessions included, so the window runs to 2026-11-08 02:45 EST). Scope is wider than the scout's: Worker/scout surfaces (explore guards) appear here that the scout's scope excluded.
- **Post-fix window** = a second miner run `--since 2026-11-08T06:05:00.000Z` (01:05 EST 2026-11-08, after the last `sources` commit 4e7dd53c7 at 01:04 EST): **31 refusals** — what still fires after the fixes were written.

## Before/after by cause (the 12 causes the Job names)

| Guard / code | Before (scout, home key, → 22:11 EST 11-07) | After (miner, all keys, → 02:45 EST 11-08) | Post-fix window (01:05→02:45 EST 11-08) |
|---|---:|---:|---:|
| cli: usage | 61 | 76 | 9 |
| cli: spec_invalid | 53 | 59 | 1 |
| φ: loop_guard_read_streak (>12) | 49 | 69 | 4 |
| φ: noblock_lead | 42 | 51 | 2 |
| cli: lint_failed | 23 | 34 | 2 |
| cli: outline_invalid | 15 | 16 | 0 |
| cli: shape_incomplete | 13 | 9 | 0 |
| φ: ask_user_question_guard | 8 | 10 | 0 |
| cli: touches_missing | 6 | 7 | 1 |
| φ: primary_checkout | 5 | 5 | 0 |
| cli: shape_violation | 5 | 4 | 0 |
| cli: illegal_transition | 5 | 5 | 0 |
| **Totals (all guards incl. below-table codes)** | **356** (home key) | **607** (all keys) | **31** |

Every "after" count is at or above its "before" count except shape_incomplete (13→9) and shape_violation (5→4): the window grew by ~4.5 live hours and the scope grew from the home key to all session keys, so counts can only rise. A per-cause drop is therefore read from the post-fix window, not from the before/after columns.

## Summary (≤1500 characters)

No cause is removed yet: the fixes are not installed. PR #983 (sources: skills, Spec template, writing-for-agents pointer, CLI help) and PR #985 (enforce: command-example guard) are open, unmerged, as of 02:47 EST 2026-11-08 — a cause only drops after its fix is installed, so a still-shipping install is the expected reading. The post-fix window (records after the last sources commit, 01:05–02:45 EST) still fires: usage 9 (CLI arg/flag errors — the --tier/--spec help fix is in that unmerged PR), loop guard >12 (4) and noblock_lead (2) (behaviour guards the sources fixes only seed, not remove), lint_failed 2, touches_missing 1, spec_invalid 1. Quiet in that window: outline_invalid, shape_incomplete, ask guard, primary_checkout, shape_violation, illegal_transition — but a ~1h40m window with a handful of Lead sessions cannot separate a removed cause from a quiet one; that check is the metrics workflow's job once the Events land. What the fixes target — usage through help text, spec_invalid/outline_invalid through the Spec template and stated fields, lint_failed through stated outline steps — all still appear post-commit, consistent with an install still shipping.
