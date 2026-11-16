GX pass rate after per-line acceptance (hs-d563), batch 60: 8 of 8 runs with at least one accepted line (100%, first measurement 72.7%); 8 of 8 clean runs (zero rejected lines; 1 of the 8 was cut at `length`, with 43 of 60 lines accepted); 463 of 480 records curated (96.5%; the other 17 were left uncurated by the cut); 0 runs that curated nothing, 0 retries at batch 30.

# GX curation pass rate after per-line acceptance (hs-d558, hs-d563)

Measured by Job `measure-gx2`. Tier `GX` = `github-copilot/gemini-3.8-flash:off`, one step, no fallback. Sources: the sandbox `$CPD_HOME` (a temp dir, removed at the end): `loop.log` (the log: one `cpd context distill --context building-cpd --batch 60` result per run), `run/context-digests/building-cpd/runs.jsonl` (one line per pi run), `cpd.db` (a copy of the live store, read with `sqlite3 -readonly`). The first measurement is `curate-gx-pass-rate.md`.

Method, as the first measurement, with these differences: the scratch build is this Job's branch (per-line acceptance, 100-byte prompt) with `origin/cpd/curate/ladder-h` (#243) and `origin/cpd/tiers/tier-g` (#245) merged and hand-resolved (`origin/main` has no `Letter::G`; the ladder removed, one step; `core/src/config/tier.rs` taken from the first measurement's resolution, whose base file is unchanged); the loop ran in the background from the start (`nohup`) and finished in 26 minutes. The loop script retries a failed run once at batch 30; no run failed, so it never did. 8 loop runs = 8 pi runs (the cap of 10 was not reached). Records in force and uncurated at the start: 769 (709 remaining after run 1 + 60); the spec says 768 (the live store gained one record).

How the sources line up: `loop.log` has 8 results and `runs.jsonl` 8 lines, in the same order; the sum of `curated` in the log (60 x 7 + 43 = 463) equals the rows of `record_curations` (463); the sum of `retire` (105) equals the rows of `record_retirements` (105) and the sum of `docs` (11) equals the `doc_candidate = 'pending'` rows (11).

## 1. Runs

All 8 runs used `--batch 60`, so 480 records were sent (`batch.md` holds 60 records each time). `rejected` is `runs.jsonl` / the log's `rejected` (always `[]`). Retire and docs are the log's counts for the run.

| run | batch | result | accepted | rejected | cut? | seconds | tokens in | tokens out | cost $ | retire / docs |
|--:|--:|--|--:|--|--|--:|--:|--:|--:|--|
| 1 | 60 | valid | 60 | none | no | 196.4 | 12758 | 4050 | 0.02476 | 22 / 0 |
| 2 | 60 | valid | 60 | none | no | 182.0 | 16031 | 3830 | 0.02639 | 21 / 0 |
| 3 | 60 | valid | 60 | none | no | 239.9 | 19999 | 3516 | 0.02818 | 0 / 0 |
| 4 | 60 | valid | 60 | none | no | 255.7 | 21594 | 3537 | 0.02946 | 3 / 0 |
| 5 | 60 | valid | **43** | none | **yes** | 281.7 | 23595 | 2555 | 0.02728 | 0 / 2 |
| 6 | 60 | valid | 60 | none | no | 176.3 | 24499 | 3638 | 0.03202 | 12 / 4 |
| 7 | 60 | valid | 60 | none | no | 173.0 | 27487 | 3897 | 0.03523 | 20 / 1 |
| 8 | 60 | valid | 60 | none | no | 231.8 | 30156 | 4173 | 0.03827 | 27 / 4 |
| sum | | 8 valid | 463 | 0 | 1 | 1736.8 (29 min) | 176119 | 29196 | 0.24157 | 105 / 11 |

Remaining after each run: 709, 649, 589, 529, 486, 426, 366, 306 (the log). The 17 records the cut run left uncurated (60 - 43) were the first of the next batch (run 6), as the rule says; none of them is lost.

The cut run (5): `"cut":true`, 2555 tokens out, 43 complete lines accepted (59.4 tokens per accepted line). The two `length` failures of the first measurement stopped at 2559 and 2556 tokens out; this cut is at 2555: three cuts in 26 pi runs (2 of 18, 1 of 8), all at 2555 to 2559. Valid runs wrote 3516 to 4173 tokens out here (and up to 4334 before), so the cut is not a plain cap on the tokens out the log shows (unverified: hidden reasoning, a cap that is not on every run; the log cannot say). What changed is the effect: before, such a run curated 0 and cost a retry at 30; now it curated 43.

## 2. Rejected lines, and the `short` lengths

Rejected lines by cause: short 0, topic 0, target 0, missing 0, unknown 0, repeated 0; no line rejected in 480 sent. Before: 5 `short` > 120 in 696 lines written (0.7%). A zero count in 463 lines does not say the rate is 0: with 463 lines at 0.7%, 0 rejections would be expected about 4% of the time (0.993^463), so the 100-byte prompt very probably lowered the rate but one run per rate cannot say by how much (my arithmetic).

`short` length of the 463 accepted lines (bytes, `length(cast(short as blob))` in `record_curations`):
- min 52, max 100, mean 90.8; **0 over 100 bytes** (before: 404 of 526, 77%); 0 of 110 or more (before 41%); 0 of 118 or more; exactly 100: some (the max).
- 317 (68%) are 90 to 100 bytes; 1 is 60 bytes or less.
- 3 shorts hold non-ASCII (bytes and characters differ); none is over 100 bytes.
- The model now stops at the asked bound: the whole distribution is below it, the mean 15 bytes lower than before (105.9). It tucks up against 100, so the 120 check keeps a margin of 20 bytes.
- 0 shorts begin with or hold `correction to` / `corrects`.

Topics: 20 in all (before 18); counts cockpit 82, plan 68, planning 47, testing 46, lead 41, architecture 33, install 27, code-sharing 24, process-lifecycle 16, engine 15, context 13, dotfiles 12, pr-polling 10, plannotator 7, config 7, machines 5, switch-over 4, archive 3, cut-over 2, retention 1. Near-duplicates: `plan` / `planning` (68 and 47 records on two topics that mean the same), `switch-over` / `cut-over`; `retention` used once. No format error. (Different topics from the first measurement: the run is a different sample of the model's choices.)

Retirement proposals: 105 (22% of 463): 100 `documented`, 3 `duplicate`, 2 `superseded`. `documented` targets: `docs/contract.md` 71, `docs/commands.md` 26, `docs/implementing.md` 2, `docs/contract.md#37b-the-serve-process-runtime-and-port-supervision` 1 (all in `/home/clayton/projects/cpd`). Doc candidates: 11, all `pending`.

## 3. Hand check

### 5 retirement proposals (record text from `context_records`; docs hits by `git grep -w <id> -- docs '*.md'` in `/home/clayton/projects/cpd`)

| proposal | verdict | why |
|---|---|---|
| hs-d199 -> hs-d201 (superseded) | right | both say main's draft-Jobs design (#31) stands and pi-pin's `job promote` is dropped; d201 (seq 373) is the User's later resolution and newer than d199 (seq 370); d199's extra line (pi-pin's PR drafts carry only the ETXTBSY fix, f2e2150) is not in d201 and is lost |
| hs-d202 -> hs-d190 (duplicate) | right, one nuance | d202 "confirming hs-d190": inbox's gh-poll must not add or store `mergeable`/`merge_state`; d190 says it "does not add mergeable"; d202's "or store them" is not in d190 (a small loss) |
| hs-n139 -> hs-d207 (superseded) | right | d207 says "corrects hs-n139" and names the opposite hand-off; d207 is newer |
| hs-d284 -> docs/implementing.md (documented) | right | implementing.md line 127 "Speed (hs-d284)": the 30 s clock and 300 s added-up targets are written there |
| hs-d7 -> docs/commands.md (documented) | **wrong** | the record's text is about the Lead deciding what the User's decisions do not cover (R21); the commands.md hits for `hs-d7` are an example JSON `"id": "hs-d7"` and a `store_too_new` paragraph citing `hs-d7`, which are not this content: the id grep matched, the statement is not there |

Two more `documented`: hs-d251 -> commands.md: right (the interface tree and its callers and cockpit flag are described at commands.md line 182); hs-d45 -> contract.md: the only hit is a citation in the R34 traceability table row; the statement (pr add, pr edit, pr cancel...) is not found there in the hit lines. So 4 of 5 right (1 with a nuance), 1 wrong; of the 3 `documented` I opened, 1 is wrong and 1 only cites. As before, `documented` is validated only by "the id is found in that file", and an id that is also a citation or an example (here, a collision with an old id) counts. The User approves each retirement.

### 10 short forms (random sample, `order by random()`, texts read to 260 characters)

| record | bytes | verdict | note |
|---|--:|---|---|
| hs-n92 | 99 | faithful | tui-visuals' followups PR edits four files; tui-dots rebases; drops the other files and the reason |
| hs-d192 | 92 | faithful | cross-repo PR distill pilots Epic B: an auxiliary agent distills in-force records; readable alone |
| hs-d104 | 89 | faithful | tui-visuals: cockpit visual tweaks in batches approved via Lead preview builds |
| hs-d169 | 81 | faithful | herdr dropped, dialogs count as blocked via `ui_prompt_start/end` |
| hs-q2 | 95 | faithful | the three questions of R34 in one line, readable alone |
| hs-d147 | 98 | faithful | turn-start message: up to 20 new records in full, count, `cpd context show --since` |
| hs-d172 | 78 | faithful | overlap warning only in plan render and Plan review, not on `job add` |
| hs-n151 | 93 | partly | "handover ...: state, active PRs, unmerged jobs, rules": a list of headings, it does not say which state or rule (the record's first lines name the User's rules, hs-d84, hs-d209) |
| hs-d210 | 93 | faithful | wrapper re-execs at once on a new cpd, adopting pi by pid; store error never exits while pi runs |
| hs-d106 | 94 | faithful | cross-repo Lead may run `merge -s ours origin/main` after adapter-kinds merges, then `cpd pr open`; drops the guards |

9 of 10 faithful, 1 partly; all 10 readable alone (before: 9 faithful, 1 partly, 3 with a named loss). The 100-byte bound did not make the shorts less readable in this sample; the `correction to <id>` alone form did not appear in the 463 (0 rows).

## 4. Cost

- Whole measurement: **$0.2416** for 8 runs, 463 records: **$0.000522 per record** curated (all 8 runs valid, so nothing to add for failures; the cut run's cost includes its 43 curated lines). Before: $0.4977 for 526 records = $0.000946 per record all-in ($0.000592 valid only).
- Cost per run grows with the records already curated, because `known.md` lists them: $0.02476 at 0 curated to $0.03827 at 403 (tokens in 12758 to 30156). For the cheapest and dearest: $0.000413 and $0.000638 per record.
- **The whole backlog**, 769 records at the measured per-record cost: **$0.40** (0.000522 x 769, flat; the per-record cost still rises, so the last runs cost more: at 709 curated a 60-run would be about $0.048 by the same growth, from the first and last points: my extrapolation, unverified). Before: $0.71 all-in for 751 records.
- Time: 1736.8 s for 463 records (3.75 s per record, 217 s per run, 29 min in all): the backlog about 48 min (769 x 3.75 s). Before: 272 s per valid 60-run, 74 min for 526 records.

## 5. Verdict against hs-d558

**Curation passes at GX with no fallback.** Records curated per run: 60, 60, 60, 60, 43, 60, 60, 60 (mean 57.9; min 43). Runs that curated nothing: 0 of 8 (before 3 of 11 at batch 60, and 7 of 18 pi runs failed). Retries needed: 0 (before 7 extra pi runs, $0.1863 wasted). 463 of 480 sent (96.5%); the 17 that were not are the cut run's tail and were re-batched.

What is left, none of it a failure of the run:
- The `length` cut: 1 in 8 runs, 17 records (3.5% of the sent). The next run re-batches them at no extra cost beyond the lines of one pi run. If it should matter, the concrete design change: lower the default batch size from 60 to 40 (60 lines at 59 tokens per line needs about 3560 tokens out; the three cuts happened at 2555 to 2559, which 40 lines at 60 tokens, 2400, stays under; this is a guess, unverified, as the cut is not a plain cap), or leave it: a cut run still curated 43 and the loss is under 4%.
- `documented` retirements validated by an id grep hit: 1 wrong of 3 opened here (hs-d7), as before 2 of 7 only cited. Design change if wanted: show the hit line (not only the path) in the Inbox item so the User sees whether the file states the record. Not needed for the pass rate.
- Topics `plan`/`planning`, `switch-over`/`cut-over` overlap; the model reuses the `known.md` topic lines but still splits meaning.

Nothing implemented; no code, prompt or batch size changed. Teardown: the scratch worktree `/tmp/measure-gx2` and branch `measure-gx2-scratch` and the sandbox `$CPD_HOME` were removed; `~/.cpd` was only read (the `.backup` of `cpd.db` and one count at the end).
