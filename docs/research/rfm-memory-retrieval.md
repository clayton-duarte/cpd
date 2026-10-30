# Context memory retrieval: findings (Plan `rfm-memory-retrieval`)

## Summary

- **Q1, holographic vs FTS5 + `sqlite-vec`:** the holographic bridge is live and the only search path; the native FTS5 table is designed but not built (section 1, Status today).
- Its vectors are SHA-256 bag-of-words: `delete`/`remove` cosine 0.028, and FTS5 alone selects the candidates, so it adds no semantics (section 1 (a), (b)).
- Every `cpd context search` spawns Python twice: 264-271 ms median over 15 runs, against about 20-25 ms predicted for in-process FTS5 (section 1 (d); the prediction is unverified).
- Recommendation: replace the bridge with in-process FTS5 first, add `sqlite-vec` only later, behind FTS5, in shadow mode; reversing `context-holographic.md` needs the User (section 1, Recommendation).
- **Q2, built vs proposed:** the pushed digest is already gone (a Manager gets about 75 tokens), so the proposal's pinned core is not needed; supersession exists, the `subject` key and any read log do not (section 2, verdict table).
- No read is recorded anywhere, but 449 of 533 id-shaped tokens in 656 records cite a real record, so `used` can come from citations once reads are logged (section 2 part 1).
- Not built on purpose: pinned core, `entry_stats`, a `contradicts` relation, the ranking formula and promotion jobs; each waits for read data or conflicts with "recency wins" (section 2 parts 3-5; section 3 preamble).
- **Roadmap item 1, `ctx-quote`:** fix the present defect: `hs-d655`, `db.migrations`, `foo/bar`, `a:b`, `it's` all fail as `usage` today; a query sanitiser, no schema change (section 1 (c); section 3 item 1).
- **Roadmap item 2, `ctx-reads`:** table `context_reads` and `cpd context reads`, an additive migration; starts the data that every ranking step needs (section 2 part 1; section 3 item 2).
- **Roadmap item 3, `ctx-fts`:** in-process FTS5 table with an `id` column, a breaking migration and a one-way door; target median at most 60 ms against 271 ms (section 1 (d); section 3 item 3).
- Later items: topics line, citation-based `used`, `--subject` (changes the git record format), bridge removal, shadow retrieval, `sqlite-vec` (section 3 items 4-9).
- Five User decisions and twelve records for the Lead are listed at the end of section 3.

Research only; no production code. Every number carries its method; anything not measured or read in source is labelled `unverified` in the same sentence. Sources: origin/main at `77b80a89` (fetched 2026-10-30); the holographic package read from `~/.local/share/cpd/holographic/venv/lib/python3.12/site-packages/holographic_memory/` (cited below as `hm/<file>:line`); measurements on 2026-10-30 EDT on the machine `pro` against its live store (Context `cpd`, 656 records, `cpd` 0.17.80-77b80a89).

## 1. Holographic vs native FTS5 + sqlite-vec

### Status today

- The holographic bridge is **current and the only search path**. `cpd context search` goes through `contrib/holographic/bridge.py` (`core/src/context/search.rs:133-141,167`; `core/src/context/engine.rs:168,174`), and `docs/contract.md:2434` ("The holographic engine rail", hs-d675, hs-d677) still specifies it. `git log origin/main` shows adoption (`cb06bc9a` "Search the Context through the holographic engine", `7f95d19b` design) and no commit that retires it.
- **The native FTS5 path is not shipped.** `docs/research/context-holographic.md:7-11` says the planned `context_search` FTS5 table and its triggers "are **not** built", superseding `docs/research/context-retrieval-design.md`. No `context_search` table exists in `core/src`, and no migration creates one (`grep -rn context_search core/src cli/src engine/src` returns nothing; `cli/tests/flow/main.rs:24` only names a test module). So "in-process FTS5" below is the design in `context-retrieval-design.md`, with its measurements cited, not a shipped command.
- The decisions hs-d655 (retrieve on demand), hs-d675 (adopt a local engine) and hs-d194 (cpd must not require a model) are cited by the docs (`context-holographic.md:7-9`, `context-memory.md:207`) but are **not in this machine's store** (`cpd context show --record hs-d655` returns `record_not_found` in Context `cpd`; ids here are `pro-*`). I read them only through the docs: unverified against the records themselves.
- A **newer** decision overrules the "no model" line of `context-memory.md:207`: Notes `pro-n174` and `pro-n176` (Plan `rfm-curation-models`, 2026-10-29 22:20 EDT) set the v1 stack as hosted embeddings through Bifrost (`gemini-embedding-2`, 768-d) plus FTS5, no local model. Neither says the bridge is retired.

### (a) Vectors are SHA-256 bag-of-words phase vectors, no semantics

- `hm/holographic.py:43-67` `encode_atom`: each word becomes 1024 phases from `hashlib.sha256(f"{word}:{i}")` blocks (`hm/holographic.py:63`). `encode_text` (`hm/holographic.py:111-132`) lowercases, splits on whitespace, strips edge punctuation, and bundles one atom per token. No model, no learned embedding.
- Measured with the package's own functions (one call per pair, `encode_text` then `similarity`, 1024 dims):

| Pair | Phase cosine |
|---|---:|
| `delete` / `delete` | 1.0 |
| `delete` / `remove` | 0.028 |
| `car` / `automobile` | -0.0065 |
| `config key` / `settings option` | -0.0073 |

  Synonyms score about 0 (the same as unrelated words); the vector only measures shared tokens, so it duplicates what Jaccard already measures.

### (b) Stage 1 is gated only by FTS5; vectors only rerank

- `hm/retrieval.py:66` takes candidates from `_fts_candidates` (the `facts_fts MATCH ?` query, `hm/retrieval.py:481-543`); `hm/retrieval.py:68` returns `[]` when there are none. Only those candidates reach the rerank: `relevance = 0.4*fts + 0.3*jaccard + 0.3*hrr` (weights at `hm/retrieval.py:29-31`, formula at `:92-94`). A fact that FTS5 does not match is never scored, whatever its vector says.
- The FTS5 table is `fts5(content, tags, content=facts, content_rowid=fact_id)` (`hm/store.py:49`): the default `unicode61` tokenizer, **no stemming** (`context-memory.md:196` recommends `porter unicode61`; the engine does not use it). The bridge indexes `topic short text` as `content` and `tags=""` (`contrib/holographic/bridge.py:25-31,119`), and the record id only as `category`, which is not in the FTS table.
- Consequence for exact identifiers: a query for a record's own id never returns that record. Measured: `cpd context search '"pro-d217"' --context cpd --limit 20` returns only `pro-d219` (which cites `pro-d217`), while `pro-d217` exists (`cpd context show --context cpd --record pro-d217`). Single run, deterministic.

### (c) Hyphenated queries fail, in the bridge and in `cpd context search` today

- Reproduced on the shipped CLI (`cpd` 0.17.80-77b80a89), each one run:

| Command | Result |
|---|---|
| `cpd context search hs-d655` | `usage`: `no such column: d655` |
| `cpd context search pro-d201` | `usage`: `no such column: d201` |
| `cpd context search drafts-parked` | `usage`: `no such column: parked` |
| `cpd context search sqlite-vec` | `usage`: `no such column: vec` |
| `cpd context search "first-message digests"` | `usage`: `no such column: message` |
| `cpd context search 'pro-d217*'` | `usage`: `no such column: d217` |
| `cpd context search '"pro-d217"'` (quoted) | ok |

- Cause: FTS5 reads `-` outside a quoted string as a column filter or NOT, so `hs-d655` parses as column `d655`. The query is **not built anywhere**: `core/src/context/search.rs:125` only `trim()`s it, `search_request` (`search.rs:38-40`) puts it verbatim in `{"cmd":"search","query":…}`, and the bridge runs it as `MATCH ?` (`contrib/holographic/bridge.py:161-163`). The bridge probe turns the `OperationalError` into `bad_query` (`bridge.py:164-165`), and `check_ok` (`search.rs:56-68`) maps it to `usage`. Inside the package the same error is swallowed as "no hits" (`hm/retrieval.py:521-524`), which is why the bridge probes first.
- The raw-FTS5 contract is deliberate (`search.rs:34`: "FTS5 syntax; a trailing `*` is a prefix match"; `context-retrieval-design.md:132-136` accepts `OR`, `NEAR`, quotes). So the fix is a **query sanitiser**, applied in `search()` at `search.rs:125` before `search_request`, not a change in the engine: quote each whitespace-separated token that holds a character other than `[A-Za-z0-9_*]` (so `hs-d655` becomes `"hs-d655"`), and leave already-quoted phrases, `AND`/`OR`/`NOT`/`NEAR` and parentheses alone. The quoted form already works (table above); the sanitiser's exact grammar is unverified (not built). It is the same for either backend, because both receive an FTS5 `MATCH` string.

### (d) Latency: every bridge search spawns Python, twice

- Source: one `cpd context search` runs `bridge.run` once for `stats` (`search.rs:141`) and once for `index`+`search` (`search.rs:167`), a third time on the short-page retry (`search.rs:180`); each `run` spawns a Python child (`engine.rs:168,174`).
- Measured end to end with `subprocess.run` around the shipped CLI, `--context cpd` (656 records, 10 MB engine DB), `timeout` 60 s each, index already up to date:

| Command | Runs | min | median | max |
|---|---:|---:|---:|---:|
| `cpd context search 'retriev*' --limit 20` | 15 | 231.6 ms | **271.1 ms** | 319.5 ms |
| `cpd context search decision --limit 20` | 15 | 248.7 ms | **270.4 ms** | 306.9 ms |
| `cpd context search zzzzqqq` (no hit) | 10 | 231.9 ms | **264.1 ms** | 334.0 ms |
| `cpd context show --record pro-d217` (cpd.db only, no bridge) | 15 | 16.6 ms | **18.8 ms** | 33.8 ms |
| `cpd context list` (cpd.db only, no bridge) | 15 | 23.9 ms | **43.5 ms** | 116.4 ms |
| `cpd --version` | 15 | 13.0 ms | **14.0 ms** | 20.0 ms |

  The two spawns match the design's own measurement of one spawn plus one request at 97-143 ms, median 128 ms (`context-holographic.md:43-46`; Python import cost 84 ms). The ~270 ms is about 250 ms over the no-bridge CLI, and nearly all of it is process start and import.
- In-process FTS5, from the design's measurements, not re-run: 0.04-0.33 ms per `MATCH` over the real store, median 0.10 ms for `decision` (`context-retrieval-design.md:55-69`, bundled SQLite 3.53.2), plus the 13-19 ms of the CLI itself (`cpd context show --record` above). A back-fill of 21 ms for 1.4k records (`context-memory.md:36,206`; `context-retrieval-design.md:66-68`). Expected `cpd context search` latency of the native path: about 20-25 ms (inference from the two figures; unverified, the command is not built). The engine's own search (no spawn) is 1.35-20 ms (`context-holographic.md:39-42`), so the holographic stage itself is not the cost.

### (e) The duplicate SQLite DBs under the engine directory

- What they are: `<context_engine_dir>/<context>.db`, one per Context searched (`search.rs:139`; `reindex.rs:36`; default dir `$CPD_HOME/engines/holographic`, `core/src/config/file.rs:464-466`). The directory listing on this machine (names and sizes only) has six: `bifrost.db` 0.7 MB, `cpd.db` 10.0 MB, `eec-codereview.db` 2.1 MB, `eec-general.db` 2.0 MB, `host.db` 0.3 MB, `slack.db` 0.6 MB: about 15.7 MB of copies of text that already sits in the 129 MB `cpd.db` (a `ls -la`, one run).
- Who reads them: from source, only the bridge process, started by `search` and `reindex` (`grep context_engine_dir` over `core/src cli/src engine/src` finds `search.rs:139` and `reindex.rs:36`, nothing else). No cpd Rust code opens them directly; no other command reads them (that no non-Rust tool reads them is unverified).
- What stays in sync: nothing, automatically. They are derived, projected lazily at search time: `search()` compares the watermark (`indexed_seq`, `curated_at` in the `cpd_watermark` table, `bridge.py:49-79`) with `max(seq)` and the curation clocks, re-indexes only newer records (`search.rs:147-163`), and never on writes (`docs/contract.md:2434`). Gaps visible in source: `index` upserts by `category` and does not delete (`bridge.py:105-148`), so a deleted record stays in the copy until `reset`; `reset` runs only on `indexed_seq == 0` (`search.rs:148-150`) or `cpd context reindex`. Contexts that are archived or deleted keep their file (unverified: no code path that removes the file was found).

### Comparison

Columns: **H** = the shipped holographic bridge; **F** = in-process FTS5 table in `cpd.db` (`context-retrieval-design.md`; not built); **F+V** = F plus `sqlite-vec` in `cpd.db` (the proposal).

| | H (shipped) | F (designed, not built) | F+V (proposal) |
|---|---|---|---|
| Exact identifiers (config keys, ids) | Hyphenated and dotted tokens fail unquoted ((c)); a record's own id is not indexed ((b)); no stemming | Same FTS5 syntax, so same failure without a sanitiser. Index `id` as a column and the id lookup becomes exact; `porter unicode61` is a one-line choice (`context-memory.md:196`) | Same as F, and FTS5 stays the exact-match leg (`Per-turn retrieval`: "embeddings alone miss exact identifiers"). A vector never replaces it |
| Synonyms | None: 0.03 cosine for `delete`/`remove` ((a)), and FTS5 gates stage 1 ((b)) | None; stemming covers inflections only | Yes, by the embedder's quality. Measured elsewhere, not here: the FTS5+RRF hybrid (`pro-n174`, `rfm-curation-embeddings.md`) |
| Latency per `cpd context search` | 264-271 ms median, 15 runs ((d)) | Design: 0.04-0.33 ms in SQLite, ~20-25 ms for the CLI (inference, unverified) | F plus one embedding call for the query: `pro-n174` measures p50 405 ms per hosted call. Brute-force kNN is 0.22 ms at 10,000 x 768-d (`pro-n174`; `rfm-curation-embeddings.md:121-129`, numpy, not `sqlite-vec` itself: unverified). A local model removes the network but not the cost of loading it |
| Build and dependency cost | Python 3.12 venv + numpy + a pinned git clone of `holographic-memory` (the PyPI name is another project, `context-holographic.md:59`), dotfiles install, the bridge script, 15.7 MB of copies | Nothing new: `rusqlite` 0.40 `bundled` (`Cargo.toml:23`) already has FTS5 (289 `fts5*` symbols, `context-retrieval-design.md:30-32`). A migration and five triggers | F plus the crate `sqlite-vec` 0.1.10-alpha.4 (`cargo info sqlite-vec`, 2026-10-30; MIT/Apache-2.0; **alpha**, pre-v1). Its `build.rs` compiles one 340 KB `sqlite-vec.c` through `cc`. It loads with `sqlite3_auto_extension(sqlite3_vec_init)` before opening the Connection (the crate's own test, `src/lib.rs`), an `unsafe` call into C (cpd has no `unsafe_code` lint in `Cargo.toml`/`core/src/lib.rs`: grep found none). Plus an embedding source |
| Embedding source | None (hash vectors) | None | A hosted model through Bifrost is the current decision (`pro-n176`: `gemini-embedding-2`, 768-d), which sends the full text of each record out (open question `pro-q1`). A local model is not decided; the cost table in `rfm-curation-embeddings.md` is arithmetic, labelled `unverified` there (for example `all-minilm-l6-v2`, 45 MB, ~34 ms on CPU), and I did not measure it |
| Failure modes | Python missing -> `engine_missing`; child crash/timeout -> `engine_failed` (`contract.md:82,2434`); stale copy ((e)); FTS error swallowed in the package | A bad query is `usage`; trigger bug leaves the index stale. All in one SQLite transaction with `cpd.db`, so no drift | F's modes plus: embedder down or rate-limited (search must degrade to FTS5, never fail); embedder change invalidates every vector; alpha extension; vectors in `cpd.db` leave the file bigger (10,000 x 768-d float32 = 30.7 MB, `pro-n174`) |
| Migration and rollback | Already live. Rollback is `context_engine_enabled = false` (exit 3) | One additive migration (table + triggers) and a back-fill (~21 ms). Rollback: drop the table, keep the bridge until F is proven (`cli` flag or config, not built) | Add the vec table after F, in shadow mode as the proposal's step 3 says. Rollback: drop the vec table; FTS5 stays. Vectors are derived data, so no information is lost |

### Recommendation

1. **Replace the holographic bridge with in-process FTS5 in `cpd.db`**, and do it first, alone. It removes two Python spawns (about 250 ms of the 270 ms), the dotfiles install, and the 15.7 MB of out-of-sync copies; it adds only a migration. The holographic vectors give nothing for synonyms ((a)) and cannot widen recall ((b)), so the bridge loses no capability that matters. This follows the existing design (`context-retrieval-design.md`), which was superseded for the engine choice by `context-holographic.md`; that is a newer decision overruling an older one, so the User's confirmation is needed to reverse it (open risk 1).
2. **Fix the hyphen failure in the same change** with the query sanitiser at `core/src/context/search.rs:125`, plus an `id` column in the FTS table so a record id finds the record. This is independent of the backend: it fails the same way on either today.
3. **Add `sqlite-vec` afterwards, behind FTS5, in shadow mode**, only when the User accepts the hosted-embedding privacy answer (`pro-q1`). FTS5 stays the exact-match leg. The synonym gain is the only reason to add it and it is not measured here.

### Open risks

1. Reversing `context-holographic.md` needs a new recorded decision; hs-d655/hs-d675 are not readable in this store (unverified).
2. `sqlite-vec` is alpha (0.1.10-alpha.4) and loads through an `unsafe` auto-extension; pin it and test on the release targets.
3. Embeddings send record text to a hosted provider (`pro-q1`, open); a local model is unmeasured.
4. The sanitiser grammar must keep the documented FTS5 operators working; it needs tests that a quoted phrase, `OR` and `term*` pass unchanged.
5. Latency of the native path is a design figure, not measured on the shipped command; measure it once the FTS5 table exists.
6. The measurements are one machine, one Context (656 records), indexes warm; the first search after a deploy re-indexes from scratch (about 25 s at 1.4k records, `context-holographic.md:36-37`), a cost the native path removes.

## 2. What cpd built vs what the proposal adds

Method for every number below: the live store of Context `cpd` on machine `pro` (all 656 records are `pro-*`), read with the shipped CLI on 2026-10-30 EDT: `cpd context show --since 0 --context cpd` (one run, saved to a temp file outside the worktree, then counted with a short Python script: kinds, `supersedes` fields, text sizes, id regex) and `cpd context show --topic <t>` for each of the 20 topics. Tokens are bytes/4 (an estimate, not a tokenizer count). `~/.cpd/cpd.db` was not read directly, so the `record_retirements` rows are not counted: every "in force" count below is therefore an upper bound that ignores approved retirements, except the curated count, which the CLI computes with retirements applied (`core/src/context/curated.rs:33-40`).

### What cpd already built (the baseline)

- **No digest is pushed.** A Manager's first message is a header plus one pointer line (`core/src/context/manager.rs:172-180`); a Lead's is the header, the brief, and the same pointer line, "no store read" (`core/src/plan/new.rs:246-263`). A Worker reads its Spec only (`pi/skills/cpd-lead/SKILL.md:51`, "a Worker runs without skills and reads only its Spec"). The Manager's pointer message is about 300 bytes (about 75 tokens, estimate from the two format strings). Briefs are short: 107 briefs in the store, median 128 bytes, 90th percentile 219 bytes, max 826 (one run).
- **The decision behind it, hs-d655, is not readable in this store.** `cpd context show --record hs-d655` returns `record_not_found` and a quoted search for it returns nothing (also noted in section 1). I read it through the docs that cite it (`docs/research/context-memory.md:226-229`, "hs-d655 drops the digest: the first message is the agent's brief (a Lead) or nothing but a pointer (a Manager)") and through the code above, which does exactly that: the claim "full-head digest injection is gone" is verified in source, the record itself is unverified. No newer record on this machine reverses it (a `cpd context search` for `digest` and `first message` found only unrelated records `pro-d24`, `pro-n23`, `pro-n24`, `pro-n151`; single runs).
- **`context_records` is append-only**: `context_records_immutable BEFORE UPDATE ... RAISE(ABORT, 'append_only: context_records')` (`core/src/store/migrations/0001-initial.sql:322-323`); it is "an index of the git store; rebuildable" (`0001-initial.sql:44`). Supersession lives on the **successor** (`supersedes` column), never as a mutation of the old row.
- **Curations and retirements** are side tables keyed `(context, record)` (`core/src/store/migrations/20261022T0203-curation.sql:9-31`): `record_retirements(reason IN duplicate|superseded|documented, target, status pending|approved|rejected)` and `record_curations(topic 1-40, short <=120 bytes, doc_candidate pending|sent|dismissed, doc_paths)`. Both are registered `no_delete` history tables (`:33`).
- **In force** = kind decision/note/question/answer, not named by any `supersedes` at or before `as_of`, and not in an approved retirement (`core/src/context/inforce.rs:35-47`, `retired` at `:25-29`).
- **Store size today** (Context `cpd`, 8 days of data, 2026-10-23 to 2026-10-30, 91 distinct writer sessions): 656 records = 234 decisions, 204 notes, 109 plans, 107 briefs, 2 questions; 30 records carry `supersedes` (6.8% of the 438 decisions and notes). Not named by a `supersedes`, of kind decision/note/question/answer: 410 records, 243,340 bytes of text (about 61k tokens, median 489 bytes). **395 curated records in force across 20 topics, whose `short` forms total 36,331 bytes (about 9.1k tokens).** The whole `context show --since 0` markdown is 314,223 bytes (about 78k tokens).
- **What the existing Manager and Lead updates already do.** `pi/lib/updates.ts:289-305` appends a hidden message each Manager turn with the records new since its cursor (at most `MAX_LISTED = 20`, `updates.ts:35`, then `cpd context show --since <seq>`), plus `Plans:`, `Pending work:`, `Errors:`. A Lead gets "the short message: new decision records by other Sessions (id and first sentence)" (`updates.ts:263`). These are appended messages, so the prompt prefix does not change: the proposal's "inject only what is new, append, don't rewrite" is built, for recency but not for relevance.
- **No read is recorded anywhere.** `core/src/context/show.rs` and `search.rs` contain no `INSERT` or `UPDATE` (grep, 0 hits). `context_reads`/`entry_reads` do not exist in any migration.

### Verdict table

| # | Proposal feature | Verdict | Evidence | Smallest cpd-shaped addition |
|---|---|---|---|---|
| 1 | Read tracking: `entry_reads`, `entry_stats`, channels, `used`, dedupe, never-block | **missing** | No read row anywhere (above). Citations are already in the text: 533 id-shaped tokens in 656 records, 449 of them (84%) resolve to a record of this store (see 1a) | Table `context_reads` + `core/src/context/reads.rs`; log from `show.rs` and `search.rs`; flag-free; `used` from a citation scan at write time (SQL in 1c) |
| 2 | Deterministic `subject` + `scope` tags | **partly built**: explicit supersession is built, the key is not | `--supersedes <record>` on `decide`/`note` (`core/src/context/mod.rs:209-226`), validated at write (`core/src/context/contexts.rs:253-275`, `supersedes_invalid`, `record_not_found`), applied by `in_force` (`inforce.rs:35-47`). 30 of 438 decisions/notes use it | `--subject <key>` and `--scope <path>...` on `decide`/`note`; the write resolves a subject to a `supersedes` (SQL in 2) |
| 3 | Pinned core + per-task / per-turn retrieval | **partly built; pinned core not needed**; relevance retrieval is manual | Pushed text today is about 75 tokens (Manager) or the brief (Lead); the always-injected core is `AGENTS.md` (`context-memory.md:226-229`). Recency pushes exist (`updates.ts:35,263,289-305`). Retrieval is `cpd context search`, 264-271 ms median (section 1 (d)), 20 shorts = 402-541 tokens (4 queries, one run each, below) | A topics-with-counts line in the Manager's turn-start message; a `--scope` filter on `search`; retrieval hook only after reads are logged (see 3) |
| 4 | Write-time pairwise curation | **partly built** (curation and retire proposals exist; the neighbour window and a `contradicts` verdict do not) | `distill.rs:125-129` batch of at most 60 records / 48 KiB; `known.md` holds every curated record's short (`distill.rs:442-451`), so a batch reads 36 KB / about 9.1k tokens of context records at 395 records; reasons `duplicate`/`superseded`/`documented` (`curation.sql:12`) | Cut `known.md` to the nearest neighbours of the batch from the FTS5 index (section 1's recommendation); no schema delta; keep it asynchronous |
| 5 | Ranking formula, promotion/demotion table | **missing; build only the log now**, ranking after data | No reads, no pinned tier, no `used`. The store is 8 days old; the proposal itself waits 2-3 weeks (`Rollout` step 6). Part of the table is built without reads: "suggest for repo docs" is `doc_candidate` + `git grep` of the id in docs (`distill.rs:421-423,624`) and "archive when superseded" is `record_retirements` | A read-count query over `context_reads` (see 5); no decay, no `alpha`, no promotion job yet |

### 1. Read tracking

**(a) Today.** Nothing logs a read, so no use signal exists. One proxy already exists in the data: the id citations inside records. Counted with the regex `\b([a-z][a-z0-9]*)-([dnqapb])([0-9]+)\b` over the text of the 656 records (one run):

| Class | Count | Reading |
|---|---:|---|
| Resolves to a record of this Context | 449 | a real citation |
| `hs-d...`, another machine's id, not in this store | 51 | real citation, unresolvable here: dropped if resolved against the store |
| `<job>-a<n>`, an Attempt id (`config-a1`, `tests-a1`, `focus-a1`) | 32 | **false positive**: the Attempt id shape collides with the answer letter `a` |
| `pro-...`, absent | 1 | typo or deleted |

209 of the 656 records are cited at least once by another record, 99 at least twice, 12 at least five times (most cited: `pro-d179` 11, `pro-d30` 8, `pro-d126` 8). So a citation count is computable now with no new table, as a shadow of the proposal's `used_30d`; it measures what writers cite, not what readers read.

**(b) Where cpd would detect `used = 1`.** At write time, in `Contexts::write` (`contexts.rs:167`), next to `check_links` (`contexts.rs:232-275`, which already validates `supersedes`), after the record is accepted, in the same transaction:

- `context decide` / `note` / `ask` / `answer`: scan `text`. Exclude the record's own `supersedes` (a replacement is not a use).
- `plan new --brief` and `plan outline` (`core/src/plan/outline.rs`): scan the brief and the outline JSON string fields.
- `job add --spec` (Spec text a Lead writes): scan; the Lead is the user, the Worker never searches.
- `session send` (`core/src/session/send.rs`): scan the message; the sender is `CPD_SESSION`/`PI_SESSION_ID` (`send.rs:22-29`).

Rule: a match counts only if (1) `RecordId::parse` accepts it (`ids.rs:335-351`, machine, letter `d n q a p`), (2) the id exists in `context_records` for that Context, (3) the citing session has a **read row** for that record in the same session (this ties `used` to an actual read; without it a copied id is not a use), and (4) it is not the record's own id. Rule (2) drops the `hs-` and `pro-` misses above; rule (3) removes the rest of the Attempt-id collisions, because an Attempt id `x-a1` would need a record `x-a1` that the session read. Residual false positives: a record cited only to contradict it (a "supersedes" in prose), or copy-pasted from a brief (the Lead repeating the Manager's ids): 46 of 107 briefs contain an id (measured), so the Manager's briefs already make Leads "cite". Dedupe below handles the repeat, not the intent. The post-session check (reading the diff for reflected content) needs a model and is not recommended now.

**(c) Schema delta** (proposal's `entry_reads` + `entry_stats`, cpd-shaped: keyed `(context, record)` like `record_curations`, STRICT, a history table):

```sql
CREATE TABLE context_reads (
  context  TEXT NOT NULL REFERENCES contexts(id),
  record   TEXT NOT NULL,
  session  TEXT NOT NULL,                 -- CPD_SESSION; a read without a Session is not logged
  channel  TEXT NOT NULL CHECK (channel IN ('pushed', 'search', 'show')),
  read_at  TEXT NOT NULL,
  rank     INTEGER,                       -- position in the result list (search only)
  score    REAL,
  used     INTEGER CHECK (used IN (0, 1)),
  PRIMARY KEY (context, record, session, channel),   -- one row per record, session, channel (dedupe)
  FOREIGN KEY (context, record) REFERENCES context_records(context, id)
) STRICT;
```

- **Channels.** The proposal's `pinned` has no cpd counterpart (nothing is pinned or pushed beyond the pointer; `AGENTS.md` is not in the store). cpd has `pushed` (Manager/Lead update lines, `updates.ts`), `search` (`cpd context search`), `show` (`--record`, `--topic`). `explicit` = `search` + `show`.
- **Dedupe per session/channel** is the primary key with `INSERT OR IGNORE`; a repeated read adds nothing. Cost: the table cannot count repeats. The proposal does the same.
- **`entry_stats` is not needed.** At the store's size the rollup is a `GROUP BY` over `context_reads` (see 5). Add the rollup table only when the query is slow (unmeasured, not built).
- **Pruning at 90 days** conflicts with a `no_delete` history table (`curation.sql:33` sets `no_delete = 1` for the curation tables): register `context_reads` with `no_delete = 0` (the column is read from the migration; how the runner treats it is unverified) or skip pruning: one row per record per session per channel is at most 656 x 91 = 59.7k rows today (upper bound, 8 days).
- **Never block a session.** cpd commands are short processes, so "asynchronous" is a best-effort `INSERT OR IGNORE` after the output is built, in its own small transaction: a failure becomes an `error` Event (the existing mechanism) and the command still exits 0. The reads are on a single local SQLite file; `context show --record` is 18.8 ms median (section 1 (d)); the insert's own cost is unmeasured. This is the one place the axiom "fail loud" is relaxed, and only for the log, not for the read.
- **Guard against feedback loops.** Today nothing is ranked, so there is no loop yet. When ranking is built: `pushed` reads never feed promotion (the proposal's third guard, kept as is); exploration slots are not needed while retrieval is manual (the Manager decides what to search).
- **Rollout** matches the proposal's step 1 ("Instrument reads, no behavior change"): ship `context_reads` first.

### 2. `subject` key and `scope` tags

**Compared with what exists.** `--supersedes <record-id>` is explicit and manual: the writer must know the old id. The proposal's `subject` lets the writer name a topic instead, so the supersession is found by the system. In cpd this is sugar over the same field, so `in_force` (`inforce.rs:35-47`) and `render.rs` do not change:

1. `decide --subject db.migrations.tool` looks up the in-force record (after retirements, `in_force_without`) with that subject in the same write transaction.
2. Exactly one: its id becomes `supersedes` (the existing `supersedes_invalid` check applies: a decision may supersede a decision or note). None: nothing. More than one (two machines wrote concurrently; the existing tests keep both, `inforce.rs` test `two_decisions_superseding_one_record_both_stay_and_the_old_one_goes`): refuse with a new error `subject_ambiguous` listing the ids, so the writer chooses with `--supersedes`. No silent choice.
3. `--supersedes` and `--subject` together: allowed; `supersedes` wins and the subject is only stored. A wrong automatic supersession is undone the cpd way: a new decision with the same subject that restates the old text (recency wins), never an edit.

**Append-only and retirements.** Nothing is updated: `subject` and `scope` are new columns on the record, set at insert, so `context_records_immutable` still holds. A subject on the new row, plus a `supersedes` on it, leaves the old row untouched. `record_retirements` stay separate (an approved retirement is the User's decision on duplicates/documented records and is not needed for a subject supersession, which the writer made). A retired record with subject X must not count as the holder of X: the lookup uses `in_force_without`.

**Schema delta.** The record is also a line of the git store: `Record::parse_line` rejects unknown fields (`record.rs:154`, `deny_unknown_fields`) and is versioned (`VERSION = 1`, `record.rs:13`). A subject that only lives in an SQL side table would be lost on a rebuild from git (`0001-initial.sql:44`: "rebuildable"). So it belongs in the record:

```sql
ALTER TABLE context_records ADD COLUMN subject TEXT
  CHECK (subject IS NULL OR subject GLOB '[a-z]*' AND length(subject) BETWEEN 1 AND 80);
ALTER TABLE context_records ADD COLUMN scope TEXT
  CHECK (scope IS NULL OR (json_valid(scope) AND json_type(scope) = 'array'));
CREATE INDEX context_records_subject ON context_records(context, subject) WHERE subject IS NOT NULL;
```

with the matching optional `subject` and `scope` fields in the JSONL record (`Raw`, `Record::to_json`) and `Draft::Decision`/`Note*` variants carrying them. cpd's migrations carry a schema hash and "breaking after N" markers (`curation.sql:1`); the exact migration shape for an `ALTER` on a table with an immutability trigger is unverified. Do **not** add `subject`/`scope` to kinds that cannot supersede (question, plan, brief): carrying them in `Draft::Decision` and `Draft::Note*` only makes the illegal state unwritable.

**Who declares `scope` and what it is for.** The writer, optionally. The curator already writes `topic` (20 values in use, `cpd context show --topic x` lists them) and the distiller never sees the working files. Paths a decision names are usually in its own text, so FTS5 finds them today (a quoted token: section 1 (c)). Scope matters only as a search filter (`search --scope <path-prefix>`), not for ranking. Build `--subject` first, `--scope` only when retrieval is built (YAGNI).

### 3. Pinned core and per-task / per-turn retrieval

**Is the pointer line plus `cpd context search` enough at the current size?** For retrieval, yes; for discovery, partly. Measured:

| Quantity | Value | Method |
|---|---:|---|
| Pushed per Manager first message | about 75 tokens | format string, `manager.rs:174-179`, bytes/4 |
| All curated shorts in force | 36,331 bytes, about 9.1k tokens, 395 records, 20 topics | `show --topic` for each topic |
| One search page of 20 shorts | 2,043 / 2,166 / 1,911 / 1,610 bytes (about 400-540 tokens) | `search decision`, `supersede*`, `curat*`, `"record"` `--limit 20`; one run each |
| The proposal's budget (pinned 2-3k + retrieved 3-4k) | 5-7k tokens | proposal text |

The proposal's target "under 5k injected tokens per session at 1,000+ entries" is already met by cpd at 656 records (75 tokens plus on-demand pages) because nothing is pushed. Adding a pinned 2-3k core would **raise** the injected cost on every Manager and Lead session; the pinned core's job (rules for every session) is done by `AGENTS.md`, which hs-d655 chose as the single source (`context-memory.md:226-229`). **Verdict: not needed.** If one ever is, promote by `context_reads`, never by hand-picking (the proposal's point), after the log has data.

Gaps that are real:

- **Discovery.** The model does not know what it does not know. cpd's pointer line lists commands, not topics. Cheapest fix: one line `Topics: adapter 12, attempt 9, ...` in the Manager's turn-start message (20 topics, about 150-250 bytes; `curated::topics` already exists, `curated.rs:55-58`, and the same list appears in the `topic_not_found` error). Count per topic needs one more function. This is the proposal's "small pinned index (topics with counts)".
- **Search is broken for ids and hyphenated queries** (section 1 (c)): `cpd context search hs-d655` is a `usage` error. Any retrieval that sends ids or paths to FTS5 needs the sanitiser first.
- **Adherence is unmeasured.** Whether Managers and Leads actually search before acting is not known, because no read is logged. That is the argument for table `context_reads` (1c) before any retrieval hook.

**Per role.**

| Role | Proposal | cpd today | Recommendation |
|---|---|---|---|
| Manager | retrieve every turn | incremental update of new records, 20 max (`updates.ts:35,289-305`); search on demand | Keep. Add the topics line. A per-turn retrieval (query from the latest message plus the pending plans text) in `pi/lib/updates.ts` only after shadow data: log what it would have injected in `context_reads` (channel `pushed`, `used` null) for 2-3 weeks |
| Lead | once per feature from the brief, again as the draft changes, then per turn | brief in the first message; skill says read at four named points (`pi/skills/cpd-lead/SKILL.md:63`); update message of new decisions | The brief already names ids (46 of 107 briefs hold at least one). Cheapest improvement is a Manager habit, enforced by a `plan new` lint warning when a brief cites no record id; per-feature search stays manual. Retrieval from the brief is the one hook worth a shadow run |
| Worker | none (lead bundles) | Spec only, no skills | Already built; keep. A Worker never searches, so it never writes a read row (no `CPD_SESSION` read) |

**Cache-safe injection.** Built: `updates.ts` appends a hidden custom message at turn start; nothing rewrites the first message or system prompt (the first message is delivered once, `pi/lib/first-message.ts:15-25`). Any retrieval block must reuse that path, not edit the head.

**Thresholds.** "Inject nothing below a score" needs a labelled set of tasks: none exists. Present scores are the holographic reranker's 0.17-0.28 on single-word queries (`context search digest`, one run), not calibrated: unverified as a threshold. The FTS5 + `sqlite-vec` score of section 1 would need its own calibration.

### 4. Write-time pairwise curation vs today's distiller

- **What the distiller is.** A detached, asynchronous run per Context (flock, one at a time, `distill.rs:1-7`), at most 60 uncurated records and 48 KiB per step (`:125-129`), a one-shot pi call with `batch.md`, `known.md`, `docs.md` (`:402,433-461`; prompt in `distill_prompt.md`). It writes a `topic`, a `short`, and optionally a retirement proposal the User approves in the Inbox (`curate-retire`) or a doc candidate (`curate-docs`). The whole-store judgement the proposal calls weak is present only as `known.md`: the shorts of all curated records in force, **36 KB at 395 records**, every batch. At the proposal's 1,000 entries that is about 23k tokens (linear extrapolation, unmeasured), more than a neighbour window of 5-10 shorts (about 1k tokens, 10 x 92 bytes/4).
- **The pairwise verdict exists in another shape.** `duplicate` (keep the older, retire the later) and `superseded` (a newer record overrules) are the proposal's `duplicate` and `supersedes` (`distill_prompt.md`, the `retire` field). `contradicts` has no home: the `reason` CHECK allows three values (`curation.sql:12`). cpd's axiom is "recency wins", so a contradiction is resolved as `superseded`, not flagged; I do not recommend a `contradicts` relation.
- **What differs.** The proposal judges at write time, per record, with a local 7-8B model. cpd's batches are asynchronous and use a hosted model; cpd's own rule that a write must not need a model (hs-d194, cited in `context-memory.md:207`; the record itself is not readable here, unverified) is kept by the distiller and would be broken by a synchronous judge in `context decide`. Subject supersession (2) is the part of "write-time" that needs no model and is the one to take.
- **Recommendation (later, not now).** When the FTS5 table of section 1 exists, build `known.md` from the top-k neighbours (k about 8) of each batch record instead of every short. No schema change, no new command, the output contract (`topic`, `short`, `retire`, `doc`) is untouched. It pays only when `known.md` outgrows the model's window or its price; at 395 records it is about 9k tokens and does not. Labelling about 100 pairs to compare judges (the proposal's step 5) belongs to the Plan that owns curation models (`pro-n174`/`pro-n176`, section 1); not measured here.

### 5. Ranking formula and promotion/demotion

| Piece | Build now? | Why |
|---|---|---|
| `context_reads` log | **yes** | Costs nothing to collect, ranks nothing; every later piece needs it (the proposal's step 1) |
| Plain counts per record (`reads`, distinct sessions, `used`) as a query (`cpd context reads`) | **yes**, a query only: an additive migration refuses a new view (`core/tests/migrations.rs:210`) | Answers "what is read" by a query; no decay, no weights |
| Utility `U(e)` with channel weights and a 30-day half-life | after data | The weights (0.2/1.0/2.0/+2.0) are untuned guesses; with no `pinned` channel and citations as the only `used` source the formula has nothing to fit |
| `score = rel * (1 + alpha*log(1+U)) * recency` | after data | `rel` needs FTS5 scores that are normalised to 0-1: section 1's backend is not built; alpha 0.1-0.3 can only be tuned against labelled queries |
| Exploration slots | no | Retrieval is manual: the Manager chooses, so no entry is starved by a ranker |
| Promote to pinned core | no | There is no pinned tier and none is needed (3) |
| Suggest for repo docs | **already built** | `doc_candidate` and the `git grep` of the id in each repo's docs (`distill.rs:421-423,624`) |
| Archive when superseded | **already built** | `supersedes` leaves force (`inforce.rs`); `record_retirements` for duplicate/documented with the User's approval |
| Suggest for `AGENTS.md` | no | `AGENTS.md` is the User's file; the proposal says never auto-written; the evidence it needs (reads across unrelated scopes) does not exist |
| Demote after 60 days / archive after 180 days unread | no | Needs a tier that does not exist; the whole store is 8 days old |

**Minimum data before ranking is worth building.** The proposal's promotion rule needs "at least 5 distinct sessions in 30 days" per record. cpd's store has 91 distinct writer sessions in 8 days (a floor for sessions that read; readers are not counted). Rough plan, an estimate not a measurement: 30 days of `context_reads` (so a full 30-day window exists) and at least 200 records with 3 or more distinct reading sessions, checked with the query above, before any weight is chosen. Until then the plain counts are the product. Before any ranking, build a labelled recall set (the proposal's "Validation") of about 30 real queries; tuning without it is guessing.

### The four open questions, for cpd

1. **How should `used` be detected?** Citation scan at write time (1b), with the two guards (id exists in the Context; the citing session read it), and nothing else. Reject the post-session model check for now: it needs a model per session and its only gain is to catch reflected content, not measurable here. Expected precision is not measured: label 50 citation rows by hand once the log has data.
2. **Who declares `scope`?** The writer, optional, via `--scope` on `decide`/`note` (2). Do not infer by model: cpd keeps the write path model-free (hs-d194, unverified). Build `--subject` first; `--scope` only with a search filter.
3. **One store per Context or shared?** One per Context, as built: `context_records` is keyed `(context, id)` and the id carries the machine (`pro-d12`), so a read row is `(context, record)` (1c). Sharing across Contexts is a different feature (it touches record ids and ranking across owners); no evidence of need: the 51 `hs-` citations (measured, 1a) are ids written on another machine, a federation concern, not a sharing one; whether any belongs to another Context is unverified.
4. **Retrieval budget per phase.** cpd's phases are grounding, plan, dispatch, deploy, not the proposal's Context/Plan/Dispatch. No budget is needed for push today (75 tokens). For pulled pages, the default `--limit 20` is already about 400-540 tokens (3). Recommendation: keep `--limit 20` and cap any future automatic block at one such page (about 500 tokens) per turn, with `--limit` for more; revisit with the logged read counts, not before.

## 3. Follow-up roadmap

Order rule: the proposal's own ("ship read logging first, ranking needs weeks of data"), adjusted by sections 1 and 2: first the present defect (a search that fails on any query holding `-`, `.`, `/`, `:` or `'`), then the item that starts collecting data, then the backend swap that also cheapens every later read. Nothing here changes a pushed message except `ctx-topics` and `ctx-shadow`. Migration facts that shape the items, read in source: an **additive** migration may create tables and non-unique indexes and may only `ADD COLUMN` to an existing table; it may not add a trigger or a unique index on an existing table, a view, or (by default-deny) a virtual table (`core/tests/migrations.rs:56-80,185-215`; `docs/research/migrations.md` for the file naming `<YYYYMMDDTHHMM>-<slug>.sql`, first line `-- cpd-migration: additive` or `breaking after <n>`, made by `scripts/new-migration.sh`). So FTS5 and `sqlite-vec` (virtual tables, triggers on `context_records`) are **breaking** migrations, and a CHECK list of a new table must be complete the day it is created: `context_reads` below carries the `shadow` channel for a later item because an existing table's SQL may not be rewritten. The SQL below is written from source and the design docs and was **not executed** (unverified until each item's red tests run it).

| # | Plan id | Intent | Schema | Depends on | One-way door |
|---|---|---|---|---|---|
| 1 | `ctx-quote` | `cpd context search` no longer fails on ids, paths, dotted keys, apostrophes | no schema change | none | no |
| 2 | `ctx-reads` | Log every read of a record (`show`, `search`, pushed updates) and count them | additive: `context_reads` | none | schema only (empty table) |
| 3 | `ctx-fts` | Search from an in-process FTS5 table in `cpd.db` instead of the Python bridge | breaking: `context_search` + 5 triggers | 1, 2 | yes (breaking barrier) |
| 4 | `ctx-topics` | A `Topics: name count, ...` line in the Manager's turn-start message | no schema change | 2 (to measure) | no |
| 5 | `ctx-used` | Set `used = 1` from id citations, guarded by a read in the same session | no schema change | 2 | no |
| 6 | `ctx-subject` | `--subject` on `decide`/`note`; same subject supersedes the holder | additive: `subject`, `scope` columns | none (5 is not required) | **yes** (record format) |
| 7 | `ctx-bridge-rm` | Delete the holographic bridge, its config key, its 15.7 MB of copies | no schema change | 3 live for 14 days | no (code); the derived DBs rebuild |
| 8 | `ctx-shadow` | Log what retrieval would push for a Manager turn, push nothing | no schema change (uses `shadow`) | 2, 3, 4, 5 and 14 days of data | no |
| 9 | `ctx-vec` | `sqlite-vec` + hosted embeddings behind FTS5, only if shadow shows a synonym gap | breaking: `context_embeddings`, `context_vec` | 3, 8, User decision 2 | **yes** (data egress, alpha extension) |

Not on the roadmap, on purpose (each is a gate, not a task): pinned core (section 2 part 3: `AGENTS.md` does it, adding one raises injected tokens); `entry_stats` rollup table (a `GROUP BY` is enough, part 1c); `contradicts` relation (recency wins, part 4); a model judge at write time (breaks "a write needs no model", part 4); the utility/ranking formula and promotion jobs (need the labelled set and 30 days of `context_reads`, part 5). Revisit them after item 8's data.

### Item 1: `ctx-quote`

- **Intent.** Fix the defect of section 1 (c): a sanitiser applied to the query before it reaches the backend. I also ran, single runs on `cpd` 0.17.80, `db.migrations` (`fts5: syntax error near "."`), `foo/bar` (near `/`), `a:b` (`no such column: a`) and `it's` (near `'`): all `usage`, so the fix covers more than the hyphen.
- **Schema.** no schema change.
- **CLI.** No new flag. Rule: split on whitespace; a token with a character outside `[A-Za-z0-9_*]` that is not already inside a `"..."` phrase becomes `"<token>"` (inner `"` doubled); `AND`, `OR`, `NOT`, `NEAR`, parentheses, quoted phrases and `term*` pass unchanged. An unterminated `"` still returns `usage` (`The search query is malformed`, the code `check_ok` already maps: `core/src/context/search.rs:56-68`). No new error code.
- **Files.** `core/src/context/search.rs` (new `fn sanitise(query: &str) -> String` called at `:125`, before `search_request` at `:38-40`), its unit tests, `cli/tests/flow/context_search.rs`.
- **Verification (red first).** Tests: `hyphenated_token_is_quoted`, `dotted_and_slashed_and_colon_tokens_are_quoted`, `apostrophe_token_is_quoted`, `quoted_phrase_or_and_prefix_pass_unchanged`, `unterminated_quote_stays_usage`, flow `search_by_record_id_exits_ok`. Acceptance: all seven commands of section 1 (c) plus the four above exit 0 (`ok: true`) on the shipped binary (method: one run each, `timeout 60`). `cpd context search hs-d655` returns `ok` with zero or more hits (the record is unreadable here, section 1).
- **Rollback.** `git revert`; no data touched. Two-way door.

### Item 2: `ctx-reads`

- **Intent.** The proposal's step 1: a read log, no behaviour change (section 2 part 1).
- **Schema** (`scripts/new-migration.sh context-reads additive`):

```sql
-- cpd-migration: additive
CREATE TABLE context_reads (
  context  TEXT NOT NULL REFERENCES contexts(id),
  record   TEXT NOT NULL,
  session  TEXT NOT NULL CHECK (length(session) >= 1),
  channel  TEXT NOT NULL CHECK (channel IN ('pushed', 'search', 'show', 'shadow')),
  read_at  TEXT NOT NULL,
  rank     INTEGER CHECK (rank IS NULL OR rank >= 1),
  score    REAL,
  used     INTEGER CHECK (used IN (0, 1)),
  PRIMARY KEY (context, record, session, channel),
  FOREIGN KEY (context, record) REFERENCES context_records(context, id)
) STRICT;
CREATE INDEX context_reads_record ON context_reads(context, record, read_at);
INSERT INTO history_tables (name, no_delete) VALUES ('context_reads', 0);
```

  Differences from section 2 part 1c: the index (the per-record count query), and `shadow` in the CHECK (item 8; not a read, so the counting query excludes it). `no_delete = 0` allows a later prune; how the runner treats the column is unverified (section 2).
- **CLI.** `cpd context show --record <id>` and `--topic <t>` log channel `show`; `cpd context search` logs `search` with `rank` (1-based) and `score`. `cpd context show --since <seq> --pushed` (new flag; requires `--since`, conflicts with `--record` and `--topic`, else `usage`) logs `pushed`; `pi/lib/updates.ts` passes it. New read command: `cpd context reads [--context <c>] [--since <iso>] [--channel <c,c>] [--min-sessions <n>]`, one line per record: reads, distinct sessions, used. An unknown channel is `usage`; an unknown Context is the existing Context-not-found refusal of `Select`. No `CPD_SESSION`: nothing is logged, no error. A logging failure never changes the exit code (the one place "fail loud" is relaxed); it is reported as an `error` Event (mechanism per section 2; kind name unverified).
- **Files.** `core/src/store/migrations/<ts>-context-reads.sql` (new), `core/src/context/reads.rs` (new), `core/src/context/mod.rs` (args, dispatch), `show.rs`, `search.rs`, `pi/lib/updates.ts:289-305` (the `--pushed` flag), `docs/commands.md`, `docs/contract.md` (schema section), the schema pin (`scripts/repin.sh`).
- **Verification (red first).** Tests: `a_read_is_one_row_per_record_session_channel`, `a_repeat_read_adds_no_row`, `a_read_without_a_session_logs_nothing`, `a_failed_log_does_not_fail_show`, `search_logs_rank_and_score`, `pushed_flag_requires_since`, `reads_counts_distinct_sessions_and_excludes_shadow`, plus the additive-migration check in `core/tests/migrations.rs`. Acceptance: logging adds at most 5 ms to the median of `cpd context show --record pro-d217` (baseline 18.8 ms, 15 runs, section 1 (d); method: same harness, 15 runs, median). Data gate for items 8 and 9, not a test: after 14 days `cpd context reads --min-sessions 3` lists at least 200 records (section 2 part 5's threshold; an estimate).
- **Rollback.** Revert the logging code; the table stays empty. The migration cannot be removed (a migration is never un-applied): one-way for the schema, harmless.

### Item 3: `ctx-fts`

- **Intent.** Section 1's recommendation: search in-process, find a record by its id, drop two Python spawns.
- **Schema** (`scripts/new-migration.sh context-search breaking`; `after <n>` is computed by the script, **80** migration files at this base: `ls core/src/store/migrations/*.sql | wc -l`; the design's 49 in `context-retrieval-design.md` is stale):

```sql
-- cpd-migration: breaking after <n>
CREATE VIRTUAL TABLE context_search USING fts5(
  id, text, topic, short,
  tokenize = 'porter unicode61', content = '', contentless_delete = 1
);  -- rowid = context_records.seq
INSERT INTO context_search(rowid, id, text, topic, short)
  SELECT r.seq, r.id, r.text, k.topic, k.short
    FROM context_records r
    LEFT JOIN record_curations k ON k.context = r.context AND k.record = r.id;
CREATE TRIGGER context_search_ai AFTER INSERT ON context_records BEGIN
  INSERT INTO context_search(rowid, id, text) VALUES (new.seq, new.id, new.text);
END;
CREATE TRIGGER context_search_ad AFTER DELETE ON context_records BEGIN
  DELETE FROM context_search WHERE rowid = old.seq;
END;
CREATE TRIGGER context_search_cur_ai AFTER INSERT ON record_curations BEGIN
  DELETE FROM context_search WHERE rowid = (SELECT seq FROM context_records WHERE context = new.context AND id = new.record);
  INSERT INTO context_search(rowid, id, text, topic, short)
    SELECT seq, id, text, new.topic, new.short FROM context_records WHERE context = new.context AND id = new.record;
END;
CREATE TRIGGER context_search_cur_au AFTER UPDATE ON record_curations BEGIN
  DELETE FROM context_search WHERE rowid = (SELECT seq FROM context_records WHERE context = new.context AND id = new.record);
  INSERT INTO context_search(rowid, id, text, topic, short)
    SELECT seq, id, text, new.topic, new.short FROM context_records WHERE context = new.context AND id = new.record;
END;
CREATE TRIGGER context_search_cur_ad AFTER DELETE ON record_curations BEGIN
  DELETE FROM context_search WHERE rowid = (SELECT seq FROM context_records WHERE context = old.context AND id = old.record);
  INSERT INTO context_search(rowid, id, text)
    SELECT seq, id, text FROM context_records WHERE context = old.context AND id = old.record;
END;
```

  Query: `SELECT r.id, bm25(context_search) FROM context_search s JOIN context_records r ON r.seq = s.rowid WHERE context_search MATCH ?1 AND r.context = ?2 ORDER BY bm25(context_search) LIMIT ?3`, then the existing in-force, kind and `--all` filters. The triggers are on existing tables, which is why the migration is breaking.
- **CLI.** No new flag. `cpd context reindex` keeps its flags and now rebuilds `context_search` for the Context (so the 5-trigger design has a repair). `--limit`, `--kind`, `--all` unchanged. `engine_missing` and `engine_failed` are no longer raised by `search`. The queries of section 1 (c) still work, through item 1's sanitiser.
- **Files.** `core/src/store/migrations/<ts>-context-search.sql` (new), `core/src/context/search.rs:125-180` (replace `bridge::run`, keep the bridge code compiled and unused until item 7), `core/src/context/reindex.rs:36`, `docs/contract.md:2434` ("The holographic engine rail"), schema pin, `core/tests/migrations.rs`.
- **Verification (red first).** Tests: `a_record_is_found_by_its_own_id`, `a_new_record_is_searchable_in_the_same_transaction`, `a_curation_updates_topic_and_short_in_the_index`, `a_deleted_context_leaves_no_index_row`, `search_filters_by_context`, `porter_stems_decide_and_decision` (each a read of `context_search` after the real triggers fire), `reindex_rebuilds_a_dropped_row`, `breaking_migration_backfills_existing_rows`. Acceptance: median wall time of `cpd context search 'retriev*' --limit 20` at most 60 ms on the 656-record Context (15 runs, `subprocess.run`, same harness as section 1 (d); baseline 271.1 ms; the design predicts 20-25 ms, unverified); recall: for each of the 4 queries of section 2 part 3, the native top 20 contains at least 18 of the 20 records the bridge returned (method: one run each, set intersection), and `cpd context search '"pro-d217"'` returns `pro-d217` (today: only `pro-d219`).
- **Rollback.** Revert the search code (the bridge is still in the tree until item 7); the table and triggers stay and cost an insert per write. A breaking migration is a barrier that closes the additive window: one-way door.

### Item 4: `ctx-topics`

- **Intent.** The proposal's "small pinned index (topics with counts)" for discovery, section 2 part 3.
- **Schema.** no schema change.
- **CLI.** New `cpd context show --topics` (conflicts with `--since`, `--record`, `--topic`: `usage`) prints `Topics: adapter 12, attempt 9, ...` for in-force curated records, most records first. `pi/lib/updates.ts` adds that line to the Manager's turn-start message only (not Lead, not Worker). A Context with no curation prints `Topics: none`.
- **Files.** `core/src/context/curated.rs` (new `topic_counts` beside `topics` at `:55-58`), `core/src/context/show.rs`, `core/src/context/mod.rs` (`ShowArgs`), `pi/lib/updates.ts:289-305`, `docs/commands.md`.
- **Verification (red first).** Tests: `topics_line_counts_in_force_curated_records`, `a_retired_record_is_not_counted`, `topics_conflicts_with_record`, a `pi` test `manager_update_carries_the_topics_line`, and `lead_update_has_no_topics_line`. Acceptance: the line is at most 300 bytes on the 20-topic store (section 2 part 3 estimates 150-250; measure once, `wc -c`). Effect, not a test: the share of Manager sessions with a `search` or `show` row in `context_reads` in the 14 days after, against the 14 days before (needs item 2). A rise is the evidence; no rise means remove it.
- **Rollback.** Remove the line in `updates.ts`. Two-way door.

### Item 5: `ctx-used`

- **Intent.** Section 2 part 1b: `used = 1` from a citation, at write time.
- **Schema.** no schema change (`context_reads.used` exists from item 2).
- **CLI.** No flag. In the write path of `decide`, `note`, `ask`, `answer`, `plan new --brief`, `plan outline`, `job add --spec`, `session send`: scan the text with `\b([a-z][a-z0-9]*)-([dnqapb])([0-9]+)\b`; set `used = 1` on the row `(context, record, CPD_SESSION, any channel)` of each match only if `RecordId::parse` accepts it (`core/src/ids.rs:335-351`), the record exists in the Context, the session has a read row for it, it is not the record's own id and not its `supersedes`. No match, no write, no error.
- **Files.** `core/src/context/contexts.rs:167,232-275`, `core/src/context/reads.rs`, `core/src/plan/new.rs`, `core/src/plan/outline.rs`, `core/src/job/` (the `job add` write; path unverified), `core/src/session/send.rs:22-29`.
- **Verification (red first).** Tests: `a_cited_read_record_is_marked_used`, `a_cited_unread_record_is_not_marked`, `a_supersedes_target_is_not_a_use`, `an_attempt_id_is_not_a_record`, `a_foreign_machine_id_is_ignored`, `a_note_without_session_marks_nothing`. Acceptance: precision of `used = 1` at least 90% on 50 rows drawn at random from `context_reads` once item 2 has data, labelled by hand ("did the citing text rely on the record?"); an unlabelled run proves nothing. Write overhead at most 5 ms median over 15 `cpd context note` runs in a scratch Context.
- **Rollback.** Revert the scan; `used` stays what was set. Two-way door.

### Item 6: `ctx-subject`

- **Intent.** Section 2 part 2: a named key that finds the record to supersede, with no model.
- **Schema** (additive: `ADD COLUMN` nullable, a non-unique partial index on an existing table is allowed by `core/tests/migrations.rs:204-209`; a `CHECK` on an added column on a table with the `context_records_immutable` trigger is unverified):

```sql
-- cpd-migration: additive
ALTER TABLE context_records ADD COLUMN subject TEXT
  CHECK (subject IS NULL OR (subject GLOB '[a-z]*' AND length(subject) BETWEEN 1 AND 80));
ALTER TABLE context_records ADD COLUMN scope TEXT
  CHECK (scope IS NULL OR (json_valid(scope) AND json_type(scope) = 'array'));
CREATE INDEX context_records_subject ON context_records(context, subject) WHERE subject IS NOT NULL;
```

  Note the parentheses: section 2's draft CHECK for `subject` mixes `OR` and `AND` without them; `AND` binds tighter, so it was equivalent, but the explicit form is the one to write.
- **CLI.** `cpd context decide <text> --subject <key>` and `cpd context note <text> --subject <key>` (not `ask`, `answer`, `plan`, `brief`). A key that fails the CHECK is `usage`. Exactly one in-force holder (after retirements, `in_force_without`): its id becomes `supersedes`. None: nothing. More than one: refused with **new** code `subject_ambiguous` (class `Refused`, `core/src/error.rs:174` area; `details.records` lists the ids), the writer picks with `--supersedes`. A holder whose kind may not be superseded by the writer's kind: `supersedes_invalid` (existing). `--supersedes` and `--subject` together: `supersedes` wins, the subject is stored. `--scope` is **not** built here (YAGNI, section 2 part 2); the column is added now because an `ADD COLUMN` later costs another migration and an unknown JSONL field is the expensive part (below).
- **Files.** `core/src/store/migrations/<ts>-record-subject.sql` (new), `core/src/context/record.rs:13,154` (`VERSION`, optional `subject`/`scope` in `Raw` and `Record::to_json`), `core/src/context/contexts.rs:167,232-275` (the lookup in the write transaction), `core/src/context/mod.rs:209-226` (`DecideArgs`, `NoteArgs`), `core/src/context/inforce.rs`, `core/src/error.rs`, `docs/contract.md`, `docs/commands.md`.
- **One-way door, with a cost.** `Record::parse_line` has `deny_unknown_fields` (`record.rs:154`): a binary without this change fails on a git-store line that carries `subject`. The Context `cpd` is written from at least two machines (`pro-*` and `hs-*` ids, section 2 part 1a), so `hs` must run the new binary before the first record with a subject is written. Mitigation, not a guess: omit the field from the line when it is `None`, so records without a subject stay readable by old binaries; and a `cpd` that would write a subject runs `cpd --version`'s store check first (existing). Ask the User (decision 3).
- **Verification (red first).** Tests: `a_subject_supersedes_the_in_force_holder`, `two_holders_refuse_with_subject_ambiguous_and_list_ids`, `a_retired_holder_does_not_hold_the_subject`, `supersedes_wins_over_subject`, `a_bad_subject_is_usage`, `a_record_without_subject_serialises_as_before`, `a_record_with_subject_roundtrips_the_jsonl`, `an_old_line_parses`, `subject_on_a_question_is_refused_by_the_type` (a compile-level test: no variant carries it). Acceptance: median of `cpd context decide --subject` at most 5 ms above the same command without it (15 runs, scratch Context). No backfill of the 656 existing records: they stay `NULL` (the proposal's model pass is not recommended; a wrong subject supersedes a true record).
- **Rollback.** Stop passing `--subject`; the old rows are untouched (append-only), and a wrong supersession is undone by a new record with the same subject (recency wins). The column and the JSONL field stay forever: one-way door.

### Item 7: `ctx-bridge-rm`

- **Intent.** Delete first, after the replacement is proven (section 1 recommendation 1).
- **Schema.** no schema change.
- **CLI.** `cpd context reindex` keeps its flags (it now rebuilds `context_search`). The config key `context_engine_enabled` and the engine-dir setting go away; whether a config file that still holds them must keep loading is unverified, so a test decides (`an_old_config_with_engine_keys_loads`); if the reader rejects unknown keys the keys stay parsed and ignored for one release.
- **Files.** `contrib/holographic/` (delete), `core/src/context/engine.rs`, `core/src/context/search.rs:133-180`, `core/src/config/file.rs:60,312,407,464-466`, `docs/contract.md:2434`, `docs/research/context-holographic.md` (mark superseded). Outside the repo and unverified: the dotfiles install of the Python venv.
- **Dependency.** Item 3 live 14 days with no revert and item 1 shipped.
- **Verification.** Tests: `search_spawns_no_process`, `an_old_config_with_engine_keys_loads`. Acceptance: `grep -rn "holographic" core cli engine pi | wc -l` prints 0 (docs excluded); the engine directory's 15.7 MB of copies (section 1 (e)) can be removed with no command depending on them.
- **Rollback.** `git revert`. The derived engine DBs rebuild with the old code's `reset`. Two-way door for code; the dotfiles part is not in this repo.

### Item 8: `ctx-shadow`

- **Intent.** Section 2 part 3: run retrieval for each Manager turn and log what it would have pushed, push nothing; the proposal's "shadow mode".
- **Schema.** no schema change (channel `shadow` is already in item 2's CHECK; `used` filled by item 5).
- **CLI.** `cpd context search <query> --shadow`: logs the results as channel `shadow` (with `rank`, `score`) and prints only `{"shadow":<count>}`; conflicts with `--all` (`usage`). `pi/lib/updates.ts` builds the query from the latest message plus the pending plans text and calls it once per Manager turn.
- **Files.** `core/src/context/search.rs`, `core/src/context/mod.rs` (`SearchArgs`), `core/src/context/reads.rs`, `pi/lib/updates.ts`.
- **Verification (red first).** Tests: `shadow_logs_rows_and_prints_only_a_count`, `shadow_rows_are_not_counted_as_reads`, `shadow_conflicts_with_all`, a `pi` test `manager_turn_calls_shadow_search_once`. Acceptance: recall@10 on a hand-labelled set of 30 real Manager turns, where the target records are those the Manager cited in its next 3 turns (labelled by hand, set kept in `docs/research/`), compared with the recency baseline (the last 10 records by `seq`); proposed threshold, to be set by the User: retrieval recall@10 at least 0.7 and at least 2x the baseline. Cost: the query adds at most 60 ms per turn (item 3's budget).
- **Rollback.** Stop calling `--shadow` from `updates.ts`; rows stay. Two-way door.

### Item 9: `ctx-vec`

- **Intent.** The synonym gain, only if item 8 shows FTS5 misses what a Manager then cited (at least 20% of cited records absent from the shadow top 20: proposed gate).
- **Schema** (`breaking after <n>`; the `vec0` syntax is from the crate's documented usage and is **unverified**, not read in source here):

```sql
-- cpd-migration: breaking after <n>
CREATE TABLE context_embeddings (
  context     TEXT NOT NULL REFERENCES contexts(id),
  record      TEXT NOT NULL,
  model       TEXT NOT NULL CHECK (length(model) >= 1),
  dims        INTEGER NOT NULL CHECK (dims = 768),
  embedded_at TEXT NOT NULL,
  PRIMARY KEY (context, record, model),
  FOREIGN KEY (context, record) REFERENCES context_records(context, id)
) STRICT;
CREATE VIRTUAL TABLE context_vec USING vec0(embedding float[768]);  -- rowid = context_records.seq
```

- **CLI.** `cpd context reindex --embeddings` embeds records without a row for the configured model (768-d `gemini-embedding-2` through Bifrost, `pro-n176`); refuses with a **new** code `embedder_failed` (class `Failure`) when the provider errors. `cpd context search` merges FTS5 and vector results by reciprocal-rank fusion and **degrades to FTS5 alone**, with no error, when the embedder fails; an embedder change (`model` differs) leaves old rows unused until reindexed.
- **Files.** New migration, `core/Cargo.toml` (pin `sqlite-vec = "=0.1.10-alpha.4"`; `sqlite3_auto_extension` before the first Connection, `core/src/store/mod.rs`), `core/src/context/search.rs`, `core/src/context/reindex.rs`, a new `core/src/context/embed.rs`, `core/src/error.rs`, release targets in `scripts/` (build on each: unverified).
- **Dependency.** 3, 8, and the User's answer on sending record text to a hosted provider (decision 2, `pro-q1`).
- **Verification (red first).** Tests (embedder mocked, as the policy requires): `search_degrades_to_fts_when_the_embedder_fails`, `a_changed_model_invalidates_vectors`, `rrf_orders_a_synonym_hit_above_no_hit`, `reindex_embeddings_is_idempotent`, `embedder_failure_is_embedder_failed`. Acceptance: on item 8's labelled set, recall@10 improves over FTS5 alone by at least 10 points (proposed); p50 of `cpd context search` at most 600 ms with the hosted call (p50 405 ms per call, `pro-n174`, plus item 3's budget); DB growth at 656 records is about 2 MB (656 x 768 x 4 bytes, arithmetic).
- **Rollback.** Drop use of `context_vec` in `search.rs`; FTS5 stays, vectors are derived. One-way door: a breaking migration, an alpha C extension in the build, and record text already sent to the provider.

### Decisions needed from the User

1. Reverse `context-holographic.md` and replace the Python bridge by in-process FTS5 (items 3 and 7)? Yes unblocks item 3; no leaves the 270 ms spawns and the 15.7 MB copies.
2. May the full text of records go to a hosted embedding provider through Bifrost (`pro-q1`, item 9)?
3. May `ctx-subject` change the git record format (an optional `subject` field), given that every machine writing Context `cpd` must upgrade first?
4. Confirm the two rejections of section 2: no pinned core (`AGENTS.md` stays the core) and no `contradicts` relation (recency wins)?
5. Make `hs-d655`, `hs-d675` and `hs-d194` readable on this machine, so the three decisions the docs rely on can be checked against the records (today unverified)?

### Records to write

- note: rfm-memory-retrieval s1: cpd context search fails on ids, dotted, slashed and apostrophe queries (usage, FTS5 syntax); cause is no sanitiser at core/src/context/search.rs:125.
- note: rfm-memory-retrieval s1: holographic vectors are SHA-256 bag-of-words (delete/remove cosine 0.028) and FTS5 gates stage 1, so they add no semantics; search median is 271 ms (2 Python spawns).
- decide: replace the holographic bridge with in-process FTS5 in cpd.db (breaking migration, id column, porter); delete the bridge after 14 days. Needs the User (reverses context-holographic.md).
- note: rfm-memory-retrieval s2: no read is logged anywhere; 449 of 533 id-shaped tokens in 656 records resolve to a record, so citations can supply used=1 once reads are logged.
- decide: add table context_reads (additive, channels pushed/search/show/shadow, one row per record+session+channel) before any ranking; ranking waits for 14-30 days of data and a labelled set.
- decide: no pinned core and no contradicts relation: AGENTS.md is the core and recency wins; cpd already pushes about 75 tokens per Manager first message. Awaiting the User's confirmation.
- decide: subject key is sugar over supersedes: --subject on decide/note, one in-force holder supersedes, several refuse with subject_ambiguous; no model, no backfill of old records.
- note: rfm-memory-retrieval: additive migrations refuse views, triggers on existing tables and virtual tables (core/tests/migrations.rs:56-80,185-215); FTS5 and sqlite-vec need breaking migrations.
- note: rfm-memory-retrieval: Record::parse_line has deny_unknown_fields (record.rs:154), so a subject field breaks old binaries on other machines (hs); upgrade all before writing one.
- ask: may record text go to a hosted embedding provider through Bifrost for sqlite-vec (pro-q1)? Gates the ctx-vec item of the rfm-memory-retrieval roadmap.
- ask: hs-d655, hs-d675 and hs-d194 are not readable in this store (record_not_found); can they be synced so the docs that cite them can be checked?
- decide: roadmap order of rfm-memory-retrieval: ctx-quote, ctx-reads, ctx-fts, ctx-topics, ctx-used, ctx-subject, ctx-bridge-rm, ctx-shadow, ctx-vec; each a small Plan with red tests first.
