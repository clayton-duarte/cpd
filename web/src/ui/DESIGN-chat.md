# Dango v3 chat transcript: design contract (ChatHeader, transcript rows, folds, tools)

Scope: Plan `webui-phase3-good`, Workflow `ui`, package `web/`. This file is the contract for the chat surface: `ChatHeader`, the user bubble, the assistant text (a Bubble with no card) with typeset Markdown, the cpd Marker rows, the compaction divider, the Chain of Thought row (pro-d61), Tool rows and Tool Groups, the pending, failed and queued rows, and the user message with its image thumbnails. The chat is the approved preview's (pro-d55 to pro-d61). Tests are written from this file alone, the implementation from the tests and this file.

It builds on `web/src/ui/DESIGN.md` (called **DESIGN.md**). Reused by reference, never restated: the hook names (DESIGN.md 1.1), the styling rules (1.2: Tailwind utilities, tokens not hex, `cn`, `data-tone`/`data-glyph` from `dango-status.ts`), the tokens (2b), the glyph classes (2c, `glyph-active`, `glyph-done`, `glyph-blocked`, ...), `glyphClass` / `toneClasses` (2d), the `Button` part (2f) and the typeset preset `typeset typeset-chat` (2g). Decisions: pro-d16, pro-d18, pro-d19, pro-d21, pro-n11, pro-n12, pro-n14, pro-n15, and pro-d55 to pro-d61 (amended in section 6); assumptions A1 to A6.

Sources read: the boards' chat zone (`Main.dc.html` lines 257 to 402, via `sc-main-board.md` section 3b), COMPONENTS.md rows for the title bar, user message, Thinking, tool groups, `ToolTickStrip`, φ notices and compaction, and the Phase 2 code (`web/src/ui/chat/*`, `web/src/ui/ChatPane.tsx`, `web/src/state/store.ts`, `web/src/protocol/domain.ts`) with the Phase 2 test inventory (`sc-web-chat.md` section 7).

**Contradiction rule** (as DESIGN.md): the board's rendered value wins over the handoff text; Phase 2 behaviour (data, ARIA, hooks, tests) wins over the board unless this file says otherwise. Each resolution is one line marked **Resolution**.

**File boundary (pro-n15) and no backend change (A6), as written for Phase 2.** For pro-d48 to pro-d61 the boundary is widened by the protocol and store changes listed in CONTRACT section 11 (pending `at`, `images` on pending rows, the echoed image bytes); no other `web/src/protocol/**`, `web/src/state/**` or `web/src/server/**` change is a chat contract. `web/src/ui/chat/virtual.ts` and `rows.ts` are unchanged in behaviour. A design element whose data the protocol or the store does not carry renders only when an optional prop is passed, or is omitted, never faked; each is listed under **Not wired** (section 5).

**Shadcn parts (pro-d55).** The chat is built from shadcn parts in `web/src/ui/components/` (2f): `MessageScroller` family (the transcript), `Message` and `Bubble` (every message), `Trace` and `TraceCollapsible` (the Chain of Thought, the tool rows, the tool groups, the thinking blocks), `Marker` (cpd folds, compaction, the working marker), `CodeBlock` (tool output, edit diffs, fenced code), `Attachment` (non-image attachments), `Tooltip` (the Copy action), `Empty`, `Spinner` and `Button` (`Load earlier`, the header-less controls). **Resolution:** the Phase 2 native `details`/`summary` blocks are replaced by these parts; the Phase 2 hooks they carried are kept on the part's root (section 4).

---

## 0. Shared pieces

### 0.1 `web/src/ui/chat/format.ts` (new, pure, no React)

```ts
/** "HH:MM" (24 h, zero padded, the browser's local time zone) of an ISO timestamp; null when `at` is undefined or not a valid date. */
export function formatClock(at: string | undefined): string | null;
/** ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`. Moved from ToolRow.tsx, same output (Phase 2 pins "1.5s"). */
export function formatDuration(ms: number): string;
/** n < 1000 ? String(n) : `${Math.round(n / 100) / 10}k`. 1234 -> "1.2k", 412000 -> "412k", 999 -> "999". */
export function formatTokens(n: number): string;
```

`formatClock("2026-05-04T13:58:07-04:00")` is `"13:58"` when the local zone is EST. A test runs with a fixed `TZ` or builds the ISO string from a local `Date`; it never depends on the machine zone.

### 0.2 Toggle state: `web/src/ui/chat/useChatToggles.ts` (new)

```ts
export type ThinkingMode = "folded" | "expanded";
export type ToolsMode = "grouped" | "expanded";
export interface ChatToggles {
  readonly thinking: ThinkingMode;          // initial "folded"
  readonly tools: ToolsMode;                // initial "grouped"
  readonly toggleThinking: () => void;      // folded <-> expanded
  readonly toggleTools: () => void;         // grouped <-> expanded
}
export function useChatToggles(): ChatToggles;
```

- State is local React state (`useState`), not persisted, not in the store. Owner: `ChatPane` (one call per mount), which passes it down (2.1). The Chain of Thought and Tool Group defaults follow it (2.4.2, 3.4).
- The hook registers one `keydown` listener on `document` for its lifetime (added on mount, removed on unmount, no dependency churn: it uses functional `setState`). A key event toggles when `event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && !event.isComposing` and `event.key.toLowerCase()` is `"t"` (thinking) or `"o"` (tools); it then calls `event.preventDefault()`. Focus inside the composer textarea does not matter (the hints are global in the board).
- The toggles have **no buttons** (pro-d57: the header Thinking and Tools buttons are removed). The shortcuts are the only control; `Ctrl+T` and `Ctrl+O` are unverified on platforms that reserve them. On macOS (`⌃` = Control) both reach the page.

### 0.3 Vertical rhythm (superseded, pro-d55)

The Phase 2 rule (vertical space as padding inside a measured wrapper, for the window) no longer applies: the transcript is a shadcn `MessageScroller` that lays rows out in normal flow with `gap-3` on its content (2.9). Nothing in the chat is measured by row.

---

## 1. ChatHeader (`web/src/ui/chat/ChatHeader.tsx`)

pro-d60: the ChatHeader shows only the title. The breadcrumb lives in the cockpit Header (DESIGN-panes 4.1); the Thinking and Tools toggle buttons and the run status are removed (pro-d57).

### 1.1 Props

```ts
export interface ChatHeaderProps {
  readonly sessionKey: string;                 // KEPT: `view.ref?.key ?? view.id`
  readonly title: string | null;               // KEPT
  readonly role?: SessionRole;                 // KEPT in the type; not rendered
  readonly runner?: Runner;                    // KEPT in the type; not rendered
}
export function ChatHeader(props: ChatHeaderProps): JSX.Element;
```

`ChatPane` passes `sessionKey`, `title` and, as before, `role` and `runner` only when `view.ref` is non-null (`{...(view.ref === null ? {} : { role, runner })}`). No `busy`, no toggle props. The Phase 2 props `thinking`, `onToggleThinking`, `tools`, `onToggleTools`, `busy` are removed from the type.

### 1.2 Behaviour

- The title is `title ?? sessionKey`. The title is the only text of the header.
- `document.title` (KEPT, unchanged): `sessionKey` when `title === null`, else `` `${sessionKey} · ${title}` ``; restored on unmount; effect deps `[sessionKey, title]`.
- No breadcrumb, no key sub-line, no run status, no buttons.

### 1.3 DOM

```
header   flex flex-wrap items-center gap-3 border-b px-4 py-2
└─ div[data-part="heading"]   min-w-0 flex-1
    └─ h1[data-part="title"]   m-0 truncate text-title font-semibold text-fg-strong      text = title ?? sessionKey
```

- `h1[data-part="title"]` is the only heading. Its text is the title, or the session key when there is no title (the `h1` is not a separate key line any more).
- **Resolution:** the Phase 2 `h1[data-part="key"]` sub-line, `span[data-part="run-status"]` and the Thinking/Tools `div[data-part="toggles"]` are removed; their KEPT hooks are retired (4).

---

## 2. Transcript (`web/src/ui/chat/Transcript.tsx`)

pro-d55 (vanilla shadcn parts), pro-d56 (messages), pro-d57 (85% turns, no noisy loaders, auto-scroll, tool paths, sent time), pro-d59 (no stamp on assistant messages), pro-d61 (Chain of Thought, image thumbnails).

### 2.1 Props

```ts
export interface TranscriptProps {
  items: readonly TranscriptItem[];         // KEPT
  hasEarlier: boolean;                      // KEPT
  loadingEarlier: boolean;                  // KEPT
  onLoadEarlier(): void;                    // KEPT
  tailEpoch: number;                        // KEPT (drives the auto-scroll reset, 2.8.1)
  queued: readonly QueuedSubmission[];      // KEPT
  pending: readonly PendingSubmission[];    // KEPT
  observer?: SizeObserver;                  // KEPT in the type; not used (2.9)
  sessionKey?: string;                      // KEPT
  thinking?: "folded" | "expanded";         // KEPT; the default open state of Thinking blocks (2.4.2). Default "folded".
  tools?: "grouped" | "expanded";           // KEPT; the default open state of Tool Groups and Chains. Default "grouped".
}
```

`ChatPane` calls `useChatToggles()` (0.2) and passes `thinking` and `tools`. The toggles have no buttons: they are the keyboard shortcuts `Ctrl+T` and `Ctrl+O` only (0.2).

### 2.2 DOM outline (whole transcript)

```
MessageScrollerProvider  threshold 160 px, scrollToEndOnMount            (2.8.1)
├─ TranscriptAutoScroll  renders nothing                                  (2.8.1)
└─ MessageScroller[data-part="transcript"]   flex min-h-0 flex-1 flex-col overflow-hidden
    ├─ MessageScrollerViewport[aria-label="Conversation transcript"][data-part="viewport"]   flex-1 overflow-x-hidden px-4 py-4
    │   └─ MessageScrollerContent   mx-auto flex w-full max-w-4xl min-w-0 flex-col gap-3
    │       ├─ div.flex.justify-center.py-1  (only if hasEarlier)
    │       │   └─ Button[type=button][variant=outline][size=sm][data-action="load-earlier"]  "Load earlier"  ("Loading…" while loadingEarlier; disabled)
    │       ├─ Empty[data-empty]  (only when there are no rows, no queued and no pending)  my-12 border-none py-12 text-muted-foreground
    │       │   ├─ EmptyHeader › EmptyMedia[variant="icon"] › MessageSquareIcon
    │       │   ├─ EmptyTitle        "No messages yet"
    │       │   └─ EmptyDescription  "Send a message below to start the conversation."
    │       ├─ MessageScrollerItem[data-row={row.key}] ×rows, in `groupRows` order   flex w-full min-w-0 flex-col
    │       │   └─ div   user row: ml-auto w-full max-w-[85%]; other rows: mr-auto w-full max-w-[85%]; min-w-0 overflow-hidden
    │       │       └─ RowBody (2.3 to 2.5, 3.x)
    │       └─ ul[aria-label="Queued messages"]  (only when queued or pending is non-empty)   m-0 ml-auto flex w-full max-w-[85%] flex-col gap-2 p-0
    │           ├─ li[data-queued={clientSubmissionId}] ×queued      (2.6)
    │           └─ li[data-pending={state}] ×pending                (2.7)
    └─ MessageScrollerButton[data-action="jump-to-bottom"]   shadcn secondary icon-sm, direction end (2.8)
```

- **Width (pro-d57):** every row is at most 85% of the 4xl column: the user's rows right (`ml-auto`), the assistant's and the chain's rows left (`mr-auto`). The user's bubble is muted (2.3), so the right edge matches it.
- The `data-row` attribute holds the row key (`groupRows`, `rows.ts`, unchanged): the user item's id, the assistant text item's id, `group:<first tool id>` for a Tool Group, `chain:<first step key>` for a Chain of Thought, and the item id otherwise.

### 2.3 User message

Component: `Item`, case `user`. A `Message[align="end"][data-kind="user"]` (shadcn `Message`, `components/message.tsx`) holding a `MessageContent`:

```
Message[data-kind="user"][data-align="end"]
└─ MessageContent
    ├─ div[data-part="attachments"]   AttachmentGroup justify-end      (only when item.attachments.length > 0; 2.3.1)
    ├─ Bubble[align="end"][variant="muted"][data-part="bubble"]   max-w-full min-w-0 overflow-hidden
    │   └─ BubbleContent[data-part="text"]   min-w-0 overflow-hidden break-words whitespace-pre-wrap      text = item.text
    └─ MessageFooter[data-part="meta"]   gap-1.5
        ├─ span   "You · {HH:MM}" when formatClock(item.at) !== null, else "You"       (pro-d57: the sent time)
        └─ CopyTurnButton   (only when item.text !== "")                              (2.3.2)
```

- The bubble is the shadcn `Bubble` with `variant="muted"` (a fill, no border), as on the assistant side (2.4). The Phase 2 `div[data-part="text"]` inline style is replaced by the class list above.
- **Resolution:** the Phase 2 meta `span[data-part="meta"]` text `you · HH:MM` (lower case) becomes `You · HH:MM`, and the Copy action is added beside it.

**2.3.1 Attachments and image thumbnails (pro-d61).** `UserAttachments` renders each attachment in order:

- an `image` attachment with `dataBase64`: `div[data-attachment="image"]` (`overflow-hidden rounded-xl border bg-card shadow-xs`) holding `img[alt="Attached screenshot"][data-part="thumb"]` (`block max-h-32 w-auto max-w-52 object-cover`, `src` = `data:<mimeType>;base64,<dataBase64>`);
- any other attachment (an image without bytes, or a text file): `Attachment[size="sm"][data-attachment="<kind>"]` with `AttachmentMedia[data-part="thumb"][variant="icon"]` (`ImageIcon` for an image, `FileTextIcon` otherwise) and `AttachmentTitle[data-part="name"]` (`font-mono`), the text being the `mimeType` for an image and the file name otherwise.

The same component renders the **optimistic** user message (a pending submission's `images`, 2.7) and the **echoed** one (the transcript item's `attachments`, with `dataBase64` from the normaliser, CONTRACT 6.5). A pending or echoed image therefore shows its thumbnail before and after the echo.

**2.3.2 Copy action.** `CopyTurnButton`: `TooltipProvider` > `Tooltip` > `TooltipTrigger asChild` > `Button[type="button"][variant="ghost"][size="icon-xs"][data-action="copy-turn"]` (`size-5 text-muted-foreground`), `aria-label` `Copy message` (`Copied` for 1500 ms after a click); the icon is `CopyIcon` (`CheckIcon` while copied); `TooltipContent side="bottom"` text `Copy` (`Copied` while copied). A click writes the text to the clipboard and sets the copied state.

### 2.4 Assistant text

Component: `Item`, case `assistant`. The row holds only the text blocks of the item (`groupRows` splits the thinking blocks into a chain, 2.4.2). Each text block is a muted `Bubble`:

```
Message[data-kind="assistant"][data-align="start"]
└─ MessageContent
    ├─ Bubble[variant="muted"]   max-w-full min-w-0 overflow-hidden        (one per text block, in order)
    │   └─ BubbleContent[data-part="card"]   w-full min-w-0 overflow-hidden break-words
    │         <Markdown text={b.text} />   (2.4.1)
    ├─ Marker[role="status"][data-part="working"]   (only while item.streaming; always after the bubbles)
    │   ├─ MarkerIcon › Spinner
    │   └─ MarkerContent.shimmer  "Working…" (U+2026)
    └─ MessageFooter.gap-2   (only when there is a text block)
        ├─ span   formatClock(item.at), when non-null
        └─ CopyTurnButton value = the text blocks joined by a blank line (2.3.2), only when non-empty
```

- **Pro-d59:** there is **no** `MessageHeader` and no session-key stamp on assistant messages. The Phase 2 `span[data-part="stamp"]` is removed.
- The `Working…` marker is the only streaming cue left in the transcript. It is a `Marker`, its text uses `shimmer` (DESIGN.md 2h). The `Thinking…` / `Using <tool>…` working cue of Phase 2 (2.4.3) is **removed** (pro-d57 "remove noisy Thinking/Running loaders").
- An item with only thinking blocks shows no bubble, no footer and no marker; its thinking is inside a Chain of Thought row (2.4.2).

#### 2.4.1 `Markdown.tsx` (`web/src/ui/chat/Markdown.tsx`)

`splitMarkdown` and `Segment` are unchanged; `react-markdown` plus `remark-gfm` only. `Markdown` returns `null` for no segments (KEPT) and otherwise `<div className="typeset typeset-chat">` (DESIGN.md 2g). Preview change (pro-d55): a fenced code block is a shadcn `CodeBlock` (`size="sm"`, `not-prose`, `my-2`) with a `CodeBlockHeader` (the language as `CodeBlockTitle`, `font-mono`) and a `CodeBlockCopyButton` on the raw text, and a `CodeBlockContent` with `lineNumbers`. `Markdown.test` 1 to 5 test `splitMarkdown` and the segment memo and are unaffected by the block part.

#### 2.4.2 Chain of Thought (pro-d61): thinking and tool steps

`groupRows` (`web/src/ui/chat/rows.ts`) turns each **run of consecutive Thought and Tool items** into one row of kind `chain`: a `Thought` is one thinking block of an assistant item, a `Tool` is one tool item. Text, user messages, folds, compaction and orphan results end a run. Within the run, two or more consecutive tools form a `tools` step (a Tool Group, 3.4) and a single tool is a `tool` step. `ChainOfThoughtRow` renders the row:

```
Trace[data-kind="chain-of-thought"]
└─ TraceCollapsible[open = open ?? (thinking === "expanded" || tools === "expanded")]   collapsed by default
    ├─ TraceCollapsibleTrigger
    │   └─ TraceLine[status = running ? "running" : failedCount > 0 ? "error" : none]
    │       ├─ TraceIcon   Spinner while running, else BrainIcon
    │       ├─ TraceTitle[data-part="label"]   "Chain of Thought"
    │       └─ TraceDescription[data-part="meta"]   font-mono text-xs   parts joined by " · ":
    │             "{N} step" or "{N} steps"   (N = thoughts + tools; a tool inside a Tool Group counts once)
    │             "{k} failed"   (only when k = failed tools > 0)
    │             "{HH:MM}"   (formatClock of the row's `at`: the last thought's time; only when non-null)
    └─ TraceCollapsibleContent[data-part="chain-body"]   flex flex-col gap-2
        └─ the steps, in order:
            ├─ thinking → ReasoningBlock (below)
            ├─ tool     → Trace[data-part="tool-rail"] › ToolRow (3.2)
            └─ tools    → ToolGroup (3.4)
```

- **Reasoning block** (`ReasoningBlock`): `Trace[data-kind="thinking"]`. Its line: `TraceIcon` (`Spinner` while running, else `BrainIcon`), `TraceTitle[data-part="label"]` `Thinking` while running, else `Thought`, and `TraceDescription[data-part="meta"]` with the thought's clock (`formatClock(at)`, when non-null). A thought with text is a `TraceCollapsible` whose content is `div[data-part="thinking-body"]` (`whitespace-pre-wrap`, the Markdown of the text); a thought with no text is the line alone. Its open state: `open ?? (running || defaultOpen)`, `defaultOpen = thinking === "expanded"`.
- **Default**: the Chain is collapsed by default (`thinking` and `tools` both at their defaults). The Chain's `data-state` is `open` or `closed`. A Chain with a running step opens only through its own state (the `running` flag shows the spinner, not the open state).
- **Re-apply:** a change of `thinking` or `tools` clears the local open state, so the new default applies (the Phase 2 re-apply rule, KEPT).
- **Keys:** the Chain's row key is `chain:<key of its first step>` (`rows.ts`, unchanged in behaviour).
- `failedCount` counts failed tools; the trigger's status is `error` when any tool failed, `running` when any step is running.
- **Retired:** the Phase 2 `details`-based Thinking row and its `[data-part="caret"]`. A Thinking block that is not in a Chain (none is produced by `groupRows`) does not exist.

### 2.5 Compaction divider

Component: `CompactionRow` (`web/src/ui/chat/Folds.tsx`), props **KEPT** `{ item: CompactionItem }`.

```
TraceCollapsible[data-kind="compaction"][data-state="closed"]          KEPT hook; never open by default.   div
├─ TraceCollapsibleTrigger
│   └─ Marker[variant="separator"]                                     shadcn Marker (2f): the rules on both sides come from the part
│       └─ MarkerContent  label (below)                                font-mono text-meta text-muted-foreground
└─ TraceCollapsibleContent
     └─ div[data-part="compaction-body"]  <Markdown text={item.summary} />    the one rail; no card
```

Label (exact): `tokensBefore === undefined` → `Context compacted`; else `` `Context compacted · from ${formatTokens(item.tokensBefore)} tokens` `` (1234 → `Context compacted · from 1.2k tokens`).

**Resolution:** COMPONENTS draws `Context compacted · 412k → 38k`; the after-size is not in `CompactionItem`, so only the before-size is shown. `Phase 2 change:` the summary text was `Compaction` / `Compaction (1234 tokens)`, and the summary was a `details` with a card body; both are replaced by the parts above. `Transcript.test` 5 is updated (2.10).

### 2.6 Queued submissions

Component: the `ul[aria-label="Queued messages"]` (2.2), one `li[data-queued="<clientSubmissionId>"]` per entry of `queued` (the messages sent while the agent was busy, awaiting their turn).

```
li[data-queued]
└─ Message[align="end"]
    └─ MessageContent
        ├─ Bubble[align="end"][variant="outline"][data-part="bubble"]   max-w-full
        │   └─ BubbleContent[data-part="text"]   border-dashed whitespace-pre-wrap     text = q.text
        └─ MessageFooter[data-part="queued-status"]   font-mono     "queued"
```

- The queued bubble is the outline variant with a dashed border. It is not muted, so it reads as not yet sent.
- A queued submission has no time and no copy action.
- The Phase 2 cancel `×` (`data-action="cancel-queued"`) is not in the transcript: it stays in the Composer's header chips (DESIGN-compose 1.2, `capabilities.abortSubmission`).

### 2.7 Pending and failed user rows

Component: `li[data-pending="<state>"]` for each entry of `pending` (`sending`, `sent` or `failed`), in the same list after the queued rows.

```
li[data-pending][data-state=sending|sent|failed]
└─ Message[align="end"]
    └─ MessageContent
        ├─ UserAttachments   (the pending `images`, as `dataBase64`; 2.3.1)
        ├─ Bubble[align="end"][variant = state === "failed" ? "destructive" : "muted"][data-part="bubble"]
        │   └─ BubbleContent[data-part="text"]   min-w-0 overflow-hidden break-words whitespace-pre-wrap     text = p.text
        └─ MessageFooter   gap-1.5
            ├─ (state sending or sent)  span[data-part="pending-status"][data-state]   "You · {HH:MM}" or "You"
            │     + CopyTurnButton (when p.text !== "")
            └─ (state failed)  span[data-part="pending-status"][data-state="failed"][role="alert"]
                    flex items-center gap-1.5 font-mono text-destructive
                    ├─ span[aria-hidden="true"]  glyphClass.blocked
                    └─ text: `failed: ${p.error?.message ?? "unknown error"}`
```

- **Resolution (pro-d57 and the Phase 2 defect):** the Phase 2 texts `sending…` and `sent` are removed; a sending or sent row shows the sent time (`You · HH:MM`), which is the status the User needs. Only the `failed` state shows failure text, so a `sent` row never reads `failed` (the Phase 2 defect, fixed in the same way).
- A pending row's time is `p.at` (the store stamps it when the submission is dispatched, CONTRACT 6.2).
- **Matching:** the echoed user item replaces the pending row. The store matches on the normalised text (`normalizeUserText` drops the `Message from …:` prefix and the `[Attached image: …]` lines; CONTRACT 6.2), so a pending row and its echo are one message.

### 2.8 Load earlier, Jump to bottom and pinning

**Load earlier.** `Button[type="button"][variant="outline"][size="sm"][data-action="load-earlier"]` in a `div.flex.justify-center.py-1` at the top of the content (only when `hasEarlier`). Text `Load earlier`; `Loading…` while `loadingEarlier` (the button is disabled then). A click calls `onLoadEarlier`.

**Jump to bottom.** `MessageScrollerButton[data-action="jump-to-bottom"]`, a shadcn `Button` (`variant="secondary"`, `size="icon-sm"`, `direction="end"`) that the scroller shows while the viewport is not at the end. A click scrolls to the end. Its accessible name is the part's default (unverified).

**2.8.1 Pinning and auto-scroll (pro-d57).** The edge threshold is **160 px** (`SCROLL_EDGE_THRESHOLD = 160`, passed to `MessageScrollerProvider threshold`). The viewport counts as at the end when it is within 160 px of the bottom.

`TranscriptAutoScroll` (renders nothing) keeps `pinned`:

- `pinned` follows the scroller's `end` flag (`pinned = !scrollable.end`, updated when `end` changes).
- when `tailEpoch` changes (a new tail window of the session), `pinned = true` and the viewport scrolls to the end at once (`behavior: "auto"`);
- when `items`, `queued` or `pending` change and `pinned` is true, the viewport scrolls to the end smoothly (`behavior: "smooth"`);
- when `pinned` is false (the User scrolled up), new rows do not move the viewport. The jump button shows.
- On mount the viewport scrolls to the end (`scrollToEndOnMount`).

### 2.9 Rendering (pro-d55): no windowing

The transcript renders every row: there is no virtual window, no `div[data-row-key]` measured wrapper, no `row-body` padding wrapper and no `[role="log"]`. Rows are `MessageScrollerItem`s (`data-row` = the row key), which the scroller's content lays out in normal flow with `gap-3` between rows (the Phase 2 `py-[3px]` padding rule of 0.3 and the 2.9 Phase 2 contract are superseded).

- `virtual.ts` stays in the repository (its tests are unchanged) but `Transcript` does not use it except for the `SizeObserver` type of the `observer` prop, which is declared and not read (2.1).
- Row heights change by content (a Collapsible opening, a Show-more click, a Markdown stream). The scroller follows them through the pinning rule (2.8.1).

### 2.10 Phase 2 tests this section changes

| Test (file) | Change |
|---|---|
| `Transcript.test` 1 | the user row is `div[data-kind="user"]` (was `article`); an image attachment with bytes is `img[data-part="thumb"]` (`alt` `Attached screenshot`); one without bytes is an `Attachment` whose name is its mime type |
| `Transcript.test` 2 | `[data-kind="thinking"]` now lives in `[data-kind="chain-of-thought"]`, closed by default; the trigger text is `Chain of Thought`; the thought's own trigger text is `Thought` (or `Thinking` while running); `[data-part="thinking-body"]` text contains `pondering` after the Chain is opened |
| `Transcript.test` 3 | `[role="status"][data-part="working"]` text `Working…`, last child of the assistant row, only while streaming |
| `Transcript.test` 4, 11 | `button[data-action="jump-to-bottom"]` is the `MessageScrollerButton` (no text); `Load earlier` unchanged |
| `Transcript.test` 6, 18 | `li[data-pending]` text is the time (`You · HH:MM`), never `sending…` or `sent`; `failed: boom` only for `failed` with `role="alert"` |
| new | the 85% wrapper class on user and assistant rows; the assistant row has no `[data-part="stamp"]`; the Chain's `chain-body` is absent while closed; `thinking="expanded"` opens the Chain; the Queued `li` has no copy action |
| new | the auto-scroll rules of 2.8.1 with the scroller mocked (external), including the 160 px threshold argument |
| `ToolRow.test` 5 | the Tool Group is `[data-kind="tool-group"]` (3.4), `group-count` `3 tool calls` |

---

## 3. Folds and tools

Rules for every block in this section, from the Context's chat decision:
- **One left rail per tool.** The rail is the Trace part's own left border (`trace.tsx`, `TraceCollapsibleContent`). A block adds no second rail, and a Tool Group adds none.
- **No fixed heights.** A row is as tall as its content (2.9). No `h-[22px]`, `h-[26px]` or `h-6` on a block's line.
- **No truncation and no character cut.** No `truncate`, no `cut(…)` of a detail, a title or a failure reason. Text wraps.
- **Nothing is clipped.** Diffs wrap (`whitespace-pre-wrap`); no `overflow-hidden` on a block's content.
- **Cards are gone.** No text block or tool block has `bg-card`, a border around its content, or a rounded box. The Marker and the Trace parts are the only forms. (A user attachment chip keeps its Phase 2 border: it is a chip inside the user message, not a block.)

### 3.1 Marker rows: cpd folds (`web/src/ui/chat/Folds.tsx`)

Component: `FoldRow`, props **KEPT** `{ item: FoldItem }` (`memo`). `FoldKind` is `cpd-context | cpd-plan-decision | cpd-plan-summary | cpd-skills | cpd`; **every** fold kind renders as a Marker row.

```
TraceCollapsible[data-kind="fold"][data-fold={item.foldKind}][data-state="open"|"closed"]   KEPT hook; open only when item.defaultOpen (cpd-plan-summary)     div
├─ TraceCollapsibleTrigger[data-part="marker"]
│   └─ Marker   (shadcn, variant "default", 2f)
│       ├─ MarkerIcon: span[data-part="phi"][aria-hidden="true"]  "φ"    font-display text-[15px] font-bold text-status-decision      (Roboto 700, #ca88a3)
│       └─ MarkerContent
│           ├─ span[data-part="source"]  {item.foldKind}          font-mono text-muted-foreground      (#aba3af)
│           ├─ span[data-part="marker-text"]  {item.title}        text-foreground; wraps, no truncate      (#d8d2cc)
│           └─ span[data-part="time"]  {formatClock(item.at)}     font-mono text-meta text-fg-dim      (only when non-null; 11px, #87808c)
└─ TraceCollapsibleContent
     └─ div[data-part="fold-body"]  <Markdown text={item.body} />   the one rail; no card
```

Examples (from the board): `cpd-plan-decision` · `Plan review r1 approved` · `13:52`; `cpd-context` · `decision · Stream high-water mark is per-session` · `14:01`. `item.title` is built by the store (title rules in `domain.ts`), not here.

- `data-state` is the Trace part's own state, set from `item.defaultOpen` (KEPT behaviour: true only for `cpd-plan-summary`). A click or Enter or Space on the trigger toggles it. No script of cpd's own.
- **Resolution (Context, chat decision; supersedes the Phase 2 native `details` resolution):** the chat blocks use the shadcn `Marker`, `Trace` and `Collapsible` parts (2f). The block root is the part's `div`, and it keeps `data-kind`, `data-fold` and `data-state`; `details` and the native `open` attribute are not used in chat blocks.
- `Phase 2 change:` `Folds.test` 1 asserted the `summary` text equals the title; the trigger now holds the φ, the source and the time, so those assertions read `[data-part="marker-text"]` (text `Title cpd-context`). Tests 2 and 3 read `data-state` in place of `open`; the body text is unchanged.

### 3.2 `ToolRow` (`web/src/ui/chat/ToolRow.tsx`)

```ts
export const ToolRow: MemoExoticComponent<(props: {
  tool: ToolItem;
  nested?: boolean;            // true inside a Tool Group (3.4); the row's look does not change
}) => JSX.Element>;
export function toolName(tool: ToolItem): string;      // KEPT export: the name without a leading "mcp__"
export function toolDetail(tool: ToolItem): string;    // KEPT export: detailOf(tool.name, tool.args), truncated (below)
export function duration(ms: number): string;          // KEPT (ToolRow.tsx): ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`
export const OrphanRow: MemoExoticComponent<(props: { item: OrphanResultItem }) => JSX.Element>;   // KEPT
```

**Open state (pro-d56, "edit tool blocks collapsed by default").** Every tool row starts **closed**, whatever the `tools` mode: `edit` included, and rows inside an expanded Tool Group too. The row's open state is local (`useState(false)`). The `tools` mode opens the Tool Group, not its rows.

**Detail text (pro-d57, "truncate long tool paths").** `toolDetail` is the target of the call, cut by its tool:

| Tool | Detail | Cut |
|---|---|---|
| `read`, `write`, `edit`, `ls` | `args.path` | 100 characters |
| `bash`, `powershell` | the first line of `args.command` | 100 |
| `grep` | `/<pattern>/ in <path>` | 100 |
| `find` | `<pattern> in <path>` | 100 |
| any other | the first present key of `DETAIL_KEYS` (`command`, `query`, `queries`, `q`, `url`, `urls`, `path`, `file_path`, `paths`, `pattern`, `prompt`, `question`, `claim`, `name`, `id`, `tool`), else the first argument; arrays join with `, `, objects are JSON; control characters become spaces and runs of spaces collapse | 160 |

`cut(s, n)` is `s` when it is at most `n` characters long, else the first `n` characters and `…` (U+2026). The Phase 2 "no cut" rule of this section is superseded by this table. The failure reason is still not cut.

**Structure.** The row is a `TraceCollapsible` when it has output, else a plain `div`; both carry the hooks below.

```
TraceCollapsible[data-kind="tool"][data-tool={tool.name}][data-status={tool.status}][data-state="open"|"closed"]    KEPT hooks       div
├─ TraceCollapsibleTrigger
│   └─ TraceLine[className="font-mono text-xs"][status]    status "running" for a running tool, "error" for a failed one, none otherwise
│       ├─ TraceIcon: span[data-part="glyph"][aria-hidden="true"][data-tone][data-glyph]   ToolIcon (the glyph by tool name and status)
│       ├─ span[data-part="name"]  {toolName(tool)}    shrink-0 text-muted-foreground
│       ├─ TraceTitle[data-part="detail"]  {toolDetail(tool)}   text-foreground
│       ├─ TraceDescription[data-part="delta"]  (edit with a diff only)
│       │   ├─ span[data-part="added"]    `+${A}`               text-status-done
│       │   └─ span[data-part="removed"]  `−${R}` (U+2212)      text-status-blocked
│       └─ TraceDescription[data-part="status"]  {statusOf(tool)}   font-mono
└─ TraceCollapsibleContent   (only when the row has output, 3.6)
```

- A row with **no output** (no `result`, or an `edit` with no diff) is `div[data-kind="tool"]` with the same line and no trigger.
- **Resolution:** the Phase 2 `div[data-part="tool-rail"]` is retired. The row's rail is its own `Trace` (3.3).

### 3.3 Lone tool row

A `Row` of `kind: "item"` whose item is a tool renders `Trace[data-part="tool-rail"]` (the shadcn `Trace` root, which draws the rail) around `ToolRow`. A tool step inside a Chain (2.4.2) is wrapped the same way. There is no other wrapper.

### 3.4 Tool Group (`web/src/ui/chat/ToolGroup.tsx`)

A run of two or more consecutive tools (`groupRows` in `rows.ts`, unchanged) is one `ToolGroup` row, key `group:<first tool id>`. A lone tool is a 3.3 row. Inside a Chain of Thought (2.4.2) a run of two or more tools is a `ToolGroup` step.

```ts
export type ToolsMode = "grouped" | "expanded";
export const ToolGroup: MemoExoticComponent<(props: { tools: readonly ToolItem[]; mode: ToolsMode }) => JSX.Element>;
```

**Derived values.** `failed` = count of failed tools; `running` = some tool is running; `agg` = `failed > 0 ? "failed" : running ? "running" : "done"` (failed wins over running); `timed` = the tools with a `result.durationMs`; `total` = their sum.

```
Trace[data-kind="tool-group"][data-tool-count={n}][data-status={agg}]
└─ TraceCollapsible[open = open ?? (running || mode === "expanded")]   reset when `mode` changes
    ├─ TraceCollapsibleTrigger[data-part="group-header"]
    │   └─ TraceLine[className="text-xs"][status = agg "running" → "running", "failed" → "error", else none]
    │       ├─ TraceIcon[data-part="glyph"][data-tone][data-glyph]   running → Spinner · failed → CircleXIcon (text-destructive) · done → CircleCheckIcon
    │       ├─ span[data-part="name"]  {toolName(first)}   shrink-0 font-mono text-fg-strong
    │       ├─ TraceTitle[data-part="summary"]  `${toolDetail(first)} + ${n} more step(s)`   n = tools.length − 1 ("step" for 1, "steps" otherwise)
    │       ├─ ToolTickStrip(statuses)   (3.5)
    │       ├─ TraceDescription[data-part="totals"]  font-mono   text below
    │       └─ span.sr-only[data-part="group-count"]  `${tools.length} tool calls`
    └─ TraceCollapsibleContent
        └─ Trace[data-part="group-rows"] › ToolRow[nested] ×tools
```

- `totals` text: `running` when `running`, else `done`; then ` in ${duration(total)}` when some tool is timed and nothing is running; then ` · ${failed} failed` when `failed > 0`. Examples: `done in 6.2s · 1 failed`, `done`, `running · 1 failed`.
- **Resolution:** the Phase 2 group was a `details` with a `summary` `N tool calls`. It is now a `Trace` with the summary above, and `N tool calls` is the `sr-only` `[data-part="group-count"]`. The group's open state follows `mode` and a running group opens by itself. Its rows are closed (3.2).
- Phase 2 `[data-part="caret"]` and `[data-part="glyph"]` placement changed to the Trace parts above; the `group-header` is the trigger.

### 3.5 `ToolTickStrip` (`web/src/ui/chat/ToolTickStrip.tsx`, new)

```ts
import type { ToolStatus } from "../../protocol/domain.ts";
export function ToolTickStrip(props: { readonly statuses: readonly ToolStatus[] }): JSX.Element;
```

```
span[data-part="tick-strip"][role="img"][aria-label={label}]       flex gap-0.5      (gap 2px)
└─ span[data-part="tick"][data-status={status}]  × statuses.length, same order      block h-[10px] w-[3px] rounded-[1px] + colour class
```

- A tick is a glyph, not a row: its 10px height is the glyph's size and does not set any row's height.
- Colour (literal fills, tokens): `done` → `bg-status-done` (`#8da96e`), `failed` → `bg-status-blocked` (`#e17f6e`), `running` → `bg-status-active` (`#67ada7`). One tick per call: `statuses.length` ticks (12 calls: 12 × 3 + 11 × 2 = 58px wide).
- `label` (exact): `` `${n} steps` `` when no tick is failed, else `` `${n} steps, ${f} failed` `` (board: `12 steps, 1 failed`); `n = statuses.length`, `f` = failed count. A group has `n ≥ 2`, so the singular is not needed; for `n = 1` the label is `1 steps` (never rendered by `Transcript`).
- **Resolution:** the board draws ticks only for done and failed (`No tick for running or queued`); the brief says one tick per call, so a running call has a `bg-status-active` tick (the protocol has no `queued` tool status).
- Not interactive; no focus; no motion.

### 3.6 Output blocks: `bash` output and `edit` diffs

Output is a shadcn `CodeBlock` (`size="sm"`, `components/code-block.tsx`, pro-d55), shown inside the row's closed `TraceCollapsibleContent` (3.2). The first `TOOL_PREVIEW_LINES` (10) lines show, then `Show N more lines`.

**Plain output** (a result with text): `CodeBlock[size="sm"][variant="destructive"]` when the tool failed, else `[variant="default"]`; `CodeBlockActions` > `CodeBlockCopyButton` (the full text); `CodeBlockContent[wrap][data-part="output"]` `max-h-64` with the shown lines; `CodeBlockFooter` > `Button[type="button"][variant="ghost"][size="xs"][data-action="expand-output"]` `font-mono` text `Show {rest} more lines` (only when rest > 0; a click shows all lines).

**Edit diff** (an `edit` with `tool.diff`): `CodeBlock[size="sm"][data-part="diff"]` > `CodeBlockActions` > `CodeBlockCopyButton` (the diff); `CodeBlockContent` `max-h-64`; one `CodeBlockLine[data-diff="add"|"remove"|"context"][variant="added"|"removed"|"default"]` per shown line (an add or remove line shows without its `+` or `-`). The same `Show N more lines` footer applies.

- **Resolution:** the Phase 2 `pre[data-part="output"]` for bash and `div[data-part="diff"]` with `data-diff` rows are replaced by the parts above; `data-part="output"` and `data-part="diff"` and `data-diff` are KEPT on those parts.
- The Phase 2 `[data-diff="add"]` and `[data-diff="remove"]` page CSS is removed (DESIGN.md 2a table); the tone comes from the `CodeBlockLine` variant.
- `Done in 1.5s (3 lines)` and `Done (+1 -1)` are the status line of 3.2 (`statusOf`), unchanged.

---

## 4. KEPT hooks and the test that pins each (as amended)

Hooks that the approved preview keeps, with their owner. A hook marked **retired** is no longer in the DOM and its tests are updated in 2.10 and 3.x.

| Hook (KEPT) | Where | Pinned by |
|---|---|---|
| `h1[data-part="title"]`, text `title ?? sessionKey`; `document.title` rules and restore | ChatHeader (1) | `ChatHeader.test` (updated: no key sub-line) |
| `[data-part="transcript"]`, `[aria-label="Conversation transcript"][data-part="viewport"]` | Transcript (2.2) | `Transcript.test` 1, 9, 10 |
| `[data-row]` (the row key: `u1`, `a1`, `group:tool:t1`, `chain:…`, `c1`, `tool:t3`), one per row | Transcript (2.2, 2.9) | `Transcript.test` 7, 8 (updated: every row is mounted, no window); `Rows.test` 2 |
| `button[data-action="load-earlier"]` text `Load earlier` | Transcript (2.8) | `Transcript.test` 4 |
| `button[data-action="jump-to-bottom"]` (the `MessageScrollerButton`) | Transcript (2.8) | `Transcript.test` 11 (updated) |
| `div[data-kind="user"]`, `[data-part="text"]` (whitespace kept), `[data-attachment="image"]` (2.3.1), `[data-part="meta"]` | user message (2.3) | `Transcript.test` 1 (updated) |
| `[data-kind="assistant"]`, Markdown `strong` | assistant text (2.4) | `Transcript.test` 1; `Rows.test` 1 |
| `[role="status"][data-part="working"]` text `Working…`, last child of the assistant row | working marker (2.4) | `Transcript.test` 3 |
| `[data-kind="chain-of-thought"]`, closed by default (`data-state`); `[data-kind="thinking"]`; `[data-part="thinking-body"]` | Chain of Thought (2.4.2) | `Transcript.test` 2 (updated) |
| `[data-kind="compaction"]`, closed by default | compaction (2.5) | `Transcript.test` 5 (updated) |
| `ul[aria-label="Queued messages"]`; `li[data-queued]`; `li[data-pending]` (state) | queued and pending (2.6, 2.7) | `Transcript.test` 6, 18 (updated) |
| `[data-kind="fold"][data-fold]`, `data-state="open"` only for `cpd-plan-summary` | Marker row (3.1) | `Folds.test` 1, 2, 3 |
| `[data-kind="tool"][data-tool][data-status]`, `data-state="closed"` for every tool by default; `[data-part="name"]`, `[data-part="detail"]`, `[data-part="status"]` | ToolRow (3.2) | `ToolRow.test` 1, 2 (updated) |
| `[data-part="output"]`, `button[data-action="expand-output"]` `Show N more lines` | output (3.6) | `ToolRow.test` 3 |
| `[data-part="diff"]`, `[data-diff="add"\|"remove"\|"context"]`, status `Done (+1 -1)` | edit diff (3.6) | `ToolRow.test` 4 |
| `[data-kind="tool-group"]`, `[data-part="group-header"]`, `[data-part="group-count"]`, one per run of two or more tools | Tool Group (3.4) | `ToolRow.test` 5 (updated); `Rows.test` 2 |
| `groupRows` keys and grouping, `rows.ts` | grouping (2.4.2, 3.4) | `rows.test` 1, 2, 3 (updated: chain rows) |
| `virtual.ts` | not used by the Transcript (2.9) | `virtual.test` 1 to 7 (unchanged) |
| `article[data-kind="orphan-result"]` text `Result of … (call not loaded)` | OrphanRow | no Phase 2 test pins it |
| `splitMarkdown` and the per-segment memo | Markdown (2.4.1) | `Markdown.test` 1 to 5 |
| `footer[aria-label="Session footer"]` | **retired** from the chat; the context card is the Footer of DESIGN-compose 3 | `Footer.test` (updated, DESIGN-compose 3.4) |

**Retired hooks** (no longer in the DOM): `details` and `summary` in chat blocks; `div[data-part="tool-rail"]` as a plain `div` (it is now the shadcn `Trace` root, 3.3); the Thinking pill's `[data-part="caret"]`; `[role="log"][aria-label="Transcript"]`; `[data-at-bottom]` and `div[data-row-key]` (the scroller replaces the window); `span[data-part="stamp"]` (pro-d59); the header `div[data-part="toggles"]`, `[data-action="toggle-thinking"]` and `[data-action="toggle-tools"]`; `span[data-part="run-status"]`; `h1[data-part="key"]`.

**Tests to add** (new files, written from this document): `format.test.ts` (0.1), `useChatToggles.test.tsx` (initial values, both shortcuts with and without meta, alt and shift, listener removed on unmount), `rows.test.ts` (Chain of Thought grouping: a run of thoughts and tools is one chain; a text item ends the run; two tools form a group inside the chain; `chain:` keys), `ToolRow.test.tsx` (the path cut at 100 and the 160 cut; closed by default in both modes), `ChatHeader.test.tsx` (title only; no toggles; the `document.title` rules).

---

## 5. Not wired

Data absent from `web/src/protocol/**` and the store; each element is omitted (or renders only from an optional prop), never faked.

- Thinking duration (`41s · 13:58`): only the clock `HH:MM` of `item.at` is shown.
- Tool Group time (`13:59`) and per-row time as a separate field: `ToolItem` has no timestamp; the duration is inside the status text.
- Attachment file name and size for images (`ci-run-8812.png`, `184 KB`): image attachments carry `mimeType` and optional `dataBase64`.
- `▲ 3,912 earlier messages`: no total count (the copy is `Load earlier`).
- `Running command…` and the running command line, `12s` elapsed: only `busy` and `streaming` exist.
- Compaction after-size (`412k → 38k`) and the optional Checkpoint.
- Failed tool calls breaking out of a collapsed group; per-group expand state memory across toggles.
- Retry, Edit or dismiss of a failed pending row (`dismissPending` exists in the store but `Transcript` has no prop for it).
- Image count on a pending row (`PendingSubmission.imageCount` is not drawn; the thumbnails are).
- Persisted Thinking and Tools modes; per-session modes.
- The TUI's diff trimming to one context line around each change and its `⋮` hunk gap marks: every diff line is shown (3.6).
- Hover timestamps; message reactions.

---

## 6. Amendments pro-d48..pro-d61 (chat)

| Decision | Contract | Section |
|---|---|---|
| pro-d55 | transcript, messages, reasoning, tools, folds and the composer use shadcn parts (`MessageScroller`, `Message`, `Bubble`, `Trace`, `CodeBlock`, `Marker`, `Spinner`, `Empty`, `Tooltip`) | 2.2, 2.3, 2.4, 2.4.1, 2.9, 3.2, 3.6 |
| pro-d56 | user and assistant messages on shadcn `Message` with muted `Bubble`; `MessageFooter` with the Copy `Tooltip`; edit tool blocks collapsed | 2.3, 2.3.2, 2.4, 3.2 |
| pro-d57 | turns at most 85% of the column; no Thinking or Running loaders; no session telemetry row; no header toggle buttons; transcript auto-scroll at 160 px; long tool paths truncated; sent time on user messages | 0.2, 1, 2.2, 2.3, 2.7, 2.8.1, 3.2 |
| pro-d59 | no session-key stamp on assistant messages | 2.4 |
| pro-d60 | ChatHeader shows only the title; `ask_user_question` interactive for pi (DESIGN-compose 4, CONTRACT 6.5) | 1 |
| pro-d61 | consecutive thoughts and tools form one collapsed Chain of Thought row with a step count and a time; user and echoed messages show image thumbnails | 2.3.1, 2.4.2, 2.7 |

---
