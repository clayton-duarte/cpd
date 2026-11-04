# Phase 1 manual verification (pro-d15)

Dev bench only: minimal visuals, no auth beyond Vite's localhost bind. Contract: [`CONTRACT.md`](./CONTRACT.md). Tick the results table at the end.

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
| Load the page | 5-zone frame: header on top showing `C-P-D`; Context tree left; Plan pane above the chat in the centre; Inbox right; status bar at the bottom | whole page |
| Read the Context tree | Contexts, then Plans with Session buttons and a state dot (aria label gives the state); archived ones are in a separate group. Before data: `Loading contexts…` | left |
| Click a Plan / Lead session | Plan pane shows the Plan title, `Workflow <key>` boxes and Explore jobs; chat attaches (`Select a session` placeholder disappears) | centre |
| Read the Inbox | heading `Inbox (<n>)`, one row per item with its actions; `Nothing needs you` when empty; `Engine is not running` appears if the engine is stopped (check with `cpd watch --check`) | right |
| Click an Inbox row's session button | that session is selected | right then centre |
| Select a session, read the status bar | `Runner: pi` or `Runner: durable` matches `cpd session list`; `State: …` matches the tree dot; `cpd: <version>` (or `cpd: unknown`) | bottom |
| Select nothing (reload) | `Runner: none`, `State: none`, plan pane `No plan selected` | bottom, centre |

The frame is minimal; the full design is Phase 3/4.

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

## 3. Known limitations of Phase 1

- **Durable 1.3.0 only (A3):** only pi-durable 1.3.0 host frames (`snapshot`/`events`) are understood. A 1.5.0 `ConversationView` is not verifiable yet; an unknown host version or frame shape fails loud with `unsupported_host_version` / `protocol_error`, never a silent fallback.
- **pi sessions (A4):** read from the session JSONL and written through `cpd session send` / `cpd session retier`. No abort, steer, images, model or thinking change, or compaction (cpd has no command for them); interactive dialogs are read-only. The UI disables each control and shows the reason.
- **Dev bench:** runs only under `vite serve`; transport is Vite `server.ws` custom events (`cpd:c2s`, `cpd:s2c`); no auth beyond Vite's localhost bind; `vite build` output is not runnable.
- **No visual polish:** unstyled semantic HTML and one small CSS file; the design is Phase 3/4.

## 4. Results

| # | Area | Durable | Pi | Notes |
|---|---|---|---|---|
| 1 | Cockpit frame and panes | [ ] | [ ] | |
| 2 | Chat title and footer | [ ] | [ ] | |
| 3 | Live attach, Load earlier, send, steer/follow up, abort | [ ] | [ ] (limits as expected) | |
| 4 | Image and file import | [ ] | [ ] (images refused as expected) | |
| 5 | Tier / model / thinking | [ ] | [ ] (model, thinking disabled as expected) | |
| 6 | Interactive dialogs | [ ] | [ ] (read-only as expected) | |
| 7 | Streaming, Thinking…, tools, diffs, cpd and compaction folds | [ ] | [ ] | |
