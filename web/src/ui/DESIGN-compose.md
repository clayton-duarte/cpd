# Dango v3 + shadcn/ui: design contract (composer, tier selector, footer, AskCard, PlanReviewCard)

Scope: Plan `webui-phase3-good`, Workflow `ui`, package `web/`. Companion of `web/src/ui/DESIGN.md` (**read it first**: hook conventions 1.1, styling rules 1.2, tokens 2b, glyph classes 2c, `dango-status.ts` 2d, `cn` 2e, shadcn parts 2f, typeset 2g). Everything defined there is reused by reference and not restated: token utilities (`bg-card`, `border-input`, `text-fg-strong`, `bg-primary`, ...), `Button` / `Badge` / `Textarea` parts (`web/src/ui/components/*.tsx`), the focus ring, `font-mono`, the four text sizes, `.typeset .typeset-chat`. Tests are written from this file alone by one Worker, the implementation by another.

Decisions: pro-d16, pro-d18, pro-d19, pro-d21, pro-n11, pro-n12, pro-n14, pro-n15. Assumptions A1 to A6 bind (no `@/` alias, relative imports with extension; no new dependency; no change to `web/src/protocol/**`, `web/src/state/**`, `web/src/server/**`; an element whose data does not exist renders only from an optional prop and is otherwise omitted).

**Contradiction rule** (as in DESIGN.md): a board's rendered value beats the handoff text; Phase 2 behaviour (data, ARIA, tests) beats a board unless stated. Each resolution is one line marked **Resolution**.

**Board sources**: `Main.dc.html` (composer, footer), `Question.dc.html` (AskCard), `PlanReview.dc.html` (PlanReviewCard). Values below are source values read from the scout artefacts; nothing was rendered in a browser (unverified).

**Tests named here that do not exist yet** are new files written by the test Worker: `composer.dango.test.tsx`, `tier.dango.test.tsx`, `footer.dango.test.tsx`, `ask.dango.test.tsx`, `planreview.dango.test.tsx` (same folders as the components). The Phase 2 test files stay and must pass unchanged unless a "Test changes" line says otherwise.

---

## 0. ChatPane wiring (the one place the three pieces meet)

`web/src/ui/ChatPane.tsx` keeps its props `{ view: SessionView; connection: Connection; reader: FileReaderBoundary }` and keeps `div[data-chat-dropzone]` with its `onDragOver` / `onDrop` (pinned by `App.test` "dropping an image sends it as an attachment"). New child order inside the dropzone, top to bottom: `ChatHeader`, `Transcript`, the dialog card (`AskCard` or `PlanReviewCard`, when `view.dialog !== null`), `Composer`, `Footer`. (Phase 2 order was TierSelector, Footer, Composer; the board puts the footer last and the TierSelector inside the Composer.)

What ChatPane passes (source field → prop):

| Prop | Value | Source |
|---|---|---|
| `Composer.settings` | `<TierSelector capabilities footer models busy onConfigure onRequestModels />` (props unchanged from Phase 2) | `view.capabilities`, `view.footer`, `view.models`, `view.busy`, `connection.configure`, `connection.listModels` |
| `Composer.busy` | the computed `busy` of `DESIGN-chat.md` 2.4.3: `view.busy \|\| view.ref?.state === "working" \|\| view.pending.some((p) => p.state === "sending")` | `view.busy`, `view.ref`, `view.pending` (the same value ChatPane passes to `ChatHeader` and `Transcript`) |
| `Composer.label` | `view.ref?.key ?? view.id` | `SessionRef.key` (`web/src/protocol/domain.ts`) |
| `Footer.cwd` | `view.ref?.cwd` | `SessionRef.cwd?` |
| `Footer.sessionKey` | `view.ref?.key ?? view.id` | `SessionRef.key` |
| `Footer.contextWindow` | `view.models?.find((m) => m.model === <known footer.model>)?.contextWindow` | `ModelChoice.contextWindow` |
| `AskCard.asker`, `PlanReviewCard` | `AskCard.asker = view.ref?.key ?? view.id`; PlanReviewCard gets no new prop from ChatPane | `SessionRef.key` |
| everything else new (`tokens`, `cacheHitPercent`, `recommended`, `review`, `onEditQueued`) | **not passed** | no source field exists (see each "Not wired") |

Layout of the three cards in the chat column: normal flow, never absolute. The Composer is `mx-4`, the dialog card is `mx-4 mb-3`, the footer is full width. The chat `main[aria-label="Chat"]` is `flex flex-col`; `Transcript` is the only `flex-1 min-h-0` child; the dropzone is `flex h-full min-h-0 flex-col`. Transcript virtualization contract (`data-row-key`, normal-flow rows, sticky auto-scroll) is not touched by this file.

---

## 1. Composer

File `web/src/ui/composer/Composer.tsx`. Exports `Composer` and `ComposerHandle` (`{ addFiles(files: readonly FileLike[]): Promise<void> }`, KEPT).

### 1.1 Props

```ts
import type { ReactNode, Ref } from "react";
import type { Attachment, Capabilities, QueuedSubmission, WhenBusy } from "../../protocol/domain.ts";
import type { FileReaderBoundary } from "./attachments.ts";

export function Composer(props: {
  // KEPT, unchanged; `busy` is the computed value of DESIGN-chat.md 2.4.3 (ChatPane passes it)
  capabilities: Capabilities | null; busy: boolean; queued: readonly QueuedSubmission[];
  reader: FileReaderBoundary;
  onSubmit(input: { text: string; attachments: readonly Attachment[]; whenBusy: WhenBusy }): void;
  onAbort(): void; onCancelQueued(clientSubmissionId: string): void;
  ref?: Ref<ComposerHandle>;
  // NEW, all optional
  settings?: ReactNode;                  // the TierSelector, rendered in the action row after the attach button; absent: nothing rendered there
  label?: string;                        // session key; used in the textarea placeholder; absent: placeholder without the name
  onEditQueued?(clientSubmissionId: string): void;   // when passed, each queued chip shows an `Edit` button; absent: no Edit button (Not wired)
}): React.JSX.Element;
```

Data mapping: queued chips come from `props.queued` (`QueuedSubmission { clientSubmissionId, submissionId?, text, whenBusy }`, `text` is the first 200 characters, filled by the store from `session.queued` messages). Attachments are local state, built by `addFiles()` of `attachments.ts` (behaviour unchanged: images png/jpeg/gif/webp up to `IMAGE_MAX_COUNT` and `IMAGE_MAX_TOTAL_BASE64`, text files inline up to `TEXT_INLINE_MAX_BYTES`, binary rejected with the reasons of `RejectionReason`). `attachments.ts` and `attachments.test.ts` are not changed.

### 1.2 DOM outline

Every hook is KEPT unless marked NEW.

```
form[aria-label="Composer"][data-drop="idle|over"]                                   KEPT
     className: mx-4 flex flex-col rounded-[10px] border border-input bg-card
                focus-within:border-ramune-bright
                data-[drop=over]:border-dashed data-[drop=over]:border-primary data-[drop=over]:bg-primary/5
├─ span[data-part="drop-cue"]  "Drop to attach"                  (only while data-drop="over")   NEW   className: px-3 pt-1.5 font-mono text-meta text-primary
├─ ul[aria-label="Queued"][data-part="queued-bar"]            (only when queued.length > 0)   NEW data-part
│    className: m-0 flex list-none flex-wrap items-center gap-2 px-2.5 pb-0 pt-[7px]
│  └─ li[data-queued="<clientSubmissionId>"][data-when-busy="followUp|steer"]   (data-queued KEPT; data-when-busy NEW)
│       className: flex h-6 items-center gap-2 rounded-md border border-dashed border-line pl-2 pr-1 text-[12px]
│     ├─ span[data-part="queued-kind"]   "follow-up queued" | "steer queued"        className: font-mono text-meta text-muted-foreground
│     ├─ span[data-part="queued-text"]   {q.text}                                   className: truncate text-fg-strong   (max-w 320px)
│     ├─ button[type="button"][data-action="edit-queued"]  "Edit"                   only when onEditQueued passed; className: px-1 text-[11.5px] text-fg-strong   NEW
│     └─ button[type="button"][aria-label="Cancel queued message"]  "×"-icon        KEPT aria-label (Phase 2 wins over the board's "Cancel queued follow-up")
│          className: grid size-5 place-items-center text-fg-strong; icon lucide XIcon 12px, stroke 2.4
│          disabled={reasons.abortSubmission !== null}  title={reasons.abortSubmission ?? undefined}   KEPT
├─ ul[aria-label="Attachments"][data-part="attachment-bar"]   (only when attachments.length > 0)
│    className: m-0 flex list-none flex-wrap gap-2 px-2.5 pt-[7px]
│  └─ li[data-attachment="<name>"][data-kind="image|text"]                           KEPT
│       className: flex h-6 items-center gap-1.5 rounded-md border border-edge bg-surface-raised pl-2 pr-1 font-mono text-[11.5px] text-foreground
│     ├─ img[data-part="attachment-thumb"][alt="<name>"][src="data:<mimeType>;base64,<dataBase64>"]   NEW, images only; className h-6 w-[34px] rounded-[3px] object-cover
│     ├─ span[data-part="attachment-name"]  {name}              NEW
│     └─ button[type="button"][aria-label="Remove <name>"]  XIcon 12px                KEPT
├─ ul[role="alert"][aria-label="Rejected files"]              (only when rejected.length > 0)   KEPT
│    className: m-0 list-none px-3 pt-1.5 text-key text-status-blocked-bright
│  └─ li[data-part="rejected-file"]  text `<name>: <reason>`   NEW hook; `reason` is `AttachmentRejection.message` (the readable text; the enum `reason` is the code)
├─ textarea[aria-label="Message"][rows=2]   KEPT aria-label (exact string; App.test and Composer.test select it)
│    className: w-full resize-none border-0 bg-transparent px-3 py-2 text-[13.5px] leading-[1.5] text-fg-strong placeholder:text-fg-dim
│               focus-visible:outline-none   (the card's focus-within border is the focus affordance, see 1.5)
├─ input[type="file"][multiple][aria-label="Attach files"][tabindex="-1"]   KEPT (tests set its `files` and fire `change`); className: sr-only; disabled when capabilities === null
├─ div[data-part="action-row"]   className: flex flex-wrap items-center gap-1.5 px-2 pb-2
│  ├─ button[type="button"][data-action="attach"][aria-label="Choose files to attach"]   NEW   className: grid size-[30px] place-items-center rounded-md text-fg-strong hover:bg-row-hover
│  │    PaperclipIcon 16px stroke 2; onClick → the hidden file input's `.click()`; disabled when capabilities === null
│  │    (**Resolution**: the board's button is labelled "Attach files"; that name already belongs to the Phase 2 file input, so the visible button gets its own label and the input keeps the Phase 2 one.)
│  ├─ {settings}                         (the TierSelector, section 2)
│  ├─ span[aria-hidden]  className: flex-1  (spacer)
│  ├─ BUSY (busy === true):
│  │  ├─ div[role="group"][aria-label="Send while the agent is running"][data-part="send-split"]   NEW
│  │  │    className: flex overflow-hidden rounded-[7px] border border-input text-[12px]
│  │  │  ├─ button[type="button"][data-when-busy="steer"]      KEPT   className: flex h-[30px] items-center gap-1.5 bg-primary px-3 font-semibold text-primary-foreground disabled:opacity-50
│  │  │  │     children: "Steer" + kbd "⏎"
│  │  │  └─ button[type="button"][data-when-busy="followUp"]   KEPT   className: flex h-[30px] items-center gap-1.5 px-3 text-fg-strong hover:bg-row-hover disabled:opacity-50
│  │  │        children: "Follow-up" + kbd "⌥⏎"
│  │  └─ button[type="button"][data-action="abort"][aria-label="Stop"]   KEPT data-action
│  │       className: flex h-[30px] items-center gap-1.5 rounded-[7px] border border-status-blocked/55 px-3 text-[12px] text-status-blocked-bright
│  │       children: square icon (svg 10x10, `rect` width 10 height 10 rx 2, fill currentColor, aria-hidden) + "Stop" + kbd "⌃C"
│  └─ IDLE (busy === false):
│     └─ button[type="submit"][data-action="send"]   KEPT
│          className: flex h-[30px] items-center gap-1.5 rounded-[7px] bg-primary px-3 text-[12px] font-semibold text-primary-foreground disabled:opacity-50
│          children: "Send" + kbd "⏎"
└─ div[data-part="reasons"]   (only when at least one reason is non-null)   NEW   className: flex flex-col gap-0.5 px-3 pb-2
   └─ small[data-reason]  {reason}   KEPT   className: text-meta text-muted-foreground
```

`kbd` elements (all of the composer): `font-mono text-[11px]`; inside `Steer` they inherit the button colour (`text-primary-foreground`); inside `Follow-up` and `Stop` they are `text-muted-foreground`. Exact copy: `Steer`, `Follow-up`, `Stop`, `Send`, `Edit`, `follow-up queued`, `steer queued`; keys `⏎`, `⌥⏎`, `⌃C` (the board's strings). **Resolution:** Phase 2 labelled the buttons `Follow up` and `Abort`; the board's `Follow-up` and `Stop` win (tests select by `data-when-busy` / `data-action`, not by text).

The reasons (`small[data-reason]`) are collected below the action row, not after each button; their order is: busy: steer, follow-up, abort reasons; idle: follow-up reason; a `null` reason renders nothing. `Composer.test` "unsupported capabilities are disabled with their reason" reads them with `querySelectorAll("small[data-reason]")` and `arrayContaining`, so the position does not matter.

### 1.3 Textarea

- `rows={2}` minimum; auto-grows with its content up to a maximum of 192px (about 8 lines of 13.5px at 1.5), then scrolls (`overflow-y: auto`). Implementation contract: a layout effect on `text` sets `el.style.height = "auto"` then `el.style.height = Math.min(el.scrollHeight, 192) + "px"` (an imperative DOM write; the only inline style of the component, 1.2 item 1 of DESIGN.md allows it because Tailwind cannot express a measured height). After a submit (text cleared) the height returns to the 2-row minimum. jsdom reports `scrollHeight` 0; tests must not assert the pixel height.
- Placeholder (U+2026 is the one-character ellipsis):
  - `busy` and `label` given: `Steer {label}, or queue a follow-up…` (the board's string with `label` = `chat-research`)
  - `busy` and no `label`: `Steer, or queue a follow-up…`
  - idle and `label` given: `Message {label}…`; idle without `label`: `Message…`
  - `capabilities === null`: `Not attached` (the textarea is `disabled`)
  - (idle copy is not on the board; it is the one invented string, chosen to mirror the busy one.)
- `aria-label="Message"` is fixed (KEPT); the board's `label.sr` text `Message chat-research` is **not** used (Phase 2 selectors pin the exact name).
- Disabled when `capabilities === null` (KEPT; `app.css` gives it `cursor: not-allowed`).

### 1.4 Keyboard and submit behaviour

`reasons` = `{ steer, followUp, abort, abortSubmission }`, each `null` when supported, else `Support.reason` (KEPT). `canSend` = `text.trim() !== "" || attachments.length > 0` (KEPT). `submit(whenBusy)` is a no-op when `capabilities === null`, `!canSend`, or `reasons[whenBusy] !== null`; otherwise it calls `onSubmit({ text: text.trimEnd(), attachments, whenBusy })` and clears text, attachments and rejections (KEPT).

| Key / action (focus in the textarea) | Idle | Busy |
|---|---|---|
| `Enter` (no modifier) | `submit("followUp")` (KEPT; `Composer.test` 1, `App.test` "submit sends a session.submit message") | `submit("steer")` when `reasons.steer === null`, else `submit("followUp")` when `reasons.followUp === null`, else nothing. **Resolution:** Phase 2 preferred follow-up when busy; the board labels `⏎` on Steer and `⌥⏎` on Follow-up and the visible hints must be true, so busy Enter steers. No Phase 2 test pins busy Enter. |
| `Alt+Enter` (`e.altKey`, option on macOS) | `submit("followUp")` | `submit("followUp")` (nothing when its reason is non-null) |
| `Shift+Enter` | newline (default; no `preventDefault`) (KEPT) | newline |
| `Enter` while `e.nativeEvent.isComposing` | ignored (KEPT) | ignored |
| `Ctrl+C` (`e.ctrlKey`, key `c`) | nothing (default copy) | only when the textarea selection is empty (`selectionStart === selectionEnd`) and `reasons.abort === null`: `preventDefault()` and `onAbort()`; with a non-empty selection the default copy runs. This makes the `⌃C` hint on Stop true without breaking copy. |
| paste with files | `preventDefault()`, `add(filesFromClipboard(...))` (KEPT) | same |

Every `Enter` / `Alt+Enter` path calls `preventDefault()` first (KEPT). Clicking `Steer`, `Follow-up`, `Send`, `Stop` calls the same functions as the keys. The form's `onSubmit` (the idle Send button, or implicit submit) does `enterTarget()` of Phase 2: `followUp` when idle; busy: follow-up if supported else steer (the button is not shown busy, so this path is only reachable idle; KEPT unchanged).

Drag and drop: `onDragEnter` / `onDragOver` with `Files` in `dataTransfer.types` → `preventDefault()`, `data-drop="over"`; `onDragLeave` → `idle`; `onDrop` → `preventDefault()`, `stopPropagation()`, `idle`, `add(filesFromDrop(...))`. **Dropped files are added exactly once:** the Composer's `stopPropagation()` keeps a drop on the Composer from bubbling to the ChatPane dropzone, so the dropzone's `onDrop` does not run for it. A drop elsewhere in the chat column is added once by the dropzone through `ComposerHandle.addFiles`. The outer `div[data-chat-dropzone]` of ChatPane stays the page-wide target (`App.test`). Tests: `Composer.test` "dragging files over the composer shows a drop state and dropping adds attachments".

### 1.5 States

| State | Rendering |
|---|---|
| idle | action row: attach, settings, spacer, `Send ⏎` |
| busy | action row: attach, settings, spacer, split group `[Steer ⏎ | Follow-up ⌥⏎]`, `Stop ⌃C` (Stop exists **only** while `busy`) |
| `capabilities === null` | textarea, file input and attach button `disabled`; placeholder `Not attached`; send buttons per `reasons` (all `null` when caps is `null`, but `submit` no-ops) |
| a capability unsupported | that button `disabled` with `title={reason}` and the reason in `small[data-reason]`; others unaffected (KEPT) |
| focus | the card border turns `#a1c6ef` (`focus-within:border-ramune-bright`); the textarea itself shows no outline. **Resolution:** the board draws `#a1c6ef` permanently (source caveat 10: "does not show state"); the Plan's decision is a focus border, so it is `border-input` (`#35313b`) at rest and `#a1c6ef` on `focus-within`. The 2px keyboard ring of DESIGN.md 1.2 item 8 still applies to every button inside. |
| drag over | `data-drop="over"`: the form takes `border-primary bg-primary/5` (dashed border) and `span[data-part="drop-cue"]` reads `Drop to attach` |
| queued present | chip bar at the top (only then) |

### 1.6 Not wired

- `Edit` on a queued chip: no backend action exists (`QueuedSubmission` can only be cancelled via `onCancelQueued`); rendered only when `onEditQueued` is passed; ChatPane does not pass it.
- Chip label for `whenBusy` other than `followUp` / `steer`: `WhenBusy` has only those two, nothing else to cover.
- The board's "Thinking" and "Tools" toggles and the `Lead · durable` / `pi + durable` runner label are not part of the Composer.
- Steer / Follow-up semantics beyond the existing `whenBusy` values are not changed (board gap 12).

### 1.7 Kept Phase 2 hooks and their tests

| Hook | Pinned by |
|---|---|
| `form[aria-label="Composer"]`, `form.dataset.drop` `idle`/`over` | `Composer.test` "dragging files over the composer shows a drop state…" |
| `textarea[aria-label="Message"]`, Enter submits `{ text: trimmed-end, attachments: [], whenBusy: "followUp" }` and clears | `Composer.test` "enter submits text and clears the input"; `App.test` "submit sends a session.submit message…", "dropping an image sends it as an attachment" |
| busy: no `button[data-action="send"]`; `button[data-when-busy="steer"]` and `button[data-when-busy="followUp"]` exist; click steer submits `whenBusy: "steer"` | `Composer.test` "while busy the composer offers steer and follow-up" |
| `button[data-action="abort"]` calls `onAbort` once | `Composer.test` "abort button calls onAbort while busy" |
| `ul[aria-label="Queued"] li[data-queued="c1"]` contains the text; `button[aria-label="Cancel queued message"]` calls `onCancelQueued("c1")` | `Composer.test` "queued submissions are listed with a cancel control" |
| `input[type="file"][aria-label="Attach files"]`; `ul[aria-label="Attachments"] li[data-attachment="a.png"]` with `data-kind="image"` | `Composer.test` "file picker adds attachment chips" |
| disabled + `title` + `small[data-reason]` for unsupported steer / abort; follow-up stays enabled; nothing submitted | `Composer.test` "unsupported capabilities are disabled with their reason" |
| `[data-chat-dropzone]` on the ChatPane root | `App.test` "dropping an image sends it as an attachment" |
| attachment rules (types, limits, reasons, `buildPrompt`) | `attachments.test.ts` (unchanged) |

New tests (`composer.dango.test.tsx`): busy Enter steers; busy Alt+Enter follows up; Ctrl+C aborts only with an empty selection and a supported abort; `Stop` absent when idle; `Edit` only with `onEditQueued`; chip kind text; placeholder strings; `data-part` hooks; the attach button clicks the file input; `Stop` has no `data-action="send"`.

---

## 2. TierSelector

File `web/src/ui/composer/TierSelector.tsx`. Exports `TierSelector` and `ConfigureHandler` (KEPT).

### 2.1 Props (unchanged)

```ts
export type ConfigureHandler = (change: ConfigureChange) => void;
export function TierSelector(props: {
  capabilities: Capabilities | null; footer: FooterStats | null; models: readonly ModelChoice[] | null;
  busy: boolean; onConfigure: ConfigureHandler; onRequestModels(): void;
}): React.JSX.Element;
```

Data: tier = `footer.tier` (`Stat<string>`), model = `footer.model` (`provider/modelId`), thinking = `footer.thinking`; option lists from `capabilities.configure.tier.options`, `models[]` (`ModelChoice { model, name, thinkingLevels, contextWindow }`), the thinking levels of the current model; the current value is always in the list (`withCurrent`, KEPT). `known(stat)` is the value when `state === "known"`, else `null`. Reasons (KEPT strings): `Not attached` when `capabilities === null`; the capability's `reason` (or `Unsupported`) when unsupported; `A run is pending: the model changes when the session is idle` for model and thinking while `busy`.

### 2.2 DOM outline

The three native `<select>` elements stay (KEPT): they are the menu's controls, so the Phase 2 tests (which fire `change` on them) keep working and the menu is keyboard and screen-reader accessible without a custom listbox. They are always in the DOM; the panel is `hidden` while closed.

```
fieldset[aria-label="Session settings"][data-state="closed|open"]   KEPT aria-label; NEW data-state
     className: relative m-0 min-w-0 border-0 p-0
├─ button[type="button"][data-part="tier-pill"][aria-haspopup="true"][aria-expanded="false|true"][aria-controls="<panelId>"]   NEW
│     aria-label "Tier, model and thinking level"   (Phase 2 had no pill)
│     className: flex h-[30px] items-center gap-2 rounded-md border border-input px-2.5 text-[12px] hover:bg-row-hover
│     disabled when capabilities === null; title "Not attached" then
│  ├─ span[data-part="pill-tier"]      {tier | "unknown"}   className: font-mono font-semibold text-fg-strong   (unknown: text-muted-foreground font-normal)
│  ├─ span[data-part="pill-model"]     {model id after the last "/" | "unknown"}   title={full model string}   className: font-mono text-muted-foreground
│  ├─ span[data-part="pill-thinking"]  {thinking | "unknown"}   className: font-mono text-mitarashi-bright   (#e4bb7f)
│  └─ ChevronDownIcon 10px, stroke 2.6, text-muted-foreground, aria-hidden   (the board's `M6 9l6 6 6-6`)
└─ div[data-part="tier-menu"][id="<panelId>"][role="group"][aria-label="Tier, model and thinking level"][hidden when closed]   NEW
      className: absolute bottom-full left-0 z-20 mb-1.5 flex w-[280px] flex-col gap-2.5 rounded-lg border border-input bg-popover p-3 text-popover-foreground shadow-lg
   ├─ label[data-part="menu-row"]  span "Tier" (text-meta uppercase tracking-[.08em] text-muted-foreground)
   │    select[aria-label="Tier"]            KEPT
   │    small[data-reason]  (tier reason)   KEPT
   ├─ label[data-part="menu-row"]  span "Model"
   │    select[aria-label="Model"]           KEPT (onFocus → onRequestModels once)
   │    small[data-reason]  (model reason)  KEPT
   └─ label[data-part="menu-row"]  span "Thinking"
        select[aria-label="Thinking level"]  KEPT
        small[data-reason]  (thinking reason) KEPT
```

`<panelId>` is `useId()`. Selects: `h-8 w-full rounded-md border border-input bg-background px-2 font-mono text-[12px] text-fg-strong`; disabled ones keep their value, `disabled`, `title={reason}` (KEPT). The `<small data-reason>` of a row is `text-meta text-muted-foreground` and renders only when the reason is non-null; the order of all three `small[data-reason]` in the DOM is tier, model, thinking (`TierSelector.test` 3 reads the first one).

Option content (KEPT): Tier select: `unknown` disabled placeholder option when tier is unknown, then each tier; Model select: `models` mapped to `{ value: m.model, text: m.name }`, or the single current model (`model ?? ""`, text `model ?? "unknown"`) until `models` is loaded; Thinking select: `unknown` placeholder when unknown, then the levels.

### 2.3 Behaviour

- Click the pill: toggle open. Opening also triggers `onRequestModels()` once per mounted component (the `requested` ref is shared with the Model select's `onFocus`, which stays: the first of the two calls wins, the other is a no-op). Phase 2's contract is "request the list on first focus of the Model select" (`TierSelector.test` 2 passes `onRequestModels` as a `vi.fn()` but does not assert it, so opening is an additive trigger).
- `Escape` while the menu is open: close it and return focus to the pill. A `pointerdown` outside the fieldset closes it. Tab order inside: pill, Tier, Model, Thinking; tabbing out of the last select does not close it (the outside-pointerdown and Escape are the only closers; selecting an option does not close it either, so tier, model and thinking can be set in one visit).
- A `change` on a select calls `onConfigure` with `{ kind: "tier", tier }`, `{ kind: "model", model }` or `{ kind: "thinking", level }` (KEPT).
- The pill shows the live values; after `onConfigure` the store updates `footer` and the pill follows (no optimistic text).
- `data-state` on the fieldset mirrors `aria-expanded` (`open` / `closed`).

### 2.4 Not wired

Tier meanings (what `H3`, `S1`, `S2` mean) are not specified by the board and not shown. A "thinking" value of `null` (clear) cannot be chosen from the menu (the select lists only levels; `ConfigureChange { kind: "thinking", level: null }` has no UI), as in Phase 2.

### 2.5 Kept hooks and tests

`fieldset[aria-label="Session settings"]`; `select[aria-label="Tier"|"Model"|"Thinking level"]` with `value`, option values, `disabled`, `title`; `small[data-reason]`. Pinned by `TierSelector.test` "lists tiers and models from the session capabilities", "choosing a tier, model or thinking level calls onConfigure", "unsupported controls are disabled with a reason" (unchanged; they pass because the selects exist while the panel is `hidden`). New (`tier.dango.test.tsx`): pill text for known and unknown stats, `aria-expanded` toggle, Escape closes and refocuses the pill, `onRequestModels` called once on first open, the pill disabled when `capabilities === null`.

---

## 3. Footer

File `web/src/ui/chat/Footer.tsx`. Exports `Footer`.

### 3.1 Props

```ts
import type { FooterStats } from "../../protocol/domain.ts";
export function Footer(props: {
  footer: FooterStats | null;                              // KEPT
  cwd?: string;                                            // SessionRef.cwd
  sessionKey?: string;                                     // SessionRef.key
  contextWindow?: number;                                  // ModelChoice.contextWindow, tokens
  tokens?: { input: number; output: number; cacheRead: number; cacheWrite: number };   // no source (Not wired)
  cacheHitPercent?: number;                                // 0..100, no source (Not wired)
}): JSX.Element;
```

Mapping from the board's items to sources:

| Board item | Source | Wired |
|---|---|---|
| `~/src/cpd-webui` (cwd) | `cwd` prop ← `SessionRef.cwd?` | yes, when passed |
| `cpd-webui:chat-research` (session key) | `sessionKey` prop ← `SessionRef.key` | yes, when passed |
| `⎇ feat/chat-rpc` | `footer.branch` (`Stat<string>`) | yes |
| `↑1.2M ↓84k R11.8M W412k` | `tokens` prop | **no data** in `FooterStats` or the store: only when `tokens` passed |
| `CH 99.1%` | `cacheHitPercent` prop | **no data**: only when passed |
| `$0.698` | `footer.cost` (`Stat<number>`, USD) | yes |
| `ctx [bar] 8.8% / 1.0M (auto)` | `footer.contextPercent` (`Stat<number>`, 0..100) and `contextWindow` | percent yes; `/ 1.0M` only when `contextWindow` passed; `(auto)` never shown (no auto-compact flag exists) |
| `vertex/claude-sonnet-5-5 · high` | `footer.model`, `footer.tier`, `footer.thinking` | yes |

### 3.2 DOM outline

```
footer[aria-label="Session footer"][data-part="telemetry"]    KEPT aria-label; NEW data-part
     className: flex h-[30px] items-center gap-3.5 overflow-hidden whitespace-nowrap px-4 font-mono text-meta text-fg-dim
├─ span[data-part="cwd"]          {cwd}                        only when cwd passed       className: truncate text-fg-dim
├─ span[data-part="session-key"]  {sessionKey}                 only when passed           className: text-muted-foreground
├─ span[data-stat="branch"]       "⎇ " + <Value branch>        KEPT hook   (sr-only prefix "branch: ")
├─ span[data-part="tokens"]       "↑{input} ↓{output} R{cacheRead} W{cacheWrite}"   only when tokens passed
├─ span[data-part="cache"]        "CH {cacheHitPercent.toFixed(1)}%"                only when passed
├─ span[data-stat="cost"]         <Value cost>   "$" + value.toFixed(3)             KEPT hook; className: text-muted-foreground
├─ span[data-stat="context"][data-part="context"]   KEPT data-stat; className: flex items-center gap-1.5
│  ├─ "ctx"
│  ├─ span[role="progressbar"][aria-label="Context used"][aria-valuemin=0][aria-valuemax=100][aria-valuenow={pct}][data-part="context-bar"]
│  │      only when contextPercent is known
│  │      className: inline-block h-1 w-12 overflow-hidden rounded-[2px] bg-input
│  │   └─ span[data-part="context-fill"]   className: block h-full bg-mizu   width = clamp(pct, 0, 100)% (set through the `--w` CSS variable: `className="w-[var(--w)]"` with `style={{ "--w": pct + "%" }}`)
│  └─ <Value contextPercent>  "{pct}%" + (contextWindow ? " / " + compact(contextWindow) : "")
├─ span[aria-hidden]  className: flex-1  (spacer)
└─ span[data-part="model-line"]   className: flex gap-1.5 text-muted-foreground
   ├─ span[data-stat="tier"]      <Value tier>
   ├─ span[data-stat="model"]     <Value model>
   └─ span[data-stat="thinking"]  <Value thinking>
```

All eight `data-stat` / `data-part` hooks live inside the one `footer`. The `style` with a CSS variable is the single inline style (a measured width); the percentage itself is also in `aria-valuenow`, so tests assert `aria-valuenow`, not the style.

**Text formats** (exact):
- Percent `pct`: integer → `"42"`, non-integer → one decimal `"8.8"`; shown as `"42%"`, `"8.8%"` (Phase 2 printed the raw number; `Footer.test` expects `42%` for `42`, which still holds). The fill width uses the raw number clamped to 0 to 100. **Resolution:** the board draws a literal 9% fill against an 8.8% label (caveat 6); the fill follows the value.
- Cost: `$` + `value.toFixed(3)` (KEPT; `Footer.test` expects `$1.235` for `1.23456`).
- `compact(n)` for tokens and the window: `n < 1000` → `String(n)`; `n < 1_000_000` → `Math.round(n / 1000) + "k"`; else `(n / 1_000_000).toFixed(1) + "M"` (`84000` → `84k`, `1_200_000` → `1.2M`, `1_000_000` → `1.0M`).
- The meter has no threshold colours (the board has none): fill is always `bg-mizu` (`#67ada7`).
- Colours: whole row `text-fg-dim` (`#87808c`, allowed: the footer sits on `bg-background`); session key, cost, tier, model, thinking `text-muted-foreground` (`#aba3af`).
- Sizes: height 30px, padding `0 16px`, gap 14px, 11px (`text-meta`), mono (Roboto Mono), no wrap, overflow hidden (the board; nothing scrolls).
- The Phase 2 prefixes `model: `, `tier: `, `thinking: `, `context: `, `cost: `, `branch: ` become `<span className="sr-only">model: </span>` (still in `textContent`; visible text is board-clean).

**Unknown stats (KEPT contract):** a `Stat` with `state: "unknown"` renders `<span data-state="unknown" title={reason}>unknown</span>` in place of its value (className `text-muted-foreground`; no bar for an unknown context). With `footer === null` each of the six stats is `{ state: "unknown", reason: "not attached" }`, so exactly 6 `[data-state="unknown"]` elements exist (tier, model, thinking, context, cost, branch); the optional items (`cwd`, `sessionKey`, `tokens`, `cacheHitPercent`) render **nothing** when absent and never add an `unknown` marker (`Footer.test` "unknown stats render an explicit unknown marker" counts them). With every stat known there is no `[data-state="unknown"]`.

### 3.3 Not wired

Tokens (`↑ ↓ R W`), cache hit rate (`CH`), `(auto)` compaction flag, a `Cost` show/hide toggle (the status bar's `$` pressed state belongs to the Status Bar contract and does not hide this field), colour thresholds on the meter.

### 3.4 Kept hooks and tests

`footer[aria-label="Session footer"]` with `[data-stat="model|tier|thinking|context|cost|branch"]`, `[data-state="unknown"]` with `title` = reason, text `unknown`, 6 markers for `footer={null}` with title `not attached`. Pinned by `Footer.test` "shows model, tier, thinking, context percent, cost and branch" and "unknown stats render an explicit unknown marker" (unchanged). New (`footer.dango.test.tsx`): optional items absent then no extra nodes; present then their exact text; `progressbar` `aria-valuenow`; `compact()` outputs; `/ 1.0M` suffix.

---

## 4. AskCard

File `web/src/ui/dialogs/AskCard.tsx`. Canonical types (`DialogResponse`, `Question`, `QuestionOption`, `QuestionAnswer`, `Dialog`) are imported from `../../protocol/domain.ts`, never redeclared (`web/src/types-unique.test.ts`).

### 4.1 Props

```ts
type AskDialog = Extract<Dialog, { method: "ask_user_question" }>;
export function AskCard(props: {
  dialog: AskDialog;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
  // NEW, optional
  asker?: string;   // who asks (session key); card title "{asker} asks you"; absent: "Questions for you"
  recommended?: readonly { readonly questionIndex: number; readonly label: string }[];   // options to badge `recommended`; absent: no badge (Not wired)
}): React.JSX.Element;
```

Data mapping: `dialog.questions` (1 to 4 `Question { header ≤16 chars, question, options[2..4] { label, description, preview? }, multiSelect? }`); tab label ← `Question.header`; question text ← `Question.question`; mode line ← `multiSelect`; option label/description ← `QuestionOption`; preview ← `QuestionOption.preview` (Markdown, single-select only); count `N questions` ← `dialog.questions.length`. Answers use the unchanged `answerFor()` rules and response shape (section 4.6).

### 4.2 State

Per question `Pick { selected: string[]; custom: string; focused: string | null }` (KEPT; `EMPTY` initial) plus `current: number` (the active tab, initial `0`). A question is **answered** when `answerFor(q, i, pick) !== null` (KEPT function).

### 4.3 DOM outline (interactive)

```
form[aria-label="Questions"][data-dialog="ask_user_question"]    KEPT
     className: mx-4 mb-3 flex max-h-[min(70vh,560px)] min-h-0 flex-col overflow-hidden rounded-xl border border-status-decision bg-card
     (the board's 12px radius, `#ca88a3` border, `#211f26` card)
├─ div[data-part="title-bar"]    NEW
│    className: flex h-11 shrink-0 items-center gap-2.5 border-b border-edge bg-status-decision/10 pl-3.5 pr-2
│  ├─ span[data-part="phi"][aria-hidden]  "φ"      className: font-display text-[18px] font-bold text-status-decision-bright   (Roboto 700, #ecaec7)
│  ├─ span[data-part="title"]  "{asker} asks you" | "Questions for you"   className: font-semibold text-fg-strong
│  ├─ span[data-part="count"]  "{N} questions" | "1 question"              className: font-mono text-[11.5px] text-muted-foreground
│  ├─ span[aria-hidden] className: flex-1
│  └─ div[role="tablist"][aria-label="Question tabs"][data-part="tabs"]  (only when N > 1)   className: flex gap-1
│     └─ button[type="button"][role="tab"][data-part="tab"][data-tab="<i>"][aria-selected][aria-controls="<dialog.id>-panel-<i>"][id="<dialog.id>-tab-<i>"]
│          tabIndex = 0 for the selected tab, -1 for the others
│          className: flex h-[30px] items-center gap-[7px] rounded-[7px] border px-2.5
│                     selected: border-line bg-surface-pressed text-fg-strong   |   not selected: border-transparent text-muted-foreground
│          ├─ span[data-part="tab-mark"][data-state="done|open"]   "✓" (answered; className text-status-done) | "○" (open; className text-fg-dim); font-mono text-[11px]; aria-hidden
│          ├─ span.sr-only  "answered" | "not answered"
│          └─ span font-mono text-[12px]   {q.header}
├─ div[data-part="body"]   className: flex min-h-0 flex-1 overflow-y-auto       (one per card; contains the fieldsets)
│  └─ fieldset[data-question="<i>"][id="<dialog.id>-panel-<i>"][role="tabpanel"][aria-labelledby="<dialog.id>-tab-<i>"][hidden unless i === current]   KEPT data-question
│       className: m-0 flex min-h-0 min-w-0 flex-1 border-0 p-0     (role/ids only when N > 1; a single question has no tab roles)
│     └─ div[data-layout="side-by-side" | "single"]    KEPT hook, see 4.4
│        ├─ div[data-part="options"]    KEPT
│        │    className: flex min-w-0 flex-col gap-1.5 p-4 [border-right below]
│        │  ├─ legend  "{q.header}: {q.question}"   KEPT text (AskCard.test 1 reads `legend` textContent `H0: Question 0?`)
│        │  │     render: <legend className="mb-1 p-0 text-[16px] leading-[1.4] font-medium text-fg-strong"><span className="sr-only">{q.header}: </span>{q.question}</legend>
│        │  ├─ span[data-part="mode"]  "single-select" | "multi-select · space toggles"   className: mb-1 font-mono text-[11px] text-muted-foreground
│        │  ├─ label[data-part="option"][data-option="<label>"][data-state="picked|idle"][data-focused="true|false"] × N      (one per option, in order)
│        │  │    className: flex cursor-pointer gap-3 rounded-lg border px-3 py-2.5 hover:bg-row-hover
│        │  │               focused: border-ramune-bright bg-status-focus/10   |   not focused: border-border
│        │  │  ├─ input[type="radio"|"checkbox"][name="<dialog.id>-q<i>"][value="<label>"]   KEPT   className: sr-only peer
│        │  │  ├─ span[data-part="badge"]  "{index+1}"    className: grid size-5 shrink-0 place-items-center rounded-[4px] border border-edge font-mono text-[11px] text-muted-foreground
│        │  │  ├─ span[data-part="option-text"]   className: flex min-w-0 flex-1 flex-col gap-[3px]
│        │  │  │  ├─ span className: flex items-center gap-2
│        │  │  │  │  ├─ span className: font-semibold text-fg-strong   {o.label}
│        │  │  │  │  └─ span[data-part="recommended"]  "recommended"   (only when listed in `recommended`)   className: rounded-[4px] border border-status-decision/50 px-[5px] font-mono text-[10.5px] text-status-decision-bright
│        │  │  │  └─ span className: text-key leading-[1.45] text-muted-foreground   {o.description}
│        │  │  └─ span[data-part="mark"][aria-hidden]  className: mt-0.5 grid size-4 shrink-0 place-items-center border-[1.5px] box-border
│        │  │       border: picked → border-ramune-bright, else border-line;  radius: single → rounded-full, multi → rounded-[4px]
│        │  │       └─ span (only when picked)  className: size-2 bg-ramune-bright;  single rounded-full, multi rounded-[2px]
│        │  └─ div[data-part="custom"]    KEPT-adjacent (Phase 2 had a bare label)   className: flex items-center gap-3 rounded-lg border border-dashed border-line px-3 py-2.5
│        │     ├─ span[data-part="badge"]  "{options.length + 1}"   same badge classes
│        │     └─ input[type="text"][aria-label="Type something."][placeholder="Type something."]   KEPT   className: min-w-0 flex-1 bg-transparent text-[13px] leading-[1.45] text-fg-strong placeholder:text-fg-dim
│        └─ div[data-part="preview-pane"]       (only when data-layout = side-by-side)   className: flex min-w-0 flex-col bg-background
│           ├─ div[data-part="preview-header"]  className: flex h-[38px] items-center gap-2.5 border-b border-border px-4
│           │  ├─ span "Preview"   className: text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground
│           │  ├─ span[data-part="preview-focus"]  "{focusIndex+1} · {label}"  (empty when nothing focused)   className: font-mono text-[12px] text-fg-strong
│           │  ├─ span className: flex-1
│           │  └─ span  "follows focus · ⇥ to compare"   className: font-mono text-meta text-muted-foreground
│           └─ div[data-part="preview"]   KEPT: contains ONLY the rendered Markdown, nothing else
│                className: typeset typeset-chat min-h-0 flex-1 overflow-auto px-5 py-[18px] text-[12.5px] leading-[1.55] [&_pre]:text-[12.5px] [&_pre]:leading-[1.55]
│                children: <Markdown remarkPlugins={[remarkGfm]}>{preview}</Markdown> when the focused option has a `preview`, else nothing (empty)
└─ div[data-part="footer"]   NEW   className: flex h-[52px] shrink-0 items-center gap-3 border-t border-border px-3.5
   ├─ span[data-part="hints"]  "↑↓ move · 1–4 pick · ⇥ next question · ⏎ submit · esc cancel"   className: font-mono text-meta text-muted-foreground   (exact string, U+2191 U+2193, en dash, U+21E5, U+23CE)
   ├─ span className: flex-1
   ├─ button[type="button"][data-action="cancel"]  "Cancel"                        KEPT   className: Button variant="ghost" h-8 rounded-[7px] px-3.5 text-muted-foreground
   ├─ button[type="button"][data-action="skip"]    "Skip question"                 NEW    className: Button variant="outline" h-8 rounded-[7px] border-edge px-3.5 text-fg-strong
   └─ button[type="submit"]  "Submit {answered}/{N} answers" + kbd "⏎"               KEPT type; className: Button default h-8 rounded-[7px] bg-primary px-4 font-semibold text-primary-foreground; kbd font-mono text-[11px]
```

- `{answered}` = number of questions with `answerFor(...) !== null`; `{N}` = `dialog.questions.length`. With N = 1: `Submit 0/1 answers` / `Submit 1/1 answers` (the board always writes `answers`; no singular form).
- The submit button is always enabled (the board has none disabled; Phase 2 allows submitting with unanswered questions, which are listed in `unanswered`).
- `Cancel` is not on the board (the board cancels with `esc`); Phase 2 has the button and `AskCard.test` clicks it, so it stays as a ghost button.
- **The one hook for the two-column layout is `data-layout`, with value `side-by-side` on the element that wraps the options column and the preview pane.** `app.css` styles exactly `[data-layout="side-by-side"]` (DESIGN.md 2a replaced `[data-side-by-side]`; the attribute `data-side-by-side` is never emitted and never styled). Its rule, written in `app.css` by the shell Job: `display: grid; grid-template-columns: 470px minmax(0, 1fr); min-height: 0; width: 100%;` and `[data-layout="side-by-side"] > [data-part="options"] { border-right: 1px solid var(--border); }` (rule 1.2 item 5: the element carries no grid or width utility of its own). **Resolution:** Phase 2's `data-layout="side-by-side"` attribute was right and the CSS selector wrong (a defect, DESIGN.md 2a); the board's 470px options column is kept as the first grid track (the board's content-box 470 + padding + border is about 503px; here 470px is the whole column, border-box, matching the brief).

### 4.4 When the preview pane exists

Per question: `data-layout="side-by-side"` and the preview pane are rendered **iff** at least one of that question's options has a `preview` (the brief); otherwise the wrapper is `data-layout="single"` (no grid rule matches; `className="flex flex-col"`), has only `[data-part="options"]` and no preview pane. **Resolution:** the board's pane is unconditional (all its sample options have previews); the brief and the data model (`preview?` optional) win. `AskCard.test` "the focused option preview renders side by side" has previews on both options, so it finds the layout; before any focus the preview pane's `[data-part="preview"]` `textContent` is `""` (initial focus is none; the board's initial focus on option 1 is dropped because that test pins the empty initial state). With a multi-select question `preview` is ignored by the type (single-select only), so a multi question never gets the pane unless a malformed option carries one.

Preview content: Markdown (`react-markdown` + `remark-gfm`, Phase 2) rendered inside the `typeset typeset-chat` container; `pane.querySelector("strong")` must find `<strong>` for `**alpha preview**`. **Resolution:** the board draws previews as a `pre` with `white-space: pre`; the Phase 2 pinned behaviour is Markdown (`strong` assertion), and a fenced block in the Markdown renders as a mono `pre` at 12.5px with `overflow: auto`, so ASCII mockups look the same.

### 4.5 Focus, pick and keyboard

`focused` is the label of the option last focused or chosen (KEPT: `onFocus` of an option input and `choose()` set it). `choose(label)`: single → `{ selected: [label], custom: "", focused: label }`; multi → toggles the label in `selected` and sets `focused` (both KEPT). Typing in the custom input: `custom` set; single-select clears `selected` (KEPT: `AskCard.test` "a custom text row is appended automatically…").

Keyboard (new unless marked KEPT; all handlers on the form's `onKeyDown`; none runs for a key with `ctrlKey`, `metaKey` or `altKey`, and none runs while `e.nativeEvent.isComposing`):

| Key | Where focus is | Effect |
|---|---|---|
| `Escape` | anywhere in the form | `onRespond({ method: "ask_user_question", outcome: "cancelled" })` (KEPT; `AskCard.test` "Escape cancels", event dispatched on the form) |
| `ArrowDown` / `ArrowUp` | an option input or the custom input of the current question | `preventDefault()`; move DOM focus to the next / previous item of the ordered list [option inputs…, custom input]; no wrap (stays on the first / last); focusing an option input sets `focused` (preview follows). Does **not** pick (for radios this suppresses the native arrow-selects-and-checks behaviour). |
| `Space` | an option input | native: picks (radio) or toggles (checkbox) (hint `space toggles` for multi) |
| `1` to `4` | any element except the custom input and any `textarea` / text `input` | pick the option at that 1-based index of the current question if it exists (same as clicking it: `choose(label)`; for a multi question it toggles) and move DOM focus to it; if the digit equals `options.length + 1` focus the custom input; otherwise ignore |
| `Enter` | anywhere except a `button` | `preventDefault()`; run `submit` (the same function as the Submit button; the answers are those of all questions) |
| `Tab` (no shift) | an option input or the custom input of question `i`, with `i < N - 1` | `preventDefault()`; switch to question `i + 1` and focus its first option input. On the last question Tab is native (moves into the footer buttons). |
| `Shift+Tab` | an option or custom input of question `i`, with `i > 0` | `preventDefault()`; switch to question `i - 1` and focus its first option input. On the first question native. |
| `ArrowLeft` / `ArrowRight` | a tab button | move focus and selection to the previous / next tab (`current` changes; roving `tabIndex`) |

`Skip question` (`data-action="skip"`): resets the current question's pick to `EMPTY` and, if `current < N - 1`, moves to the next question; the skipped question is then reported in `unanswered` when submitted (same rule as Phase 2). Clicking a tab sets `current`. Switching questions never loses a pick (all `Pick`s stay in state; non-current fieldsets stay in the DOM with `hidden`, which is why `AskCard.test` can click inputs of any question).

Initial `current` is `0`; the first option input of the current question is **not** auto-focused (Phase 2 does not steal focus; a test can not assume it).

### 4.6 Response shape (unchanged, KEPT)

`submit` builds `answers` with `answerFor` for each question, collects `unanswered` indexes and responds `{ method: "ask_user_question", outcome: "answered", result: { answers, cancelled: false, ...(unanswered.length > 0 ? { unanswered } : {}) } }`. `answerFor`: multi → `{ questionIndex, question, kind: "multi", selected (in option order), answer: custom }` (null when nothing selected and no custom); single with custom text → `{ ..., kind: "custom", answer: custom }`; single with a pick → `{ ..., kind: "option", answer: label, preview? }` where `preview` is included only when that option has one. Cancel → `{ method: "ask_user_question", outcome: "cancelled" }`.

### 4.7 Read-only (`readOnly === true`)

```
section[aria-label="Questions"][data-dialog="ask_user_question"][data-readonly]     KEPT
     className: mx-4 mb-3 overflow-hidden rounded-xl border border-status-decision bg-card
├─ div[data-part="title-bar"]  same title bar without tabs: φ, title, count          (spans only)
└─ div className: flex flex-col gap-3 p-4
   ├─ ol  className: m-0 flex list-none flex-col gap-3 p-0
   │  └─ li (one per question)   KEPT   children: p "{q.header}: {q.question}" (font-medium text-fg-strong) and ul > li "{o.label} - {o.description}" (KEPT format, text-key text-muted-foreground)
   └─ p[data-part="note"]  "Answer in the session's own terminal; this view is read-only."    KEPT exact string; className: font-mono text-meta text-muted-foreground
```

No `form`, `button`, `input`, `textarea` anywhere (`AskCard.test` "read-only mode renders the questions without controls"). The title bar must therefore contain no tablist and no buttons.

### 4.8 Not wired

- `recommended` badge: neither `Question` nor `QuestionOption` has a field for it (only a label convention); rendered only from the `recommended` prop, which ChatPane does not pass.
- The board's page header (workflow name `Lead · grounding`, `Waiting on you` status) and its `Answer above, or type here to reply in your own words…` outside input: the page header belongs to the board, not the card; the outside input is the real `Composer` rendered below the card by ChatPane (its placeholder is Composer's, section 1).
- The board's `1–4 pick` does not cover a 5th key (cap is 4 options + custom row = at most the digit 5 for the custom row, handled above).
- Card title `asker` falls back to `Questions for you` when not passed.
- Notes (`QuestionAnswer.notes`, `globalNote`) have no input (as in Phase 2).

### 4.9 Kept hooks and tests

| Hook | Pinned by (`AskCard.test`) |
|---|---|
| `form[aria-label="Questions"][data-dialog="ask_user_question"]`; `fieldset[data-question="<i>"]` for 1 and 4 questions; `legend` text `H0: Question 0?`; description text; `input[value]` type radio | "renders 1 to 4 questions with options" |
| radio exclusivity, `.checked` | "single-select picks one option" |
| checkbox type, both checked | "multi-select picks many options" |
| `[data-layout="side-by-side"]`, `[data-part="options"]`, `[data-part="preview"]` empty initially, follows focus, `strong` | "the focused option preview renders side by side" |
| `input[type="text"][aria-label="Type something."]`, custom answer response, clears the pick | "a custom text row is appended automatically and submits typed text" |
| form `keydown` Escape → cancelled; `button[data-action="cancel"]` → cancelled | "Escape cancels" |
| `button[type="submit"]`, response with `unanswered: [2]`, `preview` only on options that have one | "submit sends answers per question index" |
| `section[…][data-readonly]`, `ol > li`, note text, no controls | "read-only mode renders the questions without controls" |
| `App.test` mounts the dialog path through ChatPane (`[data-dialog]`) | "a dialog message renders the dialog card…" (plan review variant) |

Test changes: none to the Phase 2 file. Because every fieldset stays in the DOM, `fieldset[data-question="2"]` and its inputs are queryable even when hidden. New (`ask.dango.test.tsx`): tabs (marks, `aria-selected`, switching), submit label `Submit N/M answers`, Skip, hint string, digit and arrow keys, Tab/Shift+Tab question switching and its release on the first/last question, `data-layout` value `single` without previews, `recommended` badge only from the prop, `asker` title, `Enter` submits, no `data-side-by-side` attribute anywhere.

---

## 5. PlanReviewCard

File `web/src/ui/dialogs/PlanReviewCard.tsx`.

### 5.1 What the data can feed

The dialog carries only `{ id, interactive, title, summary }` (`Dialog` with `method: "cpd_plan_review"`, `web/src/protocol/domain.ts`; the response is `approve`, `changes` with `feedback`, `reject` or `cancelled`). Nothing in `web/src/protocol/**` or the store carries a revision, workflow list, job DAG, overlap, risks, scope or review statistics. Therefore, from existing data:

| Design element | Fed by | Status |
|---|---|---|
| Title `Plan review · {title}` | `dialog.title` | wired |
| Outline section body | `dialog.summary` (Markdown) | wired (the whole body today) |
| Decision bar: `Reject plan`, `Request changes`, `Approve & lock`, feedback textarea | `onRespond` | wired |
| Subtitle `cpd-context:… · r2 · 2 workflows · 9 jobs`, revision `r2`, previous revision verdict, `Diff vs r1`, section nav with counts, workflow cards and job DAG, overlap alert, risks, out of scope, review statistics | optional `review` prop | rendered only when the matching field is passed; ChatPane passes nothing, so **none** appears today (listed under Not wired) |

### 5.2 Props

```ts
import type { Dialog, DialogResponse } from "../../protocol/domain.ts";

// Local to PlanReviewCard.tsx, not a protocol type (no source for any field exists today).
export interface PlanReviewExtras {
  readonly subtitle?: string;            // header line 2, e.g. "cpd-context:exp-write-curation · r2 · 2 workflows · 9 jobs"
  readonly revision?: string;            // current revision label, e.g. "r2" (shown only with `previous`)
  readonly previous?: { readonly revision: string; readonly verdict: string; readonly note?: string };   // "r1" "changes requested · “split curation from TTL”"
  readonly onDiff?: () => void;          // when passed (and not readOnly): the "Diff vs {previous.revision}" button
  readonly workflows?: readonly {
    readonly name: string; readonly route: string;     // "write-path", "cpd → main"
    readonly status: string;                           // "draft"
    readonly flag?: string;                            // "new in r2"
    readonly summary: string;                          // "5 jobs · TDD"
    readonly jobs: readonly { readonly name: string; readonly role: string; readonly tier: string; readonly overlap?: boolean }[];
  }[];
  readonly overlaps?: readonly string[]; // one alert per string
  readonly risks?: readonly string[];
  readonly outOfScope?: readonly string[];
  readonly stats?: string;               // "240 approved · 61 changes requested · 0 rejected, all-time"
}

export function PlanReviewCard(props: {
  dialog: Extract<Dialog, { method: "cpd_plan_review" }>;
  readOnly: boolean;
  onRespond(response: DialogResponse): void;
  review?: PlanReviewExtras;             // NEW, optional
}): React.JSX.Element;
```

`PlanReviewExtras` is declared once, here, in `PlanReviewCard.tsx` (it is not one of the 11 canonical interfaces of `types-unique.test.ts`).

### 5.3 DOM outline

```
section[aria-label="Plan review"][data-dialog="cpd_plan_review"]    KEPT
     className: mx-4 mb-3 flex max-h-[min(80vh,720px)] min-h-0 flex-col overflow-hidden rounded-xl border border-status-decision bg-card
├─ header[data-part="header"]    NEW   className: flex h-14 shrink-0 items-center gap-3 border-b border-edge bg-status-decision/10 px-4
│  ├─ span[data-part="phi"][aria-hidden] "φ"   className: font-display text-[20px] font-bold text-status-decision-bright
│  ├─ div className: flex min-w-0 flex-col
│  │  ├─ div className: flex items-baseline gap-1.5
│  │  │  ├─ span "Plan review ·"   className: text-[14px] font-semibold text-fg-strong
│  │  │  └─ h3  {dialog.title}     KEPT: textContent is exactly the title (PlanReviewCard.test 1); className: m-0 text-[14px] font-semibold text-fg-strong
│  │  └─ span[data-part="subtitle"]  {review.subtitle}   only when passed   className: truncate font-mono text-[11.5px] text-muted-foreground
│  ├─ span className: flex-1
│  ├─ span[data-part="previous"]   only when review.previous   className: flex gap-2 text-[12px] text-muted-foreground
│  │  ├─ span font-mono text-mitarashi-bright  {previous.revision}
│  │  └─ span  "{previous.verdict}" + (note ? " · “{note}”" : "")
│  └─ button[type="button"][data-action="diff"]  "Diff vs {previous.revision}"   only when review.onDiff && review.previous && !readOnly
│       className: Button variant="outline" h-7 rounded-md border-edge px-2.5 text-[12px] text-fg-strong
├─ div[data-part="body-row"]  className: flex min-h-0 flex-1
│  ├─ nav[aria-label="Plan sections"][data-part="outline-nav"]    only when at least one of workflows / overlaps / risks / outOfScope is passed and non-empty
│  │    className: flex w-[180px] shrink-0 flex-col gap-0.5 border-r border-border px-2 py-3.5 text-key
│  │  └─ a[href="#<dialog.id>-<section>"][data-section="<outline|workflows|jobs|overlaps|risks|oos>"][aria-current="true" on the active one]
│  │       className: flex justify-between rounded-md px-2.5 py-1.5; active: bg-surface-pressed text-fg-strong; else text-muted-foreground hover:text-fg-strong
│  │       items in order: "Outline" (always), "Workflows" + count (workflows.length), "Jobs" + count (sum of jobs), "Overlaps" + count (text-mitarashi-bright count), "Risks", "Out of scope"; an item exists only when its data is non-empty
│  │       count: span font-mono; onClick → preventDefault() + scrollIntoView of the section element + set the active item
│  └─ div[data-part="body"]   className: flex min-w-0 flex-1 flex-col gap-5 overflow-y-auto px-[22px] py-[18px]
│     ├─ div[data-part="outline"][id="<dialog.id>-outline"]
│     │    ├─ h4 "Outline"   (the section label style: text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground mb-2) — rendered only when other sections exist
│     │    └─ div className: typeset typeset-chat max-w-[720px] text-[14px] text-fg-strong
│     │         <Markdown remarkPlugins={[remarkGfm]}>{dialog.summary}</Markdown>        KEPT (test 1 reads `strong` = "bold")
│     ├─ div[data-part="workflows"][id="<dialog.id>-workflows"]   only when review.workflows non-empty   className: flex flex-col gap-2.5
│     │  ├─ h4 "Workflows"
│     │  └─ article[data-part="workflow"][data-workflow="<name>"] × n    className: flex flex-col gap-2.5 rounded-[9px] border border-border bg-background px-3 py-2.5
│     │     ├─ div className: flex items-center gap-2.5
│     │     │  ├─ span font-mono font-semibold text-fg-strong {name}
│     │     │  ├─ span font-mono text-[11.5px] text-muted-foreground {route}
│     │     │  ├─ span[data-part="status-pill"] {status}  className: rounded-[4px] bg-status-decision/[0.14] px-1.5 py-px font-mono text-[11px] text-status-decision-bright
│     │     │  ├─ span font-mono text-[11px] text-status-done-bright {flag}   only when flag
│     │     │  ├─ span className: flex-1
│     │     │  └─ span font-mono text-[11.5px] text-muted-foreground {summary}
│     │     └─ div[data-part="dag"] className: flex flex-wrap items-center gap-1.5 font-mono text-[12px]
│     │        └─ per job: span[data-job="<name>"] className: flex items-center gap-1.5 rounded-[5px] border border-dashed border-status-decision px-2 py-[3px]
│     │              children: span text-fg-strong {name}, span text-muted-foreground "{role} · {tier}", and (overlap) span text-mitarashi-bright "overlap"
│     │           between jobs: span[aria-hidden] "→" text-line
│     ├─ div[data-part="columns"]  className: grid grid-cols-3 gap-5      (only the columns that have data; the grid still has 3 tracks)
│     │  ├─ div[data-part="overlaps"][id="<dialog.id>-overlaps"]   only when review.overlaps non-empty
│     │  │    ├─ h4 "Overlaps"
│     │  │    └─ div[data-part="overlap"][role="note"] × n  className: flex gap-2 rounded-lg border border-status-landing/50 bg-status-landing/[0.07] px-3 py-2.5 text-key
│     │  │         ├─ span[data-tone="landing"][data-glyph="landing"][aria-hidden] className: glyphClass.landing mt-1.5   (the Mitarashi download-arrow shape, `glyph glyph-landing`)
│     │  │         └─ span.sr-only "overlap: "  then the string
│     │  ├─ div[data-part="risks"][id="<dialog.id>-risks"]   only when non-empty:  h4 "Risks", ul className: m-0 flex list-disc flex-col gap-1.5 pl-4 text-key
│     │  └─ div[data-part="oos"][id="<dialog.id>-oos"]       only when non-empty:  h4 "Out of scope", same ul
└─ footer[data-part="decision-bar"]    NEW — only when readOnly === false   className: flex shrink-0 flex-col gap-2.5 border-t border-edge bg-row-hover px-4 py-3
   ├─ div[data-part="feedback"]   only while `asking` (Phase 2: opened by Request changes)   className: flex flex-col gap-2 rounded-lg border border-input bg-background px-2.5 py-2
   │  ├─ textarea[aria-label="Feedback"][rows=2][placeholder="Feedback for the Lead — required to request changes"]   KEPT aria-label   className: w-full resize-none bg-transparent text-[13px] text-fg-strong placeholder:text-fg-dim   (autofocus when it appears)
   │  └─ div className: flex justify-end
   │     └─ button[type="button"][data-action="send-feedback"]  "Send feedback"   KEPT   disabled while feedback.trim() === ""   className: Button variant="outline" h-8 border-line px-3.5 text-fg-strong
   └─ div[data-part="actions"]   className: flex items-center gap-2
      ├─ button[type="button"][data-action="reject"]  "Reject plan"   KEPT   className: h-[34px] rounded-[7px] px-3 text-[13px] text-status-blocked-bright hover:bg-status-blocked/10
      ├─ span className: flex-1
      ├─ span[data-part="stats"]  {review.stats}   only when passed   className: mr-1.5 font-mono text-meta text-muted-foreground
      ├─ button[type="button"][data-action="request-changes"][aria-expanded="false|true"]  "Request changes"   KEPT   className: Button variant="outline" h-[34px] rounded-[7px] border-line px-3.5 text-[13px] text-fg-strong
      └─ button[type="button"][data-action="approve"]  "Approve & lock" + kbd "⌘⏎"   KEPT   className: h-[34px] rounded-[7px] bg-primary px-[18px] text-[13px] font-semibold text-primary-foreground; kbd font-mono text-[11px]
```

- **Resolution (feedback box):** the board shows the feedback textarea always; Phase 2 shows it only after `Request changes` (`PlanReviewCard.test` 3 asserts `querySelector("textarea")` is `null` first). Phase 2 wins: the box appears on `Request changes` (`asking`), with focus moved into it.
- **Resolution (labels):** Phase 2 buttons read `Approve`, `Request changes`, `Reject`; the board's `Approve & lock`, `Request changes`, `Reject plan` win (tests select by `data-action`).
- **Resolution (approve shortcut):** the board shows `⌘⏎` on the button and `a approve` in the nav hints; the button's `⌘⏎` wins and the nav key hints are not rendered.
- Section order and the `h3` are fixed above; `dialog.id` is used in element ids so two cards can never collide.
- The card scrolls inside `body` (`overflow-y-auto`), header and decision bar stay.
- "Out of scope" `h4` label uses the sentence case string; the section label style is an uppercase 11px label (the board's `h2` rule), written with utilities, not an `h2` (the heading levels under the card's `h3` are `h4`).

### 5.4 Behaviour

| Action | Effect |
|---|---|
| click `Approve & lock` | `onRespond({ method: "cpd_plan_review", outcome: "approve" })` (KEPT) |
| click `Reject plan` | `onRespond({ method: "cpd_plan_review", outcome: "reject" })` (KEPT) |
| click `Request changes` | `asking = true`: the feedback box appears, the textarea gets focus, the button gets `aria-expanded="true"`. A second click keeps it open (no toggle). Nothing is sent. (KEPT) |
| `Send feedback` (disabled while `feedback.trim() === ""`) | `onRespond({ method: "cpd_plan_review", outcome: "changes", feedback: feedback.trim() })` (KEPT; feedback trimmed, non-empty) |
| `Cmd+Enter` or `Ctrl+Enter` (`e.metaKey \|\| e.ctrlKey`, key `Enter`) with focus anywhere in the card **except the feedback textarea** | `preventDefault()`; same as clicking `Approve & lock`. In the textarea it does nothing (so typing feedback cannot approve by accident). No-op when `readOnly`. |
| click a nav link | scroll that section into view, mark it `aria-current="true"` |
| click `Diff vs {previous.revision}` | `review.onDiff()` |

No shortcut exists for `Reject plan` or `Request changes`; the board's `c`, `j/k` and `[ ]` hints are not built. `outcome: "cancelled"` (in the response union) has no control, as in Phase 2.

States: interactive, asking, readonly. There is no loading state and no disabled `Approve` (the response is always valid).

### 5.5 Read-only (`readOnly === true`)

Same card without the `footer[data-part="decision-bar"]` and without the `Diff` button: header, nav (if any), body (summary and optional sections). No `button`, `textarea` or `input` anywhere (`PlanReviewCard.test` "read-only mode renders the review without controls"; nav entries are `a` elements, which that test does not query, and exist only with `review` data). A read-only card from ChatPane has no `review`, so it is the title and the summary only.

### 5.6 Not wired

Revision label and the previous revision verdict, `Diff vs r1`, the section nav and its counts, the workflow cards and job DAG, the overlap alert, the risks and out-of-scope lists, the review statistics line (`240 approved · …`), the `a` / `c` / `j k` / `[ ]` key hints, the `Reject plan` shortcut, and the subtitle (`cpd-context:… · r2 · …`): each renders only when its `review` field is passed and ChatPane passes no `review`. Reason: `Dialog` for `cpd_plan_review` is `{ title, summary }` and nothing in `web/src/protocol/**` or `web/src/state/store.ts` carries them (A6; no backend change). Workflow/Job counts for the real plan live in the cockpit plan view (`DESIGN-panes.md`), not here.

### 5.7 Kept hooks and tests

| Hook | Pinned by (`PlanReviewCard.test`) |
|---|---|
| `section[aria-label="Plan review"][data-dialog="cpd_plan_review"]`; `h3` text exactly the title; summary rendered as Markdown (`strong` text `bold`) | "shows the plan title and summary" |
| `button[data-action="approve"]` → `{ method, outcome: "approve" }` | "Approve responds approve" |
| `button[data-action="request-changes"]` reveals `textarea[aria-label="Feedback"]` (none before); `button[data-action="send-feedback"]` disabled for empty or whitespace; response `{ outcome: "changes", feedback: "tighten scope" }` (trimmed) | "Request changes requires feedback text" |
| `button[data-action="reject"]` → `{ outcome: "reject" }` | "Reject responds reject" |
| read-only: title and summary present, no `button, textarea, input` | "read-only mode renders the review without controls" |
| `[data-dialog="cpd_plan_review"]` contains the title; `[data-action="approve"]` click sends `dialog.respond` | `App.test` "a dialog message renders the dialog card and answering sends dialog.respond" |

Test changes: none to the Phase 2 file. New (`planreview.dango.test.tsx`): the `Plan review ·` prefix outside the `h3`; `Approve & lock` / `Reject plan` copy; `⌘⏎` approves from outside the textarea and not from inside; no `nav`, `article`, `[data-part="overlaps"]`, `[data-part="stats"]` without `review`; each optional section renders only when its field is passed; `Diff` button only with `onDiff` and `previous` and not read-only; `dialog.id` in the ids; focus lands in the textarea after `Request changes`.

---

## 6. Summary of hooks added (new, all in this file)

Composer: `data-part` `queued-bar`, `queued-kind`, `queued-text`, `attachment-bar`, `action-row`, `send-split`, `reasons`; `data-action` `attach`, `edit-queued`; `data-when-busy` on `li[data-queued]`. TierSelector: `data-state` on the fieldset; `data-part` `tier-pill`, `pill-tier`, `pill-model`, `pill-thinking`, `tier-menu`, `menu-row`. Footer: `data-part` `telemetry`, `cwd`, `session-key`, `tokens`, `cache`, `context`, `context-bar`, `context-fill`, `model-line`. AskCard: `data-part` `title-bar`, `phi`, `title`, `count`, `tabs`, `tab`, `tab-mark`, `body`, `mode`, `option`, `badge`, `option-text`, `recommended`, `mark`, `custom`, `preview-pane`, `preview-header`, `preview-focus`, `footer`, `hints`, `note`; `data-tab`, `data-option`, `data-state`, `data-focused`, `data-action="skip"`; `data-layout` values `side-by-side` and `single`. PlanReviewCard: `data-part` `header`, `phi`, `subtitle`, `previous`, `body-row`, `outline-nav`, `body`, `outline`, `workflows`, `workflow`, `status-pill`, `dag`, `columns`, `overlaps`, `overlap`, `risks`, `oos`, `decision-bar`, `feedback`, `actions`, `stats`; `data-section`, `data-workflow`, `data-job`, `data-action="diff"`.
