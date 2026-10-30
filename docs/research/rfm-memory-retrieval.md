# Context memory retrieval: findings (Plan `rfm-memory-retrieval`)

Research only; no production code. Every number carries its method; anything not measured or read in source is labelled `unverified` in the same sentence. Sources: origin/main at `77b80a89` (fetched 2026-10-30); the holographic package read from `~/.local/share/cpd/holographic/venv/lib/python3.12/site-packages/holographic_memory/` (cited below as `hm/<file>:line`); measurements on 2026-10-30 EDT on the machine `pro` against its live store (Context `cpd`, 656 records, `cpd` 0.17.80-77b80a89).

## 1. Holographic vs native FTS5 + sqlite-vec

### Status today

- The holographic bridge is **current and the only search path**. `cpd context search` goes through `contrib/holographic/bridge.py` (`core/src/context/search.rs:133-141,167`; `core/src/context/engine.rs:168,174`), and `docs/contract.md:2434` ("The holographic engine rail", hs-d675, hs-d677) still specifies it. `git log origin/main` shows adoption (`cb06bc9a` "Search the Context through the holographic engine", `7f95d19b` design) and no commit that retires it.
- **The native FTS5 path is not shipped.** `docs/research/context-holographic.md:7-11` says the planned `context_search` FTS5 table and its triggers "are **not** built", superseding `docs/research/context-retrieval-design.md`. No `context_search` table exists in `core/src`, and no migration creates one (`grep -rn context_search core/src cli/src engine/src` returns nothing; `cli/tests/flow/main.rs:24` only names a test module). So "in-process FTS5" below is the design in `context-retrieval-design.md`, with its measurements cited, not a shipped command.
- The decisions hs-d655 (retrieve on demand), hs-d675 (adopt a local engine) and hs-d194 (cpd must not require a model) are cited by the docs (`context-holographic.md:7-9`, `context-memory.md:207`) but are **not in this machine's store** (`cpd context show --record hs-d655` returns `record_not_found` in Context `cpd`; ids here are `pro-*`). I read them only through the docs: unverified against the records themselves.
- A **newer** decision overrules the "no model" line of `context-memory.md:207`: Notes `pro-n174` and `pro-n176` (Plan `rfm-curation-models`, 2026-10-30 02:20 UTC) set the v1 stack as hosted embeddings through Bifrost (`gemini-embedding-2`, 768-d) plus FTS5, no local model. Neither says the bridge is retired.

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
| 1 | Read tracking: `entry_reads`, `entry_stats`, channels, `used`, dedupe, never-block | **missing** | No read row anywhere (above). Citations are already in the text: 533 id-shaped tokens in 656 records, 449 of them (84%) resolve to a record of this store (see 1a) | Table `context_reads` + `core/src/context/reads.rs`; log from `show.rs` and `search.rs`; flag-free; `used` from a citation scan at write time (SQL in 1b) |
| 2 | Deterministic `subject` + `scope` tags | **partly built**: explicit supersession is built, the key is not | `--supersedes <record>` on `decide`/`note` (`core/src/context/mod.rs:209-226`), validated at write (`core/src/context/contexts.rs:253-275`, `supersedes_invalid`, `record_not_found`), applied by `in_force` (`inforce.rs:35-47`). 30 of 438 decisions/notes use it | `--subject <key>` and `--scope <path>...` on `decide`/`note`; the write resolves a subject to a `supersedes` (SQL in 2) |
| 3 | Pinned core + per-task / per-turn retrieval | **partly built; pinned core not needed**; relevance retrieval is manual | Pushed text today is about 75 tokens (Manager) or the brief (Lead); the always-injected core is `AGENTS.md` (`context-memory.md:226-229`). Recency pushes exist (`updates.ts:35,263,289-305`). Retrieval is `cpd context search`, 264-271 ms median (section 1 (d)), 20 shorts = 402-541 tokens (4 queries, one run each, below) | A topics-with-counts line in the Manager's turn-start message; a `--scope` filter on `search`; retrieval hook only after reads are logged (see 3) |
| 4 | Write-time pairwise curation | **partly built** (curation and retire proposals exist; the neighbour window and a `contradicts` verdict do not) | `distill.rs:125-129` batch of at most 60 records / 48 KiB; `known.md` holds every curated record's short (`distill.rs:442-451`), so a batch reads 36 KB / about 9.1k tokens of context records at 395 records; reasons `duplicate`/`superseded`/`documented` (`curation.sql:12`) | Cut `known.md` to the nearest neighbours of the batch from the FTS5 index (section 1's recommendation); no schema delta; keep it asynchronous |
| 5 | Ranking formula, promotion/demotion table | **missing; build only the log now**, ranking after data | No reads, no pinned tier, no `used`. The store is 8 days old; the proposal itself waits 2-3 weeks (`Rollout` step 6). Part of the table is built without reads: "suggest for repo docs" is `doc_candidate` + `git grep` of the id in docs (`distill.rs:421-423,624`) and "archive when superseded" is `record_retirements` | A read-count SQL view over `context_reads` (see 5); no decay, no `alpha`, no promotion job yet |

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
| Plain counts per record (`reads`, distinct sessions, `used`) as a SQL view | **yes**, a view only | Answers "what is read" by a query; no decay, no weights |
| Utility `U(e)` with channel weights and a 30-day half-life | after data | The weights (0.2/1.0/2.0/+2.0) are untuned guesses; with no `pinned` channel and citations as the only `used` source the formula has nothing to fit |
| `score = rel * (1 + alpha*log(1+U)) * recency` | after data | `rel` needs FTS5 scores that are normalised to 0-1: section 1's backend is not built; alpha 0.1-0.3 can only be tuned against labelled queries |
| Exploration slots | no | Retrieval is manual: the Manager chooses, so no entry is starved by a ranker |
| Promote to pinned core | no | There is no pinned tier and none is needed (3) |
| Suggest for repo docs | **already built** | `doc_candidate` and the `git grep` of the id in each repo's docs (`distill.rs:421-423,624`) |
| Archive when superseded | **already built** | `supersedes` leaves force (`inforce.rs`); `record_retirements` for duplicate/documented with the User's approval |
| Suggest for `AGENTS.md` | no | `AGENTS.md` is the User's file; the proposal says never auto-written; the evidence it needs (reads across unrelated scopes) does not exist |
| Demote after 60 days / archive after 180 days unread | no | Needs a tier that does not exist; the whole store is 8 days old |

**Minimum data before ranking is worth building.** The proposal's promotion rule needs "at least 5 distinct sessions in 30 days" per record. cpd's store has 91 distinct writer sessions in 8 days (a floor for sessions that read; readers are not counted). Rough plan, an estimate not a measurement: 30 days of `context_reads` (so a full 30-day window exists) and at least 200 records with 3 or more distinct reading sessions, checked with the view above, before any weight is chosen. Until then the plain counts are the product. Before any ranking, build a labelled recall set (the proposal's "Validation") of about 30 real queries; tuning without it is guessing.

### The four open questions, for cpd

1. **How should `used` be detected?** Citation scan at write time (1b), with the two guards (id exists in the Context; the citing session read it), and nothing else. Reject the post-session model check for now: it needs a model per session and its only gain is to catch reflected content, not measurable here. Expected precision is not measured: label 50 citation rows by hand once the log has data.
2. **Who declares `scope`?** The writer, optional, via `--scope` on `decide`/`note` (2). Do not infer by model: cpd keeps the write path model-free (hs-d194, unverified). Build `--subject` first; `--scope` only with a search filter.
3. **One store per Context or shared?** One per Context, as built: `context_records` is keyed `(context, id)` and the id carries the machine (`pro-d12`), so a read row is `(context, record)` (1c). Sharing across Contexts is a different feature (it touches record ids and ranking across owners); no evidence of need: the 51 `hs-` citations (measured, 1a) are ids written on another machine, a federation concern, not a sharing one; whether any belongs to another Context is unverified.
4. **Retrieval budget per phase.** cpd's phases are grounding, plan, dispatch, deploy, not the proposal's Context/Plan/Dispatch. No budget is needed for push today (75 tokens). For pulled pages, the default `--limit 20` is already about 400-540 tokens (3). Recommendation: keep `--limit 20` and cap any future automatic block at one such page (about 500 tokens) per turn, with `--limit` for more; revisit with the logged read counts, not before.
