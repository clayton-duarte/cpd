# Dango v3 + shadcn/ui: design contract (composer, tier selector, footer, AskCard, PlanReviewCard)

Scope: Plan `webui-phase3-good`, Workflow `ui`, package `web/`, amended by the approved preview (pro-d54 to pro-d58, pro-d60, pro-d61). Companion of `web/src/ui/DESIGN.md` (**read it first**: hook conventions 1.1, styling rules 1.2, tokens 2b, shadcn parts 2f, utilities and the prompt border 2h). Everything defined there is reused by reference and not restated. Tests are written from this file alone by one Worker, the implementation by another.

**What changed (pro-d54 to pro-d58).** The composer is an AI Elements `PromptInput` layout (header, body with the textarea, footer with tools). Its only send path is **Send**, which submits a follow-up; the Steer, Follow-up and Stop buttons are removed (pro-d57). The prompt border pulses while the session is busy (pro-d58). The session telemetry row of the Footer is removed; the Footer keeps only the context HoverCard (pro-d56, pro-d57). The Tier, Model and Effort selects are shadcn dropdown menus. The AskCard is a multi-step Questionnaire. The PlanReviewCard is a Confirmation Item.

---

## 0. ChatPane wiring (the one place the three pieces meet)

`web/src/ui/ChatPane.tsx` keeps its props `{ view: SessionView; connection: Connection; reader: FileReaderBoundary }` and the `div[data-chat-dropzone]` with its `onDragOver` (`preventDefault` when files are dragged) and `onDrop` (`preventDefault`, then `composer.current?.addFiles(filesFromDrop(e.dataTransfer))`), pinned by `App.test` "dropping an image sends it as an attachment".

Child order inside the dropzone, top to bottom: `ChatHeader` (DESIGN-chat 1), `Transcript` (DESIGN-chat 2), the dialog card (`AskCard` for `ask_user_question`, else `PlanReviewCard`, when `view.dialog !== null`; `readOnly = !dialog.interactive`), the `Footer` context card (3), and the `Composer` (1). The main `main[aria-label="Chat"]` is `flex flex-col`; `Transcript` is the only `flex-1 min-h-0` child.

What ChatPane passes (source field → prop):

| Prop | Value | Source |
|---|---|---|
| `Composer.busy` | `view.busy \|\| view.pending.some((p) => p.state === "sending")` (DESIGN-chat 2.4 and DESIGN.md 2h) | `view.busy`, `view.pending` |
| `Composer.settings` | `<TierSelector capabilities footer models busy onConfigure onRequestModels />` | `view.capabilities`, `view.footer`, `view.models`, `view.busy`, `connection.configure`, `connection.listModels` |
| `Composer.onSubmit` | `({ text, attachments, whenBusy }) => { const { text: prompt, images } = buildPrompt(text, attachments); connection.submit({ text: prompt, images, whenBusy }) }` | `attachments.ts` |
| `Composer.onCancelQueued` | `(id) => connection.abortSubmission(id)` | `connection` |
| `Footer.footer` | `view.footer` | `SessionView.footer` (the Footer is mounted, 3) |
| `AskCard.onRespond`, `PlanReviewCard.onRespond` | `(r) => connection.respond(dialog.id, r)` | `connection` |
| `ChatHeader`, `Transcript` | DESIGN-chat 1 and 2 | |

`Composer.onAbort` is still declared (`connection.abort`) but the composer renders no Stop button (1.6). `label`, `onEditQueued`, `Footer.cwd`, `Footer.sessionKey`, `Footer.tokens`, `Footer.cacheHitPercent` and the Footer's `contextWindow` are not passed: no control or row reads them.

---

## 1. Composer

File `web/src/ui/composer/Composer.tsx`. Exports `Composer` and `ComposerHandle` (`{ addFiles(files: readonly FileLike[]): Promise<void> }`, KEPT). Built from the shadcn `InputGroup` parts (`web/src/ui/components/input-group.tsx`, DESIGN.md 2f), in the AI Elements `PromptInput` slot layout: header, body, footer. There is no separate `PromptInput` module.

### 1.1 Props

```ts
export function Composer(props: {
  // KEPT
  capabilities: Capabilities | null; busy: boolean; queued: readonly QueuedSubmission[];
  reader: FileReaderBoundary;
  onSubmit(input: { text: string; attachments: readonly Attachment[]; whenBusy: WhenBusy }): void;
  onAbort(): void;                       // KEPT in the type; no control calls it (1.6)
  onCancelQueued(clientSubmissionId: string): void;
  ref?: Ref<ComposerHandle>;
  settings?: ReactNode;                  // the TierSelector (2), in the footer
}): React.JSX.Element;
```

`whenBusy` is always `"followUp"` from this component (1.4). Removed: `label`, `onEditQueued` (no caller; 1.6).

Data mapping: queued chips come from `props.queued` (`QueuedSubmission { clientSubmissionId, submissionId?, text, whenBusy }`). Attachments are local state, built by `addFiles()` of `attachments.ts`, unchanged: images png, jpeg, gif and webp up to `IMAGE_MAX_COUNT` and `IMAGE_MAX_TOTAL_BASE64`; text files inline up to `TEXT_INLINE_MAX_BYTES`; binary rejected with the reasons of `RejectionReason`. `attachments.ts` and `attachments.test.ts` are not changed.

### 1.2 DOM outline

```
div[data-part="composer"]                                       mx-3 mb-2.5 flex shrink-0 flex-col gap-2
├─ input[type="file"][multiple][aria-label="Attach files"][hidden]    (the hidden picker; its change calls addFiles)
├─ ul[role="alert"][data-part="rejections"]                      only when rejections exist; one li[data-rejection="<reason>"] per rejection: "{name}: {message}"
└─ InputGroup[data-busy][class="rounded-3xl bg-card" + "prompt-border-pulse" while busy]   (DESIGN.md 2h)
    │
    ├─ InputGroupAddon[align="block-start"][class="flex-col items-stretch gap-2 px-3 pt-3 pb-0"]   HEADER   (only when queued or attachments exist)
    │   ├─ ul[aria-label="Queued submissions"][data-part="queue"]   only when queued exist
    │   │   └─ li[data-queued-chip="<clientSubmissionId>"]   inline-flex max-w-[360px] items-center gap-1.5 rounded-md border bg-background px-2 py-0.5 text-xs
    │   │       ├─ span.font-mono.text-muted-foreground  "queued"
    │   │       ├─ span[data-part="text"].truncate  {q.text}
    │   │       └─ Button[variant="ghost"][size="icon-xs"][data-action="cancel-queued"][aria-label="Cancel queued: {q.text}"] "×"
    │   │           (only when capabilities.abortSubmission and q.submissionId !== undefined)
    │   └─ AttachmentGroup[aria-label="Staged attachments"][data-part="attachments"]   only when attachments exist
    │       └─ Attachment[size="sm"][data-attachment="<kind>"] ×attachments
    │           ├─ AttachmentMedia[data-part="thumb"][variant="image" | "icon"] → img (data URI, alt "") or FileTextIcon
    │           ├─ AttachmentContent › AttachmentTitle[data-part="name"].font-mono  {a.name}
    │           └─ AttachmentActions › AttachmentAction[aria-label="Remove {a.name}"][data-action="remove-attachment"] XIcon
    │
    ├─ InputGroupTextarea[aria-label="Message"][rows=1]        BODY   min-h-10 px-4 py-3 text-sm leading-relaxed
    │   placeholder "Ask anything… (Enter to send, Shift+Enter for newline)"   (1.3)
    │
    └─ InputGroupAddon[align="block-end"][class="justify-between px-3 pb-2.5 pt-0"]   FOOTER
        ├─ div.flex.items-center.gap-1.5                       tools, left
        │   └─ InputGroupButton[type="button"][variant="outline"][size="icon-sm"][class="rounded-full"][aria-label="Attach files"]  PlusIcon   (opens the hidden picker)
        └─ div[data-part="actions"].flex.items-center.gap-1.5    tools, right
            ├─ {settings}        the TierSelector (2)
            └─ InputGroupButton[type="button"][variant="default"][size="icon-sm"][data-action="send"][aria-label="Send"][class="rounded-full"]   ArrowUpIcon
                  disabled when !canSubmit; text "Send" in sr-only
```

- **Drop cue (kept from the Phase 2 composer; the approved preview lacks it).** While files are dragged over the Composer, `data-drop="over"` is set on the `InputGroup` and a `span[data-part="drop-cue"]` reads `Drop to attach` (`font-mono text-meta text-primary`, at the top of the InputGroup, before the body). `onDragLeave` and the drop reset it. The **adding** of dropped files happens once, in the chat dropzone's `onDrop` (0); the Composer's drop handler does not add files.
- **Resolution:** the Phase 2 `form[aria-label="Composer"]` and its `data-part="send-split"` group are gone (the form is not a `form`; the Send button is not a submit button, and Enter is handled by the textarea, 1.4).
- **Resolution:** the Phase 2 `button[data-action="attach"]` with `aria-label="Choose files to attach"` is the `Attach files` button above.

### 1.3 Textarea

- `rows={1}` minimum; auto-grows with its content up to a maximum of 192 px (about 8 lines), then scrolls (`overflow-y: auto`). A layout effect on `text` sets `el.style.height = "auto"` and then `el.style.height = Math.min(el.scrollHeight, 192) + "px"` (an imperative DOM write, the only inline style of the component: DESIGN.md 1.2 item 1). After a submit the height returns to the minimum. jsdom reports `scrollHeight` 0; tests do not assert the pixel height.
- Placeholder: `Ask anything… (Enter to send, Shift+Enter for newline)` (U+2026), the same whether busy or idle. **Resolution:** the Phase 2 busy placeholders (`Steer …, or queue a follow-up…`) are removed with the Steer button.
- `aria-label="Message"` is fixed (KEPT).
- Disabled when `capabilities === null` (KEPT; DESIGN.md `button:disabled, textarea:disabled` rule gives it `cursor: not-allowed`).

### 1.4 Keyboard and submit behaviour

`canSend` = `text.trim() !== "" || attachments.length > 0` (KEPT). `canSubmit` = `capabilities !== null && canSend`. `send()` is a no-op when `!canSubmit`; otherwise it calls `onSubmit({ text, attachments, whenBusy: "followUp" })` and clears text, attachments and rejections (KEPT).

| Key / action (focus in the textarea unless said) | Result |
|---|---|
| `Enter` (no modifier, not composing) | `preventDefault()`, `send()` |
| `Shift+Enter` | newline (default; no `preventDefault`) |
| `Enter` while `e.nativeEvent.isComposing` | ignored (IME) |
| paste with files | `preventDefault()`, add the files (`filesFromClipboard`) |
| click **Send** | `send()` (the button is disabled while `!canSubmit`) |

- **Send is never disabled by `busy`.** It is disabled only when there is nothing to send, or when `capabilities` is `null`. Busy does not change what Send does: it submits a follow-up (`whenBusy: "followUp"`). **Resolution:** pro-d57 says "Send always enabled"; the approved preview disables Send for an empty composer, and this contract keeps that.
- There is no Alt+Enter, no Ctrl+C abort and no busy-dependent Enter. The Phase 2 `Steer` on busy Enter is removed (pro-d57).

### 1.5 States

| State | Rendering |
|---|---|
| idle | footer: attach, settings, Send (disabled when empty) |
| busy (`view.busy` or a send pending) | the same footer; the `InputGroup` carries `prompt-border-pulse` (DESIGN.md 2h); no Stop and no Steer or Follow-up control exists |
| `capabilities === null` | textarea, attach button and file picker `disabled`; Send `disabled`; the placeholder is the same string |
| queued present | queue chips in the header (1.2) |
| attachments staged | attachment chips in the header |
| rejections | the `role="alert"` list above the InputGroup |
| focus | the `InputGroup` part's own focus styles (unverified beyond the part); the DESIGN.md 1.2 item 8 ring applies to every button inside |
| drag over | `data-drop="over"` and the drop cue (1.2) |

### 1.6 Not wired and removed

- **Removed (pro-d57):** the Steer, Follow-up and Stop buttons, the `Steer` / `Follow-up` / `Stop` copy, the `⏎`, `⌥⏎` and `⌃C` hints, `data-when-busy` on any control, the `send-split` group, the busy-only action row, and the `small[data-reason]` reason lines under the actions.
- `Edit` on a queued chip: no backend action exists; no control.
- The board's "Thinking" and "Tools" toggles and the `Lead · durable` / `pi + durable` runner label are not part of the Composer.
- `label` in the placeholder: removed with the Steer copy.

### 1.7 Kept hooks and tests

| Hook | Pinned by |
|---|---|
| `div[data-part="composer"]`; `textarea[aria-label="Message"]` | `Composer.test` (updated) |
| `textarea` Enter submits `{ text: trimmed-end, attachments: [], whenBusy: "followUp" }` and clears | `Composer.test` "enter submits text and clears the input" |
| `button[data-action="send"]` enabled with text, disabled empty; click submits `followUp` | `Composer.test` (new) |
| `input[type="file"][aria-label="Attach files"]`; `button[aria-label="Attach files"]` opens it | `Composer.test` "file picker adds attachment chips" (updated: the button) |
| `[data-attachment="<kind>"]` chips with `button[data-action="remove-attachment"]` | `Composer.test` (updated) |
| `ul[aria-label="Queued submissions"] li[data-queued-chip]` text; `button[data-action="cancel-queued"]` calls `onCancelQueued(id)` | `Composer.test` "queued submissions are listed with a cancel control" (updated) |
| `InputGroup` with `prompt-border-pulse` while `busy`, not while idle | `Composer.test` (new) |
| `[data-drop="over"]` and `span[data-part="drop-cue"]` `Drop to attach` | `Composer.test` "dragging files over the composer shows a drop state" (updated) |
| `[data-chat-dropzone]` on the ChatPane root; one add per drop | `App.test` "dropping an image sends it as an attachment" |
| attachment rules (types, limits, reasons, `buildPrompt`) | `attachments.test.ts` (unchanged) |

---

## 2. TierSelector

File `web/src/ui/composer/TierSelector.tsx`. Three shadcn dropdown menus with radio items, in the composer footer (1.2 `settings`). This replaces the Phase 2 native selects (pro-d55: the model selector is a vanilla shadcn part).

### 2.1 Props (unchanged)

```ts
export function TierSelector(props: {
  capabilities: Capabilities | null;
  footer: FooterStats | null;
  models: readonly ModelChoice[] | null;
  busy: boolean;
  onConfigure(change: ConfigureChange): void;   // { kind: "tier", tier } | { kind: "model", model } | { kind: "thinking", level }
  onRequestModels(): void;
}): React.JSX.Element;
```

### 2.2 DOM outline

```
div[data-part="tier-selector"]   flex flex-wrap items-center gap-1
├─ DropdownMenu
│   ├─ DropdownMenuTrigger asChild → InputGroupButton[size="sm"][aria-label="Tier"][class="font-mono text-xs"]
│   │     disabled when the Tier capability is unsupported or there are no tiers; title = the reason
│   │     text: the current tier, or "Tier"; ChevronDownIcon
│   └─ DropdownMenuContent[align="start"] › DropdownMenuRadioGroup[value = current tier]
│         DropdownMenuGroup › DropdownMenuLabel "Tier" · DropdownMenuRadioItem per tier[value][class="font-mono text-xs"]
├─ DropdownMenu   (opens → onRequestModels() once per mount)
│   ├─ InputGroupButton[size="sm"][aria-label="Model"][class="font-mono text-xs"]
│   │     disabled when the model capability is unsupported; text: the current model's name, else "Model"
│   └─ DropdownMenuContent › DropdownMenuRadioGroup[value = current model]
│         DropdownMenuLabel "Model" · DropdownMenuRadioItem per model (text: the model's name)
└─ DropdownMenu
    ├─ InputGroupButton[size="sm"][aria-label="Thinking level"][class="font-mono text-xs"]
    │     disabled when the thinking capability is unsupported or there are no levels; text: the current level, else "Effort"
    └─ DropdownMenuContent › DropdownMenuRadioGroup[value = current level]
          DropdownMenuLabel "Effort" · DropdownMenuRadioItem per level
```

### 2.3 Behaviour

- Choosing a radio item calls `onConfigure` with `{ kind: "tier", tier }`, `{ kind: "model", model }` or `{ kind: "thinking", level }` (KEPT). The menu closes on choice (dropdown default).
- The current value is shown on the trigger from `footer` (`known` values only). After `onConfigure` the store updates `footer` and the trigger follows: no optimistic text.
- The current value is always an option: if it is not in the list it is added to it (`withCurrent`).
- `Model` lists the `models` (name as text, id as value); with no models, only the current model. The first open calls `onRequestModels()` once.
- When `busy` and the model or thinking capability is not allowed while busy, the trigger is disabled with the reason `A run is pending: the model changes when the session is idle` as its `title`.
- No Phase 2 "Tier" pill with a fieldset or Escape handler: the Radix menus own focus and Escape.

### 2.4 Not wired

- `recommended` and `tokens`: no control.
- The Lead · durable runner label.

### 2.5 Kept hooks and tests

`div[data-part="tier-selector"]`; `aria-label` `Tier`, `Model`, `Thinking level` on the triggers; `disabled` and `title` reasons. `TierSelector.test` (updated to the menus): the trigger text, the radio items, `onConfigure` payloads, the disabled reasons.

---

## 3. Footer (context HoverCard only)

File `web/src/ui/chat/Footer.tsx`. Pro-d56 names the Footer context as a HoverCard with a Progress bar. Pro-d57 removes the session telemetry row. What remains is one button.

### 3.1 Props

```ts
export function Footer(props: { footer: FooterStats | null; contextWindow?: number }): JSX.Element;
```

`contextWindow` is optional: when it is absent, the card shows the percent alone. No other prop is read.

### 3.2 DOM outline

```
footer[aria-label="Session footer"][data-part="telemetry"]   flex items-center justify-between gap-3 px-2.5 pb-1.5 text-xs text-muted-foreground   (the preview's name; it holds only the context card)
└─ HoverCard[openDelay=10][closeDelay=100]
    ├─ HoverCardTrigger asChild → Button[type="button"][variant="outline"][size="sm"][data-stat="context"]
    │     text: `{pct}% context`; `unknown` (Value, title = reason) when the percent is not known
    └─ HoverCardContent[align="end"][class="flex w-64 flex-col gap-3"]
        ├─ div.flex.items-center.justify-between.text-sm
        │   ├─ span.font-medium  "Context"
        │   └─ span.text-muted-foreground   "unknown" · "{pct}%" · "{pct}% / {compact(contextWindow, 1)}"
        └─ Progress[data-part="context-bar"][value = pct clamped to 0..100][aria-label="Context used"]
```

- The footer is on its own row above the Composer (ChatPane order, 0), with the card at its right. It holds nothing else: no cwd, session key, tokens, cache, model, tier, thinking, cost or branch. **Removed (pro-d57):** the telemetry row `span[data-stat="model|tier|thinking|cost|branch"]`, `[data-part="cwd"]`, `[data-part="session-key"]`, `[data-part="tokens"]`, `[data-part="cache"]`.
- The percent is `footer.contextPercent` when `known`. `compact(n, 1)` gives `1.0M` for a million, `200.0k`-style for thousands as in Phase 2.
- `compact` and the `Value` helper are kept.
- Placement is main's (between the dialog card and the Composer). **Resolution:** the approved preview's ChatPane does not mount this Footer at all; this contract mounts it again in the same place as main, with only the context card.

### 3.3 Kept hooks and tests

`footer[aria-label="Session footer"]`; `[data-stat="context"]` button; `[data-part="context-bar"]`. `Footer.test` 1, 2 are updated: the telemetry assertions are removed; the context HoverCard assertions remain (open on hover, percent text, Progress value).

---

## 4. AskCard

File `web/src/ui/dialogs/AskCard.tsx`. Renders `Dialog` with `method: "ask_user_question"`. Interactive for pi and durable sessions (CONTRACT 6.5; pro-d60: `ask_user_question` works in the WebUI for pi sessions). A multi-step Questionnaire (pro-d56).

### 4.1 Props

```ts
export function AskCard(props: {
  dialog: Extract<Dialog, { method: "ask_user_question" }>;
  readOnly: boolean;                   // = !dialog.interactive; the parent passes it
  onRespond(response: DialogResponse): void;
  asker?: string;                      // KEPT, unused by the preview's output
  recommended?: readonly { readonly questionIndex: number; readonly label: string }[];   // KEPT; no control reads it
}): React.JSX.Element;
```

### 4.2 Interactive DOM

```
div[class="mx-4 mb-3 max-h-[min(70vh,560px)] shrink-0 overflow-y-auto rounded-2xl border bg-card p-5"]
└─ Questionnaire[aria-label="Questions"][data-dialog="ask_user_question"][defaultItem="q0"][shortcuts="letters"]   onKeyDown: Escape → cancel
    ├─ QuestionnaireProgress
    ├─ QuestionnaireItem[data-question="<i>"] ×questions        one step per question, in order
    │   ├─ QuestionnaireTitle        "{header}: {question}"
    │   ├─ QuestionnaireDescription  "{header} · Select all that apply, or type your own."  (multi-select)
    │   │                            "{header} · Choose an option, or type your own."      (single-select)
    │   ├─ div[data-layout="side-by-side" | "single"]          side-by-side when any option has a preview
    │   │   ├─ QuestionnaireChoices[data-part="options"]
    │   │   │   └─ QuestionnaireChoice[data-part="option"][data-option="<label>"] ×options
    │   │   │         the option's description (inside the choice)
    │   │   └─ (when any option has a preview)  div[data-part="preview-pane"]
    │   │         ├─ span[data-part="preview-header"] › span[data-part="preview-focus"]  the focused option's label
    │   │         └─ div[data-part="preview"]   <Markdown text={focused option's preview} />
    │   └─ QuestionnaireInput[aria-label="Type something."]  placeholder "Type something."
    ├─ QuestionnaireError
    └─ QuestionnaireActions
        ├─ Button[type="button"][variant="outline"][data-action="cancel"]  "Cancel"  → cancel
        ├─ QuestionnairePrevious
        ├─ QuestionnaireSkip
        ├─ QuestionnaireNext
        └─ QuestionnaireSubmit        on the last step: submits
```

- Each question is one step. Single-select: one option, or custom text. Multi-select: many options, and custom text. The custom text is the `Type something.` input.
- **Cancel:** the Cancel button and `Escape` (inside the form) call `onRespond({ method: "ask_user_question", outcome: "cancelled" })`.
- **Submit:** `onSubmit` reads `q<i>` values from the form. Per question, in order: multi-select with options chosen and/or custom text → `{ questionIndex, question, kind: "multi", selected: [labels in option order], answer: customText }`; single-select with custom text → `{ …, kind: "custom", answer }`; single-select with an option → `{ …, kind: "option", answer: label }` plus `preview` when the option has one; nothing chosen → the question is omitted from `answers` and its index goes into `unanswered`. The response is `{ method, outcome: "answered", result: { answers, cancelled: false, unanswered? } }` (`unanswered` only when non-empty).
- Keys: `shortcuts="letters"` is passed to the Questionnaire; the letter-key behaviour is the part's (unverified here). Next, Previous and Skip are the Questionnaire's own buttons.

### 4.3 Read-only (`readOnly === true`)

No Questionnaire. `section[aria-label="Questions"][data-dialog="ask_user_question"][data-readonly]` (`mx-4 mb-3 shrink-0 rounded-2xl border bg-card p-5`) with an `ol` of questions (each `p` reads `{header}: {question}`, then a `ul` of options `"{label} - {description}"`) and `p[data-part="note"]` `Answer in the session's own terminal; this view is read-only.` (`font-mono text-meta text-muted-foreground`). No button, input or textarea. The read-only path is used only for a dialog with `interactive: false`; no pi or durable dialog is produced that way today (CONTRACT 6.5 and 6.4).

### 4.4 Kept hooks and tests

| Hook | Pinned by |
|---|---|
| `[data-dialog="ask_user_question"]` on the Questionnaire; `aria-label="Questions"` | `AskCard.test` (updated) |
| `[data-question="<i>"]` per question; `[data-option]` per option | `AskCard.test` (updated) |
| `[data-action="cancel"]` and Escape → `outcome: "cancelled"` | `AskCard.test` (updated) |
| the submit payload per question (2 questions: one option, one multi with custom text) | `AskCard.test` (updated) |
| `[data-readonly]`, the note text, no controls | `AskCard.test` (kept) |

---

## 5. PlanReviewCard

File `web/src/ui/dialogs/PlanReviewCard.tsx`. Renders `Dialog` with `method: "cpd_plan_review"`. A Confirmation: a shadcn `Item` (`components/item.tsx`) as the card frame (pro-d56). Interactive for durable and pi sessions (CONTRACT 6.4 and 6.5).

### 5.1 What the data can feed

As before: `dialog.title` and `dialog.summary` (Markdown). The optional `review: PlanReviewExtras` props (`subtitle`, `previous`, `stats`, `onDiff`, `workflows`, `overlaps`, `risks`, `outOfScope`) are passed only where a source exists; the cockpit passes none today, so the Confirmation shows the title and summary, the three decisions, and the default description (5.6).

### 5.2 Props

```ts
export function PlanReviewCard(props: {
  dialog: Extract<Dialog, { method: "cpd_plan_review" }>;
  readOnly: boolean;                   // = !dialog.interactive
  onRespond(response: DialogResponse): void;
  review?: PlanReviewExtras;
}): React.JSX.Element;
```

### 5.3 DOM outline

```
Item[variant="outline"][size="sm"][asChild]  mx-4 mb-3 w-auto shrink-0
└─ section[aria-label="Plan review"][data-dialog="cpd_plan_review"]   (+ data-readonly when readOnly)
    ├─ ItemMedia[variant="icon"][data-part="phi"]  RouteIcon
    ├─ ItemContent[data-part="header"]
    │   ├─ ItemTitle   span "Plan review ·" + h3.m-0.font-medium {dialog.title}     textContent of the h3 is exactly the title
    │   └─ ItemDescription
    │         "Review the plan summary and approve or request changes."   (when there is no subtitle)
    │         span[data-part="subtitle"].font-mono   {review.subtitle}   (only when passed)
    │         span[data-part="previous"].ml-2.font-mono  "{revision} · {verdict}" (+ " · “{note}”")   (only when passed)
    │         span[data-part="stats"].ml-2.font-mono   {review.stats}   (only when passed)
    ├─ ItemActions[data-part="actions"]     (not rendered when readOnly)
    │   ├─ Button[variant="outline"][size="sm"][data-action="diff"]  "Diff vs {revision}"   (only when review.onDiff and review.previous)
    │   ├─ Button[variant="outline"][size="sm"][data-action="reject"]  Spinner while submitting · "Reject plan"
    │   ├─ Button[variant="outline"][size="sm"][data-action="request-changes"][aria-expanded]  "Request changes"
    │   └─ Button[size="sm"][data-action="approve"]  Spinner while submitting · "Approve & lock" + kbd "⌘⏎"
    └─ ItemFooter[data-part="decision-bar"]   flex-col items-stretch gap-3 border-t pt-3     (when there is a body, or when asking)
        ├─ div[data-part="body-row"]   flex min-h-0 gap-3          (only when there is a body)
        │   ├─ nav[aria-label="Plan sections"][data-part="outline-nav"]   (only when a section besides Outline exists)
        │   └─ div[data-part="body"]   the sections of 5.3.1
        └─ div[data-part="feedback"]    (only while asking, and not read-only)
            ├─ Textarea[aria-label="Feedback"]   autofocus when asking
            └─ Button[data-action="send-feedback"]  "Send feedback"   disabled while the trimmed text is empty
```

- **Read-only** (`readOnly`): the Item and the title, the description and the `body`; no `ItemActions` and no feedback.
- The `section` carries `data-readonly` only when read-only.
- **Resolution (frame):** the Phase 2 and board `header`, `footer[data-part="decision-bar"]` and `div[data-part="actions"]` are the Item parts above; the hooks `data-part="header"`, `data-part="actions"` and `data-part="decision-bar"` are KEPT on them. The Phase 3 `h3` and the `section` keep their names.

**5.3.1 Sections in `body` (the Phase 3 layout, unchanged).** The nav and the body hold the sections below. A section appears only when its data is non-empty; the nav lists `Outline` always and the others when present. Ids are `<dialog.id>-<section>` so two cards cannot collide.

```
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
```

- `outline` is the summary as Markdown (`typeset typeset-chat`, `strong` kept); `Outline` is the label only when other sections exist.
- The `h4` labels are sentence case except the section labels, which are uppercase `text-meta` labels written with utilities, not `h2`.

### 5.4 Behaviour

| Action | Effect |
|---|---|
| click `Approve & lock` | `onRespond({ method: "cpd_plan_review", outcome: "approve" })`; the button shows a Spinner and the buttons are disabled until the next dialog (`submitting`) |
| click `Reject plan` | `onRespond({ method: "cpd_plan_review", outcome: "reject" })`, same submitting state |
| click `Request changes` | `asking = true`: the feedback box appears, the Textarea gets focus, `aria-expanded="true"`. A second click keeps it open (no toggle). Nothing is sent. |
| `Send feedback` (disabled while `feedback.trim() === ""`) | `onRespond({ method: "cpd_plan_review", outcome: "changes", feedback: feedback.trim() })` |
| `Cmd+Enter` or `Ctrl+Enter` (`e.metaKey \|\| e.ctrlKey`, key `Enter`) with focus in the card but not in the feedback Textarea | `preventDefault()`, same as `Approve & lock`. In the Textarea it does nothing. |
| click `Diff vs …` | `review.onDiff()` |

- The card's state resets when `dialog.id` changes (submitting, asking and feedback are cleared).
- No shortcut for `Reject plan` or `Request changes`. `outcome: "cancelled"` has no control.

### 5.5 Read-only and the pi plan review

For a pi session the plan review is interactive (CONTRACT 6.5), so the three decisions show. Read-only appears only for a dialog with `interactive: false`.

### 5.6 Not wired

The `review` extras (`subtitle`, `previous`, `stats`, `onDiff`, workflows, overlaps, risks, out of scope) are rendered only when passed; the cockpit passes none, so they are not shown from the store. The review statistics line, the `a` / `c` / `j k` / `[ ]` keys of the board.

### 5.7 Kept hooks and tests

| Hook | Pinned by (`PlanReviewCard.test`) |
|---|---|
| `section[aria-label="Plan review"][data-dialog="cpd_plan_review"]`; `h3` text exactly the title; the summary as Markdown (`strong` text `bold`) | "shows the plan title and summary" |
| `button[data-action="approve"]` → `{ method, outcome: "approve" }` | "Approve responds approve" |
| `button[data-action="request-changes"]` reveals `textarea[aria-label="Feedback"]` (none before); `button[data-action="send-feedback"]` disabled for empty or whitespace; `{ outcome: "changes", feedback: "tighten scope" }` (trimmed) | "Request changes …" |
| `button[data-action="reject"]` → `{ outcome: "reject" }` | "Reject responds reject" |
| read-only: title and summary present, no `button, textarea, input` | "read-only mode renders the review without controls" |
| `[data-dialog="cpd_plan_review"]` contains the title; `[data-action="approve"]` click sends `dialog.respond` | `App.test` "a dialog message renders the dialog card and answering sends dialog.respond" |

New tests (`planreview.dango.test.tsx`): the `Plan review ·` prefix outside the `h3`; `Approve & lock` and `Reject plan` copy; `⌘⏎` approves from outside the textarea and not from inside; the actions are in the Item's actions column; the submitting state disables the buttons.

---

## 6. Summary of hooks (this file)

Composer: `data-part` `composer`, `queue`, `attachments`, `actions`, `rejections`, `drop-cue`; `data-action` `send`, `cancel-queued`, `remove-attachment`; `data-queued-chip`; `data-drop` on the InputGroup; `data-busy`. TierSelector: `data-part="tier-selector"`; `aria-label` `Tier`, `Model`, `Thinking level`. Footer: `data-part="context-bar"`, `data-stat="context"`. AskCard: `data-question`, `data-option`, `data-layout`, `data-part` `options`, `option`, `preview-pane`, `preview-header`, `preview-focus`, `preview`, `note`; `data-action` `cancel`. PlanReviewCard: `data-part` `phi`, `header`, `subtitle`, `previous`, `stats`, `actions`, `decision-bar`, `body-row`, `outline-nav`, `body`, `feedback`, `outline`, `workflows`, `overlaps`, `risks`, `oos`; `data-action` `diff`, `reject`, `request-changes`, `approve`, `send-feedback`; `data-section`, `data-workflow`, `data-job`.

---
