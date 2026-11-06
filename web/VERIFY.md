# Manual verification, phases 1 to 4 (pro-d15, pro-d20, Phase 3/4)

The Phase 1 checklist (sections 0 to 4) is kept; the Phase 2 checklist is section 5; the Phase 3/4 checklist (high-fidelity Dango v3 UI) is section 6; the Electron app checklist (Dock, Tray, native shell, live TUI and WebUI switch) is section 7.

Dev bench only: no auth beyond Vite's localhost bind. The UI is the high-fidelity Dango v3 + shadcn/ui design ([`src/ui/DESIGN.md`](./src/ui/DESIGN.md) and its sibling `DESIGN-*.md` files). Contract: [`CONTRACT.md`](./CONTRACT.md). Tick the results table at the end.

## 0. Prerequisites

1. Install at the repo root: `pnpm install --frozen-lockfile`.
2. `cpd` on `PATH` (or `$CPD_BIN`), `$CPD_HOME` set or default `~/.cpd`. Check: `cpd doctor`.
3. A running engine (`cpd watch`, docs/commands.md §12). `cpd watch --check` reads what it needs; `cpd watch --ensure` starts it in `cpd-work:watch` and returns at once. `cpd tui` also builds the cockpit and starts the engine (§13).
4. Sessions to use (list them with `cpd session list`; the `runner` field is `pi` or `durable`):
   - **One pi-runner session**: any Lead or Manager with `runner` `pi` (the default: config keys `lead_runner` / `manager_runner`, `cpd config`). Make a Manager with `cpd context manager --context <c> --runner pi`, or a Lead with `cpd plan new <id> --context <c> --title <t> --runner pi` (§3.10, §4.1).
   - **One durable-runner session**: a Lead or Manager with `runner` `durable`: `cpd context manager --context <c> --runner durable` or `cpd plan new <id> --context <c> --title <t> --runner durable`. The runner is fixed at creation (a different `--runner` on an existing one is refused). A durable session has no pane: the engine's hosts step starts its host at the next tick; the socket is `$CPD_HOME/run/host/<id>.sock`. `cpd session open <id>` resumes a stopped durable session and waits for the socket. `cpd session tail <id>` reads the same transcript read-only in a terminal, to cross-check.
   - Both sessions should have more than 40 transcript items (for Load earlier), at least one `edit` tool call with a diff, one compaction and one cpd fold; if they do not, make the session do the work first (send prompts with `cpd session send <id> "<text>" --unblocks "<reason>"`).
   - For step 6 you need on the durable session an `ask_user_question` and a `cpd_plan_review` pending (ask the agent to call them; a Lead in phase `plan` calls `cpd_plan_review`).
5. Have ready: a small PNG/JPEG, a text file, and a non-image binary file.

## 1. Start and stop

```
pnpm --filter cpd-web-client run dev
```

Vite prints `Local: http://localhost:5173/` (`web/vite.config.ts`: `port: 5173`, `strictPort: true`, so a busy port is an error, not a new port). Open it in a browser. Stop with Ctrl-C in that terminal. Nothing else is started.

## 2. Checklist

### 1. Cockpit frame and panes

| Action | Expected | Look at |
|---|---|---|
| Load the page | 5-zone frame: header on top showing the φ mark (accessible name `C-P-D`); Context tree left; Plan pane above the chat in the centre; Inbox right; status bar at the bottom | whole page |
| Read the Context tree | One row per Context, its Manager chat (id, Manager dot, counters); under it one row per Plan (Lead dot, plan id, counters `planned`, `active`, `failed`, `done`); forks are extra rows; Workers are never rows. A Plan row opens the Plan and its Lead chat. Archived ones are in a separate group. Before data: `Loading contexts…` | left |
| Click a Plan / Lead session | Plan pane shows the Plan title, `Workflow <key>` boxes and Explore jobs; chat attaches (`Select a session` placeholder disappears) | centre |
| Click a Manager (its Context row) | Plan pane shows the Manager overview, not the flowchart: a sticky header with the Context id and its counters, no Flowchart button, one row per active Plan (`[data-manager-plan]`) and a `Done plans (n)` collapsible; the Context's tree collapses while its Manager is selected (pro-d78) | left, centre |
| Click a Plan row in the Manager overview | the Plan and its Lead are selected: the Plan pane shows the Plan once loaded, and the chat attaches | centre, right |
| Read a Workflow with a PR | the `#<n> <state>` link ends with an external-link icon; `Post PR` shows only for a PR not yet posted, not merged and listed as postable in the Inbox (pro-d77) | centre |
| Narrow the tree, the Plan pane or the Inbox | nothing scrolls sideways; a DAG wider than its Workflow card scrolls inside that card (pro-d72) | left, centre, right |
| Read the Inbox | heading `Inbox (<n>)`, one row per item with its actions; `Nothing needs you` when empty; `Engine is not running` appears if the engine is stopped (check with `cpd watch --check`) | right |
| Click an Inbox row's session button | that session is selected | right then centre |
| Select a session, read the status bar | `Runner: pi` or `Runner: durable` matches `cpd session list`; `State: …` matches the tree dot; the version as a bare badge, with no `cpd: ` prefix (docs/ui-preview.md, Production and preview modes, item 6) | bottom |
| Select nothing (reload) | `Runner: none`, `State: none`, plan pane shows the Plan flowchart (`[data-part="flow-canvas"]`, with an `Expand all` button; pro-d68) | bottom, centre |

The Phase 3/4 look of the frame is checked in section 6.

### 2. Chat title and footer

| Action | Expected | Look at |
|---|---|---|
| Select a session with a dynamic title | chat header shows the session key (h1) and the title next to it; the browser tab title is `<key> · <title>`; no title means just the key | chat top, tab |
| Read the `Session footer` bar | `model:`, `tier:`, `thinking:`, `context: <n>%`, `cost: $<x.xxx>`, `branch:`; a value that cannot be known shows `unknown` with the reason as hover text, never a made-up value. Cross-check tier/model/thinking with `cpd session show <id>` | chat bottom |
| Before attach completes | footer shows `unknown` for all six (`not attached`) | footer |

### 3. Live attach, paging, send

Repeat for the **durable** and the **pi** session.

| Action | Expected (durable) | Expected (pi: limits are EXPECTED, A4) |
|---|---|---|
| Select the session | transcript shows the tail window, at most 40 items, newest at the bottom | same, read from the session JSONL (`file` of `cpd session show <id>`) |
| Click `Load earlier` | the previous 40 items are prepended, scroll position stays, button disappears when the start is reached | same |
| Type in `Message`, click `Send` (or Enter) | your message appears, the reply streams live | same, delivered with `cpd session send`; the reply shows up as the JSONL file grows |
| While the agent is busy, type text and press Enter or click `Send` | the message is sent as a follow-up (pro-d57: no Steer, Follow-up or Stop control exists) | `Send` follows the capabilities; no steer or abort control is shown (DESIGN-compose 1.6) |
| Append to the session from outside (`cpd session send <id> "x" --unblocks "verify"`) | the new items appear live without reload | same |
| Reload the page | reattaches to the same session tail | same |

### 4. Image and file import

| Action | Expected | Look at |
|---|---|---|
| Click `Attach files`, pick the PNG | a chip in the `Attachments` list | composer |
| Paste an image (Cmd/Ctrl-V in `Message`) | a chip appears | composer |
| Drag a file over the composer | the form shows the drop state (`data-drop="over"`), drop adds a chip | composer |
| Pick the binary / an oversize file | listed under `Rejected files` with a message; nothing sent | composer |
| Durable: send text + the image, ask "what is in this image?" | the model describes the image: the payload reached the model (the user item also shows the image). Cross-check: `cpd session tail <id>` shows the turn | chat |
| Pi: attach an image and send | EXPECTED: sent. The text reaches the session with a blank line and one `[Attached image: <path>]` line per image; each path is under `<tmpdir>/cpd-webui-attachments/`; the agent's read tool opens it and the agent can describe the image (as on durable) | chat; `cpd session tail <id>` shows the line |
| Text file | its content is sent with the prompt on both runners (the agent can quote it) | chat |

### 5. Tier / model / thinking selector

| Action | Expected | Look at |
|---|---|---|
| Open `Session settings`, change `Tier` | `cpd session show <id>` shows the new tier after the change; the footer `tier:` updates (pi: through `cpd session retier`) | selectors, footer |
| Durable, idle: change `Model` (focus it first: the list loads on focus) then `Thinking level` | footer `model:` / `thinking:` update | selectors |
| Durable, busy | `Model` and `Thinking level` disabled with "A run is pending: the model changes when the session is idle" | selectors |
| Pi session | `Model` and `Thinking level` disabled: "cpd cannot change the model of a pi session; pick a Tier instead" / "…thinking level…"; `Tier` works | selectors |
| No session selected | all three disabled `Not attached` | selectors |

### 6. Interactive dialogs

| Action | Expected | Look at |
|---|---|---|
| Durable: make the agent call `ask_user_question` | a `Questions` card with the options (label and description) and a `Type something.` box | chat |
| Pick an option (an option with a preview shows it), click `Submit` | the agent receives that answer and continues | chat |
| Repeat; type in `Type something.` instead, `Submit` | the agent receives the custom text | chat |
| Repeat; press `Esc` (or click `Cancel`) | the dialog cancels, the agent is told it was cancelled | chat |
| Durable: a `cpd_plan_review` appears (`Plan review` card with the summary in Markdown) | click `Approve` → approved; second review: `Request changes`, type in `Feedback`, submit → feedback delivered; third: `Reject` | chat; `cpd plan show <id>` |
| Pi: a pending ask in the transcript | rendered READ-ONLY (no answering), with the reason "a TUI dialog of a pi session cannot be answered from outside…"; answer it in the TUI | chat |
| Pi: a pending `cpd_plan_review` | answerable: `Approve` runs `cpd plan decide <review> --approve --plan <plan>`; `Reject` runs `… --reject --plan <plan>`; `Change` with feedback writes `<CPD_HOME>/run/<plan>/feedback-<review>.md` and runs `… --reject --feedback-file <that path> --plan <plan>`; the card closes when pi records the review's new status | chat; `cpd plan show <id>` |

### 7. Streaming, folds, tools

| Action | Expected | Look at |
|---|---|---|
| Send a prompt that needs a long answer | text streams in as Markdown (lists, code, tables), no flicker, scroll follows | chat |
| A turn with thinking | a `Thinking…` fold; click to open the reasoning | chat |
| A turn with tool calls | compact tool rows (`<n> tool calls` group, one row per call with status Running → Done or `Failed: <reason>`) | chat |
| An `edit` tool call | row open by default with a diff: added lines marked add, removed lines remove | chat |
| A cpd fold (a cpd-owned message, e.g. the first message) | a collapsed fold with its title; click to open | chat |
| A compacted session | a `Compaction (<n> tokens)` fold with the summary in Markdown | chat |

## 3. Known limitations (Phase 1, amended for Phase 2)

- **Durable v1 and v2 (superseded by CONTRACT 10.8):** pi-durable 1.3.0 (protocol 1, `snapshot`/`events`) and 1.5.0 (protocol 2, `view`/`ops`) are both understood; v2 is tested only against mocked frames until `cpd:pi-1-5-upgrade` has merged. An unknown host version or frame shape fails loud with `unsupported_host_version` / `protocol_error`, never a silent fallback.
- **pi sessions (A4):** read from the session JSONL and written through `cpd session send` / `cpd session retier`. No abort, steer, model or thinking change, or compaction (cpd has no command for them); a pending `ask_user_question` is answerable from the WebUI (pro-d60; the answer goes to the pi extension, CONTRACT 11.4); plan reviews are answerable; images are accepted and sent as paths in the text (see the README section "Server behaviour and pi sessions", CONTRACT 11.3). The UI disables each control it cannot drive and shows the reason.
- **Dev bench:** runs only under `vite serve`; transport is Vite `server.ws` custom events (`cpd:c2s`, `cpd:s2c`); no auth beyond Vite's localhost bind; `vite build` output is not runnable.
- **Phase 2 (CONTRACT 10.9):** no keep-mounted rows for selection, no in-app search, fold open state is not persisted across unmount, no v2 `history` paging beyond the host's newest-50 window, reference-style Markdown link definitions across segments do not resolve, from-null `cpd` nudges show as User messages, durable `cpd-event`/`cpd-first-message`/`cpd-error`/`cpd-plan-review` entries stay dropped.
- **Unverified without a running v2 host:** tool-call streaming ops, multi-round tool runs, window overflow ops, history paging, `host_state.name` as title.
- **Not covered by Phase 2 (docs/research/webui-chat-base.md):** C8 instant session switch, C9 highlighting off the hot path, C10 virtualisation escape hatches (search, keep-mounted, fold state across unmount), C11 heavy rows bounded, C12 hooks exposed by the base.
- **Phase 3/4 (high-fidelity UI):** dark theme only (no light theme); fonts are CSS stacks, none is bundled, so a machine without IBM Plex Sans, EB Garamond or Maple Mono falls back to the next family of the stack; no responsive layout below 980px and no Sheet drawers; panel sizes are not persisted; no command palette (⌘K), no Reset layout, Legend, Logs or Tiers dialogs, no Workers meter, host CPU/RAM/disk or incident pill (their data is not in the protocol); Snooze and Dismiss in the inbox are disabled hints (no backend action); session cost `$` and per-plan cost are not shown; the Context tree is one row per Context (its Manager chat), one row per Plan (its Lead chat), forks as extra rows, and never Workers; every element whose data the protocol does not carry is listed under "Not wired" in the DESIGN files and is absent from the UI, never faked.

## 4. Results (Phase 1)

| # | Area | Durable | Pi | Notes |
|---|---|---|---|---|
| 1 | Cockpit frame and panes | [ ] | [ ] | |
| 2 | Chat title and footer | [ ] | [ ] | |
| 3 | Live attach, Load earlier, send (follow-up only, pro-d57) | [ ] | [ ] (limits as expected) | |
| 4 | Image and file import | [ ] | [ ] (images sent as paths, as expected) | |
| 5 | Tier / model / thinking | [ ] | [ ] (model, thinking disabled as expected) | |
| 6 | Interactive dialogs | [ ] | [ ] (ask and plan review answerable, §9 steps 23 and 24) | |
| 7 | Streaming, Thinking…, tools, diffs, cpd and compaction folds | [ ] | [ ] | |

## 5. Phase 2 checklist (pro-d20)

Prerequisite additions: a pi-runner session; a durable v1 session (pi-durable 1.3.0 host); for step 8 a durable v2 session (pi-durable 1.5.0 host); the largest local pi session file, `<your largest ~/.pi/agent/sessions/*.jsonl>` (about 25 MB on the User's machine; find it with `ls -S ~/.pi/agent/sessions/*.jsonl | head -1`).

### 5.1 Steps

| # | Steps | Expected |
|---|---|---|
| 1 | pi-runner round trip. (a) Select the pi session, type a message in the chat, press Enter. (b) From another session run `cpd session send <pi-id> "ping" --unblocks "verify"` (or have another session message this one). | (a) Within a second your text appears as a User message in the transcript, exactly once: the pending row (`sending…`, then sent) disappears when the User message arrives, and no duplicate remains after 30 s. (b) The message from the other session shows as a collapsed fold whose summary reads `Message from <key>` (fold kind `cpd`); click opens it. A message the web chat sent (`unblocks` `User message from the web chat`) is a user bubble even when its sender is a session id. |
| 2 | Sticky auto-scroll. Attach a session with a long transcript; send a message; let the reply stream. | On attach the view is at the bottom (`data-at-bottom="true"`); after send it scrolls to your message; during streaming the newest text stays in view without jitter. |
| 3 | Scroll-up opt-out. Scroll up one screen while a reply streams; then click `Jump to bottom`; scroll up again and scroll back to the bottom by hand. | Scrolling up stops following and the view does not move as the last row grows; a `Jump to bottom` button appears (and `data-at-bottom="false"`); clicking it scrolls to the end, hides the button and follows again; returning within about 48 px of the bottom re-pins. |
| 4 | Load earlier. Scroll to the top of a session with more than 40 items, read a message in the middle of the screen, click `Load earlier`. | The previous page is prepended and the message you were reading stays at the same screen position (moves at most 1 px); the button disappears at the start of the transcript. |
| 5 | Panes. Wheel-scroll over the Context tree, the Plan pane, the chat and the Inbox in turn, with enough content to overflow each. | Each pane scrolls on its own; the page itself never scrolls (no outer scrollbar); header and status bar stay fixed. |
| 6 | Large transcript. Start the dev server, select a pi session whose file is `<your largest ~/.pi/agent/sessions/*.jsonl>` (25 MB). In DevTools Elements count `[data-row-key]` nodes; while a Lead streams, type in the composer. | The last messages paint on first load without freezing the tab; the row count stays <= 60 (DOM rows mounted, CONTRACT 10.4) and does not grow while you scroll; typing stays responsive (no visible lag) while a Lead streams. |
| 7 | Durable v1 session. Repeat steps 1 (without 1b), 2, 3, 4 and Phase 1 section 3 on a durable session whose host is pi-durable 1.3.0. | Everything works as before; a message typed in the web chat appears once as a User message (entry kind `cpd-chat`). |
| 8 | **Run only after `cpd:pi-1-5-upgrade` has merged.** Durable v2 session (pi-durable 1.5.0 host): same checks as step 7. | Same results as step 7. Not verifiable before the merge (unverified: tool-call streaming ops, multi-round tool runs, window overflow ops, history paging, `host_state.name` as title); record what differs in Notes. Paging beyond the host's newest-50 window is a known limitation. |

### 5.2 Measurements (docs/research/webui-chat-base.md lines 138-149)

Covered by Phase 2: C1 to C7. Measure in Chrome DevTools Performance on the largest session; leave Result empty until measured.

| ID | Check (verbatim from r7-chat-perf §8) | Result |
|---|---|---|
| C1 | **Cold open of a 25 MB, 5,000+ message session**: first paint of the last messages ≤ 500 ms, main thread never blocked > 100 ms (Performance trace long-task count = 0 > 100 ms). Fail if it parses/renders all messages before first paint. | |
| C2 | **Windowed DOM**: with 5,000 messages loaded, live message DOM subtrees ≤ ~100 (window + overscan), and DOM node count does not grow with total N (count at N=500 vs N=5,000 within 10%). | |
| C3 | **Per-token cost independent of N**: stream 200 tok/s into the last message at N=500 and N=5,000; per-frame scripting time p95 ≤ 8 ms and within 20% between the two N. Fail if cost grows with N (the cpd TUI failure mode: 0.097 ms/component). | |
| C4 | **Streaming markdown is incremental**: DevTools/Profiler shows only the last open block re-renders per token (earlier blocks of the same message, and all other messages, do not re-render). Fail if the whole message re-parses each token. | |
| C5 | **Bottom-anchoring**: pinned at the end while streaming (no visible jitter, drift ≤ 1 px/s); scrolling up by one screen stops following and does not move even as the last row grows; returning within threshold re-pins. | |
| C6 | **Prepend stability**: load older page while reading mid-history; the visible message moves ≤ 1 px; stable message ids (not indexes) as keys. | |
| C7 | **Typing responsiveness during streaming**: input keydown→paint p95 ≤ 50 ms while streaming at 200 tok/s with N=5,000 (cpd baseline to beat: TUI 54 ms median on 24 MB). | |

### 5.3 Phase 2 results

| # | Area | Durable v1 | Durable v2 | Pi | Notes |
|---|---|---|---|---|---|
| 1 | pi round trip, cpd fold | n/a | n/a | [ ] | |
| 2 | Sticky auto-scroll | [ ] | [ ] | [ ] | |
| 3 | Scroll-up opt-out, Jump to bottom | [ ] | [ ] | [ ] | |
| 4 | Load earlier keeps position | [ ] | [ ] | [ ] | |
| 5 | Panes scroll independently | [ ] | [ ] | [ ] | |
| 6 | Large transcript | n/a | n/a | [ ] | |
| 7 | Durable v1 | [ ] | n/a | n/a | |
| 8 | Durable v2 (after merge) | n/a | [ ] | n/a | |

## 6. Phase 3/4 checklist (high-fidelity Dango v3)

Prerequisites: sections 0 and 1 above; a browser window at least 1280 px wide and 800 px high; DevTools open for the steps that name it. Have a Plan in phase `dispatch` with at least two Workflows (one with a failed Job, one with a queued Job), a Lead session on the durable runner, an inbox with items of several urgencies, and (steps 19 and 20) the ability to make a durable agent call `ask_user_question` and `cpd_plan_review`. Specification: [`src/ui/DESIGN.md`](./src/ui/DESIGN.md) (foundation and shell), `src/ui/DESIGN-panes.md`, `src/ui/DESIGN-chat.md`, `src/ui/DESIGN-compose.md`. Measure sizes with DevTools (Elements, computed style). Record results in 6.2.

### 6.1 Steps

| # | Steps | Expected |
|---|---|---|
| **Foundation** | | |
| 1 | Load the page. In DevTools, select `<body>`, read computed `background-color`, `color`, `font-size`, `font-family`. Select any element with class `text-status-blocked` or `glyph-blocked` and read its colour. | Body background `rgb(28, 26, 32)` (`#1c1a20`), text `rgb(216, 210, 204)` (`#d8d2cc`), 13px, `IBM Plex Sans` first in the stack; no white flash, no light theme. The blocked glyph is `#e17f6e`. (Proves Tailwind, tokens and `dango-status.ts` classes are generated.) |
| 2 | In DevTools run `getComputedStyle(document.documentElement).getPropertyValue("--ring")` and the same for `--sakura-bright`, `--line`, `--surface-sunken`, `--card`, `--surface-raised`, `--surface-pressed`, `--border`, `--input`, `--edge`. | Values resolve to `#a1c6ef` (ring), `#ecaec7` (sakura-bright), `#5a504a` (line), `#19171c`, `#211f26`, `#26232c`, `#2b2731`, `#2e2a33`, `#35313b`, `#3a3640` (or the equal `oklch(...)` values the browser prints). |
| 3 | Click into the `Message` box and look at the text cursor; select some chat text with the mouse. | The caret is pink (`#ecaec7`); the selection background is `#5a504a`, selected text `#f3ead9`. |
| 4 | Tab through the page. | Every focused control shows a 2px `#a1c6ef` outline with 1px offset; focus is never invisible. |
| 5 | Find each glyph in the UI (tree dots, job nodes, inbox rows). Compare with the legend of shapes: triangle (failed), down arrow (landing), diamond (waiting on you), hollow diamond (draft), spinning arc (running), square (review), solid dot (done), neutral solid dot (unread), dashed ring (queued), ring (idle). In the browser's rendering settings enable "reduce motion" and reload. | Each status is a distinct shape as well as a colour (no colour-only status). With reduced motion the running arc no longer spins. |
| **Cockpit shell** | | |
| 6 | Measure the header, the status bar, the Context tree column and the Inbox column. | Header 44px high with a 1px bottom border `#2e2a33`; status bar 30px high, background `#19171c`, 1px top border; tree column 272px wide with a right border; inbox column 340px wide with a left border. The page has no outer scrollbar. |
| 7 | Read the header. | Left: a φ mark in a serif face (28px, `#f3ead9`); then, when a Plan is shown, the breadcrumb (DESIGN-panes 4.1; superseded by §9 step 12); right: `engine` with a glyph and `running` (green dot) when `cpd watch --check` says the engine runs. Stop the engine: the glyph becomes a triangle and the text `not running`. No search box is shown (command palette not wired). |
| 8 | Read the status bar. | Right side: `Runner: pi`/`durable`/`none`, `State: <state>` with a glyph, the version as a bare badge with no `cpd: ` prefix (docs/ui-preview.md item 6), all matching Phase 1 step 1. No Legend, Logs, Tiers, Workers, CPU/RAM or incident element appears. |
| 9 | Read the Context tree: header row `CONTEXTS`. One row per Context, its Manager chat (id, Manager dot, counters). Under it one row per Plan (Lead dot, plan id, counters `planned`, `active`, `failed`, `done`). Forks are extra rows under their Context. Hover a row. | Each Plan row shows its Lead's dot and the four counters; each fork row shows its own dot. No Worker appears as a row. Archived items sit under `Archived (<n>)`. |
| 10 | Click a Plan row, then a fork row. | The Plan row gets a blue tint (`rgba(122,162,206,.16)`) and brighter text; the Plan pane and its Lead chat open as in Phase 1 step 1. The fork row opens its chat. |
| 11 | Plan/chat split: drag the thin bar between the Plan pane and the chat (`Resize plan view and chat`) up and down. Focus it with Tab and press ArrowUp / ArrowDown. | The plan gets taller or shorter, never below 20% or above 80% of the column; each pane scrolls on its own; with the keyboard the bar moves (step size is the library's). Double-click on the bar restores the default 40/60 split. |
| 12 | Click `Collapse plan view` (top right of the Plan pane), then `Expand plan view`, then `Zoom plan view`, then `Zoom plan view` again. | Collapse: the plan shrinks to its 44px title row, the button label becomes `Expand plan view`, `Zoom plan view` is disabled. Expand restores the previous size. Zoom: the chat shrinks to nothing and the plan fills the column (`Zoom plan view` has `aria-pressed="true"`, `Collapse plan view` is disabled); the second click restores the previous split. Tab never lands inside a hidden pane. |
| **Plan view** (DESIGN-panes.md) | | |
| 13 | Select a Plan in phase `dispatch`. | Title row 44px with the Plan key, the phase stepper `grounding — planning — dispatch — deploy — done` with `dispatch` highlighted teal (`aria-current="step"`), then the explore strip with job chips, then one card per Workflow (34px header: name, `repo → base`, phase badge, PR note) with a left-to-right graph of Job nodes (148 x 46 px, glyph, key, tier, status text). |
| 14 | Compare each Job node with its status: running (teal border, spinning arc), merged (green dot), queued (dashed border, dashed ring), checking (amber, down arrow), failed (red border, triangle, `failed` text). Click the `Post PR` action of a Workflow if one is offered. | Colour, border style and glyph agree with the status; an unknown status string shows a neutral ring with the raw text; the PR action still sends `pr.post` as in Phase 1/2. `Retry` is disabled or absent unless a backend action exists. |
| **Inbox** (DESIGN-panes.md) | | |
| 15 | Read the Inbox: header `INBOX`, the open count, six tier counts; rows grouped under `blocking`, `delivery`, `review`, `decide`, `in flight`, `unread` (only groups that have rows). Move the keyboard focus onto a row (J / K or Tab). | Each row has the tier's glyph, subject (monospace), age, sub-line; the group label has the tier's colour. The focused row is tinted blue and shows `Open ⏎`, `Snooze S`, `Dismiss E`; Snooze and Dismiss are disabled (no backend action) unless the row carries that action; existing row actions (Claim, Approve, `Snooze 15m`) still work as in Phase 2 and `Open` selects the row's session. Empty inbox: `Nothing needs you`; stopped engine: `Engine is not running`. |
| **Chat** (DESIGN-chat.md) | | |
| 16 | Select a session. Read the chat header, then scroll the transcript. | Title 15px semibold, the key in `#a1c6ef` monospace, `Lead · durable`-style meta, the session's own state. User messages are right-aligned bubbles (`#26232c`, up to 78% wide) with a `you · HH:MM` meta line; assistant messages are cards (`#211f26`, 10px radius) with Markdown (inline code teal monospace 12px, lists indented 20px); `cpd` and compaction items are one-line φ markers. Phase 2 behaviour holds: scroll position, `Jump to bottom`, `Load earlier`, row count <= 60 (Phase 2 step 6). |
| **Tool rows** (DESIGN-chat.md) | | |
| 17 | Find a turn with tool calls and one `edit`. Expand and collapse the group and a thinking fold. | The group has a 2px left rail, a summary line (`read <target> + N more steps`), a strip of one 3x10px tick per call (green, red for a failed call), `done in <s> · <n> failed`; each row is 22px: status glyph, verb (`read`, `bash`, `edit`), target, duration; a failed `bash` shows its output block with `… N more lines`; the `edit` row shows `+<added> −<removed>` and a diff with red removed and green added lines on dark backgrounds. `Thinking` is a collapsed pill that expands to the reasoning. |
| **Composer and footer** (DESIGN-compose.md) | | |
| 18 | Read the composer and footer; type text; press Enter; while the agent runs, press Enter again. | Superseded by §9 steps 14 to 16 (pro-d54, pro-d57): the composer has a header, a textarea and a footer with the attach button, the Tier, Model and Effort menus and `Send`; there is no Steer, Follow-up or Stop; the footer is only the `N% context` card. Earlier text, kept for history: Composer: 10px-radius card with a `#a1c6ef` border, the tier/model/thinking selector pill (`S2 claude-… high`); unknown values read `unknown` (Phase 1 step 2), never invented. Every Phase 1 and 2 behaviour (send, steer, abort, attach, drop, paste, rejections) still works; disabled controls keep their reason. |
| **Ask card** (DESIGN-compose.md) | | |
| 19 | Durable: make the agent call `ask_user_question` with 2 or 3 questions, one multi-select, one option with a preview. Pick answers with the mouse, then with the keyboard (1 to 4, arrows, Tab), and Submit; repeat with `Type something.` and with Esc. | A pink-bordered card, tabs per question with answered/unanswered marks, options with numbered badges and the `recommended` badge where given, the preview pane beside the options when two-column, `Submit N/M answers ⏎` as the one filled button; the answers reach the agent exactly as in Phase 1 step 6; Esc or `Cancel` cancels. A pi session shows the read-only card with its reason. |
| **Plan review card** (DESIGN-compose.md) | | |
| 20 | Durable: a Lead in phase `plan` calls `cpd_plan_review`. Read the card; use `Approve`, then on a second review `Request changes` with feedback, then on a third `Reject`. | A card headed `Plan review · <plan>` with the summary rendered as Markdown; a decision bar with a `Feedback` textarea, `Reject` (red text), `Request changes`, and `Approve` as the one filled button; each outcome reaches the agent as in Phase 1 step 6 and `cpd plan show <id>` agrees. A pi session's review is answerable from the card (section 2, step 6). |

### 6.2 Phase 3/4 results

| # | Area | Durable | Pi | Notes |
|---|---|---|---|---|
| 1 to 5 | Foundation: tokens, fonts, cursor and selection, focus ring, glyphs | [ ] | [ ] | |
| 6 to 12 | Cockpit shell: header, status bar, Context tree, split, collapse, zoom | [ ] | [ ] | |
| 13, 14 | Plan view | [ ] | [ ] | |
| 15 | Inbox | [ ] | [ ] | |
| 16 | Chat surface | [ ] | [ ] | |
| 17 | Tool rows and thinking | [ ] | [ ] | |
| 18 | Composer and footer | [ ] | [ ] | |
| 19 | Ask card | [ ] | [ ] (Questionnaire, §9 step 23) | |
| 20 | Plan review card | [ ] | [ ] (answerable as expected) | |

## 7. Electron app checklist (pro-d21, pro-d23, pro-d24)

Specification: [`ELECTRON.md`](./ELECTRON.md). The Dock and Tray visuals are checked by hand here and never in tests. Steps 7.2 to 7.5 are macOS only; Linux and Windows have no Dock and no Tray.

Prerequisites: `pnpm install --frozen-lockfile` at the repo root; `cpd` on `PATH`; a running engine (`cpd watch --check`); Maple Mono installed (ELECTRON.md, Run). For the attention steps, the Inbox must hold at least one `red` row (blocking) and one `magenta` row (decision): a failing Job and a pending question or plan review will do. Start the app with `pnpm --filter cpd-web-client run electron`, and quit it with Cmd-Q or the Tray's `Quit cpd` item.

### 7.1 Steps

| # | Steps | Expected |
|---|---|---|
| **Dock icon (dev)** | | |
| 1 | Start the app. Look at the Dock while the app runs. | The Dock shows the cpd icon, the Shiro phi on Kuronuri `#1c1a20`, not the generic Electron icon. |
| **Tray and attention image** | | |
| 2 | With no attention item, look at the menu bar. Switch the system appearance between light and dark. | A φ template icon in the menu bar that follows the appearance. Hovering it shows the tooltip `cpd`. |
| 3 | Make a `red` row appear in the Inbox (the window may be open or closed). | The Tray icon changes to the attention variant. Resolve or clear the rows: the Tray returns to the normal icon. |
| **Dock badge** | | |
| 4 | With the counts from the Prerequisites, read the Dock badge. | The badge shows the blocking count plus the decision count. The Inbox header tier counts agree: the red (blocking) and magenta (decide) counts add up to the badge. |
| 5 | Clear every red and magenta row. | The badge disappears. |
| **Bounce** | | |
| 6 | Unfocus the window (focus another app). Make a new `red` row appear. | The Dock icon bounces once. |
| 7 | Unfocus the window. Make a new `magenta` row appear, with no new `red` row. | The badge goes up. The Dock does not bounce. |
| 8 | Focus the window. Make a new `red` row appear. | The badge goes up. The Dock does not bounce. |
| 9 | Quit the app while a `red` row exists. Relaunch it with the window unfocused. | No bounce at startup: the first count is the baseline. A further new `red` row, while unfocused, bounces the Dock. |
| **Tray items** | | |
| 10 | Click the Tray icon. Read the menu. | Items in this order: `Open cockpit`, `Switch to TUI`, `Switch to WebUI`, a separator, `Quit cpd`. |
| 11 | In TUI mode, choose `Open cockpit`. | The window shows and takes focus, and the app switches to WebUI (View > WebUI is checked). |
| 12 | Choose `Switch to TUI`, then `Switch to WebUI`. | Each choice matches View > TUI and View > WebUI, and the view changes at once. |
| 13 | Choose `Quit cpd`. Then run `pgrep -fl 'cpd tui'`. | The app quits and its Tray icon disappears. `pgrep` prints nothing, so no `cpd tui` process remains. |
| **Live TUI and WebUI switch** | | |
| 14 | Start in TUI with a `cpd tui` cockpit running. Run `pgrep -fl 'cpd tui'` and note the process id. Choose View > WebUI, then View > TUI. Run `pgrep -fl 'cpd tui'` again. | The terminal hides and the WebUI shows, then the terminal returns with the cursor focused. The process id is the same before and after, so the TUI session was not restarted. |
| 15 | In WebUI, select a session, scroll its transcript, and type a draft in `Message` without sending. Switch to TUI, then back to WebUI. | The WebUI still shows the same session, the scroll position and the draft. Nothing reloads. |
| 16 | In TUI, press CmdOrCtrl+Shift+M. Then switch to WebUI with View > WebUI and press CmdOrCtrl+Shift+M again. | In TUI the accelerator does nothing, so the keys reach cpd. In WebUI it switches to TUI. |
| 17 | Switch to WebUI, wait while cpd prints output in the cockpit, then switch to TUI. | The terminal shows the current cockpit, not a stale frame, and is focused. |
| **WebUI in the window** | | |
| 18 | In WebUI, click a link to a PR or a deploy run. | The link opens in the system browser. The app opens no window for it. |
| 19 | Open the macOS app menu, then About cpd. | The panel shows `cpd`, the app version and `Continuous Parallel Delivery`. The app stays dark when the system appearance is light. |
| **Packaging (macOS, once per release)** | | |
| 20 | Run `scripts/prepare-macos-assets.sh`, then `pnpm --filter cpd-web-client run dist:mac`. | A DMG and a zip appear under `web/dist/`. |
| 21 | Run `find web/dist -name Info.plist -path '*cpd.app*'`, then `/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' <that path>`. | The bundle identifier is `com.cpd.desktop`. |
| 22 | In the packaged app's `Contents/Resources/`, list `icon.png`, `icon.icns` (or `Assets.car`) and `tray/`. | `tray/trayTemplate.png` and `tray/trayAttentionTemplate.png` are present. `icon.png` is present too, so the packaged Dock icon can load. If `icon.png` is missing, this check fails: record it in Notes. |

| **Terminal fixes (pro-d25)** | | |
| 23 | In TUI, hover over a tmux pane border, then click-drag it. | Hover does not drag the border. Click-drag still resizes the pane. |
| 24 | Drag the window edge to resize, then release. | The TUI layout grows to the final window size. |
| 25 | Move the pointer over the terminal. | The pointer is an arrow, not a text cursor. |
| 26 | Look at the tmux status line and Powerline caps. | Powerline caps and status icons render. |

### 7.2 Results

| # | Area | Result | Notes |
|---|---|---|---|
| 1 | Dock icon (dev) | [ ] | |
| 2, 3 | Tray and attention image | [ ] | |
| 4, 5 | Dock badge | [ ] | |
| 6 to 9 | Bounce | [ ] | |
| 10 to 13 | Tray items and quit | [ ] | |
| 14 to 17 | Live TUI and WebUI switch | [ ] | |
| 18, 19 | WebUI in the window: links, About, dark theme | [ ] | |
| 20 to 22 | Packaging | [ ] | |
| 23 to 26 | Terminal fixes: hover, resize, pointer, Nerd Fonts | [ ] | |

## 8. Auto-update and CLI bootstrap

Specification: [`ELECTRON.md`](./ELECTRON.md), section "Auto-update and CLI bootstrap". Run these on the app launched from the cpd checkout (macOS), with the checkout on branch `main`, clean, and `origin` reachable. `cpd` is on `PATH` and an engine is running. Steps (a), (b) and (d) need a merged PR: merge only the change the step names, into `main`.

### 8.1 Steps

1. **(a) Renderer reload on merge.** In WebUI (View > WebUI), select a plan and a session, and note the `cpd tui` process id (`pgrep -fl 'cpd tui'`). Focus another app. Merge a PR that touches `web/src/ui/`. Wait about 1 minute.
   Expected: the renderer reloads only after the window is unfocused: the WebUI shows the same plan and session again, restored from `sessionStorage` key `cpd.selection`. The TUI terminal is repainted with the current cockpit, and `pgrep -fl 'cpd tui'` shows the same process id (still attached). No window takes focus during the reload. Unverified: the build time is not measured by the contract, so "about 1 minute" is the expectation, not a measured result.
2. **(a) No reload while focused.** Focus the cockpit window and leave it active. Merge another PR that touches `web/src/ui/`. Wait 2 minutes with the window focused and the keyboard and pointer in use.
   Expected: no reload while the window stays focused and the system is active. The reload happens after you unfocus the window, or after 300 s with no system input.
3. **(a) Same-size resize (unverified).** Right after step 1, check that the TUI is painted. Drag the window edge by one column and back.
   Expected: the TUI repaints at each size. Unverified: whether a same-size resize repaints tmux. The app works around it with a one-column nudge (`resize(cols - 1)`, then `resize(cols)` after 50 ms). If the terminal stays blank after step 1, record it in the results.
4. **(b) main.ts change shows the restart item.** Merge a PR that changes `web/src/electron/main.ts` and nothing under `web/src/ui/`. Wait for the build.
   Expected: the native menu shows `Restart to Apply Update`, enabled. The renderer does not reload: the WebUI keeps its selection and draft, and the `cpd tui` process id is unchanged. Then click `Restart to Apply Update`.
   Expected after the click: the app applies the update. The click is the only way a changed `main.js` or `preload.cjs` is loaded.
5. **(c) CLI bootstrap on a missing command.** Quit the app. Run `mv ~/.local/bin/cpd ~/.local/bin/cpd.aside`, then relaunch the app.
   Expected: the window shows the overlay titled `Installing the cpd command tool`, with the detail `Runs once in the background; the cockpit starts when it finishes.`. When it finishes, the overlay goes and the cockpit starts. `ls -l ~/.local/bin/cpd` shows the file. Then run `rm ~/.local/bin/cpd.aside`.
   If the overlay shows `cpd command tool install failed` instead, the log is `~/.cpd/run/cli-bootstrap.log`. Record it in the results.
6. **(d) CLI install waits for a running Attempt.** Start a Job so that an Attempt is running: `cpd attempt list --active` lists it. Note the checkout's HEAD (`git rev-parse HEAD`). Merge a PR into `main`.
   Expected: the updater log has a line saying an Attempt is running and that the CLI install waits. The exact wording is not fixed by the contract (unverified). `cat <checkout>/target/.cli-installed-sha` still shows the old sha while the Attempt runs.
7. **(d) CLI install after the Attempt ends.** Wait for the Attempt to finish. Wait up to about 1 minute.
   Expected: the install runs. `cat <checkout>/target/.cli-installed-sha` prints the HEAD from step 6, matching `git rev-parse HEAD`.
8. **(e) Dirty checkout is kept (A7).** In the checkout, run `touch web/verify-dirty.txt`, an untracked file that `origin/main` does not touch. Relaunch the app (the launch catch-up check runs).
   Expected: the updater log has no `dirty` reason, since that skip no longer exists. The file `web/verify-dirty.txt` is still there after the sync. Only a dirty path that `origin/main` supersedes is replaced by the upstream version. Then run `rm web/verify-dirty.txt`.

### 8.2 Results

| # | Area | Result | Notes |
|---|---|---|---|
| 1 | (a) Renderer reload on merge | [ ] | |
| 2 | (a) No reload while focused | [ ] | |
| 3 | (a) Same-size resize repaint (unverified) | [ ] | |
| 4 | (b) main.ts change shows the restart item | [ ] | |
| 5 | (c) CLI bootstrap overlay | [ ] | |
| 6 | (d) CLI install waits for a running Attempt | [ ] | |
| 7 | (d) CLI install after the Attempt ends | [ ] | |
| 8 | (e) Dirty checkout is kept | [ ] | |

## 9. Checklist pro-d48..pro-d61 (approved preview, consolidated to main)

Run on the cockpit at a window of at least 1280 px wide unless a step says otherwise. Each step names its contract. Expected results are what the approved preview shows; a difference is a defect unless the contract section says otherwise.

### 9.1 Steps

1. **Fonts (pro-d48, DESIGN.md 2b).** Open DevTools, select the `h2` `Inbox`. Computed `font-family` starts with `Roboto`; select a row's age: `Roboto Mono`. Expected: both load (no fallback `sans-serif` in the first position).
2. **Tier filter (pro-d49, DESIGN-panes 2.4).** In the Inbox header, expected seven items in order `all`, `blocker`, `delivery`, `decision`, `support`, `notice`, `unread`, each `label count`, none disabled, right-aligned. Click `blocker`: only blocker rows show, and `blocker` stays on. Click `delivery`: blocker and delivery show (additive). Click `all`: every row shows. Select all six tiers: the filter resets to `all`.
3. **Isolation (pro-d49).** With `all` selected, click `notice`: only notice rows show. Click `notice` again: back to `all`.
4. **Gap and collapse (pro-d50, DESIGN-panes 2.4).** Expected: at least 32 px between `Inbox` and the filters. Narrow the Inbox column (drag the `split-right` handle) until the filters fit no longer: they become one `Filter inbox by tier` button. Open it: a vertical list of the same seven items. Expected: the button reads `all` or `N tiers` with the count.
5. **Column defaults and double-click (pro-d50, DESIGN.md 3c).** Expected at start: tree 20%, centre 50%, Inbox 30%. Drag the handles away, then double-click `split-left`: the tree returns to 20%. Drag, then double-click `split-right`: the Inbox returns to 30%.
6. **Reset layout (DESIGN.md 3g).** After step 5's drags, click `Reset layout`. Expected: tree 20%, centre 50%, Inbox 30%, Plan 40%, chat 60%.
7. **Row layout (pro-d51, pro-d52, DESIGN-panes 2.5).** Expected: each row is an outlined item with three columns. The icon is at the top left and the age at the bottom left, the age on the last text line's baseline. The external-link icon (linked kinds) or the chevron is at the top right, the primary button at the bottom right (`Merge`, `Post`, `RePost`, `Approve`, `Claim`, `Start`, `Curate`, or `Dismiss`).
8. **Links (pro-d53, DESIGN-panes 2.6).** Click the external-link icon of a `pr-ready` row: the PR opens in a new tab, and the row's Plan and Session do not change. Click the row body of a `pr-ready` row: its Plan and Session are selected, and the chat shows that Session.
9. **Engine start (pro-d53).** With the engine stopped, the `engine-down` row shows `Start`. Click the row: the engine starts. Expected: the row's `Start` shows the spinner, then the row disappears once the engine reports ok.
10. **Curate dialog (pro-d53, DESIGN-panes 2.6).** Click a `curate-retire` row. Expected: its Plan and Session are selected, and the dialog `Retire records of <context>` opens with one checkbox per record, all checked. Uncheck one and click `Confirm`: the dialog closes; the context's records are retired (`cpd context retire`).
11. **Async action (pro-d53, DESIGN-panes 2.7).** Click `Dismiss` on a row. Expected: the button is disabled and shows a spinner, then the row leaves the list. Force a failure (for example a row whose action the engine refuses): the button is enabled again and a red error toast appears at the bottom right with the error code, dismissible with `Dismiss error`.
12. **Breadcrumb (pro-d60, DESIGN-panes 4.1).** Select a Session of a Context. Expected: the header reads `<hostname> / contexts / <context> / <plan> / <worker or fork>`; `standing` for `cpd`, `host` or `codereview`. Click the host crumb: the host Manager chat opens. Click the Context crumb: the Context's Manager chat opens. Click the Plan crumb: the Plan and its Lead chat open.
13. **Chat title (pro-d60, DESIGN-chat 1).** The chat header shows only the session title (or key when untitled). No breadcrumb, no toggle buttons and no run status are in it.
14. **Composer (pro-d54, pro-d57, DESIGN-compose 1).** Expected: the composer has a header (queued messages and attachments), a textarea, and a footer with the attach button, the Tier, Model and Effort menus, and Send. There is no Steer, Follow-up or Stop control, while busy or idle. Type and press Enter: the message is sent as a follow-up. Send is disabled when empty.
15. **Border pulse (pro-d58, DESIGN.md 2h).** Send a message: while the session is busy, the composer border pulses. When it is idle, the border is still.
16. **Footer (pro-d57, DESIGN-compose 3).** Expected: no row of model, tier, cost, tokens or cwd above the composer. Only the `N% context` button, right-aligned. Hover it: a card shows `Context` and a bar.
17. **Chat width and turns (pro-d57, DESIGN-chat 2.2).** Expected: user messages sit at the right and assistant messages at the left; each is at most 85% of the column width.
18. **Sent time and copy (pro-d56, pro-d57, DESIGN-chat 2.3).** Expected: a user message shows `You · HH:MM` under it and a copy button with the tooltip `Copy`. An assistant message has no session key or stamp above or below it (pro-d59).
19. **Auto-scroll (pro-d57, DESIGN-chat 2.8.1).** Scroll up by more than 160 px and send nothing: new messages do not move the view, and the jump-to-bottom button shows. Scroll back to the end: the view follows new messages.
20. **Chain of Thought (pro-d61, DESIGN-chat 2.4.2).** A turn with thinking and tool calls shows one collapsed row `Chain of Thought` with `N steps` and a time. Expand it: the thoughts and the tool calls, in order. Two tools in a row are one `Tool Group`.
21. **Tool rows (pro-d56, pro-d57, DESIGN-chat 3.2).** Each tool row is closed; an `edit` is closed too. A file path longer than 100 characters ends in `…`.
22. **Image thumbnails (pro-d61, DESIGN-chat 2.3.1).** Attach an image and send it: the pending message shows its thumbnail at once, and after the echo the transcript message still shows it.
23. **Questionnaire (pro-d56, DESIGN-compose 4).** In a pi session, have the agent ask a question with `ask_user_question`. Expected: a multi-step questionnaire in the chat; step through it with Next and Previous; answer; the agent continues in the session. `Escape` or Cancel ends it.
24. **Plan review (pro-d56, DESIGN-compose 5).** In a session with a pending plan review, expected: an outlined card `Plan review · <title>` with the actions `Reject plan`, `Request changes`, `Approve & lock`. `Request changes` opens a feedback box; `Send feedback` is disabled when it is empty. `⌘⏎` approves when the focus is outside the feedback box.

### 9.2 Results

Record the date and time in America/Toronto (for example `2026-11-05 09:07 EST`), the browser, and for each step `pass` or `fail` with what you saw. A `fail` names the step number.

---
