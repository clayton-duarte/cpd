# Dango v3 chat transcript: design contract (ChatHeader, transcript rows, folds, tools)

Scope: Plan `webui-phase3-good`, Workflow `ui`, package `web/`. This file is the contract for the chat surface: `ChatHeader`, the user bubble, the assistant card with typeset Markdown, the cpd Marker rows, the compaction divider, the Thinking pill, Tool rows and Tool Groups, the pending, failed and queued rows. Tests are written from this file alone, the implementation from the tests and this file.

It builds on `web/src/ui/DESIGN.md` (called **DESIGN.md**). Reused by reference, never restated: the hook names (DESIGN.md 1.1), the styling rules (1.2: Tailwind utilities, tokens not hex, `cn`, `data-tone`/`data-glyph` from `dango-status.ts`), the tokens (2b), the glyph classes (2c, `glyph-active`, `glyph-done`, `glyph-blocked`, ...), `glyphClass` / `toneClasses` (2d), the `Button` part (2f) and the typeset preset `typeset typeset-chat` (2g). Decisions: pro-d16, pro-d18, pro-d19, pro-d21, pro-n11, pro-n12, pro-n14, pro-n15; assumptions A1 to A6.

Sources read: the boards' chat zone (`Main.dc.html` lines 257 to 402, via `sc-main-board.md` section 3b), COMPONENTS.md rows for the title bar, user message, Thinking, tool groups, `ToolTickStrip`, φ notices and compaction, and the Phase 2 code (`web/src/ui/chat/*`, `web/src/ui/ChatPane.tsx`, `web/src/state/store.ts`, `web/src/protocol/domain.ts`) with the Phase 2 test inventory (`sc-web-chat.md` section 7).

**Contradiction rule** (as DESIGN.md): the board's rendered value wins over the handoff text; Phase 2 behaviour (data, ARIA, hooks, tests) wins over the board unless this file says otherwise. Each resolution is one line marked **Resolution**.

**File boundary (pro-n15) and no backend change (A6).** Implementation touches only `web/src/ui/**` and `web/src/lib/**`. Nothing in `web/src/protocol/**`, `web/src/state/**`, `web/src/server/**`. `web/src/ui/chat/virtual.ts` and `rows.ts` are unchanged in behaviour. A design element whose data the protocol or the store does not carry renders only when an optional prop is passed, or is omitted, never faked; each is listed under **Not wired** (section 5).

**Shadcn parts.** `Button` (`../components/button.tsx`) is used for the header toggles and the two transcript buttons. The registry parts `Bubble`, `Marker`, `Attachment`, `Trace` and `MessageScroller` of DESIGN.md 2f are **not** imported by any chat component. **Resolution:** the Phase 2 hooks that tests pin are native elements (`details`/`summary`, `article[data-kind]`, `div[data-row-key]`), which those parts do not produce, and their DOM is not pinned by DESIGN.md; the board's classes are applied to plain elements instead. The virtualised `Transcript` is not replaced by `MessageScroller` (DESIGN.md 2f).

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
  /** `view.busy`. true: the live status shows. Absent or false: it does not. */
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

Source of each element: key `SessionView.ref.key` (fallback `SessionView.id`); title `SessionView.title`; role and runner `SessionView.ref` (`SessionRef.role`, `SessionRef.runner`); running status `SessionView.busy`. `ChatPane` passes `role={view.ref?.role}`, `runner={view.ref?.runner}`, `busy={view.busy}` and the four toggle props from `useChatToggles()`; with `exactOptionalPropertyTypes` an absent `ref` spreads nothing (`{...(view.ref === null ? {} : { role: view.ref.role, runner: view.ref.runner })}`).

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
| Thinking | `thinking: ThinkingMode` | `"true"` when `thinking === "expanded"`, else `"false"` | `Thinking: folded` / `Thinking: expanded` | sets the **default open state of every Thinking pill** in the transcript (folded = closed, expanded = open), 2.4 |
| Tools | `tools: ToolsMode` | `"true"` when `tools === "expanded"`, else `"false"` | `Tools: grouped` / `Tools: expanded` | sets the **default open state of every Tool Group** (grouped = collapsed to its header, expanded = open), 3.4 |

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
  /** The assistant card stamp `<key> · <time>`. Absent: the card shows no stamp. `ChatPane` passes `view.ref?.key ?? view.id`. */
  sessionKey?: string;
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
│   │               └─ <row>   article[data-kind="user"] | article[data-kind="assistant"] | details[data-kind="fold"] | details[data-kind="compaction"] | details[data-kind="tool-group"] | div[data-part="tool-rail"] > details[data-kind="tool"] | article[data-kind="orphan-result"]
│   └─ ul[aria-label="Queued messages"]                       KEPT, inside the log, after virtual-space; NEW className "flex flex-col gap-0 px-4 pb-2.5 list-none m-0 p-0"
│       ├─ li[data-queued]                                    2.6
│       └─ li[data-pending]                                   2.7
└─ button[type="button"][data-action="jump-to-bottom"] "Jump to bottom"     KEPT; only when not pinned; sibling after the log.
      <Button variant="outline" size="sm"> className "rounded-full border-input bg-popover px-3 text-[12px] text-fg-strong"; position is app.css (absolute, bottom 0, centred), no position utility on it
```

`div[data-part="tool-rail"]` wraps a **lone** tool row (a `Row` of `kind: "item"` whose item is a tool): `pl-3 shadow-[inset_2px_0_0_var(--input)]` (2.9, 3.2).

### 2.3 User bubble

Component: `Item`, case `user` (in `Transcript.tsx`).

```
article[data-kind="user"]                                   KEPT.  flex flex-col items-end gap-1
├─ div[data-part="bubble"]                                  NEW. max-w-[78%] flex flex-col gap-2 rounded-[10px] border border-input bg-surface-raised px-3 py-[9px]     (#26232c, border #35313b, radius 10px, padding 9px 12px, gap 8px)
│   ├─ div[data-part="text"][style="white-space: pre-wrap"]  KEPT inline style (the Phase 2 exception), text = item.text.   m-0 text-body text-fg-strong      (13px IBM Plex, #f3ead9)
│   └─ div[data-part="attachments"]  flex flex-wrap gap-2    only when item.attachments.length > 0
│       └─ span[data-attachment="image"|"text"]               KEPT hook, one per attachment, in order.   flex items-center gap-2 rounded-md border border-input py-1 pr-2 pl-1
│           ├─ span[data-part="thumb"][aria-hidden="true"]    grid h-6 w-[34px] place-items-center rounded-[3px] bg-edge text-muted-foreground      (34x24, #3a3640)
│           │     image with dataBase64: <img alt="" src={`data:${mimeType};base64,${dataBase64}`} className="h-6 w-[34px] rounded-[3px] object-cover" />
│           │     otherwise: lucide ImageIcon (image) or FileTextIcon (text), size 14, stroke muted-foreground (#aba3af)
│           └─ span[data-part="name"]                         font-mono text-[11.5px] text-fg-strong      text = a.kind === "image" ? a.mimeType : a.name   (KEPT text)
└─ span[data-part="meta"]                                    font-mono text-meta text-muted-foreground      (11px, #aba3af)
      text: `you · ${formatClock(item.at)}` when formatClock(item.at) !== null, else `you`
```

- `[data-attachment]` is the same element as before (Phase 2 test 1: `[data-attachment="image"]` text is `image/png`); its text is now the `name` span only, and the thumb has no text, so `textContent` is unchanged.
- The user bubble is right-aligned by `items-end` on the article (not by `float` or `margin-left: auto` on a measured box).
- **Resolution:** the board's chip shows a file name and a size (`ci-run-8812.png`, `184 KB`); an image attachment carries only `mimeType` (and optional `dataBase64`), so the chip shows the mime type and no size (Not wired).

### 2.4 Assistant card, Thinking pill, working marker

Component: `Item`, case `assistant`. The article's children keep the order of `item.blocks`: each `thinking` block is a pill, each `text` block is a card.

```
article[data-kind="assistant"]                              KEPT.  flex flex-col gap-1.5
├─ (per thinking block, in order)  <ThinkingPill key={thinking} text={b.text} time={formatClock(item.at)} defaultOpen={thinking === "expanded"} />
├─ (per text block, in order)  div[data-part="card"]        NEW.  flex flex-col gap-1.5 rounded-[10px] border border-border bg-card px-3.5 py-2.5      (#211f26, border #2e2a33, radius 10px, padding 10px 14px, gap 6px)
│   ├─ <Markdown text={b.text} />                           root element is div.typeset.typeset-chat (2.4.1)
│   └─ span[data-part="stamp"]  (ONLY in the LAST card of the article, and only when `sessionKey` is passed)
│         self-end font-mono text-meta text-muted-foreground      (11px, #aba3af, bottom-right)
│         text: `${sessionKey} · ${formatClock(item.at)}` when the clock is non-null, else `${sessionKey}`
└─ span[role="status"][data-part="working"]                 KEPT hook and role; only while item.streaming; always the LAST child of the article.
      flex h-[26px] items-center gap-2.5 px-2 text-[12px] text-mizu-bright      (#92d1ca, h 26px, padding 0 8px)
    ├─ span[aria-hidden="true"]  className={glyphClass.active}    (data-tone="active" data-glyph="active")
    └─ text node  "Working…"                                      (U+2026; the span's textContent is exactly "Working…")
```

An assistant item with only thinking blocks has no card and no stamp. A `streaming` item whose last block is a thinking block still shows the working marker.

#### 2.4.1 `Markdown.tsx` (`web/src/ui/chat/Markdown.tsx`)

`splitMarkdown` and `Segment` are unchanged; `react-markdown` plus `remark-gfm` only. `Markdown` now returns `null` for no segments (KEPT) and otherwise `<div className="typeset typeset-chat">{segments}</div>` instead of a fragment (DESIGN.md 2g). `Markdown.test` 1 to 5 are unaffected (they test `splitMarkdown` and the segment memo, not a wrapper). Inline code colour `#92d1ca` 12px, list indent 20px, list gap 2px and block gap 6px come from the preset (DESIGN.md 2g), not from this file.

#### 2.4.2 `ThinkingPill` (in `web/src/ui/chat/Transcript.tsx`, not exported; used by `Item` only)

```ts
function ThinkingPill(props: { text: string; time: string | null; defaultOpen: boolean }): JSX.Element;
```

State: `const [open, setOpen] = useState(props.defaultOpen)`; `onToggle={(e) => setOpen(e.currentTarget.open)}`. The parent passes `key={thinking}` so a toggle press remounts the pill and re-applies the default (1.4).

```
details[data-kind="thinking"][open?=open]                   KEPT hook; closed in "folded".   group self-start
├─ summary                                                  flex h-6 cursor-pointer list-none items-center gap-2 rounded-xl border border-border px-2.5 text-[12px] text-muted-foreground hover:bg-row-hover    (h 24px, radius 12px, border #2e2a33, padding 0 10px, gap 8px, #aba3af)
│   ├─ span[data-part="caret"][aria-hidden="true"]  "▸"     font-mono text-[10px] group-open:rotate-90      (▸ closed, ▾ open)
│   ├─ span[data-part="label"]  "Thinking"
│   └─ span[data-part="meta"]  {time}                       font-mono text-fg-dim      (only when time !== null; #87808c)
└─ div[data-part="thinking-body"]  {text}                   mt-1 ml-1 whitespace-pre-wrap border-l-2 border-border pl-3 text-body text-muted-foreground
```

- The summary hides the browser marker: `[&::-webkit-details-marker]:hidden` plus `list-none`.
- The body is the raw `b.text` (not Markdown), as in Phase 2. **Resolution:** the board draws only the folded pill; the expanded layout is this file's decision (the pill stays, the raw text appears below it behind a 2px `border` rail).
- **Resolution:** the board's meta is `41s · 13:58` (a duration and a time); the protocol carries no thinking duration, so the meta is the clock only (Not wired: duration).
- Keyboard: `summary` is natively focusable; Enter and Space toggle. `⌃T` toggles all.
- `Phase 2 change:` the summary text was `Thinking…` + the raw text inside the `details`; it is now `Thinking` plus the optional time, the raw text is in `[data-part="thinking-body"]`. `Transcript.test` 2 is updated (2.10).

### 2.5 Compaction divider

Component: `CompactionRow` (`web/src/ui/chat/Folds.tsx`), props **KEPT** `{ item: CompactionItem }`.

```
details[data-kind="compaction"]                             KEPT; never open by default.   group
├─ summary                                                  flex h-6 cursor-pointer list-none items-center gap-3 text-meta text-muted-foreground    (a separator: line, label, line)
│   ├─ span[data-part="rule"][aria-hidden="true"]  h-px flex-1 bg-border
│   ├─ span[data-part="label"]  text below                  font-mono
│   └─ span[data-part="rule"][aria-hidden="true"]  h-px flex-1 bg-border
└─ div[data-part="compaction-body"]  <Markdown text={item.summary} />     mt-1 rounded-[10px] border border-border bg-card px-3.5 py-2.5
```

Label (exact): `tokensBefore === undefined` → `Context compacted`; else `` `Context compacted · from ${formatTokens(item.tokensBefore)} tokens` `` (1234 → `Context compacted · from 1.2k tokens`).

**Resolution:** COMPONENTS draws `Context compacted · 412k → 38k`; the after-size is not in `CompactionItem`, so only the before-size is shown. `Phase 2 change:` the summary text was `Compaction` / `Compaction (1234 tokens)`; `Transcript.test` 5 is updated (2.10). The `details[data-kind="compaction"]` hook, closed by default, and the summary's Markdown body are KEPT.

### 2.6 Queued submissions

`li[data-queued={clientSubmissionId}]` **KEPT**, in `ul[aria-label="Queued messages"]` (KEPT). New DOM, same pattern as 2.7:

```
li[data-queued]                                             flex flex-col items-end gap-1 py-[3px]
├─ div[data-part="bubble"]                                  max-w-[78%] rounded-[10px] border border-dashed border-line bg-surface-raised px-3 py-[9px]
│   └─ div[data-part="text"]  {q.text}                      whitespace-pre-wrap text-body text-fg-strong
└─ span[data-part="queued-status"]  "queued"                font-mono text-meta text-muted-foreground
```

### 2.7 Pending and failed user rows, and the `sent` defect

Source: `PendingSubmission.state` is `"sending" | "sent" | "failed"` (`web/src/state/store.ts`); `error?: WireError` with `message`.

**Known defect (Phase 2):** the row text is `` `${p.text} ` `` + `p.state === "sending" ? "sending…" : "failed: " + (p.error?.message ?? "unknown error")`, so a row in state `sent` (server acked, the echo of the user item not yet arrived; `session.submitAck` sets it, `echoed()` or a tail window removes it) is shown as `failed: unknown error`. **Fix:** the state is rendered exhaustively; only `failed` shows failure text.

Exact DOM for each state (the `li` is a child of `ul[aria-label="Queued messages"]`, `key = clientSubmissionId`, hook `data-pending` KEPT):

```
li[data-pending="sending"]            flex flex-col items-end gap-1 py-[3px]
├─ div[data-part="bubble"]            max-w-[78%] rounded-[10px] border border-input bg-surface-raised px-3 py-[9px] opacity-70
│   └─ div[data-part="text"]  {p.text}        whitespace-pre-wrap text-body text-fg-strong
└─ span[data-part="pending-status"][data-state="sending"]  "sending…"     font-mono text-meta text-muted-foreground      (U+2026)

li[data-pending="sent"]               (identical shape to "sending")
├─ div[data-part="bubble"]            same classes as "sending" (opacity-70)
│   └─ div[data-part="text"]  {p.text}
└─ span[data-part="pending-status"][data-state="sent"]  "sent"            font-mono text-meta text-muted-foreground
      NO role="alert", NO glyph, NO text containing "failed" or "unknown error"

li[data-pending="failed"]
├─ div[data-part="bubble"]            max-w-[78%] rounded-[10px] border border-status-blocked bg-status-blocked/10 px-3 py-[9px]
│   └─ div[data-part="text"]  {p.text}
└─ span[data-part="pending-status"][data-state="failed"][role="alert"]    flex items-center gap-1.5 font-mono text-meta text-status-blocked
    ├─ span[aria-hidden="true"]  className={glyphClass.blocked}           (data-tone="blocked" data-glyph="blocked")
    └─ text node  `failed: ${p.error?.message ?? "unknown error"}`
```

Test-facing facts: `sent` `textContent` is `` `${p.text}sent` `` (bubble text, then the status); it contains neither `failed` nor `unknown error`; the status has `data-state="sent"`; there is no `[role="alert"]` in the row. For `sending` the `li` text contains `sending…` and for `failed` with `error.message = "boom"` it contains `failed: boom` (Phase 2 test 6, still green); `failed` with no `error` contains `failed: unknown error`. Pending rows are **not** virtualised (they sit after `virtual-space`, as in Phase 2) and are not in `groupRows`. A new pending row still re-pins the viewport (Phase 2 behaviour, test 15).

### 2.8 Load earlier and Jump to bottom

Both KEPT hooks and copy (`Load earlier`, `Jump to bottom`); classes in 2.2. **Resolution:** the board's `▲ 3,912 earlier messages` needs a count the window does not carry; the Phase 2 copy `Load earlier` stays.

### 2.9 Virtualization contract (what every new wrapper keeps)

1. `MeasuredRow` and `div[data-row-key={row.key}]` are unchanged: no `className`, no `style`, no `data-index`, the same ref and `ResizeObserver` wiring. The measured element is that div (its border box).
2. Every row body is in **normal flow**: no `position: absolute` or `fixed`, no CSS `height`/`min-height`/`max-height`, no `transform` on a row element. (The window's `translateY` and the space's `height` + `position: relative` are Phase 2 inline styles, unchanged.) `overflow-hidden` is allowed only on inner blocks that size to content (the diff block).
3. Row keys come from `groupRows` unchanged (`item.id`, `group:<first tool id>`); `rows.ts` and `virtual.ts` are not edited.
4. The new wrapper `div[data-part="row-body"]` is the **only** child of `div[data-row-key]` and is rendered for every row, so `[data-row-key]`'s `firstElementChild` is stable across re-renders (`Rows.test` 2). Vertical spacing is its padding (0.3), never a margin.
5. A row changes height only by content (a `details` opening, a Show-more click, a Markdown stream); the observer reports it, as in Phase 2. Toggling Thinking or Tools changes heights and goes through the same `onHeight` compensation.
6. The log, pin, `data-at-bottom`, `jump-to-bottom` and prepend-anchor logic are not touched.

### 2.10 Phase 2 tests this section changes

| Test | Change |
|---|---|
| `Transcript.test` 1 | none: `article[data-kind="user"]` text still contains `hello  there` (the `[style white-space: pre-wrap]` div is now `[data-part="text"]`), `[data-attachment="image"]` text `image/png`, `article[data-kind="assistant"] strong` text `bold answer` |
| `Transcript.test` 2 | **updated:** `details[data-kind="thinking"]` closed, `summary` text is `Thinking` (no time when `at` is absent), `[data-part="thinking-body"]` text contains `pondering` |
| `Transcript.test` 3 | none (one `[role="status"][data-part="working"]`, text `Working…`, last child of the first assistant article) |
| `Transcript.test` 4, 7 to 17 | none |
| `Transcript.test` 5 | **updated:** `details[data-kind="compaction"]` closed; `[data-part="label"]` text is `Context compacted · from 1.2k tokens` for `tokensBefore: 1234`; the summary Markdown text is inside the details |
| `Transcript.test` 6, 18 | none (`li[data-queued]`, `li[data-pending="sending"]` contains `sending…`, `li[data-pending="failed"]` contains `failed: boom`, list inside the log after `virtual-space`) |
| `Transcript.test` 8 | none (`[data-row-key]` keys; the `u1` row contains `article[data-kind="user"]`) |
| new | `sent` row (2.7); `sent` row never contains `failed`; failed row has `[role="alert"]`; `Item` stamp text; user `meta` text; `thinking="expanded"` renders the pill open |

---

## 3. Folds and tools

### 3.1 Marker rows: cpd folds (`web/src/ui/chat/Folds.tsx`)

Component: `FoldRow`, props **KEPT** `{ item: FoldItem }` (`memo`). `FoldKind` is `cpd-context | cpd-plan-decision | cpd-plan-summary | cpd-skills | cpd`; **every** fold kind renders as a Marker row (the four named in the brief and `cpd-skills`).

```
details[data-kind="fold"][data-fold={item.foldKind}][open?=item.defaultOpen]     KEPT.  group
├─ summary[data-part="marker"]                              flex h-6 cursor-pointer list-none items-center gap-2 rounded-[5px] px-2 text-[12px] hover:bg-row-hover      (h 24px, padding 0 8px, gap 8px, radius 5px, 12px)
│   ├─ span[data-part="phi"][aria-hidden="true"]  "φ"       w-3 font-display text-[15px] font-bold text-status-decision      (EB Garamond 700, 15px, #ca88a3, width 12px)
│   ├─ span[data-part="source"]  {item.foldKind}            font-mono text-muted-foreground      (#aba3af)
│   ├─ span[data-part="marker-text"]  {item.title}          min-w-0 flex-1 truncate text-foreground      (#d8d2cc)
│   └─ span[data-part="time"]  {formatClock(item.at)}       font-mono text-meta text-fg-dim      (only when non-null; 11px, #87808c)
└─ div[data-part="fold-body"]  <Markdown text={item.body} />      mt-1 ml-5 rounded-[10px] border border-border bg-card px-3.5 py-2.5
```

Examples (from the board): `cpd-plan-decision` · `Plan review r1 approved` · `13:52`; `cpd-context` · `decision · Stream high-water mark is per-session` · `14:01`. `item.title` is built by the store (title rules in `domain.ts`), not here.

- `open` is the native attribute set by React from `item.defaultOpen` (true only for `cpd-plan-summary`, KEPT); the fold is otherwise one line. A click on the summary toggles it natively; Enter and Space too. No state is held.
- **Resolution:** COMPONENTS names the shadcn `Marker` + `Collapsible`; the native `details`/`summary` keeps the Phase 2 hooks (`details[data-kind="fold"]`, `open`, `data-fold`) with no script.
- `Phase 2 change:` `Folds.test` 1 asserted the `summary` text equals the title; the summary now holds the φ, the source and the time, so those assertions read `[data-part="marker-text"]` (text `Title cpd-context`). Tests 2 and 3 (`open`, `data-fold`, body text) are unchanged.

### 3.2 `ToolRow` (`web/src/ui/chat/ToolRow.tsx`)

```ts
export const ToolRow: MemoExoticComponent<(props: {
  tool: ToolItem;
  /** true: the row is inside a Tool Group (its summary starts at 26px). Default false. */
  nested?: boolean;
}) => JSX.Element>;
export function toolName(tool: ToolItem): string;      // NEW export: name without a leading "mcp__"
export function toolDetail(tool: ToolItem): string;    // NEW export: detailOf(tool.name, tool.args), unchanged rules
export const OrphanRow: MemoExoticComponent<(props: { item: OrphanResultItem }) => JSX.Element>;   // KEPT
```

`detailOf`, `statusOf`, `splitLines`, `diffKind`, `Body`, `Output`, `PlainOutput`, `TOOL_PREVIEW_LINES` (10) and the `mcp__` prefix rule are **KEPT** in behaviour and copy (`Running`, `Failed: <reason>`, `Done in 1.5s (3 lines)`, `Done (+1 -1)`, `Show N more lines`); `formatDuration` moves to `format.ts`. A lone tool row (outside a group) is rendered by `RowBody` inside `div[data-part="tool-rail"]` (3.3); inside a group, no extra rail (the group has it).

Glyph by status (the status is `ToolItem.status`): `running` → `glyphClass.active` / tone `active`; `done` → `glyphClass.done` / tone `done`; `failed` → `glyphClass.blocked` / tone `blocked` (the board's `g-err` is the triangle `glyph-blocked`).

```
details[data-kind="tool"][data-tool={tool.name}][data-status={tool.status}][open?=(tool.name === "edit")]    KEPT.   group
├─ summary                                                  flex h-[22px] cursor-pointer list-none items-center gap-2.5 pr-2 text-[12px] hover:bg-row-hover rounded-[5px]; padding-left: nested ? pl-[26px] : pl-2     (h 22px, gap 10px, padding 0 8px 0 26px)
│   ├─ span[data-part="glyph"][aria-hidden="true"][data-tone][data-glyph]  className={glyphClass[g]}
│   ├─ span[data-part="name"]  {toolName(tool)}             KEPT. font-mono min-w-10 text-muted-foreground      (verb, #aba3af)
│   ├─ span[data-part="detail"]  {toolDetail(tool)}         KEPT. min-w-0 flex-1 truncate font-mono text-foreground      (target, #d8d2cc)
│   ├─ span[data-part="delta"]  (edit with tool.diff only)  font-mono
│   │   ├─ span[data-part="added"]    `+${A}`               text-status-done       (#8da96e)
│   │   └─ span[data-part="removed"]  `−${R}`  (U+2212)     text-status-blocked    (#e17f6e)    A and R as in statusOf
│   └─ span[data-part="status"]  {statusOf(tool)}           KEPT. font-mono text-meta text-fg-dim      (11px, #87808c; the board shows only a time, the Phase 2 text wins)
└─ output (3.5 and 3.6)  or nothing when result is undefined
```

Both gaps in the summary row are `gap-2.5` (10px, the board's). `edit` stays open by default (KEPT, `ToolRow.test` 2); every other tool starts closed and shows its output on open. **Resolution:** the board's row shows glyph, verb, target, time; the Phase 2 text `Done in 1.5s (3 lines)` is in the same slot as the time and is kept for its tests.

`OrphanRow` DOM keeps `article[data-kind="orphan-result"]` > `p` (text `Result of {name} (call not loaded)`, `text-[12px] text-muted-foreground`) and the `pre[data-part="output"]` of 3.5.

### 3.3 Standalone tool row rail

`RowBody` renders a `Row` of `kind: "item"` with a tool item as:

```
div[data-part="tool-rail"]      pl-3 shadow-[inset_2px_0_0_var(--input)]      (the 2px rail is an inset box-shadow, #35313b, 12px left padding)
└─ <ToolRow tool={item} />      (nested false)
```

### 3.4 Tool Group (`web/src/ui/chat/ToolGroup.tsx`, new)

Grouping **already exists**: `groupRows` (`web/src/ui/chat/rows.ts`) turns a run of two or more consecutive `kind === "tool"` items into `{ kind: "tools", key: "group:<first tool id>", tools }`; a lone tool is an `item` row. `state/transcript.ts` produces the items and does no grouping. **Reuse it unchanged**; `Transcript` does not group at render time and `rows.ts` is not edited. The old inline `ToolsRow` in `Transcript.tsx` is replaced by `ToolGroup`.

```ts
import type { ToolItem } from "../../protocol/domain.ts";
import type { ToolsMode } from "./useChatToggles.ts";

export interface ToolGroupProps {
  /** Two or more consecutive tool items, from a `Row` of kind "tools". */
  readonly tools: readonly ToolItem[];
  /** The Tools toggle: "grouped" = collapsed, "expanded" = open. */
  readonly mode: ToolsMode;
}
export const ToolGroup: MemoExoticComponent<(props: ToolGroupProps) => JSX.Element>;
```

`memo` comparator: same `mode` and same length and element-wise identical `tools` (`sameTools` extended with `a.mode === b.mode`). `RowBody` renders `<ToolGroup key={mode} tools={row.tools} mode={mode} />` so a toggle press remounts every group and discards local overrides (1.4).

State: `const [open, setOpen] = useState(props.mode === "expanded")`; `<details open={open} onToggle={(e) => setOpen(e.currentTarget.open)}>`.

Derived (pure, in the component file): `failed = tools.filter(t => t.status === "failed").length`; `running = tools.some(t => t.status === "running")`; `total = sum of result.durationMs over tools that have one` (`hasTime` = at least one has it); aggregate status `agg = failed > 0 ? "failed" : running ? "running" : "done"` (**failed wins over running**).

```
details[data-kind="tool-group"][data-tool-count={tools.length}][data-status={agg}][open?=open]     KEPT hook; NEW data-tool-count, data-status.   group pl-3 shadow-[inset_2px_0_0_var(--input)]
├─ summary[data-part="group-header"]                        flex h-[26px] cursor-pointer list-none items-center gap-2.5 rounded-[5px] px-2 text-[12px] hover:bg-row-hover      (h 26px, gap 10px, padding 0 8px, radius 5px)
│   ├─ span[data-part="caret"][aria-hidden="true"]  "▸"     font-mono text-[10px] text-muted-foreground group-open:rotate-90
│   ├─ span[data-part="glyph"][aria-hidden="true"][data-tone][data-glyph]    agg "failed" → blocked, "running" → active, "done" → done  (glyphClass of it)
│   ├─ span[data-part="name"]  {toolName(tools[0])}         font-mono text-fg-strong      (#f3ead9)
│   ├─ span[data-part="summary"]  text below                min-w-0 truncate text-foreground      (#d8d2cc)
│   ├─ <ToolTickStrip statuses={tools.map(t => t.status)} />
│   ├─ span.flex-1
│   ├─ span[data-part="totals"]  text below                 font-mono text-meta text-muted-foreground      (11px, #aba3af)
│   └─ span[data-part="group-count"].sr-only  `${tools.length} tool calls`
└─ div[data-part="group-rows"]                              flex flex-col gap-0.5      (gap 2px)
    └─ <ToolRow key={t.id} tool={t} nested />  per tool
```

- `[data-part="summary"]` text: `` `${toolDetail(tools[0])} + ${n} more ${n === 1 ? "step" : "steps"}` `` with `n = tools.length - 1` (board: `stream.test.ts + 11 more steps`; the detail is the full target, not its basename).
- `[data-part="totals"]` text: `running` (when `running`) else `done`; then ` in ${formatDuration(total)}` when `hasTime` and not `running`; then ` · ${failed} failed` when `failed > 0`. Examples: `done in 6.2s · 1 failed`, `done`, `running · 1 failed`. **Resolution:** the board shows `6.21s` (two decimals); the single `formatDuration` of Phase 2 gives one decimal (`6.2s`) and is kept.
- Closed (`tools="grouped"`): only the header shows, so a failed call is visible only as a red tick, the `failed` count and the blocked glyph. **Resolution:** COMPONENTS says failed calls "break out of the fold"; that is not done (the toggle is a single global rule, Not wired).
- Keyboard: the `summary` is the control (Enter, Space). Tab order: header, then each opened row's summary and its Show-more button.
- `Phase 2 change:` the group was `details[open]` with `summary` text `N tool calls`, open by default. Now it follows `tools` (collapsed by default), and `N tool calls` is the `sr-only` `[data-part="group-count"]`. `ToolRow.test` 5 is updated: with `tools` unset the group is closed (`open` false); with `mode="expanded"` it is open; the three nested `details[data-kind="tool"]` exist in the DOM either way; `[data-part="group-count"]` text is `3 tool calls`; a single item renders no `tool-group`.

### 3.5 `ToolTickStrip` (`web/src/ui/chat/ToolTickStrip.tsx`, new)

```ts
import type { ToolStatus } from "../../protocol/domain.ts";
export function ToolTickStrip(props: { readonly statuses: readonly ToolStatus[] }): JSX.Element;
```

```
span[data-part="tick-strip"][role="img"][aria-label={label}]       flex gap-0.5      (gap 2px)
└─ span[data-part="tick"][data-status={status}]  × statuses.length, same order      block h-[10px] w-[3px] rounded-[1px] + colour class
```

- Colour (literal fills, tokens): `done` → `bg-status-done` (`#8da96e`), `failed` → `bg-status-blocked` (`#e17f6e`), `running` → `bg-status-active` (`#67ada7`). One tick per call: `statuses.length` ticks (12 calls: 12 × 3 + 11 × 2 = 58px wide).
- `label` (exact): `` `${n} steps` `` when no tick is failed, else `` `${n} steps, ${f} failed` `` (board: `12 steps, 1 failed`); `n = statuses.length`, `f` = failed count. A group has `n ≥ 2`, so the singular is not needed; for `n = 1` the label is `1 steps` (never rendered by `Transcript`).
- **Resolution:** the board draws ticks only for done and failed (`No tick for running or queued`); the brief says one tick per call, so a running call has a `bg-status-active` tick (the protocol has no `queued` tool status).
- Not interactive; no focus; no motion.

### 3.6 Output blocks: inline `bash` output and `edit` diffs

Both are the `Output` of `ToolRow` (inside the `details`, after the summary). Hooks **KEPT**: `pre[data-part="output"]`, `[data-diff="add"|"remove"|"context"]`, `button[data-action="expand-output"]`; 10-line preview, `Show N more lines`, the expand-once behaviour.

```
(plain output, every tool except edit-with-diff)
pre[data-part="output"]           ml-11 mr-2 mb-0.5 rounded-md bg-card px-2.5 py-1.5 font-mono text-[11.5px] leading-[1.5] whitespace-pre-wrap text-muted-foreground      (margin 0 8px 2px 44px, padding 6px 10px, radius 6px, #211f26, 11.5px / 1.5, #aba3af)
└─ span × shown lines (leading "\n" text on lines after the first: unchanged)

(edit with tool.diff)
div[data-part="diff"]             ml-11 mr-2 mb-0.5 overflow-hidden rounded-md bg-card font-mono text-[11.5px] leading-[1.55]      (margin 0 8px 2px 44px, radius 6px, #211f26, 11.5px / 1.55; the Phase 2 plain div gains this hook)
└─ div[data-diff={kind}] × shown lines         block px-2.5 whitespace-pre text-foreground + kind class
      add      bg-status-done/[0.11]      (rgba(141,169,110,.11))
      remove   bg-status-blocked/10       (rgba(225,127,110,.10))
      context  text-muted-foreground      (hunk headers such as "@@ -1 +1 @@" and unchanged lines)

button[type="button"][data-action="expand-output"]  "Show {rest} more lines"      ml-11 mr-2 py-0.5 font-mono text-meta text-fg-strong      (after the block; #f3ead9, 11px)
```

- For `add` and `remove` lines the first character is wrapped: `<span data-part="sign" className="text-status-done">+</span>` (add) or `<span data-part="sign" className="text-status-blocked">-</span>` (remove) followed by the rest of the line, so `textContent` of the `[data-diff]` element is the unchanged diff line (`+new`, `-old`; Phase 2 test 4 pins it). Lines starting `+++` or `---` are `context` (KEPT `diffKind`) and have no sign span. **Resolution:** the board's removed marker is `−` (U+2212); the line text must stay `-old`, so the ASCII hyphen is coloured instead.
- `bg-card` is used for both blocks and the dim colour `fg-dim` is not (it is only allowed on `background` and `surface-sunken`); the hunk header is `text-muted-foreground`, not the board's `#87808c`.
- The light-mode `app.css` rules for `[data-diff="add"]` and `[data-diff="remove"]` are removed (DESIGN.md 2a); the colours above replace them.
- A failed `bash` shows its output on opening the row, in the same `pre`. **Resolution:** the board draws the failed output open under the row; `ToolRow` stays closed by default except `edit` (Phase 2 `ToolRow.test` 2 pins `open` only for `edit`).

---

## 4. KEPT Phase 2 hooks and the test that pins each

| Hook (KEPT) | Where | Pinned by |
|---|---|---|
| `header h1[data-part="key"]` text = key; `[data-part="title"]` only when `title !== null`; `document.title` rules and restore | ChatHeader | `ChatHeader.test` 1, 2, 3 |
| `[data-part="transcript"] > [role="log"][aria-label="Transcript"]`, `data-at-bottom` | Transcript | `Transcript.test` 1, 9, 10, 11, 12, 13 |
| `div[data-row-key]` (keys `u1`, `a1`, `group:tool:t1`, `c1`, `tool:t3`), one per mounted row, at most 60 for 1000 items | Transcript | `Transcript.test` 7, 8; `Rows.test` 2 |
| `button[data-action="load-earlier"]` text `Load earlier`; `button[data-action="jump-to-bottom"]` text `Jump to bottom` (parent `[data-part="transcript"]`) | Transcript | `Transcript.test` 4, 11 |
| `article[data-kind="user"]` (text, whitespace kept), `[data-attachment="image"]` text `image/png` | user bubble | `Transcript.test` 1 |
| `article[data-kind="assistant"]`, Markdown `strong` | assistant card | `Transcript.test` 1; `Rows.test` 1 |
| `details[data-kind="thinking"]` closed by default | Thinking pill | `Transcript.test` 2 (updated, 2.10) |
| `[role="status"][data-part="working"]` text `Working…`, last child of the assistant article | working marker | `Transcript.test` 3 |
| `details[data-kind="compaction"]` closed by default | compaction | `Transcript.test` 5 (updated) |
| `ul[aria-label="Queued messages"]` inside the log after `virtual-space`; `li[data-queued]`; `li[data-pending]` with `sending…` and `failed: boom` | queued and pending | `Transcript.test` 6, 18 |
| `details[data-kind="fold"][data-fold][open]` (open only for `cpd-plan-summary`) | Marker row | `Folds.test` 1, 2, 3 |
| `details[data-kind="tool"][data-tool][data-status]`, `open` only for `edit`; `[data-part="name"]`, `[data-part="detail"]`, `[data-part="status"]` texts | ToolRow | `ToolRow.test` 1, 2 |
| `pre[data-part="output"]` 10 lines; `button[data-action="expand-output"]` `Show N more lines` | output | `ToolRow.test` 3 |
| `[data-diff="add"\|"remove"\|"context"]` texts and the status `Done in 1.5s (+1 -1)` | diff | `ToolRow.test` 4 |
| `details[data-kind="tool-group"]`, one per run of two or more tools, none for a single item | Tool Group | `ToolRow.test` 5 (updated, 3.4); `Rows.test` 2 (`[data-row-key^="group:"]`) |
| `groupRows` keys, `rows.ts` unchanged | grouping | `rows.test` 1, 2, 3 |
| `virtual.ts` unchanged | virtualization | `virtual.test` 1 to 7 |
| `article[data-kind="orphan-result"]`, text `Result of … (call not loaded)` | OrphanRow | no Phase 2 test pins it (hook kept by this contract) |
| `splitMarkdown` and the per-segment memo | Markdown | `Markdown.test` 1 to 5 |
| `Footer` (`footer[aria-label="Session footer"]`, `[data-stat]`) | not changed here | `Footer.test` 1, 2 |

`web/src/types-unique.test.ts` is unaffected: no interface named there is redeclared (`ChatHeaderProps`, `ToolGroupProps`, `ChatToggles` are new names).

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
- The shadcn `Marker`, `Bubble`, `Attachment`, `Trace` and `MessageScroller` parts (see the shadcn note at the top).
- A copy-to-clipboard button on the assistant card; hover timestamps; message reactions.
