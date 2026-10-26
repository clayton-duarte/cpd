# Agent memory: prior art and on-demand retrieval for cpd

Research for Job `memory-docs`, Plan `context-on-demand`. It answers hs-d655 (the Context
is retrieved on demand, not pushed; the digest is dropped) and the User's follow-up (works
like the harness concept of agent memory; can cpd adopt an existing solution). Findings
only: this file proposes no code change.

## Method

- **Question.** How do coding harnesses and memory libraries write, store and retrieve
  long-term agent memory, and what should cpd do to retrieve its own Context records on
  demand, under cpd's constraints (machine-local SQLite, no LLM API, one Rust binary,
  ~1000-1500 records, fast; hs-d655, hs-n480).
- **Sources.** `curl`/`wget` under `timeout`, preferring a project's own README, docs and
  repository over blog posts. Every source was read on **2026-10-25**; each Prior art row
  carries its source URL, and the column head says the read date. Claims I could not verify
  from a source are labelled `unverified`; claims derived from evidence but not directly
  stated are labelled `inferred`, in the same sentence.
- **Activity.** Stars, license and `pushed_at` come from the GitHub REST API
  (`api.github.com/repos/<owner>/<repo>`, read 2026-10-25) unless a row says otherwise.
  Harness rows are proprietary products with no repository, so their "activity" is the docs
  read date.
- **cpd measurement (only cpd's own SQLite, real data, `timeout`).** The live store
  `$CPD_HOME/cpd.db` (1,446 `context_records`, 1,002 `record_curations`; Context
  `building-cpd` has 1,345 of them) was read read-only. `sqlite3` is CLI 3.45.1; cpd bundles
  SQLite 3.53.2 (`libsqlite3-sys`), so the figures are the CLI's, not the binary's, and the
  bundled build's query plans are `unverified` (this matches `docs/research/quality-sql-perf.md`).
  FTS5 is compiled into cpd's bundled SQLite (verified 2026-10-25: 289 `fts5*` symbols in
  `target/debug/build/libsqlite3-sys-*/out/libsqlite3.a`).

  | Measurement (1,446 rows, real `context_records.text`) | Result |
  |---|---|
  | `text LIKE '%decision%'`, whole CLI run including open | 5 ms |
  | `text LIKE '%engine%'` | 1 ms |
  | `lower(text) LIKE '%retriev%'` | 2 ms |
  | Build FTS5 (`porter unicode61`) over the same rows, in-memory | 21 ms |
  | `fts MATCH 'decision'` / `'retriev*'` (index built) | < 1 ms |

  Both are trivial at this size; the choice below is about ranking and scaling, not raw
  speed. No other system was benchmarked.

### Sources (all read 2026-10-25)

- Claude Code: <https://docs.claude.com/en/docs/claude-code/memory>
- Cursor: <https://cursor.com/docs/rules.md>, <https://cursor.com/llms.txt>
- Codex: <https://developers.openai.com/codex/agent-configuration/agents-md>,
  <https://developers.openai.com/codex/dots/tasks-and-memory>
- OpenCode: <https://opencode.ai/docs/rules/>,
  <https://raw.githubusercontent.com/sst/opencode/dev/LICENSE>
- mem0 <https://github.com/mem0ai/mem0>; Letta <https://github.com/letta-ai/letta-code>,
  <https://github.com/letta-ai/letta>; Graphiti <https://github.com/getzep/graphiti>;
  LangMem <https://github.com/langchain-ai/langmem>;
  Basic Memory <https://github.com/basicmachines-co/basic-memory>;
  OpenMemory <https://github.com/mem0ai/openmemory>, <https://docs.mem0.ai/llms.txt>;
  sqlite-vec <https://github.com/asg017/sqlite-vec>; FTS5 <https://sqlite.org/fts5.html>
- OpenViking <https://github.com/volcengine/OpenViking>;
  honcho <https://github.com/plastic-labs/honcho>;
  hindsight <https://github.com/vectorize-io/hindsight>;
  holographic <https://github.com/bysc1000/holographic-memory>;
  retaindb <https://github.com/RetainDB/RetainDB>;
  byterover <https://github.com/campfirein/byterover-cli>;
  supermemory <https://github.com/supermemoryai/supermemory>;
  memori <https://github.com/MemoriLabs/Memori>

## Prior art

Every system the User named is here. "Retrieval" names the mechanism (keyword, FTS, graph,
vector, or a mix). "Local" means it can run entirely on one machine. "Needs LLM/embed"
says whether a model is required for the memory to work. "License / activity" is the
project license and last `pushed_at` (or the docs read date for proprietary harnesses).
The User's list spelled one name loosely: `ovenviking` is **OpenViking** (verified: the
search result is <https://github.com/volcengine/OpenViking>); `holographic` is read as the
Hermes Holographic Memory provider (the closest match; other projects use the word, see
Open questions).

| System | Write path | Storage | Retrieval | Local / self-hosted | External dependencies | Needs LLM/embed | License / activity | Source (read 2026-10-25) |
|---|---|---|---|---|---|---|---|---|
| **Claude Code** (harness) | User writes `CLAUDE.md`; Claude itself writes auto-memory notes (`MEMORY.md` index + one topic file per memory) | Markdown files, `~/.claude/projects/<project>/memory/`; no DB | Automatic: `CLAUDE.md` and the first 200 lines / 25 KB of `MEMORY.md` loaded every session; topic files read on demand with file tools | Machine-local files; no server | None (the host model) | Model only; no embedding model for memory | Proprietary; docs read 2026-10-25 | [docs.claude.com](https://docs.claude.com/en/docs/claude-code/memory) |
| **Cursor** (harness) | User/team writes Rules (`.cursor/rules/*.mdc`) or `AGENTS.md`; `/create-rule`; Team Rules from the dashboard | Files in the repo; Team Rules hosted | Automatic at prompt build: `alwaysApply` in every session, glob auto-attach, agent-selected by description, or manual `@`-mention; `@` mentions for ad-hoc context. No auto-memory page in the current docs | Local client + files; Team Rules hosted | None beyond the model | Model only | Proprietary; docs read 2026-10-25 | [cursor.com/docs/rules.md](https://cursor.com/docs/rules.md), [llms.txt](https://cursor.com/llms.txt) |
| **OpenAI Codex** (harness) | User writes `AGENTS.md` / `AGENTS.override.md` (global `~/.codex` plus the project directory chain) | Files | Automatic: an instruction chain built once per run, ≤32 KiB by default; no persistent memory store for the coding CLI (`dots` memory is a different ChatGPT product) | CLI on the machine; cloud tasks hosted | None beyond the model | Model only | Proprietary; docs read 2026-10-25 | [agents-md](https://developers.openai.com/codex/agent-configuration/agents-md), [tasks-and-memory](https://developers.openai.com/codex/dots/tasks-and-memory) |
| **OpenCode** (harness) | User writes `AGENTS.md` (project/global), `/init`; `CLAUDE.md` fallback; skills and plugins | Files | Automatic: `AGENTS.md` included in the LLM context; skills loaded on demand | Yes | None beyond the model | Model only | MIT; docs read 2026-10-25 | [opencode.ai/docs/rules](https://opencode.ai/docs/rules/), [LICENSE](https://raw.githubusercontent.com/sst/opencode/dev/LICENSE) |
| **mem0** (OSS lib) | `memory.add(...)`; an LLM extracts facts (v3 is ADD-only, no update/delete) | Vector store (default Qdrant) + SQLite history; self-host server uses Postgres/pgvector | Hybrid: semantic vectors + BM25 keyword + entity matching, fused | Yes: library, or Docker self-host | Vector DB, LLM provider, embedder | Yes, both (default `gpt-5-mini`, `text-embedding-3-small`; local Ollama is an option) | Apache-2.0; pushed 2026-10-23; 67,790★ | [github.com/mem0ai/mem0](https://github.com/mem0ai/mem0) |
| **Letta / MemGPT** (OSS) | The agent edits its own memory blocks (system-prompt learning) and writes archival memory | Server DB (Postgres/SQLite); letta-code adds git-tracked MemFS and a Letta Cloud option | Memory blocks in context, plus archival/message search (vector) | Yes, self-host; Cloud is the letta-code default | Server runtime, DB, LLM provider | Yes | Apache-2.0; letta-code pushed 2026-10-25 (3,709★), letta 2026-10-18 (25,253★) | [letta-code](https://github.com/letta-ai/letta-code), [letta](https://github.com/letta-ai/letta) |
| **Zep / Graphiti** (OSS + hosted) | Episodes ingested; an LLM extracts entities and relationships into a bi-temporal graph | Graph DB (Neo4j / FalkorDB / Neptune; Kuzu deprecated) + full-text backend; Zep uses its hosted `Konig` | Hybrid: semantic embedding + BM25 keyword + graph traversal | Graphiti self-host (needs the graph DB); Zep is a hosted service | Graph DB + LLM + embedder | Yes | Apache-2.0; pushed 2026-10-25; 32,204★ | [github.com/getzep/graphiti](https://github.com/getzep/graphiti) |
| **LangMem** (OSS) | Memory tools (`create_manage_memory_tool`) in the hot path, or a background manager that extracts/consolidates | Any store; native LangGraph `BaseStore` (in-memory or Postgres) | `create_search_memory_tool` (semantic search over the store's embedding index) | Yes, library | LangGraph + store + LLM + embedder | Yes | MIT; pushed 2026-10-02; 1,722★ | [github.com/langchain-ai/langmem](https://github.com/langchain-ai/langmem) |
| **Basic Memory** (OSS) | AI/user writes Markdown notes (wikilinks, frontmatter) through MCP tools; sync indexes them | Markdown files + a SQLite index | Full-text + semantic vector search (FastEmbed), optional cross-encoder rerank; a knowledge graph from wikilinks | Local-first; cloud optional | MCP host; a local embedding model (FastEmbed, downloaded) | Embedding model for semantic search; the host agent is the LLM | AGPL-3.0; pushed 2026-10-25; 4,349★ | [github.com/basicmachines-co/basic-memory](https://github.com/basicmachines-co/basic-memory) |
| **OpenMemory** (mem0) | Classic: MCP tools added memories. Current repo: import/export coding sessions | Classic local server had its own local store (`unverified`; its path is gone from `mem0ai/mem0`). Current repo: local session store | Classic: semantic search (`unverified`). Current repo: pick sessions to port | Yes | Classic needed Docker + LLM/embedder; current is a Node CLI | Classic yes; current no | MIT; pushed 2026-07-29; 99★. The name's behaviour has changed; see Fit for cpd | [github.com/mem0ai/openmemory](https://github.com/mem0ai/openmemory), [docs.mem0.ai/llms.txt](https://docs.mem0.ai/llms.txt) |
| **sqlite-vec** (OSS extension) | `INSERT` vectors into `vec0` virtual tables; vectors are produced elsewhere | SQLite file | KNN vector search (float/int8/binary) | Yes, embedded | None for the extension; an embedder is needed to make vectors | Needs an embedding model (external) | Apache-2.0; pushed 2026-05-18; 8,216★; pre-v1 | [github.com/asg017/sqlite-vec](https://github.com/asg017/sqlite-vec) |
| **SQLite FTS5** (bundled module) | `INSERT` into an fts5 virtual table; external-content and contentless tables index existing rows | SQLite file (shadow tables) | Full-text: keyword, prefix, phrase, NEAR; BM25 ranking; `porter` / `unicode61` tokenizers | Yes, embedded | None | None | Public domain (SQLite); cpd bundles SQLite 3.53.2 with FTS5 compiled in (verified 2026-10-25) | [sqlite.org/fts5.html](https://sqlite.org/fts5.html) |
| **OpenViking** (`ovenviking`) | Commit/archive a session; memories extracted as Markdown; `write`; `ov compile` | Server-side virtual filesystem (`viking://`) + vector index | Directory-scoped semantic vector search plus `grep`/read over the tree | Self-host server (Docker); cloud and commercial editions | Python/uv server; a provider with an embedding model and a VLM | Yes (embedding + VLM; local Ollama is an option) | AGPL-3.0 server; pushed 2026-10-25; 40,791★ | [github.com/volcengine/OpenViking](https://github.com/volcengine/OpenViking) |
| **honcho** (OSS) | Store messages/events; a background "deriver" reasons over them | Postgres + pgvector (+ Redis) | Peer representations, session context, semantic search, natural-language insights | Yes, self-host (`honcho start`) or managed | Postgres+pgvector, Redis, Docker, an LLM provider key | Yes (LLM derivations + `text-embedding-3-small`) | AGPL-3.0; pushed 2026-10-25; 7,768★ | [github.com/plastic-labs/honcho](https://github.com/plastic-labs/honcho) |
| **hindsight** (OSS) | `retain`: an LLM extracts facts, temporal data, entities, relations | Postgres + pgvector (or Oracle 23ai); an embedded Python option | Four parallel strategies — semantic vector, BM25 keyword, graph, temporal — merged with RRF + cross-encoder rerank; "mental models" are a plain DB read | Yes, self-host (Docker/pip); Cloud option | Postgres+pgvector (or embedded); an LLM + embedder (25+ providers, local Ollama) | Yes | MIT; pushed 2026-10-25; 53,677★ | [github.com/vectorize-io/hindsight](https://github.com/vectorize-io/hindsight) |
| **holographic** (Hermes provider) | `MemoryStore.add_fact`; MCP tools; the plugin build also extracts via an LLM | SQLite (WAL) | FTS5 full-text + Jaccard-overlap rerank + optional HRR phase vectors (numpy); the standalone build has zero external dependencies | Yes, embedded | None standalone; the plugin build adds BGE/OpenAI embeddings, a cross-encoder and LLM extraction | Standalone: no. Plugin: optional embeddings/LLM | MIT; pushed 2026-06-11; 9★ | [github.com/bysc1000/holographic-memory](https://github.com/bysc1000/holographic-memory) |
| **retaindb** (OSS) | MCP tools (`remember`, `context`, `handoff`); a local runtime | RetainDB Local: atomic disk snapshot + append-only journal under `~/.retaindb/`; server: Postgres + pgvector | Lexical (BM25) + vector + graph, RRF fusion + rerank | Yes, local-first; server path self-hosted | None in local mode (no Postgres/Redis/Qdrant/keys); Postgres+pgvector for the server | Optional local transformer embeddings; otherwise hash vectors | Apache-2.0 / BSL 1.1; pushed 2026-06-12; 55★ | [github.com/RetainDB/RetainDB](https://github.com/RetainDB/RetainDB) |
| **byterover** (OSS, archived) | `brv curate`; the agent stores knowledge through a daemon | Local daemon store under the XDG data dir; cloud optional | Agentic map + indexed search (origin local vs shared); MCP | Yes | Node daemon; an LLM provider (20 supported, local Ollama possible) | Yes (LLM) | Elastic License 2.0 per README; GitHub reports NOASSERTION; repository **archived**, pushed 2026-06-25; 4,946★ | [github.com/campfirein/byterover-cli](https://github.com/campfirein/byterover-cli) |
| **supermemory** (OSS) | `add` / documents; automatic fact extraction; plugins and MCP | Local single binary with an embedded graph engine, or Docker volume; hosted platform | Hybrid search + user profiles; local embeddings | Yes, one binary or Docker; platform hosted | Local embeddings (`Xenova/bge-base-en-v1.5`) bundled; optional OpenAI/Gemini/Ollama; no external service needed | Embedding model required (local by default); text memories can use local Ollama | MIT; pushed 2026-10-25; 31,393★ | [github.com/supermemoryai/supermemory](https://github.com/supermemoryai/supermemory) |
| **memori** (OSS) | Automatic capture of LLM interactions; a background augmentation service | BYODB (bring your own database) or Memori Cloud | Structured memory recall (facts, events, preferences, relationships, rules) | BYODB local; Cloud hosted | An LLM API; `MEMORI_API_KEY` for augmentation (rate-limited without an account); a database | Yes (LLM); embeddings not stated | Apache-2.0 per the README badge, GitHub reports NOASSERTION; pushed 2026-10-03; 17,143★ | [github.com/MemoriLabs/Memori](https://github.com/MemoriLabs/Memori) |

## Fit for cpd

**None fits cpd's constraints as a replacement for retrieval on its own store.** Three
groups, and each fails for a concrete reason.

1. **Model-dependent and server-dependent memory systems** — mem0, Letta/MemGPT,
   Zep/Graphiti, LangMem, honcho, hindsight, OpenViking, memori, byterover. Every one needs
   an LLM call and/or an embedding model and/or a server (Postgres, pgvector, Redis, Neo4j,
   Docker). cpd calls no LLM API (hs-d194: it runs `pi` one-shot only for the distiller),
   is one Rust binary with a bundled SQLite, and must stay machine-local. Adopting one would
   add a process, a database and a model to a tool whose whole store is a single SQLite
   file.
2. **Embedded/local systems** — Basic Memory, OpenMemory, sqlite-vec, supermemory,
   retaindb, holographic. These are the close ones:
   - **sqlite-vec** is an index, not a retrieval design: it needs an embedding model to
     produce the vectors, which cpd does not have and must not add (a model download and a
     non-deterministic dependency). `inferred`: it also does not rank by keyword, so it would
     answer only semantic queries.
   - **Basic Memory** is AGPL-3.0, is its own MCP server with its own Markdown-file store,
     and needs a local embedding model for semantic search. It would not read cpd's
     `context_records`/`record_curations`; cpd's records are append-only rows with
     in-force/supersession/retirement semantics and a `topic`/`short` curation that Basic
     Memory does not model.
   - **OpenMemory** no longer exists as the local mem0 memory server the name is known for:
     the `openmemory/` path is gone from `mem0ai/mem0`, and `mem0ai/openmemory` is now a
     CLI that ports coding sessions between harnesses (verified 2026-10-25). Its classic
     behaviour is `unverified`.
   - **supermemory** is the strongest local fit technically (one binary, embedded graph,
     local embeddings, MIT), but it still requires an embedding model and is a full memory
     engine with its own store; adopting it means not using cpd's store for retrieval.
   - **retaindb** is local-first and needs no keys in local mode, but it is a Node daemon
     with its own `~/.retaindb/` journal and its own memory model.
   - **holographic** is the closest match on the constraints — MIT, SQLite, FTS5 + optional
     HRR, zero external dependencies, an MCP server — and it is the best evidence that
     FTS5 on SQLite is the right mechanism for a machine-local agent. It does not fit as a
     dependency: 9 stars, last push 2026-06-11, a personal Hermes provider, its own store
     and schema, and no notion of cpd's in-force/supersession/retirement/curation.
3. **Harnesses** — Claude Code, Cursor, Codex, OpenCode — have no reusable memory library;
   they show the pattern (a small always-injected instruction file plus on-demand reads),
   not a component cpd can take.

The decisive point is cpd's **semantics**, not storage: retrieval must answer over
`context_records` in force (supersession by `seq`, approved retirement), with the curated
`short`/`topic` when present, and expose a `- [id] short` line that links to full text via
`context show --record <id>`. No external system models that, and adding one would create a
second source of truth. cpd should **build** retrieval on its own store, taking the
mechanism (FTS5) rather than the system.

## Recommendation

**Build, not adopt.** Add retrieval over cpd's own store; take no external memory library.
The mechanism is **SQLite FTS5** (keyword/BM25), not vectors and not a graph, over a corpus
built from `context_records.text` plus `record_curations.topic` and
`record_curations.short`.

- **CLI syntax.**
  ```
  cpd context search <query> [--context <c>] [--limit <n>] [--kind <k,k>] [--all]
  ```
  `<query>` is one or more words, all of which must match (AND); a trailing `*` is a prefix
  match; FTS5 operators (`OR`, `NEAR`, quotes) are accepted. `--context` defaults to the
  selected Context like `context show`; `--kind` filters to `decision,note,question,answer`
  (`plan`/`brief` are not searched by default); `--all` includes records out of force
  (superseded or retired), which are excluded by default.
- **Result shape.** `markdown` is one line per hit, `- [<id>] <short>`, where `<short>` is
  the record's `record_curations.short` when it has one and otherwise the first sentence of
  `text` (truncated at 120 bytes, matching the curation limit). The full text is never in
  the result: `cpd context show --record <id>` prints it. `records` is
  `[{id, short, kind, at, seq, score, in_force}]`. A hit is a ranked pointer, exactly as
  `--topic` is today.
- **Ordering.** FTS5 BM25 rank, best first, with ties broken by **newest `seq` first**
  (recency wins, in the same direction as supersession). Relevance decides; recency only
  separates equals.
- **Default limit.** **20**, matching the existing "at most 20" idiom of first messages and
  turn-start updates. `--limit 0` (or `--all`) returns every hit. No hits is not an error:
  empty `records`, empty `markdown`.

Reasoning tied to the constraints: FTS5 is already compiled into cpd's SQLite (verified), so
it adds **no dependency, no process, no LLM and no embedding model**; it is machine-local and
deterministic; it gives BM25 ranking, prefix and phrase search, which a plain `LIKE` cannot;
and it scales past the ~1000-1500 rows where a scan is comfortable. A `LIKE` scan is
measurably enough today (1-5 ms), so if the team wants the smallest possible first step it
can ship keyword/substring search and add the FTS5 index later — but FTS5 is the target,
because ranking is the point of a search command and the index is a one-time migration.
Vectors are rejected because they need a model cpd must not depend on; a graph is rejected
because cpd's relations are already a one-column `supersedes` and a retirement table, far
below what a graph store costs. hs-n480 (14.5 s for `context show --digest` on 900 records)
is a caution not to recompute a whole rendering per row: the FTS5 index is built once and
queried, not rebuilt.

## Retrieval for cpd

**Corpus.** One FTS5 table indexed over `context_records.text`, `record_curations.topic` and
`record_curations.short`. Because `context_records` is append-only (its `BEFORE UPDATE`
trigger refuses edits) and `record_curations` may be inserted or updated by the distiller, an
**external-content** FTS5 table with `INSERT`/`UPDATE` triggers on those two tables is the
natural shape; SQLite FTS5 documents external-content and contentless tables explicitly
(<https://sqlite.org/fts5.html>, read 2026-10-25). Tokenizer: `porter unicode61` gives
stemming and case-folding without a model; prefix queries (`term*`) cover unfinished words.
A migration and a new schema hash are required (there is no FTS table today), and the index
adds storage roughly the size of the indexed text.

**Trade-offs at ~1000 rows.**

| Mechanism | At 1,446 rows | Cost | Why not / why |
|---|---|---|---|
| Substring scan (`LIKE`/`instr` over `text` + curation) | 1-5 ms, measured | No migration, no index, trivially explainable | No ranking, no prefix/stemming; grows linearly |
| **FTS5 keyword/BM25** | 21 ms to build, < 1 ms per query, measured | Migration, an external-content table and two triggers, ~text-sized index | **Chosen:** ranks, prefix/phrase, scales, no model |
| Vector (sqlite-vec + an embedder) | not measured (no embedder in cpd) | A local embedding model and its download, non-determinism, a second index | Rejected: cpd must not require a model (hs-d655, hs-d194) |
| Graph (Neo4j/FalkorDB/Graphiti) | n/a | A graph DB process | Rejected: cpd's relations are `supersedes` + `record_retirements`, not worth a graph engine |

**Why not reuse an existing design.** The embedded systems close to cpd (holographic,
RetainDB, Basic Memory, supermemory) all pair FTS with embeddings or their own store. cpd's
distinct asset is that `record_curations.short` and `topic` already exist, written by the
distiller, and that in-force/retirement is already computed in `core/src/context/inforce.rs`.
Searching `text` + `short` + `topic` reuses both; no external library can.

**Command, shape, order, limit** are in `## Recommendation`. Two details:
- Search is a **read**: no Event, no write, idempotent, like `context show`.
- Errors match `context show`: an empty `<query>` is `usage`; an unknown `--context` is
  `context_not_found` (exit 4); no hits is success.
- Records out of force are hidden by default so a search does not surface a superseded
  decision as current; `--all` is the explicit way to see them, labelled as `context show`
  labels them.

## First messages

hs-d655 drops the digest: the first message is the agent's **brief** (a Lead) or nothing but
a pointer (a Manager). Invariants are not restated in a message: hs-d655 puts the axioms and
hard rules in `AGENTS.md`, the single-source always-injected file.

- **A fresh Lead's first message** holds the header (`You are the Lead of Plan <id>
  "<title>" in Context <c>. Phase grounding: follow the cpd-lead skill.`), then **the brief
  in full** (the Manager wrote it; it names the goal, scope, the repos and the record ids
  that apply), then **one pointer line**:
  `Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd
  context show --record <id>`. Nothing else. The digest and the "at most 20 uncurated
  records raw" list go away: history is not restated, and the Lead already reads new records
  at its named points (`cpd context show --since <seq>`; cpd-lead skill). The working memory
  is the chat plus the brief.
- **A fresh Manager's first message** holds the header (`You are the Manager of Context <c>
  "<title>". Follow the cpd-manager skill.`) and the same pointer line. It has **no brief**,
  so nothing is provided in full; the digest is dropped here too. This is enough because the
  Manager's **turn-start message is already incremental and stays** (hs-d655): each turn it
  carries new records in force since its cursor plus a `Plans:` section (created, phase,
  PR opened/merged, job failed, done) and a fresh `Pending work:` section (cpd-manager skill;
  contract §15.7). A Manager that needs the current shape reads it on demand
  (`cpd context show`, `cpd plan list`, `cpd context pending --context <c>`,
  `cpd context search <query>`) instead of having it pushed.
- **If a seed is wanted for the Manager** (a Manager has no first turn-start update before it
  acts), the bounded, no-digest alternative is: the **open questions in force** and the
  **active Plans with their phase**, capped like the turn-start sections (20 lines each,
  then the command to read the rest). Those are the only Context-wide facts a Manager acts on
  before its first turn; the open questions are not history and do not depend on the dropped
  digest. This is a design choice, not a requirement of hs-d655.

The commands to render both messages already exist (`plan new` step 3, `context manager`),
and both call the same "Consumers" function (contract §4.6). Dropping the digest means that
function becomes the header + brief (Lead) or header (Manager) + pointer, and the ≤20 raw
records leave it; `context show --digest` and `context show --topic` are unaffected by the
retrieval decision (topic still reads a whole topic on demand).

## Open questions

- **`holographic` is ambiguous.** The User's list may mean the Hermes provider
  (bysc1000/holographic-memory, read here), or other projects that use the word (e.g.
  `NeoVertex1/nuggets`, `kannaka-labs/kannaka-memory`, `writerslogic/holographic-memory`).
  I read the closest match; if the User meant another, the row's storage/retrieval may differ.
  (inferred)
- **OpenMemory's classic behaviour is unverified.** The local mem0 memory server the name is
  known for is no longer in `mem0ai/mem0`; the current repository is a different tool. Its
  original storage (reportedly Qdrant + SQLite) and search are stated from memory, not from a
  current source.
- **Bundled-SQLite query plans.** The measured times are the system `sqlite3` 3.45.1; cpd
  bundles SQLite 3.53.2, so the FTS5 plan and the exact timings under the binary are
  `unverified`. The FTS5 symbols are verified present.
- **Corpus choice.** Indexing `text` + `topic` + `short` in one FTS5 table is the simplest
  shape; whether `short` should be indexed against its record id, and whether `plan`/`brief`
  should be searchable, are design decisions for the Job that builds it.
- **Whether the fresh-Manager seed is needed.** The recommendation is header + pointer; the
  bounded open-questions + active-Plans seed is an alternative if a Manager without a
  first turn-start update stalls. Needs the User's call, not a research answer.
- **Digest removal mechanics.** hs-d655 drops the digest, but `context show --digest` is in
  the contract; whether it is removed outright or kept as a debug view is not decided here.
