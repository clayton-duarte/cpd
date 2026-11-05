# Dango v3 chat transcript: design contract (ChatHeader, transcript rows, folds, tools)

Scope: Plan `webui-phase3-good`, Workflow `ui`, package `web/`. This file is the contract for the chat surface: `ChatHeader`, the user bubble, the assistant text (a Bubble with no card) with typeset Markdown, the cpd Marker rows, the compaction divider, the Thinking row, Tool rows and Tool Groups, the pending, failed and queued rows. Tests are written from this file alone, the implementation from the tests and this file.

It builds on `web/src/ui/DESIGN.md` (called **DESIGN.md**). Reused by reference, never restated: the hook names (DESIGN.md 1.1), the styling rules (1.2: Tailwind utilities, tokens not hex, `cn`, `data-tone`/`data-glyph` from `dango-status.ts`), the tokens (2b), the glyph classes (2c, `glyph-active`, `glyph-done`, `glyph-blocked`, ...), `glyphClass` / `toneClasses` (2d), the `Button` part (2f) and the typeset preset `typeset typeset-chat` (2g). Decisions: pro-d16, pro-d18, pro-d19, pro-d21, pro-n11, pro-n12, pro-n14, pro-n15; assumptions A1 to A6.

Sources read: the boards' chat zone (`Main.dc.html` lines 257 to 402, via `sc-main-board.md` section 3b), COMPONENTS.md rows for the title bar, user message, Thinking, tool groups, `ToolTickStrip`, φ notices and compaction, and the Phase 2 code (`web/src/ui/chat/*`, `web/src/ui/ChatPane.tsx`, `web/src/state/store.ts`, `web/src/protocol/domain.ts`) with the Phase 2 test inventory (`sc-web-chat.md` section 7).

**Contradiction rule** (as DESIGN.md): the board's rendered value wins over the handoff text; Phase 2 behaviour (data, ARIA, hooks, tests) wins over the board unless this file says otherwise. Each resolution is one line marked **Resolution**.

**File boundary (pro-n15) and no backend change (A6).** Implementation touches only `web/src/ui/**` and `web/src/lib/**`. Nothing in `web/src/protocol/**`, `web/src/state/**`, `web/src/server/**`. `web/src/ui/chat/virtual.ts` and `rows.ts` are unchanged in behaviour. A design element whose data the protocol or the store does not carry renders only when an optional prop is passed, or is omitted, never faked; each is listed under **Not wired** (section 5).

**Shadcn parts (Context, chat decision).** The chat blocks use four shadcn parts only, from `web/src/ui/components/` (2f): `Bubble` (the user message, the assistant text, queued and pending rows), `Trace` (the Thinking row, tool rows, tool groups, the working cues), `Marker` (cpd folds, compaction) and `Collapsible` (the root of every collapsible block). `Button` (`../components/button.tsx`) stays for the header toggles and the two transcript buttons (`Load earlier`, `Jump to bottom`). **Resolution:** the Phase 2 native `details`/`summary` blocks are replaced by these parts; the Phase 2 hooks they carried are kept on the part's root (section 4). The `Attachment` and `MessageScroller` parts are not used.

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

- State is local React state (`useState`), not persisted, not in the store. Owner: `ChatPane` (one call per mount), which passes it down (2.1, 1.1).
- The hook registers one `keydown` listener on `document` for its lifetime (added on mount, removed on unmount, no dependency churn: it uses functional `setState`). A key event toggles when `event.ctrlKey && !event.metaKey && !event.altKey && !event.shiftKey && !event.isComposing` and `event.key.toLowerCase()` is `"t"` (thinking) or `"o"` (tools); it then calls `event.preventDefault()`. Focus inside the composer textarea does not matter (the hints are global in the board).
- Unverified: a browser may reserve `Ctrl+T` / `Ctrl+O` on some platforms (Windows, Linux) and not deliver the event; the two header buttons are the always-available path. On macOS (`⌃` = Control) both reach the page.

### 0.3 Vertical rhythm (the virtualization-safe gap)

The board spaces rows 6px apart (`gap: 6px`). A margin on a row root would collapse through the measured `data-row-key` wrapper and be lost from its measured height. **Rule:** vertical space between rows is **padding inside the measured wrapper**, never margin, never `gap` on a parent that is outside it. `RowBody` (2.9) wraps every row body in `div[data-part="row-body"]` with `py-[3px]` (3 + 3 = the board's 6px). Horizontal padding `px-4` (16px, the board's) is on `div[data-part="virtual-window"]`, not on the measured wrapper.

---

## 1. ChatHeader (`web/src/ui/chat/ChatHeader.tsx`)

### 1.1 Props

```ts
import type { Runner, SessionRole } from "../../protocol/domain.ts";
import type { ThinkingMode, ToolsMode } from "./useChatToggles.ts";

export interface ChatHeaderProps {
  readonly sessionKey: string;                     // KEPT. `view.ref?.key ?? view.id`
  readonly title: string | null;                   // KEPT. `view.title`
  /** `view.ref.role`. Absent: no role in the subtitle. */
  readonly role?: SessionRole;
  /** `view.ref.runner`. Absent: no runner in the subtitle. */
  readonly runner?: Runner;
  /** The computed `busy` of 2.4.3 (`ChatPane` passes it, not `view.busy` alone). true: the live status shows. Absent or false: it does not. */
  readonly busy?: boolean;
  /** Thinking toggle. Rendered only when `onToggleThinking` is passed; `thinking` then defaults to "folded". */
  readonly thinking?: ThinkingMode;
  readonly onToggleThinking?: () => void;
  /** Tools toggle. Rendered only when `onToggleTools` is passed; `tools` then defaults to "grouped". */
  readonly tools?: ToolsMode;
  readonly onToggleTools?: () => void;
}
export function ChatHeader(props: ChatHeaderProps): JSX.Element;
```

`<ChatHeader sessionKey="k" title={null} />` (the Phase 2 call) renders without a toggle and without a status.

Source of each element: key `SessionView.ref.key` (fallback `SessionView.id`); title `SessionView.title`; role and runner `SessionView.ref` (`SessionRef.role`, `SessionRef.runner`); running status: the computed `busy` of 2.4.3. `ChatPane` passes `role={view.ref?.role}`, `runner={view.ref?.runner}`, `busy={busy}` (the computed value) and the four toggle props from `useChatToggles()`; with `exactOptionalPropertyTypes` an absent `ref` spreads nothing (`{...(view.ref === null ? {} : { role: view.ref.role, runner: view.ref.runner })}`).

### 1.2 Behaviour

`document.title` effect: **KEPT** exactly (`sessionKey` when `title === null`, else `` `${sessionKey} · ${title}` ``; restored on unmount; deps `[sessionKey, title]`).

### 1.3 DOM

```
header                                                    KEPT element. flex flex-wrap items-center gap-3 px-4 pt-1 pb-2
├─ div[data-part="heading"]                               flex-1 min-w-[260px]
│   ├─ span[data-part="title"]                            KEPT hook, only when title !== null.   block text-title font-semibold text-fg-strong      (15px, weight 600, #f3ead9, line-height 1.3)
│   └─ div[data-part="subtitle"]                          flex items-center gap-2 mt-[3px] text-[11.5px]
│       ├─ h1[data-part="key"]                            KEPT hook, text = sessionKey.   m-0 font-mono text-[11.5px] font-normal text-ramune-bright   (#a1c6ef)
│       ├─ span[aria-hidden="true"]  "·"                  text-fg-dim        (only when role or runner is passed)
│       ├─ span[data-part="role"]    {role}               text-fg-dim capitalize     (only when role passed; DOM text is the raw value "lead")
│       ├─ span[aria-hidden="true"]  "·"                  text-fg-dim        (only when both role and runner are passed)
│       ├─ span[data-part="runner"]  {runner}             text-fg-dim        (only when runner passed)
│       └─ span[data-part="run-status"][data-state="running"]   (only when busy === true)
│             flex items-center gap-1.5 text-mizu-bright   (#92d1ca)
│           ├─ span[aria-hidden="true"]  className={glyphClass.active}  (data-tone="active" data-glyph="active")
│           └─ span  "Running…"                           (U+2026)
└─ div[data-part="toggles"][role="group"][aria-label="Transcript display"]    flex gap-1.5 text-[11.5px]    (only when onToggleThinking or onToggleTools is passed)
    ├─ <Button variant="outline" type="button" data-action="toggle-thinking" aria-pressed aria-keyshortcuts="Control+T">   (only when onToggleThinking)
    │     className "h-[26px] gap-1.5 rounded-md border-input px-2 text-[11.5px] text-fg-strong"
    │   ├─ span  "Thinking: folded" | "Thinking: expanded"
    │   └─ kbd.font-mono  "⌃T"      text-meta text-muted-foreground
    └─ <Button … type="button" data-action="toggle-tools" aria-pressed aria-keyshortcuts="Control+O">   (only when onToggleTools)
          └─ span "Tools: grouped" | "Tools: expanded"   +  kbd.font-mono "⌃O"
```

Measurements and colours are the board's: h1/title 15px semibold `#f3ead9`; subtitle 11.5px, `margin-top: 3px`, `gap: 8px`; key `#a1c6ef`; role and runner `#87808c` (`text-fg-dim`, the header sits on `bg-background`, where `fg-dim` is allowed); live status `#92d1ca`; toggle button height 26px, padding `0 8px`, border `1px #35313b` (`border-input`), radius 6px, label `#f3ead9`, kbd `#aba3af`.

**Resolution:** the board puts the 15px title in the `h1` and the key in the sub-line; Phase 2 pins `h1[data-part="key"]` (text = the key) and `[data-part="title"]` (`ChatHeader.test` 1). The key therefore stays the `h1` (moved into the sub-line, styled as the sub-line key) and the 15px title is the `span[data-part="title"]` above it. With `title === null` there is no title line and the `h1` is the only text. **Resolution:** the board's `Running command…` names a command the store does not carry; the copy is `Running…` (no command), shown while `busy`.

### 1.4 Toggles: what each does, `aria-pressed`, the label rule

| Button | State hook | `aria-pressed` | Label (exact) | Effect |
|---|---|---|---|---|
| Thinking | `thinking: ThinkingMode` | `"true"` when `thinking === "expanded"`, else `"false"` | `Thinking: folded` / `Thinking: expanded` | sets the **default open state of every Thinking row** in the transcript (folded = closed, expanded = open), 2.4.2 |
| Tools | `tools: ToolsMode` | `"true"` when `tools === "expanded"`, else `"false"` | `Tools: grouped` / `Tools: expanded` | sets the **default open state of every Tool Group and every tool row** (grouped = closed, expanded = open: `Tools: expanded` expands the rows, and their output shows its 10-row preview), 3.2 and 3.4 |

- Click and `⌃T` / `⌃O` call `onToggleThinking` / `onToggleTools`. Enter and Space activate the native button.
- **Label rule (resolves the board's caveat 1, `Tools: grouped` beside an expanded group):** the label names the **global default**, never the live state of one group. A person opening one group or one Thinking pill by hand changes only that element (a local override); the label stays. A toggle press re-applies the new default to **all** elements and discards every local override (the elements remount, `key` = the mode, 2.4 and 3.4). The board's drawn example (label `grouped`, one group open) is therefore a legal state: a group opened by hand.
- `aria-pressed` and the changing label say the same thing twice; this is accepted (the board's labels are copy, the attribute is for assistive tech).

---

## 2. Transcript rows (`web/src/ui/chat/Transcript.tsx`)

### 2.1 Props

```ts
export function Transcript(props: {
  items: readonly TranscriptItem[];        // KEPT
  hasEarlier: boolean;                     // KEPT
  queued: readonly QueuedSubmission[];     // KEPT
  pending: readonly PendingSubmission[];   // KEPT
  onLoadEarlier(): void;                   // KEPT
  tailEpoch: number;                       // KEPT
  sizes?: SizeObserver;                    // KEPT
  /** The assistant text stamp `<key> · <time>`. Absent: the text shows no stamp. `ChatPane` passes `view.ref?.key ?? view.id`. */
  sessionKey?: string;
  /** The computed `busy` of 2.4.3. Absent or false: no working cue. Default false. */
  busy?: boolean;
  /** Default "folded". */
  thinking?: ThinkingMode;
  /** Default "grouped". */
  tools?: ToolsMode;
}): JSX.Element;
```

`ChatPane` calls `useChatToggles()` once and passes `thinking`/`tools` to both `ChatHeader` and `Transcript`. The `Item`, `ToolGroup` and `ToolRow` components stay `memo`; `thinking`, `tools` and `sessionKey` are plain props of them (strings), so a streaming delta that changes one item does not re-render the other rows (`Rows.test` stays green).

### 2.2 DOM outline (whole transcript)

Everything not named here is **KEPT** from Phase 2 (`sc-web-chat.md` 3.7):

```
div[data-part="transcript"]                                  KEPT (app.css)
├─ div[role="log"][aria-label="Transcript"][data-at-bottom]  KEPT (app.css)
│   ├─ button[type="button"][data-action="load-earlier"]  "Load earlier"      only if hasEarlier.  mx-auto my-2 block rounded-[10px] border border-border px-2.5 py-0.5 font-mono text-meta text-muted-foreground hover:bg-row-hover
│   ├─ div[data-part="virtual-space"]                         KEPT, inline style height + position: relative (unchanged)
│   │   └─ div[data-part="virtual-window"]                    KEPT, inline style translateY (unchanged); NEW className "px-4"
│   │       └─ div[data-row-key]                              KEPT, MeasuredRow, no className, no style, the only measured element
│   │           └─ div[data-part="row-body"]                  NEW, py-[3px]; the one wrapper of every row (RowBody)
│   │               └─ <row>   article[data-kind="user"] | article[data-kind="assistant"] | TraceCollapsible[data-kind="fold"] | TraceCollapsible[data-kind="compaction"] | TraceCollapsible[data-kind="tool-group"] | TraceCollapsible[data-kind="tool"] | article[data-kind="orphan-result"]
│   ├─ ul[aria-label="Queued messages"]                       KEPT, inside the log, after virtual-space; NEW className "flex flex-col gap-0 px-4 pb-2.5 list-none m-0 p-0"
│   │   ├─ li[data-queued]                                    2.6
│   │   └─ li[data-pending]                                   2.7
│   └─ div[role="status"][data-part="working-cue"]            NEW, last child of the log, only while busy (2.4.3)
└─ button[type="button"][data-action="jump-to-bottom"] "Jump to bottom"     KEPT; only when not pinned; sibling after the log.
      <Button variant="outline" size="sm"> className "rounded-full border-input bg-popover px-3 text-[12px] text-fg-strong"; position is app.css (absolute, bottom 0, centred), no position utility on it
```

A **lone** tool row (a `Row` of `kind: "item"` whose item is a tool) has no wrapper; its rail is its own (3.3).

### 2.3 User message

Component: `Item`, case `user` (in `Transcript.tsx`). The user message is a `Bubble` (2f) with the `muted` variant: a fill, no border.

```
article[data-kind="user"]                                   KEPT.  flex flex-col items-end gap-1
├─ Bubble[variant="muted"][align="end"]                      shadcn Bubble (2f); its own max width (80%) replaces the Phase 2 78%
│   └─ BubbleContent[data-part="bubble"]                     no border; text-body text-fg-strong; px-3 py-[9px]
│       ├─ div[data-part="text"][style="white-space: pre-wrap"]  KEPT inline style (the Phase 2 exception), text = item.text.   m-0 text-body text-fg-strong      (13px IBM Plex, #f3ead9)
│       └─ div[data-part="attachments"]  flex flex-wrap gap-2    only when item.attachments.length > 0
│           └─ span[data-attachment="image"|"text"]               KEPT hook, one per attachment, in order.   flex items-center gap-2 rounded-md border border-input py-1 pr-2 pl-1
│               ├─ span[data-part="thumb"][aria-hidden="true"]    grid h-6 w-[34px] place-items-center rounded-[3px] bg-edge text-muted-foreground      (34x24, #3a3640)
│               │     image with dataBase64: <img alt="" src={`data:${mimeType};base64,${dataBase64}`} className="h-6 w-[34px] rounded-[3px] object-cover" />
│               │     otherwise: lucide ImageIcon (image) or FileTextIcon (text), size 14, stroke muted-foreground (#aba3af)
│               └─ span[data-part="name"]                         font-mono text-[11.5px] text-fg-strong      text = a.kind === "image" ? a.mimeType : a.name   (KEPT text)
└─ span[data-part="meta"]                                    font-mono text-meta text-muted-foreground      (11px, #aba3af)
      text: `you · ${formatClock(item.at)}` when formatClock(item.at) !== null, else `you`
```

- `[data-attachment]` is the same element as before (Phase 2 test 1: `[data-attachment="image"]` text is `image/png`); its text is now the `name` span only, and the thumb has no text, so `textContent` is unchanged.
- The user message is right-aligned by `items-end` on the article (not by `float` or `margin-left: auto` on a measured box).
- **Resolution:** the board's chip shows a file name and a size (`ci-run-8812.png`, `184 KB`); an image attachment carries only `mimeType` (and optional `dataBase64`), so the chip shows the mime type and no size (Not wired).
- **Assumption:** the user message takes `muted` because the Context allows no bordered card per text block and the Bubble parts offer no fill-free user variant that keeps the right-hand alignment; flagged for the Lead.

### 2.4 Assistant text, Thinking, working marker

Component: `Item`, case `assistant`. The article's children keep the order of `item.blocks`: each `thinking` block is a Thinking row (2.4.2), each `text` block is a text Bubble. There is no card and no border around any text block.

```
article[data-kind="assistant"]                              KEPT.  flex flex-col gap-1.5
├─ (per thinking block, in order)  <ThinkingRow key={thinking} text={b.text} time={formatClock(item.at)} defaultOpen={thinking === "expanded"} />
├─ (per text block, in order)  Bubble[variant="ghost"][align="start"][data-part="card"]    shadcn Bubble (2f); no fill, no border, no padding; the Phase 2 hook name `card` is KEPT (DESIGN.md 1.1) and names no card here
│   └─ BubbleContent
│       ├─ <Markdown text={b.text} />                       root element is div.typeset.typeset-chat (2.4.1)
│       └─ span[data-part="stamp"]  (ONLY in the LAST text block of the article, and only when `sessionKey` is passed)
│             self-end font-mono text-meta text-muted-foreground      (11px, #aba3af, bottom-right)
│             text: `${sessionKey} · ${formatClock(item.at)}` when the clock is non-null, else `${sessionKey}`
└─ TraceLine[status="running"][role="status"][data-part="working"]   KEPT hook and role; only while item.streaming; always the LAST child of the article.
      (shadcn Trace, 2f)
    ├─ TraceIcon: span[aria-hidden="true"]  className={glyphClass.active}    (data-tone="active" data-glyph="active")
    └─ TraceTitle  "Working…"                                    (U+2026; the span's textContent is exactly "Working…")
```

An assistant item with only thinking blocks has no text bubble and no stamp. A `streaming` item whose last block is a thinking block still shows the working marker.

#### 2.4.1 `Markdown.tsx` (`web/src/ui/chat/Markdown.tsx`)

`splitMarkdown` and `Segment` are unchanged; `react-markdown` plus `remark-gfm` only. `Markdown` now returns `null` for no segments (KEPT) and otherwise `<div className="typeset typeset-chat">{segments}</div>` instead of a fragment (DESIGN.md 2g). `Markdown.test` 1 to 5 are unaffected (they test `splitMarkdown` and the segment memo, not a wrapper). Inline code colour `#92d1ca` 12px, list indent 20px, list gap 2px and block gap 6px come from the preset (DESIGN.md 2g), not from this file.

#### 2.4.2 Thinking row (`web/src/ui/chat/Transcript.tsx`, not exported; used by `Item` only)

```ts
function ThinkingRow(props: { text: string; time: string | null; defaultOpen: boolean }): JSX.Element;
```

The row is a shadcn `TraceCollapsible` (2f). State: the part's own (`open` from `defaultOpen`). The parent passes `key={thinking}` so a toggle press remounts the row and re-applies the default (1.4).

```
TraceCollapsible[data-kind="thinking"][data-state="open"|"closed"]     KEPT hook; closed in "folded".   div
├─ TraceCollapsibleTrigger                                             the row's one line; no pill, no border
│   ├─ TraceIcon  "▸" (closed) / "▾" (open)  aria-hidden              font-mono text-[10px]
│   ├─ TraceTitle  "Thinking"                                         text-muted-foreground
│   └─ TraceDescription  {time}                                       font-mono text-fg-dim   (only when time !== null)
└─ TraceCollapsibleContent                                             the one left rail (the part's border-l)
     └─ div[data-part="thinking-body"]  {text}                         whitespace-pre-wrap; text-body text-muted-foreground; no clipping
```

- The body is the raw `b.text` (not Markdown), as in Phase 2. The rail is the part's own left border (`trace.tsx`), so the row adds no second rail.
- **Resolution:** the board's meta is `41s · 13:58` (a duration and a time); the protocol carries no thinking duration, so the meta is the clock only (Not wired: duration).
- Keyboard: the trigger is a native `button`; Enter and Space toggle. `⌃T` toggles all.
- `Phase 2 change:` the summary was a bordered `summary` pill (`details`); it is now a Trace line (no border, no pill, no fixed height). `Transcript.test` 2 is updated (2.10).

### 2.4.3 Working cue (`busy`, `Transcript`)

**Computed busy.** `ChatPane` computes `busy = view.busy || view.ref?.state === "working" || view.pending.some((p) => p.state === "sending")` and passes this one value to `ChatHeader` (1.1), `Transcript` (2.1) and `Composer` (`DESIGN-compose.md` section 0). It replaces `view.busy` in those three places. `TierSelector` keeps `view.busy` (`DESIGN-compose.md` section 0).

**Props.** `busy?: boolean` on `Transcript`, default `false`.

**Rule.** When `busy` is true and the last item of `items` is not an assistant item with `streaming === true`, the log renders a working cue as its last child, after the Queued messages list (2.2). When the last item is a streaming assistant item, the 2.4 marker shows instead and no cue renders.

```
TraceLine[status="running"][role="status"][data-part="working-cue"]     NEW; shadcn Trace (2f); no fixed height
├─ TraceIcon: span[aria-hidden="true"]  className={glyphClass.active}     (data-tone="active" data-glyph="active")
└─ TraceTitle: text `Using ${tool.name}…` when the last item is a tool item with status "running"; else "Thinking…"
```

- The text uses U+2026. The tool name is `ToolItem.name`, the raw name the protocol carries.
- The cue is not a row: it has no `data-row-key`, is not in `groupRows`, is not virtualised and is not measured by `MeasuredRow`. It sits in normal flow after the rows, like the Queued list.
- **Resolution:** the board's cue names a command and an elapsed time; the protocol carries neither, so the cue is `Using <tool name>…` or `Thinking…` (Not wired: elapsed time).

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

`li[data-queued={clientSubmissionId}]` **KEPT**, in `ul[aria-label="Queued messages"]` (KEPT). The same shape as 2.7; a queued message is a `muted` Bubble at 70% opacity, with no border:

```
li[data-queued]                                             flex flex-col items-end gap-1 py-[3px]
├─ Bubble[variant="muted"][align="end"]  opacity-70
│   └─ BubbleContent[data-part="bubble"]
│       └─ div[data-part="text"]  {q.text}                  whitespace-pre-wrap text-body text-fg-strong
└─ span[data-part="queued-status"]  "queued"                font-mono text-meta text-muted-foreground
```

### 2.7 Pending and failed user rows, and the `sent` defect

Source: `PendingSubmission.state` is `"sending" | "sent" | "failed"` (`web/src/state/store.ts`); `error?: WireError` with `message`.

**Known defect (Phase 2):** the row text is `` `${p.text} ` `` + `p.state === "sending" ? "sending…" : "failed: " + (p.error?.message ?? "unknown error")`, so a row in state `sent` (server acked, the echo of the user item not yet arrived; `session.submitAck` sets it, `echoed()` or a tail window removes it) is shown as `failed: unknown error`. **Fix:** the state is rendered exhaustively; only `failed` shows failure text.

Exact DOM for each state (the `li` is a child of `ul[aria-label="Queued messages"]`, `key = clientSubmissionId`, hook `data-pending` KEPT). Every pending bubble is a `muted` Bubble with no border; a failed one takes the blocked fill, not a border:

```
li[data-pending="sending"]            flex flex-col items-end gap-1 py-[3px]
├─ Bubble[variant="muted"][align="end"]  opacity-70
│   └─ BubbleContent[data-part="bubble"]  └─ div[data-part="text"]  {p.text}     whitespace-pre-wrap text-body text-fg-strong
└─ span[data-part="pending-status"][data-state="sending"]  "sending…"     font-mono text-meta text-muted-foreground      (U+2026)

li[data-pending="sent"]               (identical shape to "sending")
├─ Bubble[variant="muted"][align="end"]  opacity-70
│   └─ BubbleContent[data-part="bubble"]  └─ div[data-part="text"]  {p.text}
└─ span[data-part="pending-status"][data-state="sent"]  "sent"            font-mono text-meta text-muted-foreground
      NO role="alert", NO glyph, NO text containing "failed" or "unknown error"

li[data-pending="failed"]
├─ Bubble[variant="destructive"][align="end"]                      shadcn Bubble (2f): bg-destructive/10, text-destructive
│   └─ BubbleContent[data-part="bubble"]  └─ div[data-part="text"]  {p.text}
└─ span[data-part="pending-status"][data-state="failed"][role="alert"]    flex items-center gap-1.5 font-mono text-meta text-status-blocked
    ├─ span[aria-hidden="true"]  className={glyphClass.blocked}           (data-tone="blocked" data-glyph="blocked")
    └─ text node  `failed: ${p.error?.message ?? "unknown error"}`
```

**Resolution:** the failed bubble uses the `destructive` Bubble variant (a 10% fill of the blocked colour and its text colour), which replaces the Phase 2 red border and `bg-status-blocked/10`.

Test-facing facts: `sent` `textContent` is `` `${p.text}sent` `` (bubble text, then the status); it contains neither `failed` nor `unknown error`; the status has `data-state="sent"`; there is no `[role="alert"]` in the row. For `sending` the `li` text contains `sending…` and for `failed` with `error.message = "boom"` it contains `failed: boom` (Phase 2 test 6, still green); `failed` with no `error` contains `failed: unknown error`. Pending rows are **not** virtualised (they sit after `virtual-space`, as in Phase 2) and are not in `groupRows`. A new pending row still re-pins the viewport (Phase 2 behaviour, test 15).

### 2.8 Load earlier and Jump to bottom

Both KEPT hooks and copy (`Load earlier`, `Jump to bottom`); classes in 2.2. **Resolution:** the board's `▲ 3,912 earlier messages` needs a count the window does not carry; the Phase 2 copy `Load earlier` stays.

### 2.8.1 Pinning to the bottom

The log is pinned to the bottom only while the user has not scrolled up. State: `pinned` (initially true) and `lastScrollTop` (a ref: the `scrollTop` of the previous scroll event).

**Scroll event (`onScroll`).**
1. If `lastScrollTop - scrollTop > 1` (the `scrollTop` is more than 1px lower than the previous event's, that is, the user scrolled up), `pinned = false` at once, even inside the near-bottom distance.
2. Otherwise `pinned = isNearBottom(...)` (`virtual.ts`, 48px).
3. `lastScrollTop = scrollTop`, on every event.

**Bottom write (layout effect, after each commit).** `log.scrollTop = log.scrollHeight` is written only when `pinned` is true AND at least one of these holds since the previous commit:
- the layout total height changed;
- `tailEpoch` changed;
- `pending` grew;
- `busy` turned on (false to true);
- `pinned` turned on (the transcript just became pinned, for example by the Jump to bottom button).

An ordinary re-render, with none of these, never writes `scrollTop`. The bottom write's own scroll event moves `scrollTop` down, so step 1 does not unpin. **Evidence:** `sc-scroll.md`: the write on every commit while pinned undid a user's scroll-up; this rule replaces that write.

### 2.9 Virtualization contract (what every new wrapper keeps)

1. `MeasuredRow` and `div[data-row-key={row.key}]` are unchanged: no `className`, no `style`, no `data-index`, the same ref and `ResizeObserver` wiring. The measured element is that div (its border box).
2. Every row body is in **normal flow**: no `position: absolute` or `fixed`, no CSS `height`/`min-height`/`max-height`, no `transform` on a row element. (The window's `translateY` and the space's `height` + `position: relative` are Phase 2 inline styles, unchanged.) No block of a row sets `overflow-hidden`; the Trace part's own animation overflow is the part's.
3. Row keys come from `groupRows` unchanged (`item.id`, `group:<first tool id>`); `rows.ts` and `virtual.ts` are not edited.
4. The new wrapper `div[data-part="row-body"]` is the **only** child of `div[data-row-key]` and is rendered for every row, so `[data-row-key]`'s `firstElementChild` is stable across re-renders (`Rows.test` 2). Vertical spacing is its padding (0.3), never a margin.
5. A row changes height only by content (a Collapsible opening, a Show-more click, a Markdown stream); the observer reports it, as in Phase 2. Toggling Thinking or Tools changes heights and goes through the same `onHeight` compensation.
6. The `data-at-bottom`, `jump-to-bottom` and prepend-anchor logic are not touched; the pin and bottom-write rule is 2.8.1.

### 2.10 Phase 2 tests this section changes

| Test | Change |
|---|---|
| `Transcript.test` 1 | none: `article[data-kind="user"]` text still contains `hello  there` (the `[style white-space: pre-wrap]` div is `[data-part="text"]`), `[data-attachment="image"]` text `image/png`, `article[data-kind="assistant"] strong` text `bold answer` |
| `Transcript.test` 2 | **updated:** `[data-kind="thinking"][data-state="closed"]`; the trigger text is `Thinking` (no time when `at` is absent); `[data-part="thinking-body"]` text contains `pondering` |
| `Transcript.test` 3 | none (one `[role="status"][data-part="working"]`, text `Working…`, last child of the first assistant article) |
| `Transcript.test` 4, 7 to 17 | none |
| `Transcript.test` 5 | **updated:** `[data-kind="compaction"][data-state="closed"]`; `[data-part="label"]` text is `Context compacted · from 1.2k tokens` for `tokensBefore: 1234`; the summary Markdown text is in the content |
| `Transcript.test` 6, 18 | none (`li[data-queued]`, `li[data-pending="sending"]` contains `sending…`, `li[data-pending="failed"]` contains `failed: boom`, list inside the log after `virtual-space`) |
| `Transcript.test` 8 | none (`[data-row-key]` keys; the `u1` row contains `article[data-kind="user"]`) |
| `Folds.test` 1 to 3, `ToolRow.test` 1 to 5 | **updated** as in 3.1 to 3.4: `data-state` for `open`; every tool closed by default, `edit` included; the group's and the nested rows' state follow `mode` |
| new | `sent` row (2.7); `sent` row never contains `failed`; failed row has `[role="alert"]` and the `destructive` Bubble; `Item` stamp text; user `meta` text; `thinking="expanded"` renders the row with `data-state="open"` |

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
  /** The Tools toggle (1.4). Sets the row's default open state: "grouped" = closed, "expanded" = open. Every tool row takes it, grouped or not. */
  mode: ToolsMode;
}) => JSX.Element>;
export function toolName(tool: ToolItem): string;      // NEW export: name without a leading "mcp__"
export function toolDetail(tool: ToolItem): string;    // NEW export: detailOf(tool.name, tool.args)
export const OrphanRow: MemoExoticComponent<(props: { item: OrphanResultItem }) => JSX.Element>;   // KEPT
```

`detailOf`, `statusOf`, `splitLines`, `diffKind`, `Body`, `Output`, `PlainOutput`, `TOOL_PREVIEW_LINES` (10) and the `mcp__` prefix rule are **KEPT** in copy (`Running`, `Failed: <reason>`, `Done in 1.5s (3 lines)`, `Done (+1 -1)`, `Show N more lines`); `formatDuration` moves to `format.ts`. The Phase 2 first-line rule for a multi-line bash command and the 160-character cut of a tool detail are **removed** (no truncation, no character cut): `toolDetail` is the whole target in every state, and its lines are kept. The failure reason is not cut either.

Glyph by status (the status is `ToolItem.status`): `running` → `glyphClass.active` / tone `active`; `done` → `glyphClass.done` / tone `done`; `failed` → `glyphClass.blocked` / tone `blocked` (the board's `g-err` is the triangle `glyph-blocked`).

```
TraceCollapsible[data-kind="tool"][data-tool={tool.name}][data-status={tool.status}][data-state="open"|"closed"]    KEPT hooks; open from `mode` (3.4 rule)     div
├─ TraceCollapsibleTrigger[data-part="summary"]
│   └─ TraceLine[status]         status "running" for a running tool, "error" for a failed one, none for done (shadcn Trace)
│       ├─ TraceIcon: span[data-part="glyph"][aria-hidden="true"][data-tone][data-glyph]   className={glyphClass[g]}
│       ├─ TraceTitle: span[data-part="name"]  {toolName(tool)}     KEPT. text-muted-foreground      (verb, #aba3af)
│       └─ TraceDescription
│            ├─ span[data-part="detail"]  {toolDetail(tool)}       KEPT. font-mono text-foreground; the whole target, wrapped      (#d8d2cc)
│            ├─ span[data-part="delta"]  (edit with tool.diff only)  font-mono
│            │   ├─ span[data-part="added"]    `+${A}`               text-status-done       (#8da96e)
│            │   └─ span[data-part="removed"]  `−${R}`  (U+2212)     text-status-blocked    (#e17f6e)    A and R as in statusOf
│            └─ span[data-part="status"]  {statusOf(tool)}          KEPT. font-mono text-meta text-fg-dim      (11px, #87808c)
└─ TraceCollapsibleContent                                          the tool's one rail
     └─ output (3.6)  or nothing when result is undefined
```

- **Resolution:** the board's row shows glyph, verb, target, time; the Phase 2 text `Done in 1.5s (3 lines)` is in the same slot as the time and is kept for its tests.
- `edit` no longer starts open (Context: edits collapsed by default). Every tool row starts closed when the Tools mode is `grouped`.
- `OrphanRow` DOM keeps `article[data-kind="orphan-result"]` > `p` (text `Result of {name} (call not loaded)`, `text-[12px] text-muted-foreground`) and the `pre[data-part="output"]` of 3.6.

### 3.3 Lone tool row

`RowBody` renders a `Row` of `kind: "item"` with a tool item as `<ToolRow tool={item} mode={mode} />`. There is no wrapper: the Phase 2 `div[data-part="tool-rail"]` is removed, and the row's one rail is its own (3.2).

### 3.4 Tool Group (`web/src/ui/chat/ToolGroup.tsx`, new)

Grouping **already exists**: `groupRows` (`web/src/ui/chat/rows.ts`) turns a run of two or more consecutive `kind === "tool"` items into `{ kind: "tools", key: "group:<first tool id>", tools }`; a lone tool is an `item` row. `state/transcript.ts` produces the items and does no grouping. **Reuse it unchanged**; `Transcript` does not group at render time and `rows.ts` is not edited. The old inline `ToolsRow` in `Transcript.tsx` is replaced by `ToolGroup`.

```ts
import type { ToolItem } from "../../protocol/domain.ts";
import type { ToolsMode } from "./useChatToggles.ts";

export interface ToolGroupProps {
  /** Two or more consecutive tool items, from a `Row` of kind "tools". */
  readonly tools: readonly ToolItem[];
  /** The Tools toggle: "grouped" = closed, "expanded" = open. Also the default of every tool row inside. */
  readonly mode: ToolsMode;
}
export const ToolGroup: MemoExoticComponent<(props: ToolGroupProps) => JSX.Element>;
```

`memo` comparator: same `mode` and same length and element-wise identical `tools` (`sameTools` extended with `a.mode === b.mode`). `RowBody` renders `<ToolGroup key={mode} tools={row.tools} mode={mode} />` so a toggle press remounts every group and every tool row and discards local overrides (1.4).

State: `const [open, setOpen] = useState(props.mode === "expanded")`, passed to the part as `open={open} onOpenChange={setOpen}`.

Derived (pure, in the component file): `failed = tools.filter(t => t.status === "failed").length`; `running = tools.some(t => t.status === "running")`; `total = sum of result.durationMs over tools that have one` (`hasTime` = at least one has it); aggregate status `agg = failed > 0 ? "failed" : running ? "running" : "done"` (**failed wins over running**).

```
TraceCollapsible[data-kind="tool-group"][data-tool-count={tools.length}][data-status={agg}][data-state="open"|"closed"]   KEPT hook; NEW data-tool-count, data-status.   div; no rail, no indent
├─ TraceCollapsibleTrigger[data-part="group-header"]
│   └─ TraceLine[status]      "error" when agg is failed, "running" when running, none when done
│       ├─ span[data-part="caret"][aria-hidden="true"]  "▸" closed / "▾" open      font-mono text-[10px] text-muted-foreground
│       ├─ TraceIcon: span[data-part="glyph"][aria-hidden="true"][data-tone][data-glyph]   agg "failed" → blocked, "running" → active, "done" → done
│       ├─ TraceTitle: span[data-part="name"]  {toolName(tools[0])}      text-fg-strong      (#f3ead9)
│       └─ TraceDescription
│            ├─ span[data-part="summary"]  text below          text-foreground; wraps      (#d8d2cc)
│            ├─ <ToolTickStrip statuses={tools.map(t => t.status)} />     (3.5)
│            └─ span[data-part="totals"]  text below           font-mono text-meta text-muted-foreground      (11px, #aba3af)
│       └─ span[data-part="group-count"].sr-only  `${tools.length} tool calls`
└─ TraceCollapsibleContent
     └─ div[data-part="group-rows"]    flex flex-col gap-0.5      (no rail: each row has its own)
         └─ <ToolRow key={t.id} tool={t} mode={mode} />  per tool
```

- `[data-part="summary"]` text: `` `${toolDetail(tools[0])} + ${n} more ${n === 1 ? "step" : "steps"}` `` with `n = tools.length - 1` (board: `stream.test.ts + 11 more steps`; the detail is the full target, not its basename). It wraps; it is not cut.
- `[data-part="totals"]` text: `running` (when `running`) else `done`; then ` in ${formatDuration(total)}` when `hasTime` and not `running`; then ` · ${failed} failed` when `failed > 0`. Examples: `done in 6.2s · 1 failed`, `done`, `running · 1 failed`. **Resolution:** the board shows `6.21s` (two decimals); the single `formatDuration` of Phase 2 gives one decimal (`6.2s`) and is kept.
- Closed (`tools="grouped"`): only the header shows, so a failed call is visible as a red tick, the `failed` count and the blocked glyph. **Resolution:** COMPONENTS says failed calls "break out of the fold"; that is not done (the toggle is a single global rule, Not wired).
- Keyboard: the trigger is a native `button`. Tab order: the header, then each opened row's trigger and its expand control.
- `Phase 2 change:` the group was `details[open]` with `summary` text `N tool calls`, open by default. Now it follows `mode` (closed by default), and `N tool calls` is the `sr-only` `[data-part="group-count"]`. `ToolRow.test` 5 is updated: with `mode` `grouped` the group is closed; with `mode="expanded"` it is open and every nested row is open; the three nested `[data-kind="tool"]` exist in the DOM either way; `[data-part="group-count"]` text is `3 tool calls`; a single item renders no `tool-group`.

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

Both are the `Output` of `ToolRow`, inside its `TraceCollapsibleContent` (after the trigger). Hooks **KEPT**: `pre[data-part="output"]`, `[data-diff="add"|"remove"|"context"]`, `button[data-action="expand-output"]`; 10-line preview, `Show N more lines`, the expand-once behaviour.

```
(plain output, every tool except edit-with-diff)
pre[data-part="output"]           mb-0.5 font-mono text-[11.5px] leading-[1.5] whitespace-pre-wrap text-muted-foreground     (no fill, no border, no indent; 11.5px / 1.5, #aba3af)
└─ span × shown lines (leading "\n" text on lines after the first: unchanged)

(edit with tool.diff)
div[data-part="diff"]             mb-0.5 font-mono text-[11.5px] leading-[1.55]      (no fill, no border, no indent, no overflow-hidden)
└─ div[data-diff={kind}] × shown lines         block whitespace-pre-wrap text-foreground + kind class
      add      bg-status-done/[0.11]      (rgba(141,169,110,.11))
      remove   bg-status-blocked/10       (rgba(225,127,110,.10))
      context  text-muted-foreground      (hunk headers such as "@@ -1 +1 @@" and unchanged lines)

button[type="button"][data-action="expand-output"]  "Show {rest} more lines"      py-0.5 font-mono text-meta text-fg-strong      (after the block; #f3ead9, 11px)
```

- **Preview: 10 rows.** A row is one line of the tool's output as the protocol gives it (a logical line); the preview shows the first 10 and the button shows the rest. A wrapped line is not counted as two rows. **Assumption:** the Context's "10 rows" is read as 10 logical lines, the Phase 2 count.
- **Diffs** keep every hunk and every line. Their lines wrap (`whitespace-pre-wrap`) and are not clipped.
- **Edits** are closed by default (3.2); when opened they show the same 10-line preview with the expand control.
- For `add` and `remove` lines the first character is wrapped: `<span data-part="sign" className="text-status-done">+</span>` (add) or `<span data-part="sign" className="text-status-blocked">-</span>` (remove) followed by the rest of the line, so `textContent` of the `[data-diff]` element is the unchanged diff line (`+new`, `-old`; Phase 2 test 4 pins it). Lines starting `+++` or `---` are `context` (KEPT `diffKind`) and have no sign span. **Resolution:** the board's removed marker is `−` (U+2212); the line text must stay `-old`, so the ASCII hyphen is coloured instead.
- The light-mode `app.css` rules for `[data-diff="add"]` and `[data-diff="remove"]` are removed (DESIGN.md 2a); the colours above replace them.
- A failed `bash` is closed by default like every tool row; its output shows when the row is opened, in the same `pre`.

---

## 4. KEPT Phase 2 hooks and the test that pins each

| Hook (KEPT) | Where | Pinned by |
|---|---|---|
| `header h1[data-part="key"]` text = key; `[data-part="title"]` only when `title !== null`; `document.title` rules and restore | ChatHeader | `ChatHeader.test` 1, 2, 3 |
| `[data-part="transcript"] > [role="log"][aria-label="Transcript"]`, `data-at-bottom` | Transcript | `Transcript.test` 1, 9, 10, 11, 12, 13 |
| `div[data-row-key]` (keys `u1`, `a1`, `group:tool:t1`, `c1`, `tool:t3`), one per mounted row, at most 60 for 1000 items | Transcript | `Transcript.test` 7, 8; `Rows.test` 2 |
| `button[data-action="load-earlier"]` text `Load earlier`; `button[data-action="jump-to-bottom"]` text `Jump to bottom` (parent `[data-part="transcript"]`) | Transcript | `Transcript.test` 4, 11 |
| `article[data-kind="user"]` (text, whitespace kept), `[data-attachment="image"]` text `image/png` | user message | `Transcript.test` 1 |
| `article[data-kind="assistant"]`, Markdown `strong` | assistant text | `Transcript.test` 1; `Rows.test` 1 |
| `[data-kind="thinking"]`, closed by default (`data-state`) | Thinking row | `Transcript.test` 2 (updated, 2.10) |
| `[role="status"][data-part="working"]` text `Working…`, last child of the assistant article | working marker | `Transcript.test` 3 |
| `[data-kind="compaction"]`, closed by default (`data-state`) | compaction | `Transcript.test` 5 (updated) |
| `ul[aria-label="Queued messages"]` inside the log after `virtual-space`; `li[data-queued]`; `li[data-pending]` with `sending…` and `failed: boom` | queued and pending | `Transcript.test` 6, 18 |
| `[data-kind="fold"][data-fold]`, `data-state="open"` only for `cpd-plan-summary` | Marker row | `Folds.test` 1, 2, 3 (updated, 3.1) |
| `[data-kind="tool"][data-tool][data-status]`, `data-state="closed"` for every tool by default (edit included); `[data-part="name"]`, `[data-part="detail"]`, `[data-part="status"]` texts | ToolRow | `ToolRow.test` 1, 2 (updated, 3.2) |
| `pre[data-part="output"]` 10 lines; `button[data-action="expand-output"]` `Show N more lines` | output | `ToolRow.test` 3 |
| `[data-diff="add"\|"remove"\|"context"]` texts and the status `Done in 1.5s (+1 -1)` | diff | `ToolRow.test` 4 |
| `[data-kind="tool-group"]`, one per run of two or more tools, none for a single item | Tool Group | `ToolRow.test` 5 (updated, 3.4); `Rows.test` 2 (`[data-row-key^="group:"]`) |
| `groupRows` keys, `rows.ts` unchanged | grouping | `rows.test` 1, 2, 3 |
| `virtual.ts` unchanged | virtualization | `virtual.test` 1 to 7 |
| `article[data-kind="orphan-result"]`, text `Result of … (call not loaded)` | OrphanRow | no Phase 2 test pins it (hook kept by this contract) |
| `splitMarkdown` and the per-segment memo | Markdown | `Markdown.test` 1 to 5 |
| `Footer` (`footer[aria-label="Session footer"]`, `[data-stat]`) | not changed here | `Footer.test` 1, 2 |

**Retired hooks** (no longer in the DOM): `details` and `summary` in chat blocks (every chat block root is a Collapsible `div`, and its trigger is a native `button`); `div[data-part="tool-rail"]`; the Thinking pill's `[data-part="caret"]` (the Trace icon replaces it). `summary[data-part="marker"]` is now `[data-part="marker"]` on the Trigger. Each test that pinned one is listed as updated in 2.10 and 3.x.

`web/src/types-unique.test.ts` is unaffected: no interface named there is redeclared (`ChatHeaderProps`, `ToolGroupProps`, `ChatToggles` are new names; `ThinkingRow` is not exported).

**Tests to add** (new files, written from this document): `format.test.ts` (0.1), `useChatToggles.test.tsx` (initial values, both toggles, `Ctrl+T`/`Ctrl+O` with and without meta/alt/shift, listener removed on unmount), `ChatHeader.dango.test.tsx` (1.3 and 1.4), `ToolGroup.test.tsx` (3.4), `ToolTickStrip.test.tsx` (3.5), plus the additions listed in 2.10. All tests mock nothing but `react-markdown` where Phase 2 already does; none uses a real timer.

---

## 5. Not wired

Data absent from `web/src/protocol/**` and the store; each element is omitted (or renders only from an optional prop), never faked.

- Thinking duration (`41s · 13:58`): only the clock `HH:MM` of `item.at` is shown.
- Tool Group time (`13:59`) and per-row time as a separate field: `ToolItem` has no timestamp; the duration is inside the Phase 2 status text.
- Attachment file name and size for images (`ci-run-8812.png`, `184 KB`): image attachments carry `mimeType` and optional `dataBase64`.
- `▲ 3,912 earlier messages`: no total count (the copy is `Load earlier`).
- `Running command…` and the running command line, `12s` elapsed: only `busy` and `streaming` exist.
- Compaction after-size (`412k → 38k`) and the optional Checkpoint.
- Failed tool calls breaking out of a collapsed group; per-group expand state memory across toggles.
- Retry, Edit or dismiss of a failed pending row (`dismissPending` exists in the store but `Transcript` has no prop for it).
- Image count on a pending row (`PendingSubmission.imageCount` is not drawn).
- Persisted Thinking and Tools modes; per-session modes.
- The TUI's diff trimming to one context line around each change and its `⋮` hunk gap marks: every diff line is shown (3.6).
- The `Attachment` and `MessageScroller` parts (the chat uses `Bubble`, `Trace`, `Marker` and `Collapsible` only, see the shadcn note at the top).
- A copy-to-clipboard button on the assistant text; hover timestamps; message reactions.


## Amendments pro-d48..pro-d61 (consolidate-docs, skeleton)

TBD.
