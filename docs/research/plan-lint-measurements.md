# Plan-lint measurements

Source: the six artefacts in `~/.cpd/run/plan-lint/explore/` named beside each figure (replay-check.md, replay-bound.md, replay-conflicts.md, replay-rows.md, decided-replay.md, rule-reach.md). Figures are copied from them, not recomputed, except where marked (recomputed). Ledger reads were read-only. Times are EDT (America/Toronto).

## Equal-file overlap

A parallel pair is two non-cancelled Jobs of one PR with no dependency path either way. Figures as of 2026-10-30 (replay-check.md, section A):

| Measure | Value | Artefact |
| --- | --- | --- |
| PRs with a parallel pair | 160 | replay-check.md (A); replay-bound.md (a) |
| Parallel pairs | 2,130 | replay-check.md (A); replay-bound.md (a) |
| Pairs overlapping by directory or file | 627 | replay-check.md (A); replay-bound.md (b) |
| Pairs with an equal file | 211 (9.9%) | replay-check.md (A); replay-bound.md (c) |
| Equal-file mentions (pair, path) | 361 | replay-check.md (A) |
| Pairs with an equal file, lockfile-exempt | 209 (9.8%) | replay-check.md (A); replay-bound.md (d) |
| PRs with a lockfile-exempt pair | 69 | replay-bound.md (f) |
| Distinct equal paths | 146 | replay-check.md (A) |

Top equal paths, pairs with lockfiles included: `core/src/store/schema.rs` 26, `Cargo.lock` 21, `docs/contract.md` 21, `Cargo.toml` 20, `docs/commands.md` 15, `docs/implementing.md` 15, `CONTEXT.md` 9, `docs/interface.md` 6 (replay-check.md, section A). Lockfiles account for 2 pairs only (replay-bound.md, section e).

Cutoff reconstruction (replay-check.md, section B). The research's figures match the ledger exactly for every cut from 2026-10-29 20:18:50 to 20:23:00 EDT; the 20:20 EDT cut is one of them. At that cutoff: 262 PRs with an eligible Job, 138 with a parallel pair, 1,995 parallel pairs, 573 overlapping, **184 equal-file pairs (9.2% of 1,995) and 320 equal-file mentions**, 183 lockfile-exempt.

The research's 117 and 96 were never totals. 117 is the sum of six displayed rows (26+21+20+20+15+15), and 412 = 295 + 117 is the sum of the twelve displayed rows (replay-check.md, section C).

## Does overlap predict conflict

Whole ledger, as of 2026-10-30 (replay-conflicts.md). The artefact's verdict: no. Class EQ (equal file) does not have a markedly higher conflict rate than DIR, PAR or SEQ.

| Class | Jobs | Jobs with a merge conflict | Rate |
| --- | --- | --- | --- |
| EQ | 247 | 4 | 1.6% |
| DIR | 102 | 4 | 3.9% |
| PAR | 502 | 0 | 0% |
| SEQ | 560 | 0 | 0% |
| Universe | 1,411 | 8 | 0.57% |

- Precision (pair level): 5 of 209 lockfile-exempt equal-file pairs have a Job with a `merge_conflict` Attempt, 2.4%.
- Recall (Job level): 4 of 8 conflicted Jobs are class EQ, 50%. By Attempt, 6 of 11.
- The ledger holds 15 `merge_conflict` Attempts: 11 in the universe, 4 on `open`/`update` Jobs. 8 of the 11 sit in one Plan (`tests-mock`), so they cluster and are not independent.
- Caveat: 5 hits and 8 conflicted Jobs leave every rate here wide. The artefact computes no interval and this page does not either. Precision of 2.4% is about four times the 0.57% base rate, on 5 pairs.

## decided_uncited

Dropped (decided-replay.md). The rule would warn on **65 of 141 Plans (46.1%)** with at least one uncited `decided` id (135 of 506 ids; last 7 days, 57 of 130 Plans, 43.8%). A warning on nearly half of all Plans, with no link to failure, is not a rule to ship. The artefact reads Jobs as they are now, after Lead edits and locks.

## Frozen rows

`replay-rows.json` (in `~/.cpd/run/plan-lint/explore/`, not in the repo, because it holds other repos' paths; described in replay-rows.md). **sha256 `00ecad1ea4e2a5d97d9237f08c9c4a1994109deeebde360005dd7df7f4460cb4`** (recomputed: matches). 1,973 Jobs, 431 PRs (recomputed: matches).

Oracle, computed from the file alone (replay-rows.md): **2,204 parallel pairs**; **213 pairs** with an equal non-shared file (shared exempted per entry); 215 pairs with any equal file. The artefact attributes the 4 pairs above the 209 to ledger growth; that is unverified.

## Decision

Decision pro-d284 (as stated in this Job's spec): ship the equal-file rule as a warning, lockfiles exempt (D1 option a). Restated bound: **at most 9.9% of parallel pairs** (211 of 2,130; replay-check.md, section A; replay-bound.md, c). It supersedes the research's 5.9%, which was a six-row sum (replay-check.md, section C).

## Replay of the Rust rule

- Engine commit: `5cbf40bedb3ee0c85c1b93894188266892b90e50` (`lint::run`, core/src/plan/lint.rs).
- Rows: replay-rows.json, sha256 `00ecad1ea4e2a5d97d9237f08c9c4a1994109deeebde360005dd7df7f4460cb4` (verified with `shasum -a 256`); 1,973 Jobs, 431 PRs.
- Rows to Jobs: grouped by (plan, pr); every non-cancelled row a `draft` Job, every cancelled row a `cancelled` Job; tier `S1`, spec touches = the row's; one `waiting` PR of shape `free` per group; phase `Dispatch` (so `pr_empty` is off), no outline, empty waiting set. Only `parallel_write_overlap` findings kept.
- Scratch run output (a crate outside the repo, run through `scripts/cargo.sh`): `found 213 unique 213 oracle 213 identical sets: yes`
- Count found: 213. Oracle count: 213. identical sets: yes (compared as (plan/pr, job, other, paths), job/other ordered by key).
- Bound: at most 218 (9.9% of 2,204 parallel pairs); 213 is within it.
- Result: pass
