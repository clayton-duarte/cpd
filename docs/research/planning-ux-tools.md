# Planning UX: what ask_user_question and plannotator are, from source

Evidence for Plan `plan-ux` (Job `tools`, attempt `tools-a1`). Facts only, no recommendations.

Sources read (all installed, read-only; line numbers are those of the files as installed):

| Short name | Path |
|---|---|
| AUQ | `~/.pi/agent/npm/node_modules/@juicesharp/rpiv-ask-user-question` (`package.json` version `2.13.8`) |
| PN | `~/.pi/agent/npm/node_modules/@plannotator/pi-extension` (0.28.17; `server/serverPlan.ts` is the plan-review server) |
| PN-html | `PN/plannotator.html`, the bundled review page: one 23.6 MB minified file of 20,601 lines. Cited as `plannotator.html:<line>:<column>` (found by search; column is 1-based). |
| cpd | this repo, `pi/lib/*.ts`, `pi/skills/*`, `docs/commands.md` (§ numbers), `docs/contract.md` |

Conventions: "unverified" marks a claim not read in source or not run. "Inferred" marks a reading of code I did not execute. I executed no AUQ dialog and no plannotator page.

## 1. ask_user_question limits

The tool is `ask_user_question`, registered at `AUQ/ask-user-question.ts:375-458`.

### 1.1 Schema limits (the model sees these; pi validates them before `execute`)

| Limit | Value | Citation |
|---|---|---|
| Questions per call | min 1, max 4 | `tool/types.ts:4` (`MAX_QUESTIONS = 4`), `:81-85` (`minItems: 1`, `maxItems: MAX_QUESTIONS`); runtime re-check `tool/validate-questionnaire.ts:20-25` |
| Options per question | min 2, max 4 (the model's own options; the `Type something.` row is added on top) | `tool/types.ts:5-6`, `:66-71`; runtime min re-check `validate-questionnaire.ts:36-38` (no runtime max check: the schema alone enforces 4) |
| Option `label` | max 60 characters ("hard limit, requests over the limit are rejected"); the prompt asks for 1-5 words | `types.ts:8`, `:41-44` |
| `header` | max 16 characters | `types.ts:7`, `:62-65` |
| `question` | no length limit in the schema (`Type.String` with a description only) | `types.ts:58-61` |
| Option `description` | no length limit; required field | `types.ts:45-48` |
| Option `preview` | optional string, no length limit in the schema; rendered as markdown | `types.ts:49-54`, `ask-user-question.ts:373` |
| `multiSelect` | optional boolean, default `false` | `types.ts:72-78` |
| Unique question text within a call | enforced (`duplicate_question`) | `validate-questionnaire.ts:29-33` |
| Unique option labels within a question | enforced (`duplicate_option_label`) | `validate-questionnaire.ts:44-50` |
| Reserved option labels | `Other`, `Type something.`, `Next`; any of them as an authored label is refused (`reserved_label`), checked before the duplicate-label check | `types.ts:37`, `validate-questionnaire.ts:8,41-43`; sentinel labels `row-intent.ts:81` (`Type something.`) and `:91-92` (`Next`) |
| Total text across a call | no limit in AUQ. The 1500-character cap is cpd's (section 3.3) | n/a |
| Line terminators | `\r\n` becomes `\n`, a lone `\r` is deleted, in `question`, `header`, `label`, `description`, `preview`, before validation | `tool/normalize-params.ts:20-22`, `:48-55` |
| Double-encoded arguments | a JSON string where an array/object belongs is decoded before validation | `normalize-params.ts:67-77` (`prepareQuestionArguments`), registered `ask-user-question.ts:385` |

Every refusal returns a normal tool result with `details.error` set and `cancelled: true`, never a thrown error (`ask-user-question.ts:394-401`). The error codes are `no_ui`, `no_custom_ui`, `no_questions`, `empty_options`, `too_many_questions`, `duplicate_question`, `duplicate_option_label`, `reserved_label`, `session_load_failed`, `stale_module_cache` (`types.ts:124-134`).

### 1.2 Previews

- Single-select only. The tool description says so (`ask-user-question.ts:373`). No validator rejects a preview on a multiSelect question (`validate-questionnaire.ts:19-55` has no preview check); the pane simply renders the plain option list for a multiSelect question and ignores previews (`view/components/preview/preview-pane.ts:151`, `:252`, `:261`).
- Layout: side by side (options left, preview right) only when the terminal and the pane are at least 100 columns wide; otherwise stacked (`view/components/preview/preview-layout-decider.ts:7`, `:36-38`). Left column floor 30 columns and at most half the pane (`:16`, `:18`); preview column floor 45 columns (`:20`).
- Rows: the preview box is capped at 20 rows side by side and 15 stacked (`view/components/preview/markdown-content-cache.ts:8`, `:10`). The box border takes 2 rows (`preview-box-renderer.ts:8`) and a notes-affordance line takes 2 more (`markdown-content-cache.ts:13`), so content gets 20-2-2 = 16 rows side by side and 15-2-2 = 11 stacked (`preview-block-renderer.ts:40-43`). Content past that is cut and the box's bottom border says `✂ ── N lines hidden ──` (`preview-box-renderer.ts:51-56`). I found no key in the router that scrolls a preview (`state/key-router.ts` routes only the keys listed in 1.5): treat "the preview does not scroll" as inferred.
- Content width: the box is at least 40 columns of content (`preview-box-renderer.ts:14`) and at most the preview column; longer lines are truncated, not wrapped (`renderBorderedBox`, `preview-box-renderer.ts:48`, uses `truncateToWidth`). Fence markers (three backticks) are stripped (`:22-27`).
- Under a smaller row allowance (the dialog is taller than the screen) the preview box shrinks further, the affordance row goes first (`preview-block-renderer.ts:44-62`).
- A chosen option's preview text is echoed back to the model: `selected preview: <preview>` (`tool/response-envelope.ts:59`; set at `state/state-reducer.ts:214-217`).

### 1.3 Free text and notes

| Feature | Behaviour | Citation |
|---|---|---|
| `Type something.` row | appended automatically to every question, single and multi select; the model must not author it | `state/row-intent.ts:81-89` (`autoAppendOnSingleSelect: true`, `autoAppendOnMultiSelect: true`); `ask-user-question.ts:335-345` |
| Free-text answer, single select | answer `{kind: "custom", answer: <text or null>}` | `state/key-router.ts:79-90` (`buildSingleSelectAnswer`); `tool/types.ts:107-112` |
| Free-text answer, multi select | `{kind: "multi", selected: [labels], answer: <custom text or null>}` | `state/state-reducer.ts:210-212`, `tool/format-answer.ts:20-25` |
| Per-question note | key `n` on any question tab (single or multi select, with or without preview); stored as `answers[].notes`; the model receives `user notes: <text>` | `state/key-router.ts:17`, `:342-344`; `types.ts:113`; `response-envelope.ts:60` |
| Global note | key `n` on the Submit tab (only exists when there are 2 or more questions); `result.globalNote`; the model receives `global note: <text>.` | `key-router.ts:232-234`; `types.ts:139-149`; `response-envelope.ts:43`; `questionnaire-session.ts:96` (`isMulti = questions.length > 1`) |
| Length limit on typed text or notes | none found in AUQ (unverified: pi-tui's Input/Editor widget may have its own, not read) | n/a |
| External editor for the inline text | supported (`input_edit`, `$EDITOR` from pi settings) | `key-router.ts:196`; `ask-user-question.ts:225-241` |

### 1.4 How answers come back

The tool result has `content[0].text` (what the model reads) and `details` (a `QuestionnaireResult`, `tool/types.ts:136-157`). Built by `buildQuestionnaireResponse` (`response-envelope.ts:19-45`):

```text
User has answered your questions: "<question 1>"="<answer 1>". selected preview: <preview>. user notes: <note>. "<question 2>"="<a, b>". unanswered: "<question 3>". global note: <note>. You can now continue with the user's answers in mind.
```

Prefix and suffix constants: `response-envelope.ts:5-6`. Per-answer segment: `:57-62`. Scalar by kind (`format-answer.ts:20-30`): multi = selected labels plus any custom text joined by `, `; custom = the typed text or `(no input)`; option = the label.

`details` carries `answers: [{questionIndex, question, kind: "option"|"custom"|"multi", answer, selected?, notes?, preview?}]`, `cancelled`, `globalNote?`, `unanswered?` (indexes), `error?` (`types.ts:107-157`).

Partial submit is allowed: on the Submit tab, Submit always submits even when questions are unanswered (`key-router.ts:218-223`); the unanswered ones are named in `unanswered: "<question>", ...` (`response-envelope.ts:40`, `:49-52`).

### 1.5 Esc, cancel, timeout, blocking

- Esc (`tui.select.cancel`) cancels the whole questionnaire from every state except the notes editor. In the notes editor Esc only closes the editor (`key-router.ts:171`). Cancel from the question tabs, the Submit tab, the Type-something input and the collapsed view: `key-router.ts:166`, `:197`, `:211`, `:265`, `:280`; reducer `state/state-reducer.ts:303` (`doneFor(..., cancelled = true)`). The Submit tab also has a Cancel row (`key-router.ts:223`).
- What the model receives on cancel: text `User declined to answer questions`, `details.cancelled = true`, partial `answers` and any `globalNote` kept in `details` only (`response-envelope.ts:4`, `:20-26`). A submit with zero answers and no global note gets the same decline text (`:44-46`).
- Timeout: none. I searched all AUQ `.ts` files for `timeout`, `setTimeout`, `signal`; the only timer is the 2 s pre-warm of the render module (`ask-user-question.ts:114`, `:331`) and the tool ignores its `_signal` argument (`:387`). So the dialog waits until the User answers or cancels. An abort of the agent turn is not wired to the dialog (unverified at pi level).
- Blocks the agent: yes. `execute` awaits `ctx.ui.custom(...)` (`ask-user-question.ts:284`, `:439`); the tool call does not return until the dialog resolves. It emits `rpiv:ask-user:blocked {active: true}` while open (`:60-63`, `:436`, `:454`) and a terminal bell when stdout is a TTY (`:105-111`).
- Without a UI (`ctx.hasUI` false): the tool returns `Error: UI not available (running in non-interactive mode)` with `error: "no_ui"` (`:66-68`, `:392`), and the reconciler removes the tool from the active set so the model never sees it (`reconcile.ts:25-39`).
- RPC hosts (`ctx.mode === "rpc"`): a sequential select/input walker replaces the TUI (`rpc-fallback.ts:88-98`, `ask-user-question.ts:412-414`): no tabs, no side-by-side preview (previews folded into the title, truncated to 600 characters each, `rpc-fallback.ts:36`, `:72-79`), multi-select is a typed list of numbers like `1,3; custom` (`:30-33`, `:164-203`). A dismissed dialog cancels the whole questionnaire (`:82-84`, `:94`).
- Custom UI not available: tool returns `no_custom_ui`, `session_load_failed` or `stale_module_cache` text telling the model the User never saw the questions (`ask-user-question.ts:86-93`).

### 1.6 Rendering: where it shows, wrapping, scrolling

- Where: by default inline in pi's editor slot (the transcript keeps scrolling). When another questionnaire already holds the slot, or another overlay is open, it opens as an overlay at the bottom, full width (`presentation.ts:80-91`, `:93-122`; `ask-user-question.ts:286-299`, `:432-445`).
- Row budget inline: `min(terminalRows, max(12, floor(terminalRows * 0.7)))` (`presentation.ts:10-11`, `:59-60`); overlay and host-allocated panes use all rows (`:17`, `:26-35`).
- Wrapping: option labels and descriptions wrap to the pane width (`view/components/wrapping-select.ts:280`, `:294`, `wrapTextWithAnsi`). The question text is part of the dialog body (not read line by line in this Job; unverified whether the question text is windowed or wrapped).
- Scrolling: when the dialog is taller than its row budget, the middle region is windowed around the focused option with `↑`/`↓`/`↕` markers; arrows never cover the focused row (`view/scroll-window.ts:14`, `:34-55`, `:61-73`; `view/dialog-builder.ts:225-236`).
- Collapse: `Ctrl+]` (configurable, `off` disables) hides the overlay or shrinks the inline dialog to one row so the transcript can be read; while collapsed only Esc and the toggle work (`config.ts:9-10`, `key-router.ts:162-167`, `ask-user-question.ts:155-188`).
- Footer hint line: `Enter to select · ↑/↓ to navigate · n to add notes · Tab to switch questions · Esc to cancel` (`view/dialog-copy.ts:5-12`, `:39-42`). Locales shipped: de, en, es, fr, pt, pt-BR, ru, uk, zh (`locales/`).
- Tab bar and Submit tab: only when 2 or more questions (`questionnaire-session.ts:96`, `build-questionnaire.ts:233-237`). The Submit tab shows a "Review your answers" heading and an incomplete-answers warning (`view/dialog-copy.ts:49-51`).
- Tool prompt text steers the model: group questions in one call, do not stack calls, make the recommended option first with `(Recommended)` appended (`ask-user-question.ts:347-364`).

## 2. plannotator as cpd uses it

cpd uses plannotator only through the shared event API: one `plan-review` request, `review-status` polling and the `plannotator:review-result` event (`pi/lib/plannotator.ts:23-43`, `pi/lib/review.ts:44`, `:146`). It does not use plannotator's plan mode, its `plannotator_submit_plan` tool, or its slash commands.

### 2.1 Event API (what plannotator exposes to other extensions)

| Item | Detail | Citation |
|---|---|---|
| Channels | request `plannotator:request`; result `plannotator:review-result`; handoff `plannotator:plan-approved` | `PN/plannotator-events.ts:83-85` |
| Actions | `plan-mode`, `plan-review`, `review-status`, `code-review`, `annotate`, `annotate-last`, `archive` | `plannotator-events.ts:88-95` |
| `plan-review` payload | `{planContent: string (required, non-blank), planFilePath?, origin?}`. The handler passes only `planContent` on to the server: `planFilePath` is read by nobody in this path | type `:132-136`; handler `:443-449` (`startPlanReview(ctx, planContent)`) |
| Answer to `plan-review` | `{status: "handled", result: {status: "pending", reviewId}}` at once; the decision comes later | `:467-473` |
| Result event | `{reviewId, approved, feedback?, savedPath?, agentSwitch?, permissionMode?, answersOnly?}` | `:143-153`, emitted `:465` |
| `review-status` | `{status: "pending"}`, `{status: "completed", ...result}` or `{status: "missing"}`, kept in `~/.pi/plannotator-review-status.json`, written atomically | `:162-169`, `:254-256`, `:289-299`, `:413-421`, `:450`, `:462` |
| A decision outlives the session | recorded first, emitted only when the opening session is still live; the replacement session asks `review-status` | `:361-365`, `:461-465` |
| Event timeout | `PLANNOTATOR_TIMEOUT_MS = 5000` is declared and never referenced: `grep -n PLANNOTATOR_TIMEOUT_MS *.ts` in `PN` finds only its declaration (`.ts` files only; the minified bundle was not searched); cpd waits 15 s for `respond` | `plannotator-events.ts:86`; cpd `pi/lib/follower.ts:18` (`requestTimeoutMs: 15_000`), `pi/lib/plannotator.ts:25` |
| Needs a UI | without `ctx.hasUI` the start throws `Plannotator browser review is unavailable in this session.` | `PN/plannotator-browser.ts:378-380`, `:381-384` |

### 2.2 What opens and where

- A local HTTP server per review, in the Pi process: `startPlanReviewServer` (`server/serverPlan.ts:125`), listening via `listenOnPort` (`:683`).
- Port: `PLANNOTATOR_PORT` (a port or an inclusive range) if set; else 19432 in remote mode; else a random port (`server/network.ts:15`, `:105-148`). A taken port is retried 5 times, 500 ms apart (`:194-195`), after one self-preemption attempt that stops this process's older sessions (`plannotator-browser.ts:235-252`).
- Host: remote mode binds `0.0.0.0`, local mode `127.0.0.1` (`network.ts:151-153`). Remote mode = `PLANNOTATOR_REMOTE`, else SSH env vars (`network.ts:68-95`). The advertised URL is `http://localhost:<port>`, or `http://<PLANNOTATOR_URL_HOST>:<port>` in remote mode only (`network.ts:177-192`). Requests with a Host header outside an allowlist get 403 (`server/request-host-guard.ts:53-103`).
- The page: a React app served from the bundled `plannotator.html` (`serverPlan.ts:678`); `GET /api/plan` returns `{plan, serverSession, origin, previousPlan, versionInfo, sharingEnabled, repoInfo, projectRoot, serverConfig, ...}` (`:372-402`).
- Browser: `openBrowser(url)` is called without awaiting; in RPC or remote mode the URL is also sent as a pi notification `[Plannotator] <url>` (`plannotator-browser.ts:254-260`, `:340-345`). cpd's `session run` sets `PLANNOTATOR_PORT`, `PLANNOTATOR_REMOTE=1` and `PLANNOTATOR_URL_HOST` for each Lead (docs/commands.md:1819; docs/contract.md:2057, :2111); the User reaches the page from the Inbox or Plan-view link (section 3.5).
- Per-plan storage: `PLANNOTATOR_DATA_DIR=$CPD_HOME/run/<plan>/plannotator` (docs/contract.md:2057). Observed for this Plan: `drafts/`, `feedback/`, `history/`, `plans/` under `~/.cpd/run/plan-ux/plannotator/`.

### 2.3 What the User sees and can do (the page)

The server-side facts below are read from source; the page's own behaviour comes from the minified bundle (`PN-html`) and is limited to what a string search shows. I did not open the page.

| Feature | Evidence |
|---|---|
| Rendered markdown of the whole plan (the `planContent` string), including Mermaid diagrams (689 `mermaid` hits in the bundle) | `serverPlan.ts:372-402` (`plan`); `plannotator.html` (bundle) |
| Annotation types: `DELETION` ("Remove this" with the quoted text), `COMMENT` (inline, on a selection), `GLOBAL_COMMENT` ("General feedback about the plan"), quick labels | `plannotator.html:437:161` (export switch), `:428:41` |
| Quick labels (preset one-click comments, each with a tip sent to the agent): Looks good, Clarify this, Missing overview, Verify this, Give me an example, Match existing patterns, Consider alternatives, Ensure no regression, Out of scope, Needs tests, Nice approach | `plannotator.html:654:1647` |
| Reply threads on annotations ("**Reply (author):**"), attached images, sticky/hover annotation toolbar | `plannotator.html:437` (export of replies and images) |
| Feedback export format: `# Plan Feedback`, then `I've reviewed this plan and have N pieces of feedback:`, then one numbered section per annotation (`## 1. ...`, with a `(location)` prefix, the quoted text and `> comment`), then `---` | `plannotator.html:452:8` (`i_t = (e,t,n=[],a="Plan Feedback",i="plan",...)`), `:437:161` |
| Anchors: each annotation carries a block id and a location string; the export prefixes `(<location>)` before the annotation text. The exact location format is in minified code I did not decode (unverified) | `plannotator.html:437` |
| Decision buttons: `Approve` (no changes), `Approve with a note…` (when approval notes are supported), `Request changes…` (write overall feedback), and `Send answers` when the reviewer only answered the plan's questions | `plannotator.html:8821:774`, `:8821:1031`, `:8660:4429` |
| Plan diff against the previous version of the same plan, with word-level diff blocks and per-block annotation (`plan-diff`) | `serverPlan.ts:190-193` (`previousPlan`), `:372-402`; `plannotator.html:6702:2245` |
| Version history: list and fetch earlier versions (`/api/plan/versions`, `/api/plan/version?v=`) | `serverPlan.ts:351-371`; `plannotator.html:6513:4433` |
| `:::question` blocks: a plan can embed questions (`question`, `question-multi`, `question-text`) that the reviewer answers inline; answers come back in the feedback, with `answersOnly: true` when that is the only feedback | `PN/generated/question-block.ts:1-60`; `serverPlan.ts:76`, `:651`, `:673`; `plannotator-events.ts:150-153`; `plannotator.html:8660:4429` |
| Draft persistence: unsent annotations are saved as a draft keyed by a hash of the plan text and survive a page reload | `serverPlan.ts:273`, `:447` (`/api/draft`); `plannotator.html:6513:13505` |
| Stale tab guards: a decision from a tab that loaded another server's session is refused 409 `session_mismatch`; a decision on a plan revised after the tab loaded it is refused 409 `plan_revised` | `serverPlan.ts:246`, `:301-312`, `:521-529`, `:626-634` |
| Other page features present in the server or bundle: Ask AI panel ("Ask this session" answers through the Pi session), image upload, linked-doc and Obsidian-vault reference browsing, skill references, VS Code diff, share links, Obsidian/Bear/Octarine export on approve, Table of Contents, vim mode | `serverPlan.ts:443-476`, `:461-471`, `:555-585`; `plannotator.html:689:1643`, `:677:27462`, `:525:50181`, `:6706:5941`, `:8669:10981`, `:6446:62835`, `:6701:17289`, `:677:18365` |

Diff base (inferred, not run): the history key is `generateSlug(plan)`, the first markdown heading sanitised plus today's date (`PN/generated/storage.ts:48-55`). `saveToHistory` numbers versions inside that slug and returns the same version when the content is identical (`storage.ts:228-253`). `previousPlan` is the previous version of the same slug (`serverPlan.ts:190-193`). So the plan diff exists only when the earlier review had the same first heading and date. cpd's reviewed file opens with `# <review title>` (docs/commands.md:689); the observed history directories carry slugs like `plan-cross-repo-2026-10-19/` with versions `005.md`, `006.md` (`~/.cpd/run/cross-repo/plannotator/history/cpd/`), i.e. history accumulates when the heading repeats.

### 2.4 What comes back to the agent

| Outcome | What plannotator returns | What cpd takes | Citation |
|---|---|---|---|
| Approve | `{approved: true, feedback?}` (`feedback` is the "approve with note" text; plus `plan`, `savedPath`, `agentSwitch`, `permissionMode`) | `reviewId`, `approved`, `feedback` only | `serverPlan.ts:514-618`; cpd `pi/lib/plannotator.ts:75-78` |
| Request changes | `{approved: false, feedback}`; feedback = the exported annotation text or `Plan rejected by user` if the body had none | same three fields | `serverPlan.ts:638-672` |
| Only answers | `{approved: false, answersOnly: true, feedback}` | `answersOnly` is dropped by cpd's parser; cpd treats it as a rejection with feedback | `serverPlan.ts:651`, `:673`; cpd `plannotator.ts:75-78` |
| Annotation anchors as data | not returned as data in the event API: only the feedback string; anchors live inside that string | feedback string written to `feedback-<review>.md` | `plannotator-events.ts:143-153`; cpd `review.ts:108-113` |
| Saved copy | `savedPath` of a final snapshot and the annotations file, unless the page disables plan save | ignored | `serverPlan.ts:591-603`, `:660-670` |
| Close the tab, no decision | Nothing happens. The server keeps running; no timeout in the plan server (searched `serverPlan.ts` and `plannotator-browser.ts` for timers: only the 1.5 s teardown after a decision). Review stays `pending` | cpd's Lead stays `blocked`; the Inbox item stays | `plannotator-browser.ts:359`, `:398-400`; `serverPlan.ts:249-251`; cpd docs/commands.md:2599 |
| Decided twice | second decision answers `{ok: true, duplicate: true}` | n/a | `serverPlan.ts:525-528`, `:630-633` (`decisionSettled`) |
| Session reload | tab survives; the replacement session polls `review-status` | cpd `recover` polls every `pollMs` (1000) | `plannotator-events.ts:361-365`; cpd `review.ts:139-168`, `follower.ts:18` |

Size limits: none found. `readBody` accumulates the whole request body with no cap (`server/helpers.ts:11-14` and following lines), and `plan-review` checks only that `planContent` is a non-blank string (`plannotator-events.ts:444-448`). Whether the browser handles a 1 MB plan is unverified (section 3.2 lists plans that large).

Timeouts: the plan server has no decision timeout (above). The only timeouts are cpd's 15 s wait for plannotator's `respond` (`pi/lib/plannotator.ts:25`) and plannotator's 5 s constant (`plannotator-events.ts:86`, unused for `plan-review`).

Blocks the agent: no. The request answers at once with `pending`; the decision arrives later as an event (`plannotator-events.ts:467-473`). cpd's `cpd_plan_review` tool "Returns at once" (`pi/lib/review.ts:177`) and the Lead stops and waits for a `cpd-plan-decision` follow-up message (`review.ts:58-60`, `:67`). The Session shows `blocked` with the reason `plan-review:<review>` labelled `cpd: plan review` (`review.ts:49`).

### 2.5 plannotator features cpd does not use

From `PN/index.ts`, `PN/plannotator-events.ts`, `PN/README.md`:

- Plan mode state machine idle → planning → executing (`/plannotator-plan-mode`, `--plan`, `Ctrl+Alt+P`): `index.ts:871`, `:1658`; README "Plan mode". cpd never enters it.
- The `plannotator_submit_plan` tool and `plannotator_mark_done` tool (`index.ts:1667`, `:1708`; execution tracking with `[DONE:n]`). cpd's Workers are launched with them excluded (docs/contract.md:2062).
- `plan-mode` action (`plannotator-events.ts:429-442`).
- `code-review` action and `/plannotator-review` (git, GitButler, JJ, PRs, patch files): `plannotator-events.ts:171-196`, `:476-488`, `index.ts:1510`.
- `annotate` action and `/plannotator-annotate` (markdown, folders, URLs, live proxy of a running app; `gate: true` adds an Approve button): `plannotator-events.ts:198-213`, `:489-508`, `index.ts:1518`.
- `annotate-last` and `/plannotator-last` (annotate the agent's last message): `plannotator-events.ts:509-522`, `index.ts:1533`.
- `archive` browser of saved plans and decisions: `plannotator-events.ts:215-221`, `:523-527`.
- The `plannotator` agent tool (open/list/close reviews; off unless `PLANNOTATOR_AGENT_TOOL=1`): `index.ts:412`, `:1552-1560`, README "The `plannotator` tool".
- The `plannotator_inbox` tool (its own Inbox with threads and `wait_for_reply`; off by default): README "The `plannotator_inbox` tool", `PN/inbox.ts`.
- External execution handoff `plannotator:plan-approved` (`executionMode: external`): `plannotator-events.ts:155-160`, README.
- Live revision of an open review: `updatePlan` pushes a revised plan into the open tab, which keeps its comments (`serverPlan.ts:715-745`, `plannotator-browser.ts:372-376` option `planRevisions`). The event API calls `startPlanReview(ctx, planContent)` with no options (`plannotator-events.ts:449`), so `planRevisions` is off and cpd cannot reach it. cpd instead asks for a new review per revision (docs/commands.md:2599).
- Closing a review from the extension side: `closeKeepingDraft` (`serverPlan.ts:699-714`) is only on the in-process server object; the event API has no request for it. cpd documents this (`pi/lib/plan-review.ts:44`, docs/commands.md §15.5 "Deciding elsewhere" step 4).
- Per-phase config (model, thinking, active tools, instructions) in `plannotator.json`: README "Configuring per-phase behavior".
- Ask AI in the page ("Ask this session" through `sessionBridge`): `serverPlan.ts:131-138`, `:453`; the event API's `plan-review` path passes no `sessionBridge` (`plannotator-browser.ts:386-395`, options default `{}`), so whether Ask AI is wired to the Lead for cpd's reviews is unverified.
- Note export on approve (Obsidian, Bear, Octarine), share links, update advert (`serverPlan.ts:555-585`).

## 3. cpd's wiring

### 3.1 The tool and the flow

`cpd_plan_review` (`pi/lib/review.ts:170-198`):

1. Registered for Plan Leads only: `not_a_lead` error otherwise (`:187`). Parameter: `title`, one string, "at most 72 characters" in its description (`:178-182`). `exposure: "model-only"` (`:184`).
2. Runs `cpd plan review --plan <plan> --title <title>` (`:188`). That command renders the plan and copies it to `run/<plan>/plan-<review>.md`, so a later render cannot change what the User reviews. The copy starts with `# <review title>`, then `Plan <plan id>: <Plan title> · review <review id>`, then the rendered content (docs/commands.md:689).
3. Refused before anything is rendered when a draft Job has no Tier or no Spec (`draft_incomplete`), or no Job is a draft, or the phase is `grounding` (docs/commands.md:682-686).
4. Reads that file and sends `{planContent, planFilePath}` to plannotator, waiting up to 15 s (`review.ts:36-44`; `follower.ts:18`).
5. Writes the session entry `cpd-plan-review` `{plan, review, plannotator_id, status: "pending"}` (`review.ts:46-48`; type `plan-review.ts:4`, `:130-131`), opens the blocking reason `plan-review:<review>` (`review.ts:49`).
6. Returns the `next` text: "The User reviews the plan in plannotator. Stop and wait for the decision message." (`review.ts:195`, docs/commands.md:699).

Latest only: a newer review closes the blocking reasons of this session's older ones (`review.ts:22-24`; docs/commands.md §15.5).

### 3.2 What the rendered Plan contains and its size

`cpd plan render` output format (docs/commands.md:647, code `core/src/plan/review.rs:98`): the title, Context and phase; a table of PRs (key, repo, base, needs) and a Mermaid `graph LR` when there are 2 or more PRs; `## Overlaps`; then one `## PR <key>` section per PR, in needs order, with a Jobs table (key, title, Tier, status, needs, touches), a Mermaid `graph TD` of the Jobs' Dependencies and one `###` subsection per Job with its Spec (`#### Goal`, `#### Context`, `#### Touches`, `#### Acceptance`, `#### Out of scope`). After the plan phase, Locked Jobs appear as context and drafts as "under review". It is GFM only: tables, code, Mermaid, no raw HTML (same line). Nothing outside the Plan's Jobs and PRs (no Requirements, no interview answers) is in it, unless the Lead copied them into a Job's `context` (docs/commands.md:690 and the skill, `pi/skills/cpd-lead/SKILL.md:94`).

Measured with `wc` on files that `cpd plan review` wrote (`plan-<review>.md`):

| File | Bytes (`wc -c`) | Characters (`wc -m`) | Lines | `##` / `###` headings | Mermaid blocks |
|---|---|---|---|---|---|
| `~/.cpd/run/cpd-ci/plan-r1.md` | 5,172 | 5,169 | 81 | 3 / 5 | 1 |
| `~/.cpd/run/bacon/plan-r1.md` | 19,020 | 19,010 | 217 | 3 / 5 | 1 |
| `~/.cpd/run/dev-speed/plan-r1.md` | 28,401 | 28,357 | 355 | 5 / 14 | 5 |
| `~/.cpd/run/mac-pro-debug/plan-r2.md` | 45,737 | 45,669 | 393 | 4 / 14 | 3 |
| `~/.cpd/run/plan-ux/plan-r1.md` (this Plan) | 18,382 | 18,372 | 174 | 3 / 5 | 1 |
| `~/.cpd/run/cross-repo/plan-r5.md` (largest of 14 reviews of that Plan) | 1,001,544 | 1,000,742 | 10,777 | 14 / 99 | 13 |

Where the characters sit: in `bacon/plan-r1.md` the single `## PR bacon` section is 18,675 of 19,020 bytes, so the three Job Specs (about 6 KB each) are almost all of it. In `cross-repo/plan-r5.md` the `## Overlaps` section alone is 499,832 characters (measured with `awk` by section; `## PR cpd` is 264,248, `## PR plumbing` 94,893). I did not read why the Overlaps section is that large; the other 13 reviews of `cross-repo` range from 531,640 to 1,001,544 bytes (`ls -la`).

### 3.3 askguard (PR #174; decisions hs-d468, hs-d472)

Hooked at pi's `tool_call` for `ask_user_question` in one handler with the Manager and Worker guards (`pi/lib/adapter.ts:39-45`); `askVerdict` at `:60-72`; spec docs/commands.md:2863-2885 (§15.13). The result of a refusal is `{block: true, reason}`. Order: G2 first (any Session), then G1 (Leads with a Plan) (`adapter.ts:60-67`).

**G2: a call over 1500 characters** (any role: Lead, Manager, side fork).

- Constant: `LIMIT = 1500` (`pi/lib/askguard.ts:5`), no config key.
- Condition: `textLength(input) > LIMIT` (`askguard.ts:34-37`).
- What is counted (`askguard.ts:17-31`): the string length (JavaScript `.length`, UTF-16 code units) of, for every question, `question` and `header`; for every option, `label`, `description` and `preview`. Not counted: the `multiSelect` flag, the `Type something.` row, anything that is not a string. A part of the wrong shape counts 0 (`:15`, `:20`, `:23`, `:25`, `:27`). A call with no `questions` array counts 0.
- Message, one line (`askguard.ts:12-13`): `cpd: ask_user_question is for a few short choices; put anything larger in plannotator; refused: <n> characters (limit 1500)`.
- Not counted by this limit but allowed by AUQ: up to 4 questions x 4 options x (60-character label + unbounded description + unbounded preview). The 1500-character total is therefore the binding limit in cpd, not the schema's.

**G1: a Lead whose Plan has draft Jobs.**

- Condition: role `lead` and `r.info.plan !== null` (`adapter.ts:63`) and at least one Job of `cpd plan show` has `status === "draft"` (`askguard.ts:39-44`). A Manager, a Lead with no Plan and a side fork of a different role are not subject to G1 (a "side fork" has role `lead` per docs/commands.md:2997; whether `r.info.plan` is set for it is unverified).
- The Plan is read at each call (`host.call(["plan","show","--plan",...])`, `adapter.ts:66`).
- A `jobs` field that is not a list throws `bad_output` (`askguard.ts:42`); a failed read is logged and refuses nothing (`adapter.ts:68-70`).
- Message, one line (`askguard.ts:8-9`): `cpd: a Plan with draft Jobs is presented for review with cpd_plan_review (plannotator), not ask_user_question; refused: <n> draft Jobs`.
- Tests: `pi/test/askguard.test.ts` (G1 count, per-call read, G2 sums; read in this Job only for the constants at its top).

Workers have no `ask_user_question` at all: they are started with `--exclude-tools ask_user_question,cpd_plan_review,...` (docs/contract.md:2062).

### 3.4 How the decision is recorded

- Who runs `plan decide`: the pi adapter, not the Lead. `plan decide` is internal; "the cpd-lead skill forbids the Lead to run it" (docs/commands.md:707).
- Path: on `plannotator:review-result` (or recovery's `completed`), `decide()` writes the feedback to `$CPD_HOME/run/<plan>/feedback-<review>.md` and runs `cpd plan decide <review> --approve|--reject [--feedback-file <f>] --plan <plan>` (`review.ts:99-117`). `review_decided` means another caller decided first; the adapter reads that decision (`:119-129`).
- Then it appends the entry `approved` or `rejected`, closes the blocking reason and sends the Lead a `cpd-plan-decision` follow-up message (`review.ts:58-76`; text `plan-review.ts:47-55`): `Plan review rN was APPROVED.` or `REJECTED.`, then `Feedback from the reviewer:` and the feedback, then `Next: cpd plan lock` (approved) or `Revise the plan (job edit, job add, job cancel), then call cpd_plan_review again.` (rejected). If the Jobs changed since the review, it says so (`:51`).
- A review decided outside plannotator (CLI): the adapter does not run `plan decide` and appends a note that plannotator's page stays open (`review.ts:82-96`; `plan-review.ts:44`).
- A review plannotator no longer knows (`missing`): entry `lost`, Lead told to call the tool again (`review.ts:150-157`; `plan-review.ts:57-58`).

### 3.5 The Inbox plan-review item

- Kind `plan-review`, condition: the Plan's latest review is `pending`; urgency magenta; label `<plan> plan review: <review title>` or `<plan> plan review` with no title (docs/contract.md:450; code `core/src/inbox/rules.rs:208-240`, label `:231-234`).
- Linked to the Plan's Lead Session (`rules.rs:236`); its `url` is `http://<review host>:<port>/` from `run/<plan>/plannotator-port` (docs/contract.md:439; docs/commands.md:2246, :2281). A missing or non-numeric port file gives an item with no link.
- A click on the item opens the Plan's Lead (`session open`); the OSC 8 link is for the terminal's modifier-click (docs/commands.md:2246, :3001). Whether the terminal opens the link through tmux is stated as unverified there.
- The Plan view draws one row ` Plan review r<n> pending 󰏌` above its boxes while the latest review is pending, as a link to the same URL (docs/commands.md:2281).
- It is separate from the `dialog` item, which fires when a Session is `blocked` with any label other than `cpd: plan review` (docs/contract.md:449). An `ask_user_question` dialog is such a blocked state, so it produces a `dialog` item `<who> asks you`, red when the Plan is in `dispatch` or `pr`, magenta in `grounding`, `plan`, `done` or with no Plan (same line). (That a Session is `blocked` while the dialog is open is from the contract line; I did not trace the agent-state code for it.)

### 3.6 Where the skills point the Lead

- cpd-lead Rules: `ask_user_question` only for a few short choices; a Plan, its drafts or a design go to the User only through `cpd_plan_review`; the adapter refuses G1 and G2 (`pi/skills/cpd-lead/SKILL.md:29`). After `cpd_plan_review` or `ask_user_question`, stop; events arrive as new turns (`:39`).
- Clarifying: interview with `ask_user_question` following `references/questions.md`; group independent questions into one round (`SKILL.md:79`). Specifying: confirm assumptions in one short `ask_user_question` (Accept or Change); long ones, or when drafts exist, go through `cpd_plan_review` (`:94`).
- Approving: present drafts only with `cpd_plan_review` (`:156`); in a running Plan, run `cpd plan render`, "show the batch to the User in chat", then call the tool (`:214-215`).
- `references/questions.md`: "At most 4 questions, 2-4 options each. Header at most 16 characters."; "A `preview` goes on single-select options only, and stays to a few lines (the box is capped at 20 rows and does not scroll)"; reading the result via `details.answers`, `user notes:`, `global note:`, and `details.cancelled` (`pi/skills/cpd-lead/references/questions.md:7-8`, `:13-17`). The "capped at 20 rows" there is the side-by-side cap; the content area is 16 rows (section 1.2).
- cpd-manager: same rule, minus the draft guard (`pi/skills/cpd-manager/SKILL.md:32`).

## 4. Side by side

| Capability or limit | ask_user_question | plannotator via cpd | Evidence |
|---|---|---|---|
| Who sees it, where | the User, in the pi editor slot (or an overlay) of the Lead's pane | the User, in a browser tab at `http://<review host>:<port>/` | AUQ `presentation.ts:119-122`; PN `serverPlan.ts:683`; cpd contract.md:439 |
| Blocks the agent turn | yes, `execute` awaits the dialog | no, returns `pending` at once; decision arrives as a follow-up message | AUQ `ask-user-question.ts:439`; PN `plannotator-events.ts:467-473`; cpd `review.ts:177` |
| Timeout | none | none in plannotator; cpd's 15 s applies only to the open request | AUQ (search, 1.5); PN `plannotator-browser.ts:359`; cpd `follower.ts:18` |
| Input unit | 1-4 questions, each 2-4 options | one markdown document (the whole rendered Plan) | AUQ `types.ts:4-6`; cpd `review.ts:36-44` |
| Size limit | cpd: 1500 characters total; AUQ: label 60, header 16, others unbounded | none found; measured Plans 5 KB to 1 MB | cpd `askguard.ts:5`, `:17-31`; AUQ `types.ts:7-8`; PN `helpers.ts:11-14`; section 3.2 |
| Structure of what is shown | question, header chip, labelled options with descriptions, optional preview | headings, tables, Mermaid, task lists, `:::question` blocks | AUQ `types.ts:40-79`; PN `question-block.ts:1-60`; cpd commands.md:647 |
| Preview/long content | markdown preview in a box, 16 content rows side by side or 11 stacked, then cut | the full document, scrolled by the browser | AUQ `markdown-content-cache.ts:8-13`, `preview-block-renderer.ts:40-43`; PN page |
| Multiple choice | `multiSelect: true` (no preview in that mode) | `:::question-multi` blocks in the document (cpd does not emit them) | AUQ `types.ts:72-78`, `preview-pane.ts:151`; PN `question-block.ts`; cpd grep `:::` in `core/src/plan/`, `pi/lib/` found none |
| Free text | `Type something.` row on every question; per-question note (`n`); one global note | inline comments, deletions, global comment, "approve with a note", reply threads, images | AUQ `row-intent.ts:81-89`, `key-router.ts:232-234`, `:342`; PN `plannotator.html:437:161`, `:8821:774` |
| Quick reactions | none | 11 quick labels with agent-facing tips | `plannotator.html:654:1647` |
| Anchors | the question the answer belongs to (`questionIndex`, `question`) | block/location per annotation, inside the feedback text | AUQ `types.ts:107-112`; PN `plannotator.html:437:161` (location format unverified) |
| Result shape | text envelope plus structured `details.answers[]` | `{reviewId, approved, feedback?}` (cpd parser), `feedback` is one text blob | AUQ `response-envelope.ts:19-45`; cpd `plannotator.ts:75-78` |
| Approve / reject | no such concept (cpd's skill says never approve a Plan with it) | `approved: true/false`; cpd runs `plan decide` | cpd `questions.md:11`, `review.ts:99-117` |
| Esc / close | Esc cancels all; model gets `User declined to answer questions` | closing the tab decides nothing; review stays `pending` | AUQ `response-envelope.ts:4`, `key-router.ts:166-280`; PN `plannotator-browser.ts:359-364`; cpd commands.md:2599 |
| Partial answers | allowed on Submit; `unanswered:` names the blanks | a draft of annotations persists in the page; nothing is sent until a decision | AUQ `key-router.ts:218-223`, `response-envelope.ts:40`; PN `serverPlan.ts:273`, `:447` |
| Revisions | a new call asks new questions; no memory | a new review per revision; plan diff and version list when the first heading and date match | cpd commands.md:2599; PN `storage.ts:48-55`, `:228-253`, `serverPlan.ts:190-193` |
| Remote use | RPC hosts get sequential select/input dialogs; previews cut to 600 characters | works over the LAN by host/port (cpd sets remote mode, host and a port per Plan) | AUQ `rpc-fallback.ts:36`, `:88-98`; cpd commands.md:1819, contract.md:2111 |
| Needs interactive UI | yes (`hasUI`); tool is hidden otherwise | yes (`hasUI`); the event answers `unavailable`/`error` otherwise | AUQ `reconcile.ts:25-39`; PN `plannotator-browser.ts:378-380` |
| cpd guard | G2 (all Sessions): refused over 1500 characters; G1 (Leads): refused while the Plan has draft Jobs | `cpd_plan_review` refuses stub drafts, non-Lead callers | cpd `askguard.ts`, `adapter.ts:60-72`; `review.ts:187`; commands.md:682-686 |
| Inbox | a `blocked` Session shows a `dialog` item `<who> asks you` | a `plan-review` item `<plan> plan review: <title>` with a link | cpd contract.md:449-450 |

## 5. What only plannotator can do, and what only ask_user_question can do

### What only plannotator can do (as cpd uses it)

- Show a whole document of any measured size (5 KB to 1 MB) rendered, with tables and Mermaid, and let the User annotate a selection of it: `serverPlan.ts:372-402`; `plannotator.html:437:161`; section 3.2.
- Return anchored feedback: per-annotation location and quoted text (the deletion and comment forms) inside one feedback text: `plannotator.html:437:161`, `:452:8`.
- Show a diff against the previous review of the same plan, and a version list (when the first heading and date match): `serverPlan.ts:190-193`, `:351-371`; `storage.ts:48-55`.
- One-click quick labels with fixed agent-facing tips: `plannotator.html:654:1647`.
- Keep unsent annotations as a draft across a page reload, and survive a pi `/reload`: `serverPlan.ts:273`; `plannotator-events.ts:361-365`.
- Let a review wait without blocking the agent or the User's chat, and be recoverable by `review-status`: `plannotator-events.ts:467-473`, `:413-421`; cpd `review.ts:139-168`.
- Carry `:::question` blocks inside the plan so questions and the plan are one document (cpd does not emit them): `question-block.ts:1-60`.
- Approve with a note, request changes with overall feedback, reply threads and images on annotations: `plannotator.html:8821:774`, `:437:161`.

### What only ask_user_question can do

- Put a choice straight into the Lead's own pane, with the answer back inside the same tool call, so the agent continues in the same turn: `ask-user-question.ts:284`, `:439-451`.
- Offer 2-4 labelled options with a one-line description each and a `(Recommended)` convention, and ask up to 4 questions in one call: `types.ts:4-6`, `ask-user-question.ts:347-364`.
- Return a structured `details.answers[]` (`kind`, `answer`, `selected`, `notes`, `preview`) the model reads directly, plus `unanswered` indexes: `types.ts:107-157`, `response-envelope.ts:19-62`.
- `multiSelect` with an extra free-text answer on the same question: `types.ts:72-78`, `format-answer.ts:20-25`.
- Per-option side-by-side markdown preview (single select): `preview-layout-decider.ts:36-38`, `preview-block-renderer.ts:40-43`.
- Let the User type an answer in place of choosing (every question), and add a note to any answer or one global note: `row-intent.ts:81-89`, `key-router.ts:232-234`, `:342`.
- Run with no browser and no port: inline in the terminal, with an RPC fallback for non-TUI hosts: `presentation.ts:119-122`, `rpc-fallback.ts:88-98`.
- Cancel the whole thing with Esc and have the model told so in a fixed sentence: `response-envelope.ts:4`, `:20-26`.
