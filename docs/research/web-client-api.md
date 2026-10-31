# The cpd web client against the typed API mock: wire surface, mock contract, fold and architecture

Sources: `docs/research/durable-leads.md` §4.2–§4.6, `pi/durable-host/protocol.ts`, `docs/research/web-ui.md` §2–§4, §7–§8, `docs/commands.md` §4.2/§4.3/§10.1/§21.1, `pi/ask-user-question/tool/types.ts`, and the Context records hs-d768..hs-d783, hs-n595, hs-n596, hs-n599. This doc is the contract the web-client Jobs (client-tests, client-impl, views-*, verify-web) follow; it pins the decided wire shapes, the mock's decisions, the TS fold and the architecture, and flags every still-open P1.6/P1.8 shape `@open(P1.6/P1.8)` — never invented (A5, hs-n596).

Status: this is a contract document for the web-client Plan (plan `web-client`, hs-d780), not a decision of its own. Every claim cites a record id or a `file:line`.

## 0. The client in one paragraph

The web client is a browser UI for cpd with cockpit parity (hs-d757, web-ui.md §2), built in `web/` as npm package `cpd-web-client`, a pnpm workspace member (A1, hs-n603), on Vite + React + shadcn/ui stock components only (hs-d770, hs-d783). It is developed against a **typed mock of the decided API** first, wired to the core and server when those land (hs-d780, superseding hs-d778's W1-to-W7 build order). Client scope (A7): **Tree, Plan, Inbox, Logs and Chat render read-only**; actions are **typed POSTs** with mock handlers; the **TS chat fold** (hs-d772) and approval answering via `ui_response` (hs-d775) are fully typed and reachable; the **Escalation count shows** with no answer UI (hs-d776). Live views travel over a **WebSocket** with an in-process commit-watcher + Event-cursor model (hs-d773, hs-n799 [sic: n599]).

## 1. The typed wire surface

### 1.1 The decided v1 set (`pi/durable-host/protocol.ts`)

The client types the decided wire shapes of durable-leads.md §4.5; the v1 host frames are implemented on main in `pi/durable-host/protocol.ts` (the `ToHost` union at :26-33, the `ToClient` union at :41-50, `Cursor` :11, `ErrorCode` :13-17, `LIMITS` :2-9). The decided ToClient and ToHost messages and their fields:

| Direction | Type | Decided fields | Source |
|---|---|---|---|
| H→C | `welcome` | `protocol`, `host_version`, `pi_durable`, `session`, `role` (`lead`\|`manager`), `epoch`, `client_id`, `cwd`, `limits:{client_line}` | protocol.ts:42; durable-leads 4.5 row 17 |
| H→C | `attached` | `req`, `mode` (`snapshot`\|`resume`), `epoch`, `from_seq`, `head_seq`, `client_id` | protocol.ts:43; row 18 |
| H→C | `snapshot` | `epoch`, `seq`, `snapshot` (a pi-durable `SnapshotEvent`, unchanged) | protocol.ts:44; row 19 |
| H→C | `events` | `epoch`, `seq`, `events` (an array of `AgentEvent`, one commit, unchanged) | protocol.ts:45; row 20 |
| H→C | `result` | `req`, `data` (object, by command — 1.3) | protocol.ts:46; row 21 |
| H→C | `error` | `req?`, `code`, `message`, `details?` | protocol.ts:47; row 22; codes in 1.4 |
| H→C | `host_state` | `state` (`idle`\|`working`\|`waiting`\|`blocked`), `name?`, `clients` (int), `held_ui` (int), `agent?` (an `AgentState`) | protocol.ts:48; row 27 — `clients` and `held_ui` included |
| H→C | `pong` | `req?` | protocol.ts:49; row 28 |
| H→C | `bye` | `reason` (`shutdown`\|`restart`\|`storage_failed`\|`slow_client`\|`detached`), `reconnect` (bool) | protocol.ts:50; row 29 |
| C→H | `hello` | `protocol` (max int), `min_protocol` (int), `client` (string), `name?` | protocol.ts:27; row 1 |
| C→H | `attach` | `req`, `cursor?:{epoch,seq}`, `conversation?` (v1: absent or `"root"`) | protocol.ts:28; row 2 |
| C→H | `submit` | `req`, `text` (v1 text only), `when_busy` (`followUp` default \| `steer` \| `reject`), `request_id?` | protocol.ts:29; row 3 |
| C→H | `abort` | `req`, `background` (bool, default false) | protocol.ts:30; row 4 |
| C→H | `abort_submission` | `req`, `submission_id` (number) | protocol.ts:31; row 5 |
| C→H | `ping` | `req?` | protocol.ts:32; row 15 |
| C→H | `detach` | `req` | protocol.ts:33; row 16 |

`result.data` by command (row 21): `submit` `{submission_id,status}` (`queued`\|`placed`\|`done`\|`unanswered`), `abort` `{}`, `abort_submission` `{outcome}` (`aborted`\|`already_placed`\|`settled`\|`not_found`), `ui_response` `{}`, `detach` `{}`.

Error codes (row 22 and protocol.ts:13-17): `protocol_mismatch` (closed), `hello_timeout` (closed), `hello_required` (protocol.ts:17), `bad_frame` (kept), `frame_too_large` (closed), `unknown_type` (kept), `bad_args` (kept), `not_attached` (kept), `busy` (kept), `ui_unknown` (kept; changed by the spike — replaces `ui_resolved` as an error code, row 22 / 4.4 rule 3), `unsupported` (kept), `storage_failed` (closed, then `bye`), `shutting_down` (closed), `internal` (kept), `refused` (kept, `reload` busy-guard, row 9).

Framing, handshake, correlation, ordering, limits: durable-leads 4.2 (NDJSON, one object per line; `hello` first, `welcome` or `protocol_mismatch`; every C→H message except `hello`/`ping` carries `req` and gets exactly one reply with the same `req` — `attached` for `attach`; `LIMITS` protocol.ts:2-9; a `snapshot` or `events` replaces/extends the client's view per the fold, section 5).

### 1.2 The eight P1.6/P1.8 messages

The web client needs the P1.6 and P1.8 messages; host protocol v1 plus P1.6/P1.8 carries a browser client with **no protocol change or version bump** (hs-n595; web-ui.md 3.3, 3.5). P1.6 is messages 14, 23, 24 (`ui_request`, `ui_response`, `ui_resolved`); P1.8 is messages 6, 7, 8, 11, 12 (`compact`, `configure`, `list_models`, `fork`, `history`) (durable-leads 7.2 "P1.6", "P1.8"; web-ui.md 3.3). Their recorded shapes:

**P1.6.**

- `ui_request` (row 23): `id`, `method` (`select`\|`confirm`\|`input`\|`editor`\|`questionnaire`), the method's fields, `purpose` (`ask_user_question`\|`plan_review`\|`secret_approve`\|`host_approve`\|`mcp_login`\|`other`), `created_at`, `expires_at?`, `tool_call_id?`, `replayed?` (4.4 "Added fields"). The first four methods use pi's field names (read: `rpc-types.d.ts:406-476`): `select{title,options[],timeout?}`, `confirm{title,message,timeout?}`, `input{title,placeholder?}`, `editor{title,prefill?}` (4.4). The `questionnaire` fields are **`@open(P1.6/P1.8)`** (`questions:[{question,header,multiSelect,options:[{label,description,preview?}]}]` — inferred from `pi/ask-user-question/tool/types.ts:41-91`, row 23; C3). Whether `ui_request` frames replay after `host_state` with `replayed:true` is **`@open(P1.6/P1.8)`** (C3).
- `ui_response` (row 14): `req`, `id`, exactly one of `value` (string) \| `confirmed` (bool) \| `answer` (object, for `questionnaire`) \| `cancelled:true`. The `answer` object is the questionnaire result — recorded as `QuestionnaireResult` (`pi/ask-user-question/tool/types.ts:109-157`: `answers:[{questionIndex,question,kind,answer,selected?,notes?,preview?}]`, `cancelled`, `globalNote?`, `unanswered?`, `error?`) — and its exact wire envelope is **`@open(P1.6/P1.8)`** (hs-n596(1), C3).
- `ui_resolved` (row 24): `id`, `outcome` (`answered`\|`cancelled`\|`timeout`\|`superseded`\|`aborted`), `by?` (client id or `"host"`). Decided; measured outcomes `answered` and `aborted` (spike, 4.4 rules 2 and 4).

**P1.8.**

- `history` (row 12): `req`, `before?` (opaque cursor from a previous `result`), `limit` (default 50). Result `history {entries,next?}`, newest first, fork-aware; `Cursor` is opaque (read: `Conversation.entries`, `types.d.ts` near :619). The exact `entries`/`next` result shape is **`@open(P1.6/P1.8)`** (row 21 "inferred" for `history`; C4; hs-n596(1)).
- `list_models` (row 8): `req`. Result `{models:[{provider,model_id,name,thinking_levels,context_window}]}` is **`@open(P1.6/P1.8)`** (row 21 marks it inferred; the registry and `getSupportedThinkingLevels` are read: `pi/durable-worker/harness.ts:20-28`, `pi-ai/dist/models.d.ts:267-268`; not built in the spike).
- `configure` (row 7): `req`, `model?:{provider,model_id}|null`, `thinking_level?` (`off`\|`minimal`\|`low`\|`medium`\|`high`\|`xhigh`\|`max`)\|null. Applies from the next request; the host validates against `list_models` and refuses with `bad_args`; `model` persists across a restart; `thinking_level` not built (row 7, measured for `model`). The `{agent}` result shape is **`@open(P1.6/P1.8)`** (row 21; C4).
- `fork` (row 11): `req`, `at` (entry id), `ownership?`. v1 does not serve the new conversation (a new Session with its own host, or a second conversation in the same store: Q7). The `{conversation_id}` result and what the client opens next are **`@open(P1.6/P1.8)`** (C4 "a new Session id the host does not serve, Q7"; hs-n596(1)).
- `compact` (row 6): `req`, `instructions?`. Result `{task_id}` (`TaskId<CompactionResult>`, read: `Conversation.compact`, `types.d.ts:610`); progress as `compaction_start/end` events (row 6, read).

The rows of §4.5 outside the v1 set and outside P1.6/P1.8 — `reload` (row 9), `mcp` (row 10), `rename` (row 13), `notice` (row 25), `status` (row 26) — are part of the v1 table's design with the fields their rows state, and are not in the client's first mock surface (they land with P1.9 host operations, durable-leads 7.2 "P1.9"); the client types them only if a view needs them, without inventing shapes.

## 2. `@open(P1.6/P1.8)`: every shape durable-leads marks inferred or that P1.6/P1.8 must pin

The list, exactly, of what is recorded but not settled and must never be written as a decided shape (A5, hs-n596; durable-leads row 23, row 21; C3, C4):

1. the `questionnaire` request fields (`ui_request.method = "questionnaire"`'s `questions` array) — `@open(P1.6/P1.8)` (row 23, inferred from `tool/types.ts:41-91`);
2. the `ui_response.answer` envelope for `questionnaire` — `@open(P1.6/P1.8)` (C3, hs-n596(1)); the *contents* are recorded as `QuestionnaireResult` (`tool/types.ts:109-157`);
3. the `history` result (`{entries,next?}`, opaque `before` cursor) — `@open(P1.6/P1.8)` (row 21, C4);
4. the `list_models` result (`{models:[...]}`) — `@open(P1.6/P1.8)` (row 21 "inferred", hs-n596(1));
5. the `configure` result (`{agent}`) — `@open(P1.6/P1.8)` (row 21, C4);
6. the `fork` result (`{conversation_id}`, what it points at) — `@open(P1.6/P1.8)` (row 11, C4, hs-n596(1));
7. whether `ui_request` frames replay after `host_state` with `replayed:true` — `@open(P1.6/P1.8)` (C3).

Not part of this surface, sent to durable-leads as asks and never decided — the client must not depend on them (web-ui.md 3.5; hs-n595): C1 `attach.entries_limit` (windowing), C2 a `hello.role: "viewer"`, C5 the host's behaviour over a 64 MiB line.

## 3. The read-model mirror and the parity skeleton

The mock's read-model types mirror the CLI's documented JSON (hs-d771, A6), **flagged as the W1 core `Serialize` output to supersede** (hs-d771: move the shared read model to core with `Serialize`; the CLI commands print it; the gaps close as CLI commands first). The mirror covers, per `docs/commands.md`:

- `plan list` (§4.2): `{ok, plans:[{id, context, title, phase, lead_session, archived_at, counts:{draft,queued,running,merged,failed,cancelled}, prs:[{key, repo, status, pr, pull_request}]}], events_cursor}`;
- `plan show` (§4.3): `{ok, plan:{id, context, title, jira, phase, lead_session, created_at, locked_at, done_at, archived_at, outline}, prs:[{key, repo, base, base_pr, jira, needs, status, lead_branch, lead_worktree, pr, pull_request, all_merged_at, start_failed_at, created_at, started_at, merged_at, jobs}], jobs:[{key, pr, title, status, tier, appended, needs, fingerprint, retry_tier, goal, context, touches, acceptance, out_of_scope, attempts:[...]}], escalations, review, counts, tamper, events_cursor}`;
- `session list` (§10.1): `{ok, sessions:[{id, role, context, plan, attempt, name, runner, state, state_label, state_at, archived_at, asleep_at, place, tmux_pane, file, forked_from, plan_lead, tier, cwd, title, titled_at}], events_cursor}`;
- `inbox list` (§21.1): `{ok, engine:{running}, items:[{id, kind, urgency, label, subject, context, plan, job, session, record, event, url, opened_at, closed_at, snoozed_until, dismissed_at, claimed_at, age_seconds}], hidden:{snoozed,dismissed,claimed}, events_cursor}` (item shape per `core/src/inbox/cmd.rs:121-142`; the synthetic `engine-down` item has `id:null`).

The mirror lives in `web/src/parity` (TS types over these JSON shapes). The **parity skeleton** (A6, hs-d779) is a test in `web/src/parity` that consumes the mirror types and, when W1 lands (hs-d771), imports the core `Serialize` read-model types and asserts the CLI's real output equals the mirror — the parity test pinning the one shared read model plus the rule that a new cockpit read or action lands in core with a CLI command first (hs-d779).

## 4. The mock contract

The mock is a dev-time stand-in for the decided API that the client is built against before the core and server land (hs-d780). Its contract:

1. **One typed API surface.** The `web/src/api` module exposes a single interface over the whole typed surface — the decided ToHost sends, the ToClient receives (1.1), the mirror reads (3) and the action POSTs (4.3) — which both the mock and the later real transport implement. The mock implements it in-process in `web/src/api/mock`, served by the Vite dev server on port 5173 (A2).
2. **Token on every request; the mock refuses token-less requests** (hs-d781, A4). The client's API layer attaches the token to every request from the start (WebSocket upgrade and every POST); any request without it is refused. (hs-d781: cpd web is a network listener on localhost and the LAN with Host+Origin+token auth; server auth is a later Plan — A4.)
3. **`ui_response` answering per durable-leads 4.4** (hs-d775): first `ui_response` wins — the host removes the request from `held`, resolves the promise (`cancelled` gives `undefined`/`false`), answers the sender with `result`, and broadcasts `ui_resolved {id, outcome:"answered", by}` to all attached clients (4.4 rule 2); a late answer gets `error {code:"ui_unknown"}` (4.4 rule 3); the host itself resolves with `timeout`, `superseded` and `aborted` outcomes (4.4 rule 4); detach never touches `held`, and a reattaching client receives one `ui_request` per held request with `replayed:true` and the same id (4.4 rule 5). The mock models held state so the client's approval flows (plan review, secret/host approve, ask_user_question) are fully exercised (A7, hs-d775).
4. **Actions as typed POSTs.** Every client action is a typed POST with a mock handler (A7): the cockpit actions of web-ui.md §2.2 the client offers (New Context/Plan/Session/fork, rename, archive/unarchive, delete, open/park/close, Inbox snooze/dismiss/claim/approve/merge/post-for-review, curation checklists, runner toggle) and the host commands (`submit`, `abort`, `abort_submission`, `ui_response`). Each POST carries the token; the mock refuses token-less ones (hs-d781).
5. **The live channel.** Live views travel over a WebSocket (hs-d773, hs-n599 — the in-process commit-watcher + Event-cursor model, option C of web-ui.md §8 Q5); commands are POSTs. The mock simulates the host socket's stream frames (1.1) and the read-model updates, and honours the fold rules (5).

## 5. Fold semantics (the TS fold)

The chat bridge relays host-socket frames to the browser, which folds them — a TS fold (hs-d772, web-ui.md §8 Q4 A). The fold's semantics are decided in durable-leads 4.3 and the spike:

1. **Apply each `AgentEvent` of an `events` frame in order** onto the client's `SnapshotEvent` state (durable-leads.md:385, "The fold"; hs-d772).
2. **A whole message arrives at `message_start` and is replaced at `message_end`**; a short answer has no `message_update` at all, and `message_update` appears only when generation outlasts the 100 ms commit interval — a client never waits for a delta (durable-leads.md:385, "Changed by the spike"; 4.9 row 1).
3. **A `snapshot` frame replaces the whole view** (durable-leads.md:383, "Rebase": a client replaces its whole view on any `snapshot`; a restart is a new `epoch` and always arrives as `snapshot` mode, web-ui.md 3.3).
4. **Cursor epoch/seq resume.** A cursor is `{"epoch":"...","seq":N}` = "I applied every frame up to N of that epoch"; on `attach` with `base.seq <= cursor.seq <= head` and the same epoch the host answers `mode:"resume"` and sends every ring frame with `seq > cursor.seq`; otherwise `mode:"snapshot"` (durable-leads.md:370-377; web-ui.md 3.3: the browser keeps `{epoch,seq}` of the last applied frame).

## 6. Architecture: `web/src/*` and who renders what

`web/` is npm package `cpd-web-client`, a pnpm workspace member whose Vite dev server runs against the mock on port 5173 (A1, A2, hs-n603). Layout and responsibilities:

| Path | Holds |
|---|---|
| `web/src/api` | the typed wire surface (1.1), the one `CpdApi` interface, the token attachment (A4), the typed POST action calls, the WebSocket live channel (hs-d773) |
| `web/src/api/mock` | the typed mock implementing `CpdApi`: stream frames, read-model mirror, action handlers, the token gate (section 4) |
| `web/src/fold` | the TS chat fold: the four fold rules over `AgentEvent`s onto a `SnapshotEvent` (section 5, hs-d772) |
| `web/src/parity` | the read-model mirror types and the parity skeleton (section 3, hs-d771, hs-d779) |
| `web/src/views` | the cockpit views: Tree, Plan, Inbox, Logs, Chat — all read-only (A7) |
| `web/src/app` | the app shell: routing (react-router, hs-d770), the live WebSocket connection, the token bootstrap |
| `web/src/components` | shadcn/ui stock components only — pure shadcn components, their theme tokens, no custom CSS files, no custom or wrapped components (hs-d783) |

Who renders what (A7, hs-d776): **Tree** = Context/Plan/Session rows with dots, costs and the Inbox strip facts; **Plan** = PR boxes, Job pills, Pulse, the chat slot; **Inbox** = the 18 kinds with per-second age; **Logs** = the events log; **Chat** = the durable conversation, read-only, rendered from the fold. **Approvals are answered** — Plan review, secret approval, host-repair approval, `ask_user_question` — via `ui_response`, because any attached client may answer as durable-leads Q1 decided (hs-d775; web-ui.md 3.4 "approvals and Origin are the server's job in all three"). The **Escalation count shows only**; there is no Escalation-answer UI in the web (hs-d776). Sessions without a socket get a read-only/send-only web chat (pi from the session-file transcript plus `cpd session send`; a stopped durable host shows the last transcript with a host-not-running banner) (hs-d777).

## 7. Decisions cited, by record

hs-d768 (cpd web subcommand, new crate, one binary — the server side the client later talks to); hs-d769 (sync tiny_http-class HTTP server); hs-d770 (Vite + React + shadcn/ui; bundler build joins the Gate); hs-d771 (shared read model to core with Serialize; CLI prints it); hs-d772 (chat bridge relays frames, browser folds in TS); hs-d773 (live views over WebSocket, POST commands); hs-d774 (unix socket behind tailscale serve — **superseded by hs-d781**); hs-d775 (any attached client may answer approvals via ui_response); hs-d776 (no Escalation-answer UI; count only); hs-d777 (read-only/send-only chat for socket-less Sessions); hs-d778 (build order — **superseded by hs-d780**); hs-d779 (parity test over the one shared read model); hs-d780 (build the client first against a typed mock of the decided API); hs-d781 (network listener, localhost + LAN, Host+Origin+token auth stays, supersedes hs-d774); hs-d782 (server port 6364 — the later Plan); hs-d783 (online shadcn/ui stock only); hs-n595 (host protocol v1 + P1.6/P1.8 carries a browser client, no protocol change; C1–C5 sent to the Manager); hs-n596 (the P1.6/P1.8 Specs must state the exact wire types — questionnaire, history, list_models, configure, fork result; 64 MiB line; optional entries_limit; optional viewer role); hs-n599 (Q5 = WebSocket option C, tungstenite family).

---

*Everything above that durable-leads.md marks inferred or that the P1.6/P1.8 Specs must pin is flagged `@open(P1.6/P1.8)` in §2 and §1.2; nothing else in this doc is a guess.*
