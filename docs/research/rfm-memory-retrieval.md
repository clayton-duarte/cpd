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
