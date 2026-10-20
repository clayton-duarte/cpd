# plannotator sessions as Inbox items (hs-d496)

Job `pnbox-research` of Plan `inbox`. Question: how does every plannotator session a cpd Session opens for the User become an Inbox item (magenta, hs-d483) whose link the User can reach (hs-d264), open until the session ends, with a raw call that skips it refused (hs-d468 style)? This file proposes a design. It changes no code.

## Answer first

- **The localhost link has one source.** plannotator builds every advertised URL in `pn:server/network.ts:177-192` (`buildAdvertisedUrl`). With no URL host set it returns `http://localhost:<port>` (`:179`). A Manager's pi gets no `PLANNOTATOR_URL_HOST`, because cpd gives a Manager no `PLANNOTATOR_*` variable except `PLANNOTATOR_UPDATE_CHECK` (`core/src/host/pi/launch.rs:129-137`) and `session run` strips every inherited one (`core/src/session/run.rs:195-199`). Its pi still runs in **remote mode**: the tmux server's global environment holds `SSH_CONNECTION` and `SSH_TTY`, the Manager's pane inherits them, and plannotator treats either as remote (`pn:server/network.ts:92-95`). Remote mode means a fixed port of 19432 (`:133-138`) and a bind to `0.0.0.0` (`:151-153`). That is exactly the evidence of hs-d496: `http://localhost:19432` printed, the server answering on `192.168.3.3:19432`.
- **The fix** is for cpd to give the Manager the same three variables it gives a Plan's Lead: `PLANNOTATOR_REMOTE=1`, `PLANNOTATOR_URL_HOST=<review_host()>` and its own `PLANNOTATOR_PORT`. A Manager's port is needed too: its default 19432 is the first port of the Plans' range `19432-19441` (`core/src/config/file.rs:295`), so the two collide.
- **Recommended route: (a), the adapter tracks every session.** The adapter reads plannotator's process-wide registry of open reviews and mirrors it into cpd. A new `cpd review open|close` writes a new table, and E8 turns its rows into a new magenta Inbox kind, `review-waiting`, whose URL E8 builds from the review host and the port (as `plan-review` does). A `tool_call` guard refuses a `plannotator` or `plannotator_submit_plan` call that could not be tracked or linked. Route (b), wrapping annotate in a cpd tool, cannot see when a session ends, loses the decision on `/reload` and gives no URL (§4.2).
- **Biggest risk.** The registry is plannotator's internal object on `globalThis`, not a published API. Its key is versioned (`pn:plannotator-tool-host.ts:182`). The adapter must parse it at the boundary and fail closed: refuse new sessions and say so.

## Method and evidence

- **Read:** cpd at `3de57f7d` (`pi/extension.ts`, `pi/lib/*.ts`, `core/src/host/{addr.rs,pi/launch.rs}`, `core/src/session/{run.rs,port.rs}`, `core/src/plan/lead.rs`, `core/src/inbox/{model.rs,rules.rs}`, `core/src/events/kind.rs`) and `docs/contract.md` §2.2, §4.4, §4.5, §5.1, §5.2, §6.5, plus `docs/commands.md` §13.2, §15.5, §15.13.
- **Also read:** `@plannotator/pi-extension` 0.28.17 at `~/.pi/agent/npm/node_modules/@plannotator/pi-extension`, written `pn:` below. Files read: `index.ts`, `plannotator-events.ts`, `plannotator-tool-host.ts`, `plannotator-browser.ts`, `inbox.ts`, `annotate-outcome.ts`, `tool-scope.ts`, `server/network.ts`, and `generated/{config.ts,plannotator-tool.ts,data-dir.ts}`.
- **pi 1.3.0 docs and types:** `docs/extensions.md` (events, `tool_call`, nested calls) and `dist/core/extensions/types.d.ts`.
- **Ran (read only):**
  - `tmux -L cpd show-environment -g`: `SSH_CONNECTION`, `SSH_TTY`, `SSH_CLIENT` and `PLANNOTATOR_REMOTE=1` are set globally.
  - `/proc/<pid>/environ` of every live `pi`, grouped by role in the table of §2.
  - `cat ~/.plannotator/config.json` and `run/*/plannotator/config.json`.
  - `which plannotator`: no plannotator CLI is installed.
  - No plannotator session was opened. No file outside this one was written.

## 1. Every way a cpd Session can open a plannotator session

### 1.1 What plannotator registers

| Entry point | What it opens | Registered / active when | Source |
|---|---|---|---|
| Tool `plannotator` (`PLANNOTATOR_TOOL_NAME`), actions `annotate`, `review`, `last` | annotate a file, a URL, a folder or a list of files (bundle); code review; annotate the last message. `gate: true` adds Approve | Registered only when `resolveAgentTool` is true: `PLANNOTATOR_AGENT_TOOL`, else `config.json` `agentTool`, else the pi default **false**. Activated once at `session_start`, and only with a UI | `pn:generated/plannotator-tool.ts:54-56,166-168`; `pn:index.ts:412,494-504,1560-1577`; `pn:generated/config.ts:1021-1025,1059-1065` |
| Tool `plannotator`, actions `list`, `close` | Nothing. They list or close this pi session's open reviews | as above | `pn:index.ts:1593-1600`; `pn:plannotator-tool-host.ts:149-173` |
| Tool `plannotator_submit_plan` (`PLAN_SUBMIT_TOOL`) | a plan review of a markdown file | Always registered. Active only in plannotator's own planning phase: stripped while idle (`pn:index.ts:2491-2497`, `pn:tool-scope.ts:15-19`), added on entering planning (`pn:index.ts:708-735,763-770`) | `pn:index.ts:1708-2010` |
| Tool `plannotator_mark_done` | nothing (a checklist tick) | executing phase | `pn:index.ts:1667-1700` |
| Tool `plannotator_inbox` | nothing: messages of plannotator's own Inbox app, not a review | Only when `PLANNOTATOR_INBOX_TOOL`/`inboxTool` allows it (pi default off) **and** `<data dir>/inbox/inbox.json` exists | `pn:inbox.ts:69-81,96-109`; `pn:generated/config.ts:1100-1106` |
| Slash `/plannotator-annotate`, `/plannotator-review`, `/plannotator-last` | the same launches as the tool | Always registered. Only a human can type them | `pn:index.ts:1510-1545` |
| Slash `/plannotator-plan-mode`, Ctrl+Alt+P, flag `--plan` | turn planning on, which makes `plannotator_submit_plan` active | always registered | `pn:index.ts:530-534,871-876,1658-1663` |
| Event API `pi.events.emit("plannotator:request", {action})`, actions `plan-review`, `annotate`, `annotate-last`, `code-review`, `archive` (plus `plan-mode` and `review-status`, which open nothing) | `plan-review` answers at once with `{status: "pending", reviewId}`. **`annotate`, `annotate-last`, `code-review` and `archive` answer only when the reviewer decides** (they await `open*`, which awaits `waitForDecision`) | Always listening, for any extension in the same pi | `pn:plannotator-events.ts:83-95,403-537`; `pn:plannotator-browser.ts:424-429,816-836,926-933` |

### 1.2 Which are reachable for each cpd role today

| | Lead (Plan Lead or fork) | Manager | Worker |
|---|---|---|---|
| `plannotator` tool | **Only where its Plan's data dir enables it.** `PLANNOTATOR_DATA_DIR=run/<plan>/plannotator` (`launch.rs:140`), and plannotator reads `config.json` from the data dir (`pn:generated/config.ts:465-487`). Today `cross-repo`, `mac-pro-debug`, `repo-policy` and `switch-over` have `{"agentTool": true}` there (guess: written by plannotator's settings UI); the others have no file, so the tool is not registered | **Yes.** No `PLANNOTATOR_DATA_DIR`, so `~/.plannotator/config.json` applies, and it holds `"agentTool": true`. This is the hs-d496 case | Never: `--exclude-tools …,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox` (`launch.rs:14-15,153`) |
| `plannotator_submit_plan` | Only after planning is turned on. cpd never turns it on (no `--plan`, no `plan-mode` request), so only the User's slash command or shortcut can | same as Lead | excluded |
| `plannotator_inbox` | no (`inboxTool` unset, no `inbox/inbox.json`) | no (`~/.plannotator/inbox` does not exist) | excluded |
| Slash commands | the User can type them in the Lead's pane | the User can type them | no human |
| Event API | the adapter sends `plan-review` for a Plan Lead only (`pi/lib/review.ts:44`, `pi/lib/startup.ts:142-161`), through `cpd_plan_review` or a `plan-review` Event | the adapter sends none (`startup.ts:142` comment) | none |

Other extensions in `lead_pi_packages` could also send event-API requests. I did not check which packages are configured (unverified).

## 2. The URL of each session

**One builder.** Every server's URL is `buildAdvertisedUrl(port)`: plan review at `pn:server/serverPlan.ts:696`, annotate and last-message at `pn:server/serverAnnotate.ts:1511`, code review at `pn:server/serverReview.ts:3976`. The function, `pn:server/network.ts:177-192`, works like this:

1. `host = resolveUrlHost(config)`: `PLANNOTATOR_URL_HOST`, else `config.urlHost`, else undefined (`pn:generated/config.ts:1312-1325`).
2. No host gives `http://localhost:<port>` (`:179`).
3. A host in a **local** session is ignored, with one warning on stderr, and also gives `http://localhost:<port>` (`:180-188`).
4. A host in a **remote** session gives `http://<host>:<port>`. The value `auto` resolves through Tailscale, or falls back to localhost (`:189-191`).

**Remote mode** (`isRemoteSession`, `:87-97`) is `PLANNOTATOR_REMOTE` when it is `1`/`true`/`0`/`false`, else true when `SSH_TTY` or `SSH_CONNECTION` is set. Remote mode decides three things:

- **The bind address:** `0.0.0.0` when remote, `127.0.0.1` otherwise (`:151-153`).
- **The port:** `PLANNOTATOR_PORT`, a single port or a range. Without it, remote mode uses **19432** and local mode a random port (`:116-141`).
- **The browser:** remote mode opens none and shows the URL with `ctx.ui.notify` (`pn:plannotator-browser.ts:254-261`).

So the URL host is display-only, and it is right only when remote mode and `PLANNOTATOR_URL_HOST` are both set.

**The URL reaches the agent and the User through these paths, all the same string:**

- The `plannotator` tool's result text and `details.url` (`pn:index.ts:1651-1655`, `pn:generated/plannotator-tool.ts:322-342`).
- `sessionOpenedMessage` (`pn:index.ts:230-237`) and the `[Plannotator] <url>` notification.
- The registry entry's `url` (`pn:index.ts:934,1970-1977`).
- The event API's `plan-review` answer carries **no** URL (`pn:plannotator-events.ts:467-473`). That is why cpd builds the `plan-review` item's URL itself from `review_host()` and `run/<plan>/plannotator-port` (`core/src/inbox/rules.rs:192-206`).

**What each cpd Session gets today** (code, then the live `/proc/<pid>/environ` of 2026-10-20):

| Session | cpd sets | Inherited from the tmux server | Effect in plannotator |
|---|---|---|---|
| Plan Lead / fork | `PLANNOTATOR_DATA_DIR`, `_UPDATE_CHECK=0`, `_PORT=<the Plan's port>`, `_REMOTE=1`, `_URL_HOST=<review_host()>` (`launch.rs:139-147`; port and host from `run.rs:168-181`; the tmux spawn `plan/lead.rs:41-53`) | `SSH_*`, `PLANNOTATOR_REMOTE=1` (overridden) | remote, bound to `0.0.0.0:<plan port>`, URL `http://192.168.3.3:<port>`. Correct |
| Manager | `PLANNOTATOR_UPDATE_CHECK=0` only (`launch.rs:129-137`); `session run` removes every other inherited `PLANNOTATOR_*` (`run.rs:195-199`; contract §6.5 "Manager") | `SSH_CONNECTION`, `SSH_TTY` (seen on every live Manager pi: pids 1091007, 1642488, 3934027) | remote by SSH detection, fixed port **19432**, bound to `0.0.0.0`, **URL `http://localhost:19432`**. The bug |
| Worker | `CPD_*` only (`launch.rs:151-176`) | `PLANNOTATOR_REMOTE=1`, `SSH_*` | irrelevant: every plannotator tool is excluded |

`grep -rn PLANNOTATOR_URL_HOST` over `core/` and `cli/`: the only setters are `launch.rs:146` and `plan/lead.rs:52`, both for a Lead with a Plan. The readers are `core/src/host/addr.rs:9,17-19` and the tests.

**`review_host()` can itself return `localhost`** when no override is set and no primary IPv4 is found (`core/src/host/addr.rs:30-42`). hs-d496 says "never localhost", so a Session whose review host is `localhost` must not open a session (§5, guard G2).

**Seen in passing (out of scope, unverified cause).** Several live Leads share a port: `pi-pin`, `compact-tools` and `b2-budget` all hold 19435; `inbox` and `bacon` hold 19439; `manager` and `dev-speed` hold 19440. `port.rs:48-69` counts only Plans that are not `done` and not archived as holders, so a Lead whose Plan is `done` keeps running on a port another Plan was given. A fork also gets its Plan's port, because `run.rs:169-175` assigns by `row.plan` (I infer). Two processes on one fixed port: the second server fails with `Port N in use after 5 retries` (`pn:server/network.ts:238-244`).

## 3. Seeing a session open and end; intercepting a call

### 3.1 Signals plannotator gives

| Signal | Covers | Open | End | Source |
|---|---|---|---|---|
| `plannotator:review-result` on `pi.events` | event-API `plan-review` only | – | approved or denied (not "closed") | `pn:plannotator-events.ts:451-466` |
| `review-status` request | event-API `plan-review` only, persisted in `~/.pi/plannotator-review-status.json` | `pending` | `completed` / `missing` | `pn:plannotator-events.ts:254-256,413-421,450,462` |
| `plannotator:plan-approved` | the submit-plan flow in external execution mode | – | approval only | `pn:index.ts:852-866` |
| Tool result of `plannotator` (`details: {action, session: "pn-xxxxxx", url}`) | tool opens | **yes** | – | `pn:index.ts:1651-1655` |
| Tool result of `plannotator_submit_plan` (`details.reviewId`) | submit-plan opens | yes (no URL in details) | – | `pn:index.ts:2004-2008` |
| Decision follow-up message `Plannotator: <subject> (pn-xxxxxx) — <outcome>.` (`pi.sendUserMessage`) | tool and slash reviews | – | feedback, approved with notes, gated approval. **Not** sent for "closed", "closed with no feedback", an un-gated approval with no notes, or a close by the agent: those only `ctx.ui.notify` | `pn:index.ts:1350-1402,1450-1500,1014-1056`; heading `pn:generated/plannotator-tool.ts:358-367`; outcomes `pn:annotate-outcome.ts:18-38` |
| **Process registry** `globalThis.__plannotatorPiReviewRegistry_v1` (`openFor(owner)`, `openAll()`; entries `{id, kind: annotate\|review\|last\|plan, subject, target, url, owner, startedAt, hostControl}`) | **every** tool, slash and submit-plan review, but not event-API ones | `add` when the server is up (`pn:index.ts:934,1970`) | `remove` in **every** end path: decision, close, stop or error (`pn:index.ts:937-951,798,1983,1990`); `hostControl.status()` gives `decided` | `pn:plannotator-tool-host.ts:38-78,80-175,182-198` |
| The server's port | every session | listening | the server stops after the decision (`pn:plannotator-browser.ts:350-365`) or when pi exits (`pn:index.ts:514-526`) | an HTTP probe is ambiguous: another session can take the same fixed port (`pn:plannotator-browser.ts:235-252`) |

The registry's `owner` is `ctx.sessionManager.getSessionId()` (`pn:index.ts:907-913`). cpd starts every Lead and Manager with `--session-id <cpd Session id>` (`launch.rs:121`), or resumes its file, so **owner equals `CPD_SESSION`**. The registry lives for the pi **process**, so it survives `/reload`, `/resume` and `/new` (`pn:plannotator-tool-host.ts:5-13`), as do the servers (`pn:index.ts:521-526`). On process exit the servers die and nothing records them (`review-status` covers event-API plan reviews only).

Reaching the registry: plannotator itself relies on `globalThis` being shared between extension instances in one pi process. The cpd adapter is loaded by the same pi, so I infer it sees the same `globalThis`. This is unverified until a test in a real pi shows it.

### 3.2 Intercepting

- **`tool_call`** (pi `docs/extensions.md:106`; `types.d.ts:931-941,1089-1098`) runs before any tool and can `block` with a `reason`, as the adapter already does for `ask_user_question`, `bash` and `askguard` (`pi/lib/adapter.ts:43-55,61-72`). It sees every tool, `plannotator` included. Nested calls (codemode scripts, `ctx.executeTool`) also pass through it, with `parentToolCallId` (`docs/extensions.md:149`). So the adapter **can refuse** a `plannotator` or `plannotator_submit_plan` call before it runs.
- **`tool_result`** (`types.d.ts:985-1000,1114-1120`) can read `details` and replace `content`, so the adapter could also rewrite the URL the agent sees.
- **Slash commands cannot be intercepted.** pi gives raw input to the `input` event "unless an extension command with the same name handles it" (pi `docs/prompt-templates.md:36`). The registry still sees the reviews they open.
- **Event-API requests** from other extensions cannot be intercepted (it is a plain bus, `pn:plannotator-events.ts:403`).

## 4. The two candidate routes

### 4.1 Route (a): the adapter registers every session with cpd

**Flow.**

1. On `session_start`, Lead or Manager only, the adapter starts a tracker.
2. **Open signals:** at every `tool_result` of `plannotator` or `plannotator_submit_plan`, and every `pollMs` (the review recovery interval, `pi/lib/review.ts:140-168`), the tracker reads `registry.openFor(CPD_SESSION)`. That covers the tool, the slash commands and submit-plan.
3. **Diff:** for each id cpd does not have yet it runs `cpd review open --session <id> --pn <pn-id> --kind <annotate|review|last|plan> --subject <subject> --port <port of entry.url>`. For each id cpd has open that the registry no longer lists it runs `cpd review close`.
4. **Rows:** cpd writes a row, and E8 turns open rows into Inbox items. The URL is built by E8 from `review_host()` and the row's port, exactly as `plannotator_url` does for `plan-review` (`core/src/inbox/rules.rs:192-206`). So the item's link is right even if plannotator printed localhost, as long as the server binds `0.0.0.0` (guard G2).
5. **The guard** refuses what cannot be tracked or linked (§5).

**cpd changes.**

- **Table `plannotator_reviews`:** `session` (FK `sessions`), `pn_id` (`pn-[0-9a-f]{6}`, unique only per pi process, so the key is `(session, pn_id)`), `kind`, `subject` (1–200 characters), `port`, `opened_at`, `closed_at?`, `ended` (`decided|lost`), plus a `history_tables` row. This is an **additive** migration (A1 allows `CREATE TABLE` plus the `history_tables` insert, contract §4.5), e.g. `<UTC>-plannotator-reviews`.
- **Inbox kind:**
  - Recommended: a new kind `review-waiting`, magenta, subject `pn:<session>/<pn_id>`, label `<who> review: <subject>`.
  - Cost: `inbox_items.kind` is a `CHECK` (contract §4.4, `docs/contract.md:1296-1316`) and SQLite cannot alter a check, so the table must be rebuilt as `0013-pr-inbox` did (contract §4.5, `:1687`). That is a **breaking** migration (`DROP` is outside A1), e.g. `<UTC>-review-waiting`, declared `breaking after <n>`. `Kind` in `core/src/inbox/model.rs:21-60` is closed, so an old binary could not parse the kind anyway.
  - Alternative: widen `plan-review` (no rebuild). But its subject is `review:<plan>/<id>`, it links a Plan, and the TUI's click opens "the Lead of its Plan" (`tui/src/tree/inbox.rs:204-237`). A Manager's review has no Plan, so it would need a special case in every reader: an illegal state made representable. Not recommended.
- **Events (§5.1, every state change appends one):** `review-opened` `{pn_id, kind, subject, port}` and `review-closed` `{pn_id, ended}`. Each is one additive `INSERT INTO event_kinds` migration (contract §5.2, `:1796`), and the adapter delivers nothing for them.
- **`PROTOCOL` 7 → 8** (`core/src/events/kind.rs:38`, `pi/lib/event.ts:8`). Two kinds are added and the adapter runs a new command (contract §5.2 "The protocol number", `:1849`).
- **Commands:**
  - `cpd review open` is idempotent on `(session, pn_id)`.
  - `cpd review close [--pn <id> | --all]` is idempotent, and closing what is not open changes nothing.
  - `cpd review list --session <id>` returns the open rows the tracker compares at start-up.
- **E8:** a rule `review_waiting(conn)` next to `plan_reviews` in `core/src/inbox/rules.rs:54-64`. Condition: the row is open **and** its Session is neither archived nor `stopped`. An open/close race or a crash then never leaves an item for a pi that no longer exists. Urgency is always magenta (hs-d496). Label `<who>` comes from `seats()` (`rules.rs:96-139`).
- **TUI:**
  - `tui/src/tree/inbox.rs:184-237`: the click opens the item's Session, then copies its `url`; the OSC 8 link works as for `plan-review` (`:237`).
  - `tui/src/read/dot.rs:72-75,174`: the Session's dot.
  - `tui/src/tree/kinds.rs:17`: the kind list.
- **Manager env (the URL fix):**
  - `launch.rs:129-137`, `run.rs:168-199`: add `PLANNOTATOR_REMOTE=1`, `PLANNOTATOR_URL_HOST`, `PLANNOTATOR_PORT=<the Manager's port>`.
  - `core/src/session/port.rs:22-45`: assign a port to a Manager too, keyed by its Context (e.g. `run/<context>/plannotator-port`), with live Managers counted as holders.
  - Contract §6.5 "Manager" and the test `cli/tests/flow/context_manager.rs:155,198`, which pin "only `PLANNOTATOR_UPDATE_CHECK`", flip.
- **Explicit agent tool (optional):** set `PLANNOTATOR_AGENT_TOOL=1` for every Lead and Manager. Whether the tool exists then no longer depends on a stray `config.json` in a data dir (§1.2). Guess: the User wants the annotate tool everywhere, since hs-d496 names "an annotate review, a gated approval" as legitimate.

**Restarts.**

- **`/reload` or resume in the same process.** The registry and servers live on (`pn:plannotator-tool-host.ts:5-13`, `pn:index.ts:521-526`). The new adapter instance reconciles at `session_start`, `cpd review list` against `registry.openFor`, and closes what is gone as `lost`.
- **pi exits (quit, crash, install reload).** The servers die. The adapter's `stop()` reports `stopped` (`pi/lib/startup.ts:193-208`) and E8's "Session not stopped" condition closes the items. The next `session_start` closes the rows as `lost`. For extra safety, `session run` (Rust) can run the close on pi's exit, since it owns the process (`run.rs:167-205`). That is unverified for a SIGKILL'd wrapper.
- **plannotator restart.** plannotator is in-process, so it restarts only with pi. A plannotator update plus `/reload` can bring a registry of a new shape under a new key: the boundary parser fails, the guard refuses new sessions loudly (G3), and the old sessions end with the process.

**What can go wrong.**

1. The registry is internal. A plannotator update can rename it. Mitigation: plannotator is pinned (`~/.pi/agent/settings.json:31`, `@0.28.17`); a contract test runs the parser against the installed package's `createPiReviewRegistry` (`pn:plannotator-tool-host.ts:80`); the adapter fails closed.
2. **Poll latency:** up to `pollMs` for a slash-command review. The tool path is immediate (`tool_result`).
3. **A `review open` that fails after the session opened.** The adapter delivers the error and should then close the session with `hostControl.close()` (`pn:plannotator-tool-host.ts:108-127`), so no untracked session remains. Plan reviews (`kind: plan`) cannot be closed that way (`:110`); for those the error is delivered and the item is missing until the next poll succeeds.
4. **Event-API sessions** (`annotate`, `code-review`, `annotate-last`, `archive`) sent by any extension other than cpd's adapter are invisible. Today none is sent (unverified for `lead_pi_packages`).
5. **Fixed port.** A second session in the same pi stops the first (self-preemption, `pn:plannotator-browser.ts:235-252`), or the tool refuses it (`pn:index.ts:1612-1620`). Either way the registry stays exact. Across processes, a shared port fails loudly (§2, end).
6. **Double items.** The decision follow-up starts a turn. When it ends it is not a reply (the run was not started by the User, `pi/lib/agent-state.ts:22-25`), so no `unread` follows. Session state is untouched: the plannotator tool ends the turn (`terminate`) and the Session goes `idle`, not `blocked`, so no `dialog` item duplicates it either.

### 4.2 Route (b): a cpd tool wraps annotate; the raw tools are refused

**Flow.** The adapter registers `cpd_review` (target, gate) for Leads and Managers. It sends the event API's `annotate` (or `code-review`) request, opens a cpd row and item, waits for the request's answer, closes the row, and sends the decision to the agent itself. The `tool_call` guard refuses `plannotator` (`annotate`, `review`, `last`) and `plannotator_submit_plan` always, steering to `cpd_review`. Another way to make the raw tool absent is to set `PLANNOTATOR_AGENT_TOOL=0`.

**cpd changes.** The same table, Inbox kind, Events and PROTOCOL bump as (a), and the same Manager env and port. The adapter also needs a single fixed `PLANNOTATOR_PORT`, because the event API returns no URL (`pn:plannotator-events.ts:489-507`).

**Restarts.**

- **`/reload`.** The event-API request was answered to the old adapter instance's `respond` closure, and nothing persists it (only `plan-review` is stored, `pn:plannotator-events.ts:450,462`). The page stays open, but the decision is lost and the row stays open with no way to learn the end. The new instance can only close it as `lost` while the User may still be typing in the tab.
- **pi exit:** as (a).

**What can go wrong.**

1. **No URL.** It must be computed from a single fixed port, so Managers need one: the same work as (a), with no cross-check.
2. **Narrower than the tool.** The event-API `annotate` takes `{filePath, markdown, mode, folderPath, gate}`. There are no bundles, no live-app or URL fetching, no "Ask this session" bridge (`pn:plannotator-events.ts:198-205,489-507` against `pn:index.ts:1081-1402`). The adapter must read the file itself.
3. **DRY.** The adapter must compose the decision messages plannotator already composes (`getAnnotate*Prompt`, `pn:index.ts:1356-1401`), so its copy drifts with every plannotator release.
4. **Slash commands can't be refused** (§3.2), so a User-typed `/plannotator-annotate` still makes a session with no item, unless (b) also polls the registry, which is route (a) again.
5. **A blocked request** (the request answers only at the decision) needs its own timeout handling, unlike `request()`'s 15 s (`pi/lib/plannotator.ts:23-43`).

### 4.3 The two routes side by side

| | (a) track the registry | (b) wrap annotate |
|---|---|---|
| Covers tool, slash, submit-plan | yes | tool only (slash cannot be refused) |
| Sees "closed" and "no feedback" ends | yes (registry removal) | yes, while the process and instance live |
| Survives `/reload` | yes | no (decision lost) |
| URL | from plannotator's port plus `review_host()`; any port setting | needs a single fixed port |
| Duplicates plannotator code | no | decision prompts, file reading |
| Depends on plannotator internals | the registry (versioned key, parsed, fail closed) | the event API (also "not a stable contract", `pi/lib/plannotator.ts:5`) |

## 5. Recommendation and Jobs

**Route (a)**, with three guard rules in the adapter's existing `tool_call` handler (`pi/lib/adapter.ts:43`), for the open actions (`plannotator` `annotate`/`review`/`last`, and `plannotator_submit_plan`), Lead or Manager:

- **G1:** no live cpd run (`host.run` undefined, start-up not finished): refused, "cpd: plannotator waits for the cpd adapter to start".
- **G2:** the env cannot give a reachable link: `PLANNOTATOR_REMOTE !== "1"`, or `PLANNOTATOR_URL_HOST` is empty or a loopback name or address, or `PLANNOTATOR_PORT` is not one port. Refused with the variable named. After the env fix this fires only for a hand-started pi or a host with no IPv4 (`addr.rs:30-42`).
- **G3:** the registry is unreadable (absent, or wrong shape): refused, "cpd cannot track plannotator <version>: its review registry changed". Reported once per run, like `protocol_mismatch` (`pi/lib/protocol.ts:7-24`).

`list` and `close` pass. `ask_user_question` stays with `askguard.ts`. `cpd_plan_review` and the event-API `plan-review` are unchanged: they keep the `plan-review` kind.

**Order.** PR 1 (Manager env) can ship first and alone: it fixes the localhost link today, with no store change. PR 2 (store, command, E8, TUI) and PR 3 (adapter) follow. PR 2 must bump `PROTOCOL` on both sides in one change, because `pi/test/protocol.test.ts` ties `pi/lib/event.ts` to `core/src/events/kind.rs`. So PR 2 touches `pi/lib/event.ts` (kinds with delivery `none`, and `PROTOCOL`), coordinated with pi-pin. PR 3 is the adapter tracker and guard, owned by pi-pin's Lead.

| Job | PR | What | Files |
|---|---|---|---|
| `pnbox-docs` (docs) | 2 (and 1's §6.5 part) | Contract §2.2 (kind `review-waiting` row, subject `pn:<session>/<pn>`), §3.7 E8, §4.4 (table, `inbox_items` check), §4.5 (two migrations listed), §5.2 (two kinds, PROTOCOL 8), §6.5 (Manager gets `PLANNOTATOR_REMOTE`, `_URL_HOST`, `_PORT`, and `_AGENT_TOOL` if adopted; Manager port); commands.md: a `review` section (`open`, `close`, `list`), §13.2 (click, link, dot), §15 new subsection "Every plannotator session is tracked" (tracker, G1–G3), §21, §19 amendment row | `docs/contract.md`, `docs/commands.md` |
| `pnbox-env-tests` (red) | 1 | A Manager's launch env holds `PLANNOTATOR_REMOTE=1`, `PLANNOTATOR_URL_HOST=<review host>`, `PLANNOTATOR_PORT=<its port>`; a Manager's port is no Plan's | `core/src/host/pi/launch.rs` (tests), `core/src/session/port.rs` (tests), `cli/tests/flow/context_manager.rs:155,198`, `cli/tests/session/session.rs` (`hosts.$CPD_SESSION` style, `:1630-1640`) |
| `pnbox-env-impl` | 1 | Manager env and port | `core/src/host/pi/launch.rs`, `core/src/session/run.rs`, `core/src/session/port.rs`, `core/src/plan/lead.rs` (Manager spawn) |
| `pnbox-tests` (red, core/cli) | 2 | Migration shape (additive table, breaking `inbox_items` rebuild keeps rows and ids); `review open`/`close`/`list` idempotent with Events; E8 opens `review-waiting` magenta with `http://<review host>:<port>/`, and closes it when the row closes or the Session is `stopped` or archived; TUI click and link | `core/src/store/schema/tests.rs`, `core/src/inbox/rules.rs` (tests), `core/src/events/kind.rs` (tests), new `cli/tests/flow/review.rs`, `tui/src/tree/inbox.rs` (tests), `pi/test/protocol.test.ts` |
| `pnbox-impl` (core/cli/tui) | 2 | Two migrations (`scripts/new-migration.sh`; `SCHEMA_HASH` via `scripts/repin.sh`); `Kind::ReviewWaiting`; the rule; the commands; the Event kinds; `PROTOCOL` 8 on both sides | `core/src/store/migrations/<UTC>-plannotator-reviews.sql`, `core/src/store/migrations/<UTC>-review-waiting.sql`, `core/src/inbox/{model.rs,rules.rs}`, `core/src/events/{kind.rs,typed.rs}`, a new `core/src/review/` (or `core/src/session/review.rs`), `cli/src` (args), `tui/src/tree/{inbox.rs,kinds.rs}`, `tui/src/read/dot.rs`, `pi/lib/event.ts` (kinds and `PROTOCOL` only) |
| `pnbox-pi-tests` (red, pi-pin) | 3 | Tracker: a registry with `pn-1` runs `review open`; its removal runs `review close`; a `/reload` start closes rows the registry lacks; a bad registry shape refuses (G3). Guard: G1–G3 block, `list`/`close` pass, a nested (`parentToolCallId`) call is blocked too | new `pi/test/pntrack.test.ts`, `pi/test/pnguard.test.ts` |
| `pnbox-pi-impl` (pi-pin) | 3 | The tracker (registry parse at the boundary, diff, `cpd review` calls) and the guard | new `pi/lib/pntrack.ts`, `pi/lib/pnguard.ts`; `pi/lib/adapter.ts` (`tool_call`, `tool_result`), `pi/lib/startup.ts` (start and reconcile at `session_start`) |

**Open points for the Lead** (each a design choice the docs Job must settle; my assumption is first):

1. Kind name `review-waiting` (new kind, breaking rebuild) rather than widening `plan-review`.
2. A Manager's port keyed by its Context (`run/<context>/plannotator-port`, Managers counted as holders), not a second config range. Unverified: that Context and Plan ids can never be equal (both live under `run/`).
3. Set `PLANNOTATOR_AGENT_TOOL=1` for Leads and Managers (explicit), or leave it to `config.json`.
4. A tracked session does not mark the Session `blocked` (no `dialog` duplicate), unlike `cpd: plan review` (`pi/lib/review.ts:49`, `core/src/inbox/rules.rs:161-162`).
