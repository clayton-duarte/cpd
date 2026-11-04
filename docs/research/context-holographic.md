> **Removed (pro-d1, pro-d9):** the holographic engine is gone and `cpd context search` is an in-memory FTS5 index now (contract §4.6 "The search rail"); only the `## Search contract` section (query rewrite R1-R10) of this note is still in force, the rest is history.

# Adopting holographic as cpd's memory engine

Design for Job `holo-docs`, Plan `context-on-demand`. It decides how cpd reaches
**holographic** — `bysc1000/holographic-memory`, the Hermes provider, the engine the User named on
2026-10-25 — to serve `cpd context search`, and how dotfiles installs it. It builds the two build Jobs.

It **supersedes** the engine choice in [`context-memory-engine.md`](context-memory-engine.md) (which
recommended an engine but left the choice to the User) and the self-owned FTS5 index in
[`context-retrieval-design.md`](context-retrieval-design.md) ("Index and migration"): the planned
`context_search` FTS5 table and its five triggers are **not** built. The **agent surface** of
`context-retrieval-design.md` ("Search contract", "First messages") is kept unchanged; only what
answers the query moves behind the engine. Decisions cited: **hs-d655** (retrieve on demand),
**hs-d675** (adopt a local memory engine), User 2026-10-25 (the engine is holographic; cpd is the
only interface the agent uses; dotfiles installs every cpd dependency). The prior files are read,
not redone.

Every number below carries its method; anything not measured is labelled `unverified` in the same
sentence. This file is design only: it writes no cpd or dotfiles code.

## Method

- **Question.** How does Rust cpd reach a pure-Python library with no CLI, what is the exact JSON
  protocol, when is the derived index written and how is staleness handled, what does
  `cpd context search` return when holographic answers, what happens when the engine is absent or
  broken, how does dotfiles install it, and how is the whole thing cut into PRs?
- **Prototype (under `/tmp`, removed before submit).** `git clone
  https://github.com/bysc1000/holographic-memory` at commit
  `166ba35c5dd6973956dabee97165fc4eb0750b20` (branch `main`, `pyproject` version **0.16.0**, MIT),
  installed into a `uv` venv (CPython **3.12.3**, the machine interpreter) with **numpy 2.5.4**
  (`uv pip install './repo[numpy]'`). A prototype bridge (`bridge.py`, JSON lines over stdin/stdout)
  implemented the protocol below and was driven directly and over the pipe. Corpus: read-only export
  of `$CPD_HOME/cpd.db` on **2026-10-25**, 1,465 `context_records` (Context `building-cpd` 1,362,
  `dotfiles` 71, `homeserver` 32). **Record ids are unique only within a Context** (measured:
  1,465 records, 1,362 distinct ids; `hs-p1`, `hs-n1`, `hs-d1` each occur in all three Contexts), so
  the design keys the index by Context (below).
- **Measurements** (all `timeout`-wrapped, all processes killed, tree removed):
  - **Full index** (reset + `index` 1,362 records, resident child, 200-record batches): **25.1 s**;
    a second run over 1,465 records took **27.0 s** — consistent with the engine file's 27.2 s.
  - **Search**, `FactRetriever.search(query, limit=20)` measured **in-process** (`perf_counter`,
    30 reps × 10 real terms): **min 1.35 ms, median 14.2 ms, max 20.3 ms** (common terms like
    `decision` ~14–16 ms; rare ones like `retriev*` ~1.4 ms). This is the engine file's 0.06–15 ms,
    reproduced.
  - **One-shot spawn + one request**, `subprocess.run([python, bridge.py, --db, …])`: **min 97.2,
    median 128.3, max 142.7 ms** for a search (10 calls); **min 94.1, median 103.8 ms** for `ping`.
    The spawn overhead is the import: `python -c 'import holographic_memory'` **83.6 ms** min,
    `import numpy` 74.9 ms, `MemoryStore(...)` open 86.1 ms.
  - **Disk / memory:** engine DB 15.5 MB for 1,362 facts; peak RSS of a one-shot bridge 34,492 KB
    (`/usr/bin/time -v`); resident child 60 MB (`ps -o rss`).
  - **Protocol probes:** `retriev*` (prefix) works; `a b` is FTS5 AND; `a OR b` works; malformed
    `"unterminated`, `AND`, and `""` all raise `sqlite3.OperationalError` at the `MATCH` (the bridge
    maps this to `bad_query`). `FactRetriever._fts_candidates` swallows the same error as `[]`, so
    the bridge must probe first (below).
- **Install facts (read, not guessed).** The dotfiles clone is at `~/dotfiles` (this machine:
  `/home/clayton/dotfiles`), installed by `bootstrap.sh` through chezmoi. cpd is installed by
  `scripts/cpd-install.sh` (run by `home/.chezmoiscripts/run_after_75-cpd.sh.tmpl`), which runs the
  cpd repo's `scripts/install-cpd.sh`; that script installs the binary to `~/.local/bin/cpd` and the
  pi adapter to `~/.local/share/cpd/pi` (`<dir of the binary>/../share/cpd/pi`, contract §8.2).
  `uv` is installed on every machine by `home/.chezmoidata/packages.yaml`.
- **PyPI trap (measured 2026-10-25).** The name `holographic-memory` **on PyPI is a different
  project**: version **1.4.0**, author Benedict Chen, licence *"Custom Non-Commercial License with
  Donation Requirements"*. `pip install holographic-memory` installs the wrong engine under a
  non-commercial licence. The Hermes provider has **no PyPI release and no git tag** (GitHub
  releases and tags both empty); it is pinned by **commit** from `github.com/bysc1000/holographic-memory`.

## Bridge

**Decision: a small Python bridge shipped in the cpd repo at `contrib/holographic/bridge.py`,
installed by cpd's installer, spawned by cpd per operation, speaking JSON lines over
stdin/stdout.** Rejected alternatives, with the reason:

- **holographic's stdio MCP server over JSON-RPC.** No bridge to maintain, but it adds an MCP client
  to cpd, exposes engine tool semantics (`add`/`probe`/`reason`/`update`/…) that cpd does not use,
  and gives cpd a protocol much larger than the four operations it needs. cpd is the only interface
  the agent uses; cpd does not need to become an MCP host. **Rejected (YAGNI).**
- **A thin CLI dotfiles installs.** Both the bridge and a CLI are Python wrappers over the same
  library; putting the wrapper in dotfiles (another repo) splits cpd's protocol across two repos and
  leaves the protocol untested where cpd's tests live. **Rejected (DRY, tests).**

The bridge is cpd's authored code: it lives with cpd, is versioned with cpd, and is installed by
cpd's own `scripts/install-cpd.sh` next to the pi adapter. dotfiles installs only the external
dependency (the venv: holographic + numpy). The bridge is ~60 lines of straight-line code.

### Exact protocol

JSON Lines, UTF-8, one JSON object per line (`\n`-terminated). cpd writes one or more requests, then
closes stdin; the bridge answers one response line per request **in order** and exits on EOF.
Standard output carries the protocol and nothing else; diagnostics go to stderr. Unknown `cmd` is
`bad_cmd`.

Requests:

```json
{"cmd":"ping"}
{"cmd":"stats"}
{"cmd":"reset"}
{"cmd":"index","facts":[{"record":"hs-d7","text":"…","topic":"…","short":"…"}]}
{"cmd":"search","query":"retriev*","limit":20}
```

| Command | Request fields | Successful response | Meaning |
|---|---|---|---|
| `ping` | — | `{"ok":true,"engine":"holographic","version":"0.16.0","pid":123}` | Health/version, used by `cpd doctor` and by cpd before the first use. |
| `stats` | — | `{"ok":true,"facts":1362,"indexed_seq":1362,"curated_at":"2026-10-25T12:00:00Z"}` | Fact count and the projection watermark cpd last recorded (see "Index and staleness"). |
| `reset` | — | `{"ok":true}` | Delete every fact of this Context's DB. Used by a full reindex only. |
| `index` | `facts` (array) | `{"ok":true,"inserted":N,"updated":M}` | Upsert each fact by `record` (the mapping key). |
| `search` | `query` (string), `limit` (integer) | `{"ok":true,"hits":[{"fact_id":7,"record":"hs-d7","score":0.2773}]}` | Ranked hits, best first. `record` is the cpd record id; `score` is holographic's float (higher is better). |

Errors (any command):

```json
{"ok":false,"error":"bad_json"|"bad_cmd"|"bad_query"|"engine_error","message":"…"}
```

`bad_query` is the one that matters: the bridge runs a syntax probe
`SELECT 1 FROM facts_fts WHERE facts_fts MATCH ? LIMIT 1` **before** `FactRetriever.search`. A
malformed expression raises `sqlite3.OperationalError` there and is reported as `bad_query`;
holographic's own `_fts_candidates` catches the same error and returns `[]` (measured in the
source), which would silently turn a malformed query into "no hits". The probe restores cpd's `usage`
refusal.

### What a fact holds

Per Context, one holographic DB. Each cpd record projects to exactly one fact:

- `content` = `topic`, `short` and `text` joined by spaces (empty parts dropped). This is the FTS5 and
  Jaccard corpus, matching `context-retrieval-design.md`'s "text + topic + short".
- `category` = the cpd **record id** (`hs-d7`). It is **not** FTS-indexed (measured: `facts_fts`
  indexes `content, tags` only), so it is the mapping back to `context_records` with no ranking
  pollution and no cpd migration.
- `tags` = `""`.

The bridge uses `add_facts_batch`, **never `add_fact`**: `add_fact` merges near-duplicates by
n-gram Jaccard (the engine file's warning: "a cpd restatement could collapse two records");
`add_facts_batch` skips only exact-duplicate content. `index` is an upsert by `category`:
`SELECT fact_id … WHERE category = ?`, then `update_fact` when it exists, else `add_facts_batch`.
If two records in one Context have byte-identical content, the second insert is skipped; the bridge
then retries it with `" [<record>]"` appended, so every record keeps exactly one fact. (The engine DB
is never read for display text — cpd reads `context_records.text`.)

### Interpreter resolution and timeout

| What | Config key | Default (key empty) |
|---|---|---|
| Bridge script | `context_engine_bridge` | `<dir of the cpd binary>/../share/cpd/holographic/bridge.py` |
| Python | `context_engine_python` | `<dir of the bridge>/venv/bin/python` |
| Data dir | `context_engine_dir` | `$CPD_HOME/engines/holographic` |
| Enabled | `context_engine_enabled` | `true` |
| Timeout | `context_engine_timeout_ms` | `30000` |
| Reindex timeout | `context_engine_reindex_timeout_ms` | `600000` |

The two path defaults mirror `Config::pi_adapter_dir` (`core/src/config/file.rs`): an empty key means
"wherever the installer put it", relative to the cpd binary, so no machine-specific path is written.
The interpreter default is the dotfiles venv, `<share>/cpd/holographic/venv/bin/python`; the bridge
default is `<share>/cpd/holographic/bridge.py`. Both may be overridden for tests or a custom install.

### Process lifecycle and no orphans

cpd spawns the bridge for **one operation** (a `context search`, or a `context reindex`), writes the
request(s), reads the responses, closes stdin, and waits. It never leaves a child running. The
implementation wraps `std::process::Child` in a guard that, on **every** exit path — normal, read
error, write error, non-zero exit, protocol error and timeout — calls `kill()` and then `wait()` to
reap; Rust's `Child` does not kill on drop. On timeout the guard kills the child **and** its wait is
bounded, so no zombie survives the command. No port, no socket, no daemon, no lock. A single spawn
carries the whole operation, so an incremental sync + search is still one child.

Lifecycle recommendation, from the measured numbers:

- **Short-lived child per operation. Recommended.** One spawn is **~100–143 ms** (import-dominated);
  the search itself is **1.4–20 ms**. Since the bridge does the staleness sync and the search in the
  *same* child, a search costs one spawn, not two.
- **A resident cpd-supervised child. Rejected.** It would remove the ~100 ms spawn (resident queries
  are 1.4–20 ms), but `cpd context search` is a separate short-lived CLI process: a resident engine
  would need a new IPC channel (socket/port), a supervisor, health checks and teardown on crash, on
  `SIGKILL` and on cpd upgrade — a whole new leak surface to save well under a perceptible amount on
  an interactive read. A resident engine only pays once cpd can talk to it without a spawn.

## Index and staleness

**Decision: `cpd.db` stays the source of truth and the engine is a derived, rebuildable index cpd
projects into lazily, at search time. Writes never call the bridge.** This is Design 1/2 of the
engine file with the projection done on the read path, not the write path.

- **Why not project on every write:** a spawn costs ~100 ms, and every `context decide/note/ask/
  answer` would pay it. The write path is cpd's core; it must not block on a Python child, and a
  projection that fails after a committed write leaves silent drift. **Project-on-write rejected
  (measured spawn cost).**
- **Why not a `cpd context reindex` only:** a user would have to run it after every write, and the
  index would be stale by default. `reindex` stays as the explicit operator command.
- **Why lazy at search time:** the first read after writes pays a small incremental cost and the
  index is never stale when it answers. A search already spawns once; the sync rides that child.

**One DB per Context** at `<context_engine_dir>/<context>.db` (e.g. `building-cpd.db`). This is
forced by the measurement that record ids repeat across Contexts (1,465 records → 1,362 distinct
ids): a single DB keyed by record id would collapse them. It also makes `reset` scoped to one Context
and makes a Context delete a single file removal.

**Watermark.** The bridge keeps its own projection watermark in the engine DB
(`indexed_seq`, `curated_at`; returned by `stats`). The engine is derived, so its bookkeeping belongs
with it; deleting the DB deletes the watermark, and the next search rebuilds. `cpd.db` is unchanged:
**no cpd migration** is needed for the index (the `context_search` FTS5 migration of
`context-retrieval-design.md` is dropped with the table). The `record_curations.curated_at` column
(measured in `20261022T0203-curation.sql`) is the curation clock.

**Freshness at `cpd context search`** (inside the one child):

1. `stats` → `(facts, indexed_seq, curated_at)`; a missing DB returns `(0, 0, null)`.
2. `SELECT max(seq) FROM context_records WHERE context=?` → `want_seq`.
3. `SELECT max(curated_at) FROM record_curations WHERE context=?` → `want_curated`.
4. If `indexed_seq == 0` (first use) → `reset`, then `index` every record; else [`index` the records
   with `seq > indexed_seq`, plus the records whose curation changed
   (`record_curations.curated_at > curated_at`) ], then `index` them.
5. `search`.

Step 4 for `building-cpd` (1,362 records) is the **25.1 s** full build on first use — a one-time
cost per Context. It can be paid ahead of time with `cpd context reindex`; `search` builds it anyway
rather than answer from nothing. Incremental syncs are a handful of records (the same upsert, <1 ms
of engine work plus the ~100 ms spawn already being paid).

**Event coverage.** The index reflects the four events like this:

| Event | What changes in cpd.db | Engine action | Why |
|---|---|---|---|
| **New record** (`context decide/note/ask/answer`) | a new `context_records` row | next search's incremental `index` (`seq > indexed_seq`) | the record's text must be searchable |
| **Curation update** (`topic`/`short`) | `record_curations` `INSERT … ON CONFLICT DO UPDATE`, new `curated_at` | next search's incremental `index`, upsert by record id | `topic`/`short` are in the FTS corpus; the fact's `content` changes |
| **Supersession** (`supersedes`) | nothing on the superseded row; force changes in `inforce.rs` | **none** | force is cpd's, computed live; the engine is told nothing about it (below) |
| **Approved retirement** (`record_retirements`) | a `record_retirements` row (force changes) | **none** | same |
| **Context delete** | the Context's records are removed | the delete path removes `<dir>/<context>.db` (best effort) | the next search sees a missing DB and rebuilds empty |

**Force is cpd's, never the engine's.** The index holds *every* record, in force or not. `cpd context
search` reads the live in-force set from `core/src/context/inforce.rs` and the live kind from
`context_records`, exactly as `context-retrieval-design.md` specifies, and filters engine hits
against them. So a supersession or a retirement is reflected the moment cpd.db records it, with no
projection and no chance of a stale index showing a superseded decision as current. The engine is
never asked whether a record is in force, and it could not answer — holographic has no retirement or
curation model. `cpd context reindex` re-projects every record regardless of force, so `--all`
remains complete.

**Missing or stale index.** Missing software (bridge/interpreter) is a refusal (`engine_missing`,
below), never a rebuild. Missing **data** (DB or watermark absent) is normal: a full rebuild. Stale
(watermark behind `max(seq)` or `max(curated_at)`) is an incremental top-up. A search never answers
from a knowingly stale index.

## Search contract

The agent surface is kept **exactly** from `context-retrieval-design.md` ("Search contract"): the
same command, the same `- [<id>] <short>` markdown line, the same JSON fields, the same refusals,
the same `context show --record <id>` for full text. Only the ranking and the filter mechanics
change, because holographic answers instead of cpd's FTS5.

```
cpd context search <query> [--context <c>] [--limit <n>] [--kind <k,k>] [--all]
cpd context reindex [--context <c>]
```

**What a query matches.** `cpd context search` rewrites the query once, before the bridge sees it,
and the bridge runs the rewritten string as SQLite FTS5 `MATCH` over `content` (= cpd `text` +
`topic` + `short`; `tags` is empty). Punctuation in a word is **text**, not FTS5 syntax (`hs-d655`,
`don't`, `plan/key`); the deliberate operators are kept. The rewrite is the contract below.

The query is trimmed, then scanned left to right and rewritten into the FTS5 `MATCH` string cpd
sends:

- **R1.** Whitespace separates words and is kept as written.
- **R2.** A `"` opens a phrase that runs to the next `"` (a doubled `""` inside is a literal quote).
  The phrase is sent unchanged, and a `*` right after its closing `"` stays (prefix on the phrase).
  A `"` with no closing `"` is sent as written and FTS5 refuses the query (R9).
- **R3.** Outside phrases a word is a run of characters that are neither whitespace nor `"`. Each
  leading `(` of a word opens a group and is sent unchanged; `NEAR(` at the start of a word (after
  those) is the NEAR operator, opens a NEAR group and is sent unchanged. Each trailing `)` of a word
  closes one open group, as many as there are open groups; a `)` with no open group to close stays
  in the word. A `(` inside a word stays in the word.
- **R4.** Inside a NEAR group a `,` ends a word and is sent unchanged (the `, N` distance); anywhere
  else `,` is an ordinary word character.
- **R5.** A word that is exactly `AND`, `OR` or `NOT` (uppercase) is an operator, sent unchanged;
  lowercase `and`, `or`, `not`, `near` are ordinary words.
- **R6.** A bare word, made only of ASCII letters, digits, `_` and non-ASCII characters (at least one),
  with at most one trailing `*`, is sent unchanged (`retriev*` is a prefix match).
- **R7.** Any other word is text: it is wrapped in `"..."`; one trailing `*` stays outside the quotes
  (`pro-d217*` becomes `"pro-d217"*`); a lone `*` or lone punctuation becomes `"*"`, `"-"`.
- **R8.** FTS5 column filters (`col:x`), `-x`, `^x` and `+` are not kept: R7 makes them text.
- **R9.** Still refused `usage` (exit 2, `details: {query, message}`, the query as typed): an empty
  query, an unterminated phrase, an unclosed `(` and a misplaced operator (a leading NOT, a trailing
  AND). No new error code.
- **R10.** The rewrite is idempotent: rewriting its own output changes nothing. The answer's `query`
  field and `details.query` are the query as typed (trimmed), never the rewritten form.

| typed | sent to FTS5 | note |
|---|---|---|
| `hs-d655` | `"hs-d655"` | |
| `db.migrations` | `"db.migrations"` | |
| `plan/key` | `"plan/key"` | |
| `don't` | `"don't"` | |
| `a:b` | `"a:b"` | |
| `first-message digests` | `"first-message" digests` | |
| `retrieval of hs-d655 today` | `retrieval of "hs-d655" today` | |
| `pro-d217*` | `"pro-d217"*` | |
| `retriev*` | `retriev*` | |
| `"first message" OR hs-d655` | `"first message" OR "hs-d655"` | |
| `(hs-d655 OR pro-n210) NOT draft` | `("hs-d655" OR "pro-n210") NOT draft` | |
| `NEAR(plan/key hs-d655, 3)` | `NEAR("plan/key" "hs-d655", 3)` | |
| `f(x)` | `"f(x)"` | |
| `-` | `"-"` | |
| `"unterminated` | `"unterminated` | refused `usage`: FTS5 refuses the unterminated phrase (R9) |
| `(a` | `(a` | refused `usage`: FTS5 refuses the unclosed group (R9) |

- several terms are **ANDed** (all must match) — the old behaviour;
- a trailing `*` is a **prefix** match (`retriev*`) — the old behaviour;
- **there is no stemming.** `facts_fts` uses FTS5's **default** tokenizer (`unicode61`), not
  `porter unicode61`, so `retrieve` does **not** match `retrieval`; the caller uses `retriev*`. This
  is the one honest gap versus the old design (which chose `porter`); cpd does not fight it, it
  documents it: `context-retrieval-design.md`'s `porter`/`term*` line is superseded by "prefix is the
  stemming".
- An empty or whitespace-only query is `usage` (exit 2) and never reaches the bridge. A malformed
  expression is detected by the bridge's probe → `bad_query` → `usage` (exit 2) — not a
  `store_failed`, and not a silent empty result.

**Ranking: holographic's, then cpd's tie-break.** Holographic orders by
`score = (0.4·fts_norm + 0.3·jaccard + 0.3·hrr) × trust_score` descending. Every projected fact has
the default trust 0.5 and temporal decay is off, so the order is FTS5 + Jaccard + HRR relevance,
**best first**, with `score` returned as a float (higher is better). cpd preserves that order and
breaks **exact** ties by `context_records.seq` **descending** (newest first, the direction of
supersession), exactly as `context-retrieval-design.md` broke bm25 ties. cpd does **not** re-rank:
re-ranking would need BM25 over cpd's own table, the index it is deleting. Plainly: the engine's
score is opaque to cpd, and the old `score` (bm25, lower-better) is replaced by holographic's
(higher-better). The field name is unchanged; the semantics are documented.

**Mapping a hit back to a row.** The hit's `record` is the cpd record id (the fact's `category`),
and the fact was indexed under this Context's DB, so cpd reads
`SELECT id, kind, at, seq, text FROM context_records WHERE context = ? AND id = ?`. A hit whose row
is gone (a delete that raced the reindex) is dropped; the next reindex clears it.

**Filtering, `--kind`, `--all`, and the limit.** cpd **over-fetches** and filters, because force and
kind are cpd's:

- `--kind <k,k>`: the selected kinds (`decision,note,question,answer` by default; `plan`/`brief`
  selectable), validated as `context-retrieval-design.md` specifies. cpd filters the mapped rows by
  `kind`; the engine is not told (its `category` is the record id, not the kind).
- Force: engine hits are filtered against the live in-force set (`inforce.rs`) unless `--all`. An
  out-of-force hit is labelled exactly as `context show` labels it
  (`(superseded by <id>)`, `[retired (<reason> <target>)]`); `plan`/`brief` are never in force.
- Over-fetch: cpd asks the engine for `N = min(total_facts, max(limit·5, 80))` hits, filters, and
  takes the first `limit`. If fewer than `limit` survive **and** the engine returned exactly `N`
  (it truncated), cpd retries once with `N·5`, capped at `total_facts`; it never loops unboundedly.
  Hidden hits never count against the limit, which is the old rule.
- `--limit <n>`: default **20**; `--limit 0` means **no limit**, implemented as `N = total_facts`
  (the engine has no "all"; the Context is ~1.5 k rows, so asking for all is one ~14 ms query).
  Negative or non-integer is `usage`.
- `--all` is **not** a limit; it is the in-force switch, kept separate, as before.

**Result** is the frame of `context-retrieval-design.md`, unchanged in shape:
`{"ok":true,"context":{…},"query":…,"records":[{id,short,kind,at,seq,score,in_force}],"markdown":"…"}`.
`short` is `record_curations.short` else `Record::first_line()` at ≤120 bytes; `score` is now
holographic's float; `in_force` is the same. `records` and `markdown` are empty on no hits (not an
error).

**`cpd context reindex [--context <c>]`** is the explicit operator command: resolve the Context like
`context show`, `reset` the Context's DB, `index` every record, and record the watermark. It is a
write to the derived index only; it changes no cpd record and appends no Event. It is what dotfiles
or the User runs once so the first search is warm.

## Config and failure

New config keys, parsed once in `Config::parse` (`core/src/config/file.rs`), each with a built-in
default and a recorded `Source` (the existing `Reader::get` pattern):

| Key | Type | Default | Reason |
|---|---|---|---|
| `context_engine_enabled` | bool | `true` | operator kill switch; when false `search`/`reindex` refuse `engine_disabled` without a spawn |
| `context_engine_bridge` | string (path) | `""` | empty = beside the binary, `<bin>/../share/cpd/holographic/bridge.py`; mirrors `pi_adapter` |
| `context_engine_python` | string (path) | `""` | empty = `<bridge dir>/venv/bin/python`, the dotfiles venv |
| `context_engine_dir` | string (path) | `""` | empty = `$CPD_HOME/engines/holographic` |
| `context_engine_timeout_ms` | integer | `30000` | one search/sync spawn is ~0.1 s; 30 s bounds a hung child |
| `context_engine_reindex_timeout_ms` | integer | `600000` | a full 1.5 k rebuild is ~25 s; 10 min bounds a slow machine |

New `ErrorCode`s (`core/src/error.rs`):

- `engine_missing` (exit **4**, NotFound): the bridge, the interpreter, or the engine package is
  absent. `details = {bridge, python, hint}`. **This is the answer to "engine missing": a clear
  refusal, not a rebuild** — cpd cannot build what is not installed.
- `engine_disabled` (exit **3**, Refused): `context_engine_enabled = false`. `details = {key}`.
- `engine_failed` (exit **1**, Failure): the bridge exited non-zero, returned `{"ok":false,
  "error":"engine_error"}`, printed no answer, hit the timeout, or wrote bad JSON.
  `details = {message}` (and `timeout_ms` on a timeout). The child is killed and reaped on every one
  of these paths.
- `bad_query` is **not** a new code: it maps to the existing `usage` (exit 2) with
  `details = {query, message}`, preserving `context-retrieval-design.md`'s refusal table.

Missing **data** (the DB or the watermark) is **not** an error: it is the rebuild in "Index and
staleness". The distinction is explicit: **software missing → `engine_missing`; data missing →
rebuild.**

**`cpd doctor`** gains one check, following the house pattern (`engine/src/doctor/`, name + severity,
the 10 s probe deadline): **`holographic`** (`Severity::Error`). It resolves the bridge and
interpreter, runs `ping` under `context_engine_timeout_ms`, and reports the engine version. When
`context_engine_enabled=false` it passes with "disabled". A missing bridge/interpreter is a failure
with the same `hint` as `engine_missing`. It does not read or build the index (doctor is read-only;
building is `reindex`). The existing `cli/tests/engine/doctor.rs` `NAMES` list grows by one (the
build Job owns that edit).

## Install (dotfiles)

**The bridge lives in cpd, installed by cpd's installer; dotfiles installs only the external
dependency.** Concretely:

**cpd repo.**

- `contrib/holographic/bridge.py` — the authored bridge.
- `scripts/install-cpd.sh` installs it to `~/.local/share/cpd/holographic/bridge.py`, the same way
  it installs the pi adapter to `~/.local/share/cpd/pi` (atomic `cp` + `mv`). It does **not** touch
  `~/.local/share/cpd/holographic/venv/`, which dotfiles owns.
- `cpd` resolves it by default as `<dir of cpd>/../share/cpd/holographic/bridge.py` and
  `<bridge dir>/venv/bin/python`.

**dotfiles repo.** Real paths, as they are today:

- New pin file `home/.chezmoidata/holographic.yaml` (one place, like `nextest.yaml`):
  ```yaml
  holographic:
    version: "0.16.0"
    commit: "166ba35c5dd6973956dabee97165fc4eb0750b20"
    numpy: "2.5.4"
    python: "3.12"
  ```
- New script `scripts/holographic-install.sh <commit> <numpy> <python>`:
  ```
  uv venv --python <python> ~/.local/share/cpd/holographic/venv
  uv pip install --python ~/.local/share/cpd/holographic/venv/bin/python \
    "numpy==<numpy>" \
    "holographic-memory @ git+https://github.com/bysc1000/holographic-memory@<commit>"
  ```
  It installs **from the pinned git commit**, never `pip install holographic-memory` (the PyPI name
  is a different, non-commercial project — measured above). It then verifies
  `python -c 'from importlib.metadata import version; assert version("holographic-memory")=="<version>"'`
  and `import numpy`.
- New `home/.chezmoiscripts/run_onchange_after_74-holographic.sh.tmpl` runs the script, gated on
  `has "cpd" $m.modules`, ordering it **before** `75-cpd`.
- `scripts/doctor.sh` gains a check: the venv's python exists, `import holographic_memory, numpy`
  succeeds, and the versions are the pins.
- It **does not** install the bridge (cpd's installer owns it) and does **not** write
  `~/.cpd/config.toml` keys (every key has a built-in default; the defaults need no file entry).

**Order.** The cpd PR lands first (it defines the path and installs the bridge); the dotfiles PR
then installs the venv. The dotfiles venv install is independent and can be developed in parallel,
but its health check (`doctor.sh`) refers to the cpd-installed bridge path.

**Version/health check.** `cpd doctor`'s `holographic` check (ping + version) is the cpd-side check;
`scripts/doctor.sh`'s venv check is the dotfiles-side one. Both are read-only.

## PR cut

**PR 1 — cpd: the bridge and the command.** One repo, one intent: cpd can search its Context through
holographic.

- `contrib/holographic/bridge.py` (protocol of "Bridge"); `scripts/install-cpd.sh` installs it.
- `core/src/context/search.rs` and `core/src/context/reindex.rs`, subcommands `Search`/`Reindex` in
  `core/src/context/mod.rs`; the over-fetch/filter/`inforce` mapping and the markdown of
  `context-retrieval-design.md` ("Search contract").
- The engine client (spawn + guard + timeout + JSON lines) and a `ContextEngine` trait so the command
  is tested in Rust without a process.
- Config keys and the three `ErrorCode`s; the `cpd doctor` `holographic` check.
- **Tests (no real holographic).** A `ContextEngine` fake in `core/src/fakes/` covers mapping,
  over-fetch, kind/force filtering, `--all`, `--limit 0`, ties and no-hits. A `cli/tests/` test
  drives a **fake bridge** — a stub script the test writes (the existing `write_executable` helper) —
  that answers the JSON lines and records the requests, covering: an incremental `index` before
  `search` after a new record; `bad_query` → `usage`; a non-zero exit and a hang → `engine_failed`
  with the child killed; a missing bridge → `engine_missing`; `reindex` sending `reset`+`index`.
  Never a real engine, never Python's `holographic_memory`. A Python test of the bridge's framing,
  upsert decision and `MATCH` probe runs with a **stub `holographic_memory` module** on `sys.path`,
  so it too imports no external engine.
- Needs: nothing. Order: first.

**PR 2 — dotfiles: the install.** `home/.chezmoidata/holographic.yaml`,
`scripts/holographic-install.sh`, `home/.chezmoiscripts/run_onchange_after_74-holographic.sh.tmpl`,
and the `scripts/doctor.sh` check. Needs: PR 1 merged (the bridge path it verifies), and its own
`tests/` (the repo's pytest suite) exercise the install script's argument/version checks without
network by pointing at a fake `uv` on `PATH` (the dotfiles test pattern).

No migration, no Event kind, no protocol change: `cpd.db` is untouched; `PROTOCOL` is unchanged.
`context-retrieval-design.md`'s digest removal and first-message work are unaffected and stay.

## Open questions

none

The engine choice (holographic) was the User's, and the remaining questions of
`context-memory-engine.md` — a second runtime (Python), disk/RAM, and who owns the engine's
lifecycle — are answered here: the runtime is a pinned venv installed by dotfiles, the measured
footprint is 15.5 MB disk / 34.5 MB one-shot RSS, and cpd owns the lifecycle as a short-lived child
with no daemon. Nothing in this design needs the User.
