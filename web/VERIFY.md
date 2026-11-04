# Manual verification, phases 1 to 4 (pro-d15, pro-d20, Phase 3/4)

The Phase 1 checklist (sections 0 to 4) is kept; the Phase 2 checklist is section 5; the Phase 3/4 checklist (high-fidelity Dango v3 UI) is section 6.

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
| Read the Context tree | Contexts, then Plans with Session buttons and a state dot (aria label gives the state); archived ones are in a separate group. Before data: `Loading contexts…` | left |
| Click a Plan / Lead session | Plan pane shows the Plan title, `Workflow <key>` boxes and Explore jobs; chat attaches (`Select a session` placeholder disappears) | centre |
| Read the Inbox | heading `Inbox (<n>)`, one row per item with its actions; `Nothing needs you` when empty; `Engine is not running` appears if the engine is stopped (check with `cpd watch --check`) | right |
| Click an Inbox row's session button | that session is selected | right then centre |
| Select a session, read the status bar | `Runner: pi` or `Runner: durable` matches `cpd session list`; `State: …` matches the tree dot; `cpd: <version>` (or `cpd: unknown`) | bottom |
| Select nothing (reload) | `Runner: none`, `State: none`, plan pane `No plan selected` | bottom, centre |

The Phase 3/4 look of the frame is checked in section 6.

### 2. Chat title and footer

| Action | Expected | Look at |
|---|---|---|
| Select a session with a dynamic title | chat header shows the session key (h1) and the title next to it; the browser tab title is `<key> · <title>`; no title means just the key | chat top, tab |
| Read the `Session footer` bar | `model:`, `tier:`, `thinking:`, `context: <n>%`, `cost: $<x.xxx>`, `branch:`; a value that cannot be known shows `unknown` with the reason as hover text, never a made-up value. Cross-check tier/model/thinking with `cpd session show <id>` | chat bottom |
| Before attach completes | footer shows `unknown` for all six (`not attached`) | footer |

### 3. Live attach, paging, send, steer, abort

Repeat for the **durable** and the **pi** session.

| Action | Expected (durable) | Expected (pi: limits are EXPECTED, A4) |
|---|---|---|
| Select the session | transcript shows the tail window, at most 40 items, newest at the bottom | same, read from the session JSONL (`file` of `cpd session show <id>`) |
| Click `Load earlier` | the previous 40 items are prepended, scroll position stays, button disappears when the start is reached | same |
| Type in `Message`, click `Send` (or Enter) | your message appears, the reply streams live | same, delivered with `cpd session send`; the reply shows up as the JSONL file grows |
| While the agent is busy, type text and click `Steer` | message delivered mid-run | `Steer` disabled, a reason is shown beside it and as hover text |
| While busy, click `Follow up` | message queued (`Queued messages` list), runs after the turn; `Cancel queued message` removes it | `Follow up`/`Send` follow what the capabilities say; if disabled the reason is shown |
| While busy, click `Abort` | the run stops | `Abort` disabled: "cpd has no command to abort a pi session turn" |
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
| Pi: attach an image and send | EXPECTED: refused, reason "pi sessions take prompts only through `cpd session send`, which carries text and no images"; nothing is sent | composer |
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
| Pi: a pending ask or plan review in the transcript | rendered READ-ONLY (no answering), with the reason "a TUI dialog of a pi session cannot be answered from outside…"; answer it in the TUI | chat |

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
- **pi sessions (A4):** read from the session JSONL and written through `cpd session send` / `cpd session retier`. No abort, steer, images, model or thinking change, or compaction (cpd has no command for them); interactive dialogs are read-only. The UI disables each control and shows the reason.
- **Dev bench:** runs only under `vite serve`; transport is Vite `server.ws` custom events (`cpd:c2s`, `cpd:s2c`); no auth beyond Vite's localhost bind; `vite build` output is not runnable.
- **Phase 2 (CONTRACT 10.9):** no keep-mounted rows for selection, no in-app search, fold open state is not persisted across unmount, no v2 `history` paging beyond the host's newest-50 window, reference-style Markdown link definitions across segments do not resolve, from-null `cpd` nudges show as User messages, durable `cpd-event`/`cpd-first-message`/`cpd-error`/`cpd-plan-review` entries stay dropped.
- **Unverified without a running v2 host:** tool-call streaming ops, multi-round tool runs, window overflow ops, history paging, `host_state.name` as title.
- **Not covered by Phase 2 (docs/research/webui-chat-base.md):** C8 instant session switch, C9 highlighting off the hot path, C10 virtualisation escape hatches (search, keep-mounted, fold state across unmount), C11 heavy rows bounded, C12 hooks exposed by the base.
- **Phase 3/4 (high-fidelity UI):** dark theme only (no light theme); fonts are CSS stacks, none is bundled, so a machine without IBM Plex Sans, EB Garamond or Maple Mono falls back to the next family of the stack; no responsive layout below 980px and no Sheet drawers; panel sizes are not persisted; no command palette (⌘K), no Reset layout, Legend, Logs or Tiers dialogs, no Workers meter, host CPU/RAM/disk or incident pill (their data is not in the protocol); Snooze and Dismiss in the inbox are disabled hints (no backend action); session cost `$` and per-plan cost are not shown; the Context tree has no Active/Standing sections and no collapse; every element whose data the protocol does not carry is listed under "Not wired" in the DESIGN files and is absent from the UI, never faked.

## 4. Results (Phase 1)

| # | Area | Durable | Pi | Notes |
|---|---|---|---|---|
| 1 | Cockpit frame and panes | [ ] | [ ] | |
| 2 | Chat title and footer | [ ] | [ ] | |
| 3 | Live attach, Load earlier, send, steer/follow up, abort | [ ] | [ ] (limits as expected) | |
| 4 | Image and file import | [ ] | [ ] (images refused as expected) | |
| 5 | Tier / model / thinking | [ ] | [ ] (model, thinking disabled as expected) | |
| 6 | Interactive dialogs | [ ] | [ ] (read-only as expected) | |
| 7 | Streaming, Thinking…, tools, diffs, cpd and compaction folds | [ ] | [ ] | |

## 5. Phase 2 checklist (pro-d20)

Prerequisite additions: a pi-runner session; a durable v1 session (pi-durable 1.3.0 host); for step 8 a durable v2 session (pi-durable 1.5.0 host); the largest local pi session file, `<your largest ~/.pi/agent/sessions/*.jsonl>` (about 25 MB on the User's machine; find it with `ls -S ~/.pi/agent/sessions/*.jsonl | head -1`).

### 5.1 Steps

| # | Steps | Expected |
|---|---|---|
| 1 | pi-runner round trip. (a) Select the pi session, type a message in the chat, press Enter. (b) From another session run `cpd session send <pi-id> "ping" --unblocks "verify"` (or have another session message this one). | (a) Within a second your text appears as a User message in the transcript, exactly once: the pending row (`sending…`, then sent) disappears when the User message arrives, and no duplicate remains after 30 s. (b) The message from the other session shows as a collapsed fold whose summary reads `Message from <key>` (fold kind `cpd`); click opens it. |
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
| 7 | Read the header. | Left: a φ mark in a serif face (28px, `#f3ead9`); then, when a Plan is shown, the breadcrumb `<context> / <plan>` in monospace 12px; right: `engine` with a glyph and `running` (green dot) when `cpd watch --check` says the engine runs. Stop the engine: the glyph becomes a triangle and the text `not running`. No search box is shown (command palette not wired). |
| 8 | Read the status bar. | Right side: `Runner: pi`/`durable`/`none`, `State: <state>` with a glyph, `cpd: <version>` or `cpd: unknown`, all matching Phase 1 step 1. No Legend, Logs, Tiers, Workers, CPU/RAM or incident element appears. |
| 9 | Read the Context tree: header row `CONTEXTS`; contexts in monospace semibold; plans under them with a glyph, the title and labelled counts such as `merged 13 · running 1` (numbers coloured: running teal, merged green, failed red); sessions with a state dot. Hover a row. | Row heights: context 28px, plan and session 26px. Hover tints the row `#24212a`. The count words are present. A plan or context row's glyph is the most urgent state of its sessions (error beats landing beats waiting-on-you beats working). Archived items sit under `Archived (<n>)`. |
| 10 | Click a plan, then a session. | The clicked row gets a blue tint (`rgba(122,162,206,.16)`) and brighter text; the Plan pane and chat follow as in Phase 1 step 1. |
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
| 18 | Read the composer and footer; type text; press Enter; while the agent runs queue a follow-up. | Composer: 10px-radius card with a `#a1c6ef` border, a multi-line `Message` textarea, `Attach files`, the tier/model/thinking selector pill (`S2 claude-… high`), `Steer ⏎` as the one filled blue button, `Follow-up ⌥⏎`, `Stop ⌃C` in red outline while busy; a queued follow-up shows as a dashed chip with `Cancel queued follow-up`. Footer 30px, monospace 11px: cwd, key, branch, tokens, cost, a context meter, model and thinking; unknown values read `unknown` (Phase 1 step 2), never invented. Every Phase 1 and 2 behaviour (send, steer, abort, attach, drop, paste, rejections) still works; disabled controls keep their reason. |
| **Ask card** (DESIGN-compose.md) | | |
| 19 | Durable: make the agent call `ask_user_question` with 2 or 3 questions, one multi-select, one option with a preview. Pick answers with the mouse, then with the keyboard (1 to 4, arrows, Tab), and Submit; repeat with `Type something.` and with Esc. | A pink-bordered card, tabs per question with answered/unanswered marks, options with numbered badges and the `recommended` badge where given, the preview pane beside the options when two-column, `Submit N/M answers ⏎` as the one filled button; the answers reach the agent exactly as in Phase 1 step 6; Esc or `Cancel` cancels. A pi session shows the read-only card with its reason. |
| **Plan review card** (DESIGN-compose.md) | | |
| 20 | Durable: a Lead in phase `plan` calls `cpd_plan_review`. Read the card; use `Approve`, then on a second review `Request changes` with feedback, then on a third `Reject`. | A card headed `Plan review · <plan>` with the summary rendered as Markdown; a decision bar with a `Feedback` textarea, `Reject` (red text), `Request changes`, and `Approve` as the one filled button; each outcome reaches the agent as in Phase 1 step 6 and `cpd plan show <id>` agrees. A pi session shows it read-only. |

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
| 19 | Ask card | [ ] | [ ] (read-only as expected) | |
| 20 | Plan review card | [ ] | [ ] (read-only as expected) | |
