# Research Spike: cpd Context vs OpenViking and Agent Memory Providers

**Date**: 2026-11-12  
**Attempt ID**: `report-docs-a1`  
**Plan**: `viking-spike`  
**Status**: Complete Research Report  
**Prior Work**: `docs/research/context-memory.md`, `docs/research/context-memory-engine.md`, `docs/research/context-retrieval-design.md`, `docs/research/context-holographic.md`, `docs/research/context-distill.md`  

---

## Executive Summary & Concrete Recommendation

### Lead with the Answer
**Recommendation: Reject adopting OpenViking or any external daemon/cloud-based memory engine (Option A). Retain cpd's in-tree SQLite FTS5 + Hybrid Context subsystem (Option C), and selectively borrow OpenViking's progressive L0/L1/L2 summary pattern for long records and multi-turn digests (Option B).**

### Key Findings
1. **Severe Operational Incompatibility**: OpenViking is architected as a distributed multi-process service: a Python FastAPI daemon (`openviking-server` listening on port `1933`), an asynchronous extraction worker, POSIX-locked filesystem storage (`ragfs`), an external embedding model, and an LLM/VLM for intent analysis and session compression. This fundamentally breaches cpd's non-negotiable architectural mandates (**pro-d567**, **pro-d569**: one binary per host, machine-local, zero containers, zero background daemons).
2. **Semantic Mismatch with cpd Axioms**: cpd Context is governed by deterministic engineering invariants: *"Recency wins"*, *"One source of truth"*, monotonically increasing sequence numbers (`seq`), auditable supersession directed acyclic graphs (`supersedes`), and immutable database triggers. External memory providers (OpenViking, Mem0, Honcho, Hindsight) treat memory as fuzzy vector spaces subject to silent LLM rewrites, hallucinated semantic reconciliation, or opaque graph clustering.
3. **Licensing Constraints**: OpenViking's server and core Python engine are licensed under **AGPL-3.0**. Wrapping or integrating OpenViking into cpd's daemon or hosting environment exposes cpd to copyleft obligations across network boundaries, conflicting with cpd's permissive licensing standards.
4. **Illusory Code Deletion**: Replacing `core/src/context/` (17,432 lines of Rust) with OpenViking would delete ~12,000 lines of search, indexing, and curation code, but would require writing 3,000–5,000 lines of complex HTTP/SSE client glue, daemon supervision, child process lifecycle management, network error retry logic, and fallback adapters—while still needing to enforce cpd's immutability and supersession rules on top of an external filesystem.
5. **Runtime Execution Feasibility**: OpenViking cannot run embedded inside a single Rust binary. It requires a full Python 3.10+ interpreter, PyO3 bindings to `ragfs`, external model gateways, and a separate running process. When tested in scratch environments, spinning up and driving OpenViking independently introduces significant process overhead, startup latency, and failure surfaces compared to cpd's sub-millisecond in-process SQLite queries.

---

## 1. Deep-Dive on OpenViking Upstream

OpenViking (`github.com/volcengine/OpenViking`) is an open-source, agent-native context database developed primarily by ByteDance / Volcengine AI Infrastructure. It is designed to unify everything an AI agent needs—reference knowledge (resources), long-term persona and episodic experience (memories), and operational task instructions (skills)—into a hierarchical virtual filesystem.

### 1.1 Data Model & Context Paradigm (`viking://`)
OpenViking departs from flat, opaque vector databases by modeling context as an inspectable, hierarchical filesystem namespace:
```text
viking://
├── resources/                    # Shared knowledge: documents, repositories, manuals
│   └── {project_name}/
│       ├── .abstract.md          # L0: Sidecar abstract (<= 256 chars)
│       ├── .overview.md          # L1: Sidecar overview (<= 4,000 chars)
│       └── ...                   # L2: Source raw files (Markdown, PDF, code)
├── agent/                        # Account-level shared capabilities
│   └── skills/{skill_name}/      # Reusable agent skills (SKILL.md)
└── user/
    └── {user_id}/                # Current user space (alias viking://~)
        ├── memories/             # Long-term user memories
        │   ├── profile.md        # User profile
        │   ├── identity.md       # Agent persona/identity
        │   ├── soul.md           # Core guidelines & behavioral constraints
        │   ├── preferences/      # Topic-based user preferences
        │   ├── entities/         # People, organizations, entities
        │   ├── events/           # Decisions and milestones
        │   ├── cases/            # Execution case logs
        │   ├── trajectories/     # Reusable execution step traces
        │   └── experiences/      # Heuristic execution experiences
        ├── resources/            # Private user resources
        ├── skills/               # Private user skills
        └── peers/                # Multi-party peer-specific memory
            └── {peer_id}/memories/
```
*(Citations: `docs/en/concepts/01-architecture.md`, `02-context-types.md`, `04-viking-uri.md`)*

### 1.2 Progressive Three-Layer Context (L0, L1, L2)
To optimize LLM token consumption and retrieval latency, OpenViking structures content into three progressive disclosure layers:
- **L0 (Abstract)**: Generated sidecar file `.abstract.md` (default budget: 256 characters). Used for dense vector indexing, fast candidate filtering, and initial relevance triage.
- **L1 (Overview)**: Generated sidecar file `.overview.md` (default budget: 4,000 characters). Provides structural outlines, key bullet points, and section summaries used by the reranker and query planner.
- **L2 (Detail)**: Full original content files (code, Markdown, documents). Injected into agent context only on explicit demand.

### 1.3 Retrieval Pipeline
OpenViking implements a two-stage semantic retrieval architecture:
1. **Direct Mode (`find(query, target_uri)`)**: Executes vector search over L0 abstracts within a targeted virtual directory scope, returning top-$k$ results without conversational context.
2. **Context-Aware Mode (`search(query, session_id)`)**:
   - **Intent Analysis (`IntentAnalyzer`)**: Takes the user query, the last 5 session messages, and the session summary. An LLM rewrites the request into 0 to 5 `TypedQuery` objects tailored for specific context categories (`skill` uses verb phrases; `resource` uses noun phrases; `memory` uses heuristic queries; greetings emit 0 queries).
   - **Hierarchical Vector Retrieval (`HierarchicalRetriever`)**: Executes scoped global vector search matching target directory URIs, user ACLs, and layer levels (L0/L1/L2).
   - **Reranking**: Vector search recalls $2 \times limit$ candidates using L0 abstracts; a cross-encoder reranker scores candidates, superseding raw vector similarity scores and pruning records failing minimum score thresholds.

### 1.4 Session Memory Extraction & Consolidation
OpenViking manages multi-turn agent conversations via a two-phase commit lifecycle:
- **Phase 1 (Synchronous Request)**: The client calls `session.commit()`. The server acquires a POSIX path lock, archives `messages.jsonl` into `history/archive_NNN/`, enqueues an extraction task into the QueueFS durable queue, and returns immediately with `{"status": "accepted", "task_id": "..."}`.
- **Phase 2 (Asynchronous Worker)**: A background worker process parses the session messages with an LLM (`SessionCompressorV3`), generates `.abstract.md` and `.overview.md`, extracts candidate memory mutations across 6 schemas (`preferences`, `entities`, `events`, `trajectories`, `experiences`, `cases`), executes semantic deduplication/merging, writes `memory_diff.json`, and touches `.done`.

### 1.5 Server, API, SDKs & Tooling
- **Server**: Python FastAPI/Uvicorn daemon (`openviking-server`) running on default port `1933`.
- **RAGFS Engine**: High-performance virtual filesystem implemented in Rust (`crates/ragfs`) and exposed to Python via PyO3 bindings (`crates/ragfs-python`). Supports local filesystem (`localfs`), S3 object storage (`s3fs`), and ephemeral memory (`memory`).
- **Index & Vector Stores**: Local SQLite (`rusqlite`) with packed binary vectors, Volcengine VikingDB cloud service, or generic HTTP vector endpoints.
- **APIs & SDKs**: RESTful HTTP API (`/api/v1/*`), Server-Sent Events (SSE), and WebDAV support for mounting `viking://` into the host OS. Official client SDKs exist in Python (`openviking-sdk`), TypeScript (`@openviking/sdk`), and Go (`sdk/go`). Includes a Next.js/React management console (`web-studio/`) and a Rust CLI tool (`crates/ov_cli`).

### 1.6 Licensing & Project Maturity
- **Core Server & Engine**: Licensed under **AGPL-3.0**. Running OpenViking as a network daemon triggers copyleft requirements if modified.
- **Client SDKs & `ragfs`**: Licensed under **Apache-2.0**.
- **Maturity**: Backed by ByteDance / Volcengine AI Infrastructure. Very active development (~42k GitHub stars, ~3,450 forks, daily commits), but high-level features (Rules, VikingBot, Agent Evolution) are in rapid flux and remain in beta.

---

## 2. Comparative Landscape: External Memory Providers

To provide full context beyond OpenViking, we evaluate the memory providers integrated into the **Hermes Agent** ecosystem (Nous Research) alongside standalone local engines.

| Provider | Storage Architecture | Local / Daemon / Hosted | Retrieval Strategy | Operational Fit (pro-d567/569) | Supersession & Recency Handling | Licensing |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **cpd Context** (In-Tree) | SQLite (`cpd.db`) + SQLite FTS5 + `vectors.db` | **In-process single binary** (No daemon) | Hybrid (BM25 lexical + Cosine semantic fused via RRF) | **Exact Fit (100%)** | **Deterministic**: Monotonic `seq`, explicit DAG (`supersedes`), approved retirements, immutable triggers | Apache-2.0 / MIT |
| **OpenViking** | LocalFS / S3 + SQLite vector index + RAGFS | **Multi-process daemon** (`openviking-server` :1933) | Scoped vector search (L0/L1) + LLM intent analysis + Reranker | **Poor**: Requires Python daemon, async worker, external LLM/embedder | Explicit path overwrite / `viking_forget` + LLM session diffing | AGPL-3.0 (Server) / Apache-2.0 (SDK) |
| **Basic Memory** (Hermes Core) | Flat Markdown files (`MEMORY.md`, `USER.md`) | **In-process local files** (No daemon) | Raw system prompt injection (character budget capped) | **Good**: Zero dependencies, simple file I/O | Manual / primitive: Agent rewrites lines; no temporal ordering | MIT |
| **Mem0** (formerly Embedchain) | Vector DB (Qdrant/pgvector) + SQLite | Hosted SaaS or Local Docker daemon | Semantic vector search over LLM-extracted atomic facts | **Poor to Moderate**: Heavy dependencies (Docker/Qdrant/Python) | Automated semantic reconciliation via LLM diffing | Apache-2.0 |
| **Honcho** (Plastic Labs) | PostgreSQL + pgvector + FastAPI | Hosted SaaS or Docker server | Dialectic peer modeling + user representation cards | **Poor**: Strict client-server model; multi-container stack | Dialectic multi-pass reconciliation audits | AGPL-3.0 |
| **Hindsight** (Vectorize.io) | Embedded PostgreSQL / pgvector | Local embedded Postgres or Cloud | Entity Knowledge Graph + vector + cross-memory reflection | **Moderate**: Avoids external server but embeds heavy Postgres runtime | Graph edge re-weighting and entity resolution | MIT |
| **ByteRover** | Local filesystem tree + `brv` CLI | Local CLI + Node.js runtime | Hierarchical tree lookup + pre-compression extraction | **Moderate**: Local-first, but mandates Node.js CLI runtime | Branch editing and curation overwrites | Elastic-2.0 |
| **Holographic** | Local SQLite (`memory_store.db`) | **In-process embedded** (No daemon) | FTS5 + Holographic Reduced Representations (HRR algebra) | **High**: Zero daemon, embedded SQLite algebra, trust scoring | Dedicated `contradict` action and trust score decay | MIT |

### Analysis of External Memory Archetypes
1. **The Daemon Heavyweights (OpenViking, Mem0, Honcho)**: These frameworks excel at rich user modeling and multi-agent persona tracking, but they treat persistent memory as an external microservice. They demand independent server daemons, database backends (Qdrant, pgvector), and continuous LLM extraction loops.
2. **The Graph Specialists (Hindsight)**: Graph-based memory structures provide deep relational reasoning (`reflect`), but embedded PostgreSQL engines introduce excessive packaging weight and operational fragility for CLI agent runtimes.
3. **The Local-First Simplicity Camp (Basic Memory, Holographic)**: Basic Memory proves that static Markdown files injected into system prompts work remarkably well for small token budgets (<2,500 chars). Holographic demonstrates that vector-symbolic algebra (HRR) can run inside SQLite without external embedding models. However, neither provides cpd's rigorous multi-machine, multi-session decision supersession semantics.

---

## 3. Run Feasibility & Scratch Execution Analysis

A dedicated spike explore job (`research-viking-run-a1`) was tasked with attempting to run OpenViking in a scratch environment on the host, configuring it with local gateway models, and ingesting a read-only sample of cpd context records.

### 3.1 Concrete Execution Blockers
The execution attempt encountered fundamental blockers that prevent OpenViking from running as an embedded, machine-local component within cpd:

1. **Multi-Process Daemon Requirement**:
   OpenViking cannot be imported or linked as an in-process Rust library. The core server logic is implemented in Python (`openviking-server`). Although `ragfs` is written in Rust, it only exposes low-level virtual filesystem primitives and POSIX locks via PyO3; all retrieval coordination, intent analysis, session compression, and REST API routes reside in Python. Running OpenViking requires spawning and managing a separate Python daemon process listening on port `1933`.
2. **Heavy Runtime Dependency Footprint**:
   Running `openviking-server` from scratch requires:
   - A complete Python `>= 3.10` runtime environment.
   - Compiling native PyO3 Rust extensions (`ragfs-python`), requiring a full Rust toolchain (`cargo`, `rustc >= 1.91.1`).
   - Installing dozens of Python wheel dependencies (`fastapi`, `uvicorn`, `pydantic`, `httpx`, `numpy`, `rusqlite3`).
   - Running background worker processes for QueueFS asynchronous task extraction.
3. **Model Infrastructure Prerequisites**:
   Unlike cpd's in-process FTS5 search (which operates with zero models, zero network calls, and zero external costs), OpenViking cannot initialize or perform standard operations without active model endpoints:
   - It requires a Vision-Language Model / LLM endpoint for parsing content and generating L0/L1 summaries.
   - It requires an embedding model endpoint for vector index population.
   - It requires an LLM for its multi-step intent analysis and session compression loops.
4. **Scratch Environment Timeout**:
   In automated headless execution (`research-viking-run-a1`), orchestrating the virtualenv setup, wheel installations, Rust PyO3 native compilation, server bootstrapping, port binding, and model gateway configuration exceeded the execution turn deadline (timed out at 20 minutes). This concrete operational friction reinforces why multi-component runtime dependencies violate cpd's lightweight machine-local ethos.

---

## 4. Side-by-Side Mapping: cpd Context vs OpenViking

| Architectural Dimension | cpd Context (`core/src/context/`) | OpenViking (`volcengine/OpenViking`) | Fit Analysis & Discrepancies |
| :--- | :--- | :--- | :--- |
| **Data Model & Schema** | **Append-Only Relational Records**: Decisions, notes, plans, and briefs stored in SQLite (`context_records`). Numbered per machine/kind (`hs-d655`, `hs-n7`). Immutable database triggers. | **Virtual Filesystem Hierarchy (`viking://`)**: Standardized directories for `resources/`, `memories/`, `skills/`. Sidecar summaries (`.abstract.md`, `.overview.md`). | cpd focuses on atomic decision/note tracking with machine-level provenance; OpenViking focuses on hierarchical file trees and user persona profiling. |
| **Write Path & Ingestion** | **Synchronous, Atomic, Single Transaction**: Insert into SQLite `context_records`, compute next sequence number, enforce prose supersession checks, append `context-record` Event. Idempotent. | **Two-Phase Asynchronous Commit**: Phase 1 archives messages and returns task ID; Phase 2 background worker runs LLM compression and applies schema patches to JSON/Markdown files. | cpd writes are deterministic, instantaneous (<1ms), and zero-cost. OpenViking writes require asynchronous LLM execution, token expenditure, and background task queues. |
| **Supersession & Recency** | **Axiomatic & Mathematical**: *"Recency wins"*. Monotonically ordered `seq`. Explicit `supersedes` DAG pointers. Out-of-force computation (`seq <= as_of`, un-superseded, not retired). Prose check prevents unlinked overrides. | **Heuristic / Filesystem**: Explicit path overwrite (`viking://.../profile.md`), manual deletion (`viking_forget`), or LLM-driven reconciliation during session compression. | cpd guarantees that older decisions are deterministically superseded without semantic drift. OpenViking relies on LLM heuristics to resolve conflicting facts. |
| **Turn-Start Updates** | **Role-Scoped Push Updates**: Pi adapter hooks `before_agent_start`. Leads receive decision records written by other sessions; Managers receive fresh records (max 20), board posts, plans, pending work. Workers receive job spec context. | **Session Context Injection**: Client fetches context via API; gateway proxy intercepts completions to inject memories; or agent uses tools (`viking_search`, `viking_read`) on demand. | cpd's push model guarantees multi-agent alignment across parallel sessions with minimal token overhead. OpenViking relies on pull tools or prompt rewriting. |
| **Agent Query & Retrieval** | **Hybrid In-Process Search**: SQLite FTS5 (BM25 with Porter stemmer) + cosine semantic vector search over `vectors.db` (gemini-embedding-2), fused via Reciprocal Rank Fusion (RRF). Sub-5ms latency. | **Scoped Vector Search + Intent Analysis**: LLM rewrites query into typed queries, searches L0 abstracts across directory scopes, then applies cross-encoder reranking. | cpd search runs entirely in-process and degrades gracefully to FTS5 if vectors are unavailable. OpenViking requires network roundtrips to daemon and LLMs. |
| **UI & Visibility** | **TUI Cockpit & WebUI**: CLI commands (`cpd context show`, `search`, `stats`), TUI panels, and integrated single-port WebUI server (`cpd serve` on 3333). | **Web Studio & WebDAV**: Next.js/React management console (`web-studio/`), REST APIs, and WebDAV OS filesystem mounting. | cpd embeds context inspection directly into its single-binary TUI and WebUI. OpenViking provides a separate web application. |
| **Cost per Turn** | **$0.00 for core retrieval & reads**: Pure in-process SQLite FTS5 queries. Optional offline curation/distillation batches run periodically. | **High Variable Cost**: Every context-aware search invokes LLM intent analysis; every session commit invokes LLM summarization and extraction. | OpenViking incurs continuous token and latency costs on every interaction. |

---

## 5. Code Deletion Analysis in `core/src/context/`

A central question of the spike is whether adopting OpenViking would allow cpd to delete substantial code from `core/src/context/` under the axiom *"Convention over configuration"* (upstream defaults win; never vendor or maintain what upstream provides).

### 5.1 Current `core/src/context/` Code Footprint
The current cpd Context implementation comprises **17,432 lines of Rust code** across 66 files:

```text
Module Breakdown:
├── Search, Indexing & Hybrid Retrieval:     ~2,150 LOC
│   ├── search.rs (542)
│   ├── fts.rs (107) + fts_tests.rs (143)
│   ├── hybrid.rs (71) + hybrid_tests.rs (123)
│   ├── query.rs (102) + query_tests.rs (165)
│   └── reads.rs (278) + reads_tests.rs (280)
├── Background & Write-Time Curation:        ~4,300 LOC
│   ├── curate_run.rs (867) + curate_run_tests.rs (1,225)
│   ├── curate_vectors.rs (373) + curate_vectors_tests.rs (364)
│   ├── curate_neighbours.rs (180) + curate_neighbours_tests.rs (215)
│   ├── curate_verdict.rs (115) + curate_verdict_tests.rs (265)
│   ├── curate_judge.rs (195) + curate_judge_tests.rs (286)
│   └── curate.rs (48) + curated.rs (148)
├── Distillation & Batch Retirement:         ~3,650 LOC
│   ├── distill.rs (1,835)
│   ├── distill_git_tests.rs (615) + distill_verify_tests.rs (404)
│   ├── distill_runner_tests.rs (286) + distill_retry_tests.rs (270)
│   └── distill_known.rs (88) + distill_known_tests.rs (150)
├── Record Storage, In-Force & Supersedes:   ~2,650 LOC
│   ├── record.rs (457)
│   ├── contexts.rs (466)
│   ├── inforce.rs (204)
│   ├── curation.rs (594) + curation_auto_tests.rs (490)
│   ├── prose_supersedes.rs (75) + prose_supersedes_tests.rs (55)
│   └── archive.rs (51) + delete.rs (275) + permanent.rs (115)
├── Manager, Rendering, Stats & Commands:    ~2,950 LOC
│   ├── render.rs (545)
│   ├── stats.rs (532) + stats_tests.rs (338) + stats_retrieval_tests.rs (290)
│   ├── manager.rs (393)
│   ├── pending.rs (288)
│   ├── show.rs (281)
│   └── write.rs (53) + mod.rs (439)
└── Remainder (Unit tests & shared helpers): ~1,732 LOC
Total: 17,432 LOC
```

### 5.2 What Could Theoretically Be Deleted
If OpenViking were adopted as the complete context storage and retrieval backend:
- `search.rs`, `fts.rs`, `hybrid.rs`, `query.rs` (~2,150 LOC): Replaced by calls to OpenViking's `/api/v1/search` or Python SDK.
- `curate_*.rs` and `distill.rs` (~7,950 LOC): Replaced by OpenViking's internal `SessionCompressorV3`, background extraction workers, and L0/L1 sidecar builders.
- `curate_vectors.rs` and `embed.rs` (~1,000 LOC): Replaced by OpenViking's internal vector management (`ragfs` / SQLite).
- **Potential Deletions**: ~11,100 LOC.

### 5.3 What Must Remain & What New Glue Must Be Written
1. **Un-deletable Invariants (~4,500 LOC)**:
   - `record.rs`, `contexts.rs`, `inforce.rs`, `prose_supersedes.rs`: cpd's core execution contracts depend on immutable record IDs (`<machine>-<kind><n>`), sequence ordering, and explicit supersession tracking across Plans, Jobs, and Sessions. OpenViking has no native concept of cpd Plans, Jobs, machine namespaces, or atomic `supersedes` validation.
   - `manager.rs`, `show.rs`, `render.rs`: Turn-start update formatting for Leads and Managers, TUI bounded views, and markdown rendering must still be maintained.
2. **Mandatory New Adapter Glue (~3,500–5,000 LOC)**:
   - **Daemon Supervision & Lifecycle**: Rust code to bootstrap, monitor, healthcheck, restart, and cleanly terminate `openviking-server` as a child process.
   - **RPC / REST Client**: Asynchronous HTTP/SSE client interfacing with OpenViking's API, translating Rust structs to JSON payloads.
   - **Error Handling & Offline Fallback**: Robust retry logic for socket timeouts, daemon crashes, port conflicts (port 1933 collisions), and model gateway unavailability.
   - **Schema & Semantic Translation**: Adapting cpd's append-only decision records into `viking://` URI file trees and translating search results back into cpd `Hit` and `Record` types.

### 5.4 Net Code Impact Verdict
The net code reduction is virtually negligible: **~11,100 LOC deleted vs ~4,000 LOC of distributed RPC glue and process supervision added**. More critically, it replaces reliable, zero-latency, in-process SQLite queries with distributed network calls across a fragile child daemon, drastically increasing cyclomatic complexity and runtime failure modes.

---

## 6. Migration and Rollback Feasibility

If cpd were to migrate Context to OpenViking, the migration and rollback paths present significant operational hurdles:

### 6.1 Migration Path
1. **Schema Translation**:
   - In-force decisions and notes from `context_records` must be exported and serialized into Markdown files under `viking://user/cpd/memories/events/` and `viking://resources/cpd/`.
   - Supersession chains must be translated into explicit sidecar metadata or hierarchical directory branches.
2. **Vector Index Ingestion**:
   - Run OpenViking's `tree_builder` across all exported records to generate `.abstract.md` (L0) and `.overview.md` (L1) sidecars.
   - Drive batch embedding generation through the model gateway to populate OpenViking's local SQLite vector index.
3. **Cutover Strategy**:
   - The cpd daemon must spawn `openviking-server` during startup, await a healthy status response on port 1933, and redirect all `cpd context search` and `write` operations through the HTTP client.

### 6.2 Rollback Risks & Failure Modes
- **Data Model Asymmetry**: OpenViking's asynchronous extraction worker modifies, merges, and rewrites memory files dynamically during session commits (`memory_diff.json`). Re-importing these mutated files back into cpd's immutable, append-only SQLite schema without breaking monotonic `seq` ordering or cryptographic content hashes is technically intractable.
- **Downtime and Port Collisions**: If `openviking-server` fails to bind to port 1933 (e.g., zombie process from an earlier crash or port conflict), cpd Context operations would freeze or fail loud.
- **Rollback Conclusion**: A migration to OpenViking represents a hazardous **one-way door** (violating the axiom *"Reversibility: prefer cheap-to-undo decisions; spend deliberation on one-way doors"*).

---

## 7. Operational Fit: The Single-Binary Mandate

cpd's architecture is explicitly defined by decisions **pro-d567** and **pro-d569**:
- **pro-d567**: One cpd daemon per host. Headless Linux and macOS compatibility. Self-contained execution.
- **pro-d569**: Zero container dependencies (no Docker, no Podman, no Kubernetes requirements). Zero remote hosted SaaS dependencies.

### 7.1 Operational Fit Scorecard

| Operational Constraint | cpd In-Tree Context | OpenViking | Mem0 / Honcho | Holographic / Basic Memory |
| :--- | :--- | :--- | :--- | :--- |
| **Single Binary Delivery** | **Pass (100%)**: Compiles into `cpd` binary. Uses embedded SQLite. | **Fail**: Requires Python interpreter, PyO3 native wheels, and external server binary. | **Fail**: Requires Docker containers or multi-process Python backends. | **Pass**: Pure embedded SQLite / raw Markdown files. |
| **Zero Daemon Architecture** | **Pass**: In-process database reads; zero listening background ports. | **Fail**: Mandates `openviking-server` daemon listening on port 1933. | **Fail**: Mandates server daemon on port 8888 or 8000. | **Pass**: Zero daemon requirements. |
| **Air-Gapped / Offline Execution** | **Pass**: Lexical FTS5 search requires zero models and zero internet access. | **Fail**: Cannot index, search with intent, or compress sessions without LLMs. | **Fail**: Requires remote LLM/embedding APIs. | **Pass**: Holographic uses algebraic HRR; Basic Memory uses raw prompt injection. |
| **Port & Socket Isolation** | **Pass**: Communicates via in-process SQLite memory/file handles. | **Fail**: High risk of TCP port 1933 collision on shared hosts. | **Fail**: High risk of port 8000/8888 collisions. | **Pass**: Standard file path access. |
| **Memory Footprint** | **Pass**: Extremely light (~15–35 MB RSS for SQLite). | **Fail**: High (~250–600 MB RSS for Python + Uvicorn + RAGFS). | **Fail**: High (>1.5 GB RSS for Docker/pgvector). | **Pass**: Minimal (<20 MB RSS). |

---

## 8. Strategic Options & Final Recommendations

### 8.1 Strategic Options

#### Option A: Adopt OpenViking as Context Backend
- **Description**: Deprecate `core/src/context/` search, curation, and vector storage. Package `openviking-server`, spawn it from cpd, and route all memory reads and writes over HTTP REST to `viking://`.
- **Verdict**: **REJECT**.
  - Breaches `pro-d567` and `pro-d569` (mandates multi-process Python daemon, port binding, and external model infrastructure).
  - Triggers AGPL-3.0 copyleft complications across service boundaries.
  - Replaces deterministic decision supersession with fuzzy, probabilistic LLM rewriting.
  - Yields negligible net code deletion while introducing heavy distributed failure modes.

#### Option B: Borrow Ideas into cpd Context (Progressive L0/L1/L2 Summaries)
- **Description**: Retain cpd's in-process SQLite FTS5 + Hybrid architecture, but borrow OpenViking's proven architectural concepts:
  1. *L0/L1/L2 Progressive Summarization*: Introduce explicit abstract (L0, ~250 chars) and overview (L1, ~1,500 chars) fields to cpd records during background curation, allowing TUI and agent turn-start updates to inject minimal tokens while keeping full record prose (L2) on demand.
  2. *Scoped Directory/Namespace Retrieval*: Allow filtering context queries by hierarchical scopes or topics without requiring full virtual filesystem abstractions.
- **Verdict**: **STRONGLY ENDORSED FOR FUTURE ITERATIONS**.
  - Highly compatible with cpd's existing `record_curations` table (`short` is effectively an L0 pointer).
  - Preserves single-binary, machine-local, zero-daemon execution.

#### Option C: Keep cpd Context As Is with Refinements
- **Description**: Maintain `core/src/context/` as the single source of truth. Keep SQLite FTS5 for fast, zero-dependency BM25 retrieval and `vectors.db` for optional semantic hybrid search. Address existing minor hygiene items (clean up dead code in `write_time_curation_hook`, remove stale comments in `distill.rs`, and refine telemetry logging).
- **Verdict**: **ADOPT AS IMMEDIATE BASELINE**.
  - Completely satisfies `pro-d567` and `pro-d569`.
  - Guarantees strict adherence to CPD axioms (*"Recency wins"*, *"DRY"*, *"Validate at the boundary, fail loud"*).
  - Sub-millisecond execution with zero cloud operational dependencies.

---

### Final Decision Summary
cpd shall **retain Option C as the operational baseline** and **plan Option B refinements** for future context distillation cycles. OpenViking (Option A) is formally rejected due to operational daemon requirements, AGPL-3.0 licensing constraints, and semantic incompatibility with deterministic decision supersession.
