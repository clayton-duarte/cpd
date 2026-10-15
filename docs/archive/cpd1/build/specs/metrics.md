# Job `metrics`: pi session metrics parser

Tier S2 · Needs: `scaffold` · Read `_common.md` first. Contract section: §14. Trial evidence: `docs/trials/t1-spawn.md` (F2 session shapes), `docs/research/raw/w5-observability-safety.md` (pi session JSONL ground truth, first section).

## Goal
A pure function that turns one pi session JSONL file into the per-Attempt metrics `cpd` records.

## Files you own
`src/metrics/session.ts`, `test/metrics/**` (fixtures under `test/metrics/fixtures/`).

## Requirements
- `sessionMetrics(path)` returns `{turns, tokensIn, tokensOut, cacheRead, cacheWrite, costUsd, durationS, model, thinkingLevel, finalStopReason}` per §14.
  - Read line by line, validating each relevant entry with zod (`session` header, `model_change`, `thinking_level_change`, `message` with `role` assistant and `usage`).
  - Skip other entry types, but count them in a `skipped` field for debugging.
  - Fail loud on malformed JSON lines (`session_malformed` with the line number).
  - `model` and `thinkingLevel` are the last values seen.
- Fixtures: build them by copying small real sessions from `~/.pi/agent/sessions/--Users-claytonduarte-projects-cpd--/` or from `/tmp/cpd-trials/*/sessions*`. Pick short ones under 200 KB and **redact all message text** (keep structure, usage and timestamps). Include one finished session (`stop`) and one interrupted (`toolUse`).

## Acceptance
1. Totals for each fixture equal an independent `jq` computation over the same file. Put the jq command in a test comment and its output in the report.
2. A malformed line gives `session_malformed` with the right line number.
3. An empty session (header only) gives zeros and `finalStopReason: null`.

## Out of scope
Locating the session file; writing to the Ledger.
