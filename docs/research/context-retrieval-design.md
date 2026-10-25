# On-demand Context retrieval: the design of record

Design for Job `design-docs`, Plan `context-on-demand`. It closes the open
questions of [`context-memory.md`](context-memory.md) (Job `memory-docs`) and
defines the retrieval cpd builds: **on-demand search over cpd's own store, built
by cpd, with SQLite FTS5 and no external memory library**. It writes no code;
the build Jobs are written from it.

Every number below carries its method; anything not measured is labelled
`unverified` in the same sentence. The findings file is cited as *findings*.

## FTS5 under cpd's SQLite

The findings measured the system `sqlite3` CLI 3.45.1 and left the bundled
library's plan and timings `unverified` (findings, "Method", "Open questions").
This closes that hedge with the library cpd actually ships.

**Method.** A scratch cargo crate at `/tmp/ctx-search-spike` **outside the
repo**, `rusqlite = { version = "0.40", features = ["bundled", "functions"] }`
(libsqlite3-sys 0.38.2, the same versions the workspace pins in `Cargo.toml`),
built `--offline` from the warm registry. It opened a byte copy of the live
`$CPD_HOME/cpd.db` (`contexts` 4, `context_records` 1,452, `record_curations`
1,009; Context `building-cpd` 1,351), created the proposed FTS5 table and
triggers on the copy, back-filled from the real rows, ran `EXPLAIN QUERY PLAN`,
and timed a dozen real queries 21 times each (min/median) under the proposed
predicate (`--context`, the default kinds, `LIMIT 20`). It then exercised the
triggers against the real write path. The counts are higher than the findings'
1,446/1,002 because the store grew. The scratch crate is **not** committed.

**SQLite version.** `select sqlite_version()` from the bundled library returned
`3.53.2` — the same `libsqlite3-sys` cpd pins. FTS5 is compiled in (the findings
verified 289 `fts5*` symbols).

**Query plan** (`EXPLAIN QUERY PLAN`), identical for all three shapes below:

```
SCAN context_search VIRTUAL TABLE INDEX 0:M3
SEARCH r USING INTEGER PRIMARY KEY (rowid=?)
USE TEMP B-TREE FOR ORDER BY
```

The FTS5 index drives; cpd's `context_records` is joined by `rowid`
(`context_search.rowid = context_records.seq`). The temp B-tree is for
`ORDER BY bm25(...)`, which SQLite/FTS5 cannot satisfy from an index; at ~1.5k
matches it is the whole cost and it is sub-millisecond (below).

**Timings** (contentless-delete table; the external-content and regular tables
gave the same plan and the same timings to 0.01 ms, so only one is shown).
`hits` is the row count before cpd's in-force filter:

| `MATCH` query | hits | best | median |
|---|---:|---:|---:|
| `decision` | 20 | 0.10 ms | 0.10 ms |
| `retriev*` | 1 | 0.06 ms | 0.06 ms |
| `context` | 20 | 0.32 ms | 0.33 ms |
| `engine` | 20 | 0.29 ms | 0.29 ms |
| `worker` | 20 | 0.23 ms | 0.23 ms |
| `migration` | 20 | 0.19 ms | 0.19 ms |
| `store` | 20 | 0.19 ms | 0.19 ms |
| `search` | 14 | 0.04 ms | 0.04 ms |
| `record` | 20 | 0.18 ms | 0.18 ms |
| `fts5` | 0 | 0.01 ms | 0.01 ms |
| `"query plan"` | 0 | 0.03 ms | 0.03 ms |
| `retrieval OR curation` | 20 | 0.06 ms | 0.06 ms |

`context search` is therefore far below any perceptible cost at cpd's size. The
findings' FTS5 build (21 ms over the same text) is consistent with the bundled
library's back-fill (78 ms for three tables, ~26 ms each). (`unverified`: the
findings' `LIKE`-scan timings were not re-measured here; they are not part of
this design's decision.)

**Back-fill.** One `INSERT … SELECT` from `context_records LEFT JOIN
record_curations` over 1,452 rows built all three tables in **78 ms** total (a
one-time migration cost). Building the index once and querying it, not
rebuilding per row, is the lesson of hs-n480 (findings, "Recommendation").

**Storage** (`dbstat`, page 4,096 B; per shape, over the same real rows):

| Shape | Shadow tables | Pages | Bytes |
|---|---|---:|---:|
| contentless-delete (`content=''`) | data, idx, docsize, config | 124 | 507,904 |
| external content (`content='…'`) | data, idx, docsize, config | 123 | 503,808 |
| regular (internal content) | data, idx, **content**, docsize, config | 400 | 1,638,400 |

The regular table stores a second copy of every indexed column (`_content`, 277
pages, ~1.1 MB); contentless and external store only the inverted index
(~0.5 MB). FTS5 shadow tables are ordinary `sqlite_schema` tables with names
that do not start with `sqlite_` (verified: `context_search_data`,
`context_search_idx`, `context_search_docsize`, `context_search_config`); there
is no `_content` shadow table for a contentless table.

**Trigger sync against cpd's real write path** (run on the copy with the real
`context_records`/`record_curations` history triggers in place, `cpd_txn()`
stubbed): a `context_records` insert is found by `MATCH`; a `record_curations`
insert and update change the matching `topic`; a curation delete under a
tombstoned Context clears the topic while the record stays findable; a
`context_records` delete removes it. One finding shaped the DDL: a
contentless-delete table refuses `UPDATE` of a **subset** of columns
(`cannot UPDATE a subset of columns on fts5 contentless-delete table`), so the
curation triggers set all three indexed columns (see below).

**Choice: a contentless-delete FTS5 table.** The findings recommended an
external-content table. The design refines that to `content=''` +
`contentless_delete=1`, for three reasons, all measured:

1. The corpus spans two tables (`context_records.text` and
   `record_curations.topic`/`short`). An external-content table must name one
   content table or a view; its `delete` commands then need the old column
   values, and a Context delete removes `context_records` rows, so the trigger
   would depend on the deletion order of two base tables. A contentless table
   deletes and updates **by rowid**, independent of the base rows' order.
2. It stores no second copy of the text (508 KB vs 1.64 MB for the regular
   table), matching the findings' "index adds storage roughly the size of the
   indexed text".
3. It is the same plan and the same sub-millisecond timings as the other two,
   so nothing is traded away.

The one constraint — `UPDATE` must name every indexed column — is met by the
curation triggers below. `contentless_delete=1` needs SQLite ≥ 3.43; cpd bundles
3.53.2. `snippet()`/`highlight()` are unavailable (they need content) and are not
used; `SELECT count(*)` on a contentless table is 0 and is not used.

## Search contract

```
cpd context search <query> [--context <c>] [--limit <n>] [--kind <k,k>] [--all]
```

A **read**: no write, no Event, idempotent, like `context show` (findings,
"Command, shape, order, limit"). The command is `core/src/context/search.rs`,
the subcommand `Search` in `core/src/context/mod.rs`.

**`<query>`** — one FTS5 MATCH expression over the index. Default semantics are
FTS5's: several terms are ANDed (all must match); a trailing `*` is a prefix
match (`retriev*`); the FTS5 operators `OR`, `NEAR`, `NOT`, quoted phrases and
parentheses are accepted as written (SQLite FTS5, <https://sqlite.org/fts5.html>).
An empty or whitespace-only query is refused `usage` (exit 2,
`details: {query: ""}`); a malformed MATCH expression is refused `usage` (exit 2,
`details: {query, message}`), never a `store_failed`.

**`--context <c>`** — defaults like `context show` (`Select::resolve`): the named
Context, else the selected Plan's Context (`--plan`/`CPD_PLAN`). No Context and
no selected Plan is `context_required` (exit 2); a named Context that does not
exist is `context_not_found` (exit 4, `details: {context}`), the same code
`context show` gives.

**`--limit <n>`** — an integer `n >= 0`; default **20**, matching cpd's existing
"at most 20" idiom for first messages and turn-start updates. `--limit 0` means
**no limit**: every hit (after the in-force/kind filters) is returned. A negative
or non-integer value is `usage` (exit 2). `--all` is **not** a limit; it is the
in-force switch, kept separate (explicit over implicit).

**`--kind <k,k>`** — a comma-separated list of record kinds. The default set is
`decision,note,question,answer` (`plan` and `brief` are not searched by default;
findings, "Recommendation"). Any of the six kinds may be named, so `--kind plan`
and `--kind brief` **are** selectable: the corpus is complete and a caller who
wants an old Plan record can ask for it. A kind outside the six, or an empty
list, is refused `usage` (exit 2, `details: {kind, allowed: [decision, note,
question, answer, plan, brief]}`). `--kind` replaces the default set; it does not
add to it.

**In-force filtering and `--all`.** The four in-force kinds are hidden when the
record is out of force: superseded by a later record's `supersedes`, or taken out
by an approved `record_retirements` row (contract §4.6 "Records in force",
"Retirement"). `--all` includes them, labelled (below). `plan` and `brief`
records are never in force (contract §4.6), so the in-force filter does not touch
them; their `in_force` is `false`. The in-force set is computed by the existing
`core/src/context/inforce.rs` (`in_force_without` on the Context's records plus
`retired`), not re-derived in SQL, so there is one rule for force.

**Ordering.** FTS5 `bm25(context_search)` ascending (best first); ties broken by
`context_records.seq` **descending** (newest first), the same direction as
supersession. Relevance decides; recency separates equals. Implemented in one SQL
`ORDER BY bm25(context_search) ASC, r.seq DESC`; the in-force/`--kind` filters run
in SQL/Rust over the ordered rows, then the limit is applied, so a hidden
out-of-force hit never counts against the limit.

The search SQL, exactly (the FTS5 table is **not** aliased: `MATCH` must name the
table, verified — an alias gives `no such column`):

```sql
SELECT r.id, r.kind, r.at, r.seq, bm25(context_search) AS score
  FROM context_search
  JOIN context_records r ON r.seq = context_search.rowid
 WHERE context_search MATCH ?1
   AND r.context = ?2
   AND r.kind IN (<the selected kinds>)
 ORDER BY score ASC, r.seq DESC
```

No weighting: all three indexed columns count equally (the FTS5 default). There
is no measurement to justify a weight, so none is added (YAGNI).

**Result fields.** One JSON value, matching `context show`'s frame:

```json
{"ok": true,
 "context": {"id": "building-cpd", "title": "Building cpd", "jira": null},
 "query": "retriev*",
 "records": [{"id": "hs-d7", "short": "one engine per machine", "kind": "decision",
              "at": "2026-10-14T21:12:00.000Z", "seq": 42, "score": -4.61, "in_force": true}],
 "markdown": "- [hs-d7] one engine per machine"}
```

- `short` is the record's `record_curations.short` when it has a curation,
  otherwise `Record::first_line()` (the rule `context show` already uses to list
  a brief) with runs of whitespace collapsed to one space and truncated to at
  most **120 bytes** on a UTF-8 boundary (the curation limit, findings,
  "Result shape"). It is a pointer, never the full text.
- `score` is the raw `bm25()` value (a float; lower is better, so ties are
  broken by `seq`).
- `in_force` is `true` for an in-force `decision`/`note`/`question`/`answer` and
  `false` for a superseded or retired one and for every `plan`/`brief` (which
  are never in force).

**Markdown line.** One line per hit, joined by `\n`; the full text is always
`cpd context show --record <id>`:

- in force: `- [<id>] <short>`
- superseded: `- [<id>] <short> (superseded by <successor id>)`
- retired: `- [<id>] <short> [retired (<reason> <target>)]`

The superseded/retired labels are exactly the ones `context show` puts on the
same record (contract §4.6 "Retirement"; `core/src/context/render.rs`), so a
record reads the same wherever it is shown.

**No hits is not an error:** `records` is empty and `markdown` is empty
(findings, "Command, shape, order, limit").

**Refusals** (all with `ok:false`, `error.code`, `details`):

| Condition | Code | Exit |
|---|---|---|
| empty/whitespace `<query>` | `usage` | 2 |
| malformed FTS5 expression | `usage` | 2 |
| no `--context` and no selected Plan | `context_required` | 2 |
| `--context` names no Context | `context_not_found` | 4 |
| unknown or empty `--kind` | `usage` | 2 |
| `--limit` negative or not an integer | `usage` | 2 |

`context search` never writes: it appends no Event and touches no table.

## Index and migration

**Table.** One FTS5 table `context_search`, contentless-delete, `porter unicode61`
stemming and case-folding (findings, "Retrieval for cpd"; `porter` covers
unfinished words with `term*` without a model):

```sql
CREATE VIRTUAL TABLE context_search USING fts5(
  text, topic, short,
  tokenize = 'porter unicode61',
  content = '',
  contentless_delete = 1
);
```

Its rowid is `context_records.seq` (`seq` is an `INTEGER PRIMARY KEY`, a rowid
alias), so the join is a primary-key lookup and no surrogate key is invented.

**Back-fill.** The migration fills it from the real rows; a contentless table
cannot `rebuild`, so the fill is explicit:

```sql
INSERT INTO context_search(rowid, text, topic, short)
  SELECT r.seq, r.text, k.topic, k.short
    FROM context_records r
    LEFT JOIN record_curations k ON k.context = r.context AND k.record = r.id;
```

**Triggers.** `context_records` is append-only (`context_records_immutable`
refuses every `UPDATE`) but a `context delete` removes its rows;
`record_curations` is inserted and updated by the distiller's `propose`
(`INSERT … ON CONFLICT … DO UPDATE`) and removed only under a deleted Context.
Five triggers keep the index in sync, written by the migration itself (hand-owned,
like `pull_requests_owned`, **not** generated by the reconcile — they are not
`history_tables` rows):

```sql
CREATE TRIGGER context_search_records_ins AFTER INSERT ON context_records BEGIN
  INSERT INTO context_search(rowid, text, topic, short)
  VALUES (NEW.seq, NEW.text,
    (SELECT topic FROM record_curations WHERE context = NEW.context AND record = NEW.id),
    (SELECT short FROM record_curations WHERE context = NEW.context AND record = NEW.id));
END;

CREATE TRIGGER context_search_records_del AFTER DELETE ON context_records BEGIN
  DELETE FROM context_search WHERE rowid = OLD.seq;
END;

CREATE TRIGGER context_search_curations_ins AFTER INSERT ON record_curations BEGIN
  UPDATE context_search
     SET text  = (SELECT text FROM context_records WHERE context = NEW.context AND id = NEW.record),
         topic = NEW.topic,
         short = NEW.short
   WHERE rowid = (SELECT seq FROM context_records WHERE context = NEW.context AND id = NEW.record);
END;

CREATE TRIGGER context_search_curations_upd AFTER UPDATE ON record_curations BEGIN
  UPDATE context_search
     SET text  = (SELECT text FROM context_records WHERE context = NEW.context AND id = NEW.record),
         topic = NEW.topic,
         short = NEW.short
   WHERE rowid = (SELECT seq FROM context_records WHERE context = NEW.context AND id = NEW.record);
END;

CREATE TRIGGER context_search_curations_del AFTER DELETE ON record_curations BEGIN
  UPDATE context_search
     SET text  = (SELECT text FROM context_records WHERE context = OLD.context AND id = OLD.record),
         topic = NULL,
         short = NULL
   WHERE rowid = (SELECT seq FROM context_records WHERE context = OLD.context AND id = OLD.record);
END;
```

Why this stays correct under cpd's write path:

- There is no `context_records` update trigger, because an update is impossible.
- The curation triggers set **all three** columns: a contentless-delete table
  refuses a subset `UPDATE` (measured). If the record was already deleted
  (`context delete` deletes `context_records`; curation rows can only be deleted
  once the Context is a tombstone), the rowid subquery finds no FTS row and the
  statement is a no-op; the record's FTS row is already gone via
  `context_search_records_del`. Deleting the curation while the record remains
  clears `topic`/`short` and keeps the text searchable.
- The index is a derived artifact of the two tables, maintained by the database.
  No Rust write path changes: `Contexts::write`, `propose` and `context delete`
  keep writing only their own tables, which is the DRY choice and the reason
  FTS5 was chosen over a scan.

**Migration.** One new file, `core/src/store/migrations/<YYYYMMDDTHHMM>-context-search.sql`,
made with `scripts/new-migration.sh context-search breaking`. `build.rs`
auto-discovers `migrations/*.sql` and lists it by name (contract §4.5 N4): no
shared line is edited; `scripts/sync-version.sh` (run by the script) updates the
derived version. It is pure SQL (no `.rs`).

**It must be `breaking`, not `additive`** — this refines the findings' assumed
`additive`. Two independent reasons:

1. It creates triggers on the **existing** tables `context_records` and
   `record_curations`. Contract §4.5 A1 makes "a new or changed trigger on an
   existing table" breaking, and the additive test
   (`core/tests/migrations.rs`, `an_additive_migration_may_not_add_a_required_column_a_unique_index_or_a_trigger_on_an_existing_table`)
   refuses exactly that (`is on existing table t`).
2. `CREATE VIRTUAL TABLE` is not one of A1's allowed statements. The additive
   authorizer allows `CreateTable`, `CreateIndex`, `CreateTrigger`,
   `AlterTable`, `Insert` and reads, and branches the rest to an error; a
   `CREATE VIRTUAL TABLE` produces `AuthAction::CreateVtable`, which is denied.

So `scripts/new-migration.sh context-search breaking`; `after <n>` is the number
of migrations that sort before it — **49 at this base** (14 legacy + 35 named,
`ls core/src/store/migrations/*.sql | wc -l` = 49); the script recomputes it, and
a rebase must rerun the script (contract §4.5 O3). A breaking migration is a
barrier and closes the compatibility window for this feature, which is correct:
an old binary must not write the store without the triggers.

**Schema hash.** `scripts/repin.sh` pins `SCHEMA_HASH` once when the migration
lands, and the mechanics are confirmed:

- `canonical_entries` (`core/src/store/schema.rs`) reads `sqlite_schema` rows
  `WHERE substr(name, 1, 7) <> 'sqlite_'` and hashes `[type, name, sql]` (a table
  parsed into columns/constraints/options). FTS5's shadow tables do **not** start
  with `sqlite_` (measured names above), so they are entries of that hash, as
  `table` rows with their FTS5-generated `CREATE TABLE` text;
  `context_search` itself is a `table` row with its `CREATE VIRTUAL TABLE` text.
- Only catalog text is hashed, never a row: `canonical_entries` never reads a
  shadow table's data. So the hash does not change when records are written, when
  the index grows, or with the order in which additive migrations reached a store
  — it depends only on which migrations were applied.
- Both a fresh store that runs the migration and an existing store that applies
  it end with the same FTS5 shadow schema, so the canonical hash matches
  (contract §4.5 "The migrating transaction": the schema check runs inside the
  migrating transaction and rolls it back on a mismatch). `scripts/repin.sh` runs
  the ignored `prints_the_schema_hash` test and writes the new value, once.
- Caveat (labelled): the shadow tables' `CREATE` text is generated by the bundled
  SQLite. A future `libsqlite3-sys` upgrade that changes FTS5's internal shadow
  schema would change the hash and require a repin of the affected stores; that is
  the existing rule for every schema object the bundled library generates, not a
  per-build event. `unverified`: no such upgrade was exercised here.

**No Event kind, no protocol change.** The migration adds no `event_kinds` row and
changes no Event `data` field, so `PROTOCOL` (contract §5.2) is unchanged; only
the derived `0.<PROTOCOL>.<migrations>` version moves.

## Digest removal

hs-d655 drops the pushed digest. The design removes it, rather than keeping it as
a debug view (delete first; the on-demand commands cover its role). Item by item:

| Item (current location) | Decision | What replaces it |
|---|---|---|
| `context show --digest` (`ShowArgs.digest`, `show::show_digest`, commands.md §3.3) | **removed** | `context search` for discovery; `context show --topic` for a whole topic. `context show` keeps `--since`, `--record`, `--topic`. |
| `context_recent_days` (config §8.2, `core/src/config/file.rs`) | **removed** | nothing: it only defined digest relevance. A `config.toml` that still sets it is `config_invalid` (unknown key), as any removed key (precedent: `pr_open_escalate_minutes`, contract §4.5). Upgrade note: `~/.cpd/config.toml` must not keep the line; the default was 1. |
| `digest::render` (`Digest`, `LIMIT`, `Curated`, `Loaded`, `load`, `live_plans`, `brief_citations`) | **removed** | nothing. `load` and the curation read are shared with `by_topic`/`topics`, so they move (below), not delete. |
| `digest::first_message` (the "Consumers" renderer) | **replaced** | the first message is header + the Lead's brief (if any) + one pointer line; no store read. |
| `digest::manager_message` | **replaced** | header + pointer line; no store read. |
| `digest::by_topic` / `digest::topics` | **kept** | moved out of `digest.rs` into a small curation-view module (`core/src/context/curated.rs`), same SQL and behaviour. `context show --topic` is unchanged: `- [<id>] <short>` lines, the same `topic_not_found` (exit 4) with `details.topics`. |
| the ≤20 raw uncurated records in a first message (`NEWER_MAX`, the `and K more` line) | **removed** | `cpd context show --since <seq>` and `cpd context search`. The records stay in the store and stay findable; "nothing is lost" now means "retrievable", not "pushed". |

Consequences the build Jobs own: delete `core/src/context/digest.rs`'s digest
half (keep `by_topic`/`topics`); drop `ShowArgs.digest` and `show_digest`; drop
the `context_recent_days` field, default and validation from
`core/src/config/file.rs` and the contract §8.2 table; rewrite contract §4.6
"The Context digest" and "Consumers" (the first message is header + brief +
pointer; `context show --digest` is gone; `context show --topic` stays); delete
commands.md §3.3's `--digest` paragraph; update the cpd-manager and cpd-lead
skills' "first message holds the digest ... at most 20 uncurated records" text;
drop the digest tests and add search tests. `context show --topic` and its tests
stay green.

`record_curations` is not dropped: the curator's `topic`/`short` still drive
`context show --topic`, the search index and the turn-start briefs; only the
digest reader goes.

## First messages

hs-d655: nothing is pushed but the agent's own brief (a Lead) or a pointer (a
Manager); the axioms stay in `AGENTS.md`. Both messages end with one newline and
are joined internally by one blank line (`\n\n`), exactly as
`core/src/plan/new.rs` and `core/src/context/manager.rs` write them today.

**A fresh Lead** — the existing header (`plan::header`), then the brief in full
(trimmed) when `plan new --brief-file` wrote one, then the pointer line. The
brief is the Manager's/User's; it is the only Context content pushed, and it is
already the Lead's own. Exact text (placeholders `<id>`, `<title>`, `<c>`,
`<brief>`):

```
You are the Lead of Plan <id> "<title>" in Context <c>. Phase grounding: follow the cpd-lead skill.

<brief, trimmed of trailing whitespace>

Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>
```

Without a brief, the middle block is omitted:

```
You are the Lead of Plan <id> "<title>" in Context <c>. Phase grounding: follow the cpd-lead skill.

Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>
```

A Lead's pane carries `CPD_PLAN`, so the pointer omits `--context` (the same
`--plan`/`CPD_PLAN` default `Select::resolve` gives every Context command).

**A fresh Manager** — the existing header, then the pointer. It has **no brief**.
Its pane has no `CPD_PLAN`, so the pointer names the Context explicitly. Exact
text:

```
You are the Manager of Context <c> "<title>". Follow the cpd-manager skill.

Context on demand: cpd context search <query> --context <c> · cpd context show --context <c> --since <seq> · cpd context show --context <c> --record <id>
```

**No bounded seed for the Manager.** The Manager gets header + pointer only, not
the optional "open questions in force + active Plans with phase" seed. Reasons:

1. The seed would be a second, staler render of exactly what the Manager's
   **turn-start message** already computes fresh at its first turn (new records
   since its cursor, a `Plans:` section and a `Pending work:` section; contract
   §15.7, cpd-manager skill "The turn-start message"). Duplicating it in the
   first message violates DRY and "recency wins": the first message is written
   once at registration and would drift.
2. The pointer names the on-demand reads (`cpd context search`, `cpd context
   show`, and the skill's `cpd plan list`/`cpd context pending`), so a Manager
   that wants the shape reads it at the moment it acts.
3. It matches hs-d655: nothing Context-wide is pushed, and the digest (the old
   seed's source) is gone.

This closes the findings' open question ("Whether the fresh-Manager seed is
needed"): no seed; the turn-start message is the seed.

## Open questions closed

Each question from *findings*, "## Open questions", with its answer:

1. **`holographic` is ambiguous.** Resolved: the design reads it as the Hermes
   Holographic Memory provider, `bysc1000/holographic-memory` (GitHub REST API,
   read 2026-10-26: description "Hermes Holographic Memory — local-first
   persistent memory for AI coding agents", 9★, MIT, pushed 2026-06-11). A GitHub
   search for "holographic memory" also returns `NeoVertex1/nuggets` ("First AI
   assistant with holographic memory", 399★) and `writerslogic/holographic-memory`
   (VSA/BSC for Node.js, 21★, Apache-2.0), so the name is inherently ambiguous in
   the User's shorthand. The design is unaffected: cpd takes FTS5 as the
   mechanism and builds on its own store, so even if the User meant a different
   `holographic`, no decision changes. The findings' row stands for the Hermes
   provider; the others are noted, not surveyed here.
2. **OpenMemory's classic behaviour is unverified.** Still `unverified`, now with
   a dated source: `mem0ai/openmemory` was **created 2026-07-06** and its GitHub
   description is "Open-source CLI & TUI to port AI coding sessions across Claude
   Code, Codex, and OpenCode" (99★, MIT, pushed 2026-07-29; GitHub REST API, read
   2026-10-26), and `docs.mem0.ai/llms.txt` (538 lines, read 2026-10-26) contains
   no "openmemory" mention. The classic local mem0 server's storage and search
   are not in a current source, so they stay unverified; the decision never rests
   on them (cpd builds, not adopts).
3. **Bundled-SQLite query plans.** Closed by measurement above: bundled SQLite
   **3.53.2**, plan `SCAN context_search VIRTUAL TABLE INDEX 0:M3 / SEARCH r
   USING INTEGER PRIMARY KEY (rowid=?) / USE TEMP B-TREE FOR ORDER BY`, twelve
   real queries at 0.01–0.33 ms.
4. **Corpus choice.** Decided: one contentless FTS5 table over `text`, `topic`,
   `short`; the `short` is a *pointer*, not indexed against the record id — the
   record id is the FTS `rowid` (its `seq`), so a hit already carries its id, and
   a searchable id would add false hits. `plan`/`brief` are **selectable** with
   `--kind`, not in the default set. One table, three columns, all indexed with
   equal weight.
5. **Whether the fresh-Manager seed is needed.** Decided: no seed; header +
   pointer only, because the turn-start message already delivers the bounded,
   fresh view (above).
6. **Digest removal mechanics.** Decided per item in "Digest removal": remove
   `--digest`, `context_recent_days`, `digest::render` and the raw list; replace
   `first_message`/`manager_message` with header/brief + pointer; keep
   `by_topic`/`topics` so `context show --topic` keeps working.

## Remaining for the User

none
