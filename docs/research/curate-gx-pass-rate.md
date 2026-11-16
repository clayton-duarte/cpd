GX pass rate at batch 60: 8 valid of 11 runs (72.7%); the 3 failures were 1 `length` and 2 `distill_invalid` (`short` over 120 bytes); counting the shrink retries, 7 of the 18 pi runs failed.

# GX curation pass rate on building-cpd (hs-d558)

Measured by Job `measure-gx` (runs) and written by `measure-gx-report` (this file, no new run). Tier `GX` = `github-copilot/gemini-3.8-flash:off`, one step, no fallback (hs-d562). Source: `~/.cpd/run/curate/measure-gx-data/`: `measure-gx-out.log` (the log), `run/context-digests/building-cpd/runs.jsonl` (one line per pi run), `cpd.db` (read with `sqlite3 -readonly`), the last run's `batch.md`, `known.md`, `docs.md`, `prompt.md`. The scratch merge is `/tmp/measure-gx` (it still existed).

How the two sources line up: the log has 18 `### run` headers and `runs.jsonl` has 18 lines, in the same order, with the same results (valid, valid, failed, ...); the invalid runs' `tokens_out` fits the batch sizes (section 3), and the sum of `curated` in the log (526) equals the rows of `record_curations` (526). `curated_at` timestamps equal each run's end.

Backlog size: the spec says 751 records; the log says 708 remaining after run 1 (60 curated), i.e. 768 uncurated records in force at the start. I use 751 (the spec) and show 768 where it matters. `record_curations` holds only `decision`, `note`, `answer`, `question` records (never `plan`/`brief`), and only records in force are batched.

## 1. Runs

"Run N" is the loop's counter (a state of the store); "batch" is `--batch`. A failed state is retried at 30, 15, 5, 1. `curated` is what the run wrote (the log's JSON). Seconds, tokens and cost are `runs.jsonl`.

| # | run | batch | result | curated | seconds | tokens in | tokens out | cost $ |
|--:|----:|------:|--------|--------:|--------:|----------:|-----------:|-------:|
| 1 | 1 | 60 | valid | 60 | 255.7 | 12697 | 4252 | 0.02547 |
| 2 | 2 | 60 | valid | 60 | 257.3 | 16114 | 3915 | 0.02677 |
| 3 | 3 | 60 | **failed `length`** (`distill_failed`, "the final assistant message stopped with `length`") | 0 | 302.9 | 20239 | 2559 | 0.02478 |
| 4 | 3 | 30 | valid | 30 | 343.5 | 12979 | 2149 | 0.01779 |
| 5 | 4 | 60 | valid | 60 | 325.2 | 21425 | 3654 | 0.02977 |
| 6 | 5 | 60 | valid | 60 | 374.5 | 23061 | 4185 | 0.03299 |
| 7 | 6 | 60 | valid | 60 | 306.4 | 24543 | 4075 | 0.03369 |
| 8 | 7 | 60 | valid | 60 | 218.5 | 28326 | 4034 | 0.03637 |
| 9 | 8 | 60 | **invalid**: `hs-d323` line 38 "short must be 1 to 120 bytes on one line" (the only problem) | 0 | 249.9 | 30761 | 4295 | 0.03918 |
| 10 | 8 | 30 | **failed `length`** | 0 | 317.3 | 22953 | 2556 | 0.02404 |
| 11 | 8 | 15 | valid | 15 | 260.6 | 19861 | 1091 | 0.01899 |
| 12 | 9 | 60 | **invalid**: `hs-d339` line 39, `short` > 120 bytes (the only problem) | 0 | 264.6 | 31917 | 4399 | 0.04043 |
| 13 | 9 | 30 | **invalid**: `hs-d307` line 10, `short` > 120 bytes (the only problem) | 0 | 237.8 | 23892 | 2205 | 0.02343 |
| 14 | 9 | 15 | **invalid**: `hs-d301` line 1, `short` > 120 bytes (the only problem) | 0 | 208.2 | 20183 | 1114 | 0.01931 |
| 15 | 9 | 5 | **invalid**: `hs-n277` line 5, `short` > 120 bytes (the only problem) | 0 | 46.2 | 18361 | 376 | 0.01518 |
| 16 | 9 | 1 | valid | 1 | 32.9 | 17417 | 81 | 0.01337 |
| 17 | 10 | 60 | valid | 60 | 280.7 | 31969 | 4334 | 0.04023 |
| 18 | 11 | 60 | valid | 60 | 156.7 | 28494 | 3892 | 0.03597 |

Valid runs write 3 to 19 doc candidates and 0 to 27 retirement proposals each (log: run 1 `retire` 24 `docs` 0; run 2 18/0; run 3@30 14/0; run 4 0/0; run 5 15/2; run 6 10/8; run 7 18/1; run 8@15 7/0; run 9@1 1/0; run 10 27/4; run 11 3/19).

Every `distill_invalid` has exactly one entry in `errors`, and it is always `short must be 1 to 120 bytes on one line` (no topic error, no retire error, no missing line, in 5 of 5).

## 2. Totals

- **Pass rate at batch 60: 8 / 11 = 72.7%** (runs 1, 2, 4, 5, 6, 7, 10, 11 of the loop). Over all 18 pi runs: 11 valid = 61.1%.
- **Smallest batch that passed, for each failed state:** run 3: 30 (60 `length`, 30 valid). Run 8: 15 (60 invalid, 30 `length`, 15 valid). Run 9: 1 (60, 30, 15 and 5 invalid, each on a different record, 1 valid). So 3 of 3 failed states recovered by the shrink, after 1, 2 and 4 extra pi runs (7 extra pi runs, 7 failed).
- **Records curated:** 526 in 11 valid runs (60 x 8 + 30 + 15 + 1); 242 remained at the end (log), 18 topics, 137 retirement proposals, 34 doc candidates (section 4).
- **Cost:** whole measurement $0.4977 (18 runs, 4439 s = 74 min of pi time, 405,192 tokens in, 53,166 out), of which the 7 failed runs cost $0.1863 (37%). Valid runs only: $0.3114.
- **Cost per valid run at 60:** mean $0.03266 (min $0.02547, max $0.04023), mean 23,329 tokens in, 4,043 out, 272 s. It grows with the number of curated records, because `known.md` lists them all: fitting the eight valid 60-runs, cost = $0.0258 + $0.0000290 per record already curated (my fit, 8 points: indicative only); tokens in grew from 12,697 (0 curated) to 28,494 (466 curated).
- **Whole 751-record backlog, extrapolated from zero:** (a) with no failures, 12.5 runs: flat mean $0.41, with the growth fit $0.45 (768 records: $0.46); (b) with the measured failure overhead (all-in $0.000946 per curated record, valid-only $0.000592): $0.71 all-in (751 records), $0.44 valid-only. Time: 272 s mean per valid 60-run, so about 57 min for the backlog without failures (`seconds` of valid 60-runs x 12.5); the measurement took 74 min for 526 records.
- **Last run's inputs (run 11, 60 records, 466 known):** `batch.md` 23,728 bytes; `known.md` 59,667 bytes (466 lines, 128 bytes each, plus the 18 topic lines). Their token estimate: the run's whole input (prompt.md 3,277 + docs.md 1,573 + batch.md + known.md = 88,245 bytes) was 28,494 tokens in = 0.323 tokens/byte (3.1 bytes/token), so `batch.md` is about 7,700 tokens and `known.md` about 19,300 tokens (the plain bytes/4 rule would say 5,900 and 14,900). At 751 records `known.md` is about 96 KB (about 31,000 tokens), linear.

## 3. The failures in depth

### `length` (2 runs: loop run 3 at 60, loop run 8 at 30)

| run | batch | tokens out | tokens out / line if complete |
|---|---|---|---|
| failed | 60 | 2559 | - |
| failed | 30 | 2556 | - |
| valid at 60 (8 runs) | 60 | 3654 to 4334 (mean 4043) | 61 to 72 (mean 67.4) |
| valid at 30 (run 3) | 30 | 2149 | 71.6 |
| valid at 15 / 5 / 1 | | 1091 / - / 81 | 72.7 / - / 81 |

- The two failed runs stopped at **2559 and 2556 tokens out**: 3 tokens apart, and not a round number (not 2048, 4096, 8192). So the data does not show a round output limit. Valid 60-runs wrote up to 4334 tokens, so the limit, if there is one, is not 2.55k of visible output. Guess, unverified: the cut counts something the log does not show (hidden reasoning?) or is a server-side cap; the log cannot tell.
- The run at batch 30 (2556 tokens) wrote more than a complete 30-line answer needs (2149 tokens in the valid run at 30; about 72 tokens per line); so it was not cut inside a normal answer: it was an answer longer than the batch (I infer a run-on or repeated output; unverified, because a `length` failure keeps no file: `distill.log` is empty and there is no `.invalid` for it).
- The 60-run at 2559 tokens would be about 38 complete lines (2559 / 67.4), unverified.
- `length` ended 1 of the 11 states at batch 60 (loop run 3) and 1 of the 3 tries at batch 30 (loop run 8; the other two at 30 were valid and invalid). It is not a function of batch size alone: batch 30 failed after batch 60 had failed on another rule.

### `short > 120` (5 runs, one per failure, all in loop runs 8 and 9)

- **The shorts the model wrote were not kept.** The sandbox has no `*.jsonl.invalid` (it is removed after a valid run; the last run was valid) and `distill.log` is empty. Their byte length is unknown: only "more than 120". What is known is the one the model wrote when it passed:

| record | failed at | record text (chars / bytes) | valid short later (bytes) | where it passed |
|---|---|---|---|---|
| hs-d323 | loop run 8, batch 60, line 38 | 240 / 240 | 105 | batch 60, loop run 10 |
| hs-d339 | run 9, batch 60, line 39 | 355 / 376 | 116 (112 chars; has `├─`) | batch 60, run 10 |
| hs-d307 | run 9, batch 30, line 10 | 738 / 742 | 105 | batch 60, run 10 |
| hs-d301 | run 9, batch 15, line 1 | 707 / 707 | 118 | batch 1, run 9 |
| hs-n277 | run 9, batch 5, line 5 | 770 / 770 | 102 | batch 60, run 10 |

- **The same record id never failed at two batch sizes**: 5 failures on 5 different records. Each of the five records later got a valid short (table), four of them at batch 60 in the next run, so the failure is not a property of the record. `hs-d301` was line 1 of both the batch of 15 (failed) and the batch of 5 (the only error there was `hs-n277`, so d301 was valid). The shorts are drawn at random near the bound. `hs-d323` has only 240 characters, so a long text is not what made its short long.
- The model writes close to the bound: of the 526 valid shorts, 404 (77%) are over 100 bytes, 215 (41%) are 110 bytes or more, 29 are 118 to 120 and 1 is exactly 120; the average is 105.9; none under 30. 18 shorts hold non-ASCII (3-byte `├─`, `§`...): 12 of them are over 100 characters; `hs-d339`'s short is 112 characters but 116 bytes, so a model counting characters is wrong in bytes by 4.
- Rough per-line rate (inference): 5 bad lines in the 696 lines the model wrote in valid and invalid runs (526 valid + 60 + 60 + 30 + 15 + 5 from the invalid runs) = 0.7%; at that rate a clean 60-line answer has probability 0.993^60 = 65%, the same order as the observed 8 valid of 10 states at 60 that did not end in `length` (2 invalid: loop runs 8 and 9).

### Per-line acceptance, reconstructed (option 6a)

The checker lists every failure at once (`check()` collects all problems), and each invalid run lists one error and no "missing from the output", so every other line was valid. Per-line acceptance would have curated, with no retry: run 8 at 60: **59 of 60**; run 9 at 60: **59 of 60**; at 30: **29 of 30**; at 15: **14 of 15**; at 5: **4 of 5**. Total over the five invalid runs: 165 records, in runs that curated 0 (loop run 9 curated 1 record, at batch 1, after four failed tries; loop run 8 passed at batch 15 with 15). The two `length` runs cannot be reconstructed (nothing kept); the estimate for the 60-run is about 38 complete lines (section 3, `length`); for the 30-run, a complete answer of 30 lines may have been there before the extra output: unknown.

## 4. Quality

From `record_curations` (526 rows) and `record_retirements` (137 rows), all `pending`.

- **Retirements:** 131 `documented` (96%), 3 `duplicate`, 3 `superseded`. Targets of `documented`: `docs/contract.md` 91, `docs/commands.md` 34, `docs/implementing.md` 3, `docs/contract.md#37b-the-serve-process-runtime-and-port-supervision` 1, `CONTEXT.md` 1, `BACKLOG.md` 1 (all `/home/clayton/projects/cpd:`). 137 of 526 records (26%) proposed for retirement.
- **Doc candidates:** 34 (`doc_candidate = pending`, `doc_paths = []` in all 34), 0 `sent`; 19 of them from the last run alone.
- **Kinds curated** (join on context and id): decision 286, note 226, answer 7, question 7 = 526; never `plan` or `brief`.

### Hand check of 10 retirement proposals (text from `context_records.text`)

| record -> target | reason | verdict | why |
|---|---|---|---|
| hs-d121 -> hs-d118 | duplicate | right | both say `pr add` on a Plan in phase `pr` or `done` moves it back to dispatch, `done_at` cleared, E7 starts the PR, no Plan review; d118 (seq 211) is older than d121 (seq 216) |
| hs-d201 -> hs-d199 | duplicate | right | both: main's draft Jobs design (#31) stands, pi-pin's `job promote` design is dropped; d199 is older |
| hs-d313 -> hs-d310 | duplicate | right | both: never `git stash`, `refs/stash` shared by all worktrees; d310 (older) is the longer one and also adds the guard |
| hs-n139 -> hs-d207 | superseded | right | d207 says "corrects hs-n139" and names the opposite hand-off; newer target |
| hs-d207 -> hs-d216 | superseded | right | d216 says "supersedes hs-d207". But n139 -> d207 and d207 -> d216 chain: once d207 is retired the n139 target is out of force (n139 should point to d216) |
| hs-n306 -> hs-n307 | superseded | **wrong** | n307 only corrects the path of the spec (`~/.cpd/run/quality/held/dc-docs.json`, not `/tmp`); n306 also holds the condition "held until the Manager reports the tree/Inbox, Deploy and Plan-view lanes idle, then re-add dc-docs to PR docsplit, queued", which n307 does not repeat: retiring n306 loses it |
| hs-d353 -> contract.md | documented | right | contract §(tmux options table) lists `popup-style`, `popup-border-style`, `menu-style` and cites hs-d353 |
| hs-d91 -> commands.md | documented | right | commands.md "Auto-archive on the User's quit (hs-d91, hs-d122)" and the read-only archived history are written there |
| hs-d295 -> commands.md | documented | right | commands.md "The dialog kit (User, hs-d295, hs-d297)": every confirmation and input is a tmux popup of the kit |
| hs-d283 -> contract.md | documented | partly right | the contract says the repos run no GitHub Actions, the Gate is the check, only mergeability matters (hs-d283); the record's "dotfiles gains Gate" part I did not find in the hit lines (unverified) |

Two more, outside the ten: `hs-d28 -> contract.md` is a hit in a requirements table row (`R34 ... hs-d28, hs-d29 ...`) that only cites the id; the Plan model is stated elsewhere in the contract (I did not find the sentence). `hs-d362 -> contract.md`: the hit is the sentence "hs-d351 and hs-d362 stand where they agree" with hs-d534's palette; that is a citation, not the statement. **The `documented` proposals are validated only by "the id was found in that file" (`docs.md` is a grep of ids): a citation counts as documentation.** Of the 7 `documented` hits I opened, all 7 cite the id in the target file; 3 clearly state the content (d353, d91, d295), 1 partly (d283), 2 only cite it (d28, d362) and for 1 (d115) I did not verify the statement.

Tally of the 10 in the table: 8 right (hs-d207 -> hs-d216 with a chain flaw), 1 partly right (hs-d283), 1 wrong (hs-n306).

### Hand check of 10 short forms (random sample, `order by random()`)

| record | bytes | verdict | note |
|---|---|---|---|
| hs-n283 | 105 | faithful | "#109 nolock pushed at c5730e5 before #127; hand Gate failed only on main's own tests, nolock tests passed": drops "by the User's choice", the three guards |
| hs-n183 | 110 | faithful | main broken at 1f7e19b, missing `Text` import in `core/src/plan/mod.rs`, fixed in tui-dots followups, cherry-pickable |
| hs-d146 | 100 | faithful, one loss | dotfiles apply fast-forwards the cpd repo and runs `install-cpd.sh` when HEAD moved; drops "(supersedes hs-d25)" |
| hs-q1 | 102 | faithful | a question; readable alone |
| hs-n307 | 95 | faithful, needs context | "correction to hs-n306: held dc-docs spec kept at ~/.cpd/run/quality/held/dc-docs.json, not /tmp": a reader without hs-n306 does not know what the spec is |
| hs-n221 | 103 | faithful, loses | "driver-busy-a1 dispatched beside 10 running; ...": drops "Urgent", the load (65) and the concurrency |
| hs-d222 | 116 | faithful | red `#<n> conflict` in PR box title, legend, 2 s polling |
| hs-d296 | 73 | faithful | right-click menu on a Deploy PR row: Merge, Copy Link, Go to Lead (drops when Merge shows) |
| hs-d392 | 100 | faithful | migrations named by `scripts/new-migration.sh`, numbered frozen |
| hs-d43 | 106 | partly | "Plan has 1+ PRs with branch cpd/<plan>/<key>; new table prs; jobs.pr; migration 3 makes existing Plan 1 PR": the record's first 420 characters give the key regexp and the branch, `prs` and the fields; "migration 3" I could not check in what I read; the short drops the key regexp |

9 of 10 faithful (3 with a named loss), 1 partly. Over-100-byte shorts (404 of 526) were not unreadable in this sample: five of the ten are over 100 bytes and all five are readable. Cross-record references ("correction to hs-n306") make a short unreadable alone.

### Topics

- **18 topics in all**, all 18 created in the first run (`curated_at` 04:14:08, the first run curated them all; its `known.md` had no curated record, so the model had nothing to reuse), **0 new topics in the next 10 valid runs** (`known.md` listed them from then on, the 18 topic lines).
- Counts: cockpit 116, planning 94, testing 53, lifecycle 39, lead 36, cross-repo 35, architecture 34, store 23, hygiene 21, engine 13, context 13, dotfiles 11, switch-over 9, plannotator 9, machines 8, config 6, attempts 5, retention 1.
- **Near-duplicates: none** (no plural/singular pair, no hyphen variant: the only hyphenated ones are `cross-repo` and `switch-over`, each alone). Overlaps by meaning: `lead` / `planning` / `lifecycle`, `engine` / `lifecycle`. Skew: two topics take 40% of the records; `retention` is used once. Topic format errors: 0.

## 5. The merge notes (for the Worker that opens this PR)

The scratch worktree `/tmp/measure-gx` (branch `measure-gx-scratch`) exists. Base `3350e9ac` is this Lead Branch; merged: `origin/cpd/curate/ladder-h` (#243, `18260d2c`) as `ca5b69c0` `wip-ladder`, then `origin/cpd/tiers/tier-g` (#245, `f5b9212d`) as `a0092b36` `wip2`. `git -C /tmp/measure-gx diff 3350e9ac..HEAD --stat`:

```
 cli/tests/engine/distill_notice.rs                 | 125 ---------------------
 cli/tests/engine/doctor.rs                         |   4 +-
 cli/tests/engine/main.rs                           |   1 -
 cli/tests/engine/watch.rs                          |  25 +++--
 cli/tests/engine/watch_distill_inbox.rs            |  80 ++-----------
 cli/tests/flow/config.rs                           |  21 +++-
 cli/tests/flow/driver.rs                           |   6 +-
 cli/tests/flow/job.rs                              | 121 +++++++++++++-------
 cli/tests/pr/merge.rs                              |   2 +-
 cli/tests/pr/pr.rs                                 |   2 +-
 cli/tests/pr/pr_push.rs                            |   2 +-
 cli/tests/session/session_titles.rs                |   7 +-
 cli/tests/tooling/e2e.rs                           |  10 +-
 cli/tests/tooling/e2e_cross.rs                     |   2 +-
 core/src/config/cache.rs                           |   6 +-
 core/src/config/catalog.rs                         |  14 ++-
 core/src/config/file.rs                            |  12 +-
 core/src/config/mod.rs                             |   2 +-
 core/src/config/tier.rs                            |  63 +++++------
 core/src/context/distill.rs                        |  32 +-----
 core/src/driver/start.rs                           |   2 +-
 core/src/inbox/model.rs                            |   5 +-
 core/src/inbox/rules.rs                            |  82 --------------
 core/src/job/mod.rs                                |   2 +-
 core/src/session/retitle.rs                        |   8 +-
 core/src/session/row.rs                            |   1 +
 .../store/migrations/20261022T0259-distill-h.sql   |  28 +++++
 core/src/store/schema.rs                           |   2 +-
 core/tests/tier_g.rs                               |  39 +++++++
 core/tests/worker_tiers.rs                         |  73 ++++++++++++
 docs/implementing.md                               |   2 +-
 pi/skills/cpd-lead/SKILL.md                        |   6 +-
 pi/skills/cpd-lead/references/spec-files.md        |   5 +-
 tui/src/tree/ask.rs                                |  12 --
 tui/src/tree/inbox.rs                              |   3 -
 tui/src/tree/inbox_distill.rs                      |  34 ------
 tui/src/tree/mod.rs                                |   2 -
 tui/src/tree/view.rs                               |  13 ---
 38 files changed, 354 insertions(+), 502 deletions(-)
```

The working tree of the scratch has one more change that is **staged and not committed**: `core/src/config/tier.rs` (+35 -8), the G Tier of #245 (`Letter::G`, `GX` parse and display, `Catalog.g`) put back; the committed `wip2` tree lacks it (it has the ladder-h side of that file).

Hand-resolved or hand-written (from `git show --cc` of both merges and the diffs to each parent; the rest merged without a conflict):

Merge 1 (`wip-ladder`, ladder-h into the curate base):
- `core/src/context/distill.rs`: the base has the new batch-curation `run` with a ladder loop (`let mut ladder = vec![start]; loop { ... self.distill_ladder(start) ... }`); ladder-h has the old digest-based file. The merge keeps the base's curation file and replaces the loop with one step: `let steps = vec![start]; match self.step(start, ...)`, and deletes `fn distill_ladder` (hs-d560: no second step). A stale comment "Why a step of the ladder failed" is left at line 662.
- `cli/tests/flow/config.rs` (hunks only in the merge): `open_tier = "H1"` -> `"H3"` in `config_accepts_o_tiers_below_effort_5_for_leads_and_h_for_auxiliaries` and its expected value, `S4` -> `S3` in the fixture: the Worker range is H3, S0-S3 (hs-d554); the test `context_recent_days_defaults_to_1_and_refuses_0` and `context_recent_days_above_3650_is_refused` from the base are kept next to ladder-h's new test.
- `cli/tests/engine/watch.rs`: `e3_does_not_start_a_job_on_a_tier_outside_the_worker_range...` (O3, S5, S4, H2 refused, message cites hs-d554) and `open_tier = "H3"`: ladder-h's version, merged beside the base's changes.
- `docs/implementing.md`: the area table drops `distill_notice` (that module and `cli/tests/engine/distill_notice.rs` are removed by ladder-h; the layout test needs the table to match).
- `pi/skills/cpd-lead/SKILL.md`: the Tier line becomes "H3 for docs, H3 or S0-S3 only, no O"; "if the Job failed at S3" replaces O5.
- Also: `core/src/store/migrations/20261022T0259-distill-h.sql` keeps `curate-retire` and `curate-docs` in the `inbox_items` kind CHECK (the curation migration adds them); `SCHEMA_HASH` repinned (commit message of `458ebe31`).

Merge 2 (`wip2`, tier-g into the result):
- `core/src/config/tier.rs`: tier-g's file has a `distill_ladder` that keeps `G`; ladder-h deleted the ladder. The merge needs tier-g's G additions and ladder-h's deletion: done only in the **staged, uncommitted** change above (`Catalog::distill_ladder` stays deleted).
- `core/src/context/distill.rs`: +1 line, `Letter::G => &models.g,` in the model-of-Tier `match`, which must now be exhaustive.
- `core/src/config/file.rs`: tier-g's `context_distill_tier` default `GX` and check ("must be GX") and `tiers.G` default `github-copilot/gemini-3.8-flash` merged with the base's `context_recent_days` (days of curated records, `int(v, 1, 3650)`) without a conflict; `leader_tier` refuses G.
- `core/tests/worker_tiers.rs`: +1 line, the `g: TierModel { model: "p/gemini", efforts: vec![] }` field of the `Catalog` literal.
- `core/tests/tier_g.rs`, `cli/tests/engine/doctor.rs` (fake catalog with G), `cli/tests/flow/job.rs`, `cli/tests/tooling/e2e*.rs`, `cli/tests/pr/*.rs`, `session_titles.rs`, `core/src/config/{cache,catalog,mod}.rs`, `core/src/session/{retitle,row}.rs`, `pi/skills/cpd-lead/references/spec-files.md`: tier-g's own changes.

Not verified here: that `cargo test` of the merged tree passes. In `cli/tests/flow/config.rs` the fixtures still say `context_distill_tier = "H0"` / `"H4"` (seen in the diff), which tier-g's check ("must be GX") refuses; that test would fail in the merged tree unless the PR updates it.

## 6. What the data says about the design

No implementation; each is an option with its evidence.

**(a) Per-line acceptance.** cpd writes the lines that pass its checks, leaves the others uncurated for the next run; a run fails only when no line is valid. A cut at `length` keeps the complete lines before it.
- Evidence: all 5 `distill_invalid` runs had exactly one bad line. Per-line acceptance would have curated 59/60, 59/60, 29/30, 14/15 and 4/5 records (165 in all) in runs that curated 0. Those 5 runs (table rows 9, 12 to 15) cost $0.1375 (0.03918 + 0.04043 + 0.02343 + 0.01931 + 0.01518); the 2 `length` runs (rows 3 and 10) cost $0.0488.
- `length`: the output of those runs is not kept now; to use per-line acceptance there, cpd must keep the partial output. Estimate for loop run 3 at 60: about 38 complete lines (unverified). The batch-30 `length` run wrote more than 30 lines' worth of tokens, so it needs the partial output read before anyone can say.
- Cost: the checker already collects every problem; it would write the valid lines, and the records of the others stay uncurated. Records retried are re-batched next run (the first uncurated, oldest first, already the rule). Risk: a model that writes one bad `short` per 140 lines would keep a 0.7% line rejection forever; the same record can fail again (it passed on a later try in 5 of 5 cases here).

**(b) A lower byte bound in the prompt and a stricter example.** The prompt says "1 to 120 bytes" and its four examples are 36 to 55 bytes. The model does not follow it: 41% of valid shorts are 110+ bytes, 77% are 100+, the mean is 105.9; the 5 failures are the tail of that. Options: ask for "at most 100 bytes" (so the tail stays below 120; 404 of 526 would still be over, so the model would need to shorten by about 20%), give an example near 100 bytes, say "bytes, not characters" (hs-d339's short was 112 characters and 116 bytes). Evidence that it matters: every failure is this one rule; no topic or retire error in 18 runs.

**(c) A smaller default batch.** Evidence against shrinking alone: `short` failures happened at 60, 30, 15 and 5 (each a different record) and passed at 1 only; `length` failed at 60 and 30, passed at 15 and 1. At batch 15 the loop passed 1 of 2 times, at 5: 0 of 1, at 1: 1 of 1. A smaller batch costs more input per record: `known.md` is re-sent each run (17,417 tokens in for 1 record at 406 known; 12,697 for 60 at 0 known), so cost per record at batch 1 is $0.0134, against $0.00042 at 60 in run 1 (0.02547 / 60) and $0.00060 in run 11 (0.03597 / 60): 22 to 32 times more. The seconds do not drop with the batch much either (about 33 s for 81 tokens out at batch 1; 47 s for 376 out at batch 5): a fixed 30 s of start up and input per run.

**(d) Other things the data shows.**
- `known.md` grows with every record (128 bytes each; 59,667 bytes, about 19,300 tokens at 466; about 31,000 at 751) and is most of the input, and the cost grows with it ($0.0258 + $0.000029 per curated record). A smaller `known.md` (the records of the last `context_recent_days`, or one line per topic) would flatten it; the model needs it for duplicate and superseded proposals only: 6 of 137 proposals (4%) are those; 131 are `documented`, which need `docs.md` and not `known.md`.
- `documented` retirements are validated only by an id grep hit (section 4): citations in traceability tables count. 2 of the 7 I opened only cite the record, 1 is unverified. The User approves each, so the cost is review time, not wrong deletion; the one wrong proposal I found was a `superseded` (hs-n306 -> hs-n307, a partial correction).
- Chains: n139 -> d207 -> d216 are both proposals; once d207 is retired, n139's target is out of force. The `superseded` rule says "recency wins" but the checker only requires the target to be in force and newer at the time of the check.
- Topics converged immediately (18 in run 1, none after): the topic rule works; two topics hold 40% of the records.
- Time: about 270 s per 60-record run (about 15 tokens/s out); the 74 minutes of this measurement were mostly generation, and the `seconds` of the failed runs (about 250 to 320 s) are as long as the valid ones, so a failed 60-run wastes a whole run.
- The `length` figure of 2556 to 2559 tokens for two different batches is the one thing the log cannot explain; a run that keeps `pi`'s raw output on `length` would say whether it is a cap, a loop or hidden reasoning.
