# Distiller degradation and the write-time curation pipeline

Job `curation-docs`, Plan `rfm-curation-models` (Plan 2 of 4), attempt 1. Research only: no code, no migration. It answers items (1) to (5) of the Job and ends with `Findings to record`.

**Short answer.** The batch distiller's input grows by about 36 tokens per curated record in force (R² 0.98), so one run costs about $0.03 at 1,000 records, $0.056 at 2,000 and $0.14 at 5,000 for a median 2-record batch, against a flat write-time call of about $0.003 (arithmetic, unverified: no write-time call was run). Its recall is thin: 13 retirements in 363 curations, 8 of them `documented`, which a write-time pipeline cannot produce. Neighbor curation is worth building, but it is not cheaper today (break-even is about 350 records), it needs a `contradicts`/partial-supersede outcome the batch lacks, and `documented` stays a periodic, deterministic sweep.

## Method and sources

- **Data** (copied out of `~/.cpd`, read-only, Context `cpd` only, never sent to a model): `records.json` (579 rows, seq 202 to 1170), `retirements.json` (13 rows), `runs.jsonl` (101 lines, not 100: 91 `valid`, 10 `failed`; 2026-10-23 13:35 EDT to 2026-10-29 21:17 EDT, converted from the file's UTC stamps), `distill-errors.json` (30 error Events). Scratch scripts: `/tmp/rfm-curation-models/curation-docs/{fit,nb}.py` (not committed). No model was called, nothing was installed (decision pro-d209).
- **Code read:** `core/src/context/{distill.rs, distill_prompt.md, curation.rs, curated.rs, inforce.rs, write.rs}`, `core/src/store/migrations/20261022T0203-curation.sql`, and `docs/research/{context-distill, context-retrieval-design, context-holographic}.md`.
- **Plan 1** (`rfm-memory-retrieval`) owns sqlite-vec, FTS5 and the read log. Its findings file is not in this checkout, so this file cites `context-retrieval-design.md` for FTS5 and labels what it expects from Plan 1 `unverified`.
- **Prices** are fitted, not read from a price list: over the 91 valid runs, `cost = 0.75e-6 * tokens_in + 3.75e-6 * tokens_out` fits every run with residual under 1e-17 (least squares, per model group: 39 `github-copilot/gemini-3.8-flash:off`, 52 `bifrost/vertex/gemini-3.8-flash:off`).
- The Job spec says 221 uncurated records and 100 runs; the copied files hold 216 and 101. This file uses what the files hold.

## 1. What the batch reads and why it degrades

### 1.1 Inputs per run (`distill.rs`)

| File | Content | Grows with N? |
|---|---|---|
| `prompt.md` | `distill_prompt.md`, 28 lines | no |
| `known.md` | every curated record in force as `[id] topic \| short`, topics listed first | **yes, linear** |
| `batch.md` | the oldest uncurated in-force records, at most 60 and 48 KiB (`BATCH_MAX`, `BATCH_BYTES`), full text | no (capped) |
| `docs.md` | per batch record, `git grep -l -w <id> -- docs '*.md'` hits in each repo of the Context's Plans' PRs | no |

`State::read` also loads every record and curation of the Context and `in_force_without` scans them all, once per run (local, not tokens). Only `known.md` grows with the store. The model is told to name a retirement target "in the batch or in `known.md`", so recall is bounded by what `known.md` shows.

### 1.2 Growth rate of `tokens_in`

Method: for each of the 91 valid runs, N = number of Context `cpd` records with a topic and `seq` <= the previous valid run's `upto_seq` (an upper bound on `known.md` lines: it counts curated records later retired or superseded; `records.json` has no `supersedes` column); B = text bytes of the records curated in `(previous upto, upto]` (an estimate of the batch). Ordinary least squares with numpy.

| Model | Fit | R² | n |
|---|---|---|---|
| `tokens_in ~ upto_seq` | 493 + 13.0 per seq | 0.952 | 91 |
| `tokens_in ~ N` | 2,851 + 36.3 per record | 0.932 | 91 |
| `tokens_in ~ N + B` | 2,229 + 35.6 per record + 0.354 per batch byte | 0.986 | 91 |
| `tokens_in ~ N`, runs with B < 4,000 bytes only | **2,580 + 35.9 per record** | **0.977** | 75 |

- **Growth: about 36 tokens per curated record in force.** Cross-check: a `known.md` line is about 125 bytes (id, topic, mean short 91.9 bytes, separators; by arithmetic), so 125 / 35.9 is about 3.5 bytes per token. The fit's constant is `prompt.md` plus the topic list plus the batch.
- First valid run: 4,542 tokens (`upto_seq` 232). Last: 16,398 (`upto_seq` 1169, N = 357), $0.0135.
- Typical run: median `tokens_out` 134 (about 2 lines; 0 rejected lines in all 101 runs), 363 curations over 91 valid runs (mean 4.0, median 2, max 22 per run by the interval estimate).

### 1.3 Projection per run

Model: `tokens_in = 2,580 + 35.9 N` (small batch), plus `0.75e-6` $/token in and a median 134 tokens out (about $0.0005). Arithmetic from the fit, not a measurement beyond N = 357; labelled unverified beyond that.

| In-force records N | tokens_in per run | Cost per run (2-record batch) | Cost per curated record | Cost per day at 14.3 runs/day |
|---|---:|---:|---:|---:|
| 357 (measured, last run) | 16,398 | $0.0135 | $0.0068 | $0.19 |
| 1,000 | 38,480 | $0.029 | $0.015 | $0.42 |
| 2,000 | 74,380 | $0.056 | $0.028 | $0.80 |
| 5,000 | 182,080 | $0.137 | $0.069 | $1.96 |

- Runs/day: 91 valid runs over the 6.4 days from 2026-10-23 to 2026-10-30 (arithmetic). A full 60-record backlog batch adds about 11,500 tokens at N = 1,000 (0.354 tokens per byte, 60 x 541 bytes mean) plus about 3,300 output tokens ($0.012).
- Total spent so far: $0.894 over 91 valid runs, 884,972 tokens in, 61,321 out (sum of `runs.jsonl`); the failed runs cost $0.
- At N = 5,000 one run is about 182k tokens: past a 200k window with a larger batch. The 200k figure is from the proposal's "Haiku already ran out of context"; the window size is unverified here.
- Latency: median 5.7 s on `bifrost/vertex` (n = 52), 51 s on `github-copilot` (n = 39), same model and effort: the provider, not N, dominated it so far.

### 1.4 Failures by cause

| Cause | Runs | Detail |
|---|---:|---|
| `Request timed out` at pi, model `bifrost/vertex/gemini-3.8-flash:off` | 10 of 101 (9.9%) | all at `upto_seq` 849 and 850, 56.3 to 75.9 s each, tokens 0, cost $0. Two bursts: 17:39 to 17:44 EDT (5) and 18:03 to 18:07 EDT (5) on 2026-10-27, one per minute (engine retry) |
| rejected lines (`distill_invalid`) | 0 of 101 | `rejected` is empty in every line |
| other causes in `runs.jsonl` | 0 | |

- `distill-errors.json` holds 30 error Events: the same 10 `cpd` timeouts and 20 `eec-general` "pi exited with status 1" (2026-10-26, four bursts of five within about a second each; another Context, not in `runs.jsonl`, cause not recorded in the Event text; not analysed).
- Reading: failure is transport (a gateway timeout), not model quality, and it is bursty. A retry loop spent 10 invocations on one outage. A write-time design needs a backoff, not a once-a-minute retry (section 3.7).

### 1.5 Backlog (the 216 uncurated records)

| Part | Count | Reading |
|---|---:|---|
| `plan` and `brief` records | 200 | never in force (`inforce.rs`), never curated: 34.5% of the 579 rows can never leave the "uncurated" count |
| `decision` and `note` without a curation | 16 (14 decisions, 2 notes) | 7 are named by a later record's text as superseded, replaced or overruled (pro-d4 by pro-d8, pro-d54 by pro-d55, pro-d98 and pro-d99 by pro-d100, pro-n133 by pro-n135, pro-d176 by pro-d177): probably out of force, unverified because `supersedes` is not in the copy. 9 are unexplained, among them pro-d209 (written 2026-10-29 21:26 EDT, after the last run at 21:17 EDT) and pro-d205 |

So the real backlog is small: at most 9 records, 2 explained by timing. The batch is not behind; it is expensive per record. Whether pro-d4, pro-d45 or the others were silently dropped by the model cannot be told from the copy (a record missing from the output stays uncurated without a rejection, `Batch::check`): open item for the Lead.

### 1.6 Retirements: what the batch ever concludes

Method: `retirements.json` (Context `cpd`) joined to `records.json`.

| Fact | Value |
|---|---|
| Rows | 13 of 363 curations (3.6%); whole store 29 of 806 (3.6%, Lead's count of 2026-10-29): the rate is the same |
| By reason | `documented` 8, `superseded` 4, `duplicate` 1 |
| By status | approved 11, pending 2, rejected 0 (whole store 23 / 6 / 1) |
| Time to User decision | 0.1 h to 45.5 h, median 19.8 h (7 of the 11 approved in one pass at 2026-10-28 13:08 EDT) |
| Distance in `seq` between a `duplicate`/`superseded` record and its target | 2, 2, 23, 2, 1 (pro-d129 to pro-d130, pro-d173 to pro-d174, pro-n115 to pro-n122, pro-n125 to pro-n124, pro-n75 to pro-d64) |

What the shape says about recall:

- **It only finds near neighbours.** All 5 non-`documented` pairs sit within 23 records of each other, i.e. inside the same one or two batches. A target 100 records back is in `known.md`, but the model has to find it among N lines; none has been found that way.
- **`documented` is 62% of retirements.** It needs the record's id to appear in a doc, which happens after the record is written (a PR merges a doc that cites it). A write-time hook sees a record before any doc names it, so it can never emit `documented`.
- **No `contradicts`.** The schema has three reasons (`duplicate`, `superseded`, `documented`) and the prompt has no way to say "these two disagree"; a contradiction is either ignored or forced into `superseded`.
- **No partial supersession.** See 2.2: "refines" is more common than "supersedes".
- **It never reads outside `known.md`:** `short` is at most 120 bytes (`SHORT_MAX`), so a duplicate that differs in the detail the short form dropped is invisible.

## 2. Why whole-store pairwise detection is weak here

### 2.1 What a pairwise check has to see

For the 5 batch-found pairs, the model must see the new record (about 541 bytes mean, 2,597 max) and the old one, and compare detail the `short` form dropped. Example, pro-n125 against pro-n124: both shorts say the Deploy PRs section is deleted from the inbox; the distinction (a duplicate, keep the first) lives in the full texts. `known.md` holds only shorts, so the batch decides on compressed forms; a pairwise call can carry both full texts (about 140 tokens each by 541 bytes / 3.5).

### 2.2 Five real candidates, from the `short` forms of topics `policy` and `tui`

Method: read the shorts of the 50 `policy` and 47 `tui` records in order, then the full text of the two ends. Texts quoted only as short fragments.

| # | Newer | Older | Kind | Evidence |
|---|---|---|---|---|
| 1 | pro-d83 (seq 677) | pro-n62 (508) | **supersedes**, partial | d83: "supersedes the H3->S3 step of the ladder rule (pro-n62)"; n62 is still in force (no retirement row) and still states `H0->H1->H2->H3->S3` |
| 2 | pro-d82 (673) | pro-d53 (498) | **contradicts** | d53 allows Workers "H0-H4"; d82 "refines pro-d53, which allowed H4": H0-H3 only. Both in force |
| 3 | pro-d157 (980) | pro-d156 (979) | **supersedes**, whole | d156 "research first" whether the Inbox can be its own pane; d157 "accept the gap", adjacent seqs, d156 not retired |
| 4 | pro-d153 (952) | pro-d151 (950) | **duplicate or refines** | d153 "completes pro-d151"; both define `pr.human_gated_contexts`, d153 restates the key with more rules |
| 5 | pro-d154 (959) | pro-d147 (909) | **contradicts**, partial | d154: "item 4 of pro-d147 (the right pane's title is Dispatch) is overruled by hs-d729"; items 1 to 3 stand. hs-d729 is another Context's record |

Also seen: pro-d132 "supersedes the no-lock half of pro-d116" (partial, and pro-d116 sits in topic `testing`, not `policy`).

Text-level measurement over the 379 decision and note records: 56 mentions of a link verb followed by a record id (`refines` 28, `supersedes` 16, `replaces` 7, `overrules` 2, `completes` 2, `overruled by` 1) in 53 records; 214 records name some other record id; 45 name an `hs-` id (another Context). Pattern `(supersedes|overrules|replaces|refines|completes|overruled by|void)` within 40 characters of `pro-`/`hs-` plus `[dnqa]<n>`, case-insensitive (an inline script, not kept). Reading:

- **Partial supersession is the norm**: `refines` plus `completes` (30) outnumber `supersedes` (16). Whole-record `supersedes` loses in-force status for the old record entirely (`inforce.rs`), which is wrong for #1, #2, #5.
- **Authors already declare links in prose** but the `supersedes` field (decision, note) carries only whole-record ones. 26 records say supersedes/overrules/replaces; 21 point at a `cpd` record. Whether the field was also set is unverified (not in the copy).
- **Cross-Context references** (`hs-d729`) cannot be resolved inside the Context's neighbours. Never send them to a model; show the id as text only.

### 2.3 Would neighbour lookup find them?

Method (`nb.py`): SQLite FTS5, `porter unicode61`, over the text of the 379 decision/note records of Context `cpd` (python `sqlite3`, the machine's SQLite build). For each of 11 pairs (5 retired, 6 candidates above), query with the 20 rarest non-stop terms of the newer record's text OR-joined, `bm25` order, only records with a lower `seq`, in the pool as it stood (138 to 319 records before the newer one; retired ones not removed). Rank of the older record:

| Pair (new to old) | Rank, 10 terms | Rank, 20 terms |
|---|---:|---:|
| pro-d130 to pro-d129 | 16 | 8 |
| pro-d174 to pro-d173 | none in 13 hits | 2 |
| pro-n122 to pro-n115 | 2 | 1 |
| pro-d64 to pro-n75 | 5 | 2 |
| pro-n125 to pro-n124 | 1 | 1 |
| pro-d83 to pro-n62 | 1 | 1 |
| pro-d82 to pro-d53 | 7 | 6 |
| pro-d157 to pro-d156 | 1 | 2 |
| pro-d153 to pro-d151 | 2 | 2 |
| pro-d154 to pro-d147 | 3 | 2 |
| pro-d132 to pro-d116 | none in 7 hits | none in 50 |

**8 of 11 within the top 5 and 10 of 11 within the top 10 with the 20-term query; the miss (pro-d132 to pro-d116) names "pro-d116" in its text.** That motivates the explicit-reference rule in 3.3: every record id named in the new text is always a neighbour. Sample size 11, lexical only: embedding recall is `unverified` here (Plan 1 and the `curation-embed` Job measure it); do not read k = 8 as proven.

## 3. The write-time pipeline

### 3.1 Per-step table

`record` = the record just written; `N_b` = a neighbour. Every step is deterministic except step 5.

| Step | Name | Input | Rule | Output | On failure |
|---|---|---|---|---|---|
| 0 | Trigger | a committed row of `context_records` (any writer) | enqueue by derivation: "in force and no `record_curations` row" (`State::uncurated`, already exists); no queue table | the oldest such record | none: derived state |
| 1 | Gate | `kind`, text | rules first: kinds `plan`/`brief` never (not in force); `question` and `answer`: topic and short only, k = 0; `decision` and `note`: continue. Model gate: **none** (YAGNI): `independent` is the cheap exit | go / skip / short-only | skip is final and loud in the Event count |
| 2 | Subject match | `supersedes` field, explicit ids in text | **no `subject` key** (3.2). Deterministic part: ids named in the text and the record's own `supersedes` target become forced neighbours | forced neighbour ids | unknown id: ignore, never invent |
| 3 | Neighbour lookup | text, Context, in-force set | FTS5 (20 rarest terms OR, `bm25`, top 25) and embedding kNN (top 25) over the same Context, fused by reciprocal rank, filtered to in force, plus forced neighbours, cut to k (3.3) | up to k = 8 neighbours with id, `seq`, topic, full text capped at 800 bytes | embedder down: FTS5 only, flagged; both empty: curation with `k = 0` |
| 4 | Verdict call | record, neighbours | one small-model call, JSON per neighbour (3.4) | topic, short, verdict per neighbour | invalid or no output: reject the whole record's output, record stays uncurated (3.7) |
| 5 | Validate | JSON | the checks of `Batch::check` (topic and short bounds) plus: ids equal the neighbour ids exactly; verdict is in the enum; `quote` is a verbatim substring (3.4) | accepted lines | a failed neighbour line becomes `independent` with `rejected` counted; a failed curation fails the record |
| 6 | Apply | accepted verdicts | one transaction (3.5): curation row, relation rows, pending retirements, Event | rows | rollback: nothing is written |
| 7 | Flag | `contradicts` or partial overlap | Inbox item for the User; no retirement | review item | none |

### 3.2 The `subject` key

The proposal's deterministic supersession needs `entries.subject`. cpd has no such column: `context_records` has `kind`, `supersedes` (a decision or note naming one record), `question`, `by_plan`, `at` and the text (`0010-manager.sql`); `record_curations` has `topic` (1 to 40 characters) and `short`. What a `subject` key would need:

1. A nullable column `record_curations.subject` (a stable dotted key such as `policy.worker-tiers.ladder`), written by the same model call as `topic` and `short`, because the author does not declare it.
2. A per-Context vocabulary reused like topics (`known.md` lists topics now), else keys drift; and an index on `(context, subject)`.
3. Deterministic supersession only when the model is not the one choosing both keys. Here it is, so step 2 would be a model decision dressed as a rule.

Recommendation: **do not add `subject` in v1.** The record's own `supersedes` field is the deterministic rule cpd already has (read by `inforce.rs`), and the neighbour verdict replaces the rest. A `subject` column may come later if verdicts show repeated misses. Topic is too coarse as a key: `planning` holds 124 of 363 curated records (34%), `policy` 50, `tui` 47 (counts of `records.json`); 20 topics for 363 records. Scope: "overlapping scope" is the Context (a hard filter, the store is per Context), then topic or `by_plan` as a soft boost, not a filter. Whether Plan 1 adds a `scope` column is `unverified` (its findings are not in this checkout); this pipeline needs none.

### 3.3 Neighbour lookup

| Part | Spec |
|---|---|
| Scope filter | `context = ?` (hard); boost, never filter: same topic, same `by_plan` |
| FTS5 query | the 20 rarest non-stop terms of the new text (document frequency over the Context), quoted, OR-joined; `ORDER BY bm25(...) LIMIT 25`; table and triggers as in `context-retrieval-design.md` ("Index and migration") |
| Embedding kNN | `sqlite-vec` over the same rows, top 25 by cosine; Plan 1 owns the table and the model (`unverified` here) |
| Fusion | reciprocal rank over the two lists, then the forced neighbours, then cut to **k = 8** (range 5 to 10) |
| "In force" | the rule of `inforce.rs`: kind is `decision`, `note`, `question` or `answer`; **not** named in the `supersedes` of any decision or note with `seq` <= `as_of`; **not** in an approved `record_retirements` row. A **pending** retirement is still in force (the User has not decided): the neighbour is shown with the flag `retirement pending` |
| Over-fetch | the filter runs after the search, so fetch 25 and keep 8 |
| Forced neighbours | every `[dnqa]<n>` id in the new text that is in force in this Context, plus the target of the record's own `supersedes` |
| Not in force | pending neighbours still count; `plan`, `brief`, retired and superseded never |

Implementation note: today `State::read` loads every record of the Context and `in_force_without` builds the full superseded set on each call (O(N) work, local not tokens). The write-time path must test each candidate with an indexed lookup (`NOT EXISTS (SELECT 1 FROM context_records WHERE supersedes = ?)`). I found no index on `context_records.supersedes` in `core/src/store/migrations/*.sql` (grep for `CREATE INDEX` on that table returned nothing); an index is needed for the lookup to stay cheap.

### 3.4 Pairwise verdict: prompt contract and JSON

Prompt shape (new file when built, versioned with its validator like `distill_prompt.md`): the instructions, the new record (id, kind, date, full text), then the neighbours as `[id] kind (date) topic` plus text capped at 800 bytes, oldest first. Everything in the record blocks is data, as in the current prompt. It must define each verdict in one line, plus: "`supersedes` means the NEW record overrules the WHOLE old one; if only a part is overruled, answer `contradicts`".

Output, one JSON object, no code fence:

```json
{"id": "pro-d83", "topic": "policy", "short": "Worker ladder is H0>H1>H2>H3>S1>S2>S3; drops the H3->S3 step",
 "verdicts": [
   {"neighbour": "pro-n62", "verdict": "contradicts", "confidence": "high",
    "quote": "H0->H1->H2->H3->S3"}
 ]}
```

| Field | Rule (checked in code, step 5) |
|---|---|
| `id` | exactly the record's id |
| `topic`, `short` | the bounds of `Batch::check`: topic 1 to 40 of `[a-z0-9-]`, short at most 120 bytes (prompt says 100), one line; a topic of `known` topics is reused |
| `verdicts` | one entry per neighbour, in the order given, each id exactly once, no extra id |
| `verdict` | `duplicate` \| `supersedes` \| `contradicts` \| `independent` |
| `confidence` | `high` \| `medium` \| `low`: an enum, not a probability (a 7-8B model does not calibrate a float; unverified, the `curation-judge` Job measures it) |
| `quote` | at most 160 bytes, required unless `independent`, **must be a verbatim substring of the new or the old text**: the code checks it, so an invented reason is rejected |

Direction rules reuse `Batch::retire`: `duplicate` keeps the older record and the new one is the retired one; `supersedes` retires the older one and the target is the new record; a `supersedes` whose neighbour has a higher `seq` is rejected. Verdicts of `low` confidence are stored, never applied.

One call carries all k neighbours (the proposal says "the model sees only the pair": that is k calls, k times the cost, still flat in N). A fallback per-pair call is for a `low` verdict only.

### 3.5 Apply: verdicts onto the existing tables

| Verdict | Existing table | Row | New relation row | Applied as |
|---|---|---|---|---|
| curation (always) | `record_curations` | upsert `(topic, short)` for the record, `ON CONFLICT DO UPDATE` as `propose_rejecting` does | none | at once |
| `duplicate` (N is a duplicate of older O) | `record_retirements` | `record = N`, `reason = duplicate`, `target = O`, `status = pending` | `duplicate_of(N, O)` | pending: the User decides |
| `supersedes` (N overrules whole older O) | `record_retirements` | `record = O`, `reason = superseded`, `target = N`, `status = pending` | `supersedes(N, O)` | pending |
| `contradicts` | **none**: nothing is retired | none | `contradicts(N, O)` | Inbox item for the User |
| `independent` | none | none | none | nothing stored |
| `documented` | `record_retirements` | not emitted at write time (3.8) | none | periodic sweep |

- **New table (spec, no DDL committed):** `record_relations(context, record, other, rel, confidence, quote, model, at)` with `rel` in (`duplicate_of`, `supersedes`, `contradicts`), primary key `(context, record, other, rel)`, foreign keys to `context_records`, registered in `history_tables` with `no_delete = 1` like its two siblings (`20261022T0203-curation.sql`). It exists for `contradicts` and `duplicate_of`, which `record_retirements` cannot hold; `supersedes` rows keep the quote and confidence the retirement row has no column for. `dup_count` is not a column: count approved `duplicate` rows targeting a record.
- **A contradiction flag is open while both ends are in force.** It clears when either one leaves force (a new decision with `supersedes`, or an approved retirement): no status column, no second writer.
- **`record_retirements` has one row per record** (`PRIMARY KEY (context, record)`, `ON CONFLICT DO NOTHING`, as now). A second proposal for the same record is dropped from the retirement table and kept in `record_relations`.
- **Invariants held.** Records are append-only (`context_records_immutable`, nothing here edits one). The pipeline writes only `pending`; `context retire` stays `user_only` (`curation.rs`). The proposal's "mark the old row `superseded`" automatically breaks hs-d551; rejected. At the measured 6 to 20 h decision delay, pending proposals leave duplicates in force for a day: retrieval may down-rank a record with a pending `duplicate` row, but it stays in `in_force`.
- **New Inbox kind** `curate-conflict` for `contradicts` needs a migration (the `inbox_items` check list names kinds): out of scope here, listed in `Findings to record`.

### 3.6 Where the hook runs

| Option | Pro | Con | Decision |
|---|---|---|---|
| Inline in `cpd context write` | the verdict is there at once | adds model latency (5.7 s or 51 s measured) to every write; a model outage fails or hangs writes; writes come from Sessions that must not wait | **no** |
| Enqueue, run by an engine tick | `write` stays as fast and as failure-free as now; the retry and backoff live in the engine | verdict is minutes late | **yes**: the record is already in force and findable by FTS; only curation lags |

The tick selects "in force and uncurated" (derived queue, 3.1 step 0), takes the Context's distill lock, processes the oldest record, releases it. Write rate to size for: 363 curations over 6.4 days is 57 per day, about one per 25 minutes, so a per-record call under the lock is never the bottleneck (arithmetic).

### 3.7 When the model or the embedder is down

Fail loud, never silently curate worse.

| Failure | Behaviour |
|---|---|
| Model: timeout, spawn, exit, no usable answer | the record stays uncurated, an `error` Event with the existing `distill_failed` shape (`reason`, `tail`), an entry in `runs.jsonl` (`kind = "write-curate"`, new). **Backoff, not a 1-minute retry**: 1 min, 2, 4, up to 1 h, reset on success (the 10 timeouts of 1.4 fired once a minute for 10 minutes and all failed) |
| Model: valid JSON, invalid lines | the whole record's output is rejected (as `distill_invalid`), kept as `.invalid` for the User, stays uncurated; one neighbour line failing becomes `independent` plus a counted rejection |
| Embedder down | FTS5 plus forced neighbours only; the curation is written with `neighbours = fts-only` in the run line; the record is queued for one re-embed, never blocked |
| Both down | stays uncurated; the existing batch `cpd context distill` is the fallback and curates it (topic and short only is acceptable then: no retirements) |
| Out of force at apply time | dropped, not an error (the rule of `propose_rejecting`) |

### 3.8 Idempotence, concurrency, `documented`

- **Idempotence:** "done" is the presence of the record's `record_curations` row, written in the same transaction as its relation rows, so a crash between steps leaves nothing. Re-running a done record is a no-op; re-running a failed one repeats the call and writes once. `record_relations` has a primary key; `record_retirements` has `ON CONFLICT DO NOTHING`. Same input and same model output give the same rows.
- **Concurrency:** the engine tick and `cpd context distill` both take `locks/distill-<context>.lock` (`flock`, `Lock::take`, `distill_running` when held). The tick takes it per record (seconds) and defers when a batch holds it (minutes); neither waits. Both re-read `State` inside the transaction and skip a curated record, so a record is curated by whoever gets there first.
- **`documented`:** 8 of the 13 retirements. It stays a periodic sweep that runs `git grep -l -w <id>` for in-force curated records without a `documented` row (the code of `grep_hits`), deterministic, no model needed to propose; the model's only role was `doc` (a lasting-rule flag) and it stays in the batch or the audit.

### 3.9 Cost per write, and what is O(1)

```
C_write = E + (P + t_new + k * t_nb) * p_in + (t_cur + k * t_v) * p_out
```

| Symbol | Value | Source |
|---|---|---|
| `p_in`, `p_out` | $0.75 and $3.75 per million tokens | fit of the 91 runs (Method) |
| `P`, prompt | 600 tokens | **guess:** about twice `distill_prompt.md` (28 lines) |
| `t_new` | 155 tokens | 541 bytes mean over the 379 decision and note texts / 3.5 bytes per token |
| `t_nb` | 137 tokens | 480 bytes mean after the 800-byte cap (arithmetic) / 3.5 |
| `k` | 8 | design |
| `t_cur` | 60 tokens | 134 median `tokens_out` / 2 lines, rounded |
| `t_v` | 40 tokens | **guess:** verdict, confidence, quote of at most 160 bytes |
| `E` | 0 | embedding cost not measured; **unverified**, a local or per-token embedder is negligible |

`tokens_in = 600 + 155 + 8 * 137 = 1,851`; `tokens_out = 60 + 8 * 40 = 380`. `C_write = 1,851 * 0.75e-6 + 380 * 3.75e-6 = $0.0014 + $0.0014 = $0.0028` per write, **flat in N** (arithmetic at Gemini 3.8 Flash prices; no write-time call was run). With neighbour `short` only instead of text: about 1,000 tokens in, $0.0021.

| N | Batch, per curated record (b = 4, mean) | Write-time | Cheaper |
|---:|---:|---:|---|
| 357 | $0.0034 | $0.0028 | about equal |
| 1,000 | $0.0074 | $0.0028 | write-time, 2.6x |
| 5,000 | $0.034 | $0.0028 | write-time, 12x |

(Batch per record = per-run cost of 1.3 divided by 4, run cost from the table in 1.3; with the median batch of 2 the batch is twice as costly.) At 57 writes/day: $0.16/day flat, against the batch's $0.19 now, $0.42 at 1,000 and $1.96 at 5,000.

**O(1) per write:** the model call (inputs bounded by k and the byte cap), the verdict validation, the apply transaction (at most k + 2 rows).

**Not O(1):**

| Part | Growth | Basis |
|---|---|---|
| FTS5 query | work with the number of matching rows (the `bm25` sort is a temp B-tree over matches) | `context-retrieval-design.md`: sub-millisecond at about 1.5k rows; larger N unmeasured |
| `sqlite-vec` kNN | linear scan of N vectors at a small constant | **unverified**, from memory that `vec0` is a brute-force scan; Plan 1 measures |
| In-force test | O(N) in today's code (3.3), O(log N) with an index | read from `inforce.rs` |
| Index maintenance | FTS5 segment merges and one vector append per record | `context-retrieval-design.md` triggers; amortised |
| Backlog catch-up | linear in records, one call each | arithmetic |

Local model (all **unverified**, nothing installed, pro-d209): an 8B model at Q4 is about 4.7 GB by arithmetic (8e9 x 0.5 byte + overhead), which leaves roughly 1 GB on the RTX 2060's 6 GB for the KV cache, enough for a 2.5k-token prompt; decode is bandwidth-bound (at the 2060's published 336 GB/s, about 70 tokens/s ceiling, so 380 tokens is about 5 s plus prompt time). The GTX 1050 Ti (4 GB) cannot hold an 8B model; embeddings only. Whether a 7-8B model judges 8 neighbours reliably is the question of the `curation-judge` Job, not answered here.

## 4. The periodic audit

It still fits, narrower than a rewrite of memory. Weekly, a larger model, never the whole store.

| Reviews | Why |
|---|---|
| every open `contradicts` flag (both ends in force) | the pipeline only flags them; a User or Lead settles them |
| every `low`-confidence verdict stored but not applied | the pipeline's uncertain cases |
| a random sample of `independent` and applied verdicts (size fixed in advance, for example 20 per week) | the only way to see missed duplicates and wrong applies; compares a stronger model with the write-time one |
| records with a pending retirement older than a threshold | measured delay was up to 45.5 h |
| the `documented` sweep (3.8) | deterministic |

Rules that move to `AGENTS.md` or repo docs are driven by **read counts from Plan 1's read log** (`entry_reads`, `entry_stats`), not by a model's guess. No such table exists in the live store (Lead, 2026-10-29), so until the log has weeks of data the audit proposes nothing of that kind. The old "suggest for docs" rule survives only as: a record with high reads across unrelated scopes, phrased as a rule, is listed for the User.

The audit **may not**:

- retire, supersede or archive a record (only the User approves, hs-d551);
- write a record, a `supersedes` link or any row of `record_retirements` as `approved`;
- decide a contradiction (it may rank and summarise the evidence);
- edit `AGENTS.md` or a repo doc (it may draft a proposal);
- read or send another Context's records.

## 5. Reconciliation with the proposal

| Claim | Verdict | Evidence |
|---|---|---|
| Batch cost and accuracy get worse as the store grows | **verified for cost** | 36 tokens/record, R² 0.98 (1.2); accuracy not measured here: 0 rejected lines in 101 runs, and 9 records unexplained (1.5) |
| "Whole-store judgments are weak" | **partly verified** | the batch found 5 whole-store-type pairs (all within 23 records) in 363; but 5 candidates in two topics went unflagged (2.2); the proposal's alternative (pairwise is more reliable) is **unverified** here |
| Cost per write stays flat | **model cost flat, query not strictly** | no N in `C_write`; FTS5 and vec scan grow with N, in-force test is O(N) today (3.9) |
| Prompt is a few hundred tokens | **wrong for cpd** | about 1,850 tokens with 8 neighbours' text, about 1,000 with shorts |
| "Model sees only the pair" | **unclear** | one call over k neighbours, or k calls; both flat in N |
| Local 7-8B on an RTX 2060 is enough | **unverified** | arithmetic only (3.9); quality is the `curation-judge` Job's |
| `subject` key gives deterministic supersession | **wrong for cpd** | no column; the model would pick the key, so it is not deterministic (3.2); `supersedes` is the existing deterministic rule |
| Four verdicts cover the cases | **incomplete** | partial supersession is 30 of 56 link mentions (2.2) |
| Apply: "mark the old row superseded" | **wrong for cpd** | the User approves retirements (hs-d551); the table is pending-only |
| "Old entries archived, never deleted" | **already true** | append-only records, in-force view (`inforce.rs`) |
| "Queue for the 10-minute batch if a human is needed" | **superseded** | the batch is the fallback only; flags go to the Inbox |
| Weekly audit sees flagged, low-confidence and a sample | **kept** | section 4 |
| `documented` retirements | **not in the proposal** | 8 of 13 retirements; cannot be produced at write time (1.6) |
| Haiku ran out of context | **consistent, unverified** | 182k tokens at N = 5,000 (1.3); the window size is not checked here |
| Winnow decision model | **unverified** | no source read |

## Findings to record

1. The batch distiller's `tokens_in` grows about 36 tokens per curated record in force (OLS on 91 valid runs of Context `cpd`, R² 0.977): about 38k tokens and $0.029 per run at 1,000 records, 74k and $0.056 at 2,000, 182k and $0.137 at 5,000 (projection).
2. Only 13 of 363 curations (3.6%) were ever proposed for retirement (8 documented, 4 superseded, 1 duplicate; 11 approved, 2 pending, 0 rejected); all 5 non-documented pairs sat within 23 records of each other.
3. Of 101 distiller runs, 10 failed, all gateway timeouts on bifrost/vertex at `upto_seq` 849 and 850, retried once a minute: write-time curation needs exponential backoff, and `record_curations` presence stays the idempotence marker.
4. `documented` is 8 of 13 retirements and needs the id to appear in a doc after the write, so write-time curation cannot emit it: keep a deterministic `git grep` sweep for it.
5. Partial supersession dominates: `refines` and `completes` (30) outnumber `supersedes` (16) in decision/note text; the four verdicts need a rule that a partial overrule is `contradicts`, never `supersedes`.
6. Lexical neighbour lookup (FTS5 bm25, 20 rarest terms) ranked the target in the top 5 for 8 of 11 real pairs and the top 10 for 10 of 11; the miss names its target's id in its text, so always include ids named in the text as neighbours.
7. Write-time curation costs about $0.0028 per write at k = 8 (arithmetic, unverified), flat in N, and equals the batch at about 350 records: it pays off from 1,000; the in-force test must change from an O(N) scan to an indexed lookup first.
8. Apply writes pending `record_retirements` only (User approves, hs-d551) plus a new append-only `record_relations` (`duplicate_of`, `supersedes`, `contradicts`) and a `curate-conflict` Inbox kind; add no `subject` column in v1.
