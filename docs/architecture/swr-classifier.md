# SWR for the classifier: the Lead completion badge and endpoint health

Status: **normative for the Jobs `swr-classifier-tests` and `swr-classifier-impl`** (Plan `external-sync-arch`, Workflow `swr-classifier`). Where merged code later differs from this page, the code wins. The shared design is [swr.md](swr.md); the schema is [contract §4.4](../contract.md); the classifier port is [contract §3.7a](../contract.md); the commands are [commands §32](../commands.md).

Decisions: pro-d438 (the badge is always visible and keeps its last value), pro-d445 (the hover card of the other scores), pro-d448 (all sources, one DAG), pro-d449 (cpd is the backend; clients call cpd). Accepted assumptions of [swr.md §17](swr.md) apply; the ones this page relies on are A1, A3, A4, A6 and A7.

## 1. Scope

Two halves, one table family:

- **(A) The Lead completion badge.** The web server stays a client of cpd. It persists and reads each Lead's classification through `cpd swr badge get` and `cpd swr badge put`. It has no SQLite driver; cpd is the only backend (pro-d449).
- **(B) Endpoint health.** The classifier's outage marker `run/classifier-unreachable.json` becomes two rows of `swr_endpoints`, `ollama.lab` and `bifrost`, under the 10-minute rule of [swr.md §7](swr.md). No file health state remains.

Out of scope: the GitHub, Slack and Jira endpoints. Their rows are listed in §5 so the endpoint list is whole, but their Workflows design them.

Today, on `origin/main` (353b5db7f):

| What | Where | Behaviour |
|---|---|---|
| badge state | `web/src/server/completion-service.ts:10-12` | three memory maps: `cache` (key to completion), `last` (session to completion, replayed on track), `inflight` (single-flight). Lost at every WebUI restart |
| badge key | `completion-service.ts:33` | `${ref.id}\|${last item id}\|${plan.cursor}` |
| badge classify | `web/src/server/completion-classify.ts:80` | `deps.child.spawn("node", [script])` directly, the helper `pi/lib/classify.ts`; not through the core `Runner` port |
| badge failure | `completion-classify.ts:86-106` | a non-zero exit, no line, bad JSON, `ok: false`, a spawn throw and the 30 s deadline all resolve `null`; nothing is recorded |
| badge wiring | `web/src/server/plugin.ts:138-152`, script path `:186-196` | `createCompletionService` with `planOf` (reads `cpd plan show`) and the classifier |
| outage marker | `core/src/classifier.rs`, `notify` | on `classifier_unreachable`, appends one `error` Event and writes `run/classifier-unreachable.json` (`{}`) if absent; a successful `choose()` removes it |

The line numbers are those of `origin/main` 353b5db7f. Where a line number of the Explore report (`s-badge-health`) no longer matches, this page names the function instead.

## 2. The Lead completion badge

### 2.1 Flow

The web server runs this for a Lead session. It runs when the session's items are non-empty and the plan is known: on a `session.window` in `tail` mode, and on a `session.status` settle from busy to idle (`completion-service.ts:61-63`, `:78-82`, as today).

```mermaid
sequenceDiagram
  autonumber
  participant W as web server (client)
  participant C as cpd swr badge
  participant H as classify.ts (helper)

  W->>C: badge get <session_id>
  C-->>W: row R or null (no write)
  Note over W: show R at once (pro-d438), even when stale
  alt R is null or R.entry_id differs from E (E = last item id)
    W->>H: classify (30 s deadline, as today)
    alt answer ok
      H-->>W: label, probabilities
      W->>C: badge put <session_id> --entry E ...
      W->>C: health record classifier ollama.lab ok
      Note over W: show the new badge
    else unreachable, or the deadline
      Note over W: keep R and the value shown; no put
      W->>C: health record classifier ollama.lab error --text ...
    else refused, or a bad reply
      Note over W: keep R and the value shown; no put
      W->>C: health record classifier ollama.lab ok
    end
  end
```

Rules of the flow:

- **Freshness.** A row is fresh while `entry_id` equals the Session's last transcript item id `E` (the Job's "last assistant turn", read as the last item: the key `completion-service.ts:33` uses today). Only a fresh row skips the classify call. A stale row is still shown while the classify runs (pro-d438).
- **No memory of freshness.** Freshness is read from the row, not from the engine's memory. A WebUI restart therefore shows the last badge at once and classifies only when `E` has moved. This is the exception to [swr.md §4](swr.md) ("after a restart every query is stale once"): a badge is not a `swr_queries` row, so it has no in-memory fresh-until instant.
- **Single-flight stays in the web.** The in-process `inflight` map (`completion-service.ts:12`, `:38-47`) still merges concurrent classifies of one key. It is not persisted.
- **The helper spawn stays in the web** for now. See §7, open item 3.

### 2.2 The completion type

The type is `web/src/protocol/domain.ts` on `origin/main` (merged with #918, pro-d445). The Job named `web/src/server/domain.ts`; that file does not exist.

```ts
export type CompletionLabel = "safe_to_archive" | "unanswered_ask" | "open_work" | "unrecorded_decision" | "unescalated_issue";
export type CompletionTone = "done" | "decision" | "blocked";
export interface CompletionScore { readonly label: CompletionLabel; readonly score: number }
export interface Completion { readonly label: CompletionLabel; readonly text: string; readonly score: number; readonly tone: CompletionTone; readonly others?: readonly CompletionScore[] }
```

The table and the type must agree. A change to `Completion` changes §2.3 and the `badge put` contract (§3.2) in the same commit.

### 2.3 The swr_badges mapping

The table is in contract §4.4 (DDL unchanged by this page). One row per Session.

| Column | Type | Comes from | Rule |
|---|---|---|---|
| `session_id` | TEXT PRIMARY KEY | the Session's id (`SessionRef.id`) | one row per Session; `badge put` replaces it |
| `entry_id` | TEXT NOT NULL | `--entry`: the Session's last transcript item id `E` at classify time | freshness only (§2.1); it is not part of the completion and is not returned in `completion` |
| `label` | TEXT | `Completion.label` | one of the five `CompletionLabel` names |
| `score` | REAL | `Completion.score` | an integer from 0 to 100 (the web rounds and clamps before it sends) |
| `text` | TEXT | `Completion.text` | non-empty, as given (`"<label text> · <score>%"`) |
| `tone` | TEXT | `Completion.tone` | `done`, `decision` or `blocked` |
| `others_json` | TEXT, nullable | `Completion.others` | the JSON array `[{"label","score"}, …]`, sorted as the web sends it. **NULL when `others` is absent.** An empty array is refused (`usage`), so the column is never `[]`: the web sends `others` only when it has entries |
| `synced_at` | TEXT | the time of the `badge put` that stored the row (the command's clock) | RFC 3339 |
| `inactive_at` | TEXT, nullable | `mark_inactive` (§2.4) | NULL while the Session is live; cleared by a `badge put` (`touch`) |

Read back, `badge get` returns `others` as the parsed array when `others_json` is not NULL, and **omits the key** otherwise. That matches the optional `others?` of the type, which does not accept `null`.

`swr_badges` has no CHECK on `label`, `tone` or `score` (§7, open item 4). The edge validates them (§3).

### 2.4 Freshness and eviction

- **staleTime:** Infinity while `entry_id` equals `E`; stale once `E` differs (§2.1).
- **gcTime:** 7 days, from `swr.gc_days` (A4, [contract §8.2](../contract.md)).
- **Observer (decided on this page; not one of the accepted assumptions of swr.md §17):** a badge row is wanted while its Session is not `stopped`. When the Session is marked `stopped`, its row gets `inactive_at` = now (`mark_inactive`, [swr.md §4](swr.md)). `gc::sweep` deletes the row once `inactive_at` is older than `swr.gc_days` and it has no other observer. A `badge put` for the Session clears `inactive_at`. The stop path that calls `mark_inactive` is named by the implementation Job (§7, open item 5).

### 2.5 Failure

- Any classify failure keeps the cached row and the value on screen. The web writes nothing for it. A failure never shows an empty badge, which today's `null` (`completion-classify.ts:86-106`) does.
- The failure is recorded as endpoint health (§4). No Event and no file are written for the badge itself; the outage Event, if the outage lasts 10 minutes, is the engine's (§4.3).

## 3. Command contracts

Full syntax, exit codes and the output envelope are in [commands §32.7 to §32.9](../commands.md). This section states what each command does. Every command prints one JSON value. The time is read at the command's edge and passed in (A6, [swr.md §16](swr.md)); no `swr` function reads the clock.

### 3.1 `cpd swr badge get <session_id>`

- **Reads only.** No write transaction, no Event, no outbox row.
- **Input:** `<session_id>`, a non-empty text.
- **Output:** `{"ok": true, "badge": null}` when the Session has no row (a miss is data, not an error), or `{"ok": true, "badge": {"session_id", "entry_id", "label", "score", "text", "tone", "others"?, "synced_at"}}`. `others` is present only when `others_json` is not NULL.
- **Idempotent:** yes.

### 3.2 `cpd swr badge put <session_id> --entry <id> --label <l> --score <n> --text <t> --tone <tone> [--others <json>]`

- **Input:** every flag is required except `--others`. `--entry` is non-empty text. `--label` is one of the five names of `CompletionLabel`. `--score` is an integer from 0 to 100. `--text` is non-empty. `--tone` is `done`, `decision` or `blocked`. `--others`, when given, is a non-empty JSON array of `{label, score}` whose labels are `CompletionLabel` names and whose scores are integers from 0 to 100.
- **Validation at the edge:** a bad value is refused `usage` (exit 2) before any read. Nothing is written for a refused call.
- **Effects:** the new values are compared with the stored row: `entry_id`, `label`, `score`, `text`, `tone` and `others_json` (the canonical JSON of `--others`, or NULL).
  - **Equal:** no write transaction is opened (D5, [swr.md §6](swr.md)); the stored `synced_at` is kept. The output says `changed: false`.
  - **Different:** one `Store::write` replaces the row, sets `synced_at` to the command's time and `inactive_at` to NULL, and reports `changed: true`.
- **Output:** `{"ok": true, "session_id", "entry_id", "changed", "synced_at"}`.
- **Not an outbox write.** It writes a cache row that no external system holds, so it does not go through `swr_mutations` (A6 covers external writes only).
- **Idempotent:** yes. A rerun with the same values writes nothing and reports `changed: false`.

### 3.3 `cpd swr health record <system> <endpoint> ok|error [--text <text>]`

- **Input:** `<system>` and `<endpoint>` are checked against the pairs of §5: `classifier` with `ollama.lab` or `bifrost`, `github` with `github`, `slack` with `slack`, `jira` with `jira`. Any other pair is `usage`.
- **`ok`:** `endpoint::record_ok(now)`. It clears the outage (`failing_since`, `reported_at`, `last_error`), sets `error_count` to 0, `last_ok_at` to now, `status` to `success`. A call on a row already in that state writes nothing. `--text` is refused with `ok`.
- **`error`:** `endpoint::record_failure(now, text, Kind::Transient)`. `--text` is required and non-empty. The first failure of an outage sets `failing_since`; each failure adds 1 to `error_count`; `last_error` is the text. Each call is one failure, so it is not idempotent.
- **No Event is written by this command.** The outage report is the engine's (§4.3).
- **Output:** `{"ok": true, "system", "endpoint", "status", "error_count", "failing_since"}`.
- **Failure codes:** `usage` (exit 2) for a bad pair, a missing or unexpected `--text`; `store_busy` (exit 5) when the write transaction cannot open within `store_busy_ms`.

## 4. Endpoint health

### 4.1 The rows

`swr_endpoints` ([swr.md §3.2](swr.md)) holds one row per endpoint. The two classifier rows:

| Endpoint | Calls it makes | Writes its health |
|---|---|---|
| `ollama.lab` | `classify.ts` through `PiClassifier` (merge triage, curation, distill); the badge (`completion-classify.ts`); the question guard (`pi/lib/question-guard.ts:72`) | Rust: `PiClassifier::choose` (`core/src/classifier.rs`). Web and question guard: `cpd swr health record` |
| `bifrost` | the judge `judge.ts` through `BifrostJudge` (`core/src/context/curate_judge.rs`, `JudgePort::judge`); the embedding `embed.ts` through `BifrostEmbedding` (`core/src/embedding.rs:128`) | Rust, in the two adapters |

**Where the Rust side records.** In the adapter, not in each caller. The bifrost callers are the curation sweep (its judge call is `curate_run.rs:317`, in `verify_pair`; the sweep stops at the first judge error), the distill verify (`distill.rs:482`, which ends the asking on a judge error), the search vector leg (`search.rs:377-386`, FTS-only fallback) and curation's embed (`curate_run.rs:512`, `:529`). All of them reach the two adapters, so recording there covers every caller once (DRY, one source of truth).

### 4.2 What counts as an outage

| Result of the call | Recorded as |
|---|---|
| an answer, `ok: true` | `ok` |
| an answer, or an HTTP status that is not a success (`*_refused`) | `ok`: the endpoint answered, so it is reachable |
| `*_unreachable`: connect, DNS, the request bound, or the whole-run deadline | `error`, with the text |
| `*_bad_reply` from a helper that exited non-zero or wrote no line | no record: the helper failed, which says nothing about the endpoint |
| `spawn_failed`: `node` cannot start | no record: a local fault, not the endpoint's |

The web server and the question guard apply the same table to the helper's line: its `error.code` `*_unreachable`, or the deadline, is `error`; an answer, or `*_refused`, is `ok`; the other cases in the table are no record.

This is the reachability rule: an answer or a refusal clears an outage, and only `unreachable` starts one. It replaces the old rule, under which the marker was written for `classifier_unreachable` alone (`core/src/classifier.rs`).

### 4.3 The outage report (the 10-minute rule)

- **When:** each engine tick, E24 ([contract §4.7](../contract.md), [swr.md §12](swr.md)) calls `endpoint::due_report(now)` for every endpoint row. It is true once per outage: when `now - failing_since >= 10 minutes` and `reported_at` is NULL. A true answer sets `reported_at`.
- **Why the engine and not the caller:** a failing endpoint may get no further call, so no caller would see the 10 minutes pass. The tick does.
- **What it writes:** one `error` Event, through `engine/src/watch/report.rs` (A3: the existing kind; no new kind, `PROTOCOL` unchanged). Its fields:

| Endpoint | `code` | `message` | `details` |
|---|---|---|---|
| `ollama.lab` | `classifier_unreachable` (unchanged code) | `http://ollama.lab/v1 is unreachable (connect Tailscale or disable in [experimental])` (unchanged text, pro-d369) | `{endpoint, system, since, error}` |
| `bifrost` | `bifrost_unreachable` (new code) | `bifrost is unreachable: <last_error>` | `{endpoint, system, since, error}` |

`source` is `watch`, the engine's own error, as the other engine-raised errors are. `since` is `failing_since`; `error` is `last_error`. The Event sets no `plan`, `session` or `context`, so no Lead gets a turn.

- **Once per outage:** `reported_at` is the dedup. `ok` clears it (`record_ok`, §3.3), so the next outage reports again.
- **The change from today:** the marker raised the Event at once; now it is raised after 10 minutes of continuous unreachable failure (A3). A refused reply never raises it.

### 4.4 Replaced

- `run/classifier-unreachable.json` is no longer written or read ([contract §1.4](../contract.md) row removed). A file left by an older binary is ignored.
- The notice of [contract §3.7a](../contract.md) ("the notice of an unreachable classifier") is replaced by §4.3.
- The question guard's per-run NOTICE (`pi/lib/question-guard.ts:74-78`) is pi's own UI message, not an Event and not a file. It stays. Its classify call also records `ok` or `error` through the command (open item 6).

## 5. Endpoints

Every endpoint row of `swr_endpoints`, its system, its owner and its outage code. Rows for `github`, `slack` and `jira` are shown so the list is whole; their Workflows set their codes.

| Endpoint | System | Owner | Calls today | Error codes today | Outage `error` code |
|---|---|---|---|---|---|
| `github` | `github` | the github Workflow | `gh` through E5 (`engine/src/watch/pr.rs`, `poll_with`) and the `pr` commands | `gh_failed`, `gh_rate_limited` | the github Workflow decides; `run/pr-poll-failing.json` stays until its cutover ([swr.md §15](swr.md)) |
| `slack` | `slack` | the slack Workflow | `pi/lib/slack-channel.ts` (E11, `engine/src/watch/team.rs`); `pi/lib/slack-react.ts` (E17, `engine/src/watch/react.rs`) | `slack_*` | the slack Workflow decides |
| `jira` | `jira` | the jira Workflow | `pi/lib/jira-transition.ts` (E19, `engine/src/watch/jira.rs`, `fail_row`) | `jira_*` | the jira Workflow decides |
| `ollama.lab` | `classifier` | this page | `classify.ts` (`PiClassifier`); the badge; the question guard | `classifier_unreachable`, `classifier_refused`, `classifier_bad_reply` | `classifier_unreachable` |
| `bifrost` | `classifier` | this page | `judge.ts` (`BifrostJudge`); `embed.ts` (`BifrostEmbedding`) | `judge_unreachable`, `judge_refused`, `judge_bad_reply`, `embedding_unreachable`, `embedding_refused`, `embedding_bad_reply` | `bifrost_unreachable` (new) |

The `system` column is the pair §3.3 checks. The `models` source names its provider endpoint in its own Workflow (swr.md §14); this command does not accept it until that Workflow does.

## 6. Rules

- **Time is an argument.** Every `swr` function takes `now` (or `poll_started_at`). The commands read their clock at the edge and pass it in (swr.md §16). Tests advance it with `ManualClock`.
- **No file, no queue.** The badge and the health live in `swr_badges` and `swr_endpoints`. Nothing is written under `run/` for either.
- **A failed call never clears a badge.** A failed classify writes no `swr_badges` row (§2.5).
- **Unchanged writes open no transaction.** A `badge put` with equal values opens none (§3.2). An `ok` on an endpoint already in that state opens none (§3.3).
- **Mocks.** Tests of the Rust adapters mock the helper through `FakeRunner`, and time through `ManualClock`. Tests of the web client mock the `cpd` boundary. No test runs a real node, a real `cpd`, a real endpoint or a sleep ([swr.md §16](swr.md)).

## 7. Open items

Not decided by this page. Each is recorded so no Job guesses it.

1. **`plan.cursor` is not in the badge key.** Today's key (`completion-service.ts:33`) includes `plan.cursor`, so a Workflow change re-classifies the badge. With `entry_id` set to the last item id only (§2.3), a Workflow change that adds no transcript item keeps the old badge until the transcript moves. The Lead decides whether `entry_id` also carries the cursor. The Job's wording gives `entry_id` as the last item id, and this page follows that.
2. **"The session's last assistant turn".** The Job's freshness wording names the last assistant turn; the key today is the last item of any kind (`completion-service.ts:33`), and this page follows the key. Choosing the last assistant turn would classify less often and is a behaviour change.
3. **The classify call is outside SWR.** [contract §4.7](../contract.md) requires every external read to go through a `Source`. The web keeps its helper spawn, as the Job states, so the badge's classify is an external read outside `core/src/swr/source/`. The classifier Workflow decides its source file and key kind (swr.md §18).
4. **No CHECK on `swr_badges`.** The DDL has no CHECK on `label`, `tone` or `score`. The edge validates them. A CHECK in the swr-core migration, before that migration merges, would make an illegal row unrepresentable. The Lead decides.
5. **The stop path.** §2.4 needs the code path that marks a Session `stopped` to call `mark_inactive` on its badge row. The implementation Job names it. Unverified: I have not traced where a Session's `stopped` state is written.
6. **The question guard's NOTICE.** It stays as pi's UI message (§4.4). The Lead decides whether it also becomes an Event.
7. **The bifrost base URL** is not in the repo (it is pi's `models.json`). The `bifrost` message in §4.3 names no URL for that reason. Unverified.
