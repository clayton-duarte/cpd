# t4: Can `cpd` drive plannotator for plan approval?

Date 2026-10-12. Exercised `@plannotator/pi-extension@0.28.17` (npm integrity `sha512-6OaY/J3oTRhjfQ2CeHS2AkgqPDeVq5Fa3y+qtJB/uRoT0AZoyHtl86Y+9LpxC0FmqLlqai/nof1g6oxzWsAm/Q==`; source read from the shipped tarball and from the repo clone at `94c02b8a47d5cfbe8bc435c429b94ad9503136fd`) on pi 1.1.0 and herdr 0.9.3 (session `cpd-trial`, workspace `t4`, since closed). Lead model `github-copilot/claude-haiku-4.5:low`. Wall time about 30 min.

**Deviation:** I loaded the package with `-e /tmp/cpd-trials/t4/pkg/node_modules/@plannotator/pi-extension` (the same 0.28.17 tarball, `npm install`ed into scratch), not `-e npm:…@0.28.17`. Per `docs/packages.md`, `npm:` sources install into `~/.pi/agent/npm`, which this trial must not touch. The `npm:` path is **unverified**. Pi also loaded `-ne -ns -nc` (no user extensions, skills or context files) so nothing else could change the Lead's state.

## Verdict

- **Yes, `cpd` can drive plan approval through plannotator's shared event API with plan mode off.** The adapter needs two event channels and one fixed-shape payload; both decisions (approve, deny with feedback) reached the Lead. Details are in Findings 1–2.
- **Minimal surface.** Emit `plannotator:request` with `{requestId, action:"plan-review", payload:{planContent, planFilePath?}, respond}`. The `respond` callback gets `{status:"handled", result:{status:"pending", reviewId}}`. The decision arrives on `plannotator:review-result` as `{reviewId, approved, feedback?, savedPath?, agentSwitch?, permissionMode?, answersOnly?}`. Recover with `action:"review-status"`, `payload:{reviewId}`.
- **The decision reaches `cpd` as an event forwarded by the adapter. Plannotator does not send a follow-up user message on this path.** There is no `sendUserMessage` anywhere in the event-API flow. The session JSONL shows no plannotator user message, only the `custom_message` entries my adapter wrote with `pi.sendMessage`. `cpd` must do its own delivery with `pi.sendMessage` and persist the decision in its own ledger. Don't read plannotator's files, for the reasons in Findings 7 and 8.
- **Safe to load beside a Lead that keeps its Tier.** Before and after the whole run, model (`claude-haiku-4.5`), thinking (`low`) and active tools (`read, bash, edit, write`) were identical. The session contains exactly one `model_change` and one `thinking_level_change` (both from startup), and every plannotator phase entry is `"phase":"idle"`. Caveats are in Finding 4.
- **No browser opens if the adapter sets `PLANNOTATOR_BROWSER=<executable script>`.** plannotator then spawns the script with the URL as `$1`, which also captures the URL. The `true`/`none`/`0` sentinels do not work: they are ignored and `open` is used. `PLANNOTATOR_REMOTE=1` also suppresses the browser but binds `0.0.0.0` and port 19432, so avoid it.
- **Recovery after `/reload` works, but only through polling.** The review server survives `/reload` and still accepts the decision. The replaced instance deliberately emits nothing, so the new adapter instance must ask `review-status`.
- **Concurrent reviews work.** Each gets its own random port and `reviewId`, with no interference. Don't set `PLANNOTATOR_PORT`; with a fixed port the source stops the earlier review (**unverified**).
- **Version pin: `0.28.17` exactly.** It is the latest release and there has been a release almost every day (0.28.9 on Oct 9 through 0.28.17 on Oct 12). The event API is documented in the README but is not a stable contract, so it needs a contract test on each bump (Risks).

## Acceptance checklist

| # | Item | Result | Evidence |
|---|---|---|---|
| 1 | **must** Approve path: request, URL captured, approval over HTTP, result event, follow-up message check | **pass** | Finding 1: `events.jsonl` lines `request-response`, `plannotator:review-result`; `browser-calls.log`; session JSONL |
| 2 | **must** Deny path with feedback text reaching the Lead | **pass** | Finding 2: result event carries `feedback`. The Lead (haiku) quoted it back |
| 3 | **must** No side effects: model, thinking and tools unchanged; plan mode never engaged | **pass** | Finding 3: `state` snapshots, session entry counts, `phase:"idle"` |
| 4 | **should** Recovery through `/reload` | **pass** (via polling) | Finding 5 |
| 5 | **should** Rendering of a 10-Job plan, table and Mermaid | **pass** (with gaps) | Finding 6: headless screenshots and DOM counts |
| 6 | **should** Timing | **pass** | Finding 9 |
| 7 | **should** Concurrency | **pass** | Finding 10 |

## Findings

### 0. Setup

Env set on the herdr workspace (herdr `workspace create --env`):

```
PLANNOTATOR_BROWSER=/tmp/cpd-trials/t4/fake-browser.sh   # appends "$(date) $1" to browser-calls.log
PLANNOTATOR_DATA_DIR=/tmp/cpd-trials/t4/data              # redirects ~/.plannotator
PLANNOTATOR_REMOTE=0
```

Lead started with:

```
pi -ne -ns -nc --session-dir /tmp/cpd-trials/t4/sessions --model github-copilot/claude-haiku-4.5:low \
   -e <…/pkg/node_modules/@plannotator/pi-extension> -e /tmp/cpd-trials/t4/cpd-plan-ext.ts
```

**How plannotator decides to open a browser** (`server/network.ts`, `openBrowser`, lines about 246–340):

- `isRemoteSession()` is true when `PLANNOTATOR_REMOTE` is `1`/`true`, or `SSH_TTY`/`SSH_CONNECTION` is set. If it is true and no browser is configured, nothing opens (`{opened:false,isRemote:true}`). Remote mode also binds `0.0.0.0` on port 19432.
- `PLANNOTATOR_BROWSER` or `BROWSER` set to `true|false|none|:|0|1` is treated as unset (`NOOP_BROWSER_VALUES`). In local mode this falls through to macOS `open <url>`, so it does not suppress the browser.
- `PLANNOTATOR_BROWSER` set to an executable path (contains `/`, does not end in `.app`) makes plannotator run `cmd = path, args=[url]` on darwin. This is the clean way to serve only: loopback bind, random port, and the URL lands in a file. **Demonstrated:** `browser-calls.log` got `1791844728.696072000 http://localhost:64802`, and no browser appeared.
- Glimpse (`glimpseui`) is tried only when no browser is configured.

### 1. Approve path (pass)

Command and result (trimmed):

```
/cpd-plan-review /tmp/cpd-trials/t4/plan-small.md
{"kind":"request-response","data":{"requestId":"abcff997-…","response":{"status":"handled","result":{"status":"pending","reviewId":"909f0d9e-a82e-4a92-a84e-c08c823ff72b"}}}}
browser-calls.log:  1791844728.696072000 http://localhost:64802
$ curl -s $U/api/plan | jq → {"plan":"# Plan: Add health endpoint…","serverSession":"176db6…","versionInfo":{"version":1,…},"origin":"pi",…}
$ curl -s -X POST $U/api/approve -H 'content-type: application/json' -d '{"serverSession":"176db6…","planSave":{"enabled":false}}'
{"ok":true}
{"kind":"plannotator:review-result","data":{"reviewId":"909f0d9e-…","approved":true}}
```

Session JSONL after that (the Lead was then given a prompt so pi flushed the file; without an assistant turn pi did not write the session file at all):

```
custom           customType=plannotator   data={"phase":"idle",…}                       # plannotator's own state entry, written at session_start
custom_message   customType=cpd-plan-decision  "Plan decision (event): APPROVED"       # written by MY adapter via pi.sendMessage
custom_message   customType=cpd-plan-decision  "Plan decision (event): DENIED\nFeedback: …"
message role=user  "In one line: what was the latest plan decision…"                   # my test prompt, the only user message
```

**No plannotator follow-up user message entered the session.** In source, `plannotator-events.ts:443-451` only calls `storeReviewStatus` and `emitReviewResult`. The `pi.sendUserMessage(…, {deliverAs:"followUp"})` at `index.ts:2038` is `deliverPlanDecision`, which returns early unless `phase === "planning"`, so it applies only to plan mode's own `plannotator_submit_plan` tool. The README's "Decisions are also delivered as a follow-up" does **not** hold for the event API.

Approval with notes works: `approve` with `feedback:"approve-with-notes B"` gave `{approved:true, feedback:"approve-with-notes B"}`.

### 2. Deny path with feedback (pass)

I used the exact fields the browser UI posts (`packages/editor/App.tsx:4394`: `serverSession`, `feedback`, `planSave`, `answersOnly?`, `planRevision?`, `draftGeneration?`). The feedback is Markdown in plannotator's annotation-export format.

```
curl -X POST $U/api/deny -d '{"serverSession":"…","feedback":"# Plan Feedback\n\nI've reviewed this plan and have 1 piece of feedback:\n\n## 1. Feedback on: \"Add test\"\n> please specify which test framework and add a rollback step\n\n---\n","planSave":{"enabled":false}}'
→ {"ok":true}
{"kind":"plannotator:review-result","data":{"reviewId":"83a9cc73-…","approved":false,"feedback":"# Plan Feedback\n\n…please specify which test framework and add a rollback step…"}}
```

Feedback reached the Lead: after the adapter's `pi.sendMessage({customType:"cpd-plan-decision", content, details}, {triggerTurn:false})`, I asked the Lead "what was the latest plan decision and what feedback was given?". Haiku answered: "The latest plan decision was DENIED with feedback to specify the test framework and add a rollback step for the 'Add test' item."

Notes:

- A bare deny with no feedback yields `feedback:"Plan rejected by user"` (source default).
- `answersOnly:true` appears on the event only when the reviewer just answered the plan's questions. `cpd` can ignore it.
- A repeat decision on a settled review returns `{ok:true,duplicate:true}`. The server stops about 1.5 s after the decision, and after that curl gets "connection refused" (exit 7). I saw the refusal; I did not see `duplicate:true`.
- **Simulation limit:** I posted what the UI sends, but I did not drive real annotation clicks, so the exact bytes of a UI-built `feedback` string are not captured. The format above is from `packages/ui/utils/parser.ts` and the UI source.

### 3. No side effects (pass)

`/cpd-state` snapshots (adapter reads `ctx.model`, `pi.getThinkingLevel()`, `pi.getActiveTools()`) at baseline, before each request, after decisions, and after two `/reload`s:

```
baseline / before-request / after / post-reload: model github-copilot/claude-haiku-4.5, thinking "low", activeTools ["read","bash","edit","write"]
allTools (post-reload): read,bash,powershell,edit,write,grep,find,ls,plannotator_mark_done,plannotator_submit_plan   # the two plannotator tools are registered but NOT active
```

Session file entry types: `1 session, 1 model_change, 1 thinking_level_change, 4 custom (all phase:"idle"), 5 custom_message (mine), 3 message`. No plannotator `setModel`, `setThinkingLevel` or `setActiveTools` fired. Source agrees: those calls sit behind `phase !== "idle"` paths (`index.ts` ~684–759) and `settleAgentToolActivation`, which only acts when the `plannotator` agent tool is enabled (off by default on pi).

### 4. Is it safe to load alongside a Lead that must keep its Tier? (source review plus runtime)

Hooks and registrations with plan mode off:

| Hook or registration | Effect while idle |
|---|---|
| `before_agent_start` | Returns nothing while `phase=idle`. The one exception is a one-shot "plan mode off" notice after plan mode was previously used in the session (`idleNoticePending`). It never fires on a fresh session. |
| `tool_call` | Gates writes only when `phase=planning`. |
| `session_start` / `session_tree` | `resyncPhaseFromSession` calls `pi.setActiveTools(stripPlanningOnlyTools(active))`, but only if the list differs. It did not change anything here. |
| `resources_discover` | Offers a bundled `plannotator` skill. It is `disable-model-invocation: true`, so it is not in the system prompt. I did not read the system prompt itself (the session file stores an empty system message), so I infer this from `bundled-skill.ts` comments and the startup `[Skills] plannotator` listing. |
| Tools registered | `plannotator_submit_plan`, `plannotator_mark_done`, and an optional `plannotator` agent tool. None were active. |
| Command, shortcut, flag | `/plannotator-*` commands; `Ctrl+Alt+P` toggles plan mode, so a User keypress can engage it and flip model, tools and prompts; `--plan` flag. `cpd` should never pass `--plan`. |
| "Ask this session" bridge (`createPiSessionBridgeHub`) | The review UI can send questions into the Lead session. This is browser-initiated and off in remote mode. I did not exercise it (**unverified**). |
| Network | A daily GitHub release check (`update-state.json` appeared in the data dir). `PLANNOTATOR_UPDATE_CHECK=0` disables it (read from source, not tested). |

Recommendation: load it only in the Lead. Pass `PLANNOTATOR_UPDATE_CHECK=0`. Have the adapter assert, once per Lead start, that model, thinking and active tools are unchanged by the load.

### 5. Recovery through `/reload` (pass, via polling)

Review started (`3783d23e…`), `/reload`ed the Lead (`session_shutdown`, then `session_start reason=reload`), then denied over HTTP about 5 s later:

```
curl -X POST $U/api/deny -d '{…,"feedback":"denied after reload"}' → {"ok":true}          # server survived /reload
{"kind":"review-status","data":{"reviewId":"3783d23e…","response":{"status":"handled","result":{"status":"completed","approved":false,"feedback":"denied after reload",…}}}}
{"kind":"result","data":{"via":"review-status",…}}
```

No `plannotator:review-result` event reached the new instance. The old plannotator instance's `sessionLive` is false and it emits nothing (`plannotator-events.ts:~437`). Only `review-status` recovered the decision. The new adapter instance rediscovered its pending `reviewId`s from `/tmp/cpd-trials/t4/pending.json`, a file the adapter writes. Production `cpd` must persist pending `reviewId`s itself (in the ledger) and poll `review-status` after `session_start`.

### 6. Rendering of a 10-Job plan (pass, with gaps)

I used `plan-10.md` (Dependency table, Mermaid graph, 10 Job sections with task lists and code blocks, nested lists, strikethrough, a footnote, `<details>`). I could not view the page in a real browser. I loaded it in the already-present Playwright headless shell (`chrome-headless-shell`, `playwright-core` from `~/.hermes/hermes-agent/node_modules`) with no visible window, dismissed two first-run modals, and took screenshots and DOM counts. The 23 MB single-file page also returns 200 from `/` (`text/html`).

- Rendered correctly:
  - the 10-row GFM table, with inline-code cells shown as chips;
  - the Mermaid `graph TD` as an SVG with nodes and arrows (screenshot shows J1 → J2 …);
  - fenced `ts` code with syntax colouring;
  - task-list items (shown as circles);
  - nested ordered and bullet lists;
  - blockquote, bold and strikethrough;
  - an outline of H1–H3 headings in the left Contents panel.
- `<details><summary>` rendered as a collapsed block (the inner text "hidden html" stays hidden). Treat raw HTML as unsupported in plans.
- **Footnotes do not render:** `[^1]` and `[^1]: footnote text` appear as literal text.
- A fresh browser profile shows two first-run modals ("Choose how plans look" and a news/update card). They are stored in browser state and are a one-time annoyance for the User.
- Other console noise: one 404 for a resource (not identified).

`/api/plan` serves the raw Markdown (`plan`, 1910 chars for plan-10) plus `serverSession`, `versionInfo`, `origin`, `projectRoot`. Rendering is entirely client-side.

### 7. How `cpd` learns the decision

Options found (all observed):

- **Event forwarded by the adapter (recommended).** In-process, `plannotator:review-result` is delivered about 12 ms after the HTTP decision. The adapter forwards it to `cpd` (its own file, socket or ledger) and to the Lead via `pi.sendMessage`.
- **`~/.pi/plannotator-review-status.json`**: `{reviewId: {status:"pending"|"completed", approved, feedback,…}}`, written atomically (temp file plus rename). Observed content after my run listed all 5 reviewIds as `completed`. The path is hardcoded to `homedir()/.pi` (`plannotator-events.ts:255`; only a test seam overrides it), is shared by every pi process, and is never pruned. Use it only as plannotator's own recovery store, via `review-status`.
- **`$PLANNOTATOR_DATA_DIR/feedback/<project>/index.jsonl`**: append-only archive; `project` comes from the cwd name (`work`), so it is not keyed by `reviewId`. Entry seen: `{"decision":"approved","target":{"slug":"plan-add-health-endpoint-2026-10-12","planVersion":1,…},"counts":{…}}`. Not usable for decision delivery.
- **A session entry**: plannotator writes none for event-API decisions. Only the adapter's `custom_message` carries it.

### 8. Side-effect files (cleaned up)

- **`~/.pi/`**: created `~/.pi/plannotator-review-status.json` (did not exist before; `~/.pi` held only `agent`). **Removed** at the end; `find ~/.pi -path ~/.pi/agent -prune -o -print` now prints only `~/.pi`. I did not touch `~/.pi/agent`.
- **`~/.plannotator`**: not created (`PLANNOTATOR_DATA_DIR` redirected everything: `history/`, `feedback/`, `drafts/`, `update-notes/`, `update-state.json`, all under `/tmp/cpd-trials/t4/data`).
- Pi session files went to `/tmp/cpd-trials/t4/sessions` (`--session-dir`).
- Scratch under `/tmp/cpd-trials/t4/` (extension, logs, screenshots, `chrome-prof`, `pkg/` with 192 npm packages) was left in place for inspection.

### 9. Timing (pass)

Wall times in ms, from the adapter log and shell timestamps:

| Step | Cold (first review in the process) | Warm |
|---|---|---|
| Request, then `respond` `{pending, reviewId}` | 1.87 s (lazy-loads the server graph and HTML) | 0.11 s |
| Request, then URL logged by the browser script | 2.2 s | 0.2 s |
| Decision POST, then `plannotator:review-result` | 12 ms | 11 ms |
| Decision after `/reload`, then recovered via `review-status` | 0.97 s (bounded by my 1 s poll interval) | n/a |
| Decision, then server gone | about 1.5 s (source `delay(1500)`; connection refused afterwards) | |

### 10. Concurrency (pass)

Two plan reviews requested 3 s apart without deciding either: plan-small (`96c29548…`, port 64858) and plan-10 (`248101bc…`, port 64860). Both served simultaneously (`/api/plan` returned the right plan on each port). Deciding B (approve with notes) then A (deny) produced two result events with the correct, distinct `reviewId`s and payloads:

```
{"reviewId":"248101bc-…","approved":true,"feedback":"approve-with-notes B"}
{"reviewId":"96c29548-…","approved":false,"feedback":"deny A"}
```

With the default random-port setup, concurrent reviews don't collide. With a fixed `PLANNOTATOR_PORT`, `startServerWithSelfPreemption` (`plannotator-browser.ts` ~240) stops the earlier same-process review on port collision (**read from source, not tested**).

## Recipes

**Env for the Lead pane** (set by `cpd`; no browser, no home-dir writes except the one file below, no update check):

```sh
PLANNOTATOR_BROWSER=$CPD_STATE/plannotator-open.sh   # chmod +x script: records "$1" (the URL) and returns
PLANNOTATOR_DATA_DIR=$CPD_STATE/plannotator          # do NOT leave this at ~/.plannotator
PLANNOTATOR_REMOTE=0
PLANNOTATOR_UPDATE_CHECK=0                            # from source; untested
# Do not set PLANNOTATOR_PORT. Do not pass --plan.
```

Launch: `pi -e npm:@plannotator/pi-extension@0.28.17 -e <cpd adapter>` (the `npm:` form is unverified here; I tested the same tarball by path).

**Adapter (tested; the full file is `/tmp/cpd-trials/t4/cpd-plan-ext.ts`):**

```ts
// request
pi.events.emit("plannotator:request", {
  requestId: randomUUID(),
  action: "plan-review",
  payload: { planContent, planFilePath },          // planContent must be a non-empty string
  respond: (r) => {                                // {status:"handled", result:{status:"pending", reviewId}}
    if (r.status === "handled") persistPending(r.result.reviewId);   // cpd ledger, not a temp file
    else fail(r);                                  // {status:"unavailable"|"error", error}
  },
});

// result (live session)
pi.events.on("plannotator:review-result", (e) => {
  // e = {reviewId, approved, feedback?, savedPath?, agentSwitch?, permissionMode?, answersOnly?}
  recordDecision(e);                               // forward to cpd first
  pi.sendMessage({ customType: "cpd-plan-decision", content, display: true, details: e }, { triggerTurn: false });
});

// recovery: after session_start (reload / resume), poll each pending reviewId
pi.events.emit("plannotator:request", {
  requestId: randomUUID(), action: "review-status", payload: { reviewId },
  respond: (r) => { /* r.result: {status:"pending"} | {status:"completed", reviewId, approved, feedback?, …} | {status:"missing"} */ },
});
// Clear the poll interval on session_shutdown.
```

**Simulating the browser** (all validated):

```sh
URL=$(awk '{print $2}' "$BROWSER_LOG" | tail -1)               # from the PLANNOTATOR_BROWSER script
S=$(curl -s $URL/api/plan | jq -r .serverSession)              # stale-tab guard; body without it is also accepted
curl -s -X POST $URL/api/approve -H 'content-type: application/json' -d "{\"serverSession\":\"$S\",\"planSave\":{\"enabled\":false}}"
curl -s -X POST $URL/api/deny    -H 'content-type: application/json' -d "{\"serverSession\":\"$S\",\"feedback\":\"…\",\"planSave\":{\"enabled\":false}}"
```

**Headless render check** (no window): `chrome-headless-shell` plus `playwright-core`, see `/tmp/cpd-trials/t4/shot.cjs`.

## Risks and unknowns

- **The event API is not a stable contract.** The README documents it, but the package releases roughly daily (0.28.9 on 2026-10-09 through 0.28.17 on 2026-10-12). `PlannotatorReviewResultEvent` has already gained fields (`answersOnly`). Pin an exact version (`npm:…@0.28.17`), forbid `pi update` for it, and add a smoke test that emits a plan-review, decides over HTTP, and asserts the event shape. Run that test before each bump.
- **`~/.pi/plannotator-review-status.json` is a hardcoded, global, unpruned file.** It grows with every review and is shared across pi processes. It cannot be redirected. `cpd` should treat it as plannotator's private recovery store.
- **Decision loss window.** If the whole pi process dies, the review server dies with it (the port is freed). An open browser tab then shows "session ended" (from source, `closeKeepingDraft`; not tested), and the decision is lost. Re-request on resume.
- **Recovery needs polling.** There is no push after `/reload`. The poll interval bounds latency (I used 1 s).
- **UI-built feedback format unverified.** I posted the documented shape, not real UI-composed annotations. `cpd` should pass the `feedback` string through to the Lead opaquely.
- **First-run modals** and a 23 MB page: the first load can take seconds on slower machines (I measured 200 OK on loopback but not render time).
- **Update check network call** by default; disabled with `PLANNOTATOR_UPDATE_CHECK=0` (source only).
- **`Ctrl+Alt+P` in the Lead** engages plan mode and would change model, tools and prompts. I did not test it; consider documenting it or checking for it in the adapter.
- **"Ask this session" bridge** can inject text into the Lead from the browser. I did not test it; evaluate before relying on a locked-down Lead.
- **Footnotes and raw HTML are not rendered.** Keep plans to GFM, tables, task lists, code and Mermaid.

## Escalations

None. Every blocker had a workaround within the isolation rules. The one open choice is whether `cpd` should accept the `npm:` install location (`~/.pi/agent/npm`). I avoided it by loading the same version from a scratch path. If you want the exact `-e npm:@plannotator/pi-extension@0.28.17` form tested, that needs permission for pi to write to `~/.pi/agent/npm`.
